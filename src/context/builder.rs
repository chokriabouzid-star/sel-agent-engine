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
    pub force_include_dropped: Vec<String>, // v9.3.0: force_include files that failed to load
}

impl BudgetReport {
    pub fn reduction_pct(&self) -> u8 {
        if self.tokens_before == 0 {
            return 0;
        }
        let saved = self.tokens_before.saturating_sub(self.tokens_after);
        ((saved * 100) / self.tokens_before) as u8
    }

    #[allow(dead_code)]
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

pub fn build_repair_context_block(workspace: &Path, ctx: &RepairContext) -> (String, BudgetReport) {
    let workspace_files = collect_workspace_files(workspace);
    if workspace_files.is_empty() {
        return (
            String::new(),
            BudgetReport {
                total_files: 0,
                selected_files: 0,
                tokens_before: 0,
                tokens_after: 0,
                force_include_dropped: vec![],
            },
        );
    }

    let (selected, budget) = select_repair_files(&workspace_files, ctx);
    if selected.is_empty() {
        return (String::new(), budget);
    }
    // v9.3.0: warn if any force_include files were dropped
    if !budget.force_include_dropped.is_empty() {
        eprintln!(
            "[WARN] {} force_include file(s) could not be loaded: {:?}",
            budget.force_include_dropped.len(),
            budget.force_include_dropped
        );
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
    (out, budget)
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

    // Force-include files must NEVER be dropped silently.
    // If budget is tight, we truncate them rather than omit them.
    const FORCE_INCLUDE_MAX_CHARS: usize = 4_000;

    let truncate_force_include = |content: &str| -> String {
        if content.chars().count() <= FORCE_INCLUDE_MAX_CHARS {
            return content.to_string();
        }
        let mut out: String = content.chars().take(FORCE_INCLUDE_MAX_CHARS).collect();
        out.push_str("\n... [truncated: force_include file too large for repair budget]");
        out
    };

    let force_paths: std::collections::HashSet<PathBuf> =
        ctx.force_include.iter().cloned().collect();

    let mut selected: Vec<ScoredFile> = Vec::new();
    let mut deferred: Vec<ScoredFile> = Vec::new();
    let mut tokens_after = 0usize;

    // Phase 1: reserve force_include files first, using scored/chunked content when available.
    for mut file in scored {
        if force_paths.contains(&file.path) {
            if !file.reasons.iter().any(|r| r == "force_include") {
                file.reasons.insert(0, "force_include".to_string());
            }

            let original_chars = file.content.chars().count();
            if original_chars > FORCE_INCLUDE_MAX_CHARS {
                file.content = truncate_force_include(&file.content);
                eprintln!(
                    "[TRACE] force_include truncated: {} ({} chars -> {} chars)",
                    file.path.display(),
                    original_chars,
                    file.content.chars().count()
                );
            }

            tokens_after += estimate_tokens(&file.content);
            selected.push(file);
        } else {
            deferred.push(file);
        }
    }

    // Phase 1b: if a force_include file was not in workspace_files/scored set,
    // still include it directly from disk.
    let mut force_include_dropped: Vec<String> = Vec::new();
    for path in &ctx.force_include {
        if selected.iter().any(|s| &s.path == path) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            let content = truncate_force_include(&content);
            tokens_after += estimate_tokens(&content);
            selected.push(ScoredFile {
                path: path.clone(),
                content,
                score: u8::MAX,
                reasons: vec!["force_include".to_string()],
            });
            eprintln!(
                "[TRACE] force_include added from disk fallback: {}",
                path.display()
            );
        } else {
            // v9.3.0: track force_include files that could not be loaded
            let dropped_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>")
                .to_string();
            eprintln!(
                "[WARN] force_include file not found or unreadable: {}",
                path.display()
            );
            force_include_dropped.push(dropped_name);
        }
    }

    if tokens_after > ctx.max_tokens {
        eprintln!(
            "[TRACE] force_include files exceed repair token budget: reserved={} budget={}",
            tokens_after, ctx.max_tokens
        );
    }

    let force_tokens = tokens_after;
    let remaining_tokens = ctx.max_tokens.saturating_sub(force_tokens);
    let extra_file_slots = max_files.saturating_sub(selected.len());

    // Phase 2: fill remaining budget with best scored files.
    for (extra_selected, file) in deferred.into_iter().enumerate() {
        if file.score < MIN_SCORE {
            break;
        }
        if extra_selected >= extra_file_slots {
            break;
        }
        let file_tokens = estimate_tokens(&file.content);
        if tokens_after.saturating_sub(force_tokens) + file_tokens > remaining_tokens {
            break;
        }
        tokens_after += file_tokens;
        selected.push(file);
    }

    let selected_files = selected.len();
    (
        selected,
        BudgetReport {
            total_files,
            selected_files,
            tokens_before,
            tokens_after,
            force_include_dropped,
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
        // v9.3.0: extra weight if filename appears multiple times in stderr
        let occurrences = ctx.stderr.matches(filename).count();
        if occurrences > 1 {
            let extra = ((occurrences - 1).min(3) * 2) as u8;
            score = score.saturating_add(extra);
            reasons.push(format!("stderr occurrences: {}", occurrences));
        }
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

        let (out, _budget) = build_repair_context_block(dir.path(), &ctx);
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

    #[test]
    fn test_select_repair_files_force_include_survives_budget() {
        let dir = setup(&[
            ("big.py", &"x".repeat(6_000)),
            ("small.py", "print('ok')\n"),
        ]);

        let files = collect_workspace_files(dir.path());
        let ctx = RepairContext {
            stderr: "Error in big.py".to_string(),
            force_include: vec![dir.path().join("big.py")],
            culprit_files: vec!["big.py".to_string()],
            max_tokens: 10,
            context_config: Some(crate::types::ContextConfig {
                ref_file: None,
                focus_paths: vec![],
                max_context_files: 1,
            }),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let (selected, budget) = select_repair_files(&files, &ctx);
        assert!(selected.iter().any(|f| f.path.ends_with("big.py")));
        assert!(selected
            .iter()
            .any(|f| f.reasons.iter().any(|r| r == "force_include")));
        assert!(budget.selected_files >= 1);
    }

    // ═══ Evidence Tests (Wave 1) ═══

    #[test]
    fn evidence_smart_context_prioritizes_culprit_file() {
        // Claim: Culprit file gets score >= 8 (culprit bonus)
        let dir = setup(&[
            ("main.py", "import utils\nprint(utils.run())\n"),
            ("utils.py", "def run():\n    return 1\n"),
            ("readme.txt", "This is a readme\n"),
        ]);

        let files = collect_workspace_files(dir.path());
        let ctx = RepairContext {
            stderr: "NameError in main.py".to_string(),
            culprit_files: vec!["main.py".to_string()],
            context_config: Some(crate::types::ContextConfig::default()),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let (selected, _) = select_repair_files(&files, &ctx);
        let main_file = selected.iter().find(|f| f.path.ends_with("main.py"));
        assert!(main_file.is_some(), "Culprit file must be selected");
        assert!(
            main_file.unwrap().score >= 8,
            "Culprit file must have score >= 8"
        );
    }

    #[test]
    fn evidence_stderr_repeated_mentions_boost_score() {
        // Claim: Files mentioned multiple times in stderr get higher scores (v9.3.0)
        let dir = setup(&[("buggy.py", "def broken():\n    pass\n")]);

        let path = dir.path().join("buggy.py");
        let content = std::fs::read_to_string(&path).unwrap();

        // Single mention
        let ctx_single = RepairContext {
            stderr: "Error in buggy.py".to_string(),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let (score_single, _) = compute_score(&path, &content, &ctx_single, None, &[]);

        // Triple mention
        let ctx_triple = RepairContext {
            stderr: "Error in buggy.py\nFailed buggy.py\nCrash buggy.py".to_string(),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let (score_triple, reasons) = compute_score(&path, &content, &ctx_triple, None, &[]);

        assert!(
            score_triple > score_single,
            "Repeated stderr mentions must boost score: single={} triple={}",
            score_single,
            score_triple
        );
        assert!(
            reasons.iter().any(|r| r.contains("stderr occurrences")),
            "Reasons must include stderr occurrences"
        );
    }

    #[test]
    fn evidence_force_include_dropped_is_tracked() {
        // Claim: force_include files that cannot be loaded are tracked (v9.3.0)
        let dir = setup(&[("existing.py", "x = 1\n")]);

        let files = collect_workspace_files(dir.path());
        let nonexistent = dir.path().join("ghost.py");
        let ctx = RepairContext {
            force_include: vec![nonexistent],
            context_config: Some(crate::types::ContextConfig::default()),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let (_, budget) = select_repair_files(&files, &ctx);
        assert_eq!(
            budget.force_include_dropped.len(),
            1,
            "Missing force_include file must appear in dropped list"
        );
        assert!(budget.force_include_dropped[0].contains("ghost.py"));
    }

    #[test]
    fn evidence_budget_report_reduction_pct_is_correct() {
        // Claim: BudgetReport.reduction_pct() computes correctly
        let report = BudgetReport {
            total_files: 10,
            selected_files: 3,
            tokens_before: 1000,
            tokens_after: 600,
            force_include_dropped: vec![],
        };
        assert_eq!(report.reduction_pct(), 40);
    }

    #[test]
    fn evidence_budget_report_reduction_pct_zero_when_no_reduction() {
        let report = BudgetReport {
            total_files: 5,
            selected_files: 5,
            tokens_before: 500,
            tokens_after: 500,
            force_include_dropped: vec![],
        };
        assert_eq!(report.reduction_pct(), 0);
    }

    #[test]
    fn evidence_budget_report_handles_zero_tokens_before() {
        let report = BudgetReport {
            total_files: 0,
            selected_files: 0,
            tokens_before: 0,
            tokens_after: 0,
            force_include_dropped: vec![],
        };
        assert_eq!(report.reduction_pct(), 0);
    }

    #[test]
    fn evidence_build_repair_context_returns_budget_metadata() {
        // Claim: build_repair_context_block returns (String, BudgetReport) — not just String (v9.3.0)
        let dir = setup(&[("main.py", "print(\"hello\")\n")]);

        let ctx = RepairContext {
            stderr: "Error in main.py".to_string(),
            culprit_files: vec!["main.py".to_string()],
            context_config: Some(crate::types::ContextConfig::default()),
            workspace: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let (text, budget) = build_repair_context_block(dir.path(), &ctx);
        assert!(!text.is_empty(), "Context text must not be empty");
        assert!(
            budget.selected_files >= 1,
            "At least 1 file must be selected"
        );
        assert!(budget.tokens_after > 0, "tokens_after must be > 0");
    }
}
