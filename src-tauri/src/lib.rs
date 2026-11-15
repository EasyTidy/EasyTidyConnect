use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use connect_core::db::Database;
use connect_core::models::{RunView, Settings, TagPayload, TriggerView};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;

struct AppState {
    db: Arc<Database>,
    port: u16,
    server_ok: Arc<AtomicBool>,
}

#[derive(Serialize)]
struct StatusView {
    running: bool,
    port: u16,
    ip: String,
    version: String,
    app: &'static str,
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> StatusView {
    StatusView {
        running: state.server_ok.load(Ordering::Relaxed),
        port: state.port,
        ip: connect_core::local_ip(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        app: "EasyTidy Connect",
    }
}

#[tauri::command]
fn list_triggers(state: State<'_, AppState>) -> Result<Vec<TriggerView>, String> {
    state.db.list_triggers().map_err(|err| err.to_string())
}

#[tauri::command]
fn create_trigger(
    state: State<'_, AppState>,
    name: String,
    action_type: String,
    action_config: String,
) -> Result<TriggerView, String> {
    state
        .db
        .create_trigger(&name, &action_type, &action_config)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn update_trigger(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    enabled: bool,
    action_type: String,
    action_config: String,
) -> Result<(), String> {
    state
        .db
        .update_trigger(id, &name, enabled, &action_type, &action_config)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn set_trigger_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .set_enabled(id, enabled)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_trigger(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.db.delete_trigger(id).map_err(|err| err.to_string())
}

#[tauri::command]
fn get_tag_payload(state: State<'_, AppState>, trigger_id: i64) -> Result<TagPayload, String> {
    connect_core::tag_payload(&state.db, trigger_id, state.port).map_err(|err| err.to_string())
}

#[tauri::command]
fn mark_tag_written(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state
        .db
        .mark_tag_written(id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn list_runs(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<RunView>, String> {
    state
        .db
        .list_runs(limit.unwrap_or(50))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state.db.get_settings().map_err(|err| err.to_string())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    state
        .db
        .save_settings(&settings)
        .map_err(|err| err.to_string())?;

    let autolaunch = app.autolaunch();
    let result = if settings.auto_start {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|err| err.to_string())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let app_dir: PathBuf = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".").join("EasyTidy Connect"));
            std::fs::create_dir_all(&app_dir)?;
            let db_path = app_dir.join("connect.db");
            let db = Arc::new(Database::open(&db_path)?);
            let settings = db.get_settings()?;
            let port = settings.port;

            // 同步数据库里的开机启动设置与系统实际状态。
            let autolaunch = app.autolaunch();
            let actual_auto_start = autolaunch.is_enabled().unwrap_or(false);
            if settings.auto_start != actual_auto_start {
                let result = if settings.auto_start {
                    autolaunch.enable()
                } else {
                    autolaunch.disable()
                };
                if let Err(err) = result {
                    tracing::warn!("sync autostart failed: {err}");
                }
            }

            let server_ok = Arc::new(AtomicBool::new(true));
            let server_ok_task = server_ok.clone();
            let db_task = db.clone();

            tauri::async_runtime::spawn(async move {
                if let Err(err) = connect_core::server::serve(db_task, port).await {
                    tracing::error!("connect server stopped: {err}");
                    server_ok_task.store(false, Ordering::Relaxed);
                }
            });

            app.manage(AppState {
                db,
                port,
                server_ok,
            });

            let show_item = MenuItem::with_id(app, "show", "打开 EasyTidy Connect", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("window icon"))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick { .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            app.manage(tray);

            if std::env::args().any(|arg| arg == "--minimized") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_triggers,
            create_trigger,
            update_trigger,
            set_trigger_enabled,
            delete_trigger,
            get_tag_payload,
            mark_tag_written,
            list_runs,
            get_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running EasyTidy Connect");
}
