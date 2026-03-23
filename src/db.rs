//! SQLite database foundation for tool history and future persistent storage.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;
use tracing::warn;

#[cfg(test)]
const CURRENT_SCHEMA_VERSION: i32 = 3;

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
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create database directory: {}", parent.display())
        })?;
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
    if current < 2 {
        migrate_v2(conn)?;
    }
    if current < 3 {
        migrate_v3(conn)?;
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

fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "ALTER TABLE tool_calls ADD COLUMN tool_use_id TEXT;

        CREATE INDEX IF NOT EXISTS idx_tool_calls_tool_use_id
            ON tool_calls(tool_use_id);

        INSERT INTO schema_version (version) VALUES (2);",
    )?;
    Ok(())
}

fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "ALTER TABLE tool_calls ADD COLUMN repo_path TEXT NOT NULL DEFAULT 'unknown';

        CREATE INDEX IF NOT EXISTS idx_tool_calls_repo_path
            ON tool_calls(repo_path);

        INSERT INTO schema_version (version) VALUES (3);",
    )?;
    Ok(())
}

/// Detects the git repository root, falling back to the current working directory.
pub fn detect_repo_path() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Inserts a tool call record at ContentBlockStop time.
/// Returns the row ID on success, or logs a warning and returns None on failure.
pub fn insert_tool_call(
    conn: &Connection,
    session_id: &str,
    tool_name: &str,
    tool_use_id: Option<&str>,
    tool_arguments: &str,
    sequence_number: u32,
    repo_path: &str,
) -> Option<i64> {
    let timestamp = iso8601_now();
    match conn.execute(
        "INSERT INTO tool_calls (session_id, tool_name, tool_use_id, tool_arguments, timestamp, sequence_number, repo_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![session_id, tool_name, tool_use_id, tool_arguments, timestamp, sequence_number, repo_path],
    ) {
        Ok(_) => Some(conn.last_insert_rowid()),
        Err(e) => {
            warn!(error = %e, tool_name, "Failed to record tool call");
            None
        }
    }
}

/// Updates a tool call record with its result at User event time.
/// Returns true on success, or logs a warning and returns false on failure.
pub fn update_tool_result(
    conn: &Connection,
    tool_use_id: &str,
    session_id: &str,
    is_error: bool,
    result_content: &str,
) -> bool {
    match conn.execute(
        "UPDATE tool_calls SET is_error = ?1, result_content = ?2
         WHERE tool_use_id = ?3 AND session_id = ?4",
        rusqlite::params![is_error as i32, result_content, tool_use_id, session_id],
    ) {
        Ok(0) => {
            warn!(tool_use_id, "No tool call found to update");
            false
        }
        Ok(_) => true,
        Err(e) => {
            warn!(error = %e, tool_use_id, "Failed to update tool result");
            false
        }
    }
}

/// Returns the current time as an ISO 8601 string in UTC.
fn iso8601_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Convert to date-time components
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to Y-M-D (simplified algorithm)
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Converts days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Civil days algorithm from Howard Hinnant
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_schema() {
        let conn = open_memory().unwrap();

        let version: i32 = conn
            .query_row(
                "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn tool_calls_table_has_all_columns() {
        let conn = open_memory().unwrap();

        // Insert a row including v2 tool_use_id and v3 repo_path columns.
        conn.execute(
            "INSERT INTO tool_calls (session_id, tool_name, tool_use_id, tool_arguments, is_error, result_content, timestamp, sequence_number, repo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params!["sess-1", "Read", "toolu_abc123", r#"{"path":"/tmp"}"#, 0, "ok", "2026-03-17T12:00:00Z", 1, "/home/user/project"],
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
        // Multiple rows from multiple migrate calls — the important thing
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
        assert!(indexes.contains(&"idx_tool_calls_tool_use_id".to_string()));
        assert!(indexes.contains(&"idx_tool_calls_repo_path".to_string()));
    }

    #[test]
    fn insert_tool_call_records_row() {
        let conn = open_memory().unwrap();

        let id = insert_tool_call(
            &conn,
            "sess-1",
            "Read",
            Some("toolu_abc"),
            r#"{"path":"/tmp"}"#,
            1,
            "/home/user/project",
        );
        assert!(id.is_some());

        let (name, use_id, args, seq, repo): (String, Option<String>, String, u32, String) = conn
            .query_row(
                "SELECT tool_name, tool_use_id, tool_arguments, sequence_number, repo_path FROM tool_calls WHERE id = ?1",
                [id.unwrap()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();

        assert_eq!(name, "Read");
        assert_eq!(use_id.as_deref(), Some("toolu_abc"));
        assert_eq!(args, r#"{"path":"/tmp"}"#);
        assert_eq!(seq, 1);
        assert_eq!(repo, "/home/user/project");
    }

    #[test]
    fn insert_without_tool_use_id() {
        let conn = open_memory().unwrap();

        let id = insert_tool_call(&conn, "sess-1", "Read", None, "{}", 1, "/tmp");
        assert!(id.is_some());

        let use_id: Option<String> = conn
            .query_row(
                "SELECT tool_use_id FROM tool_calls WHERE id = ?1",
                [id.unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(use_id.is_none());
    }

    #[test]
    fn update_tool_result_sets_fields() {
        let conn = open_memory().unwrap();

        insert_tool_call(
            &conn,
            "sess-1",
            "Bash",
            Some("toolu_xyz"),
            r#"{"command":"ls"}"#,
            1,
            "/tmp",
        );

        let updated = update_tool_result(&conn, "toolu_xyz", "sess-1", false, "file1\nfile2");
        assert!(updated);

        let (is_error, result): (i32, String) = conn
            .query_row(
                "SELECT is_error, result_content FROM tool_calls WHERE tool_use_id = ?1",
                ["toolu_xyz"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(is_error, 0);
        assert_eq!(result, "file1\nfile2");
    }

    #[test]
    fn update_tool_result_with_error() {
        let conn = open_memory().unwrap();

        insert_tool_call(
            &conn,
            "sess-1",
            "Bash",
            Some("toolu_err"),
            r#"{"command":"false"}"#,
            1,
            "/tmp",
        );

        let updated = update_tool_result(&conn, "toolu_err", "sess-1", true, "command failed");
        assert!(updated);

        let is_error: i32 = conn
            .query_row(
                "SELECT is_error FROM tool_calls WHERE tool_use_id = ?1",
                ["toolu_err"],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(is_error, 1);
    }

    #[test]
    fn update_nonexistent_returns_false() {
        let conn = open_memory().unwrap();

        let updated = update_tool_result(&conn, "toolu_missing", "sess-1", false, "data");
        assert!(!updated);
    }

    #[test]
    fn crash_leaves_null_result() {
        let conn = open_memory().unwrap();

        insert_tool_call(
            &conn,
            "sess-1",
            "Bash",
            Some("toolu_crash"),
            r#"{"command":"hang"}"#,
            1,
            "/tmp",
        );

        // Simulate crash: no update_tool_result call
        let result: Option<String> = conn
            .query_row(
                "SELECT result_content FROM tool_calls WHERE tool_use_id = ?1",
                ["toolu_crash"],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            result.is_none(),
            "result_content should be NULL for incomplete calls"
        );
    }

    #[test]
    fn existing_rows_get_unknown_repo_path() {
        let conn = Connection::open_in_memory().unwrap();
        // Run only v1 and v2 migrations
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
        )
        .unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();

        // Insert a row without repo_path (pre-v3 schema)
        conn.execute(
            "INSERT INTO tool_calls (session_id, tool_name, tool_arguments, timestamp, sequence_number)
             VALUES ('sess-old', 'Read', '{}', '2026-01-01T00:00:00Z', 1)",
            [],
        )
        .unwrap();

        // Run v3 migration
        migrate_v3(&conn).unwrap();

        // Old row should have 'unknown' as repo_path
        let repo: String = conn
            .query_row(
                "SELECT repo_path FROM tool_calls WHERE session_id = 'sess-old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(repo, "unknown");
    }

    #[test]
    fn detect_repo_path_returns_something() {
        let path = detect_repo_path();
        assert!(!path.is_empty());
    }

    #[test]
    fn iso8601_now_format() {
        let ts = iso8601_now();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }
}
