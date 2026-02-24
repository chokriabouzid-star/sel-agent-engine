#!/usr/bin/env bash
# fix_schema.sh — ثلاثة إصلاحات محددة
# المشاكل: schema mismatch + no max_tokens + repair prompt لا يرسل الملفات

set -e
cd ~/projects/sel-agent-v4

echo "══════════════════════════════════════"
echo " الإصلاح 1: max_tokens في llm.rs"
echo "══════════════════════════════════════"
# الـ JSON ينقطع لأنه لا يوجد max_tokens
# Groq الافتراضي = 1024 — غير كافٍ لخطة CRUD
sed -i 's/"temperature": 0.1/"temperature": 0.1, "max_tokens": 4096/' src/llm.rs
grep -n "max_tokens" src/llm.rs && echo "✓" || echo "⚠ لم يجد"

echo ""
echo "══════════════════════════════════════"
echo " الإصلاح 2: system prompt — schema واضح"
echo "══════════════════════════════════════"
# المشكلة: LLM أرسل {"command":"run","args":[...]} بدل {"type":"run","command":"..."}
# الحل: مثال صريح في system prompt + تحذير بالخط العريض

cat > /tmp/new_llm.rs << 'RUST_EOF'
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use crate::types::Message;

const SYSTEM_PROMPT: &str = r#"You are SEL Agent v0.4 — a deterministic software execution agent.

OUTPUT: Respond ONLY with a single ```json block. No text outside it.

CORRECT SCHEMA (copy exactly):
```json
{
  "version": "1.0",
  "commands": [
    {"type": "run",        "command": "python3 -m venv venv"},
    {"type": "run",        "command": "venv/bin/pip3 install fastapi uvicorn httpx pytest"},
    {"type": "write_file", "path": "main.py",      "content": "from fastapi import FastAPI\napp = FastAPI()\n"},
    {"type": "write_file", "path": "test_main.py", "content": "from fastapi.testclient import TestClient\nfrom main import app\nclient = TestClient(app)\n"},
    {"type": "run_tests",  "target": "test_main.py"},
    {"type": "done",       "message": "All tests passed"}
  ]
}
```

FIELD NAMES — CRITICAL:
- Every command MUST have "type" field
- "type": "run" uses "command" (string, NOT array)
- NEVER use "args" array
- NEVER use "command" instead of "type"

FASTAPI RULES:
1. Start: python3 -m venv venv
2. Install: venv/bin/pip3 install fastapi uvicorn httpx pytest
3. TestClient: from fastapi.testclient import TestClient  (NEVER from httpx)
4. POST body — always Pydantic BaseModel:
     from pydantic import BaseModel
     class UserCreate(BaseModel):
         name: str
         email: str
     @app.post("/users")
     def create_user(user: UserCreate):
         ...
5. Test POST — JSON must match model:
     client.post("/users", json={"name": "Alice", "email": "a@b.com"})
6. In-memory state — reset with autouse fixture:
     import pytest
     @pytest.fixture(autouse=True)
     def reset_db():
         from main import db
         db.clear()
         yield
7. Test functions start with test_
8. Include ALL steps in ONE response"#;

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
                    max_tokens: 4096,    // ← حل مشكلة الانقطاع
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
echo "✓ llm.rs: max_tokens=4096 + schema واضح"

echo ""
echo "══════════════════════════════════════"
echo " الإصلاح 3: repair prompt يرسل الملفات"
echo "══════════════════════════════════════"
# التحقق من الوضع الحالي
echo "السطر 168 في agent.rs:"
sed -n '165,175p' src/agent.rs

# الإصلاح: استبدل format! في Repairing بنسخة ترسل الملفات
cat > /tmp/fix_repair_prompt.py << 'PY_EOF'
with open('src/agent.rs', 'r') as f:
    content = f.read()

# المشكلة: format! لا يمرر main_py و test_py
# البحث عن النمط الموجود بالضبط
old = '''                    let prompt = format!(
                        "Goal: {}\\n\\nThe following steps failed:\\n{}\\n\\n\
                         Provide a corrected JSON plan that fixes these issues.\\n\
                         Include ALL steps needed (not just the fix).",
                        self.goal, errors
                    );'''

new = '''                    let prompt = format!(
                        "Goal: {}\\n\\n\
                         FAILED (error tail):\\n{}\\n\\n\
                         CURRENT main.py:\\n```python\\n{}\\n```\\n\\n\
                         CURRENT test_main.py:\\n```python\\n{}\\n```\\n\\n\
                         FIX: Pydantic BaseModel for POST, autouse fixture to reset db, \
                         from fastapi.testclient import TestClient, \
                         JSON matching model fields exactly. \
                         Complete JSON plan with ALL steps.",
                        self.goal, errors, main_py, test_py
                    );'''

if old in content:
    content = content.replace(old, new)
    print("✓ repair prompt: يرسل الملفات")
else:
    # عرض ما هو موجود
    for i, line in enumerate(content.split('\n')):
        if 'self.goal, errors' in line:
            lines = content.split('\n')
            for j in range(max(0,i-6), min(len(lines),i+2)):
                print(f"  L{j+1}: {lines[j]}")
    print("⚠ النمط غير موجود — تحقق من السطور أعلاه")

with open('src/agent.rs', 'w') as f:
    f.write(content)
PY_EOF

python3 /tmp/fix_repair_prompt.py

echo ""
echo "══════════════════════════════════════"
echo " cargo build --release"
echo "══════════════════════════════════════"
cargo build --release 2>&1 | grep -E "^error|Finished"
