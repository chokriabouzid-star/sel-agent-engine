# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v8.9.0
> **الحالة:** Stable working tree after Wave 1 + Wave 2 + Wave 3 phase 1
> **الفرع:** phase2-safe
> **المؤلف:** Chokri Bouzid
> **آخر تحديث:** 2026-06-06
> **الترخيص:** MIT


## 1) ما هو SEL Agent؟

SEL Agent هو **وكيل هندسة برمجيات مستقل** مكتوب بلغة Rust.
يستقبل هدفًا بالنص الطبيعي، ثم:

1. يبني خطة تنفيذ
2. يكتب أو يعدّل الملفات
3. يشغّل الاختبارات
4. يحلّل الفشل
5. يعيد الإصلاح تكراريًا
6. يعلن `SEL_SUCCESS` أو `SEL_FAILED`

المشروع جزء من تصور أشمل اسمه **Sovereign Execution Layer (SEL)**.


## 2) الفلسفة الأساسية

| المبدأ | التفسير |
|--------|---------|
| الاختبار هو العقد | الوكيل لا يعدّل اختبارات موجودة سابقًا أبدًا |
| الحتمية أولًا | replay و trajectories جزء أساسي من النظام |
| القياس قبل التحسين | لا إضافة بدون benchmark أو signal قياسي |
| الإصلاح الجذري | نصلح السبب البنيوي لا العرض السطحي |
| تقليل الاعتماد على LLM | ننقل المعرفة إلى engine عبر autofix و pattern library |



## 3) الحالة الحالية المؤكدة

### 3.1 Checkpoints المؤكدة
| المقياس | النتيجة |
|---------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test` | **272/272** ✅ |
| `bench --suite all --replay` | **36/36** ✅ |
| `bench-swe --lang all --replay` | **30/30** ✅ |
| `bench-sel-v11 --replay` | **18/18** ✅ |
| `bench-real-world --replay` | **14/14** ✅ |
| `smoke --replay` | **12/12** ✅ |
| `scripts/regression_gate.sh core` | ✅ |
| `scripts/regression_gate.sh full` | ✅ |

### 3.2 توزيع الاختبارات
| الهدف | عدد الاختبارات |
|-------|----------------|
| `src/lib.rs` | 128 |
| `src/main.rs` | 137 |
| `tests/executor_tests.rs` | 7 |
| **المجموع** | **272** |



## 4) سجل الإصدارات

### v8.9.0 — Pattern Library (2026-06-06)

#### ما أُضيف
- `src/pattern_library.rs` — النواة الكاملة
- `RepairRoute` enum مشترك مع v9.0
- `Pattern` struct مع `success_count` / `failure_count` / `usage_count` / `example_fix`
- `PatternStore` مع `schema_version`
- `PatternLibrary` مع `load()` / `lookup()` / `record_outcome()` / `save()`
- `infer_language_from_workspace()` داخل `pattern_library.rs`
- `normalize_signature()` يزيل line numbers و paths
- `match_score()` يحسب درجة التطابق
- `is_strong()` يشترط `usage_count >= 2` و `success_rate >= 0.7`
- ربط `lookup()` مع `state_handlers.rs` قبل بناء repair prompt
- ربط `record_outcome()` مع `agent.rs` عند Done و Failed
- `summarize_fix_plan()` يستخرج ملخص من `self.plan`
- `build_pattern_hint()` يضيف route hint + Guidance + Example fix
- `truncate_pattern_example()` بـ UTF-8 safe
- تحسين `scripts/regression_gate.sh` للمطابقة الصحيحة

#### الملفات المعدّلة
- `src/pattern_library.rs`
- `src/repair_strategy.rs`
- `src/state_handlers.rs`
- `src/agent.rs`
- `src/lib.rs`
- `src/main.rs`
- `scripts/regression_gate.sh`

### v8.8.0 — Observability + Dependency Graph (2026-06-04)

#### Wave 1 — Observability
- `ExecutionReport` backend
- كتابة التقارير إلى `~/.sel-agent/reports/`
- `latest.json`
- `sel-agent report --latest`
- `sel-agent report --summary`
- `sel-agent observatory`
- `scripts/regression_gate.sh`

#### Wave 2 — Dependency Graph
- `DependencyGraph` model
- `dependencies_of()` / `impacted_by()` / `detect_cycles()`
- parsers لـ Python / TypeScript / Go / Rust
- `build_for_workspace()`
- graph-aware context scoring في `src/context/builder.rs`

---

## 5) البنية المعمارية

### دورة الحالة

Planning → Executing → Repairing → Done / Failed
الملفات الأساسية
الملف	الدور
src/main.rs	نقطة الدخول والـ CLI
src/agent.rs	state machine loop
src/state_handlers.rs	planning / executing / repairing
src/decision.rs	validate_goal / validate_plan / context building
src/repair_strategy.rs	repair prompts + pattern hints
src/protocol.rs	Cmd + parser
src/types.rs	AgentState / ExecutionContext
src/constitution.rs	القواعد الصلبة
التنفيذ
الملف	الدور
src/executor/core.rs	SafeExecutor
src/executor/runner.rs	تشغيل الاختبارات
src/executor/file_ops.rs	write / patch / append
src/executor/autofix.rs	autofix deterministic
src/executor/compile.rs	compile checks
src/executor/sanitizers.rs	sanitize
LLM / Replay
الملف	الدور
src/llm/live.rs	live provider
src/llm/record.rs	recorder
src/llm/replay.rs	replay
src/llm/json_sanitizer.rs	JSON cleaning
Observability
الملف	الدور
src/report.rs	ExecutionReport
src/report_writer.rs	write reports
src/commands/report.rs	report CLI
src/commands/observatory.rs	TUI
Dependency Graph
الملف	الدور
src/dependency_graph/mod.rs	graph API
src/dependency_graph/model.rs	graph model
src/dependency_graph/builder.rs	build_for_workspace
src/dependency_graph/parsers/*	parsers
Pattern Library
الملف	الدور
src/pattern_library.rs	PatternLibrary + Pattern + RepairRoute
6) Pattern Library
التخزين


~/.sel-agent/patterns.json
الفكرة
عندما ينجح الوكيل في إصلاح خطأ، يسجل:

اللغة
بصمة الخطأ
route
عدد النجاحات / الإخفاقات
مثال fix ناجح
الاستخدام
في do_repairing():

تحديد لغة workspace
تحميل PatternLibrary
lookup(language, stderr)
تمرير matched_pattern إلى build_prompt(...)
النتيجة
الـ repair prompt قد يحتوي الآن على:

Known successful repair pattern: ...
Guidance: ...
Example successful fix: ...
7) Execution Reports
يكتب الوكيل تقارير إلى:



~/.sel-agent/reports/
الأوامر:


sel-agent report --latest
sel-agent report --summary
sel-agent observatory --refresh-secs 5 --limit 50
8) Regression Gate
الأوامر


bash scripts/regression_gate.sh core
bash scripts/regression_gate.sh full
المحتوى
core:
bench all
bench-swe
bench-sel-v11
full:
core
bench-real-world
smoke
ملاحظة
تم تقوية السكربت ليطابق summary النهائي الحقيقي بعد إزالة ANSI/CR من اللوج.

9) الأوامر المرجعية
الجودة اليومية

cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
Benchmarks


cargo build --release
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay
10) القيود الحالية
بعض Rust replay cases قد تكون flaky حسب cache / cargo registry state
RepairCtx::build ما يزال top-level scan فقط
graph scoring ما يزال يعيد بناء graph أكثر من اللازم
Rust graph resolution ما يزال محدودًا في use crate::...
11) التالي
v9.0.0 — Adaptive Repair Routing
المرحلة التالية المقترحة:

توجيه repair حسب نوع الخطأ
استغلال RepairRoute الموجود الآن
fallback آمن إلى Generic
12) الملخص التنفيذي


SEL Agent v8.9.0

Verified:
- cargo check: pass
- cargo clippy --all-targets --all-features -- -D warnings: pass
- cargo test: 272/272
- bench all replay: 36/36
- bench-swe replay: 30/30
- bench-sel-v11 replay: 18/18
- bench-real-world replay: 14/14
- smoke replay: 12/12

Major completed capabilities:
- execution reports
- observatory TUI
- local regression gate
- dependency graph
- graph-aware context scoring
- pattern library
- pattern hint injection
- example_fix memory from successful plans

Next strategic milestone:
- v9.0.0 Adaptive Repair Routing
