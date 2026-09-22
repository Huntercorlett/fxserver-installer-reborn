//! In-process MariaDB access for loopback connections.
//!
//! Starting `mariadb.exe` for every statement (plus a protected credential file)
//! costs hundreds of milliseconds each, which is what made the Database Browser
//! feel slow. A small pooled connection answers in a few milliseconds. Anything
//! this driver cannot do (remote hosts, unusual auth plugins, a stopped server)
//! is reported as `Unavailable` so callers fall back to the client program.

use std::{
    net::IpAddr,
    sync::Mutex,
    time::Duration,
};

use mysql::{prelude::Queryable, OptsBuilder, Pool, PoolConstraints, PoolOpts, Value};

use crate::models::mariadb::MariaDBCredentials;

pub(crate) enum NativeError {
    /// The driver could not run the request; use the client program instead.
    Unavailable,
    /// The server answered with an error the user should see.
    Server(String),
}

static POOL: Mutex<Option<(String, Pool)>> = Mutex::new(None);

pub(crate) fn supported(credentials: &MariaDBCredentials) -> bool {
    credentials.port != 0
        && (credentials.host.eq_ignore_ascii_case("localhost")
            || credentials
                .host
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()))
}

/// Drops pooled connections; call before the service is stopped or replaced.
pub(crate) fn reset() {
    *POOL.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

fn pool(credentials: &MariaDBCredentials) -> Result<Pool, NativeError> {
    let key = format!(
        "{}\0{}\0{}\0{}",
        credentials.host, credentials.port, credentials.username, credentials.password
    );
    let mut slot = POOL.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((cached, pool)) = slot.as_ref() {
        if *cached == key {
            return Ok(pool.clone());
        }
    }
    let host = if credentials.host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        credentials.host.as_str()
    };
    let constraints = PoolConstraints::new(0, 4).ok_or(NativeError::Unavailable)?;
    let opts = OptsBuilder::new()
        .ip_or_hostname(Some(host))
        .tcp_port(credentials.port)
        .user(Some(credentials.username.as_str()))
        .pass(Some(credentials.password.as_str()))
        .prefer_socket(false)
        .tcp_connect_timeout(Some(Duration::from_secs(5)))
        .read_timeout(Some(Duration::from_secs(60)))
        .write_timeout(Some(Duration::from_secs(30)))
        .init(vec!["SET NAMES utf8mb4", "SET SESSION sql_mode=''"])
        .pool_opts(PoolOpts::default().with_constraints(constraints));
    let pool = Pool::new(opts).map_err(|_| NativeError::Unavailable)?;
    *slot = Some((key, pool.clone()));
    Ok(pool)
}

fn classify(error: mysql::Error) -> NativeError {
    match error {
        mysql::Error::MySqlError(error) => NativeError::Server(format!(
            "ERROR {} ({}): {}",
            error.code, error.state, error.message
        )),
        _ => NativeError::Unavailable,
    }
}

/// Runs one or more statements and returns the first column of every row of
/// every result set as text (the same lines the client prints with
/// `--batch --skip-column-names`).
pub(crate) fn first_column(
    credentials: &MariaDBCredentials,
    sql: &str,
    max_bytes: usize,
) -> Result<Vec<String>, NativeError> {
    let pool = pool(credentials)?;
    let mut connection = pool
        .try_get_conn(Duration::from_secs(10))
        .map_err(classify)?;
    let mut output = Vec::new();
    let mut bytes = 0usize;
    let mut result = connection.query_iter(sql).map_err(classify)?;
    while let Some(set) = result.iter() {
        if set.columns().as_ref().is_empty() {
            continue;
        }
        for row in set {
            let row = row.map_err(classify)?;
            let text = match row.as_ref(0) {
                Some(Value::Bytes(value)) => String::from_utf8_lossy(value).into_owned(),
                Some(Value::NULL) | None => continue,
                Some(other) => other.as_sql(true).trim_matches('\'').to_string(),
            };
            bytes += text.len() + 1;
            if bytes > max_bytes {
                return Err(NativeError::Server(
                    "Query output exceeded 16 MiB. Narrow the filters or page size.".into(),
                ));
            }
            output.push(text);
        }
    }
    Ok(output)
}
