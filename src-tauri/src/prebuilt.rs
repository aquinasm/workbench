use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tar::Archive;

#[derive(Debug, Deserialize)]
struct BrewFormulaApi {
    name: String,
    #[serde(default)]
    dependencies: Vec<String>,
    bottle: Option<BrewBottleWrapper>,
}

#[derive(Debug, Deserialize)]
struct BrewBottleWrapper {
    stable: Option<BrewBottleStable>,
}

#[derive(Debug, Deserialize)]
struct BrewBottleStable {
    files: HashMap<String, BrewBottleFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct BrewBottleFile {
    url: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct GhcrTokenResp {
    token: String,
}

/// Pure-Rust live resolver for `dl.static-php.dev` so PHP downloads never 404 when point releases change.
pub async fn resolve_static_php_url(
    client: &reqwest::Client,
    primary_url: &str,
    version: &str,
    platform_key: &str,
) -> Result<String, String> {
    if let Ok(resp) = client.head(primary_url).send().await {
        if resp.status().is_success() {
            return Ok(primary_url.to_string());
        }
    }

    let arch_suffix = match platform_key {
        "macos-aarch64" => "macos-aarch64",
        "linux-x86_64" => "linux-x86_64",
        _ => return Ok(primary_url.to_string()),
    };

    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("8");
    let minor = parts.next().unwrap_or("4");
    let prefix = format!("php-{}.{}.", major, minor);
    let suffix = format!("-cli-{}.tar.gz", arch_suffix);

    let index_url = "https://dl.static-php.dev/static-php-cli/bulk/";
    let html = client
        .get(index_url)
        .send()
        .await
        .map_err(|e| format!("Failed querying static-php index: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed reading static-php index: {}", e))?;

    let mut best_patch: Option<(u64, String)> = None;
    for token in html.split('"') {
        let name = token.trim_start_matches("./");
        if name.starts_with(&prefix) && name.ends_with(&suffix) {
            let patch_str = &name[prefix.len()..name.len() - suffix.len()];
            if let Ok(patch_num) = patch_str.parse::<u64>() {
                if best_patch.as_ref().map(|(p, _)| patch_num > *p).unwrap_or(true) {
                    best_patch = Some((patch_num, name.to_string()));
                }
            }
        }
    }

    if let Some((_, filename)) = best_patch {
        return Ok(format!("{}{}", index_url, filename));
    }

    Err(format!(
        "No prebuilt static PHP {}.{} archive found for {}",
        major, minor, platform_key
    ))
}

/// Pure-Rust fallback for Windows PHP archives (checks both `/releases/` and `/releases/archives/`).
pub async fn resolve_windows_php_url(client: &reqwest::Client, primary_url: &str) -> String {
    if let Ok(resp) = client.head(primary_url).send().await {
        if resp.status().is_success() {
            return primary_url.to_string();
        }
    }
    let alt = if primary_url.contains("/releases/archives/") {
        primary_url.replace("/releases/archives/", "/releases/")
    } else {
        primary_url.replace("/releases/", "/releases/archives/")
    };
    if let Ok(resp) = client.head(&alt).send().await {
        if resp.status().is_success() {
            return alt;
        }
    }
    primary_url.to_string()
}

fn select_bottle_file(
    files: &HashMap<String, BrewBottleFile>,
    platform_key: &str,
) -> Option<BrewBottleFile> {
    match platform_key {
        "macos-aarch64" => {
            for tag in [
                "arm64_golden_gate",
                "arm64_tahoe",
                "arm64_sequoia",
                "arm64_sonoma",
                "arm64_ventura",
                "arm64_monterey",
                "all",
            ] {
                if let Some(f) = files.get(tag) {
                    return Some(f.clone());
                }
            }
            files
                .iter()
                .find(|(k, _)| k.starts_with("arm64_"))
                .map(|(_, v)| v.clone())
        }
        "linux-x86_64" => {
            for tag in ["x86_64_linux", "all"] {
                if let Some(f) = files.get(tag) {
                    return Some(f.clone());
                }
            }
            files
                .iter()
                .find(|(k, _)| k.ends_with("_linux"))
                .map(|(_, v)| v.clone())
        }
        _ => None,
    }
}

fn formula_name_from_api_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("formula")
        .trim_end_matches(".json")
        .to_string()
}

fn ghcr_repo_from_blob_url(blob_url: &str) -> Option<String> {
    // e.g. https://ghcr.io/v2/homebrew/core/redis/blobs/sha256:... -> homebrew/core/redis
    let after_v2 = blob_url.strip_prefix("https://ghcr.io/v2/")?;
    let (repo, _) = after_v2.split_once("/blobs/")?;
    Some(repo.to_string())
}

async fn fetch_formula_meta(
    client: &reqwest::Client,
    formula: &str,
) -> Result<BrewFormulaApi, String> {
    let url = if formula.starts_with("https://") {
        formula.to_string()
    } else {
        format!("https://formulae.brew.sh/api/formula/{}.json", formula)
    };
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed fetching metadata for {}: {}", formula, e))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Metadata request for {} returned HTTP {}",
            formula,
            resp.status()
        ));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading metadata for {}: {}", formula, e))?;
    serde_json::from_str::<BrewFormulaApi>(&body)
        .map_err(|e| format!("Invalid formula metadata for {}: {}", formula, e))
}

/// Resolves the transitive bottle dependency tree in topological order (dependencies first, root formula last).
async fn resolve_bottle_tree(
    client: &reqwest::Client,
    root_api_url: &str,
    platform_key: &str,
) -> Result<Vec<(String, BrewBottleFile)>, String> {
    let root_name = formula_name_from_api_url(root_api_url);
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root_name);

    let mut meta_map: HashMap<String, (Vec<String>, BrewBottleFile)> = HashMap::new();

    while let Some(curr) = queue.pop_front() {
        if !visited.insert(curr.clone()) {
            continue;
        }
        let meta = fetch_formula_meta(client, &curr).await?;
        let files = meta
            .bottle
            .and_then(|b| b.stable)
            .map(|s| s.files)
            .ok_or_else(|| format!("No prebuilt bottle table found for {}", meta.name))?;
        let bottle = select_bottle_file(&files, platform_key).ok_or_else(|| {
            format!(
                "No prebuilt {} binary bottle available for {}",
                meta.name, platform_key
            )
        })?;
        for dep in &meta.dependencies {
            if !visited.contains(dep) {
                queue.push_back(dep.clone());
            }
        }
        order.push(curr.clone());
        meta_map.insert(curr, (meta.dependencies, bottle));
    }

    // Reverse BFS queue so leaf dependencies unpack first and the root formula unpacks last
    order.reverse();
    let mut result = Vec::with_capacity(order.len());
    for name in order {
        if let Some((_, bottle)) = meta_map.remove(&name) {
            result.push((name, bottle));
        }
    }
    Ok(result)
}

fn sanitize_bottle_rel(path: &Path) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    let mut skipped = 0usize;
    for comp in path.components() {
        match comp {
            Component::Normal(part) => {
                if skipped < 2 {
                    skipped += 1;
                    continue;
                }
                clean.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn unpack_bottle_tar_gz(archive_path: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed opening bottle {}: {}", archive_path.display(), e))?;
    let mut archive = Archive::new(GzDecoder::new(file));

    for entry_res in archive
        .entries()
        .map_err(|e| format!("Failed reading bottle entries: {}", e))?
    {
        let mut entry = entry_res.map_err(|e| format!("Invalid bottle entry: {}", e))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_character_special()
            || entry_type.is_block_special()
            || entry_type.is_fifo()
        {
            continue;
        }
        let raw_path = entry
            .path()
            .map_err(|e| format!("Invalid path in bottle: {}", e))?
            .into_owned();
        let Some(rel) = sanitize_bottle_rel(&raw_path) else {
            continue;
        };
        // Skip internal .brew formula recipe metadata
        if rel.starts_with(".brew") {
            continue;
        }

        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating dir {}: {}", parent.display(), e))?;
        }

        if entry_type.is_symlink() || entry_type.is_hard_link() {
            // Replace existing file/symlink if a dependency already placed one
            let _ = fs::remove_file(&out_path);
            if let Ok(Some(link_target)) = entry.link_name() {
                let target_owned = link_target.into_owned();
                if target_owned.is_absolute() {
                    continue;
                }
                #[cfg(unix)]
                {
                    let _ = std::os::unix::fs::symlink(&target_owned, &out_path);
                }
            }
            continue;
        }

        if out_path.exists() && !entry_type.is_dir() {
            let _ = fs::remove_file(&out_path);
        }
        let _ = entry.unpack(&out_path);
    }
    Ok(())
}

/// Rewrites `@@HOMEBREW_PREFIX@@` / `@@HOMEBREW_CELLAR@@` placeholders in extracted configs and Mach-O headers
/// so they point to `final_dir` (the permanent install location) rather than the temporary staging directory.
fn relocate_extracted_prefix(extract_dir: &Path, final_dir: &Path) {
    let final_str = final_dir.to_string_lossy().to_string();

    // 1. Rewrite text config & wrapper script placeholders in bin/, sbin/, etc/, lib/pkgconfig/
    for sub in ["bin", "sbin", "etc", "lib/pkgconfig"] {
        let dir = extract_dir.join(sub);
        if !dir.is_dir() {
            continue;
        }
        for entry in jwalk::WalkDir::new(&dir).max_depth(3).into_iter().flatten() {
            let path = entry.path();
            if !path.is_file() || entry.path_is_symlink() {
                continue;
            }
            let Ok(meta) = path.metadata() else {
                continue;
            };
            if meta.len() > 2 * 1024 * 1024 {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                if text.contains("@@HOMEBREW_") {
                    let mut patched = text.replace("@@HOMEBREW_PREFIX@@", &final_str);
                    // Also replace @@HOMEBREW_CELLAR@@/<pkg>/<ver> with final_str
                    while let Some(idx) = patched.find("@@HOMEBREW_CELLAR@@/") {
                        let rest = &patched[idx + "@@HOMEBREW_CELLAR@@/".len()..];
                        let mut slash_count = 0;
                        let mut end_offset = 0;
                        for (i, ch) in rest.char_indices() {
                            if ch == '/' {
                                slash_count += 1;
                                if slash_count == 2 {
                                    end_offset = i;
                                    break;
                                }
                            } else if ch.is_whitespace() || ch == '"' || ch == '\'' {
                                end_offset = i;
                                break;
                            }
                        }
                        if end_offset > 0 {
                            let to_replace =
                                &patched[idx..idx + "@@HOMEBREW_CELLAR@@/".len() + end_offset];
                            patched = patched.replace(to_replace, &final_str);
                        } else {
                            patched = patched.replace("@@HOMEBREW_CELLAR@@", &final_str);
                            break;
                        }
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let orig_mode = meta.permissions().mode();
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
                        let _ = fs::write(&path, patched);
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(orig_mode));
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = fs::write(&path, patched);
                    }
                }
            }
        }
    }

    // 2. On macOS, patch Mach-O load commands referencing @@HOMEBREW_ or /opt/homebrew/ to point to final_dir/lib/...
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let extract_lib_dir = extract_dir.join("lib");
        let final_lib_dir = final_dir.join("lib");

        // Index all dylib/so filenames under extract_dir/lib so nested libraries resolve to their exact final path
        let mut lib_index: HashMap<String, PathBuf> = HashMap::new();
        if extract_lib_dir.is_dir() {
            for entry in jwalk::WalkDir::new(&extract_lib_dir)
                .max_depth(4)
                .into_iter()
                .flatten()
            {
                let p = entry.path();
                if let (Some(name), Ok(rel)) = (
                    p.file_name().and_then(|n| n.to_str()),
                    p.strip_prefix(extract_dir),
                ) {
                    if name.ends_with(".dylib") || name.ends_with(".so") {
                        lib_index
                            .entry(name.to_string())
                            .or_insert_with(|| final_dir.join(rel));
                    }
                }
            }
        }

        for sub in ["bin", "sbin", "lib", "libexec"] {
            let dir = extract_dir.join(sub);
            if !dir.is_dir() {
                continue;
            }
            for entry in jwalk::WalkDir::new(&dir).max_depth(4).into_iter().flatten() {
                let path = entry.path();
                if !path.is_file() || entry.path_is_symlink() {
                    continue;
                }
                let Ok(out) = Command::new("otool").arg("-L").arg(&path).output() else {
                    continue;
                };
                if !out.status.success() {
                    continue;
                }
                let text = String::from_utf8_lossy(&out.stdout);
                let mut changes: Vec<(String, String)> = Vec::new();
                for line in text.lines().skip(1) {
                    let dep = line
                        .trim()
                        .split(" (compatibility ")
                        .next()
                        .unwrap_or("")
                        .trim();
                    if dep.starts_with("@@HOMEBREW_") || dep.starts_with("/opt/homebrew/") {
                        if let Some(leaf) = dep.rsplit('/').next() {
                            let resolved = if extract_lib_dir.join(leaf).exists() {
                                final_lib_dir.join(leaf).to_string_lossy().to_string()
                            } else if let Some(indexed) = lib_index.get(leaf) {
                                indexed.to_string_lossy().to_string()
                            } else {
                                final_lib_dir.join(leaf).to_string_lossy().to_string()
                            };
                            changes.push((dep.to_string(), resolved));
                        }
                    }
                }
                let is_dylib = matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("dylib") | Some("so")
                );
                if !changes.is_empty() || is_dylib {
                    if let Ok(meta) = path.metadata() {
                        let orig_mode = meta.permissions().mode();
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(orig_mode | 0o200));
                    }
                    let mut cmd = Command::new("install_name_tool");
                    if is_dylib {
                        if let Ok(rel) = path.strip_prefix(extract_dir) {
                            cmd.arg("-id").arg(final_dir.join(rel));
                        }
                    }
                    for (old, new) in &changes {
                        cmd.arg("-change").arg(old).arg(new);
                    }
                    cmd.arg(&path);
                    let _ = cmd.output();
                    let _ = Command::new("codesign")
                        .args(["--force", "--sign", "-"])
                        .arg(&path)
                        .output();
                }
            }
        }
    }
}

/// Pure-Rust installer that resolves a prebuilt bottle and its dependencies from `formulae.brew.sh` + `ghcr.io`,
/// verifies every SHA-256 checksum, extracts into `extract_dir`, and relocates shared library paths to `final_dir`.
pub async fn install_homebrew_bottle_tree<F>(
    client: &reqwest::Client,
    formula_api_url: &str,
    platform_key: &str,
    staging_dir: &Path,
    extract_dir: &Path,
    final_dir: &Path,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u8, u64, u64),
{
    on_progress(2, 0, 0);
    let tree = resolve_bottle_tree(client, formula_api_url, platform_key).await?;
    let total_bottles = tree.len().max(1);
    let mut token_cache: HashMap<String, String> = HashMap::new();

    for (idx, (formula_name, bottle)) in tree.into_iter().enumerate() {
        let repo = ghcr_repo_from_blob_url(&bottle.url)
            .ok_or_else(|| format!("Invalid GHCR bottle URL for {}: {}", formula_name, bottle.url))?;

        let token = if let Some(t) = token_cache.get(&repo) {
            t.clone()
        } else {
            let token_url = format!(
                "https://ghcr.io/token?service=ghcr.io&scope=repository:{}:pull",
                repo
            );
            let t_body = client
                .get(&token_url)
                .send()
                .await
                .map_err(|e| format!("Failed requesting GHCR token for {}: {}", formula_name, e))?
                .text()
                .await
                .map_err(|e| format!("Failed reading GHCR token for {}: {}", formula_name, e))?;
            let t_resp: GhcrTokenResp = serde_json::from_str(&t_body)
                .map_err(|e| format!("Invalid GHCR token response for {}: {}", formula_name, e))?;
            token_cache.insert(repo, t_resp.token.clone());
            t_resp.token
        };

        let resp = client
            .get(&bottle.url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Failed downloading bottle for {}: {}", formula_name, e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Bottle download for {} returned HTTP {}",
                formula_name,
                resp.status()
            ));
        }

        let content_len = resp.content_length().unwrap_or(0);
        let safe_name = formula_name.replace(['/', '@'], "_");
        let tar_path = staging_dir.join(format!("bottle-{}-{}.tar.gz", idx, safe_name));
        let mut file = File::create(&tar_path)
            .map_err(|e| format!("Failed creating temp bottle file: {}", e))?;

        let mut stream = resp.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| format!("Stream read error: {}", e))?;
            hasher.update(&chunk);
            file.write_all(&chunk)
                .map_err(|e| format!("Disk write error: {}", e))?;
            downloaded += chunk.len() as u64;

            let bottle_frac = if content_len > 0 {
                (downloaded as f64 / content_len as f64).min(1.0)
            } else {
                0.5
            };
            let overall_pct = (((idx as f64 + bottle_frac) / total_bottles as f64) * 92.0) as u8;
            on_progress(overall_pct.clamp(3, 95), downloaded, content_len);
        }
        drop(file);

        let actual_sha: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        if !actual_sha.eq_ignore_ascii_case(bottle.sha256.trim()) {
            return Err(format!(
                "SHA-256 mismatch for {} bottle (expected {}, got {})",
                formula_name, bottle.sha256, actual_sha
            ));
        }

        let tp = tar_path.clone();
        let ed = extract_dir.to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            unpack_bottle_tar_gz(&tp, &ed)?;
            let _ = fs::remove_file(&tp);
            Ok(())
        })
        .await
        .map_err(|e| format!("Bottle unpack task error: {}", e))??;
    }

    on_progress(97, 0, 0);
    let ed_reloc = extract_dir.to_path_buf();
    let fd_reloc = final_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        relocate_extracted_prefix(&ed_reloc, &fd_reloc);
    })
    .await
    .map_err(|e| format!("Bottle relocation task error: {}", e))?;

    Ok(())
}
