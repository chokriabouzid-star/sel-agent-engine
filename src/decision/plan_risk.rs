// src/decision/plan_risk.rs
use crate::protocol::Cmd;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanRiskReport {
    pub estimated_risk: f32,
    pub touches_existing_test_files: bool,
    pub uses_write_file_on_existing_source: bool,
    pub has_destructive_ops: bool,
    pub command_count: usize,
    pub reasons: Vec<String>,
}

impl PlanRiskReport {
    pub fn should_replan(&self) -> bool {
        self.touches_existing_test_files
            || self.uses_write_file_on_existing_source
            || self.has_destructive_ops
            || self.estimated_risk >= 0.75
    }

    pub fn feedback_lines(&self) -> Vec<String> {
        self.reasons.clone()
    }
}

/// فحص حجم الخطة — مستقل عن بقية plan_risk
/// يُطبَّق دائماً حتى عند goal-authorized test edits
pub fn check_plan_size(plan: &[Cmd]) -> Vec<String> {
    let mut issues = Vec::new();
    if plan.len() >= 10 {
        issues.push(format!(
            "PLAN RISK: plan has {} commands — unusually large, may indicate drift. \
             Combine operations where possible.",
            plan.len()
        ));
    }
    issues
}

pub fn evaluate_plan_risk(workspace: &Path, plan: &[Cmd]) -> PlanRiskReport {
    let mut report = PlanRiskReport {
        estimated_risk: 0.0,
        touches_existing_test_files: false,
        uses_write_file_on_existing_source: false,
        has_destructive_ops: false,
        command_count: plan.len(),
        reasons: Vec::new(),
    };

    for cmd in plan {
        match cmd {
            Cmd::WriteFile { path, .. } => {
                let full = workspace.join(path);
                let exists = full.exists();
                let is_test = is_test_like_path(path);

                if exists && is_test {
                    report.touches_existing_test_files = true;
                    report.estimated_risk += 0.50;
                    report.reasons.push(format!(
                        "PLAN RISK: write_file targets existing test file '{}' \
                         — tests are contracts and should not be rewritten.",
                        path
                    ));
                } else if exists
                    && is_source_like_path(path)
                    && !is_explicitly_allowed_rewrite(path)
                {
                    report.uses_write_file_on_existing_source = true;
                    report.estimated_risk += 0.35;
                    report.reasons.push(format!(
                        "PLAN RISK: write_file targets existing source file '{}' \
                         — prefer patch_file for surgical fixes.",
                        path
                    ));
                }
            }
            Cmd::PatchFile { path, .. } => {
                let full = workspace.join(path);
                if full.exists() && is_test_like_path(path) {
                    report.touches_existing_test_files = true;
                    report.estimated_risk += 0.50;
                    report.reasons.push(format!(
                        "PLAN RISK: patch_file targets existing test file '{}' \
                         — fix implementation files instead.",
                        path
                    ));
                }
            }
            Cmd::DeleteFile { path } => {
                report.has_destructive_ops = true;
                report.estimated_risk += 0.60;
                report.reasons.push(format!(
                    "PLAN RISK: delete_file on '{}' is destructive \
                     and should be avoided unless strictly necessary.",
                    path
                ));
            }
            _ => {}
        }
    }

    if plan.len() >= 8 {
        report.estimated_risk += 0.20;
        report.reasons.push(format!(
            "PLAN RISK: plan has {} commands \
             — this is unusually large and may indicate drift.",
            plan.len()
        ));
    }

    report.estimated_risk = report.estimated_risk.min(1.0);
    report
}

fn is_test_like_path(path: &str) -> bool {
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    path.contains("/tests/")
        || path.starts_with("tests/")
        || filename.starts_with("test_")
        || filename.ends_with("_test.go")
        || filename.ends_with("_test.py")
        || filename.ends_with("_test.rs")
        || filename.contains(".test.")
        || filename.contains(".spec.")
}

fn is_source_like_path(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    ["rs", "py", "go", "ts", "js"].contains(&ext)
}

fn is_explicitly_allowed_rewrite(path: &str) -> bool {
    matches!(path, "Cargo.toml" | "package.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_plan_risk_flags_existing_test_file_write() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test_api.py"), "def test_x(): pass\n").unwrap();

        let plan = vec![Cmd::WriteFile {
            path: "tests/test_api.py".into(),
            content: "def test_y(): pass\n".into(),
        }];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(report.touches_existing_test_files);
        assert!(report.should_replan());
    }

    #[test]
    fn test_plan_risk_flags_existing_source_rewrite() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();

        let plan = vec![Cmd::WriteFile {
            path: "src/lib.rs".into(),
            content: "pub fn b() {}\n".into(),
        }];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(report.uses_write_file_on_existing_source);
        assert!(report.should_replan());
    }

    #[test]
    fn test_plan_risk_allows_small_patch_plan() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();

        let plan = vec![
            Cmd::PatchFile {
                path: "src/lib.rs".into(),
                search: "a".into(),
                replace: "b".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
            Cmd::Done {
                message: "ok".into(),
            },
        ];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(!report.should_replan());
        assert!(report.estimated_risk < 0.75);
    }

    // ═══ Evidence Tests (Wave 1) ═══

    #[test]
    fn evidence_plan_risk_triggers_on_delete_file() {
        // Claim: Plan Risk flags Cmd::DeleteFile as high risk
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("old.py"), "x = 1\n").unwrap();

        let plan = vec![Cmd::DeleteFile {
            path: "old.py".into(),
        }];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(
            report.estimated_risk >= 0.5,
            "DeleteFile must produce high risk, got {}",
            report.estimated_risk
        );
        assert!(report.should_replan(), "DeleteFile must trigger replan");
    }

    #[test]
    fn evidence_plan_risk_delete_file_produces_feedback() {
        // Claim: DeleteFile risk produces actionable feedback lines
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("remove_me.py"), "x = 1\n").unwrap();

        let plan = vec![Cmd::DeleteFile {
            path: "remove_me.py".into(),
        }];

        let report = evaluate_plan_risk(dir.path(), &plan);
        let lines = report.feedback_lines();
        assert!(
            !lines.is_empty(),
            "DeleteFile risk must produce feedback lines"
        );
    }

    #[test]
    fn evidence_plan_risk_does_not_trigger_on_safe_source_patch() {
        // Claim: No false positives on simple safe plans
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();

        let plan = vec![
            Cmd::PatchFile {
                path: "src/lib.rs".into(),
                search: "a".into(),
                replace: "b".into(),
            },
            Cmd::RunTests {
                target: "cargo test".into(),
            },
            Cmd::Done {
                message: "fixed".into(),
            },
        ];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(
            !report.should_replan(),
            "Safe patch plan should NOT trigger replan"
        );
        assert!(!report.touches_existing_test_files);
    }

    #[test]
    fn evidence_plan_risk_feedback_lines_nonempty_when_risky() {
        // Claim: feedback_lines() provides actionable info when risk is detected
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test_x.py"), "def test(): pass\n").unwrap();

        let plan = vec![Cmd::WriteFile {
            path: "tests/test_x.py".into(),
            content: "def test_new(): pass\n".into(),
        }];

        let report = evaluate_plan_risk(dir.path(), &plan);
        assert!(report.should_replan());
        let lines = report.feedback_lines();
        assert!(!lines.is_empty(), "Feedback lines must explain the risk");
    }

    #[test]
    fn evidence_plan_risk_should_replan_reflects_risk_threshold() {
        // Claim: should_replan() is true only when risk >= threshold
        let dir = TempDir::new().unwrap();

        // Empty plan = zero risk
        let report = evaluate_plan_risk(dir.path(), &[]);
        assert!(!report.should_replan());
        assert!(report.estimated_risk < 0.5);
    }
}
