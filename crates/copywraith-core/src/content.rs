use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use sha2::{Digest, Sha256};

/// Compute SHA-256 hash of bytes and return as hex string
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(&result)
}

/// Compute SHA-256 hash of a string and return as hex string
pub fn hash_text(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Returns `true` if `hash` is a valid SHA-256 hex string (exactly 64 hex chars).
/// Use this before joining a hash to a filesystem path to prevent path traversal.
pub fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Encode bytes to base64 string
pub fn bytes_to_base64(data: &[u8]) -> String {
    BASE64.encode(data)
}

/// Decode base64 string to bytes
pub fn base64_to_bytes(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64.decode(encoded)
}

/// Strip HTML tags and decode common entities to produce plain text.
pub fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_style = false;
    let mut in_script = false;
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '<' {
            // Check for <style/<script opening and closing tags.
            // Tag names are ASCII per spec, so a case-insensitive prefix
            // compare on the original chars is sufficient (and avoids the
            // index drift a separate lowercased copy can introduce for
            // non-ASCII text).
            let rest = &chars[i..];
            if starts_with_ignore_ascii_case(rest, "<style") {
                in_style = true;
            } else if starts_with_ignore_ascii_case(rest, "<script") {
                in_script = true;
            } else if starts_with_ignore_ascii_case(rest, "</style") {
                in_style = false;
            } else if starts_with_ignore_ascii_case(rest, "</script") {
                in_script = false;
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if chars[i] == '>' {
            in_tag = false;
            i += 1;
            continue;
        }

        if !in_tag && !in_style && !in_script {
            // Decode HTML entities
            if chars[i] == '&' {
                if let Some((decoded, consumed)) = decode_html_entity(&chars[i..]) {
                    result.push(decoded);
                    i += consumed;
                    continue;
                }
            }
            result.push(chars[i]);
        }

        i += 1;
    }

    // Collapse whitespace
    let collapsed: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
}

fn starts_with_ignore_ascii_case(chars: &[char], prefix: &str) -> bool {
    let mut iter = chars.iter();
    prefix
        .chars()
        .all(|p| iter.next().is_some_and(|c| c.eq_ignore_ascii_case(&p)))
}

fn decode_html_entity(chars: &[char]) -> Option<(char, usize)> {
    const MAX_ENTITY_LEN: usize = 16;

    if chars.first().copied()? != '&' {
        return None;
    }

    let semicolon_index = chars
        .iter()
        .take(MAX_ENTITY_LEN)
        .position(|ch| *ch == ';')?;
    if semicolon_index < 2 {
        return None;
    }

    let entity: String = chars[1..semicolon_index].iter().collect();
    let decoded = match entity.as_str() {
        "nbsp" => Some(' '),
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        _ => decode_numeric_html_entity(&entity),
    }?;

    Some((decoded, semicolon_index + 1))
}

fn decode_numeric_html_entity(entity: &str) -> Option<char> {
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    } else if let Some(decimal) = entity.strip_prefix('#') {
        decimal.parse::<u32>().ok().and_then(char::from_u32)
    } else {
        None
    }
}

/// Strip RTF control words and groups to extract plain text.
pub fn strip_rtf(rtf: &str) -> String {
    // Quick check: if it doesn't start with {\rtf, return as-is
    if !rtf.trim_start().starts_with("{\\rtf") {
        return rtf.to_string();
    }

    let mut result = String::with_capacity(rtf.len() / 2);
    let chars: Vec<char> = rtf.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut depth: usize = 0;
    // Track whether we're in a group that should be skipped (like \fonttbl, \colortbl, etc.)
    let mut skip_depth: Option<usize> = None;
    // Number of fallback characters following each \uN escape (set by \ucN).
    let mut uc_skip: usize = 1;
    // High half of a UTF-16 surrogate pair from a previous \uN escape.
    let mut pending_high_surrogate: Option<u32> = None;

    while i < len {
        match chars[i] {
            '{' => {
                depth += 1;
                // Check if this starts a skippable group
                if skip_depth.is_none() && i + 1 < len && chars[i + 1] == '\\' {
                    let rest: String = chars[i + 1..].iter().take(20).collect();
                    if rest.starts_with("\\fonttbl")
                        || rest.starts_with("\\colortbl")
                        || rest.starts_with("\\stylesheet")
                        || rest.starts_with("\\info")
                        || rest.starts_with("\\*\\")
                        || rest.starts_with("\\expandedcolortbl")
                    {
                        skip_depth = Some(depth);
                    }
                }
                i += 1;
            }
            '}' => {
                if skip_depth == Some(depth) {
                    skip_depth = None;
                }
                // Saturate so malformed input with unbalanced braces cannot
                // underflow (which would panic in debug builds).
                depth = depth.saturating_sub(1);
                i += 1;
            }
            '\\' if skip_depth.is_none() => {
                i += 1;
                if i >= len {
                    break;
                }
                // Special characters
                match chars[i] {
                    '\\' => {
                        result.push('\\');
                        i += 1;
                    }
                    '{' => {
                        result.push('{');
                        i += 1;
                    }
                    '}' => {
                        result.push('}');
                        i += 1;
                    }
                    '\n' | '\r' => {
                        // Line break after backslash — skip
                        i += 1;
                    }
                    '\'' => {
                        // Hex-encoded character \'xx
                        if i + 2 < len {
                            let hex: String = chars[i + 1..i + 3].iter().collect();
                            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                                result.push(byte as char);
                            }
                            i += 3;
                        } else {
                            i += 1;
                        }
                    }
                    _ => {
                        // Read control word
                        let start = i;
                        while i < len && chars[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        let word: String = chars[start..i].iter().collect();

                        // Read optional numeric parameter
                        let mut param = String::new();
                        if i < len && (chars[i] == '-' || chars[i].is_ascii_digit()) {
                            while i < len && (chars[i] == '-' || chars[i].is_ascii_digit()) {
                                param.push(chars[i]);
                                i += 1;
                            }
                        }

                        // Consume trailing space delimiter
                        if i < len && chars[i] == ' ' {
                            i += 1;
                        }

                        // Convert known control words to text
                        match word.as_str() {
                            "par" | "line" => result.push('\n'),
                            "tab" => result.push('\t'),
                            "emdash" => result.push('\u{2014}'),
                            "endash" => result.push('\u{2013}'),
                            "lquote" => result.push('\u{2018}'),
                            "rquote" => result.push('\u{2019}'),
                            "ldblquote" => result.push('\u{201C}'),
                            "rdblquote" => result.push('\u{201D}'),
                            "bullet" => result.push('\u{2022}'),
                            "uc" => {
                                uc_skip = param.parse::<usize>().unwrap_or(1).min(8);
                            }
                            "u" => {
                                // \uN unicode escape: N is a signed 16-bit
                                // decimal code unit; negative values wrap.
                                if let Ok(signed) = param.parse::<i32>() {
                                    let unit =
                                        (if signed < 0 { signed + 65536 } else { signed }) as u32;
                                    push_rtf_unicode_unit(
                                        &mut result,
                                        unit,
                                        &mut pending_high_surrogate,
                                    );
                                }
                                // Skip the fallback character(s) that follow
                                // the escape for non-unicode readers.
                                i = skip_rtf_fallback_chars(&chars, i, uc_skip);
                            }
                            _ => {} // Skip unknown control words
                        }
                    }
                }
            }
            _ if skip_depth.is_some() => {
                i += 1;
            }
            _ if depth >= 1 => {
                // Normal text inside the document
                result.push(chars[i]);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        // Fallback: if stripping produced nothing, return original truncated
        rtf.trim().to_string()
    } else {
        trimmed
    }
}

/// Append one UTF-16 code unit from an RTF `\uN` escape, combining
/// surrogate pairs across consecutive escapes.
fn push_rtf_unicode_unit(result: &mut String, unit: u32, pending_high: &mut Option<u32>) {
    match unit {
        0xD800..=0xDBFF => {
            // High surrogate: wait for the low half in the next \uN escape.
            *pending_high = Some(unit);
        }
        0xDC00..=0xDFFF => {
            if let Some(high) = pending_high.take() {
                let combined = 0x10000 + ((high - 0xD800) << 10) + (unit - 0xDC00);
                if let Some(ch) = char::from_u32(combined) {
                    result.push(ch);
                }
            }
            // A lone low surrogate is malformed; drop it.
        }
        _ => {
            *pending_high = None;
            if let Some(ch) = char::from_u32(unit) {
                result.push(ch);
            }
        }
    }
}

/// Skip the `count` fallback characters that follow a `\uN` escape.
/// A fallback unit is either a plain character or a `\'xx` hex escape.
/// Stops early at group delimiters or other control words.
fn skip_rtf_fallback_chars(chars: &[char], mut i: usize, count: usize) -> usize {
    let len = chars.len();
    for _ in 0..count {
        if i >= len {
            break;
        }
        match chars[i] {
            '{' | '}' => break,
            '\\' => {
                if i + 1 < len && chars[i + 1] == '\'' {
                    // \'xx counts as a single fallback character
                    i = (i + 4).min(len);
                } else {
                    // Another control word: fallback run is over.
                    break;
                }
            }
            _ => i += 1,
        }
    }
    i
}

/// Mask sensitive text: show first 3 characters, replace the rest with bullets.
pub fn mask_sensitive(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let visible = 3.min(chars.len());
    let prefix: String = chars[..visible].iter().collect();
    let remaining = chars
        .len()
        .saturating_sub(visible)
        .min(max_len.saturating_sub(visible));
    let bullets: String = "\u{2022}".repeat(remaining);
    format!("{}{}", prefix, bullets)
}

/// Detect image format from first bytes
pub fn detect_image_format(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if data.starts_with(b"GIF8") {
        Some("gif")
    } else if data.starts_with(b"RIFF") && data.len() > 11 && &data[8..12] == b"WEBP" {
        Some("webp")
    } else if data.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("ico")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_text() {
        let hash = hash_text("hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hash_consistency() {
        let hash1 = hash_text("test");
        let hash2 = hash_text("test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = b"hello world";
        let encoded = bytes_to_base64(original);
        let decoded = base64_to_bytes(&encoded).unwrap();
        assert_eq!(original.to_vec(), decoded);
    }

    #[test]
    fn test_detect_png() {
        let data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect_image_format(&data), Some("png"));
    }

    #[test]
    fn test_detect_jpeg() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(detect_image_format(&data), Some("jpeg"));
    }

    #[test]
    fn test_strip_html_basic() {
        let html = "<html><body><p>Hello <b>world</b></p></body></html>";
        assert_eq!(strip_html(html), "Hello world");
    }

    #[test]
    fn test_strip_html_entities() {
        assert_eq!(strip_html("a &amp; b &lt; c &gt; d"), "a & b < c > d");
    }

    #[test]
    fn test_strip_html_numeric_entities() {
        assert_eq!(strip_html("Popup&#32;position"), "Popup position");
        assert_eq!(strip_html("A&#x20;B"), "A B");
    }

    #[test]
    fn test_strip_html_plain_passthrough() {
        assert_eq!(strip_html("no tags here"), "no tags here");
    }

    #[test]
    fn test_strip_html_style_uppercase() {
        let html = "<STYLE>body { color: red }</STYLE><p>visible</p>";
        assert_eq!(strip_html(html), "visible");
    }

    #[test]
    fn test_strip_html_style_after_non_ascii_text() {
        // Unicode lowercasing can change char counts (e.g. İ -> i̇); style
        // detection must stay aligned with the original text.
        let html = "<p>İİİİİİ</p><style>p { color: red }</style><p> son</p>";
        assert_eq!(strip_html(html), "İİİİİİ son");
    }

    #[test]
    fn test_strip_rtf_basic() {
        let rtf = r"{\rtf1\ansi Hello world}";
        assert_eq!(strip_rtf(rtf), "Hello world");
    }

    #[test]
    fn test_strip_rtf_non_rtf_passthrough() {
        let plain = "just some text";
        assert_eq!(strip_rtf(plain), plain);
    }

    #[test]
    fn test_strip_rtf_unbalanced_braces_no_panic() {
        // Extra closing braces must not underflow the depth counter.
        let rtf = r"{\rtf1}}} stray";
        let _ = strip_rtf(rtf);
    }

    #[test]
    fn test_strip_rtf_unicode_escape() {
        // 舗 is a right single quotation mark; '?' is the fallback char.
        let rtf = r"{\rtf1\ansi It\u8217?s here}";
        assert_eq!(strip_rtf(rtf), "It\u{2019}s here");
    }

    #[test]
    fn test_strip_rtf_unicode_negative_escape() {
        // Negative values wrap mod 65536: -10179 -> 55357 (a high surrogate),
        // -8704 -> 56832 (low surrogate) — together U+1F600 GRINNING FACE.
        let rtf = r"{\rtf1\ansi \u-10179?\u-8704?!}";
        assert_eq!(strip_rtf(rtf), "\u{1F600}!");
    }

    #[test]
    fn test_strip_rtf_uc0_no_fallback_skip() {
        let rtf = r"{\rtf1\ansi\uc0 caf\u233 done}";
        assert_eq!(strip_rtf(rtf), "caf\u{e9}done");
    }

    #[test]
    fn test_strip_rtf_unicode_hex_fallback() {
        // \uc1 (default): the \'e9 hex escape is the fallback and is skipped.
        let rtf = r"{\rtf1\ansi caf\u233\'e9 au lait}";
        assert_eq!(strip_rtf(rtf), "caf\u{e9} au lait");
    }

    #[test]
    fn test_mask_sensitive() {
        assert_eq!(
            mask_sensitive("sk-1234567890", 20),
            "sk-\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
        );
    }

    #[test]
    fn test_mask_sensitive_short() {
        assert_eq!(mask_sensitive("ab", 20), "ab");
    }

    #[test]
    fn test_mask_sensitive_empty() {
        assert_eq!(mask_sensitive("", 20), "");
    }
}
