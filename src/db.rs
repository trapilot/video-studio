use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::models::*;
use crate::helper::*;

#[derive(Debug, Clone)]
pub struct DbManager {
    conn: Arc<Mutex<Connection>>
}

/// Schema version of the serialized `VideoState`. Rows saved with another
/// version are ignored on resume. Bump it on incompatible state changes.
pub const APP_VERSION: u8 = 1;

impl DbManager {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("open database {}", db_path))?;

        conn.execute_batch(
            "
            -- Published topics per profile, used by the Planner to avoid repeats.
            CREATE TABLE IF NOT EXISTS topic_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                profile TEXT NOT NULL,
                topic TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_topic_history_profile ON topic_history(profile, id);

            -- One row per session: the serialized VideoState plus retry bookkeeping.
            CREATE TABLE IF NOT EXISTS workflow_state (
                session_id TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                max_retry INTEGER NOT NULL DEFAULT 3,
                backoff_ms INTEGER NOT NULL DEFAULT 1000,
                last_error TEXT,
                updated_at TEXT NOT NULL
            );
            ",
        ).context("create tables")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn conn(&self) -> tokio::sync::MutexGuard<'_, Connection> {
        self.conn.lock().await
    }

    pub async fn save_topic(&self, profile: &str, topic: &str) -> Result<()> {
        let now = now_ts();
        let conn = self.conn().await;

        conn.execute(
            "INSERT INTO topic_history (profile, topic, created_at) VALUES (?1, ?2, ?3)",
            params![profile, topic, now],
        )?;
        Ok(())
    }

    pub async fn save_state(&self, state: &VideoState) -> Result<()> {
        let state_json = serde_json::to_string(state).context("serialize state")?;
        let conn = self.conn().await;

        conn.execute(
            "INSERT INTO workflow_state (
                session_id,
                state_json,
                version,
                status,
                retry_count,
                max_retry,
                backoff_ms,
                last_error,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(session_id) DO UPDATE SET
                state_json=excluded.state_json,
                version=excluded.version,
                status=excluded.status,
                retry_count=excluded.retry_count,
                max_retry=excluded.max_retry,
                backoff_ms=excluded.backoff_ms,
                last_error=excluded.last_error,
                updated_at=excluded.updated_at",
            params![
                state.session_id,
                state_json,
                APP_VERSION,
                state.meta.status,
                state.meta.retry_count,
                state.meta.max_retry,
                state.meta.backoff_ms,
                state.meta.last_error,
                now_rfc()
            ],
        ).context("save state")?;

        Ok(())
    }

    pub async fn delete_state(&self, session_id: &str) -> Result<()> {
        let conn = self.conn().await;

        conn.execute(
            "DELETE FROM workflow_state WHERE session_id = ?1",
            params![session_id],
        )?;

        Ok(())
    }

    pub async fn get_recent_topics(&self, profile: &str, limit: u8) -> Result<Vec<String>> {
        let conn = self.conn().await;

        let mut stmt = conn.prepare(
            "SELECT topic FROM topic_history WHERE profile = ?1 ORDER BY id DESC LIMIT ?2"
        )?;

        let topics = stmt
            .query_map(params![profile, limit], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(topics)
    }

    /// The most recently updated unfinished session of the current `APP_VERSION`, if any.
    pub async fn get_recent_state(&self) -> Result<Option<VideoState>> {
        let conn = self.conn().await;

        let state_json: Option<String> = conn.query_row(
            "SELECT state_json
             FROM workflow_state
             WHERE version = ?1 AND status NOT IN ('done', 'cancelled')
             ORDER BY updated_at DESC
             LIMIT 1",
            [APP_VERSION],
            |row| row.get(0),
        ).optional()?;

        match state_json {
            None => Ok(None),
            Some(json) => match serde_json::from_str::<VideoState>(&json) {
                Ok(state) => Ok(Some(state)),
                Err(e) => {
                    eprintln!("⚠️  Ignoring unreadable saved session ({}). Bump APP_VERSION after changing VideoState.", e);
                    Ok(None)
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (DbManager, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("video_studio_test_{}.db", uuid::Uuid::new_v4()));
        let db = DbManager::new(path.to_str().unwrap()).unwrap();
        (db, path)
    }

    #[tokio::test]
    async fn topics_are_scoped_per_profile() {
        let (db, path) = temp_db();
        db.save_topic("kids_edu", "Why stars twinkle").await.unwrap();
        db.save_topic("kids_edu", "Why is the sky blue").await.unwrap();
        db.save_topic("explainer", "How do batteries work").await.unwrap();

        let kids = db.get_recent_topics("kids_edu", 20).await.unwrap();
        let explainer = db.get_recent_topics("explainer", 20).await.unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(kids, vec!["Why is the sky blue", "Why stars twinkle"]);
        assert_eq!(explainer, vec!["How do batteries work"]);
    }

    #[tokio::test]
    async fn resumes_latest_unfinished_session_only() {
        let (db, path) = temp_db();
        let profile = crate::profiles::profile_by_id("explainer").unwrap();

        let mut unfinished = VideoState::new(profile);
        db.save_state(unfinished.reset()).await.unwrap();
        // A finished session saved later must not hide the unfinished one.
        let mut finished = VideoState::new(profile);
        db.save_state(finished.done()).await.unwrap();

        let resumed = db.get_recent_state().await.unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(resumed.map(|s| s.session_id), Some(unfinished.session_id));
    }
}
