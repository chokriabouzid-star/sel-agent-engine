// evaluator.rs  v6.1 RAS + DTO
use std::cmp::Ordering;

#[derive(Debug, Clone, Default)]
pub struct RawMetrics {
    pub tests_passed: u32,
    pub tests_total: u32,
    pub mutation_score: f64,
    pub repairs: u32,
    pub retries: u32,
    pub connection_errors: u32,
    pub rate_limits: u32,
    pub timeouts: u32,
    pub elapsed_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ModelScore {
    pub model: String,
    pub run_id: String,
    pub correctness: f64,
    pub reliability: f64,
    pub efficiency: f64,
    pub composite: f64,
    pub unstable: bool,
    pub raw: RawMetrics,
}

impl ModelScore {
    pub fn from_metrics(model: &str, run_id: &str, m: &RawMetrics, max_time: u64) -> Self {
        let correctness = compute_correctness(m);
        let reliability = compute_reliability(m);
        let efficiency = compute_efficiency(m.elapsed_secs, max_time);
        let composite = (correctness * 0.70) + (reliability * 0.20) + (efficiency * 0.10);
        let unstable = reliability < 0.5;
        Self {
            model: model.into(),
            run_id: run_id.into(),
            correctness,
            reliability,
            efficiency,
            composite,
            unstable,
            raw: m.clone(),
        }
    }
}

fn compute_correctness(m: &RawMetrics) -> f64 {
    let test_ratio = if m.tests_total > 0 {
        m.tests_passed as f64 / m.tests_total as f64
    } else {
        0.0
    };
    (test_ratio + m.mutation_score) / 2.0
}

fn compute_reliability(m: &RawMetrics) -> f64 {
    let penalty = (m.connection_errors as f64 * 0.15)
        + (m.rate_limits as f64 * 0.10)
        + (m.retries as f64 * 0.05)
        + (m.timeouts as f64 * 0.12)
        + (m.repairs as f64 * 0.03);
    (1.0 - penalty).max(0.0)
}

fn compute_efficiency(elapsed: u64, max_time: u64) -> f64 {
    if max_time == 0 {
        return 1.0;
    }
    //      20%
    let ratio = elapsed as f64 / max_time as f64;
    if ratio >= 0.80 {
        //    0.80
        let penalty = (ratio - 0.80) * 2.0; // penalty
        (1.0 - penalty).clamp(0.70, 1.0)
    } else {
        //    bonus
        (1.0 - ratio * 0.5).clamp(0.70, 1.0)
    }
}

// DTO  Deterministic Total Ordering (5 )
pub fn rank_models(mut scores: Vec<ModelScore>) -> Vec<ModelScore> {
    scores.sort_by(|a, b| {
        // UNSTABLE
        match (a.unstable, b.unstable) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        //  1: correctness
        b.correctness
            .partial_cmp(&a.correctness)
            .unwrap_or(Ordering::Equal)
            //  2: reliability
            .then_with(|| {
                b.reliability
                    .partial_cmp(&a.reliability)
                    .unwrap_or(Ordering::Equal)
            })
            //  3: efficiency
            .then_with(|| {
                b.efficiency
                    .partial_cmp(&a.efficiency)
                    .unwrap_or(Ordering::Equal)
            })
            //  4:
            .then_with(|| a.model.cmp(&b.model))
            //  5: run_id
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
    scores
}

//
pub fn print_comparison_table(scores: &[ModelScore]) {
    println!("\n{}", "".repeat(80));
    println!("  Model Comparison  v6.1 Reliability-Aware Scoring");
    println!("{}", "".repeat(80));
    println!(
        "  {:<28} {:>8} {:>9} {:>9} {:>9}  Status",
        "Model", "Correct", "Reliable", "Effic.", "Composite"
    );
    println!("{}", "".repeat(80));

    for (i, s) in scores.iter().enumerate() {
        let winner = if i == 0 && !s.unstable { " Winner" } else { "" };
        let status = if s.unstable { " UNSTABLE" } else { winner };
        println!(
            "  {:<28} {:>7.2} {:>8.2} {:>9.2} {:>9.3}  {}",
            s.model, s.correctness, s.reliability, s.efficiency, s.composite, status
        );
        println!(
            "  {:<28} repairs:{} retries:{} conn_err:{} rate_lim:{}",
            "", s.raw.repairs, s.raw.retries, s.raw.connection_errors, s.raw.rate_limits
        );
        println!("{}", "".repeat(80));
    }
}
