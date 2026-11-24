use crate::runtimes::safe_runtime_install_dir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LARAVEL_CORE_EXTENSIONS: &[&str] = &[
    "bcmath",
    "ctype",
    "curl",
    "dom",
    "fileinfo",
    "filter",
    "gd",
    "hash",
    "intl",
    "json",
    "mbstring",
    "openssl",
    "pcre",
    "pdo",
    "pdo_mysql",
    "redis",
    "session",
    "tokenizer",
    "xml",
    "zip",
];

const ZEND_EXTENSIONS: &[&str] = &["opcache", "xdebug"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpExtensionInfo {
    pub name: String,
    pub loaded: bool,
    pub built_in: bool,
    pub available_dynamic: bool,
    pub enabled_in_ini: bool,
    pub is_laravel_core: bool,
    pub is_zend: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpIniSettings {
    pub memory_limit: String,
    pub upload_max_filesize: String,
    pub post_max_size: String,
    pub max_execution_time: String,
    pub opcache_enable: bool,
}

impl Default for PhpIniSettings {
    fn default() -> Self {
        Self {
            memory_limit: "512M".to_string(),
            upload_max_filesize: "64M".to_string(),
            post_max_size: "64M".to_string(),
            max_execution_time: "120".to_string(),
            opcache_enable: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpVersionDetails {
    pub version: String,
    pub ini_path: String,
    pub extensions: Vec<PhpExtensionInfo>,
    pub settings: PhpIniSettings,
}

fn find_php_binary(base: &Path) -> Option<PathBuf> {
    let candidates = [
        base.join("php.exe"),
        base.join("bin/php.exe"),
        base.join("bin/php"),
        base.join("php"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

pub fn ensure_php_ini(base: &Path) -> Result<PathBuf, String> {
    let ini = base.join("php.ini");
    if ini.is_file() {
        return Ok(ini);
    }

    // On Windows zips, php.ini-development is bundled — use it as a base if present
    let dev_template = base.join("php.ini-development");
    let mut content = if dev_template.is_file() {
        fs::read_to_string(&dev_template).unwrap_or_default()
    } else {
        "; Managed by Workbench\n".to_string()
    };

    if base.join("ext").is_dir() && !content.contains("extension_dir = \"ext\"") {
        content.push_str("\n; Workbench extension directory\nextension_dir = \"ext\"\n");
    }

    // Apply sensible local Laravel defaults
    let defaults = PhpIniSettings::default();
    content = apply_ini_settings_to_str(&content, &defaults);

    fs::write(&ini, content).map_err(|e| format!("Failed creating {}: {}", ini.display(), e))?;
    Ok(ini)
}

fn query_loaded_modules(php_bin: &Path, php_dir: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut cmd = Command::new(php_bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd
        .arg("-m")
        .env("PHPRC", php_dir)
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('[') {
                    continue;
                }
                let normalized = normalize_ext_name(trimmed);
                set.insert(normalized);
            }
        }
    }
    set
}

fn normalize_ext_name(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    if lower == "zend opcache" {
        return "opcache".to_string();
    }
    lower
        .strip_prefix("php_")
        .unwrap_or(&lower)
        .strip_suffix(".dll")
        .or_else(|| lower.strip_suffix(".so"))
        .unwrap_or(&lower)
        .to_string()
}

fn scan_dynamic_extensions(base: &Path) -> HashSet<String> {
    let mut found = HashSet::new();
    let ext_dirs = [
        base.join("ext"),
        base.join("lib/php/extensions"),
        base.join("modules"),
    ];
    for dir in ext_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".dll") || fname.ends_with(".so") {
                    found.insert(normalize_ext_name(&fname));
                }
            }
        }
    }
    found
}

pub fn parse_ini_enabled_extensions(content: &str) -> HashSet<String> {
    let mut enabled = HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("extension=")
            .or_else(|| trimmed.strip_prefix("extension ="))
            .or_else(|| trimmed.strip_prefix("zend_extension="))
            .or_else(|| trimmed.strip_prefix("zend_extension ="))
        {
            let val = rest
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if !val.is_empty() {
                enabled.insert(normalize_ext_name(val));
            }
        }
    }
    enabled
}

pub fn toggle_extension_in_ini_str(content: &str, ext_name: &str, enable: bool) -> String {
    let target = normalize_ext_name(ext_name);
    let directive = if ZEND_EXTENSIONS.contains(&target.as_str()) {
        "zend_extension"
    } else {
        "extension"
    };

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut matched = false;

    for line in lines.iter_mut() {
        let trimmed = line.trim();
        let uncommented = trimmed.trim_start_matches(';').trim();
        let is_ext_line = uncommented.starts_with("extension=")
            || uncommented.starts_with("extension =")
            || uncommented.starts_with("zend_extension=")
            || uncommented.starts_with("zend_extension =");
        if !is_ext_line {
            continue;
        }
        if let Some((_, rhs)) = uncommented.split_once('=') {
            let val = rhs
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if normalize_ext_name(val) == target {
                matched = true;
                if enable {
                    *line = format!("{}={}", directive, target);
                } else {
                    *line = format!(";{}={}", directive, target);
                }
            }
        }
    }

    if enable && !matched {
        lines.push(format!("{}={}", directive, target));
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn parse_ini_settings(content: &str) -> PhpIniSettings {
    let mut settings = PhpIniSettings::default();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = v.split(';').next().unwrap_or("").trim().to_string();
        match key {
            "memory_limit" => settings.memory_limit = val,
            "upload_max_filesize" => settings.upload_max_filesize = val,
            "post_max_size" => settings.post_max_size = val,
            "max_execution_time" => settings.max_execution_time = val,
            "opcache.enable" => {
                settings.opcache_enable = matches!(val.to_ascii_lowercase().as_str(), "1" | "on" | "true");
            }
            _ => {}
        }
    }
    settings
}

pub fn apply_ini_settings_to_str(content: &str, settings: &PhpIniSettings) -> String {
    let updates: Vec<(&str, String)> = vec![
        ("memory_limit", settings.memory_limit.clone()),
        ("upload_max_filesize", settings.upload_max_filesize.clone()),
        ("post_max_size", settings.post_max_size.clone()),
        ("max_execution_time", settings.max_execution_time.clone()),
        (
            "opcache.enable",
            if settings.opcache_enable { "1" } else { "0" }.to_string(),
        ),
    ];

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut seen: HashSet<&str> = HashSet::new();

    for line in lines.iter_mut() {
        let trimmed = line.trim();
        let uncommented = trimmed.trim_start_matches(';').trim();
        if let Some((k, _)) = uncommented.split_once('=') {
            let key = k.trim();
            if let Some((target_k, val)) = updates.iter().find(|(uk, _)| *uk == key) {
                if seen.insert(*target_k) {
                    *line = format!("{} = {}", target_k, val);
                }
            }
        }
    }

    for (key, val) in updates {
        if !seen.contains(key) {
            lines.push(format!("{} = {}", key, val));
        }
    }

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn validate_ext_name(raw: &str) -> Result<String, String> {
    let normalized = normalize_ext_name(raw);
    if normalized.is_empty()
        || normalized.len() > 64
        || !normalized
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!("Invalid PHP extension name: {}", raw));
    }
    Ok(normalized)
}

fn validate_php_size(raw: &str, label: &str, allow_minus_one: bool) -> Result<String, String> {
    let v = raw.trim();
    if allow_minus_one && v == "-1" {
        return Ok("-1".to_string());
    }
    if v.is_empty() || v.len() > 16 {
        return Err(format!("Invalid {} value: {}", label, raw));
    }
    let bytes = v.as_bytes();
    let (digits, suffix_ok) = if matches!(bytes.last(), Some(b'K' | b'M' | b'G' | b'k' | b'm' | b'g')) {
        (&v[..v.len() - 1], true)
    } else {
        (v, true)
    };
    if !suffix_ok || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid {} value: {}", label, raw));
    }
    Ok(v.to_string())
}

fn validate_php_ini_settings(settings: &PhpIniSettings) -> Result<PhpIniSettings, String> {
    let memory_limit = validate_php_size(&settings.memory_limit, "memory_limit", true)?;
    let upload_max_filesize =
        validate_php_size(&settings.upload_max_filesize, "upload_max_filesize", false)?;
    let post_max_size = validate_php_size(&settings.post_max_size, "post_max_size", false)?;
    let max_exec = settings.max_execution_time.trim();
    if max_exec.is_empty() || max_exec.len() > 8 || !max_exec.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid max_execution_time value: {}",
            settings.max_execution_time
        ));
    }
    Ok(PhpIniSettings {
        memory_limit,
        upload_max_filesize,
        post_max_size,
        max_execution_time: max_exec.to_string(),
        opcache_enable: settings.opcache_enable,
    })
}

pub fn inspect_php_version(version: &str) -> Result<PhpVersionDetails, String> {
    let base = safe_runtime_install_dir("php", version)?;
    if !base.is_dir() {
        return Err(format!("PHP {} is not installed in Workbench", version));
    }
    let ini_path = ensure_php_ini(&base)?;
    let ini_content = fs::read_to_string(&ini_path).unwrap_or_default();

    let loaded = find_php_binary(&base)
        .map(|bin| query_loaded_modules(&bin, &base))
        .unwrap_or_default();
    let dynamic = scan_dynamic_extensions(&base);
    let ini_enabled = parse_ini_enabled_extensions(&ini_content);

    let mut all_names: BTreeMap<String, ()> = BTreeMap::new();
    for &core in LARAVEL_CORE_EXTENSIONS {
        all_names.insert(core.to_string(), ());
    }
    for name in &loaded {
        all_names.insert(name.clone(), ());
    }
    for name in &dynamic {
        all_names.insert(name.clone(), ());
    }

    let mut extensions: Vec<PhpExtensionInfo> = all_names
        .into_keys()
        .map(|name| {
            let is_loaded = loaded.contains(&name);
            let is_dynamic = dynamic.contains(&name);
            let is_enabled_ini = ini_enabled.contains(&name);
            let built_in = is_loaded && !is_dynamic && !is_enabled_ini;
            PhpExtensionInfo {
                is_laravel_core: LARAVEL_CORE_EXTENSIONS.contains(&name.as_str()),
                is_zend: ZEND_EXTENSIONS.contains(&name.as_str()),
                name,
                loaded: is_loaded,
                built_in,
                available_dynamic: is_dynamic,
                enabled_in_ini: is_enabled_ini,
            }
        })
        .collect();

    // Sort Laravel core extensions first, then alphabetically
    extensions.sort_by(|a, b| {
        b.is_laravel_core
            .cmp(&a.is_laravel_core)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(PhpVersionDetails {
        version: version.to_string(),
        ini_path: ini_path.to_string_lossy().to_string(),
        extensions,
        settings: parse_ini_settings(&ini_content),
    })
}

pub fn toggle_php_extension(
    version: &str,
    ext_name: &str,
    enable: bool,
) -> Result<PhpVersionDetails, String> {
    let clean_ext = validate_ext_name(ext_name)?;
    let base = safe_runtime_install_dir("php", version)?;
    let ini_path = ensure_php_ini(&base)?;
    let content = fs::read_to_string(&ini_path).unwrap_or_default();
    let updated = toggle_extension_in_ini_str(&content, &clean_ext, enable);
    fs::write(&ini_path, updated)
        .map_err(|e| format!("Failed updating {}: {}", ini_path.display(), e))?;
    inspect_php_version(version)
}

pub fn update_php_ini_settings(
    version: &str,
    settings: PhpIniSettings,
) -> Result<PhpVersionDetails, String> {
    let clean_settings = validate_php_ini_settings(&settings)?;
    let base = safe_runtime_install_dir("php", version)?;
    let ini_path = ensure_php_ini(&base)?;
    let content = fs::read_to_string(&ini_path).unwrap_or_default();
    let updated = apply_ini_settings_to_str(&content, &clean_settings);
    fs::write(&ini_path, updated)
        .map_err(|e| format!("Failed updating {}: {}", ini_path.display(), e))?;
    inspect_php_version(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_extensions_and_zend_extensions_in_ini() {
        let initial = ";extension=pdo_mysql\n;extension=gd\n";
        let enabled = toggle_extension_in_ini_str(initial, "pdo_mysql", true);
        assert!(enabled.contains("extension=pdo_mysql"));
        assert!(parse_ini_enabled_extensions(&enabled).contains("pdo_mysql"));

        let with_xdebug = toggle_extension_in_ini_str(&enabled, "xdebug", true);
        assert!(with_xdebug.contains("zend_extension=xdebug"));

        let disabled = toggle_extension_in_ini_str(&with_xdebug, "pdo_mysql", false);
        assert!(!parse_ini_enabled_extensions(&disabled).contains("pdo_mysql"));
    }

    #[test]
    fn updates_and_parses_php_ini_directives() {
        let initial = "memory_limit = 128M\n;upload_max_filesize = 2M\n";
        let next = PhpIniSettings {
            memory_limit: "1G".to_string(),
            upload_max_filesize: "128M".to_string(),
            post_max_size: "128M".to_string(),
            max_execution_time: "300".to_string(),
            opcache_enable: false,
        };
        let updated = apply_ini_settings_to_str(initial, &next);
        let parsed = parse_ini_settings(&updated);
        assert_eq!(parsed.memory_limit, "1G");
        assert_eq!(parsed.upload_max_filesize, "128M");
        assert_eq!(parsed.post_max_size, "128M");
        assert_eq!(parsed.max_execution_time, "300");
        assert!(!parsed.opcache_enable);
    }
}
