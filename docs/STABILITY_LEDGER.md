
سجل استقرار المشروع — Stability Ledger
آخر تحديث: 2026-07-09
الفرع: audit/safe-cleanup-proof
الكوميت: eb34e1e + replay fix (غير ملتزم بعد)
حالة البوابات
البوابة	الحالة	النتيجة	ملاحظات
cargo fmt	✅	clean	—
cargo check	✅	ok	—
cargo clippy	✅	0 warnings	—
cargo test	✅	492 pass	—
bench all replay	✅	36/36	كان 35/36 قبل إصلاح replay
bench-swe replay	✅	30/30	—
bench-sel-v11	❌	17/18	RC-03 فقط
bench-real-world	✅	14/14	—
smoke	✅	12/12	—
regression_gate core	✅	pass	—
regression_gate full	❌	fail	بسبب bench-sel-v11
القضايا المفتوحة
RC-03: Python Circular Import
النوع: solver quality (ليس engine bug)
الوصف: الـ trajectory المسجل لا يحل circular import
الحل المتوقع: rerecord مع حل أفضل، أو تحسين repair strategy
الأولوية: عالية (يمنع regression_gate full)
التغييرات الأخيرة
2026-07-09: إصلاح replay root cause
الملفات: src/protocol.rs, src/executor/core.rs
العيب: run->run_tests canonicalization مفقود في initial/replan
النتيجة: bench all replay 35/36 → 36/36
الحالة: غير ملتزم بعد
WIP المحفوظ
~/sel_recovery_2026_07_08/ — ملفات D1-5 التجريبية
لا تُدمج حتى اكتمال برنامج التثبيت
