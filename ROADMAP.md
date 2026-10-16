# ROADMAP — SEL Agent Engine

> خارطة قصيرة. حدّثها نهاية كل جلسة.

## الهدف الحالي (جملة واحدة)

إصلاح الحالتين الفاشلتين في Smoke (`smoke_rust_trait_impl` + `smoke_ts_fastapi_client`)
لتحقيق `12/12`، ثم تشغيل `regression_gate.sh full`، ثم إصدار `v9.3.6`.

---

## ما تم قبوله

| التاريخ | ما قُبل | الالتزام |
|---|---|---|
| 2026-10-15 | إصلاح fixture `smoke_py_binary_search` (Smoke 9/12 → 10/12) | `b3d1c12` |
| 2026-10-16 | توثيق جلسة Python + مدخلة Ledger | `db1202b` |
| 2026-10-14 | عقد القيمة الفارغة لـ SMOKE_CASES | `7e4e215` + `342b4cb` |
| 2026-10-12 | W12: SMOKE_CASES partial-run filter | `b9af609` |

---

## قيد العمل

لا شيء. بانتظار قرار المالك على أسئلة الاستشارة السبعة (انظر HANDOFF.md).

---

## مؤجّل (كل واحدة جلسة منفصلة)

| البند | السبب | الأولوية |
|---|---|---|
| `smoke_rust_trait_impl` | بانتظار قرار القراءة المعتمدة | 1 |
| `smoke_ts_fastapi_client` | بانتظار إثبات/نفي فرضية التناقض | 1 |
| تلوث مجلد TS (تسجيلات Python دخيلة) | جلسة تنظيف مستقلة | 2 |
| عدّاد `repairs:0` خاطئ في تقرير smoke | مؤجل من Ledger | 3 |
| 3 فروع غير مدموجة + 5 stashes | تنظيف اختياري | 4 |
| CI لا يشغّل Smoke — هل يُضاف؟ | قرار تصميم | 4 |
| Node 18 (CI) vs Node 20 (محلي) | تباين صغير | 5 |
| README يذكر `SEL_API_KEY`، CONTRIBUTING يذكر `GROQ_API_KEY` | تباين توثيقي | 5 |

---

## خارج النطاق عمدًا

- تعديل أهداف Smoke (إلا بإثبات «خاطئ فعليًا» وفق معيار المالك).
- تعديل الدستور (`src/constitution.rs`).
- تعديل scaffold أو إعدادات Jest/TypeScript.
- إعادة تسجيل `smoke_go_worker_pool` (ناجحة في Replay).
- إصدار `v9.3.6` قبل `12/12` + `full` أخضر.

---

## دين تقني معروف

- 31 فرعًا محليًا (معظمها مدموج).
- 5 stashes (أقدمها 2026-07-08).
- `_quarantine/` متتبَّع (ملفات معزولة).
- `Cargo.toml` يشير لمستودع `chokribouzid/sel-agent` لكن الريموت الفعلي `chokriabouzid-star/sel-agent-engine`.
