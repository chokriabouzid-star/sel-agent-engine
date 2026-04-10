# SEL Agent — Context File v7.0
# يُقرأ في بداية كل جلسة جديدة
# آخر تحديث: بعد إنجاز v7.0

---

## الإصدار الحالي
```
SEL Agent:      v7.0
Cargo.toml:     6.4.0
Observatory:    v3 (port 8777) ✅
الحالة:         مستقر — جاهز لـ v7.1
```

---

## المسارات الأساسية
```
Project:     ~/projects/active/sel-agent-v4/
Binary:      ~/projects/active/sel-agent-v4/target/release/sel-agent
Context:     ~/projects/active/sel-agent-v4/SEL_CONTEXT.md
Observatory: ~/sel-observatory/target/release/sel_observatory
Vision:      ~/projects/active/sel-agent-v4/VISION.md
Training:    ~/.sel-agent/training/sessions.jsonl  (v10.0+)
```

---

## النماذج المدعومة
```
المزود       المتغير            النموذج           الحد اليومي
──────────────────────────────────────────────────────────────
Gemini       GEMINI_API_KEY     gemini-2.0-flash  1,000,000 token (مجاني)
Groq/Kimi    GROQ_API_KEY       kimi-k2-instruct  300,000 token
Groq/Llama   GROQ_API_KEY       llama-3.3-70b     500,000 token
OpenRouter   SEL_API_KEY +      أي نموذج          مدفوع — بلا حد
             SEL_API_BASE
Ollama       (محلي)             sel-coder (v10+)  بلا حد
```

### تشغيل Gemini (الأسرع حالياً)
```bash
export GEMINI_API_KEY="AIza..."
export SEL_MODEL="gemini"
unset SEL_API_BASE
unset SEL_API_KEY
```

---

## نتائج البنش — التاريخ الكامل
```
الإصدار       Passed   Repairs   Mutation   ملاحظة
──────────────────────────────────────────────────────────────────
v6.7          32/32    0.0       100%       baseline أصلي
v6.8          32/32    0.0       100%       integration +3
v6.9 أصلي    32/32    0.0       100%       ذروة الأداء
v6.9→8.3     تالف     —         —          رحلة التجارب الضائعة
v6.9 مُعاد   32/32    0.1       80%        بعد الاسترجاع
v7.0 ✅       32/32    0.1       100%       مستقر — go fizzbuzz يحتاج repair أحياناً
```

### ملاحظة على 0.1 repairs
```
السبب: go fizzbuzz أحياناً ينسى import "fmt"
الأثر: repair واحد تلقائي — ينجح دائماً
الحل: في v7.1 مع تحسين System Prompt
```

### قاعدة Regression Guard (إلزامية)
```
قبل كل git commit:
  bench نتيجة >= نتيجة آخر commit
  إذا تراجعت → لا commit حتى تُعرف السبب
```

---

## هيكل الملفات
```
src/
├── main.rs              — CLI + bench + Plan runner + Provider display
├── agent.rs             — State Machine + Auto-Context Injection
├── executor.rs          — run/write_file/run_tests/patch_file
├── scaffold_engine.rs   — يجهز البيئة + GoalParser
├── goal_parser.rs       — ParsedGoal { kind, sub_kind, extra_deps }
├── types.rs             — FailureKind + RepairBudget + InfraError
├── constraint_engine.rs — طبقة قيود
├── scanner.rs           — has_ts_files() / has_py_files()
├── protocol.rs          — enum Cmd { Run, WriteFile, RunTests, PatchFile }
├── environment.rs       — EnvironmentCapabilities::probe()
├── llm.rs               — Multi-provider client + error classification
├── context.rs           — ContextConfig + ref_file handling
├── evaluator.rs         — Mutation testing
└── memory.rs            — Session memory

مخطط للإضافة:
├── manifest.rs          — Project Manifest (v7.2)
├── snapshot.rs          — FileSnapshot + Diff (v7.4)
├── ledger.rs            — Context Ledger (v7.4)
└── repair_strategy.rs   — Adaptive Repair (v8.1)
```

---

## إنجازات كل إصدار

### v6.3 — ScaffoldEngine
- يجهز البيئة قبل LLM
- Pinned stacks: typescript@5.3.3 ts-jest@29.1.1 jest@29.7.0

### v6.4 — RepairBudget + InfraError
- FailureKind::InfraError — يميز الشبكة عن الكود
- Retry: 3 محاولات بـ backoff (15s/45s/120s)

### v6.5 — GoalParser
- ParsedGoal { kind, sub_kind, extra_deps }
- scaffold_python يثبت extra_deps تلقائياً

### v6.6 — PatchError + TS Runner + Auto-Context
- executor.rs: .ts → npm test
- build_workspace_context(): يقرأ workspace قبل Planning

### v6.7 — Bench 28→32
- +flask hello, +fastapi route, +express api, +ts express
- 32/32 | 0.0 | 100%

### v6.8 — Integration Bench 4→7
- +flask add route, +fastapi endpoint, +marketing bot
- Auto-Context Injection مثبت على TypeScript OOP

### v6.9 — Observatory v3 + Markdown Plans
- Observatory v3: State Machine visual (port 8777)
- run_plan(): يقرأ "- [ ] goal" من TODO.md
- Marketing Bot POC: 11 أمر، nested folders ✅

### v7.0 — Provider Info + Error Messages ✅ (الحالي)
- Provider/Model/Key/Limit/Reset يظهر عند كل run وbench
- رسائل أخطاء واضحة:
  - `⚠️ [النموذج] رد بنص بدل JSON — إعادة بـ prompt مبسط`
  - `❌ [Gemini] نفد الحد اليومي — يتجدد بعد Xh Xm`
  - `⚠️ [الشبكة] انقطع الاتصال`
  - `❌ [OpenRouter] رصيد منتهٍ`
- System Prompt: 17,292 → 4,157 حرف (توفير 76%)
- Go/Rust/TS run_tests targets صحيحة
- Gemini مدعوم رسمياً

---

## FailureKind::max_attempts()
```
PatchError       => 2
InfraError       => 0   retry فقط
ImportError      => 1
NodeTestError    => 1
DatabaseError    => 2
FlaskConcurrency => 2
CollectionError  => 2
SyntaxError      => 3
TypeError        => 3
AssertionError   => 3
BuildError       => 3
Unknown          => 3
```

---

## Bench Suite (32 حالة)

### Python (14): broken import, wrong assertion, wrong signature,
  wrong logic, type mismatch, missing closing, undefined func,
  wrong logic 2, syntax error, wrong return, missing function,
  runtime error, flask hello, fastapi route

### Go (4): go add, go fizzbuzz*, go reverse, go divide
  *go fizzbuzz: أحياناً يحتاج repair لـ import "fmt"

### Node (5): node add, node palindrome, node factorial,
  node filter, express api

### TypeScript (5): ts add, ts palindrome, ts factorial,
  ts stack, ts express

### Rust (4): rust add, rust fizzbuzz, rust reverse, rust stack

---

## Integration Bench (7 حالات × 2 مراحل)
```
1. patch + ref_file           (Rust)
2. fix_rust_string_literals   (Rust)
3. duplicate detection        (Rust)
4. skeleton multi-file        (Rust)
5. flask add route            (Python/Flask)
6. fastapi add endpoint       (Python/FastAPI)
7. marketing bot patch reddit (TypeScript OOP)
```

---

## المشاكل الحالية
```
الأولوية  المشكلة                    الأثر                الإصدار
──────────────────────────────────────────────────────────────────
🔴 1      لا Provider Cascade        توقف يدوي عند نفاد   v7.1
🟠 2      go fizzbuzz 0.1 repairs    جودة ناقصة           v7.1
🟠 3      لا Project Manifest        Tasks معزولة          v7.2
🟠 4      build_workspace_context    tokens تنفد في POS   v7.2
           ينمو بلا حد
🟡 5      patch_file هش              repair عشوائي         v7.5
🟡 6      لا FileSnapshot            خطوات صامتة           v7.4
🟢 7      لا Failure Memory          يكرر الخطأ            v8.2
🟢 8      لا Self-Evaluation         لا يعرف جودة عمله    v8.0
```

---

## القواعد الذهبية

```
1.  Regression Guard: bench >= السابق قبل كل commit
2.  heredoc فقط — لا .sh scripts خارجية
3.  Python3 للتعديلات المعقدة
4.  cargo build 2>&1 | grep "^error" بعد كل تغيير
5.  bench --iterations 1 فقط أثناء التطوير
6.  اختبر كل ميزة منفردة قبل bench
7.  git commit بعد كل milestone
8.  تحديث هذه الوثيقة بعد كل commit
9.  Weekly Checkpoint: bench نفس الأسبوع الماضي أو أفضل؟
10. لا feature جديد قبل 32/32 | 0.0 | 100% مستقر
11. لا تقس POS قبل provider مستقر × 3 تشغيلات
12. كل مشكلة = حل جذري — لا ضمادات
```

---

## أوامر التشغيل

```bash
# بناء
cd ~/projects/active/sel-agent-v4
cargo build --release 2>&1 | grep "^error"

# bench كامل
./target/release/sel-agent bench --iterations 1

# integration bench
./target/release/sel-agent bench --suite integration

# run عادي
rm -rf /tmp/tq && mkdir /tmp/tq
./target/release/sel-agent run \
  --workspace /tmp/tq \
  --goal "Create Python add(a,b). Write 3 pytest tests. Run."

# Observatory
cd ~/sel-observatory && ./target/release/sel_observatory &
# http://172.27.155.106:8777/live
```

---

## استئناف الجلسة القادمة

```bash
# الأوامر الأولى — بالترتيب الإلزامي
cd ~/projects/active/sel-agent-v4
cat SEL_CONTEXT.md
git log --oneline -5

# تحقق من Provider
echo "GEMINI: ${GEMINI_API_KEY:0:15}..."
echo "MODEL:  $SEL_MODEL"

# تحقق من الحالة
./target/release/sel-agent bench --iterations 1 2>&1 | tail -10

# الهدف: 32/32 | 0.0-0.1 repairs | 100% mutation
# إذا لم يتحقق → لا تبدأ v7.1
```

---

## git log

```
3baee14  v7.0: Provider info + clear error messages — 32/32 | 100% mutation
0a4a4de  v7.0: 32/32 | 0.0 repairs | 100% mutation
4d11355  v7.0: 32/32 | 0.0 repairs | 100% mutation — stable base restored
3b8f34b  v6.9-gemini: stable base
db1039f  v6.9: Markdown Plans
```

---

*وثيقة حية — تُحدَّث بعد كل commit*
*المطور: Chokri Bouzid | المساعد: Claude, Anthropic*
*v7.0 — مستقر وجاهز لـ v7.1*
