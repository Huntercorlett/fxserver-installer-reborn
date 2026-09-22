//! Table and database administration for the Database Browser (structure list,
//! empty/drop/optimize, create table, create/drop database).

use serde::{Deserialize, Serialize};

use super::database_browser::{query_json, quote_identifier, sql_text};
use crate::{
    models::mariadb::MariaDBCredentials,
    services::mariadb::query::execute_query,
};

const PROTECTED_DATABASES: &[&str] = &["mysql", "information_schema", "performance_schema", "sys"];
const MAX_TABLES_PER_ACTION: usize = 200;
const MAX_COLUMNS: usize = 128;
const ENGINES: &[&str] = &["InnoDB", "Aria", "MyISAM", "MEMORY"];
const TYPES: &[&str] = &[
    "TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "DECIMAL", "FLOAT", "DOUBLE", "BOOLEAN",
    "CHAR", "VARCHAR", "TEXT", "MEDIUMTEXT", "LONGTEXT", "BLOB", "LONGBLOB", "JSON", "DATE",
    "TIME", "DATETIME", "TIMESTAMP", "YEAR",
];
const INTEGER_TYPES: &[&str] = &["TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT"];
const NUMERIC_TYPES: &[&str] = &[
    "TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "DECIMAL", "FLOAT", "DOUBLE",
];
const LENGTH_TYPES: &[&str] = &[
    "TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "DECIMAL", "CHAR", "VARCHAR",
];

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    name: String,
    kind: String,
    engine: Option<String>,
    rows: Option<u64>,
    collation: Option<String>,
    #[serde(default)]
    data_bytes: u64,
    #[serde(default)]
    index_bytes: u64,
    #[serde(default)]
    free_bytes: u64,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TableAction {
    Empty,
    Drop,
    Optimize,
    Analyze,
    Check,
    Repair,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableActionRequest {
    database: String,
    action: TableAction,
    tables: Vec<String>,
    /// Must equal the database name for Empty and Drop.
    confirmation: Option<String>,
    #[serde(default)]
    disable_foreign_key_checks: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    name: String,
    data_type: String,
    length: Option<String>,
    #[serde(default)]
    unsigned: bool,
    #[serde(default)]
    nullable: bool,
    /// "none", "null", "value" or "currentTimestamp".
    default_kind: String,
    default_value: Option<String>,
    #[serde(default)]
    auto_increment: bool,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    unique: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTableSpec {
    database: String,
    name: String,
    engine: Option<String>,
    collation: Option<String>,
    columns: Vec<ColumnSpec>,
}

#[derive(Serialize)]
pub struct AdminResult {
    message: String,
}

fn valid_name(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.chars().count() > 64
        || value.ends_with(' ')
        || value.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '.'))
    {
        return Err("Names must be 1-64 characters without control characters, slashes, dots or trailing spaces.".into());
    }
    quote_identifier(value)
}

fn word(value: &str, what: &str) -> Result<String, String> {
    let ok = (2..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(value.to_string())
    } else {
        Err(format!("Invalid {what}."))
    }
}

/// Returns (charset, collation) for a validated collation such as utf8mb4_unicode_ci.
fn collation_parts(collation: Option<&str>) -> Result<(String, String), String> {
    let collation = word(collation.filter(|c| !c.is_empty()).unwrap_or("utf8mb4_unicode_ci"), "collation")?;
    let charset = collation
        .split('_')
        .next()
        .filter(|part| !part.is_empty())
        .ok_or("Invalid collation.")?
        .to_string();
    Ok((charset, collation))
}

fn require_unprotected(database: &str) -> Result<(), String> {
    if PROTECTED_DATABASES.contains(&database.to_ascii_lowercase().as_str()) {
        return Err(format!("{database} is a system database and cannot be changed here."));
    }
    Ok(())
}

fn run(credentials: &MariaDBCredentials, sql: String) -> Result<crate::models::mariadb::MariaDBQueryResult, String> {
    let mut credentials = credentials.clone();
    credentials.database = None;
    let result = execute_query(credentials, sql)?;
    if result.success {
        Ok(result)
    } else {
        Err(if result.stderr.is_empty() { result.stdout } else { result.stderr })
    }
}

fn table_info_sql(database: &str) -> String {
    format!(
        "SELECT JSON_OBJECT('name',TABLE_NAME,'kind',TABLE_TYPE,'engine',ENGINE,'rows',TABLE_ROWS,'collation',TABLE_COLLATION,'dataBytes',IFNULL(DATA_LENGTH,0),'indexBytes',IFNULL(INDEX_LENGTH,0),'freeBytes',IFNULL(DATA_FREE,0)) FROM information_schema.TABLES WHERE TABLE_SCHEMA={} ORDER BY TABLE_NAME LIMIT 5000;",
        sql_text(database)
    )
}

fn table_action_sql(request: &TableActionRequest) -> Result<String, String> {
    if request.tables.is_empty() || request.tables.len() > MAX_TABLES_PER_ACTION {
        return Err(format!("Select between 1 and {MAX_TABLES_PER_ACTION} tables."));
    }
    let database = quote_identifier(&request.database)?;
    let tables = request
        .tables
        .iter()
        .map(|table| quote_identifier(table).map(|table| format!("{database}.{table}")))
        .collect::<Result<Vec<_>, _>>()?;
    let destructive = matches!(request.action, TableAction::Empty | TableAction::Drop);
    if destructive {
        require_unprotected(&request.database)?;
        if request.confirmation.as_deref() != Some(request.database.as_str()) {
            return Err("Type the database name to confirm this action.".into());
        }
    }
    let prefix = if destructive && request.disable_foreign_key_checks {
        "SET FOREIGN_KEY_CHECKS=0; "
    } else {
        ""
    };
    Ok(match request.action {
        TableAction::Empty => tables
            .iter()
            .map(|table| format!("{prefix}TRUNCATE TABLE {table};"))
            .collect::<Vec<_>>()
            .join(" "),
        TableAction::Drop => format!("{prefix}DROP TABLE {};", tables.join(", ")),
        TableAction::Optimize => format!("OPTIMIZE TABLE {};", tables.join(", ")),
        TableAction::Analyze => format!("ANALYZE TABLE {};", tables.join(", ")),
        TableAction::Check => format!("CHECK TABLE {};", tables.join(", ")),
        TableAction::Repair => format!("REPAIR TABLE {};", tables.join(", ")),
    })
}

fn literal(value: &str, numeric: bool) -> Result<String, String> {
    if value.len() > 255 || value.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Default values must be 255 characters or fewer with no control characters.".into());
    }
    if numeric {
        let digits = value.strip_prefix('-').unwrap_or(value);
        let mut parts = digits.split('.');
        let valid = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(a), None, None) | (Some(a), Some(_), None) if !a.is_empty()
        ) && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && !digits.ends_with('.');
        if !valid {
            return Err(format!("{value} is not a valid number default."));
        }
        return Ok(value.to_string());
    }
    Ok(format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''")))
}

fn column_sql(column: &ColumnSpec) -> Result<String, String> {
    let name = valid_name(&column.name)?;
    let kind = column.data_type.to_ascii_uppercase();
    if !TYPES.contains(&kind.as_str()) {
        return Err(format!("Unsupported column type {}.", column.data_type));
    }
    let length = column.length.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let mut sql = format!("{name} {kind}");
    if let Some(length) = length.or(if kind == "VARCHAR" { Some("255") } else { None }) {
        let valid = LENGTH_TYPES.contains(&kind.as_str())
            && length.len() <= 8
            && length.bytes().all(|b| b.is_ascii_digit() || b == b',')
            && !length.starts_with(',')
            && !length.ends_with(',')
            && length.matches(',').count() <= usize::from(kind == "DECIMAL");
        if !valid {
            return Err(format!("Invalid length for column {}.", column.name));
        }
        sql.push_str(&format!("({length})"));
    }
    if column.unsigned {
        if !NUMERIC_TYPES.contains(&kind.as_str()) {
            return Err(format!("UNSIGNED only applies to number columns ({}).", column.name));
        }
        sql.push_str(" UNSIGNED");
    }
    sql.push_str(if column.nullable && !column.primary { " NULL" } else { " NOT NULL" });
    match column.default_kind.as_str() {
        "none" => {}
        "null" if column.nullable && !column.primary => sql.push_str(" DEFAULT NULL"),
        "null" => return Err(format!("{} must allow NULL to default to NULL.", column.name)),
        "currentTimestamp" if matches!(kind.as_str(), "DATETIME" | "TIMESTAMP") => {
            sql.push_str(" DEFAULT CURRENT_TIMESTAMP")
        }
        "currentTimestamp" => {
            return Err(format!("CURRENT_TIMESTAMP only works on DATETIME/TIMESTAMP ({}).", column.name))
        }
        "value" => {
            let value = column.default_value.as_deref().unwrap_or_default();
            sql.push_str(&format!(" DEFAULT {}", literal(value, NUMERIC_TYPES.contains(&kind.as_str()))?));
        }
        other => return Err(format!("Unknown default type {other}.")),
    }
    if column.auto_increment {
        if !INTEGER_TYPES.contains(&kind.as_str()) || !column.primary {
            return Err(format!("AUTO_INCREMENT needs an integer PRIMARY KEY column ({}).", column.name));
        }
        sql.push_str(" AUTO_INCREMENT");
    }
    Ok(sql)
}

fn create_table_sql(spec: &CreateTableSpec) -> Result<String, String> {
    let database = quote_identifier(&spec.database)?;
    require_unprotected(&spec.database)?;
    let table = valid_name(&spec.name)?;
    if spec.columns.is_empty() || spec.columns.len() > MAX_COLUMNS {
        return Err(format!("A table needs between 1 and {MAX_COLUMNS} columns."));
    }
    let engine = spec.engine.as_deref().filter(|e| !e.is_empty()).unwrap_or("InnoDB");
    let engine = ENGINES
        .iter()
        .find(|allowed| allowed.eq_ignore_ascii_case(engine))
        .ok_or("Unsupported storage engine.")?;
    let (charset, collation) = collation_parts(spec.collation.as_deref())?;

    let mut names = std::collections::HashSet::new();
    let mut definitions = Vec::new();
    let mut primary = Vec::new();
    for column in &spec.columns {
        if !names.insert(column.name.to_lowercase()) {
            return Err(format!("Column {} is listed twice.", column.name));
        }
        definitions.push(column_sql(column)?);
        let quoted = valid_name(&column.name)?;
        if column.primary {
            primary.push(quoted.clone());
        }
        if column.unique && !column.primary {
            definitions.push(format!("UNIQUE KEY {quoted} ({quoted})"));
        }
    }
    if !primary.is_empty() {
        definitions.push(format!("PRIMARY KEY ({})", primary.join(", ")));
    }
    Ok(format!(
        "CREATE TABLE {database}.{table} ({}) ENGINE={engine} DEFAULT CHARSET={charset} COLLATE={collation};",
        definitions.join(", ")
    ))
}

#[tauri::command]
pub async fn list_table_info(
    credentials: MariaDBCredentials,
    database: String,
) -> Result<Vec<TableInfo>, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        quote_identifier(&database)?;
        query_json(&credentials, &table_info_sql(&database))
    })
    .await
}

#[tauri::command]
pub async fn run_table_action(
    credentials: MariaDBCredentials,
    request: TableActionRequest,
) -> Result<AdminResult, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        let sql = table_action_sql(&request)?;
        let result = run(&credentials, sql)?;
        let count = request.tables.len();
        let noun = if count == 1 { "table" } else { "tables" };
        let message = match request.action {
            TableAction::Empty => format!("Emptied {count} {noun}."),
            TableAction::Drop => format!("Dropped {count} {noun}."),
            _ => {
                let problems: Vec<String> = result
                    .rows
                    .iter()
                    .filter(|row| row.get(2).is_some_and(|kind| kind == "error" || kind == "warning"))
                    .filter_map(|row| Some(format!("{}: {}", row.first()?, row.get(3)?)))
                    .collect();
                if problems.is_empty() {
                    format!("Finished on {count} {noun}.")
                } else {
                    format!("Finished with messages - {}", problems.join("; "))
                }
            }
        };
        Ok(AdminResult { message })
    })
    .await
}

#[tauri::command]
pub async fn create_table(
    credentials: MariaDBCredentials,
    spec: CreateTableSpec,
) -> Result<AdminResult, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        run(&credentials, create_table_sql(&spec)?)?;
        Ok(AdminResult {
            message: format!("Table {} created.", spec.name),
        })
    })
    .await
}

#[tauri::command]
pub async fn create_database(
    credentials: MariaDBCredentials,
    name: String,
    collation: Option<String>,
) -> Result<AdminResult, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        require_unprotected(&name)?;
        let quoted = valid_name(&name)?;
        let (charset, collation) = collation_parts(collation.as_deref())?;
        run(
            &credentials,
            format!("CREATE DATABASE {quoted} CHARACTER SET {charset} COLLATE {collation};"),
        )?;
        Ok(AdminResult {
            message: format!("Database {name} created."),
        })
    })
    .await
}

#[tauri::command]
pub async fn drop_database(
    credentials: MariaDBCredentials,
    name: String,
    confirmation: String,
) -> Result<AdminResult, String> {
    super::run_blocking(move || {
        let _guard = super::mariadb::database_access()?;
        require_unprotected(&name)?;
        let quoted = quote_identifier(&name)?;
        if confirmation != name {
            return Err("Type the database name to confirm this action.".into());
        }
        run(&credentials, format!("DROP DATABASE {quoted};"))?;
        Ok(AdminResult {
            message: format!("Database {name} dropped."),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, kind: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.into(),
            data_type: kind.into(),
            length: None,
            unsigned: false,
            nullable: false,
            default_kind: "none".into(),
            default_value: None,
            auto_increment: false,
            primary: false,
            unique: false,
        }
    }

    fn spec(columns: Vec<ColumnSpec>) -> CreateTableSpec {
        CreateTableSpec {
            database: "shop".into(),
            name: "items".into(),
            engine: None,
            collation: None,
            columns,
        }
    }

    #[test]
    fn builds_a_table_with_keys_and_defaults() {
        let mut id = column("id", "int");
        id.primary = true;
        id.auto_increment = true;
        id.unsigned = true;
        let mut name = column("na`me", "varchar");
        name.unique = true;
        name.default_kind = "value".into();
        name.default_value = Some("it's".into());
        let sql = create_table_sql(&spec(vec![id, name])).unwrap();
        assert!(sql.starts_with("CREATE TABLE `shop`.`items` (`id` INT UNSIGNED NOT NULL AUTO_INCREMENT, `na``me` VARCHAR(255) NOT NULL DEFAULT 'it''s', UNIQUE KEY `na``me` (`na``me`), PRIMARY KEY (`id`))"));
        assert!(sql.ends_with("ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;"));
    }

    #[test]
    fn rejects_unsafe_or_inconsistent_definitions() {
        let mut auto = column("id", "varchar");
        auto.auto_increment = true;
        auto.primary = true;
        for bad in [
            column("x; DROP TABLE y", "int(11); --"),
            column("a/b", "int"),
            { let mut c = column("n", "int"); c.default_kind = "value".into(); c.default_value = Some("1; DROP".into()); c },
            { let mut c = column("n", "varchar"); c.length = Some("10);--".into()); c },
            auto,
        ] {
            assert!(create_table_sql(&spec(vec![bad])).is_err());
        }
        assert!(create_table_sql(&spec(vec![column("a", "int"), column("A", "int")])).is_err());
        let mut protected = spec(vec![column("a", "int")]);
        protected.database = "mysql".into();
        assert!(create_table_sql(&protected).is_err());
        let mut engine = spec(vec![column("a", "int")]);
        engine.engine = Some("InnoDB; DROP".into());
        assert!(create_table_sql(&engine).is_err());
    }

    fn request(action: TableAction, database: &str, confirmation: Option<&str>) -> TableActionRequest {
        TableActionRequest {
            database: database.into(),
            action,
            tables: vec!["a".into(), "b`c".into()],
            confirmation: confirmation.map(str::to_string),
            disable_foreign_key_checks: false,
        }
    }

    #[test]
    fn destructive_actions_need_the_database_name_and_skip_system_databases() {
        assert!(table_action_sql(&request(TableAction::Drop, "shop", None)).is_err());
        assert!(table_action_sql(&request(TableAction::Drop, "shop", Some("other"))).is_err());
        assert!(table_action_sql(&request(TableAction::Empty, "mysql", Some("mysql"))).is_err());
        assert_eq!(
            table_action_sql(&request(TableAction::Drop, "shop", Some("shop"))).unwrap(),
            "DROP TABLE `shop`.`a`, `shop`.`b``c`;"
        );
        assert_eq!(
            table_action_sql(&request(TableAction::Optimize, "shop", None)).unwrap(),
            "OPTIMIZE TABLE `shop`.`a`, `shop`.`b``c`;"
        );
        let mut fk = request(TableAction::Empty, "shop", Some("shop"));
        fk.disable_foreign_key_checks = true;
        assert!(table_action_sql(&fk).unwrap().starts_with("SET FOREIGN_KEY_CHECKS=0; TRUNCATE TABLE `shop`.`a`;"));
    }

    #[test]
    fn number_defaults_are_validated() {
        assert_eq!(literal("-12.5", true).unwrap(), "-12.5");
        for bad in ["1e5", "1.", ".5", "", "1.2.3", "--1"] {
            assert!(literal(bad, true).is_err(), "{bad}");
        }
    }
}
