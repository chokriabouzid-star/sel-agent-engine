# SEL Agent — Context File
# يُقرأ في بداية كل جلسة جديدة
---

## الإصدار الحالي
- SEL Agent: v6.5
- Cargo.toml: 6.4.0
- SEL Observatory: معطّل مؤقتاً (port 8777)

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

## نتائج آخر Benchmark (v6.5)
- Passed:         28/28 (100%)
- Avg Repairs:    0.0
- Mutation Score: 100%
- Quality Index:  1.00

---

## هيكل الملفات الأساسية
```
src/
├── main.rs            — CLI + bench + stress commands (v6.4)
├── agent.rs           — State Machine: Planning → Executing → Repairing
├── executor.rs        — تنفيذ الأوامر (run/write_file/run_tests/patch_file)
├── scaffold_engine.rs — ★ يُجهّز البيئة قبل LLM — يستخدم GoalParser
├── goal_parser.rs     — ★ NEW v6.5: ParsedGoal { kind, sub_kind, extra_deps }
├── types.rs           — FailureKind + RepairBudget + InfraError (v6.4/v6.5)
├── constraint_engine.rs — طبقة قيود جزئية (scaffold يُغني عنها)
├── scanner.rs         — has_ts_files() / has_py_files()
├── protocol.rs        — enum Cmd { Run, WriteFile, RunTests, PatchFile, ... }
├── environment.rs     — EnvironmentCapabilities::probe()
├── llm.rs             — Groq API client + retry logic
├── context.rs         — ContextConfig + ref_file handling
├── evaluator.rs       — Mutation testing
└── memory.rs          — Session memory
```

---

## إنجازات v6.3 → v6.5

### v6.3 — ScaffoldEngine (الإنجاز الأكبر)
- يُجهّز البيئة الكاملة قبل LLM (package.json + tsconfig + node_modules)
- Pinned stacks: typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0 @types/jest@29.5.11
- TypeScript: 0 repairs — Python: مستقر

### v6.4 — RepairBudget + InfraError
- FailureKind::InfraError — يميّز أخطاء الشبكة/API عن أخطاء الكود
- Retry policy: 3 محاولات بـ backoff (15s/45s/120s) بدون LLM
- RepairBudget ديناميكي: كل FailureKind له max_attempts() مستقل
- Version strings: v5.8 → v6.4 في كل مكان

### v6.5 — GoalParser + Goal-Aware Scaffold
- goal_parser.rs: ParsedGoal { kind, sub_kind, extra_deps }
- SubKind: Flask / FastAPI / Django / Express / React / Plain
- ScaffoldEngine يستخدم GoalParser بدلاً من string matching
- scaffold_python: يثبّت extra_deps (flask, fastapi+uvicorn+httpx, django)
- scaffold_typescript: يضيف extra_deps لـ npm install
- logic_hint: .js ممنوع صراحةً — Jest يبحث عن *.test.ts فقط

---

## FailureKind::max_attempts()
```
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

## القواعد الذهبية — لا تكسرها
- كل الكود بـ `cat > file << 'EOF'` heredoc — لا .sh scripts
- Python3 scripts للتعديلات المعقدة (Unicode في التعليقات)
- `cargo build 2>&1 | grep "^error"` بعد كل تغيير
- bench بـ `--iterations 1` فقط أثناء التطوير
- git commit بعد كل milestone
- اقرأ الكود قبل التعديل — diagnostic أولاً

---

## أخطاء Rust الشائعة
| الخطأ | الحل |
|-------|------|
| E0026: no field target | الحقل اسمه `command` في Cmd::Run |
| E0609: no field workspace | استخدم `self.executor.workspace` |
| E0433: unresolved crate | `cargo add <crate>` |
| npm ENOENT في bench | `create_dir_all(&workspace)` قبل agent.run() |

---

## الخطوات القادمة
1. اختبار FastAPI حقيقي — التحقق من GoalParser + extra_deps
2. Observatory v3 — WebSocket live UI
3. Template Config Files (.toml) — v6.6
4. Multi-File Context Engine — v7.0

---

## أوامر التشغيل الأساسية
```bash
# بناء
cargo build --release 2>&1 | tail -3

# bench سريع
./target/release/sel-agent bench --iterations 1

# اختبار Python Flask
rm -rf /tmp/test_flask && mkdir /tmp/test_flask && echo "" > /tmp/test_flask/requirements.txt
./target/release/sel-agent run --workspace /tmp/test_flask --goal "Create a Python Flask application with a single route /hello that returns Hello World. Include a pytest test."

# اختبار FastAPI
rm -rf /tmp/test_fastapi && mkdir /tmp/test_fastapi && echo "" > /tmp/test_fastapi/requirements.txt
./target/release/sel-agent run --workspace /tmp/test_fastapi --goal "Create a Python FastAPI application with a single route /hello that returns Hello World. Include a pytest test using TestClient."

# اختبار TypeScript
rm -rf /tmp/test_ts && mkdir /tmp/test_ts
./target/release/sel-agent run --workspace /tmp/test_ts --goal "Create a TypeScript calculator with jest tests"
```
