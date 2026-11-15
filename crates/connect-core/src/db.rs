use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::models::{RunView, Settings, TriggerRow, TriggerView};

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
            "#,
        )?;

        // 兼容 M1/M2 早期数据库。
        let _ = conn.execute("ALTER TABLE runs ADD COLUMN external_run_id TEXT", []);

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
