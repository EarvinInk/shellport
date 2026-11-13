//! Activity & history: a local, capped record of what happened on each host
//! and database: queries run, commands run on many hosts, file transfers.
//! It never stores secrets (vault values never pass through here) and stays
//! on this machine; it goes into a backup only if the user ticks it.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Entries kept; the oldest are dropped first.
const MAX_ENTRIES: usize = 3000;
/// Longest text kept per entry (SQL, command output…).
const MAX_TEXT: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Query,
    MultiRun,
    Transfer,
    /// Compress or extract on a server.
    FileAction,
}

/// One host's part of a multi-host run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostResult {
    pub host_id: String,
    pub host_name: String,
    pub ok: bool,
    pub exit_code: Option<u32>,
    pub output: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    /// Unix milliseconds.
    pub at: u64,
    pub kind: ActivityKind,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub db_id: Option<String>,
    /// Short label (host or database name, file name…).
    pub title: String,
    /// The SQL, the command, or the file path.
    pub detail: String,
    pub ok: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Rows returned or affected, files moved…
    #[serde(default)]
    pub count: Option<u64>,
    #[serde(default)]
    pub results: Vec<HostResult>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn clip(s: &str) -> String {
    if s.len() <= MAX_TEXT {
        return s.to_string();
    }
    let mut end = MAX_TEXT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [{} more bytes]", &s[..end], s.len() - end)
}

impl ActivityEntry {
    pub fn new(kind: ActivityKind, title: impl Into<String>, detail: &str, ok: bool) -> Self {
        ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            at: now_ms(),
            kind,
            host_id: None,
            db_id: None,
            title: title.into(),
            detail: clip(detail),
            ok,
            message: None,
            duration_ms: None,
            count: None,
            results: vec![],
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
pub struct ActivityFile {
    pub entries: Vec<ActivityEntry>,
}

pub struct ActivityLog {
    path: PathBuf,
    entries: Mutex<Vec<ActivityEntry>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFilter {
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub db_id: Option<String>,
    #[serde(default)]
    pub kind: Option<ActivityKind>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl ActivityLog {
    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<ActivityFile>(&b).ok())
            .map(|f| f.entries)
            .unwrap_or_default();
        ActivityLog {
            path,
            entries: Mutex::new(entries),
        }
    }

    fn save(&self, entries: &[ActivityEntry]) {
        let f = ActivityFile {
            entries: entries.to_vec(),
        };
        match serde_json::to_vec(&f) {
            Ok(b) => {
                if let Err(e) = crate::paths::write_private(&self.path, &b) {
                    log::warn!("saving activity: {e:#}");
                }
            }
            Err(e) => log::warn!("saving activity: {e}"),
        }
    }

    pub fn add(&self, e: ActivityEntry) {
        let mut v = self.entries.lock();
        v.push(e);
        let over = v.len().saturating_sub(MAX_ENTRIES);
        if over > 0 {
            v.drain(..over);
        }
        self.save(&v);
    }

    /// Newest first.
    pub fn list(&self, f: &ActivityFilter) -> Vec<ActivityEntry> {
        let q = f
            .search
            .as_deref()
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty());
        self.entries
            .lock()
            .iter()
            .rev()
            .filter(|e| {
                f.host_id.as_ref().is_none_or(|h| {
                    e.host_id.as_ref() == Some(h) || e.results.iter().any(|r| &r.host_id == h)
                })
            })
            .filter(|e| f.db_id.as_ref().is_none_or(|d| e.db_id.as_ref() == Some(d)))
            .filter(|e| f.kind.as_ref().is_none_or(|k| &e.kind == k))
            .filter(|e| {
                q.as_ref().is_none_or(|q| {
                    e.detail.to_lowercase().contains(q) || e.title.to_lowercase().contains(q)
                })
            })
            .take(f.limit.unwrap_or(500))
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<ActivityEntry> {
        self.entries.lock().clone()
    }

    pub fn delete(&self, id: &str) {
        let mut v = self.entries.lock();
        v.retain(|e| e.id != id);
        self.save(&v);
    }

    /// Clear everything, or only what concerns one host or database.
    pub fn clear(&self, host_id: Option<&str>, db_id: Option<&str>) {
        let mut v = self.entries.lock();
        match (host_id, db_id) {
            (None, None) => v.clear(),
            (h, d) => v.retain(|e| {
                let host_match = h.is_some() && e.host_id.as_deref() == h;
                let db_match = d.is_some() && e.db_id.as_deref() == d;
                !(host_match || db_match)
            }),
        }
        self.save(&v);
    }

    /// Add entries from a backup that aren't here yet.
    pub fn merge(&self, other: Vec<ActivityEntry>) -> usize {
        let mut v = self.entries.lock();
        let mut n = 0;
        for e in other {
            if !v.iter().any(|x| x.id == e.id) {
                v.push(e);
                n += 1;
            }
        }
        v.sort_by_key(|e| e.at);
        let over = v.len().saturating_sub(MAX_ENTRIES);
        if over > 0 {
            v.drain(..over);
        }
        self.save(&v);
        n
    }
}

/// Plain text of terminal output: escape sequences removed, for log files.
#[derive(Default)]
pub struct AnsiStripper {
    state: u8,
}

impl AnsiStripper {
    /// Feed raw bytes; returns printable text (keeps \n and \t, drops \r).
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            match self.state {
                0 => match b {
                    0x1b => self.state = 1,
                    b'\n' | b'\t' => out.push(b),
                    0..=0x1f | 0x7f => {}
                    _ => out.push(b),
                },
                // After ESC: CSI, OSC/DCS/… strings, or a single-char sequence.
                1 => {
                    self.state = match b {
                        b'[' => 2,
                        b']' | b'P' | b'X' | b'^' | b'_' => 3,
                        b'(' | b')' | b'*' | b'+' | b'#' | b'%' => 5,
                        _ => 0,
                    }
                }
                // CSI: parameters until a final byte 0x40..=0x7e.
                2 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.state = 0
                    }
                }
                // String until BEL or ESC \.
                3 => match b {
                    0x07 => self.state = 0,
                    0x1b => self.state = 4,
                    _ => {}
                },
                4 => self.state = if b == b'\\' { 0 } else { 3 },
                // One more byte (charset designation).
                _ => self.state = 0,
            }
        }
        out
    }
}

/// Terminal log lines: plain text, each line starting with the time it began.
#[derive(Default)]
pub struct LogLines {
    strip: AnsiStripper,
    mid_line: bool,
}

impl LogLines {
    /// Feed raw terminal bytes; `stamp` is the time to put on lines that start here.
    pub fn feed(&mut self, data: &[u8], stamp: &str) -> Vec<u8> {
        let text = self.strip.feed(data);
        let mut out = Vec::with_capacity(text.len() + 32);
        for &b in &text {
            if !self.mid_line {
                out.push(b'[');
                out.extend_from_slice(stamp.as_bytes());
                out.extend_from_slice(b"] ");
                self.mid_line = true;
            }
            out.push(b);
            if b == b'\n' {
                self.mid_line = false;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_filtered_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let log = ActivityLog::load(dir.path().join("a.json"));
        for i in 0..5 {
            let mut e = ActivityEntry::new(ActivityKind::Query, "db", &format!("SELECT {i}"), true);
            e.db_id = Some(if i % 2 == 0 { "a" } else { "b" }.into());
            e.at = i;
            log.add(e);
        }
        let a = log.list(&ActivityFilter {
            db_id: Some("a".into()),
            ..Default::default()
        });
        assert_eq!(
            a.iter().map(|e| e.detail.as_str()).collect::<Vec<_>>(),
            ["SELECT 4", "SELECT 2", "SELECT 0"]
        );
        assert_eq!(
            log.list(&ActivityFilter {
                search: Some("select 3".into()),
                ..Default::default()
            })
            .len(),
            1
        );
        log.clear(None, Some("a"));
        let again = ActivityLog::load(dir.path().join("a.json"));
        assert_eq!(again.all().len(), 2);
        assert!(clip(&"x".repeat(MAX_TEXT + 10)).ends_with("[10 more bytes]"));
    }

    #[test]
    fn strips_terminal_escapes() {
        let mut s = AnsiStripper::default();
        let raw = b"\x1b[1;32mok\x1b[0m done\r\n\x1b]0;title\x07next\x1b(Bline\n";
        assert_eq!(
            String::from_utf8(s.feed(raw)).unwrap(),
            "ok done\nnextline\n"
        );
        // Sequences split across reads.
        let mut s = AnsiStripper::default();
        let mut out = s.feed(b"a\x1b[3");
        out.extend(s.feed(b"1mb"));
        assert_eq!(out, b"ab");
    }

    #[test]
    fn log_lines_get_timestamps() {
        let mut l = LogLines::default();
        let mut out = l.feed(b"\x1b[1mone\x1b[0m\r\ntw", "10:00:00");
        // A line continued in a later read keeps its first timestamp.
        out.extend(l.feed(b"o\nthree", "10:00:05"));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "[10:00:00] one\n[10:00:00] two\n[10:00:05] three"
        );
        // Escape-only output adds no empty stamped line.
        assert!(LogLines::default().feed(b"\x1b[K", "10:00:06").is_empty());
    }
}
