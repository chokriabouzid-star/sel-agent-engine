//! LLM response protocol parser.
//!
//! Parses raw LLM text into validated `AgentResponse` structs,
//! handling markdown fences, alternative field names, and quirks.

use serde::{Deserialize, Serialize};
use thiserror::Error;

fn is_test_like_command(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("venv/bin/pytest")
        || t == "pytest"
        || t.starts_with("pytest ")
        || t.starts_with("python -m pytest")
        || t.starts_with("python3 -m pytest")
        || t.starts_with("venv/bin/python -m pytest")
        || t.starts_with("venv/bin/python3 -m pytest")
        || t.starts_with("cargo test")
        || t.starts_with("go test")
        || t == "npm test"
        || t.starts_with("npm test ")
        || t.starts_with("npx jest")
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("no JSON block found in LLM output")]
    NoJsonFound,
    #[error("JSON parse error: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("missing required field: `{0}`")]
    MissingField(String),
    #[error("empty commands list")]
    EmptyCommands,
}

// Raw deserialization struct (LLM JSON output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    #[serde(alias = "type")]
    pub action: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub message: String,
}

/// Fixes double-escaped sequences that strict reasoning models emit.
fn unescape_json_string(s: &str) -> String {
    // Two-level unescape: protect \\ first, then unescape \n and \",
    // then restore \\. This correctly handles:
    //   \n  -> newline   (double-escaped newline from reasoning models)
    //   \"  -> "         (double-escaped quote from reasoning models)
    //   \\" -> \"        (intentional escape in target code like Go)
    const P: &str = "\x00\x01\x00";
    s.replace("\\\\", P)
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
        .replace(P, "\\")
}

impl AgentCommand {
    /// Convert raw command to typed Cmd
    pub fn into_cmd(self) -> Option<Cmd> {
        match self.action.as_str() {
            "run" => {
                let trimmed = self.command.trim().to_string();
                if is_test_like_command(&trimmed) {
                    Some(Cmd::RunTests { target: trimmed })
                } else {
                    Some(Cmd::Run {
                        command: self.command,
                    })
                }
            }
            "write_file" | "write" => Some(Cmd::WriteFile {
                path: self.path,
                content: unescape_json_string(&self.content),
            }),
            "append_file" | "append" => Some(Cmd::AppendFile {
                path: self.path,
                content: unescape_json_string(&self.content),
            }),
            "delete_file" | "delete" => Some(Cmd::DeleteFile { path: self.path }),
            "patch_file" | "patch" => Some(Cmd::PatchFile {
                path: self.path,
                search: unescape_json_string(&self.search),
                replace: unescape_json_string(&self.replace),
            }),
            "read_file" | "read" => Some(Cmd::ReadFile { path: self.path }),
            "mkdir" => Some(Cmd::Mkdir { path: self.path }),
            "run_tests" | "test" => Some(Cmd::RunTests {
                target: if self.target.is_empty() {
                    self.command.clone()
                } else {
                    self.target
                },
            }),
            "done" | "finish" | "complete" => Some(Cmd::Done {
                message: self.message,
            }),
            _ => None,
        }
    }
}

// Typed execution command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
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
    /// Short description for logging
    pub fn label(&self) -> String {
        match self {
            Cmd::Run { command } => {
                format!("run: {}", command.chars().take(60).collect::<String>())
            }
            Cmd::WriteFile { path, .. } => format!("write_file: {}", path),
            Cmd::AppendFile { path, .. } => format!("append_file: {}", path),
            Cmd::DeleteFile { path } => format!("delete_file: {}", path),
            Cmd::PatchFile { path, .. } => format!("patch_file: {}", path),
            Cmd::ReadFile { path } => format!("read_file: {}", path),
            Cmd::Mkdir { path } => format!("mkdir: {}", path),
            Cmd::RunTests { target } => format!("run_tests: {}", target),
            Cmd::Done { message } => {
                if message.is_empty() {
                    "done".to_string()
                } else {
                    format!("done: {}", message.chars().take(60).collect::<String>())
                }
            }
        }
    }

    /// Simple hash for deduplication
    pub fn hash(&self) -> u64 {
        let s = self.label();
        s.bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))
    }

    pub fn is_run_tests(&self) -> bool {
        matches!(self, Cmd::RunTests { .. })
    }

    pub fn is_done(&self) -> bool {
        matches!(self, Cmd::Done { .. })
    }

    pub fn is_write_file(&self) -> bool {
        matches!(self, Cmd::WriteFile { .. })
    }

    pub fn is_patch_file(&self) -> bool {
        matches!(self, Cmd::PatchFile { .. })
    }
}

//  AgentResponse

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponseRaw {
    #[serde(default)]
    #[allow(dead_code)]
    pub plan: String,
    pub commands: Vec<AgentCommand>,
}

#[derive(Debug, Clone)]
pub struct AgentResponse {
    #[allow(dead_code)]
    pub plan: String,
    pub commands: Vec<Cmd>,
}

//  Parser

pub fn parse(raw: &str) -> Result<AgentResponse, ProtocolError> {
    let json_str = extract_json_block(raw)?;
    let resp: AgentResponseRaw = if let Ok(r) = serde_json::from_str(&json_str) {
        r
    } else {
        let normalized = normalize_field_names(&json_str);
        serde_json::from_str(&normalized)?
    };
    validate(resp)
}

fn validate(raw: AgentResponseRaw) -> Result<AgentResponse, ProtocolError> {
    if raw.commands.is_empty() {
        return Err(ProtocolError::EmptyCommands);
    }
    for (i, cmd) in raw.commands.iter().enumerate() {
        if cmd.action.is_empty() {
            return Err(ProtocolError::MissingField(format!(
                "commands[{}].action",
                i
            )));
        }
    }
    let commands: Vec<Cmd> = raw
        .commands
        .into_iter()
        .filter_map(|c| c.into_cmd())
        .collect();

    if commands.is_empty() {
        return Err(ProtocolError::EmptyCommands);
    }
    Ok(AgentResponse {
        plan: raw.plan,
        commands,
    })
}

/// Ensure run_tests appears before done
pub fn validate_test_order(resp: &mut AgentResponse) -> Result<(), String> {
    let has_run_tests = resp.commands.iter().any(|c| c.is_run_tests());
    let has_done = resp.commands.iter().any(|c| c.is_done());

    if has_done && !has_run_tests {
        // Add run_tests before done
        let done_pos = resp
            .commands
            .iter()
            .position(|c| c.is_done())
            .unwrap_or(resp.commands.len());
        resp.commands.insert(
            done_pos,
            Cmd::RunTests {
                target: "auto".to_string(),
            },
        );
        return Err("Plan missing run_tests before done  auto-injected".to_string());
    }

    // Check order: run_tests must precede done
    if has_run_tests && has_done {
        let tests_pos = resp
            .commands
            .iter()
            .position(|c| c.is_run_tests())
            .unwrap_or(0);
        let done_pos = resp.commands.iter().position(|c| c.is_done()).unwrap_or(0);
        if tests_pos > done_pos {
            return Err("run_tests appears AFTER done  fix the order".to_string());
        }
    }

    Ok(())
}

fn extract_json_block(raw: &str) -> Result<String, ProtocolError> {
    if let Some(start) = raw.find("```json") {
        let after = &raw[start + 7..];
        if let Some(end) = after.find("```") {
            return Ok(after[..end].trim().to_string());
        }
    }
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            if end > start {
                return Ok(raw[start..=end].to_string());
            }
        }
    }
    Err(ProtocolError::NoJsonFound)
}

fn normalize_field_names(json: &str) -> String {
    json.replace("\"reasoning\"", "\"plan\"")
        .replace("\"actions\"", "\"commands\"")
        .replace("\"steps\"", "\"commands\"")
        .replace("\"file\"", "\"path\"")
        .replace("\"filename\"", "\"path\"")
        .replace("\"cmd\"", "\"command\"")
        .replace("\"type\"", "\"action\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_double_escaped_json_content() {
        // Plan format emitted by openai/gpt-oss-120b
        let input = r#"{"commands":[{"action":"write_file","path":"greeter.py","content":"def greet(name):\n    return f\"Hello {name}\"\n"}]}"#;
        let resp = parse(input).expect("parsing should succeed");
        assert_eq!(resp.commands.len(), 1);
        if let Cmd::WriteFile { content, .. } = &resp.commands[0] {
            assert!(content.contains('\n'));
            assert!(content.contains("f\"Hello {name}\""));
        } else {
            panic!("expected WriteFile command");
        }
    }

    #[test]
    fn test_parse_clean_json() {
        let input =
            r#"{"plan":"fix","commands":[{"action":"write_file","path":"m.go","content":"x"}]}"#;
        let resp = parse(input).expect("test setup/use should succeed");
        assert_eq!(resp.commands.len(), 1);
        assert!(resp.commands[0].is_write_file());
    }

    #[test]
    fn test_parse_markdown_fenced() {
        let input = "Fix:\n```json\n{\"plan\":\"x\",\"commands\":[{\"action\":\"run\",\"command\":\"go test\"}]}\n```\n";
        let resp = parse(input).expect("test setup/use should succeed");
        // go test is a test command: canonicalized to RunTests by is_test_like_command
        assert!(matches!(resp.commands[0], Cmd::RunTests { .. }));
    }

    #[test]
    fn test_parse_run_tests_and_done() {
        let input = r#"{"plan":"x","commands":[{"action":"run_tests","target":"cargo test"},{"action":"done","message":"ok"}]}"#;
        let resp = parse(input).expect("test setup/use should succeed");
        assert!(resp.commands.iter().any(|c| c.is_run_tests()));
        assert!(resp.commands.iter().any(|c| c.is_done()));
    }

    #[test]
    fn test_validate_test_order_injects_run_tests() {
        let mut resp = AgentResponse {
            plan: String::new(),
            commands: vec![Cmd::Done {
                message: String::new(),
            }],
        };
        let result = validate_test_order(&mut resp);
        assert!(result.is_err()); //
        assert_eq!(resp.commands.len(), 2);
        assert!(resp.commands[0].is_run_tests());
    }

    #[test]
    fn test_empty_commands_error() {
        let input = r#"{"plan":"x","commands":[]}"#;
        assert!(matches!(parse(input), Err(ProtocolError::EmptyCommands)));
    }

    #[test]
    fn test_no_json_error() {
        assert!(matches!(
            parse("no json here"),
            Err(ProtocolError::NoJsonFound)
        ));
    }

    #[test]
    fn test_cmd_label_and_hash() {
        let c = Cmd::WriteFile {
            path: "a.rs".into(),
            content: "x".into(),
        };
        assert!(c.label().contains("a.rs"));
        assert!(c.hash() > 0);
        assert!(c.is_write_file());
    }
}
