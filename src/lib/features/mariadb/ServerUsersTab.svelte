<script lang="ts">
	import { untrack } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { getMariaDBUserAccess, listMariaDBUsers, type MariaDBCredentials, type MariaDBUser } from "$lib/modules/mariadb";

	let { credentials, refreshKey = 0 }: { credentials: MariaDBCredentials; refreshKey?: number } = $props();
	let users = $state.raw<MariaDBUser[]>([]);
	let loading = $state(false);
	let failure = $state("");
	let openKey = $state("");
	let grants = $state.raw<string[]>([]);
	let requestId = 0;

	async function load() {
		const id = ++requestId;
		loading = true; failure = ""; openKey = "";
		try {
			const next = await listMariaDBUsers({ ...credentials, database: null });
			if (id === requestId) users = next;
		} catch (caught) { if (id === requestId) failure = String(caught); }
		finally { if (id === requestId) loading = false; }
	}
	async function toggle(user: MariaDBUser) {
		const key = `${user.username}@${user.host}`;
		if (openKey === key) { openKey = ""; return; }
		try {
			const access = await getMariaDBUserAccess({ ...credentials, database: null }, user.username, user.host);
			grants = access.grants; openKey = key;
		} catch (caught) { failure = String(caught); }
	}

	$effect(() => { void refreshKey; untrack(() => void load()); });
</script>

<section class="space-y-3">
	<div class="border-b border-border pb-2"><h2 class="text-sm font-semibold">User accounts overview</h2></div>
	{#if failure}<Notice tone="error" message={failure} onDismiss={() => failure = ""} />{/if}
	<div class="overflow-auto border-y border-border">
		<table class="w-full border-collapse text-left text-xs">
			<thead class="bg-background"><tr>{#each ["User name", "Host name", "Plugin", "Password expired", "Locked", "Privileges"] as heading}<th class="border-b border-border px-3 py-2 font-medium whitespace-nowrap">{heading}</th>{/each}</tr></thead>
			<tbody>
				{#each users as user (`${user.username}@${user.host}`)}
					{@const key = `${user.username}@${user.host}`}
					<tr class="border-b border-border/50 hover:bg-muted/40">
						<td class="px-3 py-2 font-mono">{user.username || "(any)"}</td><td class="px-3 py-2 font-mono">{user.host}</td><td class="px-3 py-2">{user.plugin || "-"}</td><td class="px-3 py-2">{user.passwordExpired || "-"}</td><td class="px-3 py-2">{user.locked || "-"}</td>
						<td class="px-3 py-1"><Button variant="ghost" size="sm" onclick={() => toggle(user)}>{openKey === key ? "Hide grants" : "Show grants"}</Button></td>
					</tr>
					{#if openKey === key}<tr class="border-b border-border/50 bg-muted/20"><td colspan="6" class="px-3 py-3"><pre class="max-h-60 overflow-auto font-mono text-xs leading-5 whitespace-pre-wrap">{grants.join("\n") || "No grants."}</pre></td></tr>{/if}
				{:else}
					<tr><td colspan="6" class="px-3 py-8 text-center text-muted-foreground">{loading ? "Loading users..." : "No users."}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p class="text-xs text-muted-foreground">Create, edit, or delete accounts from Manage MariaDB.</p>
</section>
