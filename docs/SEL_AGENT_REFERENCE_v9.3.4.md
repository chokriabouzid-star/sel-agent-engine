# SEL Agent — الوثيقة المرجعية الشاملة
**الإصدار:** v9.3.4
**الحالة:** Stable — replay hardening + diagnostic improvement + corpus refresh
**الفرع:** audit/safe-cleanup-proof
**آخر commit:** b8419c1
**التاريخ:** 2026-06-26

---

## 1) ملخص هذا الإصدار

v9.3.4 هو إصدار تثبيت وصيانة يأتي بعد v9.3.3.
لا يضيف ميزات جديدة كبيرة، بل يصلح مشاكل بنيوية في:
- أمان runtime state
- حتمية replay
- دقة التشخيص
- سلامة corpus

---

## 2) ما تغيّر في v9.3.4

### A. أمان معماري
**استبدال env vars العالمية بـ executor state**
- الملف: `src/executor/core.rs` + `src/agent.rs`
- commit: `a2ee5f3`
- قبل: كان المحرك يستخدم `std::env::set_var/remove_var` لتمرير
  سياسة test-write بين وحدات runtime
- بعد: الحالة تعيش داخل `SafeExecutor` عبر:
  - `RwLock<HashSet<PathBuf>>`
  - `AtomicBool`
- التأثير: إزالة undefined behavior محتمل في بيئات async

### B. تحصين replay determinism
**منع npm install المدمرة في replay mode**
- الملف: `src/executor/core.rs`
- commit: `a2ee5f3`
- قبل: `npm install` كانت تُنفَّذ حقيقيًا في replay وتغيّر node_modules
- بعد: إذا node_modules موجودة → no-op مع trace؛ إذا غائبة → REPLAY_ENV_MISMATCH
- التأثير: حل TS-01 replay drift + منع trajectory explosion

### C. إصلاح Rust validator false positive
**منع اعتبار TypeScript مشروع Rust**
- الملف: `src/decision/validators.rs`
- commit: `a2ee5f3`
- قبل: أي ملف في `src/` أو `tests/` كان يُعتبر Rust
- بعد: فقط `.rs` و `Cargo.toml` تُعتبر Rust
- التأثير: إزالة replans غير ضرورية في مشاريع TypeScript

### D. تشخيص جديد: python/class-no-init
- الملف: `src/diagnostic.rs`
- commit: `b8419c1`
- يُطلق عند: `TypeError: ClassName() takes no arguments`
- التأثير:
  - المحرك أصبح يفهم فئة الأخطاء هذه
  - يرشد LLM إلى @dataclass أو __init__ المناسب
  - لم نستخدم AutoFix ترقيعي — الحل في التشخيص لا في executor

### E. ربط compute_report_telemetry
- الملف: `src/agent.rs`
- commit: `f91be79`
- توحيد حساب telemetry في مساري Done/Failed

---

## 3) تحديثات Corpus/Trajectories

| Trajectory | التغيير |
|-----------|---------|
| `swe_ts_06` | إعادة تسجيل → 80pts بدل 50pts |
| `run_write_a_python_user_dataclass_` | إعادة تسجيل بعد تحسين diagnostic |
| Replay corpus عام | استرجاع من backup لإصلاح TRAJECTORY_INCOMPLETE |

---

## 4) النتائج المؤكدة

### Regression Gate
scripts/regression_gate.sh full ✅ PASS

### Benchmarks
bench --suite all --replay 36/36 ✅
bench-swe --lang all --replay 30/30 ✅
bench-sel-v11 --replay 18/18 ✅
bench-real-world --replay 14/14 ✅
sel_smoke_test --replay 12/12 ✅

### Build / Tests
cargo check ✅ (44 warnings مكشوفة بعد إزالة allow(dead_code))
cargo test ✅

## 5) البنية المعمارية — ما تغيّر

### Test-Write Authorization Flow
قبل v9.3.4:
agent.rs → std::env::set_var("SEL_ALLOW_GOAL_TEST_WRITES", "1")
file_ops.rs → std::env::var("SEL_ALLOW_GOAL_TEST_WRITES")

بعد v9.3.4:
agent.rs → self.executor.set_allow_goal_test_writes(true)
file_ops.rs → self.goal_authorized_test_write_allowed(path)

### Replay NPM Guard
في replay mode:
npm install/i/ci + node_modules موجودة → skip + trace
npm install/i/ci + node_modules غائبة → REPLAY_ENV_MISMATCH

### Diagnostic Chain
stderr
→ diagnostic::analyze() ✅ موجود
→ DiagnosticReport ✅ موجود
→ as_prompt_fragment() ✅ موجود
→ combined_hints في repair prompt ✅ موجود ومربوط

Categories المضافة في v9.3.4:
- `python/class-no-init` ← جديد

## 6) القيود الحالية المعروفة

| القيد | الوضع |
|-------|-------|
| 44 warning في cargo check | مقصود — قديمًا مخفية بـ allow(dead_code) |
| Replay determinism script | غير موجود بعد ← الجلسة القادمة |
| Baseline رسمي في evals/ | غير موجود بعد ← الجلسة القادمة |
| `src/executor/runner.rs` QuickFix npm | لم يُراجع بعد ← latent replay risk |
| `FailureMemory` vs `PatternLibrary` | غير محسوم ← بعد v9.4.0 |
| Pattern Graduation | غير موجود ← v9.4.0 |
| plan_confidence = None | ← v11.0 |

---

## 7) ما يجب عدم فعله

1. `git add -A -f fixtures/trajectories` بدون حصر
2. `--rerecord` على suite كبيرة بدون خطة وquota كافية
3. refactor واسع في `state_handlers.rs` قبل v9.4.0
4. إضافة AutoFix ترقيعي خاص بحالة واحدة قبل فحص السبب الجذري
5. اعتبار replay وحده دليلًا كاملًا على جودة live

---

## 8) الخارطة إلى الأمام
v9.3.5 (الجلسة القادمة):
✓ determinism proof (scripts/check_replay_determinism.sh)
✓ baseline رسمي (evals/baselines/)
✓ release binary hash
✓ cooldown=0 في replay mode
✓ prompt hardening (Node built-ins + pip package)
✓ audit لـ src/executor/runner.rs

v9.4.0 (milestone مخصص):
← Pattern Graduation → AutoFix
← تقوية repair loop (RF-01/RF-02/RF-03)

v9.5.0:
← Test Authoring Validation (Rule 8)

v9.6.0:
← Multi-File Coordinator

v9.7.0:
← SWE-Bench External Adapter




---

## 9) الأوامر المرجعية

```bash
# التحقق اليومي
cargo fmt --all
cargo check
cargo test

# قبل أي تغيير سلوكي
scripts/regression_gate.sh core

# قبل أي release
scripts/regression_gate.sh full

# Benchmarks
./target/release/sel-agent bench --suite all --replay
./target/release/sel-agent bench-swe --lang all --replay
./target/release/sel-agent bench-sel-v11 --replay
./target/release/sel-agent bench-real-world --replay
bash sel_smoke_test.sh ./target/release/sel-agent --replay

# Observability
./target/release/sel-agent report --latest
cat ~/.sel-agent/reports/latest.json | python3 -m json.tool
10) الملخص التنفيذي v9.3.4
text

Git:
  HEAD: b8419c1 (tag: v9.3.4)
  Branch: audit/safe-cleanup-proof → main
  version = "9.3.4"

Full Gate: ✅ PASS
  bench all:        36/36 ✅
  bench-swe:        30/30 ✅
  bench-sel-v11:    18/18 ✅
  bench-real-world: 14/14 ✅
  smoke:            12/12 ✅

Active Fixes (v9.3.4):
  ✅ executor state replaces runtime env flags
  ✅ replay npm mutation guard
  ✅ rust bootstrap validator — TS false positive fixed
  ✅ python/class-no-init diagnostic
  ✅ telemetry compute unified
  ✅ TS-06 + dataclass trajectories refreshed

Pending:
  ⬜ determinism proof script
  ⬜ baseline recording
  ⬜ runner.rs audit
  ⬜ cooldown skip in replay
  ⬜ prompt hardening
  ⬜ warnings cleanup
