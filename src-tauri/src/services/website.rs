//! HTTP server behind the "Website Hosting" page.
//!
//! Each configured site is one folder served on one port. Static sites are
//! served as-is (GET/HEAD/OPTIONS, keep-alive, ETag, byte ranges) and are
//! deliberately safe by default: dotfiles, server-side scripts and
//! config-like files are never served, and requests can never leave the
//! site folder. A PHP site runs `.php` files through php-cgi in that same
//! folder, XAMPP-style — everything else in it is still a plain static file.
//! A Node site instead runs the user's own server and is reverse-proxied to.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, UdpSocket},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader},
    net::{tcp::OwnedWriteHalf, TcpListener, TcpStream},
    sync::{watch, Semaphore},
};

const MAX_CONNECTIONS: usize = 256;
const MAX_REQUESTS_PER_CONNECTION: usize = 200;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_COUNT: usize = 64;
const MAX_TARGET_BYTES: usize = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const CHUNK_BYTES: usize = 64 * 1024;
const MAX_CONNECTIONS_PER_IP: usize = 32;
const RATE_WINDOW: Duration = Duration::from_secs(10);
const MAX_REQUESTS_PER_WINDOW: u32 = 300;
const MAX_TRACKED_CLIENTS: usize = 20_000;
const REQUEST_LOG_LIMIT: usize = 100;
const CUSTOM_404_MAX_BYTES: u64 = 1024 * 1024;
const SERVER_NAME: &str = "fxserver-installer-website";

/// Files with these extensions are never served. Serving them as static files
/// would hand out the source of server-side scripts and configuration or
/// database dumps that people commonly leave inside a web folder.
const BLOCKED_EXTENSIONS: &[&str] = &[
    "php", "phtml", "php3", "php4", "php5", "php7", "phps", "asp", "aspx", "jsp", "cgi", "pl",
    "py", "rb", "sh", "bat", "cmd", "ps1", "cfg", "conf", "ini", "env", "sql", "bak", "log",
];
const BLOCKED_FILE_NAMES: &[&str] = &["web.config"];
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Exposure {
    /// Only reachable from this computer (127.0.0.1).
    #[default]
    Local,
    /// Reachable from the local network / internet (0.0.0.0).
    Network,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// Files are served exactly as they are.
    #[default]
    Static,
    /// A PHP process (e.g. `php -S 127.0.0.1:PORT -t .`) handles every request.
    Php,
    /// A Node process (e.g. `node server.js`) handles every request.
    Node,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteConfig {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub root: String,
    pub port: u16,
    #[serde(default)]
    pub exposure: Exposure,
    /// Main page served for the site root, relative to the folder (for
    /// example `home.html` or `pages/main.html`). Empty means index.html.
    #[serde(default)]
    pub index_file: String,
    /// Serve the main page for unknown extension-less paths (single page apps).
    #[serde(default)]
    pub spa_fallback: bool,
    /// Start this site automatically when the app opens.
    #[serde(default)]
    pub autostart: bool,
    /// What handles requests: plain static files, or a PHP/Node process
    /// reverse-proxied on `backend_port`.
    #[serde(default)]
    pub runtime: Runtime,
    /// Command that starts the backend, run from the website folder, e.g.
    /// `php -S 127.0.0.1:8901 -t .` or `node server.js`. Ignored for Static.
    #[serde(default)]
    pub backend_command: String,
    /// Port the backend command listens on internally. Visitors still use
    /// `port` above; every request is forwarded to this port untouched.
    #[serde(default)]
    pub backend_port: u16,
}

#[derive(Default, Serialize, Deserialize)]
struct SiteFile {
    #[serde(default)]
    sites: Vec<SiteConfig>,
}

pub fn load_sites(path: &Path) -> Result<Vec<SiteConfig>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Failed to read website settings: {error}")),
    };
    match serde_json::from_str::<SiteFile>(&raw) {
        Ok(file) => Ok(file.sites),
        Err(_) => {
            // Keep the unreadable file around instead of silently destroying it.
            let mut backup = path.as_os_str().to_owned();
            backup.push(".bad");
            let _ = fs::rename(path, PathBuf::from(backup));
            Ok(Vec::new())
        }
    }
}

pub fn save_sites(path: &Path, sites: &[SiteConfig]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create website settings folder: {error}"))?;
    }
    let body = serde_json::to_string_pretty(&SiteFile {
        sites: sites.to_vec(),
    })
    .map_err(|error| format!("Failed to encode website settings: {error}"))?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, body).map_err(|error| format!("Failed to write website settings: {error}"))?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("Failed to save website settings: {error}")
    })
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn generate_site_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("site-{:x}{:x}", nanos & 0xffff_ffff_ffff, counter)
}

fn valid_site_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Validates and normalises a site coming from the UI. `existing` is the
/// current saved list (including the site being edited, if any).
pub fn normalize_config(
    mut config: SiteConfig,
    existing: &[SiteConfig],
) -> Result<SiteConfig, String> {
    config.name = config.name.trim().to_string();
    if config.name.is_empty() {
        return Err("Enter a name for the website.".into());
    }
    if config.name.chars().count() > 60 || config.name.chars().any(char::is_control) {
        return Err(
            "Website names must be 60 characters or fewer and contain no control characters."
                .into(),
        );
    }

    let root = config.root.trim().trim_matches('"').trim().to_string();
    if root.is_empty() {
        return Err("Choose the folder that contains the website files.".into());
    }
    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        return Err(format!("The website folder was not found: {root}"));
    }
    let canonical = fs::canonicalize(root_path)
        .map_err(|error| format!("Failed to open the website folder: {error}"))?;
    if canonical.parent().is_none() {
        return Err("Choose a folder for the website, not an entire drive.".into());
    }
    config.root = root;
    config.index_file = normalize_index_file(&config.index_file)?;

    if config.port == 0 {
        return Err("Enter a port between 1 and 65535.".into());
    }

    match config.runtime {
        Runtime::Static => {
            config.backend_command.clear();
            config.backend_port = 0;
        }
        Runtime::Php => {
            config.backend_command = config.backend_command.trim().trim_matches('"').to_string();
            if config.backend_command.is_empty() {
                // XAMPP never makes you type this in — try to find it first.
                match find_php_cgi() {
                    Some(cgi) => config.backend_command = cgi.to_string_lossy().to_string(),
                    None => {
                        return Err(
                            "PHP isn't installed yet. Use the \"Install PHP\" button on this page (downloads a portable copy automatically), or enter the path to php-cgi.exe yourself.".into(),
                        )
                    }
                }
            }
            if !Path::new(&config.backend_command).is_file() {
                return Err(format!(
                    "php-cgi.exe was not found at: {}",
                    config.backend_command
                ));
            }
            config.backend_port = 0;
        }
        Runtime::Node => {
            config.backend_command = config.backend_command.trim().to_string();
            if config.backend_command.is_empty() {
                return Err("Enter the command that starts the Node app.".into());
            }
            if config.backend_port == 0 {
                return Err("Enter the port the Node app listens on.".into());
            }
            if config.backend_port == config.port {
                return Err(
                    "The backend port must be different from the website port above it.".into(),
                );
            }
        }
    }

    if config.id.trim().is_empty() {
        loop {
            let candidate = generate_site_id();
            if !existing.iter().any(|site| site.id == candidate) {
                config.id = candidate;
                break;
            }
        }
    } else {
        config.id = config.id.trim().to_string();
        if !valid_site_id(&config.id) {
            return Err("The website ID is invalid.".into());
        }
    }

    if let Some(other) = existing
        .iter()
        .find(|site| site.id != config.id && site.port == config.port)
    {
        return Err(format!(
            "Port {} is already used by \"{}\". Pick a different port.",
            config.port, other.name
        ));
    }
    Ok(config)
}

/// Validates the main page setting. Returns the cleaned relative path using
/// `/` separators, or an empty string for the default index.html.
pub fn normalize_index_file(value: &str) -> Result<String, String> {
    let value = value.trim().trim_matches('"').trim().replace('\\', "/");
    let value = value.trim_start_matches('/');
    if value.is_empty() {
        return Ok(String::new());
    }
    let invalid = || {
        "The main page must be an .html or .htm file inside the website folder, for example home.html."
            .to_string()
    };
    if value.len() > 200 || value.chars().any(|c| c.is_control() || c == ':') {
        return Err(invalid());
    }
    let segments: Vec<&str> = value.split('/').collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(invalid());
    }
    let name = segments.last().copied().unwrap_or_default().to_ascii_lowercase();
    if !(name.ends_with(".html") || name.ends_with(".htm")) || is_blocked_file_name(&name) {
        return Err(invalid());
    }
    Ok(value.to_string())
}

fn index_path(root: &Path, index_file: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in index_file.split('/').filter(|segment| !segment.is_empty()) {
        path.push(segment);
    }
    path
}

pub fn root_has_index(root: &str, index_file: &str) -> bool {
    let root = Path::new(root);
    if !index_file.is_empty() {
        return index_path(root, index_file).is_file();
    }
    root.join("index.html").is_file() || root.join("index.htm").is_file()
}

/// Lists the .html/.htm files in a site folder (relative, `/` separated) so
/// the main page can be picked from a list.
pub fn list_html_pages(root: &str) -> Vec<String> {
    const MAX_PAGES: usize = 300;
    const MAX_DEPTH: usize = 4;
    fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        if out.len() >= MAX_PAGES {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() || name.starts_with('.') {
                continue;
            }
            let relative = format!("{prefix}{name}");
            if kind.is_dir() {
                if depth < MAX_DEPTH && name != "node_modules" {
                    walk(&entry.path(), &format!("{relative}/"), depth + 1, out);
                }
            } else if kind.is_file() {
                let lower = name.to_ascii_lowercase();
                if (lower.ends_with(".html") || lower.ends_with(".htm"))
                    && !is_blocked_file_name(&name)
                    && out.len() < MAX_PAGES
                {
                    out.push(relative);
                }
            }
        }
    }
    let mut pages = Vec::new();
    walk(Path::new(root), "", 0, &mut pages);
    pages
}

/// What `detect_site_setup` found in a website folder: XAMPP-style — point it
/// at a folder and it works out whether that's a plain static site, a PHP
/// site, or a Node app, the same way XAMPP's htdocs just runs whatever you
/// drop into it without asking.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedSiteSetup {
    pub runtime: Runtime,
    /// Populated when `runtime` is Php and a php-cgi.exe was found on this
    /// machine, so the user never has to type a path themselves.
    pub php_cgi_path: Option<String>,
    /// Populated when `runtime` is Node and a package.json start script or a
    /// server.js/index.js/app.js was found, as a ready-to-use command.
    pub node_command: Option<String>,
    /// One line explaining what was detected and why, shown under the field.
    pub message: String,
}

/// Looks for `.php` files (a couple of levels deep, skipping `node_modules`
/// and dotfolders) so a folder full of PHP doesn't need to be told it's PHP.
fn folder_has_php(root: &Path) -> bool {
    fn walk(dir: &Path, depth: usize) -> bool {
        if depth > 3 {
            return false;
        }
        let Ok(entries) = fs::read_dir(dir) else { return false };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_file() {
                if name.to_ascii_lowercase().ends_with(".php") {
                    return true;
                }
            } else if kind.is_dir() && name != "node_modules" && walk(&entry.path(), depth + 1) {
                return true;
            }
        }
        false
    }
    walk(root, 0)
}

/// A Node app declares itself with a package.json (npm start / main entry) or
/// a conventional entry file sitting right in the folder.
fn detect_node_command(root: &Path) -> Option<String> {
    let package_json = root.join("package.json");
    if let Ok(raw) = fs::read_to_string(&package_json) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            let has_start_script = value
                .get("scripts")
                .and_then(|scripts| scripts.get("start"))
                .is_some();
            if has_start_script {
                return Some("npm start".to_string());
            }
            if let Some(main) = value.get("main").and_then(|main| main.as_str()) {
                if root.join(main).is_file() {
                    return Some(format!("node {main}"));
                }
            }
        }
    }
    for candidate in ["server.js", "index.js", "app.js"] {
        if root.join(candidate).is_file() {
            return Some(format!("node {candidate}"));
        }
    }
    None
}

/// Common places a Windows PHP install leaves php-cgi.exe, checked in the
/// order XAMPP users are most likely to have it: an existing XAMPP/WAMP
/// install, a manual PHP install, then whatever `php-cgi` resolves to on PATH.
pub fn find_php_cgi() -> Option<PathBuf> {
    // Prefer the portable PHP this app downloaded for itself — it's always
    // there once installed, doesn't depend on XAMPP or any system install,
    // and won't disappear if the user uninstalls something else.
    if let Some(bundled) = super::php::bundled_php_cgi_path() {
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    let fixed_candidates = [
        r"C:\xampp\php\php-cgi.exe",
        r"C:\wamp64\bin\php",
        r"C:\wamp\bin\php",
        r"C:\php\php-cgi.exe",
        r"C:\tools\php\php-cgi.exe",
        r"C:\Program Files\PHP\php-cgi.exe",
        r"C:\Program Files (x86)\PHP\php-cgi.exe",
    ];
    for candidate in fixed_candidates {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
        // wamp installs php inside a versioned subfolder, e.g. bin\php\php8.3.1\
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                let mut versions: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|p| p.is_dir())
                    .collect();
                versions.sort();
                for version_dir in versions.into_iter().rev() {
                    let exe = version_dir.join("php-cgi.exe");
                    if exe.is_file() {
                        return Some(exe);
                    }
                }
            }
        }
    }
    // Fall back to whatever the system PATH resolves php-cgi.exe to, the way
    // XAMPP's own shortcuts do.
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let exe = dir.join("php-cgi.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// XAMPP-style auto-detection: look at what's actually in the folder and
/// work out how to serve it, instead of asking the user to choose a runtime
/// and hunt down php-cgi.exe themselves.
pub fn detect_site_setup(root: &str) -> DetectedSiteSetup {
    let root_path = Path::new(root.trim().trim_matches('"'));
    if !root_path.is_dir() {
        return DetectedSiteSetup::default();
    }
    if let Some(node_command) = detect_node_command(root_path) {
        return DetectedSiteSetup {
            runtime: Runtime::Node,
            node_command: Some(node_command),
            message: "Found a package.json / server entry point — treating this as a Node app."
                .to_string(),
            ..Default::default()
        };
    }
    if folder_has_php(root_path) {
        return match find_php_cgi() {
            Some(cgi) => DetectedSiteSetup {
                runtime: Runtime::Php,
                php_cgi_path: Some(cgi.to_string_lossy().to_string()),
                message: "Found .php files and a PHP install on this PC — .php pages will run automatically, just like XAMPP's htdocs.".to_string(),
                ..Default::default()
            },
            None => DetectedSiteSetup {
                runtime: Runtime::Php,
                php_cgi_path: None,
                message: "Found .php files but PHP isn't set up yet on this PC.".to_string(),
                ..Default::default()
            },
        };
    }
    DetectedSiteSetup {
        runtime: Runtime::Static,
        message: "No .php or Node app found — serving the folder as plain static files."
            .to_string(),
        ..Default::default()
    }
}

/// Best-effort LAN address, used only to show a shareable URL.
pub fn detect_lan_ip() -> Option<Ipv4Addr> {
    // Connecting a UDP socket sends nothing; it only asks the OS which
    // interface it would use.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Running sites
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogEntry {
    /// Unix time in milliseconds.
    pub time: u64,
    pub client: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub bytes: u64,
}

#[derive(Default)]
pub struct SiteStats {
    requests: AtomicU64,
    bytes_sent: AtomicU64,
    log: Mutex<VecDeque<RequestLogEntry>>,
}

impl SiteStats {
    fn record(&self, entry: RequestLogEntry) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(entry.bytes, Ordering::Relaxed);
        let mut log = self.log.lock().unwrap_or_else(|error| error.into_inner());
        if log.len() >= REQUEST_LOG_LIMIT {
            log.pop_front();
        }
        log.push_back(entry);
    }
}

/// Per-client abuse limits. Loopback clients (this PC, or a reverse proxy on
/// it) are exempt because every proxied visitor would share one address.
#[derive(Default)]
struct Limiter {
    clients: std::sync::Mutex<std::collections::HashMap<IpAddr, ClientState>>,
}

struct ClientState {
    connections: usize,
    window_start: Instant,
    requests: u32,
}

struct ConnectionGuard {
    limiter: Arc<Limiter>,
    key: IpAddr,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let mut clients = self.limiter.lock();
        if let Some(state) = clients.get_mut(&self.key) {
            state.connections = state.connections.saturating_sub(1);
        }
    }
}

fn limiter_key(ip: IpAddr) -> Option<IpAddr> {
    match ip.to_canonical() {
        ip if ip.is_loopback() => None,
        // One IPv6 customer usually owns a whole /64, so count the prefix.
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            Some(IpAddr::V6(octets.into()))
        }
        ip => Some(ip),
    }
}

impl Limiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<IpAddr, ClientState>> {
        self.clients.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Returns a guard when the client may open another connection.
    fn open_connection(self: &Arc<Self>, ip: IpAddr, now: Instant) -> Option<Option<ConnectionGuard>> {
        let Some(key) = limiter_key(ip) else {
            return Some(None);
        };
        let mut clients = self.lock();
        if clients.len() >= MAX_TRACKED_CLIENTS / 4 {
            clients.retain(|_, state| {
                state.connections > 0 || now.duration_since(state.window_start) < RATE_WINDOW
            });
        }
        if !clients.contains_key(&key) && clients.len() >= MAX_TRACKED_CLIENTS {
            // Table is full of active clients; the global connection cap still applies.
            return Some(None);
        }
        let state = clients.entry(key).or_insert(ClientState {
            connections: 0,
            window_start: now,
            requests: 0,
        });
        if state.connections >= MAX_CONNECTIONS_PER_IP {
            return None;
        }
        state.connections += 1;
        Some(Some(ConnectionGuard {
            limiter: self.clone(),
            key,
        }))
    }

    /// Counts one request; false means the client is over its rate limit.
    fn allow_request(&self, ip: IpAddr, now: Instant) -> bool {
        let Some(key) = limiter_key(ip) else {
            return true;
        };
        let mut clients = self.lock();
        let Some(state) = clients.get_mut(&key) else {
            return true;
        };
        if now.duration_since(state.window_start) >= RATE_WINDOW {
            state.window_start = now;
            state.requests = 0;
        }
        state.requests = state.requests.saturating_add(1);
        state.requests <= MAX_REQUESTS_PER_WINDOW
    }
}

struct SiteContext {
    limiter: Arc<Limiter>,
    root: PathBuf,
    index_file: String,
    spa_fallback: bool,
    stats: Arc<SiteStats>,
    /// Set when the site's requests are forwarded to a Node process instead
    /// of being served as static files.
    backend: Option<SocketAddr>,
    /// Set when this site runs PHP: the path to php-cgi.exe. `.php` files
    /// resolved by `route_php` are executed through it; everything else in
    /// the folder is still served as a plain static file, same as XAMPP.
    php_cgi: Option<PathBuf>,
}

impl SiteContext {
    /// Main page candidates in priority order: the chosen page, then index.html/.htm.
    fn main_pages(&self) -> Vec<PathBuf> {
        let mut pages = Vec::new();
        if !self.index_file.is_empty() {
            pages.push(index_path(&self.root, &self.index_file));
        }
        pages.push(self.root.join("index.html"));
        pages.push(self.root.join("index.htm"));
        pages
    }
}

pub struct RunningSite {
    started_at: u64,
    stats: Arc<SiteStats>,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    /// The PHP/Node process this site is proxying to, if any. Killed on stop.
    backend: Option<tokio::process::Child>,
}

impl RunningSite {
    fn signal_stop(&self) {
        let _ = self.shutdown.send(true);
    }

    async fn stop(self) {
        self.signal_stop();
        let RunningSite { task, backend, .. } = self;
        let _ = tokio::time::timeout(STOP_TIMEOUT, task).await;
        if let Some(mut child) = backend {
            let _ = child.kill().await;
        }
    }
}

/// Starts the PHP/Node command that a non-static site proxies requests to.
/// Runs from the site's own folder so relative paths in the command behave
/// the way they would if the user ran it themselves in that folder.
fn spawn_backend(command_line: &str, root: &Path) -> Result<tokio::process::Child, String> {
    let mut command = tokio::process::Command::new("cmd");
    command
        .arg("/C")
        .arg(command_line)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start \"{command_line}\": {error}"))?;
    // Drain stdout/stderr so the process never blocks on a full pipe buffer.
    // Nothing is kept; the request log already shows what came through the port.
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut { stdout }, &mut tokio::io::sink()).await;
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut { stderr }, &mut tokio::io::sink()).await;
        });
    }
    Ok(child)
}

/// Binds the configured port and starts serving. Must be called from inside a
/// Tokio runtime. Binding happens synchronously so port conflicts surface as
/// an immediate, readable error.
pub fn start_site(config: &SiteConfig) -> Result<RunningSite, String> {
    let root = fs::canonicalize(&config.root)
        .ok()
        .filter(|path| path.is_dir())
        .ok_or_else(|| format!("The website folder was not found: {}", config.root))?;

    let ip = match config.exposure {
        Exposure::Local => IpAddr::V4(Ipv4Addr::LOCALHOST),
        Exposure::Network => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    };
    let std_listener = StdTcpListener::bind(SocketAddr::new(ip, config.port))
        .map_err(|error| bind_error_message(config.port, &error))?;
    std_listener
        .set_nonblocking(true)
        .map_err(|error| format!("Failed to prepare the listening socket: {error}"))?;
    let listener = TcpListener::from_std(std_listener)
        .map_err(|error| format!("Failed to start the listening socket: {error}"))?;

    let (backend, backend_addr, php_cgi) = match config.runtime {
        Runtime::Static => (None, None, None),
        Runtime::Node => {
            let child = spawn_backend(&config.backend_command, &root)?;
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.backend_port);
            (Some(child), Some(addr), None)
        }
        Runtime::Php => {
            let cgi_path = PathBuf::from(config.backend_command.trim());
            if !cgi_path.is_file() {
                return Err(format!(
                    "php-cgi.exe was not found at: {}",
                    cgi_path.to_string_lossy()
                ));
            }
            (None, None, Some(cgi_path))
        }
    };

    let stats = Arc::new(SiteStats::default());
    let context = Arc::new(SiteContext {
        limiter: Arc::new(Limiter::default()),
        root,
        index_file: config.index_file.clone(),
        spa_fallback: config.spa_fallback,
        stats: stats.clone(),
        backend: backend_addr,
        php_cgi,
    });
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(accept_loop(listener, context, receiver));

    Ok(RunningSite {
        started_at: now_millis(),
        stats,
        shutdown,
        task,
        backend,
    })
}

fn bind_error_message(port: u16, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("Port {port} is already in use by another program. Choose a different port.")
        }
        std::io::ErrorKind::PermissionDenied => format!(
            "Windows refused access to port {port}. It may be reserved by the system; choose a different port."
        ),
        _ => format!("Could not listen on port {port}: {error}"),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RunSnapshot {
    pub started_at: u64,
    pub requests: u64,
    pub bytes_sent: u64,
}

/// Owns every running site. Cheap to share behind an `Arc`/Tauri state.
#[derive(Default)]
pub struct WebsiteHost {
    running: Mutex<HashMap<String, RunningSite>>,
}

impl WebsiteHost {
    fn sites(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunningSite>> {
        self.running
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Starts a site. Must be called from inside a Tokio runtime.
    pub fn start(&self, config: &SiteConfig) -> Result<(), String> {
        let mut sites = self.sites();
        if let Some(existing) = sites.get(&config.id) {
            if !existing.task.is_finished() {
                return Err("This website is already running.".into());
            }
            sites.remove(&config.id);
        }
        let site = start_site(config)?;
        sites.insert(config.id.clone(), site);
        Ok(())
    }

    pub async fn stop(&self, id: &str) -> bool {
        let site = self.sites().remove(id);
        match site {
            Some(site) => {
                site.stop().await;
                true
            }
            None => false,
        }
    }

    /// Synchronous shutdown signal for app exit; the process is about to end,
    /// so there is nothing to await.
    pub fn signal_stop_all(&self) {
        for site in self.sites().values() {
            site.signal_stop();
        }
    }

    pub fn snapshot(&self, id: &str) -> Option<RunSnapshot> {
        let sites = self.sites();
        let site = sites.get(id).filter(|site| !site.task.is_finished())?;
        Some(RunSnapshot {
            started_at: site.started_at,
            requests: site.stats.requests.load(Ordering::Relaxed),
            bytes_sent: site.stats.bytes_sent.load(Ordering::Relaxed),
        })
    }

    pub fn recent_requests(&self, id: &str) -> Vec<RequestLogEntry> {
        let sites = self.sites();
        let Some(site) = sites.get(id) else {
            return Vec::new();
        };
        let log = site
            .stats
            .log
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        log.iter().rev().cloned().collect()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Connection handling
// ---------------------------------------------------------------------------

async fn accept_loop(
    listener: TcpListener,
    context: Arc<SiteContext>,
    mut shutdown: watch::Receiver<bool>,
) {
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let Ok(permit) = limit.clone().try_acquire_owned() else {
                        drop(stream);
                        continue;
                    };
                    let Some(guard) = context.limiter.open_connection(peer.ip(), Instant::now()) else {
                        drop(stream);
                        continue;
                    };
                    let context = context.clone();
                    let mut stop = shutdown.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let _guard = guard;
                        tokio::select! {
                            _ = run_connection(stream, peer, context) => {}
                            _ = stop.changed() => {}
                        }
                    });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }
    // Dropping the listener here releases the port.
}

struct Request {
    method: String,
    target: String,
    http11: bool,
    connection: Option<String>,
    range: Option<String>,
    if_range: Option<String>,
    if_none_match: Option<String>,
    has_body: bool,
    /// Every header as received, name lowercased. Only used to build the CGI
    /// environment for PHP; static sites ignore it.
    headers: Vec<(String, String)>,
    content_length: Option<u64>,
    chunked: bool,
}

impl Request {
    fn keep_alive(&self) -> bool {
        let connection = self
            .connection
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        if self.http11 {
            !connection.contains("close")
        } else {
            connection.contains("keep-alive")
        }
    }
}

enum ReadError {
    Closed,
    Bad(u16),
}

/// Picks static-file serving or raw proxying for one accepted connection.
async fn run_connection(stream: TcpStream, peer: SocketAddr, context: Arc<SiteContext>) {
    match context.backend {
        Some(backend) => proxy_connection(stream, backend, context.stats.clone(), peer).await,
        None => handle_connection(stream, peer, context).await,
    }
}

/// Splices the raw connection straight through to the PHP/Node process on
/// `backend`. Headers, cookies, POST bodies, chunked responses, whatever the
/// backend sends back is passed through byte for byte — nothing here parses,
/// caches, or rewrites it, so the backend's own response is exactly what the
/// visitor gets.
async fn proxy_connection(
    mut client: TcpStream,
    backend: SocketAddr,
    stats: Arc<SiteStats>,
    peer: SocketAddr,
) {
    let _ = client.set_nodelay(true);
    let mut upstream = match connect_backend(backend).await {
        Ok(stream) => stream,
        Err(_) => {
            const BODY: &[u8] = b"The PHP/Node app isn't answering on its port yet. Give it a moment and reload.";
            let head = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                BODY.len()
            );
            let _ = client.write_all(head.as_bytes()).await;
            let _ = client.write_all(BODY).await;
            stats.record(RequestLogEntry {
                time: now_millis(),
                client: peer.ip().to_string(),
                method: "PROXY".to_string(),
                path: "-".to_string(),
                status: 502,
                bytes: 0,
            });
            return;
        }
    };
    let _ = upstream.set_nodelay(true);
    let copied = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    let bytes = copied.map(|(up, down)| up + down).unwrap_or(0);
    stats.record(RequestLogEntry {
        time: now_millis(),
        client: peer.ip().to_string(),
        method: "PROXY".to_string(),
        path: "-".to_string(),
        status: 0,
        bytes,
    });
}

/// The backend may still be starting up (php/node take a moment to bind
/// their port), so one short retry before giving up.
async fn connect_backend(addr: SocketAddr) -> std::io::Result<TcpStream> {
    match TcpStream::connect(addr).await {
        Ok(stream) => Ok(stream),
        Err(first_error) => {
            tokio::time::sleep(Duration::from_millis(300)).await;
            TcpStream::connect(addr).await.map_err(|_| first_error)
        }
    }
}

async fn handle_connection(stream: TcpStream, peer: SocketAddr, context: Arc<SiteContext>) {
    let _ = stream.set_nodelay(true);
    let (read_half, mut writer) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    for _ in 0..MAX_REQUESTS_PER_CONNECTION {
        let request = match tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut reader)).await {
            Ok(Ok(request)) => request,
            Ok(Err(ReadError::Closed)) | Err(_) => return,
            Ok(Err(ReadError::Bad(status))) => {
                let reply = Reply::error(status).close();
                let mut reply = reply;
                let _ = write_reply(&mut writer, &mut reply, false, false).await;
                return;
            }
        };

        if !context.limiter.allow_request(peer.ip(), Instant::now()) {
            let mut reply = Reply::error(429)
                .header("Retry-After", RATE_WINDOW.as_secs().to_string())
                .close();
            let _ = write_reply(&mut writer, &mut reply, false, false).await;
            context.stats.record(RequestLogEntry {
                time: now_millis(),
                client: peer.ip().to_string(),
                method: sanitize_for_log(&request.method, 16),
                path: sanitize_for_log(&request.target, 200),
                status: 429,
                bytes: 0,
            });
            return;
        }

        let head_only = request.method == "HEAD";

        // Static sites never read the body (they never needed to); PHP sites
        // need it verbatim on php-cgi's stdin, so read exactly what the
        // client declared before routing.
        let body = if context.php_cgi.is_some() && request.has_body {
            match read_body(&mut reader, &request).await {
                Ok(bytes) => bytes,
                Err(status) => {
                    let mut reply = Reply::error(status).close();
                    let _ = write_reply(&mut writer, &mut reply, false, false).await;
                    context.stats.record(RequestLogEntry {
                        time: now_millis(),
                        client: peer.ip().to_string(),
                        method: sanitize_for_log(&request.method, 16),
                        path: sanitize_for_log(&request.target, 200),
                        status,
                        bytes: 0,
                    });
                    return;
                }
            }
        } else {
            Vec::new()
        };

        let keep_alive = request.keep_alive() && (context.php_cgi.is_some() || !request.has_body);
        let mut reply = route(&context, &request, &body).await;
        let close = reply.close || !keep_alive;
        let result = write_reply(&mut writer, &mut reply, head_only, !close).await;

        let (bytes, complete) = result.unwrap_or((0, false));
        context.stats.record(RequestLogEntry {
            time: now_millis(),
            client: peer.ip().to_string(),
            method: sanitize_for_log(&request.method, 16),
            path: sanitize_for_log(&request.target, 200),
            status: reply.status,
            bytes,
        });
        if close || !complete {
            return;
        }
    }
}

fn sanitize_for_log(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

async fn read_request(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Result<Request, ReadError> {
    let mut total = 0usize;
    let mut line = Vec::new();

    // Request line. Tolerate blank lines between pipelined requests.
    let request_line = loop {
        line.clear();
        read_limited_line(reader, &mut line).await?;
        total += line.len();
        let text = trim_line(&line);
        if !text.is_empty() {
            break text.to_vec();
        }
        if total > MAX_HEADER_BYTES {
            return Err(ReadError::Bad(431));
        }
    };

    let request_line = std::str::from_utf8(&request_line).map_err(|_| ReadError::Bad(400))?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ReadError::Bad(400));
    };
    let http11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Err(ReadError::Bad(505)),
    };
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(ReadError::Bad(400));
    }
    if target.len() > MAX_TARGET_BYTES {
        return Err(ReadError::Bad(414));
    }
    if target.bytes().any(|b| b < 0x21 || b == 0x7f) {
        return Err(ReadError::Bad(400));
    }

    let mut request = Request {
        method: method.to_string(),
        target: target.to_string(),
        http11,
        connection: None,
        range: None,
        if_range: None,
        if_none_match: None,
        has_body: false,
        headers: Vec::new(),
        content_length: None,
        chunked: false,
    };

    let mut header_count = 0usize;
    loop {
        line.clear();
        read_limited_line(reader, &mut line).await?;
        total += line.len();
        if total > MAX_HEADER_BYTES {
            return Err(ReadError::Bad(431));
        }
        let text = trim_line(&line);
        if text.is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADER_COUNT {
            return Err(ReadError::Bad(431));
        }
        let text = std::str::from_utf8(text).map_err(|_| ReadError::Bad(400))?;
        let Some((name, value)) = text.split_once(':') else {
            return Err(ReadError::Bad(400));
        };
        let value = value.trim().to_string();
        let name = name.trim().to_ascii_lowercase();
        match name.as_str() {
            "connection" => request.connection = Some(value.clone()),
            "range" => request.range = Some(value.clone()),
            "if-range" => request.if_range = Some(value.clone()),
            "if-none-match" => request.if_none_match = Some(value.clone()),
            "transfer-encoding" => {
                request.has_body = true;
                request.chunked = value.to_ascii_lowercase().contains("chunked");
            }
            "content-length" => {
                let parsed = value.parse::<u64>().ok();
                if parsed.map(|n| n > 0).unwrap_or(true) {
                    request.has_body = true;
                }
                request.content_length = parsed;
            }
            _ => {}
        }
        request.headers.push((name, value));
    }
    Ok(request)
}

/// Reads one line, never buffering more than the remaining header budget.
async fn read_limited_line(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    line: &mut Vec<u8>,
) -> Result<(), ReadError> {
    let mut limited = (&mut *reader).take((MAX_HEADER_BYTES + 1) as u64);
    let read = limited
        .read_until(b'\n', line)
        .await
        .map_err(|_| ReadError::Closed)?;
    if read == 0 {
        return Err(ReadError::Closed);
    }
    if !line.ends_with(b"\n") {
        // Either the header budget ran out or the peer hung up mid-line.
        return Err(if line.len() > MAX_HEADER_BYTES {
            ReadError::Bad(431)
        } else {
            ReadError::Closed
        });
    }
    Ok(())
}

const MAX_PHP_BODY_BYTES: u64 = 32 * 1024 * 1024;

/// Reads exactly the declared request body, for PHP sites only. Chunked
/// bodies aren't decoded (rare for a form post or a JSON fetch() call) and
/// come back as 501 rather than being guessed at.
async fn read_body(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    request: &Request,
) -> Result<Vec<u8>, u16> {
    if request.chunked {
        return Err(501);
    }
    let Some(length) = request.content_length else {
        return Ok(Vec::new());
    };
    if length == 0 {
        return Ok(Vec::new());
    }
    if length > MAX_PHP_BODY_BYTES {
        return Err(413);
    }
    let mut buffer = vec![0u8; length as usize];
    match tokio::time::timeout(REQUEST_TIMEOUT, reader.read_exact(&mut buffer)).await {
        Ok(Ok(_)) => Ok(buffer),
        _ => Err(400),
    }
}

fn trim_line(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

enum Body {
    None,
    Bytes(Vec<u8>),
    File {
        file: tokio::fs::File,
        start: u64,
        len: u64,
    },
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Body,
    close: bool,
}

impl Reply {
    fn new(status: u16) -> Self {
        Reply {
            status,
            headers: Vec::new(),
            body: Body::None,
            close: false,
        }
    }

    fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn close(mut self) -> Self {
        self.close = true;
        self
    }

    fn html(status: u16, body: String) -> Self {
        let mut reply = Reply::new(status).header("Content-Type", "text/html; charset=utf-8");
        reply.body = Body::Bytes(body.into_bytes());
        reply
    }

    fn error(status: u16) -> Self {
        Reply::html(status, error_page(status, default_error_detail(status)))
    }

    fn error_with(status: u16, detail: &str) -> Self {
        Reply::html(status, error_page(status, detail))
    }
}

fn default_error_detail(status: u16) -> &'static str {
    match status {
        400 => "The request could not be understood.",
        403 => "This file is not available.",
        404 => "The page you asked for does not exist.",
        405 => "This server only answers GET and HEAD requests.",
        408 => "The request body took too long to arrive.",
        413 => "The request body is too large.",
        414 => "The address is too long.",
        416 => "The requested byte range cannot be served.",
        429 => "Too many requests. Please slow down and try again shortly.",
        431 => "The request headers are too large.",
        501 => "Chunked request bodies are not supported.",
        502 => "The backend did not send a valid response.",
        504 => "The backend took too long to respond.",
        505 => "Only HTTP/1.0 and HTTP/1.1 are supported.",
        _ => "Something went wrong.",
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn error_page(status: u16, detail: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{status} {reason}</title>\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<style>body{{font:16px system-ui,sans-serif;background:#0f1115;color:#e6e6e6;display:grid;place-items:center;min-height:100vh;margin:0}}\
main{{max-width:32rem;padding:2rem}}h1{{font-size:2.5rem;margin:0 0 .5rem}}p{{color:#9aa0aa;margin:0}}</style></head>\
<body><main><h1>{status}</h1><p>{detail}</p></main></body></html>",
        reason = reason_phrase(status)
    )
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Internal Server Error",
    }
}

/// Returns (body bytes written, connection still usable).
async fn write_reply(
    writer: &mut OwnedWriteHalf,
    reply: &mut Reply,
    head_only: bool,
    keep_alive: bool,
) -> std::io::Result<(u64, bool)> {
    let body_len = match &reply.body {
        Body::None => 0,
        Body::Bytes(bytes) => bytes.len() as u64,
        Body::File { len, .. } => *len,
    };
    let no_body_status = reply.status == 204 || reply.status == 304;

    let mut head = format!(
        "HTTP/1.1 {} {}\r\nServer: {SERVER_NAME}\r\nDate: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: {}\r\n",
        reply.status,
        reason_phrase(reply.status),
        http_date(now_millis() / 1000),
        if keep_alive { "keep-alive" } else { "close" },
    );
    for (name, value) in &reply.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    if !no_body_status {
        head.push_str(&format!("Content-Length: {body_len}\r\n"));
    }
    head.push_str("\r\n");

    timed(writer.write_all(head.as_bytes())).await?;
    if head_only || no_body_status {
        timed(writer.flush()).await?;
        return Ok((0, true));
    }

    let written = match &mut reply.body {
        Body::None => 0,
        Body::Bytes(bytes) => {
            timed(writer.write_all(bytes)).await?;
            bytes.len() as u64
        }
        Body::File { file, start, len } => {
            file.seek(std::io::SeekFrom::Start(*start)).await?;
            let mut remaining = *len;
            let mut buffer = vec![0u8; CHUNK_BYTES];
            let mut sent = 0u64;
            while remaining > 0 {
                let want = remaining.min(CHUNK_BYTES as u64) as usize;
                let read = file.read(&mut buffer[..want]).await?;
                if read == 0 {
                    break; // file shrank while sending
                }
                timed(writer.write_all(&buffer[..read])).await?;
                remaining -= read as u64;
                sent += read as u64;
            }
            sent
        }
    };
    timed(writer.flush()).await?;
    Ok((written, written == body_len))
}

async fn timed<T>(
    future: impl std::future::Future<Output = std::io::Result<T>>,
) -> std::io::Result<T> {
    match tokio::time::timeout(WRITE_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "client stopped reading",
        )),
    }
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

async fn route(context: &SiteContext, request: &Request, body: &[u8]) -> Reply {
    if let Some(php_cgi) = &context.php_cgi {
        return route_php(context, request, body, php_cgi).await;
    }
    match request.method.as_str() {
        "GET" | "HEAD" => {}
        "OPTIONS" => {
            return Reply::new(204).header("Allow", "GET, HEAD, OPTIONS");
        }
        _ => {
            return Reply::error(405)
                .header("Allow", "GET, HEAD, OPTIONS")
                .close();
        }
    }

    let target = request.target.as_str();
    let path_end = target.find(['?', '#']).unwrap_or(target.len());
    let (raw_path, query) = (&target[..path_end], &target[path_end..]);
    if !raw_path.starts_with('/') {
        return Reply::error(400).close();
    }

    let segments = match decode_segments(raw_path) {
        Ok(segments) => segments,
        Err(PathError::BadRequest) => return Reply::error(400).close(),
        Err(PathError::Forbidden) => return Reply::error(403),
    };

    let mut candidate = context.root.clone();
    for segment in &segments {
        candidate.push(segment);
    }
    let last_segment = segments.last().map(String::as_str).unwrap_or("");
    if is_blocked_file_name(last_segment) {
        return Reply::error_with(403, "This file type is never served by Website Hosting.");
    }

    match lookup(context, &candidate).await {
        Lookup::Escapes => Reply::error(403),
        Lookup::File(path) => serve_file(request, &path).await,
        Lookup::Dir(dir) => {
            if !raw_path.ends_with('/') && !segments.is_empty() {
                // Redirect so relative links inside the folder resolve. Leading
                // slashes are collapsed so this can never become "//host".
                let location = format!("/{}/{}", raw_path.trim_matches('/'), query);
                return Reply::new(301)
                    .header("Location", location)
                    .header("Content-Type", "text/html; charset=utf-8");
            }
            if segments.is_empty() {
                for page in context.main_pages() {
                    if let Lookup::File(path) = lookup(context, &page).await {
                        return serve_file(request, &path).await;
                    }
                }
            } else {
                for index in ["index.html", "index.htm"] {
                    if let Lookup::File(path) = lookup(context, &dir.join(index)).await {
                        return serve_file(request, &path).await;
                    }
                }
            }
            if segments.is_empty() {
                let message = if context.index_file.is_empty() {
                    "This site folder has no index.html yet. Add one and reload.".to_string()
                } else {
                    format!(
                        "The main page {} was not found in this site folder.",
                        html_escape(&context.index_file)
                    )
                };
                not_found(context, Some(&message)).await
            } else {
                not_found(context, None).await
            }
        }
        Lookup::Missing => {
            let wants_page = !last_segment.contains('.');
            if context.spa_fallback && wants_page {
                for page in context.main_pages() {
                    if let Lookup::File(path) = lookup(context, &page).await {
                        return serve_file(request, &path).await;
                    }
                }
            }
            not_found(context, None).await
        }
    }
}

/// Routing for a PHP site: `.php` files run through php-cgi, exactly like
/// XAMPP/Apache — everything else in the same folder is still served as a
/// plain static file, on the same port.
async fn route_php(context: &SiteContext, request: &Request, body: &[u8], php_cgi: &Path) -> Reply {
    match request.method.as_str() {
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" => {}
        "OPTIONS" => {
            return Reply::new(204).header("Allow", "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS");
        }
        _ => {
            return Reply::error(405)
                .header("Allow", "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS")
                .close();
        }
    }

    let target = request.target.as_str();
    let path_end = target.find(['?', '#']).unwrap_or(target.len());
    let (raw_path, query) = (&target[..path_end], target[path_end..].trim_start_matches('?'));
    if !raw_path.starts_with('/') {
        return Reply::error(400).close();
    }

    let segments = match decode_segments(raw_path) {
        Ok(segments) => segments,
        Err(PathError::BadRequest) => return Reply::error(400).close(),
        Err(PathError::Forbidden) => return Reply::error(403),
    };

    let mut candidate = context.root.clone();
    for segment in &segments {
        candidate.push(segment);
    }
    let last_segment = segments.last().map(String::as_str).unwrap_or("");
    if is_blocked_for_php(last_segment) {
        return Reply::error_with(403, "This file type is never served by Website Hosting.");
    }

    match lookup_for_php(context, &candidate).await {
        Lookup::Escapes => Reply::error(403),
        Lookup::File(path) => {
            if is_php_path(&path) {
                execute_php(php_cgi, context, request, &path, raw_path, query, body).await
            } else if matches!(request.method.as_str(), "GET" | "HEAD") {
                serve_file(request, &path).await
            } else {
                Reply::error(405).header("Allow", "GET, HEAD, OPTIONS").close()
            }
        }
        Lookup::Dir(dir) => {
            if !raw_path.ends_with('/') && !segments.is_empty() {
                let suffix = if query.is_empty() { String::new() } else { format!("?{query}") };
                let location = format!("/{}/{}", raw_path.trim_matches('/'), suffix);
                return Reply::new(301)
                    .header("Location", location)
                    .header("Content-Type", "text/html; charset=utf-8");
            }
            let mut candidates = Vec::new();
            if segments.is_empty() {
                if !context.index_file.is_empty() {
                    candidates.push(index_path(&context.root, &context.index_file));
                }
                candidates.push(context.root.join("index.php"));
                candidates.push(context.root.join("index.html"));
                candidates.push(context.root.join("index.htm"));
            } else {
                candidates.push(dir.join("index.php"));
                candidates.push(dir.join("index.html"));
                candidates.push(dir.join("index.htm"));
            }
            for page in candidates {
                match lookup_for_php(context, &page).await {
                    Lookup::File(path) if is_php_path(&path) => {
                        return execute_php(php_cgi, context, request, &path, raw_path, query, body).await;
                    }
                    Lookup::File(path) => return serve_file(request, &path).await,
                    _ => continue,
                }
            }
            not_found(context, None).await
        }
        Lookup::Missing => not_found(context, None).await,
    }
}

fn is_php_path(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.eq_ignore_ascii_case("php"))
        .unwrap_or(false)
}

/// Same rule as `is_blocked_file_name`, except `.php` is exactly what this
/// route is for, so it's allowed through to `execute_php` instead of being
/// treated as a leaked script source file.
fn is_blocked_for_php(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if BLOCKED_FILE_NAMES.contains(&lower.as_str()) {
        return true;
    }
    match lower.rsplit_once('.') {
        Some((_, "php")) => false,
        Some((_, extension)) => BLOCKED_EXTENSIONS.contains(&extension),
        None => false,
    }
}

async fn lookup_for_php(context: &SiteContext, candidate: &Path) -> Lookup {
    let Ok(canonical) = tokio::fs::canonicalize(candidate).await else {
        return Lookup::Missing;
    };
    if !canonical.starts_with(&context.root) {
        return Lookup::Escapes;
    }
    match tokio::fs::metadata(&canonical).await {
        Ok(metadata) if metadata.is_dir() => Lookup::Dir(canonical),
        Ok(metadata) if metadata.is_file() => {
            let name = canonical
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if is_blocked_for_php(&name) {
                Lookup::Escapes
            } else {
                Lookup::File(canonical)
            }
        }
        _ => Lookup::Missing,
    }
}

/// Runs one `.php` file through php-cgi (CGI/1.1) and turns its output into
/// a `Reply`. A fresh process per request — simple and safe, at the cost of
/// the ~tens-of-milliseconds php-cgi takes to start each time.
async fn execute_php(
    php_cgi: &Path,
    context: &SiteContext,
    request: &Request,
    script: &Path,
    raw_path: &str,
    query: &str,
    body: &[u8],
) -> Reply {
    let mut command = tokio::process::Command::new(php_cgi);
    command
        .current_dir(&context.root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env("REDIRECT_STATUS", "200")
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", if request.http11 { "HTTP/1.1" } else { "HTTP/1.0" })
        .env("SERVER_SOFTWARE", SERVER_NAME)
        .env("SERVER_NAME", "localhost")
        .env("REQUEST_METHOD", &request.method)
        .env("SCRIPT_FILENAME", script)
        .env("SCRIPT_NAME", raw_path)
        .env("QUERY_STRING", query)
        .env("DOCUMENT_ROOT", &context.root)
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("CONTENT_LENGTH", body.len().to_string());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    for (name, value) in &request.headers {
        match name.as_str() {
            "content-type" => {
                command.env("CONTENT_TYPE", value);
            }
            "content-length" => {}
            _ => {
                let key = format!("HTTP_{}", name.to_ascii_uppercase().replace('-', "_"));
                command.env(key, value);
            }
        }
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Reply::error_with(500, &format!("Could not start php-cgi: {error}"));
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if !body.is_empty() {
            let _ = stdin.write_all(body).await;
        }
        drop(stdin);
    }

    match tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await {
        Ok(Ok(output)) => parse_cgi_output(&output.stdout),
        Ok(Err(error)) => Reply::error_with(502, &format!("php-cgi failed: {error}")),
        Err(_) => Reply::error(504),
    }
}

/// Splits CGI output into its header block and body, defaulting to 200 when
/// php-cgi doesn't send a `Status:` line (the normal case).
fn parse_cgi_output(raw: &[u8]) -> Reply {
    let mut split = None;
    for i in 0..raw.len() {
        if raw[i..].starts_with(b"\r\n\r\n") {
            split = Some((i, i + 4));
            break;
        }
        if raw[i..].starts_with(b"\n\n") {
            split = Some((i, i + 2));
            break;
        }
    }
    let (header_bytes, body_bytes): (&[u8], &[u8]) = match split {
        Some((end, start)) => (&raw[..end], &raw[start..]),
        None => (&[], raw),
    };

    let mut reply = Reply::new(200);
    let mut status = 200u16;
    let mut has_content_type = false;
    for line in String::from_utf8_lossy(header_bytes).split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim().to_string();
        match name.to_ascii_lowercase().as_str() {
            "status" => {
                if let Some(code) = value
                    .split_whitespace()
                    .next()
                    .and_then(|token| token.parse::<u16>().ok())
                {
                    status = code;
                }
            }
            "content-length" | "connection" | "transfer-encoding" => {}
            "content-type" => {
                has_content_type = true;
                reply = reply.header("Content-Type", value);
            }
            _ => reply = reply.header(name.to_string(), value),
        }
    }
    if !has_content_type {
        reply = reply.header("Content-Type", "text/html; charset=utf-8");
    }
    reply.status = status;
    reply.body = Body::Bytes(body_bytes.to_vec());
    reply
}

async fn not_found(context: &SiteContext, detail: Option<&str>) -> Reply {
    if detail.is_none() {
        if let Lookup::File(path) = lookup(context, &context.root.join("404.html")).await {
            if let Ok(metadata) = tokio::fs::metadata(&path).await {
                if metadata.len() <= CUSTOM_404_MAX_BYTES {
                    if let Ok(bytes) = tokio::fs::read(&path).await {
                        let mut reply =
                            Reply::new(404).header("Content-Type", "text/html; charset=utf-8");
                        reply.body = Body::Bytes(bytes);
                        return reply;
                    }
                }
            }
        }
    }
    match detail {
        Some(detail) => Reply::error_with(404, detail),
        None => Reply::error(404),
    }
}

enum Lookup {
    File(PathBuf),
    Dir(PathBuf),
    Missing,
    /// Resolves (e.g. through a link) to somewhere outside the site folder.
    Escapes,
}

async fn lookup(context: &SiteContext, candidate: &Path) -> Lookup {
    let Ok(canonical) = tokio::fs::canonicalize(candidate).await else {
        return Lookup::Missing;
    };
    if !canonical.starts_with(&context.root) {
        return Lookup::Escapes;
    }
    match tokio::fs::metadata(&canonical).await {
        Ok(metadata) if metadata.is_dir() => Lookup::Dir(canonical),
        Ok(metadata) if metadata.is_file() => {
            let name = canonical
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if is_blocked_file_name(&name) {
                Lookup::Escapes
            } else {
                Lookup::File(canonical)
            }
        }
        _ => Lookup::Missing,
    }
}

enum PathError {
    BadRequest,
    Forbidden,
}

/// Percent-decodes the request path and validates every segment. Decoding
/// happens before splitting so `%2e%2e%2f` is treated exactly like `../`.
fn decode_segments(raw_path: &str) -> Result<Vec<String>, PathError> {
    let decoded = percent_decode(raw_path).ok_or(PathError::BadRequest)?;
    let decoded = String::from_utf8(decoded).map_err(|_| PathError::BadRequest)?;
    if decoded.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(PathError::BadRequest);
    }
    if decoded.contains('\\') {
        return Err(PathError::Forbidden);
    }

    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.contains(':') {
            return Err(PathError::Forbidden);
        }
        // Hidden files and folders (.git, .env, ...) are private.
        if segment.starts_with('.') && segment != ".well-known" {
            return Err(PathError::Forbidden);
        }
        // Windows silently strips trailing dots/spaces and treats device names
        // like CON or NUL specially in any folder.
        if segment.ends_with('.') || segment.ends_with(' ') {
            return Err(PathError::Forbidden);
        }
        let stem = segment.split('.').next().unwrap_or("").to_ascii_lowercase();
        if RESERVED_DEVICE_NAMES.contains(&stem.as_str()) {
            return Err(PathError::Forbidden);
        }
        segments.push(segment.to_string());
    }
    Ok(segments)
}

fn percent_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Some(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_blocked_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if BLOCKED_FILE_NAMES.contains(&lower.as_str()) {
        return true;
    }
    match lower.rsplit_once('.') {
        Some((_, extension)) => BLOCKED_EXTENSIONS.contains(&extension),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

async fn serve_file(request: &Request, path: &Path) -> Reply {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return Reply::error(404),
    };
    let metadata = match file.metadata().await {
        Ok(metadata) => metadata,
        Err(_) => return Reply::error(404),
    };
    let len = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let etag = format!("\"{len:x}-{modified:x}\"");
    let content_type = content_type_for(path);

    let base = |status: u16| {
        let mut reply = Reply::new(status)
            .header("Content-Type", content_type)
            .header("Accept-Ranges", "bytes")
            .header("ETag", etag.clone())
            .header("Cache-Control", "no-cache");
        if modified > 0 {
            reply = reply.header("Last-Modified", http_date(modified / 1000));
        }
        reply
    };

    if let Some(header) = &request.if_none_match {
        if etag_matches(header, &etag) {
            let mut reply = Reply::new(304)
                .header("ETag", etag.clone())
                .header("Cache-Control", "no-cache");
            if modified > 0 {
                reply = reply.header("Last-Modified", http_date(modified / 1000));
            }
            return reply;
        }
    }

    let range_applies = match &request.if_range {
        Some(value) => value.trim() == etag,
        None => true,
    };
    let range = match (&request.range, range_applies) {
        (Some(header), true) => parse_range(header, len),
        _ => RangeRequest::Full,
    };

    match range {
        RangeRequest::Unsatisfiable => {
            Reply::error(416).header("Content-Range", format!("bytes */{len}"))
        }
        RangeRequest::Partial { start, end } => {
            let mut reply = base(206).header("Content-Range", format!("bytes {start}-{end}/{len}"));
            reply.body = Body::File {
                file,
                start,
                len: end - start + 1,
            };
            reply
        }
        RangeRequest::Full => {
            let mut reply = base(200);
            reply.body = Body::File {
                file,
                start: 0,
                len,
            };
            reply
        }
    }
}

fn etag_matches(header: &str, etag: &str) -> bool {
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

#[derive(Debug, PartialEq, Eq)]
enum RangeRequest {
    Full,
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}

fn parse_range(header: &str, len: u64) -> RangeRequest {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return RangeRequest::Full;
    };
    // Multi-range responses are optional; serve the whole file instead.
    if spec.contains(',') {
        return RangeRequest::Full;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeRequest::Full;
    };
    let (first, last) = (first.trim(), last.trim());

    if first.is_empty() {
        let Ok(suffix) = last.parse::<u64>() else {
            return RangeRequest::Full;
        };
        if suffix == 0 || len == 0 {
            return RangeRequest::Unsatisfiable;
        }
        return RangeRequest::Partial {
            start: len.saturating_sub(suffix),
            end: len - 1,
        };
    }

    let Ok(start) = first.parse::<u64>() else {
        return RangeRequest::Full;
    };
    if start >= len {
        return RangeRequest::Unsatisfiable;
    }
    let end = if last.is_empty() {
        len - 1
    } else {
        match last.parse::<u64>() {
            Ok(end) => end.min(len - 1),
            Err(_) => return RangeRequest::Full,
        }
    };
    if end < start {
        return RangeRequest::Full;
    }
    RangeRequest::Partial { start, end }
}

fn content_type_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/vnd.microsoft.icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// RFC 9110 IMF-fixdate, e.g. `Sun, 06 Nov 1994 08:49:37 GMT`.
fn http_date(unix_seconds: u64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (unix_seconds / 86_400) as i64;
    let seconds = unix_seconds % 86_400;
    let weekday = ((days + 4) % 7) as usize; // 1970-01-01 was a Thursday

    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        DAYS[weekday],
        day,
        MONTHS[(month - 1) as usize],
        year,
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fxi-website-test-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn write(&self, name: &str, contents: &str) {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn as_str(&self) -> String {
            self.0.to_string_lossy().to_string()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn free_port() -> u16 {
        StdTcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn config(root: &TempDir, port: u16) -> SiteConfig {
        SiteConfig {
            id: "site-test".into(),
            name: "Test".into(),
            root: root.as_str(),
            port,
            exposure: Exposure::Local,
            index_file: String::new(),
            spa_fallback: false,
            autostart: false,
        }
    }

    struct Response {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Response {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&self.body).to_string()
        }
    }

    async fn raw(port: u16, request: &str) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut data = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut data))
            .await
            .unwrap()
            .unwrap();
        let split = data
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("response head");
        let head = String::from_utf8_lossy(&data[..split]).to_string();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_string(), value.trim().to_string()))
            .collect();
        Response {
            status,
            headers,
            body: data[split + 4..].to_vec(),
        }
    }

    async fn get(port: u16, path: &str) -> Response {
        raw(
            port,
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
        )
        .await
    }

    #[test]
    fn http_dates_match_known_values() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(http_date(1_709_164_800), "Thu, 29 Feb 2024 00:00:00 GMT");
    }

    #[test]
    fn ranges_are_parsed_conservatively() {
        assert_eq!(
            parse_range("bytes=0-9", 100),
            RangeRequest::Partial { start: 0, end: 9 }
        );
        assert_eq!(
            parse_range("bytes=90-", 100),
            RangeRequest::Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=-10", 100),
            RangeRequest::Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=50-500", 100),
            RangeRequest::Partial { start: 50, end: 99 }
        );
        assert_eq!(parse_range("bytes=100-", 100), RangeRequest::Unsatisfiable);
        assert_eq!(parse_range("bytes=-0", 100), RangeRequest::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), RangeRequest::Full);
        assert_eq!(parse_range("bytes=9-2", 100), RangeRequest::Full);
        assert_eq!(parse_range("items=0-1", 100), RangeRequest::Full);
        assert_eq!(parse_range("bytes=x-y", 100), RangeRequest::Full);
    }

    #[test]
    fn path_segments_reject_traversal_and_private_names() {
        let ok = decode_segments("/a/b%20c/./d.html").ok().unwrap();
        assert_eq!(ok, vec!["a", "b c", "d.html"]);
        for bad in [
            "/../secret",
            "/%2e%2e/secret",
            "/a/%2e%2e%2fb",
            "/a/..%5csecret",
            "/a\\b",
            "/.git/config",
            "/.env",
            "/c:/windows",
            "/file.txt::$DATA",
            "/con",
            "/NUL.txt",
            "/trailing./x",
            "/space /x",
        ] {
            assert!(
                matches!(decode_segments(bad), Err(PathError::Forbidden)),
                "accepted {bad}"
            );
        }
        assert!(matches!(
            decode_segments("/%zz"),
            Err(PathError::BadRequest)
        ));
        assert!(matches!(
            decode_segments("/a%00b"),
            Err(PathError::BadRequest)
        ));
        assert!(matches!(
            decode_segments("/%ff"),
            Err(PathError::BadRequest)
        ));
        assert!(decode_segments("/.well-known/security.txt").is_ok());
    }

    #[test]
    fn blocked_files_are_detected() {
        for name in [
            "index.php",
            "Config.PHP",
            "server.cfg",
            "dump.sql",
            "web.config",
            "a.env",
        ] {
            assert!(is_blocked_file_name(name), "{name}");
        }
        for name in ["index.html", "app.js", "logo.png", "phpinfo", "readme"] {
            assert!(!is_blocked_file_name(name), "{name}");
        }
    }

    #[test]
    fn config_validation_rejects_bad_input() {
        let dir = TempDir::new();
        let base = config(&dir, 8080);
        assert!(normalize_config(base.clone(), &[]).is_ok());

        let mut empty_name = base.clone();
        empty_name.name = "   ".into();
        assert!(normalize_config(empty_name, &[]).is_err());

        let mut missing_folder = base.clone();
        missing_folder.root = dir.0.join("nope").to_string_lossy().to_string();
        assert!(normalize_config(missing_folder, &[])
            .unwrap_err()
            .contains("not found"));

        let mut zero_port = base.clone();
        zero_port.port = 0;
        assert!(normalize_config(zero_port, &[]).is_err());

        let mut bad_id = base.clone();
        bad_id.id = "../evil".into();
        assert!(normalize_config(bad_id, &[]).is_err());

        let mut quoted = base.clone();
        quoted.root = format!("\"{}\"", dir.as_str());
        assert_eq!(normalize_config(quoted, &[]).unwrap().root, dir.as_str());

        let mut fresh = base.clone();
        fresh.id = String::new();
        fresh.port = 9090;
        let created = normalize_config(fresh, &[base.clone()]).unwrap();
        assert!(valid_site_id(&created.id));

        let mut clash = base.clone();
        clash.id = "site-other".into();
        assert!(normalize_config(clash, &[base])
            .unwrap_err()
            .contains("already used"));
    }

    #[test]
    fn settings_round_trip_and_recover_from_corruption() {
        let dir = TempDir::new();
        let file = dir.0.join("nested").join("sites.json");
        assert!(load_sites(&file).unwrap().is_empty());

        let sites = vec![config(&dir, 8081)];
        save_sites(&file, &sites).unwrap();
        assert_eq!(load_sites(&file).unwrap(), sites);

        fs::write(&file, "{not json").unwrap();
        assert!(load_sites(&file).unwrap().is_empty());
        assert!(dir.0.join("nested").join("sites.json.bad").exists());
    }

    #[tokio::test]
    async fn serves_files_indexes_and_headers() {
        let dir = TempDir::new();
        dir.write("index.html", "<h1>home</h1>");
        dir.write("about/index.html", "about page");
        dir.write("css/site.css", "body{}");
        dir.write("404.html", "custom missing");
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        let home = get(port, "/").await;
        assert_eq!(home.status, 200);
        assert_eq!(home.text(), "<h1>home</h1>");
        assert_eq!(
            home.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(home.header("x-content-type-options"), Some("nosniff"));
        assert!(home.header("etag").is_some());
        assert!(home.header("last-modified").unwrap().ends_with("GMT"));

        let css = get(port, "/css/site.css?v=2").await;
        assert_eq!(css.status, 200);
        assert_eq!(css.header("content-type"), Some("text/css; charset=utf-8"));

        let about = get(port, "/about/").await;
        assert_eq!(about.text(), "about page");

        let redirect = get(port, "/about").await;
        assert_eq!(redirect.status, 301);
        assert_eq!(redirect.header("location"), Some("/about/"));

        let open_redirect = get(port, "//about").await;
        assert_eq!(open_redirect.status, 301);
        assert_eq!(open_redirect.header("location"), Some("/about/"));

        let missing = get(port, "/nope.html").await;
        assert_eq!(missing.status, 404);
        assert_eq!(missing.text(), "custom missing");

        let head = raw(
            port,
            "HEAD /index.html HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(head.status, 200);
        assert_eq!(head.header("content-length"), Some("13"));
        assert!(head.body.is_empty());

        let snapshot = host.snapshot("site-test").unwrap();
        assert!(snapshot.requests >= 7);
        assert!(host.recent_requests("site-test")[0].status > 0);

        assert!(host.stop("site-test").await);
        assert!(host.snapshot("site-test").is_none());
    }

    #[tokio::test]
    async fn refuses_unsafe_requests() {
        let dir = TempDir::new();
        dir.write("index.html", "ok");
        dir.write(".env", "SECRET=1");
        dir.write("login.php", "<?php echo $password; ?>");
        dir.write("server.cfg", "rcon_password hunter2");
        dir.write("sub/data.sql", "drop table users;");
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        for path in [
            "/.env",
            "/login.php",
            "/server.cfg",
            "/sub/data.sql",
            "/../secret",
            "/%2e%2e/secret",
            "/%2e%2e%2f%2e%2e%2fetc/passwd",
            "/a%5c..%5cb",
        ] {
            let response = get(port, path).await;
            assert_eq!(response.status, 403, "{path}");
            assert!(!response.text().contains("hunter2"));
            assert!(!response.text().contains("SECRET"));
        }
        assert_eq!(get(port, "/a%00b").await.status, 400);

        let post = raw(
            port,
            "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd",
        )
        .await;
        assert_eq!(post.status, 405);
        assert_eq!(post.header("allow"), Some("GET, HEAD, OPTIONS"));

        assert_eq!(raw(port, "GARBAGE\r\n\r\n").await.status, 400);
        assert_eq!(raw(port, "GET / HTTP/2\r\n\r\n").await.status, 505);
        assert_eq!(
            raw(port, "GET http://evil/ HTTP/1.1\r\n\r\n").await.status,
            400
        );
        let huge = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(5000));
        assert_eq!(raw(port, &huge).await.status, 414);
        let many_headers = format!(
            "GET / HTTP/1.1\r\n{}\r\n",
            "X-A: b\r\n".repeat(MAX_HEADER_COUNT + 5)
        );
        assert_eq!(raw(port, &many_headers).await.status, 431);

        host.stop("site-test").await;
        let _ = fs::remove_file(dir.0.parent().unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_cannot_escape_the_site_folder() {
        let dir = TempDir::new();
        let outside = TempDir::new();
        outside.write("secret.txt", "top secret");
        dir.write("index.html", "ok");
        std::os::unix::fs::symlink(outside.0.join("secret.txt"), dir.0.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&outside.0, dir.0.join("linked-dir")).unwrap();
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        for path in ["/link.txt", "/linked-dir/secret.txt"] {
            let response = get(port, path).await;
            assert_eq!(response.status, 403, "{path}");
            assert!(!response.text().contains("top secret"));
        }
        host.stop("site-test").await;
    }

    #[tokio::test]
    async fn supports_conditional_and_range_requests() {
        let dir = TempDir::new();
        dir.write("data.txt", "0123456789");
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        let full = get(port, "/data.txt").await;
        let etag = full.header("etag").unwrap().to_string();
        assert_eq!(full.header("accept-ranges"), Some("bytes"));

        let not_modified = raw(
            port,
            &format!("GET /data.txt HTTP/1.1\r\nHost: x\r\nIf-None-Match: {etag}\r\nConnection: close\r\n\r\n"),
        )
        .await;
        assert_eq!(not_modified.status, 304);
        assert!(not_modified.body.is_empty());

        let partial = raw(
            port,
            "GET /data.txt HTTP/1.1\r\nHost: x\r\nRange: bytes=2-5\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(partial.status, 206);
        assert_eq!(partial.text(), "2345");
        assert_eq!(partial.header("content-range"), Some("bytes 2-5/10"));

        let suffix = raw(
            port,
            "GET /data.txt HTTP/1.1\r\nHost: x\r\nRange: bytes=-3\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(suffix.text(), "789");

        let bad = raw(
            port,
            "GET /data.txt HTTP/1.1\r\nHost: x\r\nRange: bytes=50-\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(bad.status, 416);
        assert_eq!(bad.header("content-range"), Some("bytes */10"));

        let stale_if_range = raw(
            port,
            "GET /data.txt HTTP/1.1\r\nHost: x\r\nRange: bytes=0-1\r\nIf-Range: \"old\"\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(stale_if_range.status, 200);
        assert_eq!(stale_if_range.text(), "0123456789");

        host.stop("site-test").await;
    }

    #[tokio::test]
    async fn streams_large_files_intact_and_serves_ranges_across_chunks() {
        let dir = TempDir::new();
        let payload: Vec<u8> = (0..(CHUNK_BYTES * 3 + 123))
            .map(|index| (index % 251) as u8)
            .collect();
        fs::write(dir.0.join("big.bin"), &payload).unwrap();
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        let whole = get(port, "/big.bin").await;
        assert_eq!(whole.status, 200);
        assert_eq!(whole.body, payload);

        let start = CHUNK_BYTES - 10;
        let end = CHUNK_BYTES * 2 + 10;
        let partial = raw(
            port,
            &format!(
                "GET /big.bin HTTP/1.1\r\nHost: x\r\nRange: bytes={start}-{end}\r\nConnection: close\r\n\r\n"
            ),
        )
        .await;
        assert_eq!(partial.status, 206);
        assert_eq!(partial.body, payload[start..=end].to_vec());

        let snapshot = host.snapshot("site-test").unwrap();
        assert_eq!(
            snapshot.bytes_sent,
            (payload.len() + (end - start + 1)) as u64
        );
        host.stop("site-test").await;
    }

    #[tokio::test]
    async fn keep_alive_serves_several_requests_on_one_connection() {
        let dir = TempDir::new();
        dir.write("index.html", "hello");
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let request = "GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut seen = 0;
        for _ in 0..3 {
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut buffer = vec![0u8; 4096];
            let mut collected = Vec::new();
            loop {
                let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(read > 0, "connection closed early");
                collected.extend_from_slice(&buffer[..read]);
                if collected.ends_with(b"hello") {
                    break;
                }
            }
            assert!(String::from_utf8_lossy(&collected).starts_with("HTTP/1.1 200"));
            seen += 1;
        }
        assert_eq!(seen, 3);
        host.stop("site-test").await;
    }

    #[test]
    fn main_page_setting_accepts_only_html_files_inside_the_folder() {
        assert_eq!(normalize_index_file("").unwrap(), "");
        assert_eq!(normalize_index_file(" home.html ").unwrap(), "home.html");
        assert_eq!(normalize_index_file("pages\\Main.HTM").unwrap(), "pages/Main.HTM");
        for bad in ["../x.html", "a/../x.html", "C:/x.html", "x.js", "a//b.html", "dir/", ".env", "x.html/."] {
            assert!(normalize_index_file(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn custom_main_page_is_served_at_the_root_and_for_spa_routes() {
        let dir = TempDir::new();
        dir.write("index.html", "default");
        dir.write("home.html", "custom home");
        dir.write("about.html", "about");
        let port = free_port();
        let mut settings = config(&dir, port);
        settings.index_file = "home.html".into();
        settings.spa_fallback = true;
        let host = WebsiteHost::default();
        host.start(&settings).unwrap();

        assert_eq!(get(port, "/").await.text(), "custom home");
        assert_eq!(get(port, "/about.html").await.text(), "about");
        assert_eq!(get(port, "/some/route").await.text(), "custom home");
        assert!(root_has_index(&settings.root, "home.html"));
        assert!(!root_has_index(&settings.root, "gone.html"));
        host.stop("site-test").await;
    }

    #[test]
    fn limiter_caps_connections_and_request_rate_per_client() {
        let limiter = Arc::new(Limiter::default());
        let now = Instant::now();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();

        let guards: Vec<_> = (0..MAX_CONNECTIONS_PER_IP)
            .map(|_| limiter.open_connection(ip, now).unwrap())
            .collect();
        assert!(limiter.open_connection(ip, now).is_none());
        assert!(limiter.open_connection("203.0.113.8".parse().unwrap(), now).is_some());
        drop(guards);
        assert!(limiter.open_connection(ip, now).is_some());

        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            assert!(limiter.allow_request(ip, now));
        }
        assert!(!limiter.allow_request(ip, now));
        assert!(limiter.allow_request(ip, now + RATE_WINDOW));
    }

    #[test]
    fn limiter_exempts_loopback_and_groups_ipv6_prefixes() {
        let limiter = Arc::new(Limiter::default());
        let now = Instant::now();
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let _guards: Vec<_> = (0..MAX_CONNECTIONS_PER_IP * 2)
            .map(|_| limiter.open_connection(local, now).unwrap())
            .collect();
        assert!((0..MAX_REQUESTS_PER_WINDOW * 2).all(|_| limiter.allow_request(local, now)));

        let a: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        assert_eq!(limiter_key(a), limiter_key(b));
        assert_ne!(limiter_key(a), limiter_key("2001:db8:1:3::1".parse().unwrap()));
    }

    #[test]
    fn lists_html_pages_relative_to_the_folder() {
        let dir = TempDir::new();
        dir.write("index.html", "a");
        dir.write("pages/team.htm", "b");
        dir.write("app.js", "c");
        assert_eq!(list_html_pages(&dir.as_str()), vec!["index.html", "pages/team.htm"]);
    }

    #[tokio::test]
    async fn spa_fallback_only_applies_to_page_like_paths() {
        let dir = TempDir::new();
        dir.write("index.html", "app shell");
        let port = free_port();
        let mut settings = config(&dir, port);
        settings.spa_fallback = true;
        let host = WebsiteHost::default();
        host.start(&settings).unwrap();

        let route = get(port, "/dashboard/settings").await;
        assert_eq!(route.status, 200);
        assert_eq!(route.text(), "app shell");
        assert_eq!(get(port, "/missing.js").await.status, 404);
        host.stop("site-test").await;
    }

    #[tokio::test]
    async fn empty_folder_explains_itself_and_ports_are_reported_and_released() {
        let dir = TempDir::new();
        let port = free_port();
        let host = WebsiteHost::default();
        host.start(&config(&dir, port)).unwrap();

        let empty = get(port, "/").await;
        assert_eq!(empty.status, 404);
        assert!(empty.text().contains("no index.html"));

        assert!(host
            .start(&config(&dir, port))
            .unwrap_err()
            .contains("already running"));
        let mut other = config(&dir, port);
        other.id = "site-other".into();
        assert!(host.start(&other).unwrap_err().contains("already in use"));

        assert!(host.stop("site-test").await);
        assert!(!host.stop("site-test").await);
        // The port is free again immediately after stop() returns.
        host.start(&config(&dir, port)).unwrap();
        host.stop("site-test").await;
    }

    #[tokio::test]
    async fn missing_folder_fails_to_start() {
        let dir = TempDir::new();
        let mut settings = config(&dir, free_port());
        settings.root = dir.0.join("gone").to_string_lossy().to_string();
        let error = WebsiteHost::default().start(&settings).unwrap_err();
        assert!(error.contains("not found"));
    }
}
