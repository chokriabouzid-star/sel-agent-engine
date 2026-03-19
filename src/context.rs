// ─────────────────────────────────────────────
// src/context.rs — SEL Agent v1.3
// Context Budget Engine
// ─────────────────────────────────────────────

use std::fs;
use std::path::{Path, PathBuf};

// ─── الثوابت ───────────────────────────────────

pub const MAX_REPAIR_TOKENS: usize = 8_000;
pub const MAX_CONTEXT_FILES: usize = 50;  // v5.1: رفع من 20 إلى 50
const CHARS_PER_TOKEN:       usize = 4;
const MIN_SCORE:             u8    = 2;
const SMALL_FILE_LINES:      usize = 200;

// ─── الأنواع ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScoredFile {
    pub path:    PathBuf,
    pub content: String,
    pub score:   u8,
    pub reasons: Vec<String>,
}

pub struct RepairContext {
    pub stderr:        String,
    pub recent_edits:  Vec<PathBuf>,
    pub max_tokens:    usize,
    pub force_include: Vec<PathBuf>,
    pub culprit_files: Vec<String>,   // الملفات المسبّبة للخطأ — أعلى أولوية
    pub context_config: Option<crate::types::ContextConfig>,
}

impl Default for RepairContext {
    fn default() -> Self {
        Self {
            stderr:        String::new(),
            recent_edits:  vec![],
            max_tokens:    MAX_REPAIR_TOKENS,
            force_include: vec![],
            culprit_files: vec![],
            context_config: None,
        }
    }
}

#[derive(Debug)]
pub struct BudgetReport {
    pub total_files:    usize,
    pub selected_files: usize,
    pub tokens_before:  usize,
    pub tokens_after:   usize,
}

impl BudgetReport {
    pub fn reduction_pct(&self) -> u8 {
        if self.tokens_before == 0 {
            return 0;
        }
        let saved = self.tokens_before.saturating_sub(self.tokens_after);
        ((saved * 100) / self.tokens_before) as u8
    }

    pub fn print(&self) {
        println!("\n📊 Context Budget:");
        println!(
            "  Files:  {} total → {} selected",
            self.total_files, self.selected_files
        );
        println!(
            "  Tokens: {} → {} (-{}%)",
            self.tokens_before,
            self.tokens_after,
            self.reduction_pct()
        );
    }
}

// ─── الدالة الرئيسية ───────────────────────────

pub fn select_repair_files(
    workspace_files: &[PathBuf],
    ctx: &RepairContext,
) -> (Vec<ScoredFile>, BudgetReport) {

    // 1. اقرأ الملفات وصنفها
    let mut scored: Vec<ScoredFile> = workspace_files
        .iter()
        .filter_map(|path| read_and_score(path, ctx))
        .collect();

    // 2. رتب تنازلياً حسب الـ score
    scored.sort_by(|a, b| b.score.cmp(&a.score));

    let total_files  = scored.len();
    let tokens_before = scored
        .iter()
        .map(|f| estimate_tokens(&f.content))
        .sum();

    // 3. اختر ضمن حد الـ tokens
    let mut selected     = vec![];
    let mut tokens_after = 0usize;

    for file in scored {
        if file.score < MIN_SCORE {
            break; // مرتبة تنازلياً — ما بعدها أقل
        }
        let file_tokens = estimate_tokens(&file.content);
        if tokens_after + file_tokens > ctx.max_tokens {
            break;
        }
        tokens_after += file_tokens;
        selected.push(file);
    }

    // أضف force_include التي لم تُختر بعد
    for path in &ctx.force_include {
        let already = selected.iter().any(|s| &s.path == path);
        if !already {
            if let Some(content) = std::fs::read_to_string(path).ok() {
                let tokens = estimate_tokens(&content);
                if tokens_after + tokens <= ctx.max_tokens {
                    tokens_after += tokens;
                    selected.push(ScoredFile {
                        path:    path.clone(),
                        content,
                        score:   0,
                        reasons: vec!["force_include".to_string()],
                    });
                }
            }
        }
    }

    let report = BudgetReport {
        total_files,
        selected_files: selected.len(),
        tokens_before,
        tokens_after,
    };

    (selected, report)
}

// ─── Scoring ───────────────────────────────────

fn read_and_score(path: &Path, ctx: &RepairContext) -> Option<ScoredFile> {
    let content = fs::read_to_string(path).ok()?;
    let (score, reasons) = compute_score(path, &content, ctx);

    Some(ScoredFile {
        path: path.to_path_buf(),
        content,
        score,
        reasons,
    })
}

fn compute_score(
    path: &Path,
    content: &str,
    ctx: &RepairContext,
) -> (u8, Vec<String>) {
    let mut score   = 0u8;
    let mut reasons = vec![];

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // +10 ملف في focus_paths (v5.1)
    if let Some(ref config) = ctx.context_config {
        if config.focus_paths.iter().any(|fp| path.to_string_lossy().contains(fp)) {
            score += 10;
            reasons.push("focus path".to_string());
        }
    }

    // +8 ملف مسبّب مباشر (multi-file repair memory)
    if ctx.culprit_files.iter().any(|c| c == filename) {
        score += 8;
        reasons.push("culprit file".to_string());
    }
    // +5 مذكور في stderr
    if ctx.stderr.contains(filename) {
        score += 5;
        reasons.push("mentioned in error".to_string());
    }

    // +3 عُدّل في آخر attempt
    if ctx.recent_edits.contains(&path.to_path_buf()) {
        score += 3;
        reasons.push("recently edited".to_string());
    }

    // +2 يستورد ملفاً فيه خطأ
    if imports_errored_file(content, &ctx.stderr) {
        score += 2;
        reasons.push("imports errored file".to_string());
    }

    // +1 ملف صغير
    if content.lines().count() < SMALL_FILE_LINES {
        score += 1;
        reasons.push("small file".to_string());
    }

    (score, reasons)
}

fn imports_errored_file(content: &str, stderr: &str) -> bool {
    // استخرج أسماء الملفات من stderr
    let errored_stems: Vec<&str> = stderr
        .split_whitespace()
        .filter(|w| w.contains('.'))
        .filter_map(|w| {
            let clean = w.trim_matches(|c: char| {
                !c.is_alphanumeric() && c != '_' && c != '.'
            });
            clean.split('.').next()
        })
        .collect();

    // فحص أسطر الاستيراد في أول 30 سطر
    for stem in &errored_stems {
        for line in content.lines().take(30) {
            let line = line.trim();
            let is_import = line.starts_with("use ")
                || line.starts_with("import ")
                || line.starts_with("from ")
                || line.contains("require(");

            if is_import && line.contains(stem) {
                return true;
            }
        }
    }
    false
}

// ─── Token Estimation ──────────────────────────

pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN
}

// ─── Tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(stderr: &str) -> RepairContext {
        RepairContext {
            stderr:        stderr.to_string(),
            force_include: vec![],
            ..Default::default()
        }
    }

    #[test]
    fn test_mentioned_in_error() {
        let ctx = make_ctx("Error in main.rs:42: undefined variable");
        let (score, reasons) = compute_score(
            Path::new("main.rs"),
            "fn main() {}",
            &ctx,
        );
        assert_eq!(score, 6); // 5 + 1 (small)
        assert!(reasons.iter().any(|r| r == "mentioned in error"));
    }

    #[test]
    fn test_unrelated_file() {
        let ctx = make_ctx("Error in main.rs:42");
        let (score, _) = compute_score(
            Path::new("utils.rs"),
            "pub fn helper() {}",
            &ctx,
        );
        assert_eq!(score, 1); // فقط small file
    }

    #[test]
    fn test_recently_edited() {
        let path = PathBuf::from("app.py");
        let ctx = RepairContext {
            stderr:        "SyntaxError in db.py".to_string(),
            recent_edits:  vec![path.clone()],
            max_tokens:    MAX_REPAIR_TOKENS,
            force_include: vec![],
            culprit_files: vec![],
            context_config: None,
        };
        let (score, reasons) = compute_score(&path, "x = 1", &ctx);
        assert_eq!(score, 4); // 3 + 1 (small)
        assert!(reasons.iter().any(|r| r == "recently edited"));
    }

    #[test]
    fn test_python_import_detection() {
        let content = "from db import Database\nclass App:\n    pass";
        assert!(imports_errored_file(content, "Error in db.py:10"));
    }

    #[test]
    fn test_rust_use_detection() {
        let content = "use crate::db;\n\nfn main() {}";
        assert!(imports_errored_file(content, "error in db.rs:5"));
    }

    #[test]
    fn test_token_estimation() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn test_budget_report_reduction() {
        let report = BudgetReport {
            total_files:    10,
            selected_files: 3,
            tokens_before:  8000,
            tokens_after:   1600,
        };
        assert_eq!(report.reduction_pct(), 80);
    }
}

// ─── v5.1: Reference File Support ──────────────

pub fn read_ref_file(ref_file: &Path) -> Option<String> {
    match std::fs::read_to_string(ref_file) {
        Ok(content) => {
            println!("📄 Loaded ref file: {} ({} lines)", 
                ref_file.display(), 
                content.lines().count());
            Some(content)
        }
        Err(e) => {
            eprintln!("⚠️  Failed to read ref file: {}", e);
            None
        }
    }
}
