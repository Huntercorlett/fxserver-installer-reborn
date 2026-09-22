<script lang="ts">
	import { untrack } from "svelte";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { executeMariaDBQuery, type MariaDBCredentials, type MariaDBQueryResult } from "$lib/modules/mariadb";
	import { formatSize } from "$lib/modules/databaseBrowser";
	import ResultTable from "./ResultTable.svelte";

	let { credentials, refreshKey = 0 }: { credentials: MariaDBCredentials; refreshKey?: number } = $props();
	let status = $state.raw<MariaDBQueryResult | null>(null);
	let processes = $state.raw<MariaDBQueryResult | null>(null);
	let loading = $state(false);
	let failure = $state("");
	let requestId = 0;
	const values = $derived(new Map((status?.rows ?? []).map(([name, value]) => [name, value])));
	const num = (name: string) => Number(values.get(name) ?? 0) || 0;
	const cards = $derived([
		["Uptime", uptime(num("Uptime"))], ["Connections", num("Connections").toLocaleString()], ["Threads connected", String(num("Threads_connected"))],
		["Threads running", String(num("Threads_running"))], ["Max used connections", String(num("Max_used_connections"))], ["Queries", num("Questions").toLocaleString()],
		["Slow queries", num("Slow_queries").toLocaleString()], ["Aborted connects", num("Aborted_connects").toLocaleString()], ["Received", formatSize(num("Bytes_received"))], ["Sent", formatSize(num("Bytes_sent"))],
	]);

	function uptime(seconds: number) {
		const days = Math.floor(seconds / 86400), hours = Math.floor((seconds % 86400) / 3600), minutes = Math.floor((seconds % 3600) / 60);
		return days ? `${days}d ${hours}h ${minutes}m` : hours ? `${hours}h ${minutes}m` : `${minutes}m ${seconds % 60}s`;
	}
	async function load() {
		const id = ++requestId;
		loading = true; failure = "";
		try {
			const scope = { ...credentials, database: null };
			const [nextStatus, nextProcesses] = await Promise.all([executeMariaDBQuery(scope, "SHOW GLOBAL STATUS;"), executeMariaDBQuery(scope, "SHOW FULL PROCESSLIST;")]);
			if (id !== requestId) return;
			if (!nextStatus.success) { failure = nextStatus.stderr || "Could not read server status."; return; }
			status = nextStatus; processes = nextProcesses.success ? nextProcesses : null;
		} catch (caught) { if (id === requestId) failure = String(caught); }
		finally { if (id === requestId) loading = false; }
	}

	$effect(() => { void refreshKey; untrack(() => void load()); });
</script>

<section class="space-y-5">
	<div class="border-b border-border pb-2"><h2 class="text-sm font-semibold">Server status</h2></div>
	{#if failure}<Notice tone="error" message={failure} onDismiss={() => failure = ""} />{/if}
	{#if status}
		<div class="grid gap-2 sm:grid-cols-3 xl:grid-cols-5">{#each cards as [label, value]}<div class="rounded-sm border border-border bg-card p-3"><p class="text-xs text-muted-foreground">{label}</p><p class="mt-1 text-sm font-semibold">{value}</p></div>{/each}</div>
		{#if processes}<div class="space-y-2"><h3 class="text-xs font-semibold text-muted-foreground">Processes</h3><ResultTable columns={processes.columns} rows={processes.rows} filterable={false} /></div>{/if}
		<div class="space-y-2"><h3 class="text-xs font-semibold text-muted-foreground">All status variables</h3><ResultTable columns={status.columns} rows={status.rows} /></div>
	{:else if loading}<p class="py-8 text-sm text-muted-foreground">Loading...</p>{/if}
</section>
