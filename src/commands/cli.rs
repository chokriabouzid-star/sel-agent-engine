use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sel-agent", version = "8.4.1")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}


#[derive(Subcommand)]
pub enum Commands {
    Run {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        goal: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "false")]
        dry_run: bool,
        #[arg(long)]
        ref_file: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
        #[arg(long)]
        record: bool,
        #[arg(long)]
        replay: bool,
        #[arg(long)]
        rerecord: bool,
    },
    Health,
    Stress {
        #[arg(long, default_value = "3")]
        max_repairs: u8,

        /// عدد الحالات (default: 8)
        #[arg(long, default_value_t = 8)]
        cases: usize,

        /// ثوانٍ انتظار بين الحالات
        #[arg(long, default_value_t = 5)]
        delay: u64,

        /// تسجيل trajectories
        #[arg(long)]
        record: bool,

        /// تشغيل من trajectories محفوظة
        #[arg(long)]
        replay: bool,

        /// إعادة تسجيل trajectories الفاشلة تلقائياً
        #[arg(long)]
        rerecord: bool,
    },
    Bench {
        #[arg(long, default_value = "all")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
        #[arg(long, default_value = "1")]
        iterations: u8,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
        #[arg(long)]
        record: bool,
        #[arg(long)]
        replay: bool,
        #[arg(long, default_value = "false")]
        quick: bool,
        #[arg(long, default_value = "0")]
        delay: u64,
        #[arg(long)]
        skip_recorded: bool,
        #[arg(long)]
        rerecord: bool,
    },
    Scan {
        ///  
        #[arg(long, default_value = ".")]
        workspace: String,
        ///  JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Compare {
        #[arg(long, value_delimiter = ',')]
        models: Vec<String>,
        #[arg(long, default_value = "python")]
        suite: String,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    Plan {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        plan: PathBuf,
        #[arg(long, default_value = "3")]
        max_repairs: u8,
    },
    #[command(name = "bench-swe")]
    BenchSwe {
        /// اللغة: all | python | go | rust | typescript
        #[arg(long, default_value = "all")]
        lang: String,

        /// تشغيل اختبارات محددة: "PY-01,GO-01"
        #[arg(long)]
        focus: Option<String>,

        /// أقصى عدد محاولات إصلاح لكل حالة
        #[arg(long, default_value_t = 5)]
        max_repairs: u8,

        /// ثوانٍ انتظار بين الحالات
        #[arg(long, default_value_t = 8)]
        delay: u64,

        /// تسجيل trajectories
        #[arg(long)]
        record: bool,

        /// تشغيل من trajectories محفوظة
        #[arg(long)]
        replay: bool,

        /// إعادة تسجيل trajectories الفاشلة تلقائياً
        #[arg(long)]
        rerecord: bool,
    },
    #[command(name = "bench-real-world")]
    BenchRealWorld {
        #[arg(long)]
        tier: Option<u8>,
        #[arg(long, default_value = "6")]
        max_repairs: u8,
        #[arg(long)]
        record: bool,
        #[arg(long)]
        replay: bool,
        #[arg(long)]
        rerecord: bool,
        #[arg(long, default_value = "15")]
        delay: u64,
        #[arg(long)]
        skip_recorded: bool,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
    },


    /// SELBench v1.1-rc — Extended benchmark (18 core + 2 system)
    #[command(name = "bench-sel-v11")]
    BenchSelV11 {
        /// تشغيل حالات محددة: "RC-01,RC-05"
        #[arg(long)]
        focus: Option<String>,

        /// أقصى عدد محاولات إصلاح
        #[arg(long, default_value_t = 5)]
        max_repairs: u8,

        /// ثوانٍ انتظار بين الحالات
        #[arg(long, default_value_t = 0)]
        delay: u64,

        /// تشغيل System Checks أيضًا
        #[arg(long)]
        include_system: bool,

        /// تسجيل trajectories
        #[arg(long)]
        record: bool,

        /// تشغيل من trajectories محفوظة
        #[arg(long)]
        replay: bool,

        /// إعادة تسجيل trajectories الفاشلة
        #[arg(long)]
        rerecord: bool,
    },
    /// SELBench v1.0 — Internal capability benchmark
    #[command(name = "bench-sel")]
    BenchSel {
        /// تشغيل حالات محددة: "SB-01,SB-03"
        #[arg(long)]
        focus: Option<String>,

        /// أقصى عدد محاولات إصلاح
        #[arg(long, default_value_t = 5)]
        max_repairs: u8,

        /// ثوانٍ انتظار بين الحالات
        #[arg(long, default_value_t = 0)]
        delay: u64,

        /// تسجيل trajectories
        #[arg(long)]
        record: bool,

        /// تشغيل من trajectories محفوظة
        #[arg(long)]
        replay: bool,

        /// إعادة تسجيل trajectories الفاشلة
        #[arg(long)]
        rerecord: bool,
    },
    /// Clear the provider state cache (reset all exhausted/expired flags)
    #[command(name = "reset-providers")]
    ResetProviders,
}

