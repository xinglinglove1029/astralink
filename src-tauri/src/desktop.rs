use crate::commands::AppCore;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

struct Lifecycle {
    exiting: AtomicBool,
    ready: AtomicBool,
}
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
fn request_exit(app: &tauri::AppHandle) {
    let Some(lifecycle) = app.try_state::<Lifecycle>() else {
        return;
    };
    if lifecycle.exiting.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(core) = app.try_state::<AppCore>() {
        core.begin_shutdown();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(core) = app.try_state::<AppCore>() {
            core.shutdown().await;
        }
        if let Some(lifecycle) = app.try_state::<Lifecycle>() {
            lifecycle.ready.store(true, Ordering::SeqCst);
        }
        app.exit(0);
    });
}
pub fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    app.manage(Lifecycle {
        exiting: AtomicBool::new(false),
        ready: AtomicBool::new(false),
    });
    let show_item = MenuItem::with_id(
        app,
        "astralink-show",
        "显示星枢 Astralink",
        true,
        None::<&str>,
    )?;
    let stop_item = MenuItem::with_id(app, "astralink-stop", "停止 MCP", true, None::<&str>)?;
    let exit_item = MenuItem::with_id(app, "astralink-exit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &stop_item, &exit_item])?;
    let mut tray = TrayIconBuilder::with_id("astralink-tray")
        .tooltip("星枢 · Astralink")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "astralink-show" => show(app),
            "astralink-stop" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let core = app.state::<AppCore>();
                    core.stop_service().await;
                });
            }
            "astralink-exit" => request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        let exiting = app
            .try_state::<Lifecycle>()
            .is_some_and(|s| s.exiting.load(Ordering::SeqCst));
        let minimize = app
            .try_state::<AppCore>()
            .is_some_and(|core| core.store().is_ok_and(|s| s.settings().minimize_to_tray));
        if minimize && !exiting {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}
pub fn run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::ExitRequested { api, .. } = event {
        let ready = app
            .try_state::<Lifecycle>()
            .is_some_and(|s| s.ready.load(Ordering::SeqCst));
        if !ready {
            api.prevent_exit();
            request_exit(app);
        }
    }
}
