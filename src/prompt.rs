// src/prompt.rs — v7.2
// Centralized prompt building — replaces scattered logic in agent.rs

// ────────────────────────────────────────────────────────────────
// Types
// ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Rule {
    pub kind: RuleKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum RuleKind {
    Never,
    Always,
    Prefer,
    Context,
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub index: usize,
    pub goal: String,
    pub status: TaskStatus,
    pub files_created: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Passed { repairs: u32 },
    Failed { reason: String },
}

// ────────────────────────────────────────────────────────────────
// PromptEngine
// ────────────────────────────────────────────────────────────────

pub struct PromptEngine {
    pub model: String,
    pub rules: Vec<Rule>,
    pub history: Vec<TaskResult>,
}

impl PromptEngine {
    /// v7.2: القواعد كنص — للاستخدام المباشر في agent.rs
    pub fn default_rules_text() -> String {
        let engine = Self::new("default".into());
        engine.rules_section()
    }

    pub fn new(model: String) -> Self {
        Self {
            model,
            rules: Self::default_rules(),
            history: vec![],
        }
    }

    pub fn set_history(&mut self, history: Vec<TaskResult>) {
        self.history = history;
    }

    // ────────────────────────────────────────────────────────────
    // القواعد الافتراضية — كل قاعدة = مشكلة حقيقية اكتُشفت
    // ────────────────────────────────────────────────────────────

    fn default_rules() -> Vec<Rule> {
        vec![
            // اكتُشف في POS Test — passlib تكسر Python 3.12
            Rule {
                kind: RuleKind::Never,
                text: "import passlib. Use bcrypt directly:\n\
                       import bcrypt\n\
                       hashed = bcrypt.hashpw(pw.encode('utf-8'), bcrypt.gensalt())\n\
                       valid  = bcrypt.checkpw(pw.encode('utf-8'), hashed)".into(),
            },
            // اكتُشف في POS Test — deprecated في Python 3.12
            Rule {
                kind: RuleKind::Never,
                text: "use datetime.utcnow(). \
                       Use datetime.now(datetime.timezone.utc) instead".into(),
            },
            // اكتُشف في POS Test — Pydantic V2
            Rule {
                kind: RuleKind::Never,
                text: "use .dict() on Pydantic models. \
                       Use .model_dump() instead".into(),
            },
            // اكتُشف في POS Test — SQLAlchemy 2.0
            Rule {
                kind: RuleKind::Never,
                text: "use sqlalchemy.ext.declarative. \
                       Use sqlalchemy.orm.declarative_base() instead".into(),
            },
            // اكتُشف في POS Test — FastAPI يحتاجها دائماً
            Rule {
                kind: RuleKind::Always,
                text: "install python-multipart and httpx \
                       when using FastAPI".into(),
            },
            // اكتُشف في POS Test — Tasks تدمّر بعضها
            Rule {
                kind: RuleKind::Always,
                text: "use patch_file (NOT write_file) \
                       for any file listed in EXISTING FILES \
                       or PREVIOUS TASKS".into(),
            },
            // سياق البيئة
            Rule {
                kind: RuleKind::Context,
                text: "Python version is 3.12. \
                       Some older packages (passlib, crypt) \
                       are incompatible.".into(),
            },
            // v7.2.1: Session iterator bug — اكتُشف في POS Test Task 2
            Rule {
                kind: RuleKind::Never,
                text: "use next(get_session()) or next(get_db()). \
                       SQLAlchemy sessions are NOT iterators. \
                       Use: session = SessionLocal() directly, or \
                       Depends(get_db) in FastAPI endpoints.".into(),
            },
            // v7.2.1: on_event deprecated
            Rule {
                kind: RuleKind::Never,
                text: "use @app.on_event('startup'). \
                       Use lifespan context manager instead.".into(),
            },
            // v7.2.1: hashpw must have salt
            Rule {
                kind: RuleKind::Always,
                text: "pass TWO arguments to bcrypt.hashpw(): \
                       bcrypt.hashpw(password.encode('utf-8'), bcrypt.gensalt()). \
                       Never call hashpw() with only one argument.".into(),
            },
        ]
    }

    // ────────────────────────────────────────────────────────────
    // Planning Prompt
    // ────────────────────────────────────────────────────────────

    pub fn build_planning_prompt(
        &self,
        goal: &str,
        workspace_files: &[(String, String)],
        env_context: &str,
        feedback: Option<&str>,
    ) -> String {
        let mut prompt = String::with_capacity(8_000);

        // ١. القواعد — أولاً دائماً (~200 tokens)
        prompt.push_str(&self.rules_section());

        // ٢. تاريخ المهام السابقة (~100 tokens)
        prompt.push_str(&self.history_section());

        // ٣. البيئة (~50 tokens)
        if !env_context.is_empty() {
            prompt.push_str(env_context);
            prompt.push('\n');
        }

        // ٤. ملفات المشروع — تُقطع إذا تجاوزت الحد
        let budget = self.workspace_chars_budget();
        prompt.push_str(&self.workspace_section(workspace_files, budget));

        // ٥. الهدف
        prompt.push_str(&format!("\nGoal: {}\n", goal));

        // ٦. feedback من محاولة سابقة
        if let Some(fb) = feedback {
            prompt.push_str(&format!(
                "\nPrevious attempt failed:\n{}\nFix the issues.\n",
                fb
            ));
        }

        prompt.push_str("\nProvide the execution plan as JSON.\n");
        prompt
    }

    // ────────────────────────────────────────────────────────────
    // Repair Prompt — نفس القواعد + سياق الخطأ
    // ────────────────────────────────────────────────────────────

    pub fn build_repair_prompt(
        &self,
        goal: &str,
        errors: &str,
        files_context: &str,
        repair_hint: &str,
        attempt_note: &str,
        extra_notes: &[&str],
    ) -> String {
        let mut prompt = String::with_capacity(6_000);

        // القواعد أولاً — حتى في الـ repair
        prompt.push_str(&self.rules_section());

        // الهدف
        prompt.push_str(&format!("Goal: {}\n\n", goal));

        // hint محدد
        if !repair_hint.is_empty() {
            prompt.push_str(&format!("HINT: {}\n\n", repair_hint));
        }

        // معلومة المحاولة
        prompt.push_str(&format!("{}\n\n", attempt_note));

        // ملاحظات إضافية
        for note in extra_notes {
            if !note.is_empty() {
                prompt.push_str(note);
                prompt.push('\n');
            }
        }

        // الأخطاء
        prompt.push_str(&format!("FAILED STEPS:\n{}\n\n", errors));

        // الملفات الحالية
        prompt.push_str(&format!("CURRENT FILES:\n{}\n", files_context));

        prompt.push_str("Fix ALL issues. Provide complete corrected plan.");
        prompt
    }

    // ────────────────────────────────────────────────────────────
    // الأقسام الداخلية
    // ────────────────────────────────────────────────────────────

    fn rules_section(&self) -> String {
        let mut s = String::from(
            "=== RULES (mandatory — never violate) ===\n"
        );

        for rule in &self.rules {
            let prefix = match rule.kind {
                RuleKind::Never   => "NEVER",
                RuleKind::Always  => "ALWAYS",
                RuleKind::Prefer  => "PREFER",
                RuleKind::Context => "NOTE",
            };
            s.push_str(&format!("- {}: {}\n", prefix, rule.text));
        }

        s.push_str("=== END RULES ===\n\n");
        s
    }

    pub fn history_section(&self) -> String {
        if self.history.is_empty() {
            return String::new();
        }

        let mut s = String::from(
            "=== PREVIOUS TASKS IN THIS PLAN ===\n"
        );

        for task in &self.history {
            match &task.status {
                TaskStatus::Passed { repairs } => {
                    s.push_str(&format!(
                        "Task {}: ✅ Passed (repairs: {}) — {}\n  \
                         Files created: {}\n  \
                         → Use patch_file to modify these files.\n  \
                         → Do NOT rewrite them with write_file.\n\n",
                        task.index + 1,
                        repairs,
                        task.goal,
                        task.files_created.join(", ")
                    ));
                }
                TaskStatus::Failed { reason } => {
                    s.push_str(&format!(
                        "Task {}: ❌ Failed — {}\n  \
                         Reason: {}\n  \
                         → Do NOT repeat this mistake.\n\n",
                        task.index + 1,
                        task.goal,
                        reason
                    ));
                }
            }
        }

        s.push_str("=== END PREVIOUS TASKS ===\n\n");
        s
    }

    fn workspace_chars_budget(&self) -> usize {
        // 60% من حد النموذج — محافظ وآمن
        let token_limit: usize = if self.model.contains("kimi-k2") {
            8_000
        } else if self.model.contains("llama-3.3") {
            28_000
        } else if self.model.contains("gpt-4") {
            100_000
        } else if self.model.contains("qwen") {
            28_000
        } else if self.model.contains("gemma") {
            8_000
        } else {
            6_000  // حد محافظ للنماذج غير المعروفة
        };

        (token_limit * 60 / 100) * 4  // tokens → chars (1 token ≈ 4 chars)
    }

    fn workspace_section(
        &self,
        files: &[(String, String)],
        budget: usize,
    ) -> String {
        if files.is_empty() {
            return String::new();
        }

        let mut s = String::from(
            "=== EXISTING WORKSPACE FILES ===\n\
             CRITICAL: Use patch_file for ALL files listed below.\n\n"
        );
        let mut used = 0usize;
        let mut skipped = 0usize;

        for (name, content) in files {
            let lines: Vec<&str> = content.lines().collect();
            let preview: String = lines.iter()
                .take(200)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");

            let entry = format!(
                "--- FILE: {} ({} lines) ---\n{}\n\n",
                name, lines.len(), preview
            );

            if used + entry.len() > budget {
                s.push_str(&format!(
                    "--- FILE: {} [SKIPPED — context limit] ---\n",
                    name
                ));
                skipped += 1;
                continue;
            }

            s.push_str(&entry);
            used += entry.len();
        }

        if skipped > 0 {
            s.push_str(&format!(
                "⚠️  {} file(s) skipped — context limit reached.\n",
                skipped
            ));
        }

        s.push_str(&format!(
            "=== END FILES (~{} chars used) ===\n\n",
            used
        ));
        s
    }
}

// ────────────────────────────────────────────────────────────────
// CodeGate — فحص وإصلاح الكود قبل الكتابة
// ────────────────────────────────────────────────────────────────

pub struct CodeGate;

impl CodeGate {
    /// يفحص ويُصلح الكود تلقائياً قبل الكتابة على القرص
    pub fn pre_write_fix(filename: &str, content: &str) -> String {
        // فقط ملفات Python حالياً
        if !filename.ends_with(".py") {
            return content.to_string();
        }

        let mut lines: Vec<String> = content
            .lines()
            .map(|l| l.to_string())
            .collect();

        let mut fixes = 0u32;

        for line in lines.iter_mut() {
            let trimmed = line.trim().to_string();

            // passlib → bcrypt
            if trimmed.starts_with("from passlib")
                || trimmed.starts_with("import passlib")
            {
                let indent: String = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect();
                *line = format!(
                    "{}import bcrypt  \
                     # auto-fixed: passlib incompatible with Python 3.12",
                    indent
                );
                fixes += 1;
            }

            // crypt module → bcrypt
            if trimmed == "import crypt"
                || trimmed.starts_with("from crypt ")
            {
                let indent: String = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect();
                *line = format!(
                    "{}import bcrypt  \
                     # auto-fixed: crypt deprecated in Python 3.13",
                    indent
                );
                fixes += 1;
            }
        }

        let mut result = lines.join("\n");

        // إصلاح CryptContext patterns
        let replacements: &[(&str, &str)] = &[
            (
                "CryptContext(schemes=[\"bcrypt\"], deprecated=\"auto\")",
                "None  # auto-fixed: use bcrypt directly",
            ),
            (
                "CryptContext(schemes=['bcrypt'], deprecated='auto')",
                "None  # auto-fixed: use bcrypt directly",
            ),
            (
                "CryptContext(schemes=[\"bcrypt\"])",
                "None  # auto-fixed: use bcrypt directly",
            ),
            (
                "CryptContext(schemes=['bcrypt'])",
                "None  # auto-fixed: use bcrypt directly",
            ),
            (
                "pwd_context.verify(",
                "bcrypt.checkpw(",
            ),
            (
                "pwd_context.hash(",
                "bcrypt.hashpw(",
            ),
        ];

        for (old, new) in replacements {
            if result.contains(old) {
                result = result.replace(old, new);
                fixes += 1;
            }
        }

        // v7.2.1: Fix hashpw() missing salt argument
        let hashpw_vars = ["password", "pwd", "pw", "raw_password", "plain_password"];
        for var in &hashpw_vars {
            let bad = format!("bcrypt.hashpw({})", var);
            if result.contains(&bad) {
                let good = format!(
                    "bcrypt.hashpw({var}.encode(\'utf-8\') if isinstance({var}, str) else {var}, bcrypt.gensalt())"
                );
                result = result.replace(&bad, &good);
                fixes += 1;
            }
        }

        // v7.2.1: Fix checkpw() missing encode
        let checkpw_vars = ["password", "pwd", "pw"];
        for var in &checkpw_vars {
            let bad = format!("bcrypt.checkpw({}, ", var);
            if result.contains(&bad) {
                let good = format!(
                    "bcrypt.checkpw({var}.encode(\'utf-8\') if isinstance({var}, str) else {var}, "
                );
                result = result.replace(&bad, &good);
                fixes += 1;
            }
        }

        // ─────────────────────────────────────────────────
        // v7.2.3: Fix datetime.timezone.utc usage
        // المشكلة: from datetime import datetime
        //          ثم: datetime.now(datetime.timezone.utc)
        //          هذا خطأ — datetime هنا class وليس module
        // الحل:    استخدم timezone مستوردة مباشرة
        // ─────────────────────────────────────────────────
        let datetime_patterns = [
            // Pattern 1: lambda في Column — كل الأشكال
            (
                "default=lambda: datetime.now(datetime.timezone.utc)",
                "default=lambda: datetime.now(timezone.utc)",
            ),
            (
                "default=lambda: datetime.now(datetime.UTC)",
                "default=lambda: datetime.now(timezone.utc)",
            ),
            (
                "default=lambda: datetime.utcnow()",
                "default=lambda: datetime.now(timezone.utc)",
            ),
            // Pattern 2: استدعاء مباشر — كل الأشكال
            (
                "datetime.now(datetime.UTC)",
                "datetime.now(timezone.utc)",
            ),
            (
                "datetime.now(datetime.timezone.utc)",
                "datetime.now(timezone.utc)",
            ),
            (
                "datetime.now(datetime.UTC)",
                "datetime.now(timezone.utc)",
            ),
            // Pattern 3: utcnow deprecated
            (
                "datetime.utcnow()",
                "datetime.now(timezone.utc)",
            ),
        ];

        for (old_pat, new_pat) in &datetime_patterns {
            if result.contains(old_pat) {
                result = result.replace(old_pat, new_pat);
                fixes += 1;
            }
        }

        // تأكد أن timezone مستوردة — كل الحالات
        if result.contains("timezone.utc") && !result.contains(", timezone") && !result.contains("import timezone") {
            if result.contains("from datetime import datetime") {
                // from datetime import datetime → from datetime import datetime, timezone
                result = result.replacen(
                    "from datetime import datetime",
                    "from datetime import datetime, timezone",
                    1,
                );
                fixes += 1;
            } else if result.contains("from datetime import ") {
                // from datetime import X, Y → أضف timezone
                let old_line = result.lines()
                    .find(|l| l.trim().starts_with("from datetime import "))
                    .unwrap_or("")
                    .to_string();
                if !old_line.is_empty() && !old_line.contains("timezone") {
                    let new_line = format!("{}, timezone", old_line.trim_end());
                    result = result.replacen(&old_line, &new_line, 1);
                    fixes += 1;
                }
            } else if result.contains("import datetime") {
                // import datetime → import datetime + from datetime import timezone
                result = result.replacen(
                    "import datetime",
                    "import datetime
from datetime import timezone",
                    1,
                );
                fixes += 1;
            } else {
                // لا يوجد أي import — أضف في البداية
                result = format!("from datetime import datetime, timezone
{}", result);
                fixes += 1;
            }
        }

        // ─────────────────────────────────────────────────
        // v7.2.3: Fix relative imports في scripts
        // المشكلة: from . import models ← خطأ في script مباشر
        // الحل:    import models
        // ─────────────────────────────────────────────────
        if result.contains("from . import ") {
            result = result.replace("from . import ", "import ");
            fixes += 1;
        }
        if result.contains("from .") && result.contains(" import ") {
            // from .models import User → from models import User
            let re_result = result.clone();
            for line in re_result.lines() {
                if line.trim().starts_with("from .") && line.contains(" import ") {
                    let fixed = line.replacen("from .", "from ", 1);
                    result = result.replace(line, &fixed);
                    fixes += 1;
                }
            }
        }

        // ─────────────────────────────────────────────────
        // v7.2.3: Fix SQLAlchemy declarative_base warning
        // ─────────────────────────────────────────────────
        if result.contains("from sqlalchemy.ext.declarative import declarative_base") {
            result = result.replace(
                "from sqlalchemy.ext.declarative import declarative_base",
                "from sqlalchemy.orm import declarative_base",
            );
            fixes += 1;
        }

        // ─────────────────────────────────────────────────
        // v7.2.3: Fix FastAPI imports mixed with datetime
        // المشكلة: from datetime import timedelta, Depends, HTTPException
        // الحل:    فصل الاستيرادات
        // ─────────────────────────────────────────────────
        let bad_datetime_imports = [
            "from datetime import timedelta, Depends",
            "from datetime import datetime, Depends",
            "from datetime import Depends",
        ];
        for bad in &bad_datetime_imports {
            if result.contains(bad) {
                // استبدل بالاستيرادات الصحيحة
                result = result.replace(
                    bad,
                    "from datetime import datetime, timedelta, timezone
from fastapi import Depends",
                );
                fixes += 1;
            }
        }

        if fixes > 0 {
            println!(
                "   🔧 CodeGate: auto-fixed {} issue{} in {}",
                fixes,
                if fixes > 1 { "s" } else { "" },
                filename
            );
        }

        result
    }
}

// ────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rules_section_contains_passlib() {
        let engine = PromptEngine::new("llama-3.3".into());
        let rules = engine.rules_section();
        assert!(rules.contains("passlib"));
        assert!(rules.contains("bcrypt"));
    }

    #[test]
    fn test_history_section_empty() {
        let engine = PromptEngine::new("llama-3.3".into());
        assert!(engine.history_section().is_empty());
    }

    #[test]
    fn test_history_section_with_tasks() {
        let mut engine = PromptEngine::new("llama-3.3".into());
        engine.set_history(vec![
            TaskResult {
                index: 0,
                goal: "Create models".into(),
                status: TaskStatus::Passed { repairs: 0 },
                files_created: vec!["models.py".into()],
            },
        ]);
        let history = engine.history_section();
        assert!(history.contains("models.py"));
        assert!(history.contains("patch_file"));
    }

    #[test]
    fn test_workspace_budget_kimi() {
        let engine = PromptEngine::new("kimi-k2-instruct".into());
        let budget = engine.workspace_chars_budget();
        // 8000 * 60% * 4 = 19200
        assert_eq!(budget, 19_200);
    }

    #[test]
    fn test_workspace_budget_llama() {
        let engine = PromptEngine::new("llama-3.3-70b".into());
        let budget = engine.workspace_chars_budget();
        // 28000 * 60% * 4 = 67200
        assert_eq!(budget, 67_200);
    }

    #[test]
    fn test_workspace_section_truncates() {
        let engine = PromptEngine::new("kimi-k2".into());
        // ملف كبير جداً
        let big_content = "x".repeat(100_000);
        let files = vec![
            ("file1.py".into(), "small content".into()),
            ("file2.py".into(), big_content.clone()),
            ("file3.py".into(), big_content),
        ];
        let section = engine.workspace_section(&files, 1_000);
        assert!(section.contains("SKIPPED"));
    }

    #[test]
    fn test_codegate_blocks_passlib() {
        let content = "from passlib.context import CryptContext\n\
                       def hash_pw(pw):\n    return pwd.hash(pw)\n";
        let fixed = CodeGate::pre_write_fix("auth.py", content);
        assert!(!fixed.contains("from passlib"));
        assert!(fixed.contains("bcrypt"));
    }

    #[test]
    fn test_codegate_blocks_import_passlib() {
        let content = "import passlib\nfrom passlib.hash import bcrypt_sha256\n";
        let fixed = CodeGate::pre_write_fix("auth.py", content);
        assert!(!fixed.contains("import passlib\n"));
        assert!(!fixed.contains("from passlib"));
    }

    #[test]
    fn test_codegate_preserves_indent() {
        let content = "def setup():\n    from passlib.context import CryptContext\n";
        let fixed = CodeGate::pre_write_fix("auth.py", content);
        // يجب أن يحافظ على المسافة البادئة
        assert!(fixed.contains("    import bcrypt"));
    }

    #[test]
    fn test_codegate_skips_non_python() {
        let content = "from passlib.context import CryptContext";
        let fixed = CodeGate::pre_write_fix("auth.ts", content);
        // TypeScript — لا تعديل
        assert_eq!(fixed, content);
    }

    #[test]
    fn test_repair_prompt_contains_rules() {
        let engine = PromptEngine::new("llama-3.3".into());
        let prompt = engine.build_repair_prompt(
            "Create auth",
            "ModuleNotFoundError: passlib",
            "",
            "Use bcrypt",
            "Attempt 1/3",
            &[],
        );
        assert!(prompt.contains("NEVER"));
        assert!(prompt.contains("passlib"));
    }
}
