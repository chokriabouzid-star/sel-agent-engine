جلسة 2026-10-10: W6 — منع EXPLAIN MODE من انتظار stdin عند SEL_NON_INTERACTIVE=1

الهدف
منع دخول EXPLAIN MODE التفاعلي وقراءة stdin عند التصريح الصريح بأن التشغيل غير تفاعلي، حتى إذا أبلغ كل من stdin وstdout أنهما طرفيتان. هذا السجل لا يدّعي اختبار تعليق طرفي فعليًا.

خط الأساس
- الفرع: fix/explain-non-interactive، بدأ من b9d3006، مع شجرة نظيفة.
- cargo test --locked قبل W6: 734 passed / 0 failed، والخروج 0.
- الالتزام الإنتاجي النهائي: a0e4649
  fix(agent): honor SEL_NON_INTERACTIVE to skip EXPLAIN MODE stdin wait

التشخيص
في src/agent.rs داخل AgentState::WaitingForUserInput، كانت الشروط القديمة تتخطى EXPLAIN عند وضع Bench أو غياب طرفية stdin أو stdout. لم يكن هناك فحص صريح لـ SEL_NON_INTERACTIVE؛ لذلك إذا كان الطرفان طرفيتين وكان التشغيل معلنًا صراحةً كغير تفاعلي، كان المسار يصل إلى stdin.read_line(). لم يُنفذ اختبار PTY لإعادة إنتاج انتظار فعلي.

التغيير الإنتاجي
- أُخرج قرار التخطي إلى should_skip_explain ومدخله قيم صريحة، من دون قراءة البيئة داخل الدالة.
- حُفظت أولوية bench_mode ووجود SEL_BENCH_MODE، وبقي عقد SEL_BENCH_MODE كما كان: is_ok().
- بقي فحص كل من stdin وstdout terminal.
- أصبح SEL_NON_INTERACTIVE مفعّلًا فقط عندما تكون قيمته النصية "1".
- عند التخطي بقيت رسالة [Bench] وسبب الفشل max_repairs_bench كما كانا؛ لم يُفصل سبب خاص لغير التفاعلي في W6.
- بقي read_line في المسار الذي لا يطلب التخطي.

RED ثم GREEN
- src/main.rs:9 يسجّل mod agent؛ لذا اختبارات هذا الملف في هدف الـbinary sel-agent، لا في هدف المكتبة.
- أعطى التشغيل الأول عبر --lib صفر اختبارات مطابقة و227 filtered out؛ لم يكن ذلك RED ولا دليلًا على الاختبارات الجديدة.
- سُجّلت RED الصحيحة عبر:
  cargo test --locked --bin sel-agent explain_skip_tests -- --nocapture
  النتيجة: 7 اختبارات، 6 passed و1 failed، TEST_EXIT=101. فشل اختبار non-interactive assertion دلاليًا: actual None وexpected Some(NonInteractive).
- بعد الإصلاح، الاختبارات السبعة في الهدف نفسه: 7 passed / 0 failed.
- بقيت حراس الأولوية وغياب كل من stdin/stdout والتشغيل التفاعلي خضراء.
- اسم اختبار RED في المصدر يصف مرحلته التاريخية؛ الاختبار النهائي أخضر، ولا ينبغي فهم الاسم أو التعليق على أنه فشل حالي.

التحقق وبوابة Replay
- cargo check --locked: exit 0.
- cargo fmt --all -- --check: OK.
- cargo clippy --locked --all-targets --all-features -- -D warnings: exit 0.
- cargo test --locked بعد W6: 741 passed / 0 failed، TEST_EXIT=0؛ أي 734 اختبار خط الأساس زائد الاختبارات السبعة.
- عند الالتزام a0e4649 شغّل pre-commit hook بوابة core طبيعيًا، دون تجاوز:
  bench all replay 36/36 + bench-swe replay 30/30 + bench-sel-v11 replay 18/18 = 84/84.
- الالتزام غيّر src/agent.rs وحده: 105 insertions(+), 5 deletions(-).

حدود الدليل
- اختبار الوحدة يختبر قرار الدالة بقيم منطقية؛ لا يضبط متغير البيئة في العملية.
- مراجعة call site تثبت تمرير نتيجة المقارنة الدقيقة SEL_NON_INTERACTIVE == "1" قبل مسار read_line؛ لا يوجد اختبار طرف إلى طرف تحت PTY.
- لم تُختبر قيم البيئة "0" والفارغة باختبار مستقل؛ الكود لا يقبل إلا "1".
- ظلت رسالة وسبب التخطي التاريخيان كما هما، بما في ذلك تسمية max_repairs_bench لحالة non-interactive.
- لم يوجد بند W6 مطابق في TODO.md، ولم يُعدّل TODO.md.
- يُحتفظ بجدول الحالة التاريخية أعلى Stability Ledger كما هو؛ نتيجة 494 فيه ليست قياس W6.

