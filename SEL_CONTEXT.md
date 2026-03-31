# SEL Agent — Context File
# يُقرأ في بداية كل جلسة جديدة
---

## الإصدار الحالي
- SEL Agent: v6.8
- Cargo.toml: 6.9.1
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
- Fallback: arcee-ai/trinity-large-preview:free (via OpenRouter)
- حد يومي: ~300K tokens — استخدم --iterations 1 دائماً أثناء التطوير

---

## نتائج آخر Benchmark (v6.8)
- Bench:        32/32 (100%) | 0.2 avg repairs | 100% mutation | 1.00 quality
- Integration:  7/7 (100%)   | 0.0 avg repairs | Phase1+Phase2

---

## هيكل الملفات الأساسية
```
src/
├── main.rs              — CLI + bench + stress (v6.8)
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

## إنجازات v6.3 → v6.8

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

### v6.6 — PatchError + TS Runner + Auto-Context Injection
- FailureKind::PatchError (max_attempts=2)
- executor.rs: .ts files + is_ts_workspace → npm test
- build_workspace_context(): يقرأ كل ملفات الـ workspace قبل Planning

### v6.7 — Bench Suite Expansion 28→32
- +flask hello, +fastapi route, +express api, +ts express
- Bench: 32/32 | 0.0 repairs

### v6.8 — Integration Bench Expansion 4→7
- +flask add route (Phase1+2: 0 repairs)
- +fastapi add endpoint (Phase1+2: 0 repairs)
- +marketing bot patch reddit (Phase1+2: 0 repairs)
- Marketing Bot: TypeScript multi-file OOP + jest.mock + nested folders
- Auto-Context Injection مثبت: يضيف reddit.ts بدون تعديل الملفات الموجودة

---

## FailureKind::max_attempts()
```
PatchError       => 2
InfraError       => 0  (retry فقط)
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
1. Observatory v3 — State Machine visual في Live UI
2. Workspace Memory (Context Selection Engine) — v7.0

---

## أوامر التشغيل الأساسية
```bash
# بناء
cargo build --release 2>&1 | tail -3

# bench كامل
./target/release/sel-agent bench --iterations 1

# integration bench
./target/release/sel-agent bench --suite integration

# Observatory
cd ~/sel-observatory && ./target/release/sel_observatory &
# افتح: http://172.27.155.106:8777/live
```
