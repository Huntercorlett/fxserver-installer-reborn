import { invoke } from "@tauri-apps/api/core";

export type WebsiteExposure = "local" | "network";

/** "static" serves files as-is. "php"/"node" proxy every request to a backend command. */
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
	/** Command that starts the backend, run from the website folder. Ignored for "static". */
	backendCommand: string;
	/** Port the backend command listens on internally. Ignored for "static". */
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

const backendExamples: Record<Exclude<WebsiteRuntime, "static">, string> = {
	php: "php -S 127.0.0.1:8901 -t .",
	node: "node server.js",
};

export function backendCommandExample(runtime: WebsiteRuntime) {
	return runtime === "static" ? "" : backendExamples[runtime];
}

/** Returns a user-facing problem with the form values, or an empty string. */
export function validateWebsiteSite(site: WebsiteSiteConfig): string {
	if (!site.name.trim()) return "Enter a name for the website.";
	if (!site.root.trim()) return "Choose the folder that contains the website files.";
	if (!Number.isInteger(site.port) || site.port < 1 || site.port > 65535) return "Enter a port between 1 and 65535.";
	if (site.runtime !== "static") {
		if (!site.backendCommand.trim()) return "Enter the command that starts the PHP or Node app.";
		if (!Number.isInteger(site.backendPort) || site.backendPort < 1 || site.backendPort > 65535) return "Enter the port the PHP or Node app listens on.";
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
