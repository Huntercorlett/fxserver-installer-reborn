<script lang="ts">
	import CopyIcon from "@lucide/svelte/icons/copy";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import GlobeIcon from "@lucide/svelte/icons/globe";
	import ListIcon from "@lucide/svelte/icons/list";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import PlayIcon from "@lucide/svelte/icons/play";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SquareIcon from "@lucide/svelte/icons/square";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { onDestroy, onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { log } from "$lib/core/logger.svelte";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { chooseFolder } from "$lib/core/selectFolder";
	import {
		backendCommandExample,
		emptyWebsiteSite,
		firewallCommand,
		formatBytes,
		getWebsiteRequests,
		getWebsiteSites,
		listWebsitePages,
		removeWebsiteSite,
		saveWebsiteSite,
		startWebsiteSite,
		stopWebsiteSite,
		validateWebsiteSite,
		type WebsiteRequestEntry,
		type WebsiteRuntime,
		type WebsiteSiteConfig,
		type WebsiteSiteStatus,
	} from "$lib/modules/website";

	const refreshMs = 2000;
	const timeFormatter = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });

	let sites = $state<WebsiteSiteStatus[]>([]);
	let loaded = $state(false);
	let error = $state("");
	let message = $state("");
	let form = $state<WebsiteSiteConfig | null>(null);
	let formError = $state("");
	let formBusy = $state(false);
	let busyId = $state("");
	let confirmRemoveId = $state("");
	let requestsOpenId = $state("");
	let requests = $state.raw<WebsiteRequestEntry[]>([]);
	let copied = $state("");
	let refreshing = false;
	let timer: number | undefined;
	let copiedTimer: number | undefined;

	const editingExisting = $derived(Boolean(form?.id));
	const editingRunning = $derived(Boolean(form?.id && sites.find((site) => site.config.id === form?.id)?.running));

	onMount(() => {
		void refresh();
		timer = window.setInterval(() => void refresh(true), refreshMs);
	});

	onDestroy(() => {
		window.clearInterval(timer);
		window.clearTimeout(copiedTimer);
	});

	function describe(caught: unknown) {
		return caught instanceof Error ? caught.message : String(caught);
	}

	async function refresh(silent = false) {
		if (refreshing) return;
		refreshing = true;
		try {
			sites = await getWebsiteSites();
			if (requestsOpenId) requests = await getWebsiteRequests(requestsOpenId);
			if (!silent) error = "";
		} catch (caught) {
			if (!silent) {
				error = describe(caught);
				log("Website hosting failed to refresh.", { level: "error", scope: "website", detail: error });
			}
		} finally {
			loaded = true;
			refreshing = false;
		}
	}

	async function run(id: string, action: () => Promise<WebsiteSiteStatus[]>, done?: string) {
		busyId = id;
		error = "";
		message = "";
		try {
			sites = await action();
			if (done) message = done;
		} catch (caught) {
			error = describe(caught);
			log("Website hosting action failed.", { level: "error", scope: "website", detail: error });
			await refresh(true);
		} finally {
			busyId = "";
		}
	}

	function start(site: WebsiteSiteStatus) {
		return run(site.config.id, () => startWebsiteSite(site.config.id), `${site.config.name} is running on port ${site.config.port}.`);
	}

	function stop(site: WebsiteSiteStatus) {
		return run(site.config.id, () => stopWebsiteSite(site.config.id), `${site.config.name} stopped.`);
	}

	async function remove(site: WebsiteSiteStatus) {
		confirmRemoveId = "";
		if (requestsOpenId === site.config.id) requestsOpenId = "";
		await run(site.config.id, () => removeWebsiteSite(site.config.id), `${site.config.name} was removed. Its files were not touched.`);
	}

	let pages = $state<string[]>([]);

	async function loadPages() {
		const root = form?.root.trim();
		try {
			pages = root ? await listWebsitePages(root) : [];
		} catch {
			pages = [];
		}
	}

	function openForm(site?: WebsiteSiteConfig) {
		form = site ? { ...site } : { ...emptyWebsiteSite(), port: nextFreePort() };
		formError = "";
		message = "";
		void loadPages();
	}

	function nextFreePort() {
		const used = new Set(sites.map((site) => site.config.port));
		let port = 8080;
		while (used.has(port)) port += 1;
		return port;
	}

	async function browse() {
		if (!form) return;
		try {
			const selected = await chooseFolder(form.root);
			if (selected && form) {
				form.root = selected;
				void loadPages();
				if (!form.name.trim()) form.name = selected.split(/[\\/]/).filter(Boolean).pop() ?? "";
			}
		} catch (caught) {
			formError = describe(caught);
		}
	}

	async function submit() {
		if (!form) return;
		const problem = validateWebsiteSite(form);
		if (problem) {
			formError = problem;
			return;
		}
		formBusy = true;
		formError = "";
		try {
			const wasRunning = editingRunning;
			sites = await saveWebsiteSite({ ...form, name: form.name.trim(), root: form.root.trim() });
			message = wasRunning ? "Website saved and restarted." : editingExisting ? "Website saved." : "Website added. Press Start to begin serving it.";
			form = null;
		} catch (caught) {
			formError = describe(caught);
		} finally {
			formBusy = false;
		}
	}

	async function toggleRequests(site: WebsiteSiteStatus) {
		if (requestsOpenId === site.config.id) {
			requestsOpenId = "";
			requests = [];
			return;
		}
		requestsOpenId = site.config.id;
		requests = [];
		try {
			requests = await getWebsiteRequests(site.config.id);
		} catch (caught) {
			error = describe(caught);
		}
	}

	async function copy(key: string, value: string) {
		try {
			await navigator.clipboard.writeText(value);
			copied = key;
			window.clearTimeout(copiedTimer);
			copiedTimer = window.setTimeout(() => (copied = ""), 1500);
		} catch (caught) {
			error = `Copy failed: ${describe(caught)}`;
		}
	}

	async function open(url: string) {
		try {
			await openExternalUrl(url);
		} catch (caught) {
			error = describe(caught);
		}
	}

	function statusClass(code: number) {
		if (code >= 500) return "text-red-300";
		if (code >= 400) return "text-amber-300";
		if (code >= 300) return "text-sky-300";
		return "text-emerald-300";
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Website Hosting</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Website Hosting</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
				Serve a folder of website files on any port, right next to your FXServer. Each website gets its own port and can be started and stopped independently.
			</p>
		</div>
		<div class="flex flex-wrap gap-2">
			<Button variant="outline" onclick={() => refresh()} title="Reload website status">
				<RefreshCwIcon />
				Refresh
			</Button>
			<Button onclick={() => openForm()} disabled={form !== null} title="Add a website">
				<PlusIcon />
				Add Website
			</Button>
		</div>
	</div>

	{#if error}
		<Notice tone="error" message={error} onDismiss={() => (error = "")} class="px-4 py-3 text-sm" />
	{/if}
	{#if message}
		<Notice tone="success" {message} onDismiss={() => (message = "")} class="px-4 py-3 text-sm" />
	{/if}

	<Notice
		tone="info"
		title="Static, PHP, or Node"
		message="Static serves files exactly as they are (HTML, CSS, JavaScript, images, fonts, video). PHP and Node hand every request straight to your own command (php or node) and pass the response straight back, so nothing here caches it. Sites only stay online while FXServer Installer is open, and it keeps running in the tray when you close the window."
		class="px-4 py-3 text-sm"
	/>

	{#if form}
		<Card.Root class="rounded-sm border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>{editingExisting ? "Edit Website" : "Add Website"}</Card.Title>
				<Card.Description>
					{editingRunning ? "This website is running. Saving restarts it so the changes take effect." : "Pick the folder with your website files, its main page, and the port to serve it on."}
				</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4 pt-4">
				<div class="grid gap-4 sm:grid-cols-[1fr_10rem]">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Name</span>
						<Input bind:value={form.name} disabled={formBusy} placeholder="My server website" maxlength={60} />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Port</span>
						<Input type="number" bind:value={form.port} disabled={formBusy} min={1} max={65535} placeholder="8080" />
					</label>
				</div>

				<div class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Website folder</span>
					<div class="flex gap-2">
						<Input bind:value={form.root} disabled={formBusy} onchange={loadPages} placeholder="C:\websites\my-site" class="min-w-0 flex-1" />
						<Button variant="outline" onclick={browse} disabled={formBusy} title="Choose the website folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</div>

				{#if form.runtime === "static"}
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Main page</span>
						<Input bind:value={form.indexFile} disabled={formBusy} list="website-pages" placeholder="index.html" />
						<datalist id="website-pages">
							{#each pages as page}
								<option value={page}></option>
							{/each}
						</datalist>
						<span class="text-xs leading-5 text-muted-foreground">The .html file shown at the site address. Pick one from the folder or type a name; leave empty for index.html. Every other .html file is still reachable by its own address.</span>
					</label>
				{/if}

				<div class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Who can open it</span>
					<div class="flex flex-wrap gap-2">
						<Button variant={form.exposure === "local" ? "default" : "outline"} onclick={() => form && (form.exposure = "local")} disabled={formBusy}>This computer only</Button>
						<Button variant={form.exposure === "network" ? "default" : "outline"} onclick={() => form && (form.exposure = "network")} disabled={formBusy}>Other devices / internet</Button>
					</div>
					<p class="text-xs leading-5 text-muted-foreground">
						{#if form.exposure === "local"}
							Only this PC can open the website (http://127.0.0.1:{form.port || "port"}/). Good for testing.
						{:else}
							Anyone who can reach this PC on port {form.port || "port"} can open the website. You will usually also need to allow the port in Windows Firewall, and forward it on your router to reach it from the internet.
						{/if}
					</p>
				</div>

				<div class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">What handles requests</span>
					<div class="flex flex-wrap gap-2">
						<Button variant={form.runtime === "static" ? "default" : "outline"} onclick={() => form && (form.runtime = "static")} disabled={formBusy}>Static files</Button>
						<Button variant={form.runtime === "php" ? "default" : "outline"} onclick={() => form && (form.runtime = "php")} disabled={formBusy}>PHP</Button>
						<Button variant={form.runtime === "node" ? "default" : "outline"} onclick={() => form && (form.runtime = "node")} disabled={formBusy}>Node</Button>
					</div>
					{#if form.runtime !== "static"}
						{@const runtime = form.runtime as WebsiteRuntime}
						<div class="grid gap-4 pt-2 sm:grid-cols-[1fr_10rem]">
							<label class="grid gap-2">
								<span class="text-xs font-medium text-muted-foreground">Backend command</span>
								<Input bind:value={form.backendCommand} disabled={formBusy} placeholder={backendCommandExample(runtime)} class="font-mono" />
							</label>
							<label class="grid gap-2">
								<span class="text-xs font-medium text-muted-foreground">Backend port</span>
								<Input type="number" bind:value={form.backendPort} disabled={formBusy} min={1} max={65535} placeholder="8901" />
							</label>
						</div>
						<p class="text-xs leading-5 text-muted-foreground">
							Run from the website folder above. It has to listen on 127.0.0.1 at the backend port — for example <code class="rounded-xs bg-muted px-1 py-0.5">{backendCommandExample(runtime)}</code>. Every request on port {form.port || "the website port"} above is handed straight to it and the response is sent back untouched, so caching is entirely up to the app itself.
						</p>
					{/if}
				</div>

				<div class="grid gap-3">
					{#if form.runtime === "static"}
						<label class="flex items-start gap-3 text-sm">
							<Checkbox bind:checked={form.spaFallback} disabled={formBusy} class="mt-0.5" />
							<span>
								<span class="font-medium text-foreground">Single-page app mode</span>
								<span class="block text-xs leading-5 text-muted-foreground">Unknown page addresses show the main page, so client-side routers (React, Vue, Svelte) keep working on refresh.</span>
							</span>
						</label>
					{/if}
					<label class="flex items-start gap-3 text-sm">
						<Checkbox bind:checked={form.autostart} disabled={formBusy} class="mt-0.5" />
						<span>
							<span class="font-medium text-foreground">Start with FXServer Installer</span>
							<span class="block text-xs leading-5 text-muted-foreground">Bring this website online automatically whenever the app opens.</span>
						</span>
					</label>
				</div>

				{#if formError}
					<Notice tone="error" message={formError} onDismiss={() => (formError = "")} class="px-3 py-2 text-xs" />
				{/if}
			</Card.Content>
			<Card.Footer class="flex flex-wrap justify-end gap-2 border-t border-border pt-4">
				<Button variant="outline" onclick={() => (form = null)} disabled={formBusy}>Cancel</Button>
				<Button onclick={submit} disabled={formBusy}>
					{#if formBusy}<LoaderCircleIcon class="animate-spin" />{/if}
					{editingExisting ? "Save Changes" : "Add Website"}
				</Button>
			</Card.Footer>
		</Card.Root>
	{/if}

	{#if !loaded}
		<p role="status" class="text-sm text-muted-foreground">Loading websites...</p>
	{:else if sites.length === 0 && !form}
		<Card.Root class="rounded-sm border-dashed border-border bg-card/60 shadow-none">
			<Card.Content class="flex flex-col items-center gap-3 py-12 text-center">
				<GlobeIcon class="size-8 text-muted-foreground" />
				<div>
					<p class="text-sm font-medium text-foreground">No websites yet</p>
					<p class="mt-1 max-w-md text-xs leading-5 text-muted-foreground">Add a folder containing your .html files and choose a port. It is served as soon as you press Start.</p>
				</div>
				<Button onclick={() => openForm()}>
					<PlusIcon />
					Add Website
				</Button>
			</Card.Content>
		</Card.Root>
	{/if}

	<div class="grid gap-4">
		{#each sites as site (site.config.id)}
			{@const busy = busyId === site.config.id}
			<Card.Root class="rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex flex-wrap items-start justify-between gap-3">
						<div class="min-w-0">
							<div class="flex flex-wrap items-center gap-2">
								<Card.Title class="truncate">{site.config.name}</Card.Title>
								<span
									class={[
										"rounded-sm border px-2 py-0.5 text-[11px] font-medium",
										site.running ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-200" : "border-border bg-background/70 text-muted-foreground",
									]}
								>
									{site.running ? "Running" : "Stopped"}
								</span>
							</div>
							<Card.Description class="mt-1 truncate" title={site.config.root}>{site.config.root}</Card.Description>
						</div>
						<div class="flex flex-wrap gap-2">
							{#if site.running}
								<Button variant="outline" onclick={() => stop(site)} disabled={busy} title="Stop serving this website">
									{#if busy}<LoaderCircleIcon class="animate-spin" />{:else}<SquareIcon />{/if}
									Stop
								</Button>
							{:else}
								<Button onclick={() => start(site)} disabled={busy} title="Start serving this website">
									{#if busy}<LoaderCircleIcon class="animate-spin" />{:else}<PlayIcon />{/if}
									Start
								</Button>
							{/if}
							<Button variant="outline" onclick={() => open(site.localUrl)} disabled={!site.running} title="Open in your browser">
								<ExternalLinkIcon />
								Open
							</Button>
							<Button variant="outline" size="icon" onclick={() => openForm(site.config)} disabled={busy || form !== null} title="Edit website" aria-label="Edit website">
								<PencilIcon />
							</Button>
							{#if confirmRemoveId === site.config.id}
								<Button variant="destructive" onclick={() => remove(site)} disabled={busy}>Remove for good?</Button>
								<Button variant="outline" onclick={() => (confirmRemoveId = "")}>Keep</Button>
							{:else}
								<Button variant="outline" size="icon" onclick={() => (confirmRemoveId = site.config.id)} disabled={busy} title="Remove website (files are not deleted)" aria-label="Remove website">
									<Trash2Icon />
								</Button>
							{/if}
						</div>
					</div>
				</Card.Header>

				<Card.Content class="space-y-4 pt-4">
					{#if site.error}
						<Notice tone="error" title="Could not start" message={site.error} class="px-3 py-2 text-xs" />
					{/if}
					{#if !site.hasIndex}
						<Notice tone="warn" message={site.config.indexFile ? `The main page ${site.config.indexFile} was not found in this folder, so the home page will show a notice until you add it.` : "No index.html was found in this folder yet, so the home page will show a notice until you add one."} class="px-3 py-2 text-xs" />
					{/if}

					<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
						<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
							<p class="text-xs text-muted-foreground">Port</p>
							<p class="mt-1 text-lg font-semibold">{site.config.port}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
							<p class="text-xs text-muted-foreground">Access</p>
							<p class="mt-1 text-sm font-medium">{site.config.exposure === "network" ? "Other devices / internet" : "This computer only"}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
							<p class="text-xs text-muted-foreground">Requests</p>
							<p class="mt-1 text-lg font-semibold">{site.requests.toLocaleString()}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
							<p class="text-xs text-muted-foreground">Data sent</p>
							<p class="mt-1 text-lg font-semibold">{formatBytes(site.bytesSent)}</p>
						</div>
					</div>

					<div class="flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted-foreground">
						{#if site.running}
							<span>Running since {timeFormatter.format(new Date(site.startedAt ?? Date.now()))}</span>
						{/if}
						{#if site.config.spaFallback}<span>Single-page app mode</span>{/if}
						{#if site.config.autostart}<span>Starts with the app</span>{/if}
					</div>

					<div class="grid gap-2">
						{#each [{ key: `${site.config.id}-local`, label: "On this computer", url: site.localUrl }, ...(site.networkUrl ? [{ key: `${site.config.id}-net`, label: "On your network", url: site.networkUrl }] : [])] as entry (entry.key)}
							<div class="flex flex-wrap items-center gap-2 text-sm">
								<span class="w-32 shrink-0 text-xs text-muted-foreground">{entry.label}</span>
								<code class="min-w-0 truncate rounded-sm border border-border bg-background/70 px-2 py-1 text-xs">{entry.url}</code>
								<Button variant="ghost" size="icon-xs" onclick={() => copy(entry.key, entry.url)} title="Copy address" aria-label="Copy address">
									<CopyIcon />
								</Button>
								{#if copied === entry.key}<span class="text-xs text-emerald-300">Copied</span>{/if}
							</div>
						{/each}
					</div>

					{#if site.config.exposure === "network"}
						<div class="grid gap-2 rounded-sm border border-border bg-background/50 px-3 py-2">
							<p class="text-xs leading-5 text-muted-foreground">
								To let other devices in, allow this port through Windows Firewall. Run this in an administrator Command Prompt:
							</p>
							<div class="flex flex-wrap items-center gap-2">
								<code class="min-w-0 flex-1 break-all rounded-sm border border-border bg-background/70 px-2 py-1 text-[11px]">{firewallCommand(site.config.port)}</code>
								<Button variant="ghost" size="icon-xs" onclick={() => copy(`${site.config.id}-fw`, firewallCommand(site.config.port))} title="Copy command" aria-label="Copy firewall command">
									<CopyIcon />
								</Button>
								{#if copied === `${site.config.id}-fw`}<span class="text-xs text-emerald-300">Copied</span>{/if}
							</div>
						</div>
					{/if}

					<div>
						<Button variant="ghost" size="sm" onclick={() => toggleRequests(site)} disabled={!site.running && requestsOpenId !== site.config.id}>
							<ListIcon />
							{requestsOpenId === site.config.id ? "Hide recent requests" : "Recent requests"}
						</Button>
						{#if requestsOpenId === site.config.id}
							<div class="mt-2 max-h-72 overflow-auto rounded-sm border border-border bg-background/70">
								{#if requests.length === 0}
									<p class="px-3 py-3 text-xs text-muted-foreground">No requests yet. Open the website in a browser to see them appear here.</p>
								{:else}
									<table class="w-full text-left text-xs">
										<thead class="sticky top-0 bg-card text-muted-foreground">
											<tr>
												<th class="px-3 py-2 font-medium">Time</th>
												<th class="px-3 py-2 font-medium">Client</th>
												<th class="px-3 py-2 font-medium">Request</th>
												<th class="px-3 py-2 font-medium">Status</th>
												<th class="px-3 py-2 text-right font-medium">Size</th>
											</tr>
										</thead>
										<tbody>
											{#each requests as entry, index (`${entry.time}-${index}`)}
												<tr class="border-t border-border">
													<td class="px-3 py-1.5 whitespace-nowrap text-muted-foreground">{timeFormatter.format(new Date(entry.time))}</td>
													<td class="px-3 py-1.5 whitespace-nowrap text-muted-foreground">{entry.client}</td>
													<td class="max-w-[24rem] truncate px-3 py-1.5 font-mono" title={`${entry.method} ${entry.path}`}>{entry.method} {entry.path}</td>
													<td class={`px-3 py-1.5 font-medium ${statusClass(entry.status)}`}>{entry.status}</td>
													<td class="px-3 py-1.5 text-right whitespace-nowrap text-muted-foreground">{formatBytes(entry.bytes)}</td>
												</tr>
											{/each}
										</tbody>
									</table>
								{/if}
							</div>
						{/if}
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	<Card.Root class="rounded-sm border-border bg-card/60 shadow-none">
		<Card.Content class="space-y-1 py-4 text-xs leading-5 text-muted-foreground">
			<p class="font-medium text-foreground">What is (and is not) served</p>
			<p>Hidden files and folders (such as .git or .env) and anything outside the website folder are never served. Scripts and settings files (.php, .py, .sh, .bat, .cfg, .ini, .env, .sql, .log, .bak and similar) are refused so their contents cannot leak.</p>
			<p>Do not point a website at your FXServer or txData folder. Choose a dedicated folder for the site.</p>
		</Card.Content>
	</Card.Root>
</section>
