//! OpenSSH-style `*` / `?` wildcard matching without exponential backtracking.

/// Match `text` against `pat`, where `*` matches any run and `?` one character.
pub fn glob_match(pat: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    // Position of the last `*` in the pattern and the text index it is matched up to.
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn basics() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("web-?", "web-1"));
        assert!(glob_match("*.example.com", "a.b.example.com"));
        assert!(!glob_match("*.example.com", "example.org"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
    }

    #[test]
    fn pathological_pattern_is_fast() {
        let text = "a".repeat(5000);
        let t = std::time::Instant::now();
        assert!(!glob_match("*a*a*a*a*a*a*a*a*a*a*b", &text));
        assert!(t.elapsed().as_millis() < 500);
    }
}
