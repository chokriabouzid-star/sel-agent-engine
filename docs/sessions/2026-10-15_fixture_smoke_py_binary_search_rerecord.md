# جلسة إعادة تسجيل fixture حالة smoke_py_binary_search

**التاريخ:** 2026-10-15
**التزام الـfixture:** `b3d1c12`
**النوع:** إصلاح fixture/trajectory — لا تغيير في الكود أو الدستور أو الهدف أو الـscaffold

---

## الخلفية

آخر قياس كامل موثق لـSmoke Replay (2026-10-11، ثنائي مبني من `3838805`) أعطى `9/12`. قياس منفرد جديد على `88a5c1e` أكد فشل `smoke_py_binary_search` بنتيجة `0/1` و`REPLAY_RC=1`.

---

## التشخيص (من محتوى ملفات JSON وسجل Replay)

- **`001.json`** (2026-07-20، Groq `llama-3.3-70b-versatile`): الخطة الابتدائية كتبت `binary_search.py` و`test_binary_search.py` دون السطر `from binary_search import binary_search`. فشلت الاختبارات الخمسة بـ`NameError`.
- **`002.json`**: أعاد كتابة `binary_search.py` بالمحتوى نفسه ولم يلمس الاختبار.
- **`003.json`**: حاول كتابة `test_binary_search.py` الموجود، فحجبه المحرك بـ`CONSTITUTION_VIOLATION:no-modify-tests`.
- **`004.json`** (2026-07-04، أقدم من `001` بستة عشر يومًا): يفترض workspace مختلفًا (`sorted_list` و`unittest`). عدد أوامره (5) يطابق المحاولة 3/3 في Replay القديم، وحُجبت كتابته الأولى على ملف الاختبار.
- **غير محسوم:** هل استُهلك `002.json` في Replay القديم. `ReplayProvider` يقرأ الملفات بحسب عدّاد الاستدعاءات، والمخرجات المقروءة كانت ذيل السجل لا السجل كاملًا.

**الخلاصة:** العيب في الاختبار المسجّل نفسه. الإصلاحات المسجلة إما غير كافية أو محجوبة دستوريًا، والمحرك تصرف كما صُمم. إعادة التسجيل هي المسار المشروع الوحيد.

---

## الإجراء

1. نسخة احتياطية مؤرخة خارج المستودع: `~/sel-fixtures-backups/20261015-223927/`، تحقق منها بـsha256 مرتين، قبل العزل وعنده.
2. التحقق من مسار الكتابة: `PersistentCache` يبني `current_dir/fixtures/trajectories`، و`main.rs` يضيف `run_<slug>` للتسجيل وReplay بالتعبير نفسه. تأكد ذلك تجريبيًا بـReplay لم يُظهر `TRAJECTORY_INCOMPLETE`.
3. فرع `fix/smoke-py-binary-search-rerecord` من `main @ 88a5c1e`، ثم نقل `001–004` إلى `quarantine_run_fix_this_broken_binary_search` داخل النسخة الاحتياطية، لا حذفها.
4. تشغيل حي واحد بموافقة صريحة:

~~~
SMOKE_CASES=smoke_py_binary_search scripts/sel_smoke_test.sh target/release/sel-agent --record
Groq openai/gpt-oss-120b — 220s — PASSED — repairs: 0
~~~

الخطة الأولى والوحيدة تضمنت أربعة أوامر: `write_file binary_search.py`، ثم `write_file test_binary_search.py` **مع الاستيراد الصحيح**، ثم `run_tests` بنتيجة `5 passed, 0 failed`، ثم `done`. ظهر `SEL_SUCCESS` بعد مرحلة `Mutation Check`. لم يظهر سطر نجاح صريح لفحص الطفرات، لذلك لا يُدّعى ذلك.

**إيجابية كاذبة في سكربت المراجعة:** رفع علم `WRITE TO TEST FILE` لأنه لا يفرّق بين إنشاء ملف اختبار جديد في الخطة الأولى، وهو مسموح، وتعديل ملف موجود، وهو محجوب. المحرك لم يطبع `CONSTITUTION_VIOLATION`.

---

## الدليل

- **Replay منفرد بعد التسجيل:** `REPLAY_RC=0`، و`SEL_SUCCESS`، و`5 passed, 0 failed`، دون أخطاء بنيوية، ودون `SEL_ALLOW_STALE_REPLAY` أو `--rerecord`. فحص الطفرات تُخطّي في Replay عمدًا للحتمية.
- **بوابة Core:** على الشجرة المرحّلة قبل الالتزام: `36/36 + 30/30 + 18/18 = 84/84`، و`CORE_GATE_EXIT=0`.
- **الالتزام:** `b3d1c12`، ويضم 5 مسارات فقط: تعديل `001.json`، وحذف `002–004`، وتحديث `index.json`.
- **Smoke Replay الكامل** على `b3d1c12`: `10/12`، و`FULL_SMOKE_RC=2`، و`STRUCTURAL_ERRORS: NONE`، دون تراجع في أي حالة ناجحة.
- **الدمج:** `--ff-only` إلى `main`، ثم الدفع، ثم التحقق من `REMOTE_MAIN == LOCAL_MAIN == b3d1c12`.

---

## تصحيحات أثناء الجلسة

- **`.gitignore`:** القاعدة `fixtures/trajectories/` موجودة (سطر 15). يرد `git check-ignore` الافتراضي «غير متجاهل» للملفات المتتبعة، ويظهر التطابق مع `--no-index`. أي ملف جديد يحتاج `git add -f`. لم يلزم ذلك هنا لأن المسارات الخمسة متتبعة أصلًا.
- **حارس المسار الأول:** توقف خطأً لأنه بحث عن نص حرفي. لم يحدث أي تغيير قبل تصحيحه.

---

## الحالتان المتبقيتان (تشخيص أولي، دون إصلاح)

- **`smoke_rust_trait_impl`:** في `001.json` حقول `Point` خاصة. مخرجات المترجم (`E0422` مع `unused import: pointfmt::Point`) **تشير** إلى استعمال `Point` في نطاق لا يراه، وهذا استنتاج لم يُقرأ من نص الاختبار بعد. الإصلاحات المسجلة: `002` جعل الحقول عامة ولم يكفِ، و`003/004` حاولا تعديل الاختبار فحُجبا. `005/006` تسجيلات أقدم.
- **`smoke_ts_fastapi_client`:** أصلح المحرك `TS2459` حتميًا. بقي `TS2339` على `mockedAxios.get.mockResolvedValue` مع `jest.mocked(axios, { shallow: true })`، ونص الهدف يذكر النمطين معًا. الملف `003.json` تسجيل Python استُهلك في Replay، فاصطدم بـ`LANGUAGE LOCK BLOCKED`. الفهرس يذكر `008.json` غير الموجود على القرص. العمل الداخلي المرئي امتد بين `[719.5s]` و`[737.6s]`؛ سبب الزمن السابق **غير محسوم**. إن تكرر `TS2339` بعد تسجيل جديد، فالأمر جلسة هدف/scaffold مستقلة.

---

## حدود الضمان

- أُصلحت حالة واحدة فقط.
- لم تُشغَّل `regression_gate.sh full`.
- لم يُصدر `9.3.6`.
- الحالات المسجلة حية غير حتمية. Replay يمنع طلبات LLM في مساره العادي، لكنه ليس عزلًا شبكيًا لكل عملية فرعية.
