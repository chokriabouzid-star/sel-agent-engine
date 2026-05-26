// src/context/builder.rs
use crate::chunker::{
    extract_error_locations, get_file_content_smart, SmartContent, MAX_FILE_LINES,
};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_REPAIR_TOKENS: usize = 8_000;
pub const MAX_CONTEXT_FILES: usize = 50;
const CHARS_PER_TOKEN: usize = 4;
const MIN_SCORE: u8 = 2;
const SMALL_FILE_LINES: usize = 200;

#[derive(Debug, Clone)]
pub struct ScoredFile {
    pub path: PathBuf,
    pub content: String,
    pub score: u8,
    pub reasons: Vec<String>,
}

pub struct RepairContext {
    pub stderr: String,
    pub recent_edits: Vec<PathBuf>,
    pub max_tokens: usize,
    pub force_include: Vec<PathBuf>,
    pub culprit_files: Vec<String>,
    pub context_config: Option<crate::types::ContextConfig>,
}

impl Default for RepairContext {
    fn default() -> Self {
        Self {
            stderr: String::new(),
            recent_edits: vec![],
            max_tokens: MAX_REPAIR_TOKENS,
            force_include: vec![],
            culprit_files: vec![],
            context_config: None,
        }
    }
}

#[derive(Debug)]
pub struct BudgetReport {
    pub total_files: usize,
    pub selected_files: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
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
        println!("\n Context Budget:");
        println!(
            "  Files:  {} total  {} selected",
            self.total_files, self.selected_files
        );
        println!(
            "  Tokens: {}  {} (-{}%)",
            self.tokens_before,
            self.tokens_after,
            self.reduction_pct()
        );
    }
}

pub fn select_repair_files(
    workspace_files: &[PathBuf],
    ctx: &RepairContext,
) -> (Vec<ScoredFile>, BudgetReport) {
    let mut scored: Vec<ScoredFile> = workspace_files
        .iter()
        .filter_map(|path| read_and_score(path, ctx))
        .collect();
    scored.sort_by(|a, b| b.score.cmp(&a.score));

    let total_files = scored.len();
    let tokens_before = scored.iter().map(|f| estimate_tokens(&f.content)).sum();

    let mut selected = vec![];
    let mut tokens_after = 0usize;

    for file in scored {
        if file.score < MIN_SCORE {
            break;
        }
        let file_tokens = estimate_tokens(&file.content);
        if tokens_after + file_tokens > ctx.max_tokens {
            break;
        }
        tokens_after += file_tokens;
        selected.push(file);
    }

    for path in &ctx.force_include {
        if !selected.iter().any(|s| &s.path == path) {
            if let Ok(content) = std::fs::read_to_string(path) {
                let tokens = estimate_tokens(&content);
                if tokens_after + tokens <= ctx.max_tokens {
                    tokens_after += tokens;
                    selected.push(ScoredFile {
                        path: path.clone(),
                        content,
                        score: 0,
                        reasons: vec!["force_include".to_string()],
                    });
                }
            }
        }
    }

    let selected_files = selected.len();
    (
        selected,
        BudgetReport {
            total_files,
            selected_files,
            tokens_before,
            tokens_after,
        },
    )
}

fn read_and_score(path: &Path, ctx: &RepairContext) -> Option<ScoredFile> {
    let raw = fs::read_to_string(path).ok()?;
    let line_count = raw.lines().count();
    let content = if line_count > MAX_FILE_LINES && !ctx.stderr.is_empty() {
        let locs = extract_error_locations(&ctx.stderr);
        if !locs.is_empty() {
            match get_file_content_smart(path, &locs) {
                Ok(SmartContent::Chunk(chunk)) => format!(
                    "//  CHUNKED: {} ({} lines, showing {}-{})\n{}",
                    path.display(),
                    line_count,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content
                ),
                Ok(SmartContent::FullFile(c)) => c,
                Err(_) => raw,
            }
        } else {
            raw
        }
    } else {
        raw
    };

    let (score, reasons) = compute_score(path, &content, ctx);
    Some(ScoredFile {
        path: path.to_path_buf(),
        content,
        score,
        reasons,
    })
}

fn compute_score(path: &Path, content: &str, ctx: &RepairContext) -> (u8, Vec<String>) {
    let mut score = 0u8;
    let mut reasons = vec![];
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    if let Some(ref config) = ctx.context_config {
        if config
            .focus_paths
            .iter()
            .any(|fp| path.to_string_lossy().contains(fp))
        {
            score += 10;
            reasons.push("focus path".to_string());
        }
    }

    if ctx.culprit_files.iter().any(|c| c == filename) {
        score += 8;
        reasons.push("culprit file".to_string());
    }
    if ctx.stderr.contains(filename) {
        score += 5;
        reasons.push("mentioned in error".to_string());
    }
    if ctx.recent_edits.contains(&path.to_path_buf()) {
        score += 3;
        reasons.push("recently edited".to_string());
    }
    if imports_errored_file(content, &ctx.stderr) {
        score += 2;
        reasons.push("imports errored file".to_string());
    }
    if content.lines().count() < SMALL_FILE_LINES {
        score += 1;
        reasons.push("small file".to_string());
    }

    (score, reasons)
}

fn imports_errored_file(content: &str, stderr: &str) -> bool {
    let errored_stems: Vec<&str> = stderr
        .split_whitespace()
        .filter(|w| w.contains('.'))
        .filter_map(|w| {
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '.');
            clean.split('.').next()
        })
        .collect();

    for stem in &errored_stems {
        for line in content.lines().take(30) {
            let line = line.trim();
            if (line.starts_with("use ")
                || line.starts_with("import ")
                || line.starts_with("from ")
                || line.contains("require("))
                && line.contains(stem)
            {
                return true;
            }
        }
    }
    false
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(CHARS_PER_TOKEN)
}

pub fn read_ref_file(ref_file: &Path) -> Option<String> {
    match std::fs::read_to_string(ref_file) {
        Ok(content) => {
            println!(
                " Loaded ref file: {} ({} lines)",
                ref_file.display(),
                content.lines().count()
            );
            Some(content)
        }
        Err(e) => {
            eprintln!("  Failed to read ref file: {}", e);
            None
        }
    }
}
