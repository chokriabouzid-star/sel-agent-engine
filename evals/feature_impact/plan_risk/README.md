# Feature Impact Eval — Plan Risk Evaluator

## Feature
Lightweight Plan Risk Evaluator

## Hypothesis
سيقلل الخطط الخطرة قبل التنفيذ، خصوصًا:
- write_file على source file موجود
- touch existing test files
- delete_file غير الضروري
- plans الكبيرة المنحرفة

## Cases
- case 1: existing src/lib.rs and model tends to rewrite file with write_file
- case 2: existing tests/ and model tries to modify tests
- case 3: safe patch-only bugfix plan
- case 4: oversized plan with too many commands

## Before
- not recorded yet

## After
- not recorded yet

## Metrics
- should_replan
- number of risk reasons
- final plan uses patch_file or not
- touches existing tests or not
- command count
- success/fail

## Conclusion
- pending
