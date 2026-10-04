جلسة 2026-10-04: W2 — mutation_check يحترم crates Rust المتداخلة

الهدف
إصلاح محرك واحد (Task 2 / W2): mutation_check كان يصنّف كل طفرات ملف داخل crate متداخلة كـ Uncompilable.

خط الأساس
f0ea08e على main، شجرة نظيفة.
cargo test --locked = 719 passed / 0 failed (مجموع أسطر test result).
تصحيح: الـ Handoff السابق ذكر 713، والقياس الفعلي عند f0ea08e هو 719.

التشخيص (بالسطر)
src/executor/mutation.rs، ذراع "rs": cargo test --quiet يُشغَّل من self.workspace بلا --manifest-path.
عند غياب Cargo.toml في الجذر يفشل cargo بـ could not find Cargo.toml.
src/diagnostic.rs:144 يلتقطها، فيصنّفها stderr_is_compile_failure كفشل ترجمة، فتُرجَع Uncompilable دون تنفيذ أي اختبار.

ما تم إنجازه
الفرع: fix/rust-nested-mutation
RED أولًا: tests/rust_nested_mutation.rs (3 اختبارات تستدعي SafeExecutor::mutation_check مباشرة)
- nested_crate_killed_mutation_is_strong
- nested_crate_surviving_mutation_is_weak (صمّام ضد النجاح الزائف: يثبت تنفيذ الاختبارات فعلًا)
- isolated_rs_without_manifest_is_skipped (مطابقة نصية دقيقة)
على f0ea08e: 3 failed، وleft = Uncompilable(...) في الحالات الثلاث، في 0.35s.
GREEN: find_nearest_cargo_manifest() بحث صعودي من الملف المصدر بعد canonicalize، محدود بجذر workspace شاملًا.
تمرير --manifest-path، وعند الغياب Skipped("no Cargo manifest found").
بعد الإصلاح: 3 passed في 2.67s.

البوابات
cargo fmt --all -- --check = 0 (فشل مرة أولًا بفارق تنسيقي، ثم صُحّح بـ cargo fmt)
cargo clippy --locked --all-targets --all-features -- -D warnings = 0
cargo test --locked = 722 passed / 0 failed
regression_gate.sh core عبر hook pre-commit الطبيعي: 36/36 + 30/30 + 18/18 = 84/84، بلا --no-verify
الكوميت: d609e7a

الملفات المعدلة
src/executor/mutation.rs (find_nearest_cargo_manifest + ذراع "rs")
tests/rust_nested_mutation.rs (جديد)

ما لم يُحل
- حالات حدّية غير مختبرة: symlink يخرج من workspace، ملف خارج workspace، workspace جذري مع member متداخل.
- W4: المهلة (tokio::time::timeout) والاستعادة عند panic في mutation.rs لم تُلمس. مرجع TODO.md:55 (mutation.rs:226) صار منزاحًا بعد هذا الكوميت.
- فجوة في docs/STABILITY_LEDGER.md: الالتزامات 46de35b و0989364 و98841dd وf0ea08e غير موثقة، وجدول الحالة ما زال من 2026-07-17. تحتاج جلسة توثيق مستقلة.

الدروس المستفادة
- لا set -e ولا exit في أوامر تُلصق تفاعليًا: أغلقت جلسة WSL.
- قواعد العمل في docs/AI_WORK_RULES.md وليست في الجذر.
- RED يجب أن يكون فشلًا دلاليًا بعد نجاح cargo check، لا خطأ ترجمة.
- cargo fmt --check قبل إعلان أي GREEN نهائي.
- البوابة عبر hook pre-commit كافية ورسمية، ولا مكان لـ --no-verify إطلاقًا.
