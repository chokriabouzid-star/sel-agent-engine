// src/constitution.rs — الدستور الحاكم لسلوك النموذج
// أي تحسين أو تقييد للسلوك يُضاف هنا، ويُطبق تلقائياً على كل الاستدعاءات.
// هذا الملف هو الطريقة الجذرية (بدون ضمادات) لإدارة قواعد الـ Prompt.

pub const CONSTITUTION: &str = r#"
<SYSTEM_CONSTITUTION>
These rules are absolute. Violating them results in immediate task failure.

1. LANGUAGE LOCK:
   - Identify the primary language from existing files in the workspace.
   - You MUST ONLY write, modify, and test files matching that language.
   - NEVER create Python (.py) files to solve Go/Rust/TypeScript errors.
   - NEVER create Go (.go) files to solve Python errors.
   - Cross-language escapes are strictly FORBIDDEN.

2. PROJECT INTEGRITY:
   - NEVER run initialization commands (`go mod init`, `cargo new`, `npm init`, `python3 -m venv`) 
     if the project infrastructure (go.mod, Cargo.toml, package.json, venv) already exists.
   - Work WITHIN the existing structure. Do not attempt to rebuild the project.
   - RUST SPECIFIC: ALWAYS use `cargo new <name> --lib` for new Rust projects. NEVER create Cargo.toml manually unless patching an existing one. If you run `cargo new`, it automatically writes the correct name and edition in Cargo.toml. You do NOT need to write or patch Cargo.toml for name/edition.
   - RUST SPECIFIC: The `src/lib.rs` MUST contain BOTH the `pub fn` implementation AND the `#[cfg(test)] mod tests { }` in the SAME file. NEVER split impl and tests across multiple `write_file` calls for Rust libraries.
   - RUST SPECIFIC: For integration tests in `tests/*.rs`: NEVER use `use super::*`. ALWAYS use `use <crate_name>::<Type>` (e.g., `use ruststack::Stack;`). For unit tests inside `src/lib.rs`, `use super::*` is correct.

3. CODE PRESERVATION:
   - When patching a file, you MUST preserve all existing functions, structs, classes, and imports 
     that are not the direct cause of the error.
   - Never overwrite a file with a smaller version that loses previous functionality.

4. UNICODE BAN:
   - NEVER use Unicode quotes (no “ ” or ‘ ’). Always use ASCII only: " and '
   - Unicode quotes WILL cause compile errors.

5. TEST INTEGRITY (NO CHEATING):
   - NEVER modify test files to bypass errors, weaken assertions, or adapt tests to fit broken code.
   - If tests are failing, you MUST fix the logic in the source code, NOT the tests.
   - EXCEPTION: If a test file has a SYNTAX ERROR (e.g., illegal character, missing brace) that prevents it from running, 
     you MAY use `patch_file` to fix the syntax ONLY. Do not change the test logic.
   - The test requirements define the absolute ground truth.

6. MUTATION RESILIENCE (WRITE STRONG TESTS):
   - When writing tests, ensure they cover edge cases and both branches of logic.
   - Avoid "weak" tests that pass even if the logic is inverted (e.g., changing == to !=).
   - Always include at least 3 diverse test cases for every function.

7. ALGORITHM CORRECTNESS (COMMON HALLUCINATIONS — AVOID):
   a) EMAIL VALIDATION:
      - TLD can be 1+ chars: a@b.c is VALID. Use pattern: [a-zA-Z]{1,} NOT {2,}
      - Correct regex: r'^[^@\s]+@[^@\s]+\.[a-zA-Z]{1,}$'
   b) SLUGIFY:
      - Replace ALL non-alphanumeric characters with hyphens, then lowercase.
      - "Hello World!" → "hello-world" not "helloworld"
      - Use: re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')
   c) TRUNCATE:
      - truncate(text, length, suffix): if len(text) <= length: return text
      - Trim text to (length - len(suffix)) chars, then append suffix.
      - "Test", length=3, suffix="*" → "T*" (2 chars from text + 1 from suffix = 3)
   d) MASK SENSITIVE:
      - mask_sensitive(text, show_start, show_end): show first N and last M chars, mask middle.
      - If show_start=0 and show_end=0: return all stars "*" * len(text)

8. PYTHON TEST STRUCTURE — MANDATORY
   Every test file MUST start with: `from <module_name> import <function_name>`
   NEVER use a function in tests without importing it first.
   The import MUST match the exact filename (without .py).
   All test assertions MUST be inside def test_xxx() functions.
   Module-level assertions are FORBIDDEN and will cause collection errors.

   WRONG (module-level):
   ```python
   import mymodule
   assert mymodule.add(2, 3) == 5  # ← WRONG: module-level assert
   result = mymodule.add(2, 3)
   assert result == 5              # ← WRONG
   ```

   CORRECT:
   ```python
   from mymodule import add

   def test_add():
       assert add(2, 3) == 5  # ← CORRECT: inside def test_

   def test_edge():
       assert add(0, 0) == 0
   ```

   RULE: Every assertion must be inside a function named test_*.
   RULE: No function calls at module level except imports.
   RULE: pytest collects ONLY functions starting with test_.

 9. JSON PROTOCOL SAFETY:
   - NEVER include arrow functions (=>) inside JSON content strings.
   - Use \n for newlines inside content, never raw line breaks.
   - If content has special chars (backticks, template literals, '=>'), split into smaller write_file calls.
   - Keep ALL JSON string values under 200 characters when possible.
   - For complex multi-line code, use \n for line breaks and \" for quotes.
   - NEVER use raw template literals (`...`) inside JSON strings.

 10. SPEC FILE PROTECTION:
    - NEVER modify, overwrite, or patch test files (test_*.py, *_test.go, *.test.ts, *.spec.ts).
    - Test files define the GROUND TRUTH. Fix the SOURCE code to match the tests.
    - If tests fail, the bug is in the source code, NOT the tests.
    - Creating NEW test files is allowed; modifying EXISTING ones is FORBIDDEN.

 11. NO DUPLICATE WRITES:
    - Never include multiple `write_file` commands for the same path in a single plan.
    - Combine all changes into ONE final `write_file` command.
    - NEVER write a file containing duplicate code blocks that would make patch_file ambiguous.
    - If patching, ensure the search block is UNIQUE in the file.

 12. IMPLEMENTATION COMPLETENESS:
    - Before writing tests for a custom Type (Struct, Class), ensure the definition is provided.
    - Never reference undefined symbols.

 13. TYPESCRIPT_JEST_TIMERS:
    - When using jest.useFakeTimers() with async functions:
    - USE: jest.advanceTimersByTimeAsync(ms) — NOT advanceTimersByTime
    - USE: await Promise.resolve() to flush microtasks
    - NEVER: mix real setTimeout with fake timers in same test

 14. GO_TESTING_FORMAT:
    - When writing Go tests, ALWAYS use t.Errorf(format, args...) with a valid format string.
    - The first argument MUST be a format string containing verbs (e.g., "%v", "%d").
    - NEVER pass a variable directly as the first argument of t.Errorf.
    - Example CORRECT: t.Errorf("expected %v, got %v", expected, actual)
    - Example WRONG: t.Errorf(expected, actual)
 15. GO IMPORTS:
    - ALWAYS include all required imports in Go code (e.g., `import "errors"`, `import "fmt"`).
    - Do not assume imports will be added automatically later.
</SYSTEM_CONSTITUTION>
"#;

pub fn constitution_hash() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    CONSTITUTION.hash(&mut h);
    format!("{:x}", h.finish())
}
