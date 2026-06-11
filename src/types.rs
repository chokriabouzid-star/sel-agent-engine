// src/types.rs  v1.3:

use std::collections::HashSet;
use std::path::PathBuf;

//
// State Machine
//

#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    Planning,
    Executing,
    Repairing,
    WaitingForUserInput(String),
    Done,
    Failed(String),
}

//
//
//

#[derive(Debug, Clone)]
pub struct MutationContext {
    pub surviving: String,
}

#[derive(Debug, Default)]
pub struct ExecutionContext {
    pub tests_passed: bool,
    pub last_exit_code: Option<i32>,
    pub failed_steps: Vec<FailedStep>,
    pub repair_attempts: u8,
    pub successful_hashes: HashSet<String>,
    pub max_repairs: u8,
    pub start_time: Option<std::time::Instant>,
    pub mutations_total: u32,
    pub mutations_killed: u32,
    pub replan_attempts: u8,                // v5.6: Unique Patch Enforcer
    pub last_failed_steps: Vec<FailedStep>, // v5.8:
    pub current_failure_kind: Option<FailureKind>, // v6.4
    pub skip_mutation: bool, // v6.5: disable mutation enforcement for real-world bench
    pub last_mutation_context: Option<MutationContext>, // v7.5.2
    pub checklist_run_tests_injected: bool, // v7.6.1: Prevent infinite run_tests injections
    pub autofix_count: u32,  // v7.9.9 P5: Track system-driven fixes
    pub mutation_survival_counts: std::collections::HashMap<String, u8>, // v8.1: Equivalent mutant tracking
    pub bench_mode: bool, // v8.1: Differentiate run mode and bench mode for test augmentation
    pub recent_edits: Vec<PathBuf>, // v9.0: recent file edits for smart repair context
    pub cached_dependency_graph: Option<crate::dependency_graph::DependencyGraph>, // v9.0: cached graph for smart repair context
    pub cached_dependency_graph_workspace: Option<PathBuf>, // v9.0: cache key

    // v9.2.1: Plan Risk Telemetry
    pub plan_risk_triggered: bool,
    pub plan_risk_reasons: Vec<String>,
    pub replan_count: u32,
    pub commands_before_replan: usize,
}

impl ExecutionContext {
    pub fn new(max_repairs: u8) -> Self {
        Self {
            max_repairs,
            start_time: None,
            successful_hashes: std::collections::HashSet::new(),
            ..Default::default()
        }
    }
    pub fn reset_for_repair(&mut self) {
        self.tests_passed = false;
        self.last_exit_code = None;
        if !self.failed_steps.is_empty() {
            self.last_failed_steps = self.failed_steps.clone(); // v5.8
        }
        self.failed_steps.clear();
        self.last_mutation_context = None;
        self.mutations_total = 0;
        self.mutations_killed = 0;
    }
    pub fn has_failures(&self) -> bool {
        !self.failed_steps.is_empty()
    }

    pub fn record_recent_edit(&mut self, workspace: &std::path::Path, rel_path: &str) {
        if rel_path.trim().is_empty() {
            return;
        }

        let full = workspace.join(rel_path);
        self.recent_edits.retain(|p| p != &full);
        self.recent_edits.push(full);

        const MAX_RECENT_EDITS: usize = 16;
        if self.recent_edits.len() > MAX_RECENT_EDITS {
            let overflow = self.recent_edits.len() - MAX_RECENT_EDITS;
            self.recent_edits.drain(0..overflow);
        }
    }

    pub fn cached_dependency_graph_for(
        &self,
        workspace: &std::path::Path,
    ) -> Option<&crate::dependency_graph::DependencyGraph> {
        match (
            self.cached_dependency_graph_workspace.as_ref(),
            self.cached_dependency_graph.as_ref(),
        ) {
            (Some(cached_ws), Some(graph)) if cached_ws == workspace => Some(graph),
            _ => None,
        }
    }

    pub fn cache_dependency_graph(
        &mut self,
        workspace: &std::path::Path,
        graph: crate::dependency_graph::DependencyGraph,
    ) {
        self.cached_dependency_graph_workspace = Some(workspace.to_path_buf());
        self.cached_dependency_graph = Some(graph);
    }

    pub fn invalidate_dependency_graph_cache(&mut self) {
        self.cached_dependency_graph = None;
        self.cached_dependency_graph_workspace = None;
    }
}

#[derive(Debug, Clone)]
pub struct FailedStep {
    pub step_index: usize,
    pub label: String,
    pub stderr: String,
    pub exit_code: i32,
    pub culprit_file: Option<String>, //
}

impl FailedStep {
    pub fn extract_culprit(stderr: &str) -> Option<String> {
        for line in stderr.lines() {
            // Python traceback: File "/path/file.py", line 42
            if line.contains("File \"") && line.contains(".py") {
                if let Some(s) = line.find("File \"") {
                    let rest = &line[s + 6..];
                    if let Some(e) = rest.find('"') {
                        let path = &rest[..e];
                        if !path.contains("venv") && !path.contains("site-packages") {
                            let base = path.rfind('/').map(|i| i + 1).unwrap_or(0);
                            let name = &path[base..];
                            if !name.starts_with("test_") {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
            // pytest: service.py:1: in <module>
            if line.contains(".py:") && line.contains(": in ") {
                //   stdlib venv
                if line.contains("/usr/lib")
                    || line.contains("venv/")
                    || line.contains("site-packages")
                {
                    continue;
                }
                if let Some(pos) = line.find(".py:") {
                    let start = line[..pos]
                        .rfind(['/', ' ', '\t'])
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let name = &line[start..pos + 3];
                    if !name.starts_with("test_") && !name.starts_with("__") {
                        return Some(name.to_string());
                    }
                }
            }
            // Rust: --> src/lib.rs:42:5
            if line.trim_start().starts_with("--> ") {
                let rest = line.trim_start().trim_start_matches("--> ");
                if let Some(colon) = rest.find(':') {
                    let path = &rest[..colon];
                    let base = path.rfind('/').map(|i| i + 1).unwrap_or(0);
                    return Some(path[base..].to_string());
                }
            }
            // Go: file.go:42
            if line.contains(".go:") {
                if let Some(pos) = line.find(".go:") {
                    let start = line[..pos].rfind(['/', ' ']).map(|i| i + 1).unwrap_or(0);
                    return Some(line[start..pos + 3].to_string());
                }
            }
            // Node.js: file.js:42
            if line.contains(".js:") && !line.contains("node_modules") {
                if let Some(pos) = line.find(".js:") {
                    let start = line[..pos].rfind(['/', ' ']).map(|i| i + 1).unwrap_or(0);
                    return Some(line[start..pos + 3].to_string());
                }
            }
        }
        None
    }
}
//
//
//

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub autofix_triggered: bool, // v7.9.9 P5
}

impl ExecResult {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            exit_code: 0,
            stdout: msg.into(),
            stderr: String::new(),
            duration_ms: 0,
            autofix_triggered: false,
        }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            exit_code: 1,
            stdout: String::new(),
            stderr: msg.into(),
            duration_ms: 0,
            autofix_triggered: false,
        }
    }
}

//
// BenchCase
//

#[derive(Debug, Clone)]
pub struct BenchCase {
    pub name: String,
    pub lang: String,
    pub goal: String,
    pub scaffold_files: Vec<(String, String)>, // v7.9.9: (path, content)  broken code to fix
}

impl BenchCase {
    pub fn new(name: &str, lang: &str, goal: &str) -> Self {
        Self {
            name: name.to_string(),
            lang: lang.to_string(),
            goal: goal.to_string(),
            scaffold_files: Vec::new(),
        }
    }

    /// v7.9.9: Create a bugfix task with pre-placed broken code
    pub fn bugfix(name: &str, lang: &str, goal: &str, files: Vec<(&str, &str)>) -> Self {
        Self {
            name: name.to_string(),
            lang: lang.to_string(),
            goal: goal.to_string(),
            scaffold_files: files
                .into_iter()
                .map(|(p, c)| (p.to_string(), c.to_string()))
                .collect(),
        }
    }

    pub fn is_bugfix(&self) -> bool {
        !self.scaffold_files.is_empty()
    }
}

//
// Message
//

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(s: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: s.into(),
        }
    }
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: s.into(),
        }
    }
}

//
//
//

#[derive(Debug)]
pub enum SafetyError {
    PathTraversal(String),
    BlockedCommand(String),
    WorkspaceEscape(String),
}

impl std::fmt::Display for SafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::PathTraversal(s) => write!(f, "Path traversal: {}", s),
            Self::BlockedCommand(s) => write!(f, "Blocked command: {}", s),
            Self::WorkspaceEscape(s) => write!(f, "Workspace escape: {}", s),
        }
    }
}

pub use crate::failure::FailureKind;

//
// Execution Context
//

impl ExecutionContext {
    pub fn load_hashes(&mut self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let h = line.trim();
                if !h.is_empty() {
                    self.successful_hashes.insert(h.to_string());
                }
            }
        }
    }

    pub fn save_hashes(&self, workspace: &std::path::Path) {
        let path = workspace.join(".sel_hashes");
        let content = self
            .successful_hashes
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(path, content);
    }
}

//
// v5.1: Context Configuration
//

#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub ref_file: Option<PathBuf>, //
    pub focus_paths: Vec<String>,  //
    pub max_context_files: usize,  //    (50  20)
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            ref_file: None,
            focus_paths: vec![],
            max_context_files: 50, //   20  50
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_recent_edit_dedups_and_keeps_latest_order() {
        let workspace = PathBuf::from("/tmp/ws");
        let mut ctx = ExecutionContext::new(3);

        ctx.record_recent_edit(&workspace, "src/lib.rs");
        ctx.record_recent_edit(&workspace, "src/main.rs");
        ctx.record_recent_edit(&workspace, "src/lib.rs");

        assert_eq!(ctx.recent_edits.len(), 2);
        assert_eq!(ctx.recent_edits[0], workspace.join("src/main.rs"));
        assert_eq!(ctx.recent_edits[1], workspace.join("src/lib.rs"));
    }

    #[test]
    fn test_record_recent_edit_caps_history() {
        let workspace = PathBuf::from("/tmp/ws");
        let mut ctx = ExecutionContext::new(3);

        for i in 0..20 {
            ctx.record_recent_edit(&workspace, &format!("src/file{}.rs", i));
        }

        assert_eq!(ctx.recent_edits.len(), 16);
        assert_eq!(
            ctx.recent_edits.first(),
            Some(&workspace.join("src/file4.rs"))
        );
        assert_eq!(
            ctx.recent_edits.last(),
            Some(&workspace.join("src/file19.rs"))
        );
    }

    #[test]
    fn test_dependency_graph_cache_roundtrip_and_invalidate() {
        let workspace = PathBuf::from("/tmp/ws");
        let mut ctx = ExecutionContext::new(3);

        assert!(ctx.cached_dependency_graph_for(&workspace).is_none());

        let mut graph = crate::dependency_graph::DependencyGraph::new();
        graph.add_edge("a.py", "b.py", crate::dependency_graph::EdgeKind::Import);

        ctx.cache_dependency_graph(&workspace, graph);

        let cached = ctx.cached_dependency_graph_for(&workspace).unwrap();
        assert_eq!(cached.edge_count(), 1);

        ctx.invalidate_dependency_graph_cache();
        assert!(ctx.cached_dependency_graph_for(&workspace).is_none());
    }
}
