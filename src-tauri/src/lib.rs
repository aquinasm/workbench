pub mod commands;
pub mod config;
pub mod databases;
pub mod db;
pub mod prebuilt;
pub mod runtimes;
pub mod services;
pub mod shell;
pub mod storage;
pub mod telemetry;

use services::{stop_all_services, ServiceSupervisor, SharedSupervisor};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, RunEvent,
};
use telemetry::{SharedTelemetry, TelemetryState};
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = config::ensure_layout();
    let supervisor: SharedSupervisor = Arc::new(Mutex::new(ServiceSupervisor::default()));
    let telemetry: SharedTelemetry = Arc::new(Mutex::new(TelemetryState::default()));
    let sup_for_exit = Arc::clone(&supervisor);

    let app = tauri::Builder::default()
        .manage(supervisor)
        .manage(telemetry)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Open Workbench", true, None::<&str>)?;
            let start_all_i =
                MenuItem::with_id(app, "start_all", "Start All Services", true, None::<&str>)?;
            let stop_all_i =
                MenuItem::with_id(app, "stop_all", "Stop All Services", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Workbench", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &start_all_i, &stop_all_i, &quit_i])?;

            if let Some(icon) = app.default_window_icon().cloned() {
                let _tray = TrayIconBuilder::new()
                    .icon(icon)
                    .menu(&menu)
                    .tooltip("Workbench — Local Development Environment")
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "start_all" => {
                            let app_h = app.clone();
                            let sup = app.state::<SharedSupervisor>().inner().clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = services::start_all_services(&app_h, &sup).await;
                            });
                        }
                        "stop_all" => {
                            let sup = app.state::<SharedSupervisor>().inner().clone();
                            tauri::async_runtime::spawn(async move {
                                stop_all_services(&sup).await;
                            });
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .build(app);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::update_config,
            commands::pick_workspace_folder,
            commands::add_scan_root,
            commands::remove_scan_root,
            commands::discover_projects_fast_cmd,
            commands::scan_projects,
            commands::hibernate_project_cmd,
            commands::hibernate_projects_bulk,
            commands::wake_project_cmd,
            commands::scan_global_caches_cmd,
            commands::clean_global_cache_cmd,
            commands::get_cleanup_stats_cmd,
            commands::get_shell_status_cmd,
            commands::install_shell_integration_cmd,
            commands::remove_shell_integration_cmd,
            commands::clean_uninstall_cmd,
            commands::detect_system_tools_cmd,
            commands::open_in_terminal,
            commands::list_runtimes_cmd,
            commands::install_runtime_cmd,
            commands::uninstall_runtime_cmd,
            commands::set_default_runtime_cmd,
            commands::sync_shims_cmd,
            commands::get_rustup_status_cmd,
            commands::install_rustup_toolchain_cmd,
            commands::set_default_rustup_toolchain_cmd,
            commands::uninstall_rustup_toolchain_cmd,
            commands::get_project_pins_cmd,
            commands::set_project_pin_cmd,
            commands::list_services_cmd,
            commands::install_service_cmd,
            commands::uninstall_service_cmd,
            commands::start_service_cmd,
            commands::stop_service_cmd,
            commands::restart_service_cmd,
            commands::start_all_services_cmd,
            commands::stop_all_services_cmd,
            commands::get_service_logs_cmd,
            commands::add_local_site_cmd,
            commands::remove_local_site_cmd,
            commands::preview_webserver_config_cmd,
            commands::get_network_status_cmd,
            commands::sync_hosts_file_cmd,
            commands::trust_local_ca_cmd,
            commands::inspect_php_version_cmd,
            commands::toggle_php_extension_cmd,
            commands::update_php_ini_settings_cmd,
            commands::save_db_server_cmd,
            commands::remove_db_server_cmd,
            commands::inspect_db_server_cmd,
            commands::list_database_tables_cmd,
            commands::inspect_table_cmd,
            commands::create_database_cmd,
            commands::drop_database_cmd,
            commands::create_db_user_cmd,
            commands::drop_db_user_cmd,
            commands::assign_user_to_database_cmd,
            commands::revoke_user_from_database_cmd,
            commands::export_database_cmd,
            commands::import_database_cmd,
            commands::export_db_server_cmd,
            commands::import_db_server_cmd,
            commands::check_for_updates_cmd,
            commands::get_system_stats_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Workbench");

    app.run(move |_app_handle, event| {
        if let RunEvent::Exit = event {
            tauri::async_runtime::block_on(stop_all_services(&sup_for_exit));
        }
    });
}
