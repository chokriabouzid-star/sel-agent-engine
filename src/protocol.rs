// src/protocol.rs — v1.3: JSON Protocol

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════
// Schema
// ══════════════════════════════════════════════════════

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub version: String,
    pub commands: Vec<Cmd>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cmd {
    Run {
        command: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    AppendFile {
        path: String,
        content: String,
    },
    DeleteFile {
        path: String,
    },
    PatchFile {
        path: String,
        #[serde(default)]
        search: String,
        replace: String,
    },
    ReadFile {
        path: String,
    },
    Mkdir {
        path: String,
    },
    RunTests {
        target: String,
    },
    Done {
        #[serde(default)]
        message: String,
    },
}

impl Cmd {
    pub fn label(&self) -> String {
        match self {
            Cmd::Run { command } => format!("run: {}", &command[..command.len().min(60)]),
            Cmd::WriteFile { path, .. } => format!("write_file: {}", path),
            Cmd::AppendFile { path, .. } => format!("append_file: {}", path),
            Cmd::DeleteFile { path } => format!("delete_file: {}", path),
            Cmd::PatchFile { path, .. } => format!("patch_file: {}", path),
            Cmd::ReadFile { path } => format!("read_file: {}", path),
            Cmd::Mkdir { path } => format!("mkdir: {}", path),
            Cmd::RunTests { target } => format!("run_tests: {}", target),
            Cmd::Done { message } => format!("done: {}", message),
        }
    }
    pub fn hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        match self {
            Cmd::Run { command } => {
                "run".hash(&mut h);
                command.hash(&mut h);
            }
            Cmd::WriteFile { path, content } => {
                "write_file".hash(&mut h);
                path.hash(&mut h);
                content.hash(&mut h);
            }
            Cmd::AppendFile { path, content } => {
                "append_file".hash(&mut h);
                path.hash(&mut h);
                content.hash(&mut h);
            }
            Cmd::DeleteFile { path } => {
                "delete_file".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::PatchFile {
                path,
                search,
                replace,
            } => {
                "patch_file".hash(&mut h);
                path.hash(&mut h);
                search.hash(&mut h);
                replace.hash(&mut h);
            }
            Cmd::ReadFile { path } => {
                "read_file".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::Mkdir { path } => {
                "mkdir".hash(&mut h);
                path.hash(&mut h);
            }
            Cmd::RunTests { target } => {
                "run_tests".hash(&mut h);
                target.hash(&mut h);
            }
            Cmd::Done { .. } => {
                "done".hash(&mut h);
            }
        }
        format!("{:x}", h.finish())
    }
    pub fn is_done(&self) -> bool {
        matches!(self, Cmd::Done { .. })
    }
    pub fn is_run_tests(&self) -> bool {
        matches!(self, Cmd::RunTests { .. })
    }
    pub fn is_write_file(&self) -> bool {
        matches!(self, Cmd::WriteFile { .. } | Cmd::AppendFile { .. })
    }
    pub fn is_delete_file(&self) -> bool {
        matches!(self, Cmd::DeleteFile { .. })
    }
    pub fn is_patch_file(&self) -> bool {
        matches!(self, Cmd::PatchFile { .. })
    }
}

// ══════════════════════════════════════════════════════
// Parser
// ══════════════════════════════════════════════════════

pub fn extract_json(text: &str) -> Option<&str> {
    let marker = "```json";
    if let Some(s) = text.find(marker) {
        let rest = text[s + marker.len()..].trim_start_matches('\n');
        if let Some(e) = rest.find("```") {
            return Some(rest[..e].trim());
        }
    }
    // Fallback: try finding outermost brackets if markdown tags are omitted
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                return Some(&text[start..=end]);
            }
        }
    }
    None
}

/// يصلح escape sequences غير الصالحة في JSON التي يولدها LLM
/// مثال: \' → ' و \` → `
fn fix_json_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 64);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                let next = chars[i + 1];
                match next {
                    // valid JSON escapes — keep both chars
                    '"' | '\\' | '/' | 'n' | 'r' | 't' | 'b' | 'f' | 'u' => {
                        result.push('\\');
                        result.push(next);
                        i += 2;
                        continue;
                    }
                    // invalid escape — drop backslash, keep char only
                    _ => {
                        result.push(next);
                        i += 2;
                        continue;
                    }
                }
            } else if c == '\n' {
                result.push('\\');
                result.push('n');
            } else if c == '\r' {
                result.push('\\');
                result.push('r');
            } else if c == '\t' {
                result.push('\\');
                result.push('t');
            } else {
                if c == '"' {
                    in_string = false;
                }
                result.push(c);
            }
        } else {
            if c == '"' {
                in_string = true;
            }
            result.push(c);
        }

        i += 1;
    }
    result
}

/// v7.4: Aggressive sanitization for content fields containing broken code
fn sanitize_content_fields(json_str: &str) -> String {
    let mut result = String::with_capacity(json_str.len() + 64);
    let chars: Vec<char> = json_str.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut in_content_field = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                result.push(c);
                result.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
                in_content_field = false;
                result.push(c);
                i += 1;
                continue;
            }
            // Inside content field — sanitize problematic characters
            if in_content_field {
                // Control characters (except already-escaped ones)
                if c == '\t' {
                    result.push_str("\\t");
                    i += 1;
                    continue;
                }
            }
            result.push(c);
        } else {
            if c == '"' {
                in_string = true;
                // Check if this is a content/search/replace field
                let prefix: String = result.chars().rev().take(20).collect::<String>().chars().rev().collect();
                if prefix.contains("\"content\":") || prefix.contains("\"search\":") || prefix.contains("\"replace\":") {
                    in_content_field = true;
                }
            }
            result.push(c);
        }

        i += 1;
    }
    result
}

pub fn parse(response: &str) -> Result<Plan> {
    // Phase 1: try normal parse
    let json = extract_json(response).ok_or_else(|| anyhow!("No ```json block found in response"))?;
    let cleaned = fix_json_escapes(json);
    
    // Try Phase 1: Direct Plan parse
    if let Ok(plan) = serde_json::from_str::<Plan>(&cleaned) {
        return Ok(plan);
    }
    
    // Try Phase 2: Bare list of commands
    if let Ok(commands) = serde_json::from_str::<Vec<Cmd>>(&cleaned) {
        eprintln!("[TRACE] parse: detected bare command list");
        return Ok(Plan { version: "1.0".into(), commands });
    }

    // Try Phase 3: Map with commands but no version
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        if let Some(cmds_val) = val.get("commands") {
            if let Ok(commands) = serde_json::from_value::<Vec<Cmd>>(cmds_val.clone()) {
                eprintln!("[TRACE] parse: recovered plan without version field");
                return Ok(Plan { version: "1.0".into(), commands });
            }
        }
    }

    // Phase 4: try with content field sanitization (for control characters)
    let sanitized = sanitize_content_fields(&cleaned);
    if let Ok(plan) = serde_json::from_str::<Plan>(&sanitized) {
        eprintln!("[TRACE] parse: succeeded with content sanitization");
        return Ok(plan);
    }

    // Phase 5: Final attempt — try to recover by stringifying any accidental objects in string fields
    // This handles the "invalid type: map, expected a string" error
    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        if let Some(cmds) = val.get_mut("commands").and_then(|c| c.as_array_mut()) {
            for cmd in cmds {
                if let Some(obj) = cmd.as_object_mut() {
                    // List of fields that MUST be strings
                    let string_fields = ["command", "content", "path", "search", "replace", "target", "message"];
                    for field in string_fields {
                        if let Some(f_val) = obj.get_mut(field) {
                            if f_val.is_object() || f_val.is_array() {
                                let cmd_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
                                eprintln!("[TRACE] parse: stringifying accidental {} object in field '{}'", cmd_type, field);
                                *f_val = serde_json::Value::String(f_val.to_string());
                            }
                        }
                    }
                }
            }
            if let Ok(plan) = serde_json::from_value::<Plan>(val) {
                return Ok(plan);
            }
        }
    }
    
    // Phase 6: original error for diagnostics
    serde_json::from_str(&cleaned).map_err(|e| {
        anyhow!("JSON parse error: {}\n---\n{}", e, {
            let start = json.len().min(600).saturating_sub(50);
            let end = json.len().min(800);
            &json[start..end]
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan() {
        let r = r#"
Some text
```json
{"version":"1.0","commands":[
  {"type":"run","command":"echo hi"},
  {"type":"write_file","path":"a.py","content":"x=1"},
  {"type":"run_tests","target":"a.py"},
  {"type":"done","message":"ok"}
]}
```"#;
        let p = parse(r).unwrap();
        assert_eq!(p.commands.len(), 4);
        assert!(p.commands[0].label().contains("echo hi"));
        assert!(p.commands.last().unwrap().is_done());
    }

    #[test]
    fn fails_without_block() {
        assert!(parse("no json here").is_err());
    }
}

/// Ensures test files are written before RunTests is called.
pub fn validate_test_order(plan: &Plan) -> Result<(), String> {
    let has_run_tests = plan
        .commands
        .iter()
        .any(|c| matches!(c, Cmd::RunTests { .. }));
    if !has_run_tests {
        return Ok(());
    }

    let test_pos = plan.commands.iter().position(|c| match c {
        Cmd::WriteFile { path, .. } => path.contains("test"),
        _ => false,
    });
    let run_pos = plan
        .commands
        .iter()
        .position(|c| matches!(c, Cmd::RunTests { .. }));

    match (test_pos, run_pos) {
        (Some(t), Some(r)) if t < r => Ok(()),
        (None, _) => Err("Plan calls RunTests but writes no test file".into()),
        _ => Err("Test file must be written before RunTests".into()),
    }
}
