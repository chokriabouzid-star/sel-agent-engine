# SEL Agent — Context File
# يُقرأ في بداية كل جلسة جديدة
---

## الإصدار الحالي
- SEL Agent: v6.6
- Cargo.toml: 6.4.0
- SEL Observatory: يعمل (port 8777) — WebSocket Live ✅

---

## المسارات الأساسية
- Project:  ~/projects/active/sel-agent-v4/
- Binary:   ~/projects/active/sel-agent-v4/target/release/sel-agent
- Context:  ~/projects/active/sel-agent-v4/SEL_CONTEXT.md
- Observatory: ~/sel-observatory/target/release/sel_observatory

---

## النموذج الأساسي
- Primary:  kimi-k2-instruct (via Groq API)
- Fallback: llama-3.3-70b-versatile
- حد يومي: ~300K tokens — استخدم --iterations 1 دائماً أثناء التطوير

---

## نتائج آخر Benchmark (v6.6)
- Passed:         28/28 (100%)
- Avg Repairs:    0.0
- Mutation Score: 100%
- Quality Index:  1.00

---

## هيكل الملفات الأساسية
```
src/
├── main.rs              — CLI + bench + stress (v6.4)
├── agent.rs             — State Machine + Auto-Context Injection (v6.6)
├── executor.rs          — run/write_file/run_tests/patch_file + TS runner fix (v6.6)
├── scaffold_engine.rs   — يُجهّز البيئة + يستخدم GoalParser
├── goal_parser.rs       — ParsedGoal { kind, sub_kind, extra_deps } (v6.5)
├── types.rs             — FailureKind + RepairBudget + InfraError + PatchError (v6.6)
├── constraint_engine.rs — طبقة قيود جزئية
├── scanner.rs           — has_ts_files() / has_py_files()
├── protocol.rs          — enum Cmd { Run, WriteFile, RunTests, PatchFile, ... }
├── environment.rs       — EnvironmentCapabilities::probe()
├── llm.rs               — Groq API client + retry logic
├── context.rs           — ContextConfig + ref_file handling
├── evaluator.rs         — Mutation testing
└── memory.rs            — Session memory
```

---

## إنجازات v6.3 → v6.6

### v6.3 — ScaffoldEngine
- يُجهّز البيئة قبل LLM (package.json + tsconfig + node_modules)
- Pinned stacks: typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11

### v6.4 — RepairBudget + InfraError
- FailureKind::InfraError — يميّز أخطاء الشبكة عن الكود
- Retry policy: 3 محاولات بـ backoff (15s/45s/120s) بدون LLM
- RepairBudget ديناميكي: كل FailureKind له max_attempts() مستقل

### v6.5 — GoalParser + Goal-Aware Scaffold
- goal_parser.rs: ParsedGoal { kind, sub_kind, extra_deps }
- SubKind: Flask / FastAPI / Django / Express / React / Plain
- scaffold_python: يثبّت extra_deps تلقائياً
- logic_hint: .js ممنوع — Jest يبحث عن *.test.ts فقط

### v6.6 — PatchError + TS Runner + Auto-Context Injection
- FailureKind::PatchError (max_attempts=2) — search block not found
- executor.rs: .ts files + is_ts_workspace → npm test (لا pytest)
- build_workspace_context(): يقرأ كل ملفات الـ workspace قبل Planning
- يدعم: .ts .js .py .go .rs .toml — يتجاهل node_modules/venv/dist

---

## FailureKind::max_attempts()
```
PatchError       => 2  (context mismatch — hint مخصص)
InfraError       => 0  (retry فقط — لا LLM)
ImportError      => 1
NodeTestError    => 1
DatabaseError    => 2
FlaskConcurrency => 2
CollectionError  => 2
SyntaxError      => 3
TypeError        => 3
AssertionError   => 3
BuildError       => 3
Unknown          => 3
```

---

## القواعد الذهبية
- كل الكود بـ `cat > file << 'EOF'` heredoc — لا .sh scripts
- Python3 scripts للتعديلات المعقدة (Unicode في التعليقات)
- `cargo build 2>&1 | grep "^error"` بعد كل تغيير
- bench بـ `--iterations 1` فقط أثناء التطوير
- اختبر كل ميزة منفردة قبل bench
- git commit بعد كل milestone

---

## حدود النظام الحالية (v7.0)
- تعديل مشاريع كبيرة موجودة (Workspace Memory) → v7.0
- Import Graph للـ context الذكي → v7.0
- Multi-File Context Engine → v7.0

---

## الخطوات القادمة
1. Observatory v3 — تحسين Live UI (State Machine visual)
2. Benchmark suite توسيع — إضافة حالات تعديل مشاريع
3. Workspace Memory (Context Selection Engine) — v7.0

---

## أوامر التشغيل الأساسية
```bash
# بناء
cargo build --release 2>&1 | tail -3

# bench سريع
./target/release/sel-agent bench --iterations 1

# Observatory
cd ~/sel-observatory && ./target/release/sel_observatory &
# افتح: http://172.27.155.106:8777/live

# اختبار FastAPI
rm -rf /tmp/test_fastapi && mkdir /tmp/test_fastapi && echo "" > /tmp/test_fastapi/requirements.txt
./target/release/sel-agent run --workspace /tmp/test_fastapi \
  --goal "Create a Python FastAPI application with /hello route. Include pytest test using TestClient."

# اختبار TypeScript
rm -rf /tmp/test_ts && mkdir /tmp/test_ts
./target/release/sel-agent run --workspace /tmp/test_ts \
  --goal "Create a TypeScript calculator with jest tests"
```
