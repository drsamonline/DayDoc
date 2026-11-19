//! Two explicit ID spaces.
//!
//! `id` is a ULID: internal, opaque, stable across renames and moves.
//! `decision_id` is `DR-NNNN`: the human reference and the supersession
//! pointer. They are never conflated.

/// A fresh ULID for a note.
pub fn new_ulid() -> String {
    ulid::Ulid::new().to_string()
}

/// Format an allocation counter value as a human decision reference.
pub fn decision_id(n: u64) -> String {
    format!("DR-{n:04}")
}

/// Parse `DR-0007` (or `dr-7`, or `7`) into `7`.
pub fn parse_decision_id(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let digits = s
        .strip_prefix("DR-")
        .or_else(|| s.strip_prefix("dr-"))
        .or_else(|| s.strip_prefix("Dr-"))
        .unwrap_or(s);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()
}

/// A filesystem-safe slug from a title: lowercase, alphanumeric and hyphens.
pub fn slug(title: &str, max_len: usize) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= max_len {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_ids_round_trip() {
        assert_eq!(decision_id(7), "DR-0007");
        assert_eq!(decision_id(1234), "DR-1234");
        assert_eq!(parse_decision_id("DR-0007"), Some(7));
        assert_eq!(parse_decision_id("dr-12"), Some(12));
        assert_eq!(parse_decision_id("nonsense"), None);
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("Adopt Okta for SSO", 40), "adopt-okta-for-sso");
        assert_eq!(slug("   ", 40), "untitled");
        assert_eq!(slug("a/b\\c:d", 40), "a-b-c-d");
    }

    #[test]
    fn ulids_are_unique_and_sortable_by_time() {
        let a = new_ulid();
        let b = new_ulid();
        assert_ne!(a, b);
        assert_eq!(a.len(), 26);
    }
}
