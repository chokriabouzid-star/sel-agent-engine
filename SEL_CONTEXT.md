# SEL Agent — Context File
# يُقرأ في بداية كل جلسة جديدة

---

## الإصدار الحالي
- SEL Agent: v4.1-final
- SEL Observatory: v1.3-final

---

## المسارات الأساسية
- Agent: ~/projects/sel-agent-v4/
- Binary: ~/projects/sel-agent-v4/target/release/sel-agent
- Observatory: ~/sel-observatory/
- Observatory Binary: ~/sel-observatory/target/release/sel_observatory
- Observatory DB: ~/sel-observatory/sel_observatory.db
- Observatory Port: 8777

---

## النموذج الأساسي
- Primary: moonshotai/kimi-k2-instruct-0905 (via Groq API)
- Fallback: llama-3.3-70b-versatile
- ENV: SEL_MODEL, SEL_VERSION, GROQ_API_KEY

---

## نتائج آخر Benchmark (v4.1-final)
- Stress: 24/24 passed | avg repairs: 0.8
- TypeScript: 4/4 | avg repairs: 0.2 | mutation: 100%
- v4.0 → v4.1: avg repairs 0.9 → 0.8

---

## بنية الكود الأساسية
src/
  main.rs      — CLI: Run, Health, Stress, Bench
  agent.rs     — Agent::run(), repair_count(), mutation_score()
                 Goal Validator v1.2, Language Detection, Recursive Walker (max 20 files)
  types.rs     — ExecutionContext, FailedStep, FailureKind
  executor.rs  — patch_file(), mutation_check(), ALLOWED programs
                 AutoFix: pytest install, jest conflict, go mod tidy, sha2::Digest
  llm.rs       — SYSTEM_PROMPT, model_name()
  protocol.rs  — Cmd enum (WriteFile, AppendFile, PatchFile, DeleteFile, ReadFile, Run, RunTests, Done)
  context.rs   — Context Budget Engine (MAX_REPAIR_TOKENS=8000)

---

## Suites المتاحة
- sel-agent bench --suite [python|go|node|rust|typescript|all]
- الحالات: 28 حالة (12 Python, 4 Go, 4 Node, 4 Rust, 4 TypeScript)
- Stress test: 24 حالة (بدون TypeScript)

---

## FILE OPERATIONS RULES (v4.0)
- write_file:  فقط للملفات الجديدة
- patch_file:  دائماً لتعديل الملفات الموجودة (search/replace موضعي)
- append_file: فقط للإضافة في نهاية الملف
- Validation:  search block موجود مرة واحدة بالضبط أو error
- whitespace normalization: تلقائي عند فشل البحث

---

## TypeScript Rules (v4.1)
- Install: npm install typescript ts-jest @types/jest jest
- jest.config.js: module.exports = { preset: "ts-jest", testEnvironment: "node" }
- tsconfig.json: strict: false, module: commonjs
- AutoFix: إزالة jest field من package.json عند وجود jest.config.js
- Mutation selector: يتجاهل jest.config.js و tsconfig.json

---

## قواعد SYSTEM_PROMPT الحالية (llm.rs)
Python (8 قواعد):
  1. pytest.approx للأرقام العشرية
  2. كل if/else له test
  3. الاختبارات تحاول كسر الكود
  4. None/null — اختبر الحالتين
  5. conditional returns — اختبر الفرعين
  6. minimum 6 tests per file
  7. edge cases: empty, zero, negative, boundary
  8. لا تختبر happy path فقط
Repair rules: patch_file إلزامي للملفات الموجودة، write_file محظور في الـ repair

---

## Git Tags المهمة
- v3.2-final: delete_file, language guard, recursive walker, 24/24
- v4.0: patch_file (search/replace), tool selection bias fixed
- v4.0-final: skip guard fixed, whitespace normalization, repair prompt
- v4.1: TypeScript suite 4/4
- v4.1-final: kimi-k2-0905 default, jest autofix, avg repairs 0.8

---

## الحدود المعروفة (مرشحة لـ v5.0)
- Goal Validator يرفض أهداف المشاريع الحقيقية (لا قيم محددة)
- Recursive Walker محدود بـ 20 ملف — لا يكفي للمشاريع الكبيرة
- patch_file يفشل مع ملفات كبيرة (search block غير فريد)
- Groq free tier: TPM limit 10,000 — مشاريع كبيرة تسبب 413

---

## الخطوة التالية: v5.0
هدف: دعم المشاريع الحقيقية
  1. Goal Validator مرن — يقبل أهداف "fix failing tests"
  2. Context Chunking — إرسال أجزاء من الملفات الكبيرة فقط
  3. patch_file مع line number hint — لتجنب ambiguity
  4. --focus flag — تحديد الملفات المستهدفة يدوياً

---

## خارطة الطريق الكاملة
v3.0 → Live View (WebSocket) ✅
v3.1 → Language Guard + File Walker ✅
v3.2 → delete_file + stress 24/24 ✅
v4.0 → patch_file (surgical edits) ✅
v4.1 → TypeScript suite + kimi-k2-0905 ✅
v5.0 → Real Project Support (Goal Validator + Context Chunking)
v5.1 → Java suite
v5.2 → Model Comparison Engine
v6.0 → Self-Improvement
v6.1 → Multi-Agent

---

## ملاحظات تشغيلية
- Groq free tier: حد يومي للـ tokens
- bench --suite rust الأسرع للاختبار السريع
- SEL_MODEL env var لتغيير النموذج مؤقتاً
- Observatory: fuser -k 8777/tcp قبل التشغيل

---
آخر تحديث: v4.1-final — 2026-03-18

---

## تجربة حقيقية: القسطاس — violations crate (2026-03-18)

### المهمة
تنفيذ violations crate الفارغ بناءً على اختبارات موجودة.

### ما حدث
- SEL كتب الكود الأساسي ✅
- SEL كتب أنواع خاطئة ([u8;4] بدل [u64;4]) ❌
- SEL دمّر Cargo.toml (558 → 167 bytes) ❌
- SEL قبل 0 passed كنجاح ❌
- التدخل اليدوي أصلح كل شيء ✅
- النتيجة النهائية: 59 اختبار، صفر فشل ✅

### الدروس
1. Goal غامض → SEL يخمّن الأنواع ويخطئ
   الحل: حدد الأنواع بدقة في الـ goal أو أعطه ملف مرجعي
2. SEL يعتبر 0 passed نجاحاً — خطأ خطير
   الحل v5.0: فرض minimum_tests > 0 في Goal Validator
3. write_file على Cargo.toml موجود = خطر
   الحل v5.0: Cargo.toml محمي افتراضياً مثل constitution
4. المهمة المثالية لـ SEL: ملفات محددة + مواصفة دقيقة + اختبارات مرجعية موجودة

### قاعدة جديدة للـ v5.0
- PROTECTED_FILES: Cargo.toml, Cargo.lock, constitution/*
- GOAL_VALIDATOR: يرفض النجاح إذا passed == 0
- GOAL_FORMAT المثالي:
  "--workspace <crate> --goal <task> --ref-file <archive/original>"

---

## إصلاحات v5.0-dev (2026-03-18)

### الإصلاح 1: منع 0 passed من أن يُعتبر نجاحاً
- الملف: src/executor.rs
- المشكلة: success = exit_ok فقط، بغض النظر عن عدد الاختبارات
- الحل: success = exit_ok && passed > 0
- يطبق على: Rust, Go, Jest

### الإصلاح 2: parse_rust_tests يجمع كل النتائج
- المشكلة: كانت تأخذ أول test result فقط (غالباً 0 passed)
- الحل: تجمع كل test result lines وتجمع الأعداد
- النتيجة: 0+2+0 = 2 passed بدل 0

---
## إصلاحات v5.0-final (2026-03-19)
- write_file: Cargo.toml + Cargo.lock + go.mod + go.sum محمية من الكتابة العشوائية
- Goal Validator: يقبل أهداف المشاريع الحقيقية (fix, implement, refactor, existing...)
- parse_rust_tests: يجمع كل النتائج (0+2+0 = 2 بدل 0)
- 0 passed = فشل في Rust + Go + Jest
- Bench: 28/28 | Mutation 94% | Quality 0.94

## الخطوة التالية: Observatory UI v2.0
- رسم بياني للـ Quality Index عبر الإصدارات
- Bench Dashboard في الواجهة
- مقارنة النماذج بصرياً
آخر تحديث: v5.0-final — 2026-03-19

## مشكلة معلقة — click/core.py (ID 538-541)
- Goal: Fix bug in src/click/core.py line 682
- فشل بعد 4 repairs
- السبب: SEL لم يتعرف على السياق الكامل للمشروع
- الحل المقترح في v5.1: --ref-file + context chunking

---
## الخطوة التالية: v5.1
هدف: دعم المشاريع الكبيرة والمعقدة

### المشاكل المستهدفة
1. SEL لا يرى السياق الكافي للمشاريع الكبيرة (20 ملف محدود)
2. patch_file يفشل عندما لا يجد search block بدقة
3. click/core.py فشل بعد 4 repairs بسبب نقص السياق

### الحلول المخططة
- --ref-file flag: إعطاء SEL ملف مرجعي يحتوي الأنواع والتوقيعات
- Context Chunking: إرسال الجزء الصحيح من الملف فقط
- --focus flag: تحديد الملفات المستهدفة يدوياً
- Recursive Walker: رفع الحد من 20 إلى 50 ملف

### مثال الاستخدام المستهدف
sel-agent run \
  --workspace ~/al-qistas \
  --goal "Fix violations crate — implement all types" \
  --ref-file ~/al-qistas/archive/violations_original.rs \
  --focus src/violations/ \
  --max-repairs 5

آخر تحديث: v5.0-final — 2026-03-19

---
## v5.1 — تم التنفيذ ✅

### الميزات المضافة
1. **--ref-file flag** - ملف مرجعي للأنواع والتوقيعات يُضاف للـ repair context
2. **--focus flag** - مسارات محددة تحصل على +10 score في Context Budget
3. **MAX_CONTEXT_FILES: 50** - رفع الحد من 20 إلى 50 ملف
4. **focus_paths scoring** - منطق ذكي يعطي أولوية للملفات المستهدفة
5. **ref_file في repair prompt** - يتم إضافة محتوى الـ ref file تلقائياً في الـ repair context

### مثال الاستخدام
```bash
sel-agent run \
  --workspace ~/my-project \
  --goal "Fix the authentication bug" \
  --ref-file ~/my-project/docs/auth_types.rs \
  --focus src/auth/,src/middleware/ \
  --max-repairs 5
```

### التغييرات التقنية
- **types.rs**: إضافة `ContextConfig` struct
- **main.rs**: CLI flags جديدة + تمريرها للـ Agent
- **agent.rs**: تخزين `context_config` + استخدامه في repair
- **context.rs**: 
  - رفع `MAX_CONTEXT_FILES` من 20→50
  - إضافة `focus_paths` scoring (+10)
  - دالة `read_ref_file()` لقراءة الملف المرجعي

### Benchmark القادم
بعد اختبار v5.1 على مشروع al-qistas الحقيقي

تاريخ الإصدار: 2026-03-19

### اختبار v5.1 الحقيقي ✅

**المشروع:** test-v51-project (Rust multi-file)
**الخطأ:** User struct missing email field
**الإصلاح:** 
- استخدم `--ref-file` لرؤية types_reference.rs
- استخدم `--focus src/models/,src/auth/` للتركيز
- أصلح في **0 repairs** (نجح من المحاولة الأولى)
- استخدم `patch_file` 3 مرات (لم يعد write_file)

**النتيجة:** ✅ SUCCESS - البناء نجح والاختبارات تعمل

تاريخ الاختبار: 2026-03-19

### اختبار v5.1 الحقيقي ✅

**المشروع:** test-v51-project (Rust multi-file)
**الخطأ:** User struct missing email field
**الإصلاح:** 
- استخدم `--ref-file` لرؤية types_reference.rs
- استخدم `--focus src/models/,src/auth/` للتركيز
- أصلح في **0 repairs** (نجح من المحاولة الأولى)
- استخدم `patch_file` 3 مرات (لم يعد write_file)

**النتيجة:** ✅ SUCCESS - البناء نجح والاختبارات تعمل

تاريخ الاختبار: 2026-03-19

### اختبار v5.1 الحقيقي ✅

**المشروع:** test-v51-project (Rust multi-file)
**الخطأ:** User struct missing email field
**الإصلاح:** 
- استخدم `--ref-file` لرؤية types_reference.rs
- استخدم `--focus src/models/,src/auth/` للتركيز
- أصلح في **0 repairs** (نجح من المحاولة الأولى)
- استخدم `patch_file` 3 مرات (لم يعد write_file)

**النتيجة:** ✅ SUCCESS - البناء نجح والاختبارات تعمل

تاريخ الاختبار: 2026-03-19

### اختبار v5.1 الحقيقي ✅

**المشروع:** test-v51-project (Rust multi-file)
**الخطأ:** User struct missing email field
**الإصلاح:** 
- استخدم `--ref-file` لرؤية types_reference.rs
- استخدم `--focus src/models/,src/auth/` للتركيز
- أصلح في **0 repairs** (نجح من المحاولة الأولى)
- استخدم `patch_file` 3 مرات (لم يعد write_file)

**النتيجة:** ✅ SUCCESS - البناء نجح والاختبارات تعمل

تاريخ الاختبار: 2026-03-19

### اختبار v5.1 على القسطاس ✅

**المشروع:** al-qistas-system/crates/violations
**المهمة:** إضافة `get_violation_severity()` method
**الميزات المستخدمة:**
- `--ref-file src/snapshot.rs`
- `--focus src/`
- `--max-repairs 3`

**النتيجة:**
- ✅ **0 repairs** - نجح من المحاولة الأولى
- ✅ **6 tests passed** - اختبارات شاملة لكل الحالات
- ✅ `patch_file` × 2 - src/snapshot.rs + tests/violations_test.rs
- ✅ منطق شرطي معقد (1-2→low, 3-4→medium, 5-6→high)

تاريخ: 2026-03-19

---
## v5.1 - الملخص النهائي

### الاختبارات
| الاختبار | النتيجة | التفاصيل |
|---------|---------|----------|
| test-v51-project | ✅ SUCCESS | 0 repairs, 1 test passed |
| al-qistas severity | ✅ SUCCESS | 0 repairs, 6 tests passed |
| al-qistas idempotency | ⚠️ PARTIAL | أضاف tests لكن أفسد سطر |

### Bug مكتشف
- **patch_file validation** يحتاج تحسين
- السطر المُفسد: `}ype = 7;` بدل `snapshot.violation_type = 7;`
- للإصلاح في v5.2 أو v5.1.1

### الإحصائيات النهائية
- **Success Rate**: 66% (2/3 اختبارات كاملة)
- **Avg Repairs**: 0.0 (للاختبارات الناجحة)
- **Quality**: v5.1 مستقر مع نقطة تحسين واحدة

تاريخ: 2026-03-19

---
## v5.2 — تم التنفيذ ✅

### الإصلاحات المُنفذة:
1. **patch_file → write_file fallback** (بعد فشلين)
2. **patch_file validation** (}ype, #\[, braces, line count)

### الاختبار على al-qistas:
**السيناريو:** نفس الاختبار الذي فشل في v5.1
```
v5.1: ❌ FAILED (أفسد الكود + 3 repairs ضائعة)
v5.2: ✅ SUCCESS (9 tests passed, 0 repairs ضائعة)
```

### النتيجة:
- **Repair efficiency:** +100% (0 بدل 3)
- **Code safety:** 100% (لم يُفسد أي كود)
- **Success rate:** 100% (1/1)

تاريخ: 2026-03-19
