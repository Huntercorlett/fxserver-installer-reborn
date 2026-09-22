<script lang="ts">
	import EyeIcon from "@lucide/svelte/icons/table-properties";
	import ListIcon from "@lucide/svelte/icons/list";
	import SearchIcon from "@lucide/svelte/icons/search";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import EraserIcon from "@lucide/svelte/icons/eraser";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { formatSize, type TableAction, type TableInfo } from "$lib/modules/databaseBrowser";

	type Props = {
		tables: TableInfo[];
		busy: boolean;
		onOpen: (name: string, view: "rows" | "columns", intent: "none" | "insert" | "search") => void;
		onAction: (action: TableAction, names: string[]) => void;
	};
	let { tables, busy, onOpen, onAction }: Props = $props();

	let picked = $state<string[]>([]);
	let bulk = $state("");
	// Derived (not an effect) so tables that disappear drop out of the selection without re-triggering itself.
	const selected = $derived(picked.filter((name) => tables.some((item) => item.name === name)));
	const all = $derived(tables.length > 0 && selected.length === tables.length);
	const sums = $derived({
		rows: tables.reduce((total, item) => total + (item.rows ?? 0), 0),
		size: tables.reduce((total, item) => total + item.dataBytes + item.indexBytes, 0),
		overhead: tables.reduce((total, item) => total + item.freeBytes, 0),
	});
	function toggle(name: string) { picked = selected.includes(name) ? selected.filter((item) => item !== name) : [...selected, name]; }
	function runBulk(event: Event) {
		const action = (event.currentTarget as HTMLSelectElement).value as TableAction | "";
		bulk = "";
		if (action && selected.length) onAction(action, [...selected]);
	}
	const link = "inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 text-primary hover:bg-muted disabled:pointer-events-none disabled:opacity-40";
	const danger = "inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 text-destructive hover:bg-destructive/10 disabled:pointer-events-none disabled:opacity-40";
</script>

<div class="overflow-x-auto rounded-md border border-border">
	<table class="w-full border-collapse text-left text-xs">
		<thead class="bg-muted/60 text-muted-foreground">
			<tr>
				<th class="w-8 px-3 py-2"><span class="sr-only">Select</span></th>
				<th class="px-3 py-2 font-medium">Table</th>
				<th class="px-3 py-2 font-medium">Action</th>
				<th class="px-3 py-2 text-right font-medium">Rows</th>
				<th class="px-3 py-2 font-medium">Type</th>
				<th class="px-3 py-2 font-medium">Collation</th>
				<th class="px-3 py-2 text-right font-medium">Size</th>
				<th class="px-3 py-2 text-right font-medium">Overhead</th>
			</tr>
		</thead>
		<tbody>
			{#each tables as item (item.name)}
				{@const isTable = item.kind === "BASE TABLE"}
				<tr class="border-t border-border/60 hover:bg-muted/40" class:bg-muted={selected.includes(item.name)}>
					<td class="px-3 py-1.5"><input type="checkbox" checked={selected.includes(item.name)} onchange={() => toggle(item.name)} aria-label={`Select ${item.name}`} class="size-3.5 accent-foreground" /></td>
					<td class="px-3 py-1.5 font-mono"><button type="button" class="hover:underline disabled:no-underline" disabled={busy || !isTable} onclick={() => onOpen(item.name, "rows", "none")}>{item.name}</button></td>
					<td class="px-3 py-1.5">
						<div class="flex flex-wrap items-center gap-x-0.5 whitespace-nowrap">
							<button type="button" class={link} disabled={busy || !isTable} onclick={() => onOpen(item.name, "rows", "none")}><EyeIcon class="size-3" />Browse</button>
							<button type="button" class={link} disabled={busy || !isTable} onclick={() => onOpen(item.name, "columns", "none")}><ListIcon class="size-3" />Structure</button>
							<button type="button" class={link} disabled={busy || !isTable} onclick={() => onOpen(item.name, "rows", "search")}><SearchIcon class="size-3" />Search</button>
							<button type="button" class={link} disabled={busy || !isTable} onclick={() => onOpen(item.name, "rows", "insert")}><PlusIcon class="size-3" />Insert</button>
							<button type="button" class={danger} disabled={busy || !isTable} onclick={() => onAction("empty", [item.name])}><EraserIcon class="size-3" />Empty</button>
							<button type="button" class={danger} disabled={busy} onclick={() => onAction("drop", [item.name])}><Trash2Icon class="size-3" />Drop</button>
						</div>
					</td>
					<td class="px-3 py-1.5 text-right font-mono" title="InnoDB row counts are estimates">{item.rows === null ? "-" : `${item.engine === "InnoDB" ? "~" : ""}${item.rows.toLocaleString()}`}</td>
					<td class="px-3 py-1.5">{isTable ? (item.engine ?? "-") : "View"}</td>
					<td class="px-3 py-1.5 font-mono text-muted-foreground">{item.collation ?? "-"}</td>
					<td class="px-3 py-1.5 text-right font-mono">{isTable ? formatSize(item.dataBytes + item.indexBytes) : "-"}</td>
					<td class="px-3 py-1.5 text-right font-mono text-muted-foreground">{item.freeBytes ? formatSize(item.freeBytes) : "-"}</td>
				</tr>
			{:else}
				<tr><td colspan="8" class="px-3 py-10 text-center text-muted-foreground">No tables in this database.</td></tr>
			{/each}
		</tbody>
		{#if tables.length}
			<tfoot class="border-t border-border bg-muted/40 font-medium">
				<tr><td></td><td class="px-3 py-2">{tables.length} {tables.length === 1 ? "table" : "tables"}</td><td class="px-3 py-2 text-muted-foreground">Sum</td><td class="px-3 py-2 text-right font-mono">{sums.rows.toLocaleString()}</td><td colspan="2"></td><td class="px-3 py-2 text-right font-mono">{formatSize(sums.size)}</td><td class="px-3 py-2 text-right font-mono">{sums.overhead ? formatSize(sums.overhead) : "-"}</td></tr>
			</tfoot>
		{/if}
	</table>
</div>

<div class="mt-3 flex flex-wrap items-center gap-3 text-xs">
	<label class="flex items-center gap-2"><input type="checkbox" checked={all} onchange={() => (picked = all ? [] : tables.map((item) => item.name))} class="size-3.5 accent-foreground" disabled={!tables.length} />Check all</label>
	<select value={bulk} onchange={runBulk} disabled={busy || !selected.length} aria-label="With selected" class="h-8 rounded-sm border border-input bg-background px-2 text-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50">
		<option value="">With selected ({selected.length})</option>
		<option value="empty">Empty</option>
		<option value="drop">Drop</option>
		<option value="optimize">Optimize table</option>
		<option value="analyze">Analyze table</option>
		<option value="check">Check table</option>
		<option value="repair">Repair table</option>
	</select>
</div>
