use crate::config::{shims_dir, workbench_home};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MARKER_START: &str = "# >>> Workbench >>>";
const MARKER_END: &str = "# <<< Workbench <<<";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellIntegrationStatus {
    pub shell_name: String,
    pub profile_path: String,
    pub shims_path: String,
    pub installed: bool,
    pub workbench_home: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallReport {
    pub shell_profiles_cleaned: Vec<String>,
    pub workbench_home_removed: bool,
    pub removed_path: String,
    pub user_projects_touched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub category: String,
    pub installed: bool,
    pub version: Option<String>,
    pub binary_path: Option<String>,
    pub source: Option<String>,
}

fn detect_shell_profile() -> (String, PathBuf) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    #[cfg(target_os = "windows")]
    {
        return (
            "PowerShell / Windows PATH".to_string(),
            home.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1"),
        );
    }

    #[cfg(not(target_os = "windows"))]
    {
        let shell_env = std::env::var("SHELL").unwrap_or_default();
        if shell_env.ends_with("zsh") {
            ("zsh".to_string(), home.join(".zshrc"))
        } else if shell_env.ends_with("fish") {
            ("fish".to_string(), home.join(".config/fish/config.fish"))
        } else if shell_env.ends_with("bash") {
            let bashrc = home.join(".bashrc");
            if bashrc.exists() {
                ("bash".to_string(), bashrc)
            } else {
                ("bash".to_string(), home.join(".bash_profile"))
            }
        } else {
            #[cfg(target_os = "macos")]
            {
                ("zsh".to_string(), home.join(".zshrc"))
            }
            #[cfg(not(target_os = "macos"))]
            {
                ("bash".to_string(), home.join(".bashrc"))
            }
        }
    }
}

fn build_shell_block(shell_name: &str, shims: &Path) -> Result<String, String> {
    let shims_str = shims.to_string_lossy();
    if shims_str.contains('\n') || shims_str.contains('\r') || shims_str.contains('\0') {
        return Err("Invalid characters in Workbench shims path.".to_string());
    }
    let block = if shell_name == "fish" {
        let escaped = shims_str.replace('\\', "\\\\").replace('\'', "\\'");
        format!(
            "{}\nfish_add_path -p '{}'\n{}\n",
            MARKER_START, escaped, MARKER_END
        )
    } else if shell_name.starts_with("PowerShell") {
        let escaped = shims_str.replace('\'', "''");
        format!(
            "{}\n$env:PATH = '{};' + $env:PATH\n{}\n",
            MARKER_START, escaped, MARKER_END
        )
    } else {
        let escaped = shims_str.replace('\'', "'\\''");
        format!(
            "{}\nexport PATH='{}':\"$PATH\"\n{}\n",
            MARKER_START, escaped, MARKER_END
        )
    };
    Ok(block)
}

pub fn strip_workbench_block(content: &str) -> (String, bool) {
    let Some(start_idx) = content.find(MARKER_START) else {
        return (content.to_string(), false);
    };
    let Some(end_rel) = content[start_idx..].find(MARKER_END) else {
        return (content.to_string(), false);
    };
    let mut end_idx = start_idx + end_rel + MARKER_END.len();
    if content[end_idx..].starts_with('\n') {
        end_idx += 1;
    }
    let mut prefix = &content[..start_idx];
    if prefix.ends_with("\n\n") && content[end_idx..].is_empty() {
        prefix = &prefix[..prefix.len() - 1];
    }
    let mut result = String::with_capacity(content.len());
    result.push_str(prefix);
    result.push_str(&content[end_idx..]);
    (result, true)
}

pub fn get_shell_status() -> ShellIntegrationStatus {
    let (shell_name, profile_path) = detect_shell_profile();
    let shims = shims_dir();
    let installed = fs::read_to_string(&profile_path)
        .map(|c| c.contains(MARKER_START) && c.contains(MARKER_END))
        .unwrap_or(false);

    ShellIntegrationStatus {
        shell_name,
        profile_path: profile_path.to_string_lossy().to_string(),
        shims_path: shims.to_string_lossy().to_string(),
        installed,
        workbench_home: workbench_home().to_string_lossy().to_string(),
    }
}

pub fn install_shell_integration() -> Result<ShellIntegrationStatus, String> {
    let (shell_name, profile_path) = detect_shell_profile();
    let shims = shims_dir();
    fs::create_dir_all(&shims)
        .map_err(|e| format!("Failed to create shims dir: {}", e))?;

    if let Some(parent) = profile_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create shell profile parent dir: {}", e))?;
    }

    let existing = fs::read_to_string(&profile_path).unwrap_or_default();
    let (cleaned, _) = strip_workbench_block(&existing);
    let block = build_shell_block(&shell_name, &shims)?;

    let mut updated = cleaned;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() && !updated.ends_with("\n\n") {
        updated.push('\n');
    }
    updated.push_str(&block);

    fs::write(&profile_path, updated)
        .map_err(|e| format!("Failed to write {}: {}", profile_path.display(), e))?;

    Ok(get_shell_status())
}

pub fn remove_shell_integration() -> Result<Vec<String>, String> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let candidates = vec![
        home.join(".zshrc"),
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".config/fish/config.fish"),
        home.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1"),
    ];

    let mut cleaned_profiles = Vec::new();
    for profile in candidates {
        if !profile.is_file() {
            continue;
        }
        let content = fs::read_to_string(&profile)
            .map_err(|e| format!("Failed reading {}: {}", profile.display(), e))?;
        let (stripped, changed) = strip_workbench_block(&content);
        if changed {
            fs::write(&profile, stripped)
                .map_err(|e| format!("Failed updating {}: {}", profile.display(), e))?;
            cleaned_profiles.push(profile.to_string_lossy().to_string());
        }
    }
    Ok(cleaned_profiles)
}

/// Cleanly removes ONLY Workbench's shell integration and `~/.workbench` app data.
/// Never touches any user project directories.
pub fn clean_uninstall_workbench() -> Result<UninstallReport, String> {
    let cleaned = remove_shell_integration()?;
    let wb_home = workbench_home();
    let removed_path = wb_home.to_string_lossy().to_string();

    let mut removed = false;
    if wb_home.exists() {
        // Extra safety check: ensure path ends with .workbench or Workbench
        let folder_name = wb_home
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if folder_name != ".workbench" && folder_name != "Workbench" {
            return Err(format!(
                "Refusing to remove unexpected Workbench home path: {}",
                removed_path
            ));
        }
        fs::remove_dir_all(&wb_home)
            .map_err(|e| format!("Failed removing {}: {}", removed_path, e))?;
        removed = true;
    }

    Ok(UninstallReport {
        shell_profiles_cleaned: cleaned,
        workbench_home_removed: removed,
        removed_path,
        user_projects_touched: false,
    })
}

pub fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs_list = Vec::new();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    if let Some(path_var) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&path_var) {
            if p.is_dir() {
                dirs_list.push(p);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for p in [
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/local/sbin"),
            PathBuf::from("/Applications/ServBay/bin"),
            PathBuf::from("/Applications/ServBay/script/alias"),
            home.join(".cargo/bin"),
            home.join("fvm/default/bin"),
            home.join("development/flutter/bin"),
            home.join("flutter/bin"),
            home.join(".composer/vendor/bin"),
            home.join(".config/herd/bin"),
        ] {
            if p.is_dir() {
                dirs_list.push(p);
            }
        }
        // Check Homebrew keg-only opt formulas (e.g. php@8.3, mariadb@10.11, httpd)
        if let Ok(entries) = fs::read_dir("/opt/homebrew/opt") {
            for entry in entries.flatten() {
                let bin = entry.path().join("bin");
                if bin.is_dir() {
                    dirs_list.push(bin);
                }
                let sbin = entry.path().join("sbin");
                if sbin.is_dir() {
                    dirs_list.push(sbin);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for p in [
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/snap/bin"),
            home.join(".cargo/bin"),
            home.join("flutter/bin"),
            home.join("fvm/default/bin"),
            home.join(".local/bin"),
        ] {
            if p.is_dir() {
                dirs_list.push(p);
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for p in [
            PathBuf::from("C:\\Program Files\\nodejs"),
            PathBuf::from("C:\\flutter\\bin"),
            PathBuf::from("C:\\php"),
            home.join(".cargo\\bin"),
            home.join("scoop\\shims"),
        ] {
            if p.is_dir() {
                dirs_list.push(p);
            }
        }
    }

    // Deduplicate while preserving priority order
    let mut seen = std::collections::HashSet::new();
    dirs_list.retain(|p| seen.insert(p.clone()));
    dirs_list
}

pub fn classify_source(path: &Path) -> String {
    let s = path.to_string_lossy().to_lowercase();
    if s.contains(".workbench") {
        "Workbench".to_string()
    } else if s.contains("homebrew") || s.contains("cellar") {
        "Homebrew".to_string()
    } else if s.contains("servbay") {
        "ServBay".to_string()
    } else if s.contains("herd") {
        "Herd".to_string()
    } else if s.contains(".nvm") {
        "nvm".to_string()
    } else if s.contains(".fnm") {
        "fnm".to_string()
    } else if s.contains("fvm") {
        "FVM".to_string()
    } else if s.contains(".cargo") {
        "rustup / Cargo".to_string()
    } else if s.contains("scoop") {
        "Scoop".to_string()
    } else {
        "System".to_string()
    }
}

pub fn find_binary_anywhere(names: &[&str]) -> Option<PathBuf> {
    let search_dirs = extra_search_dirs();
    let wb_shims = shims_dir();
    for dir in search_dirs {
        // Skip Workbench's own shims dir when probing host-installed tools
        if dir == wb_shims {
            continue;
        }
        for name in names {
            #[cfg(target_os = "windows")]
            let candidates = [
                dir.join(format!("{}.exe", name)),
                dir.join(format!("{}.cmd", name)),
                dir.join(format!("{}.bat", name)),
                dir.join(name),
            ];
            #[cfg(not(target_os = "windows"))]
            let candidates = [dir.join(name)];

            for cand in candidates {
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

fn probe_tool(bins: &[&str], args: &[&str]) -> (bool, Option<String>, Option<String>, Option<String>) {
    let Some(bin_path) = find_binary_anywhere(bins) else {
        return (false, None, None, None);
    };

    let source = Some(classify_source(&bin_path));
    let version = Command::new(&bin_path)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let combined = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            combined.lines().next().map(|l| l.trim().to_string())
        });

    (
        true,
        version,
        Some(bin_path.to_string_lossy().to_string()),
        source,
    )
}

type ToolSpec<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);

pub fn detect_system_tools() -> Vec<DetectedTool> {
    let specs: &[ToolSpec<'_>] = &[
        ("php", "PHP", "runtime", &["php"], &["-v"]),
        ("composer", "Composer", "runtime", &["composer"], &["--version"]),
        ("node", "Node.js", "runtime", &["node"], &["-v"]),
        ("npm", "npm", "runtime", &["npm"], &["-v"]),
        ("pnpm", "pnpm", "runtime", &["pnpm"], &["-v"]),
        ("flutter", "Flutter", "runtime", &["flutter"], &["--version"]),
        ("rust", "Rust (rustc)", "runtime", &["rustc"], &["-V"]),
        ("cargo", "Cargo", "runtime", &["cargo"], &["-V"]),
        ("mariadb", "MariaDB / MySQL", "service", &["mariadbd", "mysqld", "mariadb"], &["--version"]),
        ("redis", "Redis", "service", &["redis-server"], &["--version"]),
        ("caddy", "Caddy", "service", &["caddy"], &["version"]),
        ("apache", "Apache HTTP", "service", &["httpd", "apache2"], &["-v"]),
    ];

    specs
        .iter()
        .map(|(id, name, category, bins, args)| {
            let (installed, version, binary_path, source) = probe_tool(bins, args);
            DetectedTool {
                id: id.to_string(),
                name: name.to_string(),
                category: category.to_string(),
                installed,
                version,
                binary_path,
                source,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_block_strip_preserves_original_content() {
        let original = "export EDITOR=nvim\nalias ll='ls -la'\n";
        let block = build_shell_block("zsh", Path::new("/Users/test/.workbench/shims")).unwrap();
        let combined = format!("{}\n{}", original, block);

        let (stripped, changed) = strip_workbench_block(&combined);
        assert!(changed);
        assert_eq!(stripped, original);
    }
}
