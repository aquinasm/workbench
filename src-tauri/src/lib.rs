pub mod commands;
pub mod config;
pub mod databases;
pub mod db;
pub mod dns;
pub mod prebuilt;
pub mod runtimes;
pub mod services;
pub mod shell;
pub mod storage;
pub mod telemetry;
pub mod tray;

use services::{stop_all_services, ServiceSupervisor, SharedSupervisor};
use std::sync::Arc;
use tauri::{Manager, RunEvent, WindowEvent};
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
            dns::spawn_local_test_dns();
            tray::init_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Keep Workbench and all running services alive in the system tray / menubar
                // when closing the window on macOS, Windows, and Linux.
                api.prevent_close();
                tray::hide_to_tray(window.app_handle(), window);
            }
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
            commands::install_app_update_cmd,
            commands::get_system_stats_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Workbench");

    app.run(move |app_handle, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            tray::show_main_window(app_handle);
        }
        RunEvent::Exit => {
            tauri::async_runtime::block_on(stop_all_services(&sup_for_exit));
        }
        _ => {
            let _ = app_handle;
        }
    });
}
