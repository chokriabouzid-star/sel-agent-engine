// src/protocol.rs — v1.3: JSON Protocol

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════
// Schema
// ══════════════════════════════════════════════════════

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub version:  String,
    pub commands: Vec<Cmd>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cmd {
    Run       { command: String },
    WriteFile { path: String, content: String },
    AppendFile{ path: String, content: String },
    ReadFile  { path: String },
    Mkdir     { path: String },
    RunTests  { target: String },
    Done      { #[serde(default)] message: String },
}

impl Cmd {
    pub fn label(&self) -> String {
        match self {
            Cmd::Run       { command }    => format!("run: {}", &command[..command.len().min(60)]),
            Cmd::WriteFile { path, .. }   => format!("write_file: {}", path),
            Cmd::AppendFile{ path, .. }   => format!("append_file: {}", path),
            Cmd::ReadFile  { path }       => format!("read_file: {}", path),
            Cmd::Mkdir     { path }       => format!("mkdir: {}", path),
            Cmd::RunTests  { target }     => format!("run_tests: {}", target),
            Cmd::Done      { message }    => format!("done: {}", message),
        }
    }
    pub fn hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        match self {
            Cmd::Run       { command }         => { "run".hash(&mut h);        command.hash(&mut h); }
            Cmd::WriteFile { path, content }   => { "write_file".hash(&mut h); path.hash(&mut h); content.hash(&mut h); }
            Cmd::AppendFile{ path, content }   => { "append_file".hash(&mut h);path.hash(&mut h); content.hash(&mut h); }
            Cmd::ReadFile  { path }            => { "read_file".hash(&mut h);  path.hash(&mut h); }
            Cmd::Mkdir     { path }            => { "mkdir".hash(&mut h);      path.hash(&mut h); }
            Cmd::RunTests  { target }          => { "run_tests".hash(&mut h);  target.hash(&mut h); }
            Cmd::Done      { .. }              => { "done".hash(&mut h); }
        }
        format!("{:x}", h.finish())
    }
    pub fn is_done(&self)     -> bool { matches!(self, Cmd::Done { .. }) }
    pub fn is_run_tests(&self)-> bool { matches!(self, Cmd::RunTests { .. }) }
    pub fn is_write_file(&self) -> bool { matches!(self, Cmd::WriteFile { .. } | Cmd::AppendFile { .. }) }
}

// ══════════════════════════════════════════════════════
// Parser
// ══════════════════════════════════════════════════════

/// استخرج ```json block من رد LLM
pub fn extract_json(text: &str) -> Option<&str> {
    let marker = "```json";
    let s = text.find(marker)? + marker.len();
    let rest = text[s..].trim_start_matches('\n');
    let e = rest.find("```")?;
    Some(rest[..e].trim())
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
                result.push('\\'); result.push('n');
            } else if c == '\r' {
                result.push('\\'); result.push('r');
            } else if c == '\t' {
                result.push('\\'); result.push('t');
            } else {
                if c == '"' { in_string = false; }
                result.push(c);
            }
        } else {
            if c == '"' { in_string = true; }
            result.push(c);
        }

        i += 1;
    }
    result
}

pub fn parse(response: &str) -> Result<Plan> {
    let json = extract_json(response)
        .ok_or_else(|| anyhow!("No ```json block found in response"))?;
    let cleaned = fix_json_escapes(json);
    serde_json::from_str(&cleaned)
        .map_err(|e| anyhow!("JSON parse error: {}\n---\n{}", e, { let start = json.len().min(600).saturating_sub(50); let end = json.len().min(800); &json[start..end] }))
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
