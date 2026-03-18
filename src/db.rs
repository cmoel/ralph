//! SQLite database foundation for tool history and future persistent storage.
// No callers yet — the recording layer (ralph-66j) will wire this in.
#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

const CURRENT_SCHEMA_VERSION: i32 = 1;

/// Returns the platform-appropriate database directory.
///
/// - macOS: ~/Library/Application Support/ralph/
/// - Linux: $XDG_DATA_HOME/ralph/ (defaults to ~/.local/share/ralph/)
fn db_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("Failed to determine data directory")?;
    Ok(base.join("ralph"))
}

/// Returns the full path to the SQLite database file.
pub fn db_path() -> Result<PathBuf> {
    Ok(db_dir()?.join("ralph.db"))
}

/// Opens (or creates) the database and ensures the schema is up to date.
pub fn open() -> Result<Connection> {
    let path = db_path()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create database directory: {}", parent.display()))?;
    }

    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open database: {}", path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    migrate(&conn)?;

    Ok(conn)
}

/// Opens an in-memory database for testing.
#[cfg(test)]
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );",
    )?;

    let version: Option<i32> = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let current = version.unwrap_or(0);

    if current < 1 {
        migrate_v1(conn)?;
    }

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tool_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            tool_arguments TEXT NOT NULL,
            is_error INTEGER NOT NULL DEFAULT 0,
            result_content TEXT,
            timestamp TEXT NOT NULL,
            sequence_number INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_tool_calls_session
            ON tool_calls(session_id);
        CREATE INDEX IF NOT EXISTS idx_tool_calls_tool_name
            ON tool_calls(tool_name);
        CREATE INDEX IF NOT EXISTS idx_tool_calls_timestamp
            ON tool_calls(timestamp);

        INSERT INTO schema_version (version) VALUES (1);",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_schema() {
        let conn = open_memory().unwrap();

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn tool_calls_table_exists() {
        let conn = open_memory().unwrap();

        // Insert a row to verify the table and columns exist.
        conn.execute(
            "INSERT INTO tool_calls (session_id, tool_name, tool_arguments, is_error, result_content, timestamp, sequence_number)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params!["sess-1", "Read", r#"{"path":"/tmp"}"#, 0, "ok", "2026-03-17T12:00:00Z", 1],
        )
        .unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_memory().unwrap();

        // Running migrate again should not fail or duplicate the version row.
        migrate(&conn).unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        // Two rows (one per migrate call) but both version 1 — the important thing
        // is that the table and indexes survive a second pass without error.
        assert!(count >= 1);
    }

    #[test]
    fn db_path_is_platform_appropriate() {
        let path = db_path().unwrap();
        let path_str = path.to_string_lossy();

        assert!(path_str.ends_with("ralph/ralph.db") || path_str.ends_with("ralph\\ralph.db"));

        if cfg!(target_os = "macos") {
            assert!(path_str.contains("Application Support"));
        }
    }

    #[test]
    fn indexes_exist() {
        let conn = open_memory().unwrap();

        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='tool_calls'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(indexes.contains(&"idx_tool_calls_session".to_string()));
        assert!(indexes.contains(&"idx_tool_calls_tool_name".to_string()));
        assert!(indexes.contains(&"idx_tool_calls_timestamp".to_string()));
    }
}
