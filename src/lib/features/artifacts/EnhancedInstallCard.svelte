<script lang="ts">
	import DownloadIcon from "@lucide/svelte/icons/download";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { taskSession } from "$lib/core/tasks.svelte";
	import { enhancedDownloadUrl, fetchEnhancedCatalog, installEnhancedArtifact, type EnhancedArtifactBuild, type ArtifactInstallResult } from "$lib/modules/artifact";

	type Props = {
		destination: string;
		blocked: boolean;
		oninstalled: (result: ArtifactInstallResult) => void;
	};

	let { destination, blocked, oninstalled }: Props = $props();

	let builds = $state<EnhancedArtifactBuild[]>([]);
	let selected = $state("");
	let customUrl = $state("");
	let warning = $state("");
	let error = $state("");
	let loading = $state(false);
	const installing = $derived(taskSession.items.some((task) => task.command === "install_enhanced_artifact" && task.status === "running"));
	const url = $derived(customUrl.trim() || selected);

	onMount(() => void refresh());

	async function refresh() {
		loading = true;
		error = "";
		try {
			const catalog = await fetchEnhancedCatalog();
			builds = catalog.builds;
			selected = catalog.builds[0]?.downloadUrl ?? "";
			warning = catalog.warning ?? "";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			loading = false;
		}
	}

	async function install() {
		if (!url || !destination.trim() || installing) return;
		error = "";
		try {
			oninstalled(await installEnhancedArtifact(url, destination.trim()));
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		}
	}
</script>

<Card.Root class="rounded-sm border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-start justify-between gap-3">
			<div>
				<Card.Title>FiveM for GTAV Enhanced</Card.Title>
				<Card.Description>Installs cfx-server.exe from the Enhanced tab of the Cfx.re Server Download page.</Card.Description>
			</div>
			<div class="flex gap-2">
				<Button variant="outline" size="sm" onclick={() => openExternalUrl(enhancedDownloadUrl)}>Open page</Button>
				<Button variant="outline" size="sm" onclick={refresh} disabled={loading || installing} title="Reload the Enhanced builds">
					<RefreshCwIcon class={loading ? "animate-spin" : undefined} />
				</Button>
			</div>
		</div>
	</Card.Header>
	<Card.Content class="space-y-3">
		{#if error}
			<Notice tone="error" message={error} onDismiss={() => (error = "")} class="px-3 py-2 text-xs" />
		{/if}
		{#if blocked}
			<Notice tone="warn" message="This folder holds a Legacy FXServer.exe build. Pick a different folder for Enhanced." class="px-3 py-2 text-xs" />
		{/if}
		{#if warning}
			<Notice tone="warn" message={warning} class="px-3 py-2 text-xs" />
		{/if}

		<label class="grid gap-1.5">
			<span class="text-xs font-medium text-muted-foreground">Build</span>
			<select bind:value={selected} disabled={!builds.length || installing} class="h-9 rounded-sm border border-input bg-background px-2.5 font-mono text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50">
				{#if !builds.length}
					<option value="">{loading ? "Loading..." : "No builds found"}</option>
				{/if}
				{#each builds as build, index}
					<option value={build.downloadUrl}>{build.version.slice(0, 8)}{index === 0 ? " (latest)" : ""}</option>
				{/each}
			</select>
		</label>

		<label class="grid gap-1.5">
			<span class="text-xs font-medium text-muted-foreground">Or paste the Download link</span>
			<Input bind:value={customUrl} disabled={installing} placeholder="https://downloads.cfx-services.net/prod/.../cfx-server_win_x64.zip" class="rounded-sm font-mono" />
		</label>

		<Button class="w-full rounded-sm" onclick={install} disabled={installing || loading || blocked || !url || !destination.trim()} title="Download and extract the Enhanced server into the install folder">
			{#if installing}
				<LoaderCircleIcon class="animate-spin" />
				Installing...
			{:else}
				<DownloadIcon />
				Install Enhanced Server
			{/if}
		</Button>
	</Card.Content>
</Card.Root>
