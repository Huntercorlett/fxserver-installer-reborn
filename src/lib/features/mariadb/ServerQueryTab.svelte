<script lang="ts">
	import { untrack } from "svelte";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { executeMariaDBQuery, type MariaDBCredentials, type MariaDBQueryResult } from "$lib/modules/mariadb";
	import ResultTable from "./ResultTable.svelte";

	let { credentials, sql, title, refreshKey = 0 }: { credentials: MariaDBCredentials; sql: string; title: string; refreshKey?: number } = $props();
	let result = $state.raw<MariaDBQueryResult | null>(null);
	let loading = $state(false);
	let failure = $state("");
	let requestId = 0;

	async function load() {
		const id = ++requestId;
		loading = true; failure = "";
		try {
			const next = await executeMariaDBQuery({ ...credentials, database: null }, sql);
			if (id !== requestId) return;
			if (next.success) result = next; else { result = null; failure = next.stderr || "The query failed."; }
		} catch (caught) { if (id === requestId) { result = null; failure = String(caught); } }
		finally { if (id === requestId) loading = false; }
	}

	$effect(() => { void [refreshKey, sql]; untrack(() => void load()); });
</script>

<section class="space-y-3">
	<div class="border-b border-border pb-2"><h2 class="text-sm font-semibold">{title}</h2></div>
	{#if failure}<Notice tone="error" message={failure} onDismiss={() => failure = ""} />{/if}
	{#if result}<ResultTable columns={result.columns} rows={result.rows} />{:else if loading}<p class="py-8 text-sm text-muted-foreground">Loading...</p>{/if}
</section>
