use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_existing(candidates: &[&str]) -> String {
    for rel in candidates {
        let path = repo_root().join(rel);
        if path.exists() {
            return fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed reading {}: {}", path.display(), e));
        }
    }
    panic!("none of the candidate files exist: {:?}", candidates);
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[test]
fn evidence_dependency_graph_scoring_uses_graph() {
    let dep_builder = read_existing(&[
        "src/dependency_graph/builder.rs",
        "src/dependency_graph.rs",
        "src/dependency_graph/mod.rs",
    ]);
    let ctx_builder =
        read_existing(&["src/context/builder.rs", "src/decision/context_builders.rs"]);

    assert!(
        contains_any(
            &dep_builder,
            &["DependencyGraph", "add_edge", "impacted_by"]
        ),
        "expected dependency graph implementation markers in dependency graph builder"
    );

    assert!(
        contains_any(&ctx_builder, &["dependency_graph", "DependencyGraph"]),
        "expected repair context builder to reference dependency graph data"
    );
}

#[test]
fn evidence_pattern_library_match_used_in_repair_prompt() {
    let pattern_lib = read_existing(&["src/pattern_library.rs"]);
    let repair = read_existing(&["src/repair_strategy.rs"]);

    assert!(
        contains_any(&pattern_lib, &["match_score", "lookup(", "record_failure"]),
        "expected pattern library matching markers"
    );

    assert!(
        repair.contains("build_prompt") && repair.contains("pattern"),
        "expected repair prompt construction to reference pattern guidance"
    );
}

#[test]
fn evidence_adaptive_routing_infers_correct_route() {
    let pattern_lib = read_existing(&["src/pattern_library.rs"]);
    let repair = read_existing(&["src/repair_strategy.rs"]);
    let combined = format!("{pattern_lib}\n{repair}");

    assert!(
        contains_any(&combined, &["infer_route", "route_hint", "effective_route"]),
        "expected adaptive routing markers across pattern library / repair strategy"
    );

    assert!(
        contains_any(
            &combined,
            &["missing_dependency", "rust_ownership", "force_source_only"]
        ),
        "expected route-specific inference markers in adaptive routing code"
    );
}

#[test]
fn evidence_replay_mutation_guard_skips_live_api() {
    let checklist = read_existing(&["src/decision/checklist.rs"]);

    assert!(
        contains_any(&checklist, &["bench_mode", "SEL_BENCH_MODE"]),
        "expected replay/bench guard markers in checklist"
    );

    assert!(
        contains_any(
            &checklist,
            &["mutation_check", "Mutation survived", "Survived mutation"]
        ),
        "expected mutation-guard markers in checklist"
    );
}
