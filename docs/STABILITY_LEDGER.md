# سجل استقرار المشروع — Stability Ledger

**آخر تحديث:** 2026-10-10 (W6: EXPLAIN MODE يتجاوز القراءة التفاعلية لـ stdin عند SEL_NON_INTERACTIVE=1)

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

### ✅ 2026-10-10 — W6: EXPLAIN MODE لم يكن يحترم التصريح الصريح بعدم التفاعل

**السبب الجذري:** كان فرع `AgentState::WaitingForUserInput` يتخطى EXPLAIN في وضع Bench أو عند غياب طرفية stdin أو stdout، لكنه لم يفحص `SEL_NON_INTERACTIVE`. لذلك، عند وجود الطرفيتين والإعلان صراحةً عن التشغيل غير التفاعلي، كان يمكن أن يصل المسار إلى `stdin.read_line()` وينتظر إدخالًا.

**الإصلاح (`a0e4649`):** استُخرج قرار التخطي إلى الدالة النقية `should_skip_explain`، وأضاف موقع الاستدعاء تفعيلًا عند كون قيمة `SEL_NON_INTERACTIVE` هي `"1"` حرفيًا. بقي عقد `SEL_BENCH_MODE` كما كان (`is_ok()`)، وبقي فحص طرفيتي stdin وstdout. لم تتغير رسالة التخطي أو حالة الفشل التاريخية (`[Bench]` و`max_repairs_bench`).

**الدليل قبل الإصلاح:**
- اختبار القرار مع طرفيتين ظاهريتين ومن دون Bench أعاد `None` بدل `Some(NonInteractive)`: فشل assertion دلالي، `1 failed / 6 passed`، و`TEST_EXIT=101`.
- اختبارات `src/agent.rs` تابعة للـbinary `sel-agent` لأن `src/main.rs:9` يضمّن `mod agent`; نتيجة `--lib` ذات الصفر اختبارات المطابقة لم تُحسب RED.

**الدليل بعد الإصلاح:**
- الاختبارات المستهدفة: `7 passed / 0 failed`.
- `cargo check --locked`: نجاح؛ `cargo fmt --all -- --check`: نجاح؛ وClippy مع `-D warnings`: نجاح.
- `cargo test --locked`: `741 passed / 0 failed`، مقابل خط أساس قبل W6 مقداره `734 passed / 0 failed`.
- بوابة Replay عبر pre-commit hook الطبيعي عند `a0e4649`: `36/36 + 30/30 + 18/18 = 84/84`.

**حدود الضمان:** لم يُجرَ اختبار طرف إلى طرف تحت PTY؛ الاختبار يثبت قرار الدالة، ومراجعة موقع الاستدعاء تثبت تمرير نتيجة فحص البيئة قبل الوصول إلى `read_line()`. لم تُختبر قيم `"0"` والفارغة باختبار مستقل؛ الكود لا يفعّل العلم إلا للقيمة `"1"`. كما احتُفظ بوسم الفشل التاريخي `max_repairs_bench` حتى لحالة عدم التفاعل. جدول الحالة التاريخية المؤرخ 2026-07-17 (494 اختبارًا) محفوظ دون تغيير، وليس قياس W6.

### ✅ 2026-10-09 — W5: ذراع Node كان يتجاهل `scripts.test` ويحقن أعلام Jest في أي عدّاء

**السبب الجذري:** ذراع `ProjectType::Node` في `resolve_test_command` (`src/workspace_oracle.rs`) كان يقرر من نص الهدف فقط ولا يقرأ محتوى `package.json`. مشروع يعلن `"scripts": {"test": "node --test"}` كان يُعاد له `npx jest --runInBand --forceExit` (هدف صريح) أو `npm test -- --runInBand --forceExit` (هدف auto)، فتصل أعلام Jest إلى `node --test` الذي يرفضها: `node: bad option: --runInBand`. إضافة إلى ذلك، مسار Node في `runner.rs` كان يفسر كل المخرجات عبر `parse_jest`، فملخص TAP (`# pass` / `# fail`) يعطي `(0,0)` ويُصنف النجاح فشلًا.

**الإصلاح (`b333440`):** ثلاث خطوات بملف واحد لكل خطوة: (1) `package_json_test_script()` تقرأ `scripts.test` غير الفارغ من جذر الـ workspace؛ عند وجوده يُعاد `("npm", ["test"])` حرفيًا، والـ fallback القديم محفوظ عند غيابه. (2) `parse_node_tests()` في `parsers.rs` تقرأ ملخص TAP عند وجود `# pass` و`# fail` معًا وإلا fallback لـ `parse_jest`. (3) سطر واحد في `runner.rs` يوصل المسار بالـ parser الجديد.

**الدليل قبل الإصلاح:** `tests/node_scripts_test.rs` على `fa568b6`: 3 failed / 1 passed، والدليل الحاسم offline: `node: bad option: --runInBand` بعد `> node --test --runInBand --forceExit`.

**الدليل بعد الإصلاح:**
- 4/4 ok؛ E2E يطبع `Running: npm test` ثم `node --test` وينتهي `1 passed, 0 failed`.
- فشل وسيط موثق بعد خطوة الـ Oracle وحدها (`0 passed, 0 failed` رغم `# pass 1`) أثبت ضرورة خطوة الـ parser.
- `guard_node_arm_is_untouched` (بلا `scripts.test`) بقي أخضر.
- اختبارات الوحدة: نجاح TAP، فشل TAP `(1,1)`، وfallback لـ Jest.
- `cargo fmt --check` = 0، و`clippy --locked --all-targets --all-features -- -D warnings` = 0.
- `cargo test --locked` = 734 passed / 0 failed.
- `regression_gate.sh core` عبر hook pre-commit: 36/36 + 30/30 + 18/18 — وبه ثبتت سلامة مهام TypeScript في البنش (fixtures تعلن `"test": "jest"` وصارت تعمل عبر `npm test` بلا الأعلام).

**حدود الضمان:** ذراع `_` للمشروع Unknown ما زال يحقن أعلام Jest؛ ذراع `js|ts` في `mutation.rs` ما زال يفرض jest؛ القراءة من جذر الـ workspace فقط؛ JSON التالف يسقط بصمت إلى الـ fallback.

### ✅ 2026-09-24 (وُثِّق 2026-10-07) — أحفاد العملية المقتولة كانوا ينجون من المهلة

**السبب الجذري:** بعد `a752295` كان `output_with_timeout` يقتل الابن المباشر فقط عبر `child.kill()`. أحفاد مثل sub-binaries الخاصة بـ `npm test` و`pytest -n` و`go test` كانت تبقى حية بعد انتهاء المهلة وتستمر أثناء `Repairing`.

**الإصلاح (`46de35b`):** في `src/executor/process.rs`: `cmd.as_std_mut().process_group(0)` عند الـ spawn على Unix، وعند المهلة `libc::kill(-pgid, SIGKILL)` لمجموعة العمليات كاملة ثم `child.wait()` لحصد الابن. أُضيفت تبعية `libc` لـ Unix فقط. المنصات الأخرى تحتفظ بضمان الابن المباشر.

**الدليل:** `tests/process_lifecycle_safety.rs` يسجّل PID الحفيد (shell: `sleep 30 &`؛ Node: `spawn('sleep')`) ويتحقق من موته بعد المهلة؛ ملف PID مفقود أو تالف يُفشل الاختبار. أحمر قبل الإصلاح، أخضر بعده.

**ملاحظة أمانة:** رسالة `46de35b` تصرّح بأن بوابة Replay لم تُشغَّل عند ذلك الالتزام (تسجيلات غير صالحة وانقطاع مزوّد)، وأن الـ hook تُجووز لذلك الالتزام وحده عبر `hooksPath` فارغ مؤقت. التغطية حصلت لاحقًا: `regression_gate.sh core` مرّ 84/84 عبر الـ hook الطبيعي في `d609e7a` و`a652387` و`ed3b8fc`، وكلها تحتوي هذا الإصلاح.

**تدقيق 2026-10-07:** الاختبار 2/2 أخضر في 3.09s؛ لا `tokio::time::timeout(d, cmd.output())` خام ولا `.kill()` مباشر في `src/` خارج المساعد نفسه. أُغلق بند TODO المقابل الذي بقي 🟡 بالخطأ.

### ✅ 2026-10-06 — W4/Part-2: `mutation_check` كان يترك ملف المستخدم مطفّرًا عند إلغاء الـ future

**السبب الجذري:** الاستعادة في `SafeExecutor::mutation_check` (`src/executor/mutation.rs`) كانت أربع استدعاءات يدوية لـ `std::fs::write(&source_path, &original)`، كلها بعد الـ `.await` الوحيد (`output_with_timeout`). إسقاط الـ future عند تلك النقطة، أو unwind قبل سطر الاستعادة، يتخطى كل الاستعادات، فيبقى ملف المستخدم مطفّرًا على القرص. `kill_on_drop` ينظّف العملية لكنه لا يعرف ملف المصدر.

**الإصلاح:** `SourceRestoreGuard { path, original, armed }`. `arm()` قبل كل كتابة طفرة؛ `restore()` يعيد الأصل ويطفئ عند فشل الكتابة وبعد الـ await وعند الخروج الطبيعي؛ `Drop` يستدعي `restore()` فيغطي الإلغاء والـ panic. أُزيلت الاستعادة اليدوية الزائدة قبل `return Uncompilable`. لم تتغير مهلة Part-1 `min(self.timeout_secs, 30)` ولا دلالات Strong/Weak/Uncompilable.

**الدليل قبل الإصلاح:** `tests/mutation_restore_guard.rs` على `119fc14`: FAILED في 0.09s — `left: return x - y` و`right: return x + y`.

**الدليل بعد الإصلاح:**
- ok في 0.09s والأصل مُستعاد بعد إسقاط الـ future.
- W4/Part-1 (`mutation_lifecycle_safety`) 1/1، وW2 (`rust_nested_mutation`) 3/3.
- `cargo fmt --check` = 0، و`cargo clippy --locked --all-targets --all-features -- -D warnings` = 0.
- `cargo test --locked` = 724 passed / 0 failed.
- `regression_gate.sh core` عبر hook pre-commit: 36/36 + 30/30 + 18/18.
- كوميت `ed3b8fc`.

**حدود الضمان:** الاختبار يغطي إلغاء الـ future لا panic صريحًا. الاختبار unix-only؛ الحارس نفسه ليس خاصًا بمنصة.

### ✅ 2026-10-04 — W4/Part-1: `mutation_check` كان يترك أحفاد عدّاء الاختبار أحياء عند المهلة

**السبب الجذري:** حلقة الطفرات في `SafeExecutor::mutation_check` (`src/executor/mutation.rs`) كانت تستدعي `tokio::time::timeout(30s, Command::output())`. إسقاط الـ future عند المهلة يقتل الابن المباشر فقط، فينجو الأحفاد (`pytest`، `cargo`، `jest` وما تولّده) ويستمرون أثناء `Repairing`. كذلك كان الثابت `30s` يتجاهل `self.timeout_secs` كليًا. `git blame` أرجع الثابت إلى `c90b218` ضمن `chore: upgrade to v8.3.0 + cleanup` بلا مبرر مكتوب. هذا كان الموضع الوحيد الذي لا يمرّ عبر `executor::process::output_with_timeout`.

**الإصلاح:**
- بناء `tokio::process::Command` صراحة.
- تمرير الأمر إلى `executor::process::output_with_timeout`.
- استخدام deadline: `min(self.timeout_secs, 30)`.
- تغيير المطابقة من `Ok(Ok(result))` إلى `Ok(Some(result))`.
- عند `Ok(None)` تُعامل الطفرة كمهلة: لا تُحسب Strong ولا Weak.
- لم يُلمس مسار الاستعادة اليدوية؛ حارس RAII باقٍ لـ W4/Part-2.

**سبب سقف 30s:** الطفرات تُشغَّل بالتتابع داخل `mutation_check`. اعتماد مهلة المنفِّذ بلا قيد كان سيضاعف أسوأ حالة لكل طفرة. لذلك يحفظ `min(self.timeout_secs, 30)` السلوك التاريخي للإنتاج (`120 → 30` و`60 → 30`) مع احترام المهل الأصغر (`1 → 1` في الاختبار).

**الدليل قبل الإصلاح:** `tests/mutation_lifecycle_safety.rs` على `f44b5d9`: FAILED في 30.16s مع `grandchild PID 35425 is still alive after mutation_check returned`.

**الدليل بعد الإصلاح:**
- ok في ~1.1s والحفيد ميت، أي أن `timeout_secs = 1` صار محترمًا فعلًا.
- `cargo fmt --check` = 0، و`cargo clippy --locked --all-targets --all-features -- -D warnings` = 0.
- `cargo test --locked` = 723 passed / 0 failed.
- اختبارات W2 (`rust_nested_mutation`) بقيت 3/3 على نفس مسار الكود.
- `regression_gate.sh core` عبر hook pre-commit: 36/36 + 30/30 + 18/18.
- كوميت `a652387`، ثم ff-only إلى `main`.

**حدود الضمان:** الاختبار يثبت احترام المهلة الصغيرة وقتل الحفيد، ولا يثبت آليًا التحويل `120 → 30`. حارس RAII للاستعادة عند إلغاء الـ future أو panic لم يُنفَّذ بعد (W4/Part-2).

### ✅ 2026-10-04 — W2: `mutation_check` كان يصنّف كل طفرات crates Rust المتداخلة كـ `Uncompilable`

**السبب الجذري:** ذراع `"rs"` في `SafeExecutor::mutation_check` (`src/executor/mutation.rs`) كان يشغّل `cargo test --quiet` من `self.workspace` بلا `--manifest-path`. عند crate متداخلة (`add_lib/Cargo.toml`) بلا manifest في الجذر، يفشل cargo بـ `could not find Cargo.toml`، فيلتقطه `src/diagnostic.rs` ويصنّفه `stderr_is_compile_failure` كفشل ترجمة. النتيجة: `Uncompilable` لكل طفرة دون تنفيذ أي اختبار.

**الإصلاح:** إضافة `find_nearest_cargo_manifest()`: بحث صعودي من الملف المصدر بعد `canonicalize`، محدود بجذر الـ workspace شاملًا. يُمرَّر المسار عبر `--manifest-path`، وعند الغياب يُرجَع `MutationResult::Skipped("no Cargo manifest found")`. لم تُلمس المهلة ولا مسار الاستعادة (W4).

**الدليل قبل الإصلاح:** `tests/rust_nested_mutation.rs` على `f0ea08e` أعطى 3 failed، و`left: Uncompilable(...)` في الحالات الثلاث، في 0.35s.

**الدليل بعد الإصلاح:**
- 3 passed في 2.67s، واختبار `Weak` يثبت تنفيذ الاختبارات فعلًا داخل الـ crate المتداخلة.
- `cargo fmt --check` = 0، و`cargo clippy --locked --all-targets --all-features -- -D warnings` = 0.
- `cargo test --locked` = 722 passed / 0 failed (خط الأساس 719).
- `regression_gate.sh core` عبر hook pre-commit: 36/36 + 30/30 + 18/18.
- كوميت `d609e7a`.

**حدود الضمان:** symlink يخرج من الـ workspace، وملف خارجه، وworkspace جذري مع member متداخل: كلها غير مختبرة بعد.

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

**2026-10-09:** W5 — ذراع Node يحترم `scripts.test` (يُشغَّل عبر `npm test` حرفيًا) و`parse_node_tests` يفهم ملخص TAP مع fallback لـ Jest. RED: 3 failed بدليل `node: bad option: --runInBand`؛ GREEN: 4/4 و`1 passed, 0 failed`. السويت 734/0، والبوابة 84/84 عبر الـ hook. كوميت `b333440`.
**2026-10-07:** تدقيق دورة حياة العمليات — تأكيد أن `46de35b` أغلق بند "grandchildren survive a killed parent" (مجموعة عمليات + `killpg` + حصاد)، والانحدار 2/2 أخضر، ولا مسار يتجاوز `output_with_timeout`. تحديث TODO وإدخال الالتزام في هذا السجل بأثر رجعي. لا تغيير في الكود.
**2026-10-06:** W4/Part-2 — `SourceRestoreGuard` يستعيد المصدر عند كل مسار خروج، بما فيها إسقاط الـ future عند `.await`. RED: ملف مطفّر (`return x - y`) في 0.09s؛ GREEN: الأصل مُستعاد. 724/0، وPart-1 1/1، وW2 3/3، والبوابة 84/84 عبر الـ hook. كوميت `ed3b8fc`.
**2026-10-04:** W4/Part-1 — `mutation_check` صار يشغّل عدّاء اختبار الطافر عبر `output_with_timeout` مع `deadline = min(self.timeout_secs, 30)`، فتُقتل مجموعة العمليات كاملة عند المهلة. RED: 30.16s وحفيد حي؛ GREEN: ~1.1s وحفيد ميت. 723/0، وW2 3/3، والبوابة 84/84 عبر الـ hook. كوميت `a652387`. حارس RAII للاستعادة ما زال مفتوحًا.
**2026-10-04:** W2 — `mutation_check` يمرّر `--manifest-path` لأقرب `Cargo.toml` صعودًا من الملف المصدر، و`Skipped("no Cargo manifest found")` عند الغياب. RED 3/3 ثم GREEN 3/3، و722/0، والبوابة 84/84 عبر الـ hook. كوميت `d609e7a`.
**2026-08-11:** حسم نهائي لالتباس وجود/غياب حرف `N` في `src/repair_strategy.rs:233`. الدليل الحاسم هذه المرة لا يعتمد على النسخ اليدوي فقط: `sed -n '233p' src/repair_strategy.rs | cat -A` أظهر `CONSTITUTION_VIOLATION:no-modify-tests` كاملة، وchecksum السطر هو `11fbf22d4789df456b2eb57be2389f0a77a8d435afae8d38a5edb552534c56eb`. أُضيف أيضاً اختبار سلوكي في `repair_strategy.rs` يبني خطأ `no-modify-tests` الحقيقي من `constitution.rs` عبر `check_write(...).unwrap_err().to_string()` ثم يمرّره إلى `build_prompt()` ليثبت أن فرع `no-modify-tests` يُفعَّل فعلاً، وبذلك يُغلَق التناقض بين الجلستين السابقة والحالية بدليل حرفي + سلوكي دائم.
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
