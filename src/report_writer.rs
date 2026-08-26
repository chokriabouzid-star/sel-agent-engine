use crate::report::ExecutionReport;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ReportWriter {
    dir: PathBuf,
}

impl ReportWriter {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default() -> Self {
        let dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sel-agent")
            .join("reports");
        Self::new(dir)
    }

    #[allow(dead_code)] // accessor reserved for report backend introspection and future CLI/report tooling
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn write(&self, report: &ExecutionReport) -> Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;

        let short_hash = &report.goal_hash[..report.goal_hash.len().min(8)];
        let filename = format!("{}_{}.json", report.timestamp_utc, short_hash);
        let path = self.dir.join(filename);

        let json = serde_json::to_string_pretty(report)?;
        fs::write(&path, json.as_bytes())?;

        // Keep a convenient pointer for future `report --latest`
        let latest = self.dir.join("latest.json");
        let _ = fs::write(latest, json.as_bytes());

        Ok(path)
    }
}
