use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupRecord {
    pub id: i64,
    pub target_path: String,
    pub target_name: String,
    pub category: String,
    pub bytes_reclaimed: u64,
    pub delete_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStats {
    pub total_bytes_reclaimed: u64,
    pub total_cleanups: u64,
    pub recent: Vec<CleanupRecord>,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open SQLite DB at {}: {}", path.display(), e))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                CREATE TABLE IF NOT EXISTS cleanup_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    target_path TEXT NOT NULL,
                    target_name TEXT NOT NULL,
                    category TEXT NOT NULL,
                    bytes_reclaimed INTEGER NOT NULL,
                    delete_mode TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_cleanup_created_at ON cleanup_history(created_at DESC);
                ",
            )
            .map_err(|e| format!("Failed to run DB migrations: {}", e))
    }

    pub fn record_cleanup(
        &self,
        target_path: &str,
        target_name: &str,
        category: &str,
        bytes_reclaimed: u64,
        delete_mode: &str,
    ) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO cleanup_history (target_path, target_name, category, bytes_reclaimed, delete_mode, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    target_path,
                    target_name,
                    category,
                    bytes_reclaimed as i64,
                    delete_mode,
                    now
                ],
            )
            .map_err(|e| format!("Failed to record cleanup: {}", e))?;
        Ok(())
    }

    pub fn get_cleanup_stats(&self, limit: usize) -> Result<CleanupStats, String> {
        let (total_bytes, total_count): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(bytes_reclaimed), 0), COUNT(*) FROM cleanup_history",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Failed to query cleanup totals: {}", e))?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, target_path, target_name, category, bytes_reclaimed, delete_mode, created_at
                 FROM cleanup_history
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(|e| format!("Failed to prepare cleanup history query: {}", e))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(CleanupRecord {
                    id: row.get(0)?,
                    target_path: row.get(1)?,
                    target_name: row.get(2)?,
                    category: row.get(3)?,
                    bytes_reclaimed: row.get::<_, i64>(4)?.max(0) as u64,
                    delete_mode: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to fetch cleanup history: {}", e))?;

        let mut recent = Vec::new();
        for item in rows {
            recent.push(item.map_err(|e| format!("Failed to read row: {}", e))?);
        }

        Ok(CleanupStats {
            total_bytes_reclaimed: total_bytes.max(0) as u64,
            total_cleanups: total_count.max(0) as u64,
            recent,
        })
    }
}
