use crate::config::load_config;
use crate::services::{
    get_network_status, list_services, start_all_services, start_service, stop_all_services,
    stop_service, SharedSupervisor,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};

pub const TRAY_ID: &str = "workbench-tray";

pub fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn hide_to_tray(app: &AppHandle, window: &tauri::Window) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    {
        // Hide from macOS Dock & Cmd+Tab while keeping Workbench alive in the menubar tray
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("cmd");
        cmd.creation_flags(0x08000000);
        let _ = cmd.args(["/C", "start", "", url]).spawn();
    }
}

fn build_site_url_for_tray(
    domain: &str,
    server: &str,
    https: bool,
    portless: bool,
    caddy_port: u16,
    caddy_https_port: u16,
    apache_port: u16,
) -> String {
    let proto = if https { "https" } else { "http" };
    if portless {
        return format!("{}://{}", proto, domain);
    }
    let port = if server == "caddy" || https {
        if https {
            caddy_https_port
        } else {
            caddy_port
        }
    } else {
        apache_port
    };
    let is_default = (https && port == 443) || (!https && port == 80);
    if is_default {
        format!("{}://{}", proto, domain)
    } else {
        format!("{}://{}:{}", proto, domain, port)
    }
}

/// Builds a live, structured context menu reflecting currently running services and linked `.test` domains.
pub async fn build_tray_menu(
    app: &AppHandle,
    sup: &SharedSupervisor,
) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let services = list_services(sup).await.unwrap_or_default();
    let cfg = load_config().unwrap_or_default();
    let net = get_network_status().ok();
    let portless = net.as_ref().map(|n| n.portless_enabled).unwrap_or(false);
    let caddy_port = net.as_ref().map(|n| n.caddy_port).unwrap_or(8080);
    let caddy_https_port = net.as_ref().map(|n| n.caddy_https_port).unwrap_or(8443);
    let apache_port = net.as_ref().map(|n| n.apache_port).unwrap_or(8081);

    let running_count = services
        .iter()
        .filter(|s| s.status == "running" || s.status == "external_running")
        .count();

    let status_label = if running_count > 0 {
        format!("Workbench  ·  {} running", running_count)
    } else {
        "Workbench  ·  Idle".to_string()
    };

    let header_i = MenuItem::with_id(app, "header", &status_label, false, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    let show_i = MenuItem::with_id(app, "show", "Open Workbench", true, None::<&str>)?;
    let pma_label = if portless {
        "Open phpMyAdmin (phpmyadmin.test)"
    } else {
        "Open phpMyAdmin"
    };
    let pma_i = MenuItem::with_id(app, "open_pma", pma_label, true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    // Services submenu with live per-service start/stop toggles
    let services_sub = Submenu::with_id(app, "sub_services", "Services", true)?;
    for svc in &services {
        let is_running = svc.status == "running" || svc.status == "external_running";
        let can_run = svc.installed_managed || svc.host_binary.is_some();
        let dot = if is_running { "●" } else { "○" };
        let action = if is_running {
            "Stop"
        } else if can_run {
            "Start"
        } else {
            "Not Installed"
        };
        let label = format!("{}  {}  (:{}) — {}", dot, svc.name, svc.port, action);
        let id = format!(
            "svc:{}:{}",
            if is_running { "stop" } else { "start" },
            svc.id
        );
        let item = MenuItem::with_id(app, id, &label, is_running || can_run, None::<&str>)?;
        services_sub.append(&item)?;
    }

    // Domains submenu with live clickable .test sites
    let domains_title = if cfg.sites.is_empty() {
        "Domains".to_string()
    } else {
        format!("Domains ({})", cfg.sites.len())
    };
    let domains_sub = Submenu::with_id(app, "sub_domains", &domains_title, true)?;
    if cfg.sites.is_empty() {
        let empty = MenuItem::with_id(app, "domains_empty", "No domains linked", false, None::<&str>)?;
        domains_sub.append(&empty)?;
    } else {
        let mut sorted_sites = cfg.sites.clone();
        sorted_sites.sort_by(|a, b| a.domain.cmp(&b.domain));
        for site in sorted_sites {
            let url = build_site_url_for_tray(
                &site.domain,
                &site.server,
                site.https,
                portless,
                caddy_port,
                caddy_https_port,
                apache_port,
            );
            let label = format!("{}  ({})", url, site.server);
            let id = format!("open_url:{}", url);
            let item = MenuItem::with_id(app, id, &label, true, None::<&str>)?;
            domains_sub.append(&item)?;
        }
    }
    let dom_sep = PredefinedMenuItem::separator(app)?;
    let manage_dom_i = MenuItem::with_id(app, "nav:/domains", "Manage Domains...", true, None::<&str>)?;
    domains_sub.append(&dom_sep)?;
    domains_sub.append(&manage_dom_i)?;

    // Quick Navigate submenu
    let nav_sub = Submenu::with_id(app, "sub_nav", "Go To", true)?;
    for (route, label) in [
        ("/", "Dashboard"),
        ("/projects", "Projects"),
        ("/domains", "Domains"),
        ("/databases", "Databases"),
        ("/services", "Services"),
        ("/runtimes", "Runtimes"),
        ("/storage", "Storage"),
        ("/settings", "Settings"),
    ] {
        let item = MenuItem::with_id(app, format!("nav:{}", route), label, true, None::<&str>)?;
        nav_sub.append(&item)?;
    }

    let sep3 = PredefinedMenuItem::separator(app)?;
    let start_all_i = MenuItem::with_id(app, "start_all", "Start All Services", true, None::<&str>)?;
    let stop_all_i = MenuItem::with_id(
        app,
        "stop_all",
        "Stop All Services",
        running_count > 0,
        None::<&str>,
    )?;
    let sep4 = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit Workbench", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &header_i,
            &sep1,
            &show_i,
            &pma_i,
            &sep2,
            &services_sub,
            &domains_sub,
            &nav_sub,
            &sep3,
            &start_all_i,
            &stop_all_i,
            &sep4,
            &quit_i,
        ],
    )
}

/// Refreshes the system tray / menubar menu in place with live service & domain state.
pub async fn refresh_tray_menu(app: &AppHandle, sup: &SharedSupervisor) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_tray_menu(app, sup).await {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

/// Initializes the system tray / menubar icon and event handlers during app setup.
pub fn init_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_i = MenuItem::with_id(app, "show", "Open Workbench", true, None::<&str>)?;
    let start_all_i = MenuItem::with_id(app, "start_all", "Start All Services", true, None::<&str>)?;
    let stop_all_i = MenuItem::with_id(app, "stop_all", "Stop All Services", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit Workbench", true, None::<&str>)?;
    let initial_menu = Menu::with_items(app, &[&show_i, &start_all_i, &stop_all_i, &quit_i])?;

    if let Some(icon) = app.default_window_icon().cloned() {
        let _tray = TrayIconBuilder::with_id(TRAY_ID)
            .icon(icon)
            .menu(&initial_menu)
            // On macOS, left-click opens the menubar menu; on Windows/Linux, right-click opens the menu
            // and left-click restores the main window.
            .show_menu_on_left_click(cfg!(target_os = "macos"))
            .tooltip("Workbench — Local Development Environment")
            .on_tray_icon_event(|tray, event| {
                #[cfg(not(target_os = "macos"))]
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    show_main_window(tray.app_handle());
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = (tray, event);
                }
            })
            .on_menu_event(|app, event| {
                let id = event.id.as_ref();
                if id == "show" {
                    show_main_window(app);
                } else if id == "open_pma" {
                    let net = get_network_status().ok();
                    let cfg = load_config().unwrap_or_default();
                    let pma_port = cfg.service_ports.get("phpmyadmin").copied().unwrap_or(8088);
                    let url = if net.map(|n| n.portless_enabled).unwrap_or(false) {
                        "https://phpmyadmin.test".to_string()
                    } else {
                        format!("http://127.0.0.1:{}", pma_port)
                    };
                    open_url_in_browser(&url);
                } else if let Some(url) = id.strip_prefix("open_url:") {
                    open_url_in_browser(url);
                } else if let Some(route) = id.strip_prefix("nav:") {
                    show_main_window(app);
                    let _ = app.emit("tray-navigate", route.to_string());
                } else if let Some(rest) = id.strip_prefix("svc:") {
                    if let Some((action, service_id)) = rest.split_once(':') {
                        let app_h = app.clone();
                        let sup = app.state::<SharedSupervisor>().inner().clone();
                        let action = action.to_string();
                        let service_id = service_id.to_string();
                        tauri::async_runtime::spawn(async move {
                            if action == "start" {
                                let _ = start_service(&app_h, &sup, &service_id, None).await;
                            } else {
                                let _ = stop_service(&sup, &service_id).await;
                            }
                            let _ = app_h.emit("services-changed", ());
                            refresh_tray_menu(&app_h, &sup).await;
                        });
                    }
                } else if id == "start_all" {
                    let app_h = app.clone();
                    let sup = app.state::<SharedSupervisor>().inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = start_all_services(&app_h, &sup).await;
                        let _ = app_h.emit("services-changed", ());
                        refresh_tray_menu(&app_h, &sup).await;
                    });
                } else if id == "stop_all" {
                    let app_h = app.clone();
                    let sup = app.state::<SharedSupervisor>().inner().clone();
                    tauri::async_runtime::spawn(async move {
                        stop_all_services(&sup).await;
                        let _ = app_h.emit("services-changed", ());
                        refresh_tray_menu(&app_h, &sup).await;
                    });
                } else if id == "quit" {
                    app.exit(0);
                }
            })
            .build(app)?;
    }

    // Populate the live dynamic menu immediately after startup
    let app_h = app.handle().clone();
    let sup = app.state::<SharedSupervisor>().inner().clone();
    tauri::async_runtime::spawn(async move {
        refresh_tray_menu(&app_h, &sup).await;
    });

    Ok(())
}
