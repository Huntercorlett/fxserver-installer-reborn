use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::services::website::{self, Exposure, RequestLogEntry, SiteConfig, WebsiteHost};

const SETTINGS_FILE: &str = "website-hosting.json";

/// Owns the running websites plus the last start error for each one.
#[derive(Default)]
pub struct WebsiteManager {
    host: WebsiteHost,
    errors: Mutex<HashMap<String, String>>,
    // Serialises save/start/stop/remove so two quick clicks cannot race.
    operation: tokio::sync::Mutex<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebsiteSiteStatus {
    config: SiteConfig,
    running: bool,
    started_at: Option<u64>,
    requests: u64,
    bytes_sent: u64,
    local_url: String,
    network_url: Option<String>,
    has_index: bool,
    error: Option<String>,
}

impl WebsiteManager {
    fn errors(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.errors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn try_start(&self, config: &SiteConfig) -> Result<(), String> {
        match self.host.start(config) {
            Ok(()) => {
                self.errors().remove(&config.id);
                Ok(())
            }
            Err(error) => {
                self.errors().insert(config.id.clone(), error.clone());
                Err(error)
            }
        }
    }

    fn statuses(&self, sites: Vec<SiteConfig>) -> Vec<WebsiteSiteStatus> {
        let lan_ip = if sites.iter().any(|site| site.exposure == Exposure::Network) {
            website::detect_lan_ip()
        } else {
            None
        };
        let errors = self.errors().clone();
        sites
            .into_iter()
            .map(|config| {
                let snapshot = self.host.snapshot(&config.id);
                WebsiteSiteStatus {
                    running: snapshot.is_some(),
                    started_at: snapshot.map(|value| value.started_at),
                    requests: snapshot.map_or(0, |value| value.requests),
                    bytes_sent: snapshot.map_or(0, |value| value.bytes_sent),
                    local_url: format!("http://127.0.0.1:{}/", config.port),
                    network_url: match (config.exposure, lan_ip) {
                        (Exposure::Network, Some(ip)) => {
                            Some(format!("http://{ip}:{}/", config.port))
                        }
                        _ => None,
                    },
                    has_index: website::root_has_index(&config.root, &config.index_file),
                    error: errors.get(&config.id).cloned(),
                    config,
                }
            })
            .collect()
    }

    /// Starts every site marked "start with the app". Called once at launch.
    pub fn start_autostart(&self, app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let manager = app.state::<WebsiteManager>();
            let _guard = manager.operation.lock().await;
            let Ok(path) = settings_path(&app) else {
                return;
            };
            let Ok(sites) = super::run_blocking(move || website::load_sites(&path)).await else {
                return;
            };
            for site in sites.iter().filter(|site| site.autostart) {
                let _ = manager.try_start(site);
            }
        });
    }

    /// Closes every listening socket during app shutdown.
    pub fn stop_all(&self) {
        self.host.signal_stop_all();
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve application data directory: {error}"))?
        .join(SETTINGS_FILE))
}

async fn load_sites(app: &AppHandle) -> Result<Vec<SiteConfig>, String> {
    let path = settings_path(app)?;
    super::run_blocking(move || website::load_sites(&path)).await
}

async fn store_sites(app: &AppHandle, sites: Vec<SiteConfig>) -> Result<(), String> {
    let path = settings_path(app)?;
    super::run_blocking(move || website::save_sites(&path, &sites)).await
}

#[tauri::command]
pub async fn get_website_sites(
    app: AppHandle,
    manager: State<'_, WebsiteManager>,
) -> Result<Vec<WebsiteSiteStatus>, String> {
    let sites = load_sites(&app).await?;
    Ok(manager.statuses(sites))
}

#[tauri::command]
pub async fn save_website_site(
    app: AppHandle,
    manager: State<'_, WebsiteManager>,
    site: SiteConfig,
) -> Result<Vec<WebsiteSiteStatus>, String> {
    let _guard = manager.operation.lock().await;
    let mut sites = load_sites(&app).await?;
    let config = {
        let existing = sites.clone();
        super::run_blocking(move || website::normalize_config(site, &existing)).await?
    };

    match sites.iter_mut().find(|item| item.id == config.id) {
        Some(slot) => *slot = config.clone(),
        None => sites.push(config.clone()),
    }
    store_sites(&app, sites.clone()).await?;

    // A running site keeps serving the old folder/port until it is restarted,
    // so apply the edit right away. A failed restart is shown on the card.
    if manager.host.snapshot(&config.id).is_some() {
        manager.host.stop(&config.id).await;
        let _ = manager.try_start(&config);
    }
    Ok(manager.statuses(sites))
}

#[tauri::command]
pub async fn remove_website_site(
    app: AppHandle,
    manager: State<'_, WebsiteManager>,
    id: String,
) -> Result<Vec<WebsiteSiteStatus>, String> {
    let _guard = manager.operation.lock().await;
    manager.host.stop(&id).await;
    manager.errors().remove(&id);
    let mut sites = load_sites(&app).await?;
    sites.retain(|site| site.id != id);
    store_sites(&app, sites.clone()).await?;
    Ok(manager.statuses(sites))
}

#[tauri::command]
pub async fn start_website_site(
    app: AppHandle,
    manager: State<'_, WebsiteManager>,
    id: String,
) -> Result<Vec<WebsiteSiteStatus>, String> {
    let _guard = manager.operation.lock().await;
    let sites = load_sites(&app).await?;
    let config = sites
        .iter()
        .find(|site| site.id == id)
        .cloned()
        .ok_or_else(|| "That website no longer exists.".to_string())?;
    manager.try_start(&config)?;
    Ok(manager.statuses(sites))
}

#[tauri::command]
pub async fn stop_website_site(
    app: AppHandle,
    manager: State<'_, WebsiteManager>,
    id: String,
) -> Result<Vec<WebsiteSiteStatus>, String> {
    let _guard = manager.operation.lock().await;
    manager.host.stop(&id).await;
    manager.errors().remove(&id);
    let sites = load_sites(&app).await?;
    Ok(manager.statuses(sites))
}

#[tauri::command]
pub async fn get_website_requests(
    manager: State<'_, WebsiteManager>,
    id: String,
) -> Result<Vec<RequestLogEntry>, String> {
    Ok(manager.host.recent_requests(&id))
}

#[tauri::command]
pub async fn list_website_pages(root: String) -> Result<Vec<String>, String> {
    super::run_blocking(move || Ok(website::list_html_pages(root.trim().trim_matches('"')))).await
}
