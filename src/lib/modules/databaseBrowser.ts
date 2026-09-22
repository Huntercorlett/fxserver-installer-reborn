import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export interface BrowserColumn { name: string; columnType: string; nullable: boolean; defaultValue: string | null; extra: string; binary: boolean }
export interface BrowserIndex { name: string; column: string | null; sequence: number; unique: boolean; indexType: string; prefixLength: number | null }
export interface BrowserMetadata { columns: BrowserColumn[]; indexes: BrowserIndex[]; editable?: boolean; editReason?: string | null }
export type FilterOperator = "eq" | "ne" | "lt" | "lte" | "gt" | "gte" | "contains" | "isNull" | "isNotNull";
export interface BrowserFilter { column: string; operator: FilterOperator; value: string | null }
export interface BrowserRequest { database: string; table: string; filters: BrowserFilter[]; sortColumn: string | null; descending: boolean; offset: number; pageSize: number }
export interface BrowserPage { rows: (string | null)[][]; hasMore: boolean; truncatedCells: boolean; pageSize?: number }
export interface BrowserExport { path: string; rows: number; hasMore: boolean }
export type CellInput = { kind: "null" } | { kind: "text" | "number"; value: string };
export interface ColumnInput { column: string; value: CellInput }
export type ChangeKind = "insert" | "update" | "delete";
export interface BrowserChange { workspaceId: string; database: string; table: string; kind: ChangeKind; values: ColumnInput[]; original: (string | null)[] | null }
export interface ChangePreview { token: string; sql: string; parameters: ColumnInput[]; confirmation: string; expiresAt: number; kind: ChangeKind; host: string; port: number }

export function getBrowserMetadata(credentials: MariaDBCredentials, database: string, table: string) {
	return trackTask("get_database_browser_metadata", "Read table metadata", () => invoke<BrowserMetadata>("get_database_browser_metadata", { credentials, database, table }));
}
export function getBrowserRows(credentials: MariaDBCredentials, request: BrowserRequest) {
	return trackTask("get_database_browser_rows", "Browse database rows", () => invoke<BrowserPage>("get_database_browser_rows", { credentials, request }));
}
export function exportBrowserCsv(credentials: MariaDBCredentials, request: BrowserRequest, outputPath: string) {
	return trackTask("export_database_browser_csv", "Export database CSV", () => invoke<BrowserExport>("export_database_browser_csv", { credentials, request, outputPath }));
}

export function previewBrowserChange(credentials: MariaDBCredentials, change: BrowserChange) {
	return trackTask("preview_database_browser_change", "Preview single-row change", () => invoke<ChangePreview>("preview_database_browser_change", { credentials, change }));
}
export function applyBrowserChange(workspaceId: string, token: string, confirmation: string) {
	return trackTask("apply_database_browser_change", "Apply confirmed single-row change", () => invoke<number>("apply_database_browser_change", { workspaceId, token, confirmation }));
}

export interface TableInfo { name: string; kind: string; engine: string | null; rows: number | null; collation: string | null; dataBytes: number; indexBytes: number; freeBytes: number }
export type TableAction = "empty" | "drop" | "optimize" | "analyze" | "check" | "repair";
export interface TableActionRequest { database: string; action: TableAction; tables: string[]; confirmation?: string | null; disableForeignKeyChecks?: boolean }
export interface ColumnSpec { name: string; dataType: string; length: string; unsigned: boolean; nullable: boolean; defaultKind: "none" | "null" | "value" | "currentTimestamp"; defaultValue: string; autoIncrement: boolean; primary: boolean; unique: boolean }
export interface CreateTableSpec { database: string; name: string; engine: string; collation: string; columns: ColumnSpec[] }

export const columnTypes = ["INT", "BIGINT", "SMALLINT", "TINYINT", "MEDIUMINT", "DECIMAL", "FLOAT", "DOUBLE", "BOOLEAN", "VARCHAR", "CHAR", "TEXT", "MEDIUMTEXT", "LONGTEXT", "BLOB", "LONGBLOB", "JSON", "DATE", "TIME", "DATETIME", "TIMESTAMP", "YEAR"];
export const systemDatabases = ["mysql", "information_schema", "performance_schema", "sys"];

export function emptyColumn(): ColumnSpec {
	return { name: "", dataType: "INT", length: "", unsigned: false, nullable: false, defaultKind: "none", defaultValue: "", autoIncrement: false, primary: false, unique: false };
}

export function formatSize(bytes: number) {
	if (!bytes) return "0 B";
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
	return `${value.toFixed(unit === 0 || value >= 100 ? 0 : 1)} ${units[unit]}`;
}

export function listTableInfo(credentials: MariaDBCredentials, database: string) {
	return trackTask("list_table_info", "List tables", () => invoke<TableInfo[]>("list_table_info", { credentials, database }));
}
export function runTableAction(credentials: MariaDBCredentials, request: TableActionRequest) {
	return trackTask("run_table_action", `Table ${request.action}`, () => invoke<{ message: string }>("run_table_action", { credentials, request }));
}
export function createTable(credentials: MariaDBCredentials, spec: CreateTableSpec) {
	return trackTask("create_table", "Create table", () => invoke<{ message: string }>("create_table", { credentials, spec }));
}
export function createDatabase(credentials: MariaDBCredentials, name: string, collation: string) {
	return trackTask("create_database", "Create database", () => invoke<{ message: string }>("create_database", { credentials, name, collation }));
}
export function dropDatabase(credentials: MariaDBCredentials, name: string, confirmation: string) {
	return trackTask("drop_database", "Drop database", () => invoke<{ message: string }>("drop_database", { credentials, name, confirmation }));
}
