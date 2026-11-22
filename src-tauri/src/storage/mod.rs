use crate::config::DeleteMode;
use jwalk::WalkDir;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectKind {
    Laravel,
    Php,
    SvelteKit,
    Node,
    Flutter,
    Rust,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub rel_path: String,
    pub abs_path: String,
    pub label: String,
    pub size_bytes: u64,
    pub clean_contents_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub kind: ProjectKind,
    pub secondary_kinds: Vec<ProjectKind>,
    pub artifacts: Vec<ArtifactInfo>,
    pub total_reclaimable_bytes: u64,
    pub last_modified_epoch: u64,
    pub idle_days: u64,
    pub hibernated: bool,
    pub version_hints: HashMap<String, String>,
    pub wake_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub projects: Vec<ProjectInfo>,
    pub total_reclaimable_bytes: u64,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HibernateResult {
    pub project_path: String,
    pub project_name: String,
    pub bytes_reclaimed: u64,
    pub deleted_artifacts: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalCacheInfo {
    pub id: String,
    pub name: String,
    pub ecosystem: String,
    pub path: String,
    pub size_bytes: u64,
    pub exists: bool,
    pub description: String,
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    "target",
    ".git",
    "build",
    "dist",
    ".dart_tool",
    "Pods",
    ".svelte-kit",
    ".next",
    ".nuxt",
    ".gradle",
    ".idea",
    ".vscode",
    "__pycache__",
];

struct ArtifactSpec {
    rel_path: &'static str,
    label: &'static str,
    clean_contents_only: bool,
}

fn artifact_specs_for(kind: &ProjectKind) -> Vec<ArtifactSpec> {
    match kind {
        ProjectKind::Laravel => vec![
            ArtifactSpec { rel_path: "vendor", label: "Composer vendor", clean_contents_only: false },
            ArtifactSpec { rel_path: "node_modules", label: "Node modules", clean_contents_only: false },
            ArtifactSpec { rel_path: "public/build", label: "Vite build output", clean_contents_only: false },
            ArtifactSpec { rel_path: "storage/framework/cache/data", label: "Laravel cache", clean_contents_only: true },
            ArtifactSpec { rel_path: "storage/framework/views", label: "Compiled Blade views", clean_contents_only: true },
            ArtifactSpec { rel_path: "storage/logs", label: "Laravel logs", clean_contents_only: true },
        ],
        ProjectKind::Php => vec![
            ArtifactSpec { rel_path: "vendor", label: "Composer vendor", clean_contents_only: false },
            ArtifactSpec { rel_path: "node_modules", label: "Node modules", clean_contents_only: false },
        ],
        ProjectKind::SvelteKit => vec![
            ArtifactSpec { rel_path: "node_modules", label: "Node modules", clean_contents_only: false },
            ArtifactSpec { rel_path: ".svelte-kit", label: "SvelteKit build cache", clean_contents_only: false },
            ArtifactSpec { rel_path: "build", label: "Build output", clean_contents_only: false },
            ArtifactSpec { rel_path: "dist", label: "Dist output", clean_contents_only: false },
            ArtifactSpec { rel_path: ".vite", label: "Vite cache", clean_contents_only: false },
        ],
        ProjectKind::Node => vec![
            ArtifactSpec { rel_path: "node_modules", label: "Node modules", clean_contents_only: false },
            ArtifactSpec { rel_path: "dist", label: "Dist output", clean_contents_only: false },
            ArtifactSpec { rel_path: "build", label: "Build output", clean_contents_only: false },
            ArtifactSpec { rel_path: ".next", label: "Next.js cache", clean_contents_only: false },
            ArtifactSpec { rel_path: ".nuxt", label: "Nuxt cache", clean_contents_only: false },
            ArtifactSpec { rel_path: ".output", label: "Nitro output", clean_contents_only: false },
            ArtifactSpec { rel_path: ".vite", label: "Vite cache", clean_contents_only: false },
        ],
        ProjectKind::Flutter => vec![
            ArtifactSpec { rel_path: "build", label: "Flutter build", clean_contents_only: false },
            ArtifactSpec { rel_path: ".dart_tool", label: "Dart tool cache", clean_contents_only: false },
            ArtifactSpec { rel_path: "ios/Pods", label: "iOS CocoaPods", clean_contents_only: false },
            ArtifactSpec { rel_path: "ios/.symlinks", label: "iOS plugin symlinks", clean_contents_only: false },
            ArtifactSpec { rel_path: "android/.gradle", label: "Android Gradle cache", clean_contents_only: false },
            ArtifactSpec { rel_path: "android/app/build", label: "Android build output", clean_contents_only: false },
        ],
        ProjectKind::Rust => vec![
            ArtifactSpec { rel_path: "target", label: "Cargo target", clean_contents_only: false },
        ],
    }
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    // Use Serial parallelism here because callers already parallelize across projects/caches with Rayon,
    // avoiding nested thread-pool oversubscription and stalls.
    WalkDir::new(path)
        .parallelism(jwalk::Parallelism::Serial)
        .skip_hidden(false)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

pub fn detect_project_kinds(dir: &Path) -> Vec<ProjectKind> {
    let mut kinds = Vec::new();
    let has_composer = dir.join("composer.json").is_file();
    let has_artisan = dir.join("artisan").is_file();
    let has_package = dir.join("package.json").is_file();
    let has_svelte = dir.join("svelte.config.js").is_file()
        || dir.join("svelte.config.ts").is_file()
        || dir.join("svelte.config.mjs").is_file();
    let has_pubspec = dir.join("pubspec.yaml").is_file();
    let has_cargo = dir.join("Cargo.toml").is_file();

    if has_composer && has_artisan {
        kinds.push(ProjectKind::Laravel);
    } else if has_composer {
        kinds.push(ProjectKind::Php);
    }

    if has_pubspec {
        kinds.push(ProjectKind::Flutter);
    }

    if has_svelte && has_package {
        kinds.push(ProjectKind::SvelteKit);
    } else if has_package && kinds.is_empty() {
        kinds.push(ProjectKind::Node);
    }

    if has_cargo {
        kinds.push(ProjectKind::Rust);
    }

    kinds
}

fn detect_version_hints(dir: &Path) -> HashMap<String, String> {
    let mut hints = HashMap::new();
    let file_checks = [
        (".php-version", "php"),
        (".nvmrc", "node"),
        (".node-version", "node"),
        (".fvmrc", "flutter"),
        ("rust-toolchain.toml", "rust"),
        ("rust-toolchain", "rust"),
    ];
    for (filename, key) in file_checks {
        let p = dir.join(filename);
        if let Ok(content) = fs::read_to_string(&p) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                if filename == "rust-toolchain.toml" {
                    if let Ok(val) = toml::from_str::<toml::Value>(trimmed) {
                        if let Some(ch) = val
                            .get("toolchain")
                            .and_then(|t| t.get("channel"))
                            .and_then(|c| c.as_str())
                        {
                            hints.insert(key.to_string(), ch.to_string());
                        }
                    }
                } else if filename == ".fvmrc" {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if let Some(v) = val.get("flutter").and_then(|f| f.as_str()) {
                            hints.insert(key.to_string(), v.to_string());
                        }
                    }
                } else {
                    if let Some(first_line) = trimmed.lines().next() {
                        hints.insert(key.to_string(), first_line.trim().to_string());
                    }
                }
            }
        }
    }
    // .workbench.toml overrides ecosystem hints
    let wb_toml = dir.join(".workbench.toml");
    if let Ok(raw) = fs::read_to_string(&wb_toml) {
        if let Ok(val) = toml::from_str::<toml::Value>(&raw) {
            if let Some(table) = val.get("runtimes").and_then(|r| r.as_table()) {
                for (k, v) in table {
                    if let Some(s) = v.as_str() {
                        hints.insert(k.clone(), s.to_string());
                    }
                }
            }
        }
    }
    hints
}

fn detect_wake_commands(dir: &Path, kinds: &[ProjectKind]) -> Vec<String> {
    let mut cmds = Vec::new();
    let has_composer = dir.join("composer.json").is_file();
    let has_package = dir.join("package.json").is_file();

    if has_composer {
        cmds.push("composer install".to_string());
    }
    if has_package {
        if dir.join("pnpm-lock.yaml").is_file() {
            cmds.push("pnpm install".to_string());
        } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
            cmds.push("bun install".to_string());
        } else if dir.join("yarn.lock").is_file() {
            cmds.push("yarn install".to_string());
        } else if dir.join("package-lock.json").is_file() {
            cmds.push("npm ci".to_string());
        } else {
            cmds.push("npm install".to_string());
        }
    }
    if kinds.contains(&ProjectKind::Flutter) {
        cmds.push("flutter pub get".to_string());
    }
    if kinds.contains(&ProjectKind::Rust) {
        cmds.push("cargo check".to_string());
    }
    cmds
}

fn compute_last_modified_epoch(dir: &Path) -> u64 {
    let mut latest = dir
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Check top-level entries & .git/HEAD or .git/index for a fast, accurate activity signal
    for probe in [".git/index", ".git/HEAD", "composer.json", "package.json", "pubspec.yaml", "Cargo.toml"] {
        let p = dir.join(probe);
        if let Ok(meta) = p.metadata() {
            if let Ok(mod_time) = meta.modified() {
                if let Ok(secs) = mod_time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()) {
                    if secs > latest {
                        latest = secs;
                    }
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten().take(40) {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(mod_time) = meta.modified() {
                    if let Ok(secs) = mod_time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()) {
                        if secs > latest {
                            latest = secs;
                        }
                    }
                }
            }
        }
    }
    latest
}

fn discover_project_dirs(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut discovered = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let kinds = detect_project_kinds(&dir);
        if !kinds.is_empty() {
            discovered.push(dir);
            // Do not recurse into sub-folders of a discovered project (avoids scanning monorepo/packages or src-tauri separately unless user adds them explicitly, EXCEPT Tauri Rust projects where src-tauri/target is huge — handle that explicitly!)
            continue;
        }
        if depth == max_depth {
            continue;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() || ft.is_symlink() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || SKIP_DIRS.contains(&name_str.as_ref()) {
                    continue;
                }
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    discovered
}

pub fn inspect_project(dir: &Path) -> Option<ProjectInfo> {
    inspect_project_inner(dir, true)
}

pub fn inspect_project_fast(dir: &Path) -> Option<ProjectInfo> {
    inspect_project_inner(dir, false)
}

fn inspect_project_inner(dir: &Path, compute_sizes: bool) -> Option<ProjectInfo> {
    let kinds = detect_project_kinds(dir);
    if kinds.is_empty() {
        return None;
    }

    let primary = kinds[0].clone();
    let secondary = kinds[1..].to_vec();

    // Collect deduplicated artifact specs across all detected kinds
    let mut seen_rel = HashSet::new();
    let mut specs = Vec::new();
    for k in &kinds {
        for spec in artifact_specs_for(k) {
            if seen_rel.insert(spec.rel_path) {
                specs.push(spec);
            }
        }
    }
    // Also detect Tauri Rust target folder inside JS/SvelteKit projects
    if dir.join("src-tauri").join("Cargo.toml").is_file() && seen_rel.insert("src-tauri/target") {
        specs.push(ArtifactSpec {
            rel_path: "src-tauri/target",
            label: "Tauri Cargo target",
            clean_contents_only: false,
        });
    }

    let artifacts: Vec<ArtifactInfo> = specs
        .iter()
        .filter_map(|spec| {
            let abs = dir.join(spec.rel_path);
            if !abs.exists() {
                return None;
            }
            let size = if compute_sizes {
                let s = dir_size_bytes(&abs);
                if s == 0 {
                    return None;
                }
                s
            } else {
                0
            };
            Some(ArtifactInfo {
                rel_path: spec.rel_path.to_string(),
                abs_path: abs.to_string_lossy().to_string(),
                label: spec.label.to_string(),
                size_bytes: size,
                clean_contents_only: spec.clean_contents_only,
            })
        })
        .collect();

    let total_reclaimable_bytes: u64 = artifacts.iter().map(|a| a.size_bytes).sum();
    let hibernated = if compute_sizes {
        total_reclaimable_bytes == 0
    } else {
        artifacts.is_empty()
    };
    let last_modified_epoch = compute_last_modified_epoch(dir);
    let now_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let idle_days = now_epoch.saturating_sub(last_modified_epoch) / 86_400;

    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());

    Some(ProjectInfo {
        name,
        path: dir.to_string_lossy().to_string(),
        kind: primary,
        secondary_kinds: secondary,
        artifacts,
        total_reclaimable_bytes,
        last_modified_epoch,
        idle_days,
        hibernated,
        version_hints: detect_version_hints(dir),
        wake_commands: detect_wake_commands(dir, &kinds),
    })
}

pub fn discover_projects_fast(roots: &[String]) -> ScanResult {
    let start = std::time::Instant::now();
    let mut project_dirs = Vec::new();
    let mut seen_paths = HashSet::new();

    for root in roots {
        let p = Path::new(root);
        if !p.is_dir() {
            continue;
        }
        for found in discover_project_dirs(p, 4) {
            let key = found.to_string_lossy().to_string();
            if seen_paths.insert(key) {
                project_dirs.push(found);
            }
        }
    }

    let mut projects: Vec<ProjectInfo> = project_dirs
        .par_iter()
        .filter_map(|dir| inspect_project_fast(dir))
        .collect();

    projects.sort_by_cached_key(|a| a.name.to_lowercase());

    ScanResult {
        projects,
        total_reclaimable_bytes: 0,
        scan_duration_ms: start.elapsed().as_millis() as u64,
    }
}

pub fn scan_workspaces(roots: &[String]) -> ScanResult {
    let start = std::time::Instant::now();
    let mut project_dirs = Vec::new();
    let mut seen_paths = HashSet::new();

    for root in roots {
        let p = Path::new(root);
        if !p.is_dir() {
            continue;
        }
        for found in discover_project_dirs(p, 4) {
            let key = found.to_string_lossy().to_string();
            if seen_paths.insert(key) {
                project_dirs.push(found);
            }
        }
    }

    let mut projects: Vec<ProjectInfo> = project_dirs
        .par_iter()
        .filter_map(|dir| inspect_project(dir))
        .collect();

    projects.sort_by_key(|a| std::cmp::Reverse(a.total_reclaimable_bytes));
    let total_reclaimable_bytes = projects.iter().map(|p| p.total_reclaimable_bytes).sum();

    ScanResult {
        projects,
        total_reclaimable_bytes,
        scan_duration_ms: start.elapsed().as_millis() as u64,
    }
}

fn remove_path(path: &Path, mode: &DeleteMode) -> Result<(), String> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        return fs::remove_file(path)
            .map_err(|e| format!("Failed to remove symlink {}: {}", path.display(), e));
    }
    match mode {
        DeleteMode::Trash => trash::delete(path)
            .map_err(|e| format!("Failed to move {} to trash: {}", path.display(), e)),
        DeleteMode::Permanent => {
            if meta.is_dir() {
                fs::remove_dir_all(path)
                    .map_err(|e| format!("Failed to remove dir {}: {}", path.display(), e))
            } else {
                fs::remove_file(path)
                    .map_err(|e| format!("Failed to remove file {}: {}", path.display(), e))
            }
        }
    }
}

pub fn hibernate_project(
    project_path: &str,
    selected_rel_paths: Option<&[String]>,
    mode: &DeleteMode,
) -> Result<HibernateResult, String> {
    let proj_dir = Path::new(project_path);
    if !proj_dir.is_dir() {
        return Err(format!("Project path does not exist: {}", project_path));
    }
    let canonical_project = fs::canonicalize(proj_dir)
        .map_err(|e| format!("Failed to canonicalize project path: {}", e))?;

    let info = inspect_project(proj_dir)
        .ok_or_else(|| format!("Unrecognized project at {}", project_path))?;

    let filter_set: Option<HashSet<&str>> =
        selected_rel_paths.map(|list| list.iter().map(|s| s.as_str()).collect());

    let mut bytes_reclaimed = 0u64;
    let mut deleted_artifacts = Vec::new();
    let mut errors = Vec::new();

    for artifact in info.artifacts {
        if let Some(ref set) = filter_set {
            if !set.contains(artifact.rel_path.as_str()) {
                continue;
            }
        }
        let target = proj_dir.join(&artifact.rel_path);
        // Safety guard: canonical path MUST stay strictly inside canonical_project
        let Ok(canonical_target) = fs::canonicalize(&target) else {
            continue;
        };
        if !canonical_target.starts_with(&canonical_project) || canonical_target == canonical_project {
            errors.push(format!("Blocked unsafe path outside project: {}", artifact.rel_path));
            continue;
        }

        if artifact.clean_contents_only && canonical_target.is_dir() {
            match fs::read_dir(&canonical_target) {
                Ok(entries) => {
                    let mut had_err = false;
                    for entry in entries.flatten() {
                        let p = entry.path();
                        // Preserve .gitignore inside Laravel storage dirs
                        if p.file_name().map(|n| n == ".gitignore").unwrap_or(false) {
                            continue;
                        }
                        if let Err(e) = remove_path(&p, mode) {
                            errors.push(e);
                            had_err = true;
                        }
                    }
                    if !had_err {
                        bytes_reclaimed += artifact.size_bytes;
                        deleted_artifacts.push(artifact.rel_path);
                    }
                }
                Err(e) => errors.push(format!("Failed reading {}: {}", artifact.rel_path, e)),
            }
        } else {
            match remove_path(&canonical_target, mode) {
                Ok(()) => {
                    bytes_reclaimed += artifact.size_bytes;
                    deleted_artifacts.push(artifact.rel_path);
                }
                Err(e) => errors.push(e),
            }
        }
    }

    Ok(HibernateResult {
        project_path: info.path,
        project_name: info.name,
        bytes_reclaimed,
        deleted_artifacts,
        errors,
    })
}

type CacheCandidate = (&'static str, &'static str, &'static str, PathBuf, &'static str);

fn global_cache_candidates() -> Vec<CacheCandidate> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| home.join(".cache"));

    let mut candidates: Vec<CacheCandidate> = vec![
        (
            "composer",
            "Composer Cache",
            "PHP / Laravel",
            {
                #[cfg(target_os = "macos")]
                { home.join("Library/Caches/composer") }
                #[cfg(not(target_os = "macos"))]
                { cache_dir.join("composer") }
            },
            "Downloaded Composer packages and zip archives",
        ),
        (
            "npm",
            "npm Cache",
            "Node.js",
            {
                #[cfg(target_os = "windows")]
                { dirs::data_local_dir().unwrap_or_else(|| home.clone()).join("npm-cache") }
                #[cfg(not(target_os = "windows"))]
                { home.join(".npm") }
            },
            "Cached npm tarballs and package metadata",
        ),
        (
            "pnpm",
            "pnpm Store",
            "Node.js",
            {
                #[cfg(target_os = "macos")]
                { home.join("Library/pnpm/store") }
                #[cfg(target_os = "windows")]
                { dirs::data_local_dir().unwrap_or_else(|| home.clone()).join("pnpm/store") }
                #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
                { dirs::data_local_dir().unwrap_or_else(|| home.join(".local/share")).join("pnpm/store") }
            },
            "Content-addressable pnpm package store",
        ),
        (
            "yarn",
            "Yarn Cache",
            "Node.js",
            {
                #[cfg(target_os = "macos")]
                { home.join("Library/Caches/Yarn") }
                #[cfg(not(target_os = "macos"))]
                { cache_dir.join("yarn") }
            },
            "Cached Yarn packages",
        ),
        (
            "pub",
            "Flutter Pub Cache",
            "Flutter / Dart",
            home.join(".pub-cache"),
            "Downloaded Dart & Flutter packages",
        ),
        (
            "gradle",
            "Gradle Caches",
            "Flutter / Android",
            home.join(".gradle/caches"),
            "Android Gradle build & dependency caches",
        ),
        (
            "cargo",
            "Cargo Registry Cache",
            "Rust",
            home.join(".cargo/registry/cache"),
            "Downloaded Rust crate .crate archives",
        ),
    ];

    #[cfg(target_os = "macos")]
    candidates.push((
        "cocoapods",
        "CocoaPods Cache",
        "Flutter / iOS",
        home.join("Library/Caches/CocoaPods"),
        "Cached iOS CocoaPods specs and pods",
    ));

    candidates
}

pub fn scan_global_caches() -> Vec<GlobalCacheInfo> {
    global_cache_candidates()
        .into_par_iter()
        .map(|(id, name, ecosystem, path, description)| {
            let exists = path.exists();
            let size_bytes = if exists { dir_size_bytes(&path) } else { 0 };
            GlobalCacheInfo {
                id: id.to_string(),
                name: name.to_string(),
                ecosystem: ecosystem.to_string(),
                path: path.to_string_lossy().to_string(),
                size_bytes,
                exists,
                description: description.to_string(),
            }
        })
        .collect()
}

pub fn clean_global_cache(cache_id: &str, mode: &DeleteMode) -> Result<(String, u64), String> {
    let (_, name, _, path, _) = global_cache_candidates()
        .into_iter()
        .find(|(id, _, _, _, _)| *id == cache_id)
        .ok_or_else(|| format!("Unknown cache id: {}", cache_id))?;

    if !path.exists() {
        return Ok((name.to_string(), 0));
    }

    let size_bytes = dir_size_bytes(&path);
    if size_bytes == 0 {
        return Ok((name.to_string(), 0));
    }

    // Clean contents of cache directory while keeping the directory itself
    let entries = fs::read_dir(&path)
        .map_err(|e| format!("Failed to read cache directory {}: {}", path.display(), e))?;
    for entry in entries.flatten() {
        remove_path(&entry.path(), mode)?;
    }
    Ok((name.to_string(), size_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fingerprints_and_hibernates_laravel_safely() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("my-laravel-app");
        fs::create_dir_all(proj.join("vendor/pkg")).unwrap();
        fs::create_dir_all(proj.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(proj.join("storage/logs")).unwrap();
        fs::write(proj.join("artisan"), "#!/usr/bin/env php").unwrap();
        fs::write(proj.join("composer.json"), "{}").unwrap();
        fs::write(proj.join(".env"), "APP_KEY=secret").unwrap();
        fs::write(proj.join("vendor/pkg/a.php"), vec![b'x'; 2048]).unwrap();
        fs::write(proj.join("node_modules/pkg/index.js"), vec![b'y'; 4096]).unwrap();
        fs::write(proj.join("storage/logs/laravel.log"), vec![b'z'; 1024]).unwrap();
        fs::write(proj.join("storage/logs/.gitignore"), "*\n!.gitignore\n").unwrap();

        let info = inspect_project(&proj).expect("should detect Laravel");
        assert_eq!(info.kind, ProjectKind::Laravel);
        assert!(info.total_reclaimable_bytes >= 7168);

        let res = hibernate_project(
            &proj.to_string_lossy(),
            None,
            &DeleteMode::Permanent,
        )
        .unwrap();
        assert!(res.errors.is_empty());
        assert!(!proj.join("vendor").exists());
        assert!(!proj.join("node_modules").exists());
        assert!(!proj.join("storage/logs/laravel.log").exists());
        // Source & config files MUST remain untouched
        assert!(proj.join(".env").is_file());
        assert!(proj.join("artisan").is_file());
        assert!(proj.join("composer.json").is_file());
        assert!(proj.join("storage/logs/.gitignore").is_file());
    }

    #[test]
    fn fingerprints_sveltekit_flutter_and_rust() {
        let tmp = tempdir().unwrap();

        let sk = tmp.path().join("web-app");
        fs::create_dir_all(sk.join(".svelte-kit")).unwrap();
        fs::write(sk.join("package.json"), "{}").unwrap();
        fs::write(sk.join("svelte.config.js"), "export default {}").unwrap();
        fs::write(sk.join(".svelte-kit/cache.js"), "abc").unwrap();
        assert_eq!(inspect_project(&sk).unwrap().kind, ProjectKind::SvelteKit);

        let fl = tmp.path().join("mobile-app");
        fs::create_dir_all(fl.join("build")).unwrap();
        fs::write(fl.join("pubspec.yaml"), "name: demo").unwrap();
        fs::write(fl.join("build/out.apk"), "1234").unwrap();
        assert_eq!(inspect_project(&fl).unwrap().kind, ProjectKind::Flutter);

        let rs = tmp.path().join("cli-tool");
        fs::create_dir_all(rs.join("target/debug")).unwrap();
        fs::write(rs.join("Cargo.toml"), "[package]\nname='x'").unwrap();
        fs::write(rs.join("target/debug/bin"), vec![0u8; 5000]).unwrap();
        let rs_info = inspect_project(&rs).unwrap();
        assert_eq!(rs_info.kind, ProjectKind::Rust);
        assert_eq!(rs_info.total_reclaimable_bytes, 5000);
    }
}
