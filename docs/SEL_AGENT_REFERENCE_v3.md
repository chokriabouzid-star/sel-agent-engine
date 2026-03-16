# SEL Agent — وثيقة مرجعية رسمية
## الإصدار: v3.0-final
**التاريخ:** 2026-03-16
**المطور:** Chokri Bouzid
**الحالة:** مستقر — جاهز للتطوير

---

## 1. نظرة عامة

SEL Agent هو وكيل برمجي مستقل (Autonomous Coding Agent) مكتوب بـ Rust.
يأخذ هدفاً بلغة طبيعية، يخطط، ينفذ، يختبر، ويصلح الأخطاء تلقائياً.

**المكونات:**
- `~/projects/sel-agent-v4/` — المحرك الرئيسي (Rust)
- `~/sel-observatory/` — لوحة المراقبة (Rust + WebSocket)

---

## 2. البنية المعمارية

### State Machine (5 حالات)
```
Planning → Executing → Testing → [Mutation Check] → Done
                ↓                        ↓
            Repairing ←─────────────────┘
                ↓
            Failed (بعد max-repairs)
```

### الملفات الرئيسية
```
src/
├── main.rs       — CLI: run / stress / bench / health
├── agent.rs      — State Machine + send_event (WebSocket)
├── protocol.rs   — JSON plan parser + Protocol Resilience
├── executor.rs   — تنفيذ الأوامر + Context Budget
├── llm.rs        — Groq API client (pub model, pub api_key)
├── types.rs      — ExecutionContext + FailedStep
└── observatory.rs — report_run() POST to Observatory
```

---

## 3. الميزات المبنية (حسب الإصدار)

### v1.1
- Context Budget Engine: يُرتّب الملفات بالصلة (stderr +5, recent +3, test +2)
- Structured Repair Memory: FailedStep يتتبع command + stderr + failure kind
- Mutation Check: advisory-only
- NodeTestError classifier

### v1.2
- Multi-file Repair Memory + culprit detection (+8 score)
- Token-Aware Repair: ImportError/NodeTestError → أسماء ملفات فقط (توفير 60-80% tokens)
- Repair History Guard: يمنع LLM من تكرار نفس الإصلاح
- Goal Validator: يرفض الأهداف الغامضة قبل Planning
- Smart Mutation: 10 operators، تغطية ~90%

### v1.3
- Protocol Resilience: retry حتى مرتين عند فشل JSON parsing
- Mutation Enforcement: الاختبارات الضعيفة تُطلق repair cycle
- FlaskConcurrency classifier: LookupError: flask.app_ctx
- --dry-run flag

### v1.5
- SEL Observatory integration: report_run() بعد كل SEL_SUCCESS/SEL_FAILED
- Health check subcommand
- Stress test 12/12

### v1.6 / v2.0
- llm.rs System Prompt rules (3 قواعد إلزامية):
  1. Floats: ALWAYS pytest.approx(x, rel=1e-6) — NEVER ==
  2. Branches: every if/else needs a test for each branch
  3. Assume your code is wrong. Tests must try to break it.
- Observatory metadata: duration_secs حقيقي
- --iterations N flag: مقارنة نماذج متعددة
- Mutation score tracking: quality_index

### v3.0-final (آخر إصدار)
- Live UI عبر WebSocket في /live
- send_event() في agent.rs يُرسل:
  - "start" → أول سطر في run() (قبل loop)
  - "step"  → لكل Cmd في Executing loop (مع label + progress X/N)
  - "repair" → مع رقم المحاولة
  - "mutation" → مع score حقيقي من mutation_score()
  - "done" → مع success + mutation_score()
- Model name من self.llm.model (pub field)
- LlmClient fields: pub model, pub api_key, pub endpoint

---

## 4. إعدادات النموذج

**النموذج الافتراضي:** `moonshotai/kimi-k2-instruct`
**API:** Groq — `https://api.groq.com/openai/v1/chat/completions`
**env var:** `SEL_MODEL` (اختياري، يُقرأ في llm.rs)

---

## 5. SEL Observatory

**المسار:** `~/sel-observatory/`
**الإصدار:** v2.0-final

### Endpoints
```
GET  /                  — Dashboard رئيسي
GET  /live              — Live UI (WebSocket)
GET  /ws                — WebSocket endpoint
POST /api/event         — استقبال أحداث حية من SEL Agent
POST /api/runs          — تسجيل run مكتمل
GET  /api/runs          — قائمة كل الـ runs
GET  /api/run-stats     — إحصائيات (success_rate, avg_repairs, total)
GET  /api/projects      — قائمة المشاريع المكتشفة
GET  /api/bench-history — سجل benchmark التاريخي
```

### Live UI يعرض
- Goal (من بداية الـ run)
- Model name
- Mutation %
- Repairs count
- سجل أحداث مع timestamps
- Progress bar

---

## 6. نتائج Benchmarks (آخر قياس — v1.5/v2.0)
```
Stress (12 tasks):    12/12 | avg repairs: 0.9
Hard (8 tasks):        8/8  | avg repairs: 0.5
Observatory tracking: 13 runs | success_rate: 84.6% | avg_repairs: 0.85
```

### حالات الفشل المعروفة
1. **Float comparison** — `assert add(0.1,0.2) == 0.3` → حُلّت في v2.0 بـ pytest.approx
2. **Mutation gap** — divide(a,b): اختبار فرع واحد فقط → حُلّت في v2.0 بقاعدة branches
3. **model: unknown** → حُلّت في v3.0 بـ pub model field
4. **duration_secs: 0** → حُلّت في v1.6 بـ Instant::now()

---

## 7. طريقة التشغيل
```bash
# run عادي
./target/release/sel-agent run \
  --workspace /tmp/test \
  --goal "..." \
  --max-repairs 3

# stress test
./target/release/sel-agent stress --max-repairs 3

# bench
./target/release/sel-agent bench --suite python

# health check
./target/release/sel-agent health

# dry-run (معاينة الخطة فقط)
./target/release/sel-agent run --workspace /tmp/x --goal "..." --dry-run

# تشغيل Observatory
cd ~/sel-observatory
fuser -k 8777/tcp 2>/dev/null; sleep 1
./target/release/sel_observatory &
# افتح: http://localhost:8777/live
```

---

## 8. أسلوب التطوير

**القاعدة الأساسية:** كل تعديل عبر Python patch script بصيغة:
```bash
cat > /tmp/patch_name.py << 'PYEOF'
from pathlib import Path
f = Path.home() / "projects/sel-agent-v4/src/file.rs"
src = f.read_text()
old = '...'
new = '...'
if old in src:
    src = src.replace(old, new, 1)
    f.write_text(src)
    print("✅ done")
else:
    print("❌ not found")
PYEOF
python3 /tmp/patch_name.py
cargo build --release 2>&1 | grep -E "^error|Finished"
```

**السبب:** المستخدم يُفضّل نسخ الأوامر مباشرة دون تعديل يدوي.

---

## 9. النقائص المعروفة (مرشحة لـ v3.1+)

- Step يظهر في سجل الأحداث لكن progress bar لا يتحدث تدريجياً (يقفز للنهاية)
- Stress test يُشغّل الاختبارات بشكل متسلسل (لا parallel)
- لا يوجد سجل تاريخي للـ mutation scores عبر الإصدارات
- Observatory: model: unknown في runs القديمة

---

## 10. كيفية استخدام هذه الوثيقة

في أي محادثة جديدة مع أي نموذج:
1. ارفع هذا الملف
2. قل: "أنا أعمل على SEL Agent، اقرأ الوثيقة المرفقة"
3. أرسل مخرجات الأوامر المطلوبة
4. النموذج سيفهم السياق الكامل فوراً


---

## 11. تحديث v3.1 (2026-03-16)

### مشاكل اكتُشفت من الاختبار الميداني
1. **انتكاسة اللغة** — agent يكتب Python في مشروع Rust
2. **استبدال الملفات** — write_file يمسح كود موجود عند الإضافة
3. **تحذير صامت** — write_file يحذر لكن لا يوقف التنفيذ

### الإصلاحات المطبقة

**executor.rs — write_file overwrite warning:**
إذا كان الملف موجوداً > 500 bytes والمحتوى الجديد < ثلثه → يطبع تحذيراً.

**agent.rs — Language Detection في Planning:**
يفحص workspace قبل بناء الـ prompt:
- Cargo.toml موجود → CRITICAL: Write ONLY Rust code
- package.json موجود → CRITICAL: Write ONLY JS/TS code
- go.mod موجود → CRITICAL: Write ONLY Go code

**agent.rs — Existing Files Context:**
يقرأ كل ملفات .rs/.py/.js/.ts/.go الموجودة في workspace/src
ويضيفها للـ prompt مع:
"CRITICAL — EXISTING FILES (you MUST preserve ALL existing code, only ADD new code)"
الحد الأقصى: 3000 chars per file.

### نتيجة الاختبار
- LlmClient محفوظ ✅
- subtract مضافة ✅
- 60 سطر بعد 28 سطر ✅

---

## 11. تحديث v3.1 (2026-03-16)

### مشاكل اكتُشفت من الاختبار الميداني
1. انتكاسة اللغة — agent يكتب Python في مشروع Rust
2. استبدال الملفات — write_file يمسح كود موجود عند الإضافة
3. recursive walker — الكود القديم لا يقرأ المجلدات الفرعية

### الإصلاحات المطبقة

**executor.rs — write_file overwrite warning:**
إذا كان الملف > 500 bytes والمحتوى الجديد < ثلثه → يطبع تحذيراً.

**agent.rs — Language Detection في Planning:**
يفحص workspace قبل بناء الـ prompt:
- Cargo.toml → CRITICAL: Write ONLY Rust code
- package.json → CRITICAL: Write ONLY JS/TS code
- go.mod → CRITICAL: Write ONLY Go code

**agent.rs — Recursive File Walker:**
يقرأ كل ملفات .rs/.py/.js/.ts/.go بشكل recursive حتى عمق 3
يتجاهل: target/ .git/ node_modules/ venv/
يضيفها للـ prompt مع:
"CRITICAL — EXISTING FILES (you MUST preserve ALL existing code and APPEND only)"
الحد الأقصى: 2500 chars per file.

### نتيجة الاختبار النهائي
- LlmClient محفوظ في lib.rs (2695 bytes) ✅
- subtract مضافة ✅
- 12 passed (قديم + جديد) ✅
- 0 repairs ✅
- Mutation 100% ✅
