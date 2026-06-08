# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v9.0.0
> **الحالة:** Stable working tree after adaptive repair routing + smart repair context
> **الفرع:** phase2-safe
> **آخر تحديث:** 2026-06-08
> **الترخيص:** MIT

---

## 1) ما هو SEL Agent؟

SEL Agent هو وكيل هندسة برمجيات مستقل مكتوب بلغة Rust.
يستقبل هدفًا بالنص الطبيعي، ثم:

1. يبني خطة تنفيذ
2. يكتب أو يعدّل الملفات
3. يشغّل الاختبارات
4. يحلل الفشل
5. يوجه الإصلاح تكيفيًا حسب نوع الخطأ
6. يمنع الدوران في حلقات إصلاح متكررة
7. يعلن `SEL_SUCCESS` أو `SEL_FAILED`

---

## 2) الجديد في v9.0.0

### Adaptive Repair Routing
- `infer_route_from_stderr()` يصنف الخطأ أثناء التشغيل
- `do_repairing()` يستخدم:
  - `matched_pattern.route` إن وجد
  - أو fallback حي من stderr
- `build_prompt()` صار يستقبل route فعليًا ويضيف `REPAIR ROUTE` instructions

### Repair Loop Escalation
- `error_fingerprint()` لبصمة الخطأ
- same-error streak detection
- streak = 2 → warning
- streak >= 3 → loop escalation
- تكرار `CONSTITUTION_VIOLATION:no-modify-tests` يفرض `ForceSourceOnly`

### Smart Repair Context
- `context/builder.rs` موصول الآن بمسار repair runtime
- scoring يعتمد على:
  - culprit files
  - focus paths
  - recent edits
  - graph-aware relationships
- token budget + max_context_files
- fallback آمن إلى legacy workspace context

### Recent Edits Tracking
- `ExecutionContext` يسجل آخر الملفات المعدّلة بنجاح
- dedup + cap
- تُستخدم لتحسين ترتيب الملفات داخل smart context

### Dependency Graph Caching
- الجراف يُبنى مرة واحدة لكل workspace
- يُعاد استخدامه عبر repair attempts
- يُلغى cache بعد successful file edit

### Rust Dependency Graph Improvements
- دعم:
  - `pub mod`
  - `use crate::...`
  - `use self::...`
  - `use super::...`

### RepairCtx Improvement
- `RepairCtx::build()` صار recursive بدل top-level فقط

---

## 3) الحالة الحالية المؤكدة

| المقياس | النتيجة |
|---------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test` | **304/304** ✅ |
| `bash scripts/regression_gate.sh core` | ✅ |
| `bench --suite all --replay` | **36/36** ✅ |
| `bench-swe --lang all --replay` | **30/30** ✅ |
| `bench-sel-v11 --replay` | **18/18** ✅ |

### توزيع الاختبارات
| الهدف | عدد الاختبارات |
|-------|----------------|
| `src/lib.rs` | 141 |
| `src/main.rs` | 156 |
| `tests/executor_tests.rs` | 7 |
| **المجموع** | **304** |

---

## 4) المسار الحي للإصلاح الآن

```text
stderr
  → infer_route_from_stderr()
  → effective_route
  → build_prompt(route-aware)
  → smart repair context
  → loop escalation
  → execution
  → record_pattern_outcome()
5) الملفات الأساسية المتأثرة في v9.0.0
الملف	الدور
src/pattern_library.rs	route inference + pattern memory
src/repair_strategy.rs	route-aware repair prompt
src/state_handlers.rs	runtime repair orchestration
src/context/builder.rs	smart repair context selection
src/types.rs	recent edits + dependency graph cache
src/dependency_graph/parsers/rust_lang.rs	improved Rust import resolution
6) أوامر التحقق المرجعية
Bash

cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
bash scripts/regression_gate.sh core
7) ملاحظات صيانة
النسخة لها single source of truth في Cargo.toml
CLI وruntime version output يعتمدان على env!("CARGO_PKG_VERSION")
patch scripts القديمة وملفات .bak* أُزيلت من المستودع
توجد ملاحظات صيانة في:
text

docs/MAINTAINER_NOTES.md
8) الملخص التنفيذي
text

SEL Agent v9.0.0

Major runtime upgrades:
- adaptive repair routing
- repair loop escalation
- smart repair context
- recent edit scoring
- dependency graph caching
- improved Rust dependency graph resolution

Verified now:
- cargo check: pass
- cargo clippy: pass
- cargo test: 304/304
- regression_gate core: pass
- bench all replay: 36/36
- bench-swe replay: 30/30
- bench-sel-v11 replay: 18/18
