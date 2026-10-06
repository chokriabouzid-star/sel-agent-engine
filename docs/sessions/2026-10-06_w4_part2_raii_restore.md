جلسة 2026-10-06: W4/Part-2 — mutation_check يستعيد المصدر عبر حارس RAII عند الإلغاء

الهدف
إصلاح محرك واحد (W4، الجزء الثاني): استعادة المصدر كانت أربعة استدعاءات يدوية
std::fs::write(&source_path, &original) كلها بعد الـ .await الوحيد في الحلقة.
إسقاط الـ future عند تلك النقطة، أو panic قبل سطر الاستعادة، يترك ملف المستخدم
مطفّرًا على القرص.

خط الأساس
119fc14 على main، شجرة نظيفة.
cargo test --locked = 723 passed / 0 failed.

التشخيص
الملف يُكتب مطفّرًا قبل استدعاء output_with_timeout(...).await.
كانت الاستعادات اليدوية الأربع بعد الـ await، فلا تُنفَّذ عند إسقاط الـ future.
kill_on_drop ينظّف العملية لكنه لا يعرف ملف المصدر.

ما تم إنجازه
الفرع: fix/mutation-raii-restore
RED أولًا: tests/mutation_restore_guard.rs (unix-only)
- calc.py بنمط طفرة واحد (" + ") فنافذة إلغاء واحدة.
- pytest مزيف يلمس علامة started ثم exec sleep 300؛ الابن المباشر هو النائم،
  لذلك لا يتسرب حفيد من الاختبار نفسه.
- timeout_secs=10 حتى لا تُطلَق المهلة الداخلية قبل الإلغاء المتعمد.
- Box::pin (وليس tokio::pin!) + tokio::select! حتى تظهر العلامة، ثم assert_ne
  أثناء التشغيل لإثبات أن الملف مطفّر، ثم drop(fut)، ثم assert_eq مع الأصل.
على 119fc14: FAILED في 0.09s:
  left:  "def add(x, y):\n    return x - y"
  right: "def add(x, y):\n    return x + y\n"

GREEN:
- إضافة SourceRestoreGuard { path, original, armed }.
- arm() قبل كل كتابة طفرة.
- restore() يعيد الأصل ويطفئ الحارس عند فشل الكتابة، وبعد الـ await، وعند الخروج.
- Drop يستدعي restore()، فيغطي إلغاء الـ future وunwind الـ panic.
- أُزيلت الاستعادة اليدوية الزائدة قبل return Uncompilable.
بعد الإصلاح: الاختبار ok في 0.09s.
W4/Part-1 = 1/1، وW2 = 3/3.

عقد الحارس
armed == true ⇔ الملف على القرص قد يحمل طافرًا.
كل نافذة arm → await → restore محمية لكل طافر على حدة.
لم يتغير: deadline = min(self.timeout_secs, 30)، ومطابقة Ok(Some)/Ok(None)،
ودلالات Strong/Weak/Uncompilable.

البوابات
cargo fmt --all -- --check = 0
cargo clippy --locked --all-targets --all-features -- -D warnings = 0
cargo test --locked = 724 passed / 0 failed
regression_gate.sh core عبر hook pre-commit: 36/36 + 30/30 + 18/18 = 84/84،
بلا --no-verify
الكوميت: ed3b8fc

الملفات المعدلة
src/executor/mutation.rs (SourceRestoreGuard + مركزة الاستعادة)
tests/mutation_restore_guard.rs (جديد)

ما لم يُحل
- الاختبار يغطي إلغاء الـ future، لا panic صريحًا؛ Drop يغطي unwind نظريًا.
- الاختبار unix-only؛ الحارس نفسه ليس خاصًا بمنصة.

الدروس المستفادة
- tokio::pin! + drop لا يُسقط الـ future الحقيقي قبل نهاية الـ scope؛ Box::pin يُسقطه فورًا.
- لا tokio::spawn لاختبار الإلغاء دون إثبات أن SafeExecutor: Send + 'static.
- الاقتطاع البصري للأحرف اللاتينية بجوار النص العربي لا يُحسم بالعرض؛ فحص bytes وgit show
  هما الدليل النهائي.
- التوثيق يجب أن يسبق الدمج: Part-2 توثّق قبل دمجها في main.
