import { invoke } from "@tauri-apps/api/core";
import type { MariaDBCredentials } from "$lib/modules/mariadb";

export const databaseSession = $state<{
	credentials: MariaDBCredentials | null;
	connectionString: string;
	revision: number;
	/** Workspace the current login belongs to (set by the workspace store). */
	workspaceId: string;
	/** True when the login is saved (encrypted) on this PC for the workspace. */
	remember: boolean;
	defaults: { host: string; port: number; username: string; database: string };
}>({
	credentials: null,
	connectionString: "",
	revision: 0,
	workspaceId: "default",
	remember: false,
	defaults: { host: "localhost", port: 3306, username: "root", database: "" },
});

export function formatMariaDBConnectionString(credentials: MariaDBCredentials) {
	const username = encodeURIComponent(credentials.username.trim());
	const password = encodeURIComponent(credentials.password);
	const host = credentials.host.trim() || "localhost";
	const port = Number(credentials.port) || 3306;
	const database = credentials.database?.trim();
	return `mysql://${username}${password ? `:${password}` : ""}@${host}:${port}${database ? `/${encodeURIComponent(database)}` : ""}`;
}

export function rememberDatabaseCredentials(credentials: MariaDBCredentials, revision = databaseSession.revision) {
	if (revision !== databaseSession.revision) return false;
	databaseSession.credentials = { ...credentials };
	databaseSession.connectionString = formatMariaDBConnectionString(credentials);
	databaseSession.defaults = { host: credentials.host, port: credentials.port, username: credentials.username, database: credentials.database ?? "" };
	window.dispatchEvent(new Event("workspace-settings-changed"));
	if (databaseSession.remember) void saveLogin(databaseSession.workspaceId, credentials);
	return true;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function saveLogin(workspaceId: string, credentials: MariaDBCredentials) {
	if (!hasTauriRuntime()) return;
	try {
		await invoke<void>("save_mariadb_login", { workspaceId, credentials });
	} catch {
		databaseSession.remember = false;
	}
}

/** Turns "remember on this PC" on or off for the active workspace. */
export async function setRememberLogin(remember: boolean) {
	databaseSession.remember = remember;
	if (!hasTauriRuntime()) return;
	try {
		if (remember) {
			if (databaseSession.credentials) await saveLogin(databaseSession.workspaceId, databaseSession.credentials);
		} else {
			await invoke<void>("clear_mariadb_login", { workspaceId: databaseSession.workspaceId });
		}
	} catch {
		databaseSession.remember = false;
	}
}

/**
 * Loads the saved login for a workspace into the shared session (unvalidated;
 * pages validate it when opened). Ignored if the workspace changed meanwhile
 * (an already entered login is kept; only the "remembered" flag is set).
 */
export async function restoreSavedLogin(workspaceId: string, revision: number) {
	if (!hasTauriRuntime()) return;
	try {
		const saved = await invoke<MariaDBCredentials | null>("load_mariadb_login", { workspaceId });
		if (!saved || revision !== databaseSession.revision) return;
		databaseSession.remember = true;
		if (databaseSession.credentials) return;
		databaseSession.credentials = { ...saved };
		databaseSession.connectionString = formatMariaDBConnectionString(saved);
		databaseSession.defaults = { host: saved.host, port: saved.port, username: saved.username, database: saved.database ?? "" };
		databaseSession.remember = true;
	} catch {
		// Nothing usable saved; the connection card simply asks.
	}
}
