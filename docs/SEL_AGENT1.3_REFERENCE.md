# SEL Agent v1.3 — Reference Document
*آخر تحديث: 2026-03-08*

---

## نظرة عامة

SEL Agent هو **Autonomous Software Engineering Agent** مبني بـ Rust.
يعمل كـ state machine يخطّط وينفّذ ويصلح الكود تلقائياً.

- **المطوّر:** Chokri Bouzid
- **اللغة:** Rust
- **المسار:** `~/projects/sel-agent-v4/`
- **الثنائي:** `~/projects/sel-agent-v4/target/release/sel-agent`
- **LLM:** Groq API — `moonshotai/kimi-k2-instruct`
- **متغيرات البيئة:** `GROQ_API_KEY` (مطلوب)، `SEL_DEBUG=1` (اختياري)

---

## الاستخدام

```bash
./target/release/sel-agent run \
  --workspace ~/my-project \
  --goal "Create add(a,b) in math.py. Write pytest tests. Install pytest. Run tests." \
  --max-repairs 3
```

### خيارات

| الخيار | الوصف |
|---|---|
| `--workspace` | مجلد العمل (يُنشأ تلقائياً) |
| `--goal` | الهدف بالإنجليزية (يجب أن يحتوي كلمة test) |
| `--max-repairs` | أقصى عدد محاولات الإصلاح (افتراضي: 3) |
| `--dry-run` | عرض الخطة فقط بدون تنفيذ |

---

## بنية الملفات

```
sel-agent-v4/
├── src/
│   ├── main.rs        ← CLI + dry-run flag
│   ├── agent.rs       ← State machine + repair loop
│   ├── executor.rs    ← تنفيذ الأوامر + mutation testing
│   ├── llm.rs         ← Groq API + system prompt
│   ├── types.rs       ← FailureKind + FailedStep + AgentState
│   ├── protocol.rs    ← JSON plan parser + resilience
│   └── context.rs     ← Context + hash cache
├── Cargo.toml         ← version = "1.3.0"
└── benchmark_v13.sh   ← 8 اختبارات
```

---

## نتائج البنشمارك — 8/8 ✅

```
✅ L1-T1 — Simple multiply
✅ L1-T2 — String utility
✅ L2-T3 — Subdirectory import
✅ L3-T4 — Logic repair
✅ L4-T5 — Calculator package
✅ L5-T6 — JSON parser
✅ MUT-T7 — Mutation enforcement
✅ CHX-T8 — Flask chaos
```

---

## Features المُنجزة في v1.3

### 1. Protocol Resilience
**الملف:** `src/agent.rs`
إعادة المحاولة حتى 3 مرات عند فشل JSON parsing من LLM.

### 2. Mutation Enforcement
**الملف:** `src/agent.rs` + `src/executor.rs`
- `MutationResult::Weak(orig, mutd)` → `AgentState::Repairing`
- يُرسل diff المتحوّل للـ LLM: أي سطر تغيّر وكيف
- `apply_all_mutations` — يجرّب كل المتحوّلات، يتجاهل index/counter operations

### 3. FlaskConcurrency Classifier
**الملف:** `src/types.rs`
`FailureKind::FlaskConcurrency` يكشف أخطاء Flask app context.

### 4. --dry-run Flag
**الملف:** `src/main.rs`
عرض الخطة كاملة بدون تنفيذ أي أمر.

### 5. PYTHONPATH Injection
**الملف:** `src/executor.rs`
يحل `ModuleNotFoundError` عند وجود tests في subdirectory.

### 6. Smart Exit Code Detection
**الملف:** `src/executor.rs`
القاعدة الآمنة للنجاح: passed>0 و failed==0 ولا ERROR collecting.
يتجاهل exit code `-1` من PyQt6-WebEngine.

### 7. PyQt6 Rules في System Prompt
**الملف:** `src/llm.rs`
- دائماً PyQt6 (لا PyQt5)
- `setUrl(QUrl('...'))` وليس `setUrl('...')`
- imports صحيحة لـ WebEngine

### 8. venv-Aware Cache
**الملف:** `src/agent.rs`
إعادة تشغيل `pip install` إذا حُذف الـ venv حتى لو كان في الـ cache.

---

## مستويات الاختبار

| المستوى | الوصف | الحالة |
|---|---|---|
| L1 | دوال بسيطة | ✅ |
| L2 | Subdirectory imports | ✅ |
| L3 | Self-repair (logic bugs) | ✅ |
| L4 | مشاريع متعددة الملفات | ✅ |
| L5 | Mutation testing | ✅ |
| MUT | Mutation enforcement | ✅ |
| CHX | Flask chaos tests | ✅ |
| GUI | PyQt6 applications | ✅ (بـ repair) |

---

## FailureKind المدعومة

| النوع | يكشف |
|---|---|
| `SyntaxError` | أخطاء Python syntax |
| `ImportError` | module not found |
| `TypeError` | نوع خاطئ (مثل setUrl(str)) |
| `AssertionError` | فشل assertions |
| `FlaskConcurrency` | Flask app context errors |
| `Unknown` | أي خطأ آخر |

---

## قواعد صياغة Goal

1. يجب أن يحتوي كلمة **TEST** أو **test** وإلا يرفضه الـ agent
2. حدّد اسم الملف الرئيسي صراحةً
3. اذكر المكتبات المطلوبة في Install
4. لـ PyQt6: اذكر `QT_QPA_PLATFORM=offscreen` للاختبارات

---

## الخطوات القادمة (v1.4)

- [ ] Budget منفصل: `code_repairs` / `test_repairs` / `mutation_repairs`
- [ ] Import Repair Engine: تحويل المشروع إلى package تلقائياً
- [ ] Context-Aware Repair: إرسال project tree للـ LLM
- [ ] Workspace snapshot logger: `.sel_agent/state/`
