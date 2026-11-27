//! The one secret filter, used by every write path.

use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

struct Patterns {
    hex: Regex,
    token: Regex,
    prefix: Regex,
    spoken: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        hex: Regex::new(r"[0-9a-fA-F]{32,}").unwrap(),
        token: Regex::new(r"[A-Za-z0-9+/_-]{32,}").unwrap(),
        // `\b` keeps "desk-buddy" (…sk-b…) from matching `sk-`.
        prefix: Regex::new(r"\b(?:sk[-_]|ghp_|gho_|github_pat_|xox[abpr]-|AKIA)[A-Za-z0-9_-]").unwrap(),
        // "password is x", "wifi password: x", "pin = x", "password for the router is x"
        spoken: Regex::new(
            r"(?i)(?:\b(?:password|passcode|pin)(?:\s+for\s+[^.,;:=]{1,40}?)?(?:\s+is\s+|\s*[:=]\s*)|mật khẩu[\s:]+)\s*\S",
        )
        .unwrap(),
    })
}

/// True when `s` looks like a key, token or password the bear must not keep.
pub fn looks_secret(s: &str) -> bool {
    let p = patterns();
    let s: String = s.nfc().collect();
    p.hex.is_match(&s)
        || p.token.find_iter(&s).any(|m| mixed(m.as_str()))
        || p.prefix.is_match(&s)
        || s.contains("-----BEGIN")
        || p.spoken.is_match(&s)
}

/// Upper case, lower case and a digit: random-looking, unlike a long slug or path.
fn mixed(run: &str) -> bool {
    run.chars().any(|c| c.is_ascii_uppercase())
        && run.chars().any(|c| c.is_ascii_lowercase())
        && run.chars().any(|c| c.is_ascii_digit())
}
