# SEL Agent — وثيقة مرجعية رسمية
## الإصدار: v3.2-final
**التاريخ:** 2026-03-17
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
├── main.rs        — CLI: run / stress / bench / health
├── agent.rs       — State Machine + send_event (WebSocket)
│                    Language Detection + Recursive File Walker
├── protocol.rs    — JSON plan parser + Protocol Resilience
│                    Cmd: WriteFile, AppendFile, DeleteFile,
│                         RunTests, Run, Done
├── executor.rs    — تنفيذ الأوامر + Context Budget
│                    write_file (overwrite warning)
│                    delete_file (safe, protected files)
├── llm.rs         — Groq API client + System Prompt rules
│                    pub model, pub api_key, pub endpoint
├── types.rs       — ExecutionContext + FailedStep
└── observatory.rs — report_run() POST to Observatory
```

---

## 3. الميزات الكاملة (v1.1 → v3.2)

### v1.1 — v1.2
- Context Budget Engine (stderr +5, recent +3, test +2)
- Structured Repair Memory + culprit detection
- Token-Aware Repair (60-80% token reduction)
- Repair History Guard
- Goal Validator
- Smart Mutation (10 operators, ~90% coverage)

### v1.3
- Protocol Resilience: retry حتى مرتين عند فشل JSON
- Mutation Enforcement: اختبارات ضعيفة → repair cycle
- FlaskConcurrency classifier
- --dry-run flag

### v1.5 — v1.6
- SEL Observatory integration
- Health + Stress subcommands
- duration_secs حقيقي في Observatory

### v2.0
- System Prompt rules (3 قواعد إلزامية):
  1. Floats: ALWAYS pytest.approx — NEVER ==
  2. Branches: every if/else needs test for each branch
  3. Tests must try to BREAK the code
- --iterations N flag
- Mutation score tracking

### v3.0-final
- Live UI عبر WebSocket (/live)
- send_event(): start (أول run), step (لكل Cmd),
  repair (مع رقم), mutation (score حقيقي), done
- Model name من self.llm.model

### v3.1
- Language Detection في Planning:
  Cargo.toml → Rust only
  package.json → JS/TS only
  go.mod → Go only
- Recursive File Walker (عمق 3، يتجاهل target/.git/node_modules/venv)
- write_file overwrite warning (> 500 bytes → < ثلثه)

### v3.2-final
- delete_file command:
  - safe_path() protection
  - لا يحذف: Cargo.toml, go.mod, package.json, Cargo.lock
  - لا يحذف مجلدات
  - idempotent (ملف غير موجود → ok)
- FILE OPERATIONS rule في system prompt
- Stress test: default max_repairs = 5
- divide goal: اختبار كلا الفرعين إلزامي

---

## 4. إعدادات النموذج

**النموذج الافتراضي:** `moonshotai/kimi-k2-instruct`
**API:** Groq — `https://api.groq.com/openai/v1/chat/completions`
**env var:** `SEL_MODEL` (اختياري)

---

## 5. SEL Observatory v2.0-final

**المسار:** `~/sel-observatory/`

### Endpoints
```
GET  /              — Dashboard
GET  /live          — Live UI (WebSocket)
GET  /ws            — WebSocket endpoint
POST /api/event     — أحداث حية من SEL Agent
POST /api/runs      — تسجيل run مكتمل
GET  /api/runs      — قائمة الـ runs
GET  /api/run-stats — success_rate, avg_repairs, total
GET  /api/projects  — المشاريع المكتشفة
GET  /api/bench-history — سجل benchmark
```

### Live UI يعرض
- Goal (من بداية الـ run)
- Model name (kimi-k2-instruct)
- Mutation % (score حقيقي)
- Repairs count
- سجل أحداث مع timestamps
- Progress bar per step

---

## 6. Benchmark النهائي (v3.2-final)
```
Stress 24/24 | avg repairs: 1.0 | 100% success
Python  12/12 ✅  (broken import, wrong logic, mutation...)
Go       4/4  ✅  (add, fizzbuzz, reverse, divide)
Node     4/4  ✅  (add, palindrome, factorial, filter)
Rust     4/4  ✅  (add, fizzbuzz, reverse, stack)
```

### مقارنة عبر الإصدارات
```
v1.5: 12/12 | avg repairs: 0.9
v2.0: 13/13 | avg repairs: 0.85
v3.2: 24/24 | avg repairs: 1.0  ← suite تضاعفت
```

---

## 7. طريقة التشغيل
```bash
# run عادي
./target/release/sel-agent run \
  --workspace /tmp/test \
  --goal "..." \
  --max-repairs 3

# stress test (24 مهمة)
./target/release/sel-agent stress

# bench
./target/release/sel-agent bench --suite python

# health check
./target/release/sel-agent health

# dry-run
./target/release/sel-agent run --workspace /tmp/x --goal "..." --dry-run

# Observatory
cd ~/sel-observatory
fuser -k 8777/tcp 2>/dev/null; sleep 1
./target/release/sel_observatory &
# http://localhost:8777/live
```

---

## 8. أسلوب التطوير

كل تعديل عبر Python patch script:
```bash
cat > /tmp/patch.py << 'PYEOF'
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
python3 /tmp/patch.py && cargo build --release 2>&1 | grep -E "^error|Finished"
```

---

## 8b. v4.0 — patch_file (مطبّق)

**patch_file command:**
```json
{"type":"patch_file","path":"src/lib.rs",
 "old":"fn add(a,b)","new":"fn add(a: i32, b: i32) -> i32"}
```
- Validation: search block موجود مرة واحدة بالضبط
- System prompt: write_file للجديد فقط، patch_file للموجود دائماً
- Benchmark: 24/24 | avg repairs: 0.9 (كان 1.0)

## 9. النقائص المعروفة (مرشحة لـ v4.0)

- **Tool Selection Bias:** agent يستخدم write_file دائماً
  بدل append_file للإضافات الصغيرة → هدر tokens
- **patch_file مفقود:** لا يوجد diff-based editing بعد
- **avg repairs: 1.0:** كل مهمة Python تحتاج repair واحد
  بسبب No such file في الـ stress broken cases
- **Context explosion:** الملفات الكبيرة تُرسل كاملة في كل repair

---

## 10. الخطوة القادمة المقترحة (v4.0)

**patch_file command:**
بدل كتابة الملف كاملاً، الـ agent يُرسل:
```json
{"type":"patch_file","path":"src/lib.rs",
 "old":"fn add(a,b)","new":"fn add(a: i32, b: i32) -> i32"}
```
المتوقع: تخفيض tokens 60-80% وتقليل regression risk.

---

## 11. كيفية الاستخدام في محادثة جديدة

1. ارفع هذا الملف
2. قل: "أنا أعمل على SEL Agent، اقرأ الوثيقة"
3. أرسل مخرجات الأوامر المطلوبة
