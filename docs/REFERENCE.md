# SEL Agent — الوثيقة المرجعية

> **الإصدار الحالي:** v9.3.5
> **التاريخ:** 2026-08-18
> **الحالة:** Stable
> **الترخيص:** MIT

---

## 1. ما هو SEL Agent؟

SEL Agent هو **وكيل هندسة برمجيات مستقل** (Autonomous Software Engineering Agent) مكتوب بـ Rust.
يستقبل هدفاً بالنص الطبيعي ثم ينفّذه كاملاً بلا تدخل بشري:

1. يحلّل وضوح الهدف
2. يبني خطة تنفيذ ويقيّم مخاطرها
3. يكتب أو يعدّل الملفات
4. يشغّل الاختبارات
5. يحلّل الفشل ويوجّه الإصلاح تكيفياً
6. يمنع الدوران في حلقات إصلاح متكررة
7. يتعلّم من الأنماط الناجحة والفاشلة
8. يعلن `SEL_SUCCESS` أو `SEL_FAILED`

### الفلسفة الأساسية

| المبدأ | التفسير |
|--------|---------|
| الاختبار هو العقد | الوكيل لا يعدّل اختبارات موجودة أبداً |
| الحتمية أولاً | replay و trajectories جزء غير قابل للتفاوض |
| القياس قبل التحسين | لا ميزة بدون impact eval موثَّق |
| الإصلاح الجذري | نصلح السبب البنيوي لا العرض السطحي |
| تقليل الاعتماد على LLM | نقل المعرفة إلى engine عبر routing + patterns + autofix |
| المعرفة في الكود | لا patch scripts، لا `.bak` files — Git فقط |
| الفصل بين Milestones | refactor في milestone منفصل عن feature |
| لا تخمين | نعمل على الكود الحقيقي والمخرجات الحقيقية فقط |

---

## 2. الدستور — 7 قواعد صلبة

يطبّقها `src/constitution.rs` على كل عملية كتابة أو أمر:

| # | القاعدة | الوصف |
|---|---------|-------|
| 1 | **no-modify-tests** | لا يُعدَّل أي ملف اختبار موجود أبداً |
| 2 | **no-empty-write** | لا يُكتب محتوى فارغ |
| 3 | **no-binary-in-text** | لا bytes ثنائية في ملفات نصية |
| 4 | **no-system-path** | الكتابة فقط ضمن الـ workspace |
| 5 | **no-overwrite-go-mod** | `go.mod` محمي — hard reject في planning |
| 6 | **no-dangerous-cmd** | يمنع: `rm -rf .` / `..` / `*` / `~` وما شابهها |
| 7 | **no-network-in-test** | لا شبكة أثناء وضع الـ replay |

> قاعدة قادمة في v9.5.0: **Rule 8: authored-test-quality** — mutation validation للاختبارات المؤلفة.

---

## 3. المعمارية — هيكل الملفات

```
sel-agent-engine/
├── src/
│   ├── main.rs                  # نقطة الدخول + CLI (clap)
│   ├── lib.rs                   # pub mod declarations
│   ├── agent.rs                 # حلقة التنفيذ الرئيسية + final outcome
│   ├── state_handlers.rs        # State Machine — معالجة كل حالة
│   ├── constitution.rs          # Rules 1-7 (hard enforcement)
│   ├── failure.rs               # تصنيف الأخطاء (FailureKind)
│   ├── repair_strategy.rs       # بناء prompt الإصلاح + routing
│   ├── protocol.rs              # تحليل JSON من LLM
│   ├── types.rs                 # أنواع البيانات المشتركة (ExecutionContext)
│   ├── goal_parser.rs           # استخلاص نوع المهمة من الـ Goal
│   ├── diagnostic.rs            # تحليل stderr → تلميح تشخيصي
│   ├── chunker.rs               # تقطيع الملفات للسياق
│   ├── constraint_engine.rs     # قيود البيئة (Python/Node/Go/Rust)
│   ├── pattern_library.rs       # مكتبة الأنماط + adaptive routing
│   ├── memory.rs                # ذاكرة الجلسة
│   ├── manifest.rs              # trajectory manifest
│   ├── scaffold_engine.rs       # scaffold البيئات
│   ├── report.rs                # ExecutionReport (JSON نهائي)
│   ├── report_writer.rs         # كتابة التقارير على القرص
│   ├── cost.rs                  # تتبع تكلفة LLM
│   ├── provider.rs              # إدارة مزودي LLM + SPO
│   ├── provider_state.rs        # حالة المزودين
│   ├── bench_sel.rs             # Bench suite (36 مهمة)
│   │
│   ├── decision/                # facade + 5 submodules (v9.2.5)
│   │   ├── mod.rs               # facade
│   │   ├── checklist.rs         # Pre-repair checklist (الإصلاحات الحتمية)
│   │   ├── context_builders.rs  # بناء سياق الإصلاح للـ LLM
│   │   ├── goal.rs              # GoalClarity + validate_goal
│   │   ├── plan_risk.rs         # تقييم مخاطر الخطة
│   │   └── validators.rs        # patch_uniqueness + plan_integrity
│   │
│   ├── executor/
│   │   ├── core.rs              # SafeExecutor — تنفيذ الأوامر
│   │   ├── file_ops.rs          # write_file / patch_file
│   │   ├── compile.rs           # فحص تجميع Go/Rust بعد كل كتابة
│   │   ├── autofix.rs           # إصلاحات تلقائية (Go/Rust)
│   │   ├── sanitizers.rs        # تنظيف Unicode / Python quotes
│   │   ├── parsers.rs           # تحليل مخرجات Jest/pytest/cargo
│   │   ├── runner.rs            # تشغيل الأوامر + head+tail stderr capture
│   │   ├── mutation.rs          # replay mutation safety
│   │   └── bench_bugs.rs        # bench-specific bug coverage
│   │
│   ├── llm/
│   │   ├── mod.rs
│   │   ├── live.rs              # استدعاء LLM الحي
│   │   ├── record.rs            # تسجيل trajectories
│   │   ├── replay.rs            # إعادة تشغيل trajectories
│   │   ├── key_pool.rs          # إدارة مفاتيح API
│   │   └── json_sanitizer.rs    # تنظيف JSON من LLM
│   │
│   ├── context/
│   │   ├── builder.rs           # Smart Context Builder → (String, BudgetReport)
│   │   └── scanner.rs           # مسح الـ workspace
│   │
│   └── dependency_graph/
│       ├── mod.rs
│       ├── model.rs
│       ├── builder.rs
│       └── parsers/             # Python / TS / Go / Rust
│
├── tests/
│   ├── executor_tests.rs        # integration tests (7 passed)
│   └── evidence_wave2.rs        # Wave 2 evidence (4 passed)
│
├── fixtures/
│   ├── trajectories/            # سجلات تنفيذ للـ replay (لا تُعدَّل يدوياً)
│   └── workspaces/              # بيئات scaffold جاهزة
│
├── scripts/
│   └── regression_gate.sh       # بوابة الجودة: core / smoke / full
│
├── docs/
│   ├── REFERENCE.md             # هذه الوثيقة
│   ├── EVIDENCE_MATRIX.md       # claim → evidence registry
│   ├── FEATURE_IMPACT_PROTOCOL.md
│   ├── MAINTAINER_NOTES.md
│   └── architecture/
│
├── evals/                       # feature impact evaluations
├── sel_smoke_test.sh            # Smoke test (12 مهمة حقيقية)
├── Cargo.toml                   # مصدر الحقيقة للإصدار
├── LICENSE                      # MIT
└── CONTRIBUTING.md
```

---

## 4. دورة حياة المهمة

### المسار الكامل للتخطيط

```
goal (نص)
  → validate_goal()
  → GoalClarity::analyze()
  → goal_advisory_hints()
  → build_planning_prompt()
  → plan_with_resilience()
  → validate_plan_integrity()
  → validate_protected_writes()    ← يشمل go.mod
  → validate_patch_uniqueness()
  → evaluate_plan_risk()
  → replan_with_feedback()         ← إن risk > threshold
  → constraint_engine::apply()
  → dedup write_file
  → AgentState::Executing
```

### المسار الكامل للإصلاح

```
stderr
  → classify FailureKind
  → error_fingerprint + streak detection
  → infer_route_from_stderr()
     OR matched_pattern.as_ref().route
  → effective_route
  → pre_repair_checklist()         ← الإصلاحات الحتمية أولاً
  → build_budgeted_repair_prompt()
     - route-aware instructions
     - pattern hint + example_fix
     - smart repair context (scored)
     - global cap (24,000 chars)
  → plan_with_resilience()
  → execution
  → record_pattern_outcome()
  → normalize_signature() → persist
  → BudgetReport → ExecutionContext  ← telemetry
```

### نتائج المهمة

| الحالة | المعنى |
|--------|--------|
| `SEL_SUCCESS` | جميع الاختبارات نجحت |
| `SEL_FAILED` | استُنفدت محاولات الإصلاح (افتراضي: 5) |
| `TRAJECTORY_INCOMPLETE` | replay يتوقع بيئة غير موجودة |

---

## 5. Pre-Repair Checklist (`decision/checklist.rs`)

تُنفَّذ قبل كل استدعاء LLM — إصلاحات حتمية بدون LLM:

| الفحص | الوصف | النطاق |
|-------|--------|--------|
| Check 1 | إضافة `run_tests` المفقودة | جميع الأوضاع |
| Check 2 | Python `NameError` → auto-import | جميع الأوضاع |
| Check 3 | Rust E0762 — Unicode lifetime quotes | جميع الأوضاع |
| Check 3b | Rust E0422 — missing `pub` (يدعم sub-crates) | جميع الأوضاع |
| Check 4 | `patch_file` → `write_file` strategy switch | جميع الأوضاع |
| Check 5 | `Cargo.toml` corruption recovery | جميع الأوضاع |
| Check 5b | Python pip install stdlib → skip | جميع الأوضاع |
| Check 6 | TS retry fix (source-only, Rule-1 safe) | جميع الأوضاع |
| Check 7-8 | Go worker-pool + TS api-client | bench فقط |

---

## 6. سلسلة معالجة `write_file`

```
content (raw)
  ↓
sanitize_code()                  ← Unicode smart quotes → ASCII
  ↓
  if .rs → sanitize_rust_lifetime_quotes()
           fix_rust_string_literals()
  if .py → fix_python_string_quoting()
  ↓
constitution::check_write()      ← Rules 1-7
  ↓
std::fs::write()
  ↓
  if .go → go_compile_check()
           autofix_go_*()
```

---

## 7. الإصلاحات التلقائية (`executor/autofix.rs`)

| الدالة | المشكلة | اللغة |
|--------|---------|-------|
| `autofix_go_worker_pool_deadlock` | goroutine deadlock | Go |
| `autofix_go_missing_comma` | missing comma في composite literal (يدعم `_test.go`) | Go |
| `autofix_go_unused_import` | unused import | Go |
| `autofix_go_test_run_shadow_alias` | shadowed `t` | Go |
| `autofix_go_test_table_shadow_run` | `t.Run` shadow | Go |
| `autofix_go_undefined_import` | undefined import | Go |

---

## 8. Sanitizers (`executor/sanitizers.rs`)

| الدالة | الوصف |
|--------|-------|
| `sanitize_code` | Unicode smart quotes → ASCII |
| `sanitize_rust_lifetime_quotes` | `"static` → `&'static` |
| `fix_rust_string_literals` | single-quoted strings → double |
| `fix_python_string_quoting` | `assert '...'` → `assert "..."` |
| `sanitize_go_mod_content` | `-go 1.21` → `go 1.21` |
| `fix_toml_duplicates` | مفاتيح مكررة في `Cargo.toml` |

---

## 9. Adaptive Repair Routing (`pattern_library.rs`)

| Pattern في stderr | Route |
|-------------------|-------|
| `CONSTITUTION_VIOLATION:no-modify-tests` | `ForceSourceOnly` |
| `circular import` | `CircularImport` |
| `no module named` / `cannot find module` | `MissingDependency` |
| `undefined:` / `is not defined` | `FunctionDeleted` |
| `cannot borrow` / `borrowed value` | `RustOwnership` |
| `nil pointer` / `NoneType` | `NullGuard` |
| `mismatched types` / `TypeError` | `TypeMismatch` |
| `TS2459` / `TS1192` | `ts/api-client-export` |
| `PromiseRejectionHandledWarning` | `ts/retry-unhandled-rejection` |
| غير ذلك | `Generic` |

---

## 10. Smart Repair Context — Scoring (v9.3.0)

الملفات تُختار بالنقاط — الأعلى نقاطاً يدخل ضمن budget:

| Signal | النقاط |
|--------|--------|
| Focus path | +10 |
| Culprit file | +8 |
| Mentioned in stderr | +5 |
| stderr occurrences > 1 | +2 لكل تكرار إضافي (max +6) |
| Dependency of culprit (graph) | +4 |
| Recently edited | +3 |
| Impacts culprit (graph) | +3 |
| Imports errored file | +2 |
| In dependency cycle (graph) | +2 |
| Small file (< 200 tokens) | +1 |

**Budget:**
- Local: `MAX_REPAIR_TOKENS = 8,000`
- Global: `MAX_REPAIR_PROMPT_CHARS = 24,000`

---

## 11. Context Budget Telemetry (v9.3.0)

كل repair loop ينتج `BudgetReport`:

```rust
BudgetReport {
    total_files:            usize,
    selected_files:         usize,
    tokens_before:          usize,
    tokens_after:           usize,
    force_include_dropped:  Vec<String>,  // ملفات force_include لم تُحمَّل
}
```

يُجمَّع في `ExecutionContext` → يُحسب متوسط في `agent.rs` → يُكتب في `ExecutionReport` JSON:

```json
{
  "avg_context_tokens": 0,
  "avg_selected_files": 0,
  "context_reduction_pct": 0,
  "force_include_dropped_count": 0
}
```

> القيم = 0 في runs بدون repairs — سلوك صحيح.

---

## 12. Telemetry Fields — الحالة الكاملة

| الحقل | الموقع | منذ |
|-------|--------|-----|
| `tokens_in` | `ExecutionReport` | v9.2.1 |
| `tokens_out` | `ExecutionReport` | v9.2.1 |
| `total_tokens` | `ExecutionReport` | v9.2.6 |
| `avg_tokens_per_task` | `ExecutionReport` | v9.2.6 |
| `plan_risk_triggered` | `ExecutionReport` | v9.2.1 |
| `replan_count` | `ExecutionReport` | v9.2.1 |
| `plan_risk_reasons` | `ExecutionReport` | v9.2.1 |
| `tokens_used` | `ExecutionContext` | v9.2.6 |
| `plan_confidence` | `ExecutionContext` | v9.2.6 (None حتى v11.0) |
| `avg_context_tokens` | `ExecutionReport` | v9.3.0 |
| `avg_selected_files` | `ExecutionReport` | v9.3.0 |
| `context_reduction_pct` | `ExecutionReport` | v9.3.0 |
| `force_include_dropped_count` | `ExecutionReport` | v9.3.0 |

---

## 13. Plan Risk Evaluation (`decision/plan_risk.rs`)

| الشرط | الأثر |
|-------|-------|
| `write_file` على test موجود | +0.50 |
| `patch_file` على test موجود | +0.50 |
| `delete_file` | +0.60 |
| `write_file` على source موجود | +0.35 |
| `plan.len() >= 8` | +0.20 |
| `write_file` على `go.mod` | **PLAN ERROR** (hard reject) |

```bash
SEL_DISABLE_PLAN_RISK=1  # للمقارنة A/B فقط
```

---

## 14. Pattern Library (`pattern_library.rs`)

```rust
pub struct Pattern {
    pub id:               String,
    pub language:         String,
    pub error_signature:  String,
    pub route:            RepairRoute,
    pub success_count:    u32,
    pub failure_count:    u32,
    pub usage_count:      u32,
    pub last_seen_utc:    String,
    pub example_fix:      Option<String>,
    pub failed_contexts:  Vec<String>,
}

// is_strong() = usage_count >= 2 && success_rate >= 0.7
```

**القيود الحالية:**
- `is_graduate()` غير موجود ← v9.4.0
- `success_rate` الخام يضخّم الثقة ← v9.8.0
- التواقيع مرتبطة بمشروع واحد ← v9.8.0

---

## 15. مزودو LLM (`src/provider.rs`)

| # | المزود | النموذج | الدور |
|---|--------|---------|-------|
| 1 | Groq | openai/gpt-oss-120b | افتراضي |
| 2 | Gemini | models/gemini-3.6-flash | fallback |
| 3 | Cerebras | gpt-oss-120b | fallback |
| 4 | OpenRouter | متعدد | fallback |

---

## 16. Smoke Tests (12 مهمة)

| المهمة | اللغة | النتيجة |
|--------|-------|---------|
| `smoke_py_binary_search` | Python | ✅ |
| `smoke_py_temperature` | Python | ✅ |
| `smoke_py_dataclass` | Python | ✅ |
| `smoke_go_worker_pool` | Go | ✅ |
| `smoke_go_generics` | Go | ✅ |
| `smoke_go_concurrent` | Go | ✅ |
| `smoke_ts_utils` | TypeScript | ✅ |
| `smoke_ts_retry` | TypeScript | ✅ |
| `smoke_ts_fastapi_client` | TypeScript | ✅ |
| `smoke_rust_csv` | Rust | ✅ |
| `smoke_rust_fib` | Rust | ✅ |
| `smoke_rust_trait_impl` | Rust | ✅ |

---

## 17. مناطق الخطورة المعمارية

| الملف | الخطر |
|-------|-------|
| `src/constitution.rs` | أي تغيير يمكّن تجاوز عقد الاختبار |
| `src/state_handlers.rs` | تعديل قد ينقل `Repairing` → `Done` بصمت |
| `src/agent.rs` | يملك القرار النهائي للتنفيذ |
| `src/pattern_library.rs` | pattern خاطئ يُفسد إصلاحات المستقبل |
| `src/decision/checklist.rs` | semantic shortcuts خارج bench mode |
| `fixtures/trajectories/` | تعديل يدوي يكسر الحتمية |

---

## 18. القيود الحالية المعروفة وخارطة الطريق

| القيد | الخطوة التالية |
|-------|---------------|
| Pattern Graduation غير موجود | v9.4.0 |
| Test Authoring validation | v9.5.0 |
| Multi-file planning | v9.6.0 |
| SWE-Bench external adapter | v9.7.0 |
| Pattern cross-project | v9.8.0 |
| `plan_confidence = None` | v11.0 |
| `evaluator.rs` غير موصول | مستقبلي |

---

## 19. المتغيرات البيئية

| المتغير | الوصف | الإلزامية |
|---------|-------|-----------|
| `GROQ_API_KEY` | مفتاح Groq | **إلزامي** |
| `GEMINI_API_KEY` | مفتاح Gemini (fallback) | موصى به |
| `CEREBRAS_API_KEY` | مفتاح Cerebras (fallback) | اختياري |
| `OPENROUTER_API_KEY` | مفتاح OpenRouter (fallback) | اختياري |
| `SEL_BENCH_MODE` | تفعيل bench-only semantic fixes | اختياري |
| `SEL_MAX_REPAIRS` | الحد الأقصى لمحاولات الإصلاح | افتراضي: `5` |
| `SEL_WORKSPACE` | مسار workspace مخصص | اختياري |
| `SEL_DISABLE_PLAN_RISK` | تعطيل Plan Risk (A/B فقط) | اختياري |

---

## 20. أرقام الجودة — v9.3.5

| المقياس | القيمة |
|---------|--------|
| `src/lib.rs` unit tests | **151 passed** |
| `src/main.rs` unit tests | **206 passed** |
| `tests/evidence_wave2.rs` | **4 passed** |
| `tests/executor_tests.rs` | **7 passed** |
| bench suite (all) | **36/36** |
| bench-swe | **30/30** |
| bench-sel-v11 | **18/18** |
| bench-real-world | **14/14** |
| smoke test | **12/12 — 100%** |
| `cargo clippy -D warnings` | **0 errors** |
| `cargo fmt` | **clean** |
| regression gate full | **✅ PASSED** |

---

## 21. الأوامر المرجعية

```bash
# ── الجودة اليومية ──────────────────────────────────────
cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# ── قبل كل milestone ────────────────────────────────────
bash scripts/regression_gate.sh core

# ── قبل كل release ──────────────────────────────────────
bash scripts/regression_gate.sh full

# ── Benchmarks ──────────────────────────────────────────
cargo build --release
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay

# ── Evidence ─────────────────────────────────────────────
cargo test evidence_

# ── Observability ────────────────────────────────────────
./target/release/sel-agent report --latest
./target/release/sel-agent report --summary
cat ~/.sel-agent/patterns.json | python3 -m json.tool

# ── Context budget في التقرير ────────────────────────────
cat ~/.sel-agent/reports/latest.json | python3 -m json.tool \
  | grep -E '"avg_context_tokens"|"avg_selected_files"|"context_reduction_pct"|"force_include_dropped_count"'
```

---

*SEL Agent v9.3.5 — Autonomous Execution Engine*
*smoke: 12/12 · bench: 36/36 · regression: full PASSED · clippy: clean*
