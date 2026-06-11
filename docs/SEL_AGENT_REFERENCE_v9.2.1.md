# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v9.2.1
> **الحالة:** Stable — 7 quality fixes on top of adaptive repair + smart context + plan risk
> **الفرع:** phase2-safe
> **آخر تحديث:** 2026-06-11
> **الترخيص:** MIT

---

## 1) ما هو SEL Agent؟

SEL Agent هو **وكيل هندسة برمجيات مستقل** مكتوب بلغة Rust.
يستقبل هدفًا بالنص الطبيعي، ثم:

1. يحلل وضوح الهدف
2. يبني خطة تنفيذ
3. يقيّم مخاطر الخطة
4. يكتب أو يعدّل الملفات
5. يشغّل الاختبارات
6. يحلل الفشل ويوجه الإصلاح تكيفيًا
7. يمنع الدوران في حلقات إصلاح متكررة
8. يتعلم من الأنماط الناجحة والفاشلة
9. يعلن `SEL_SUCCESS` أو `SEL_FAILED`

---

## 2) الفلسفة الأساسية

| المبدأ | التفسير |
|--------|---------|
| الاختبار هو العقد | الوكيل لا يعدّل اختبارات موجودة أبدًا |
| الحتمية أولًا | replay و trajectories جزء أساسي من النظام |
| القياس قبل التحسين | لا ميزة بدون impact eval موثق |
| الإصلاح الجذري | نصلح السبب البنيوي لا العرض السطحي |
| تقليل الاعتماد على LLM | ننقل المعرفة إلى engine عبر routing + patterns |
| المعرفة في الكود | لا patch scripts، لا .bak files، Git فقط |

---

## 3) الحالة الحالية المؤكدة

| المقياس | النتيجة |
|---------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 0 warnings |
| `cargo test` | ✅ |
| `bash scripts/regression_gate.sh core` | ✅ |
| `bash scripts/regression_gate.sh full` | ✅ |
| `bench --suite all --replay` | **36/36** ✅ |
| `bench-swe --lang all --replay` | **30/30** ✅ |
| `bench-sel-v11 --replay` | **18/18** ✅ |
| `bench-real-world --replay` | **14/14** ✅ |
| `smoke --replay` | **12/12** ✅ |

---

## 4) سجل الإصدارات

### v9.2.1 — Quality Fixes

#### الإصلاحات السبعة المؤكدة

| # | الملف | الإصلاح | الأثر |
|---|-------|---------|-------|
| 1 | `src/state_handlers.rs` | `matched_pattern.as_ref()` بدل `.map()` المباشر | pattern hints و example_fix تصل فعلياً إلى build_prompt |
| 2 | `src/repair_strategy.rs` | `is_test_file` — إزالة `name.contains("test")` العامة | ملفات مثل `test_helper.rs` لم تعد تُصنَّف كـ test files |
| 3 | `src/evaluator.rs` | guard ضد `mutation_score = -1.0` | correctness لا تعطي قيمة سالبة عند غياب mutation data |
| 4 | `src/executor/runner.rs` | `capture_stderr()` — head+tail بدل tail فقط | الأخطاء في منتصف output لم تعد تضيع |
| 5 | `src/pattern_library.rs` | `normalize_signature()` تتجاوز noise lines | patterns.json لن تتلوث بـ test runner output |
| 6 | `src/agent.rs` | `latest_pattern_error()` — إضافة `last_failed_steps` كـ priority 2 | patterns تسجّل أخطاء حقيقية لا stdout ناجح |
| 7 | `src/executor/mutation.rs` | `replay_mode` early return | mutation check لا يشغّل tests حقيقية خارج trajectory |

#### ملفات أخرى
- `fixtures/trajectories/index.json` — تحديث trajectory لـ PY-03 بعد تغيّر السلوك

---

### v9.0.0 — Adaptive Repair & Smart Context

- Adaptive Repair Routing — `infer_route_from_stderr()` + `effective_route`
- Repair Loop Escalation — `error_fingerprint()` + streak detection
- Smart Repair Context — scored file selection + token budget
- Recent Edits Tracking — dedup + cap=16
- Dependency Graph Caching — يُبنى مرة واحدة لكل workspace
- Rust Dependency Parser Improvements

---

### v9.1 phase 2 — Plan Risk Evaluation

- `PlanRiskReport` — تقييم مخاطر الخطة قبل التنفيذ
- قواعد التقييم: write_file على test (+0.50)، delete_file (+0.60)، إلخ
- `SEL_DISABLE_PLAN_RISK=1` للتعطيل

---

### v9.1 phase 1 — Prompt Quality

- `GoalClarity::analyze()` — تصنيف وضوح الهدف
- `build_budgeted_repair_prompt()` — سقف 24000 حرف مع أولويات تقليص
- `failed_contexts` في Pattern — backward compatible
- `CARGO_NET_OFFLINE=true` في replay mode

---

## 5) البنية المعمارية

### دورة الحالة
Planning → Executing → Repairing → Done / Failed

text


### المسار الكامل للتخطيط
goal
→ validate_goal()
→ GoalClarity::analyze()
→ build_planning_prompt() + clarity_hint
→ plan_with_resilience()
→ validate_plan_integrity()
→ validate_patch_uniqueness()
→ evaluate_plan_risk()
→ replan_with_feedback() إذا لزم
→ constraint_engine::apply()
→ dedup write_file
→ AgentState::Executing

text


### المسار الكامل للإصلاح
stderr
→ classify FailureKind
→ error_fingerprint + streak detection
→ infer_route_from_stderr() OR matched_pattern.as_ref().route
→ effective_route
→ build_budgeted_repair_prompt()
→ attempt_bundle (route-aware)
→ smart repair context (scored + budget)
→ global prompt cap (24,000 chars)
→ plan_with_resilience()
→ execution
→ record_pattern_outcome()
→ latest_pattern_error()
→ failed_steps → last_failed_steps → error_history → failure_reason
→ normalize_signature() → skip noise lines

text


---

## 6) الملفات الأساسية

| الملف | الدور |
|-------|-------|
| `src/main.rs` | نقطة الدخول والـ CLI |
| `src/agent.rs` | state machine loop |
| `src/state_handlers.rs` | planning / executing / repairing |
| `src/decision.rs` | GoalClarity + PlanRisk + validators |
| `src/repair_strategy.rs` | route-aware repair prompts |
| `src/protocol.rs` | Cmd + parser |
| `src/types.rs` | AgentState / ExecutionContext |
| `src/constitution.rs` | القواعد الصلبة |
| `src/constraint_engine.rs` | plan filtering |
| `src/pattern_library.rs` | PatternLibrary + route inference |
| `src/context/builder.rs` | scored file selection + token budget |
| `src/executor/runner.rs` | تشغيل الاختبارات + offline replay |
| `src/executor/mutation.rs` | mutation check + replay guard |
| `src/evaluator.rs` | bench metrics + correctness scoring |
| `src/dependency_graph/` | graph API + parsers |

---

## 7) Adaptive Repair Routing

| Pattern في stderr | Route |
|-------------------|-------|
| `constitution_violation:no-modify-tests` | ForceSourceOnly |
| `circular import` | CircularImport |
| `no module named` / `cannot find module` | MissingDependency |
| `undefined:` / `is not defined` | FunctionDeleted |
| `cannot borrow` / `borrowed value` | RustOwnership |
| `nil pointer` / `nullreference` / `nonetype` | NullGuard |
| `mismatched types` / `typeerror` | TypeMismatch |
| غير ذلك | Generic |

---

## 8) Smart Repair Context

### Scoring System
| Signal | النقاط |
|--------|--------|
| Focus path | +10 |
| Culprit file | +8 |
| Mentioned in error | +5 |
| Graph: dependency of culprit | +4 |
| Recently edited | +3 |
| Graph: impacts culprit | +3 |
| Imports errored file | +2 |
| Graph: in dependency cycle | +2 |
| Small file | +1 |

### Budget
- Local: `MAX_REPAIR_TOKENS = 8000`
- Global: `MAX_REPAIR_PROMPT_CHARS = 24000`

---

## 9) Plan Risk Evaluation

| Rule | الخطر |
|------|-------|
| write_file على test موجود | +0.50 |
| patch_file على test موجود | +0.50 |
| delete_file | +0.60 |
| write_file على source موجود | +0.35 |
| plan.len() >= 8 | +0.20 |

```bash
SEL_DISABLE_PLAN_RISK=1  # للتعطيل في A/B comparison
10) Pattern Library
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
is_strong()
Rust

usage_count >= 2 && success_rate >= 0.7
Noise-filtered signatures
normalize_signature() تتجاوز:

=== RUN / --- PASS / --- FAIL
test session starts
Compiling / Finished / Updating crates.io
test result: / running / Downloading
11) مناطق الخطورة المعمارية
الملف	الخطر
src/constitution.rs	يغير القواعد الصلبة للوكيل
src/state_handlers.rs	قلب آلة الحالة
src/agent.rs	يملك القرار النهائي
src/pattern_library.rs	قاعدة التعلم — pattern خاطئ يفسد الإصلاحات
fixtures/trajectories/	عقود replay — لا تعدل يدويًا
12) الأوامر المرجعية
التطوير اليومي
Bash

cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
قبل كل milestone
Bash

cargo test
cargo clippy --all-targets --all-features -- -D warnings
bash scripts/regression_gate.sh core
قبل كل release
Bash

bash scripts/regression_gate.sh full
Benchmarks
Bash

cargo build --release
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay
13) سياسة Version Consistency
Cargo.toml = مصدر النسخة الوحيد
src/commands/cli.rs = env!("CARGO_PKG_VERSION")
src/main.rs = env!("CARGO_PKG_VERSION")
لا أرقام نسخة ثابتة في الكود
14) القيود الحالية
Pattern Graduation → AutoFix: لم يُبنَ بعد
match_score يعتمد على prefix فقط
quick_fix يدعم 11 package فقط
Test Authoring validation: لم تُبنَ بعد
evaluator.rs غير موصول بـ agent.rs مباشرة
15) الخطوة التالية — v9.2.1
Pattern Graduation → AutoFix
text

Pattern
  is_graduate() = usage_count >= 10 && success_rate >= 0.95
  → to_autofix_rule()
  → DeterministicFix (no LLM call)
الفائدة المتوقعة: تقليل LLM calls بنسبة 15-30% للأخطاء الشائعة.

16) الملخص التنفيذي
text

SEL Agent v9.2.1 — Verified

cargo check:    ✅
cargo clippy:   ✅ 0 warnings
cargo test:     ✅
regression_gate full: ✅
  bench all:        36/36
  bench-swe:        30/30
  bench-sel-v11:    18/18
  bench-real-world: 14/14
  smoke:            12/12

Active runtime features:
- adaptive repair routing (8 routes)
- repair loop escalation + streak detection
- smart repair context (scored + dependency graph)
- global prompt budget (24k chars)
- dependency graph caching
- recent edit tracking
- goal clarity analysis
- plan risk evaluation
- pattern memory + failed contexts
- noise-filtered pattern signatures
- head+tail stderr capture
- replay mutation safety
- feature impact evaluation protocol

Fixed in v9.2.1:
- matched_pattern hint loss (as_ref fix)
- stderr tail-only capture
- is_test_file false positives
- evaluator negative mutation score
- pattern signature noise pollution
- latest_pattern_error priority
- mutation check in replay mode

Next: v9.2.1 Pattern Graduation → AutoFix


## 14.1 v9.2.1 — Plan Risk Telemetry

### ما أُضيف فعلياً
- `ExecutionContext`:
  - `plan_risk_triggered: bool`
  - `plan_risk_reasons: Vec<String>`
  - `replan_count: u32`
  - `commands_before_replan: usize`
- `ExecutionReport`:
  - `plan_risk_triggered: bool`
  - `replan_count: u64`
  - `plan_risk_reasons: Vec<String>`

### Wiring المؤكد
- تسجيل plan risk telemetry في `do_planning()`
- زيادة `replan_count` في `replan_with_feedback()`
- تحديث `plan_risk_reasons` أيضاً أثناء replans اللاحقة
- تمرير الحقول إلى `report_run()` وكتابتها في `~/.sel-agent/reports/latest.json`

### التحقق
- `cargo test` = **323/323**
- `regression_gate.sh core` = ✅
- report JSON يحتوي الحقول الثلاثة الجديدة = ✅

