<script lang="ts">
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import TableIcon from "@lucide/svelte/icons/table";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
	import { Input } from "$lib/components/ui/input/index.js";
	import { systemDatabases } from "$lib/modules/databaseBrowser";

	type Props = {
		databases: string[];
		database: string;
		table: string;
		tables: string[];
		busy: boolean;
		username: string;
		onDatabase: (name: string) => void;
		onTable: (name: string) => void;
		onNewDatabase: () => void;
		onNewTable: () => void;
	};
	let { databases, database, table, tables, busy, username, onDatabase, onTable, onNewDatabase, onNewTable }: Props = $props();

	let filter = $state("");
	const needle = $derived(filter.trim().toLowerCase());
	const visibleDatabases = $derived(databases.filter((name) => !needle || name.toLowerCase().includes(needle) || name === database));
	const visibleTables = $derived(tables.filter((name) => !needle || name.toLowerCase().includes(needle)));
	const row = "flex w-full items-center gap-1.5 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-muted disabled:opacity-60";
</script>

<nav aria-label="Databases" class="flex max-h-[calc(100vh-7rem)] flex-col overflow-hidden rounded-md border border-border bg-card">
	<div class="border-b border-border p-2"><Input bind:value={filter} placeholder="Filter databases and tables" class="h-8 text-xs" aria-label="Filter databases and tables" /></div>
	<div class="min-h-0 flex-1 overflow-y-auto p-1.5">
		<button type="button" class={`${row} text-primary`} disabled={busy} onclick={onNewDatabase}><PlusIcon class="size-3.5" />New</button>
		{#each visibleDatabases as name (name)}
			{@const open = name === database}
			<button type="button" class={`${row} ${open && !table ? "bg-muted font-medium text-foreground" : "text-foreground/90"}`} disabled={busy} onclick={() => onDatabase(open ? "" : name)} title={name}>
				{#if open}<ChevronDownIcon class="size-3.5 shrink-0 text-muted-foreground" />{:else}<ChevronRightIcon class="size-3.5 shrink-0 text-muted-foreground" />{/if}
				<DatabaseIcon class={`size-3.5 shrink-0 ${systemDatabases.includes(name) ? "text-muted-foreground" : "text-primary"}`} />
				<span class="truncate">{name}</span>
			</button>
			{#if open}
				<div class="ml-4 border-l border-border pl-1.5">
					<button type="button" class={`${row} text-primary`} disabled={busy} onclick={onNewTable}><PlusIcon class="size-3.5" />New</button>
					{#each visibleTables as name (name)}
						<button type="button" class={`${row} ${name === table ? "bg-muted font-medium text-foreground" : "text-foreground/80"}`} disabled={busy} onclick={() => onTable(name)} title={name}>
							<TableIcon class="size-3.5 shrink-0 text-muted-foreground" /><span class="truncate">{name}</span>
						</button>
					{:else}
						<p class="px-2 py-1 text-xs text-muted-foreground">No tables</p>
					{/each}
				</div>
			{/if}
		{/each}
	</div>
	<p class="border-t border-border px-3 py-2 text-[11px] leading-4 text-muted-foreground">Signed in as <span class="font-mono text-foreground">{username}</span>. Only databases this user can access are listed; sign in as root to see everything.</p>
</nav>
