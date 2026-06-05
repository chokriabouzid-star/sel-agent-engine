# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v8.8.0  
> **الحالة:** Stable working tree after Wave 1 + Wave 2 completion  
> **الفرع:** phase2-safe  
> **المؤلف:** Chokri Bouzid  
> **آخر تحديث للحالة المرجعية:** 2026-06-04  
> **الترخيص:** MIT

---

## 1) ما هو SEL Agent؟

SEL Agent هو **وكيل هندسة برمجيات مستقل** (Autonomous Software Engineering Agent) مكتوب بلغة Rust.  
يستقبل هدفًا بالنص الطبيعي، ثم:

1. يبني خطة تنفيذ
2. يكتب أو يعدّل الملفات
3. يشغّل الاختبارات
4. يحلّل الفشل
5. يعيد الإصلاح تكراريًا
6. يعلن:
   - `SEL_SUCCESS` عند النجاح
   - `SEL_FAILED` عند استنزاف ميزانية الإصلاح أو تعذر التنفيذ

المشروع جزء من تصور أشمل اسمه **Sovereign Execution Layer (SEL)**، والوكيل هو الذراع التنفيذية التي تحوّل المتطلبات إلى كود قابل للتشغيل والتحقق.

---

## 2) الفلسفة الأساسية

- **الاختبار هو العقد**: لا يُفترض بالوكيل تعديل الاختبارات الموجودة مسبقًا.
- **الحتمية أولًا**: replay و trajectories جزء أساسي من النظام.
- **القياس قبل التحسين**: لا إضافة بدون benchmark أو signal قياسي واضح.
- **الإصلاح الجذري لا الترقيع**: نصلح السبب البنيوي لا العرض السطحي.
- **تقليل الاعتماد على جودة النموذج**: نقل المعرفة من الـ LLM إلى engine عبر autofix و semantic repair.

---

## 3) الحالة الحالية المؤكدة

### 3.1 مؤكد بعد آخر دورة تحقق كاملة
| المقياس | النتيجة |
|--------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test` | **240/240 (100%)** |
| `bash scripts/regression_gate.sh core` | ✅ |
| `bench --suite all --replay` | **36/36 (100%)** |
| `bench-swe --lang all --replay` | **30/30 (100%)** |
| `bench-sel-v11 --replay` | **18/18 (100%)** |

### 3.2 آخر نتائج موسعة معروفة ومستقرة
| المقياس | النتيجة |
|--------|---------|
| `bench-real-world --replay` | **14/14 (100%)** |
| `smoke --replay` | **12/12 (100%)** |

> ملاحظة: آخر gate رسمي بعد Wave 2 كان **core gate**.  
> أما `bench-real-world` و `smoke` فآخر نتيجة معروفة لهما كانت خضراء بالكامل قبل إغلاق Wave 2، ولم تظهر أي مؤشرات regression بعدها.

### 3.3 حجم المشروع
| البند | القيمة |
|------|--------|
| Rust files | **60** |
| Rust LOC | **19,943** |
| trajectory directories | **127** |
| trajectory JSON files | **386** |

---

## 4) ما الذي تغيّر جوهريًا منذ v8.5.2؟

### Wave 1 — Observability
أُضيفت طبقة قياس حقيقية للنظام:

- `ExecutionReport` backend
- كتابة تقارير JSON لكل تشغيل في:
  - `~/.sel-agent/reports/`
- ملف:
  - `latest.json`
- أوامر:
  - `sel-agent report --latest`
  - `sel-agent report --summary`
- واجهة TUI تفاعلية:
  - `sel-agent observatory`
- بوابة regression محلية:
  - `scripts/regression_gate.sh`

### Wave 2 — Dependency Graph
أُضيف فهم بنيوي حقيقي للمشروعات:

- `DependencyGraph` core model
- `dependencies_of()`
- `impacted_by()`
- `detect_cycles()`
- parsers لأربع لغات:
  - Python
  - TypeScript
  - Go
  - Rust
- `build_for_workspace()`
- graph-aware context scoring داخل:
  - `src/context/builder.rs`

---

## 5) البنية المعمارية الحالية

### دورة الحالة (State Machine)
```text
Planning → Executing → Repairing → Done / Failed
الملفات الأساسية
الملف	الدور
src/main.rs	نقطة الدخول والـ CLI
src/agent.rs	المحرك الأساسي للوكيل
src/state_handlers.rs	منطق الحالات: planning / executing / repairing
src/decision.rs	semantic repair + deterministic checklist fixes
src/repair_strategy.rs	بناء repair prompts
src/constitution.rs	القواعد الصلبة غير القابلة للتجاوز
src/scaffold_engine.rs	تجهيز بيئات Python / TypeScript / cache scaffolds
src/workspace_oracle.rs	استنتاج نوع المشروع ومسارات الاختبار
src/executor/runner.rs	تشغيل الاختبارات والأوامر
src/executor/file_ops.rs	write/patch/append/delete مع الحماية
src/executor/autofix.rs	إصلاحات تلقائية deterministic
src/executor/compile.rs	compile checks قبل بعض الكتابات
src/llm/record.rs	تسجيل trajectories
src/llm/replay.rs	إعادة تشغيل trajectories
src/report.rs	نموذج ExecutionReport
src/report_writer.rs	كتابة التقارير إلى القرص
src/commands/report.rs	report --latest/--summary
src/commands/observatory.rs	واجهة TUI للمرصد
src/dependency_graph/mod.rs	قلب الـ graph
src/dependency_graph/builder.rs	بناء graph من workspace
src/dependency_graph/parsers/	parsers اللغات الأربع
src/context/builder.rs	انتقاء ملفات السياق — أصبح graph-aware
src/bench_sel.rs	SELBench الداخلي
src/bench_swe.rs	Mini SWE-Bench
src/bench_realworld.rs	Real-World benchmark
6) الدستور (Constitution)
الدستور فعّال ومتصّل بمسارات التنفيذ، وليس مجرد prompt.

القواعد الحالية
no-modify-tests
no-empty-write
no-binary-in-text
no-system-path
no-overwrite-go-mod
no-dangerous-cmd
no-network-in-test
السلوك
ملفات الاختبار الموجودة قبل بدء الجلسة فقط هي المحمية.
ملفات الاختبار التي ينشئها الوكيل أثناء الجلسة يمكنه تعديلها لاحقًا.
عند الانتهاك يصدر:
CONSTITUTION_VIOLATION:<rule>
7) طبقات القوة الحالية
7.1 AutoFix deterministic
إصلاحات لا تعتمد على جودة الـ LLM:

Go
missing comma in composite literals
unused import removal
undefined stdlib import insertion
t.Run shadow alias fix
worker pool deadlock fix
Python
إنشاء venv + تثبيت pytest خارج replay عند الحاجة
تجاوز pip install لموديولات stdlib مثل unittest
Replay Python
عند replay، إذا كانت trajectory تطلب venv/bin/pytest
والـ venv غير موجودة
يحاول النظام استعادة cached Python venv من scaffold cache
Signal معروف:

text

⚡ Replay Env: restored cached Python venv
7.2 Semantic Repair deterministic
في src/decision.rs توجد إصلاحات حتمية مبنية على فئة الخطأ:

Go worker pool
deadlock / timeout fix
mutation survival fix (workers <= 0)
TypeScript retry
PromiseRejectionHandledWarning / fake timers
mutation survival عبر اختبار timing أقوى
TypeScript API client
axios mocking / never typing issues
Python
stdlib pip misuse
replay env restoration
8) Observability — الطبقة الجديدة
8.1 Execution Reports
كل run يمكن أن ينتج تقرير JSON محفوظًا محليًا في:

text

~/.sel-agent/reports/
8.2 Report CLI
Bash

sel-agent report --latest
sel-agent report --summary
8.3 Observatory TUI
Bash

sel-agent observatory --refresh-secs 5 --limit 50
توفر:

Summary
Recent runs
Failure view
selected run details
refresh دوري
8.4 Regression Gate
Bash

bash scripts/regression_gate.sh core
bash scripts/regression_gate.sh full
حاليًا:

core = bench all + bench-swe + bench-sel-v11
full = يضيف real-world + smoke
9) Dependency Graph — الطبقة الجديدة
9.1 ما الذي أُنجز؟
core graph model
directed edges between files
reverse impact analysis
cycle detection
workspace graph builder
parsers لأربع لغات
graph-aware scoring in context selection
9.2 ما الذي يفعله الآن؟
إذا كان هناك ملف culprit أو ملف قريب من الخطأ:

يُعطى الملف نفسه score أعلى
تُعطى تبعياته المباشرة score إضافيًا
تُعطى الملفات التي تتأثر به score إضافيًا
الملفات داخل cycles تُعطى bonus إضافي
9.3 ما الذي لم يفعله بعد؟
لا multi-file patch atomic بعد
لا symbol-level refactor بعد
لا architecture-aware routing بعد
لا adaptive repair مبني بالكامل على graph بعد
10) نظام Replay / Record / Rerecord
ما هي trajectories؟
كل مهمة يمكن تسجيلها كسلسلة نداءات LLM:

001.json
002.json
003.json
...
سلوك مهم
عدد ملفات trajectory ليس ثابتًا:

one-shot success → غالبًا 001.json
repair path → قد تظهر 002.json أو أكثر
الأوضاع
الوضع	الوصف
--record	تنفيذ عبر LLM وتسجيل trajectory جديدة
--replay	تنفيذ offline من trajectory موجودة
--replay --rerecord	replay مع healing تلقائي
11) الأوامر المرجعية الحالية
الجودة
Bash

cargo fmt --all
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
Replay benchmarks
Bash

./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay
التقارير
Bash

./target/release/sel-agent report --latest
./target/release/sel-agent report --summary
./target/release/sel-agent observatory --refresh-secs 5 --limit 50
البوابات
Bash

bash scripts/regression_gate.sh core
bash scripts/regression_gate.sh full
12) القيود الحالية المتبقية
هذه ليست failures حالية، بل تحسينات مستقبلية:

12.1 Trajectory Manifest
لا يوجد بعد meta.json لكل trajectory directory يصف:

عدد النداءات المتوقع
completion status
outcome
mode
12.2 Stability Layer
لا يوجد تصنيف رسمي بعد:

PASS
REPLAY_PASS
RERECORD_PASS
FLAKY_RECOVERED
FAIL
12.3 Pattern Library
لا توجد بعد مكتبة أنماط متعلمة من النجاحات السابقة.

12.4 Adaptive Repair Routing
لا يوجد بعد routing كامل حسب نوع الفشل + graph state + pattern match.

12.5 Plan Evaluator
لا يوجد بعد تقييم احتمالية نجاح الخطة قبل التنفيذ.

13) مستوى النضج الحالي
SEL Agent الآن لم يعد مجرد وكيل إصلاح، بل أصبح:

Observable
Replay-stable
Deterministic where possible
Graph-aware in context scoring
Protected by local regression gate
وهذا يعني أن المشروع انتقل من:

مرحلة تثبيت الأعطال الأساسية

إلى:

مرحلة بناء الذكاء الهندسي التراكمي

14) خارطة الطريق بعد v8.8.0
المرحلة التالية — Wave 3
W3-1
Pattern Library
built-in patterns
JSON-backed storage
basic matcher
W3-2
Adaptive Repair Routing
اختيار strategy حسب:
error kind
graph state
matched pattern
W3-3
Plan Evaluator
risk scoring
احتمال النجاح قبل التنفيذ
15) الملخص التنفيذي النهائي
text

SEL Agent v8.8.0

Verified:
- cargo check: pass
- clippy -D warnings: pass
- cargo test: 240/240
- regression_gate core: pass
- bench all replay: 36/36
- bench-swe replay: 30/30
- bench-sel-v11 replay: 18/18

Major completed capabilities:
- execution reports
- report CLI
- observatory TUI
- local regression gate
- dependency graph core
- Python / TS / Go / Rust parsers
- graph-aware context scoring

Current maturity:
- deterministic fixes are strong
- replay remains stable
- observability is real
- project-level structural understanding has started
- next strategic milestone: Pattern Library + Adaptive Repair
