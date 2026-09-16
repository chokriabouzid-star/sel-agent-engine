## ✅ DONE 2026-09-16 — P0 — go test -race enforcement + runner timeout recovery

**Evidence (live, 2026-09-15):** `workerpool` goal demanded `go test -race ./... must pass`;
Oracle ran `go test ./... -v` -> `SEL_SUCCESS`. Manual `go test -race` on the same
workspace: `WARNING: DATA RACE … Found 1 data race(s)`, exit 1 -> **false-positive success**.
`ratelimit` task: `go test timeout` returned `Err(anyhow!)` -> fatal `SEL_FAILED`, no repair.

**Root cause:** `WorkspaceOracle::resolve_test_command` hard-codes `["test","./...","-v"]`;
every runner branch mapped `tokio::time::timeout` -> `Err`, and `state_handlers` treats `Err`
as fatal while `Ok(!success)` enters `Repairing`.

**Fix (6 files):**
- `workspace_oracle::goal_requires_go_race(goal)` — pure, explicit phrases only.
- `SafeExecutor.force_go_race: AtomicBool` + `set_force_go_race`; both `Agent` constructors set
  it from the goal (`[TRACE] P0: goal requires Go race detector`).
- `runner` Go branch injects `-race` when forced and absent (`[TRACE] P0: injected -race -> go …`).
- `runner` ALL branches (cargo/go/node×2/pytest): timeout -> `Ok(ExecResult::fail)`.
- `failure.rs`: `* test timeout` -> `BuildError` (repair via LLM, not silent InfraError retry).
- `diagnostic.rs`: `test/runner-timeout` hint (deadlock / time.Sleep guidance).
- Tests: 6 unit + 2 behavioral (real `go test -race` flips racy suite to FAIL; real node
  timeout returns repairable failure). Skip gracefully without toolchains.

**Not covered / follow-ups:**
- `-race` needs cgo (`CGO_ENABLED=1`, gcc). If unavailable, `go test -race` itself fails ->
  surfaces as BuildError to the LLM; consider a preflight message.
- Timed-out child processes are not killed (`kill_on_drop` not set) — orphan until they exit.
- No enforcement yet for Rust/Node equivalents (e.g. `cargo miri`, jest `--detectOpenHandles`).

---


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

