import { log } from "$lib/core/logger.svelte";
import { listTxDataProfiles, type TxDataProfilesResult } from "$lib/modules/fxserver";
import { publicEnvironment } from "$lib/core/workspaceSettings";
import { getInstallPath } from "$lib/core/paths.svelte";

const envStorageKey = "fxserver.manage.env";
const profileStorageKey = "fxserver.manage.serverProfile";
const legacyLogProfileStorageKey = "fxserver.manage.logProfile";
const txDataModeKey = "fxserver.manage.txDataMode.v2";

export const fxserverSettings = $state({
	txDataPath: "",
	txDataAuto: true,
	profile: "",
	profiles: [] as string[],
	hasRootLogs: false,
	loadingProfiles: false,
	profileError: "",
});

let loaded = false;
let profileRequest = 0;
let syncTimer: ReturnType<typeof setTimeout> | undefined;

export function loadFxserverSettings() {
	if (loaded) return;
	loaded = true;
	window.addEventListener("workspace-settings-changed", syncTxDataToArtifact);

	try {
		const savedEnv = readSavedEnvironment();
		localStorage.setItem(envStorageKey, JSON.stringify(savedEnv));
		fxserverSettings.txDataPath = typeof savedEnv.TXHOST_DATA_PATH === "string" ? savedEnv.TXHOST_DATA_PATH : "";
		fxserverSettings.profile = localStorage.getItem(profileStorageKey) ?? localStorage.getItem(legacyLogProfileStorageKey) ?? "";
	} catch {
		fxserverSettings.txDataPath = "";
		fxserverSettings.profile = "";
	}
	syncTxDataToArtifact();
}

/** txAdmin's own default: a `txData` folder next to (one level above) the artifact folder. */
export function defaultTxDataPath(artifactPath: string) {
	const trimmed = trimTrailingSeparators(artifactPath);
	const index = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
	if (index <= 0) return "";
	const parent = trimmed.slice(0, index).replace(/[\\/]+$/, "");
	if (!parent) return "";
	const separator = trimmed.includes("/") && !trimmed.includes("\\") ? "/" : "\\";
	return `${parent}${separator}txData`;
}

function normalized(path: string) {
	return trimTrailingSeparators(path).toLowerCase().replaceAll("/", "\\");
}

/** The folder that contains the artifact folder. It is never a valid txData location on its own, so it is not treated as a custom choice. */
function isArtifactParent(path: string, artifact: string) {
	const trimmed = trimTrailingSeparators(artifact);
	const index = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));

	return index > 0 && normalized(path) === normalized(trimmed.slice(0, index));
}

function isInside(path: string, folder: string) {
	const child = trimTrailingSeparators(path).toLowerCase().replaceAll("/", "\\");
	const parent = trimTrailingSeparators(folder).toLowerCase().replaceAll("/", "\\");
	return Boolean(parent) && (child === parent || child.startsWith(`${parent}\\`));
}

/**
 * Keeps the txData path one folder above the artifact folder until the user picks their own folder.
 * A saved folder that is the artifact folder itself (or inside it) is never a valid txData location.
 */
export function syncTxDataToArtifact() {
	if (!loaded) return;
	const artifact = getInstallPath();
	const next = defaultTxDataPath(artifact);
	const mode = localStorage.getItem(txDataModeKey);
	const current = fxserverSettings.txDataPath.trim();
	if (mode === "manual" || !next) {
		fxserverSettings.txDataAuto = mode !== "manual";
		return;
	}
	if (mode === null && current && !isInside(current, artifact) && !isArtifactParent(current, artifact)) {
		localStorage.setItem(txDataModeKey, "manual");
		fxserverSettings.txDataAuto = false;
		return;
	}
	localStorage.setItem(txDataModeKey, "auto");
	fxserverSettings.txDataAuto = true;
	if (current === next) return;
	setTxDataPath(next, "auto");
	clearTimeout(syncTimer);
	syncTimer = setTimeout(() => void refreshTxDataProfiles(), 400);
}

export function useDefaultTxData() {
	localStorage.setItem(txDataModeKey, "auto");
	fxserverSettings.txDataAuto = true;
	syncTxDataToArtifact();
}

export function readSavedEnvironment() {
	try {
		const value = JSON.parse(localStorage.getItem(envStorageKey) || "{}");
		if (!value || typeof value !== "object" || Array.isArray(value)) return {};
		return publicEnvironment(value);
	} catch {
		return {};
	}
}

export function writeSavedEnvironment(values: Record<string, string>) {
	localStorage.setItem(envStorageKey, JSON.stringify(publicEnvironment(values)));
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export function setTxDataPath(path: string, source: "user" | "auto" = "user") {
	loadFxserverSettings();
	if (source === "user") {
		const isDefault = path.trim() === defaultTxDataPath(getInstallPath());
		localStorage.setItem(txDataModeKey, isDefault ? "auto" : "manual");
		fxserverSettings.txDataAuto = isDefault;
	}
	if (fxserverSettings.txDataPath !== path.trim()) resetTxDataProfiles();
	fxserverSettings.txDataPath = path.trim();
	const savedEnvironment = readSavedEnvironment();
	if (fxserverSettings.txDataPath) {
		savedEnvironment.TXHOST_DATA_PATH = fxserverSettings.txDataPath;
	} else {
		delete savedEnvironment.TXHOST_DATA_PATH;
	}
	writeSavedEnvironment(savedEnvironment);
}

export function resetTxDataProfiles() {
	profileRequest += 1;
	fxserverSettings.profiles = [];
	fxserverSettings.hasRootLogs = false;
	fxserverSettings.profileError = "";
	fxserverSettings.loadingProfiles = false;
}

export function setServerProfile(profile: string) {
	loadFxserverSettings();
	fxserverSettings.profile = profile.trim();
	localStorage.setItem(profileStorageKey, fxserverSettings.profile);
	localStorage.removeItem(legacyLogProfileStorageKey);
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

function trimTrailingSeparators(path: string) {
	return path.trim().replace(/[\\/]+$/, "");
}

/**
 * txData must be the folder that CONTAINS the profile folders (each with a config.json). Users often
 * pick the profile folder itself (it holds config.json and no sub-profiles) or the server folder that
 * contains txData, so detect both and return the corrected location.
 */
async function detectTxDataLayout(path: string, found: TxDataProfilesResult) {
	const trimmed = trimTrailingSeparators(path);

	if (!found.profiles.length && found.hasRootConfig) {
		const index = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
		const name = trimmed.slice(index + 1);
		let parent = trimmed.slice(0, index);
		if (index > 0 && name) {
			if (/^[A-Za-z]:$/.test(parent)) parent += "\\";
			const up = await listTxDataProfiles(parent);
			const match = up.profiles.find((profileName) => profileName.toLowerCase() === name.toLowerCase());
			if (match) return { path: parent, profile: match, result: up };
		}
	}

	const nested = found.profiles.find((profileName) => profileName.toLowerCase() === "txdata");
	if (nested) {
		const inner = `${trimmed}\\${nested}`;
		const down = await listTxDataProfiles(inner);
		if (down.profiles.length) return { path: inner, profile: "", result: down };
	}

	return null;
}

export async function refreshTxDataProfiles() {
	loadFxserverSettings();
	const request = ++profileRequest;
	const path = fxserverSettings.txDataPath.trim();
	fxserverSettings.profileError = "";
	fxserverSettings.profiles = [];
	fxserverSettings.hasRootLogs = false;

	if (!path) { fxserverSettings.loadingProfiles = false; return; }

	fxserverSettings.loadingProfiles = true;
	try {
		let result = await listTxDataProfiles(path);
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;

		// The automatic txData folder (one level above the artifact folder) is never moved by layout detection.
		const adjusted = fxserverSettings.txDataAuto ? null : await detectTxDataLayout(path, result).catch(() => null);
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;

		let suggestedProfile = "";
		if (adjusted) {
			result = adjusted.result;
			suggestedProfile = adjusted.profile;
			setTxDataPath(adjusted.path);
			log("Adjusted the txData folder to the folder that contains the profiles.", { level: "info", scope: "fxserver.settings", detail: `${path} -> ${adjusted.path}` });
		}

		fxserverSettings.profiles = result.profiles;
		fxserverSettings.hasRootLogs = result.hasRootLogs;
		if (suggestedProfile) {
			setServerProfile(suggestedProfile);
		} else if (fxserverSettings.profile && !result.profiles.includes(fxserverSettings.profile)) {
			setServerProfile("");
		}
		fxserverSettings.loadingProfiles = false;
	} catch (error) {
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;
		fxserverSettings.profileError = error instanceof Error ? error.message : String(error);
		log("Could not refresh txData profiles.", {
			level: "error",
			scope: "fxserver.settings",
			detail: fxserverSettings.profileError,
		});
	} finally {
		if (request === profileRequest) fxserverSettings.loadingProfiles = false;
	}
}
