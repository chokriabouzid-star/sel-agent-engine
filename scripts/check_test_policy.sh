#!/usr/bin/env bash
# يتحقق أن سياسة ملفات الاختبار موجودة في الأماكن الأساسية
set -e
ERRORS=0

check() {
    local file="$1" pattern="$2" label="$3"
    if ! grep -q "$pattern" "$file" 2>/dev/null; then
        echo "❌ MISSING in $file: $label"
        ERRORS=$((ERRORS + 1))
    else
        echo "✅ OK: $label"
    fi
}

check "src/constitution.rs" \
    "no-modify-tests" \
    "Constitution Rule 1 label"

check "src/executor/file_ops.rs" \
    "blocks_existing_spec_modification" \
    "executor spec modification guard"

check "src/executor/file_ops.rs" \
    "goal_authorized_test_write_allowed" \
    "executor goal-authorized exception"

check "src/agent.rs" \
    "extract_goal_authorized_test_files" \
    "agent goal-authorized extraction"

check "src/agent.rs" \
    "protected_test_files" \
    "agent protected test files snapshot"

check "src/llm/mod.rs" \
    "SPEC FILE PROTECTION" \
    "LLM system prompt spec protection"

if [ $ERRORS -gt 0 ]; then
    echo ""
    echo "❌ $ERRORS location(s) missing policy"
    exit 1
fi

echo ""
echo "✅ Test policy consistent across all locations"
