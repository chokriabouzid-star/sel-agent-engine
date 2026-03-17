# SEL Agent — Context File
# يُقرأ في بداية كل جلسة جديدة

---

## الإصدار الحالي
- SEL Agent: v4.0
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
- Primary: moonshotai/kimi-k2-instruct (via Groq API)
- Fallback: llama-3.3-70b-versatile
- ENV: SEL_MODEL, SEL_VERSION, GROQ_API_KEY

---

## نتائج آخر Benchmark (v4.0)
- Stress: 24/24 passed | avg repairs: 0.9
- v3.2 → v4.0: avg repairs 1.0 → 0.9 (patch_file)
- kimi-k2:  mutation 83%, quality 0.83, repairs 1.9 ✅ PRIMARY
- llama-3.3: mutation 75%, quality 0.75, repairs 1.7

---

## بنية الكود الأساسية
src/
  main.rs      — CLI: Run, Health, Stress, Bench
  agent.rs     — Agent::run(), repair_count(), mutation_score()
  types.rs     — ExecutionContext (start_time, mutations_total, mutations_killed)
  executor.rs  — apply_all_mutations(), mutation_check(), ALLOWED programs
  llm.rs       — SYSTEM_PROMPT, model_name(), قواعد Python/Go/Node/Rust
  protocol.rs  — Cmd enum (+ PatchFile v4.0), validate_test_order()
  context.rs   — workspace management

---

## Bench System
- الأمر: sel-agent bench --suite [python|go|node|rust|all] --iterations N
- الحالات: 24 حالة (12 Python, 4 Go, 4 Node, 4 Rust)
- POST تلقائي إلى Observatory بعد كل bench
- ENV: SEL_VERSION لتسمية الجلسة

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

---

## FILE OPERATIONS RULES (v4.0)
- write_file:  فقط للملفات الجديدة
- patch_file:  دائماً لتعديل الملفات الموجودة (search/replace موضعي)
- append_file: فقط للإضافة في نهاية الملف
- Validation:  search block موجود مرة واحدة بالضبط أو error

---

## Observatory API
- GET  /api/projects    — قائمة المشاريع
- GET  /api/runs        — آخر 20 run
- GET  /api/run-stats   — إحصائيات عامة
- POST /api/run         — تسجيل run جديد
- GET  /api/bench       — bench history
- POST /api/bench       — تسجيل bench جديد

---

## Git Tags المهمة
- v1.5-final: heartbeat UI (indicatif)
- v1.7-final: stress 24/24, mutation 4 لغات
- v1.8-final: bench subcommand, quality index
- v1.9-final: bench POST to Observatory
- v2.0-final: iterations, prompt rules 6-8, model comparison
- v3.2-final: delete_file, language guard, recursive walker, 24/24
- v4.0: patch_file (search/replace), tool selection bias fixed

---

## القرارات المهمة (لا تتغير)
- Workspace ephemeral لكل حالة (tmpdir/sel-bench-N)
- Mutation check يعمل على: .py .go .js .ts .rs
- Observatory يُحذف db عند تغيير schema ثم يُعاد تشغيله
- fuser -k 8777/tcp قبل كل تشغيل للـ Observatory

---

## الخطوة التالية: v4.1
هدف: تحسين patch_file + TypeScript suite
  - auto-retry عند فشل patch (expanded context)
  - bench --suite typescript
  - avg repairs هدف: 0.7

---

## خارطة الطريق الكاملة
v3.0 → Live View (WebSocket) ✅
v3.1 → Language Guard + File Walker ✅
v3.2 → delete_file + stress 24/24 ✅
v4.0 → patch_file (surgical edits) ✅
v4.1 → TypeScript suite + auto-retry patch
v4.1 → Java suite
v4.2 → C suite
v5.0 → Regression Detection
v5.1 → Model Comparison Engine
v5.2 → Export & Research
v6.0 → Self-Improvement
v6.1 → Multi-Agent
v6.2 → Web Interface كامل

---

## ملاحظات تشغيلية
- Groq free tier: حد يومي للـ tokens — انتبه عند bench --iterations كبير
- Observatory يحتاج sleep 6 بعد التشغيل قبل أي curl
- DeepSeek free tier: غير متاح (402)
- bench --suite rust الأسرع للاختبار السريع

---
آخر تحديث: v4.0 — 2026-03-17
