<script lang="ts">
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";

	type Props = {
		title: string;
		description: string;
		sql: string[];
		confirmWord: string;
		actionLabel: string;
		foreignKeyOption?: boolean;
		busy: boolean;
		onConfirm: (disableForeignKeyChecks: boolean) => void;
		onCancel: () => void;
	};
	let { title, description, sql, confirmWord, actionLabel, foreignKeyOption = false, busy, onConfirm, onCancel }: Props = $props();
	let typed = $state("");
	let disableForeignKeys = $state(false);
</script>

<svelte:window onkeydown={(event) => event.key === "Escape" && !busy && onCancel()} />

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" role="presentation" onclick={(event) => event.target === event.currentTarget && !busy && onCancel()}>
	<div role="alertdialog" aria-modal="true" aria-labelledby="danger-title" class="w-full max-w-lg space-y-4 rounded-md border border-destructive/40 bg-card p-5 shadow-xl">
		<div class="flex items-start gap-3">
			<TriangleAlertIcon class="mt-0.5 size-5 shrink-0 text-destructive" />
			<div class="min-w-0 space-y-1"><h2 id="danger-title" class="text-base font-semibold">{title}</h2><p class="text-sm text-muted-foreground">{description}</p></div>
		</div>
		<pre class="max-h-40 overflow-auto rounded-sm border border-border bg-background p-3 font-mono text-xs whitespace-pre-wrap">{sql.join("\n")}</pre>
		{#if foreignKeyOption}
			<label class="flex items-start gap-2 text-xs text-muted-foreground"><input type="checkbox" bind:checked={disableForeignKeys} class="mt-0.5 size-3.5 accent-foreground" />Ignore foreign key checks for this action</label>
		{/if}
		<label class="grid gap-1.5 text-xs font-medium text-muted-foreground">Type <span class="font-mono text-foreground">{confirmWord}</span> to confirm
			<Input bind:value={typed} autocomplete="off" spellcheck={false} class="font-mono" />
		</label>
		<div class="flex justify-end gap-2">
			<Button variant="outline" disabled={busy} onclick={onCancel}>Cancel</Button>
			<Button variant="destructive" disabled={busy || typed !== confirmWord} onclick={() => onConfirm(disableForeignKeys)}>{actionLabel}</Button>
		</div>
	</div>
</div>
