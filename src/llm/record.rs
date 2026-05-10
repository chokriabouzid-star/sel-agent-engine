// src/llm/record.rs

use super::{LLMProvider, LLMRequest, LLMResponse, LlmCallStats};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
pub struct TrajectoryRecord {
    pub req: LLMRequest,
    pub resp: LLMResponse,
    pub latency_ms: u64,
    #[serde(default)]
    pub constitution_hash: String,
    #[serde(default)]
    pub recorded_at: String,
    #[serde(default)]
    pub provider_used: String,
    #[serde(default)]
    pub task_kind: String,
    #[serde(default)]
    pub tokens_used: u64,
}

pub struct RecorderProvider {
    inner: Box<dyn LLMProvider>,
    record_dir: PathBuf,
    counter: Arc<Mutex<usize>>,
}

impl RecorderProvider {
    pub fn new(inner: Box<dyn LLMProvider>, record_dir: impl AsRef<Path>) -> Self {
        let path = record_dir.as_ref().to_path_buf();
        if !path.exists() {
            let _ = fs::create_dir_all(&path);
        }

        Self {
            inner,
            record_dir: path,
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LLMProvider for RecorderProvider {
    async fn complete(&self, req: LLMRequest) -> Result<LLMResponse> {
        let start = Instant::now();
        let resp = self.inner.complete(req.clone()).await?;
        let latency_ms = start.elapsed().as_millis() as u64;

        let record = TrajectoryRecord {
            req,
            resp: resp.clone(),
            latency_ms,
            constitution_hash: crate::constitution::constitution_hash(),
            recorded_at: chrono::Utc::now().to_rfc3339(),
            provider_used: resp.provider_used.clone(),
            task_kind: resp.task_kind.clone(),
            tokens_used: (resp.tokens_in + resp.tokens_out) as u64,
        };

        let count = {
            let mut c = self.counter.lock().unwrap();
            *c += 1;
            *c
        };

        let file_path = self.record_dir.join(format!("{:03}.json", count));
        if let Ok(json) = serde_json::to_string_pretty(&record) {
            let _ = fs::write(&file_path, json);
        }

        // --- Index Update ---
        if let Some(parent) = self.record_dir.parent() {
            let mut index = crate::trajectory_index::TrajectoryIndex::load(parent);
            let case_name = self
                .record_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let lang = parent
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Collect existing JSON files for this task
            let mut files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&self.record_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.ends_with(".json") {
                            files.push(name.to_string());
                        }
                    }
                }
            }
            files.sort();

            index.upsert(crate::trajectory_index::TrajectoryEntry {
                task_id: case_name,
                files,
                interaction_count: count,
                recorded_at: chrono::Utc::now().to_rfc3339(),
                constitution_hash: crate::constitution::constitution_hash(),
                has_repairs: count > 1,
                language: lang,
            });
            index.save(parent);
        }

        Ok(resp)
    }

    fn mode(&self) -> &'static str {
        "record"
    }

    fn get_stats(&self) -> LlmCallStats {
        self.inner.get_stats()
    }
}
