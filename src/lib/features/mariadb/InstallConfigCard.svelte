<script lang="ts">
	import DownloadIcon from "@lucide/svelte/icons/download";
	import SlidersHorizontalIcon from "@lucide/svelte/icons/sliders-horizontal";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { onMount } from "svelte";
	import { listMariaDBReleases, listMariaDBVersions, type MariaDBInstallOptions, type MariaDBPackageInfo, type MariaDBRelease, type MariaDBSeries } from "$lib/modules/mariadb";

	type Props = {
		busy: boolean;
		packageInfo: MariaDBPackageInfo | null;
		installStage: string;
		installOptions: MariaDBInstallOptions;
		onInstall: () => void;
	};

	let { busy, packageInfo, installStage, installOptions = $bindable(), onInstall }: Props = $props();

	const selectClass = "h-9 rounded-sm border border-input bg-background px-2.5 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";

	const presets = {
		fivem: { label: "FiveM server (recommended)", values: { optimizeForTransactions: true, useUtf8: true, allowRemoteRootAccess: false, createAnonymousUser: false, skipNetworking: false, installHeidiSql: false, installDevelopmentFiles: false } },
		heidi: { label: "FiveM server + HeidiSQL", values: { optimizeForTransactions: true, useUtf8: true, allowRemoteRootAccess: false, createAnonymousUser: false, skipNetworking: false, installHeidiSql: true, installDevelopmentFiles: false } },
		developer: { label: "Developer (HeidiSQL + dev files)", values: { optimizeForTransactions: true, useUtf8: true, allowRemoteRootAccess: false, createAnonymousUser: false, skipNetworking: false, installHeidiSql: true, installDevelopmentFiles: true } },
		minimal: { label: "Minimal (server only, no tuning)", values: { optimizeForTransactions: false, useUtf8: true, allowRemoteRootAccess: false, createAnonymousUser: false, skipNetworking: false, installHeidiSql: false, installDevelopmentFiles: false } },
	} as const;

	let series = $state<MariaDBSeries[]>([]);
	let releases = $state<MariaDBRelease[]>([]);
	let selectedSeries = $state("");
	let selectedRelease = $state("");
	let versionError = $state("");
	let loadingVersions = $state(false);

	onMount(async () => {
		loadingVersions = true;
		try {
			series = await listMariaDBVersions();
		} catch (error) {
			versionError = error instanceof Error ? error.message : String(error);
		} finally {
			loadingVersions = false;
		}
	});

	function applyPreset(event: Event) {
		const preset = presets[(event.currentTarget as HTMLSelectElement).value as keyof typeof presets];
		if (preset) Object.assign(installOptions, preset.values);
	}

	async function chooseSeries(event: Event) {
		selectedSeries = (event.currentTarget as HTMLSelectElement).value;
		selectedRelease = "";
		releases = [];
		installOptions.version = selectedSeries;
		if (!selectedSeries) return;
		loadingVersions = true;
		versionError = "";
		try {
			releases = await listMariaDBReleases(selectedSeries);
		} catch (error) {
			versionError = error instanceof Error ? error.message : String(error);
		} finally {
			loadingVersions = false;
		}
	}

	function chooseRelease(event: Event) {
		selectedRelease = (event.currentTarget as HTMLSelectElement).value;
		installOptions.version = selectedRelease || selectedSeries;
	}

	const boolOptions = [
		["allowRemoteRootAccess", "Remote root", "Allow the MariaDB root account to connect from remote hosts."],
		["createAnonymousUser", "Anonymous user", "Create the default anonymous database user during installation."],
		["skipNetworking", "Skip networking", "Disable TCP networking and only allow local socket or pipe access."],
		["optimizeForTransactions", "Optimize", "Apply MariaDB's standard transactional configuration preset."],
		["useUtf8", "UTF-8", "Use UTF-8 as the default server character set."],
		["installHeidiSql", "HeidiSQL", "Install the bundled HeidiSQL database administration tool."],
		["installDevelopmentFiles", "Dev files", "Install MariaDB development headers and libraries."],
	] as const;
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<SlidersHorizontalIcon class="size-4" />
				</div>
				<div class="min-w-0">
					<Card.Title>Install Configuration</Card.Title>
					<Card.Description>
						{installOptions.version ? `MariaDB ${installOptions.version}` : packageInfo?.latestVersion ? `MariaDB ${packageInfo.latestVersion}` : "Default MariaDB 10.11 LTS"} - official Windows MSI.
					</Card.Description>
				</div>
			</div>
			<Button onclick={onInstall} disabled={busy} title="Install MariaDB with these settings">
				<DownloadIcon />
				Install
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		<div class="grid gap-3 md:grid-cols-3">
			<div class="rounded-sm border border-border bg-background px-3 py-2">
				<p class="text-xs text-muted-foreground">Recommended Version</p>
				<p class="mt-1 font-semibold">{packageInfo?.latestVersion || (packageInfo ? "Unavailable" : "Checking...")}</p>
			</div>
			<div class="rounded-sm border border-border bg-background px-3 py-2 md:col-span-2">
				<p class="text-xs text-muted-foreground">Install Progress</p>
				<p class="mt-1 text-sm font-medium">{installStage || "Ready to install."}</p>
			</div>
		</div>

		{#if busy && installStage}
			<p class="rounded-sm border border-primary/30 bg-primary/10 px-3 py-2 text-xs text-primary">
				If Windows asks for administrator permission, press Yes to allow the MariaDB MSI to register the service.
			</p>
		{/if}

		<div class="grid gap-3 md:grid-cols-3">
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Preset</span>
				<select onchange={applyPreset} title="Fill the options below with a ready-made combination." class={selectClass}>
					<option value="">Custom</option>
					{#each Object.entries(presets) as [key, preset]}
						<option value={key}>{preset.label}</option>
					{/each}
				</select>
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">MariaDB Series</span>
				<select value={selectedSeries} onchange={chooseSeries} disabled={loadingVersions && !series.length} title="Choose which MariaDB release series to install." class={selectClass}>
					<option value="">Default (10.11 LTS)</option>
					{#each series as item}
						<option value={item.series}>{item.series}{item.support ? ` - ${item.support}` : item.status ? ` - ${item.status}` : ""}</option>
					{/each}
				</select>
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Exact Version</span>
				<select value={selectedRelease} onchange={chooseRelease} disabled={!selectedSeries || loadingVersions} title="Pin a specific patch release, or install the newest in the series." class={selectClass}>
					<option value="">{selectedSeries ? "Latest in series" : "Select a series first"}</option>
					{#each releases as release}
						<option value={release.version}>{release.version}{release.date ? ` (${release.date})` : ""}</option>
					{/each}
				</select>
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Root Password</span>
				<PasswordInput bind:value={installOptions.rootPassword} placeholder="Required root password" title="Root password used by the MariaDB installer." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Service Name</span>
				<Input bind:value={installOptions.serviceName} placeholder="MariaDB" title="Windows service name to register for MariaDB." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Port</span>
				<Input type="number" bind:value={installOptions.port} placeholder="3306" title="TCP port MariaDB should listen on." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Install Directory</span>
				<Input bind:value={installOptions.installDir} placeholder="C:\\Program Files\\MariaDB" title="Optional MariaDB installation directory." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Data Directory</span>
				<Input bind:value={installOptions.dataDir} placeholder="C:\\Program Files\\MariaDB\\data" title="Optional directory for MariaDB data files." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Buffer Pool</span>
				<Input bind:value={installOptions.bufferPoolSize} placeholder="RAM/8, 512M, 1G" title="Optional InnoDB buffer pool size." />
			</label>
			<label class="grid gap-1.5">
				<span class="text-xs font-medium text-muted-foreground">Page Size</span>
				<select
					bind:value={installOptions.pageSize}
					title="Optional InnoDB page size."
					class="h-9 rounded-sm border border-input bg-background px-2.5 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
				>
					<option value="">Default</option>
					<option value="4K">4K</option>
					<option value="8K">8K</option>
					<option value="16K">16K</option>
					<option value="32K">32K</option>
					<option value="64K">64K</option>
				</select>
			</label>
		</div>

		<div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
			{#each boolOptions as [key, label, description]}
				<label class="flex h-9 items-center gap-2 rounded-sm border border-border bg-background px-2.5 text-sm whitespace-nowrap" title={description}>
					<input
						type="checkbox"
						bind:checked={installOptions[key]}
						class="size-3.5 rounded-xs border-border bg-background accent-foreground"
						title={description}
					/>
					<span>{label}</span>
				</label>
			{/each}
		</div>

		{#if versionError}
			<p class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs text-amber-100">Could not load the MariaDB version list ({versionError}). The default 10.11 LTS will be used unless a version is selected.</p>
		{/if}

		{#if installOptions.skipNetworking}
			<p class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs text-amber-100">
				Skip networking disables TCP/IP. The service can install and run, but this app's MariaDB connection tools expect localhost TCP access.
			</p>
		{/if}

		<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
			If preserved MariaDB data is detected, the app installs fresh binaries into an empty folder and reattaches the Windows service to the existing databases.
		</p>
	</Card.Content>
</Card.Root>
