# SEL Agent — الوثيقة المرجعية الشاملة
## من أداة تنفيذ إلى شريك هندسي ذكي

**الإصدار الحالي:** v7.1  
**تاريخ الوثيقة:** مارس 2026  
**المعماري:** Chokri Bouzid  
**المساعد المعماري:** Claude, Anthropic

---

# الفصل الأول: ما بنيناه — الأساس الصلب

## 1.1 هوية المشروع

SEL Agent وكيل برمجي مستقل، مكتوب بلغة Rust، يأخذ هدفاً بالغة الطبيعية، يخطط خطوات التنفيذ عبر LLM، ينفذها، ويصلح الأخطاء تلقائياً باستخدام State Machine محكم.

**ليس:** محرر ذكي مثل Cursor أو مساعد كتابة مثل Copilot.  
**هو:** وكيل مستقل يُنجز المهمة كاملة بدون تدخل.

## 1.2 الإنجازات الموثقة (v7.1)

```
Bench Suite:        32/32  (100%) — 5 لغات
Integration Bench:   7/7   (100%) — Phase1 + Phase2
Avg Repairs:         0.0
Quality Index:       1.00
Failure Intelligence: v7.1 ✅
Multi-Provider:       v6.9.1 ✅
```

## 1.3 البنية المعمارية الحالية

```
┌─────────────────────────────────────────────────────┐
│                    SEL Agent Core                    │
├──────────────┬──────────────────┬───────────────────┤
│   Planning   │    Executing     │    Repairing      │
│              │                  │                   │
│ GoalParser   │ ScaffoldEngine   │ FailureKind       │
│ AutoContext  │ Executor         │ RepairBudget      │
│ Injection    │ run/write/patch  │ FailureExtractor  │
└──────────────┴──────────────────┴───────────────────┘
         ↕                ↕                ↕
┌─────────────────────────────────────────────────────┐
│              SEL Observatory v3                      │
│   State Machine Visual + Event Timeline + Stats      │
└─────────────────────────────────────────────────────┘
```

## 1.4 تاريخ الإصدارات

| الإصدار | الإنجاز | الأثر |
|---------|---------|-------|
| v6.3 | ScaffoldEngine | إلغاء 80% أخطاء البيئة |
| v6.4 | RepairBudget + InfraError | repair ذكي حسب نوع الخطأ |
| v6.5 | GoalParser + Goal-Aware Scaffold | فهم نوع المشروع |
| v6.6 | PatchError + Auto-Context Injection | يقرأ الـ workspace قبل Planning |
| v6.7 | Bench 28→32 حالة | Flask, FastAPI, Express, TS Express |
| v6.8 | Integration Bench 4→7 حالات | Marketing Bot multi-file OOP |
| v6.9 | Observatory v3 + Markdown Plans | State Machine visual + --plan TODO.md |
| v6.9.1 | Multi-Provider Support | OpenRouter + أي نموذج |
| v7.0 | Failure Intelligence Layer | تشخيص دقيق للأخطاء |
| v7.1 | (تفاصيل من git log) | — |

## 1.5 هيكل الملفات

```
src/
├── main.rs              — CLI + bench + Plan runner
├── agent.rs             — State Machine + Auto-Context Injection
├── executor.rs          — run/write_file/run_tests/patch_file
├── scaffold_engine.rs   — تجهيز البيئة الحتمي
├── goal_parser.rs       — ParsedGoal { kind, sub_kind, extra_deps }
├── types.rs             — FailureKind + RepairBudget
├── failure_extractor.rs — FailureReport { kind, file, line, symbol }
├── llm.rs               — Multi-Provider API client
├── protocol.rs          — enum Cmd { Run, WriteFile, RunTests, PatchFile }
├── environment.rs       — EnvironmentCapabilities::probe()
├── context.rs           — ContextConfig + ref_file
├── evaluator.rs         — Mutation testing
└── memory.rs            — Session memory
```

---

# الفصل الثاني: نقاط القوة والضعف — تقييم صادق

## 2.1 نقاط القوة

**محرك تنفيذ صلب:**
- State Machine محكم — لا حالات غامضة
- ScaffoldEngine يُجهّز البيئة قبل LLM — يلغي 80% أخطاء البيئة
- Auto-Context Injection — يفهم المشروع الموجود قبل التخطيط
- FailureKind Taxonomy — كل نوع خطأ له استراتيجية مختلفة

**قابلية التحقق:**
- Bench 32/32 حقيقي وموثق — لا أرقام مضخمة
- Integration bench يختبر سيناريوهات واقعية
- Mutation testing يقيس جودة الكود فعلاً

**استقلالية كاملة:**
- يعمل محلياً على جهازك — لا سحابة
- Multi-Provider — لا اعتماد على provider واحد
- مفتوح المصدر — تتحكم في كل شيء

## 2.2 نقاط الضعف المتبقية

| المشكلة | الأثر | الحل | الإصدار |
|---------|-------|------|---------|
| Failure display غامض | المستخدم لا يفهم ما حدث | Network Transparency | v7.2 |
| لا Model Profiling | اختيار النموذج عشوائي | Model Profiler | v7.3 |
| patch_file هش | يفشل عند تغيير بسيط | Patch Intelligence | v7.4 |
| لا ذاكرة بين الجلسات | يبدأ من صفر دائماً | Workspace Memory | v8.0 |
| لا فهم مشاريع كبيرة | يتشبع فوق 50 ملف | Import Graph | v8.0 |

---

# الفصل الثالث: خارطة الطريق التفصيلية

## المرحلة الحالية: v7.x — "طبقة الذكاء حول النموذج"

---

### v7.2 — Network & Model Transparency

**المشكلة:** عند فشل الشبكة أو النموذج، المستخدم يرى رسالة غامضة أو لا يرى شيئاً.

**الحل — تشخيص دقيق وعرض واضح:**

```
🔴 Rate limit reached
   Waiting 45s before retry (2/3)
   Tip: switch to OpenRouter to avoid this

🔴 Model returned invalid JSON
   Retrying with stricter prompt (1/2)
   If persists: try a different model

🟡 Slow response — 8.2s latency
   Model may be overloaded

🔴 No internet connection
   Check your network and retry
```

**ما يُضاف في Observatory:**
- Network Health indicator — أخضر/أصفر/أحمر
- Model response time — متوسط وقت الاستجابة
- JSON parse success rate — كم مرة النموذج أعاد JSON صحيح
- Retry history — كم retry حدث ولماذا

**الملفات المعنية:** `llm.rs` + `agent.rs` + Observatory HTML

---

### v7.3 — Model Profiler + Auto-Model Selection

**المشكلة:** كل نموذج له شخصية مختلفة — المستخدم لا يعرف أيها أنسب لمهمته.

**الحقيقة عن النماذج:**

| النموذج | قوي في | ضعيف في |
|---------|--------|---------|
| kimi-k2 | كود Python/TS | JSON أحياناً |
| llama-3.3 | JSON منظم | كود معقد |
| GPT-4o | تخطيط معماري | سرعة/تكلفة |
| Gemini | ملفات كبيرة | دقة التفاصيل |

**الحل — بطاقة شخصية لكل نموذج:**

```
📊 Model Profile: kimi-k2-instruct
─────────────────────────────────
Planning Quality:     ████████░░  82%
JSON Reliability:     ██████░░░░  61%
Code Correctness:     █████████░  90%
Repair Success:       ████████░░  78%
Multi-file Context:   ███████░░░  70%
Speed (avg):          1.8s/call
─────────────────────────────────
Best for: Python, TypeScript, new projects
Avoid for: complex JSON, large context
```

**4 suites متخصصة للقياس:**
1. **JSON Suite** — كم مرة يُنتج JSON صحيح بدون repairs
2. **Planning Suite** — جودة الخطة (منطق، اكتمال، خطوات)
3. **Code Suite** — الـ bench الحالي مقسّم بلغة ونوع مهمة
4. **Context Suite** — فهم المشاريع الموجودة الكبيرة

**الاستخدام العملي:**
```bash
# تشغيل profiler
sel-agent profile --model kimi-k2-instruct

# مقارنة نموذجين
sel-agent compare --models kimi-k2,llama-3.3 --suite json

# اختيار تلقائي حسب المهمة
sel-agent run --goal "..." --auto-model
```

**في Observatory — صفحة `/models`:**
```
┌─────────────────────────────────────┐
│  Model Leaderboard                  │
├──────────────┬──────┬──────┬────────┤
│ Model        │ Code │ JSON │ Speed  │
├──────────────┼──────┼──────┼────────┤
│ kimi-k2      │  90% │  61% │  1.8s  │
│ llama-3.3    │  75% │  88% │  0.9s  │
│ gpt-4o       │  92% │  95% │  3.2s  │
└──────────────┴──────┴──────┴────────┘
```

---

### v7.4 — Patch Intelligence

**المشكلة:** `patch_file` يعتمد على exact string matching — يفشل عند أي تغيير بسيط في السياق.

**الحل — ثلاث طبقات للـ patching:**

```
المحاولة 1: Exact match (الحالي)
      ↓ فشل
المحاولة 2: Fuzzy match (تحمّل فروق بسيطة)
      ↓ فشل
المحاولة 3: Line-number anchor (استخدام أرقام الأسطر)
      ↓ فشل
المحاولة 4: Diff-based patching (إعادة توليد الـ diff)
```

**الأثر المتوقع:** إلغاء `PatchError` شبه كامل — من أكثر أسباب الـ repair الحالية.

---

## المرحلة القادمة: v8.x — "طبقة الذكاء حول الكود"

---

### v8.0 — Workspace Memory (القفزة المعمارية الكبرى)

**هذا هو v7.0 الحقيقي بالمفهوم المعماري — قفزة في هوية الوكيل.**

**المشكلة:** كل جلسة تبدأ من صفر. الوكيل لا يعرف ما بناه أمس.

**الفرق بين ما عندنا الآن وما سيكون:**

```
Auto-Context Injection (الحالي):
→ يقرأ الملفات الموجودة
→ يعرف "ماذا" في المشروع

Workspace Memory (v8.0):
→ يفهم "لماذا" بُنيت هكذا
→ يتذكر القرارات المعمارية
→ يعرف تاريخ كل ملف
```

**البنية التقنية:**

```rust
struct WorkspaceModel {
    files: HashMap<PathBuf, FileModel>,
    dependencies: Vec<Dependency>,
    architecture_notes: Vec<String>,
    decision_log: Vec<ArchDecision>,
}

struct FileModel {
    purpose: String,        // ماذا يفعل هذا الملف
    exports: Vec<String>,   // ما يُصدّره
    imports: Vec<String>,   // ما يستورده
    last_modified: SystemTime,
    change_reason: String,  // لماذا تغيّر آخر مرة
}
```

يُحفظ في `.sel/workspace.json` داخل كل مشروع. يُحدَّث بعد كل تعديل.

---

### v8.1 — Self-Evaluation Engine

**المشكلة:** Tests passing ≠ good system.

بعد كل `SEL_SUCCESS`، يسأل الوكيل نفسه:
- هل الكود قابل للقراءة؟
- هل هناك edge cases ناقصة؟
- هل التصميم يحترم مبدأ Single Responsibility؟
- هل هناك تكرار يمكن تجنبه؟

**المخرج:** تقرير قصير يظهر في Observatory — لا إعادة كتابة تلقائية.

---

### v8.2 — Adaptive Repair Strategies

بدل إرسال الخطأ للـ LLM مباشرة — اختيار استراتيجية بحسب FailureKind:

```
NameError    → أضف الـ import أو الـ function المفقودة تحديداً
AssertionError → راجع الـ logic فقط — لا تُعد كتابة الملف
SyntaxError  → أصلح السطر المحدد فقط
ImportError  → تحقق من requirements/package.json أولاً
```

**الأثر:** توفير 40-50% في tokens الـ repair.

---

### v8.3 — Execution Memory

```rust
struct MemoryEntry {
    failure_pattern: String,  // بصمة الخطأ
    fix_applied: String,      // ما أصلحناه
    success: bool,
    language: String,
}
```

عند رؤية خطأ مشابه — يتحقق من الذاكرة أولاً. إذا وجد pattern ناجح — يطبقه بدون LLM.

**الأثر:** تقليل استدعاءات LLM تدريجياً مع الاستخدام.

---

## المرحلة البعيدة: v9.x → v11.0

### v9.0 — Project Decomposition + Desktop UI

**التحول:** SEL يأخذ هدفاً كبيراً ويُقسّمه هو — لا المستخدم.

```
"أريد تطبيق SaaS لإدارة المشاريع"
         ↓
Phase 1: Database schema
Phase 2: Auth system
Phase 3: REST API
Phase 4: Integration tests
Phase 5: Documentation
```

**Desktop UI — Tauri:**
تطبيق desktop خفيف (5-10 MB) بدل CLI:

```
┌─────────────────────────────────────┐
│  🎯 اكتب هدفك هنا...               │
│  ________________________________   │
│                      [▶️ Run]       │
├─────────────────────────────────────┤
│  State Machine + Event Timeline     │
│  (Observatory مدمج)                │
└─────────────────────────────────────┘
```

3 أعمدة: Workspaces+History | Goal+StateMachine+Log | Stats+Bench+Files

---

### v10.0 — "الشريك الذي يسأل لماذا"

**التحول الجوهري:**

الوكيل يبدأ **يسألك قبل أن تسأله:**

> "لاحظت أن هذه الدالة تُستدعى في 3 أماكن بطرق مختلفة — هل تريد توحيدها؟"

> "هذا الـ endpoint لا يعالج حالة الـ timeout — هل أضيف معالجة؟"

هذا ليس "AI يقترح ميزات" — هذا مهندس يراقب ويفكر.

**Token Efficiency Engine:**
- Prompt Compression — إرسال فقط الـ context الضروري
- Partial Context — الـ function المعنية فقط، لا كل الملف
- Cache Layer — إعادة استخدام Plans مشابهة

---

### v11.0 — "المهندس الذي يعرفك"

الوكيل يملك ذاكرة عميقة عبر مشاريع:
- يعرف كل مشروع بنيته وكل قرار اتخذته
- يعرف أسلوبك في التصميم وتفضيلاتك
- يقترح الخطوة التالية قبل أن تسألها

**التحول في دور المبرمج:**
من **كاتب كود** إلى **مصمم نوايا**.

---

# الفصل الرابع: اللغات — خارطة التوسع

| الإصدار | اللغة | السبب | المتطلبات |
|---------|-------|-------|-----------|
| v7.x | Rust scaffold رسمي | موجود لكن بدون scaffold | Cargo.toml pinned |
| v7.x | Go scaffold رسمي | موجود لكن بدون scaffold | go.mod pinned |
| v8.0 | Java | 40% مشاريع enterprise | Gradle + JUnit |
| v8.1 | PHP | 70% الويب، زبائن كثيرون | Composer + PHPUnit |
| v9.0 | C | مشاريع العتاد | Valgrind + ASan + clang-tidy |
| v9.1 | C++ | بعد C مباشرة | CMake + نفس أدوات C |
| مستقبل | Swift/Kotlin | عند طلب فعلي | Xcode/Android (قيد بيئة) |
| أبداً | Assembly | لا test runner قياسي | — |

**لماذا C/C++ يحتاجان Valgrind + ASan؟**
أخطاء الذاكرة في C لا تظهر في الاختبارات العادية — تظهر كـ `Segmentation fault` بدون موقع أو سبب. Valgrind وAddressSanitizer يجعلان الأخطاء الخفية مرئية — وبالتالي قابلة للإصلاح آلياً.

---

# الفصل الخامس: المقارنة مع المنافسين

## الموقع في السوق

```
أدوات AI للبرمجة
├── مساعد (Copilot, Tabnine)     → يقترح أثناء الكتابة
├── محرر ذكي (Cursor, Aider)     → يعدّل ملفات موجودة
└── وكيل مستقل (SEL, Devin)     → يُنجز المهمة كاملة ← هنا نحن
```

## المقارنة المباشرة

| | SEL Agent | Cursor | Devin | Copilot |
|---|---|---|---|---|
| يعمل بدون تدخل | ✅ | ❌ | ✅ | ❌ |
| يُصلح أخطاءه | ✅ | ❌ | ✅ | ❌ |
| يعمل محلياً | ✅ | ❌ | ❌ | ❌ |
| مفتوح المصدر | ✅ | ❌ | ❌ | ❌ |
| التكلفة/شهر | ~5-10$ | 20$ | 500$ | 10$ |
| Bench موثق | ✅ 32/32 | ❌ | ⚠️ | ❌ |
| Multi-provider | ✅ | ❌ | ❌ | ❌ |

**لماذا Devin بـ 500$/شهر؟**
يعمل على خوادمهم الخاصة — أنت تدفع مقابل CPU, RAM, وقت, كهرباء. SEL يعمل على جهازك — الخادم أنت.

**الميزة التنافسية الحقيقية لـ SEL:**
المنافسون يبنون لـ "المبرمج العام". أنت تبني لـ Chokri — وهذا لا يمكن لأحد منافستك فيه.

---

# الفصل السادس: المبادئ الثابتة

هذه المبادئ لا تتغير بتغيّر الإصدار:

**١. الصدق في القياس**
أرقام Bench حقيقية دائماً. "Stable for Small-Medium Projects" وصف دقيق — لا ادعاءات زائفة.

**٢. القيود الحقيقية أولاً**
300K tokens يومياً. مطور واحد. هدف محدد. كل قرار معماري يحترم هذه القيود.

**٣. البساطة على الجمال**
لا نبني ما هو جميل على الورق. نبني ما يحل مشكلة حقيقية اليوم.

**٤. الاختبار قبل الادعاء**
كل ميزة: اختبار فردي → bench كامل → commit → تحديث Context.

**٥. السياق مقدّس**
`SEL_CONTEXT.md` يُقرأ في بداية كل جلسة. يُحدَّث بعد كل milestone.

**٦. لا نعود للوراء في الإصدارات**
ما فات فات. الميزات الناقصة تأتي كإصدارات جديدة — لا تعديل للتاريخ.

---

# الفصل السابع: الخيط الرابط

كل إصدار يجيب على سؤال واحد:

| الإصدار | السؤال |
|---------|--------|
| v6.x (منجز) | **كيف ننفّذ؟** |
| v7.x (الآن) | **كيف نفهم ما يحدث؟** |
| v8.x | **هل ما بُني جيد؟** |
| v9.x | **ما الذي يجب أن يُبنى؟** |
| v10.x | **ما الذي تعلّمناه؟** |
| v11.0 | **ما الذي تريده فعلاً؟** |

---

# الفصل الثامن: أوامر الاستئناف

## بداية كل جلسة

```bash
cd ~/projects/active/sel-agent-v4
cat SEL_CONTEXT.md
git log --oneline -5
./target/release/sel-agent bench --iterations 1 2>&1 | tail -12
```

## أوامر أساسية

```bash
# بناء
cargo build --release 2>&1 | grep "^error"

# bench كامل
./target/release/sel-agent bench --iterations 1

# integration bench
./target/release/sel-agent bench --suite integration

# Markdown Plans
./target/release/sel-agent plan \
  --workspace /tmp/project \
  --plan TODO.md

# Observatory
cd ~/sel-observatory && ./target/release/sel_observatory &
# http://172.27.155.106:8777/live

# اختبار سريع
rm -rf /tmp/test_q && mkdir /tmp/test_q
./target/release/sel-agent run --workspace /tmp/test_q \
  --goal "Create Python function add(a,b). Write pytest test. Run tests."
```

---

# الخلاصة النهائية

> SEL Agent اليوم أداة ممتازة في نطاقها.
> الهدف الحقيقي أعمق من ذلك.
> **القفزة الكبرى ستكون في اللحظة التي يبدأ فيها الوكيل بسؤالك "لماذا؟" قبل أن ينفّذ.**
> هذا ما يميّز أداة عن شريك.

---

*وثيقة حية — تُحدَّث مع كل إصدار رئيسي*  
*المعماري: Chokri Bouzid | المساعد المعماري: Claude, Anthropic*
