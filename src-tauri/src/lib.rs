use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine as _;
use connect_core::bridge::{BridgeHealth, BridgeItem};
use connect_core::db::Database;
use connect_core::models::{
    AiProvider, AiProviderInput, AnniversaryProject, CustomTemplate, ProjectInput, ProjectVersion, PublishResult,
    PublishTarget, PublishTargetInput, RunView, Settings, ShareSettings, TagPayload, TriggerView,
    WechatConfig,
};
use connect_core::ai::AiGenerateInput;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;

struct AppState {
    db: Arc<Database>,
    port: u16,
    server_ok: Arc<AtomicBool>,
    data_dir: PathBuf,
    tunnels: Arc<connect_core::TunnelManager>,
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

/// 旧版本的应用标识（数据目录名）。
const LEGACY_IDENTIFIERS: [&str; 1] = ["com.easytidy.connect"];

/// 应用标识从 com.easytidy.connect 改为 net.easytidy.connect 后，把旧数据目录（数据库、照片、
/// 自动下载的工具）迁移到新目录。只在新目录还没有数据库时执行；同一磁盘下直接改名，失败时逐个复制。
fn migrate_legacy_data_dir(app_dir: &std::path::Path) {
    if app_dir.join("connect.db").exists() {
        return;
    }
    let Some(parent) = app_dir.parent() else { return };
    for legacy in LEGACY_IDENTIFIERS {
        let old = parent.join(legacy);
        if old == app_dir || !old.join("connect.db").exists() {
            continue;
        }
        // 新目录可能已被 WebView 等提前创建（空目录），先删掉空目录再改名
        if app_dir.exists() && std::fs::read_dir(app_dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
            let _ = std::fs::remove_dir(app_dir);
        }
        let moved = !app_dir.exists() && std::fs::rename(&old, app_dir).is_ok();
        if !moved {
            if let Err(err) = copy_dir_all(&old, app_dir) {
                tracing::warn!("migrate data dir {} -> {} failed: {err}", old.display(), app_dir.display());
                continue;
            }
        }
        tracing::info!("migrated data dir {} -> {}", old.display(), app_dir.display());
        return;
    }
}

fn copy_dir_all(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
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

#[derive(Serialize)]
struct TemplateInfo {
    id: &'static str,
    name: &'static str,
}

#[tauri::command]
fn list_templates() -> Vec<TemplateInfo> {
    connect_core::anniversary::TEMPLATES
        .iter()
        .map(|(id, name)| TemplateInfo { id, name })
        .collect()
}

#[tauri::command]
fn list_projects(state: State<'_, AppState>) -> Result<Vec<AnniversaryProject>, String> {
    state.db.list_projects().map_err(|err| err.to_string())
}

#[tauri::command]
fn get_project(state: State<'_, AppState>, id: i64) -> Result<Option<AnniversaryProject>, String> {
    state.db.get_project(id).map_err(|err| err.to_string())
}

#[tauri::command]
fn create_project(
    state: State<'_, AppState>,
    input: ProjectInput,
) -> Result<AnniversaryProject, String> {
    state.db.create_project(&input).map_err(|err| err.to_string())
}

#[tauri::command]
fn update_project(
    state: State<'_, AppState>,
    id: i64,
    input: ProjectInput,
) -> Result<(), String> {
    state
        .db
        .update_project(id, &input)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_project(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.db.delete_project(id).map_err(|err| err.to_string())
}

fn project_photos_dir(state: &AppState, project_id: i64) -> PathBuf {
    state
        .data_dir
        .join("anniversaries")
        .join(project_id.to_string())
        .join("photos")
}

#[tauri::command]
fn add_project_photo(
    state: State<'_, AppState>,
    project_id: i64,
    file_name: String,
    data_base64: String,
) -> Result<AnniversaryProject, String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|err| format!("图片 base64 无效：{err}"))?;
    if data.len() > 12 * 1024 * 1024 {
        return Err("单张图片不能超过 12 MB".to_string());
    }

    let ext = std::path::Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let ext = match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "gif" => ext,
        _ => "jpg".to_string(),
    };

    let dir = project_photos_dir(&state, project_id);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let stored = format!("{}.{}", uuid::Uuid::new_v4().simple(), ext);
    std::fs::write(dir.join(&stored), data).map_err(|err| err.to_string())?;

    let project = state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let mut photos = project.photos.clone();
    photos.push(stored);
    state
        .db
        .set_project_photos(project_id, &photos)
        .map_err(|err| err.to_string())?;
    state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "项目不存在".to_string())
}

/// 调整照片顺序（拖拽排序、设为封面）：新顺序必须与现有照片完全一致，只是顺序不同。
#[tauri::command]
fn reorder_project_photos(
    state: State<'_, AppState>,
    project_id: i64,
    photos: Vec<String>,
) -> Result<AnniversaryProject, String> {
    let project = state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let mut current = project.photos.clone();
    let mut requested = photos.clone();
    current.sort();
    requested.sort();
    if current != requested {
        return Err("照片列表已变化，请刷新后重试".to_string());
    }
    state
        .db
        .set_project_photos(project_id, &photos)
        .map_err(|err| err.to_string())?;
    state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "项目不存在".to_string())
}

#[tauri::command]
fn remove_project_photo(
    state: State<'_, AppState>,
    project_id: i64,
    file_name: String,
) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("非法文件名".to_string());
    }
    let project = state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let photos = project
        .photos
        .iter()
        .filter(|item| *item != &file_name)
        .cloned()
        .collect::<Vec<_>>();
    let _ = std::fs::remove_file(project_photos_dir(&state, project_id).join(&file_name));
    state
        .db
        .set_project_photos(project_id, &photos)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn render_preview(state: State<'_, AppState>, project: AnniversaryProject) -> String {
    let photo_base = format!(
        "http://127.0.0.1:{}/a/{}/photos/",
        state.port, project.slug
    );
    let media_base = format!(
        "http://127.0.0.1:{}/a/{}/media/",
        state.port, project.slug
    );
    connect_core::anniversary::render_with_db(&state.db, &project, &photo_base, &media_base)
}

#[tauri::command]
fn add_project_media(
    state: State<'_, AppState>,
    project_id: i64,
    kind: String,
    file_name: String,
    data_base64: String,
) -> Result<AnniversaryProject, String> {
    if kind != "music" && kind != "video" {
        return Err("kind 必须是 music 或 video".to_string());
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|err| format!("文件 base64 无效：{err}"))?;
    if data.len() > 40 * 1024 * 1024 {
        return Err("单个音视频文件不能超过 40 MB".to_string());
    }

    let ext = std::path::Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let allowed = if kind == "music" {
        ["mp3", "m4a", "wav", "ogg"].contains(&ext.as_str())
    } else {
        ["mp4", "webm", "mov"].contains(&ext.as_str())
    };
    if !allowed {
        return Err("不支持的音视频格式".to_string());
    }

    let dir = state
        .data_dir
        .join("anniversaries")
        .join(project_id.to_string())
        .join("media");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let stored = format!("{}.{}", uuid::Uuid::new_v4().simple(), ext);
    std::fs::write(dir.join(&stored), data).map_err(|err| err.to_string())?;

    let project = state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let old = if kind == "music" {
        project.music.clone()
    } else {
        project.video.clone()
    };
    if !old.is_empty() && !old.starts_with("http") {
        let _ = std::fs::remove_file(dir.join(old.trim_start_matches("media/")));
    }

    state
        .db
        .set_project_media(project_id, &kind, &stored)
        .map_err(|err| err.to_string())?;
    state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "项目不存在".to_string())
}

#[tauri::command]
fn remove_project_media(
    state: State<'_, AppState>,
    project_id: i64,
    kind: String,
) -> Result<(), String> {
    if kind != "music" && kind != "video" {
        return Err("kind 必须是 music 或 video".to_string());
    }
    let project = state
        .db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let old = if kind == "music" {
        project.music
    } else {
        project.video
    };
    if !old.is_empty() && !old.starts_with("http") {
        let dir = state
            .data_dir
            .join("anniversaries")
            .join(project_id.to_string())
            .join("media");
        let _ = std::fs::remove_file(dir.join(old.trim_start_matches("media/")));
    }
    state
        .db
        .set_project_media(project_id, &kind, "")
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn list_project_versions(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<ProjectVersion>, String> {
    state
        .db
        .list_project_versions(project_id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn list_ai_providers(state: State<'_, AppState>) -> Result<Vec<AiProvider>, String> {
    state.db.list_ai_providers().map_err(|err| err.to_string())
}

#[tauri::command]
fn create_ai_provider(
    state: State<'_, AppState>,
    input: AiProviderInput,
) -> Result<AiProvider, String> {
    state
        .db
        .create_ai_provider(&input)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_ai_provider(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state
        .db
        .delete_ai_provider(id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn generate_copy(
    state: State<'_, AppState>,
    provider_id: i64,
    input: AiGenerateInput,
) -> Result<connect_core::GeneratedCopy, String> {
    let provider = state
        .db
        .get_ai_provider(provider_id)
        .map_err(|err| err.to_string())?
        .ok_or("AI provider 不存在")?;
    connect_core::ai::generate_copy(&provider, &input)
        .await
        .map_err(|err| err.to_string())
}

/// Windows 11（内核版本号 ≥ 22000）才支持云母材质。
fn supports_mica() -> bool {
    if !cfg!(windows) {
        return false;
    }
    sysinfo::System::kernel_version()
        .map(|version| {
            version
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|part| part.parse::<u32>().ok())
                .any(|build| build >= 22000)
        })
        .unwrap_or(false)
}

/// 同步窗口外观：标题栏明暗跟随应用主题；`mica` 为真且系统支持时启用云母材质。
/// 返回云母是否已生效，前端据此决定是否把背景设为透明。
#[tauri::command]
fn set_window_appearance(window: tauri::WebviewWindow, mica: bool, dark: bool) -> Result<bool, String> {
    let _ = window.set_theme(Some(if dark { tauri::Theme::Dark } else { tauri::Theme::Light }));
    if mica && supports_mica() {
        use tauri::window::{Effect, EffectState, EffectsBuilder};
        window
            .set_effects(
                EffectsBuilder::new()
                    .effect(Effect::Mica)
                    .state(EffectState::FollowsWindowActiveState)
                    .build(),
            )
            .map_err(|err| err.to_string())?;
        Ok(true)
    } else {
        window.set_effects(None).map_err(|err| err.to_string())?;
        Ok(false)
    }
}

#[tauri::command]
fn list_custom_templates(state: State<'_, AppState>) -> Result<Vec<CustomTemplate>, String> {
    state.db.list_custom_templates().map_err(|err| err.to_string())
}

/// AI 设计模板。`base_id` 有值时在该模板基础上按要求调整（覆盖保存），否则新建一个模板。
#[tauri::command]
async fn generate_custom_template(
    state: State<'_, AppState>,
    provider_id: i64,
    request: String,
    theme: String,
    base_id: Option<i64>,
) -> Result<CustomTemplate, String> {
    let provider = state
        .db
        .get_ai_provider(provider_id)
        .map_err(|err| err.to_string())?
        .ok_or("AI provider 不存在")?;
    let base = match base_id {
        Some(id) => Some(
            state
                .db
                .get_custom_template(id)
                .map_err(|err| err.to_string())?
                .ok_or("要调整的模板不存在")?,
        ),
        None => None,
    };
    let generated = connect_core::ai::generate_template(
        &provider,
        &request,
        &theme,
        base.as_ref().map(|t| t.css.as_str()),
    )
    .await
    .map_err(|err| err.to_string())?;

    match base {
        Some(base) => {
            let prompt = format!("{}\n调整：{}", base.prompt, request.trim());
            state
                .db
                .update_custom_template(base.id, &base.name, prompt.trim(), &generated.css, generated.dark)
        }
        None => state.db.create_custom_template(
            &generated.name,
            request.trim(),
            &generated.css,
            generated.dark,
        ),
    }
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn rename_custom_template(state: State<'_, AppState>, id: i64, name: String) -> Result<CustomTemplate, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("模板名称不能为空".to_string());
    }
    let template = state
        .db
        .get_custom_template(id)
        .map_err(|err| err.to_string())?
        .ok_or("模板不存在")?;
    state
        .db
        .update_custom_template(id, name, &template.prompt, &template.css, template.dark)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_custom_template(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.db.delete_custom_template(id).map_err(|err| err.to_string())
}

#[tauri::command]
fn list_publish_targets(state: State<'_, AppState>) -> Result<Vec<PublishTarget>, String> {
    state
        .db
        .list_publish_targets()
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn create_publish_target(
    state: State<'_, AppState>,
    input: PublishTargetInput,
) -> Result<PublishTarget, String> {
    state
        .db
        .create_publish_target(&input)
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn delete_publish_target(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state
        .db
        .delete_publish_target(id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn test_publish_target(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let target = state
        .db
        .get_publish_target(id)
        .map_err(|err| err.to_string())?
        .ok_or("发布目标不存在")?;
    // 使用授权账户的目标：测试时顺便创建 bucket / Pages 项目 / 隧道
    let target = connect_core::resolve_cloudflare_target(&state.db, &target)
        .await
        .map_err(|err| format!("{err:#}"))?;
    if connect_core::cloudflare::uses_auth(&target) && target.kind != "cloudflare_tunnel" {
        return Ok(());
    }

    if target.kind == "local" {
        // 本机发布不需要任何配置
        return Ok(());
    }
    if connect_core::tunnel::is_tunnel_kind(&target.kind) {
        // 临时建立隧道验证配置（需要时会自动下载 cloudflared / frpc）
        state
            .tunnels
            .test(&target, connect_core::server::PUBLIC_PREVIEW_PORT)
            .await
            .map(|_| ())
            .map_err(|err| format!("{err:#}"))
    } else if target.kind == "cloudflare_pages" {
        connect_core::pages::test_target(&target)
            .await
            .map_err(|err| err.to_string())
    } else {
        connect_core::publish::test_target(&target)
            .await
            .map_err(|err| err.to_string())
    }
}

#[tauri::command]
async fn publish_project(
    state: State<'_, AppState>,
    project_id: i64,
    target_id: i64,
) -> Result<PublishResult, String> {
    let db = state.db.clone();
    let data_dir = state.data_dir.clone();
    let tunnels = state.tunnels.clone();

    let project = db
        .get_project(project_id)
        .map_err(|err| err.to_string())?
        .ok_or("项目不存在")?;
    let target = db
        .get_publish_target(target_id)
        .map_err(|err| err.to_string())?
        .ok_or("发布目标不存在")?;

    // 本机发布：由本机服务直接提供页面，同一局域网内用内网地址查看
    if target.kind == "local" {
        let url = connect_core::server::local_page_url(state.port, &project.slug);
        let _ = db.add_project_version(project.id, &url, &target.kind);
        return Ok(PublishResult {
            url,
            slug: project.slug,
            uploaded: 0,
        });
    }

    if connect_core::tunnel::is_tunnel_kind(&target.kind) {
        let target = connect_core::resolve_cloudflare_target(&db, &target)
            .await
            .map_err(|err| format!("{err:#}"))?;
        let tunnel = tunnels
            .ensure(&target, connect_core::server::PUBLIC_PREVIEW_PORT)
            .await
            .map_err(|err| format!("{err:#}"))?;
        let url = format!("{}/a/{}", tunnel.url.trim_end_matches('/'), project.slug);
        let _ = db.add_project_version(project.id, &url, &target.kind);
        return Ok(PublishResult {
            url,
            slug: project.slug,
            uploaded: 0,
        });
    }

    // S3/R2/Cloudflare Pages：渲染时写入分享卡片元数据（标题、描述、缩略图）。
    let result = connect_core::share::publish_static(&db, &data_dir, &project, &target)
        .await
        .map_err(|err| err.to_string())?;
    let _ = db.add_project_version(project.id, &result.url, &target.kind);
    Ok(result)
}

/// 「关于」对话框里显示的信息。
#[derive(Serialize)]
struct AboutInfo {
    name: &'static str,
    version: &'static str,
    tauri_version: &'static str,
    os: &'static str,
    arch: &'static str,
    os_version: String,
    data_dir: String,
    lan_address: String,
    homepage: &'static str,
}

#[tauri::command]
fn get_about_info(state: State<'_, AppState>) -> AboutInfo {
    AboutInfo {
        name: "EasyTidy Connect",
        version: env!("CARGO_PKG_VERSION"),
        tauri_version: tauri::VERSION,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        os_version: sysinfo::System::long_os_version().unwrap_or_default(),
        data_dir: state.data_dir.display().to_string(),
        lan_address: format!("{}:{}", connect_core::local_ip(), state.port),
        homepage: "https://github.com/EasyTidy/EasyTidyConnect",
    }
}

#[tauri::command]
fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.data_dir.display().to_string(), None::<&str>)
        .map_err(|err| err.to_string())
}

/// 把公开链接写入 NFC 用的信息：局域网写入辅助页（手机扫码复制）与占用字节。
#[derive(Serialize)]
struct NfcLinkInfo {
    url: String,
    write_page: String,
    bytes: usize,
}

#[tauri::command]
fn nfc_link_info(state: State<'_, AppState>, url: String, name: String) -> Result<NfcLinkInfo, String> {
    let url = url.trim().to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("只能写入 http(s) 链接".to_string());
    }
    let encode = |value: &str| -> String { url::form_urlencoded::byte_serialize(value.as_bytes()).collect() };
    let bytes = connect_core::nfc::ndef_uri_size(&url);
    Ok(NfcLinkInfo {
        write_page: format!(
            "http://{}:{}/nfc?u={}&n={}",
            connect_core::local_ip(),
            state.port,
            encode(&url),
            encode(name.trim())
        ),
        bytes,
        url,
    })
}

/* ---------- Cloudflare 一键授权 ---------- */

fn cloudflare_client_id(state: &AppState) -> String {
    let saved = state.db.get_cloudflare_client_id().unwrap_or_default();
    if saved.trim().is_empty() {
        connect_core::cloudflare::default_client_id()
    } else {
        saved
    }
}

fn cloudflare_status(state: &AppState) -> connect_core::cloudflare::CloudflareStatus {
    let auth = state.db.get_cloudflare_auth().unwrap_or_default();
    let client_id = cloudflare_client_id(state);
    connect_core::cloudflare::CloudflareStatus {
        connected: auth.is_connected(),
        method: auth.method,
        account_id: auth.account_id,
        account_name: auth.account_name,
        oauth_available: !client_id.trim().is_empty(),
        client_id,
    }
}

#[tauri::command]
fn cloudflare_get_status(state: State<'_, AppState>) -> connect_core::cloudflare::CloudflareStatus {
    cloudflare_status(&state)
}

#[tauri::command]
fn cloudflare_set_client_id(
    state: State<'_, AppState>,
    client_id: String,
) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    state
        .db
        .save_cloudflare_client_id(&client_id)
        .map_err(|err| err.to_string())?;
    Ok(cloudflare_status(&state))
}

/// 选定账户：只有一个时自动选中，多个时优先保留已选的，否则取第一个。
async fn finish_cloudflare_auth(
    state: &AppState,
    mut auth: connect_core::cloudflare::CloudflareAuth,
) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    let api = connect_core::cloudflare::Api::new(&auth.access_token).map_err(|err| err.to_string())?;
    let accounts = api.accounts().await.map_err(|err| format!("{err:#}"))?;
    let Some(first) = accounts.first() else {
        return Err("这个授权看不到任何 Cloudflare 账户，请确认授权时选择了账户".to_string());
    };
    let previous = state.db.get_cloudflare_auth().unwrap_or_default().account_id;
    let chosen = accounts.iter().find(|a| a.id == previous).unwrap_or(first);
    auth.account_id = chosen.id.clone();
    auth.account_name = chosen.name.clone();
    state.db.save_cloudflare_auth(&auth).map_err(|err| err.to_string())?;
    Ok(cloudflare_status(state))
}

/// 一键授权：打开浏览器 → 用户在 Cloudflare 同意 → 本机 8976 端口收到回调 → 换取令牌 → 选定账户。
#[tauri::command]
async fn cloudflare_authorize(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    use tauri_plugin_opener::OpenerExt;
    let client_id = cloudflare_client_id(&state);
    let pending = connect_core::cloudflare::begin_authorization(&client_id).map_err(|err| err.to_string())?;
    let waiter = connect_core::cloudflare::wait_for_callback(&pending);
    app.opener()
        .open_url(pending.url.clone(), None::<&str>)
        .map_err(|err| format!("无法打开浏览器：{err}"))?;
    let code = waiter.await.map_err(|err| format!("{err:#}"))?;
    let auth = connect_core::cloudflare::exchange_code(&pending, &code)
        .await
        .map_err(|err| format!("{err:#}"))?;
    finish_cloudflare_auth(&state, auth).await
}

/// 兜底：用户粘贴在 Cloudflare 创建的 API Token。
#[tauri::command]
async fn cloudflare_connect_token(
    state: State<'_, AppState>,
    token: String,
) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("请粘贴 API Token".to_string());
    }
    let auth = connect_core::cloudflare::CloudflareAuth {
        method: "token".to_string(),
        access_token: token,
        ..Default::default()
    };
    finish_cloudflare_auth(&state, auth).await
}

#[tauri::command]
fn cloudflare_token_template_url() -> String {
    connect_core::cloudflare::token_template_url()
}

#[tauri::command]
async fn cloudflare_list_accounts(state: State<'_, AppState>) -> Result<Vec<connect_core::cloudflare::Account>, String> {
    let auth = connect_core::cloudflare_auth(&state.db).await.map_err(|err| format!("{err:#}"))?;
    connect_core::cloudflare::Api::new(&auth.access_token)
        .map_err(|err| err.to_string())?
        .accounts()
        .await
        .map_err(|err| format!("{err:#}"))
}

#[tauri::command]
fn cloudflare_select_account(
    state: State<'_, AppState>,
    account: connect_core::cloudflare::Account,
) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    let mut auth = state.db.get_cloudflare_auth().map_err(|err| err.to_string())?;
    auth.account_id = account.id;
    auth.account_name = account.name;
    state.db.save_cloudflare_auth(&auth).map_err(|err| err.to_string())?;
    Ok(cloudflare_status(&state))
}

#[tauri::command]
async fn cloudflare_disconnect(state: State<'_, AppState>) -> Result<connect_core::cloudflare::CloudflareStatus, String> {
    let auth = state.db.get_cloudflare_auth().unwrap_or_default();
    connect_core::cloudflare::revoke(&auth).await;
    state
        .db
        .save_cloudflare_auth(&Default::default())
        .map_err(|err| err.to_string())?;
    Ok(cloudflare_status(&state))
}

#[tauri::command]
fn get_wechat_config(state: State<'_, AppState>) -> Result<WechatConfig, String> {
    state.db.get_wechat_config().map_err(|err| err.to_string())
}

#[tauri::command]
fn save_wechat_config(state: State<'_, AppState>, config: WechatConfig) -> Result<(), String> {
    // 使用签名服务器时 AppSecret 只放在服务器上，本机不保存。
    let config = if config.uses_signing_server() {
        WechatConfig { app_secret: String::new(), ..config }
    } else {
        config
    };
    state.db.save_wechat_config(&config).map_err(|err| err.to_string())
}

/// 测试微信签名：填了签名服务地址就请求它（验证服务器 IP 白名单等）；否则用本机直接换票验证 AppID / AppSecret。
#[tauri::command]
async fn test_wechat(config: WechatConfig) -> Result<String, String> {
    let domain = config
        .allowed_domains
        .split([',', '，', ' ', '\n'])
        .map(|item| item.trim().trim_start_matches("*.").trim_start_matches('.'))
        .find(|item| !item.is_empty())
        .unwrap_or("example.com")
        .to_string();
    let page = format!("https://{domain}/");
    let signer = config.signer_url.trim().to_string();

    // 本机签名：先在本机换票，验证 AppID / AppSecret 与本机 IP 白名单。
    if !config.uses_signing_server() {
        connect_core::wechat::shared_cache()
            .signature(&config.to_signer(), &page)
            .await
            .map_err(|err| format!("{err:#}"))?;
        if signer.is_empty() {
            return Ok("AppID / AppSecret 有效，本机 IP 已在白名单中；还需填写签名服务地址（内网穿透地址/wx/jssdk）".to_string());
        }
    } else if signer.is_empty() {
        return Err("请填写签名服务器地址".to_string());
    }

    // 再请求签名服务地址，确认发布页实际调用的接口可用。
    let response = reqwest::Client::new()
        .get(&signer)
        .query(&[("url", page.as_str())])
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("无法访问签名服务：{err}"))?;
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("签名服务返回的不是 JSON：{err}"))?;
    match body.get("signature").and_then(|v| v.as_str()) {
        Some(_) => Ok(format!(
            "签名服务正常（AppID {}）",
            body.get("appId").and_then(|v| v.as_str()).unwrap_or("")
        )),
        None => Err(body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("签名服务返回了未知结果")
            .to_string()),
    }
}

#[tauri::command]
fn get_share_settings(state: State<'_, AppState>, project_id: i64) -> Result<ShareSettings, String> {
    state
        .db
        .get_share_settings(project_id)
        .map(|value| value.unwrap_or_default())
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn save_share_settings(
    state: State<'_, AppState>,
    project_id: i64,
    settings: ShareSettings,
) -> Result<(), String> {
    state
        .db
        .save_share_settings(project_id, &settings)
        .map_err(|err| err.to_string())
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
        .plugin(tauri_plugin_opener::init())
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
            migrate_legacy_data_dir(&app_dir);
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
            let data_dir_task = app_dir.clone();
            // 自动下载的 cloudflared / frpc 放在应用数据目录的 tools 下
            let tunnels = Arc::new(connect_core::TunnelManager::new(app_dir.join("tools")));
            let tunnels_task = tunnels.clone();

            tauri::async_runtime::spawn(async move {
                if let Err(err) =
                    connect_core::server::serve(db_task, port, data_dir_task, tunnels_task).await
                {
                    tracing::error!("connect server stopped: {err}");
                    server_ok_task.store(false, Ordering::Relaxed);
                }
            });

            let public_db = db.clone();
            let public_data = app_dir.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = connect_core::server::serve_public(
                    public_db,
                    connect_core::server::PUBLIC_PREVIEW_PORT,
                    public_data,
                )
                .await
                {
                    tracing::error!("public preview server stopped: {err}");
                }
            });

            app.manage(AppState {
                db,
                port,
                server_ok,
                data_dir: app_dir,
                tunnels,
            });

            let version = env!("CARGO_PKG_VERSION");
            let show_item = MenuItem::with_id(app, "show", "打开 EasyTidy Connect", true, None::<&str>)?;
            let about_item = MenuItem::with_id(app, "about", "关于 EasyTidy Connect", true, None::<&str>)?;
            let version_item = MenuItem::with_id(app, "version", format!("版本 {version}"), false, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show_item, &about_item, &separator, &version_item, &quit_item])?;

            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("window icon"))
                .tooltip(format!("EasyTidy Connect v{version}"))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "about" => {
                        // 打开主窗口并显示「关于」对话框（前端监听 show-about 事件）
                        show_main(app);
                        let _ = app.emit("show-about", ());
                    }
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
            import_bridge,
            list_templates,
            list_projects,
            get_project,
            create_project,
            update_project,
            delete_project,
            add_project_photo,
            remove_project_photo,
            reorder_project_photos,
            add_project_media,
            remove_project_media,
            render_preview,
            list_project_versions,
            list_ai_providers,
            create_ai_provider,
            delete_ai_provider,
            generate_copy,
            set_window_appearance,
            list_custom_templates,
            generate_custom_template,
            rename_custom_template,
            delete_custom_template,
            list_publish_targets,
            create_publish_target,
            delete_publish_target,
            test_publish_target,
            publish_project,
            get_share_settings,
            save_share_settings,
            get_about_info,
            open_data_dir,
            nfc_link_info,
            cloudflare_get_status,
            cloudflare_set_client_id,
            cloudflare_authorize,
            cloudflare_connect_token,
            cloudflare_token_template_url,
            cloudflare_list_accounts,
            cloudflare_select_account,
            cloudflare_disconnect,
            get_wechat_config,
            save_wechat_config,
            test_wechat
        ])
        .build(tauri::generate_context!())
        .expect("error while building EasyTidy Connect")
        .run(|app_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                let tunnels = app_handle.state::<AppState>().tunnels.clone();
                tauri::async_runtime::block_on(async move {
                    tunnels.stop_all().await;
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_data_dir_once() {
        let root = std::env::temp_dir().join(format!("connect-migrate-{}", uuid::Uuid::new_v4()));
        let old = root.join("com.easytidy.connect");
        std::fs::create_dir_all(old.join("anniversaries/1/photos")).unwrap();
        std::fs::write(old.join("connect.db"), b"db").unwrap();
        std::fs::write(old.join("anniversaries/1/photos/a.jpg"), b"jpg").unwrap();
        let new = root.join("net.easytidy.connect");
        // WebView 可能已经提前创建了空的新目录
        std::fs::create_dir_all(&new).unwrap();

        migrate_legacy_data_dir(&new);
        assert_eq!(std::fs::read(new.join("connect.db")).unwrap(), b"db");
        assert_eq!(std::fs::read(new.join("anniversaries/1/photos/a.jpg")).unwrap(), b"jpg");

        // 新目录已有数据库时不再迁移、不覆盖
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("connect.db"), b"old-again").unwrap();
        migrate_legacy_data_dir(&new);
        assert_eq!(std::fs::read(new.join("connect.db")).unwrap(), b"db");
        let _ = std::fs::remove_dir_all(root);
    }
}
