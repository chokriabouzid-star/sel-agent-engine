# سجل استقرار المشروع — Stability Ledger

**آخر تحديث:** 2026-08-10 (إزالة override الصامت لـ max_repairs وتثبيت --max-repairs كعقد صارم)

---

## الحالة المؤكَّدة — من تشغيلة حية كاملة (LIVE) بتاريخ 2026-07-17 + إغلاق جلسة 2026-07-20

| البوابة | الوضع | النتيجة |
|---|---|---|
| cargo test | ✅ | 494 pass, 0 failed |
| cargo clippy -D warnings | ✅ | 0 warnings |
| bench all replay | ✅ | 36/36 |
| bench-swe replay | ✅ | 30/30 |
| bench-sel-v11 replay | ✅ | 18/18 |
| regression_gate core | ✅ | PASS |
| smoke (replay) | ✅ | 12/12 |
| smoke_ts_fastapi_client | ✅ | TypeScript صحيح، 68s، repairs:0 |

---
## القضايا المُغلَقة

### ✅ 2026-08-10 — إزالة الرفع الصامت لـ `max_repairs` عبر heuristic نصي في `do_repairing`

**السبب الجذري:** `src/state_handlers.rs` كان يشتق `dynamic_max_repairs` من `ctx.max_repairs` ثم يرفعه إلى 5 عند وجود كلمات مثل `typescript` أو `node.js` أو `jest` أو `http server` أو `httptest` أو (`go` + `http`) داخل نص الهدف، بلا فهم للنفي، وبلا احترام صارم لقيمة `--max-repairs` التي مررها المستخدم. هذا أدى إلى ظهور لوج من نوع `Repairing (Attempt 1/5)` رغم أن البانر نفسه يطبع `Max repairs: 1`.

**الإصلاح:** حذف `dynamic_max_repairs` والـ heuristic كاملاً، والاكتفاء بـ:
- `FailureKind::InfraError => 0`
- وكل ما عدا ذلك يستخدم `ctx.max_repairs` مباشرة

**لماذا الإزالة الكاملة صحيحة:** أوامر البانش الفرعية التي تحتاج سقفاً أعلى لديها أصلاً قيم CLI افتراضية مستقلة (`bench-swe`, `bench-sel`, `bench-sel-v11`, `bench-real-world` تستخدم 5 افتراضياً)، لذلك override صامت داخل مسار `run` لم يعد مبرَّراً.

**الدليل قبل الإصلاح:**
- تشغيل حي طبع:
  - `Max repairs: 1`
  - `Repairing (Attempt 1/5)...`
  - `Repairing (Attempt 2/5)...`
- وكان الهدف يحتوي نصاً منفياً:
  - `Do not create Python, JavaScript, or TypeScript files.`

**الدليل بعد الإصلاح:**
- تشغيل حي جديد بهدف يحتوي نفس الإشارة المنفية إلى TypeScript طبع:
  - `Max repairs: 1`
  - `Repairing (Attempt 1/1)...`
- ولم يظهر `/5`
- `cargo check` و`cargo test` مرّا
- `scripts/regression_gate.sh core` → PASS

### ✅ 2026-08-09 — `initial_snapshot` على مسار `Done`: منع `Drop` من محو العمل الناجح بعد `SEL_SUCCESS`

**السبب الجذري:** `Agent::run()` كان ينشئ `self.initial_snapshot = Some(Snapshot::take(&ws))` عند بداية الجلسة. هذا الـ snapshot يبقى `active: true`، ويُستهلك فقط في مسار `Failed` عبر `rollback()`. في مسار `Done` لم يكن يُستدعَى `commit()` مطلقاً. عند خروج `run()` ووقوع `Drop` على `Agent`، كان `Drop for Snapshot` ينفّذ `git reset --hard` + `git clean -fd` (+ `stash apply/drop` إن وُجد)، فيُرجع الـ workspace إلى baseline أو يمحو الملفات الجديدة كلياً **حتى بعد نجاح حقيقي كامل وإعلان `SEL_SUCCESS`**.

**الإصلاح:** إضافة:
- `if let Some(mut snap) = self.initial_snapshot.take() { snap.commit(); }`
في بداية فرع `AgentState::Done` داخل `src/agent.rs`، وبنفس نمط `.rollback()` الموجود مسبقاً في مسار `Failed`. لم يُلمَس `src/snapshot.rs`.

**الدليل الحاسم قبل الإصلاح:** تشغيل حي من أول محاولة على `/tmp/sel_first_try_probe` انتهى بـ`1 passed, 0 failed` ثم `SEL_SUCCESS`، لكن بعد خروج العملية مباشرة:
- `cat: /tmp/sel_first_try_probe/src/lib.rs: No such file or directory`

**الدليل الحاسم بعد الإصلاح:**
- تشغيل حي مماثل على `/tmp/sel_first_try_probe_v2` انتهى بـ`SEL_SUCCESS`
- وبقي `/tmp/sel_first_try_probe_v2/src/lib.rs` موجوداً بعد خروج العملية
- ومحتواه كان:
  - `pub fn add(a: i32, b: i32) -> i32 { a + b }`
  - مع اختبار `test_add`
- مسار الفشل بقي سليماً: `/tmp/sel_failed_path_check/main.rs` عاد إلى `fn main() {}`
- `scripts/regression_gate.sh core` → PASS
- تحقق replay إضافي بعد الإصلاح: `sel_smoke_test.sh ./target/release/sel-agent --replay` → `12 / 12`

**نطاق الضرر قبل الإصلاح:** أي `SEL_SUCCESS` حي سابق في تاريخ المشروع كان يمكن ألا يترك عملاً محفوظاً فعلياً على القرص بعد خروج العملية، لأن استعادة `initial_snapshot` كانت تحدث بعد النجاح على مستوى الجلسة كلها.

### ✅ 2026-08-08 — MissingTests (Rust): منع نجاح كاذب بعد حقن `test_stub_placeholder` مع `Mutation skipped`

**السبب الجذري:** إصلاح `MissingTests` في `src/decision/checklist.rs` يحقن اختباراً وهمياً باسم `test_stub_placeholder` ثم يمسح `ctx.failed_steps`. الحارس القديم في `src/state_handlers.rs` كان يعتمد على وجود `"running 0 tests"` أو `"0 passed"` داخل `failed_steps` مع `mutations_total == 0`، لذلك كان يمكن أن يفوّت سيناريو: **اختبار وهمي فقط + لا طفرات قابلة للتطبيق (`Skipped`)**.

**الإصلاح:** توسيع `should_reject_missing_tests_success()` بحيث يرفض النجاح فقط عندما:
- توجد محاولة إصلاح فعلية،
- و`skip_mutation == false`,
- و`mutations_total == 0`,
- وداخل Cargo workspace تكون كل مؤشرات الاختبارات الموجودة هي `test_stub_placeholder` فقط، بلا اختبار حقيقي إضافي.

**الدليل:**
- وحدات: `cargo test --quiet should_reject_missing_tests_success` → `5 passed; 0 failed`
- حيّاً: بعد `running 0 tests` ثم `Pre-Repair 3c: injected #[cfg(test)] stub into "lib.rs"` ثم `1 passed, 0 failed` و`Mutation skipped for src/lib.rs: No mutable patterns found` انتقل المحرك إلى `Repairing` بدل `SEL_SUCCESS`
- بعد إصلاح `initial_snapshot` في 2026-08-09 زال الالتباس الأوسع، فتأكد أن هذا البند نفسه كان مُصلَحاً بالفعل على مستوى منطق MissingTests


## القضايا المفتوحة — بترتيب الأولوية

### 1) ✅ مُغلَقة — انحراف لغوي صامت — `smoke_ts_fastapi_client`

**مُغلَقة في:** 2026-07-20 — كوميت `f9be265` على فرع `fix/stage-3-5-lang-mismatch`

**الخلل كان:** هدف يذكر `fastapi` يُصنَّف دائماً كـ Python حتى لو طلب صراحة عميل TypeScript.
**الإصلاح:** فصل `fastapi_signals` في متغير مستقل مشروط بـ `!ts_signals` في `detect_kind()`.
**الدليل:** `smoke_ts_fastapi_client` ينتج TypeScript صحيحاً، interaction_count 4→2 (لا LANGUAGE LOCK BLOCKED).
**التغطية:** `test_typescript_client_for_fastapi_backend_detected_as_typescript` (jest.mocked/api.test.ts/getUser).

---

### 2) 🟡 عدّاد `repairs:0` خاطئ في تقرير smoke test (مؤجَّل)

السبب الجذري غير مؤكَّد — يحتاج قراءة `sel_smoke_test.sh` كاملاً. جلسة منفصلة.

---

### 3) بنود مؤجَّلة (كل واحدة جلسة منفصلة)

- D1-5: دمج `bench_sel.rs` — WIP في `~/sel_recovery_2026_07_08/`، غير مدموج.
- `agent.rs` طبقة دفاع ثانية — patch محفوظ في `/tmp/agent_rs_language_guard_FUTURE.patch`، يحتاج أمر عمل مستقل + regression_gate full كاملة.
- `language` في `fixtures/trajectories/index.json` دائماً `"trajectories"` بدل اللغة الفعلية.
- H4: تعارض curl/constitution.
- H6: تكرار `is_test_like_command`.
- H7: `estimate_tokens` قياسات متضاربة.
- توحيد رقم الإصدار عبر الأدوات.
- Scaffold صريح لـ Go/Rust.

---

### مرفوض صراحة — لا يُنفَّذ بلا بيانات استخدام حقيقية

إعادة بناء معماري ضخم (Event Sourcing، Plugin Trait عام، AST Mutation Engine، Chaos Testing، Sandbox جديد، Plugin SDK). رُفض من المراجعة المعمارية.

---

## التغييرات الأخيرة

**2026-08-11:** فحص تشخيصي لاشتباه خطأ إملائي (`CONSTITUTION_VIOLATIO` بلا `N`) في `src/repair_strategy.rs`. النتيجة السلبية المؤكدة: لا يوجد الخطأ في المصدر؛ `sed` أظهر `CONSTITUTION_VIOLATION` كاملة في `repair_strategy.rs`، وموضع `pattern_library.rs` كان صحيحاً أيضاً، كما أن `grep -rn "VIOLATIO[^N]" src/ --include="*.rs"` لم يُرجع أي تطابقات. مراجعة لوج حي لاحق لم تُظهر `CONSTITUTION_VIOLATION:no-modify-tests` فعلياً (ظهر فقط مسار `Goal-authorized existing test edits` ورسائل دستور أخرى)، لذا لا يوجد ادعاء تحقق حي لهذا الفرع؛ فقط توثيق أن الاشتباه الأصلي كان إنذاراً كاذباً ناتجاً عن النسخ/الاقتطاع.
**2026-08-10:** جعل رسائل `CONSTITUTION_VIOLATION` خاصة بكل قاعدة في `src/constitution.rs` بدل نص ثابت عن "test contract" لكل الانتهاكات. الإصلاح: إضافة `Violation::critical_instruction()` مع `match` على `rule_id` ورسائل منفصلة للقواعد السبع. الدليل: في تحقق حي، `no-empty-write` صار يطبع `You attempted to write empty or blank content to a source file...`، و`no-dangerous-command` صار يطبع `You attempted to run a dangerous shell command...` بدل نص الاختبارات، مع مرور `cargo test`, `cargo build --release`, و`regression_gate core`.
**2026-08-10:** إزالة الرفع الصامت لـ `max_repairs` داخل `src/state_handlers.rs`. `--max-repairs` أصبح الآن عقداً صارماً في مسار `run`، ولم يعد مجرد ذكرٍ منفي لـ TypeScript/HTTP/Jest قادراً على رفع السقف إلى 5. الدليل: قبل الإصلاح ظهر `Attempt 1/5` رغم `Max repairs: 1`، وبعده ظهر `Attempt 1/1` مع مرور `regression_gate core`.
**2026-08-09:** إغلاق أخطر خلل حي مُثبت حتى الآن: `initial_snapshot` كان يُستعاد في `Drop` بعد `SEL_SUCCESS`، فيمحو أو يرجع العمل الناجح إلى baseline. الإصلاح: `commit()` صريح في `AgentState::Done` داخل `src/agent.rs`. الدليل الحاسم: قبل الإصلاح اختفى `src/lib.rs` تماماً بعد نجاح كامل؛ بعد الإصلاح بقي الملف على القرص، مع بقاء مسار `Failed` سليماً ومرور `regression_gate core` و`smoke replay 12/12`.
**2026-08-08:** إصلاح جزئي لفجوة MissingTests في Rust (`test_stub_placeholder` + `Mutation skipped`) مع فتح تشخيص عاجل ومستقل لمسار `snapshot/stash` لأن تطابق حالة الـworkspace النهائية مع لحظة `SEL_SUCCESS` لم يُثبت بعد.
**2026-07-20:** إغلاق القضية #1 — إصلاح `detect_kind()` في `goal_parser.rs` + اختبار `test_typescript_client_for_fastapi_backend_detected_as_typescript`. كوميت `f9be265`.
**2026-07-17:** تشغيلة حية كاملة (494 اختباراً + 5 بوابات) — اكتشاف القضية #1.
**2026-07-10:** بوابة كاملة خضراء. تاغ `v9.3.5-green-2026-07-10`.
**2026-07-09:** إصلاح replay root cause (`run→run_tests`). ملفات: `src/protocol.rs`, `src/executor/core.rs`.

---

## WIP المحفوظ

- `~/sel_recovery_2026_07_08/` — ملفات D1-5 التجريبية، لا تُدمج حتى اكتمال المراجعة.
- `/tmp/agent_rs_language_guard_FUTURE.patch` — طبقة دفاع ثانية في `agent.rs`، مؤجَّلة.

---

## نتائج ميدانية — تشغيلة حية 10 مهام (2026-07-20)

**الهدف:** اختبار واقعي لمهام صعبة (تزامن، أمان خيوط، race conditions) خارج البانش الرسمي.
**النتيجة الإجمالية:** 6 نجاح / 4 فشل (60%) — Go: 0/3، Python: 2/3، Rust: 2/2، TypeScript: 2/2.

### قوى مؤكَّدة بدليل مباشر

| القوة | الدليل |
|---|---|
| شبكة أمان التخطيط تصمد تحت تعقيد قصوى | `workerpool`: 14 أمر → 8 تكرارات → إعادة تخطيط × 3 → مخالفة دستورية → نجاح |
| الدستور يصمد أمام نموذج متهوّر | `ratelimit`: محاولة تعديل ملف اختبار محمي → `CONSTITUTION_VIOLATION` فوري |
| idempotency فعلي في حلقة الإصلاح | `retry-client`: `⏭ Skipping: already passed` |
| EXPLAIN MODE — اكتُشف اليوم لأول مرة | عند الاستعصاء التام، الوكيل يتوقف ويطلب تلميحًا بشريًا بدل الفشل الصامت |
| Fallback أنقذ مهمة فعليًا | `safequeue`: Groq+Gemini+OpenRouter فشلوا → GitHub/gpt-4o نجح |
| Timeout يمنع التعليق الأبدي | `workerpool`: goroutine deadlock → `go test timeout` بدل تعليق لا نهائي |

### ضعف جديد مؤكَّد 🔴 — الأعلى أولوية

**فشل كامل لسلسلة المزوّدين الخمسة في مهمة حقيقية:**
`ratelimit` فشلت نهائيًا بـ`401 Unauthorized` بعد تعثّر Groq+Gemini+Cerebras+OpenRouter+GitHub
كلهم بالتتابع. لم يعد فجوة نظرية — **فشل إنتاجي فعلي مُوثَّق.**

### ضعف مُعزَّز بدليل جديد 🟡

| الضعف | الدليل الجديد |
|---|---|
| حلقات "إصلاح بلا فهم" (no-op repairs) | `timed_lru`: 3 محاولات × ~1428 bytes متطابقة، نفس الخطأ (`0 != 3`) بلا تشخيص سبب جذري |
| go_compile_check يتوه في المجلدات الفرعية | `ratelimit`: `"no Go files in /tmp/task-manager01"` رغم وجودها في `ratelimit/` |
| إصلاح يُدخل عطلاً جديدًا غير مرتبط | `shardedmap`: أضاف `main.go` (حزمة `main`) في مجلد حزمة `shardedmap` → تعارض تجميع |
| Gemini معطّل فعليًا | 6/6 محاولات → HTTP 400 — يستهلك وقتًا + 3 retries بلا فائدة |

### ضعف جديد دقيق 🟡

**تصنيف "Equivalent Mutant" متساهل:** حدث مرتين (`>=`/`<=` في safe_cache، `||`/`&&` في retry-client).
كلاهما عوامل مقارنة نادرًا متكافئة — الأرجح أن الاختبارات لا تصل للحالة الحدّية، لا أن الطفرة متكافئة فعلاً.

### حد قدرة نموذج — ليس عيبًا هندسيًا

فشلا `workerpool` (إغلاق آمن + context cancellation) و`shardedmap` (race condition معقد)
من أصعب أنماط التزامن حتى لمهندسين بشريين. حد معروف للنموذج الحالي، لا أولوية إصلاح.

### المرشَّحان الأقوى للجلسة القادمة (بترتيب الأثر)

1. **فشل سلسلة المزوّدين الكامل** — موثَّق مرتين، فشل إنتاجي فعلي
2. **حلقات الإصلاح بلا فهم** — موثَّق مرتين (`smoke_py_binary_search` + `timed_lru`)
