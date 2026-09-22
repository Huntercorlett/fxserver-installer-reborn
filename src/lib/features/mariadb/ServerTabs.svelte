<script lang="ts">
	import { serverTabs, type ServerTab } from "$lib/modules/serverTabs";

	let { active, disabled = false, onSelect }: { active: ServerTab | null; disabled?: boolean; onSelect: (tab: ServerTab) => void } = $props();
</script>

<div class="flex overflow-x-auto rounded-md border border-border bg-card" role="tablist" aria-label="Server tools">
	{#each serverTabs as tab (tab.id)}
		{@const Icon = tab.icon}
		<button
			type="button"
			role="tab"
			aria-selected={active === tab.id}
			{disabled}
			class={["flex shrink-0 items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-medium whitespace-nowrap outline-none transition-colors focus-visible:bg-muted disabled:opacity-50", active === tab.id ? "border-primary bg-muted/50 text-foreground" : "border-transparent text-muted-foreground hover:bg-muted/30 hover:text-foreground"]}
			onclick={() => onSelect(tab.id)}
		>
			<Icon class="size-4" />{tab.label}
		</button>
	{/each}
</div>
