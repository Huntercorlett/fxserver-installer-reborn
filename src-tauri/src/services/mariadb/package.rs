use super::install::{run_process, InstallOutput};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
pub(super) struct Package {
    pub version: String,
    pub file_name: String,
    pub sha256: String,
}

static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Package)>>> = OnceLock::new();

/// `requested` is `None` (default 10.11 LTS), a series such as `11.4`, or an exact release such as `11.4.5`.
pub(super) fn resolve_package(requested: Option<&str>) -> Result<Package, String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    if let Some(value) = requested {
        if !valid_series(value) && !valid_release(value) {
            return Err("MariaDB version must look like 10.11 or 10.11.14.".to_string());
        }
    }
    let key = requested.unwrap_or_default().to_string();
    let mut cached = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "MariaDB package cache is unavailable.".to_string())?;
    if let Some((time, package)) = cached.get(&key) {
        if time.elapsed() < Duration::from_secs(15 * 60) {
            return Ok(package.clone());
        }
    }
    let script = format!(
        "$requested = '{}'\n{}",
        requested.unwrap_or_default(),
        include_str!("package-metadata.ps1")
    );
    let output = run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(75),
    )?;
    if !output.success {
        return Err(format!(
            "Could not resolve the MariaDB installer: {}",
            output.stderr
        ));
    }
    let package = parse_package(&output.stdout, requested)?;
    cached.insert(key, (Instant::now(), package.clone()));
    Ok(package)
}

fn valid_series(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 2 && parts.iter().all(|part| is_number(part))
}

fn valid_release(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| is_number(part))
}

fn is_number(part: &str) -> bool {
    !part.is_empty() && part.len() <= 4 && part.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Clone, Deserialize, serde::Serialize)]
pub struct MariaDBSeries {
    pub series: String,
    pub status: Option<String>,
    pub support: Option<String>,
    pub eol: Option<String>,
}

#[derive(Clone, Deserialize, serde::Serialize)]
pub struct MariaDBRelease {
    pub version: String,
    pub date: Option<String>,
}

fn run_listing<T: serde::de::DeserializeOwned>(series: Option<&str>) -> Result<Vec<T>, String> {
    let script = format!(
        "$series = '{}'\n{}",
        series.unwrap_or_default(),
        include_str!("package-versions.ps1")
    );
    let output = run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(75),
    )?;
    if !output.success {
        return Err(format!(
            "Could not load MariaDB versions: {}",
            output.stderr
        ));
    }
    serde_json::from_str(&output.stdout)
        .map_err(|error| format!("Invalid MariaDB version listing: {error}"))
}

pub(crate) fn list_series() -> Result<Vec<MariaDBSeries>, String> {
    let list: Vec<MariaDBSeries> = run_listing(None)?;
    Ok(list.into_iter().filter(|item| valid_series(&item.series)).collect())
}

pub(crate) fn list_releases(series: &str) -> Result<Vec<MariaDBRelease>, String> {
    if !valid_series(series) {
        return Err("MariaDB series must look like 10.11.".to_string());
    }
    let list: Vec<MariaDBRelease> = run_listing(Some(series))?;
    Ok(list.into_iter().filter(|item| valid_release(&item.version)).collect())
}

fn parse_package(json: &str, requested: Option<&str>) -> Result<Package, String> {
    let package: Package = serde_json::from_str(json)
        .map_err(|error| format!("Invalid MariaDB download metadata: {error}"))?;
    let valid_version = package.version.split('.').count() == 3
        && package
            .version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !valid_version
        || package.file_name != format!("mariadb-{}-winx64.msi", package.version)
        || package.sha256.len() != 64
        || !package.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(
            "MariaDB download metadata did not contain a valid Windows MSI and SHA-256."
                .to_string(),
        );
    }
    if let Some(requested) = requested {
        let matches = if valid_release(requested) {
            package.version == requested
        } else {
            package.version.starts_with(&format!("{requested}."))
        };
        if !matches {
            return Err(format!(
                "MariaDB returned version {} instead of the requested {requested}.",
                package.version
            ));
        }
    }
    Ok(package)
}

pub(super) fn download_package(
    package: &Package,
    destination: &Path,
) -> Result<InstallOutput, String> {
    let url = format!(
        "https://downloads.mariadb.org/rest-api/mariadb/{}/{}",
        package.version, package.file_name
    );
    let path = destination.to_string_lossy().replace('\'', "''");
    let verify = verify_checksum_script(destination, &package.sha256);
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
try {{
    Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile '{path}' -TimeoutSec 600
    {verify}
    Write-Output 'MariaDB installer SHA-256 verified.'
}} catch {{
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}}
"#
    );
    run_process(
        "powershell",
        &["-NoProfile", "-Command", &script],
        Duration::from_secs(660),
    )
}

pub(super) fn verify_checksum_script(path: &Path, checksum: &str) -> String {
    let path = path.to_string_lossy().replace('\'', "''");
    format!(
        r#"$stream = [IO.File]::OpenRead('{path}')
$sha = [Security.Cryptography.SHA256]::Create()
try {{
    $hash = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')
    if ($hash -ne '{checksum}') {{ throw 'MariaDB installer checksum verification failed.' }}
}} finally {{
    $sha.Dispose()
    $stream.Dispose()
}}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unverified_or_unexpected_installers() {
        let valid = serde_json::json!({"version": "12.3.3", "file_name": "mariadb-12.3.3-winx64.msi", "sha256": "a".repeat(64)});
        assert!(parse_package(&valid.to_string(), None).is_ok());
        for (field, value) in [
            ("sha256", ""),
            ("file_name", "../other.msi"),
            ("version", "12.3.3-rc"),
        ] {
            let mut bad = valid.clone();
            bad[field] = value.into();
            assert!(parse_package(&bad.to_string(), None).is_err());
        }
    }

    #[test]
    fn rejects_a_package_that_is_not_the_requested_version() {
        let json = serde_json::json!({"version": "11.4.5", "file_name": "mariadb-11.4.5-winx64.msi", "sha256": "a".repeat(64)}).to_string();
        assert!(parse_package(&json, Some("11.4")).is_ok());
        assert!(parse_package(&json, Some("11.4.5")).is_ok());
        assert!(parse_package(&json, Some("11.8")).is_err());
        assert!(parse_package(&json, Some("11.4.4")).is_err());
        assert!(!valid_series("11.4'; calc"));
        assert!(!valid_release("11.4"));
    }

    #[test]
    #[ignore = "downloads the official MSI without installing it"]
    fn downloads_and_verifies_official_msi() {
        let package = resolve_package(None).expect("official metadata");
        let path =
            std::env::temp_dir().join(format!("fxi-package-test-{}.msi", std::process::id()));
        let output = download_package(&package, &path);
        let size = std::fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let _ = std::fs::remove_file(path);
        let output = output.expect("download task");
        assert!(output.success, "{}", output.stderr);
        assert!(size > 1024 * 1024);
        eprintln!(
            "Verified MariaDB {} Windows MSI ({size} bytes)",
            package.version
        );
    }
}
