use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct TrajectoryIndex {
    pub entries: Vec<TrajectoryEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct TrajectoryEntry {
    pub task_id: String,
    pub files: Vec<String>,
    pub interaction_count: usize,
    pub recorded_at: String,
    pub constitution_hash: String,
    pub has_repairs: bool,
    pub language: String,
}

impl TrajectoryIndex {
    pub fn load(fixtures_dir: &Path) -> Self {
        let index_path = fixtures_dir.join("index.json");
        if let Ok(data) = std::fs::read_to_string(&index_path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, fixtures_dir: &Path) {
        let index_path = fixtures_dir.join("index.json");
        let data = serde_json::to_string_pretty(self).unwrap();
        let _ = std::fs::write(index_path, data);
    }

    pub fn upsert(&mut self, entry: TrajectoryEntry) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.task_id == entry.task_id) {
            *e = entry;
        } else {
            self.entries.push(entry);
        }
    }
}
