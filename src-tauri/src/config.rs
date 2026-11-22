use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static CONFIG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn config_lock() -> &'static Mutex<()> {
    CONFIG_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeleteMode {
    Trash,
    #[default]
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSite {
    pub domain: String,
    pub project_path: String,
    pub server: String, // "caddy" | "apache"
    pub mode: String,   // "php" | "proxy" | "static"
    #[serde(default)]
    pub proxy_port: Option<u16>,
    #[serde(default)]
    pub docroot_subpath: Option<String>,
    #[serde(default)]
    pub https: bool,
    #[serde(default)]
    pub expose_lan: bool,
}

fn default_db_engine() -> String {
    "mariadb".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbServerProfile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_db_engine")]
    pub engine: String, // "mariadb" | "mysql" | "postgresql" | "redis"
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchConfig {
    pub scan_roots: Vec<String>,
    #[serde(default)]
    pub delete_mode: DeleteMode,
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub defaults: HashMap<String, String>,
    #[serde(default)]
    pub external_paths: HashMap<String, String>,
    #[serde(default)]
    pub service_ports: HashMap<String, u16>,
    #[serde(default)]
    pub sites: Vec<LocalSite>,
    #[serde(default = "default_db_servers")]
    pub db_servers: Vec<DbServerProfile>,
}

fn default_db_servers() -> Vec<DbServerProfile> {
    vec![DbServerProfile {
        id: "local-mariadb".to_string(),
        name: "Local MariaDB (3306)".to_string(),
        engine: "mariadb".to_string(),
        host: "127.0.0.1".to_string(),
        port: 3306,
        username: "root".to_string(),
        password: String::new(),
    }]
}

fn default_stale_days() -> u32 {
    14
}

fn default_theme() -> String {
    "system".to_string()
}

impl Default for WorkbenchConfig {
    fn default() -> Self {
        Self {
            scan_roots: Vec::new(),
            delete_mode: DeleteMode::Permanent,
            stale_days: 14,
            theme: "system".to_string(),
            defaults: HashMap::new(),
            external_paths: HashMap::new(),
            service_ports: HashMap::new(),
            sites: Vec::new(),
            db_servers: default_db_servers(),
        }
    }
}

pub fn workbench_home() -> PathBuf {
    if let Ok(custom) = std::env::var("WORKBENCH_HOME") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs::data_local_dir() {
            return local.join("Workbench");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".workbench")
}

pub fn shims_dir() -> PathBuf {
    workbench_home().join("shims")
}

pub fn config_path() -> PathBuf {
    workbench_home().join("config.toml")
}

pub fn db_path() -> PathBuf {
    workbench_home().join("workbench.db")
}

pub fn ensure_layout() -> Result<PathBuf, String> {
    let home = workbench_home();
    for sub in ["", "shims", "runtimes", "data", "logs"] {
        let dir = if sub.is_empty() {
            home.clone()
        } else {
            home.join(sub)
        };
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o700));
    }
    Ok(home)
}

pub fn load_config() -> Result<WorkbenchConfig, String> {
    ensure_layout()?;
    let path = config_path();
    if !path.exists() {
        let cfg = WorkbenchConfig::default();
        save_config(&cfg)?;
        return Ok(cfg);
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    toml::from_str(&raw).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

pub fn save_config(cfg: &WorkbenchConfig) -> Result<(), String> {
    let _guard = config_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_layout()?;
    let path = config_path();
    let content = toml::to_string_pretty(cfg)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    let tmp_path = workbench_home().join(format!(".config.toml.{}.tmp", std::process::id()));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)
            .map_err(|e| format!("Failed to open {}: {}", tmp_path.display(), e))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;
        let _ = file.sync_all();
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        fs::write(&tmp_path, content)
            .map_err(|e| format!("Failed to write {}: {}", tmp_path.display(), e))?;
    }

    fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to atomically replace {}: {}", path.display(), e)
    })
}

pub fn is_valid_dir(path: &str) -> bool {
    Path::new(path).is_dir()
}
