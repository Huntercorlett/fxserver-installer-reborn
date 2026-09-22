import ActivityIcon from "@lucide/svelte/icons/activity";
import CpuIcon from "@lucide/svelte/icons/cpu";
import DatabaseIcon from "@lucide/svelte/icons/database";
import DownloadIcon from "@lucide/svelte/icons/download";
import SlidersHorizontalIcon from "@lucide/svelte/icons/sliders-horizontal";
import TerminalIcon from "@lucide/svelte/icons/terminal";
import TypeIcon from "@lucide/svelte/icons/type";
import UploadIcon from "@lucide/svelte/icons/upload";
import UsersIcon from "@lucide/svelte/icons/users";
import type { Component } from "svelte";

export type ServerTab = "databases" | "sql" | "status" | "users" | "export" | "import" | "variables" | "charsets" | "engines";

export const serverTabs: { id: ServerTab; label: string; icon: Component<any> }[] = [
	{ id: "databases", label: "Databases", icon: DatabaseIcon },
	{ id: "sql", label: "SQL", icon: TerminalIcon },
	{ id: "status", label: "Status", icon: ActivityIcon },
	{ id: "users", label: "User accounts", icon: UsersIcon },
	{ id: "export", label: "Export", icon: DownloadIcon },
	{ id: "import", label: "Import", icon: UploadIcon },
	{ id: "variables", label: "Variables", icon: SlidersHorizontalIcon },
	{ id: "charsets", label: "Charsets", icon: TypeIcon },
	{ id: "engines", label: "Engines", icon: CpuIcon },
];
