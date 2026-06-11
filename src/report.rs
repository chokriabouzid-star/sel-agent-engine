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
