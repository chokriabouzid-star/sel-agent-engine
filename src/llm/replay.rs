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

        // FIX M-14: strict by default — stale constitution = hard error
        // Override with SEL_ALLOW_STALE_REPLAY=1 only during fixture migration
        let current_hash = crate::constitution::constitution_hash();
        if !record.constitution_hash.is_empty()
            && record.constitution_hash != current_hash
            && count == 1
        {
            let allow_stale = std::env::var("SEL_ALLOW_STALE_REPLAY")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);

            if allow_stale {
                eprintln!(
                    "  [WARN] Replay: constitution mismatch ignored (SEL_ALLOW_STALE_REPLAY=1)\n  \
                     Recorded: {} | Current: {}\n  \
                     Re-run with --record to refresh fixtures.",
                    record.constitution_hash.chars().take(8).collect::<String>(),
                    current_hash.chars().take(8).collect::<String>()
                );
            } else {
                return Err(anyhow!(
                    "REPLAY_STALE: constitution hash mismatch \
                     (recorded={}, current={}). \
                     Re-run with --record to refresh fixtures. \
                     To skip temporarily: SEL_ALLOW_STALE_REPLAY=1",
                    record.constitution_hash.chars().take(8).collect::<String>(),
                    current_hash.chars().take(8).collect::<String>()
                ));
            }
        }

        // FIX M-13: compare system prompt at every step — not just step 1
        // Use prefix comparison (first 120 chars) to catch constitution/goal changes
        // while tolerating minor dynamic context differences
        if !record.req.system.is_empty() && !req.system.is_empty() {
            let recorded_prefix = &record.req.system[..record.req.system.len().min(120)];
            let current_prefix = &req.system[..req.system.len().min(120)];
            if recorded_prefix != current_prefix {
                let allow_stale = std::env::var("SEL_ALLOW_STALE_REPLAY")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
                if allow_stale {
                    eprintln!(
                        "  [WARN] Replay step {}: system prompt mismatch ignored (SEL_ALLOW_STALE_REPLAY=1)",
                        count
                    );
                } else {
                    return Err(anyhow!(
                        "REPLAY_STALE: system prompt mismatch at step {}                          (first 120 chars differ).                          Re-run with --record to refresh fixtures.                          To skip: SEL_ALLOW_STALE_REPLAY=1",
                        count
                    ));
                }
            }
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

#[cfg(test)]
mod replay_tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // منع تعارض env vars بين الاختبارات المتوازية
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_record(constitution_hash: &str) -> String {
        format!(
            r#"{{
                "req": {{"system": "test", "messages": [], "model": "test", "temperature": 0.0, "seed": null}},
                "resp": {{"content": "ok", "tokens_in": 1, "tokens_out": 1, "finish_reason": "stop"}},
                "latency_ms": 0,
                "constitution_hash": "{}",
                "recorded_at": "",
                "provider_used": "",
                "task_kind": "",
                "tokens_used": 0
            }}"#,
            constitution_hash
        )
    }

    /// M-14: stale constitution hash must return Err by default
    #[tokio::test]
    async fn m14_stale_constitution_rejects_by_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempdir().expect("tempdir");
        let record = make_record("00000000deadbeef");
        fs::write(dir.path().join("001.json"), &record).unwrap();

        // تأكد أن SEL_ALLOW_STALE_REPLAY غير مضبوط
        std::env::remove_var("SEL_ALLOW_STALE_REPLAY");

        let provider = ReplayProvider::new(dir.path());
        let req = LLMRequest {
            system: "test".to_string(),
            messages: vec![],
            model: "test".to_string(),
            temperature: 0.0,
            seed: None,
        };

        let result = provider.complete(req).await;

        assert!(
            result.is_err(),
            "M-14 FAIL: stale constitution should return Err by default"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("REPLAY_STALE"),
            "M-14 FAIL: error should contain REPLAY_STALE, got: {}",
            err
        );
    }

    /// M-14: SEL_ALLOW_STALE_REPLAY=1 allows stale constitution with warning
    #[tokio::test]
    async fn m14_allow_stale_env_var_bypasses_check() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempdir().expect("tempdir");
        let record = make_record("00000000deadbeef");
        fs::write(dir.path().join("001.json"), &record).unwrap();

        std::env::set_var("SEL_ALLOW_STALE_REPLAY", "1");

        let provider = ReplayProvider::new(dir.path());
        let req = LLMRequest {
            system: "test".to_string(),
            messages: vec![],
            model: "test".to_string(),
            temperature: 0.0,
            seed: None,
        };

        let result = provider.complete(req).await;

        std::env::remove_var("SEL_ALLOW_STALE_REPLAY");

        assert!(
            result.is_ok(),
            "M-14 FAIL: SEL_ALLOW_STALE_REPLAY=1 should bypass check, got: {:?}",
            result.err()
        );
    }

    /// M-13: system prompt mismatch must return Err by default
    #[tokio::test]
    async fn m13_system_prompt_mismatch_rejects_by_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempdir().expect("tempdir");
        let current_hash = crate::constitution::constitution_hash();

        // record مع system prompt مختلف
        let record = format!(
            r#"{{
                "req": {{"system": "{}", "messages": [], "model": "test", "temperature": 0.0, "seed": null}},
                "resp": {{"content": "ok", "tokens_in": 1, "tokens_out": 1, "finish_reason": "stop"}},
                "latency_ms": 0,
                "constitution_hash": "{}",
                "recorded_at": "",
                "provider_used": "",
                "task_kind": "",
                "tokens_used": 0
            }}"#,
            "OLD SYSTEM PROMPT that is completely different from current", current_hash
        );
        fs::write(dir.path().join("001.json"), &record).unwrap();
        std::env::remove_var("SEL_ALLOW_STALE_REPLAY");

        let provider = ReplayProvider::new(dir.path());
        let req = LLMRequest {
            system: "NEW SYSTEM PROMPT that is different from recorded one for testing".to_string(),
            messages: vec![],
            model: "test".to_string(),
            temperature: 0.0,
            seed: None,
        };

        let result = provider.complete(req).await;
        assert!(
            result.is_err(),
            "M-13 FAIL: system prompt mismatch should return Err by default"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("REPLAY_STALE"),
            "M-13 FAIL: error should contain REPLAY_STALE, got: {}",
            err
        );
    }

    /// M-13: SEL_ALLOW_STALE_REPLAY=1 bypasses system prompt check
    #[tokio::test]
    async fn m13_allow_stale_bypasses_system_prompt_check() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempdir().expect("tempdir");
        let current_hash = crate::constitution::constitution_hash();

        let record = format!(
            r#"{{
                "req": {{"system": "{}", "messages": [], "model": "test", "temperature": 0.0, "seed": null}},
                "resp": {{"content": "ok", "tokens_in": 1, "tokens_out": 1, "finish_reason": "stop"}},
                "latency_ms": 0,
                "constitution_hash": "{}",
                "recorded_at": "",
                "provider_used": "",
                "task_kind": "",
                "tokens_used": 0
            }}"#,
            "OLD SYSTEM PROMPT that is completely different from current", current_hash
        );
        fs::write(dir.path().join("001.json"), &record).unwrap();
        std::env::set_var("SEL_ALLOW_STALE_REPLAY", "1");

        let provider = ReplayProvider::new(dir.path());
        let req = LLMRequest {
            system: "NEW SYSTEM PROMPT that is different from recorded one for testing".to_string(),
            messages: vec![],
            model: "test".to_string(),
            temperature: 0.0,
            seed: None,
        };

        let result = provider.complete(req).await;
        std::env::remove_var("SEL_ALLOW_STALE_REPLAY");

        assert!(
            result.is_ok(),
            "M-13 FAIL: SEL_ALLOW_STALE_REPLAY=1 should bypass check, got: {:?}",
            result.err()
        );
    }

    /// M-14: matching hash proceeds normally
    #[tokio::test]
    async fn m14_matching_hash_proceeds_normally() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = tempdir().expect("tempdir");
        let current_hash = crate::constitution::constitution_hash();
        let record = make_record(&current_hash);
        fs::write(dir.path().join("001.json"), &record).unwrap();

        std::env::remove_var("SEL_ALLOW_STALE_REPLAY");

        let provider = ReplayProvider::new(dir.path());
        let req = LLMRequest {
            system: "test".to_string(),
            messages: vec![],
            model: "test".to_string(),
            temperature: 0.0,
            seed: None,
        };

        let result = provider.complete(req).await;

        assert!(
            result.is_ok(),
            "M-14 FAIL: matching hash should proceed normally, got: {:?}",
            result.err()
        );
    }
}
