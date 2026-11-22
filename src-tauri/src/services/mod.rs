use crate::config::{load_config, save_config, shims_dir, workbench_home, LocalSite};
use crate::runtimes::current_platform_key;
use crate::storage::dir_size_bytes;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tar::Archive;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const BUNDLED_SERVICES_MANIFEST: &str = include_str!("../../../manifests/services.json");
const MAX_LOG_LINES: usize = 250;

#[derive(Debug, Clone, Deserialize)]
struct ServicesManifestFile {
    services: Vec<ManifestService>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestService {
    id: String,
    name: String,
    version: String,
    description: String,
    default_port: u16,
    size_hint_mb: u64,
    targets: HashMap<String, ServiceTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceTarget {
    url: String,
    format: String,
    #[serde(default)]
    strip_components: usize,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatusInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub port: u16,
    pub status: String, // "running" | "stopped"
    pub pid: Option<u32>,
    pub installed_managed: bool,
    pub host_binary: Option<String>,
    pub host_source: Option<String>,
    pub downloadable: bool,
    pub size_hint_mb: u64,
    pub disk_bytes: u64,
    pub url: Option<String>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceLogEvent {
    pub service_id: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceProgressEvent {
    pub service_id: String,
    pub stage: String,
    pub percent: u8,
}

pub struct RunningProcess {
    pub child: Child,
    pub pid: Option<u32>,
    pub port: u16,
}

#[derive(Default)]
pub struct ServiceSupervisor {
    pub running: HashMap<String, RunningProcess>,
    pub logs: HashMap<String, VecDeque<String>>,
}

impl ServiceSupervisor {
    pub fn push_log(&mut self, service_id: &str, line: String) {
        let entry = self
            .logs
            .entry(service_id.to_string())
            .or_insert_with(|| VecDeque::with_capacity(MAX_LOG_LINES));
        if entry.len() >= MAX_LOG_LINES {
            entry.pop_front();
        }
        entry.push_back(line);
    }

    pub fn get_logs(&self, service_id: &str) -> Vec<String> {
        self.logs
            .get(service_id)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }
}

pub type SharedSupervisor = Arc<Mutex<ServiceSupervisor>>;

fn load_services_manifest() -> Result<ServicesManifestFile, String> {
    serde_json::from_str(BUNDLED_SERVICES_MANIFEST)
        .map_err(|e| format!("Failed parsing bundled services manifest: {}", e))
}

pub fn service_install_dir(id: &str, version: &str) -> PathBuf {
    workbench_home()
        .join("runtimes")
        .join("services")
        .join(id)
        .join(version)
}

pub fn service_data_dir(id: &str, version: &str) -> PathBuf {
    workbench_home().join("data").join(id).join(version)
}

fn find_host_binary(names: &[&str]) -> Option<String> {
    crate::shell::find_binary_anywhere(names).map(|p| p.to_string_lossy().to_string())
}

fn host_binary_for_service(id: &str) -> Option<String> {
    match id {
        "caddy" => find_host_binary(&["caddy"]),
        "mariadb" => find_host_binary(&["mariadbd", "mysqld"]),
        "redis" => find_host_binary(&["redis-server"]),
        "apache" => find_host_binary(&["httpd", "apache2"]),
        _ => None,
    }
}

fn probe_service_version(id: &str, bin: &str) -> Option<String> {
    let args: &[&str] = match id {
        "caddy" => &["version"],
        "mariadb" | "redis" => &["--version"],
        "apache" => &["-v"],
        _ => return None,
    };
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // Extract first semver-like token (e.g. 11.8.6, 7.4.8, 2.8.4, 2.4.66)
    for token in combined.split(|c: char| c.is_whitespace() || c == '/' || c == '=' || c == '-' || c == ',') {
        let cleaned = token
            .trim_start_matches('v')
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
        let parts: Vec<&str> = cleaned.split('.').collect();
        if parts.len() >= 2
            && !parts[0].is_empty()
            && parts[0].chars().all(|c| c.is_ascii_digit())
            && !parts[1].is_empty()
            && parts[1].chars().all(|c| c.is_ascii_digit())
        {
            return Some(cleaned.to_string());
        }
    }
    None
}

#[cfg(unix)]
fn find_pid_listening_on_port(port: u16) -> Option<u32> {
    let out = std::process::Command::new("lsof")
        .args(["-ti", &format!("tcp:{}", port), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
}

#[cfg(not(unix))]
fn find_pid_listening_on_port(_port: u16) -> Option<u32> {
    None
}

#[cfg(unix)]
fn find_pid_by_process_names(names: &[&str]) -> Option<u32> {
    let current_user = std::env::var("USER").ok().filter(|u| !u.trim().is_empty());
    for name in names {
        let mut cmd = std::process::Command::new("pgrep");
        if let Some(ref user) = current_user {
            cmd.args(["-u", user]);
        }
        if let Ok(out) = cmd.args(["-x", name]).output() {
            if out.status.success() {
                if let Some(pid) = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .and_then(|l| l.trim().parse::<u32>().ok())
                {
                    return Some(pid);
                }
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn find_pid_by_process_names(_names: &[&str]) -> Option<u32> {
    None
}

fn find_external_service_pid(service_id: &str, port: u16) -> Option<u32> {
    // 1. Check exact process names first (catches services on IPv6, Unix sockets, or ports 80/443)
    let proc_names: &[&str] = match service_id {
        "mariadb" => &["mariadbd", "mysqld"],
        "redis" => &["redis-server"],
        "caddy" => &["caddy"],
        "apache" => &["httpd", "apache2"],
        _ => &[],
    };
    if !proc_names.is_empty() {
        if let Some(pid) = find_pid_by_process_names(proc_names) {
            return Some(pid);
        }
    }
    // 2. Fallback to port check
    if !check_port_available(port) {
        return find_pid_listening_on_port(port).or(Some(0));
    }
    None
}

fn managed_binary_for_service(id: &str, version: &str) -> Option<PathBuf> {
    let base = service_install_dir(id, version);
    if !base.is_dir() {
        return None;
    }
    let candidates: Vec<PathBuf> = match id {
        "caddy" => vec![
            base.join("caddy"),
            base.join("caddy.exe"),
            base.join("bin/caddy"),
        ],
        "mariadb" => vec![
            base.join("bin/mariadbd"),
            base.join("sbin/mariadbd"),
            base.join("bin/mariadbd.exe"),
            base.join("bin/mysqld"),
            base.join("sbin/mysqld"),
            base.join("bin/mysqld.exe"),
        ],
        "redis" => vec![
            base.join("redis-server"),
            base.join("redis-server.exe"),
            base.join("bin/redis-server"),
        ],
        "apache" => vec![
            base.join("bin/httpd"),
            base.join("sbin/httpd"),
            base.join("bin/httpd.exe"),
        ],
        "phpmyadmin" => vec![base.join("index.php")],
        _ => vec![],
    };
    candidates.into_iter().find(|p| p.is_file())
}

pub fn check_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub async fn list_services(sup: &SharedSupervisor) -> Result<Vec<ServiceStatusInfo>, String> {
    let manifest = load_services_manifest()?;
    let cfg = load_config()?;
    let platform = current_platform_key();

    // Prune exited processes and snapshot running PIDs & logs, then drop lock before any I/O
    let (running_pids, logs_snapshot): (HashMap<String, Option<u32>>, HashMap<String, Vec<String>>) = {
        let mut guard = sup.lock().await;
        let mut exited = Vec::new();
        for (id, proc_info) in guard.running.iter_mut() {
            if let Ok(Some(_)) = proc_info.child.try_wait() {
                exited.push(id.clone());
            }
        }
        for id in exited {
            guard.running.remove(&id);
        }

        let pids = guard
            .running
            .iter()
            .map(|(k, v)| (k.clone(), v.pid))
            .collect();
        let logs = manifest
            .services
            .iter()
            .map(|s| (s.id.clone(), guard.get_logs(&s.id)))
            .collect();
        (pids, logs)
    };

    tokio::task::spawn_blocking(move || {
        manifest
            .services
            .into_iter()
            .map(|svc| {
                let install_dir = service_install_dir(&svc.id, &svc.version);
                let installed_managed = managed_binary_for_service(&svc.id, &svc.version).is_some();
                let disk_bytes = if install_dir.is_dir() {
                    dir_size_bytes(&install_dir)
                } else {
                    0
                };
                let port = cfg
                    .service_ports
                    .get(&svc.id)
                    .copied()
                    .unwrap_or(svc.default_port);

                let (status, pid) = match running_pids.get(&svc.id) {
                    Some(&r_pid) => ("running".to_string(), r_pid),
                    None => match find_external_service_pid(&svc.id, port) {
                        Some(ext_pid) => (
                            "external_running".to_string(),
                            if ext_pid > 0 { Some(ext_pid) } else { None },
                        ),
                        None => ("stopped".to_string(), None),
                    },
                };

                let url = match svc.id.as_str() {
                    "phpmyadmin" | "caddy" | "apache" => Some(format!("http://127.0.0.1:{}", port)),
                    _ => None,
                };

                let host_binary = host_binary_for_service(&svc.id);
                let host_source = host_binary
                    .as_ref()
                    .map(|p| crate::shell::classify_source(Path::new(p)));

                let display_version = if !installed_managed {
                    host_binary
                        .as_deref()
                        .and_then(|bin| probe_service_version(&svc.id, bin))
                        .unwrap_or_else(|| svc.version.clone())
                } else {
                    svc.version.clone()
                };

                let logs = logs_snapshot.get(&svc.id).cloned().unwrap_or_default();

                ServiceStatusInfo {
                    id: svc.id.clone(),
                    name: svc.name,
                    version: display_version,
                    description: svc.description,
                    port,
                    status,
                    pid,
                    installed_managed,
                    host_binary,
                    host_source,
                    downloadable: svc.targets.contains_key(platform),
                    size_hint_mb: svc.size_hint_mb,
                    disk_bytes,
                    url,
                    logs,
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("Failed listing services: {}", e))
}

fn sanitize_rel_path(path: &Path, strip: usize) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    let mut skipped = 0usize;
    for comp in path.components() {
        match comp {
            Component::Normal(part) => {
                if skipped < strip {
                    skipped += 1;
                    continue;
                }
                clean.push(part);
            }
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn has_symlink_ancestor(dest: &Path, rel: &Path) -> bool {
    let mut cur = dest.to_path_buf();
    for comp in rel.components() {
        cur.push(comp);
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                return true;
            }
        }
    }
    false
}

fn link_target_stays_within_dest(link_target: &Path, rel_entry: &Path, is_hardlink: bool) -> bool {
    let mut depth: isize = if is_hardlink {
        0
    } else {
        rel_entry
            .parent()
            .map(|p| p.components().count() as isize)
            .unwrap_or(0)
    };
    for comp in link_target.components() {
        match comp {
            Component::RootDir | Component::Prefix(_) => return false,
            Component::CurDir => {}
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
        }
    }
    true
}

pub async fn install_service(app: &AppHandle, service_id: &str) -> Result<(), String> {
    let manifest = load_services_manifest()?;
    let platform = current_platform_key();

    let svc = manifest
        .services
        .into_iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| format!("Unknown service: {}", service_id))?;

    let target = svc
        .targets
        .get(platform)
        .ok_or_else(|| format!("No portable download for {} on {}", svc.name, platform))?
        .clone();

    if !target.url.starts_with("https://") {
        return Err(format!("Refusing non-HTTPS download URL: {}", target.url));
    }

    let final_dir = service_install_dir(&svc.id, &svc.version);
    let staging = workbench_home()
        .join("runtimes")
        .join("services")
        .join(&svc.id)
        .join(format!(".staging-{}", svc.version));
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging).map_err(|e| format!("Failed creating staging dir: {}", e))?;

    let _ = app.emit(
        "service-progress",
        ServiceProgressEvent {
            service_id: service_id.to_string(),
            stage: "downloading".to_string(),
            percent: 0,
        },
    );

    let client = reqwest::Client::builder()
        .user_agent("Workbench/0.1.0")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let extract_dir = staging.join("extracted");
    fs::create_dir_all(&extract_dir).map_err(|e| format!("Extract dir error: {}", e))?;

    if target.format == "homebrew-bottle" {
        let mut last_pct = 0u8;
        let res = crate::prebuilt::install_homebrew_bottle_tree(
            &client,
            &target.url,
            platform,
            &staging,
            &extract_dir,
            &final_dir,
            |pct, _, _| {
                if pct >= last_pct + 2 || pct >= 95 {
                    last_pct = pct;
                    let _ = app.emit(
                        "service-progress",
                        ServiceProgressEvent {
                            service_id: service_id.to_string(),
                            stage: if pct >= 96 {
                                "extracting".to_string()
                            } else {
                                "downloading".to_string()
                            },
                            percent: pct,
                        },
                    );
                }
            },
        )
        .await;

        if let Err(e) = res {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    } else {
        let resp = client
            .get(&target.url)
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        if !resp.status().is_success() {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("Download returned HTTP {}", resp.status()));
        }

        let total = resp.content_length().unwrap_or(0);
        let archive_path = staging.join(format!("download.{}", target.format));
        let mut file =
            File::create(&archive_path).map_err(|e| format!("Temp file error: {}", e))?;
        let mut stream = resp.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        let mut last_pct = 0u8;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| format!("Stream error: {}", e))?;
            hasher.update(&chunk);
            file.write_all(&chunk)
                .map_err(|e| format!("Write error: {}", e))?;
            downloaded += chunk.len() as u64;
            let pct = downloaded
                .saturating_mul(100)
                .checked_div(total)
                .map(|p| p.min(99) as u8)
                .unwrap_or(50);
            if pct >= last_pct + 3 || downloaded == total {
                last_pct = pct;
                let _ = app.emit(
                    "service-progress",
                    ServiceProgressEvent {
                        service_id: service_id.to_string(),
                        stage: "downloading".to_string(),
                        percent: pct,
                    },
                );
            }
        }
        drop(file);

        if let Some(expected_sha) = target
            .sha256
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let actual_sha: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            if !actual_sha.eq_ignore_ascii_case(expected_sha) {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!(
                    "SHA-256 checksum mismatch for {} (expected {}, got {})",
                    svc.name, expected_sha, actual_sha
                ));
            }
        }

        let _ = app.emit(
            "service-progress",
            ServiceProgressEvent {
                service_id: service_id.to_string(),
                stage: "extracting".to_string(),
                percent: 99,
            },
        );

        let fmt = target.format.clone();
        let strip = target.strip_components;
        let ap = archive_path.clone();
        let ed = extract_dir.clone();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if fmt == "tar.gz" {
                let f = File::open(&ap).map_err(|e| format!("Open tar.gz error: {}", e))?;
                let mut archive = Archive::new(GzDecoder::new(f));
                for entry_res in archive
                    .entries()
                    .map_err(|e| format!("Tar entries error: {}", e))?
                {
                    let mut entry = entry_res.map_err(|e| format!("Tar entry error: {}", e))?;
                    let entry_type = entry.header().entry_type();
                    if entry_type.is_character_special()
                        || entry_type.is_block_special()
                        || entry_type.is_fifo()
                    {
                        continue;
                    }
                    let raw = entry
                        .path()
                        .map_err(|e| format!("Tar path error: {}", e))?
                        .into_owned();
                    let Some(rel) = sanitize_rel_path(&raw, strip) else {
                        continue;
                    };
                    if entry_type.is_symlink() || entry_type.is_hard_link() {
                        let link_name = entry
                            .link_name()
                            .map_err(|e| format!("Invalid tar link target: {}", e))?;
                        let Some(link_target) = link_name else {
                            continue;
                        };
                        if !link_target_stays_within_dest(
                            &link_target,
                            &rel,
                            entry_type.is_hard_link(),
                        ) {
                            return Err(format!(
                                "Blocked unsafe tar link escaping archive root: {} -> {}",
                                rel.display(),
                                link_target.display()
                            ));
                        }
                    }
                    if let Some(parent_rel) = rel.parent() {
                        if has_symlink_ancestor(&ed, parent_rel) {
                            return Err(format!(
                                "Blocked tar extraction through symlinked parent directory: {}",
                                rel.display()
                            ));
                        }
                    }
                    let out = ed.join(&rel);
                    if let Some(p) = out.parent() {
                        fs::create_dir_all(p).map_err(|e| format!("Mkdir error: {}", e))?;
                    }
                    entry
                        .unpack(&out)
                        .map_err(|e| format!("Unpack error: {}", e))?;
                }
                Ok(())
            } else if fmt == "zip" {
                let f = File::open(&ap).map_err(|e| format!("Open zip error: {}", e))?;
                let mut zip =
                    zip::ZipArchive::new(f).map_err(|e| format!("Zip read error: {}", e))?;
                for i in 0..zip.len() {
                    let mut item = zip
                        .by_index(i)
                        .map_err(|e| format!("Zip item error: {}", e))?;
                    if let Some(mode) = item.unix_mode() {
                        if (mode & 0o170000) == 0o120000 {
                            continue;
                        }
                    }
                    let Some(enclosed) = item.enclosed_name().map(|p| p.to_path_buf()) else {
                        continue;
                    };
                    let Some(rel) = sanitize_rel_path(&enclosed, strip) else {
                        continue;
                    };
                    if let Some(parent_rel) = rel.parent() {
                        if has_symlink_ancestor(&ed, parent_rel) {
                            return Err(format!(
                                "Blocked zip extraction through symlinked parent directory: {}",
                                rel.display()
                            ));
                        }
                    }
                    let out = ed.join(&rel);
                    if item.is_dir() {
                        fs::create_dir_all(&out).map_err(|e| format!("Mkdir error: {}", e))?;
                    } else {
                        if let Some(p) = out.parent() {
                            fs::create_dir_all(p).map_err(|e| format!("Mkdir error: {}", e))?;
                        }
                        let mut out_f =
                            File::create(&out).map_err(|e| format!("Create error: {}", e))?;
                        std::io::copy(&mut item, &mut out_f)
                            .map_err(|e| format!("Copy error: {}", e))?;
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Some(mode) = item.unix_mode() {
                                let safe_mode = mode & 0o755;
                                let _ = fs::set_permissions(
                                    &out,
                                    fs::Permissions::from_mode(safe_mode),
                                );
                            }
                        }
                    }
                }
                Ok(())
            } else {
                Err(format!("Unsupported service archive format: {}", fmt))
            }
        })
        .await
        .map_err(|e| format!("Extract task error: {}", e))??;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for bin in ["caddy", "bin/mariadbd", "bin/mariadb-install-db", "redis-server"] {
            let p = extract_dir.join(bin);
            if p.is_file() {
                let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
            }
        }
    }

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir).map_err(|e| format!("Replace error: {}", e))?;
    }
    if let Some(p) = final_dir.parent() {
        fs::create_dir_all(p).map_err(|e| format!("Parent dir error: {}", e))?;
    }
    fs::rename(&extract_dir, &final_dir).map_err(|e| format!("Finalize error: {}", e))?;
    let _ = fs::remove_dir_all(&staging);

    let _ = app.emit(
        "service-progress",
        ServiceProgressEvent {
            service_id: service_id.to_string(),
            stage: "done".to_string(),
            percent: 100,
        },
    );

    Ok(())
}

// --- Pure Config Generators (Caddyfile, Apache, phpMyAdmin, Redis) ---

pub fn validate_local_site(site: &LocalSite) -> Result<(), String> {
    let domain = site.domain.trim();
    if domain.is_empty() || domain.len() > 255 {
        return Err("Site domain must be between 1 and 255 characters.".to_string());
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
    {
        return Err(format!("Invalid characters in site domain: {}", site.domain));
    }
    let host_part = domain.split(':').next().unwrap_or(domain).to_ascii_lowercase();
    if !host_part.ends_with(".test") || host_part.len() <= ".test".len() {
        return Err("Local site domains must use the .test TLD (e.g. myapp.test).".to_string());
    }
    if site.server != "caddy" && site.server != "apache" {
        return Err(format!("Unsupported web server: {}", site.server));
    }
    if !matches!(site.mode.as_str(), "php" | "proxy" | "static") {
        return Err(format!("Unsupported site mode: {}", site.mode));
    }
    if site.project_path.contains(['\n', '\r', '\0', '"', '{', '}']) {
        return Err("Project path contains invalid characters for web server config.".to_string());
    }
    if let Some(ref sub) = site.docroot_subpath {
        let trimmed = sub.trim();
        if trimmed.contains(['\n', '\r', '\0', '"', '{', '}'])
            || trimmed.starts_with('/')
            || trimmed.starts_with('\\')
            || Path::new(trimmed)
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        {
            return Err("Invalid docroot subpath.".to_string());
        }
    }
    Ok(())
}

fn escape_php_single_quoted(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(['\n', '\r', '\0'], "")
}

fn generate_random_secret_32() -> String {
    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = File::open("/dev/urandom") {
            let mut buf = [0u8; 32];
            if f.read_exact(&mut buf).is_ok() {
                hasher.update(buf);
            }
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hasher.update(now.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let random_state = std::collections::hash_map::RandomState::new();
    let h = std::hash::BuildHasher::hash_one(&random_state, now);
    hasher.update(h.to_le_bytes());
    let digest = hasher.finalize();
    digest[..16].iter().map(|b| format!("{:02x}", b)).collect()
}

fn load_or_create_pma_secret() -> String {
    let dir = workbench_home().join("data").join("phpmyadmin");
    let secret_path = dir.join(".blowfish_secret");
    if let Ok(existing) = fs::read_to_string(&secret_path) {
        let trimmed = existing.trim();
        if trimmed.len() == 32 && trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
            return trimmed.to_string();
        }
    }
    let secret = generate_random_secret_32();
    if fs::create_dir_all(&dir).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            if let Ok(mut f) = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&secret_path)
            {
                let _ = f.write_all(secret.as_bytes());
                let _ = fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o600));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = fs::write(&secret_path, &secret);
        }
    }
    secret
}

pub fn generate_caddyfile(port: u16, sites: &[LocalSite], pma_port: u16) -> String {
    let mut out = String::new();
    out.push_str("# Auto-generated by Workbench — do not edit manually\n");
    out.push_str("{\n\tauto_https disable_redirects\n}\n\n");

    let valid_caddy_sites: Vec<&LocalSite> = sites
        .iter()
        .filter(|s| s.server == "caddy" && validate_local_site(s).is_ok())
        .collect();

    let https_port = if port == 80 { 443 } else { port.saturating_add(363) };
    let lan_ip = get_local_lan_ip();
    let lan_exposed_site = valid_caddy_sites.iter().find(|s| s.expose_lan).copied();

    if let Some(lan_site) = lan_exposed_site {
        let lan_addr = match &lan_ip {
            Some(ip) => format!("http://{}:{}, :{}", ip, port, port),
            None => format!(":{}", port),
        };
        out.push_str(&format!("# LAN-exposed site: {}\n", lan_site.domain));
        out.push_str(&format!("{} {{\n", lan_addr));
        write_caddy_site_body(&mut out, lan_site);
        out.push_str("}\n\n");
    } else {
        out.push_str(&format!(
            "http://127.0.0.1:{} {{\n\trespond \"Workbench Caddy is running ({} sites linked)\" 200\n}}\n\n",
            port,
            valid_caddy_sites.len()
        ));
    }

    out.push_str(&format!(
        "http://pma.test:{} {{\n\treverse_proxy 127.0.0.1:{}\n}}\n\n",
        port, pma_port
    ));

    for site in valid_caddy_sites {
        let host = if site.domain.contains(':') {
            site.domain.clone()
        } else {
            format!("http://{}:{}", site.domain, port)
        };
        out.push_str(&format!("{} {{\n", host));
        write_caddy_site_body(&mut out, site);
        out.push_str("}\n\n");

        if site.https {
            let domain_only = site.domain.split(':').next().unwrap_or(&site.domain);
            out.push_str(&format!("https://{}:{} {{\n", domain_only, https_port));
            out.push_str("\ttls internal\n");
            write_caddy_site_body(&mut out, site);
            out.push_str("}\n\n");
        }
    }

    out
}

fn write_caddy_site_body(out: &mut String, site: &LocalSite) {
    if site.mode == "proxy" {
        let target_port = site.proxy_port.unwrap_or(5173);
        out.push_str(&format!("\treverse_proxy 127.0.0.1:{}\n", target_port));
    } else {
        let root = match &site.docroot_subpath {
            Some(sub) if !sub.trim().is_empty() => Path::new(&site.project_path).join(sub),
            _ => Path::new(&site.project_path).to_path_buf(),
        };
        out.push_str(&format!("\troot * \"{}\"\n", root.display()));
        if site.mode == "php" {
            out.push_str("\tphp_fastcgi 127.0.0.1:9000\n");
        }
        out.push_str("\tfile_server\n");
    }
}

pub fn generate_apache_conf(port: u16, sites: &[LocalSite]) -> String {
    let valid_apache_sites: Vec<&LocalSite> = sites
        .iter()
        .filter(|s| s.server == "apache" && validate_local_site(s).is_ok())
        .collect();
    let any_lan = valid_apache_sites.iter().any(|s| s.expose_lan);
    let bind_addr = if any_lan { "0.0.0.0" } else { "127.0.0.1" };
    let vhost_addr = if any_lan { "*" } else { "127.0.0.1" };

    let mut out = String::new();
    out.push_str("# Auto-generated by Workbench — Apache virtual hosts\n");
    out.push_str(&format!("Listen {}:{}\n", bind_addr, port));
    out.push_str("ServerName localhost\n\n");

    for site in valid_apache_sites {
        let root = match &site.docroot_subpath {
            Some(sub) if !sub.trim().is_empty() => Path::new(&site.project_path).join(sub),
            _ => Path::new(&site.project_path).to_path_buf(),
        };
        out.push_str(&format!("<VirtualHost {}:{}>\n", vhost_addr, port));
        out.push_str(&format!("    ServerName {}\n", site.domain));
        out.push_str(&format!("    DocumentRoot \"{}\"\n", root.display()));
        out.push_str(&format!("    <Directory \"{}\">\n", root.display()));
        out.push_str("        AllowOverride All\n");
        out.push_str("        Require all granted\n");
        out.push_str("    </Directory>\n");
        out.push_str("</VirtualHost>\n\n");
    }

    out
}

pub fn generate_phpmyadmin_config(mariadb_ports: &[(String, u16)]) -> String {
    let secret = load_or_create_pma_secret();
    let mut out = String::new();
    out.push_str("<?php\n");
    out.push_str("// Auto-generated by Workbench\n");
    out.push_str(&format!(
        "$cfg['blowfish_secret'] = '{}';\n\n",
        escape_php_single_quoted(&secret)
    ));

    let servers: Vec<(String, u16)> = if mariadb_ports.is_empty() {
        vec![("MariaDB (Local)".to_string(), 3306)]
    } else {
        mariadb_ports.to_vec()
    };

    for (idx, (label, port)) in servers.iter().enumerate() {
        let i = idx + 1;
        out.push_str(&format!(
            "$cfg['Servers'][{}]['verbose'] = '{}';\n",
            i,
            escape_php_single_quoted(label)
        ));
        out.push_str(&format!("$cfg['Servers'][{}]['host'] = '127.0.0.1';\n", i));
        out.push_str(&format!("$cfg['Servers'][{}]['port'] = '{}';\n", i, port));
        out.push_str(&format!("$cfg['Servers'][{}]['auth_type'] = 'cookie';\n", i));
        out.push_str(&format!(
            "$cfg['Servers'][{}]['AllowNoPassword'] = true;\n\n",
            i
        ));
    }

    out
}

pub async fn start_service(
    app: &AppHandle,
    sup: &SharedSupervisor,
    service_id: &str,
    override_port: Option<u16>,
) -> Result<ServiceStatusInfo, String> {
    let manifest = load_services_manifest()?;
    let svc = manifest
        .services
        .into_iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| format!("Unknown service: {}", service_id))?;

    let mut cfg = load_config()?;
    let port = override_port
        .or_else(|| cfg.service_ports.get(service_id).copied())
        .unwrap_or(svc.default_port);

    if override_port.is_some() {
        cfg.service_ports.insert(service_id.to_string(), port);
        let _ = save_config(&cfg);
    }

    {
        let guard = sup.lock().await;
        if guard.running.contains_key(service_id) {
            return Err(format!("{} is already running", svc.name));
        }
    }

    if !check_port_available(port) {
        return Err(format!(
            "Port {} is already in use by another process. Change the port or stop the conflicting process.",
            port
        ));
    }

    let data_dir = service_data_dir(&svc.id, &svc.version);
    fs::create_dir_all(&data_dir).map_err(|e| format!("Failed creating data dir: {}", e))?;

    let mut cmd = match service_id {
        "caddy" => {
            let bin = managed_binary_for_service("caddy", &svc.version)
                .or_else(|| host_binary_for_service("caddy").map(PathBuf::from))
                .ok_or_else(|| "Caddy is not installed. Click Install first.".to_string())?;
            let pma_port = cfg.service_ports.get("phpmyadmin").copied().unwrap_or(8088);
            let caddyfile = data_dir.join("Caddyfile");
            fs::write(&caddyfile, generate_caddyfile(port, &cfg.sites, pma_port))
                .map_err(|e| format!("Failed writing Caddyfile: {}", e))?;
            let mut c = Command::new(bin);
            c.args(["run", "--config"]).arg(&caddyfile);
            c
        }
        "phpmyadmin" => {
            let pma_dir = service_install_dir("phpmyadmin", &svc.version);
            if !pma_dir.join("index.php").is_file() {
                return Err("phpMyAdmin is not installed. Click Install first.".to_string());
            }
            let mariadb_port = cfg.service_ports.get("mariadb").copied().unwrap_or(3306);
            let config_inc = pma_dir.join("config.inc.php");
            fs::write(
                &config_inc,
                generate_phpmyadmin_config(&[("MariaDB".to_string(), mariadb_port)]),
            )
            .map_err(|e| format!("Failed writing phpMyAdmin config.inc.php: {}", e))?;

            // Prefer Workbench PHP shim, fallback to host php
            let shim_php = shims_dir().join(if cfg!(target_os = "windows") {
                "php.exe"
            } else {
                "php"
            });
            let php_bin = if shim_php.is_file() {
                shim_php
            } else {
                find_host_binary(&["php"])
                    .map(PathBuf::from)
                    .ok_or_else(|| {
                        "PHP is required to serve phpMyAdmin. Install PHP in Runtimes first."
                            .to_string()
                    })?
            };
            let mut c = Command::new(php_bin);
            c.args(["-S", &format!("127.0.0.1:{}", port), "-t"])
                .arg(&pma_dir);
            c
        }
        "redis" => {
            let bin = managed_binary_for_service("redis", &svc.version)
                .or_else(|| host_binary_for_service("redis").map(PathBuf::from))
                .ok_or_else(|| {
                    "Redis binary not found. Install Redis via Workbench or Homebrew/apt."
                        .to_string()
                })?;
            let conf = data_dir.join("redis.conf");
            fs::write(
                &conf,
                format!(
                    "bind 127.0.0.1\nport {}\ndir \"{}\"\nappendonly no\nsave \"\"\n",
                    port,
                    data_dir.display()
                ),
            )
            .map_err(|e| format!("Failed writing redis.conf: {}", e))?;
            let mut c = Command::new(bin);
            c.arg(&conf);
            c
        }
        "mariadb" => {
            let bin = managed_binary_for_service("mariadb", &svc.version)
                .or_else(|| host_binary_for_service("mariadb").map(PathBuf::from))
                .ok_or_else(|| {
                    "MariaDB binary not found. Install MariaDB via Workbench or Homebrew/apt."
                        .to_string()
                })?;
            let mysql_sys = data_dir.join("mysql");
            if !mysql_sys.is_dir() {
                // Initialize isolated datadir on first run
                let init_bin = bin
                    .parent()
                    .map(|p| p.join("mariadb-install-db"))
                    .filter(|p| p.is_file())
                    .or_else(|| find_host_binary(&["mariadb-install-db", "mysql_install_db"]).map(PathBuf::from));
                if let Some(init) = init_bin {
                    let _ = std::process::Command::new(init)
                        .arg(format!("--datadir={}", data_dir.display()))
                        .output();
                }
            }
            let mut c = Command::new(bin);
            c.arg(format!("--datadir={}", data_dir.display()))
                .arg(format!("--port={}", port))
                .arg("--bind-address=127.0.0.1")
                .arg("--console");
            c
        }
        "apache" => {
            let bin = managed_binary_for_service("apache", &svc.version)
                .or_else(|| host_binary_for_service("apache").map(PathBuf::from))
                .ok_or_else(|| {
                    "Apache (httpd) not found on this machine. Install via Homebrew/apt or use Caddy."
                        .to_string()
                })?;
            let conf = data_dir.join("httpd-workbench.conf");
            fs::write(&conf, generate_apache_conf(port, &cfg.sites))
                .map_err(|e| format!("Failed writing Apache config: {}", e))?;
            let mut c = Command::new(bin);
            c.args(["-D", "FOREGROUND", "-f"]).arg(&conf);
            c
        }
        _ => return Err(format!("Unsupported service: {}", service_id)),
    };

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed starting {}: {}", svc.name, e))?;
    let pid = child.id();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut guard = sup.lock().await;
        guard.push_log(
            service_id,
            format!("Started {} on 127.0.0.1:{} (PID {:?})", svc.name, port, pid),
        );
        guard.running.insert(
            service_id.to_string(),
            RunningProcess { child, pid, port },
        );
    }

    // Stream stdout & stderr into ring buffer and frontend events
    if let Some(out) = stdout {
        let sup_c = Arc::clone(sup);
        let app_c = app.clone();
        let sid = service_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(out).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                {
                    let mut g = sup_c.lock().await;
                    g.push_log(&sid, line.clone());
                }
                let _ = app_c.emit(
                    "service-log",
                    ServiceLogEvent {
                        service_id: sid.clone(),
                        line,
                    },
                );
            }
        });
    }

    if let Some(err) = stderr {
        let sup_c = Arc::clone(sup);
        let app_c = app.clone();
        let sid = service_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(err).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                {
                    let mut g = sup_c.lock().await;
                    g.push_log(&sid, line.clone());
                }
                let _ = app_c.emit(
                    "service-log",
                    ServiceLogEvent {
                        service_id: sid.clone(),
                        line,
                    },
                );
            }
        });
    }

    let list = list_services(sup).await?;
    list.into_iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| "Service vanished after start".to_string())
}

/// Gracefully terminates a supervised service (SIGTERM on Unix / polite taskkill on Windows),
/// waiting up to 5 seconds for clean database/cache flush before falling back to force-kill.
pub async fn stop_service(sup: &SharedSupervisor, service_id: &str) -> Result<(), String> {
    let mut proc_opt = {
        let mut guard = sup.lock().await;
        guard.running.remove(service_id)
    };
    if let Some(ref mut proc_info) = proc_opt {
        if let Some(pid) = proc_info.pid {
            {
                let mut guard = sup.lock().await;
                guard.push_log(
                    service_id,
                    format!("Sending graceful shutdown signal to {} (PID {})...", service_id, pid),
                );
            }
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string()])
                    .status();
            }
        }

        let graceful = tokio::time::timeout(Duration::from_secs(5), proc_info.child.wait()).await;
        let mode_msg = match graceful {
            Ok(_) => format!("Stopped {} gracefully.", service_id),
            Err(_) => {
                let _ = proc_info.child.kill().await;
                let _ = proc_info.child.wait().await;
                format!("Force-stopped {} after 5s graceful timeout.", service_id)
            }
        };
        let mut guard = sup.lock().await;
        guard.push_log(service_id, mode_msg);
        return Ok(());
    }

    // If not supervised by Workbench, check if an external process is listening on this service's port
    if let Ok(manifest) = load_services_manifest() {
        if let Some(svc) = manifest.services.into_iter().find(|s| s.id == service_id) {
            let cfg = load_config().unwrap_or_default();
            let port = cfg
                .service_ports
                .get(service_id)
                .copied()
                .unwrap_or(svc.default_port);
            if let Some(ext_pid) = find_external_service_pid(service_id, port).filter(|&p| p > 0) {
                #[cfg(unix)]
                {
                    let _ = std::process::Command::new("kill")
                        .args(["-TERM", &ext_pid.to_string()])
                        .status();
                }
                let mut guard = sup.lock().await;
                guard.push_log(
                    service_id,
                    format!(
                        "Sent graceful SIGTERM to external {} process (PID {}).",
                        svc.name, ext_pid
                    ),
                );
            }
        }
    }
    Ok(())
}

pub async fn restart_service(
    app: &AppHandle,
    sup: &SharedSupervisor,
    service_id: &str,
    port: Option<u16>,
) -> Result<ServiceStatusInfo, String> {
    stop_service(sup, service_id).await?;
    start_service(app, sup, service_id, port).await
}

pub async fn start_all_services(
    app: &AppHandle,
    sup: &SharedSupervisor,
) -> Result<Vec<String>, String> {
    let services = list_services(sup).await?;
    let mut started = Vec::new();
    let mut errors = Vec::new();

    for svc in services {
        if svc.status == "running" {
            continue;
        }
        let can_run =
            svc.installed_managed || (svc.id != "phpmyadmin" && svc.host_binary.is_some());
        if !can_run {
            continue;
        }
        match start_service(app, sup, &svc.id, None).await {
            Ok(_) => started.push(svc.id),
            Err(e) => errors.push(format!("{}: {}", svc.name, e)),
        }
    }

    if started.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(started)
}

pub async fn stop_all_services(sup: &SharedSupervisor) {
    let ids: Vec<String> = {
        let guard = sup.lock().await;
        guard.running.keys().cloned().collect()
    };
    let futs = ids.into_iter().map(|id| async move {
        let _ = stop_service(sup, &id).await;
    });
    futures_util::future::join_all(futs).await;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceLogsResponse {
    pub service_id: String,
    pub source_path: Option<String>,
    pub lines: Vec<String>,
}

fn find_external_log_file(service_id: &str) -> Option<PathBuf> {
    let mut candidate_dirs: Vec<PathBuf> = vec![workbench_home().join("logs")];

    #[cfg(target_os = "macos")]
    {
        candidate_dirs.extend([
            PathBuf::from("/Applications/ServBay/logs").join(service_id),
            PathBuf::from("/Applications/ServBay/logs"),
            PathBuf::from("/opt/homebrew/var/log"),
            PathBuf::from("/usr/local/var/log"),
        ]);
        if service_id == "mariadb" {
            candidate_dirs.push(PathBuf::from("/Applications/ServBay/db/mariadb"));
            candidate_dirs.push(PathBuf::from("/opt/homebrew/var/mysql"));
        }
        if service_id == "apache" {
            candidate_dirs.push(PathBuf::from("/Applications/ServBay/logs/apache"));
            candidate_dirs.push(PathBuf::from("/opt/homebrew/var/log/httpd"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        candidate_dirs.extend([
            PathBuf::from("/var/log").join(service_id),
            PathBuf::from("/var/log/mysql"),
            PathBuf::from("/var/log/apache2"),
            PathBuf::from("/var/log/httpd"),
            PathBuf::from("/var/log/redis"),
            PathBuf::from("/var/log/caddy"),
        ]);
    }

    let keywords: &[&str] = match service_id {
        "mariadb" => &["mariadb", "mysql", ".err"],
        "redis" => &["redis"],
        "caddy" => &["caddy"],
        "apache" => &["apache", "httpd", "error_log", "error.log"],
        "phpmyadmin" => &["phpmyadmin", "pma", "php"],
        _ => &[service_id],
    };

    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for dir in candidate_dirs {
        if !dir.is_dir() {
            continue;
        }
        // Check depth 0 and depth 1 (e.g. /Applications/ServBay/logs/mariadb/11.8/mariadb.err)
        let mut dirs_to_scan = vec![dir.clone()];
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    dirs_to_scan.push(e.path());
                }
            }
        }
        for scan_dir in dirs_to_scan {
            let Ok(entries) = fs::read_dir(&scan_dir) else { continue };
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_file() {
                    continue;
                }
                let lower = p.to_string_lossy().to_lowercase();
                let is_log_ext = lower.ends_with(".log") || lower.ends_with(".err") || lower.ends_with("error_log");
                if !is_log_ext {
                    continue;
                }
                if !keywords.iter().any(|k| lower.contains(k)) {
                    continue;
                }
                if let Ok(meta) = p.metadata() {
                    if meta.len() == 0 {
                        continue;
                    }
                    let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, p));
                    }
                }
            }
        }
    }

    best.map(|(_, p)| p)
}

fn read_tail_lines(path: &Path, max_lines: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = File::open(path) else {
        return vec![];
    };
    let Ok(meta) = file.metadata() else {
        return vec![];
    };
    // Seek directly to the last 128 KB instead of reading huge log files into RAM
    let len = meta.len();
    let tail_bytes = len.min(131_072);
    if len > tail_bytes && file.seek(SeekFrom::End(-(tail_bytes as i64))).is_err() {
        return vec![];
    }
    let mut buf = Vec::with_capacity(tail_bytes as usize);
    if file.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let skip = all.len().saturating_sub(max_lines);
    all.into_iter().skip(skip).collect()
}

pub async fn get_service_logs(
    sup: &SharedSupervisor,
    service_id: &str,
) -> Result<ServiceLogsResponse, String> {
    let in_memory = {
        let guard = sup.lock().await;
        guard.get_logs(service_id)
    };

    if !in_memory.is_empty() {
        return Ok(ServiceLogsResponse {
            service_id: service_id.to_string(),
            source_path: Some(
                workbench_home()
                    .join("logs")
                    .join(format!("{}.log", service_id))
                    .to_string_lossy()
                    .to_string(),
            ),
            lines: in_memory,
        });
    }

    let sid = service_id.to_string();
    tokio::task::spawn_blocking(move || {
        if let Some(log_file) = find_external_log_file(&sid) {
            let lines = read_tail_lines(&log_file, 200);
            ServiceLogsResponse {
                service_id: sid,
                source_path: Some(log_file.to_string_lossy().to_string()),
                lines,
            }
        } else {
            ServiceLogsResponse {
                service_id: sid,
                source_path: None,
                lines: vec![],
            }
        }
    })
    .await
    .map_err(|e| format!("Failed reading logs: {}", e))
}

pub fn add_local_site(site: LocalSite) -> Result<Vec<LocalSite>, String> {
    validate_local_site(&site)?;
    let mut cfg = load_config()?;
    // If enabling LAN exposure on this Caddy site, ensure only one site claims the LAN port
    if site.expose_lan && site.server == "caddy" {
        for existing in cfg.sites.iter_mut() {
            if existing.server == "caddy" && existing.domain != site.domain {
                existing.expose_lan = false;
            }
        }
    }
    cfg.sites.retain(|s| s.domain != site.domain);
    cfg.sites.push(site);
    save_config(&cfg)?;
    Ok(cfg.sites)
}

pub fn remove_local_site(domain: &str) -> Result<Vec<LocalSite>, String> {
    let mut cfg = load_config()?;
    cfg.sites.retain(|s| s.domain != domain);
    save_config(&cfg)?;
    Ok(cfg.sites)
}

// --- Local DNS (/etc/hosts), SSL CA Trust, and LAN IP Detection ---

const HOSTS_MARKER_START: &str = "# >>> Workbench Hosts >>>";
const HOSTS_MARKER_END: &str = "# <<< Workbench Hosts <<<";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub lan_ip: Option<String>,
    pub hosts_path: String,
    pub synced_domains: Vec<String>,
    pub missing_domains: Vec<String>,
    pub caddy_available: bool,
    pub caddy_port: u16,
    pub caddy_https_port: u16,
    pub apache_port: u16,
}

pub fn get_local_lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    let addr = sock.local_addr().ok()?;
    let ip = addr.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

pub fn hosts_file_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("/etc/hosts")
    }
}

fn strip_workbench_hosts_block(content: &str) -> String {
    let Some(start) = content.find(HOSTS_MARKER_START) else {
        return content.to_string();
    };
    let Some(end_rel) = content[start..].find(HOSTS_MARKER_END) else {
        return content.to_string();
    };
    let mut end = start + end_rel + HOSTS_MARKER_END.len();
    if content[end..].starts_with("\r\n") {
        end += 2;
    } else if content[end..].starts_with('\n') {
        end += 1;
    }
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..start]);
    out.push_str(&content[end..]);
    out
}

fn build_hosts_block(domains: &[String]) -> String {
    if domains.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(HOSTS_MARKER_START);
    out.push('\n');
    for d in domains {
        out.push_str(&format!("127.0.0.1 {}\n", d));
        out.push_str(&format!("::1 {}\n", d));
    }
    out.push_str(HOSTS_MARKER_END);
    out.push('\n');
    out
}

fn desired_hosts_domains(sites: &[LocalSite]) -> Vec<String> {
    let mut list: Vec<String> = vec!["pma.test".to_string()];
    for s in sites {
        if validate_local_site(s).is_ok() {
            let host = s.domain.split(':').next().unwrap_or(&s.domain).to_ascii_lowercase();
            if !list.contains(&host) {
                list.push(host);
            }
        }
    }
    list
}

fn find_caddy_binary() -> Option<PathBuf> {
    if let Ok(manifest) = load_services_manifest() {
        if let Some(svc) = manifest.services.into_iter().find(|s| s.id == "caddy") {
            if let Some(managed) = managed_binary_for_service("caddy", &svc.version) {
                return Some(managed);
            }
        }
    }
    host_binary_for_service("caddy").map(PathBuf::from)
}

pub fn get_network_status() -> Result<NetworkStatus, String> {
    let cfg = load_config()?;
    let hosts_path = hosts_file_path();
    let hosts_content = fs::read_to_string(&hosts_path).unwrap_or_default();

    let desired = desired_hosts_domains(&cfg.sites);
    let mut synced_domains = Vec::new();
    let mut missing_domains = Vec::new();

    for domain in desired {
        let mapped = hosts_content.lines().any(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return false;
            }
            let mut parts = trimmed.split_whitespace();
            let Some(ip) = parts.next() else { return false };
            if ip != "127.0.0.1" && ip != "::1" {
                return false;
            }
            parts.any(|h| h.eq_ignore_ascii_case(&domain))
        });
        if mapped {
            synced_domains.push(domain);
        } else {
            missing_domains.push(domain);
        }
    }

    let caddy_port = cfg.service_ports.get("caddy").copied().unwrap_or(8080);
    let caddy_https_port = if caddy_port == 80 {
        443
    } else {
        caddy_port.saturating_add(363)
    };
    let apache_port = cfg.service_ports.get("apache").copied().unwrap_or(8081);

    Ok(NetworkStatus {
        lan_ip: get_local_lan_ip(),
        hosts_path: hosts_path.to_string_lossy().to_string(),
        synced_domains,
        missing_domains,
        caddy_available: find_caddy_binary().is_some(),
        caddy_port,
        caddy_https_port,
        apache_port,
    })
}

pub fn sync_hosts_file() -> Result<NetworkStatus, String> {
    let cfg = load_config()?;
    let hosts_path = hosts_file_path();
    let existing = fs::read_to_string(&hosts_path)
        .map_err(|e| format!("Failed reading {}: {}", hosts_path.display(), e))?;

    let desired = desired_hosts_domains(&cfg.sites);
    let mut updated = strip_workbench_hosts_block(&existing);
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    let block = build_hosts_block(&desired);
    if !block.is_empty() {
        if !updated.is_empty() && !updated.ends_with("\n\n") {
            updated.push('\n');
        }
        updated.push_str(&block);
    }

    // Fast path: direct write if process already has permission
    if fs::write(&hosts_path, &updated).is_ok() {
        return get_network_status();
    }

    // Write staged hosts file inside Workbench data dir with create_new + 0600 and request native OS elevation
    let tmp_dir = workbench_home().join("data");
    fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed creating staging dir: {}", e))?;
    let unique_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let staged = tmp_dir.join(format!(".hosts-{}-{}.staged", std::process::id(), unique_ts));
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)
            .map_err(|e| format!("Failed creating staged hosts file: {}", e))?;
        f.write_all(updated.as_bytes())
            .map_err(|e| format!("Failed writing staged hosts file: {}", e))?;
    }
    #[cfg(not(unix))]
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|e| format!("Failed creating staged hosts file: {}", e))?;
        f.write_all(updated.as_bytes())
            .map_err(|e| format!("Failed writing staged hosts file: {}", e))?;
    }

    let staged_str = staged.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    {
        // Pass path via argv and use AppleScript `quoted form of` to eliminate injection risks
        let script = "on run argv\nset src to item 1 of argv\ndo shell script \"cp \" & quoted form of src & \" /etc/hosts && chmod 644 /etc/hosts && dscacheutil -flushcache && killall -HUP mDNSResponder\" with administrator privileges\nend run";
        let out = std::process::Command::new("osascript")
            .args(["-e", script, "--", &staged_str])
            .output()
            .map_err(|e| format!("Failed launching macOS elevation prompt: {}", e))?;
        let _ = fs::remove_file(&staged);
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "Administrator permission was cancelled.".to_string()
            } else {
                err
            });
        }
    }

    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("pkexec")
            .args(["cp", &staged_str, "/etc/hosts"])
            .output()
            .map_err(|e| format!("Failed launching pkexec: {}", e))?;
        let _ = fs::remove_file(&staged);
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "Administrator permission was cancelled.".to_string()
            } else {
                err
            });
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Use Copy-Item -LiteralPath via env var instead of cmd.exe string interpolation
        let ps = "Start-Process -Verb RunAs -Wait -FilePath 'powershell.exe' -ArgumentList @('-NoProfile','-Command','Copy-Item -Force -LiteralPath $env:WB_HOSTS_SRC -Destination \"C:\\Windows\\System32\\drivers\\etc\\hosts\"')";
        let out = std::process::Command::new("powershell")
            .env("WB_HOSTS_SRC", &staged_str)
            .args(["-NoProfile", "-Command", ps])
            .output()
            .map_err(|e| format!("Failed launching UAC prompt: {}", e))?;
        let _ = fs::remove_file(&staged);
        if !out.status.success() {
            return Err("Administrator permission was cancelled or failed.".to_string());
        }
    }

    get_network_status()
}

fn caddy_local_root_cert_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library/Application Support/Caddy/pki/authorities/local/root.crt")
        })
    }
    #[cfg(target_os = "linux")]
    {
        dirs::data_dir().map(|d| d.join("caddy/pki/authorities/local/root.crt"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("Caddy/pki/authorities/local/root.crt"))
    }
}

pub async fn trust_local_ca(app: &AppHandle, sup: &SharedSupervisor) -> Result<String, String> {
    let caddy = find_caddy_binary().ok_or_else(|| {
        "Caddy is required for automatic local HTTPS certificates. Install Caddy in Services first."
            .to_string()
    })?;

    let cert_path = caddy_local_root_cert_path();
    let cert_exists = cert_path.as_ref().map(|p| p.is_file()).unwrap_or(false);

    // Ensure Caddy is running if the local CA hasn't been initialized yet (or on non-macOS where `caddy trust` queries localhost:2019)
    let needs_running_caddy = !cert_exists || !cfg!(target_os = "macos");
    if needs_running_caddy && check_port_available(2019) {
        let already_supervised = {
            let guard = sup.lock().await;
            guard.running.contains_key("caddy")
        };
        if !already_supervised {
            let _ = start_service(app, sup, "caddy", None).await?;
        }
        // Give Caddy a moment to initialize its local PKI and bind :2019
        for _ in 0..15 {
            if !check_port_available(2019)
                && cert_path.as_ref().map(|p| p.is_file()).unwrap_or(true)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Trigger Caddy's local PKI generation if root.crt still doesn't exist yet
        if let Some(ref cert) = cert_path {
            if !cert.is_file() {
                if let Ok(client) = reqwest::Client::builder()
                    .timeout(Duration::from_secs(3))
                    .build()
                {
                    let _ = client.get("http://127.0.0.1:2019/pki/ca/local").send().await;
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            if cert.is_file() {
                let cert_str = cert.to_string_lossy().to_string();
                let login_keychain = dirs::home_dir()
                    .map(|h| h.join("Library/Keychains/login.keychain-db"))
                    .filter(|p| p.exists())
                    .map(|p| p.to_string_lossy().to_string());

                // Run `security add-trusted-cert` directly inside the user's interactive Aqua GUI session
                // (without `osascript ... with administrator privileges`, which blocks SecTrustSettingsSetTrustSettings
                // on macOS Sonoma/Sequoia/Tahoe with "authorization was denied since no user interaction was possible").
                let out = tokio::task::spawn_blocking(move || {
                    let mut cmd = std::process::Command::new("security");
                    cmd.args(["add-trusted-cert", "-r", "trustRoot"]);
                    if let Some(kc) = login_keychain {
                        cmd.args(["-k", &kc]);
                    }
                    cmd.arg(&cert_str);
                    cmd.output()
                })
                .await
                .map_err(|e| format!("Trust CA task failed: {}", e))?
                .map_err(|e| format!("Failed launching macOS Keychain trust prompt: {}", e))?;

                if out.status.success() {
                    return Ok("Caddy local root CA added to your macOS Login Keychain.".to_string());
                }
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                return Err(if err.is_empty() {
                    "Keychain trust authorization was cancelled.".to_string()
                } else {
                    err
                });
            }
        }
    }

    let out = tokio::task::spawn_blocking(move || std::process::Command::new(caddy).arg("trust").output())
        .await
        .map_err(|e| format!("Trust CA task failed: {}", e))?
        .map_err(|e| format!("Failed running 'caddy trust': {}", e))?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .trim()
    .to_string();

    if out.status.success() {
        Ok(if combined.is_empty() {
            "Local development root CA trusted successfully.".to_string()
        } else {
            combined
        })
    } else {
        Err(if combined.is_empty() {
            format!("'caddy trust' exited with status {}", out.status)
        } else {
            combined
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_manifest_parses_cleanly() {
        let m = load_services_manifest().expect("services.json should parse");
        assert!(m.services.iter().any(|s| s.id == "caddy"));
        assert!(m.services.iter().any(|s| s.id == "phpmyadmin"));
        assert!(m.services.iter().any(|s| s.id == "mariadb"));
        assert!(m.services.iter().any(|s| s.id == "redis"));
        assert!(m.services.iter().any(|s| s.id == "apache"));
    }

    #[test]
    fn generates_caddyfile_and_apache_and_pma_configs() {
        let sites = vec![
            LocalSite {
                domain: "shop.test".to_string(),
                project_path: "/work/shop".to_string(),
                server: "caddy".to_string(),
                mode: "php".to_string(),
                proxy_port: None,
                docroot_subpath: Some("public".to_string()),
                https: true,
                expose_lan: true,
            },
            LocalSite {
                domain: "web.test".to_string(),
                project_path: "/work/web".to_string(),
                server: "caddy".to_string(),
                mode: "proxy".to_string(),
                proxy_port: Some(5173),
                docroot_subpath: None,
                https: false,
                expose_lan: false,
            },
            LocalSite {
                domain: "legacy.test".to_string(),
                project_path: "/work/legacy".to_string(),
                server: "apache".to_string(),
                mode: "php".to_string(),
                proxy_port: None,
                docroot_subpath: Some("public".to_string()),
                https: false,
                expose_lan: false,
            },
        ];

        let caddy = generate_caddyfile(8080, &sites, 8088);
        assert!(caddy.contains("http://shop.test:8080"));
        assert!(caddy.contains("https://shop.test:8443"));
        assert!(caddy.contains("tls internal"));
        assert!(caddy.contains("php_fastcgi 127.0.0.1:9000"));
        assert!(caddy.contains("reverse_proxy 127.0.0.1:5173"));
        assert!(caddy.contains("http://pma.test:8080"));

        let apache = generate_apache_conf(8081, &sites);
        assert!(apache.contains("ServerName legacy.test"));
        assert!(apache.contains("AllowOverride All"));

        let pma = generate_phpmyadmin_config(&[
            ("MariaDB 11.4".to_string(), 3306),
            ("MariaDB 10.11".to_string(), 3307),
        ]);
        assert!(pma.contains("$cfg['Servers'][1]['port'] = '3306';"));
        assert!(pma.contains("$cfg['Servers'][2]['port'] = '3307';"));
    }
}
