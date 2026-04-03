# SEL Agent — الوثيقة المرجعية الشاملة
## من أداة تنفيذ إلى شريك هندسي ذكي

**الإصدار الحالي:** v7.1  
**تاريخ التحديث:** مارس 2026  
**المعماري:** Chokri Bouzid  
**المساعد المعماري:** Claude, Anthropic

---

# الفصل الأول: ما بنيناه — السجل الحقيقي

## 1.1 هوية المشروع

SEL Agent وكيل برمجي مستقل، مكتوب بلغة Rust، يأخذ هدفاً بالغة الطبيعية، يخطط خطوات التنفيذ عبر LLM، ينفذها، ويصلح الأخطاء تلقائياً باستخدام State Machine محكم.

**ليس:** محرر ذكي مثل Cursor أو مساعد كتابة مثل Copilot.  
**هو:** وكيل مستقل يُنجز المهمة كاملة بدون تدخل — على جهازك، بنموذجك، بكودك.

## 1.2 النتائج الحالية (v7.1)

```
╔══════════════════════════════════════════╗
║  Bench:        32/32  — 100%            ║
║  Integration:   7/7   — 100%            ║
║  Avg Repairs:   0.0                     ║
║  Mutation:      100%                    ║
║  Quality:       1.00                    ║
╚══════════════════════════════════════════╝
```

## 1.3 تاريخ الإصدارات الموثق

```
v6.3  ── ScaffoldEngine
│         يُجهّز البيئة قبل LLM — يلغي 80% أخطاء البيئة
│         Pinned stacks: Python, TypeScript, Node.js
│
v6.4  ── RepairBudget + InfraError
│         يميّز أخطاء الشبكة عن الكود
│         Retry policy: 3 محاولات بـ backoff بدون LLM
│
v6.5  ── GoalParser + Goal-Aware Scaffold
│         ParsedGoal { kind, sub_kind, extra_deps }
│         SubKind: Flask / FastAPI / Express / React / Plain
│
v6.6  ── PatchError + Auto-Context Injection
│         يقرأ كل ملفات الـ workspace قبل Planning
│         TS runner fix: npm test بدل pytest
│
v6.7  ── Bench Suite 28→32 حالة
│         +flask hello, +fastapi route, +express api, +ts express
│
v6.8  ── Integration Bench 4→7 حالات
│         +flask add route, +fastapi add endpoint
│         +marketing bot patch reddit (TypeScript multi-file OOP)
│
v6.9  ── Observatory v3 + Markdown Plans
│         State Machine visual (5 nodes) + Event Timeline
│         Commands::Plan — يقرأ "- [ ] goal" lines بالترتيب
│
v6.9.1── Multi-Provider Support
│         SEL_API_KEY + SEL_API_BASE + SEL_MODEL
│         OpenRouter كـ universal gateway — 200+ نموذج
│         7 aliases: openrouter, groq, cerebras, together...
│         GROQ_API_KEY يبقى fallback
│
v7.1  ── Failure Intelligence Layer ✅ (الحالي)
          src/failure_extractor.rs (333 lines)
          FailureReport { kind, file, line, expected, got, hint }
          repair hints مخصصة لكل FailureKind
          مدمج في agent.rs repair loop
```

## 1.4 هيكل الملفات الأساسية

```
src/
├── main.rs              — CLI + bench + Plan runner
├── agent.rs             — State Machine + Auto-Context Injection
├── executor.rs          — run / write_file / run_tests / patch_file
├── scaffold_engine.rs   — تجهيز البيئة الحتمي قبل LLM
├── goal_parser.rs       — ParsedGoal { kind, sub_kind, extra_deps }
├── types.rs             — FailureKind + RepairBudget
├── failure_extractor.rs — FailureReport { kind, file, line, hint } ← v7.1
├── llm.rs               — Multi-Provider API client ← v6.9.1
├── protocol.rs          — enum Cmd { Run, WriteFile, RunTests, PatchFile }
├── environment.rs       — EnvironmentCapabilities::probe()
├── context.rs           — ContextConfig + ref_file handling
├── evaluator.rs         — Mutation testing
└── memory.rs            — Session memory

~/sel-observatory/       — Observatory v3 (Rust + Tide + SQLite, port 8777)
```

---

# الفصل الثاني: التقييم الصادق

## 2.1 نقاط القوة

| النقطة | التفاصيل |
|--------|----------|
| محرك تنفيذ صلب | State Machine محكم — لا حالات غامضة |
| ScaffoldEngine | تجهيز البيئة قبل LLM — يلغي 80% أخطاء البيئة |
| Auto-Context Injection | يفهم المشروع الموجود قبل التخطيط |
| Failure Intelligence | يشخّص الخطأ بدقة — repair موجّه لا عشوائي |
| Multi-Provider | 200+ نموذج — لا توقف عند نفاد tokens |
| Bench موثق | 32/32 حقيقي — لا أرقام مضخمة |
| استقلالية كاملة | محلي، مفتوح المصدر، تتحكم في كل شيء |

## 2.2 نقاط الضعف المتبقية (مرتبة بالأولوية)

| # | المشكلة | الأثر الحالي | الحل | الإصدار |
|---|---------|-------------|------|---------|
| 1 | فشل الشبكة/النموذج غامض | المستخدم لا يفهم ما حدث | Network Transparency | v7.2 |
| 2 | اختيار النموذج عشوائي | نموذج ضعيف للمهمة = repairs زائدة | Model Profiler | v7.3 |
| 3 | patch_file هش | يفشل عند أي تغيير بسيط في السياق | Patch Intelligence | v7.4 |
| 4 | لا ذاكرة بين الجلسات | يبدأ من صفر في كل جلسة | Workspace Memory | v8.0 |
| 5 | لا فهم مشاريع كبيرة | يتشبع فوق 50 ملف | Import Graph | v8.0 |
| 6 | Repair loop غير ذكي | أحياناً يكرر نفس الخطأ | Adaptive Strategies | v8.1 |

---

# الفصل الثالث: خارطة الطريق التفصيلية

## المنطق العام للخارطة

```
v7.x  ── طبقة الذكاء حول النموذج والشبكة
          (الشفافية، الاختيار الذكي، الـ patching القوي)

v8.x  ── طبقة الذكاء حول الكود
          (الذاكرة، التقييم الذاتي، الـ repair الموجّه)

v9.x  ── طبقة الذكاء حول المشروع
          (التحليل، التقسيم، الواجهة الرسومية)

v10.x ── طبقة الذكاء حول المبرمج
          (يعرفك، يتوقعك، يسألك لماذا)

v11.0 ── الشريك الكامل
          (من كاتب كود إلى مصمم نوايا)
```

---

## v7.2 — Network & Model Transparency

**السؤال الذي يجيب عنه:** ماذا حدث بالضبط؟

**المشكلة:**
عند فشل الشبكة أو النموذج، المستخدم يرى رسالة غامضة أو لا يرى شيئاً. لا يعرف: هل المشكلة في النت؟ في الـ API key؟ في حد الـ tokens؟ في النموذج نفسه؟

**الحل — تشخيص دقيق وعرض واضح:**

```
🔴 Rate limit reached
   Waiting 45s before retry (2/3)
   💡 Tip: switch to OpenRouter to avoid daily limits

🔴 Model returned invalid JSON (2/2 attempts)
   Could not parse execution plan
   💡 Try: export SEL_MODEL=llama-3.3-70b-versatile

🟡 Slow response — 8.2s latency
   Model may be overloaded — continuing...

🔴 No internet connection
   Check your network and retry
   All local files are safe
```

**ما يُضاف في Observatory — Network Panel:**
- Network Health indicator (🟢/🟡/🔴)
- Model response time (avg + last)
- JSON parse success rate
- Retry history مع الأسباب

**الملفات:** `llm.rs` + `agent.rs` + Observatory `/live`

---

## v7.3 — Model Profiler + Auto-Model Selection

**السؤال الذي يجيب عنه:** أي نموذج أنسب لهذه المهمة؟

**المشكلة:**
كل نموذج له شخصية مختلفة. المستخدم لا يعرف هذا — يختار عشوائياً:

| النموذج | قوي في | ضعيف في |
|---------|--------|---------|
| kimi-k2 | كود Python/TS دقيق | JSON أحياناً |
| llama-3.3 | JSON منظم، سرعة | كود معقد |
| qwen3-coder | كود عام، تعدد لغات | سياق طويل |
| GPT-4o | تخطيط معماري | تكلفة/سرعة |

**الحل — بطاقة شخصية لكل نموذج:**

```
📊 Model Profile: kimi-k2-instruct
────────────────────────────────────
Planning Quality:     ████████░░  82%
JSON Reliability:     ██████░░░░  61%
Code Correctness:     █████████░  90%
Repair Success:       ████████░░  78%
Multi-file Context:   ███████░░░  70%
Avg Response Time:    1.8s
────────────────────────────────────
✅ Best for:  Python, TypeScript, new projects
⚠️  Avoid for: complex JSON, large context
```

**4 suites قياس متخصصة:**

```
JSON Suite      — كم مرة يُنتج JSON صحيح بدون retry
Planning Suite  — جودة الخطة: منطق، اكتمال، عدد خطوات
Code Suite      — bench الحالي مقسّم بلغة ونوع
Context Suite   — فهم مشاريع موجودة كبيرة
```

**الاستخدام:**
```bash
sel-agent profile --model kimi-k2-instruct
sel-agent compare --models kimi-k2,llama-3.3 --suite json
sel-agent run --goal "..." --auto-model
```

**في Observatory — صفحة `/models`:**
```
Model Leaderboard
──────────────┬──────┬──────┬────────
 Model        │ Code │ JSON │ Speed
──────────────┼──────┼──────┼────────
 kimi-k2      │  90% │  61% │  1.8s
 llama-3.3    │  75% │  88% │  0.9s
 qwen3-coder  │  85% │  79% │  1.2s
```

---

## v7.4 — Patch Intelligence

**السؤال الذي يجيب عنه:** كيف نعدّل ملفاً موجوداً بدون كسره؟

**المشكلة:**
`patch_file` يعتمد على exact string matching. أي تغيير بسيط — سطر فارغ، مسافة، تعليق — يُفشله.

**الحل — 4 طبقات متدرجة:**

```
المحاولة 1: Exact match (الحالي)
      ↓ فشل
المحاولة 2: Fuzzy match (تحمّل فروق طفيفة في المسافات)
      ↓ فشل
المحاولة 3: Line-number anchor (أرقام الأسطر كـ مرساة)
      ↓ فشل
المحاولة 4: Diff-based (إعادة توليد الـ diff كاملاً)
```

**الأثر المتوقع:** إلغاء `PatchError` شبه كامل — من أكثر أسباب الـ repair الحالية.

---

## v8.0 — Workspace Memory (القفزة الكبرى)

**السؤال الذي يجيب عنه:** ماذا يوجد في هذا المشروع ولماذا بُني هكذا؟

**هذا هو التحول الجوهري من "أداة" إلى "نظام".**

**الفرق بين ما عندنا والهدف:**

```
Auto-Context Injection (الحالي):
→ يقرأ الملفات الموجودة
→ يعرف "ماذا" في المشروع

Workspace Memory (v8.0):
→ يفهم "لماذا" بُنيت هكذا
→ يتذكر القرارات المعمارية
→ يعرف تاريخ كل ملف وسبب تغييره
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
    purpose: String,         // ماذا يفعل هذا الملف
    exports: Vec<String>,    // ما يُصدّره
    imports: Vec<String>,    // ما يستورده
    last_modified: SystemTime,
    change_reason: String,   // لماذا تغيّر آخر مرة
}
```

يُحفظ في `.sel/workspace.json` — يُحدَّث بعد كل تعديل — يُقرأ في بداية كل جلسة.

---

## v8.1 — Self-Evaluation Engine

**السؤال الذي يجيب عنه:** هل ما بُني جيد؟

**المشكلة:** Tests passing ≠ good system.

بعد كل `SEL_SUCCESS`، يسأل الوكيل نفسه:
- هل الكود قابل للقراءة؟
- هل هناك edge cases ناقصة؟
- هل التصميم يحترم Single Responsibility؟
- هل هناك تكرار يمكن تجنبه؟

**المخرج:** تقرير قصير في Observatory — لا إعادة كتابة تلقائية.

---

## v8.2 — Adaptive Repair Strategies

**السؤال الذي يجيب عنه:** كيف نصلح بأقل tokens ممكنة؟

بدل إرسال الخطأ الخام للـ LLM — اختيار استراتيجية بحسب FailureKind:

```
NameError      → أضف الـ import أو الـ function المفقودة فقط
AssertionError → راجع الـ logic — لا تُعد كتابة الملف
SyntaxError    → أصلح السطر المحدد فقط
ImportError    → تحقق من requirements/package.json أولاً
PatchError     → استخدم write_file كـ fallback
```

**الأثر:** توفير 40-50% في tokens الـ repair.

---

## v8.3 — Execution Memory

**السؤال الذي يجيب عنه:** هل رأينا هذا الخطأ من قبل؟

```rust
struct MemoryEntry {
    failure_pattern: String,  // بصمة الخطأ
    fix_applied: String,      // ما أصلحناه
    success: bool,
    language: String,
}
```

عند رؤية خطأ مشابه — يتحقق من الذاكرة أولاً قبل استدعاء LLM.  
إذا وجد pattern ناجح — يطبقه مباشرة.

**الأثر:** تقليل استدعاءات LLM تدريجياً مع الاستخدام.

---

## v9.0 — Project Decomposition + Desktop UI

**السؤال الذي يجيب عنه:** ما الذي يجب أن يُبنى؟

**Project Decomposition:**
SEL يأخذ هدفاً كبيراً ويُقسّمه هو:

```
"أريد تطبيق SaaS لإدارة المشاريع"
              ↓
Phase 1: Database schema + migrations
Phase 2: Auth system (JWT)
Phase 3: REST API endpoints
Phase 4: Integration tests
Phase 5: Documentation
```

كل phase = مهمة في `--plan TODO.md` تُنفَّذ تلقائياً.

**Desktop UI — Tauri:**
تطبيق desktop خفيف (5-10 MB) بثلاثة أعمدة:

```
┌──────────────┬─────────────────────────┬──────────────┐
│  Workspaces  │   🎯 Goal Input          │    Stats     │
│  + History   │   ─────────────────     │  + Bench     │
│              │   State Machine          │  + Files     │
│  Token meter │   Event Timeline         │  + Model     │
└──────────────┴─────────────────────────┴──────────────┘
```

Observatory مدمج في الواجهة — لا نافذة منفصلة.

---

## v10.0 — "الشريك الذي يسأل لماذا"

**السؤال الذي يجيب عنه:** ما الذي تعلّمناه؟

**التحول الجوهري:**
الوكيل يبدأ يسألك قبل أن تسأله:

> "لاحظت أن هذه الدالة تُستدعى في 3 أماكن بطرق مختلفة — هل تريد توحيدها؟"

> "هذا الـ endpoint لا يعالج حالة الـ timeout — هل أضيف معالجة؟"

> "الـ test coverage في هذا الملف 40% — هل أكمل الاختبارات؟"

**Token Efficiency Engine:**
- Prompt Compression — الـ context الضروري فقط
- Partial Context — الـ function المعنية فقط، لا كل الملف
- Cache Layer — إعادة استخدام Plans مشابهة

**الهدف:** نفس النتائج بنصف الـ tokens.

---

## v11.0 — "المهندس الذي يعرفك"

**السؤال الذي يجيب عنه:** ما الذي تريده فعلاً؟

ذاكرة عميقة عبر مشاريع:
- يعرف كل مشروع بنيته وكل قرار اتخذته
- يعرف أسلوبك في التصميم وتفضيلاتك
- يقترح الخطوة التالية قبل أن تسألها

**التحول في دور المبرمج:**

```
الآن:    كاتب كود + موجّه وكيل
v11.0:   مصمم نوايا
```

---

# الفصل الرابع: اللغات — خارطة التوسع المنطقية

| الإصدار | اللغة | السبب | المتطلب التقني |
|---------|-------|-------|----------------|
| v7.x | Rust scaffold رسمي | يعمل لكن بدون scaffold محدد | Cargo.toml pinned |
| v7.x | Go scaffold رسمي | يعمل لكن بدون scaffold محدد | go.mod pinned |
| v8.0 | Java | 40% مشاريع enterprise | Gradle + JUnit |
| v8.1 | PHP | 70% الويب، زبائن كثيرون | Composer + PHPUnit |
| v9.0 | C | مشاريع العتاد الخاصة | Valgrind + ASan + clang-tidy |
| v9.1 | C++ | بعد C مباشرة | CMake + نفس أدوات C |
| عند الطلب | Swift/Kotlin | تطبيقات Mobile | Xcode/Android Studio |
| أبداً | Assembly | لا test runner قياسي | — |

**ملاحظة مهمة على C/C++:**
أخطاء الذاكرة في C لا تظهر في الاختبارات العادية (`Segmentation fault` بدون سبب أو موقع). Valgrind + AddressSanitizer يجعلان الأخطاء الخفية مرئية وقابلة للإصلاح الآلي. بدونهما، repair loop لا يعمل في C.

---

# الفصل الخامس: الموقع التنافسي

## التصنيف الصحيح

```
أدوات AI للبرمجة
├── مساعد        → Copilot, Tabnine    (يقترح أثناء الكتابة)
├── محرر ذكي    → Cursor, Aider       (يعدّل ملفات موجودة)
└── وكيل مستقل  → SEL Agent, Devin   (يُنجز المهمة كاملة) ← هنا
```

## المقارنة المباشرة

| | SEL Agent | Cursor | Devin | Copilot |
|---|---|---|---|---|
| ينفذ بدون تدخل | ✅ | ❌ | ✅ | ❌ |
| يُصلح أخطاءه | ✅ | ❌ | ✅ | ❌ |
| يعمل محلياً | ✅ | ❌ | ❌ | ❌ |
| مفتوح المصدر | ✅ | ❌ | ❌ | ❌ |
| Multi-provider | ✅ | ❌ | ❌ | ❌ |
| Bench موثق | ✅ 32/32 | ❌ | ⚠️ | ❌ |
| التكلفة/شهر | ~5-10$ | 20$ | 500$ | 10$ |

**لماذا Devin بـ 500$/شهر؟**
يعمل على خوادمهم — أنت تدفع مقابل CPU, RAM, وقت, كهرباء لكل مهمة.  
SEL يعمل على جهازك — الخادم أنت. هذا ميزة، لا نقص.

**الميزة التي لا يمكن منافستها:**
المنافسون يبنون لـ "المبرمج العام".  
SEL يُبنى لـ Chokri — بأسلوبه، بقيوده، بمشاريعه.

---

# الفصل السادس: المبادئ الثابتة

**١. الصدق في القياس**
32/32 حقيقي. "Stable for Small-Medium Projects" وصف دقيق. لا أرقام مضخمة.

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

**٧. الاستقرار قبل التوسع**
bench 32/32 يجب أن يبقى 32/32 بعد كل إضافة. لا ننتقل لخطوة جديدة قبل التحقق.

---

# الفصل السابع: الخيط الرابط

كل مجموعة إصدارات تجيب على سؤال واحد:

| المرحلة | الإصدارات | السؤال |
|---------|-----------|--------|
| الأساس | v6.x (منجز) | **كيف ننفّذ؟** |
| الشفافية | v7.x (الآن) | **كيف نفهم ما يحدث؟** |
| الجودة | v8.x | **هل ما بُني جيد؟** |
| المنتج | v9.x | **ما الذي يجب أن يُبنى؟** |
| التعلم | v10.x | **ما الذي تعلّمناه؟** |
| الشراكة | v11.0 | **ما الذي تريده فعلاً؟** |

---

# الفصل الثامن: المرجع التشغيلي

## بداية كل جلسة

```bash
cd ~/projects/active/sel-agent-v4
cat SEL_CONTEXT.md
git log --oneline -5
./target/release/sel-agent bench --iterations 1 2>&1 | tail -12
```

## أوامر أساسية

```bash
# بناء والتحقق
cargo build --release 2>&1 | grep "^error"

# bench كامل
./target/release/sel-agent bench --iterations 1

# bench suite محدد
./target/release/sel-agent bench --suite python
./target/release/sel-agent bench --suite integration

# تشغيل هدف واحد
rm -rf /tmp/test_ws && mkdir /tmp/test_ws
./target/release/sel-agent run \
  --workspace /tmp/test_ws \
  --goal "Create Python function add(a,b). Write pytest test. Run tests."

# Markdown Plans
./target/release/sel-agent plan \
  --workspace /tmp/project \
  --plan TODO.md

# Observatory
cd ~/sel-observatory && ./target/release/sel_observatory &
# افتح: http://172.27.155.106:8777/live

# تغيير النموذج
export SEL_MODEL="llama-3.3-70b-versatile"
export SEL_MODEL="qwen/qwen3-coder"
```

## المسارات الأساسية

```
Project:     ~/projects/active/sel-agent-v4/
Binary:      ~/projects/active/sel-agent-v4/target/release/sel-agent
Context:     ~/projects/active/sel-agent-v4/SEL_CONTEXT.md
Reference:   ~/projects/active/sel-agent-v4/REFERENCE.md
Vision:      ~/projects/active/sel-agent-v4/VISION.md
Observatory: ~/sel-observatory/target/release/sel_observatory
```

## FailureKind::max_attempts()

```
PatchError       => 2  (context mismatch)
InfraError       => 0  (retry فقط — لا LLM)
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

# الخلاصة

> SEL Agent اليوم: أداة ممتازة تبني مشاريع جديدة بدون تدخل.
>
> SEL Agent غداً: نظام يفهم، يتذكر، ويسألك لماذا.
>
> **القفزة الكبرى لن تكون في الكود — ستكون في اللحظة التي يبدأ فيها الوكيل بسؤالك "لماذا؟" قبل أن ينفّذ.**

---

*وثيقة حية — تُحدَّث مع كل إصدار رئيسي*  
*المعماري: Chokri Bouzid | المساعد المعماري: Claude, Anthropic*
