# SEL Agent — Feature Impact Protocol

## الهدف
هذا البروتوكول يضمن أننا لا نكتفي فقط بأن الميزة الجديدة:
- لا تكسر النظام
- وتجتاز الاختبارات

بل نثبت أيضًا أنها:
- تحسن سلوك الوكيل فعلًا
- أو تقلل المخاطر
- أو تقلل التكلفة
- أو ترفع الاستقرار

---

## المبدأ
كل ميزة جديدة أو تحسين مهم يجب أن يمر عبر مرحلتين:

1. **Safety Verification**
   - cargo check
   - cargo test
   - cargo clippy --all-targets --all-features -- -D warnings
   - regression_gate حسب مستوى التغيير

2. **Impact Verification**
   - baseline قبل الميزة
   - نفس الحالات بعد الميزة
   - مقارنة قبل/بعد
   - استنتاج صريح: هل تحسن السلوك فعلًا أم لا؟

---

## متى يجب استخدام هذا البروتوكول؟
استخدمه عند أي تغيير يؤثر على:
- planning behavior
- repair behavior
- prompt construction
- context selection
- pattern memory
- autofix logic
- retry/replan decisions
- benchmark determinism

لا يلزم عادة للتغييرات الصغيرة جدًا مثل:
- refactor داخلي بلا تغيير سلوكي
- rename فقط
- cleanup docs فقط

---

## أنواع التحقق

### 1) Canary Case
حالة واحدة صغيرة، مرتبطة مباشرة بالميزة.

مثال:
- Goal Clarity → هدف غامض
- Plan Risk → خطة تستخدم write_file على ملف موجود
- Prompt Budget → حالة ذات prompt طويل

### 2) Micro-Suite
من 3 إلى 5 حالات قصيرة تمثل نفس السلوك، وتعطي إشارة أقوى من حالة واحدة.

### 3) Main Gate
التحقق العام المعتاد:
- cargo test
- clippy
- regression gate

---

## منهجية القياس

### قبل الميزة
شغّل:
- الحالة/الحالات نفسها
- بنفس الإعداد
- وسجل النتائج

### بعد الميزة
أعد نفس التشغيل:
- بنفس البيئة
- ونفس replay mode إن أمكن
- وقارن النتائج

---

## المقاييس الأساسية

### مقاييس عامة
- success / fail
- duration
- repair_attempts
- replan_count
- llm_calls
- autofix_count

### مقاييس التخطيط
- plan_risk_rejections
- touches_existing_test_files
- write_file_on_existing_source
- command_count

### مقاييس الإصلاح
- same_error_streak
- route_used
- pattern_hit
- context_files_selected
- prompt_chars

### مقاييس الاستقرار
- deterministic replay pass/fail
- flake frequency
- full/core regression gate pass/fail

---

## قرار النجاح
الميزة تعتبر ناجحة إذا تحقق واحد أو أكثر من التالي:
- نفس النجاح مع تكلفة أقل
- نجاح أعلى على نفس الحالات
- عدد repairs أقل
- خطط أكثر أمانًا
- prompt أصغر بدون فقدان الجودة
- تراجع flakiness أو replay failures

إذا لم يتحسن شيء واضح:
- لا نعتبر الميزة مثبتة الأثر
- يمكن الاحتفاظ بها فقط إذا كانت safety/maintainability improvement موثقة بوضوح

---

## نموذج التوثيق لكل ميزة

لكل ميزة جديدة أنشئ ملفًا في:
```text
evals/feature_impact/<feature-name>/README.md
ويجب أن يحتوي على:

Markdown

# Feature Impact Eval

## Feature
اسم الميزة

## Hypothesis
ما الذي نتوقع أن تحسنه؟

## Cases
- case 1
- case 2
- case 3

## Before
- success/fail
- repairs
- duration
- notes

## After
- success/fail
- repairs
- duration
- notes

## Conclusion
- positive / neutral / negative
- هل نحتفظ بالميزة؟
- هل نحتاج iteration أخرى؟
أفضل الممارسات
1) قارن داخل نفس البيئة
يفضل replay mode إن أمكن.

2) لا تعتمد على حالة واحدة فقط
ابدأ بـ Canary، لكن لا تستنتج من حالة واحدة وحدها إذا كانت الميزة كبيرة.

3) إن أمكن، استخدم ON/OFF toggle
إذا كانت الميزة قابلة للتعطيل عبر env var أو config:

شغّل ON
شغّل OFF
قارن داخل نفس الكود
هذا أفضل من المقارنة بين commitين متباعدين.

4) فرق بين safety و impact
قد تكون الميزة:

صحيحة هندسيًا
ولا تكسر النظام
لكنها بلا أثر عملي واضح
وهذا يجب أن يقال بوضوح.

المستوى الأدنى المقبول قبل الدمج
لكل ميزة سلوكية جديدة:

Canary case واحد على الأقل
Micro-suite إذا كانت الميزة تؤثر على planning/repair
cargo test
clippy
regression gate مناسب للحجم
أمثلة على ميزات يجب قياس أثرها
Goal Clarity
Plan Risk Evaluator
Global Prompt Budget
Pattern Trust Gating
Pattern Graduation → AutoFix
Smart Context Scoring changes
Replay determinism fixes
مخرجات مطلوبة قبل إعلان أي milestone
قبل أن نقول:

"الميزة نجحت"
أو "v9.2 جاهزة"
يجب أن نملك:

نتيجة safety verification
نتيجة impact verification
استنتاج مكتوب وقابل للمراجعة
