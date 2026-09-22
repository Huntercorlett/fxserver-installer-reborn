<script lang="ts">
	import { untrack } from "svelte";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import PlayIcon from "@lucide/svelte/icons/play";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { chooseSqlFile } from "$lib/core/selectFile";
	import { executeMariaDBQuery, type MariaDBCredentials, type MariaDBQueryResult } from "$lib/modules/mariadb";
	import { runSqlWithForeignKeyRepair, type SqlDiagnosis } from "$lib/modules/sqlDiagnostics";
	import { readTextFile } from "$lib/modules/system";

	type Props = { credentials: MariaDBCredentials; databases: string[]; database: string; onImported: () => void };

	const globalScope = "__global__";
	let { credentials, databases, database, onImported }: Props = $props();
	let scope = $state(untrack(() => databases.includes(database) ? database : globalScope));
	let path = $state("");
	let sql = $state("");
	let result = $state.raw<MariaDBQueryResult | null>(null);
	let diagnosis = $state<SqlDiagnosis | null>(null);
	let busy = $state(false);
	let error = $state("");
	let message = $state("");
	let targetCollation = $state("utf8mb4_unicode_ci");

	async function browse() {
		error = ""; message = "";
		try {
			const selected = await chooseSqlFile(path || undefined);
			if (!selected) return;
			sql = await readTextFile(selected); path = selected; result = null; diagnosis = null;
		} catch (caught) { error = String(caught); }
	}
	async function run() {
		if (busy || !sql.trim()) return;
		busy = true; error = ""; message = ""; result = null; diagnosis = null;
		try {
			const run = await runSqlWithForeignKeyRepair({ ...credentials, database: scope === globalScope ? null : scope }, sql);
			result = run.result; diagnosis = run.diagnosis;
			if (run.repairs) sql = run.sql;
			if (run.result.success) { message = "SQL executed successfully."; onImported(); }
		} catch (caught) { error = String(caught); }
		finally { busy = false; }
	}
	async function generateCollationFix(): Promise<boolean> {
		if (scope === globalScope) { error = "Choose a database first."; return false; }
		const collation = targetCollation.trim().toLowerCase();
		if (!/^[a-z0-9]+_[a-z0-9_]+$/.test(collation)) { error = "Enter a collation name such as utf8mb4_unicode_ci or utf8mb4_general_ci."; return false; }
		const charset = collation.split("_")[0];
		busy = true; error = ""; message = ""; diagnosis = null;
		try {
			const database = scope;
			const literal = `'${database.replaceAll("\\", "\\\\").replaceAll("'", "''")}'`;
			const identifier = (value: string) => `\`${value.replaceAll("`", "``")}\``;
			const global = { ...credentials, database: null };
			const scan = await executeMariaDBQuery(global, `SELECT DISTINCT c.TABLE_NAME FROM information_schema.COLUMNS c JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME WHERE c.TABLE_SCHEMA = ${literal} AND t.TABLE_TYPE = 'BASE TABLE' AND c.COLLATION_NAME IS NOT NULL AND c.COLLATION_NAME <> '${collation}' UNION SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = ${literal} AND TABLE_TYPE = 'BASE TABLE' AND TABLE_COLLATION <> '${collation}' ORDER BY 1;`);
			if (!scan.success) { error = scan.stderr || "Could not scan the database."; return false; }
			const tables = scan.rows.map((row) => row[0]).filter(Boolean);
			const converting = new Set(tables);

			// Foreign keys on a converted column cannot be altered in place (ERROR 1832): drop them, convert, then recreate them exactly as they were.
			const keyScan = await executeMariaDBQuery(global, `SELECT k.CONSTRAINT_NAME, k.TABLE_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.TABLE_NAME = k.TABLE_NAME AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME WHERE k.TABLE_SCHEMA = ${literal} AND k.REFERENCED_TABLE_SCHEMA = ${literal} ORDER BY k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION;`);
			if (!keyScan.success) { error = keyScan.stderr || "Could not read the foreign keys."; return false; }
			const keys = new Map<string, { name: string; table: string; columns: string[]; refTable: string; refColumns: string[]; update: string; remove: string }>();
			for (const [name, table, column, refTable, refColumn, update, remove] of keyScan.rows) {
				if (!converting.has(table) && !converting.has(refTable)) continue;
				const id = `${table}\u0000${name}`;
				const key = keys.get(id) ?? { name, table, columns: [], refTable, refColumns: [], update, remove };
				key.columns.push(column); key.refColumns.push(refColumn); keys.set(id, key);
			}
			const affected = [...keys.values()];
			const list = (columns: string[]) => columns.map(identifier).join(", ");
			sql = [
				`-- Match ${database} to ${charset} / ${collation}. Back up first, then review and run.`,
				"SET FOREIGN_KEY_CHECKS=0;",
				`ALTER DATABASE ${identifier(database)} CHARACTER SET ${charset} COLLATE ${collation};`,
				...affected.map((key) => `ALTER TABLE ${identifier(database)}.${identifier(key.table)} DROP FOREIGN KEY ${identifier(key.name)};`),
				...tables.map((table) => `ALTER TABLE ${identifier(database)}.${identifier(table)} CONVERT TO CHARACTER SET ${charset} COLLATE ${collation};`),
				...affected.map((key) => `ALTER TABLE ${identifier(database)}.${identifier(key.table)} ADD CONSTRAINT ${identifier(key.name)} FOREIGN KEY (${list(key.columns)}) REFERENCES ${identifier(database)}.${identifier(key.refTable)} (${list(key.refColumns)}) ON DELETE ${key.remove} ON UPDATE ${key.update};`),
				"SET FOREIGN_KEY_CHECKS=1;",
			].join("\n");
			path = ""; result = null;
			message = tables.length
				? `Generated a fix for ${tables.length} table${tables.length === 1 ? "" : "s"} and ${affected.length} foreign key${affected.length === 1 ? "" : "s"}. Back up ${database}, review the SQL, then press Run SQL.`
				: `Every table in ${database} already uses ${collation}. Run SQL will only set the database default so new tables from resources match.`;
			return true;
		} catch (caught) { error = String(caught); return false; }
		finally { busy = false; }
	}
	async function fixForFivem() {
		targetCollation = "utf8mb4_unicode_ci";
		if (await generateCollationFix()) await run();
	}
</script>

<section class="space-y-3">
	<div class="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-2">
		<h2 class="text-sm font-semibold">SQL file</h2>
		<label class="flex items-center gap-2 text-xs text-muted-foreground">Database<select bind:value={scope} disabled={busy} class="h-8 rounded-sm border border-input bg-background px-2 text-xs text-foreground outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"><option value={globalScope}>None (dump selects its own)</option>{#each databases as name}<option value={name}>{name}</option>{/each}</select></label>
	</div>
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => message = ""} />{/if}
	<div class="flex flex-wrap items-center gap-3"><Button variant="outline" size="sm" disabled={busy} onclick={browse}><FolderOpenIcon />Choose .sql file</Button><span class="min-w-0 truncate font-mono text-xs text-muted-foreground" title={path}>{path || "No file chosen"}</span></div>
	<textarea bind:value={sql} spellcheck="false" aria-label="SQL to import" placeholder="Choose a .sql file, or paste SQL here." class="h-56 min-h-32 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-xs leading-5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"></textarea>
	<div class="flex flex-wrap items-center gap-2">
		<Button size="sm" disabled={busy || !sql.trim()} onclick={run}><PlayIcon />Run SQL</Button>
		<Input bind:value={targetCollation} disabled={busy} maxlength={64} aria-label="Target collation" class="h-8 w-52 font-mono text-xs" placeholder="utf8mb4_unicode_ci" />
		<Button size="sm" disabled={busy || scope === globalScope} onclick={fixForFivem} title="Converts the selected database to FiveM's standard utf8mb4_unicode_ci: scans, drops and recreates foreign keys, converts, and runs. Back up first.">Fix database for FiveM</Button>
		<Button variant="outline" size="sm" disabled={busy || scope === globalScope} onclick={generateCollationFix} title="Generate SQL that matches every table in the selected database to the target collation, without running it.">Generate collation fix</Button>
	</div>
	{#if diagnosis}
		<Notice tone="warn" title={diagnosis.title} message={diagnosis.message} onDismiss={() => diagnosis = null} />
		{#if diagnosis.fixedSql}<Button variant="outline" size="sm" onclick={() => { sql = diagnosis?.fixedSql ?? sql; diagnosis = null; }}>Apply fix to SQL</Button>{/if}
	{/if}
	{#if result && !result.success}<pre class="max-h-80 overflow-auto rounded-sm border border-destructive/30 bg-destructive/10 p-4 font-mono text-xs leading-6 whitespace-pre-wrap text-destructive">{result.stderr || result.stdout || "Import failed."}</pre>{/if}
</section>
