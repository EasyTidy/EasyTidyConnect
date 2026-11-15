use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use connect_core::bridge::{BridgeHealth, BridgeItem};
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
    let effective_auto_start = match settings.integration_mode.as_deref() {
        Some("follow") | Some("manual") => false,
        Some("independent") => true,
        _ => settings.auto_start,
    };

    let mut persisted = settings;
    persisted.auto_start = effective_auto_start;

    state
        .db
        .save_settings(&persisted)
        .map_err(|err| err.to_string())?;

    let autolaunch = app.autolaunch();
    let result = if effective_auto_start {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|err| err.to_string())
}

#[tauri::command]
async fn test_bridge(bridge_url: String, bridge_token: String) -> Result<BridgeHealth, String> {
    let client = connect_core::BridgeClient::new().map_err(|err| err.to_string())?;
    client
        .health(&bridge_url, &bridge_token)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn bridge_tasks(bridge_url: String, bridge_token: String) -> Result<Vec<BridgeItem>, String> {
    let client = connect_core::BridgeClient::new().map_err(|err| err.to_string())?;
    client
        .tasks(&bridge_url, &bridge_token)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn bridge_workflows(
    bridge_url: String,
    bridge_token: String,
) -> Result<Vec<BridgeItem>, String> {
    let client = connect_core::BridgeClient::new().map_err(|err| err.to_string())?;
    client
        .workflows(&bridge_url, &bridge_token)
        .await
        .map_err(|err| err.to_string())
}

#[derive(Serialize)]
struct ImportedBridge {
    app: String,
    url: String,
    token: String,
    version: Option<String>,
}

#[tauri::command]
fn import_bridge() -> Result<ImportedBridge, String> {
    read_bridge_descriptor().ok_or_else(|| {
        "未找到 EasyTidy 的 connect-bridge.json，请先启动 EasyTidyPro。".to_string()
    })
}

fn read_bridge_descriptor() -> Option<ImportedBridge> {
    let appdata = std::env::var("APPDATA").ok()?;
    for app in ["EasyTidyPro", "EasyTidyProDev"] {
        let path = PathBuf::from(&appdata).join(app).join("connect-bridge.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let (Some(url), Some(token)) = (
            value.get("url").and_then(|item| item.as_str()),
            value.get("token").and_then(|item| item.as_str()),
        ) else {
            continue;
        };
        return Some(ImportedBridge {
            app: value
                .get("app")
                .and_then(|item| item.as_str())
                .unwrap_or("EasyTidyPro")
                .to_string(),
            url: url.to_string(),
            token: token.to_string(),
            version: value
                .get("version")
                .and_then(|item| item.as_str())
                .map(|item| item.to_string()),
        });
    }
    None
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[derive(Default)]
struct LaunchArgs {
    embedded: bool,
    minimized: bool,
    parent_pid: Option<u32>,
    bridge_url: Option<String>,
    bridge_token_file: Option<PathBuf>,
}

fn parse_launch_args() -> LaunchArgs {
    let mut result = LaunchArgs::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--embedded" => result.embedded = true,
            "--minimized" => result.minimized = true,
            "--parent-pid" => {
                result.parent_pid = args.next().and_then(|value| value.parse().ok());
            }
            "--bridge-url" => result.bridge_url = args.next(),
            "--bridge-token-file" => {
                result.bridge_token_file = args.next().map(PathBuf::from);
            }
            _ => {}
        }
    }
    result
}

fn read_and_remove_token(path: &PathBuf) -> Option<String> {
    let token = std::fs::read_to_string(path).ok()?.trim().to_string();
    let _ = std::fs::remove_file(path);
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn spawn_parent_monitor(app: AppHandle, parent_pid: u32) {
    std::thread::spawn(move || {
        use sysinfo::{Pid, System};
        let pid = Pid::from(parent_pid as usize);
        let mut system = System::new();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let alive = system.refresh_process(pid) && system.process(pid).is_some();
            if !alive {
                tracing::info!("parent EasyTidy exited, stopping embedded Connect");
                app.exit(0);
                break;
            }
        }
    });
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
            let launch = parse_launch_args();
            let app_dir: PathBuf = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from(".").join("EasyTidy Connect"));
            std::fs::create_dir_all(&app_dir)?;
            let db_path = app_dir.join("connect.db");
            let db = Arc::new(Database::open(&db_path)?);
            let mut settings = db.get_settings()?;

            // EasyTidy 启动时可传入 Bridge 地址与一次性 token 文件，避免 token 出现在命令行。
            let mut bridge_changed = false;
            if let Some(url) = &launch.bridge_url {
                settings.bridge_url = url.clone();
                bridge_changed = true;
            }
            if let Some(path) = &launch.bridge_token_file {
                if let Some(token) = read_and_remove_token(path) {
                    settings.bridge_token = token;
                    bridge_changed = true;
                }
            }

            // 首次启动自动读取 EasyTidy 写的本机 Bridge 描述，省去手工复制 token。
            if settings.bridge_url.is_empty() || settings.bridge_token.is_empty() {
                if let Some(imported) = read_bridge_descriptor() {
                    settings.bridge_url = imported.url;
                    settings.bridge_token = imported.token;
                    bridge_changed = true;
                }
            }

            if bridge_changed {
                db.save_settings(&settings)?;
            }

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

            if launch.embedded || launch.minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            if let Some(parent_pid) = launch.parent_pid {
                spawn_parent_monitor(app.handle().clone(), parent_pid);
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
            save_settings,
            test_bridge,
            bridge_tasks,
            bridge_workflows,
            import_bridge
        ])
        .run(tauri::generate_context!())
        .expect("error while running EasyTidy Connect");
}
