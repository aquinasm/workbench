use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Debug, Default, Deserialize)]
struct ProjectWorkbenchToml {
    #[serde(default)]
    runtimes: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct GlobalWorkbenchToml {
    #[serde(default)]
    defaults: HashMap<String, String>,
    #[serde(default)]
    external_paths: HashMap<String, String>,
}

fn load_global_config(wb_home: &Path) -> GlobalWorkbenchToml {
    let global_cfg = wb_home.join("config.toml");
    fs::read_to_string(global_cfg)
        .ok()
        .and_then(|raw| toml::from_str::<GlobalWorkbenchToml>(&raw).ok())
        .unwrap_or_default()
}

fn resolve_external_binary(
    wb_home: &Path,
    tool: &str,
    requested: &str,
    bin_name: &str,
) -> Option<PathBuf> {
    let cfg = load_global_config(wb_home);
    let key = format!("{}:{}", tool, requested);
    let primary = PathBuf::from(cfg.external_paths.get(&key)?);
    if !primary.is_file() {
        return None;
    }
    if bin_name == tool || (tool == "composer" && bin_name == "composer") {
        return Some(primary);
    }
    // Resolve sibling binary (e.g. npm/npx next to external node, or dart next to external flutter)
    let parent = primary.parent()?;
    #[cfg(target_os = "windows")]
    for ext in ["exe", "cmd", "bat"] {
        let sib = parent.join(format!("{}.{}", bin_name, ext));
        if sib.is_file() {
            return Some(sib);
        }
    }
    let sib = parent.join(bin_name);
    if sib.is_file() {
        Some(sib)
    } else {
        Some(primary)
    }
}

fn workbench_home() -> PathBuf {
    if let Ok(custom) = env::var("WORKBENCH_HOME") {
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

/// Maps a CLI shim name (e.g. `npm`, `dart`) to its parent runtime tool key (`node`, `flutter`).
fn tool_for_binary(bin_name: &str) -> &'static str {
    match bin_name {
        "php" | "php-fpm" | "php-cgi" => "php",
        "composer" => "composer",
        "node" | "npm" | "npx" | "corepack" => "node",
        "flutter" | "dart" => "flutter",
        _ => "unknown",
    }
}

/// Walks up from `start_dir` checking `.workbench.toml` and ecosystem version files.
pub fn resolve_requested_version(start_dir: &Path, tool: &str, wb_home: &Path) -> Option<String> {
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        // 1. Highest priority: .workbench.toml
        let wb_file = dir.join(".workbench.toml");
        if wb_file.is_file() {
            if let Ok(raw) = fs::read_to_string(&wb_file) {
                if let Ok(parsed) = toml::from_str::<ProjectWorkbenchToml>(&raw) {
                    if let Some(v) = parsed.runtimes.get(tool) {
                        let trimmed = v.trim().trim_start_matches('v');
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }

        // 2. Ecosystem files
        let eco_version = match tool {
            "php" => read_first_line(&dir.join(".php-version")),
            "node" => read_first_line(&dir.join(".nvmrc"))
                .or_else(|| read_first_line(&dir.join(".node-version"))),
            "flutter" => read_fvmrc(&dir.join(".fvmrc")),
            _ => None,
        };
        if let Some(v) = eco_version {
            return Some(v);
        }

        current = dir.parent();
    }

    // 3. Fallback: global default in ~/.workbench/config.toml
    let global_cfg = wb_home.join("config.toml");
    if let Ok(raw) = fs::read_to_string(global_cfg) {
        if let Ok(parsed) = toml::from_str::<GlobalWorkbenchToml>(&raw) {
            if let Some(v) = parsed.defaults.get(tool) {
                let trimmed = v.trim().trim_start_matches('v');
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

fn read_first_line(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let line = raw.lines().next()?.trim().trim_start_matches('v');
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn read_fvmrc(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let v = val.get("flutter")?.as_str()?.trim().trim_start_matches('v');
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Finds the best matching installed version directory under `~/.workbench/runtimes/<tool>/`.
pub fn match_installed_version(
    wb_home: &Path,
    tool: &str,
    requested: Option<&str>,
) -> Option<String> {
    let tool_dir = wb_home.join("runtimes").join(tool);
    let entries = fs::read_dir(&tool_dir).ok()?;
    let mut versions: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    if versions.is_empty() {
        return None;
    }

    // Sort descending so highest matching semver is chosen first
    versions.sort_by(|a, b| compare_versions(b, a));

    if let Some(req) = requested {
        // Exact match first
        if let Some(exact) = versions.iter().find(|v| v.as_str() == req) {
            return Some(exact.clone());
        }
        // Prefix match (e.g. "8.3" -> "8.3.17", "22" -> "22.14.0")
        let prefix_dot = format!("{}.", req);
        if let Some(prefixed) = versions.iter().find(|v| v.starts_with(&prefix_dot)) {
            return Some(prefixed.clone());
        }
    }

    versions.into_iter().next()
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|part| {
                part.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    };
    parse(a).cmp(&parse(b))
}

fn resolve_binary_path(wb_home: &Path, tool: &str, version: &str, bin_name: &str) -> Option<PathBuf> {
    let base = wb_home.join("runtimes").join(tool).join(version);
    #[cfg(target_os = "windows")]
    let candidates: Vec<PathBuf> = match bin_name {
        "php" => vec![base.join("php.exe"), base.join("bin/php.exe")],
        "node" => vec![base.join("node.exe"), base.join("bin/node.exe")],
        "npm" => vec![base.join("npm.cmd"), base.join("bin/npm.cmd")],
        "npx" => vec![base.join("npx.cmd"), base.join("bin/npx.cmd")],
        "flutter" => vec![base.join("bin/flutter.bat"), base.join("flutter/bin/flutter.bat")],
        "dart" => vec![base.join("bin/dart.bat"), base.join("flutter/bin/dart.bat")],
        "composer" => vec![base.join("composer.phar")],
        _ => vec![base.join(format!("{}.exe", bin_name))],
    };
    #[cfg(not(target_os = "windows"))]
    let candidates: Vec<PathBuf> = match bin_name {
        "composer" => vec![base.join("composer.phar")],
        "flutter" | "dart" => vec![
            base.join("bin").join(bin_name),
            base.join("flutter/bin").join(bin_name),
        ],
        _ => vec![base.join("bin").join(bin_name), base.join(bin_name)],
    };

    candidates.into_iter().find(|p| p.is_file())
}

fn find_host_fallback(bin_name: &str, shims_dir: &Path) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    let canonical_shims = fs::canonicalize(shims_dir).ok();
    for dir in env::split_paths(&path_var) {
        if let (Some(ref cs), Ok(cd)) = (&canonical_shims, fs::canonicalize(&dir)) {
            if &cd == cs {
                continue;
            }
        }
        #[cfg(target_os = "windows")]
        let candidate = dir.join(format!("{}.exe", bin_name));
        #[cfg(not(target_os = "windows"))]
        let candidate = dir.join(bin_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn exec_command(mut cmd: Command) -> ! {
    #[cfg(unix)]
    {
        let err = cmd.exec();
        eprintln!("workbench-shim: failed to exec target: {}", err);
        process::exit(127);
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => process::exit(status.code().unwrap_or(1)),
            Err(err) => {
                eprintln!("workbench-shim: failed to run target: {}", err);
                process::exit(127);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let invoked = Path::new(&args[0])
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let tool = tool_for_binary(&invoked);
    let wb_home = workbench_home();
    let shims_dir = wb_home.join("shims");
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let passthrough_args = &args[1..];

    if tool == "composer" {
        let req_composer = resolve_requested_version(&cwd, "composer", &wb_home);
        if let Some(ref req) = req_composer {
            if let Some(ext_bin) = resolve_external_binary(&wb_home, "composer", req, "composer") {
                let mut cmd = Command::new(ext_bin);
                cmd.args(passthrough_args);
                exec_command(cmd);
            }
        }
        if let Some(ver) = match_installed_version(&wb_home, "composer", req_composer.as_deref()) {
            if let Some(phar) = resolve_binary_path(&wb_home, "composer", &ver, "composer") {
                let req_php = resolve_requested_version(&cwd, "php", &wb_home);
                let php_bin = req_php
                    .as_deref()
                    .and_then(|r| resolve_external_binary(&wb_home, "php", r, "php"))
                    .or_else(|| {
                        match_installed_version(&wb_home, "php", req_php.as_deref())
                            .and_then(|pv| resolve_binary_path(&wb_home, "php", &pv, "php"))
                    })
                    .or_else(|| find_host_fallback("php", &shims_dir))
                    .unwrap_or_else(|| PathBuf::from("php"));
                let mut cmd = Command::new(php_bin);
                cmd.arg(phar).args(passthrough_args);
                exec_command(cmd);
            }
        }
    } else if tool != "unknown" {
        let req = resolve_requested_version(&cwd, tool, &wb_home);
        if let Some(ref req_ver) = req {
            if let Some(ext_bin) = resolve_external_binary(&wb_home, tool, req_ver, &invoked) {
                let mut cmd = Command::new(&ext_bin);
                if let Some(bin_parent) = ext_bin.parent() {
                    if let Ok(existing_path) = env::var("PATH") {
                        let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
                        cmd.env("PATH", format!("{}{}{}", bin_parent.display(), sep, existing_path));
                    }
                }
                cmd.args(passthrough_args);
                exec_command(cmd);
            }
        }
        if let Some(ver) = match_installed_version(&wb_home, tool, req.as_deref()) {
            if let Some(target_bin) = resolve_binary_path(&wb_home, tool, &ver, &invoked) {
                let mut cmd = Command::new(&target_bin);
                if tool == "php" {
                    let php_dir = wb_home.join("runtimes").join("php").join(&ver);
                    cmd.env("PHPRC", &php_dir);
                }
                if let Some(bin_parent) = target_bin.parent() {
                    if let Ok(existing_path) = env::var("PATH") {
                        let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
                        cmd.env("PATH", format!("{}{}{}", bin_parent.display(), sep, existing_path));
                    }
                }
                cmd.args(passthrough_args);
                exec_command(cmd);
            }
        }
    }

    // Fallback to host PATH binary if not yet installed in Workbench
    if let Some(fallback) = find_host_fallback(&invoked, &shims_dir) {
        let mut cmd = Command::new(fallback);
        cmd.args(passthrough_args);
        exec_command(cmd);
    }

    eprintln!(
        "workbench-shim: '{}' is not installed in Workbench or on your system PATH. Install a version in Workbench -> Runtimes.",
        invoked
    );
    process::exit(127);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_workbench_toml_over_ecosystem_and_global() {
        let tmp = tempdir().unwrap();
        let wb_home = tmp.path().join(".workbench");
        let project = tmp.path().join("app");
        let nested = project.join("src/routes");
        fs::create_dir_all(&wb_home).unwrap();
        fs::create_dir_all(&nested).unwrap();

        fs::write(wb_home.join("config.toml"), "[defaults]\nphp = \"8.2\"\nnode = \"20\"\n").unwrap();
        fs::write(project.join(".php-version"), "8.3\n").unwrap();
        fs::write(project.join(".nvmrc"), "v22.14.0\n").unwrap();
        fs::write(project.join(".workbench.toml"), "[runtimes]\nphp = \"8.4\"\n").unwrap();

        // .workbench.toml beats .php-version and global default
        assert_eq!(
            resolve_requested_version(&nested, "php", &wb_home).as_deref(),
            Some("8.4")
        );
        // .nvmrc beats global default when not in .workbench.toml
        assert_eq!(
            resolve_requested_version(&nested, "node", &wb_home).as_deref(),
            Some("22.14.0")
        );
    }

    #[test]
    fn matches_prefix_and_picks_highest_semver() {
        let tmp = tempdir().unwrap();
        let wb_home = tmp.path().join(".workbench");
        fs::create_dir_all(wb_home.join("runtimes/php/8.3.10")).unwrap();
        fs::create_dir_all(wb_home.join("runtimes/php/8.3.17")).unwrap();
        fs::create_dir_all(wb_home.join("runtimes/php/8.4.4")).unwrap();

        assert_eq!(
            match_installed_version(&wb_home, "php", Some("8.3")).as_deref(),
            Some("8.3.17")
        );
        assert_eq!(
            match_installed_version(&wb_home, "php", None).as_deref(),
            Some("8.4.4")
        );
    }
}
