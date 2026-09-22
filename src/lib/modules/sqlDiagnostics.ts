import { executeMariaDBQuery, type MariaDBCredentials, type MariaDBQueryResult } from "$lib/modules/mariadb";

export interface SqlDiagnosis {
	kind: "collation" | "other";
	title: string;
	message: string;
	/** Rewritten SQL the user can review and re-run; null when the fix is not a SQL edit. */
	fixedSql: string | null;
}

interface ForeignKey {
	table: string;
	column: string;
	refTable: string;
	refColumn: string;
}

interface ColumnInfo {
	type: string;
	charset: string;
	collation: string;
	key: string;
}

const literal = (value: string) => `'${value.replaceAll("\\", "\\\\").replaceAll("'", "''")}'`;
const unquote = (value: string) => value.trim().replace(/^`|`$/g, "");

function failedTable(stderr: string) {
	const match = stderr.match(/Can't create table `([^`]+)`\.`([^`]+)`/i);
	if (match && /errno:\s*150|\b1005\b|\b1215\b/i.test(stderr)) return { database: match[1], table: match[2] };
	if (/\b1215\b|Cannot add foreign key constraint/i.test(stderr)) return { database: "", table: "" };
	return null;
}

function tableStatement(sql: string, table: string) {
	const start = new RegExp(`CREATE\\s+TABLE\\s+(?:IF\\s+NOT\\s+EXISTS\\s+)?(?:\`?[\\w$]+\`?\\.)?\`?${table.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\`?\\s*\\(`, "i").exec(sql);
	if (!start) return null;
	const end = sql.indexOf(";", start.index);
	return { from: start.index, to: end < 0 ? sql.length : end, text: sql.slice(start.index, end < 0 ? sql.length : end) };
}

function foreignKeys(statement: string, table: string): ForeignKey[] {
	const found: ForeignKey[] = [];
	const pattern = /FOREIGN\s+KEY\s*(?:`[^`]*`\s*)?\(([^)]+)\)\s*REFERENCES\s+(?:`?[\w$]+`?\.)?`?([\w$]+)`?\s*\(([^)]+)\)/gi;
	for (const match of statement.matchAll(pattern)) {
		const columns = match[1].split(",").map(unquote);
		const refColumns = match[3].split(",").map(unquote);
		columns.forEach((column, index) => found.push({ table, column, refTable: match[2], refColumn: refColumns[index] ?? refColumns[0] }));
	}
	return found;
}

async function columnInfo(credentials: MariaDBCredentials, database: string, table: string, column: string): Promise<ColumnInfo | null> {
	const result = await executeMariaDBQuery({ ...credentials, database: null },
		`SELECT COLUMN_TYPE, IFNULL(CHARACTER_SET_NAME, ''), IFNULL(COLLATION_NAME, ''), COLUMN_KEY FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ${literal(database)} AND TABLE_NAME = ${literal(table)} AND COLUMN_NAME = ${literal(column)};`);
	const row = result.success ? result.rows[0] : undefined;
	return row ? { type: row[0], charset: row[1], collation: row[2], key: row[3] } : null;
}

async function tableEngine(credentials: MariaDBCredentials, database: string, table: string) {
	const result = await executeMariaDBQuery({ ...credentials, database: null },
		`SELECT IFNULL(ENGINE, '') FROM information_schema.TABLES WHERE TABLE_SCHEMA = ${literal(database)} AND TABLE_NAME = ${literal(table)};`);
	return result.success ? (result.rows[0]?.[0] ?? "") : "";
}

async function databaseCollation(credentials: MariaDBCredentials, database: string) {
	const result = await executeMariaDBQuery({ ...credentials, database: null },
		`SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ${literal(database)};`);
	const row = result.success ? result.rows[0] : undefined;
	return row ? { charset: row[0], collation: row[1] } : null;
}

/** Pulls the column definition of `column` out of a CREATE TABLE statement. */
function localColumn(statement: string, column: string) {
	const match = new RegExp(`(^|[\\s(,])\`${column.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\`\\s+([a-z]+(?:\\s*\\([^)]*\\))?)([^,\\n]*)`, "i").exec(statement);
	if (!match) return null;
	const collate = match[3].match(/\bCOLLATE\s+([\w]+)/i);
	return { type: match[2].replace(/\s+/g, "").toLowerCase(), collation: collate?.[1] ?? "" };
}

function alignCollation(sql: string, table: string, column: string, charset: string, collation: string) {
	const statement = tableStatement(sql, table);
	if (!statement) return null;
	const pattern = new RegExp(`(\`${column.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\`\\s+[a-z]+(?:\\s*\\([^)]*\\))?)([^,\\n]*)`, "i");
	if (!pattern.test(statement.text)) return null;
	const rewritten = statement.text.replace(pattern, (_all, head: string, rest: string) => {
		const cleaned = rest.replace(/\s*\bCHARACTER\s+SET\s+\w+/i, "").replace(/\s*\bCHARSET\s+\w+/i, "").replace(/\s*\bCOLLATE\s+\w+/i, "");
		return `${head} CHARACTER SET ${charset} COLLATE ${collation}${cleaned}`;
	});
	return sql.slice(0, statement.from) + rewritten + sql.slice(statement.to);
}

/**
 * Explains a "Foreign key constraint is incorrectly formed" (errno 150 / 1215) failure by
 * comparing the local FK column with the referenced column on the real server.
 */
export async function diagnoseSqlFailure(credentials: MariaDBCredentials, sql: string, stderr: string): Promise<SqlDiagnosis | null> {
	const failed = failedTable(stderr);
	if (!failed) return null;
	const database = failed.database || credentials.database || "";
	if (!database) return null;

	const statement = failed.table ? tableStatement(sql, failed.table) : null;
	const keys = statement ? foreignKeys(statement.text, failed.table) : [];
	if (!keys.length) {
		return { kind: "other", title: "Foreign key problem", message: `MariaDB rejected a foreign key in \`${database}\`${failed.table ? `.\`${failed.table}\`` : ""}, but the failing constraint could not be located in the SQL. Check that every referenced table exists and has matching column types and collations.`, fixedSql: null };
	}

	const serverDefault = await databaseCollation(credentials, database);
	for (const key of keys) {
		const referenced = await columnInfo(credentials, database, key.refTable, key.refColumn);
		if (!referenced) {
			return {
				kind: "other",
				title: `Missing table \`${key.refTable}\``,
				message: `\`${failed.table}\` references \`${key.refTable}\`.\`${key.refColumn}\`, but that column does not exist in \`${database}\`. Run the SQL that creates \`${key.refTable}\` (for QBox: the qbx_core / players schema) into \`${database}\` first, then run this file again.`,
				fixedSql: null,
			};
		}

		const engine = await tableEngine(credentials, database, key.refTable);
		if (engine && engine.toLowerCase() !== "innodb") {
			return {
				kind: "other",
				title: `\`${key.refTable}\` is not InnoDB`,
				message: `\`${key.refTable}\` uses the ${engine} engine, and foreign keys can only point at InnoDB tables. Convert it with: ALTER TABLE \`${database}\`.\`${key.refTable}\` ENGINE=InnoDB;`,
				fixedSql: null,
			};
		}

		const local = localColumn(statement!.text, key.column);
		const localCollation = local?.collation || serverDefault?.collation || "";
		if (referenced.key === "") {
			return {
				kind: "other",
				title: `\`${key.refTable}\`.\`${key.refColumn}\` is not indexed`,
				message: `MariaDB needs an index (PRIMARY/UNIQUE/KEY) on the referenced column. Add one with: ALTER TABLE \`${database}\`.\`${key.refTable}\` ADD INDEX (\`${key.refColumn}\`);`,
				fixedSql: null,
			};
		}
		if (local && referenced.type.replace(/\s+/g, "").toLowerCase() !== local.type && !/^(varchar|char)/.test(local.type)) {
			return {
				kind: "other",
				title: "Column types do not match",
				message: `\`${failed.table}\`.\`${key.column}\` is ${local.type}, but \`${key.refTable}\`.\`${key.refColumn}\` is ${referenced.type}. Both sides of a foreign key must use the same type.`,
				fixedSql: null,
			};
		}
		if (referenced.collation && localCollation && referenced.collation.toLowerCase() !== localCollation.toLowerCase()) {
			return {
				kind: "collation",
				title: "Collation mismatch",
				message: `\`${failed.table}\`.\`${key.column}\` uses ${localCollation}, but \`${key.refTable}\`.\`${key.refColumn}\` uses ${referenced.collation}. MariaDB requires them to match. "Apply fix" rewrites the column in the SQL below to ${referenced.collation}; review it and run again.`,
				fixedSql: alignCollation(sql, failed.table, key.column, referenced.charset, referenced.collation),
			};
		}
	}

	return { kind: "other", title: "Foreign key problem", message: `Referenced tables exist and look compatible. Compare them by running: SHOW CREATE TABLE \`${database}\`.\`${keys[0].refTable}\`; and send the result.`, fixedSql: null };
}

const destructive = /\b(INSERT|UPDATE|DELETE|REPLACE|DROP|TRUNCATE)\b/i;

/**
 * Runs SQL; when a CREATE TABLE fails because a foreign key column's collation differs from the
 * referenced column, rewrites that column to match and retries. Auto-retry only happens for
 * schema-only SQL, because the client stops at the first error and a retry re-runs the whole file.
 */
export async function runSqlWithForeignKeyRepair(credentials: MariaDBCredentials, sql: string) {
	let current = sql;
	let repairs = 0;
	let result: MariaDBQueryResult = await executeMariaDBQuery(credentials, current);
	let diagnosis: SqlDiagnosis | null = null;

	while (!result.success) {
		diagnosis = await diagnoseSqlFailure(credentials, current, result.stderr).catch(() => null);
		if (!diagnosis || diagnosis.kind !== "collation" || !diagnosis.fixedSql || destructive.test(current) || repairs >= 8) break;
		current = diagnosis.fixedSql;
		repairs += 1;
		result = await executeMariaDBQuery(credentials, current);
		diagnosis = null;
	}

	return { result, diagnosis, sql: current, repairs };
}
