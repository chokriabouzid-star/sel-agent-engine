# A3 Evidence — Unified Run Policy (Node built-in + pip + replay npm)

## القاعدة المعمارية المطبقة
ما يمكن فرضه في الكود لا يُترك للـ prompt.
- الكود: حتمي
- الـ prompt: احتمالي

## السبب الجذري المثبت (TS-01)
- الخطة الأصلية تنتج `npm install crypto`
- `crypto` هو Node.js built-in — لا يُنصَّب
- قبل الإصلاح: يُنفَّذ → 5 repairs → SEL_FAILED
- prompt hardening وحده لم يمنعه (النموذج كرره)

## الحل (توحيد لا تكديس)
أُنشئ مصدر حقيقة واحد لسياسة أوامر التشغيل:
- src/executor/run_policy.rs (preflight_shell + ShellPolicyDecision)
  يجمع:
    * replay npm mutation policy
    * npm install <built-in> rejection (live)
    * pip install missing-package rejection
    * allowed-programs / service routing
- src/executor/node_builtins.rs وسّع بـ:
    * extract_npm_package()
    * is_npm_install_builtin()
- src/executor/core.rs::shell() بُسّط ليستهلك preflight_shell فقط
- أُزيل ALLOWED const المكرر من core.rs
- أُزيلت قواعد pip/builtin من system prompt (صارت مفروضة بالكود)

## الأدلة (LIVE)
TS-01 BEFORE: npm install crypto executed → SEL_FAILED (5 repairs)
TS-01 AFTER : npm install crypto rejected → import fix → SEL_SUCCESS (1 repair, 80pts)

PY-03 AFTER : 0 repairs / 100pts (no regression)
PY-04 AFTER : 1 repair  / 80pts (remaining repair is logic, not pip-policy)

## الاستقرار
cargo test: 398 passed / 0 failed
regression gate core: 36/36 + 30/30 + 18/18

## ملاحظة للمتابعة
guard QuickFix في runner.rs أصبح الآن دفاعًا ثانويًا فقط،
لأن run_policy يمنع npm install <built-in> قبل التنفيذ.
يُراجع لاحقًا كتبسيط مستقل.
