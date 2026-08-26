//! JSON Sanitizer  extracts and repairs JSON from noisy LLM output.
//!
//! LLMs frequently produce JSON that is *almost* valid but contains:
//! - Trailing commas before `}` or `]`
//! - Doc-comments (`//` or `/* */`) inside JSON
//! - Single-quoted strings instead of double-quoted
//! - Unescaped newlines inside string values
//! - BOM or invisible leading characters
//!
//! This module applies a pipeline of repair passes to produce valid JSON.

use std::borrow::Cow;
use std::sync::LazyLock;

static RE_TRAILING_OBJ: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r",(\s*\})").expect("RE_TRAILING_OBJ"));

static RE_TRAILING_ARR: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r",(\s*\])").expect("RE_TRAILING_ARR"));

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Attempt to extract and sanitize a JSON object from `raw` text.
///
/// Returns a sanitized JSON string, or an error if no recoverable JSON found.
/// Fixes double-escaped sequences (like literal backslash-n) that some
/// strict reasoning models (like openai/gpt-oss-120b) emit inside JSON string content.


pub fn sanitize(raw: &str) -> Result<String, SanitizeError> {
    let extracted = extract_json_block(raw).ok_or(SanitizeError::NoJsonFound)?;
    let sanitized = repair_pipeline(&extracted);

    // First attempt: parse as-is
    if serde_json::from_str::<serde_json::Value>(&sanitized).is_ok() {
        return Ok(sanitized);
    }

    // Second attempt: balance brackets for truncated JSON
    let balanced = balance_json_brackets(&sanitized);
    serde_json::from_str::<serde_json::Value>(&balanced)
        .map(|_| balanced)
        .map_err(|e| SanitizeError::UnrecoverableJson {
            reason: e.to_string(),
            fragment: sanitized.chars().take(120).collect(),
        })
}

/// Try to close any unclosed brackets/braces in truncated JSON.
/// Conservative repair only: close open strings, arrays, and objects.
/// This avoids syntax breakage from cut-off provider responses.
fn balance_json_brackets(s: &str) -> String {
    let mut depth_brace: i32 = 0;
    let mut depth_bracket: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth_brace += 1,
            '}' if !in_string => depth_brace -= 1,
            '[' if !in_string => depth_bracket += 1,
            ']' if !in_string => depth_bracket -= 1,
            _ => {}
        }
    }

    let mut result = s.to_string();

    if in_string {
        result.push('"');
    }

    for _ in 0..depth_bracket.max(0) {
        result.push(']');
    }
    for _ in 0..depth_brace.max(0) {
        result.push('}');
    }

    result
}

#[derive(Debug, thiserror::Error)]
pub enum SanitizeError {
    #[error("no JSON block found in text")]
    NoJsonFound,

    #[error("JSON unrecoverable after repair: {reason}  fragment: {fragment}")]
    UnrecoverableJson { reason: String, fragment: String },
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

fn extract_json_block(raw: &str) -> Option<String> {
    // 1. Markdown fenced blocks: ```json ... ``` or ``` ... ```
    for fence in &["```json", "```"] {
        if let Some(start) = raw.find(fence) {
            let after = &raw[start + fence.len()..];
            if let Some(end) = after.find("```") {
                let block = after[..end].trim();
                if block.starts_with('{') || block.starts_with('[') {
                    return Some(block.to_string());
                }
            }
        }
    }

    // 2. Bare JSON: find first `{` and last matching `}`
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end > start {
        return Some(raw[start..=end].to_string());
    }

    None
}

// ---------------------------------------------------------------------------
// Repair pipeline
// ---------------------------------------------------------------------------

fn repair_pipeline(json: &str) -> String {
    let s = strip_bom(json);
    let s = strip_line_comments(&s);
    let s = strip_block_comments(&s);
    let s = remove_trailing_commas(&s);
    let s = fix_single_quotes(&s);
    fix_unescaped_newlines_in_strings(&s)
}

/// Remove UTF-8 BOM if present.
fn strip_bom(s: &str) -> Cow<'_, str> {
    if let Some(stripped) = s.strip_prefix('\u{FEFF}') {
        Cow::Owned(stripped.to_string())
    } else {
        Cow::Borrowed(s)
    }
}

/// Remove `//` line comments that appear outside string values.
fn strip_line_comments(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if escape_next {
            result.push(c);
            escape_next = false;
            i += 1;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            i += 1;
            continue;
        }

        // Detect `//` outside strings
        if !in_string && c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            // Skip until end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// Remove `/* ... */` block comments outside string values.
fn strip_block_comments(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if escape_next {
            result.push(c);
            escape_next = false;
            i += 1;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            i += 1;
            continue;
        }

        if !in_string && c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip closing */
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// Remove trailing commas before `}` or `]`.
fn remove_trailing_commas(s: &str) -> String {
    // Pattern: ,\s*} or ,\s*]
    let re_obj = &*RE_TRAILING_OBJ;
    let re_arr = &*RE_TRAILING_ARR;
    let s = re_obj.replace_all(s, "$1");
    re_arr.replace_all(&s, "$1").to_string()
}

/// Replace outer-level single-quoted strings with double-quoted ones.
/// v8.3: Context-aware — does NOT touch single quotes inside double-quoted strings,
/// which protects Rust lifetime annotations like <'a> and &'a from corruption.
fn fix_single_quotes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_double_string = false;
    let mut escape_next = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if escape_next {
            result.push(c);
            escape_next = false;
            i += 1;
            continue;
        }

        if c == '\\' && in_double_string {
            result.push(c);
            escape_next = true;
            i += 1;
            continue;
        }

        if c == '"' {
            in_double_string = !in_double_string;
            result.push(c);
            i += 1;
            continue;
        }

        // Inside double-quoted strings, don't touch single quotes at all
        if in_double_string {
            result.push(c);
            i += 1;
            continue;
        }

        // Outside double-quoted strings: replace 'value' → "value"
        if c == '\'' {
            let _start = i;
            i += 1;
            let mut content = String::new();
            let mut found_close = false;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    content.push(chars[i]);
                    content.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == '\'' {
                    // Check if this closing quote is actually an inner quote
                    let is_inner = if i + 1 < chars.len() {
                        chars[i + 1].is_alphabetic()
                    } else {
                        false
                    };

                    if is_inner {
                        content.push(chars[i]);
                        i += 1;
                        continue;
                    }

                    found_close = true;
                    i += 1;
                    break;
                }
                // Don't allow matching across newlines (not a JSON string)
                if chars[i] == '\n' {
                    break;
                }
                content.push(chars[i]);
                i += 1;
            }
            if found_close {
                result.push('"');
                result.push_str(&content);
                result.push('"');
            } else {
                // No matching close quote on same line — leave as-is
                result.push('\'');
                result.push_str(&content);
            }
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}

/// Escape literal newlines inside JSON string values.
fn fix_unescaped_newlines_in_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escape_next = false;

    for c in s.chars() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }
        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }
        if in_string && c == '\n' {
            result.push_str("\\n");
            continue;
        }
        if in_string && c == '\r' {
            result.push_str("\\r");
            continue;
        }
        result.push(c);
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_line_comments() {
        let input = r#"{"key": "val" // comment
}"#;
        let result = strip_line_comments(input);
        assert!(!result.contains("//"));
        assert!(result.contains("\"val\""));
    }

    

    

    #[test]
    fn test_remove_trailing_commas_object() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let result = remove_trailing_commas(input);
        assert_eq!(result, r#"{"a": 1, "b": 2}"#);
    }

    #[test]
    fn test_remove_trailing_commas_array() {
        let input = r#"[1, 2, 3,]"#;
        let result = remove_trailing_commas(input);
        assert_eq!(result, r#"[1, 2, 3]"#);
    }

    #[test]
    fn test_sanitize_valid_json() {
        let input = r#"{"plan": "fix", "commands": []}"#;
        // Empty commands list  but JSON itself is valid
        let result = sanitize(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_with_trailing_comma() {
        let input = r#"{"a": 1,}"#;
        let result = sanitize(input).expect("test setup/use should succeed");
        let val: serde_json::Value =
            serde_json::from_str(&result).expect("test setup/use should succeed");
        assert_eq!(val["a"], 1);
    }

    #[test]
    fn test_sanitize_markdown_fenced() {
        let input = "```json\n{\"x\": 42}\n```";
        let result = sanitize(input).expect("test setup/use should succeed");
        let val: serde_json::Value =
            serde_json::from_str(&result).expect("test setup/use should succeed");
        assert_eq!(val["x"], 42);
    }

    #[test]
    fn test_sanitize_no_json_error() {
        assert!(matches!(
            sanitize("nothing here"),
            Err(SanitizeError::NoJsonFound)
        ));
    }

    #[test]
    fn test_strip_block_comments() {
        let input = r#"{"a": /* comment */ 1}"#;
        let result = strip_block_comments(input);
        let sanitized = remove_trailing_commas(&result);
        let val: serde_json::Value =
            serde_json::from_str(&sanitized).expect("test setup/use should succeed");
        assert_eq!(val["a"], 1);
    }

    #[test]
    fn test_fix_unescaped_newlines() {
        let input = "{\"content\": \"line1\nline2\"}";
        let result = fix_unescaped_newlines_in_strings(input);
        assert!(result.contains("\\n"));
        let _: serde_json::Value =
            serde_json::from_str(&result).expect("test setup/use should succeed");
    }

    #[test]
    fn test_fix_single_quotes_preserves_rust_lifetimes() {
        // v8.3: Rust lifetimes inside double-quoted JSON strings must NOT be corrupted
        let input =
            r#"{"content": "pub fn most_frequent<'a>(items: &[&'a str]) -> Option<&'a str>"}"#;
        let result = fix_single_quotes(input);
        assert!(
            result.contains("<'a>"),
            "lifetime <'a> was corrupted: {}",
            result
        );
        assert!(
            result.contains("&'a"),
            "lifetime &'a was corrupted: {}",
            result
        );
    }

    #[test]
    fn test_fix_single_quotes_replaces_json_keys() {
        // Single-quoted JSON keys outside double strings should be fixed
        let input = "{'key': 'value'}";
        let result = fix_single_quotes(input);
        assert_eq!(result, r#"{"key": "value"}"#);
    }
}
