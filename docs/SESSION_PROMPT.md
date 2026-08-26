# برومبت بداية الجلسة — الصقه في كل محادثة جديدة

انسخ ما يلي وألصقه كأول رسالة في أي محادثة جديدة مع أي نموذج:

---

## السياق

أنا أعمل على مشروع Rust اسمه `sel-agent-engine`.
هو وكيل ذكي يحل مهام برمجية تلقائيًا (Python/Go/Rust/TypeScript).
المشروع موجود في `~/projects/active/sel-agent-engine`.

### البنية الأساسية:
src/
agent.rs — حلقة التنفيذ الرئيسية
protocol.rs — parser لردود LLM → أوامر مهيكلة (Cmd)
state_handlers.rs — إدارة الحالات (Planning/Executing/Repairing)
executor/
core.rs — تنفيذ أوامر shell/write/patch
runner.rs — تنفيذ run_tests (pytest/cargo/go/jest)
run_policy.rs — سياسات preflight للأوامر
llm/
replay.rs — إعادة تشغيل trajectories مسجلة
record.rs — تسجيل trajectories جديدة
scaffold_engine.rs — تجهيز بيئة العمل
snapshot.rs — إدارة git stash/restore
bench_sel.rs — بنشمارك SEL v1.1
bench_swe.rs — بنشمارك SWE
scripts/
regression_gate.sh — بوابة الانحدار (يجب أن تمر قبل أي commit)
fixtures/
trajectories/ — تسجيلات replay لكل حالة اختبار

text


### معايير الاستقرار:
```bash
cargo fmt --check
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
scripts/regression_gate.sh core   # يشترط: 36/36 + 30/30 + 18/18
scripts/regression_gate.sh full   # يضيف: real-world 14/14 + smoke 12/12
الحالة الحالية:
[هنا الصق محتوى آخر تحديث من docs/STABILITY_LEDGER.md]

قواعد العمل:
لا تعدّل كودًا قبل أن تقرأ الملفات المعنية
أعطني أوامر قراءة أولًا، ثم شخّص، ثم اقترح
تغيير واحد → تحقق واحد → لا تعديل واسع
لا git reset --hard بدون تحليل ما سيُفقد
لا تفترض أنك تعرف محتوى أي ملف
المهمة اليوم:
[صِف المهمة بجملة واحدة واضحة]

ما الممنوع اليوم:
[حدد ما لا يجب لمسه]

