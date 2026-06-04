# SEL Agent — الوثيقة المرجعية الشاملة

> **الإصدار المعتمد:** v8.5.2  
> **الحالة:** Stable working tree after replay/env fixes  
> **الفرع:** phase2-safe  
> **المؤلف:** Chokri Bouzid  
> **آخر تحديث للحالة المرجعية:** 2026-06-04  
> **الترخيص:** MIT

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

## 2) الفلسفة الأساسية

- **الاختبار هو العقد**: لا يُفترض بالوكيل تعديل الاختبارات الموجودة مسبقًا.
- **الحتمية أولًا**: replay و trajectories جزء أساسي من النظام.
- **القياس قبل التحسين**: لا إضافة بدون benchmark أو signal قياسي واضح.
- **الإصلاح الجذري لا الترقيع**: نصلح السبب البنيوي لا العرض السطحي.
- **تقليل الاعتماد على جودة النموذج**: نقل المعرفة من الـ LLM إلى engine عبر autofix و semantic repair.

## 3) الحالة الحالية المؤكدة

هذه النتائج **مؤكدة من تشغيلات حديثة فعلية** على الشجرة الحالية:

| المقياس | النتيجة |
|--------|---------|
| `cargo check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `bench --suite all --replay` | **36/36 (100%)** |
| `bench-swe --lang all --replay` | **30/30 (100%)** |
| `bench-sel-v11 --replay` | **18/18 (100%)** |
| `bench-real-world --replay` | **14/14 (100%)** |
| `smoke --replay` | **12/12 (100%)** |

### ملاحظة مهمة
الفشل السابق في بعض الحالات لم يعد قائمًا في الشجرة الحالية، بعد إصلاحات بنيوية مؤكدة، خصوصًا:
- Go worker pool
- TS retry
- Python stdlib pip misuse
- Python replay environment mismatch

## 4) البنية المعمارية العامة

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
src/bench_sel.rs	SELBench الداخلي
src/bench_swe.rs	Mini SWE-Bench
src/bench_realworld.rs	Real-World benchmark
src/commands/bench.rs	أوامر البنش العامة
src/commands/compare.rs	المقارنة والتقارير المساعدة
حجم المشروع
ملفات Rust: 60
إجمالي السطور: ~19,874
trajectories directories: ~128
trajectories JSON files: مئات الملفات
5) الدستور (Constitution)
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
6) حماية ملفات الاختبار
في بداية الجلسة:

يتم عمل scan لمساحة العمل
تُجمّع ملفات الاختبار الموجودة مسبقًا
تُحفظ في:
protected_test_files
بالتالي:

اختبارات المستخدم/fixture الأصلية: محمية
اختبارات أنشأها الوكيل داخل الجلسة نفسها: ليست محمية تلقائيًا
هذا هو السلوك المقصود حاليًا.

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
إصلاح مهم جدًا أُضيف مؤخرًا:
إذا كانت trajectory تطلب venv/bin/pytest
والـ venv غير موجودة داخل workspace أثناء replay
يحاول النظام استعادة venv من scaffold cache
بدل السقوط إلى system pytest
Signal معروف في اللوج:

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
stdlib pip misuse:
مثل pip install unittest
8) نظام Replay / Record / Rerecord
ما هي trajectories؟
كل مهمة يمكن تسجيلها كسلسلة نداءات LLM:

001.json
002.json
003.json
...
سلوك مهم
عدد ملفات trajectory ليس ثابتًا:

إذا نجحت الحالة من أول plan → غالبًا 001.json فقط
إذا احتاجت repair → يظهر 002.json أو أكثر
أوضاع التشغيل
الوضع	الوصف
--record	تنفيذ عبر LLM وتسجيل trajectory جديدة
--replay	تنفيذ offline من trajectory موجودة
--replay --rerecord	replay مع healing تلقائي للحالات المكسورة
ملاحظة هندسية مهمة
وجود 001.json وحدها ليس مشكلة بحد ذاته.
المشكلة الحقيقية تظهر إذا دخل replay repair loop بسبب environment mismatch أو trajectory stale، ثم يبدأ بطلب 002.json رغم أن المسار الأصلي one-shot.

9) آخر Bug جذري تم إصلاحه
Python Replay Environment Mismatch
العرض
بعض Python trajectories المسجلة كانت تحتوي:

JSON

"target": "venv/bin/pytest"
لكن أثناء replay كان التنفيذ يهبط فعليًا إلى:

text

python3 -m pytest
ثم إذا لم يكن pytest مثبتًا على النظام:

text

/usr/bin/python3: No module named pytest
فيفشل التنفيذ، ثم يدخل الوكيل repair loop، ثم يطلب:

text

002.json
رغم أن trajectory الحالية كانت one-shot سليمة.

الجذر
Replay لم يكن يحترم البيئة المسجلة faithfully عندما تغيب workspace/venv.

الإصلاح
في src/executor/runner.rs:

إذا كانت trajectory تتطلب venv/bin/pytest
والـ venv غير موجودة
يحاول replay استعادة venv من cache scaffold
وإن تعذر ذلك، يفشل بخطأ replay-env واضح بدل fallback مضلل
النتيجة
عاد:

bench-real-world --tier 3 --replay إلى 3/3
bench-real-world --replay إلى 14/14
10) نتائج البنشماركات الحالية
10.1 SELBench الداخلي
36/36
replay أخضر بالكامل
mutation layer فعالة على الحالات المختبرة
10.2 Mini SWE-Bench
30/30
Python: 10/10
Go: 7/7
Rust: 7/7
TypeScript: 6/6
10.3 SELBench v1.1-rc
18/18
Python: 5/5
Go: 4/4
Rust: 6/6
TypeScript: 3/3
10.4 Real-World
14/14
تشمل:
compile-first
quick-fix
language guard
real-world patterns
10.5 Smoke
12/12
تشمل:
Python binary search
Python dataclass
Go worker pool
Go generics
Go concurrent counter
TS retry
TS utils
TS fastapi client
Rust CSV
Rust Fib
Rust trait impl
11) ما الذي تم إصلاحه في هذه المرحلة من العمل؟
إصلاحات كبرى مؤكدة
Go worker pool:
syntax repair
deadlock repair
test shadow fix
mutation survival fix
TS retry:
fake timers / rejection warning
stronger semantic test
Python:
stdlib pip misuse handling
replay env restoration
UI / benchmark output:
truncation أصبحت UTF-8 safe في المواضع المؤكدة
إصلاحات safety / quality
تقليل الاعتماد على unwrap() في المسارات الحرجة
حماية provider/replay loops
replay صار أكثر faithful للمسار المسجل
12) ما الذي لا يزال مؤجلًا لكنه غير blocking؟
هذه ليست failures حالية، بل تحسينات معمارية مستقبلية:

12.1 Trajectory Manifest
لا يوجد بعد meta.json لكل trajectory directory يحتوي:

عدد النداءات
completion status
outcome
recorded mode
12.2 Unified Reports
لا يوجد بعد ExecutionReport موحد لكل تشغيل.

12.3 Stability Layer
لا يوجد تصنيف رسمي بعد:

PASS
REPLAY_PASS
RERECORD_PASS
FLAKY_RECOVERED
FAIL
12.4 Dependency Graph
الوكيل لا يفهم بعد تأثير تعديل ملف على بقية الملفات graph-wise.

12.5 Pattern Library / Adaptive Routing
لا تزال ضمن roadmap وليست مدمجة بالكامل بعد.

13) أشياء يجب فهمها قبل تحليل أي “تقلب” مستقبلي
إذا ظهرت نتائج متغيرة بين تشغيل وآخر، فالأسباب الطبيعية المعروفة هي:

الفرق بين record وreplay
trajectory stale أو ناقصة
اختلاف أول plan بين providers
one-shot path مقابل repair path
environment mismatch أثناء replay
scaffold cache warm/cold behavior
القاعدة
القياس المستقر = replay
قياس القدرة الحية = record
تحليل الاستقرار الحقيقي يحتاج Stability Layer في v8.6.x
14) خارطة الطريق الحالية
v8.5.3 (مؤجلة كتنظيف فقط، بدون bump الآن)
cleanup
trajectory hygiene
docs refresh
legacy cleanup
v8.6.0
Unified Reports
Observatory
ExecutionReport JSON
v8.6.1
Stability Layer
trajectory manifest
classification:
PASS / REPLAY_PASS / RERECORD_PASS / FLAKY_RECOVERED / FAIL
v8.8.0
Dependency Graph
أول قفزة كبيرة في “فهم المشروع”
v9.2.0+
Pattern Library
Adaptive Repair Routing
Plan Evaluator
15) الملفات الأكثر أهمية في الوضع الحالي
إذا بدأت محادثة جديدة واحتجت الدخول مباشرة إلى جوهر النظام، ابدأ بهذه الملفات:

src/executor/runner.rs
src/executor/file_ops.rs
src/executor/autofix.rs
src/state_handlers.rs
src/decision.rs
src/agent.rs
src/scaffold_engine.rs
src/llm/replay.rs
src/llm/record.rs
src/bench_realworld.rs
src/bench_swe.rs
src/bench_sel.rs
16) أوامر مرجعية مهمة
Bash

# Build / quality
cargo check
cargo clippy --all-targets --all-features -- -D warnings

# Replay benchmarks
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay

# Replay with healing
./target/release/sel-agent bench-real-world --replay --rerecord
./target/release/sel-agent bench-sel-v11 --replay --rerecord

# Smoke
bash sel_smoke_test.sh ./target/release/sel-agent --replay
17) الملخص التنفيذي النهائي
text

SEL Agent v8.5.2 — stabilized working tree

Verified:
- bench all replay: 36/36
- bench-swe replay: 30/30
- bench-sel-v11 replay: 18/18
- bench-real-world replay: 14/14
- smoke replay: 12/12
- clippy -D warnings: pass

Last root-cause fix:
- Python replay environment mismatch in src/executor/runner.rs

Current maturity:
- deterministic fixes are strong
- replay is materially more faithful
- no known benchmark blocker remains
- next strategic work is observability + stability layer + dependency graph
18) ملخص قصير جدًا لبداية أي محادثة جديدة
إذا أردت اختصارًا شديدًا، استخدم هذا النص في أول رسالة:

المشروع هو SEL Agent (Rust).
الإصدار المرجعي الحالي: v8.5.2 بدون bump جديد.
الحالة الحالية مثبتة كالتالي:

bench all re- bench all replay: 36/36
bench-swe replay: 30/30
bench-sel-v11 replay: 18/18
bench-real-world replay: 14/14
smoke replay: 12/12
clippy -D warnings: pass
آخر إصلاح جذري مهم كان في src/executor/runner.rs: إصلاح Python replay env mismatch بحيث replay يستعيد cached venv عندما تتطلب trajectory venv/bin/pytest.
الأولويات القادمة: v8.6.0 Unified Reports + Observatory، ثم v8.6.1 Stability Layer + trajectory manifest، ثم v8.8.0 Dependency Graph.
