<script lang="ts">
	import CheckIcon from "@lucide/svelte/icons/check";
	import CopyIcon from "@lucide/svelte/icons/copy";
	import HeartIcon from "@lucide/svelte/icons/heart";
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { invoke } from "@tauri-apps/api/core";

	type Credit = { name: string; discord: string };
	let credits = $state<Credit[]>([]);
	let error = $state("");
	let copied = $state("");

	onMount(async () => {
		try {
			credits = await invoke<Credit[]>("get_credits");
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		}
	});

	async function copy(handle: string) {
		try {
			await navigator.clipboard.writeText(handle);
			copied = handle;
			setTimeout(() => copied === handle && (copied = ""), 1500);
		} catch {
			error = "Could not copy to the clipboard.";
		}
	}
</script>

<section class="mx-auto max-w-2xl space-y-6">
	<header class="flex items-center gap-3"><HeartIcon class="size-6 text-muted-foreground" /><h1 class="text-2xl font-semibold">Credits</h1></header>
	{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}
	<p class="text-sm text-muted-foreground">FXServer Installer was made by:</p>
	<ul class="grid gap-3">
		{#each credits as credit (credit.name)}
			<li class="flex items-center justify-between gap-4 rounded-md border border-border bg-card px-4 py-3">
				<div class="min-w-0"><p class="truncate text-base font-semibold">{credit.name}</p><p class="truncate font-mono text-xs text-muted-foreground">Discord: {credit.discord}</p></div>
				<Button variant="outline" size="sm" onclick={() => copy(credit.discord)} aria-label={`Copy ${credit.name}'s Discord username`}>
					{#if copied === credit.discord}<CheckIcon />Copied{:else}<CopyIcon />Copy Discord{/if}
				</Button>
			</li>
		{/each}
	</ul>
</section>
