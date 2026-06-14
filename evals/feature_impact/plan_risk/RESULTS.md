# Plan Risk — Impact Eval Results
**milestone:** v9.2.0 closeout  
**date:** 2026-06-14  
**branch:** refactor/v9.2.5-decision-split  

---

## Gate Conditions

| المعيار | الهدف | النتيجة |
|---------|-------|---------|
| `success_rate_after >= success_rate_before` | لا انخفاض | ✅ |
| `avg_replan_count <= 0.5` | إعادة تخطيط نادرة | ✅ |
| `false_positive_replans == 0` | لا إعادة تخطيط زائفة | ✅ |
| `tokens_per_task` مُسجَّل | data collection فقط | ✅ |

---

## Replay Suite Results

| Suite | النتيجة |
|-------|---------|
| bench all --replay | 36/36 ✅ |
| bench-swe --replay | 30/30 ✅ |
| bench-sel-v11 --replay | 18/18 ✅ |
| bench-real-world --replay | 14/14 ✅ |
| smoke --replay | 12/12 ✅ |

---

## Plan Risk Behavior — حالات التحقق

| الحالة | السلوك المتوقع | النتيجة |
|--------|----------------|---------|
| `write_file` على test موجود | يُعاد التخطيط | ✅ `plan_risk_triggered = true` |
| `patch_file` على source فقط | لا يُعاد التخطيط | ✅ لا replan |
| `delete_file` في الخطة | يُعاد التخطيط | ✅ `plan_risk_triggered = true` |
| خطة بسيطة آمنة (source فقط) | لا false positive | ✅ `plan_risk_triggered = false` |

---

## Telemetry Fields — حالة الإضافة

| الحقل | الموقع | الحالة |
|-------|--------|--------|
| `tokens_used: u64` | `ExecutionContext` | ✅ مُضاف |
| `plan_confidence: Option<f32>` | `ExecutionContext` | ✅ مُضاف — يبقى `None` حتى v11.0 |
| `total_tokens: u64` | `ExecutionReport` | ✅ مُضاف |
| `avg_tokens_per_task: u64` | `ExecutionReport` | ✅ مُضاف |
| `tokens_in / tokens_out` | `ExecutionReport` | ✅ موجود منذ v9.2.1 |
| `plan_risk_triggered` | `ExecutionReport` | ✅ موجود منذ v9.2.1 |
| `replan_count` | `ExecutionReport` | ✅ موجود منذ v9.2.1 |
| `plan_risk_reasons` | `ExecutionReport` | ✅ موجود منذ v9.2.1 |

---

## التعريفات المعتمدة
total_tokens = tokens_in + tokens_out
avg_tokens_per_task = total_tokens / llm_calls (0 إذا llm_calls == 0)
plan_confidence = None حالياً — البيانات تُجمع من v9.2.0 للاستخدام في v11.0

text


---

## الملفات المعدّلة
src/types.rs
src/report.rs
src/agent.rs

text


---

## القيود المعروفة المتبقية

| القيد | الخطوة التالية |
|-------|----------------|
| `plan_confidence` لا تُحسب بعد | v11.0 — بعد جمع بيانات كافية |
| `avg_tokens_per_task` = per LLM call لا per semantic task | مقبول حتى يُعرَّف task counter صريح |
| `BudgetReport` لا يصل إلى `ExecutionReport` | v9.3.0 |

---

## الخلاصة

milestone v9.2.0 closeout **مغلق بالكامل**.  
جميع gate conditions حققت شروطها.  
التلميتري الجديد يُكتب في كل تقرير JSON ابتداءً من هذا الإصدار.
