# سجل استقرار المشروع — Stability Ledger

**آخر تحديث:** 2026-07-20 (إغلاق القضية #1 — انحراف لغوي صامت)

---

## الحالة المؤكَّدة — من تشغيلة حية كاملة (LIVE) بتاريخ 2026-07-17 + إغلاق جلسة 2026-07-20

| البوابة | الوضع | النتيجة |
|---|---|---|
| cargo test | ✅ | 494 pass, 0 failed |
| cargo clippy -D warnings | ✅ | 0 warnings |
| bench all replay | ✅ | 36/36 |
| bench-swe replay | ✅ | 30/30 |
| bench-sel-v11 replay | ✅ | 18/18 |
| regression_gate core | ✅ | PASS |
| smoke (replay) | ✅ | 12/12 |
| smoke_ts_fastapi_client | ✅ | TypeScript صحيح، 68s، repairs:0 |

---

## القضايا المفتوحة — بترتيب الأولوية

### 1) ✅ مُغلَقة — انحراف لغوي صامت — `smoke_ts_fastapi_client`

**مُغلَقة في:** 2026-07-20 — كوميت `f9be265` على فرع `fix/stage-3-5-lang-mismatch`

**الخلل كان:** هدف يذكر `fastapi` يُصنَّف دائماً كـ Python حتى لو طلب صراحة عميل TypeScript.
**الإصلاح:** فصل `fastapi_signals` في متغير مستقل مشروط بـ `!ts_signals` في `detect_kind()`.
**الدليل:** `smoke_ts_fastapi_client` ينتج TypeScript صحيحاً، interaction_count 4→2 (لا LANGUAGE LOCK BLOCKED).
**التغطية:** `test_typescript_client_for_fastapi_backend_detected_as_typescript` (jest.mocked/api.test.ts/getUser).

---

### 2) 🟡 عدّاد `repairs:0` خاطئ في تقرير smoke test (مؤجَّل)

السبب الجذري غير مؤكَّد — يحتاج قراءة `sel_smoke_test.sh` كاملاً. جلسة منفصلة.

---

### 3) بنود مؤجَّلة (كل واحدة جلسة منفصلة)

- D1-5: دمج `bench_sel.rs` — WIP في `~/sel_recovery_2026_07_08/`، غير مدموج.
- `agent.rs` طبقة دفاع ثانية — patch محفوظ في `/tmp/agent_rs_language_guard_FUTURE.patch`، يحتاج أمر عمل مستقل + regression_gate full كاملة.
- `language` في `fixtures/trajectories/index.json` دائماً `"trajectories"` بدل اللغة الفعلية.
- H4: تعارض curl/constitution.
- H6: تكرار `is_test_like_command`.
- H7: `estimate_tokens` قياسات متضاربة.
- توحيد رقم الإصدار عبر الأدوات.
- Scaffold صريح لـ Go/Rust.

---

### مرفوض صراحة — لا يُنفَّذ بلا بيانات استخدام حقيقية

إعادة بناء معماري ضخم (Event Sourcing، Plugin Trait عام، AST Mutation Engine، Chaos Testing، Sandbox جديد، Plugin SDK). رُفض من المراجعة المعمارية.

---

## التغييرات الأخيرة

**2026-07-20:** إغلاق القضية #1 — إصلاح `detect_kind()` في `goal_parser.rs` + اختبار `test_typescript_client_for_fastapi_backend_detected_as_typescript`. كوميت `f9be265`.
**2026-07-17:** تشغيلة حية كاملة (494 اختباراً + 5 بوابات) — اكتشاف القضية #1.
**2026-07-10:** بوابة كاملة خضراء. تاغ `v9.3.5-green-2026-07-10`.
**2026-07-09:** إصلاح replay root cause (`run→run_tests`). ملفات: `src/protocol.rs`, `src/executor/core.rs`.

---

## WIP المحفوظ

- `~/sel_recovery_2026_07_08/` — ملفات D1-5 التجريبية، لا تُدمج حتى اكتمال المراجعة.
- `/tmp/agent_rs_language_guard_FUTURE.patch` — طبقة دفاع ثانية في `agent.rs`، مؤجَّلة.

---

## نتائج ميدانية — تشغيلة حية 10 مهام (2026-07-20)

**الهدف:** اختبار واقعي لمهام صعبة (تزامن، أمان خيوط، race conditions) خارج البانش الرسمي.
**النتيجة الإجمالية:** 6 نجاح / 4 فشل (60%) — Go: 0/3، Python: 2/3، Rust: 2/2، TypeScript: 2/2.

### قوى مؤكَّدة بدليل مباشر

| القوة | الدليل |
|---|---|
| شبكة أمان التخطيط تصمد تحت تعقيد قصوى | `workerpool`: 14 أمر → 8 تكرارات → إعادة تخطيط × 3 → مخالفة دستورية → نجاح |
| الدستور يصمد أمام نموذج متهوّر | `ratelimit`: محاولة تعديل ملف اختبار محمي → `CONSTITUTION_VIOLATION` فوري |
| idempotency فعلي في حلقة الإصلاح | `retry-client`: `⏭ Skipping: already passed` |
| EXPLAIN MODE — اكتُشف اليوم لأول مرة | عند الاستعصاء التام، الوكيل يتوقف ويطلب تلميحًا بشريًا بدل الفشل الصامت |
| Fallback أنقذ مهمة فعليًا | `safequeue`: Groq+Gemini+OpenRouter فشلوا → GitHub/gpt-4o نجح |
| Timeout يمنع التعليق الأبدي | `workerpool`: goroutine deadlock → `go test timeout` بدل تعليق لا نهائي |

### ضعف جديد مؤكَّد 🔴 — الأعلى أولوية

**فشل كامل لسلسلة المزوّدين الخمسة في مهمة حقيقية:**
`ratelimit` فشلت نهائيًا بـ`401 Unauthorized` بعد تعثّر Groq+Gemini+Cerebras+OpenRouter+GitHub
كلهم بالتتابع. لم يعد فجوة نظرية — **فشل إنتاجي فعلي مُوثَّق.**

### ضعف مُعزَّز بدليل جديد 🟡

| الضعف | الدليل الجديد |
|---|---|
| حلقات "إصلاح بلا فهم" (no-op repairs) | `timed_lru`: 3 محاولات × ~1428 bytes متطابقة، نفس الخطأ (`0 != 3`) بلا تشخيص سبب جذري |
| go_compile_check يتوه في المجلدات الفرعية | `ratelimit`: `"no Go files in /tmp/task-manager01"` رغم وجودها في `ratelimit/` |
| إصلاح يُدخل عطلاً جديدًا غير مرتبط | `shardedmap`: أضاف `main.go` (حزمة `main`) في مجلد حزمة `shardedmap` → تعارض تجميع |
| Gemini معطّل فعليًا | 6/6 محاولات → HTTP 400 — يستهلك وقتًا + 3 retries بلا فائدة |

### ضعف جديد دقيق 🟡

**تصنيف "Equivalent Mutant" متساهل:** حدث مرتين (`>=`/`<=` في safe_cache، `||`/`&&` في retry-client).
كلاهما عوامل مقارنة نادرًا متكافئة — الأرجح أن الاختبارات لا تصل للحالة الحدّية، لا أن الطفرة متكافئة فعلاً.

### حد قدرة نموذج — ليس عيبًا هندسيًا

فشلا `workerpool` (إغلاق آمن + context cancellation) و`shardedmap` (race condition معقد)
من أصعب أنماط التزامن حتى لمهندسين بشريين. حد معروف للنموذج الحالي، لا أولوية إصلاح.

### المرشَّحان الأقوى للجلسة القادمة (بترتيب الأثر)

1. **فشل سلسلة المزوّدين الكامل** — موثَّق مرتين، فشل إنتاجي فعلي
2. **حلقات الإصلاح بلا فهم** — موثَّق مرتين (`smoke_py_binary_search` + `timed_lru`)
