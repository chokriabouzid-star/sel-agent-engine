## ✅ DONE 2026-09-21 — Phase 3b — Oracle flag preservation & nested Cargo manifest path resolution

**Evidence:** `tests/oracle_flag_regressions.rs` (14 tests: 4 bug + 6 guard + 4 E2E).
Before fix, `resolve_test_command` collapsed every Go/Rust target into hardcoded defaults,
dropping caller flags (`-race`, `-count=1`, `-run`, `--lib`, `--manifest-path`), and `runner.rs`
hardcoded `cargo test -- --nocapture`, ignoring resolved arguments and running the root package instead of nested manifests.

**Fix (`src/workspace_oracle.rs` & `src/executor/runner.rs`):**
- `workspace_oracle.rs`:
  - `caller_test_extras`: extracts tokens after `<tool> test` when target is a plain, safe invocation (no shell metachars `& | ; > < $ \` ' "`).
  - `go_args_from_extras`: preserves explicit flags (`-run`, `-count`, etc.), guarantees `args[0] == "test"` for `-race` injection, enforces package scope (`./...` default), and ensures `-v` for `parse_go_tests`.
  - `cargo_args_from_extras`: preserves explicit flags (`--lib`, `-p`, `--manifest-path`) and appends `-- --nocapture` tail when absent.
- `runner.rs`:
  - Cargo branch executes the Oracle-resolved `cargo_args` instead of discarding them.
  - `resolve_manifest_path_args`: normalizes relative `--manifest-path` values against the workspace root so execution in `find_cargo_workspace` points to the correct manifest.

**Guarantee scope & limits:**
- Applies to Go and Rust test command resolution when targets start with `<tool> test`.
- Shell pipelines (`go test ./... && echo ok`) fall back safely to defaults.
- Node and Python resolution arms remain untouched in this pass.
- `find_cargo_workspace` non-deterministic directory scan for multi-manifest subdirs remains P2.

---

## ✅ DONE 2026-09-20 — Phase 2 — timed-out processes are killed and reaped

**Evidence (reproduced on 8c9ad86):** `tests/process_lifecycle_safety.rs` recorded a
child PID, forced a 1s timeout, then `kill -0 <pid>` — both cases FAILED:
`Process with PID 31326 is still running` (shell) and
`Node runner with PID 31327 is still running` (run_tests).

**Root cause:** every call site used `tokio::time::timeout(d, cmd.output())`.
That only drops the future; without `kill_on_drop` the OS process keeps running
while the agent already moved into Repairing and started rewriting files.

**Fix:** new `src/executor/process.rs::output_with_timeout(&mut TCmd, Duration)`
- `kill_on_drop(true)`, piped stdio, explicit `spawn()`
- timeout → `child.kill().await` (start_kill + wait) → terminated AND reaped
  before returning, then `Ok(None)`
- normal exit → `Ok(Some(Output))`; spawn failure → `Err` (NotFound preserved)
Call sites migrated: `core.rs::shell`, `runner.rs` cargo / go / node / node-retry / pytest.
`core.rs::shell` timeout stays `Err(anyhow)` (unchanged contract); runner timeouts
stay `Ok(ExecResult::fail)` — P0 behavioral tests still green.

**Guarantee scope:** the DIRECT child only.

---

### 🟡 P2 — grandchildren survive a killed parent (discovered 2026-09-20)
`child.kill()` signals one PID. `npm test`, `pytest -n`, `go test` sub-binaries can
outlive it. Needs a process group (`setsid` / `process_group(0)` + `killpg`), plus a
regression that spawns parent→child and asserts both die.

### 🟡 P2 — mutation.rs still uses the unsafe timeout pattern
`src/executor/mutation.rs:226` still calls `tokio::time::timeout(30s, Command::output())`.
Each surviving mutant run can leak a process. Migrate to `output_with_timeout` after
reading the restore/`std::fs::write(&source_path, &original)` path — it must stay correct
on timeout.

### 🟢 P3 — compile.rs / run_policy.rs use blocking std::process with NO timeout
`go_compile_check`, `python_syntax_check`, `python_importable_in_workspace_venv` call
`std::process::Command::output()` with no deadline — a hanging toolchain blocks the
whole async runtime. Needs its own design pass (not a mechanical swap).

### 🟢 P3 — per-phase timeouts still missing
`go mod tidy`, `go mod init`, `npm install`, `python3 -m venv`, `pip install` run
un-timed inside `run_tests`. Roadmap §5c asks for separate deadlines for dependency
setup / compile / test.

---

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


## ✅ DONE 2026-09-20 — Phase 3a — Oracle: race negation + nested manifest discovery

**Evidence (reproduced on a752295):** `tests/oracle_neg_regressions.rs` —
`goal_requires_go_race("Fix bug. DO NOT enable the race detector. Run go test.")` returned
true (substring match); `WorkspaceOracle::current_type()` returned `Unknown` for a
workspace whose only manifest is `mylib/Cargo.toml`.

**Fix (`src/workspace_oracle.rs`):**
- `goal_requires_go_race`: clause-based. Split on `; \n ! ?` (NOT `.`, which would break
  `./...`). A clause mentioning "race" with an explicit negation (`do not`, `don't`, `never`,
  `without`, `disable`, `must not`, `no -race`, `skip the race`) returns false; otherwise the
  original P0 positive patterns apply per clause.
- `detect_project_type` = `detect_root_markers` (unchanged order/priority) then
  `detect_nested_markers(depth=2)`, skipping `target node_modules venv .git .venv dist build
  __pycache__`; child dirs visited in sorted order for determinism.

**Known limits (deliberate, conservative):**
- A negation word and a positive race request in the SAME clause (no `;`/newline between)
  resolves to "do not force" — we prefer not injecting `-race` over injecting it wrongly.
- Nested discovery picks the FIRST manifest in sorted order; multi-project workspaces still
  need an explicit policy (roadmap §6c).
- `resolve_test_command` still drops most caller flags for known project types and the
  cargo branch does not build `--manifest-path` from the nested location — next Phase 3 step.
