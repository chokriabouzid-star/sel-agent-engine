جلسة 2026-10-09: W5 — Node Oracle يحترم package.json scripts.test ويفهم TAP

الهدف
إصلاح محرك واحد (W5): مشروع Node يعلن "scripts": {"test": "node --test"} كان
يُجبر على npx jest --runInBand --forceExit، أو تُحقن أعلام Jest في npm test،
فيرفضها node: bad option: --runInBand / --forceExit.

خط الأساس
fa568b6 على main، شجرة نظيفة.
cargo test --locked = 724 passed / 0 failed.

التشخيص (بالسطر)
src/workspace_oracle.rs: ذراع ProjectType::Node في resolve_test_command كان
يقرر من نص الهدف فقط (contains jest/test/.js/.ts/npm) دون قراءة محتوى
package.json إطلاقًا؛ وجود الملف يُستخدم للكشف عن نوع المشروع فقط.
src/executor/runner.rs:351: مسار Node كان يفسر كل المخرجات عبر parse_jest؛
ملخص TAP الخاص بـ node:test (# pass / # fail) يعطي (0,0) فيُصنف النجاح فشلًا
بسبب شرط passed > 0.
حسم تناقض ظاهري أثناء القراءة: لا إعادة كتابة للأوامر في runner.rs؛ الفرق بين
نتائج "npm test" و"auto" في RED سببه فروع نص الهدف داخل الـ Oracle نفسه.

ما تم إنجازه
الفرع: fix/node-oracle-scripts-test
RED أولًا: tests/node_scripts_test.rs (4 اختبارات):
- explicit "npm test" أعاد npx بدل npm.
- "auto" حقن ["test","--","--runInBand","--forceExit"].
- fallback بلا scripts.test بقي أخضر (حارس).
- E2E عبر "auto" (اختير عمدًا بدل npm test حتى لا يصل npx إلى الشبكة):
  فشل حتمي offline بدليل node: bad option: --runInBand.
على fa568b6: 3 failed / 1 passed.

GREEN على ثلاث خطوات (ملف واحد لكل خطوة، cargo check بعد كل تعديل):
1) workspace_oracle.rs: دالة package_json_test_script() تقرأ scripts.test
   (نص غير فارغ) من package.json في جذر الـ workspace؛ عند وجوده يُعاد
   ("npm", ["test"]) حرفيًا — npm يوفر node_modules/.bin على PATH فيُحترم أي
   عدّاء معلن (node --test، jest، vitest، mocha). الـ fallback القديم محفوظ.
   بعدها: اختبارات Oracle الثلاثة خضراء، وE2E تحول إلى فشل وسيط متوقع:
   stdout=0 passed, 0 failed رغم TAP فيه # pass 1 — إثبات RED-2 للـ parser.
2) executor/parsers.rs: parse_node_tests() تقرأ ملخص TAP (# pass N / # fail N
   عند وجودهما معًا) وإلا fallback إلى parse_jest. + 3 اختبارات وحدة
   (نجاح TAP، فشل TAP (1,1)، وfallback لـ Jest). اختبار فشل TAP أُضيف لاحقًا
   لسد فجوة: عقد failed == 0 في success لم يكن مغطى.
3) runner.rs سطر واحد: parse_jest → parse_node_tests في مسار Node.
بعد الإصلاح: node_scripts_test = 4/4 ok، وE2E يطبع Running: npm test ثم
node --test وينتهي 1 passed, 0 failed.

البوابات
cargo fmt --all -- --check = 0
cargo clippy --locked --all-targets --all-features -- -D warnings = 0
  (تحذير parse_node_tests never used زال بعد ربط الـ runner)
cargo test --locked = 734 passed / 0 failed
  (724 + 4 تكامل + 3 وحدات parser تُحسب مرتين في هدفي lib وbin)
guard_node_arm_is_untouched (بلا scripts.test) = ok
regression_gate.sh core عبر hook pre-commit الطبيعي: 36/36 + 30/30 + 18/18
  = 84/84، بلا --no-verify — وهذا حسم أيضًا سلامة مهام TypeScript في البنش
  (fixtures تعلن "test": "jest" فصارت تعمل عبر npm test بلا الأعلام).
الكوميت: b333440

الملفات المعدلة
src/workspace_oracle.rs (package_json_test_script + أسبقية ذراع Node)
src/executor/parsers.rs (parse_node_tests + 3 اختبارات وحدة)
src/executor/runner.rs (سطر استدعاء الـ parser)
tests/node_scripts_test.rs (جديد — 4 اختبارات انحدار)

حدود النطاق (لم تُحل عمدًا — بلا RED لا تعديل)
- ذراع `_` في resolve_test_command (مشروع Unknown بهدف فارغ يكتشف Node) ما
  زال يحقن أعلام Jest.
- ذراع js|ts في mutation.rs ما زال يفرض jest.
- الأسبقية تقرأ package.json في جذر الـ workspace فقط، لا مشاريع متداخلة.
- أخطاء كتابة package.json (JSON تالف / scripts ليست كائنًا) تسقط بصمت إلى
  الـ fallback — سلوك مقصود لكنه غير مُختبر صراحة.

الدروس المستفادة
- فحوص الاقتطاع يجب أن تطابق حدود كلمات كاملة لا بوادئ: نمط \brepla(?!y)
  أنتج 11 إنذارًا كاذبًا (replan_count/replace)؛ حُسم بـ BARE TOKENS: 0.
- E2E في RED يجب أن يُصاغ بحيث يبقى offline على الكود المعطوب أيضًا
  (اختيار "auto" بدل "npm test" منع npx من الوصول للشبكة).
- فشل وسيط متوقع بين خطوات GREEN دليل تصميم لا ضجيج: فشل E2E بعد GREEN-1
  كان هو RED الفعلي للـ parser.
