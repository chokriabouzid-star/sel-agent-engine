# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v9.3.0
> **الحالة:** Stable — context budget telemetry + force_include guarantee + smarter scoring
> **الفرع:** refactor/v9.2.5-decision-split
> **آخر تحديث:** 2026-06-14
> **الترخيص:** MIT

---

## 1) ما هو SEL Agent؟

SEL Agent هو **وكيل هندسة برمجيات مستقل** مكتوب بلغة Rust.
يستقبل هدفاً بالنص الطبيعي، ثم:

1. يحلل وضوح الهدف
2. يبني خطة تنفيذ
3. يقيّم مخاطر الخطة
4. يكتب أو يعدّل الملفات
5. يشغّل الاختبارات
6. يحلل الفشل ويوجه الإصلاح تكيفياً
7. يمنع الدوران في حلقات إصلاح متكررة
8. يتعلم من الأنماط الناجحة والفاشلة
9. يعلن `SEL_SUCCESS` أو `SEL_FAILED`

---

## 2) الفلسفة الأساسية

| المبدأ | التفسير |
|--------|---------|
| الاختبار هو العقد | الوكيل لا يعدّل اختبارات موجودة أبداً |
| الحتمية أولاً | replay و trajectories جزء أساسي غير قابل للتفاوض |
| القياس قبل التحسين | لا ميزة بدون impact eval موثّق |
| الإصلاح الجذري | نصلح السبب البنيوي لا العرض السطحي |
| تقليل الاعتماد على LLM | نقل المعرفة إلى engine عبر routing + patterns + autofix |
| المعرفة في الكود | لا patch scripts، لا .bak files، Git فقط |
| الفصل بين Milestones | refactor في milestone منفصل عن feature |
| لا تخمين | نعمل على الكود الحقيقي والمخرجات الحقيقية فقط |

---

## 3) الحالة المؤكدة — v9.3.0

| المقياس | النتيجة |
|---------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings |
| `cargo test` | ✅ 143/143 |
| `cargo build --release` | ✅ |
| `regression_gate.sh core` | ✅ |
| `bench --suite all --replay` | **36/36** ✅ |
| `bench-swe --lang all --replay` | **30/30** ✅ |
| `bench-sel-v11 --replay` | **18/18** ✅ |
| `smoke --replay` | **12/12** ✅ |

---

## 4) سجل الإصدارات

### v9.3.0 — Context Budget Telemetry ✅

**commits:**
ecbff54 feat: force_include_dropped + stderr_occurrences weight
a96e578 style: normalize formatting + sync lockfile
a5def52 docs: context_budget RESULTS.md
bcabe1d feat: wire BudgetReport into ExecutionReport
e3eb879 release: bump version to v9.3.0
527b38e docs: update CHANGELOG for v9.3.0

text


#### التغييرات المعمارية

**قبل v9.3.0:**
build_repair_context_block() → String
BudgetReport يُبنى ويُفقد داخل builder
ExecutionReport لا يحتوي context metrics

text


**بعد v9.3.0:**
build_repair_context_block() → (String, BudgetReport)
BudgetReport يُجمّع في ExecutionContext عبر كل repair loop
ExecutionReport يحتوي context metrics كاملة

text


#### الحقول المُضافة

**`BudgetReport` (src/context/builder.rs):**
```rust
pub force_include_dropped: Vec<String>,  // ملفات طُلبت صراحةً لكن لم تُحمَّل
ExecutionContext (src/types.rs):

Rust

// v9.3.0: Context Budget Telemetry
pub context_tokens_total: u64,
pub context_tokens_before_total: u64,
pub context_files_total: u64,
pub context_budget_samples: u32,
pub force_include_dropped_count: u64,
ExecutionReport (src/report.rs):

Rust

#[serde(default)]
pub avg_context_tokens: u64,
#[serde(default)]
pub avg_selected_files: u64,
#[serde(default)]
pub context_reduction_pct: u8,
#[serde(default)]
pub force_include_dropped_count: u64,
تحسين الـ Scoring
compute_score() — stderr_occurrences weight:

Rust

// إذا ظهر اسم الملف أكثر من مرة في stderr
let occurrences = ctx.stderr.matches(filename).count();
if occurrences > 1 {
    let extra = ((occurrences - 1).min(3) * 2) as u8;
    score = score.saturating_add(extra);  // max +6
}
Gate Results
text

regression_gate core: ✅ PASS
cargo test: 143/143 ✅
bench all: 36/36 ✅
bench-swe: 30/30 ✅
bench-sel-v11: 18/18 ✅
force_include_dropped: مُتتبَّع ✅
20% reduction gate: data collection active — يُقاس بعد live repair runs
v9.2.6 — v9.2.0 Closeout ✅
ما أُضيف:

Rust

// ExecutionContext
pub tokens_used: u64,
pub plan_confidence: Option<f32>,   // None حتى v11.0

// ExecutionReport
pub total_tokens: u64,
pub avg_tokens_per_task: u64,
التعريفات:

text

total_tokens        = tokens_in + tokens_out
avg_tokens_per_task = total_tokens / llm_calls
plan_confidence     = None — بيانات v11.0
v9.2.5 — decision.rs Structural Refactor ✅
text

src/decision.rs              ← facade
src/decision/
├── checklist.rs
├── context_builders.rs
├── goal.rs
├── plan_risk.rs
└── validators.rs
smoke --replay: 12/12 ✅

v9.2.1 — Plan Risk Telemetry + 7 Quality Fixes ✅
7 Fixes:

#	الملف	الإصلاح
1	state_handlers.rs	matched_pattern.as_ref()
2	repair_strategy.rs	is_test_file تضييق
3	evaluator.rs	guard ضد mutation_score = -1.0
4	executor/runner.rs	head+tail stderr capture
5	pattern_library.rs	normalize_signature() noise filter
6	agent.rs	latest_pattern_error() priority
7	executor/mutation.rs	replay_mode early return
v9.0.0 — v9.2.0 — الأساس
الإصدار	الميزة
v9.2.0	Plan Risk Evaluation + Telemetry
v9.1.0	Prompt Quality + Goal Clarity
v9.0.0	Adaptive Repair Routing + Smart Context + Pattern Library
5) البنية المعمارية
دورة الحالة
text

Planning → Executing → Repairing → Done / Failed
المسار الكامل للتخطيط
text

goal
  → validate_goal()
  → GoalClarity::analyze()
  → goal_advisory_hints()
  → build_planning_prompt()
  → plan_with_resilience()
  → validate_plan_integrity()
  → validate_protected_writes()    ← يشمل go.mod
  → validate_patch_uniqueness()
  → evaluate_plan_risk()
  → replan_with_feedback()
  → constraint_engine::apply()
  → dedup write_file
  → AgentState::Executing
المسار الكامل للإصلاح
text

stderr
  → classify FailureKind
  → error_fingerprint + streak detection
  → infer_route_from_stderr()
     OR matched_pattern.as_ref().route
  → effective_route
  → pre_repair_checklist()
  → build_budgeted_repair_prompt()
     - route-aware instructions
     - pattern hint + example_fix
     - smart repair context (scored)     ← v9.3.0: stderr_occurrences weight
     - global cap (24,000 chars)
  → plan_with_resilience()
  → execution
  → record_pattern_outcome()
  → normalize_signature() → persist
  → BudgetReport → ExecutionContext     ← v9.3.0: context telemetry
هيكل الملفات
text

src/
├── agent.rs
├── state_handlers.rs
├── decision.rs                  facade
├── decision/
│   ├── checklist.rs
│   ├── context_builders.rs
│   ├── goal.rs
│   ├── plan_risk.rs
│   └── validators.rs
├── repair_strategy.rs
├── constitution.rs              7 قواعد صلبة
├── protocol.rs
├── types.rs                     ExecutionContext (يشمل context budget fields)
├── failure.rs
├── pattern_library.rs
├── diagnostic.rs
├── memory.rs
├── manifest.rs
├── scaffold_engine.rs
├── report.rs                    ExecutionReport (يشمل context budget + cost fields)
├── report_writer.rs
├── cost.rs
├── executor/
│   ├── core.rs
│   ├── file_ops.rs
│   ├── runner.rs
│   ├── autofix.rs
│   ├── compile.rs
│   ├── sanitizers.rs
│   ├── mutation.rs
│   └── parsers.rs
├── llm/
│   ├── live.rs
│   ├── record.rs
│   ├── replay.rs
│   └── key_pool.rs
├── context/
│   ├── builder.rs               ← v9.3.0: (String, BudgetReport) + occurrences weight
│   └── scanner.rs
└── dependency_graph/
    ├── mod.rs
    ├── model.rs
    ├── builder.rs
    └── parsers/
6) الدستور — 7 قواعد صلبة
text

Rule 1: no-modify-tests      → لا تعديل لملفات اختبار موجودة
Rule 2: no-empty-write       → لا كتابة محتوى فارغ
Rule 3: no-binary-in-text    → لا بيانات binary في ملفات نصية
Rule 4: no-system-path       → لا مسارات نظام خطرة
Rule 5: no-overwrite-go-mod  → لا استبدال go.mod
Rule 6: no-dangerous-cmd     → لا أوامر خطرة
Rule 7: no-network-in-test   → لا شبكة في وضع replay
قاعدة قادمة في v9.5.0:

text

Rule 8: authored-test-quality → mutation validation للاختبارات المؤلفة
7) Adaptive Repair Routing
Pattern في stderr	Route
CONSTITUTION_VIOLATION:no-modify-tests	ForceSourceOnly
circular import	CircularImport
no module named / cannot find module	MissingDependency
undefined: / is not defined	FunctionDeleted
cannot borrow / borrowed value	RustOwnership
nil pointer / NoneType	NullGuard
mismatched types / TypeError	TypeMismatch
غير ذلك	Generic
8) Smart Repair Context — Scoring (v9.3.0)
Signal	النقاط
Focus path	+10
Culprit file	+8
Mentioned in stderr	+5
stderr occurrences > 1	+2 لكل تكرار إضافي (max +6) ← v9.3.0
Graph: dependency of culprit	+4
Recently edited	+3
Graph: impacts culprit	+3
Imports errored file	+2
Graph: in dependency cycle	+2
Small file (< 200 tokens)	+1
Budget:

Local: MAX_REPAIR_TOKENS = 8,000
Global: MAX_REPAIR_PROMPT_CHARS = 24,000
9) Context Budget Telemetry (v9.3.0)
text

كل repair loop:
  build_repair_context_block() → (String, BudgetReport)
  BudgetReport {
    total_files,
    selected_files,
    tokens_before,
    tokens_after,
    force_include_dropped,     ← ملفات force_include لم تُحمَّل
  }
  → يُجمَّع في ExecutionContext
  → يُحسب متوسط في agent.rs
  → يُكتب في ExecutionReport JSON
في كل latest.json:

JSON

{
  "avg_context_tokens": 0,
  "avg_selected_files": 0,
  "context_reduction_pct": 0,
  "force_include_dropped_count": 0
}
ملاحظة: القيم = 0 في runs بدون repairs — سلوك صحيح.

10) Telemetry Fields — الحالة الكاملة
الحقل	الموقع	منذ
tokens_in	ExecutionReport	v9.2.1
tokens_out	ExecutionReport	v9.2.1
total_tokens	ExecutionReport	v9.2.6
avg_tokens_per_task	ExecutionReport	v9.2.6
plan_risk_triggered	ExecutionReport	v9.2.1
replan_count	ExecutionReport	v9.2.1
plan_risk_reasons	ExecutionReport	v9.2.1
tokens_used	ExecutionContext	v9.2.6
plan_confidence	ExecutionContext	v9.2.6
avg_context_tokens	ExecutionReport	v9.3.0
avg_selected_files	ExecutionReport	v9.3.0
context_reduction_pct	ExecutionReport	v9.3.0
force_include_dropped_count	ExecutionReport	v9.3.0
11) Plan Risk Evaluation
Rule	الخطر
write_file على test موجود	+0.50
patch_file على test موجود	+0.50
delete_file	+0.60
write_file على source موجود	+0.35
plan.len() >= 8	+0.20
write_file على go.mod	PLAN ERROR (hard reject)
Bash

SEL_DISABLE_PLAN_RISK=1  # A/B comparison
12) Pattern Library
Rust

pub struct Pattern {
    pub id: String,
    pub language: String,
    pub error_signature: String,
    pub route: RepairRoute,
    pub success_count: u32,
    pub failure_count: u32,
    pub usage_count: u32,
    pub last_seen_utc: String,
    pub example_fix: Option<String>,
    pub failed_contexts: Vec<String>,
}
// is_strong() = usage_count >= 2 && success_rate >= 0.7
القيود المعروفة:

is_graduate() غير موجود ← v9.4.0
success_rate الخام يُضخّم الثقة ← v9.8.0
التواقيع مرتبطة بمشروع واحد ← v9.8.0
13) مناطق الخطورة المعمارية
الملف	الخطر
src/constitution.rs	أي تغيير يمكّن تجاوز عقد الاختبار
src/state_handlers.rs	تعديل قد يتخطى Failed إلى Done بصمت
src/agent.rs	يملك القرار النهائي
src/pattern_library.rs	pattern خاطئ يفسد إصلاحات المستقبل
src/decision/checklist.rs	semantic shortcuts خارج bench mode
fixtures/trajectories/	تعديل يدوي يكسر الحتمية
14) القيود الحالية المعروفة
القيد	الخطوة التالية
20% context reduction لم يُثبت بعد	live data بعد repair runs
Pattern Graduation غير موجود	v9.4.0
plan_confidence = None	v11.0
Pattern cross-project	v9.8.0
Test Authoring validation	v9.5.0
Multi-file planning	v9.6.0
SWE-Bench external	v9.7.0
evaluator.rs غير موصول	مستقبلي
15) الأوامر المرجعية
Bash

# الجودة اليومية
cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# قبل كل milestone
bash scripts/regression_gate.sh core

# قبل كل release
bash scripts/regression_gate.sh full

# Benchmarks
cargo build --release
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay

# Observability
./target/release/sel-agent report --latest
./target/release/sel-agent report --summary
cat ~/.sel-agent/patterns.json | python3 -m json.tool

# Context budget في التقرير
cat ~/.sel-agent/reports/latest.json | python3 -m json.tool | \
  grep -E '"avg_context_tokens"|"avg_selected_files"|"context_reduction_pct"|"force_include_dropped_count"'
16) الملخص التنفيذي
text

SEL Agent v9.3.0 — الحالة النهائية المؤكدة

Git:
  527b38e (HEAD)
  e3eb879 (tag: v9.3.0)
  version = "9.3.0"

Validated:
  cargo test:            143/143 ✅
  cargo clippy:          ✅ 0 warnings
  regression_gate core:  ✅
  bench all:             36/36 ✅
  bench-swe:             30/30 ✅
  bench-sel-v11:         18/18 ✅

Active capabilities (cumulative):
  ✅ adaptive repair routing (8 routes)
  ✅ smart repair context — scored + dependency graph
  ✅ stderr_occurrences weight (v9.3.0)
  ✅ force_include guarantee + dropped tracking (v9.3.0)
  ✅ context budget telemetry in every report (v9.3.0)
  ✅ global prompt budget (24k chars)
  ✅ plan risk evaluation + telemetry
  ✅ decision.rs → facade + 5 submodules
  ✅ cost telemetry (tokens_in/out/total/avg)
  ✅ plan_confidence field (None — data v11.0)
  ✅ pattern memory + noise-filtered signatures
  ✅ head+tail stderr capture
  ✅ replay mutation safety

Next:
  v9.4.0 → Pattern Graduation → AutoFix  ← التالي (الأعلى قيمة)
  v9.5.0 → Test Authoring Validation
  v9.6.0 → Multi-File Coordinator
  v9.7.0 → SWE-Bench Adapter
