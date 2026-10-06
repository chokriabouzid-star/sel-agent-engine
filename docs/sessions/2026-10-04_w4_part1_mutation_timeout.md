جلسة 2026-10-04: W4/Part-1 — mutation_check يقتل مجموعة عمليات الطافر عند المهلة

الهدف
إصلاح محرك واحد (W4، الجزء الأول فقط): حلقة الطفرات كانت تستخدم
tokio::time::timeout(30s, Command::output())، فتُسقط الـ future عند المهلة دون قتل
مجموعة العمليات، فيبقى أحفاد عدّاء الاختبار أحياء، وتُتجاهل self.timeout_secs كليًا.
حارس RAII للاستعادة (الجزء الثاني) خارج نطاق هذه الجلسة عمدًا.

خط الأساس
f44b5d9 على main، شجرة نظيفة.
cargo test --locked = 722 passed / 0 failed.

التشخيص (بالسطر)
src/executor/mutation.rs: الاستدعاء كان tokio::time::timeout(30s, Command::output()).
إسقاط الـ future يقتل الابن المباشر فقط (سلوك tokio)، فينجو الأحفاد.
هذا كان الموضع الوحيد في المحرك الذي لا يمرّ عبر executor::process::output_with_timeout،
بينما core.rs:193 وrunner.rs (68, 212, 267, 322, 431) تمرّ كلها عبره.
git blame أثبت أن الثابت 30s جاء من c90b218 ضمن "chore: upgrade to v8.3.0 + cleanup"
بلا مبرر مكتوب، ولا يرتبط بأي عقد موثّق.

ما تم إنجازه
الفرع: fix/mutation-process-guard
RED أولًا: tests/mutation_lifecycle_safety.rs (unix-only، اختبار واحد يستدعي
SafeExecutor::mutation_check مباشرة):
- مصدر calc.py بنمط طفرة واحد فقط (" + ") كي لا تتضاعف المهلة.
- pytest مزيف في venv/bin/pytest (المسار الذي يفضّله mutation_check): يسبون sleep 300،
  يسجّل PID الحفيد في ملف، ثم يعلّق.
- SafeExecutor::new(ws, 1) أي timeout_secs = 1.
على f44b5d9: FAILED في 30.16s مع "grandchild PID 35425 is still alive after
mutation_check returned". الاختبار ينظّف الحفيد بنفسه حتى لا تسرّب تشغيلة RED عملية.
GREEN: بناء tokio::process::Command صراحة وتمريره إلى output_with_timeout مع
deadline = min(self.timeout_secs, 30)، وتغيير المطابقة من Ok(Ok(result)) إلى
Ok(Some(result)). بعد الإصلاح: ok في ~1.1s والحفيد ميت.

عقد المهلة المعتمد ولماذا
deadline = min(self.timeout_secs, 30).
الطفرات تُشغَّل بالتتابع داخل mutation_check، بخلاف core.rs وrunner.rs اللذين يشغّلان
أمرًا واحدًا، لذلك لم تُعتمد مطابقتهما حرفيًا. السقف يحفظ أسوأ حالة تاريخية لكل طافر
(إنتاج 120 و60 يصيران 30)، والمهل الأصغر صارت تُحترَم (اختبار 1s).
دلالة النتيجة: Ok(Some(output)) يحافظ على منطق Strong/Weak/Uncompilable،
وOk(None) لا يُحسب Strong ولا Weak، وErr يبقى على سلوك التخطي السابق.

البوابات
cargo check --locked --tests = 0
cargo fmt --all -- --check = 0
cargo clippy --locked --all-targets --all-features -- -D warnings = 0
cargo test --locked = 723 passed / 0 failed (722 + اختبار W4-1)
tests/rust_nested_mutation.rs (W2) = 3/3 ok على نفس مسار الكود
regression_gate.sh core عبر hook pre-commit الطبيعي: 36/36 + 30/30 + 18/18 = 84/84،
بلا --no-verify
الكوميت: a652387، ثم ff-only إلى main ودفع origin/main
وorigin/fix/mutation-process-guard.

الملفات المعدلة
src/executor/mutation.rs (ترحيل المهلة + مطابقة Ok(Some(..)))
tests/mutation_lifecycle_safety.rs (جديد)

ما لم يُحل
- W4/Part-2: الاستعادة ما زالت يدوية عبر std::fs::write(&source_path, &original) في
  أربعة مواضع. عند إلغاء الـ future أو panic قد يبقى ملف المستخدم مطفّرًا على القرص.
  فرع مقترح: fix/mutation-raii-restore، بدورة RED/GREEN مستقلة.
- الاختبار لا يثبت آليًا التحويل 120→30؛ إثباته المباشر يكلف 30s في كل تشغيل سويت.
  السقف موثّق في الكود وفي رسالة الالتزام فقط.
- بند TODO الآخر "grandchildren survive a killed parent" يخص مسارات أخرى ولم يُلمس هنا.

الدروس المستفادة
- RED الحقيقي يجب أن يثبت عقد الدالة نفسها: وجود اختبار يثبت أن output_with_timeout
  يعمل لا يكفي لإثبات أن mutation_check يستخدمه. لولا ذلك لمرّت أي عودة للثابت القديم
  دون كسر أي اختبار.
- اختبار RED مكلف زمنيًا مرة واحدة (30s) مقبول إذا صار بعد GREEN ~1s دائمًا.
- عند الخلاف على قرار تصميم (سقف المهلة)، يُحسم بقراءة git blame وبقية مواقع الاستدعاء
  قبل كتابة أي كود.
- وثّق قبل الدمج لا بعده: W2 وُثّقت قبل الدمج، وW4/Part-1 دُمجت ثم وُثّقت في التزام لاحق.
