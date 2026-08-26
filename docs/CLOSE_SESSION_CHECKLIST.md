
🔒 قائمة إغلاق الجلسة — Close Session Checklist
قبل أن تغلق أي محادثة، افعل هذا:
1) تحقق من الشجرة
Bash

git status --short
cargo check 2>&1 | tail -3
cargo test --quiet 2>&1 | tail -5
2) إذا تم تعديل كود، شغّل البوابة
Bash

scripts/regression_gate.sh core 2>&1 | tail -10
3) اطلب من النموذج تلخيص الجلسة بهذا البروبت
text

لخص الجلسة الآن في هذا القالب بالضبط:

## الهدف
[جملة واحدة]

## ما الذي تم
[قائمة نقطية]

## الملفات المعدلة
[قائمة بالملفات والتغييرات]

## حالة البوابات الآن
| البوابة | النتيجة |
|---------|---------|
| cargo test | ✅/❌ |
| regression_gate core | ✅/❌ |
| bench all replay | ✅/❌ X/36 |

## القضايا المفتوحة
[ما لم يُحل]

## أول خطوة في الجلسة القادمة
[جملة واحدة محددة]
4) احفظ الملخص
Bash

# استبدل YYYY-MM-DD و topic بالقيم الصحيحة
cat > docs/sessions/YYYY-MM-DD_topic.md << 'SESSION'
[الصق هنا ملخص النموذج]
SESSION
5) حدّث STABILITY_LEDGER
Bash

nano docs/STABILITY_LEDGER.md
# حدّث:
# - تاريخ آخر تحديث
# - حالة البوابات
# - القضايا المفتوحة
# - التغييرات الأخيرة
6) إذا كان الكود مستقرًا — commit
Bash

git add src/ docs/
git status --short
# راجع ما سيُضاف
git commit -m "fix(scope): وصف موجز"
✋ لا تغلق الجلسة إذا:
 الشجرة متسخة بدون خطة
 regression_gate core يفشل وعندك تعديلات
 لم تحفظ ملخص الجلسة
