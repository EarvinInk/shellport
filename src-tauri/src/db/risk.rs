//! A small SQL scanner used for safety checks: splits a script into
//! statements and spots the ones that change or destroy data.
//!
//! It is not a parser. It understands quoting (`'…'`, `E'…'`, `"…"`,
//! `$tag$…$tag$`), comments and parentheses well enough to find the top-level
//! keywords of each statement, which is all the checks need. It errs on the
//! side of warning: an unusual statement may be flagged, a plain one never
//! slips through as "read-only" when it writes.

use serde::Serialize;

/// One statement of a script.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// The statement text, trimmed, without the trailing `;`.
    pub text: String,
    /// Bare words outside parentheses, lowercased, in order.
    pub top: Vec<String>,
    /// Bare words at any depth, lowercased.
    pub all: Vec<String>,
}

/// Split a script into statements.
pub fn split(sql: &str) -> Vec<Statement> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut top = Vec::new();
    let mut all = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<Statement>, from: usize, to: usize, top: &mut Vec<String>, all: &mut Vec<String>| {
        let text = sql[from..to].trim().to_string();
        if !all.is_empty() {
            out.push(Statement { text, top: std::mem::take(top), all: std::mem::take(all) });
        } else {
            top.clear();
        }
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                // Nested block comments, as in PostgreSQL.
                let mut level = 0;
                while i < b.len() {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        level += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        level -= 1;
                        i += 2;
                        if level == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'\'' => {
                // E'…' strings allow backslash escapes.
                let escapes = i > 0 && (b[i - 1] == b'E' || b[i - 1] == b'e') && (i < 2 || !is_word(b[i - 2]));
                i += 1;
                while i < b.len() {
                    if escapes && b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'\'' {
                        if b.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && !(b[i] == b'"' && b.get(i + 1) != Some(&b'"')) {
                    i += if b[i] == b'"' { 2 } else { 1 };
                }
            }
            b'$' if i == 0 || !is_word(b[i - 1]) => {
                // $tag$ … $tag$ (tag may be empty). `$1` parameters are not tags.
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if j < b.len() && b[j] == b'$' && !b[i + 1..j].first().is_some_and(|d| d.is_ascii_digit()) {
                    let tag = &sql[i..=j];
                    match sql[j + 1..].find(tag) {
                        Some(end) => i = j + 1 + end + tag.len() - 1,
                        None => i = b.len(),
                    }
                }
            }
            b'(' => depth += 1,
            b')' => depth = (depth - 1).max(0),
            b';' => {
                push(&mut out, start, i, &mut top, &mut all);
                start = i + 1;
                depth = 0;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let s = i;
                while i < b.len() && is_word(b[i]) {
                    i += 1;
                }
                let w = sql[s..i].to_ascii_lowercase();
                if depth == 0 {
                    top.push(w.clone());
                }
                all.push(w);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    push(&mut out, start, sql.len(), &mut top, &mut all);
    out
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// Why a statement needs a second look.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    /// The statement (shortened for display).
    pub statement: String,
    pub reason: String,
}

/// The verb that decides what a statement does: the first word, or for
/// `WITH …` the first top-level verb after the CTEs.
fn verb(s: &Statement) -> &str {
    let first = s.top.first().map(String::as_str).unwrap_or("");
    if first == "with" {
        if let Some(v) = s.top.iter().find(|w| matches!(w.as_str(), "select" | "insert" | "update" | "delete" | "merge" | "values" | "table")) {
            return v;
        }
    }
    if first == "explain" && !s.top.iter().any(|w| w == "analyze" || w == "analyse") {
        return "explain";
    }
    if first == "explain" {
        // EXPLAIN ANALYZE runs the statement.
        return s.top.iter().find(|w| matches!(w.as_str(), "select" | "insert" | "update" | "delete" | "merge")).map(String::as_str).unwrap_or("explain");
    }
    first
}

fn shorten(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 160 {
        format!("{}…", one_line.chars().take(160).collect::<String>())
    } else {
        one_line
    }
}

/// Statements that change or destroy a lot of data at once.
pub fn classify(sql: &str) -> Vec<Risk> {
    let mut risks = Vec::new();
    for s in split(sql) {
        let has_where = s.top.iter().any(|w| w == "where");
        let reason = match verb(&s) {
            "update" if !has_where => Some("UPDATE without WHERE changes every row".to_string()),
            "delete" if !has_where => Some("DELETE without WHERE removes every row".to_string()),
            "truncate" => Some("TRUNCATE removes every row".to_string()),
            "drop" => {
                let what = s.top.get(1).map(String::as_str).unwrap_or("object");
                Some(format!("DROP {} cannot be undone", what.to_uppercase()))
            }
            "alter" if s.top.iter().skip(1).any(|w| w == "drop") => Some("ALTER … DROP removes part of an object".to_string()),
            _ => None,
        };
        if let Some(reason) = reason {
            risks.push(Risk { statement: shorten(&s.text), reason });
        }
    }
    risks
}

/// True when every statement only reads (used for read-only connections that
/// the server cannot enforce, e.g. behind a transaction-mode pooler).
pub fn is_read_only(sql: &str) -> bool {
    split(sql).iter().all(|s| {
        let writes_anywhere = s.all.iter().any(|w| matches!(w.as_str(), "insert" | "update" | "delete" | "merge" | "truncate" | "copy"));
        match verb(s) {
            "select" => !writes_anywhere && !s.top.iter().any(|w| w == "into"),
            "values" | "table" | "show" | "explain" | "fetch" => !writes_anywhere,
            "begin" | "start" | "commit" | "end" | "rollback" | "abort" | "close" => true,
            _ => false,
        }
    })
}

/// A single plain query whose rows can be read through a cursor.
pub fn as_cursor_query(sql: &str) -> Option<String> {
    let stmts = split(sql);
    let [s] = stmts.as_slice() else { return None };
    let verb = verb(s);
    let writes = s.all.iter().any(|w| matches!(w.as_str(), "insert" | "update" | "delete" | "merge"));
    let plain = matches!(verb, "select" | "values" | "table") && !writes && !s.top.iter().any(|w| w == "into" || w == "for");
    plain.then(|| s.text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_respecting_quotes_and_comments() {
        let s = split("SELECT ';' AS a; -- c;\nSELECT $f$ ; $f$, \"x;y\" /* ; /* nested ; */ */ ; ;");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].text, "SELECT ';' AS a");
        assert_eq!(s[1].top, vec!["select"]);
    }

    #[test]
    fn flags_dangerous_statements() {
        let r = classify("update users set active = false; delete from logs; truncate t; drop table x; alter table y drop column z");
        let reasons: Vec<_> = r.iter().map(|x| x.reason.as_str()).collect();
        assert_eq!(r.len(), 5, "{reasons:?}");
        assert!(reasons[3].contains("DROP TABLE"));
    }

    #[test]
    fn where_inside_subquery_does_not_count() {
        assert_eq!(classify("UPDATE t SET a = (SELECT 1 FROM u WHERE u.id = 2)").len(), 1);
        assert!(classify("UPDATE t SET a = 1 WHERE id = 2").is_empty());
        assert!(classify("DELETE FROM t WHERE id IN (SELECT id FROM u)").is_empty());
        assert!(classify("SELECT 'delete from x'").is_empty());
    }

    #[test]
    fn with_clause_uses_main_verb() {
        assert_eq!(classify("WITH old AS (SELECT id FROM t) DELETE FROM t").len(), 1);
        assert!(classify("WITH x AS (SELECT 1) SELECT * FROM x").is_empty());
    }

    #[test]
    fn read_only_detection() {
        assert!(is_read_only("SELECT 1; SHOW work_mem; EXPLAIN SELECT 1"));
        assert!(!is_read_only("SELECT 1; UPDATE t SET a = 1 WHERE b"));
        assert!(!is_read_only("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"));
        assert!(!is_read_only("SELECT * INTO new_t FROM t"));
        assert!(!is_read_only("EXPLAIN ANALYZE DELETE FROM t"));
        assert!(!is_read_only("CREATE TABLE t (a int)"));
    }

    #[test]
    fn cursor_queries() {
        assert_eq!(as_cursor_query("SELECT * FROM t;  ").as_deref(), Some("SELECT * FROM t"));
        assert!(as_cursor_query("SELECT 1; SELECT 2").is_none());
        assert!(as_cursor_query("SELECT * FROM t FOR UPDATE").is_none());
        assert!(as_cursor_query("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d").is_none());
        assert!(as_cursor_query("SHOW work_mem").is_none());
    }

    #[test]
    fn dollar_params_are_not_quotes() {
        assert_eq!(split("SELECT $1; SELECT 2").len(), 2);
    }
}
