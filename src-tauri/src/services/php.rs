//! Portable PHP, downloaded and unpacked next to the app itself — no XAMPP,
//! no system-wide PHP install, no admin prompt. This is what makes Website
//! Hosting "plug and play": drop a folder with .php files in, and if PHP
//! isn't around yet, the user gets one button that fetches it.

use crate::process::CommandNoWindowExt;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const MAX_ARCHIVE: u64 = 200 * 1024 * 1024;

/// Where bundled tools live: a `tools` folder right next to the app's own
/// .exe, so the whole install stays portable and self-contained.
pub fn tools_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("tools");
    Some(dir)
}

pub fn bundled_php_dir() -> Option<PathBuf> {
    Some(tools_dir()?.join("php"))
}

pub fn bundled_php_cgi_path() -> Option<PathBuf> {
    Some(bundled_php_dir()?.join("php-cgi.exe"))
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpStatus {
    /// Path to a PHP this app already downloaded for itself.
    pub bundled_path: Option<String>,
    /// Path to a PHP found elsewhere on the system (a manual install), if any.
    pub system_path: Option<String>,
}

pub fn php_status() -> PhpStatus {
    let bundled = bundled_php_cgi_path().filter(|path| path.is_file());
    let system = super::website::find_php_cgi().filter(|path| Some(path) != bundled.as_ref());
    PhpStatus {
        bundled_path: bundled.map(|p| p.to_string_lossy().to_string()),
        system_path: system.map(|p| p.to_string_lossy().to_string()),
    }
}

fn run_powershell(script: &str, timeout: Duration) -> Result<String, String> {
    let child = Command::new("powershell")
        .no_window()
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start PowerShell: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("PowerShell did not finish: {error}"))?;
    // wait_with_output has no timeout of its own; a hard timeout here would
    // need a watcher thread, but the download step below already has its own
    // timeouts, and release-page scraping is a single quick HTTPS GET.
    let _ = timeout;
    if !output.status.success() {
        return Err(format!(
            "PowerShell failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Finds the newest Non-Thread-Safe (what php-cgi.exe wants) 64-bit Windows
/// PHP release by reading windows.php.net's own releases page, and returns
/// (version, download URL) for its zip package.
fn resolve_latest_php_zip() -> Result<(String, String), String> {
    // windows.php.net doesn't have a clean JSON API like mariadb.org, so this
    // reads the releases directory listing and picks the newest NTS x64 zip.
    let script = r#"
$ProgressPreference = 'SilentlyContinue'
$html = Invoke-WebRequest -Uri 'https://windows.php.net/downloads/releases/' -UseBasicParsing -TimeoutSec 30 | Select-Object -ExpandProperty Content
$matches = [regex]::Matches($html, 'php-(8\.\d+\.\d+)-nts-Win32-vs\d+-x64\.zip')
if ($matches.Count -eq 0) { throw 'No PHP NTS x64 release found.' }
$best = $matches | ForEach-Object { $_.Value } | Sort-Object { [version]($_ -replace 'php-(8\.\d+\.\d+).*','$1') } -Descending | Select-Object -First 1
Write-Output $best
"#;
    let file_name = run_powershell(script, Duration::from_secs(40))?;
    if file_name.is_empty() || !file_name.starts_with("php-") {
        return Err("Could not find a PHP release to download.".into());
    }
    let version = file_name
        .strip_prefix("php-")
        .and_then(|rest| rest.split('-').next())
        .unwrap_or("unknown")
        .to_string();
    let url = format!("https://windows.php.net/downloads/releases/{file_name}");
    Ok((version, url))
}

fn download_to_file(url: &str, destination: &Path, report: &dyn Fn(&str)) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent("FXServer-Installer")
        .build()
        .map_err(|error| format!("Could not start the download: {error}"))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|error| format!("PHP download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("PHP download failed: {error}"))?;
    let total = response.content_length();
    if total.is_some_and(|size| size > MAX_ARCHIVE) {
        return Err("PHP download looked unexpectedly large; aborting.".into());
    }
    let mut file = File::create(destination)
        .map_err(|error| format!("Could not create a temp file for the download: {error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_reported_pct = u64::MAX;
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("PHP download was interrupted: {error}"))?;
        if read == 0 {
            break;
        }
        downloaded += read as u64;
        if downloaded > MAX_ARCHIVE {
            return Err("PHP download exceeded the expected size; aborting.".into());
        }
        std::io::Write::write_all(&mut file, &buffer[..read])
            .map_err(|error| format!("Could not save the download: {error}"))?;
        if let Some(total) = total {
            let pct = (downloaded * 100) / total.max(1);
            if pct != last_reported_pct {
                last_reported_pct = pct;
                report(&format!("Downloading PHP... {pct}%"));
            }
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(
        File::open(archive).map_err(|error| format!("Could not open the PHP archive: {error}"))?,
    )
    .map_err(|error| format!("The PHP archive looks corrupt: {error}"))?;
    fs::create_dir_all(destination)
        .map_err(|error| format!("Could not create the PHP folder: {error}"))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("Could not read the PHP archive: {error}"))?;
        let name = entry.name().replace('\\', "/");
        if name.contains("..") {
            return Err("The PHP archive contained an unsafe path.".into());
        }
        let out_path = destination.join(&name);
        if name.ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut out_file = File::create(&out_path)
            .map_err(|error| format!("Could not write {name}: {error}"))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|error| format!("Could not extract {name}: {error}"))?;
    }
    Ok(())
}

/// Turns on the extensions a normal database-backed PHP site needs
/// (mysqli/pdo_mysql for MariaDB, curl, mbstring, openssl, fileinfo, gd),
/// since a fresh PHP zip ships with everything commented out.
fn write_php_ini(php_dir: &Path) -> Result<(), String> {
    let ini_path = php_dir.join("php.ini");
    let template_path = php_dir.join("php.ini-production");
    let mut contents = fs::read_to_string(&template_path).unwrap_or_default();
    if contents.is_empty() {
        contents = String::from("[PHP]\n");
    }
    let ext_dir = php_dir.join("ext");
    let ext_dir_line = format!(
        "extension_dir = \"{}\"",
        ext_dir.to_string_lossy().replace('\\', "/")
    );
    contents = contents.replace(";extension_dir = \"ext\"", &ext_dir_line);
    if !contents.contains(&ext_dir_line) {
        contents.push('\n');
        contents.push_str(&ext_dir_line);
        contents.push('\n');
    }
    for extension in [
        "curl", "mbstring", "openssl", "fileinfo", "gd", "mysqli", "pdo_mysql",
    ] {
        let commented = format!(";extension={extension}");
        let enabled = format!("extension={extension}");
        if contents.contains(&commented) {
            contents = contents.replace(&commented, &enabled);
        } else if !contents.contains(&enabled) {
            contents.push_str(&enabled);
            contents.push('\n');
        }
    }
    fs::write(&ini_path, contents)
        .map_err(|error| format!("Could not write php.ini: {error}"))?;
    Ok(())
}

/// Downloads and unpacks a portable PHP next to the app, wires up a working
/// php.ini, and returns the path to php-cgi.exe. Safe to call again later —
/// it re-downloads over the same folder if the user asks for it.
pub fn install_bundled_php(report: &dyn Fn(&str)) -> Result<String, String> {
    let php_dir = bundled_php_dir().ok_or("Could not determine where to install PHP.")?;
    report("Checking the latest PHP release for Windows...");
    let (version, url) = resolve_latest_php_zip()?;
    report(&format!("Found PHP {version}."));

    let temp_dir = std::env::temp_dir();
    let archive_path = temp_dir.join(format!("fxserver-installer-php-{version}.zip"));
    download_to_file(&url, &archive_path, report)?;

    report("Extracting PHP...");
    if php_dir.exists() {
        fs::remove_dir_all(&php_dir)
            .map_err(|error| format!("Could not clear the old PHP folder: {error}"))?;
    }
    extract_zip(&archive_path, &php_dir)?;
    let _ = fs::remove_file(&archive_path);

    report("Configuring PHP (enabling MySQL, cURL, mbstring, GD)...");
    write_php_ini(&php_dir)?;

    let cgi_path = php_dir.join("php-cgi.exe");
    if !cgi_path.is_file() {
        return Err("PHP was downloaded but php-cgi.exe was not found inside it.".into());
    }
    report(&format!("PHP {version} is ready."));
    Ok(cgi_path.to_string_lossy().to_string())
}
