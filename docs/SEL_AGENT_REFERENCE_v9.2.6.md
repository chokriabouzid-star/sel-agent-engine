# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v9.2.6
> **الحالة:** Stable — v9.2.0 closeout complete + decision.rs split
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

---

## 3) الحالة المؤكدة — v9.2.6

| المقياس | النتيجة |
|---------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings |
| `cargo test` | ✅ |
| `regression_gate.sh core` | ✅ |
| `regression_gate.sh full` | ✅ |
| `bench --suite all --replay` | **36/36** ✅ |
| `bench-swe --lang all --replay` | **30/30** ✅ |
| `bench-sel-v11 --replay` | **18/18** ✅ |
| `bench-real-world --replay` | **14/14** ✅ |
| `smoke --replay` | **12/12** ✅ |

---

## 4) سجل الإصدارات

### v9.2.6 — v9.2.0 Closeout ✅

**commits:** `fa73378` (كود) + `8d132db` (توثيق)

#### ما أُضيف

**`src/types.rs` — `ExecutionContext`:**
```rust
// v9.2.0 closeout: Cost + planning confidence telemetry
pub tokens_used: u64,
pub plan_confidence: Option<f32>,  // None حتى v11.0
src/report.rs — ExecutionReport:

Rust

#[serde(default)]
pub total_tokens: u64,
#[serde(default)]
pub avg_tokens_per_task: u64,
src/agent.rs:

Rust

let total_tokens = stats.tokens_in as u64 + stats.tokens_out as u64;
let avg_tokens_per_task = if stats.successful_calls > 0 {
    total_tokens / stats.successful_calls as u64
} else { 0 };
self.ctx.tokens_used = total_tokens;
التعريفات المعتمدة:

text

total_tokens        = tokens_in + tokens_out
avg_tokens_per_task = total_tokens / llm_calls  (0 إذا llm_calls == 0)
plan_confidence     = None — البيانات تُجمع الآن للاستخدام في v11.0
evals/feature_impact/plan_risk/RESULTS.md — impact eval مكتمل ✅

Gate Results
text

regression_gate full: ✅
smoke --replay:       12/12 ✅
bench all:            36/36 ✅
bench-swe:            30/30 ✅
bench-sel-v11:        18/18 ✅
bench-real-world:     14/14 ✅
v9.2.5 — decision.rs Structural Refactor ✅
التغيير الجوهري: تفكيك src/decision.rs (1400+ سطر) إلى facade + submodules.

text

src/decision.rs              ← facade (re-exports فقط)
src/decision/
├── checklist.rs             ← pre_repair_checklist + semantic shortcuts
├── context_builders.rs      ← build_lang_hint, build_ref_context...
├── goal.rs                  ← GoalClarity, validate_goal, goal_advisory_hints
├── plan_risk.rs             ← evaluate_plan_risk, plan_risk_feedback
└── validators.rs            ← validate_patch_uniqueness, validate_plan_integrity...
الإصلاحات المدمجة:

الإصلاح	الأثر
Planning guard لـ go.mod	أصلح smoke_go_generics
tsconfig jest/node types	أصلح smoke_ts_fastapi_client
تضييق TS semantic triggers	قلّل repair loops الزائفة
Rust E0422 visibility fix	أصلح smoke_rust_trait_impl
retry.ts new Promise	أصلح smoke_ts_retry
py_dataclass trajectory	أصلح smoke_py_dataclass
smoke --replay: 12/12 ✅

v9.2.1 — Plan Risk Telemetry (جزئي)
ExecutionContext:

plan_risk_triggered: bool
plan_risk_reasons: Vec<String>
replan_count: u32
commands_before_replan: usize
ExecutionReport:

tokens_in: u64
tokens_out: u64
plan_risk_triggered: bool
replan_count: u64
plan_risk_reasons: Vec<String>
7 Quality Fixes:

#	الملف	الإصلاح
1	state_handlers.rs	matched_pattern.as_ref()
2	repair_strategy.rs	is_test_file تضييق
3	evaluator.rs	guard ضد mutation_score = -1.0
4	executor/runner.rs	head+tail stderr capture
5	pattern_library.rs	normalize_signature() noise filter
6	agent.rs	latest_pattern_error() priority
7	executor/mutation.rs	replay_mode early return
v9.2.0 — Plan Risk Evaluation
evaluate_plan_risk() موصولة في do_planning()
replan_with_feedback() مع workspace_context حقيقي
SEL_DISABLE_PLAN_RISK=1 للـ A/B comparison
v9.1.0 — Prompt Quality + Goal Clarity
GoalClarity::analyze() — تصنيف وضوح الهدف
goal_advisory_hints() — تلميحات سياقية
validate_goal() — hard-fail فقط عند len < 10
build_budgeted_repair_prompt() — سقف 24,000 حرف
v9.0.0 — Adaptive Repair & Smart Context
Adaptive Repair Routing — 8 routes
Smart Repair Context — scored file selection + dependency graph
Pattern Library — PatternLibrary + RepairRoute + inference
force_include guarantee
Dependency Graph Caching
Head+tail stderr capture
Replay mutation safety
v8.x — الأساس
الإصدار	الميزة
v8.9.0	Wave 3: Pattern Library
v8.8.0	Reports + Observatory + Regression Gate + Dependency Graph
v8.5.2	Stable core + replay environment fix
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
  → build_planning_prompt() + hint
  → plan_with_resilience()
  → validate_plan_integrity()
  → validate_protected_writes()    ← يشمل go.mod منذ v9.2.5
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
  → pre_repair_checklist()         ← semantic shortcuts (bench_mode gate)
  → build_budgeted_repair_prompt()
     - route-aware instructions
     - pattern hint + example_fix
     - smart repair context (scored)
     - global cap (24,000 chars)
  → plan_with_resilience()
  → execution
  → record_pattern_outcome()
  → normalize_signature() → persist
هيكل الملفات
text

src/
├── agent.rs                     state machine loop
├── state_handlers.rs            planning / executing / repairing
├── decision.rs                  facade (re-exports فقط)
├── decision/
│   ├── checklist.rs             pre_repair_checklist
│   ├── context_builders.rs      build_lang_hint, build_workspace_context...
│   ├── goal.rs                  GoalClarity + validate_goal
│   ├── plan_risk.rs             evaluate_plan_risk
│   └── validators.rs            validate_patch_uniqueness, validate_plan_integrity...
├── repair_strategy.rs           route-aware repair prompts
├── constitution.rs              7 قواعد صلبة
├── protocol.rs                  Cmd + parser
├── types.rs                     AgentState / ExecutionContext
├── failure.rs                   FailureKind classification
├── pattern_library.rs           PatternLibrary + RepairRoute
├── diagnostic.rs                error diagnosis + hints
├── memory.rs                    FailureMemory + QuickFix
├── manifest.rs                  ProjectManifest + FilePolicy
├── evaluator.rs                 RawMetrics + ModelScore
├── scaffold_engine.rs           بيئات Python/TS/Go
├── report.rs                    ExecutionReport struct
├── report_writer.rs             كتابة التقارير
├── cost.rs                      تتبع tokens + USD
├── executor/
│   ├── core.rs                  SafeExecutor
│   ├── file_ops.rs              write/patch/append
│   ├── runner.rs                run_tests + head+tail stderr
│   ├── autofix.rs               deterministic fixes
│   ├── compile.rs               compile checks
│   ├── sanitizers.rs            Rust string sanitization
│   ├── mutation.rs              mutation testing + replay guard
│   └── parsers.rs               pytest / jest / Go / Rust output
├── llm/
│   ├── live.rs                  LiveProvider + SPO cascade
│   ├── record.rs                trajectory recorder
│   ├── replay.rs                trajectory replayer
│   └── key_pool.rs              API key rotation
├── context/
│   ├── builder.rs               scored file selection + token budget
│   └── scanner.rs               ProjectProfile detection
└── dependency_graph/
    ├── mod.rs                   DependencyGraph API
    ├── model.rs                 FileNode / DependencyEdge
    ├── builder.rs               build_for_workspace()
    └── parsers/                 Python / TypeScript / Go / Rust
6) الدستور — 7 قواعد صلبة
text

Rule 1: no-modify-tests      → لا تعديل لملفات اختبار موجودة
Rule 2: no-empty-write       → لا كتابة محتوى فارغ
Rule 3: no-binary-in-text    → لا بيانات binary في ملفات نصية
Rule 4: no-system-path       → لا مسارات نظام خطرة
Rule 5: no-overwrite-go-mod  → لا استبدال go.mod
Rule 6: no-dangerous-cmd     → لا أوامر خطرة
Rule 7: no-network-in-test   → لا شبكة في وضع replay
عند الانتهاك: CONSTITUTION_VIOLATION:<rule> → يوجّه repair loop لـ ForceSourceOnly.

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
8) Smart Repair Context — Scoring
Signal	النقاط
Focus path	+10
Culprit file	+8
Mentioned in stderr	+5
Graph: dependency of culprit	+4
Recently edited	+3
Graph: impacts culprit	+3
Imports errored file	+2
Graph: in dependency cycle	+2
Small file (< 200 tokens)	+1
Budget:

Local: MAX_REPAIR_TOKENS = 8000
Global: MAX_REPAIR_PROMPT_CHARS = 24,000
9) Plan Risk Evaluation
Rule	الخطر
write_file على test موجود	+0.50
patch_file على test موجود	+0.50
delete_file	+0.60
write_file على source موجود	+0.35
plan.len() >= 8	+0.20
Bash

SEL_DISABLE_PLAN_RISK=1  # A/B comparison
10) Pattern Library
Rust

pub struct Pattern {
    pub id: String,                    // language:hash
    pub language: String,
    pub error_signature: String,       // normalized, noise-filtered
    pub route: RepairRoute,
    pub success_count: u32,
    pub failure_count: u32,
    pub usage_count: u32,
    pub last_seen_utc: String,
    pub example_fix: Option<String>,
    pub failed_contexts: Vec<String>,
}

// is_strong() = usage_count >= 2 && success_rate >= 0.7
// التخزين: ~/.sel-agent/patterns.json
11) Telemetry Fields — الحالة الكاملة
الحقل	الموقع	القيمة	منذ
tokens_in	ExecutionReport	LLM input tokens	v9.2.1
tokens_out	ExecutionReport	LLM output tokens	v9.2.1
total_tokens	ExecutionReport	tokens_in + tokens_out	v9.2.6
avg_tokens_per_task	ExecutionReport	total_tokens / llm_calls	v9.2.6
plan_risk_triggered	ExecutionReport	bool	v9.2.1
replan_count	ExecutionReport	u64	v9.2.1
plan_risk_reasons	ExecutionReport	Vec<String>	v9.2.1
tokens_used	ExecutionContext	cumulative per run	v9.2.6
plan_confidence	ExecutionContext	None حتى v11.0	v9.2.6
12) مناطق الخطورة المعمارية
الملف	الخطر
src/constitution.rs	أي تغيير يمكّن تجاوز عقد الاختبار
src/state_handlers.rs	تعديل قد يتخطى Failed إلى Done بصمت
src/agent.rs	يملك القرار النهائي للـ ownership
src/pattern_library.rs	pattern خاطئ يصبح strong ويفسد إصلاحات المستقبل
src/decision/checklist.rs	semantic shortcuts خارج bench mode
fixtures/trajectories/	تعديل يدوي يكسر الحتمية
13) القيود الحالية المعروفة
القيد	الخطوة التالية
BudgetReport لا يصل إلى ExecutionReport	v9.3.0
Pattern Graduation غير موجود	v9.4.0
plan_confidence لا تُحسب بعد	v11.0
Pattern cross-project isolation	v9.8.0
Test Authoring validation	v9.5.0
Multi-file planning	v9.6.0
SWE-Bench external adapter	v9.7.0
evaluator.rs غير موصول بـ agent.rs	مستقبلي
14) الأوامر المرجعية
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
./target/release/sel-agent observatory --refresh-secs 5 --limit 50

# Pattern Library
cat ~/.sel-agent/patterns.json | python3 -m json.tool
15) سياسة Version Consistency
text

Cargo.toml                = مصدر النسخة الوحيد
src/commands/cli.rs       = env!("CARGO_PKG_VERSION")
src/main.rs               = env!("CARGO_PKG_VERSION")
لا أرقام نسخة ثابتة في الكود
16) الملخص التنفيذي
text

SEL Agent v9.2.6 — الحالة النهائية المؤكدة

cargo check:          ✅
cargo clippy:         ✅ 0 warnings
cargo test:           ✅
regression_gate full: ✅
smoke --replay:       12/12 ✅
bench all:            36/36 ✅
bench-swe:            30/30 ✅
bench-sel-v11:        18/18 ✅
bench-real-world:     14/14 ✅

Active runtime capabilities:
  ✅ adaptive repair routing (8 routes)
  ✅ repair loop escalation + streak detection
  ✅ smart repair context (scored + dependency graph)
  ✅ global prompt budget (24k chars)
  ✅ force_include guarantee
  ✅ dependency graph caching
  ✅ goal clarity analysis + goal_advisory_hints
  ✅ validate_goal — hard-fail فقط (len < 10)
  ✅ plan risk evaluation (connected)
  ✅ validate_protected_writes في planning
  ✅ replan_with_feedback — workspace_context حقيقي
  ✅ pattern memory + failed contexts
  ✅ noise-filtered pattern signatures
  ✅ head+tail stderr capture
  ✅ replay mutation safety
  ✅ bench_mode gate على semantic shortcuts
  ✅ decision.rs → facade + 5 submodules
  ✅ tokens_in / tokens_out / total_tokens / avg_tokens_per_task
  ✅ plan_risk telemetry كامل في reports

Next (بالترتيب):
  v9.3.0 → Context Budget Optimization    (2-3 أيام)
  v9.4.0 → Pattern Graduation → AutoFix   (4-5 أيام) ← الأعلى قيمة
  v9.5.0 → Test Authoring Validation      (4-5 أيام)
  v9.6.0 → Multi-File Coordinator         (7-10 أيام)
  v9.7.0 → SWE-Bench Adapter             (3 أسابيع)
  v9.8.0 → Pattern Library v2            (5-6 أيام)
  v9.9.0 → Failure Forensics             (4-5 أيام)
