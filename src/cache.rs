use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct PersistentCache {
    base_dir: PathBuf,
}

impl PersistentCache {
    pub fn new() -> Result<Self> {
        let base = std::env::current_dir()
            .unwrap_or_default()
            .join("fixtures");
        
        let trajectories = base.join("trajectories");
        if !trajectories.exists() {
            fs::create_dir_all(&trajectories)?;
        }
        
        let quickfix = base.join("quickfix");
        if !quickfix.exists() {
            fs::create_dir_all(&quickfix)?;
        }
        
        Ok(Self { base_dir: base })
    }

    pub fn trajectories_dir(&self) -> PathBuf {
        self.base_dir.join("trajectories")
    }

    pub fn quickfix_dir(&self) -> PathBuf {
        self.base_dir.join("quickfix")
    }

    pub fn save_trajectory(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.trajectories_dir().join(format!("{}.json", key));
        fs::write(path, data)?;
        Ok(())
    }

    pub fn load_trajectory(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.trajectories_dir().join(format!("{}.json", key));
        if path.exists() {
            Ok(Some(fs::read(path)?))
        } else {
            Ok(None)
        }
    }
}
