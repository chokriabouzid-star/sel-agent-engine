// src/bench_suite.rs  v8.3.0: Unified Benchmark Facade
//     
//   T0: bench_cases.rs      (36 )  sel-agent bench --suite all
//   T1-T4: bench_realworld.rs (14 )  sel-agent bench-real-world
//   Smoke: sel_smoke_test.sh  (12 )  bash sel_smoke_test.sh
//   : fixtures/trajectories/

/// Tier classification for the unified suite
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tier {
    /// T0: Base language tests (36 cases  bench_cases.rs)
    Base,
    /// T1: Compile-first pipeline (4 cases)
    CompileFirst,
    /// T2: QuickFix  auto-repair without LLM (3 cases)
    QuickFix,
    /// T3: BugFix + Language Guard (3 cases)
    BugFix,
    /// T4: Real-world patterns (4 cases)
    RealWorld,
    /// Smoke: External bash-driven end-to-end validation (12 cases)
    Smoke,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Base => write!(f, "T0:Base"),
            Tier::CompileFirst => write!(f, "T1:Compile"),
            Tier::QuickFix => write!(f, "T2:QuickFix"),
            Tier::BugFix => write!(f, "T3:BugFix"),
            Tier::RealWorld => write!(f, "T4:RealWorld"),
            Tier::Smoke => write!(f, "Smoke:E2E"),
        }
    }
}

/// Returns the canonical trajectory base path: ./fixtures/trajectories/
pub fn trajectories_dir() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("fixtures")
        .join("trajectories")
}

/// Print unified suite summary
pub fn print_suite_info() {
    let base_count = crate::bench_cases::all_cases().len();
    println!("\n");
    println!("   SEL Agent v8.3.0  Unified Benchmark Suite                ");
    println!("");
    println!("                                                              ");
    println!("  Rust-Native Benchmarks:                                     ");
    println!("   T0:Base        {:>2} cases   sel-agent bench --suite all  ", base_count);
    println!("   T1:Compile      4 cases   sel-agent bench-real-world    ");
    println!("   T2:QuickFix     3 cases   sel-agent bench-real-world    ");
    println!("   T3:BugFix       3 cases   sel-agent bench-real-world    ");
    println!("   T4:RealWorld    4 cases   sel-agent bench-real-world    ");
    println!("                                                              ");
    println!("  Shell Benchmark:                                            ");
    println!("   Smoke:E2E      12 cases   bash sel_smoke_test.sh        ");
    println!("                                                              ");
    println!("  Total:           {:>2} cases                                  ", base_count + 14 + 12);
    println!("  Trajectories:    fixtures/trajectories/                      ");
    println!("\n");
}
