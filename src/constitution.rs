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
   - The test requirements define the absolute ground truth.

6. ALGORITHM CORRECTNESS (COMMON HALLUCINATIONS — AVOID):
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
</SYSTEM_CONSTITUTION>
"#;
