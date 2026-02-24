#!/usr/bin/env bash
# fix_network.sh — إصلاح مشكلة الشبكة
cd ~/projects/sel-agent-v4

echo "── 1. إصلاح repair prompt: أخبر LLM بفشل الشبكة واستخدم stdlib"
cat > /tmp/fix_repair.py << 'PY_EOF'
with open('src/agent.rs', 'r') as f:
    content = f.read()

# ابحث عن format! في Repairing واستبدله
old = '''                    let prompt = format!(
                        "Goal: {}\\n\\nThe following steps failed:\\n{}\\n\\n\
                         Provide a corrected JSON plan that fixes these issues.\\n\
                         Include ALL steps needed (not just the fix).",
                        self.goal, errors
                    );'''

new = '''                    // اكتشف إذا كانت مشكلة شبكة
                    let network_failed = errors.contains("Network is unreachable")
                        || errors.contains("Timeout after")
                        || errors.contains("NewConnectionError");

                    let ws = &self.executor.workspace;
                    let main_py = std::fs::read_to_string(ws.join("main.py")).unwrap_or_default();
                    let cli_py  = std::fs::read_to_string(ws.join("cli.py")).unwrap_or_default();
                    let test_py = std::fs::read_to_string(ws.join("test_main.py"))
                        .or_else(|_| std::fs::read_to_string(ws.join("test_cli.py")))
                        .or_else(|_| std::fs::read_to_string(ws.join("test_stats.py")))
                        .unwrap_or_default();

                    let network_note = if network_failed {
                        "\\n\\nCRITICAL: pip/network is NOT available. \
                         Use ONLY Python standard library: csv, statistics, os, sys, json, re. \
                         NO pandas, NO requests, NO external packages. \
                         Only pytest is pre-installed (use venv/bin/pytest)."
                    } else { "" };

                    let prompt = format!(
                        "Goal: {}{}\\n\\n\
                         FAILED:\\n{}\\n\\n\
                         CURRENT FILES:\\n\
                         cli.py/main.py: ```python\\n{}{}\\n```\\n\
                         tests: ```python\\n{}\\n```\\n\\n\
                         Fix ALL issues. venv/bin/pytest for tests. Complete plan.",
                        self.goal, network_note, errors,
                        main_py, cli_py, test_py
                    );'''

if old in content:
    content = content.replace(old, new)
    print("✓ repair prompt: network detection + stdlib hint + file contents")
else:
    print("⚠ pattern not found — searching...")
    lines = content.split('\n')
    for i, line in enumerate(lines):
        if 'self.goal, errors' in line:
            print(f"  L{i+1}: {line}")

with open('src/agent.rs', 'w') as f:
    f.write(content)
PY_EOF
python3 /tmp/fix_repair.py

echo ""
echo "── 2. إصلاح pytest path في executor (pytest → venv/bin/pytest)"
# المشكلة: run_tests يستخدم pytest بدون venv/ في بعض الحالات
grep -n '"pytest "' src/executor.rs | head -5
sed -i 's/let pytest = if self\.workspace\.join("venv\/bin\/pytest")\.exists()/let pytest = if self.workspace.join("venv\/bin\/pytest").exists()/' src/executor.rs
grep -n "venv/bin/pytest" src/executor.rs | head -5

echo ""
echo "── 3. system prompt: أضف تحذير stdlib"
sed -i 's/10. Include ALL steps in ONE response"#;/10. Include ALL steps in ONE response\n11. If pip fails or network unavailable, use Python stdlib only: csv, statistics, os, sys\n    Do NOT use pandas, requests, or any external library that needs downloading"#;/' src/llm.rs
grep -n "stdlib\|11\." src/llm.rs | head -3

echo ""
echo "── cargo build --release"
cargo build --release 2>&1 | grep -E "^error|Finished"
