use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use reqwest::{blocking::Client, Url};

use crate::{
    models::artifact::{
        ArtifactInstallResult, EnhancedArtifactBuild, EnhancedArtifactCatalog,
        EnhancedInstallRequest,
    },
    process::CommandNoWindowExt,
};

pub(super) const ENHANCED_PAGE: &str =
    "https://docs.fivem.net/docs/server-download/?platform=enhanced&os=windows";
const ARTIFACT_HOST: &str = "downloads.cfx-services.net";
const ARTIFACT_URL_PREFIX: &str = "https://downloads.cfx-services.net/prod/";
const ARTIFACT_FILE: &str = "cfx-server_win_x64.zip";
/// Last known official Enhanced link, offered only when the live page cannot be read.
const KNOWN_BUILD: &str =
    "https://downloads.cfx-services.net/prod/01a05860-2cbf-73e1-96f0-cec4a003f38c/cfx-server_win_x64.zip";
const CHANGELOG_PREFIX: &str = "https://changelogs-live.fivem.net/api/changelog/versions/";
const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_SCRIPTS: usize = 24;

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .user_agent("fxserver-installer")
        .build()
        .map_err(|error| format!("Could not create HTTP client: {error}"))
}

fn fetch_text(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Request to {url} failed: {error}"))?;
    let bytes = response
        .bytes()
        .map_err(|error| format!("Could not read {url}: {error}"))?;
    if bytes.len() > MAX_BODY {
        return Err(format!("{url} returned an unexpectedly large response."));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn normalize(text: &str) -> String {
    text.replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\/", "/")
        .replace("&amp;", "&")
}

fn scan_urls(text: &str, prefix: &str) -> Vec<String> {
    let text = normalize(text);
    let mut found = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find(prefix) {
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | ')' | ',' | '`'))
            .unwrap_or(tail.len());
        found.push(tail[..end].to_string());
        rest = &tail[end.max(prefix.len())..];
    }
    found
}

pub(super) fn validate_enhanced_url(value: &str) -> Result<String, String> {
    let invalid = || {
        format!("Enter the Enhanced Windows download link (https://{ARTIFACT_HOST}/prod/<build id>/{ARTIFACT_FILE}).")
    };
    let url = Url::parse(value.trim()).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some(ARTIFACT_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let segments: Vec<&str> = url.path().split('/').collect();
    match segments.as_slice() {
        ["", "prod", id, file] if *file == ARTIFACT_FILE && is_build_id(id) => Ok(id.to_ascii_lowercase()),
        _ => Err(invalid()),
    }
}

fn is_build_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn builds_from(urls: Vec<String>) -> Vec<EnhancedArtifactBuild> {
    let mut builds: Vec<EnhancedArtifactBuild> = Vec::new();
    for download_url in urls {
        if let Ok(version) = validate_enhanced_url(&download_url) {
            if !builds.iter().any(|build| build.version == version) {
                builds.push(EnhancedArtifactBuild { version, download_url });
            }
        }
    }
    builds
}

pub(super) fn load_catalog() -> Result<EnhancedArtifactCatalog, String> {
    let client = client(Duration::from_secs(30))?;
    let page = fetch_text(&client, ENHANCED_PAGE)?;
    let mut urls = scan_urls(&page, ARTIFACT_URL_PREFIX);
    let mut apis = scan_urls(&page, CHANGELOG_PREFIX);

    let base = Url::parse(ENHANCED_PAGE).map_err(|error| error.to_string())?;
    let normalized = normalize(&page);
    let mut scripts = Vec::new();
    let mut rest = normalized.as_str();
    while let Some(start) = rest.find("src=\"") {
        let tail = &rest[start + 5..];
        let Some(end) = tail.find('"') else { break };
        if let Ok(script) = base.join(&tail[..end]) {
            if script.host_str() == Some("docs.fivem.net") && script.path().ends_with(".js") {
                scripts.push(script.to_string());
            }
        }
        rest = &tail[end..];
    }
    scripts.sort();
    scripts.dedup();
    for script in scripts.into_iter().take(MAX_SCRIPTS) {
        if let Ok(body) = fetch_text(&client, &script) {
            urls.extend(scan_urls(&body, ARTIFACT_URL_PREFIX));
            apis.extend(scan_urls(&body, CHANGELOG_PREFIX));
        }
    }

    apis.retain(|api| api.to_ascii_lowercase().contains("enhanced"));
    apis.sort();
    apis.dedup();
    for api in apis {
        if let Ok(body) = fetch_text(&client, &api) {
            urls.extend(scan_urls(&body, ARTIFACT_URL_PREFIX));
        }
    }

    let mut builds = builds_from(urls);
    let warning = builds.is_empty().then(|| {
        "Could not read the live Enhanced link from the download page, so the last known build is offered. Open the page, copy the Download link, and paste it below for the newest build.".to_string()
    });
    if builds.is_empty() {
        builds = builds_from(vec![KNOWN_BUILD.to_string()]);
    }
    Ok(EnhancedArtifactCatalog {
        builds,
        source_url: ENHANCED_PAGE.to_string(),
        warning,
    })
}

fn extract(archive: &Path, destination: &Path) -> Result<(), String> {
    // Windows ships bsdtar, which unpacks zip far faster than Expand-Archive.
    let tar = Command::new("tar")
        .no_window()
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .output();
    if tar.as_ref().is_ok_and(|output| output.status.success()) {
        return Ok(());
    }
    let script = format!(
        "$ErrorActionPreference='Stop'; Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        archive.to_string_lossy().replace('\'', "''"),
        destination.to_string_lossy().replace('\'', "''")
    );
    let output = Command::new("powershell")
        .no_window()
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|error| format!("Could not start the archive extractor: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Extracting the Enhanced server archive failed. {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn download(client: &Client, url: &str, path: &Path) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Enhanced server download failed: {error}"))?;
    let mut file = fs::File::create(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    response
        .copy_to(&mut file)
        .map_err(|error| format!("Enhanced server download was interrupted: {error}"))?;
    file.flush()
        .map_err(|error| format!("Could not write the download: {error}"))
}

pub(super) fn install(request: EnhancedInstallRequest) -> Result<ArtifactInstallResult, String> {
    if !cfg!(target_os = "windows") {
        return Err("Enhanced server installation is only supported on Windows.".into());
    }
    let version = validate_enhanced_url(&request.url)?;
    let destination = PathBuf::from(request.destination.trim());
    if destination.as_os_str().is_empty() {
        return Err("Choose a destination folder before installing the Enhanced server.".into());
    }
    if let Some((_, crate::commands::server_exe::ServerEdition::Legacy)) =
        crate::commands::server_exe::find_server_executable(&destination)
    {
        return Err("This folder holds a Legacy FXServer.exe build. Installing Enhanced here would mix two different server builds. Choose a different folder.".into());
    }
    fs::create_dir_all(&destination)
        .map_err(|error| format!("Failed to create install folder: {error}"))?;

    let archive = destination.join(format!("fxserver-enhanced-{version}.zip"));
    let result = download(&client(Duration::from_secs(1800))?, request.url.trim(), &archive)
        .and_then(|_| extract(&archive, &destination));
    let _ = fs::remove_file(&archive);
    result?;

    if !matches!(
        crate::commands::server_exe::find_server_executable(&destination),
        Some((_, crate::commands::server_exe::ServerEdition::Enhanced))
    ) {
        return Err("Extraction finished, but cfx-server.exe was not found in the install folder. The link may not be an Enhanced server build.".into());
    }
    let marker = destination.join(".fxserver-artifact-version");
    fs::write(&marker, &version)
        .map_err(|error| format!("Failed to write the version marker: {error}"))?;

    Ok(ArtifactInstallResult {
        version,
        destination: destination.to_string_lossy().to_string(),
        marker_path: marker.to_string_lossy().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01a05860-2cbf-73e1-96f0-cec4a003f38c";

    #[test]
    fn accepts_only_the_official_enhanced_download_link() {
        assert_eq!(validate_enhanced_url(KNOWN_BUILD).unwrap(), ID);
        let good = format!("https://{ARTIFACT_HOST}/prod/{ID}/{ARTIFACT_FILE}");
        let bad_urls = vec![
            good.replacen("https://", "http://", 1),
            good.replacen(ARTIFACT_HOST, "evil.example", 1),
            good.replacen(ARTIFACT_HOST, &format!("{ARTIFACT_HOST}:444"), 1),
            good.replacen(ARTIFACT_HOST, &format!("user@{ARTIFACT_HOST}"), 1),
            format!("{good}?x=1"),
            good.replace(ARTIFACT_FILE, "server.7z"),
            good.replace(ID, "not-a-build-id"),
            format!("https://{ARTIFACT_HOST}/prod/{ID}/../{ARTIFACT_FILE}"),
            format!("https://{ARTIFACT_HOST}/dev/{ID}/{ARTIFACT_FILE}"),
        ];
        for bad in &bad_urls {
            assert!(validate_enhanced_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn keeps_page_order_and_drops_duplicates() {
        let second = KNOWN_BUILD.replace("01a05860", "11a05860");
        let builds = builds_from(vec![second.clone(), KNOWN_BUILD.to_string(), second, "https://evil.example/x".into()]);
        assert_eq!(builds.len(), 2);
        assert!(builds[0].version.starts_with("11a05860"));
    }

    #[test]
    fn finds_links_inside_escaped_json() {
        let json = format!("{{\"download\":\"{}\"}}", KNOWN_BUILD.replace('/', "\\/"));
        assert_eq!(scan_urls(&json, ARTIFACT_URL_PREFIX), vec![KNOWN_BUILD.to_string()]);
    }
}
