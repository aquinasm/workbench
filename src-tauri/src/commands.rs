use crate::config::{
    db_path, is_valid_dir, load_config, save_config, shims_dir, DbServerProfile, DeleteMode,
    LocalSite, WorkbenchConfig,
};
use crate::databases::{
    assign_user_to_database, create_database, create_db_user, drop_database, drop_db_user,
    export_database, export_server, import_database, import_server, inspect_db_server,
    inspect_table, list_database_tables, remove_db_server, revoke_user_from_database,
    save_db_server, DbServerInspection, DbTableInfo, DbTablePreview,
};
use crate::db::{CleanupStats, Database};
use crate::runtimes::php_ext::{
    inspect_php_version, toggle_php_extension, update_php_ini_settings, PhpIniSettings,
    PhpVersionDetails,
};
use crate::runtimes::{
    get_project_pins, get_rustup_status, install_runtime, list_runtime_catalog, run_rustup_action,
    set_default_runtime, set_project_pin, sync_shims, uninstall_runtime, RuntimeVersionEntry,
    RustupStatus, ShimSyncReport,
};
use crate::services::{
    add_local_site, generate_apache_conf, generate_caddyfile, get_network_status, get_service_logs,
    install_service, list_services, reload_caddy_or_restart, remove_local_site, restart_service,
    start_all_services, start_service, stop_all_services, stop_service, sync_hosts_file,
    trust_local_ca, uninstall_service, NetworkStatus, ServiceLogsResponse, ServiceStatusInfo,
    SharedSupervisor,
};
use crate::shell::{
    clean_uninstall_workbench, detect_system_tools, get_shell_status, install_shell_integration,
    remove_shell_integration, DetectedTool, ShellIntegrationStatus, UninstallReport,
};
use crate::storage::{
    clean_global_cache, discover_projects_fast, hibernate_project, scan_global_caches,
    scan_workspaces, GlobalCacheInfo, HibernateResult, ScanResult,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct WakeLogEvent {
    pub project_path: String,
    pub line: String,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WakeDoneEvent {
    pub project_path: String,
    pub success: bool,
    pub message: String,
}

#[tauri::command]
pub fn get_config() -> Result<WorkbenchConfig, String> {
    load_config()
}

#[tauri::command]
pub fn update_config(mut config: WorkbenchConfig) -> Result<WorkbenchConfig, String> {
    // Preserve backend-managed executable paths so frontend cannot inject arbitrary shim binaries
    if let Ok(existing) = load_config() {
        config.external_paths = existing.external_paths;
    }
    save_config(&config)?;
    Ok(config)
}

#[tauri::command]
pub async fn pick_workspace_folder(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Workspace Folder to Scan")
        .pick_folder(move |folder_path| {
            let resolved = folder_path.and_then(|fp| fp.into_path().ok()).map(|p| {
                p.to_string_lossy().to_string()
            });
            let _ = tx.send(resolved);
        });

    rx.await
        .map_err(|_| "Folder picker was cancelled unexpectedly".to_string())
}

#[tauri::command]
pub fn add_scan_root(path: String) -> Result<WorkbenchConfig, String> {
    if !is_valid_dir(&path) {
        return Err(format!("Directory does not exist: {}", path));
    }
    let mut cfg = load_config()?;
    if !cfg.scan_roots.contains(&path) {
        cfg.scan_roots.push(path);
        save_config(&cfg)?;
    }
    Ok(cfg)
}

#[tauri::command]
pub fn remove_scan_root(path: String) -> Result<WorkbenchConfig, String> {
    let mut cfg = load_config()?;
    cfg.scan_roots.retain(|r| r != &path);
    save_config(&cfg)?;
    Ok(cfg)
}

#[tauri::command]
pub async fn discover_projects_fast_cmd() -> Result<ScanResult, String> {
    let cfg = load_config()?;
    tokio::task::spawn_blocking(move || discover_projects_fast(&cfg.scan_roots))
        .await
        .map_err(|e| format!("Fast discovery failed: {}", e))
}

#[tauri::command]
pub async fn scan_projects() -> Result<ScanResult, String> {
    let cfg = load_config()?;
    tokio::task::spawn_blocking(move || scan_workspaces(&cfg.scan_roots))
        .await
        .map_err(|e| format!("Scan task failed: {}", e))
}

#[tauri::command]
pub async fn hibernate_project_cmd(
    project_path: String,
    selected_artifacts: Option<Vec<String>>,
    delete_mode: Option<DeleteMode>,
) -> Result<HibernateResult, String> {
    let cfg = load_config()?;
    let mode = delete_mode.unwrap_or(cfg.delete_mode);
    let mode_label = match mode {
        DeleteMode::Trash => "trash",
        DeleteMode::Permanent => "permanent",
    }
    .to_string();

    tokio::task::spawn_blocking(move || {
        let res = hibernate_project(&project_path, selected_artifacts.as_deref(), &mode)?;
        if res.bytes_reclaimed > 0 {
            if let Ok(db) = Database::open(&db_path()) {
                let _ = db.record_cleanup(
                    &res.project_path,
                    &res.project_name,
                    "project_hibernate",
                    res.bytes_reclaimed,
                    &mode_label,
                );
            }
        }
        Ok(res)
    })
    .await
    .map_err(|e| format!("Hibernate task failed: {}", e))?
}

#[tauri::command]
pub async fn hibernate_projects_bulk(
    project_paths: Vec<String>,
    delete_mode: Option<DeleteMode>,
) -> Result<Vec<HibernateResult>, String> {
    let cfg = load_config()?;
    let mode = delete_mode.unwrap_or(cfg.delete_mode);
    let mode_label = match mode {
        DeleteMode::Trash => "trash",
        DeleteMode::Permanent => "permanent",
    }
    .to_string();

    tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        let db = Database::open(&db_path()).ok();
        for p in project_paths {
            match hibernate_project(&p, None, &mode) {
                Ok(res) => {
                    if res.bytes_reclaimed > 0 {
                        if let Some(ref db_conn) = db {
                            let _ = db_conn.record_cleanup(
                                &res.project_path,
                                &res.project_name,
                                "project_hibernate",
                                res.bytes_reclaimed,
                                &mode_label,
                            );
                        }
                    }
                    results.push(res);
                }
                Err(err) => {
                    results.push(HibernateResult {
                        project_path: p.clone(),
                        project_name: p,
                        bytes_reclaimed: 0,
                        deleted_artifacts: vec![],
                        errors: vec![err],
                    });
                }
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| format!("Bulk hibernate task failed: {}", e))?
}

#[tauri::command]
pub async fn wake_project_cmd(app: AppHandle, project_path: String) -> Result<(), String> {
    let proj_for_inspect = project_path.clone();
    let info = tokio::task::spawn_blocking(move || {
        crate::storage::inspect_project_fast(Path::new(&proj_for_inspect))
    })
    .await
    .map_err(|e| format!("Project inspection failed: {}", e))?
    .ok_or_else(|| format!("Project not recognized at {}", project_path))?;

    if info.wake_commands.is_empty() {
        return Err("No restore commands detected for this project.".to_string());
    }

    let cmds = info.wake_commands.clone();
    let proj = project_path.clone();

    tokio::spawn(async move {
        let mut all_ok = true;
        for cmd_str in cmds {
            let _ = app.emit(
                "wake-log",
                WakeLogEvent {
                    project_path: proj.clone(),
                    line: format!("$ {}", cmd_str),
                    stream: "info".to_string(),
                },
            );

            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            let Some((program, prog_args)) = parts.split_first() else {
                continue;
            };

            let mut command = if cfg!(target_os = "windows") {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(program).args(prog_args);
                c
            } else {
                let mut c = Command::new(program);
                c.args(prog_args);
                c
            };

            command
                .current_dir(&proj)
                .kill_on_drop(true)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            // Prepend Workbench shims to PATH so Wake runs with pinned project versions
            let shims = shims_dir();
            if shims.is_dir() {
                let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
                let existing = std::env::var("PATH").unwrap_or_default();
                command.env("PATH", format!("{}{}{}", shims.display(), sep, existing));
            }

            match command.spawn() {
                Ok(mut child) => {
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();

                    let app_out = app.clone();
                    let proj_out = proj.clone();
                    let out_task = tokio::spawn(async move {
                        if let Some(out) = stdout {
                            let mut reader = BufReader::new(out).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                let _ = app_out.emit(
                                    "wake-log",
                                    WakeLogEvent {
                                        project_path: proj_out.clone(),
                                        line,
                                        stream: "stdout".to_string(),
                                    },
                                );
                            }
                        }
                    });

                    let app_err = app.clone();
                    let proj_err = proj.clone();
                    let err_task = tokio::spawn(async move {
                        if let Some(err) = stderr {
                            let mut reader = BufReader::new(err).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                let _ = app_err.emit(
                                    "wake-log",
                                    WakeLogEvent {
                                        project_path: proj_err.clone(),
                                        line,
                                        stream: "stderr".to_string(),
                                    },
                                );
                            }
                        }
                    });

                    let status = child.wait().await;
                    let _ = out_task.await;
                    let _ = err_task.await;

                    match status {
                        Ok(s) if s.success() => {}
                        Ok(s) => {
                            all_ok = false;
                            let _ = app.emit(
                                "wake-log",
                                WakeLogEvent {
                                    project_path: proj.clone(),
                                    line: format!("Command exited with status {}", s),
                                    stream: "stderr".to_string(),
                                },
                            );
                            break;
                        }
                        Err(e) => {
                            all_ok = false;
                            let _ = app.emit(
                                "wake-log",
                                WakeLogEvent {
                                    project_path: proj.clone(),
                                    line: format!("Failed waiting for command: {}", e),
                                    stream: "stderr".to_string(),
                                },
                            );
                            break;
                        }
                    }
                }
                Err(e) => {
                    all_ok = false;
                    let _ = app.emit(
                        "wake-log",
                        WakeLogEvent {
                            project_path: proj.clone(),
                            line: format!("Failed to spawn '{}': {}", cmd_str, e),
                            stream: "stderr".to_string(),
                        },
                    );
                    break;
                }
            }
        }

        let _ = app.emit(
            "wake-done",
            WakeDoneEvent {
                project_path: proj,
                success: all_ok,
                message: if all_ok {
                    "Project dependencies restored successfully.".to_string()
                } else {
                    "Wake finished with errors. Check terminal output.".to_string()
                },
            },
        );
    });

    Ok(())
}

#[tauri::command]
pub async fn scan_global_caches_cmd() -> Result<Vec<GlobalCacheInfo>, String> {
    tokio::task::spawn_blocking(scan_global_caches)
        .await
        .map_err(|e| format!("Cache scan failed: {}", e))
}

#[tauri::command]
pub async fn clean_global_cache_cmd(
    cache_id: String,
    delete_mode: Option<DeleteMode>,
) -> Result<u64, String> {
    let cfg = load_config()?;
    let mode = delete_mode.unwrap_or(cfg.delete_mode);
    let mode_label = match mode {
        DeleteMode::Trash => "trash",
        DeleteMode::Permanent => "permanent",
    }
    .to_string();

    tokio::task::spawn_blocking(move || {
        let (name, bytes) = clean_global_cache(&cache_id, &mode)?;
        if bytes > 0 {
            if let Ok(db) = Database::open(&db_path()) {
                let _ = db.record_cleanup(&cache_id, &name, "global_cache", bytes, &mode_label);
            }
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| format!("Clean cache failed: {}", e))?
}

#[tauri::command]
pub fn get_cleanup_stats_cmd() -> Result<CleanupStats, String> {
    let _ = crate::config::ensure_layout()?;
    let db = Database::open(&db_path())?;
    db.get_cleanup_stats(15)
}

#[tauri::command]
pub fn get_shell_status_cmd() -> ShellIntegrationStatus {
    get_shell_status()
}

#[tauri::command]
pub fn install_shell_integration_cmd() -> Result<ShellIntegrationStatus, String> {
    install_shell_integration()
}

#[tauri::command]
pub fn remove_shell_integration_cmd() -> Result<Vec<String>, String> {
    remove_shell_integration()
}

#[tauri::command]
pub async fn clean_uninstall_cmd(
    sup: State<'_, SharedSupervisor>,
) -> Result<UninstallReport, String> {
    stop_all_services(&sup).await;
    tokio::task::spawn_blocking(clean_uninstall_workbench)
        .await
        .map_err(|e| format!("Clean uninstall task failed: {}", e))?
}

#[tauri::command]
pub async fn detect_system_tools_cmd() -> Result<Vec<DetectedTool>, String> {
    tokio::task::spawn_blocking(detect_system_tools)
        .await
        .map_err(|e| format!("Tool detection failed: {}", e))
}

#[tauri::command]
pub fn open_in_terminal(path: String) -> Result<(), String> {
    if !is_valid_dir(&path) {
        return Err(format!("Directory does not exist: {}", path));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal", &path])
            .spawn()
            .map_err(|e| format!("Failed to open Terminal: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        let wt = std::process::Command::new("wt")
            .args(["-d", &path])
            .spawn();
        if wt.is_err() {
            std::process::Command::new("cmd")
                .current_dir(&path)
                .args(["/C", "start", "cmd"])
                .spawn()
                .map_err(|e| format!("Failed to open terminal: {}", e))?;
        }
    }
    #[cfg(target_os = "linux")]
    {
        let tried = std::process::Command::new("x-terminal-emulator")
            .current_dir(&path)
            .spawn()
            .or_else(|_| {
                std::process::Command::new("gnome-terminal")
                    .args(["--working-directory", &path])
                    .spawn()
            })
            .or_else(|_| {
                std::process::Command::new("konsole")
                    .args(["--workdir", &path])
                    .spawn()
            });
        tried.map_err(|e| format!("Failed to open terminal: {}", e))?;
    }
    Ok(())
}

// --- Phase 2: Multi-Version Runtime Manager & Shims ---

#[tauri::command]
pub async fn list_runtimes_cmd() -> Result<Vec<RuntimeVersionEntry>, String> {
    tokio::task::spawn_blocking(list_runtime_catalog)
        .await
        .map_err(|e| format!("Failed listing runtimes: {}", e))?
}

#[tauri::command]
pub async fn install_runtime_cmd(
    app: AppHandle,
    tool: String,
    version: String,
) -> Result<RuntimeVersionEntry, String> {
    install_runtime(&app, &tool, &version).await
}

#[tauri::command]
pub async fn uninstall_runtime_cmd(tool: String, version: String) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = uninstall_runtime(&tool, &version)?;
        if bytes > 0 {
            if let Ok(db) = Database::open(&db_path()) {
                let _ = db.record_cleanup(
                    &format!("runtime:{}:{}", tool, version),
                    &format!("{} {}", tool, version),
                    "runtime_uninstall",
                    bytes,
                    "permanent",
                );
            }
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| format!("Uninstall runtime task failed: {}", e))?
}

#[tauri::command]
pub fn set_default_runtime_cmd(tool: String, version: String) -> Result<(), String> {
    set_default_runtime(&tool, &version)
}

#[tauri::command]
pub fn sync_shims_cmd() -> Result<ShimSyncReport, String> {
    sync_shims()
}

#[tauri::command]
pub async fn get_rustup_status_cmd() -> Result<RustupStatus, String> {
    tokio::task::spawn_blocking(get_rustup_status)
        .await
        .map_err(|e| format!("Failed checking rustup: {}", e))
}

fn validate_rustup_toolchain(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.starts_with('-') {
        return Err("Invalid rustup toolchain name.".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("Invalid characters in rustup toolchain name.".to_string());
    }
    Ok(trimmed.to_string())
}

#[tauri::command]
pub async fn install_rustup_toolchain_cmd(toolchain: String) -> Result<String, String> {
    let tc = validate_rustup_toolchain(&toolchain)?;
    tokio::task::spawn_blocking(move || run_rustup_action(&["toolchain", "install", &tc]))
        .await
        .map_err(|e| format!("rustup install failed: {}", e))?
}

#[tauri::command]
pub async fn set_default_rustup_toolchain_cmd(toolchain: String) -> Result<String, String> {
    let tc = validate_rustup_toolchain(&toolchain)?;
    tokio::task::spawn_blocking(move || run_rustup_action(&["default", &tc]))
        .await
        .map_err(|e| format!("rustup default failed: {}", e))?
}

#[tauri::command]
pub async fn uninstall_rustup_toolchain_cmd(toolchain: String) -> Result<String, String> {
    let tc = validate_rustup_toolchain(&toolchain)?;
    tokio::task::spawn_blocking(move || run_rustup_action(&["toolchain", "uninstall", &tc]))
        .await
        .map_err(|e| format!("rustup uninstall failed: {}", e))?
}

#[tauri::command]
pub fn get_project_pins_cmd(project_path: String) -> Result<HashMap<String, String>, String> {
    get_project_pins(&project_path)
}

#[tauri::command]
pub fn set_project_pin_cmd(
    project_path: String,
    tool: String,
    version: Option<String>,
) -> Result<HashMap<String, String>, String> {
    set_project_pin(&project_path, &tool, version.as_deref())
}

// --- Phase 3: Local Services Supervisor, Web Servers & phpMyAdmin ---

#[tauri::command]
pub async fn list_services_cmd(
    sup: State<'_, SharedSupervisor>,
) -> Result<Vec<ServiceStatusInfo>, String> {
    list_services(&sup).await
}

#[tauri::command]
pub async fn install_service_cmd(app: AppHandle, service_id: String) -> Result<(), String> {
    let res = install_service(&app, &service_id).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn uninstall_service_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    service_id: String,
) -> Result<(), String> {
    let res = uninstall_service(&sup, &service_id).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn start_service_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    service_id: String,
    port: Option<u16>,
) -> Result<ServiceStatusInfo, String> {
    let res = start_service(&app, &sup, &service_id, port).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn stop_service_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    service_id: String,
) -> Result<(), String> {
    let res = stop_service(&sup, &service_id).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn restart_service_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    service_id: String,
    port: Option<u16>,
) -> Result<ServiceStatusInfo, String> {
    let res = restart_service(&app, &sup, &service_id, port).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn start_all_services_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
) -> Result<Vec<String>, String> {
    let res = start_all_services(&app, &sup).await;
    let _ = app.emit("services-changed", ());
    res
}

#[tauri::command]
pub async fn stop_all_services_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
) -> Result<(), String> {
    stop_all_services(&sup).await;
    let _ = app.emit("services-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn get_service_logs_cmd(
    sup: State<'_, SharedSupervisor>,
    service_id: String,
) -> Result<ServiceLogsResponse, String> {
    get_service_logs(&sup, &service_id).await
}

#[tauri::command]
pub async fn add_local_site_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    site: LocalSite,
) -> Result<Vec<LocalSite>, String> {
    let server = site.server.clone();
    let sites = add_local_site(site)?;
    let (caddy_running, apache_running) = {
        let guard = sup.lock().await;
        (
            guard.running.contains_key("caddy"),
            guard.running.contains_key("apache"),
        )
    };
    // Caddy proxies both Caddy sites and Apache sites, so always gracefully reload it when running
    if caddy_running {
        let _ = reload_caddy_or_restart(&app, &sup).await;
    }
    if server == "apache" && apache_running {
        let _ = restart_service(&app, &sup, "apache", None).await;
    }
    if caddy_running || apache_running {
        let _ = app.emit("services-changed", ());
    }
    Ok(sites)
}

#[tauri::command]
pub async fn remove_local_site_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
    domain: String,
) -> Result<Vec<LocalSite>, String> {
    let sites = remove_local_site(&domain)?;
    let (caddy_running, apache_running) = {
        let guard = sup.lock().await;
        (
            guard.running.contains_key("caddy"),
            guard.running.contains_key("apache"),
        )
    };
    if caddy_running {
        let _ = reload_caddy_or_restart(&app, &sup).await;
    }
    if apache_running {
        let _ = restart_service(&app, &sup, "apache", None).await;
    }
    let _ = app.emit("services-changed", ());
    Ok(sites)
}

#[tauri::command]
pub fn preview_webserver_config_cmd(server: String) -> Result<String, String> {
    let cfg = load_config()?;
    if server == "apache" {
        let port = cfg.service_ports.get("apache").copied().unwrap_or(8081);
        Ok(generate_apache_conf(port, &cfg.sites))
    } else {
        let port = cfg.service_ports.get("caddy").copied().unwrap_or(8080);
        let pma_port = cfg.service_ports.get("phpmyadmin").copied().unwrap_or(8088);
        let apache_port = cfg.service_ports.get("apache").copied().unwrap_or(8081);
        Ok(generate_caddyfile(port, &cfg.sites, pma_port, apache_port))
    }
}

#[tauri::command]
pub async fn get_network_status_cmd() -> Result<NetworkStatus, String> {
    tokio::task::spawn_blocking(get_network_status)
        .await
        .map_err(|e| format!("Network status check failed: {}", e))?
}

#[tauri::command]
pub async fn sync_hosts_file_cmd() -> Result<NetworkStatus, String> {
    tokio::task::spawn_blocking(sync_hosts_file)
        .await
        .map_err(|e| format!("Hosts sync failed: {}", e))?
}

#[tauri::command]
pub async fn trust_local_ca_cmd(
    app: AppHandle,
    sup: State<'_, SharedSupervisor>,
) -> Result<String, String> {
    trust_local_ca(&app, &sup).await
}

// --- PHP Extensions & php.ini Management ---

#[tauri::command]
pub async fn inspect_php_version_cmd(version: String) -> Result<PhpVersionDetails, String> {
    tokio::task::spawn_blocking(move || inspect_php_version(&version))
        .await
        .map_err(|e| format!("Failed inspecting PHP: {}", e))?
}

#[tauri::command]
pub async fn toggle_php_extension_cmd(
    version: String,
    ext_name: String,
    enable: bool,
) -> Result<PhpVersionDetails, String> {
    tokio::task::spawn_blocking(move || toggle_php_extension(&version, &ext_name, enable))
        .await
        .map_err(|e| format!("Failed toggling PHP extension: {}", e))?
}

#[tauri::command]
pub async fn update_php_ini_settings_cmd(
    version: String,
    settings: PhpIniSettings,
) -> Result<PhpVersionDetails, String> {
    tokio::task::spawn_blocking(move || update_php_ini_settings(&version, settings))
        .await
        .map_err(|e| format!("Failed updating php.ini: {}", e))?
}

// --- Dedicated Databases Management ---

#[tauri::command]
pub fn save_db_server_cmd(profile: DbServerProfile) -> Result<Vec<DbServerProfile>, String> {
    save_db_server(profile)
}

#[tauri::command]
pub fn remove_db_server_cmd(server_id: String) -> Result<Vec<DbServerProfile>, String> {
    remove_db_server(&server_id)
}

#[tauri::command]
pub async fn inspect_db_server_cmd(server_id: String) -> Result<DbServerInspection, String> {
    tokio::task::spawn_blocking(move || inspect_db_server(&server_id))
        .await
        .map_err(|e| format!("DB inspection task failed: {}", e))?
}

#[tauri::command]
pub async fn list_database_tables_cmd(
    server_id: String,
    database: String,
) -> Result<Vec<DbTableInfo>, String> {
    tokio::task::spawn_blocking(move || list_database_tables(&server_id, &database))
        .await
        .map_err(|e| format!("List tables task failed: {}", e))?
}

#[tauri::command]
pub async fn inspect_table_cmd(
    server_id: String,
    database: String,
    table: String,
    limit: Option<u32>,
    offset: Option<u64>,
) -> Result<DbTablePreview, String> {
    tokio::task::spawn_blocking(move || {
        inspect_table(
            &server_id,
            &database,
            &table,
            limit.unwrap_or(50),
            offset.unwrap_or(0),
        )
    })
    .await
    .map_err(|e| format!("Inspect table task failed: {}", e))?
}

#[tauri::command]
pub async fn create_database_cmd(
    server_id: String,
    name: String,
    charset: Option<String>,
    collation: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        create_database(&server_id, &name, charset.as_deref(), collation.as_deref())
    })
    .await
    .map_err(|e| format!("Create DB task failed: {}", e))?
}

#[tauri::command]
pub async fn drop_database_cmd(server_id: String, name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || drop_database(&server_id, &name))
        .await
        .map_err(|e| format!("Drop DB task failed: {}", e))?
}

#[tauri::command]
pub async fn create_db_user_cmd(
    server_id: String,
    username: String,
    host: String,
    password: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || create_db_user(&server_id, &username, &host, &password))
        .await
        .map_err(|e| format!("Create user task failed: {}", e))?
}

#[tauri::command]
pub async fn drop_db_user_cmd(
    server_id: String,
    username: String,
    host: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || drop_db_user(&server_id, &username, &host))
        .await
        .map_err(|e| format!("Drop user task failed: {}", e))?
}

#[tauri::command]
pub async fn assign_user_to_database_cmd(
    server_id: String,
    username: String,
    host: String,
    database: String,
    read_only: bool,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        assign_user_to_database(&server_id, &username, &host, &database, read_only)
    })
    .await
    .map_err(|e| format!("Assign user task failed: {}", e))?
}

#[tauri::command]
pub async fn revoke_user_from_database_cmd(
    server_id: String,
    username: String,
    host: String,
    database: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        revoke_user_from_database(&server_id, &username, &host, &database)
    })
    .await
    .map_err(|e| format!("Revoke user task failed: {}", e))?
}

#[tauri::command]
pub async fn export_database_cmd(
    app: AppHandle,
    server_id: String,
    database: String,
) -> Result<Option<(String, u64)>, String> {
    let default_name = format!("{}.sql.gz", database);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(format!("Export {} Backup", database))
        .set_file_name(&default_name)
        .add_filter("Gzipped SQL Backup", &["gz", "sql"])
        .save_file(move |fp| {
            let resolved = fp.and_then(|f| f.into_path().ok()).map(|p| {
                p.to_string_lossy().to_string()
            });
            let _ = tx.send(resolved);
        });

    let Some(out_path) = rx
        .await
        .map_err(|_| "Save dialog cancelled unexpectedly".to_string())?
    else {
        return Ok(None);
    };

    let out_clone = out_path.clone();
    let app_clone = app.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        export_database(&app_clone, &server_id, &database, &out_clone)
    })
    .await
    .map_err(|e| format!("Export task failed: {}", e))??;

    Ok(Some((out_path, bytes)))
}

#[tauri::command]
pub async fn import_database_cmd(
    app: AppHandle,
    server_id: String,
    database: String,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(format!("Import SQL Backup into {}", database))
        .add_filter("SQL / Gzipped SQL", &["sql", "gz"])
        .pick_file(move |fp| {
            let resolved = fp.and_then(|f| f.into_path().ok()).map(|p| {
                p.to_string_lossy().to_string()
            });
            let _ = tx.send(resolved);
        });

    let Some(in_path) = rx
        .await
        .map_err(|_| "Open dialog cancelled unexpectedly".to_string())?
    else {
        return Ok(None);
    };

    let in_clone = in_path.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || {
        import_database(&app_clone, &server_id, &database, &in_clone)
    })
    .await
    .map_err(|e| format!("Import task failed: {}", e))??;

    Ok(Some(in_path))
}

#[tauri::command]
pub async fn export_db_server_cmd(
    app: AppHandle,
    server_id: String,
) -> Result<Option<(String, u64, usize)>, String> {
    let default_name = format!("{}-all-databases.sql.gz", server_id);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export Entire Server Backup")
        .set_file_name(&default_name)
        .add_filter("Gzipped SQL Backup", &["gz", "sql"])
        .save_file(move |fp| {
            let resolved = fp.and_then(|f| f.into_path().ok()).map(|p| {
                p.to_string_lossy().to_string()
            });
            let _ = tx.send(resolved);
        });

    let Some(out_path) = rx
        .await
        .map_err(|_| "Save dialog cancelled unexpectedly".to_string())?
    else {
        return Ok(None);
    };

    let out_clone = out_path.clone();
    let app_clone = app.clone();
    let (bytes, db_count) =
        tokio::task::spawn_blocking(move || export_server(&app_clone, &server_id, &out_clone))
            .await
            .map_err(|e| format!("Server export task failed: {}", e))??;

    Ok(Some((out_path, bytes, db_count)))
}

#[tauri::command]
pub async fn import_db_server_cmd(
    app: AppHandle,
    server_id: String,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Import SQL Backup into Server")
        .add_filter("SQL / Gzipped SQL", &["sql", "gz"])
        .pick_file(move |fp| {
            let resolved = fp.and_then(|f| f.into_path().ok()).map(|p| {
                p.to_string_lossy().to_string()
            });
            let _ = tx.send(resolved);
        });

    let Some(in_path) = rx
        .await
        .map_err(|_| "Open dialog cancelled unexpectedly".to_string())?
    else {
        return Ok(None);
    };

    let in_clone = in_path.clone();
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || import_server(&app_clone, &server_id, &in_clone))
        .await
        .map_err(|e| format!("Server import task failed: {}", e))??;

    Ok(Some(in_path))
}

// --- GitHub Releases Update Checker ---

/// Override at compile time with `WORKBENCH_GITHUB_REPO=owner/repo`
const GITHUB_REPO: &str = match option_env!("WORKBENCH_GITHUB_REPO") {
    Some(r) => r,
    None => "aquinasm/workbench",
};

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub checked: bool,
    pub error: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GithubLatestRelease {
    tag_name: String,
    html_url: String,
}

fn parse_semver_tuple(raw: &str) -> (u64, u64, u64) {
    let cleaned = raw.trim().trim_start_matches(['v', 'V']);
    let core = cleaned.split('-').next().unwrap_or(cleaned);
    let mut parts = core.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    parse_semver_tuple(latest) > parse_semver_tuple(current)
}

#[tauri::command]
pub async fn check_for_updates_cmd() -> Result<UpdateCheckResult, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    let fallback_releases_url = format!("https://github.com/{}/releases", GITHUB_REPO);

    let client = reqwest::Client::builder()
        .user_agent(format!("Workbench/{}", current))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = match client
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok(UpdateCheckResult {
                current_version: current,
                latest_version: None,
                update_available: false,
                release_url: Some(fallback_releases_url),
                checked: false,
                error: Some(format!("Unable to reach GitHub: {}", e)),
            });
        }
    };

    // 404 on /releases/latest means either no release yet or only pre-releases (e.g. public beta)
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        let list_url = format!("https://api.github.com/repos/{}/releases?per_page=1", GITHUB_REPO);
        if let Ok(list_resp) = client
            .get(&list_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            if let Ok(text) = list_resp.text().await {
                if let Ok(list) = serde_json::from_str::<Vec<GithubLatestRelease>>(&text) {
                    if let Some(first) = list.into_iter().next() {
                        let latest_clean = first
                            .tag_name
                            .trim()
                            .trim_start_matches(['v', 'V'])
                            .to_string();
                        let update_available = is_newer_version(&current, &latest_clean);
                        let safe_url = if first.html_url.starts_with("https://github.com/") {
                            first.html_url
                        } else {
                            fallback_releases_url
                        };
                        return Ok(UpdateCheckResult {
                            current_version: current,
                            latest_version: Some(latest_clean),
                            update_available,
                            release_url: Some(safe_url),
                            checked: true,
                            error: None,
                        });
                    }
                }
            }
        }
        return Ok(UpdateCheckResult {
            current_version: current.clone(),
            latest_version: Some(current),
            update_available: false,
            release_url: Some(fallback_releases_url),
            checked: true,
            error: None,
        });
    }

    if !resp.status().is_success() {
        return Ok(UpdateCheckResult {
            current_version: current,
            latest_version: None,
            update_available: false,
            release_url: Some(fallback_releases_url),
            checked: false,
            error: Some(format!("GitHub API returned HTTP {}", resp.status().as_u16())),
        });
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading GitHub response: {}", e))?;

    let release: GithubLatestRelease = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return Ok(UpdateCheckResult {
                current_version: current,
                latest_version: None,
                update_available: false,
                release_url: Some(fallback_releases_url),
                checked: false,
                error: Some(format!("Invalid release metadata: {}", e)),
            });
        }
    };

    let latest_clean = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let update_available = is_newer_version(&current, &latest_clean);
    let safe_url = if release.html_url.starts_with("https://github.com/") {
        release.html_url
    } else {
        fallback_releases_url
    };

    Ok(UpdateCheckResult {
        current_version: current,
        latest_version: Some(latest_clean),
        update_available,
        release_url: Some(safe_url),
        checked: true,
        error: None,
    })
}

#[tauri::command]
pub async fn get_system_stats_cmd(
    telemetry: State<'_, crate::telemetry::SharedTelemetry>,
) -> Result<crate::telemetry::SystemStats, String> {
    let tel = std::sync::Arc::clone(telemetry.inner());
    tokio::task::spawn_blocking(move || {
        let mut guard = tel.blocking_lock();
        guard.sample()
    })
    .await
    .map_err(|e| format!("Telemetry sampling failed: {}", e))
}
