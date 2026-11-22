use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::models::{
    AiProvider, AiProviderInput, AnniversaryProject, CustomTemplate, ProjectInput, ProjectVersion,
    PublishTarget, PublishTargetInput, RunView, Settings, ShareSettings, TriggerRow, TriggerView,
    WechatConfig,
};

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create data dir")?;
        }

        let conn = Connection::open(path).context("open sqlite")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS triggers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                action_type TEXT NOT NULL,
                action_config TEXT NOT NULL DEFAULT '{}',
                public_id TEXT NOT NULL UNIQUE,
                token TEXT NOT NULL,
                write_token TEXT NOT NULL UNIQUE,
                tag_written INTEGER NOT NULL DEFAULT 0,
                verified_at TEXT,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trigger_id INTEGER NOT NULL,
                status TEXT NOT NULL,
                source TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                error TEXT,
                external_run_id TEXT
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS anniversary_projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                event_date TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                recipient TEXT NOT NULL DEFAULT '',
                opening TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL DEFAULT '',
                closing TEXT NOT NULL DEFAULT '',
                template_id TEXT NOT NULL DEFAULT 'minimal',
                theme TEXT NOT NULL DEFAULT '#2563eb',
                photos_json TEXT NOT NULL DEFAULT '[]',
                music TEXT NOT NULL DEFAULT '',
                video TEXT NOT NULL DEFAULT '',
                custom_css TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS anniversary_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                version INTEGER NOT NULL,
                published_url TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'published',
                published_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS project_shares (
                project_id INTEGER PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS custom_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                prompt TEXT NOT NULL DEFAULT '',
                css TEXT NOT NULL,
                dark INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ai_providers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                base_url TEXT NOT NULL,
                model TEXT NOT NULL,
                api_key TEXT NOT NULL DEFAULT '',
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS publish_targets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                region TEXT NOT NULL DEFAULT 'auto',
                bucket TEXT NOT NULL,
                access_key TEXT NOT NULL,
                secret_key TEXT NOT NULL,
                prefix TEXT NOT NULL DEFAULT 'anniversary',
                public_base_url TEXT NOT NULL DEFAULT '',
                cloudflared_path TEXT NOT NULL DEFAULT '',
                tunnel_token TEXT NOT NULL DEFAULT '',
                hostname TEXT NOT NULL DEFAULT '',
                account_id TEXT NOT NULL DEFAULT '',
                project_name TEXT NOT NULL DEFAULT '',
                api_token TEXT NOT NULL DEFAULT '',
                branch TEXT NOT NULL DEFAULT 'main',
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            "#,
        )?;

        // 兼容 M1/M2 早期数据库。
        let _ = conn.execute("ALTER TABLE runs ADD COLUMN external_run_id TEXT", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN cloudflared_path TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN tunnel_token TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN hostname TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN account_id TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN project_name TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN api_token TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN branch TEXT NOT NULL DEFAULT 'main'", []);
        let _ = conn.execute("ALTER TABLE anniversary_projects ADD COLUMN music TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE anniversary_projects ADD COLUMN video TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE anniversary_projects ADD COLUMN photo_focus TEXT NOT NULL DEFAULT '{}'", []);
        let _ = conn.execute("ALTER TABLE anniversary_projects ADD COLUMN slideshow INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE anniversary_versions ADD COLUMN target_kind TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE publish_targets ADD COLUMN tunnel_config TEXT NOT NULL DEFAULT ''", []);

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| anyhow::anyhow!("database lock poisoned"))
    }

    pub fn list_triggers(&self) -> Result<Vec<TriggerView>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, enabled, action_type, action_config, public_id,
                    tag_written, verified_at, created_at
             FROM triggers ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(TriggerView {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    enabled: row.get::<_, i64>(2)? != 0,
                    action_type: row.get(3)?,
                    action_config: row.get(4)?,
                    public_id: row.get(5)?,
                    tag_written: row.get::<_, i64>(6)? != 0,
                    verified_at: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_trigger(&self, id: i64) -> Result<Option<TriggerView>> {
        Ok(self.list_triggers()?.into_iter().find(|t| t.id == id))
    }

    pub fn create_trigger(
        &self,
        name: &str,
        action_type: &str,
        action_config: &str,
    ) -> Result<TriggerView> {
        let id = Uuid::new_v4().simple().to_string();
        let public_id = id[..8].to_string();
        let token = Uuid::new_v4().simple().to_string();
        let write_token = Uuid::new_v4().simple().to_string();
        let now = Utc::now().to_rfc3339();

        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO triggers
             (name, enabled, action_type, action_config, public_id, token, write_token, created_at)
             VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![name, action_type, action_config, public_id, token, write_token, now],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);

        self.get_trigger(id)?.context("created trigger missing")
    }

    pub fn update_trigger(
        &self,
        id: i64,
        name: &str,
        enabled: bool,
        action_type: &str,
        action_config: &str,
    ) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE triggers SET name=?1, enabled=?2, action_type=?3, action_config=?4 WHERE id=?5",
            params![name, if enabled { 1 } else { 0 }, action_type, action_config, id],
        )?;
        Ok(())
    }

    pub fn set_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE triggers SET enabled=?1 WHERE id=?2",
            params![if enabled { 1 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn delete_trigger(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM triggers WHERE id=?1", params![id])?;
        conn.execute("DELETE FROM runs WHERE trigger_id=?1", params![id])?;
        Ok(())
    }

    pub fn find_by_public_id(&self, public_id: &str) -> Result<Option<TriggerRow>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, enabled, action_type, action_config, public_id, token,
                    write_token, tag_written, verified_at, created_at
             FROM triggers WHERE public_id=?1",
            params![public_id],
            map_trigger_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn find_by_id(&self, id: i64) -> Result<Option<TriggerRow>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, enabled, action_type, action_config, public_id, token,
                    write_token, tag_written, verified_at, created_at
             FROM triggers WHERE id=?1",
            params![id],
            map_trigger_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn find_by_write_token(&self, write_token: &str) -> Result<Option<TriggerRow>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, enabled, action_type, action_config, public_id, token,
                    write_token, tag_written, verified_at, created_at
             FROM triggers WHERE write_token=?1",
            params![write_token],
            map_trigger_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn mark_tag_written(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("UPDATE triggers SET tag_written=1 WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn mark_verified(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE triggers SET verified_at=?1 WHERE id=?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn start_run(&self, trigger_id: i64, source: &str) -> Result<i64> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO runs (trigger_id, status, source, started_at)
             VALUES (?1, 'running', ?2, ?3)",
            params![trigger_id, source, Utc::now().to_rfc3339()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_run(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE runs SET status=?1, finished_at=?2, error=?3 WHERE id=?4",
            params![status, Utc::now().to_rfc3339(), error, id],
        )?;
        Ok(())
    }

    pub fn set_external_run(&self, id: i64, external_run_id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE runs SET external_run_id=?1 WHERE id=?2",
            params![external_run_id, id],
        )?;
        Ok(())
    }

    pub fn list_runs(&self, limit: i64) -> Result<Vec<RunView>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT r.id, r.trigger_id, COALESCE(t.name, '(deleted)'), r.status, r.source,
                    r.started_at, r.finished_at, r.error, r.external_run_id
             FROM runs r LEFT JOIN triggers t ON t.id = r.trigger_id
             ORDER BY r.id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(RunView {
                    id: row.get(0)?,
                    trigger_id: row.get(1)?,
                    trigger_name: row.get(2)?,
                    status: row.get(3)?,
                    source: row.get(4)?,
                    started_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    error: row.get(7)?,
                    external_run_id: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.lock()?;
        let device_name = get_setting(&conn, "device_name")?
            .unwrap_or_else(|| "EasyTidy Connect".to_string());
        let port = get_setting(&conn, "port")?
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(53400);
        let auto_start = get_setting(&conn, "auto_start")?
            .map(|value| value == "1")
            .unwrap_or(false);
        let bridge_url = get_setting(&conn, "bridge_url")?.unwrap_or_default();
        let bridge_token = get_setting(&conn, "bridge_token")?.unwrap_or_default();
        let integration_mode = get_setting(&conn, "integration_mode")?
            .filter(|value| !value.is_empty());

        Ok(Settings {
            device_name,
            port,
            auto_start,
            bridge_url,
            bridge_token,
            integration_mode,
        })
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let conn = self.lock()?;
        set_setting(&conn, "device_name", &settings.device_name)?;
        set_setting(&conn, "port", &settings.port.to_string())?;
        set_setting(&conn, "auto_start", if settings.auto_start { "1" } else { "0" })?;
        set_setting(&conn, "bridge_url", &settings.bridge_url)?;
        set_setting(&conn, "bridge_token", &settings.bridge_token)?;
        set_setting(
            &conn,
            "integration_mode",
            settings.integration_mode.as_deref().unwrap_or(""),
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<AnniversaryProject>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, slug, name, event_date, title, recipient, opening, body, closing,
                    template_id, theme, photos_json, music, video, custom_css, created_at, updated_at, photo_focus, slideshow
             FROM anniversary_projects ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], map_project)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_project(&self, id: i64) -> Result<Option<AnniversaryProject>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, slug, name, event_date, title, recipient, opening, body, closing,
                    template_id, theme, photos_json, music, video, custom_css, created_at, updated_at, photo_focus, slideshow
             FROM anniversary_projects WHERE id=?1",
            params![id],
            map_project,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_project_by_slug(&self, slug: &str) -> Result<Option<AnniversaryProject>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, slug, name, event_date, title, recipient, opening, body, closing,
                    template_id, theme, photos_json, music, video, custom_css, created_at, updated_at, photo_focus, slideshow
             FROM anniversary_projects WHERE slug=?1",
            params![slug],
            map_project,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn create_project(&self, input: &ProjectInput) -> Result<AnniversaryProject> {
        let slug = Uuid::new_v4().simple().to_string()[..8].to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO anniversary_projects
             (slug, name, event_date, title, recipient, opening, body, closing,
              template_id, theme, photos_json, music, video, custom_css, created_at, updated_at, photo_focus, slideshow)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'[]',?11,?12,?13,?14,?14,?15,?16)",
            params![
                slug,
                input.name,
                input.event_date,
                input.title,
                input.recipient,
                input.opening,
                input.body,
                input.closing,
                input.template_id,
                input.theme,
                input.music,
                input.video,
                input.custom_css,
                now,
                serde_json::to_string(&input.photo_focus)?,
                input.slideshow
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_project(id)?.context("created project missing")
    }

    pub fn update_project(&self, id: i64, input: &ProjectInput) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE anniversary_projects SET
                name=?1, event_date=?2, title=?3, recipient=?4, opening=?5, body=?6,
                closing=?7, template_id=?8, theme=?9, music=?10, video=?11,
                custom_css=?12, updated_at=?13, photo_focus=?15, slideshow=?16
             WHERE id=?14",
            params![
                input.name,
                input.event_date,
                input.title,
                input.recipient,
                input.opening,
                input.body,
                input.closing,
                input.template_id,
                input.theme,
                input.music,
                input.video,
                input.custom_css,
                Utc::now().to_rfc3339(),
                id,
                serde_json::to_string(&input.photo_focus)?,
                input.slideshow
            ],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM anniversary_projects WHERE id=?1", params![id])?;
        conn.execute("DELETE FROM anniversary_versions WHERE project_id=?1", params![id])?;
        conn.execute("DELETE FROM project_shares WHERE project_id=?1", params![id])?;
        Ok(())
    }

    pub fn set_project_photos(&self, id: i64, photos: &[String]) -> Result<()> {
        let json = serde_json::to_string(photos)?;
        let conn = self.lock()?;
        conn.execute(
            "UPDATE anniversary_projects SET photos_json=?1, updated_at=?2 WHERE id=?3",
            params![json, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn set_project_media(&self, id: i64, kind: &str, value: &str) -> Result<()> {
        let column = match kind {
            "music" => "music",
            "video" => "video",
            _ => anyhow::bail!("未知媒体类型：{kind}"),
        };
        let conn = self.lock()?;
        conn.execute(
            &format!(
                "UPDATE anniversary_projects SET {column}=?1, updated_at=?2 WHERE id=?3"
            ),
            params![value, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn add_project_version(
        &self,
        project_id: i64,
        published_url: &str,
        target_kind: &str,
    ) -> Result<ProjectVersion> {
        let conn = self.lock()?;
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version),0)+1 FROM anniversary_versions WHERE project_id=?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(1);
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO anniversary_versions(project_id, version, published_url, status, published_at, target_kind)
             VALUES (?1,?2,?3,'published',?4,?5)",
            params![project_id, version, published_url, now, target_kind],
        )?;
        Ok(ProjectVersion {
            id: conn.last_insert_rowid(),
            project_id,
            version,
            published_url: published_url.to_string(),
            status: "published".to_string(),
            published_at: now,
            target_kind: target_kind.to_string(),
        })
    }

    pub fn list_project_versions(&self, project_id: i64) -> Result<Vec<ProjectVersion>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, version, published_url, status, published_at, target_kind
             FROM anniversary_versions WHERE project_id=?1 ORDER BY version DESC",
        )?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(ProjectVersion {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    version: row.get(2)?,
                    published_url: row.get(3)?,
                    status: row.get(4)?,
                    published_at: row.get(5)?,
                    target_kind: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_ai_providers(&self) -> Result<Vec<AiProvider>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, base_url, model, api_key, is_default, created_at
             FROM ai_providers ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AiProvider {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    base_url: row.get(3)?,
                    model: row.get(4)?,
                    api_key: row.get(5)?,
                    is_default: row.get::<_, i64>(6)? != 0,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn create_ai_provider(&self, input: &AiProviderInput) -> Result<AiProvider> {
        let conn = self.lock()?;
        if input.is_default {
            conn.execute("UPDATE ai_providers SET is_default=0", [])?;
        }
        conn.execute(
            "INSERT INTO ai_providers(name, kind, base_url, model, api_key, is_default, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                input.name,
                input.kind,
                input.base_url,
                input.model,
                input.api_key,
                if input.is_default { 1 } else { 0 },
                Utc::now().to_rfc3339()
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.list_ai_providers()?
            .into_iter()
            .find(|p| p.id == id)
            .context("created ai provider missing")
    }

    pub fn delete_ai_provider(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM ai_providers WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn get_ai_provider(&self, id: i64) -> Result<Option<AiProvider>> {
        Ok(self.list_ai_providers()?.into_iter().find(|p| p.id == id))
    }

    pub fn list_publish_targets(&self) -> Result<Vec<PublishTarget>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, endpoint, region, bucket, access_key, secret_key,
                    prefix, public_base_url, cloudflared_path, tunnel_token, hostname,
                    account_id, project_name, api_token, branch, is_default, created_at, tunnel_config
             FROM publish_targets ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PublishTarget {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    endpoint: row.get(3)?,
                    region: row.get(4)?,
                    bucket: row.get(5)?,
                    access_key: row.get(6)?,
                    secret_key: row.get(7)?,
                    prefix: row.get(8)?,
                    public_base_url: row.get(9)?,
                    cloudflared_path: row.get(10)?,
                    tunnel_token: row.get(11)?,
                    hostname: row.get(12)?,
                    account_id: row.get(13)?,
                    project_name: row.get(14)?,
                    api_token: row.get(15)?,
                    branch: row.get(16)?,
                    is_default: row.get::<_, i64>(17)? != 0,
                    created_at: row.get(18)?,
                    tunnel_config: row.get(19)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn create_publish_target(&self, input: &PublishTargetInput) -> Result<PublishTarget> {
        let conn = self.lock()?;
        if input.is_default {
            conn.execute("UPDATE publish_targets SET is_default=0", [])?;
        }
        conn.execute(
            "INSERT INTO publish_targets
             (name, kind, endpoint, region, bucket, access_key, secret_key, prefix,
              public_base_url, cloudflared_path, tunnel_token, hostname, account_id,
              project_name, api_token, branch, is_default, created_at, tunnel_config)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                input.name,
                input.kind,
                input.endpoint,
                input.region,
                input.bucket,
                input.access_key,
                input.secret_key,
                input.prefix,
                input.public_base_url,
                input.cloudflared_path,
                input.tunnel_token,
                input.hostname,
                input.account_id,
                input.project_name,
                input.api_token,
                input.branch,
                if input.is_default { 1 } else { 0 },
                Utc::now().to_rfc3339(),
                input.tunnel_config
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.list_publish_targets()?
            .into_iter()
            .find(|p| p.id == id)
            .context("created publish target missing")
    }

    pub fn delete_publish_target(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM publish_targets WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Cloudflare 授权（OAuth 或 API Token），所有 Cloudflare 发布目标共用。
    pub fn get_cloudflare_auth(&self) -> Result<crate::cloudflare::CloudflareAuth> {
        let conn = self.lock()?;
        Ok(get_setting(&conn, "cloudflare_auth")?
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn save_cloudflare_auth(&self, auth: &crate::cloudflare::CloudflareAuth) -> Result<()> {
        let conn = self.lock()?;
        set_setting(&conn, "cloudflare_auth", &serde_json::to_string(auth)?)
    }

    /// 用户自己注册的 OAuth client ID（覆盖内置 / 环境变量里的默认值）。
    pub fn get_cloudflare_client_id(&self) -> Result<String> {
        let conn = self.lock()?;
        Ok(get_setting(&conn, "cloudflare_client_id")?.unwrap_or_default())
    }

    pub fn save_cloudflare_client_id(&self, client_id: &str) -> Result<()> {
        let conn = self.lock()?;
        set_setting(&conn, "cloudflare_client_id", client_id.trim())
    }

    /// 微信 JS-SDK 配置（存于 settings 表的 wechat 键，JSON）。
    pub fn get_wechat_config(&self) -> Result<WechatConfig> {
        let conn = self.lock()?;
        Ok(get_setting(&conn, "wechat")?
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    pub fn save_wechat_config(&self, config: &WechatConfig) -> Result<()> {
        let conn = self.lock()?;
        set_setting(&conn, "wechat", &serde_json::to_string(config)?)
    }

    pub fn get_share_settings(&self, project_id: i64) -> Result<Option<ShareSettings>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT title, description FROM project_shares WHERE project_id=?1",
            params![project_id],
            |row| {
                Ok(ShareSettings {
                    title: row.get(0)?,
                    description: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn save_share_settings(&self, project_id: i64, settings: &ShareSettings) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO project_shares(project_id, title, description) VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET title=excluded.title, description=excluded.description",
            params![project_id, settings.title.trim(), settings.description.trim()],
        )?;
        Ok(())
    }

    pub fn list_custom_templates(&self) -> Result<Vec<CustomTemplate>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, prompt, css, dark, created_at, updated_at
             FROM custom_templates ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map([], map_custom_template)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_custom_template(&self, id: i64) -> Result<Option<CustomTemplate>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, prompt, css, dark, created_at, updated_at
             FROM custom_templates WHERE id=?1",
            params![id],
            map_custom_template,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn create_custom_template(
        &self,
        name: &str,
        prompt: &str,
        css: &str,
        dark: bool,
    ) -> Result<CustomTemplate> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO custom_templates (name, prompt, css, dark, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![name, prompt, css, dark as i64, now],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_custom_template(id)?.context("created template missing")
    }

    pub fn update_custom_template(
        &self,
        id: i64,
        name: &str,
        prompt: &str,
        css: &str,
        dark: bool,
    ) -> Result<CustomTemplate> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE custom_templates SET name=?1, prompt=?2, css=?3, dark=?4, updated_at=?5 WHERE id=?6",
            params![name, prompt, css, dark as i64, Utc::now().to_rfc3339(), id],
        )?;
        drop(conn);
        self.get_custom_template(id)?.context("template missing")
    }

    pub fn delete_custom_template(&self, id: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM custom_templates WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn get_publish_target(&self, id: i64) -> Result<Option<PublishTarget>> {
        Ok(self
            .list_publish_targets()?
            .into_iter()
            .find(|p| p.id == id))
    }
}

fn map_trigger_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TriggerRow> {
    Ok(TriggerRow {
        id: row.get(0)?,
        name: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        action_type: row.get(3)?,
        action_config: row.get(4)?,
        public_id: row.get(5)?,
        token: row.get(6)?,
        write_token: row.get(7)?,
        tag_written: row.get::<_, i64>(8)? != 0,
        verified_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn map_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnniversaryProject> {
    let photos_json: String = row.get(11)?;
    let photos = serde_json::from_str(&photos_json).unwrap_or_default();
    Ok(AnniversaryProject {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        event_date: row.get(3)?,
        title: row.get(4)?,
        recipient: row.get(5)?,
        opening: row.get(6)?,
        body: row.get(7)?,
        closing: row.get(8)?,
        template_id: row.get(9)?,
        theme: row.get(10)?,
        photos,
        music: row.get(12)?,
        video: row.get(13)?,
        custom_css: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        photo_focus: row
            .get::<_, String>(17)
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default(),
        slideshow: row.get::<_, i64>(18).unwrap_or(0) != 0,
    })
}

fn map_custom_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<CustomTemplate> {
    Ok(CustomTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        css: row.get(3)?,
        dark: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
