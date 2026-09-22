<script lang="ts">
	import { onMount, untrack } from "svelte";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { backupMariaDB, getDefaultMariaDBBackupOutputDir, listMariaDBTables, type MariaDBBackupOptions, type MariaDBCredentials } from "$lib/modules/mariadb";
	import BackupCard from "./BackupCard.svelte";

	type Props = { credentials: MariaDBCredentials; databases: string[]; database: string; onMessage: (text: string) => void };

	let { credentials, databases, database, onMessage }: Props = $props();
	let backupMode = $state<"database" | "tables" | "all">("database");
	let backupDatabase = $state(untrack(() => databases.includes(database) ? database : databases[0] ?? ""));
	let selectedTable = $state("");
	let tables = $state<string[]>([]);
	let busy = $state(false);
	let error = $state("");
	let requestId = 0;
	let backupOptions = $state<MariaDBBackupOptions>({
		outputDir: "", fileName: "", database: "", tables: [], allDatabases: false, schemaOnly: false, dataOnly: false,
		includeRoutines: true, includeTriggers: true, includeEvents: false, singleTransaction: true, addDropStatements: false, whereClause: "",
	});

	onMount(() => { void getDefaultMariaDBBackupOutputDir().then((dir) => { if (dir && !backupOptions.outputDir.trim()) backupOptions.outputDir = dir; }); });

	$effect(() => {
		const name = backupDatabase.trim();
		const wanted = backupMode === "tables" && name;
		const id = ++requestId;
		if (!wanted) { tables = []; selectedTable = ""; return; }
		untrack(() => void loadTables(name, id));
	});

	async function loadTables(name: string, id: number) {
		try {
			const next = await listMariaDBTables({ ...credentials, database: null }, name);
			if (id !== requestId) return;
			tables = next; backupOptions.tables = backupOptions.tables.filter((table) => next.includes(table));
		} catch (caught) { if (id === requestId) { tables = []; error = String(caught); } }
	}
	async function runBackup() {
		if (busy) return;
		if (backupMode === "tables" && !backupOptions.tables.length) { error = "Choose at least one table to export."; return; }
		busy = true; error = "";
		try {
			const backup = await backupMariaDB({ ...credentials, database: null }, {
				...backupOptions,
				allDatabases: backupMode === "all",
				database: backupMode === "all" ? null : backupDatabase.trim() || null,
				tables: backupMode === "tables" ? backupOptions.tables : [],
				fileName: backupOptions.fileName?.trim() || null,
				whereClause: backupOptions.whereClause?.trim() || null,
			});
			onMessage(`Export created: ${backup.path}`);
		} catch (caught) { error = String(caught); }
		finally { busy = false; }
	}
</script>

<section class="space-y-3">
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	<BackupCard bind:backupOptions bind:backupMode bind:backupDatabase bind:selectedTable {busy} canBackup {databases} {tables} onBackup={runBackup} />
</section>
