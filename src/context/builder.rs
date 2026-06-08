// src/context/builder.rs
use crate::chunker::{
    extract_error_locations, get_file_content_smart, SmartContent, MAX_FILE_LINES,
};
use crate::dependency_graph;
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
    pub workspace: Option<PathBuf>,
    pub dependency_graph: Option<crate::dependency_graph::DependencyGraph>,
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
            workspace: None,
            dependency_graph: None,
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

pub fn collect_workspace_files(workspace: &Path) -> Vec<PathBuf> {
    let supported = ["ts", "js", "py", "go", "rs", "toml", "json", "mod"];

    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(workspace)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.is_file())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            supported.contains(&ext)
        })
        .filter(|p| {
            let s = p.to_string_lossy();
            !s.contains("/node_modules/")
                && !s.contains("/venv/")
                && !s.contains("/.venv/")
                && !s.contains("/dist/")
                && !s.contains("/target/")
                && !s.contains("/.git/")
                && !s.contains("/__pycache__/")
                && !s.contains("package-lock")
        })
        .collect();

    files.sort();
    files
}

pub fn build_repair_context_block(workspace: &Path, ctx: &RepairContext) -> String {
    let workspace_files = collect_workspace_files(workspace);
    if workspace_files.is_empty() {
        return String::new();
    }

    let (selected, budget) = select_repair_files(&workspace_files, ctx);
    if selected.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("=== SELECTED REPAIR CONTEXT FILES ===\n");
    out.push_str(&format!(
        "Budget: {} total files, {} selected, {} -> {} tokens (-{}%)\n\n",
        budget.total_files,
        budget.selected_files,
        budget.tokens_before,
        budget.tokens_after,
        budget.reduction_pct()
    ));

    for file in &selected {
        let rel = file
            .path
            .strip_prefix(workspace)
            .unwrap_or(&file.path)
            .to_string_lossy();
        let reasons = if file.reasons.is_empty() {
            "none".to_string()
        } else {
            file.reasons.join(", ")
        };
        out.push_str(&format!(
            "--- FILE: {} [score={}, reasons={}] ---\n{}\n\n",
            rel, file.score, reasons, file.content
        ));
    }

    out.push_str("=== END SELECTED REPAIR CONTEXT FILES ===\n\n");
    out
}

pub fn select_repair_files(
    workspace_files: &[PathBuf],
    ctx: &RepairContext,
) -> (Vec<ScoredFile>, BudgetReport) {
    let graph = ctx.dependency_graph.clone().or_else(|| {
        ctx.workspace
            .as_ref()
            .map(|ws| dependency_graph::builder::build_for_workspace(ws))
    });
    let cycles = graph
        .as_ref()
        .map(|g| g.detect_cycles())
        .unwrap_or_default();

    let mut scored: Vec<ScoredFile> = workspace_files
        .iter()
        .filter_map(|path| read_and_score(path, ctx, graph.as_ref(), &cycles))
        .collect();
    scored.sort_by_key(|b| std::cmp::Reverse(b.score));

    let total_files = scored.len();
    let tokens_before = scored.iter().map(|f| estimate_tokens(&f.content)).sum();

    let max_files = ctx
        .context_config
        .as_ref()
        .map(|c| c.max_context_files)
        .unwrap_or(MAX_CONTEXT_FILES)
        .min(MAX_CONTEXT_FILES);

    let mut selected = vec![];
    let mut tokens_after = 0usize;

    for file in scored {
        if file.score < MIN_SCORE {
            break;
        }
        if selected.len() >= max_files {
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

fn read_and_score(
    path: &Path,
    ctx: &RepairContext,
    graph: Option<&crate::dependency_graph::DependencyGraph>,
    cycles: &[Vec<PathBuf>],
) -> Option<ScoredFile> {
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

    let (score, reasons) = compute_score(path, &content, ctx, graph, cycles);
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
    graph: Option<&crate::dependency_graph::DependencyGraph>,
    cycles: &[Vec<PathBuf>],
) -> (u8, Vec<String>) {
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

    if let Some(graph) = graph {
        if graph.node_count() > 0 {
            for culprit_name in &ctx.culprit_files {
                let culprit_candidates: Vec<_> = graph
                    .nodes
                    .keys()
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n == culprit_name.as_str())
                            .unwrap_or(false)
                    })
                    .collect();

                for culprit_path in &culprit_candidates {
                    let deps = graph.dependencies_of(culprit_path);
                    if deps.iter().any(|d| d == path) {
                        score += 4;
                        reasons.push("graph: dependency of culprit".to_string());
                    }

                    let impacted = graph.impacted_by(path);
                    if impacted.iter().any(|i| i == *culprit_path) {
                        score += 3;
                        reasons.push("graph: impacts culprit".to_string());
                    }
                }
            }

            for cycle in cycles {
                if cycle.iter().any(|c| c == path) {
                    score += 2;
                    reasons.push("graph: in dependency cycle".to_string());
                    break;
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn test_build_repair_context_block_includes_culprit_file() {
        let dir = setup(&[
            ("main.py", "from utils import helper\nprint(helper())\n"),
            ("utils.py", "def helper():\n    return 1\n"),
        ]);

        let ctx = RepairContext {
            stderr: "NameError in main.py".to_string(),
            force_include: vec![dir.path().join("main.py")],
            culprit_files: vec!["main.py".to_string()],
            context_config: Some(crate::types::ContextConfig::default()),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let out = build_repair_context_block(dir.path(), &ctx);
        assert!(out.contains("main.py"));
        assert!(out.contains("culprit file") || out.contains("force_include"));
    }

    #[test]
    fn test_select_repair_files_respects_max_context_files() {
        let dir = setup(&[
            ("a.py", "print('a')\n"),
            ("b.py", "print('b')\n"),
            ("c.py", "print('c')\n"),
        ]);

        let files = collect_workspace_files(dir.path());
        let ctx = RepairContext {
            context_config: Some(crate::types::ContextConfig {
                ref_file: None,
                focus_paths: vec![".py".to_string()],
                max_context_files: 1,
            }),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let (selected, _) = select_repair_files(&files, &ctx);
        assert_eq!(selected.len(), 1);
    }
}
