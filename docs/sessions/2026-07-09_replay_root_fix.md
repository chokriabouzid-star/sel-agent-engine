
جلسة 2026-07-09: إصلاح السبب الجذري لفشل replay
الهدف
استرجاع الاستقرار بعد فوضى WIP

ما تم إنجازه
تشخيص أن commit WIP فشل بصمت (regression_gate pre-commit hook)
استرجاع 3 ملفات WIP من git dangling blobs
حفظها في ~/sel_recovery_2026_07_08/
تشخيص فشل bench all replay (35/36)
تحديد السبب الجذري:
run->run_tests canonicalization مفقود في initial/replan paths
shell() لا يحل مسارات نسبية مثل venv/bin/pytest
إصلاح في protocol.rs و core.rs
bench all replay = 36/36
ما لم يُحل
RC-03 Python Circular Import (17/18 في bench-sel-v11)
هذا solver quality issue وليس engine bug
الملفات المعدلة
src/protocol.rs (is_test_like_command + into_cmd canonicalization)
src/executor/core.rs (resolve relative paths in shell())
الدروس المستفادة
لا git reset --hard قبل التحقق من نجاح commit
دائمًا احفظ نسخة خارج الريبو قبل أي عملية خطرة
فصل أنواع المشاكل: engine vs fixture vs solver
