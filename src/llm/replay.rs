// src/llm/replay.rs

use super::record::TrajectoryRecord;
use super::{LLMProvider, LLMRequest, LLMResponse, LlmCallStats};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct ReplayProvider {
    record_dir: PathBuf,
    counter: Arc<Mutex<usize>>,
}

impl ReplayProvider {
    pub fn new(record_dir: impl AsRef<Path>) -> Self {
        Self {
            record_dir: record_dir.as_ref().to_path_buf(),
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LLMProvider for ReplayProvider {
    async fn complete(&self, req: LLMRequest) -> Result<LLMResponse> {
        let count = {
            let mut c = match self.counter.lock() {
                Ok(v) => v,
                Err(_) => return Err(anyhow::anyhow!("replay provider counter lock poisoned")),
            };
            *c += 1;
            *c
        };

        let file_path = self.record_dir.join(format!("{:03}.json", count));
        if !file_path.exists() {
            return Err(anyhow!(
                "TRAJECTORY_INCOMPLETE: {}  re-run with --record to update fixtures",
                file_path.display()
            ));
        }

        let json = fs::read_to_string(&file_path)?;
        let json = crate::llm::json_sanitizer::fix_rust_doc_comments(&json);
        let record: TrajectoryRecord = serde_json::from_str(&json)?;

        // FIX M-14: strict mode rejects stale constitution (default: warn only)
        let current_hash = crate::constitution::constitution_hash();
        if !record.constitution_hash.is_empty()
            && record.constitution_hash != current_hash
            && count == 1
        {
            let strict = std::env::var("SEL_STRICT_REPLAY")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);

            if strict {
                return Err(anyhow!(
                    "REPLAY_STALE: constitution hash mismatch                      (recorded={}, current={}).                      Re-run with --record to refresh fixtures.",
                    record.constitution_hash.chars().take(8).collect::<String>(),
                    current_hash.chars().take(8).collect::<String>()
                ));
            } else {
                eprintln!(
                    "  REPLAY STALE: constitution changed since recording.
                            Recorded: {} | Current: {}
                            Run with --record to refresh fixtures.",
                    &record.constitution_hash.chars().take(8).collect::<String>(),
                    current_hash.chars().take(8).collect::<String>()
                );
            }
        }

        // FIX M-13: warn if system prompt changed since recording
        if !record.req.system.is_empty()
            && record.req.system != req.system
            && count == 1
        {
            eprintln!(
                "  [WARN] Replay: system prompt mismatch at step 1 —                  fixture may be stale. Re-run with --record to refresh."
            );
        }

        // Simulate network delay
        let delay_ms = std::cmp::min(record.latency_ms, 500); // Max 500ms for fast replay
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        Ok(record.resp)
    }

    fn mode(&self) -> &'static str {
        "replay"
    }

    fn get_stats(&self) -> LlmCallStats {
        LlmCallStats::default() // Replay has zero network issues
    }
}
