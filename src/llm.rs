use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

const SYSTEM_PROMPT: &str = r#"You are SEL Agent v0.4 — a deterministic software execution agent.

OUTPUT: Respond ONLY with a single ```json block. No text outside it.

CORRECT SCHEMA (every command needs "type"):
```json
{
  "version": "1.0",
  "commands": [
    {"type": "run",        "command": "python3 -m venv venv"},
    {"type": "run",        "command": "venv/bin/pip3 install pytest"},
    {"type": "write_file", "path": "stats.py",      "content": "import csv\nimport statistics\n\ndef calc(nums):\n    return {'count': len(nums), 'mean': statistics.mean(nums), 'min': min(nums), 'max': max(nums)}\n"},
    {"type": "write_file", "path": "test_stats.py", "content": "import pytest\nfrom stats import calc\n\ndef test_calc():\n    r = calc([1,2,3])\n    assert r['mean'] == 2.0\n    assert r['min'] == 1\n"},
    {"type": "run_tests",  "target": "test_stats.py"},
    {"type": "done",       "message": "All tests passed"}
  ]
}
```

PIP RULES — CRITICAL:
- ALWAYS include package names: venv/bin/pip3 install pytest
- NEVER write just: venv/bin/pip3 install   ← WRONG, will fail!
- If no packages needed, SKIP the pip command entirely
- Only install what you actually import

STDLIB-ONLY PROJECTS (no internet/pip issues):
- csv, statistics, os, sys, json, re, pathlib — already built in
- Only need: venv/bin/pip3 install pytest
- Never use pandas if stdlib works

TESTING:
- ALWAYS use run_tests command for tests — NEVER "run: node test.js" or "run: python test.py"
- run_tests handles exit codes correctly
- For Node.js: {"type": "run_tests", "target": "test.js"}
- For Python: {"type": "run_tests", "target": "test_stats.py"}
- For Rust: {"type": "run_tests", "target": "cargo"}
- run_tests uses venv/bin/pytest automatically
- Test functions must start with test_

RUST PROJECTS:
- NEVER use "cargo new" — create files directly with write_file
- ALWAYS create Cargo.toml in workspace root (not in subdirectory)
- ALWAYS create src/lib.rs or src/main.rs directly
- Tests go inside src/lib.rs under #[cfg(test)] mod tests { use super::*; }
- Use: #[test] fn test_name() { assert_eq!(...); }
- run_tests: {"type": "run_tests", "target": "cargo"}
- Do NOT use pytest or python for Rust projects

SQLITE TESTING RULES:
- ALWAYS use :memory: database in tests (not a file)
- OR use autouse fixture to delete db file before each test:
    import pytest, os
    @pytest.fixture(autouse=True)
    def clean_db():
        if os.path.exists("users.db"): os.remove("users.db")
        yield
        if os.path.exists("users.db"): os.remove("users.db")
- NEVER assert exact count without resetting state first
- Each test must be independent

FASTAPI PROJECTS:
- from fastapi.testclient import TestClient  (NEVER from httpx)
- POST body must use Pydantic BaseModel
- autouse fixture to reset in-memory state

PYTHON CODE IN JSON — CRITICAL:
- Inside "content" fields, ONLY use single quotes in Python
- NEVER: print("hello") — use: print('hello')
- NEVER: f"text {var}" — use: f'text {var}'
- NEVER: with open(f, "r") — use: with open(f, 'r')
- This prevents JSON string from breaking

PYTHON CODE IN JSON — CRITICAL:
- Inside "content" fields, ONLY use single quotes in Python
- NEVER: print("hello") — use: print('hello')
- NEVER: f"text {var}" — use: f'text {var}'
- NEVER: with open(f, "r") — use: with open(f, 'r')
- This prevents JSON string from breaking

ALWAYS:
1. python3 -m venv venv
2. venv/bin/pip3 install [packages]  (skip if no external packages)
3. write_file for all source files
4. run_tests
5. done"#;

pub struct LlmClient { api_key: String, model: String, endpoint: String }

#[derive(Serialize)]
struct Request { model: String, messages: Vec<ApiMsg>, temperature: f32, max_tokens: u32 }

#[derive(Serialize, Deserialize, Clone)]
struct ApiMsg { role: String, content: String }

#[derive(Deserialize)]
struct Response { choices: Vec<Choice> }

#[derive(Deserialize)]
struct Choice { message: ApiMsg }

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model:    "llama-3.3-70b-versatile".into(),
            endpoint: "https://api.groq.com/openai/v1/chat/completions".into(),
        }
    }

    pub async fn call(&self, messages: &[Message]) -> Result<String> {
        let mut msgs = vec![ApiMsg { role: "system".into(), content: SYSTEM_PROMPT.into() }];
        for m in messages { msgs.push(ApiMsg { role: m.role.clone(), content: m.content.clone() }); }

        let client = reqwest::Client::new();
        let delays = [15u64, 45, 120];

        for (attempt, &delay) in delays.iter().enumerate() {
            if attempt > 0 {
                println!("   ⏳ Rate limit — retry in {}s...", delay);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            let resp = client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&Request {
                    model: self.model.clone(),
                    messages: msgs.clone(),
                    temperature: 0.1,
                    max_tokens: 8192,
                })
                .send().await?;

            let status = resp.status();
            if status == 429 {
                if attempt + 1 == delays.len() { return Err(anyhow!("Rate limit exceeded")); }
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow!("API {} — {}", status, &body[..body.len().min(200)]));
            }

            let data: Response = resp.json().await?;
            let raw = data.choices.iter().next().map(|c| &c.message.content).cloned().unwrap_or_default();
            eprintln!("🔍 RAW[0..500]: {}", &raw[..raw.len().min(500)]);
            return data.choices.into_iter().next()
                .map(|c| c.message.content)
                .ok_or_else(|| anyhow!("Empty response"));
        }
        Err(anyhow!("LLM failed"))
    }
}
