<script lang="ts">
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { databaseSession, setRememberLogin } from "$lib/core/databaseSession.svelte";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";

	type Props = {
		busy: boolean;
		credentialsReady: boolean;
		connectionError: string;
		credentials: MariaDBCredentials;
		stretch?: boolean;
		onApply: () => void;
	};

	let { busy, credentialsReady, connectionError, credentials = $bindable(), stretch = true, onApply }: Props = $props();

	let expanded = $state(false);
	let showAdvanced = $state(false);

	const localHosts = ["", "localhost", "127.0.0.1", "::1", "[::1]"];
	const isLocal = $derived(localHosts.includes((credentials.host ?? "").trim().toLowerCase()));
	// Local connections only need the password; other hosts need the full login.
	const showAllFields = $derived(!isLocal || showAdvanced);
	const collapsed = $derived(credentialsReady && !expanded && !connectionError);
	const summary = $derived(`${credentials.username || "root"}@${credentials.host || "localhost"}:${credentials.port || 3306}`);

	$effect(() => {
		if (credentialsReady) expanded = false;
	});

	function apply() {
		onApply();
	}
</script>

{#if collapsed}
	<Card.Root class={`${stretch ? "h-full" : ""} rounded-md border-border bg-card shadow-sm`}>
		<Card.Content class="flex flex-wrap items-center justify-between gap-3 py-4">
			<div class="flex min-w-0 items-center gap-3">
				<CircleCheckIcon class="size-5 shrink-0 text-emerald-300" />
				<div class="min-w-0">
					<p class="text-sm font-medium">Connected</p>
					<p class="truncate font-mono text-xs text-muted-foreground" title={summary}>{summary}{databaseSession.remember ? " · remembered" : ""}</p>
				</div>
			</div>
			<Button variant="outline" size="sm" onclick={() => (expanded = true)} disabled={busy} title="Change the MariaDB login">Change</Button>
		</Card.Content>
	</Card.Root>
{:else}
	<Card.Root class={`${stretch ? "h-full" : ""} rounded-md border-border bg-card shadow-sm`}>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<KeyRoundIcon class="size-5" />
				</div>
				<div>
					<Card.Title>MariaDB login</Card.Title>
					<Card.Description>
						{isLocal && !showAdvanced ? `Enter the admin password for ${summary}. It is shared with every database page.` : "Admin login used for every database page."}
					</Card.Description>
				</div>
			</div>
		</Card.Header>

		<Card.Content class="space-y-4">
			<div class="grid gap-4 sm:grid-cols-2">
				{#if showAllFields}
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Host</span>
						<Input bind:value={credentials.host} disabled={busy} placeholder="127.0.0.1" title="MariaDB host for admin actions." />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Port</span>
						<Input type="number" bind:value={credentials.port} disabled={busy} placeholder="3306" title="MariaDB port for admin actions." />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Admin Username</span>
						<Input bind:value={credentials.username} disabled={busy} placeholder="root" title="Admin username used to connect to MariaDB." />
					</label>
				{/if}
				<label class={`grid gap-2 ${showAllFields ? "" : "sm:col-span-2"}`}>
					<span class="text-xs font-medium text-muted-foreground">Admin Password</span>
					<PasswordInput bind:value={credentials.password} disabled={busy} placeholder="Root/admin password" title="Admin password used to connect to MariaDB." />
				</label>
				{#if showAllFields}
					<label class="grid gap-2 sm:col-span-2">
						<span class="text-xs font-medium text-muted-foreground">Default Database</span>
						<Input bind:value={credentials.database} disabled={busy} placeholder="Optional default schema" title="Optional database to use when running queries." />
					</label>
				{/if}
			</div>

			<label class="flex items-start gap-3 text-sm">
				<Checkbox checked={databaseSession.remember} onCheckedChange={(value) => void setRememberLogin(Boolean(value))} disabled={busy} class="mt-0.5" />
				<span>
					<span class="font-medium text-foreground">Remember on this PC</span>
					<span class="block text-xs leading-5 text-muted-foreground">Saved encrypted for your Windows account, so you are not asked again after restarting. Only saved after a successful connection.</span>
				</span>
			</label>

			<div class="flex flex-wrap gap-2">
				<Button class="min-w-40 flex-1" onclick={apply} disabled={busy} title="Connect and refresh MariaDB status, users, and databases">
					<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
					{credentialsReady ? "Reconnect" : "Connect"}
				</Button>
				{#if isLocal}
					<Button variant="ghost" onclick={() => (showAdvanced = !showAdvanced)} disabled={busy}>{showAdvanced ? "Hide advanced" : "Advanced"}</Button>
				{/if}
				{#if credentialsReady}
					<Button variant="ghost" onclick={() => (expanded = false)} disabled={busy}>Cancel</Button>
				{/if}
			</div>

			{#if connectionError}
				<p class="rounded-sm border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">{connectionError}</p>
			{:else if !credentialsReady}
				<p class="rounded-sm border border-border bg-background/60 px-3 py-2 text-xs text-muted-foreground">Not connected yet.</p>
			{/if}
		</Card.Content>
	</Card.Root>
{/if}
