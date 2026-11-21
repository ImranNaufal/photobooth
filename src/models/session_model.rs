use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

pub const DEFAULT_DB_PATH: &str = "booth_sessions.db";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoSession {
    pub id: String,
    pub created_at: String,
    pub template_id: String,
    pub composite_path: String,
    pub share_url: String,
    pub upload_status: String, // "PENDING", "UPLOADED", "FAILED"
    pub printed_copies: u32,
}

pub struct SessionModel {
    conn: Arc<Mutex<Connection>>,
}

impl SessionModel {
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let model = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        model.init_db()?;
        Ok(model)
    }

    pub fn init_db(&self) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                template_id TEXT NOT NULL,
                composite_path TEXT NOT NULL,
                share_url TEXT NOT NULL,
                upload_status TEXT NOT NULL,
                printed_copies INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;
        info!("SQLite database initialized successfully.");
        Ok(())
    }

    pub fn create_session(&self, session: &PhotoSession) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        conn.execute(
            "INSERT INTO sessions (id, created_at, template_id, composite_path, share_url, upload_status, printed_copies)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id,
                session.created_at,
                session.template_id,
                session.composite_path,
                session.share_url,
                session.upload_status,
                session.printed_copies
            ],
        )?;
        info!("Session created in DB: {}", session.id);
        Ok(())
    }

    pub fn increment_printed_copies(&self, id: &str) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        conn.execute(
            "UPDATE sessions SET printed_copies = printed_copies + 1 WHERE id = ?1",
            params![id],
        )?;
        info!("Incremented printed copies for session: {}", id);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn update_upload_status(&self, id: &str, status: &str) -> anyhow::Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        conn.execute(
            "UPDATE sessions SET upload_status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        info!("Updated upload status for session {}: {}", id, status);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_session(&self, id: &str) -> anyhow::Result<Option<PhotoSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, template_id, composite_path, share_url, upload_status, printed_copies
             FROM sessions WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(PhotoSession {
                id: row.get(0)?,
                created_at: row.get(1)?,
                template_id: row.get(2)?,
                composite_path: row.get(3)?,
                share_url: row.get(4)?,
                upload_status: row.get(5)?,
                printed_copies: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    #[allow(dead_code)]
    pub fn list_pending_uploads(&self) -> anyhow::Result<Vec<PhotoSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Mutex poison error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, created_at, template_id, composite_path, share_url, upload_status, printed_copies
             FROM sessions WHERE upload_status = 'PENDING'",
        )?;

        let session_iter = stmt.query_map([], |row| {
            Ok(PhotoSession {
                id: row.get(0)?,
                created_at: row.get(1)?,
                template_id: row.get(2)?,
                composite_path: row.get(3)?,
                share_url: row.get(4)?,
                upload_status: row.get(5)?,
                printed_copies: row.get(6)?,
            })
        })?;

        let mut results = Vec::new();
        for s in session_iter {
            results.push(s?);
        }
        Ok(results)
    }
}
