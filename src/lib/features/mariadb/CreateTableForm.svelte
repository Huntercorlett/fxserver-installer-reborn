<script lang="ts">
	import PlusIcon from "@lucide/svelte/icons/plus";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { columnTypes, emptyColumn, type ColumnSpec, type CreateTableSpec } from "$lib/modules/databaseBrowser";

	type Props = { database: string; busy: boolean; onCreate: (spec: CreateTableSpec) => void };
	let { database, busy, onCreate }: Props = $props();

	let name = $state("");
	let count = $state(4);
	let columns = $state<ColumnSpec[]>([]);
	let engine = $state("InnoDB");
	let collation = $state("utf8mb4_unicode_ci");
	let problem = $state("");

	const field = "h-8 rounded-sm border border-input bg-background px-2 text-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50";

	function start() {
		const wanted = Math.min(128, Math.max(1, Math.floor(Number(count)) || 1));
		count = wanted;
		columns = Array.from({ length: wanted }, (_, index) => {
			const column = emptyColumn();
			if (index === 0) Object.assign(column, { name: "id", primary: true, autoIncrement: true, unsigned: true });
			return column;
		});
		problem = "";
	}
	function submit() {
		const trimmed = name.trim();
		if (!trimmed) return void (problem = "Enter a table name.");
		if (columns.some((column) => !column.name.trim())) return void (problem = "Every column needs a name.");
		problem = "";
		onCreate({ database, name: trimmed, engine, collation, columns: columns.map((column) => ({ ...column, name: column.name.trim() })) });
	}
	export function reset() { name = ""; columns = []; problem = ""; }
</script>

<section id="create-table" class="space-y-3 rounded-md border border-border bg-card p-4">
	<h2 class="flex items-center gap-2 text-sm font-semibold"><PlusIcon class="size-4 text-primary" />Create table</h2>
	<div class="flex flex-wrap items-end gap-3 text-xs">
		<label class="grid gap-1 font-medium text-muted-foreground">Name<Input bind:value={name} disabled={busy} maxlength={64} placeholder="table_name" class="h-8 w-56 text-xs" /></label>
		<label class="grid gap-1 font-medium text-muted-foreground">Number of columns<Input type="number" bind:value={count} disabled={busy} min={1} max={128} class="h-8 w-28 text-xs" /></label>
		<Button size="sm" variant="outline" disabled={busy} onclick={start}>Go</Button>
	</div>

	{#if columns.length}
		<div class="overflow-x-auto rounded-sm border border-border">
			<table class="w-full border-collapse text-left text-xs">
				<thead class="bg-muted/60 text-muted-foreground"><tr>{#each ["Name", "Type", "Length", "Default", "Value", "Null", "Unsigned", "A_I", "Primary", "Unique", ""] as heading}<th class="px-2 py-1.5 font-medium whitespace-nowrap">{heading}</th>{/each}</tr></thead>
				<tbody>
					{#each columns as column, index}
						<tr class="border-t border-border/60">
							<td class="p-1.5"><input bind:value={column.name} disabled={busy} maxlength="64" aria-label={`Column ${index + 1} name`} class={`${field} w-36 font-mono`} /></td>
							<td class="p-1.5"><select bind:value={column.dataType} disabled={busy} aria-label={`Column ${index + 1} type`} class={field}>{#each columnTypes as type}<option value={type}>{type}</option>{/each}</select></td>
							<td class="p-1.5"><input bind:value={column.length} disabled={busy} maxlength="8" placeholder="auto" aria-label={`Column ${index + 1} length`} class={`${field} w-16 font-mono`} /></td>
							<td class="p-1.5"><select bind:value={column.defaultKind} disabled={busy} aria-label={`Column ${index + 1} default`} class={field}><option value="none">None</option><option value="value">As defined</option><option value="null">NULL</option><option value="currentTimestamp">CURRENT_TIMESTAMP</option></select></td>
							<td class="p-1.5"><input bind:value={column.defaultValue} disabled={busy || column.defaultKind !== "value"} maxlength="255" aria-label={`Column ${index + 1} default value`} class={`${field} w-28 font-mono`} /></td>
							<td class="p-1.5 text-center"><input type="checkbox" bind:checked={column.nullable} disabled={busy || column.primary} aria-label={`Column ${index + 1} allows NULL`} class="size-3.5 accent-foreground" /></td>
							<td class="p-1.5 text-center"><input type="checkbox" bind:checked={column.unsigned} disabled={busy} aria-label={`Column ${index + 1} unsigned`} class="size-3.5 accent-foreground" /></td>
							<td class="p-1.5 text-center"><input type="checkbox" bind:checked={column.autoIncrement} disabled={busy} onchange={() => column.autoIncrement && (column.primary = true)} aria-label={`Column ${index + 1} auto increment`} class="size-3.5 accent-foreground" /></td>
							<td class="p-1.5 text-center"><input type="checkbox" bind:checked={column.primary} disabled={busy} onchange={() => column.primary && (column.nullable = false)} aria-label={`Column ${index + 1} primary key`} class="size-3.5 accent-foreground" /></td>
							<td class="p-1.5 text-center"><input type="checkbox" bind:checked={column.unique} disabled={busy || column.primary} aria-label={`Column ${index + 1} unique`} class="size-3.5 accent-foreground" /></td>
							<td class="p-1.5"><Button variant="ghost" size="icon-sm" disabled={busy || columns.length === 1} title="Remove column" aria-label={`Remove column ${index + 1}`} onclick={() => (columns = columns.filter((_, i) => i !== index))}><XIcon /></Button></td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<div class="flex flex-wrap items-end justify-between gap-3 text-xs">
			<div class="flex flex-wrap items-end gap-3">
				<Button variant="ghost" size="sm" disabled={busy || columns.length >= 128} onclick={() => (columns = [...columns, emptyColumn()])}><PlusIcon />Add column</Button>
				<label class="grid gap-1 font-medium text-muted-foreground">Engine<select bind:value={engine} disabled={busy} class={field}>{#each ["InnoDB", "Aria", "MyISAM", "MEMORY"] as option}<option>{option}</option>{/each}</select></label>
				<label class="grid gap-1 font-medium text-muted-foreground">Collation<select bind:value={collation} disabled={busy} class={field}>{#each ["utf8mb4_unicode_ci", "utf8mb4_general_ci", "utf8mb4_bin", "utf8mb3_general_ci", "latin1_swedish_ci"] as option}<option>{option}</option>{/each}</select></label>
			</div>
			<div class="flex items-center gap-3">{#if problem}<span class="text-destructive">{problem}</span>{/if}<Button size="sm" disabled={busy} onclick={submit}>Save</Button></div>
		</div>
	{/if}
</section>
