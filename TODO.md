
## [MED] Go: literal backslash from LLM not caught by json_sanitizer
- Bench 2026-09-15 task `go add`: main_test.go:3:8 illegal character U+005C
- write_file sanitize input==output → sanitizer missed it
- Classifier: add pattern "illegal character U+005C" → autofix unescape
- Also: on "search block found N times" hint LLM to use write_file

## [LOW] Mutation: `!==`→`!!=` in TS is a syntax error, reported as "survived"
- task `node palindrome`: should be classified compile-error→skip (as Rust does)
- cost 2 repair calls + false 0% mutation score

## [LOW] Rust oracle: `Oracle:Unknown Running: cargo` without `test`/`--manifest-path`
- tasks rust add / fizzbuzz / v7_rust_quotes: verify "N passed" is real
- `rust reverse` used --manifest-path explicitly and was fine

## [INFRA] Dead provider keys: OpenRouter 401 "User not found" (key deleted, not quota)
- Gemini/Cerebras also permanently expired — rotate or remove from .env
- Design: when all fallbacks dead, wait RPM cooldown on Groq instead of failing
