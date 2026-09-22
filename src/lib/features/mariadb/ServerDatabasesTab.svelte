<script lang="ts">
	import { untrack } from "svelte";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { executeMariaDBQuery, type MariaDBCredentials } from "$lib/modules/mariadb";
	import { formatSize, systemDatabases } from "$lib/modules/databaseBrowser";

	type Props = { credentials: MariaDBCredentials; refreshKey?: number; onOpen: (name: string) => void; onNew: () => void };
	type Row = { name: string; collation: string; tables: number; bytes: number };

	const sql = "SELECT s.SCHEMA_NAME, s.DEFAULT_COLLATION_NAME, COUNT(t.TABLE_NAME), COALESCE(SUM(t.DATA_LENGTH + t.INDEX_LENGTH), 0) FROM information_schema.SCHEMATA s LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME GROUP BY s.SCHEMA_NAME, s.DEFAULT_COLLATION_NAME ORDER BY s.SCHEMA_NAME;";
	let { credentials, refreshKey = 0, onOpen, onNew }: Props = $props();
	let rows = $state.raw<Row[]>([]);
	let loading = $state(false);
	let failure = $state("");
	let filter = $state("");
	let requestId = 0;
	const shown = $derived(filter.trim() ? rows.filter((row) => row.name.toLowerCase().includes(filter.trim().toLowerCase())) : rows);
	const totalBytes = $derived(shown.reduce((sum, row) => sum + row.bytes, 0));

	async function load() {
		const id = ++requestId;
		loading = true; failure = "";
		try {
			const result = await executeMariaDBQuery({ ...credentials, database: null }, sql);
			if (id !== requestId) return;
			if (!result.success) { failure = result.stderr || "Could not list databases."; return; }
			rows = result.rows.map(([name, collation, tables, bytes]) => ({ name, collation, tables: Number(tables) || 0, bytes: Number(bytes) || 0 }));
		} catch (caught) { if (id === requestId) failure = String(caught); }
		finally { if (id === requestId) loading = false; }
	}

	$effect(() => { void refreshKey; untrack(() => void load()); });
</script>

<section class="space-y-3">
	<div class="flex flex-wrap items-center justify-between gap-2 border-b border-border pb-2">
		<h2 class="text-sm font-semibold">Databases</h2>
		<div class="flex items-center gap-2"><Input bind:value={filter} placeholder="Filter databases" aria-label="Filter databases" maxlength={64} class="h-8 w-48 text-xs" /><Button size="sm" onclick={onNew}><PlusIcon />New database</Button></div>
	</div>
	{#if failure}<Notice tone="error" message={failure} onDismiss={() => failure = ""} />{/if}
	<div class="overflow-auto border-y border-border">
		<table class="w-full border-collapse text-left text-xs">
			<thead class="bg-background"><tr>{#each ["Database", "Collation", "Tables", "Size"] as heading}<th class="border-b border-border px-3 py-2 font-medium">{heading}</th>{/each}</tr></thead>
			<tbody>
				{#each shown as row (row.name)}
					<tr class="border-b border-border/50 hover:bg-muted/40">
						<td class="px-3 py-2 font-mono"><button type="button" class="hover:underline" onclick={() => onOpen(row.name)}>{row.name}</button>{#if systemDatabases.includes(row.name.toLowerCase())}<span class="ml-2 text-muted-foreground">system</span>{/if}</td>
						<td class="px-3 py-2 font-mono text-muted-foreground">{row.collation}</td>
						<td class="px-3 py-2">{row.tables}</td>
						<td class="px-3 py-2">{formatSize(row.bytes)}</td>
					</tr>
				{:else}
					<tr><td colspan="4" class="px-3 py-8 text-center text-muted-foreground">{loading ? "Loading databases..." : "No databases."}</td></tr>
				{/each}
			</tbody>
			{#if shown.length}<tfoot><tr class="text-muted-foreground"><td class="px-3 py-2">Total: {shown.length}</td><td></td><td class="px-3 py-2">{shown.reduce((sum, row) => sum + row.tables, 0)}</td><td class="px-3 py-2">{formatSize(totalBytes)}</td></tr></tfoot>{/if}
		</table>
	</div>
	<p class="text-xs text-muted-foreground">Only databases this user can access are listed.</p>
</section>
