#!/usr/bin/env bash
# fix_final.sh — إصلاح جذري: tail stderr + file contents في repair prompt

set -e
cd ~/projects/sel-agent-v4

# ─── 1. executor.rs: احفظ stderr كاملاً بدون تقطيع ──────────
cat > /tmp/patch_executor.py << 'EOF'
with open('src/executor.rs', 'r') as f:
    c = f.read()

old = '''        Ok(ExecResult {
            success,
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      format!("{} passed, {} failed", passed, failed),
            stderr:      if success { String::new() } else { combined },
            duration_ms: 0,
        })'''

new = '''        Ok(ExecResult {
            success,
            exit_code:   out.status.code().unwrap_or(-1),
            stdout:      format!("{} passed, {} failed", passed, failed),
            // احفظ آخر 2000 حرف — الخطأ دائماً في النهاية
            stderr:      if success {
                String::new()
            } else {
                let tail_start = combined.len().saturating_sub(2000);
                combined[tail_start..].to_string()
            },
            duration_ms: 0,
        })'''

if old in c:
    c = c.replace(old, new)
    print("✓ executor.rs: tail stderr (2000)")
else:
    print("⚠ executor.rs: pattern not found")
    for i, line in enumerate(c.split('\n')):
        if 'passed, {} failed' in line:
            print(f"  L{i}: {line}")

with open('src/executor.rs', 'w') as f:
    f.write(c)
EOF
python3 /tmp/patch_executor.py

# ─── 2. agent.rs: Repairing يرسل محتوى الملفات كاملاً ────────
cat > /tmp/patch_agent.py << 'EOF'
with open('src/agent.rs', 'r') as f:
    c = f.read()

# ابحث عن prompt في Repairing
old = '''                    let prompt = format!(
                        "Goal: {}\\n\\n\
                         FAILED steps:\\n{}\\n\\n\
                         CURRENT main.py:\\n```python\\n{}\\n```\\n\\n\
                         CURRENT test_main.py:\\n```python\\n{}\\n```\\n\\n\
                         Fix ALL issues. Ensure valid Python syntax. Install httpx for TestClient.\
                         Include ALL steps in one complete plan.",
                        self.goal, errors, main_py, test_py
                    );'''

new = '''                    let prompt = format!(
                        "Goal: {}\\n\\n\
                         === FAILED STEPS (last 2000 chars of output) ===\\n\
                         {}\\n\\n\
                         === CURRENT main.py ===\\n\
                         ```python\\n{}\\n```\\n\\n\
                         === CURRENT test_main.py ===\\n\
                         ```python\\n{}\\n```\\n\\n\
                         === REQUIRED FIXES ===\\n\
                         1. ALWAYS: from fastapi.testclient import TestClient\\n\
                            NEVER: from httpx import TestClient\\n\
                         2. Test functions MUST start with test_\\n\
                         3. venv/bin/pip3 install fastapi uvicorn httpx pytest\\n\
                         4. Write COMPLETE files — no fragments\\n\
                         Provide complete JSON plan with ALL steps.",
                        self.goal, errors, main_py, test_py
                    );'''

if old in c:
    c = c.replace(old, new)
    print("✓ agent.rs: repair prompt محدث")
else:
    # ابحث عن البديل الموجود
    found = False
    for old_try in [
        'Fix ALL issues. Ensure valid Python syntax.',
        'self.goal, errors, main_py, test_py',
        'Provide a corrected JSON plan that fixes these issues',
    ]:
        if old_try in c:
            print(f"  found marker: {old_try[:50]}")
            found = True
    if not found:
        print("⚠ agent.rs: لا يوجد file context في repair — يجب إضافته")
        # ابحث عن prompt بسيط وأضف له السياق
        old2 = '''                    let prompt = format!(
                        "Goal: {}\\n\\nThe following steps failed:\\n{}\\n\\n\
                         Provide a corrected JSON plan that fixes these issues.\\n\
                         Include ALL steps needed (not just the fix).",
                        self.goal, errors
                    );'''
        new2 = '''                    // اقرأ الملفات الحالية
                    let ws = &self.executor.workspace;
                    let main_py   = std::fs::read_to_string(ws.join("main.py")).unwrap_or_default();
                    let test_py   = std::fs::read_to_string(ws.join("test_main.py")).unwrap_or_default();

                    let prompt = format!(
                        "Goal: {}\\n\\n\
                         === FAILED STEPS (stderr tail) ===\\n\
                         {}\\n\\n\
                         === CURRENT main.py ===\\n\
                         ```python\\n{}\\n```\\n\\n\
                         === CURRENT test_main.py ===\\n\
                         ```python\\n{}\\n```\\n\\n\
                         REQUIRED FIXES:\\n\
                         1. ALWAYS: from fastapi.testclient import TestClient\\n\
                            NEVER: from httpx import TestClient\\n\
                         2. Test functions MUST start with test_\\n\
                         3. Install: venv/bin/pip3 install fastapi uvicorn httpx pytest\\n\
                         Provide complete JSON plan with ALL steps.",
                        self.goal, errors, main_py, test_py
                    );'''
        if old2 in c:
            c = c.replace(old2, new2)
            print("✓ agent.rs: repair prompt + file context مضاف")
        else:
            print("⚠ agent.rs: لم يتم تعديل prompt — تحقق يدوياً")
            # أطبع السطور المحيطة بـ prompt في Repairing
            lines = c.split('\n')
            for i, line in enumerate(lines):
                if 'self.goal, errors' in line:
                    for j in range(max(0,i-8), min(len(lines),i+3)):
                        print(f"  L{j}: {lines[j]}")

with open('src/agent.rs', 'w') as f:
    f.write(c)
EOF
python3 /tmp/patch_agent.py

echo ""
echo "── cargo build --release"
cargo build --release 2>&1 | grep -E "^error|Finished"
