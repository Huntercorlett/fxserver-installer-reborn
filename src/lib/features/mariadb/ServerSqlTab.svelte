<script lang="ts">
	import { untrack } from "svelte";
	import PlayIcon from "@lucide/svelte/icons/play";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import type { MariaDBCredentials, MariaDBQueryResult } from "$lib/modules/mariadb";
	import { runSqlWithForeignKeyRepair, type SqlDiagnosis } from "$lib/modules/sqlDiagnostics";
	import ResultTable from "./ResultTable.svelte";

	type Props = { credentials: MariaDBCredentials; databases: string[]; database: string; query: string };

	const globalScope = "__global__";
	let { credentials, databases, database, query = $bindable() }: Props = $props();
	let scope = $state(untrack(() => databases.includes(database) ? database : globalScope));
	let result = $state.raw<MariaDBQueryResult | null>(null);
	let diagnosis = $state<SqlDiagnosis | null>(null);
	let busy = $state(false);
	let error = $state("");

	async function run() {
		if (busy || !query.trim()) return;
		busy = true; error = ""; diagnosis = null; result = null;
		try {
			const run = await runSqlWithForeignKeyRepair({ ...credentials, database: scope === globalScope ? null : scope }, query);
			result = run.result; diagnosis = run.diagnosis;
			if (run.repairs) query = run.sql;
		} catch (caught) { error = String(caught); }
		finally { busy = false; }
	}
	function onKeydown(event: KeyboardEvent) {
		if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void run(); }
	}
</script>

<section class="space-y-3">
	<div class="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-2">
		<h2 class="text-sm font-semibold">Run SQL queries on server "{credentials.host}"</h2>
		<label class="flex items-center gap-2 text-xs text-muted-foreground">Database<select bind:value={scope} disabled={busy} class="h-8 rounded-sm border border-input bg-background px-2 text-xs text-foreground outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"><option value={globalScope}>None (server)</option>{#each databases as name}<option value={name}>{name}</option>{/each}</select></label>
	</div>
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	<textarea bind:value={query} onkeydown={onKeydown} spellcheck="false" aria-label="SQL query" placeholder="SELECT VERSION();" class="h-48 min-h-32 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-xs leading-5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"></textarea>
	<div class="flex items-center gap-3"><Button size="sm" disabled={busy || !query.trim()} onclick={run}><PlayIcon />Go</Button><span class="text-xs text-muted-foreground">Ctrl+Enter to run</span></div>
	{#if diagnosis}
		<Notice tone="warn" title={diagnosis.title} message={diagnosis.message} onDismiss={() => diagnosis = null} />
		{#if diagnosis.fixedSql}<Button variant="outline" size="sm" onclick={() => { query = diagnosis?.fixedSql ?? query; diagnosis = null; }}>Apply fix to SQL</Button>{/if}
	{/if}
	{#if result}
		{#if result.success && result.columns.length}<ResultTable columns={result.columns} rows={result.rows} filterable={false} />
		{:else}<pre class={["max-h-80 overflow-auto rounded-sm border p-4 font-mono text-xs leading-6 whitespace-pre-wrap", result.success ? "border-border bg-card text-foreground" : "border-destructive/30 bg-destructive/10 text-destructive"]}>{result.stdout || result.stderr || "Query executed."}</pre>{/if}
	{/if}
</section>
