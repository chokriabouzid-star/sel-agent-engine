#!/usr/bin/env bash
# fix_pip_repair.sh
set -e
cd ~/projects/sel-agent-v4

echo "══ 1. executor.rs: رفض pip install الفارغ"
# أضف validation قبل تنفيذ run
cat > /tmp/fix_pip.py << 'PY_EOF'
with open('src/executor.rs', 'r') as f:
    content = f.read()

# ابحث عن مكان التحقق من whitelist وأضف قبله
old = '''        if !ALLOWED.iter().any(|a| *a == *prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list", prog
            )));
        }'''

new = '''        // رفض pip install بدون package name
        if (prog.contains("pip3") || prog.contains("pip")) {
            let is_install = parts.iter().any(|p| *p == "install");
            let has_package = parts.len() > 2 && parts.iter().skip(2).any(|p| !p.starts_with('-'));
            if is_install && !has_package {
                return Ok(ExecResult::fail(
                    "pip install needs package name: e.g. venv/bin/pip3 install pytest".to_string()
                ));
            }
        }

        if !ALLOWED.iter().any(|a| *a == *prog) {
            return Ok(ExecResult::fail(format!(
                "'{}' is not in the allowed programs list", prog
            )));
        }'''

if old in content:
    content = content.replace(old, new)
    print("✓ executor.rs: pip validation")
else:
    print("⚠ executor.rs: pattern not found")
    for i, line in enumerate(content.split('\n')):
        if 'not in the allowed' in line:
            print(f"  L{i+1}: {line}")

with open('src/executor.rs', 'w') as f:
    f.write(content)
PY_EOF
python3 /tmp/fix_pip.py

echo ""
echo "══ 2. agent.rs: repair prompt يرسل الملفات (استبدال مباشر)"
# اعرض السطور الحالية حول format!
echo "السطور الحالية:"
sed -n '164,178p' src/agent.rs

# استبدل مباشرة بالسطر الدقيق
cat > /tmp/fix_agent_prompt.py << 'PY_EOF'
with open('src/agent.rs', 'r') as f:
    content = f.read()

# البحث عن format! في Repairing بالنمط الموجود فعلاً
import re

# ابحث عن كتلة format! التي تحتوي self.goal, errors
pattern = r'(                    let prompt = format!\(\s*"Goal: \{\}\\n\\nThe following steps failed:\\n\{\}\\n\\n\\s*Provide a corrected JSON plan that fixes these issues\.\\n\\s*Include ALL steps needed \(not just the fix\)\.",\s*self\.goal, errors\s*\);)'

replacement = '''                    // قراءة الملفات الحالية
                    let ws = &self.executor.workspace;
                    let file1 = std::fs::read_to_string(ws.join("main.py"))
                        .or_else(|_| std::fs::read_to_string(ws.join("cli.py")))
                        .or_else(|_| std::fs::read_to_string(ws.join("stats.py")))
                        .unwrap_or_default();
                    let file2 = std::fs::read_to_string(ws.join("test_main.py"))
                        .or_else(|_| std::fs::read_to_string(ws.join("test_cli.py")))
                        .or_else(|_| std::fs::read_to_string(ws.join("test_stats.py")))
                        .unwrap_or_default();

                    let network_note = if errors.contains("Network is unreachable") || errors.contains("Timeout after") {
                        "\\nNETWORK UNAVAILABLE: Use ONLY Python stdlib (csv, statistics, os, sys). NO pandas."
                    } else { "" };

                    let prompt = format!(
                        "Goal: {}{}\\n\\nFAILED:\\n{}\\n\\nCURRENT main file:\\n```python\\n{}\\n```\\n\\nCURRENT test file:\\n```python\\n{}\\n```\\n\\nFix ALL issues. Use venv/bin/pytest for run_tests. Complete plan.",
                        self.goal, network_note, errors, file1, file2
                    );'''

match = re.search(pattern, content, re.DOTALL)
if match:
    content = content[:match.start()] + replacement + content[match.end():]
    print(f"✓ agent.rs: repair prompt يرسل الملفات (found at char {match.start()})")
else:
    print("⚠ regex not matched — استخدام sed مباشر")
    # عرض السطور للتشخيص
    lines = content.split('\n')
    for i, line in enumerate(lines):
        if 'self.goal, errors' in line:
            print(f"  L{i+1}: [{line}]")
            for j in range(max(0,i-5), min(len(lines), i+2)):
                print(f"  L{j+1}:  {lines[j]}")

with open('src/agent.rs', 'w') as f:
    f.write(content)
PY_EOF
python3 /tmp/fix_agent_prompt.py

echo ""
echo "══ 3. system prompt: مثال CLI صريح + تحذير pip"
cat > /tmp/new_llm.rs << 'RUST_EOF'
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
- Use run_tests command (not "run: venv/bin/python test.py")
- run_tests uses venv/bin/pytest automatically
- Test functions must start with test_

FASTAPI PROJECTS:
- from fastapi.testclient import TestClient  (NEVER from httpx)
- POST body must use Pydantic BaseModel
- autouse fixture to reset in-memory state

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
                    max_tokens: 4096,
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
            return data.choices.into_iter().next()
                .map(|c| c.message.content)
                .ok_or_else(|| anyhow!("Empty response"));
        }
        Err(anyhow!("LLM failed"))
    }
}
RUST_EOF
cp /tmp/new_llm.rs src/llm.rs
echo "✓ llm.rs: system prompt محدث مع مثال CLI"

echo ""
echo "══ cargo build --release"
cargo build --release 2>&1 | grep -E "^error|Finished"
