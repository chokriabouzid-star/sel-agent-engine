# SEL Agent — الوثيقة المرجعية v9.3.2

**التاريخ:** 2026-06-18  
**الإصدار:** v9.3.2  
**الفرع:** refactor/v9.2.5-decision-split  
**الـ commit:** 0997385  

---

## 1. نظرة عامة

SEL Agent هو محرك تنفيذ استقلالي (Autonomous Execution Engine) مبني بـ Rust.  
مهمته: استقبال هدف برمجي (Goal) وتنفيذه كاملاً — كتابة الكود، تشغيل الاختبارات، إصلاح الأخطاء — بشكل تلقائي دون تدخل بشري.

### المبادئ الأساسية (الدستور)

| # | القاعدة | الوصف |
|---|---------|-------|
| 1 | **no-modify-tests** | لا يُعدَّل أي ملف اختبار موجود أبداً |
| 2 | **no-empty-write** | لا يُكتب محتوى فارغ |
| 3 | **no-binary-in-text** | لا bytes ثنائية في ملفات نصية |
| 4 | **no-system-path** | الكتابة فقط ضمن الـ workspace |
| 5 | **no-overwrite-go-mod** | `go.mod` محمي |
| 6 | **no-dangerous-cmd** | لا أوامر خطرة |
| 7 | **no-network-in-test** | لا شبكة أثناء الاختبارات |

---

## 2. معمارية المشروع
sel-agent-engine/
├── src/
│ ├── main.rs # نقطة الدخول + CLI
│ ├── lib.rs # pub mod declarations
│ ├── agent.rs # حلقة التنفيذ الرئيسية
│ ├── state_handlers.rs # State Machine — معالجة كل حالة
│ ├── constitution.rs # القواعد الصارمة (Rules 1-7)
│ ├── failure.rs # تصنيف الأخطاء (FailureKind)
│ ├── repair_strategy.rs # بناء prompt الإصلاح + routing
│ ├── protocol.rs # تحليل JSON من LLM
│ ├── types.rs # أنواع البيانات المشتركة
│ ├── goal_parser.rs # استخلاص نوع المهمة من الـ Goal
│ ├── diagnostic.rs # تحليل stderr → تلميح تشخيصي
│ ├── chunker.rs # تقطيع الملفات للسياق
│ ├── constraint_engine.rs # قيود البيئة (Python/Node/Go/Rust)
│ ├── report.rs # تقارير JSON نهائية
│ │
│ ├── decision/
│ │ ├── mod.rs
│ │ ├── checklist.rs # Pre-repair checklist (الإصلاحات الحتمية)
│ │ ├── plan_risk.rs # تقييم مخاطر الخطة
│ │ └── context_builders.rs # بناء سياق الإصلاح للـ LLM
│ │
│ ├── executor/
│ │ ├── core.rs # SafeExecutor — تنفيذ الأوامر
│ │ ├── file_ops.rs # write_file / patch_file
│ │ ├── compile.rs # فحص تجميع Go/Rust بعد كل كتابة
│ │ ├── autofix.rs # إصلاحات تلقائية (Go/Rust)
│ │ ├── sanitizers.rs # تنظيف Unicode / Python quotes
│ │ ├── parsers.rs # تحليل مخرجات Jest/pytest/cargo
│ │ └── bench_bugs.rs # اختبارات bench-specific bugs
│ │
│ ├── llm/
│ │ ├── mod.rs
│ │ └── json_sanitizer.rs # تنظيف JSON من LLM
│ │
│ ├── provider.rs # إدارة مزودي LLM + SPO
│ ├── provider_state.rs # حالة المزودين
│ ├── pattern_library.rs # مكتبة الأنماط + adaptive routing
│ ├── dependency_graph/ # رسم بياني للتبعيات
│ │ ├── mod.rs
│ │ ├── builder.rs
│ │ └── parsers/
│ ├── context/
│ │ └── builder.rs
│ └── bench_sel.rs # Bench suite (36 tasks)
│
├── tests/
│ ├── executor_tests.rs # integration tests
│ └── evidence_wave2.rs # Wave 2 evidence checks
│
├── fixtures/
│ ├── trajectories/ # سجلات تنفيذ للـ replay
│ └── workspaces/ # بيئات scaffold جاهزة
│
├── scripts/
│ └── regression_gate.sh # بوابة الجودة (core/smoke/full)
│
└── sel_smoke_test.sh # Smoke test (12 مهمة حقيقية)

text


---

## 3. دورة حياة المهمة
Goal (نص)
↓
[agent.rs] init workspace + scaffold
↓
[state_handlers.rs] State: Planning
↓
[repair_strategy.rs] build_prompt → LLM call
↓
[protocol.rs] parse JSON plan
↓
[decision/plan_risk.rs] risk assessment
↓
[executor/core.rs] execute commands
↓
Tests Pass? ──YES──→ SEL_SUCCESS
│
NO
↓
[decision/checklist.rs] pre_repair_checklist
↓
Deterministic fix? ──YES──→ apply + retry
│
NO
↓
[repair_strategy.rs] build repair prompt
↓
[pattern_library.rs] lookup pattern + route hint
↓
LLM repair → execute → loop (max 5 attempts)
↓
Still failing? → SEL_FAILED

text


---

## 4. Pre-Repair Checklist (checklist.rs)

| الفحص | الوصف | الوضع |
|-------|--------|--------|
| Check 1 | إضافة `run_tests` المفقودة | جميع الأوضاع |
| Check 2 | Python NameError → auto-import | جميع الأوضاع |
| Check 3 | Rust E0762 — Unicode lifetime quotes | جميع الأوضاع |
| Check 3b | Rust E0422 — missing `pub` (يدعم sub-crates) | جميع الأوضاع |
| Check 4 | patch_file → write_file strategy switch | جميع الأوضاع |
| Check 5 | Cargo.toml corruption recovery | جميع الأوضاع |
| Check 5b | Python pip install stdlib → skip | جميع الأوضاع |
| Check 6 | **TS retry fix** (source-only, Rule-1 safe) | **جميع الأوضاع** |
| Check 7-8 | Go worker-pool + TS api-client | bench فقط |

---

## 5. سلسلة معالجة write_file
content raw
↓
sanitize_code() ← تنظيف Unicode quotes
↓
if .rs → sanitize_rust_lifetime_quotes() + fix_rust_string_literals()
if .py → fix_python_string_quoting()
↓
constitution::check_write() ← Rules 1-7
↓
std::fs::write()
↓
if .go → go_compile_check() + autofix_go_*()

text


---

## 6. الإصلاحات التلقائية (autofix.rs)

| الدالة | المشكلة | اللغة |
|--------|---------|--------|
| `autofix_go_worker_pool_deadlock` | goroutine deadlock | Go |
| `autofix_go_missing_comma` | missing comma في composite literal | Go |
| `autofix_go_unused_import` | unused import | Go |
| `autofix_go_test_run_shadow_alias` | shadowed `t` | Go |
| `autofix_go_test_table_shadow_run` | `t.Run` shadow | Go |
| `autofix_go_undefined_import` | undefined import | Go |

---

## 7. Sanitizers (sanitizers.rs)

| الدالة | الوصف |
|--------|--------|
| `sanitize_code` | Unicode smart quotes → ASCII |
| `sanitize_rust_lifetime_quotes` | `"static` → `&'static` |
| `fix_rust_string_literals` | single-quoted strings → double |
| `fix_python_string_quoting` | `assert '...'` → `assert "..."` |
| `sanitize_go_mod_content` | `-go 1.21` → `go 1.21` |
| `fix_toml_duplicates` | مفاتيح مكررة في Cargo.toml |

---

## 8. Adaptive Routing (pattern_library.rs)

| المحفز | الـ route |
|--------|---------|
| `ModuleNotFoundError` | `missing_dependency` |
| `cannot move` / `borrow` | `rust_ownership` |
| `CONSTITUTION_VIOLATION:no-modify-tests` | `force_source_only` |
| `TS2459` / `TS1192` | `ts/api-client-export` |
| `PromiseRejectionHandledWarning` | `ts/retry-unhandled-rejection` |

---

## 9. مزودي LLM

| # | المزود | النموذج | الدور |
|---|--------|---------|-------|
| 1 | Groq | llama-3.3-70b-versatile | افتراضي |
| 2 | Gemini | gemini-2.0-flash | fallback |
| 3 | Cerebras | llama-3.3-70b | fallback |
| 4 | OpenRouter | متعدد | fallback |
| 5 | GitHub Models | متعدد | fallback أخير |

---

## 10. Smoke Tests (12 مهمة)

| المهمة | اللغة | النتيجة |
|--------|--------|---------|
| smoke_py_binary_search | Python | ✅ |
| smoke_py_temperature | Python | ✅ |
| smoke_py_dataclass | Python | ✅ |
| smoke_go_worker_pool | Go | ✅ |
| smoke_go_generics | Go | ✅ |
| smoke_go_concurrent | Go | ✅ |
| smoke_ts_utils | TypeScript | ✅ |
| smoke_ts_retry | TypeScript | ✅ |
| smoke_ts_fastapi_client | TypeScript | ✅ |
| smoke_rust_csv | Rust | ✅ |
| smoke_rust_fib | Rust | ✅ |
| smoke_rust_trait_impl | Rust | ✅ |

---

## 11. إصلاحات v9.3.2 الحاسمة

### 11.1 TS Retry — Loop Guard + Rule-1 Safe
- أُزيلت كتابة `retry.test.ts` نهائياً
- نُقلت خارج `allow_bench` gate
- أُضيف idempotency guard: `promise.catch(() => {})`
- الـ retry.ts الصحيح يستخدم نمط Promise callback مع noop catch

### 11.2 Go Missing Comma
- `autofix_go_missing_comma` يدعم الآن `_test.go`
- يقرأ رقم السطر من stderr لإضافة الفاصلة بدقة

### 11.3 Rust E0422 — Sub-crate Support
- البحث في `workspace/src/lib.rs` أولاً
- ثم sub-directories (مثل `pointfmt/src/lib.rs`)
- ثم fallback لـ workspace root

### 11.4 Python String Quoting
- `fix_python_string_quoting()` يُطبَّق تلقائياً على `.py`

### 11.5 Wave 2 Evidence Tests
- 4 اختبارات إثبات معمارية مرتبطة بالكود الحقيقي

---

## 12. أرقام الجودة — v9.3.2

| المقياس | القيمة |
|---------|--------|
| src/lib.rs unit tests | **151 passed** |
| src/main.rs unit tests | **206 passed** |
| tests/evidence_wave2.rs | **4 passed** |
| tests/executor_tests.rs | **7 passed** |
| bench suite (all) | **36/36** |
| bench-swe | **30/30** |
| bench-sel-v11 | **18/18** |
| bench-real-world | **14/14** |
| smoke test | **12/12 — 100%** |
| cargo clippy -D warnings | **0 errors** |
| cargo fmt | **clean** |
| regression gate full | **✅ PASSED** |

---

## 13. أوامر الجودة اليومية

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test evidence_
bash scripts/regression_gate.sh full
14. المتغيرات البيئية
المتغير	الوصف
GROQ_API_KEY	مفتاح Groq (إلزامي)
GEMINI_API_KEY	مفتاح Gemini (fallback)
CEREBRAS_API_KEY	مفتاح Cerebras (fallback)
OPENROUTER_API_KEY	مفتاح OpenRouter (fallback)
GITHUB_TOKEN	GitHub Models (fallback أخير)
SEL_BENCH_MODE	تفعيل bench-only semantic fixes
SEL_MAX_REPAIRS	الحد الأقصى لمحاولات الإصلاح (افتراضي: 5)
SEL_WORKSPACE	مسار workspace مخصص
SEL Agent v9.3.2 — Autonomous Execution Engine
smoke: 12/12 · regression: full PASSED · clippy: clean
