pub mod php_ext;

use crate::config::{load_config, save_config, shims_dir, workbench_home};
use crate::storage::dir_size_bytes;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use tar::Archive;
use tauri::{AppHandle, Emitter};
use xz2::read::XzDecoder;

const BUNDLED_MANIFEST: &str = include_str!("../../../manifests/runtimes.json");

#[derive(Debug, Clone, Deserialize)]
struct ManifestFile {
    runtimes: Vec<ManifestRuntime>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestRuntime {
    tool: String,
    version: String,
    label: String,
    channel: String,
    size_hint_mb: u64,
    targets: HashMap<String, ManifestTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestTarget {
    url: String,
    format: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    strip_components: usize,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVersionEntry {
    pub tool: String,
    pub version: String,
    pub label: String,
    pub channel: String,
    pub size_hint_mb: u64,
    pub installed: bool,
    pub is_default: bool,
    pub is_external: bool,
    pub source: Option<String>,
    pub disk_bytes: u64,
    pub available_on_platform: bool,
    pub install_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProgressEvent {
    pub tool: String,
    pub version: String,
    pub stage: String,
    pub percent: u8,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustupToolchain {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustupStatus {
    pub available: bool,
    pub toolchains: Vec<RustupToolchain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimSyncReport {
    pub shims_dir: String,
    pub mode: String,
    pub shims_created: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProjectWorkbenchFile {
    #[serde(default)]
    runtimes: HashMap<String, String>,
}

pub fn current_platform_key() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-aarch64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        "unsupported"
    }
}

fn load_manifest() -> Result<ManifestFile, String> {
    serde_json::from_str(BUNDLED_MANIFEST)
        .map_err(|e| format!("Failed to parse bundled runtimes manifest: {}", e))
}

pub fn validate_runtime_tool(tool: &str) -> Result<(), String> {
    match tool {
        "php" | "composer" | "node" | "flutter" => Ok(()),
        _ => Err(format!("Unsupported runtime tool: {}", tool)),
    }
}

pub fn validate_runtime_version(version: &str) -> Result<(), String> {
    let v = version.trim();
    if v.is_empty()
        || v.len() > 64
        || v.starts_with('.')
        || v.contains("..")
        || v.contains('/')
        || v.contains('\\')
    {
        return Err(format!("Invalid runtime version: {}", version));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(format!("Invalid characters in runtime version: {}", version));
    }
    Ok(())
}

pub fn safe_runtime_install_dir(tool: &str, version: &str) -> Result<PathBuf, String> {
    validate_runtime_tool(tool)?;
    validate_runtime_version(version)?;
    Ok(workbench_home().join("runtimes").join(tool).join(version))
}

pub fn runtime_install_dir(tool: &str, version: &str) -> PathBuf {
    workbench_home().join("runtimes").join(tool).join(version)
}

fn extract_semver_token(raw: &str) -> Option<String> {
    for token in raw.split_whitespace() {
        let cleaned = token
            .trim_start_matches('v')
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
        let parts: Vec<&str> = cleaned.split('.').collect();
        if parts.len() >= 2 && parts[0].chars().all(|c| c.is_ascii_digit()) && !parts[0].is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

pub fn list_runtime_catalog() -> Result<Vec<RuntimeVersionEntry>, String> {
    let manifest = load_manifest()?;
    let mut cfg = load_config()?;
    let platform = current_platform_key();

    let mut items: Vec<RuntimeVersionEntry> = manifest
        .runtimes
        .into_iter()
        .map(|rt| {
            let install_dir = runtime_install_dir(&rt.tool, &rt.version);
            let installed = install_dir.is_dir();
            let disk_bytes = if installed {
                dir_size_bytes(&install_dir)
            } else {
                0
            };
            let is_default = cfg
                .defaults
                .get(&rt.tool)
                .map(|d| d == &rt.version)
                .unwrap_or(false);
            let available_on_platform = rt.targets.contains_key(platform);

            RuntimeVersionEntry {
                tool: rt.tool,
                version: rt.version,
                label: rt.label,
                channel: rt.channel,
                size_hint_mb: rt.size_hint_mb,
                installed,
                is_default,
                is_external: false,
                source: if installed {
                    Some("Workbench".to_string())
                } else {
                    None
                },
                disk_bytes,
                available_on_platform,
                install_path: if installed {
                    Some(install_dir.to_string_lossy().to_string())
                } else {
                    None
                },
            }
        })
        .collect();

    // Auto-detect already-installed external runtimes (Homebrew, ServBay, Herd, FVM, System)
    // and integrate them as first-class selectable runtimes marked as External.
    let detected = crate::shell::detect_system_tools();
    let mut cfg_changed = false;
    for tool_info in detected
        .into_iter()
        .filter(|t| t.installed && matches!(t.id.as_str(), "php" | "composer" | "node" | "flutter"))
    {
        let Some(bin_path) = tool_info.binary_path else {
            continue;
        };
        let semver = tool_info
            .version
            .as_deref()
            .and_then(extract_semver_token)
            .unwrap_or_else(|| "system".to_string());
        let ext_ver = format!("ext-{}", semver);
        let map_key = format!("{}:{}", tool_info.id, ext_ver);
        if cfg.external_paths.get(&map_key).map(|s| s.as_str()) != Some(bin_path.as_str()) {
            cfg.external_paths.insert(map_key, bin_path.clone());
            cfg_changed = true;
        }
        let is_default = cfg
            .defaults
            .get(&tool_info.id)
            .map(|d| d == &ext_ver)
            .unwrap_or(false);
        let src_label = tool_info
            .source
            .clone()
            .unwrap_or_else(|| "System".to_string());

        items.insert(
            0,
            RuntimeVersionEntry {
                tool: tool_info.id.clone(),
                version: ext_ver,
                label: format!("{} {} ({})", tool_info.name, semver, src_label),
                channel: "external".to_string(),
                size_hint_mb: 0,
                installed: true,
                is_default,
                is_external: true,
                source: Some(src_label),
                disk_bytes: 0,
                available_on_platform: true,
                install_path: Some(bin_path),
            },
        );
    }
    if cfg_changed {
        let _ = save_config(&cfg);
    }

    // Also include any manually-placed or older managed versions found on disk not in the manifest
    let runtimes_root = workbench_home().join("runtimes");
    for tool in ["php", "composer", "node", "flutter"] {
        let tool_dir = runtimes_root.join(tool);
        if let Ok(entries) = fs::read_dir(&tool_dir) {
            for entry in entries.flatten() {
                if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    continue;
                }
                let ver = entry.file_name().to_string_lossy().to_string();
                if ver.starts_with('.') {
                    continue;
                }
                if items.iter().any(|i| i.tool == tool && i.version == ver) {
                    continue;
                }
                let path = entry.path();
                let is_default = cfg
                    .defaults
                    .get(tool)
                    .map(|d| d == &ver)
                    .unwrap_or(false);
                items.push(RuntimeVersionEntry {
                    tool: tool.to_string(),
                    version: ver.clone(),
                    label: format!("{} {}", tool.to_uppercase(), ver),
                    channel: "custom".to_string(),
                    size_hint_mb: 0,
                    installed: true,
                    is_default,
                    is_external: false,
                    source: Some("Workbench".to_string()),
                    disk_bytes: dir_size_bytes(&path),
                    available_on_platform: true,
                    install_path: Some(path.to_string_lossy().to_string()),
                });
            }
        }
    }

    Ok(items)
}

fn sanitize_archive_rel_path(path: &Path, strip_components: usize) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    let mut skipped = 0usize;
    for comp in path.components() {
        match comp {
            Component::Normal(part) => {
                if skipped < strip_components {
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

fn extract_tar<R: std::io::Read>(
    reader: R,
    dest: &Path,
    strip_components: usize,
) -> Result<(), String> {
    let mut archive = Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|e| format!("Failed to read tar entries: {}", e))?;

    for entry_res in entries {
        let mut entry = entry_res.map_err(|e| format!("Corrupt tar entry: {}", e))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_character_special()
            || entry_type.is_block_special()
            || entry_type.is_fifo()
        {
            continue;
        }
        let raw_path = entry
            .path()
            .map_err(|e| format!("Invalid tar path: {}", e))?
            .into_owned();
        let Some(rel) = sanitize_archive_rel_path(&raw_path, strip_components) else {
            continue;
        };
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            let link_name = entry
                .link_name()
                .map_err(|e| format!("Invalid tar link target: {}", e))?;
            let Some(target) = link_name else {
                continue;
            };
            if !link_target_stays_within_dest(&target, &rel, entry_type.is_hard_link()) {
                return Err(format!(
                    "Blocked unsafe tar link escaping archive root: {} -> {}",
                    rel.display(),
                    target.display()
                ));
            }
        }
        if let Some(parent_rel) = rel.parent() {
            if has_symlink_ancestor(dest, parent_rel) {
                return Err(format!(
                    "Blocked tar extraction through symlinked parent directory: {}",
                    rel.display()
                ));
            }
        }
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating dir {}: {}", parent.display(), e))?;
        }
        entry
            .unpack(&out_path)
            .map_err(|e| format!("Failed extracting {}: {}", out_path.display(), e))?;
    }
    Ok(())
}

fn extract_zip(archive_path: &Path, dest: &Path, strip_components: usize) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed opening zip {}: {}", archive_path.display(), e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed reading zip archive: {}", e))?;

    for i in 0..zip.len() {
        let mut item = zip
            .by_index(i)
            .map_err(|e| format!("Failed reading zip entry {}: {}", i, e))?;
        // Skip symlink entries in zip archives
        if let Some(mode) = item.unix_mode() {
            if (mode & 0o170000) == 0o120000 {
                continue;
            }
        }
        let Some(enclosed) = item.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let Some(rel) = sanitize_archive_rel_path(&enclosed, strip_components) else {
            continue;
        };
        if let Some(parent_rel) = rel.parent() {
            if has_symlink_ancestor(dest, parent_rel) {
                return Err(format!(
                    "Blocked zip extraction through symlinked parent directory: {}",
                    rel.display()
                ));
            }
        }
        let out_path = dest.join(&rel);

        if item.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed creating dir {}: {}", out_path.display(), e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed creating dir {}: {}", parent.display(), e))?;
            }
            let mut outfile = File::create(&out_path)
                .map_err(|e| format!("Failed creating {}: {}", out_path.display(), e))?;
            std::io::copy(&mut item, &mut outfile)
                .map_err(|e| format!("Failed writing {}: {}", out_path.display(), e))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = item.unix_mode() {
                    let safe_mode = mode & 0o755;
                    let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(safe_mode));
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_executable_bins(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for candidate in [
        dir.join("php"),
        dir.join("bin/php"),
        dir.join("bin/node"),
        dir.join("bin/npm"),
        dir.join("bin/npx"),
        dir.join("bin/flutter"),
        dir.join("bin/dart"),
    ] {
        if candidate.is_file() {
            let _ = fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755));
        }
    }
}

pub async fn install_runtime(
    app: &AppHandle,
    tool: &str,
    version: &str,
) -> Result<RuntimeVersionEntry, String> {
    let manifest = load_manifest()?;
    let platform = current_platform_key();

    let entry = manifest
        .runtimes
        .into_iter()
        .find(|r| r.tool == tool && r.version == version)
        .ok_or_else(|| format!("Runtime {} {} not found in manifest", tool, version))?;

    let target = entry
        .targets
        .get(platform)
        .ok_or_else(|| format!("{} {} is not available for {}", tool, version, platform))?
        .clone();

    if !target.url.starts_with("https://") {
        return Err(format!("Refusing non-HTTPS download URL: {}", target.url));
    }

    let final_dir = safe_runtime_install_dir(tool, version)?;
    let staging_dir = workbench_home()
        .join("runtimes")
        .join(tool)
        .join(format!(".staging-{}", version));
    if staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed creating staging dir: {}", e))?;

    let _ = app.emit(
        "runtime-progress",
        RuntimeProgressEvent {
            tool: tool.to_string(),
            version: version.to_string(),
            stage: "downloading".to_string(),
            percent: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
        },
    );

    let client = reqwest::Client::builder()
        .user_agent("Workbench/0.1.0")
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let extract_dir = staging_dir.join("extracted");
    fs::create_dir_all(&extract_dir)
        .map_err(|e| format!("Failed creating extract dir: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut total_bytes: u64 = 0;

    if target.format == "homebrew-bottle" {
        let mut last_percent = 0u8;
        let res = crate::prebuilt::install_homebrew_bottle_tree(
            &client,
            &target.url,
            platform,
            &staging_dir,
            &extract_dir,
            |pct, dl, tot| {
                downloaded = dl;
                total_bytes = tot;
                if pct >= last_percent + 2 || pct >= 95 {
                    last_percent = pct;
                    let _ = app.emit(
                        "runtime-progress",
                        RuntimeProgressEvent {
                            tool: tool.to_string(),
                            version: version.to_string(),
                            stage: if pct >= 96 {
                                "extracting".to_string()
                            } else {
                                "downloading".to_string()
                            },
                            percent: pct,
                            downloaded_bytes: dl,
                            total_bytes: tot,
                        },
                    );
                }
            },
        )
        .await;

        if let Err(e) = res {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(e);
        }
    } else {
        // Resolve live PHP archive URLs dynamically so point-release lag or Windows archive moves never 404
        let resolved_url = if tool == "php" && target.url.contains("dl.static-php.dev") {
            match crate::prebuilt::resolve_static_php_url(&client, &target.url, version, platform)
                .await
            {
                Ok(u) => u,
                Err(e) => {
                    let _ = fs::remove_dir_all(&staging_dir);
                    return Err(e);
                }
            }
        } else if tool == "php" && target.url.contains("windows.php.net") {
            crate::prebuilt::resolve_windows_php_url(&client, &target.url).await
        } else {
            target.url.clone()
        };

        let resp = client
            .get(&resolved_url)
            .send()
            .await
            .map_err(|e| format!("Download failed for {}: {}", resolved_url, e))?;

        if !resp.status().is_success() {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(format!(
                "Download returned HTTP {} for {}",
                resp.status(),
                resolved_url
            ));
        }

        total_bytes = resp.content_length().unwrap_or(0);
        let archive_name = target
            .filename
            .clone()
            .unwrap_or_else(|| format!("download.{}", target.format));
        let archive_path = staging_dir.join(&archive_name);

        let mut file = File::create(&archive_path)
            .map_err(|e| format!("Failed creating temp file: {}", e))?;
        let mut stream = resp.bytes_stream();
        let mut hasher = Sha256::new();
        let mut last_percent: u8 = 0;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| format!("Stream read error: {}", e))?;
            hasher.update(&chunk);
            file.write_all(&chunk)
                .map_err(|e| format!("Disk write error: {}", e))?;
            downloaded += chunk.len() as u64;

            let pct = downloaded
                .saturating_mul(100)
                .checked_div(total_bytes)
                .map(|p| p.min(99) as u8)
                .unwrap_or(50);
            if pct >= last_percent + 2 || downloaded == total_bytes {
                last_percent = pct;
                let _ = app.emit(
                    "runtime-progress",
                    RuntimeProgressEvent {
                        tool: tool.to_string(),
                        version: version.to_string(),
                        stage: "downloading".to_string(),
                        percent: pct,
                        downloaded_bytes: downloaded,
                        total_bytes,
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
                let _ = fs::remove_dir_all(&staging_dir);
                return Err(format!(
                    "SHA-256 checksum mismatch for {} {} (expected {}, got {})",
                    tool, version, expected_sha, actual_sha
                ));
            }
        }

        let _ = app.emit(
            "runtime-progress",
            RuntimeProgressEvent {
                tool: tool.to_string(),
                version: version.to_string(),
                stage: "extracting".to_string(),
                percent: 99,
                downloaded_bytes: downloaded,
                total_bytes,
            },
        );

        let fmt = target.format.clone();
        let strip = target.strip_components;
        let archive_clone = archive_path.clone();
        let extract_clone = extract_dir.clone();
        let raw_filename = target.filename.clone();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            match fmt.as_str() {
                "tar.gz" => {
                    let f = File::open(&archive_clone)
                        .map_err(|e| format!("Failed opening archive: {}", e))?;
                    extract_tar(GzDecoder::new(f), &extract_clone, strip)
                }
                "tar.xz" => {
                    let f = File::open(&archive_clone)
                        .map_err(|e| format!("Failed opening archive: {}", e))?;
                    extract_tar(XzDecoder::new(f), &extract_clone, strip)
                }
                "zip" => extract_zip(&archive_clone, &extract_clone, strip),
                "raw" => {
                    let target_name = raw_filename.unwrap_or_else(|| "binary".to_string());
                    fs::copy(&archive_clone, extract_clone.join(target_name))
                        .map_err(|e| format!("Failed copying binary: {}", e))?;
                    Ok(())
                }
                other => Err(format!("Unsupported archive format: {}", other)),
            }
        })
        .await
        .map_err(|e| format!("Extraction task failed: {}", e))??;
    }

    #[cfg(unix)]
    ensure_executable_bins(&extract_dir);

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)
            .map_err(|e| format!("Failed replacing existing version: {}", e))?;
    }
    if let Some(parent) = final_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating runtime dir: {}", e))?;
    }
    fs::rename(&extract_dir, &final_dir)
        .map_err(|e| format!("Failed finalizing runtime install: {}", e))?;
    let _ = fs::remove_dir_all(&staging_dir);

    // If no default is set for this tool yet, set this newly installed version as default
    let mut cfg = load_config()?;
    let is_default = if !cfg.defaults.contains_key(tool) {
        cfg.defaults.insert(tool.to_string(), version.to_string());
        save_config(&cfg)?;
        true
    } else {
        cfg.defaults.get(tool).map(|v| v == version).unwrap_or(false)
    };

    let _ = sync_shims();

    let _ = app.emit(
        "runtime-progress",
        RuntimeProgressEvent {
            tool: tool.to_string(),
            version: version.to_string(),
            stage: "done".to_string(),
            percent: 100,
            downloaded_bytes: downloaded,
            total_bytes,
        },
    );

    Ok(RuntimeVersionEntry {
        tool: tool.to_string(),
        version: version.to_string(),
        label: entry.label,
        channel: entry.channel,
        size_hint_mb: entry.size_hint_mb,
        installed: true,
        is_default,
        is_external: false,
        source: Some("Workbench".to_string()),
        disk_bytes: dir_size_bytes(&final_dir),
        available_on_platform: true,
        install_path: Some(final_dir.to_string_lossy().to_string()),
    })
}

pub fn uninstall_runtime(tool: &str, version: &str) -> Result<u64, String> {
    if version.starts_with("ext-") {
        return Err("External runtimes are managed outside Workbench and cannot be uninstalled here.".to_string());
    }
    let dir = safe_runtime_install_dir(tool, version)?;
    if !dir.is_dir() {
        return Ok(0);
    }
    let runtimes_root = workbench_home().join("runtimes");
    let canonical_root = fs::canonicalize(&runtimes_root)
        .map_err(|e| format!("Failed canonicalizing runtimes dir: {}", e))?;
    let canonical_dir = fs::canonicalize(&dir)
        .map_err(|e| format!("Failed canonicalizing runtime dir: {}", e))?;
    if !canonical_dir.starts_with(&canonical_root) || canonical_dir == canonical_root {
        return Err("Refusing to remove directory outside Workbench runtimes.".to_string());
    }
    let bytes = dir_size_bytes(&canonical_dir);
    fs::remove_dir_all(&canonical_dir)
        .map_err(|e| format!("Failed removing {} {}: {}", tool, version, e))?;

    let mut cfg = load_config()?;
    if cfg.defaults.get(tool).map(|d| d == version).unwrap_or(false) {
        cfg.defaults.remove(tool);
        // Promote another installed version if available
        let tool_dir = workbench_home().join("runtimes").join(tool);
        if let Ok(entries) = fs::read_dir(&tool_dir) {
            if let Some(next_ver) = entries
                .flatten()
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .find(|v| !v.starts_with('.'))
            {
                cfg.defaults.insert(tool.to_string(), next_ver);
            }
        }
        save_config(&cfg)?;
    }

    let _ = sync_shims();
    Ok(bytes)
}

pub fn set_default_runtime(tool: &str, version: &str) -> Result<(), String> {
    validate_runtime_tool(tool)?;
    validate_runtime_version(version)?;
    let mut cfg = load_config()?;
    if version.starts_with("ext-") {
        let key = format!("{}:{}", tool, version);
        if !cfg.external_paths.contains_key(&key) {
            return Err(format!("External runtime {} {} was not found", tool, version));
        }
    } else {
        let dir = safe_runtime_install_dir(tool, version)?;
        if !dir.is_dir() {
            return Err(format!("{} {} is not installed", tool, version));
        }
    }
    cfg.defaults.insert(tool.to_string(), version.to_string());
    save_config(&cfg)?;
    let _ = sync_shims();
    Ok(())
}

fn locate_shim_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(target_os = "windows") {
        "workbench-shim.exe"
    } else {
        "workbench-shim"
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join(bin_name);
            if beside.is_file() {
                return Some(beside);
            }
            // In dev mode, current_exe is in target/debug/workbench -> target/debug/workbench-shim
            if let Some(parent) = dir.parent() {
                let candidate = parent.join(bin_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

pub fn sync_shims() -> Result<ShimSyncReport, String> {
    let sdir = shims_dir();
    fs::create_dir_all(&sdir).map_err(|e| format!("Failed creating shims dir: {}", e))?;

    let shim_names = ["php", "composer", "node", "npm", "npx", "corepack", "flutter", "dart"];
    let mut created = Vec::new();

    if let Some(shim_bin) = locate_shim_binary() {
        let master = sdir.join(if cfg!(target_os = "windows") {
            "workbench-shim.exe"
        } else {
            "workbench-shim"
        });
        fs::copy(&shim_bin, &master)
            .map_err(|e| format!("Failed copying workbench-shim binary: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&master, fs::Permissions::from_mode(0o755));
        }

        for name in shim_names {
            #[cfg(target_os = "windows")]
            {
                let target = sdir.join(format!("{}.exe", name));
                let _ = fs::copy(&master, &target);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let target = sdir.join(name);
                if target.exists() || target.symlink_metadata().is_ok() {
                    let _ = fs::remove_file(&target);
                }
                std::os::unix::fs::symlink(&master, &target)
                    .map_err(|e| format!("Failed creating shim symlink {}: {}", name, e))?;
            }
            created.push(name.to_string());
        }

        return Ok(ShimSyncReport {
            shims_dir: sdir.to_string_lossy().to_string(),
            mode: "native-rust-shim".to_string(),
            shims_created: created,
        });
    }

    // Lightweight fallback wrapper scripts if native shim binary isn't built alongside yet
    let cfg = load_config()?;
    let wb_home = workbench_home();

    for name in shim_names {
        let tool = match name {
            "php" => "php",
            "composer" => "composer",
            "node" | "npm" | "npx" | "corepack" => "node",
            "flutter" | "dart" => "flutter",
            _ => continue,
        };
        let Some(ver) = cfg.defaults.get(tool) else {
            continue;
        };
        if validate_runtime_version(ver).is_err() {
            continue;
        }
        let base = wb_home.join("runtimes").join(tool).join(ver);
        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let sq = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
            let script = if name == "composer" {
                let phar = base.join("composer.phar");
                format!(
                    "#!/usr/bin/env sh\nexec {} {} \"$@\"\n",
                    sq(&sdir.join("php")),
                    sq(&phar)
                )
            } else {
                let bin = if base.join("bin").join(name).is_file() {
                    base.join("bin").join(name)
                } else {
                    base.join(name)
                };
                format!("#!/usr/bin/env sh\nexec {} \"$@\"\n", sq(&bin))
            };
            let target = sdir.join(name);
            fs::write(&target, script)
                .map_err(|e| format!("Failed writing shim {}: {}", name, e))?;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(0o755));
            created.push(name.to_string());
        }
        #[cfg(target_os = "windows")]
        {
            let script = if name == "composer" {
                let phar = base.join("composer.phar");
                format!("@echo off\r\n\"{}\\php.cmd\" \"{}\" %*\r\n", sdir.display(), phar.display())
            } else {
                let bin = base.join(format!("{}.exe", name));
                format!("@echo off\r\n\"{}\" %*\r\n", bin.display())
            };
            let target = sdir.join(format!("{}.cmd", name));
            fs::write(&target, script)
                .map_err(|e| format!("Failed writing shim {}: {}", name, e))?;
            created.push(name.to_string());
        }
    }

    Ok(ShimSyncReport {
        shims_dir: sdir.to_string_lossy().to_string(),
        mode: "wrapper-script".to_string(),
        shims_created: created,
    })
}

// --- Rustup Delegation ---

pub fn get_rustup_status() -> RustupStatus {
    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output();

    let Ok(out) = output else {
        return RustupStatus {
            available: false,
            toolchains: vec![],
        };
    };

    if !out.status.success() {
        return RustupStatus {
            available: false,
            toolchains: vec![],
        };
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let toolchains = stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("no installed"))
        .map(|line| {
            let is_default = line.contains("(default)") || line.contains("(active, default)");
            let name = line.split_whitespace().next().unwrap_or(line).to_string();
            RustupToolchain { name, is_default }
        })
        .collect();

    RustupStatus {
        available: true,
        toolchains,
    }
}

pub fn run_rustup_action(args: &[&str]) -> Result<String, String> {
    let out = Command::new("rustup")
        .args(args)
        .output()
        .map_err(|e| format!("Failed executing rustup: {}", e))?;

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(combined.trim().to_string())
    } else {
        Err(format!("rustup failed: {}", combined.trim()))
    }
}

// --- Per-Project .workbench.toml Pinning ---

pub fn get_project_pins(project_path: &str) -> Result<HashMap<String, String>, String> {
    let file = Path::new(project_path).join(".workbench.toml");
    if !file.is_file() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(&file)
        .map_err(|e| format!("Failed reading {}: {}", file.display(), e))?;
    let parsed: ProjectWorkbenchFile = toml::from_str(&raw)
        .map_err(|e| format!("Failed parsing {}: {}", file.display(), e))?;
    Ok(parsed.runtimes)
}

pub fn set_project_pin(
    project_path: &str,
    tool: &str,
    version: Option<&str>,
) -> Result<HashMap<String, String>, String> {
    let dir = Path::new(project_path);
    if !dir.is_dir() {
        return Err(format!("Project directory does not exist: {}", project_path));
    }
    let file = dir.join(".workbench.toml");
    let mut parsed = if file.is_file() {
        let raw = fs::read_to_string(&file).unwrap_or_default();
        toml::from_str::<ProjectWorkbenchFile>(&raw).unwrap_or_default()
    } else {
        ProjectWorkbenchFile::default()
    };

    match version.map(|v| v.trim()).filter(|v| !v.is_empty()) {
        Some(v) => {
            parsed.runtimes.insert(tool.to_string(), v.to_string());
        }
        None => {
            parsed.runtimes.remove(tool);
        }
    }

    if parsed.runtimes.is_empty() {
        if file.is_file() {
            let _ = fs::remove_file(&file);
        }
    } else {
        let content = toml::to_string_pretty(&parsed)
            .map_err(|e| format!("Failed serializing .workbench.toml: {}", e))?;
        fs::write(&file, content)
            .map_err(|e| format!("Failed writing {}: {}", file.display(), e))?;
    }

    Ok(parsed.runtimes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bundled_manifest_parses_cleanly() {
        let manifest = load_manifest().expect("manifest should parse");
        assert!(manifest.runtimes.iter().any(|r| r.tool == "php"));
        assert!(manifest.runtimes.iter().any(|r| r.tool == "node"));
        assert!(manifest.runtimes.iter().any(|r| r.tool == "composer"));
        assert!(manifest.runtimes.iter().any(|r| r.tool == "flutter"));
    }

    #[test]
    fn blocks_path_traversal_in_archives() {
        assert!(sanitize_archive_rel_path(Path::new("../etc/passwd"), 0).is_none());
        assert!(sanitize_archive_rel_path(Path::new("/abs/path"), 0).is_none());
        assert_eq!(
            sanitize_archive_rel_path(Path::new("node-v22.14.0/bin/node"), 1),
            Some(PathBuf::from("bin/node"))
        );
    }

    #[test]
    fn project_pins_roundtrip_and_cleanup() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().to_string_lossy().to_string();

        let pins = set_project_pin(&proj, "php", Some("8.4.4")).unwrap();
        assert_eq!(pins.get("php").map(|s| s.as_str()), Some("8.4.4"));
        assert!(tmp.path().join(".workbench.toml").is_file());

        let loaded = get_project_pins(&proj).unwrap();
        assert_eq!(loaded.get("php").map(|s| s.as_str()), Some("8.4.4"));

        // Clearing last pin removes .workbench.toml cleanly
        let empty = set_project_pin(&proj, "php", None).unwrap();
        assert!(empty.is_empty());
        assert!(!tmp.path().join(".workbench.toml").exists());
    }
}
