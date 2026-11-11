//! History of IDs this client has connected to (like UltraViewer's "partner ID" box):
//! ID, machine name, OS, and the latest connection. Stored at
//! `~/.config/x-remote/lich_su_id.json` (Windows: `%APPDATA%`).

use serde_json::{json, Value};

/// One machine this client has controlled.
#[derive(Clone, Debug, PartialEq)]
pub struct HistRec {
    /// Numeric ID, no spaces.
    pub id: String,
    /// Machine name (part 15 of the init message); empty if unknown.
    pub name: String,
    /// `windows` / `linux` / `macos` (see `uvsession::frame::os_from_init`); empty if unknown.
    pub os: String,
    /// Unix seconds of the latest connection.
    pub at: u64,
}

/// Keep at most this many machines (same as UltraViewer's dropdown).
const MAX_REC: usize = 30;

fn hist_path() -> Option<std::path::PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA").map(|d| d.join("X-Remote"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config"))).map(|d| d.join("x-remote"))
    }?;
    Some(base.join("lich_su_id.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Parse a JSON list (newest first). Bad JSON → empty.
pub fn parse_json(s: &str) -> Vec<HistRec> {
    let Ok(Value::Array(v)) = serde_json::from_str::<Value>(s) else { return Vec::new() };
    let mut rows: Vec<HistRec> = v
        .iter()
        .filter_map(|m| {
            let id: String = m.get("id")?.as_str()?.chars().filter(|c| c.is_ascii_digit()).collect();
            (!id.is_empty()).then(|| HistRec {
                id,
                name: m.get("ten").and_then(Value::as_str).unwrap_or("").to_string(),
                os: m.get("hdh").and_then(Value::as_str).unwrap_or("").to_string(),
                at: m.get("luc").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    rows.sort_by(|a, b| b.at.cmp(&a.at));
    rows
}

/// Serialize the list to JSON. Keys `ten` / `hdh` / `luc` stay (on-disk format).
pub fn to_json(rows: &[HistRec]) -> String {
    let v: Vec<Value> = rows.iter().map(|m| json!({"id": m.id, "ten": m.name, "hdh": m.os, "luc": m.at})).collect();
    serde_json::to_string_pretty(&Value::Array(v)).unwrap_or_else(|_| "[]".into())
}

/// Saved list (newest first).
pub fn load() -> Vec<HistRec> {
    hist_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| parse_json(&s)).unwrap_or_default()
}

/// Update or insert a machine: move it to the front, keep the old name/OS if this visit has none.
pub fn upsert(rows: &mut Vec<HistRec>, id: &str, name: &str, os: &str, at: u64) {
    let id: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    if id.is_empty() {
        return;
    }
    let prev = rows.iter().position(|m| m.id == id).map(|i| rows.remove(i));
    let keep = |new: &str, prev: Option<&str>| if new.is_empty() { prev.unwrap_or("").to_string() } else { new.to_string() };
    rows.insert(
        0,
        HistRec {
            name: keep(name, prev.as_ref().map(|m| m.name.as_str())),
            os: keep(os, prev.as_ref().map(|m| m.os.as_str())),
            id,
            at,
        },
    );
    rows.truncate(MAX_REC);
}

/// Record one connection in the history file; return the new list.
pub fn store(id: &str, name: &str, os: &str) -> Vec<HistRec> {
    let mut rows = load();
    upsert(&mut rows, id, name, os, now_secs());
    if let Some(p) = hist_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // Write a temp file then rename, so a crash mid-write cannot leave truncated JSON.
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, to_json(&rows)).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
    rows
}

/// Relative time in Vietnamese, like UltraViewer (just now / minutes / hours / days / months).
pub fn ago(at: u64, now: u64) -> String {
    let secs = now.saturating_sub(at);
    match secs {
        0..=59 => "vừa xong".into(),
        60..=3599 => format!("{} phút trước", secs / 60),
        3600..=86_399 => format!("{} giờ trước", secs / 3600),
        86_400..=2_591_999 => format!("{} ngày trước", secs / 86_400),
        _ => format!("{} tháng trước", secs / 2_592_000),
    }
}

/// Short form for a narrow list: drop the trailing suffix from [`ago`].
pub fn short_ago(long: &str) -> String {
    long.strip_suffix(" trước").unwrap_or(long).to_string()
}

/// [`ago`] measured from now.
pub fn ago_now(at: u64) -> String {
    ago(at, now_secs())
}

/// Filter by the digits typed in the ID field (spaces ignored); empty → all.
pub fn match_id<'a>(rows: &'a [HistRec], typed: &str) -> Vec<&'a HistRec> {
    let digits: String = typed.chars().filter(|c| c.is_ascii_digit()).collect();
    rows.iter().filter(|m| m.id.starts_with(&digits)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_moves_front_and_keeps_old() {
        let mut rows = Vec::new();
        upsert(&mut rows, "100 000 002", "HOST-1", "linux", 100);
        upsert(&mut rows, "100000001", "HOST-PC", "windows", 200);
        assert_eq!(rows[0].id, "100000001");
        // Reconnect to the older machine with no name/OS this time: keep the old info, move to front.
        upsert(&mut rows, "100000002", "", "", 300);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], HistRec { id: "100000002".into(), name: "HOST-1".into(), os: "linux".into(), at: 300 });
    }

    #[test]
    fn json_roundtrip_and_bad_is_empty() {
        let mut rows = Vec::new();
        upsert(&mut rows, "100000002", "HOST-1", "linux", 100);
        upsert(&mut rows, "100000001", "HOST-PC", "windows", 200);
        assert_eq!(parse_json(&to_json(&rows)), rows);
        assert!(parse_json("{bad").is_empty());
        assert!(parse_json("[{\"ten\":\"no-id\"}]").is_empty());
    }

    #[test]
    fn time_and_filter() {
        assert_eq!(ago(1000, 1030), "vừa xong");
        assert_eq!(ago(0, 9 * 3600 + 5), "9 giờ trước");
        assert_eq!(ago(0, 3 * 86_400), "3 ngày trước");
        assert_eq!(short_ago("14 giờ trước"), "14 giờ");
        assert_eq!(short_ago("vừa xong"), "vừa xong");
        let mut rows = Vec::new();
        upsert(&mut rows, "100000002", "", "linux", 1);
        upsert(&mut rows, "100000001", "", "windows", 2);
        assert_eq!(match_id(&rows, "100 000 002").len(), 1);
        assert_eq!(match_id(&rows, "").len(), 2);
    }
}
