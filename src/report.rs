use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub version: String,
    pub goal: String,
    pub goal_hash: String,
    pub workspace: String,
    pub timestamp_utc: String,
    pub duration_secs: u64,
    pub mode: String,
    pub outcome: ExecutionOutcome,
    pub repair_attempts: i64,
    pub autofix_count: u64,
    pub tests_passed: bool,
    pub mutation_score: f64,
    pub provider_model: String,
    pub llm_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub avg_tokens_per_task: u64,
    #[serde(default)]
    pub avg_context_tokens: u64,
    #[serde(default)]
    pub avg_selected_files: u64,
    #[serde(default)]
    pub context_reduction_pct: u8,
    #[serde(default)]
    pub force_include_dropped_count: u64,
    pub failure_reason: Option<String>,

    // v9.2.1: Plan Risk Telemetry
    pub plan_risk_triggered: bool,
    pub replan_count: u64,
    pub plan_risk_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionOutcome {
    Pass,
    Fail,
}

pub fn stable_goal_hash(goal: &str) -> String {
    // Deterministic FNV-1a 64-bit hash
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in goal.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{:016x}", hash)
}

#[cfg(test)]
mod evidence {
    use super::*;

    #[test]
    fn evidence_old_report_json_deserializes_with_defaults() {
        // Claim: Reports from v9.2.5 (without new fields) still deserialize correctly
        let old_json = r#"{
            "version": "9.2.5",
            "goal": "test goal",
            "goal_hash": "abc123",
            "workspace": "/tmp/test",
            "timestamp_utc": "20260614T120000Z",
            "duration_secs": 10,
            "mode": "replay",
            "outcome": "PASS",
            "repair_attempts": 0,
            "autofix_count": 0,
            "tests_passed": true,
            "mutation_score": -1.0,
            "provider_model": "test",
            "llm_calls": 2,
            "tokens_in": 100,
            "tokens_out": 50,
            "failure_reason": null,
            "plan_risk_triggered": false,
            "replan_count": 0,
            "plan_risk_reasons": []
        }"#;

        let report: ExecutionReport = serde_json::from_str(old_json).unwrap();
        // New fields must default to 0
        assert_eq!(report.total_tokens, 0);
        assert_eq!(report.avg_tokens_per_task, 0);
        assert_eq!(report.avg_context_tokens, 0);
        assert_eq!(report.avg_selected_files, 0);
        assert_eq!(report.context_reduction_pct, 0);
        assert_eq!(report.force_include_dropped_count, 0);
    }

    #[test]
    fn evidence_new_report_roundtrip_preserves_all_fields() {
        // Claim: Serialize then deserialize preserves every field including new ones
        let report = ExecutionReport {
            version: "9.3.0".into(),
            goal: "test".into(),
            goal_hash: "hash".into(),
            workspace: "/tmp".into(),
            timestamp_utc: "20260614T120000Z".into(),
            duration_secs: 42,
            mode: "replay".into(),
            outcome: ExecutionOutcome::Pass,
            repair_attempts: 3,
            autofix_count: 1,
            tests_passed: true,
            mutation_score: 0.85,
            provider_model: "test-model".into(),
            llm_calls: 5,
            tokens_in: 200,
            tokens_out: 100,
            total_tokens: 300,
            avg_tokens_per_task: 60,
            avg_context_tokens: 1500,
            avg_selected_files: 4,
            context_reduction_pct: 35,
            force_include_dropped_count: 2,
            failure_reason: None,
            plan_risk_triggered: true,
            replan_count: 1,
            plan_risk_reasons: vec!["test risk".into()],
        };

        let json = serde_json::to_string(&report).unwrap();
        let restored: ExecutionReport = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.total_tokens, 300);
        assert_eq!(restored.avg_tokens_per_task, 60);
        assert_eq!(restored.avg_context_tokens, 1500);
        assert_eq!(restored.avg_selected_files, 4);
        assert_eq!(restored.context_reduction_pct, 35);
        assert_eq!(restored.force_include_dropped_count, 2);
        assert!(restored.plan_risk_triggered);
        assert_eq!(restored.replan_count, 1);
    }

    #[test]
    fn evidence_report_schema_contains_all_telemetry_fields() {
        // Claim: JSON output contains every telemetry field name
        let report = ExecutionReport {
            version: "9.3.0".into(),
            goal: "x".into(),
            goal_hash: "h".into(),
            workspace: "/t".into(),
            timestamp_utc: "t".into(),
            duration_secs: 0,
            mode: "replay".into(),
            outcome: ExecutionOutcome::Pass,
            repair_attempts: 0,
            autofix_count: 0,
            tests_passed: true,
            mutation_score: 0.0,
            provider_model: "m".into(),
            llm_calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            total_tokens: 0,
            avg_tokens_per_task: 0,
            avg_context_tokens: 0,
            avg_selected_files: 0,
            context_reduction_pct: 0,
            force_include_dropped_count: 0,
            failure_reason: None,
            plan_risk_triggered: false,
            replan_count: 0,
            plan_risk_reasons: vec![],
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("total_tokens"));
        assert!(json.contains("avg_tokens_per_task"));
        assert!(json.contains("avg_context_tokens"));
        assert!(json.contains("avg_selected_files"));
        assert!(json.contains("context_reduction_pct"));
        assert!(json.contains("force_include_dropped_count"));
        assert!(json.contains("plan_risk_triggered"));
        assert!(json.contains("replan_count"));
        assert!(json.contains("plan_risk_reasons"));
    }

    #[test]
    fn evidence_stable_goal_hash_is_deterministic() {
        // Claim: Same goal always produces same hash
        let h1 = stable_goal_hash("Fix the bug in main.py");
        let h2 = stable_goal_hash("Fix the bug in main.py");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 64-bit hex = 16 chars
    }

    #[test]
    fn evidence_stable_goal_hash_differs_for_different_goals() {
        let h1 = stable_goal_hash("Fix bug A");
        let h2 = stable_goal_hash("Fix bug B");
        assert_ne!(h1, h2);
    }
}
