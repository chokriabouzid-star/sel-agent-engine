# Context Budget — Impact Eval Results
**milestone:** v9.3.0
**date:** 2026-06-14
**branch:** refactor/v9.2.5-decision-split

---

## Gate Conditions

| المعيار | الهدف | النتيجة |
|---------|-------|---------|
| `regression_gate core` | PASS | ✅ |
| `smoke --replay` | 12/12 | ✅ |
| `avg_context_tokens` في report | يُكتب | ✅ |
| `avg_selected_files` في report | يُكتب | ✅ |
| `context_reduction_pct` في report | يُكتب | ✅ |
| force_include files لا تُحذف | مُثبت من v9.1.0 | ✅ |
| success_rate لا ينخفض | 12/12 = 100% | ✅ |

---

## Replay Suite Results

| Suite | النتيجة |
|-------|---------|
| bench all --replay | 36/36 ✅ |
| bench-swe --replay | 30/30 ✅ |
| bench-sel-v11 --replay | 18/18 ✅ |
| smoke --replay | 12/12 ✅ |

---

## التغييرات المعمارية

### قبل v9.3.0
build_repair_context_block() → String
BudgetReport يُبنى ويُفقد داخل builder
ExecutionReport لا يحتوي context metrics

text


### بعد v9.3.0
build_repair_context_block() → (String, BudgetReport)
BudgetReport يُجمّع في ExecutionContext عبر كل repair
ExecutionReport يحتوي:
avg_context_tokens = context_tokens_total / context_budget_samples
avg_selected_files = context_files_total / context_budget_samples
context_reduction_pct = (tokens_before - tokens_after) / tokens_before * 100

text


---

## الحقول المُضافة

| الحقل | الموقع | الوصف |
|-------|--------|-------|
| `context_tokens_total` | `ExecutionContext` | مجموع tokens_after عبر كل repairs |
| `context_tokens_before_total` | `ExecutionContext` | مجموع tokens_before لحساب reduction |
| `context_files_total` | `ExecutionContext` | مجموع selected_files |
| `context_budget_samples` | `ExecutionContext` | عدد repair loops |
| `avg_context_tokens` | `ExecutionReport` | متوسط tokens/repair |
| `avg_selected_files` | `ExecutionReport` | متوسط files/repair |
| `context_reduction_pct` | `ExecutionReport` | نسبة تقليل الـ context |

---

## ملاحظات

- القيم = 0 في runs بدون repairs — هذا سلوك صحيح
- البيانات ستتراكم تلقائياً في runs مع repair loops
- `#[serde(default)]` يحمي التوافق مع تقارير v9.2.6 القديمة
- gate لـ avg_context_tokens >= 20% reduction يتطلب بيانات live — مؤجل لـ v9.3.1 بعد جمع data

---

## القيود المعروفة لهذه النسخة

| القيد | الخطوة التالية |
|-------|----------------|
| لا قياس فعلي لـ 20% reduction بعد | v9.3.1 بعد جمع live data |
| planning context لا يُقاس | مستقبلي |

---

## الخلاصة

v9.3.0 **مغلق من الناحية البنيوية**.
البيانات تُجمع الآن في كل repair loop.
القياس الفعلي لـ >= 20% reduction يحتاج live runs مع repairs.
