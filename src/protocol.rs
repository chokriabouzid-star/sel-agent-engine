// src/protocol.rs — v1.3: JSON Protocol

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════
// Schema
// ══════════════════════════════════════════════════════

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default)]
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

/// Strip <think>...</think> blocks that Qwen3/DeepSeek models prepend
fn strip_think_blocks(text: &str) -> &str {
    // Handle <think>...</think> (greedy — strip all thinking blocks)
    if let Some(end) = text.rfind("</think>") {
        let after = &text[end + 8..];
        // Return everything after the last </think>
        return after.trim_start();
    }
    text
}

/// استخراج JSON من النص مع bracket counting صحيح
/// يتعامل مع { و } داخل strings بشكل صحيح
pub fn extract_json(text: &str) -> Option<&str> {
    let clean = strip_think_blocks(text);

    // ── حالة 1: ```json ... ``` ────────────────────────
    let marker = "```json";
    if let Some(s) = clean.find(marker) {
        let rest = &clean[s + marker.len()..];
        let rest = rest.trim_start_matches('\n');
        if let Some(e) = rest.find("```") {
            eprintln!("[TRACE] extract_json: found ```json block");
            return Some(rest[..e].trim());
        }
    }

    // ── حالة 2: bracket counting (يحل مشكلة } داخل content) ──
    if let Some(start) = clean.find('{') {
        let bytes = clean.as_bytes();
        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;

        for (i, &b) in bytes[start..].iter().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            match b {
                b'\\' if in_string => { escaped = true; }
                b'"'               => { in_string = !in_string; }
                b'{' if !in_string => { depth += 1; }
                b'}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i);
                        break;
                    }
                }
                _ => {}
            }
        }

        if let Some(e) = end {
            let json = clean[start..=e].trim();
            eprintln!("[TRACE] extract_json: bracket counting, length={}", json.len());
            return Some(json);
        } else {
            eprintln!("[TRACE] extract_json: bracket depth didn't close properly");
        }
    }

    eprintln!("[TRACE] extract_json: no valid JSON found in text");
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

/// v7.5: Aggressive sanitization for content fields containing broken code
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
                let prefix: String = result
                    .chars()
                    .rev()
                    .take(20)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                if prefix.contains("\"content\":")
                    || prefix.contains("\"search\":")
                    || prefix.contains("\"replace\":")
                {
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
    // Phase 0: strip thinking blocks
    let stripped = strip_think_blocks(response);
    
    // Phase 1: try normal parse
    let json =
        extract_json(stripped).ok_or_else(|| anyhow!("No ```json block found in response"))?;
    let cleaned = fix_json_escapes(json);
    // v7.9.8: Apply JSON sanitizer (fixes /// docs, trailing commas, truncated JSON)
    let cleaned = crate::llm::json_sanitizer::sanitize_llm_json(&cleaned);

    // Log first 200 chars for debugging
    eprintln!("[TRACE] parse: extracted JSON (first 200): {}", &cleaned[..cleaned.len().min(200)]);

    // Try Phase 1: Direct Plan parse
    if let Ok(plan) = serde_json::from_str::<Plan>(&cleaned) {
        return Ok(plan);
    }

    // Try Phase 2: Bare list of commands
    if let Ok(commands) = serde_json::from_str::<Vec<Cmd>>(&cleaned) {
        eprintln!("[TRACE] parse: detected bare command list");
        return Ok(Plan {
            version: "1.0".into(),
            commands,
        });
    }

    // Phase 2.5: Single command object -> wrap in commands array
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        if val.is_object() {
            let obj = val.as_object().unwrap();
            
            // Detect single command: has "command"/"op"/"action"/"type" but NOT "commands"/"plan"/"steps"
            let has_cmd_field = obj.contains_key("command")
                || obj.contains_key("op")
                || obj.contains_key("action")
                || obj.contains_key("type");
            let has_array_field = obj.contains_key("commands")
                || obj.contains_key("plan")
                || obj.contains_key("steps");
            
            if has_cmd_field && !has_array_field {
                eprintln!("[TRACE] parse: Phase 2.5 — single command object, wrapping");

                // normalize: command/op/action → type
                let mut normalized = obj.clone();
                if !normalized.contains_key("type") {
                    if let Some(v) = normalized.remove("command")
                        .or_else(|| normalized.remove("op"))
                        .or_else(|| normalized.remove("action"))
                    {
                        normalized.insert("type".to_string(), v);
                    }
                }
                // normalize: file/filename → path
                if !normalized.contains_key("path") {
                    if let Some(v) = normalized.remove("file")
                        .or_else(|| normalized.remove("filename"))
                    {
                        normalized.insert("path".to_string(), v);
                    }
                }

                let wrapped = serde_json::json!({
                    "version": "1.0",
                    "commands": [normalized]
                });

                match serde_json::from_value::<Plan>(wrapped) {
                    Ok(plan) => {
                        eprintln!("[TRACE] parse: Phase 2.5 succeeded");
                        return Ok(plan);
                    }
                    Err(e) => {
                        eprintln!("[TRACE] parse: Phase 2.5 failed: {}", e);
                        // استمر للـ phases التالية
                    }
                }
            }
        }
    }

    // Try Phase 3: Normalize alternative field names from various models
    // Gemini uses "plan"/"op", others may use "steps"/"action"
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        // Normalize top-level: "plan"/"steps" → "commands"
        let cmds_val = val.get("commands")
            .or_else(|| val.get("plan"))
            .or_else(|| val.get("steps"))
            .or_else(|| val.get("actions"))
            .cloned()
            .or_else(|| if val.is_array() { Some(val.clone()) } else { None });

        if let Some(serde_json::Value::Array(arr)) = cmds_val {
            // Normalize each command: "op"/"action" → "type"
            let normalized: Vec<serde_json::Value> = arr.into_iter().map(|mut cmd| {
                if let Some(obj) = cmd.as_object_mut() {
                    // Normalize nested format: {"write_file": {"path": "...", "content": "..."}}
                    if obj.len() == 1 {
                        let key = obj.keys().next().unwrap().clone();
                        if ["write_file", "patch_file", "run_tests", "run", "done"].contains(&key.as_str()) {
                            if let Some(serde_json::Value::Object(inner)) = obj.remove(&key) {
                                for (k, v) in inner {
                                    obj.insert(k, v);
                                }
                                obj.insert("type".to_string(), serde_json::json!(key));
                            }
                        }
                    }

                    // Rename "op" or "action" to "type" if "type" is missing
                    if !obj.contains_key("type") {
                        if let Some(op_val) = obj.remove("op").or_else(|| obj.remove("action")).or_else(|| obj.remove("command")) {
                            obj.insert("type".to_string(), op_val);
                        }
                    }
                    // Normalize "filename"/"file" → "path" for write_file
                    if !obj.contains_key("path") {
                        if let Some(p) = obj.remove("filename").or_else(|| obj.remove("file")) {
                            obj.insert("path".to_string(), p);
                        }
                    }
                    // Normalize "cmd"/"script" → "command" for run
                    if !obj.contains_key("command") {
                        if let Some(c) = obj.remove("cmd").or_else(|| obj.remove("script")) {
                            obj.insert("command".to_string(), c);
                        }
                    }
                    // Normalize patch_file with content but no search -> write_file
                    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                        if t == "patch_file" && !obj.contains_key("search") && obj.contains_key("content") {
                            obj.insert("type".to_string(), serde_json::json!("write_file"));
                            // No need to rename content to replace, write_file expects content!
                        } else if t == "patch_file" && !obj.contains_key("replace") {
                            if let Some(c) = obj.remove("content") {
                                obj.insert("replace".to_string(), c);
                            }
                        }
                    }
                    // v7.9.6: Normalize "install" action → "run"
                    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                        if t == "install" {
                            let dep_list = if let Some(deps) = obj.remove("dependencies").and_then(|v| v.as_array().map(|a| a.clone())) {
                                deps.iter().filter_map(|d| d.as_str()).collect::<Vec<_>>().join(" ")
                            } else if let Some(target) = obj.remove("target").and_then(|v| v.as_str().map(|s| s.to_string())) {
                                target
                            } else if let Some(pkg) = obj.remove("package").and_then(|v| v.as_str().map(|s| s.to_string())) {
                                pkg
                            } else {
                                String::new()
                            };
                            if !dep_list.is_empty() {
                                obj.insert("type".to_string(), serde_json::json!("run"));
                                obj.insert("command".to_string(), serde_json::json!(format!("npm install {}", dep_list)));
                            }
                        } else if t == "run_tests" {
                            // Normalize "command" -> "target" for run_tests
                            if !obj.contains_key("target") {
                                if let Some(c) = obj.remove("command").or_else(|| obj.remove("cmd")) {
                                    obj.insert("target".to_string(), c);
                                }
                            }
                        }
                    }
                }
                cmd
            }).collect();

            let plan_val = serde_json::json!({
                "version": "1.0",
                "commands": normalized
            });

            if let Ok(plan) = serde_json::from_value::<Plan>(plan_val) {
                eprintln!("[TRACE] parse: recovered via field normalization (plan/op → commands/type)");
                return Ok(plan);
            }
        }

        // Original Phase 3 fallback: "commands" key exists but no "version"
        if let Some(cmds_val) = val.get("commands") {
            if let Ok(commands) = serde_json::from_value::<Vec<Cmd>>(cmds_val.clone()) {
                eprintln!("[TRACE] parse: recovered plan without version field");
                return Ok(Plan {
                    version: "1.0".into(),
                    commands,
                });
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
                    let cmd_type = obj
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    // List of fields that MUST be strings
                    let string_fields = [
                        "command", "content", "path", "search", "replace", "target", "message",
                    ];
                    for field in string_fields {
                        if let Some(f_val) = obj.get_mut(field) {
                            if f_val.is_object() || f_val.is_array() {
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

    #[test]
    fn rejects_plan_without_run_tests() {
        let mut plan = Plan {
            version: "1.0".into(),
            commands: vec![
                Cmd::WriteFile {
                    path: "a.py".into(),
                    content: "x=1".into(),
                },
                Cmd::Done {
                    message: "done".into(),
                },
            ],
        };
        assert!(validate_test_order(&mut plan).is_ok());
        // Verify it was auto-appended
        assert!(matches!(plan.commands.last().unwrap(), Cmd::RunTests { .. }));
    }
}

/// Ensures test files are written before RunTests is called.
pub fn validate_test_order(plan: &mut Plan) -> Result<(), String> {
    if plan.commands.is_empty() {
        return Err("PLAN ERROR: Plan is empty. You must include commands.".into());
    }

    let has_work = plan.commands.iter().any(|c| {
        matches!(c, Cmd::WriteFile { .. } | Cmd::PatchFile { .. } | Cmd::Run { .. })
    });
    if !has_work {
        return Err("PLAN ERROR: Useless plan. You must use write_file, patch_file, or run.".into());
    }

    let has_run_tests = plan
        .commands
        .iter()
        .any(|c| matches!(c, Cmd::RunTests { .. }));
    if !has_run_tests {
        // AutoFix instead of returning an error, this saves API calls and avoids confusing the LLM!
        plan.commands.push(Cmd::RunTests { target: "auto".to_string() });
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
        (None, Some(_)) => Ok(()),
        _ => Err("If a test file is written, it must be before RunTests".into()),
    }
}
