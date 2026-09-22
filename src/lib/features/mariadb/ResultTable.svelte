<script lang="ts">
	import { Input } from "$lib/components/ui/input/index.js";

	type Props = { columns: string[]; rows: string[][]; filterable?: boolean; limit?: number; empty?: string };

	let { columns, rows, filterable = true, limit = 1000, empty = "No rows." }: Props = $props();
	let filter = $state("");
	const needle = $derived(filter.trim().toLowerCase());
	const matched = $derived(needle ? rows.filter((row) => row.some((cell) => cell?.toLowerCase().includes(needle))) : rows);
	const shown = $derived(matched.slice(0, limit));
</script>

<div class="space-y-2">
	{#if filterable && rows.length > 10}
		<Input bind:value={filter} placeholder="Filter rows" aria-label="Filter rows" maxlength={128} class="h-8 max-w-xs text-xs" />
	{/if}
	<div class="max-h-[32rem] overflow-auto border-y border-border">
		<table class="w-full border-collapse text-left text-xs">
			<thead class="sticky top-0 bg-background"><tr>{#each columns as column}<th class="border-b border-border px-3 py-2 font-medium whitespace-nowrap">{column}</th>{/each}</tr></thead>
			<tbody>
				{#each shown as row}
					<tr class="border-b border-border/50 hover:bg-muted/40">{#each row as cell}<td class="max-w-96 truncate px-3 py-2 font-mono" title={cell}>{cell}</td>{/each}</tr>
				{:else}
					<tr><td colspan={Math.max(1, columns.length)} class="px-3 py-8 text-center text-muted-foreground">{empty}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="text-xs text-muted-foreground">{matched.length > limit ? `Showing first ${limit} of ${matched.length} rows` : `${matched.length} ${matched.length === 1 ? "row" : "rows"}`}</p>
</div>
