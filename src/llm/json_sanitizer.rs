// src/llm/json_sanitizer.rs — v7.9.8: JSON Sanitizer for LLM output
// Fixes common JSON issues from LLM responses before parsing

/// Main entry point — apply all sanitizers in order
pub fn sanitize_llm_json(raw: &str) -> String {
    let s = fix_rust_doc_comments(raw);
    let s = fix_trailing_commas(&s);
    let s = fix_truncated_json(&s);
    s
}

/// Rust `///` doc comments inside JSON strings break the parser.
/// Example: "content": "/// Adds two numbers\n/// # Examples"
/// The triple-slash is treated as a comment by some parsers.
/// Fix: convert `///` to `//` inside JSON string values.
fn fix_rust_doc_comments(s: &str) -> String {
    // Fix inside JSON strings — both after escaped newlines and at the start of a value
    s.replace("\\n///", "\\n//")
     .replace("\\n    ///", "\\n    //")
     .replace("\"///", "\"//")
}

/// Close truncated JSON that was cut off mid-response.
/// Happens when the model runs out of max_tokens.
fn fix_truncated_json(s: &str) -> String {
    let mut open_strings = 0i32;
    let mut open_objects = 0i32;
    let mut open_arrays = 0i32;
    let mut escape_next = false;

    for ch in s.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            '\\' => escape_next = true,
            '"' => open_strings = 1 - open_strings,
            '{' if open_strings == 0 => open_objects += 1,
            '}' if open_strings == 0 => open_objects -= 1,
            '[' if open_strings == 0 => open_arrays += 1,
            ']' if open_strings == 0 => open_arrays -= 1,
            _ => {}
        }
    }

    let mut result = s.to_string();
    // Close unclosed string
    if open_strings == 1 {
        result.push('"');
    }
    // Close unclosed arrays
    for _ in 0..open_arrays.max(0) {
        result.push(']');
    }
    // Close unclosed objects
    for _ in 0..open_objects.max(0) {
        result.push('}');
    }
    result
}

/// Fix trailing commas: {"a": 1,} → {"a": 1}
/// Common LLM mistake, especially in multi-command plans.
fn fix_trailing_commas(s: &str) -> String {
    s.replace(",}", "}")
        .replace(",]", "]")
        .replace(", }", "}")
        .replace(", ]", "]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trailing_commas() {
        let input = r#"{"commands": [{"type": "run", "command": "echo"},]}"#;
        let fixed = fix_trailing_commas(input);
        assert!(serde_json::from_str::<serde_json::Value>(&fixed).is_ok());
    }

    #[test]
    fn test_rust_doc_comments() {
        let input = r#"{"content": "/// Add two\n/// numbers\nfn add(a: i32)"}"#;
        let fixed = fix_rust_doc_comments(input);
        assert!(!fixed.contains("///"));
        assert!(fixed.contains("//"));
    }

    #[test]
    fn test_truncated_json() {
        let input = r#"{"commands": [{"type": "run", "command": "echo"}"#;
        let fixed = fix_truncated_json(input);
        assert!(fixed.ends_with("]}"));
    }

    #[test]
    fn test_full_sanitize() {
        let input = r#"{"commands": [{"type": "write_file", "path": "lib.rs", "content": "/// Doc\nfn x() {}"},]}"#;
        let fixed = sanitize_llm_json(input);
        assert!(serde_json::from_str::<serde_json::Value>(&fixed).is_ok());
    }
}
