جلسة 2026-10-11 → 2026-10-12: W12/جلسة 0 — تدقيق الإصدار، قياس smoke، ومرشّح SMOKE_CASES

الهدف
تحديد شروط الإصدار التالي وقياس smoke قبل أي رفع للنسخة. انتهت الجلسة بأداة اختيار حالات smoke فقط؛ لم يُرفع الإصدار ولم تُشغَّل بوابة full ولم تُعدَّل fixtures.

خط الأساس
- main = origin/main = 3838805، شجرة نظيفة، cargo test --locked = 741 passed / 0 failed (TEST_EXIT=0).

قرارات الإصدار (لم تُنفَّذ بعد)
- الرقم المختار: 9.3.6. لا توجد وثيقة سياسة ترقيم مكتوبة؛ القرار يدوي مبني على سابقة CHANGELOG (v9.3.1 وv9.3.3 إصدارات patch تحتوي "Added").
- ملاحظة: تعليق src/python_env.rs:1 يذكر "Phase 2 (9.4.0)"؛ تعليق لا سياسة، ولم يغيّر القرار.
- docs/REFERENCE.md يفرض regression_gate.sh full قبل كل release، وcore قبل كل milestone. full يضيف bench-real-world 14/14 وsmoke 12/12. لم يُشغَّل full في هذه الجلسة.
- تباين توثيقي: REFERENCE.md يذكر sel_smoke_test.sh في الجذر، والملف الفعلي scripts/sel_smoke_test.sh (المسار الذي تستدعيه البوابة).

قياس smoke (2026-10-11، replay، ثنائي release مبني من 3838805)
- run --replay يختار ReplayProvider وحده؛ مسار التسجيل fixtures/trajectories/run_<slug> تحت current_dir، والأهداف الاثنا عشر لها مجلدات.
- النتيجة: 9/12، SMOKE_EXIT=2. لا REPLAY_STALE ولا TRAJECTORY_INCOMPLETE ولا REPLAY_ENV_MISMATCH.
- الحالات الفاشلة وتصنيفها (fixture/trajectory لا محرك):
  - smoke_py_binary_search: 001.json يكتب اختبارًا بلا "from binary_search import binary_search" فيظهر NameError، والدستور يمنع تعديل الاختبار.
  - smoke_ts_fastapi_client: 001.json يكتب class ApiClient بلا export؛ بعد إصلاح التصدير يبقى TS2339 (mockResolvedValue) في ملف الاختبار، والهدف نفسه يفرض نمط jest.mocked(axios, { shallow: true }).
  - smoke_rust_trait_impl: 001.json يكتب Point بحقول غير pub واختبار تكامل ينشئ Point { x, y }.
- repairs:0 في ملخص السكربت غير دقيق (حدثت محاولات إصلاح)؛ بند Ledger مؤجَّل قديم.

حادثة: تشغيل --record حي على السويت كاملة
- شُغّل scripts/sel_smoke_test.sh بـ --record من الفرع docs/release-policy-audit، فاستُدعي المزوّد الحي لكل الحالات. النتيجة 9/12 بمجموعة فاشلين مختلفة (go_worker_pool وts_fastapi_client وrust_trait_impl)؛ المخرجات الحية غير حتمية.
- الأثر: تعديل 28 ملفًا متتبعًا (منها fixtures/trajectories/index.json) وإنشاء 4 ملفات جديدة تحت مجلدات run_*.
- اكتشافان: .gitignore:15 يتجاهل fixtures/trajectories/ فلا تظهر الملفات الجديدة في git status؛ وRecorderProvider يكتب 001.json فما بعد ولا يحذف الملفات القديمة ذات الأرقام الأعلى، ويبني الفهرس من كل ملفات المجلد.
- التنظيف: نسخة احتياطية بعد التسجيل في /tmp/w12-record-snapshot-20261011-221339 (مؤقتة، ليست أرشيفًا دائمًا)؛ استعادة الـ28 ملفًا المتتبعة إلى HEAD بقائمة مدققة؛ حذف الملفات الأربعة الجديدة فقط؛ إبقاء 9 ملفات ignored أقدم خارج نطاق smoke. النتيجة WORKTREE_CLEAN=YES، ولم يُلتزم أي أثر من التسجيل.

مرشّح SMOKE_CASES (الالتزام b9af609)
- SMOKE_CASES="id1,id2" يشغّل الحالات المختارة بترتيب السكربت؛ بدون المتغير السلوك والمخرجات كما كانت.
- التصفية داخل run_case()؛ أسطر استدعاء run_case الاثنا عشر لم تتغير.
- التحقق قبل أي تشغيل: القيمة الفارغة المضبوطة، العنصر الفارغ، المكرر، والمجهول ترفض بخروج 1 مع قائمة المعرّفات. ${SMOKE_CASES+set} يميز غير المضبوط من المضبوط الفارغ.
- اختيار أقل من 12 حالة = PARTIAL RUN بتسميات (جزئي) وأحكام ليست حكم إصدار، ولا يطابق regex smoke في regression_gate.sh. اختيار الاثني عشر صراحة = تشغيل كامل.

RED ثم GREEN (tests/smoke_case_filter.rs، 9 اختبارات، ثنائي stub بلا شبكة)
- RED 1: 7 failed / 2 passed، RED_EXIT=101 (حالة واحدة شغّلت 12، وقُبل المجهول والمكرر والفارغ، ولا تسمية PARTIAL RUN).
- محاولتا ترقيع توقفتا قبل الكتابة (مرساة غير فريدة، ثم سكربت مُنفَّذ غير المسلَّم)؛ لم يُكتب أي بايت فيهما.
- RED 2 بعد تقوية الاختبارات (هوية الهدف لكل معرّف، واختيار الاثني عشر): 8 passed / 1 failed؛ اختيار الاثني عشر وُسم جزئيًا.
- GREEN: 9 passed / 0 failed.

التحقق
- bash -n: ناجح. cargo fmt --check: OK. cargo clippy --locked --all-targets --all-features -- -D warnings: 0.
- cargo test --locked: 750 passed / 0 failed (741 + 9).
- pre-commit hook طبيعي عند b9af609: 36/36 + 30/30 + 18/18 = 84/84، دون تجاوز.

حدود الدليل
- الاختبارات بثنائي stub؛ تثبت الاختيار والتحقق والتسمية، لا تشغيل sel-agent حقيقي. بوابة core لا تشغّل smoke.
- smoke ما زال 9/12 في replay؛ لم تُصلح fixtures في هذه الجلسة.

بنود مفتوحة ظهرت (لم تُعالج)
- ضعف regex smoke في regression_gate.sh: التشغيل الكامل الفاشل يطبع "المجموع: 12 / 12" ولافتة [12/12] فيطابق؛ الحارس الفعلي رمز خروج السكربت عبر pipefail.
- go_worker_pool (تشغيل حي): إصلاح الطفرات يكتب main_test.go فيرفضه الدستور ويستهلك محاولات الإصلاح.
- ts_fastapi_client (تشغيل حي): \" حرفية في أخطاء TS1127؛ السبب غير مثبت.
- fixtures/trajectories/ في .gitignore: أي trajectory جديدة تحتاج git add -f، ويجب عزل مجلد الحالة قبل --record.
- الفرع المحلي docs/release-policy-audit بلا التزامات عند 3838805.


---
**تصحيح (2026-10-14):** الادعاء أعلاه (السطر 33) بأن الأنواع الأربعة ترفض «مع قائمة المعرّفات» غير دقيق للقيمة الفارغة `SMOKE_CASES=""` — لم تكن القائمة مطبوعة حتى الإصلاح `7e4e215`. رسالة `b9af609` أعلنت العقد لكن التنفيذ خالفه حتى ذلك الالتزام.
الخطوة التالية
جلسة fixtures منفصلة: إعادة تسجيل الحالات الثلاث بـ SMOKE_CASES=<id> مع عزل مجلد الحالة، ثم replay قبل أي التزام؛ بعدها full، ثم جلسة الإصدار 9.3.6.
