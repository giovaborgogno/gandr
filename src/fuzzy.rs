//! A tiny fzf-style fuzzy matcher for the ref picker (no extra dependency).
//!
//! Greedy subsequence match with bonuses for contiguous runs and matches at
//! word boundaries (start, or after `/ _ - .`). Smart-case: case-insensitive
//! unless the query contains an uppercase character, mirroring `search`. Good
//! enough to rank a few hundred branch/tag names predictably.

/// Score `text` against `query` (higher is better); `None` if `text` doesn't
/// contain `query` as a subsequence. An empty query matches everything at 0.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    // ASCII-only case detection to match the ASCII case-folding below (so the
    // "any uppercase ⇒ case-sensitive" rule and the comparison never disagree).
    let sensitive = query.chars().any(|c| c.is_ascii_uppercase());
    let eq = |a: char, b: char| {
        if sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(&b)
        }
    };

    let q: Vec<char> = query.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut qi = 0;
    let mut score = 0i32;
    let mut prev: Option<usize> = None;

    for (i, &tc) in t.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if eq(tc, q[qi]) {
            score += 10;
            if prev == Some(i.wrapping_sub(1)) {
                score += 15; // contiguous with the previous match
            }
            let boundary = i == 0
                || t.get(i - 1)
                    .is_some_and(|&c| matches!(c, '/' | '_' | '-' | '.'));
            if boundary {
                score += 10;
            }
            prev = Some(i);
            qi += 1;
        }
    }

    if qi == q.len() {
        // Slightly prefer shorter candidates when scores are otherwise close.
        Some(score - t.len() as i32 / 4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequence_only() {
        assert!(score("mn", "main").is_some());
        assert!(score("main", "main").is_some());
        assert!(score("xyz", "main").is_none());
        // Order matters: "nm" is not a subsequence of "main".
        assert!(score("nm", "main").is_none());
    }

    #[test]
    fn empty_query_matches_all() {
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn smart_case() {
        // Lowercase query is case-insensitive.
        assert!(score("main", "MAIN").is_some());
        // Uppercase query is case-sensitive.
        assert!(score("MAIN", "main").is_none());
        assert!(score("Main", "Main").is_some());
    }

    #[test]
    fn prefers_contiguous_and_boundary_matches() {
        // "feat" contiguous at a boundary should outrank a scattered match.
        let contiguous = score("feat", "feature/x").unwrap();
        let scattered = score("feat", "f-e-a-t-xyzzy").unwrap();
        assert!(contiguous > scattered, "{contiguous} !> {scattered}");
    }

    #[test]
    fn ranks_prefix_over_late_match() {
        let prefix = score("rel", "release").unwrap();
        let late = score("rel", "pre-release-candidate-build").unwrap();
        assert!(prefix > late, "{prefix} !> {late}");
    }
}
