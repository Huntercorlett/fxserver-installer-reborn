import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type WebsiteExposure = "local" | "network";

/**
 * "static" serves files exactly as they are. "php" runs .php files through
 * php-cgi in the same folder as everything else, like XAMPP/Apache — no
 * separate process to start, static assets in the same folder still work.
 * "node" reverse-proxies the whole site to a Node process you start yourself,
 * since a Node app is its own server rather than a per-file script.
 */
export type WebsiteRuntime = "static" | "php" | "node";

export interface WebsiteSiteConfig {
	/** Empty for a site that has not been saved yet; the backend assigns one. */
	id: string;
	name: string;
	root: string;
	/** Main page relative to the folder (e.g. "home.html"). Empty means index.html. */
	indexFile: string;
	port: number;
	exposure: WebsiteExposure;
	spaFallback: boolean;
	autostart: boolean;
	runtime: WebsiteRuntime;
	/**
	 * runtime "php": full path to php-cgi.exe.
	 * runtime "node": the command that starts the Node app, run from the website folder.
	 * runtime "static": unused.
	 */
	backendCommand: string;
	/** runtime "node" only: the port the Node app listens on internally. */
	backendPort: number;
}

export interface WebsiteSiteStatus {
	config: WebsiteSiteConfig;
	running: boolean;
	startedAt: number | null;
	requests: number;
	bytesSent: number;
	localUrl: string;
	networkUrl: string | null;
	hasIndex: boolean;
	error: string | null;
}

export interface WebsiteRequestEntry {
	time: number;
	client: string;
	method: string;
	path: string;
	status: number;
	bytes: number;
}

export function emptyWebsiteSite(): WebsiteSiteConfig {
	return { id: "", name: "", root: "", indexFile: "", port: 8080, exposure: "local", spaFallback: false, autostart: false, runtime: "static", backendCommand: "", backendPort: 0 };
}

export const nodeCommandExample = "node server.js";

/** Returns a user-facing problem with the form values, or an empty string. */
export function validateWebsiteSite(site: WebsiteSiteConfig): string {
	if (!site.name.trim()) return "Enter a name for the website.";
	if (!site.root.trim()) return "Choose the folder that contains the website files.";
	if (!Number.isInteger(site.port) || site.port < 1 || site.port > 65535) return "Enter a port between 1 and 65535.";
	if (site.runtime === "php") {
		// Empty is fine here — the backend auto-locates php-cgi.exe (XAMPP-style)
		// when this is left blank, and reports it clearly if none is found.
	} else if (site.runtime === "node") {
		if (!site.backendCommand.trim()) return "Enter the command that starts the Node app.";
		if (!Number.isInteger(site.backendPort) || site.backendPort < 1 || site.backendPort > 65535) return "Enter the port the Node app listens on.";
		if (site.backendPort === site.port) return "The backend port must be different from the website port above it.";
	}
	return "";
}

export function firewallCommand(port: number) {
	return `netsh advfirewall firewall add rule name="FXServer Installer website ${port}" dir=in action=allow protocol=TCP localport=${port}`;
}

export function formatBytes(bytes: number) {
	if (bytes < 1024) return `${bytes} B`;
	const units = ["KB", "MB", "GB", "TB"];
	let value = bytes / 1024;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

export const getWebsiteSites = () => invoke<WebsiteSiteStatus[]>("get_website_sites");
export const saveWebsiteSite = (site: WebsiteSiteConfig) => invoke<WebsiteSiteStatus[]>("save_website_site", { site });
export const removeWebsiteSite = (id: string) => invoke<WebsiteSiteStatus[]>("remove_website_site", { id });
export const startWebsiteSite = (id: string) => invoke<WebsiteSiteStatus[]>("start_website_site", { id });
export const stopWebsiteSite = (id: string) => invoke<WebsiteSiteStatus[]>("stop_website_site", { id });
export const getWebsiteRequests = (id: string) => invoke<WebsiteRequestEntry[]>("get_website_requests", { id });
export const listWebsitePages = (root: string) => invoke<string[]>("list_website_pages", { root });

export interface DetectedWebsiteSetup {
	runtime: WebsiteRuntime;
	phpCgiPath: string | null;
	nodeCommand: string | null;
	message: string;
}

/** XAMPP-style folder detection: what to run, without asking the user to pick. */
export const detectWebsiteSetup = (root: string) => invoke<DetectedWebsiteSetup>("detect_website_setup", { root });

export interface PhpStatus {
	bundledPath: string | null;
	systemPath: string | null;
}

export const getPhpStatus = () => invoke<PhpStatus>("get_php_status");

/** Downloads a portable PHP next to the app itself — no XAMPP, no system install, no version picking. */
export async function installBundledPhp(onProgress?: (stage: string) => void): Promise<string> {
	let unlisten: (() => void) | undefined;
	try {
		if (onProgress) {
			unlisten = await listen<string>("php-install-progress", ({ payload }) => onProgress(payload));
		}
		return await invoke<string>("install_bundled_php");
	} finally {
		unlisten?.();
	}
}
