use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use connect_core::db::Database;
use connect_core::models::{RunView, Settings, TagPayload, TriggerView};
use serde::Serialize;
use tauri::{Manager, State};

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
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    state
        .db
        .save_settings(&settings)
        .map_err(|err| err.to_string())
}

pub fn run() {
    tauri::Builder::default()
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
