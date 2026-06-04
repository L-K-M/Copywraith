//! Heuristic detection of sensitive content in clipboard text.
//!
//! All patterns are compiled once (via `std::sync::LazyLock`) and reused across
//! calls.  The public entry point is [`contains_sensitive_data`], which returns
//! `true` if the input text matches any of the known sensitive-data patterns.

use std::sync::LazyLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` if `text` appears to contain sensitive information such as
/// passwords, credit card numbers, social security numbers, API keys, private
/// keys, JWT tokens, or standalone generated password-like tokens.
pub fn contains_sensitive_data(text: &str) -> bool {
    // Short-circuit: very short strings or obviously empty content cannot
    // contain structured secrets.
    if text.len() < 8 {
        return false;
    }

    check_credit_card(text)
        || check_ssn(text)
        || check_password_assignment(text)
        || check_api_keys(text)
        || check_private_key(text)
        || check_jwt(text)
        || check_aws_key(text)
        || check_generic_secret_assignment(text)
        || check_standalone_generated_secret(text)
}

// ---------------------------------------------------------------------------
// Credit-card numbers  (13-19 digits, optional separators, Luhn validated)
// ---------------------------------------------------------------------------

static RE_CC: LazyLock<Regex> = LazyLock::new(|| {
    // Match groups of digits optionally separated by spaces or dashes, where
    // the total digit count is between 13 and 19 (covers Visa, MasterCard,
    // Amex, Discover, etc.).
    Regex::new(r"\b(\d[\d \-]{11,22}\d)\b").unwrap()
});

fn check_credit_card(text: &str) -> bool {
    for cap in RE_CC.captures_iter(text) {
        let raw = &cap[1];
        let digits: Vec<u8> = raw
            .chars()
            .filter(|c| c.is_ascii_digit())
            .map(|c| c as u8 - b'0')
            .collect();

        if digits.len() >= 13 && digits.len() <= 19 && luhn_check(&digits) {
            return true;
        }
    }
    false
}

/// Standard Luhn / mod-10 checksum.
fn luhn_check(digits: &[u8]) -> bool {
    let mut sum: u32 = 0;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut n = d as u32;
        if double {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        double = !double;
    }
    sum % 10 == 0
}

// ---------------------------------------------------------------------------
// US Social Security Numbers  (NNN-NN-NNNN)
// ---------------------------------------------------------------------------

static RE_SSN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{3})-(\d{2})-(\d{4})\b").unwrap());

fn check_ssn(text: &str) -> bool {
    for cap in RE_SSN.captures_iter(text) {
        let area: u32 = cap[1].parse().unwrap_or(0);
        let group: u32 = cap[2].parse().unwrap_or(0);
        let serial: u32 = cap[3].parse().unwrap_or(0);

        // Exclude obviously-invalid SSNs:
        // - area 000, 666, or 900-999 are never issued
        // - group 00 or serial 0000 are never issued
        if area == 0 || area == 666 || area >= 900 {
            continue;
        }
        if group == 0 || serial == 0 {
            continue;
        }
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Password / secret assignments in config-file-style text
// ---------------------------------------------------------------------------

static RE_PASSWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)(?:password|passwd|pwd|secret|token|api_key|apikey|api[-_]?secret|auth[-_]?token|access[-_]?token|private[-_]?key|client[-_]?secret|database[-_]?url|db[-_]?password)\s*[:=]\s*\S+"#,
    )
    .unwrap()
});

fn check_password_assignment(text: &str) -> bool {
    RE_PASSWORD.is_match(text)
}

// ---------------------------------------------------------------------------
// Vendor-specific API key prefixes
// ---------------------------------------------------------------------------

static RE_API_KEYS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:\b)(sk-[A-Za-z0-9]{20,}|pk_live_[A-Za-z0-9]{20,}|pk_test_[A-Za-z0-9]{20,}|sk_live_[A-Za-z0-9]{20,}|sk_test_[A-Za-z0-9]{20,}|rk_live_[A-Za-z0-9]{20,}|rk_test_[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{36,}|gho_[A-Za-z0-9]{36,}|ghu_[A-Za-z0-9]{36,}|ghs_[A-Za-z0-9]{36,}|ghr_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{50,}|xoxb-[0-9A-Za-z\-]{20,}|xoxp-[0-9A-Za-z\-]{20,}|xoxo-[0-9A-Za-z\-]{20,}|xapp-[0-9A-Za-z\-]{20,}|glpat-[A-Za-z0-9\-_]{20,}|sq0atp-[A-Za-z0-9\-_]{20,}|sq0csp-[A-Za-z0-9\-_]{20,}|SG\.[A-Za-z0-9\-_]{22,}\.[A-Za-z0-9\-_]{22,}|key-[A-Za-z0-9]{20,}|sk-ant-api[A-Za-z0-9\-_]{20,})",
    )
    .unwrap()
});

fn check_api_keys(text: &str) -> bool {
    RE_API_KEYS.is_match(text)
}

// ---------------------------------------------------------------------------
// PEM-encoded private keys
// ---------------------------------------------------------------------------

static RE_PEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----").unwrap()
});

fn check_private_key(text: &str) -> bool {
    RE_PEM.is_match(text)
}

// ---------------------------------------------------------------------------
// JWT tokens  (three base64url segments separated by dots)
// ---------------------------------------------------------------------------

static RE_JWT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b").unwrap()
});

fn check_jwt(text: &str) -> bool {
    RE_JWT.is_match(text)
}

// ---------------------------------------------------------------------------
// AWS Access Key IDs  (AKIA + 16 uppercase alphanumeric characters)
// ---------------------------------------------------------------------------

static RE_AWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap());

fn check_aws_key(text: &str) -> bool {
    RE_AWS.is_match(text)
}

// ---------------------------------------------------------------------------
// Generic secret assignments  (hex/base64 values assigned to secret-like names)
// ---------------------------------------------------------------------------

static RE_GENERIC_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)(?:secret|token|credential|auth)\s*[:=]\s*['\x22]?[A-Za-z0-9+/=\-_]{20,}['\x22]?"#,
    )
    .unwrap()
});

fn check_generic_secret_assignment(text: &str) -> bool {
    RE_GENERIC_SECRET.is_match(text)
}

// ---------------------------------------------------------------------------
// Standalone generated passwords
// ---------------------------------------------------------------------------

static RE_UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        .unwrap()
});

fn check_standalone_generated_secret(text: &str) -> bool {
    let candidate = unquote(text.trim());
    let len = candidate.len();

    if !(12..=128).contains(&len) {
        return false;
    }
    if candidate.bytes().any(|b| !b.is_ascii_graphic()) {
        return false;
    }
    if looks_like_url(candidate) || looks_like_email(candidate) || RE_UUID.is_match(candidate) {
        return false;
    }

    let stats = TokenStats::new(candidate);

    if stats.classes() < 2 || stats.upper + stats.lower == len || stats.digit == len {
        return false;
    }
    if stats.unique * 2 < len || stats.entropy_bits < 45.0 {
        return false;
    }

    if stats.symbol > 0 {
        return stats.classes() >= 3
            && !stats.only_lower_digit_separator_symbols()
            && stats.entropy_bits >= 45.0;
    }

    if stats.lower == 0 && stats.upper > 0 && stats.digit > 0 {
        return len >= 16
            && stats.unique >= 10
            && stats.entropy_bits >= 58.0
            && !has_single_edge_digit_run_with_long_alpha(candidate);
    }

    stats.upper > 0
        && stats.lower > 0
        && stats.digit > 0
        && len >= 16
        && stats.unique >= 12
        && stats.entropy_bits >= 60.0
        && !has_lowercase_run(candidate, 4)
        && !has_single_edge_digit_run_with_long_alpha(candidate)
}

fn unquote(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if matches!(first, b'\'' | b'"' | b'`') && first == last {
            return &text[1..text.len() - 1];
        }
    }
    text
}

fn looks_like_url(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || lower.contains("://")
}

fn looks_like_email(candidate: &str) -> bool {
    let Some(at) = candidate.find('@') else {
        return false;
    };
    at > 0 && candidate[at + 1..].contains('.') && !candidate.contains('/')
}

struct TokenStats {
    upper: usize,
    lower: usize,
    digit: usize,
    symbol: usize,
    unique: usize,
    entropy_bits: f64,
    separator_symbols: usize,
}

impl TokenStats {
    fn new(candidate: &str) -> Self {
        let mut counts = [0usize; 128];
        let mut upper = 0;
        let mut lower = 0;
        let mut digit = 0;
        let mut symbol = 0;
        let mut unique = 0;
        let mut separator_symbols = 0;

        for b in candidate.bytes() {
            let idx = b as usize;
            if counts[idx] == 0 {
                unique += 1;
            }
            counts[idx] += 1;

            if b.is_ascii_uppercase() {
                upper += 1;
            } else if b.is_ascii_lowercase() {
                lower += 1;
            } else if b.is_ascii_digit() {
                digit += 1;
            } else {
                symbol += 1;
                if matches!(b, b'-' | b'_' | b'.') {
                    separator_symbols += 1;
                }
            }
        }

        let len = candidate.len() as f64;
        let entropy_bits = counts
            .iter()
            .filter(|count| **count > 0)
            .map(|count| {
                let probability = *count as f64 / len;
                -probability * probability.log2()
            })
            .sum::<f64>()
            * len;

        Self {
            upper,
            lower,
            digit,
            symbol,
            unique,
            entropy_bits,
            separator_symbols,
        }
    }

    fn classes(&self) -> usize {
        [self.upper, self.lower, self.digit, self.symbol]
            .iter()
            .filter(|count| **count > 0)
            .count()
    }

    fn only_lower_digit_separator_symbols(&self) -> bool {
        self.lower > 0
            && self.upper == 0
            && self.digit > 0
            && self.symbol > 0
            && self.symbol == self.separator_symbols
    }
}

fn has_single_edge_digit_run_with_long_alpha(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    let mut digit_runs = 0;
    let mut run_start = 0;
    let mut run_end = 0;
    let mut in_digit_run = false;

    for (idx, b) in bytes.iter().enumerate() {
        if b.is_ascii_digit() {
            if !in_digit_run {
                digit_runs += 1;
                run_start = idx;
                in_digit_run = true;
            }
            run_end = idx + 1;
        } else {
            in_digit_run = false;
        }
    }

    if digit_runs != 1 || run_end - run_start < 4 {
        return false;
    }

    let prefix_alpha = bytes[..run_start].iter().all(u8::is_ascii_alphabetic);
    let suffix_alpha = bytes[run_end..].iter().all(u8::is_ascii_alphabetic);

    (run_start == 0 && suffix_alpha && bytes.len() - run_end >= 5)
        || (run_end == bytes.len() && prefix_alpha && run_start >= 5)
}

fn has_lowercase_run(candidate: &str, min_len: usize) -> bool {
    let mut run = 0;
    for b in candidate.bytes() {
        if b.is_ascii_lowercase() {
            run += 1;
            if run >= min_len {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_credit_card_visa() {
        assert!(contains_sensitive_data("My card is 4111 1111 1111 1111"));
    }

    #[test]
    fn detects_credit_card_with_dashes() {
        assert!(contains_sensitive_data("CC: 4111-1111-1111-1111"));
    }

    #[test]
    fn detects_credit_card_plain() {
        assert!(contains_sensitive_data("4111111111111111"));
    }

    #[test]
    fn rejects_invalid_luhn() {
        assert!(!contains_sensitive_data("4111111111111112"));
    }

    #[test]
    fn detects_ssn() {
        assert!(contains_sensitive_data("SSN: 123-45-6789"));
    }

    #[test]
    fn rejects_invalid_ssn_area_000() {
        assert!(!contains_sensitive_data("000-12-3456"));
    }

    #[test]
    fn rejects_invalid_ssn_area_666() {
        assert!(!contains_sensitive_data("666-12-3456"));
    }

    #[test]
    fn rejects_invalid_ssn_area_900() {
        assert!(!contains_sensitive_data("900-12-3456"));
    }

    #[test]
    fn detects_password_assignment() {
        assert!(contains_sensitive_data("password=hunter2"));
        assert!(contains_sensitive_data("PASSWORD: mysecretpass123"));
        assert!(contains_sensitive_data("DB_PASSWORD=s3cret!"));
    }

    #[test]
    fn detects_api_key_stripe() {
        assert!(contains_sensitive_data("sk_live_abcdefghijklmnopqrstuvwx"));
    }

    #[test]
    fn detects_api_key_github_pat() {
        assert!(contains_sensitive_data(
            "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl"
        ));
    }

    #[test]
    fn detects_openai_key() {
        assert!(contains_sensitive_data(
            "sk-abcdefghijklmnopqrstuvwxyz1234567890abcd"
        ));
    }

    #[test]
    fn detects_private_key() {
        assert!(contains_sensitive_data("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(contains_sensitive_data("-----BEGIN PRIVATE KEY-----"));
        assert!(contains_sensitive_data(
            "-----BEGIN OPENSSH PRIVATE KEY-----"
        ));
    }

    #[test]
    fn detects_jwt() {
        // A minimal JWT-like structure
        assert!(contains_sensitive_data(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        ));
    }

    #[test]
    fn detects_aws_key() {
        assert!(contains_sensitive_data("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn detects_generic_secret_assignment() {
        assert!(contains_sensitive_data("secret=abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn detects_standalone_generated_password() {
        assert!(contains_sensitive_data("AGAD8XLUXJCYMT7EKY"));
        assert!(contains_sensitive_data("\"AGAD8XLUXJCYMT7EKY\""));
    }

    #[test]
    fn detects_standalone_symbol_password() {
        assert!(contains_sensitive_data("N9v$kL2p!Q8zR"));
    }

    #[test]
    fn ignores_normal_text() {
        assert!(!contains_sensitive_data("Hello, world!"));
        assert!(!contains_sensitive_data(
            "The quick brown fox jumps over the lazy dog."
        ));
        assert!(!contains_sensitive_data("Meeting at 3pm tomorrow."));
    }

    #[test]
    fn ignores_short_text() {
        assert!(!contains_sensitive_data("hi"));
        assert!(!contains_sensitive_data(""));
    }

    #[test]
    fn ignores_normal_numbers() {
        assert!(!contains_sensitive_data("Order #12345"));
        assert!(!contains_sensitive_data("Phone: 555-0123"));
    }

    #[test]
    fn ignores_common_standalone_non_secrets() {
        assert!(!contains_sensitive_data("TheQuickBrownFox"));
        assert!(!contains_sensitive_data("COPYWRAITH20260528"));
        assert!(!contains_sensitive_data("some-random-string123"));
        assert!(!contains_sensitive_data(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(!contains_sensitive_data("alice123@example.com"));
    }
}
