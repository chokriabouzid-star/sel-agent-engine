
## ✅ DONE 2026-09-15 — P1: Go literal `\"` from JSON double-escaping

**Root cause (confirmed on live artifact `/tmp/sel-bench-0-12/main_test.go`, 234 B, sha256 477f080a…):**
gpt-oss-120b emits real newlines together with `\\"` inside JSON `content`.
`serde_json` decodes `\\"` -> literal `\"` and `\n` -> real newline. Then
`protocol::smart_unescape` mixed-mode guard (`!contains('\n') && contains("\n")`)
intentionally skips it -> `\"` survives -> Go rejects with `illegal character U+005C`.
`fix_go_backslashes` only handled DOUBLE backslash (`\\"`), never single `\"`
-> `[TRACE] write_file sanitize: input=234 output=234` -> classifier returned
`Unknown` -> diagnostic returned `[generic]`.

**Fix (5 files, 300+ lines, 10 regression tests, 621 tests total, clippy -D clean):**
- `executor/sanitizers.rs`: `fix_go_escaped_quotes` (fires only when EVERY `"` is
  escaped — provably cannot break valid Go) + `unescape_go_quotes_on_line`.
- `executor/autofix.rs`: `autofix_go_escaped_quotes` — compile-triggered on `U+005C`,
  line-targeted via `file.go:LINE:COL`, whole-file fallback; wired into Go autofix loop.
- `executor/file_ops.rs`: pre-write fix in `write_file` + `patch_file`; `count>1`
  hint now points to `write_file` with COMPLETE content (breaks the
  `found N times -> run_tests -> not found` loop).
- `failure.rs`: `illegal/invalid character U+…` in `.go` -> `SyntaxError` (was `Unknown`).
- `diagnostic.rs`: `go/escaped-quotes` hint (was `[generic]`).
- Fixture `P1_GO_FIXTURE` is a byte-exact copy of the bench output.

**Design note:** whole-file rule deliberately does NOT fire on MIXED files (some
quotes escaped, some not) — the line-targeted compile autofix handles those.

---


## [LOW] Mutation: `!==`→`!!=` in TS is a syntax error, reported as "survived"
- task `node palindrome`: should be classified compile-error→skip (as Rust does)
- cost 2 repair calls + false 0% mutation score

## [LOW] Rust oracle: `Oracle:Unknown Running: cargo` without `test`/`--manifest-path`
- tasks rust add / fizzbuzz / v7_rust_quotes: verify "N passed" is real
- `rust reverse` used --manifest-path explicitly and was fine

## [INFRA] Dead provider keys: OpenRouter 401 "User not found" (key deleted, not quota)
- Gemini/Cerebras also permanently expired — rotate or remove from .env
- Design: when all fallbacks dead, wait RPM cooldown on Groq instead of failing

### 🟡 P2 — `fix_go_backslashes` قد يكسر Go صالحًا (اكتُشف أثناء P1 2026-09-15)
- `executor/sanitizers.rs`: `.replace("\\\\\"", "\\\"")` يحوّل `"C:\\\\"` (backslash
  مُهرَّب صالح + إغلاق string) إلى `"C:\\"` فيكسر التركيب. غير مُغطّى باختبار سلبي.
- الإصلاح المقترح: تقييد الاستبدال بأن يسبق `\\\\"` محرفٌ غير backslash، أو تحليل
  حالة الـ string. اختبار P1 `p1_fix_go_escaped_quotes_repairs_fixture` يضمن فقط
  أن الدالة لا تُفسد ناتجنا — لا يغطّي هذا الخلل.

### 🟢 P3 — الملف المختلط (بعض الاقتباسات مُهرَّبة) (اكتُشف 2026-09-15)
- `fix_go_escaped_quotes` عمدًا لا يُطلَق على ملف مختلط (يعود `None`). حاليًا تتكفّل
  الطبقة الثانية (compile-triggered, سطرًا بسطر) بعد رفض المترجم. مقبول، لكن يستحق
  اختبار regression لملف مختلط حقيقي عند توفّره من bench.

