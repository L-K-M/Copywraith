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
/// keys, or JWT tokens.
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
}
