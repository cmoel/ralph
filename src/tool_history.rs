//! CLI subcommand for querying tool call history from the SQLite database.

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

/// A tool call record from the database.
#[derive(Debug)]
pub struct ToolCallRecord {
    pub id: i64,
    pub session_id: String,
    pub tool_name: String,
    pub tool_arguments: String,
    pub is_error: bool,
    pub result_content: Option<String>,
    pub timestamp: String,
    pub sequence_number: u32,
}

/// Query filter for tool history.
pub enum QueryFilter {
    /// Show all tool calls from a specific session.
    Session(String),
    /// Show all calls to a specific tool (case-insensitive).
    Tool(String),
    /// Show tool calls in a time range.
    TimeRange {
        since: String,
        until: Option<String>,
    },
    /// Show most recent session's tool calls (default).
    LatestSession,
}

/// Parse a duration string like "6h", "1d", "2w" into seconds.
fn parse_duration_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str.parse().ok()?;

    match unit {
        "s" => Some(num),
        "m" => Some(num * 60),
        "h" => Some(num * 3600),
        "d" => Some(num * 86400),
        "w" => Some(num * 86400 * 7),
        _ => None,
    }
}

/// Convert a SystemTime to ISO 8601 UTC string.
fn system_time_to_iso8601(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Converts days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
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

/// Parse a time specification into an ISO 8601 timestamp.
///
/// Accepted formats:
/// - Relative: `6h`, `1d`, `2w` (duration ago from now)
/// - Named: `today`, `yesterday`
/// - Absolute: `2025-01-15`, `2025-01-15T10:30:00`
pub fn parse_time_spec(spec: &str) -> Result<String> {
    use std::time::{Duration, SystemTime};

    let spec = spec.trim();

    // Named times
    match spec {
        "today" => {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let day_start = (now / 86400) * 86400;
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(day_start);
            return Ok(system_time_to_iso8601(time));
        }
        "yesterday" => {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let day_start = (now / 86400) * 86400 - 86400;
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(day_start);
            return Ok(system_time_to_iso8601(time));
        }
        _ => {}
    }

    // Relative duration (e.g., "6h", "1d", "2w")
    if let Some(secs) = parse_duration_secs(spec) {
        let now = SystemTime::now();
        let time = now - Duration::from_secs(secs);
        return Ok(system_time_to_iso8601(time));
    }

    // Absolute date: YYYY-MM-DD
    if spec.len() == 10 && spec.chars().nth(4) == Some('-') && spec.chars().nth(7) == Some('-') {
        // Validate it parses as numbers
        let parts: Vec<&str> = spec.split('-').collect();
        if parts.len() == 3
            && parts[0].parse::<u32>().is_ok()
            && parts[1].parse::<u32>().is_ok()
            && parts[2].parse::<u32>().is_ok()
        {
            return Ok(format!("{spec}T00:00:00Z"));
        }
    }

    // Absolute datetime: YYYY-MM-DDTHH:MM:SS
    if spec.len() == 19
        && spec.chars().nth(10) == Some('T')
        && spec.chars().nth(4) == Some('-')
        && spec.chars().nth(13) == Some(':')
    {
        return Ok(format!("{spec}Z"));
    }

    bail!(
        "Unrecognized time format: '{}'\n\nAccepted formats:\n  \
         Relative: 6h, 1d, 2w (duration ago from now)\n  \
         Named:    today, yesterday\n  \
         Absolute: 2025-01-15, 2025-01-15T10:30:00",
        spec
    );
}

/// Query tool calls from the database based on a filter.
pub fn query_tool_calls(
    conn: &Connection,
    filter: &QueryFilter,
    rejected_only: bool,
) -> Result<Vec<ToolCallRecord>> {
    let (sql, params) = build_query(filter, rejected_only);

    let mut stmt = conn.prepare(&sql).context("Failed to prepare query")?;

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params
        .iter()
        .map(|p| p as &dyn rusqlite::types::ToSql)
        .collect();

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(ToolCallRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                tool_name: row.get(2)?,
                tool_arguments: row.get(3)?,
                is_error: row.get::<_, i32>(4)? != 0,
                result_content: row.get(5)?,
                timestamp: row.get(6)?,
                sequence_number: row.get(7)?,
            })
        })
        .context("Failed to execute query")?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.context("Failed to read row")?);
    }

    Ok(records)
}

/// Build the SQL query and parameters for a given filter.
fn build_query(filter: &QueryFilter, rejected_only: bool) -> (String, Vec<String>) {
    let mut conditions = Vec::new();
    let mut params = Vec::new();

    match filter {
        QueryFilter::Session(session_id) => {
            conditions.push(format!("session_id = ?{}", params.len() + 1));
            params.push(session_id.clone());
        }
        QueryFilter::Tool(tool_name) => {
            conditions.push(format!("tool_name = ?{} COLLATE NOCASE", params.len() + 1));
            params.push(tool_name.clone());
        }
        QueryFilter::TimeRange { since, until } => {
            conditions.push(format!("timestamp >= ?{}", params.len() + 1));
            params.push(since.clone());
            if let Some(until) = until {
                conditions.push(format!("timestamp <= ?{}", params.len() + 1));
                params.push(until.clone());
            }
        }
        QueryFilter::LatestSession => {
            conditions.push(
                "session_id = (SELECT session_id FROM tool_calls ORDER BY timestamp DESC LIMIT 1)"
                    .to_string(),
            );
        }
    }

    if rejected_only {
        conditions.push("is_error = 1".to_string());
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, session_id, tool_name, tool_arguments, is_error, result_content, timestamp, sequence_number \
         FROM tool_calls{} ORDER BY timestamp ASC, sequence_number ASC",
        where_clause
    );

    (sql, params)
}

/// Format tool call records as a human-readable table.
pub fn format_table(records: &[ToolCallRecord]) -> String {
    if records.is_empty() {
        return "No tool calls found.".to_string();
    }

    let mut lines = Vec::new();

    // Header
    lines.push(format!(
        "{:<4} {:<20} {:<15} {:<8} {:<10}",
        "#", "TIMESTAMP", "TOOL", "STATUS", "SESSION"
    ));
    lines.push("─".repeat(60));

    for record in records {
        let status = if record.is_error { "error" } else { "ok" };
        // Truncate session ID to last 8 chars for display
        let session_short = if record.session_id.len() > 8 {
            &record.session_id[record.session_id.len() - 8..]
        } else {
            &record.session_id
        };

        lines.push(format!(
            "{:<4} {:<20} {:<15} {:<8} {:<10}",
            record.sequence_number, record.timestamp, record.tool_name, status, session_short,
        ));
    }

    lines.push(String::new());
    lines.push(format!("{} tool call(s)", records.len()));

    lines.join("\n")
}

/// Format tool call records as JSON.
pub fn format_json(records: &[ToolCallRecord]) -> Result<String> {
    let json_records: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "session_id": r.session_id,
                "tool_name": r.tool_name,
                "tool_arguments": r.tool_arguments,
                "is_error": r.is_error,
                "result_content": r.result_content,
                "timestamp": r.timestamp,
                "sequence_number": r.sequence_number,
            })
        })
        .collect();

    serde_json::to_string_pretty(&json_records).context("Failed to serialize to JSON")
}

/// Run the tool-history subcommand.
pub fn run(
    session: Option<String>,
    tool: Option<String>,
    since: Option<String>,
    until: Option<String>,
    rejected: bool,
    json: bool,
    show_db_path: bool,
) -> Result<()> {
    use crate::db;

    if show_db_path {
        println!("{}", db::db_path()?.display());
        return Ok(());
    }

    let filter = if let Some(session_id) = session {
        QueryFilter::Session(session_id)
    } else if let Some(tool_name) = tool {
        QueryFilter::Tool(tool_name)
    } else if let Some(since_spec) = since {
        let since_ts = parse_time_spec(&since_spec)?;
        let until_ts = until.map(|u| parse_time_spec(&u)).transpose()?;
        QueryFilter::TimeRange {
            since: since_ts,
            until: until_ts,
        }
    } else {
        QueryFilter::LatestSession
    };

    let conn = db::open().context("Failed to open tool history database")?;
    let records = query_tool_calls(&conn, &filter, rejected)?;

    if json {
        println!("{}", format_json(&records)?);
    } else {
        println!("{}", format_table(&records));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup_test_db() -> Connection {
        let conn = db::open_memory().unwrap();

        // Insert test data across two sessions
        db::insert_tool_call(
            &conn,
            "sess-aaa111",
            "Read",
            Some("tu_1"),
            r#"{"path":"/tmp"}"#,
            1,
        );
        db::insert_tool_call(
            &conn,
            "sess-aaa111",
            "Bash",
            Some("tu_2"),
            r#"{"command":"ls"}"#,
            2,
        );
        db::insert_tool_call(
            &conn,
            "sess-bbb222",
            "Write",
            Some("tu_3"),
            r#"{"path":"/out"}"#,
            1,
        );
        db::insert_tool_call(
            &conn,
            "sess-bbb222",
            "Bash",
            Some("tu_4"),
            r#"{"command":"rm"}"#,
            2,
        );

        // Mark one as an error
        db::update_tool_result(&conn, "tu_4", "sess-bbb222", true, "permission denied");
        // Mark one as success
        db::update_tool_result(&conn, "tu_1", "sess-aaa111", false, "file contents");

        conn
    }

    #[test]
    fn query_by_session() {
        let conn = setup_test_db();
        let records =
            query_tool_calls(&conn, &QueryFilter::Session("sess-aaa111".into()), false).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.session_id == "sess-aaa111"));
    }

    #[test]
    fn query_by_tool_case_insensitive() {
        let conn = setup_test_db();
        let records = query_tool_calls(&conn, &QueryFilter::Tool("bash".into()), false).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.tool_name == "Bash"));
    }

    #[test]
    fn query_rejected_only() {
        let conn = setup_test_db();
        let records = query_tool_calls(&conn, &QueryFilter::Tool("Bash".into()), true).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].is_error);
        assert_eq!(records[0].session_id, "sess-bbb222");
    }

    #[test]
    fn query_latest_session() {
        let conn = setup_test_db();
        let records = query_tool_calls(&conn, &QueryFilter::LatestSession, false).unwrap();
        // Latest session should be the last one inserted
        assert!(!records.is_empty());
        let session = &records[0].session_id;
        assert!(records.iter().all(|r| r.session_id == *session));
    }

    #[test]
    fn query_time_range() {
        let conn = setup_test_db();
        // All test records have timestamps near "now", so querying since epoch should return all
        let records = query_tool_calls(
            &conn,
            &QueryFilter::TimeRange {
                since: "2000-01-01T00:00:00Z".into(),
                until: None,
            },
            false,
        )
        .unwrap();
        assert_eq!(records.len(), 4);
    }

    #[test]
    fn query_time_range_with_until() {
        let conn = setup_test_db();
        // Until year 2000 should return nothing
        let records = query_tool_calls(
            &conn,
            &QueryFilter::TimeRange {
                since: "2000-01-01T00:00:00Z".into(),
                until: Some("2000-12-31T23:59:59Z".into()),
            },
            false,
        )
        .unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn query_empty_db() {
        let conn = db::open_memory().unwrap();
        let records = query_tool_calls(&conn, &QueryFilter::LatestSession, false).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn format_table_empty() {
        let output = format_table(&[]);
        assert_eq!(output, "No tool calls found.");
    }

    #[test]
    fn format_table_with_records() {
        let conn = setup_test_db();
        let records =
            query_tool_calls(&conn, &QueryFilter::Session("sess-aaa111".into()), false).unwrap();
        let output = format_table(&records);
        assert!(output.contains("TOOL"));
        assert!(output.contains("Read"));
        assert!(output.contains("Bash"));
        assert!(output.contains("2 tool call(s)"));
    }

    #[test]
    fn format_json_output() {
        let conn = setup_test_db();
        let records =
            query_tool_calls(&conn, &QueryFilter::Session("sess-aaa111".into()), false).unwrap();
        let output = format_json(&records).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["tool_name"], "Read");
        assert_eq!(parsed[0]["session_id"], "sess-aaa111");
    }

    #[test]
    fn parse_duration_hours() {
        assert_eq!(parse_duration_secs("6h"), Some(6 * 3600));
    }

    #[test]
    fn parse_duration_days() {
        assert_eq!(parse_duration_secs("1d"), Some(86400));
    }

    #[test]
    fn parse_duration_weeks() {
        assert_eq!(parse_duration_secs("2w"), Some(2 * 7 * 86400));
    }

    #[test]
    fn parse_duration_invalid() {
        assert_eq!(parse_duration_secs("abc"), None);
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_duration_secs("6x"), None);
    }

    #[test]
    fn parse_time_spec_absolute_date() {
        let result = parse_time_spec("2025-01-15").unwrap();
        assert_eq!(result, "2025-01-15T00:00:00Z");
    }

    #[test]
    fn parse_time_spec_absolute_datetime() {
        let result = parse_time_spec("2025-01-15T10:30:00").unwrap();
        assert_eq!(result, "2025-01-15T10:30:00Z");
    }

    #[test]
    fn parse_time_spec_relative() {
        // Just verify it doesn't error — the exact value depends on current time
        let result = parse_time_spec("6h");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with('Z'));
    }

    #[test]
    fn parse_time_spec_today() {
        let result = parse_time_spec("today").unwrap();
        assert!(result.ends_with("T00:00:00Z"));
    }

    #[test]
    fn parse_time_spec_yesterday() {
        let result = parse_time_spec("yesterday").unwrap();
        assert!(result.ends_with("T00:00:00Z"));
    }

    #[test]
    fn parse_time_spec_invalid() {
        let result = parse_time_spec("not-a-time");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unrecognized time format"));
        assert!(err.contains("Accepted formats"));
    }
}
