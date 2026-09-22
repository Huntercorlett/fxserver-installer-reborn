<script lang="ts">
	import { onMount } from "svelte";
	import { save } from "@tauri-apps/plugin-dialog";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import XIcon from "@lucide/svelte/icons/x";
	import FilterIcon from "@lucide/svelte/icons/filter";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import * as Select from "$lib/components/ui/select/index.js";
	import * as Tabs from "$lib/components/ui/tabs/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import ConnectionCard from "./ConnectionCard.svelte";
	import DatabaseTree from "./DatabaseTree.svelte";
	import TableListView from "./TableListView.svelte";
	import CreateTableForm from "./CreateTableForm.svelte";
	import DangerConfirmDialog from "./DangerConfirmDialog.svelte";
	import DatabaseRowEditor from "./DatabaseRowEditor.svelte";
	import ServerTabs from "./ServerTabs.svelte";
	import ServerDatabasesTab from "./ServerDatabasesTab.svelte";
	import ServerSqlTab from "./ServerSqlTab.svelte";
	import ServerStatusTab from "./ServerStatusTab.svelte";
	import ServerUsersTab from "./ServerUsersTab.svelte";
	import ServerExportTab from "./ServerExportTab.svelte";
	import ServerImportTab from "./ServerImportTab.svelte";
	import ServerQueryTab from "./ServerQueryTab.svelte";
	import { serverTabs, type ServerTab } from "$lib/modules/serverTabs";
	import { getWorkspaceId } from "$lib/core/workspaces.svelte";
	import { databaseSession, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { listMariaDBDatabases, listMariaDBTables, validateMariaDBCredentials, type MariaDBCredentials } from "$lib/modules/mariadb";
	import { createDatabase, createTable, dropDatabase, exportBrowserCsv, getBrowserMetadata, getBrowserRows, listTableInfo, runTableAction, systemDatabases, type CreateTableSpec, type TableAction, type TableInfo, type BrowserFilter, type BrowserMetadata, type BrowserPage, type BrowserRequest, type FilterOperator } from "$lib/modules/databaseBrowser";

	let credentials = $state<MariaDBCredentials>({ ...databaseSession.defaults, password: "", ...databaseSession.credentials });
	let validated = $state("");
	let databases = $state<string[]>([]);
	let tables = $state<string[]>([]);
	let database = $state("");
	let table = $state("");
	let metadata = $state.raw<BrowserMetadata>({ columns: [], indexes: [] });
	let page = $state.raw<BrowserPage>({ rows: [], hasMore: false, truncatedCells: false });
	let filters = $state<BrowserFilter[]>([]);
	let appliedFilters = $state<BrowserFilter[]>([]);
	let sortColumn = $state<string | null>(null);
	let descending = $state(false);
	let offset = $state(0);
	let pageSize = $state("25");
	let view = $state<"rows" | "columns" | "indexes">("rows");
	let busy = $state(false);
	let error = $state("");
	let message = $state("");
	let connectionError = $state("");
	let editMode = $state(false);
	let editor = $state<{ kind: "insert" | "update" | "delete"; original: (string | null)[] | null } | null>(null);
	let mode = $state<"database" | "table" | "newDatabase">("database");
	let serverTab = $state<ServerTab | null>("databases");
	let sqlQuery = $state("SELECT VERSION();");
	let refreshKey = $state(0);
	let tableInfo = $state.raw<TableInfo[]>([]);
	let pendingIntent = $state<"none" | "insert" | "search">("none");
	let confirmation = $state<{ kind: "empty" | "drop" | "dropDatabase"; names: string[] } | null>(null);
	let newDatabaseName = $state("");
	let newDatabaseCollation = $state("utf8mb4_unicode_ci");
	let createForm = $state<{ reset: () => void } | null>(null);
	const isSystemDatabase = $derived(systemDatabases.includes(database.toLowerCase()));
	const workspaceId = getWorkspaceId();
	let active = true;
	const credentialsReady = $derived(Boolean(validated) && JSON.stringify(credentials) === validated);
	const operators: { value: FilterOperator; label: string }[] = [
		{ value: "eq", label: "Equals" }, { value: "ne", label: "Not equal" }, { value: "contains", label: "Contains" },
		{ value: "lt", label: "Less than" }, { value: "lte", label: "At most" }, { value: "gt", label: "Greater than" },
		{ value: "gte", label: "At least" }, { value: "isNull", label: "IS NULL" }, { value: "isNotNull", label: "IS NOT NULL" },
	];
	const maxPageRows = $derived(Math.min(200, Math.floor(4000 / Math.max(1, metadata.columns.length))));
	const pageSizes = $derived([...new Set([25, 50, 100, 200, maxPageRows])].filter((value) => value <= maxPageRows).sort((a, b) => a - b).map((value) => ({ value: String(value), label: String(value) })));
	const databaseOptions = $derived(databases.map((value) => ({ value, label: value })));
	const tableOptions = $derived(tables.map((value) => ({ value, label: value })));
	const columnOptions = $derived(metadata.columns.map(({ name }) => ({ value: name, label: name })));

	onMount(() => { active = true; if (databaseSession.credentials) void connect(); return () => { active = false; }; });

	async function action(work: () => Promise<void>) {
		if (busy || !active) return;
		busy = true; error = ""; message = "";
		try { await work(); } catch (caught) { if (active) error = String(caught); }
		finally { if (active) busy = false; }
	}
	function resetTable() {
		editMode = false; editor = null;
		metadata = { columns: [], indexes: [] }; page = { rows: [], hasMore: false, truncatedCells: false };
		filters = []; appliedFilters = []; offset = 0; sortColumn = null; descending = false;
	}
	async function connect() {
		await action(async () => {
			const original = { ...credentials }; const signature = JSON.stringify(original);
			connectionError = ""; validated = ""; databases = []; tables = []; database = ""; table = ""; resetTable();
			try {
				const [, available] = await Promise.all([validateMariaDBCredentials({ ...original, database: null }), listMariaDBDatabases({ ...original, database: null })]);
				if (!active || signature !== JSON.stringify(credentials)) return;
				validated = signature; databases = available; rememberDatabaseCredentials(original);
				database = available.includes(original.database ?? "") ? original.database! : available.find((name) => !["mysql", "sys", "information_schema", "performance_schema"].includes(name)) ?? available[0] ?? "";
				await loadTables();
			} catch (caught) { connectionError = String(caught); throw caught; }
		});
	}
	async function loadTables() {
		const selected = database; const signature = JSON.stringify(credentials);
		table = ""; tables = []; tableInfo = []; resetTable(); mode = "database";
		if (!selected || !credentialsReady) return;
		const info = await listTableInfo({ ...credentials, database: null }, selected);
		if (!active || selected !== database || signature !== JSON.stringify(credentials)) return;
		tableInfo = info; tables = info.filter((item) => item.kind === "BASE TABLE").map((item) => item.name);
	}
	async function loadMetadata() {
		resetTable();
		if (!database || !table || !credentialsReady) return;
		const key = `${database}/${table}`; const signature = JSON.stringify(credentials);
		const result = await getBrowserMetadata({ ...credentials }, database, table);
		if (!active || key !== `${database}/${table}` || signature !== JSON.stringify(credentials)) return;
		metadata = result; sortColumn = result.indexes.find((index) => index.name === "PRIMARY")?.column ?? result.columns[0]?.name ?? null;
		pageSize = String(Math.min(Number(pageSize), Math.floor(4000 / Math.max(1, result.columns.length))));
		const intent = pendingIntent; pendingIntent = "none";
		if (intent === "search") filters = [{ column: result.columns[0].name, operator: "contains", value: "" }];
		await loadRows();
		if (intent === "insert") {
			if (result.editable) { editMode = true; editor = { kind: "insert", original: null }; }
			else message = result.editReason ?? "This table cannot be edited.";
		}
	}
	function request(): BrowserRequest { return { database, table, filters: appliedFilters.map((filter) => ({ ...filter })), sortColumn, descending, offset, pageSize: Number(pageSize) }; }
	async function loadRows() {
		if (!credentialsReady || !table) return;
		const query = request(); const signature = JSON.stringify(credentials);
		const result = await getBrowserRows({ ...credentials }, query);
		if (active && signature === JSON.stringify(credentials) && JSON.stringify(query) === JSON.stringify(request())) {
			page = result;
			if (result.pageSize) pageSize = String(result.pageSize);
		}
	}
	function refresh() { if (serverTab) refreshKey += 1; else void action(mode === "table" ? loadMetadata : loadTables); }
	function selectDatabase(name: string) { serverTab = null; database = name; void action(loadTables); }
	function openTable(name: string, next: "rows" | "columns" = "rows", intent: "none" | "insert" | "search" = "none") {
		serverTab = null;
		void action(async () => { table = name; mode = "table"; view = next; pendingIntent = intent; await loadMetadata(); });
	}
	function showDatabase() { serverTab = null; if (mode !== "database") void action(loadTables); }
	function requestAction(kind: TableAction, names: string[]) {
		if ((kind === "empty" || kind === "drop") && isSystemDatabase) { error = `${database} is a system database and cannot be changed here.`; return; }
		if (kind === "empty" || kind === "drop") { confirmation = { kind, names }; return; }
		void performAction(kind, names);
	}
	async function performAction(kind: TableAction, names: string[], confirm?: string, ignoreForeignKeys = false) {
		await action(async () => {
			const result = await runTableAction({ ...credentials, database: null }, { database, action: kind, tables: names, confirmation: confirm ?? null, disableForeignKeyChecks: ignoreForeignKeys });
			await loadTables();
			message = result.message;
		});
	}
	async function confirmed(ignoreForeignKeys: boolean) {
		const pending = confirmation;
		if (!pending) return;
		if (pending.kind === "dropDatabase") {
			await action(async () => {
				const dropped = database;
				const result = await dropDatabase({ ...credentials, database: null }, dropped, dropped);
				const available = await listMariaDBDatabases({ ...credentials, database: null });
				databases = available; database = available.find((name) => !systemDatabases.includes(name)) ?? available[0] ?? "";
				await loadTables();
				message = result.message;
			});
		} else {
			await performAction(pending.kind, pending.names, database, ignoreForeignKeys);
		}
		confirmation = null;
	}
	async function makeTable(spec: CreateTableSpec) {
		await action(async () => {
			const result = await createTable({ ...credentials, database: null }, spec);
			createForm?.reset();
			await loadTables();
			message = result.message;
		});
	}
	async function makeDatabase() {
		const name = newDatabaseName.trim();
		if (!name) { error = "Enter a database name."; return; }
		await action(async () => {
			const result = await createDatabase({ ...credentials, database: null }, name, newDatabaseCollation);
			databases = await listMariaDBDatabases({ ...credentials, database: null });
			database = name; newDatabaseName = "";
			await loadTables();
			message = result.message;
		});
	}
	function confirmSql(): string[] {
		if (!confirmation) return [];
		const quote = (value: string) => `\`${value.replaceAll("`", "``")}\``;
		if (confirmation.kind === "dropDatabase") return [`DROP DATABASE ${quote(database)};`];
		if (confirmation.kind === "drop") return [`DROP TABLE ${confirmation.names.map((name) => `${quote(database)}.${quote(name)}`).join(", ")};`];
		return confirmation.names.map((name) => `TRUNCATE TABLE ${quote(database)}.${quote(name)};`);
	}
	async function applyFilters() { await action(async () => { appliedFilters = filters.map((filter) => ({ ...filter })); offset = 0; await loadRows(); }); }
	async function sort(name: string) { await action(async () => { descending = sortColumn === name ? !descending : false; sortColumn = name; offset = 0; await loadRows(); }); }
	async function paginate(direction: number) { await action(async () => { offset = Math.max(0, offset + direction * Number(pageSize)); await loadRows(); }); }
	async function exportCsv() {
		await action(async () => {
			const query = request(); const original = { ...credentials };
			const outputPath = await save({ defaultPath: `${table.replace(/[^a-zA-Z0-9_-]/g, "_")}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
			if (!outputPath || !active || JSON.stringify(query) !== JSON.stringify(request()) || JSON.stringify(original) !== JSON.stringify(credentials)) return;
			const result = await exportBrowserCsv(original, query, outputPath);
			if (active) message = `${result.rows} rows exported${result.hasMore ? " (5,000-row limit reached)" : ""}: ${result.path}. SQL NULL is \\N; spreadsheet formulas are prefixed with an apostrophe.`;
		});
	}
</script>

<section class="min-w-0 space-y-4">
	<header class="flex flex-wrap items-center justify-between gap-3">
		<div class="flex flex-wrap items-center gap-3"><DatabaseIcon class="size-6 text-muted-foreground" /><h1 class="text-2xl font-semibold">Database Browser</h1>{#if mode === "table"}<span class={editMode ? "text-xs text-amber-400" : "text-xs text-muted-foreground"}>{editMode ? "Editing enabled" : "Read-only rows"}</span>{/if}</div>
		<Button size="icon" variant="outline" disabled={busy || !credentialsReady || (!serverTab && !database)} onclick={refresh} title="Refresh" aria-label="Refresh"><RefreshCwIcon class={busy ? "animate-spin" : ""} /></Button>
	</header>
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => message = ""} />{/if}
	<details open={!credentialsReady}><summary class="mb-3 cursor-pointer text-sm font-medium">Connection {credentialsReady ? ` / ${credentials.host}:${credentials.port}` : ""}</summary><ConnectionCard bind:credentials {busy} {credentialsReady} {connectionError} stretch={false} onApply={connect} /></details>

	{#if credentialsReady}
		<div class="grid items-start gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
			<div class="lg:sticky lg:top-2"><DatabaseTree {databases} {database} {table} {tables} {busy} username={credentials.username} onDatabase={selectDatabase} onTable={(name) => openTable(name)} onNewDatabase={() => { serverTab = null; mode = "newDatabase"; table = ""; }} onNewTable={() => { showDatabase(); setTimeout(() => document.getElementById("create-table")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50); }} /></div>

			<div class="min-w-0 space-y-4">
				<ServerTabs active={serverTab} disabled={busy} onSelect={(tab) => { serverTab = tab; message = ""; error = ""; }} />
				<nav class="flex flex-wrap items-center gap-1.5 rounded-md border border-border bg-card px-3 py-2 text-xs text-muted-foreground" aria-label="Location">
					<span class="font-mono">{credentials.host}:{credentials.port}</span>
					{#if serverTab}<span>»</span><span class="text-foreground">{serverTabs.find((tab) => tab.id === serverTab)?.label}</span>
					{:else if mode === "newDatabase"}<span>»</span><span class="text-foreground">New database</span>
					{:else if database}<span>»</span><button type="button" class="hover:underline" class:text-foreground={mode === "database"} disabled={busy} onclick={showDatabase}>Database: <span class="font-mono">{database}</span></button>
						{#if mode === "table" && table}<span>»</span><span class="text-foreground">Table: <span class="font-mono">{table}</span></span>{/if}{/if}
				</nav>

				{#if serverTab === "databases"}
					<ServerDatabasesTab {credentials} {refreshKey} onOpen={(name) => selectDatabase(name)} onNew={() => { serverTab = null; mode = "newDatabase"; table = ""; }} />
				{:else if serverTab === "sql"}
					<ServerSqlTab {credentials} {databases} {database} bind:query={sqlQuery} />
				{:else if serverTab === "status"}
					<ServerStatusTab {credentials} {refreshKey} />
				{:else if serverTab === "users"}
					<ServerUsersTab {credentials} {refreshKey} />
				{:else if serverTab === "export"}
					<ServerExportTab {credentials} {databases} {database} onMessage={(text) => { message = text; error = ""; }} />
				{:else if serverTab === "import"}
					<ServerImportTab {credentials} {databases} {database} onImported={() => void listMariaDBDatabases({ ...credentials, database: null }).then((available) => { if (active) databases = available; }).catch(() => {})} />
				{:else if serverTab === "variables"}
					<ServerQueryTab {credentials} {refreshKey} sql="SHOW GLOBAL VARIABLES;" title="Server variables" />
				{:else if serverTab === "charsets"}
					<ServerQueryTab {credentials} {refreshKey} sql="SHOW COLLATION;" title="Character sets and collations" />
				{:else if serverTab === "engines"}
					<ServerQueryTab {credentials} {refreshKey} sql="SHOW ENGINES;" title="Storage engines" />
				{:else if mode === "newDatabase"}
					<section class="max-w-xl space-y-3 rounded-md border border-border bg-card p-4">
						<h2 class="text-sm font-semibold">Create database</h2>
						<div class="flex flex-wrap items-end gap-3 text-xs">
							<label class="grid gap-1 font-medium text-muted-foreground">Name<Input bind:value={newDatabaseName} disabled={busy} maxlength={64} placeholder="database_name" class="h-8 w-56 text-xs" /></label>
							<label class="grid gap-1 font-medium text-muted-foreground">Collation<select bind:value={newDatabaseCollation} disabled={busy} class="h-8 rounded-sm border border-input bg-background px-2 text-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50">{#each ["utf8mb4_unicode_ci", "utf8mb4_general_ci", "utf8mb4_bin", "utf8mb3_general_ci", "latin1_swedish_ci"] as option}<option>{option}</option>{/each}</select></label>
							<Button size="sm" disabled={busy} onclick={makeDatabase}>Create</Button>
						</div>
					</section>
				{:else if mode === "database" && database}
					<div class="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-2"><h2 class="text-sm font-semibold">Structure</h2>{#if !isSystemDatabase}<Button size="sm" variant="outline" class="text-destructive" disabled={busy} onclick={() => confirmation = { kind: "dropDatabase", names: [] }}><Trash2Icon />Drop database</Button>{:else}<span class="text-xs text-muted-foreground">System database (read-only here)</span>{/if}</div>
					<TableListView tables={tableInfo} {busy} onOpen={openTable} onAction={requestAction} />
					{#if !isSystemDatabase}<CreateTableForm bind:this={createForm} {database} {busy} onCreate={makeTable} />{/if}
				{:else if mode === "table" && table}
		<Tabs.Root bind:value={view} class="space-y-5" loop>
		<div class="flex flex-wrap items-center justify-between gap-3 border-b border-border">
			<Tabs.List aria-label="Table views">{#each [["rows", "Browse"], ["columns", "Structure"], ["indexes", "Indexes"]] as [tab, label]}<Tabs.Trigger value={tab}>{label}</Tabs.Trigger>{/each}</Tabs.List>
			<div class="flex flex-wrap gap-2"><Button variant="outline" size="sm" disabled={busy} onclick={exportCsv} title="Export up to 5,000 filtered rows, maximum 8 MiB"><DownloadIcon />Export CSV</Button><Button variant="outline" size="sm" disabled={busy || isSystemDatabase} onclick={() => requestAction("empty", [table])}>Empty</Button><Button variant="outline" size="sm" class="text-destructive" disabled={busy || isSystemDatabase} onclick={() => requestAction("drop", [table])}>Drop</Button></div>
		</div>
		<div class="flex flex-wrap items-center justify-between gap-3 text-xs"><label class="flex items-center gap-2"><Checkbox bind:checked={editMode} disabled={busy || !metadata.editable} onCheckedChange={() => editor = null} />Enable row editing</label>{#if metadata.editReason}<span class="text-muted-foreground">{metadata.editReason}</span>{/if}{#if editMode}<Button variant="outline" size="sm" disabled={busy} onclick={() => editor = { kind: "insert", original: null }}><PlusIcon />Insert Row</Button>{/if}</div>
		{#if editMode && editor}{#key editor}<DatabaseRowEditor {metadata} kind={editor.kind} original={editor.original} {database} {table} {workspaceId} {credentials} onClose={() => editor = null} onBusy={(value) => busy = value} onApplied={async () => { editor = null; busy = false; await action(loadRows); message = "One row changed."; }} />{/key}{/if}
		{#if view === "rows"}
			<Tabs.Content value="rows" class="space-y-4">
			<div class="space-y-2">
				{#each filters as filter, i}
					<div class="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_10rem_minmax(0,1fr)_auto]">
						<Select.Root type="single" bind:value={filter.column} items={columnOptions} disabled={busy}><Select.Trigger class="w-full min-w-0" aria-label={`Filter ${i + 1} column`}><span class="truncate">{filter.column}</span></Select.Trigger><Select.Content>{#each columnOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root>
						<Select.Root type="single" bind:value={filter.operator} items={operators} disabled={busy}><Select.Trigger class="w-full" aria-label={`Filter ${i + 1} operator`}>{operators.find((op) => op.value === filter.operator)?.label}</Select.Trigger><Select.Content>{#each operators as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root>
						<Input bind:value={filter.value} aria-label={`Filter ${i + 1} value`} maxlength={512} disabled={busy || ["isNull", "isNotNull"].includes(filter.operator)} />
						<Button size="icon-sm" variant="ghost" title="Remove filter" aria-label={`Remove filter ${i + 1}`} disabled={busy} onclick={() => filters = filters.filter((_, index) => index !== i)}><XIcon /></Button>
					</div>
				{/each}
				<div class="flex gap-2"><Button variant="ghost" size="sm" disabled={busy || filters.length >= 8 || !metadata.columns.length} onclick={() => filters = [...filters, { column: metadata.columns[0].name, operator: "eq", value: "" }]}><PlusIcon />Filter</Button><Button variant="outline" size="sm" onclick={applyFilters} disabled={busy}><FilterIcon />Apply</Button></div>
			</div>
			{#if page.truncatedCells}<p class="text-xs text-amber-400">Some cells exceed the 4,096-character preview limit.</p>{/if}
			<div class="overflow-x-auto border-y border-border" aria-busy={busy}>
				<table class="w-full border-collapse text-left text-xs"><thead class="bg-background"><tr>{#each metadata.columns as column}<th class="min-w-32 border-b border-border px-3 py-2 font-medium"><button type="button" class="flex items-center gap-2 whitespace-nowrap" disabled={busy} onclick={() => sort(column.name)} title={`Sort ${column.name}`}><span>{column.name}</span><ArrowUpDownIcon class="size-3" />{sortColumn === column.name ? (descending ? "DESC" : "ASC") : ""}</button></th>{/each}{#if editMode}<th class="w-20 border-b border-border px-3 py-2">Actions</th>{/if}</tr></thead><tbody>{#each page.rows as row}<tr class="border-b border-border/50 hover:bg-muted/40">{#each row as cell}<td class="max-w-80 truncate px-3 py-2 font-mono" class:text-muted-foreground={cell === null} title={cell === null ? "SQL NULL" : cell}>{cell === null ? "NULL" : cell === "" ? '""' : cell}</td>{/each}{#if editMode}<td class="whitespace-nowrap px-2"><Button variant="ghost" size="icon-sm" title="Edit this row" aria-label="Edit this row" disabled={busy || page.truncatedCells} onclick={() => editor = { kind: "update", original: [...row] }}><PencilIcon /></Button><Button variant="ghost" size="icon-sm" title="Delete this row" aria-label="Delete this row" disabled={busy || page.truncatedCells} onclick={() => editor = { kind: "delete", original: [...row] }}><Trash2Icon /></Button></td>{/if}</tr>{:else}<tr><td colspan={Math.max(1, metadata.columns.length + (editMode ? 1 : 0))} class="px-3 py-8 text-center text-muted-foreground">{busy ? "Loading rows..." : "No rows match."}</td></tr>{/each}</tbody></table>
			</div>
			<footer class="flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground"><span>{page.rows.length ? `${offset + 1}-${offset + page.rows.length}` : "0"} rows</span><div class="flex items-center gap-2"><Select.Root type="single" value={pageSize} items={pageSizes} disabled={busy} onValueChange={(value) => { pageSize = value; offset = 0; void action(loadRows); }}><Select.Trigger class="w-20" aria-label="Rows per page">{pageSize}</Select.Trigger><Select.Content>{#each pageSizes as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root><Button variant="outline" size="icon-sm" title="Previous page" aria-label="Previous page" disabled={busy || !offset} onclick={() => paginate(-1)}><ChevronLeftIcon /></Button><Button variant="outline" size="icon-sm" title="Next page" aria-label="Next page" disabled={busy || !page.hasMore || offset + Number(pageSize) > 1000000} onclick={() => paginate(1)}><ChevronRightIcon /></Button></div></footer>
			</Tabs.Content>
		{:else if view === "columns"}
			<Tabs.Content value="columns">
			<div class="overflow-auto"><table class="w-full text-left text-xs"><thead><tr>{#each ["Column", "Type", "Nullable", "Default", "Extra"] as heading}<th class="border-b border-border p-3">{heading}</th>{/each}</tr></thead><tbody>{#each metadata.columns as column}<tr class="border-b border-border/50"><td class="p-3 font-mono">{column.name}</td><td class="p-3 font-mono">{column.columnType}</td><td class="p-3">{column.nullable ? "Yes" : "No"}</td><td class="max-w-64 truncate p-3" title={column.defaultValue ?? "NULL"}>{column.defaultValue ?? "NULL"}</td><td class="p-3">{column.extra || "-"}</td></tr>{/each}</tbody></table></div>
			</Tabs.Content>
		{:else}
			<Tabs.Content value="indexes">
			<div class="overflow-auto"><table class="w-full text-left text-xs"><thead><tr>{#each ["Index", "Column", "Position", "Unique", "Type"] as heading}<th class="border-b border-border p-3">{heading}</th>{/each}</tr></thead><tbody>{#each metadata.indexes as index}<tr class="border-b border-border/50"><td class="p-3 font-mono">{index.name}</td><td class="p-3 font-mono">{index.column ?? "Expression"}</td><td class="p-3">{index.sequence}</td><td class="p-3">{index.unique ? "Yes" : "No"}</td><td class="p-3">{index.indexType}</td></tr>{:else}<tr><td colspan="5" class="p-6 text-center text-muted-foreground">No indexes.</td></tr>{/each}</tbody></table></div>
			</Tabs.Content>
		{/if}
		</Tabs.Root>
				{:else}
					<p class="py-8 text-sm text-muted-foreground">Choose a database from the list.</p>
				{/if}
			</div>
		</div>
	{/if}

	{#if confirmation}
		<DangerConfirmDialog
			title={confirmation.kind === "dropDatabase" ? `Drop database ${database}?` : confirmation.kind === "drop" ? `Drop ${confirmation.names.length} ${confirmation.names.length === 1 ? "table" : "tables"}?` : `Empty ${confirmation.names.length} ${confirmation.names.length === 1 ? "table" : "tables"}?`}
			description={confirmation.kind === "empty" ? "Every row is deleted permanently. The table structure stays." : "This permanently deletes the data and cannot be undone. Make a backup first if you might need it."}
			sql={confirmSql()}
			confirmWord={database}
			actionLabel={confirmation.kind === "empty" ? "Empty" : "Drop"}
			foreignKeyOption={confirmation.kind !== "dropDatabase"}
			{busy}
			onConfirm={confirmed}
			onCancel={() => confirmation = null}
		/>
	{/if}
</section>
