use anyhow::Result;
use std::path::PathBuf;

pub struct Memory {
    base_dir: PathBuf,
}

impl Memory {
    pub const fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn load_user_context(&self) -> Option<String> {
        let path = self.base_dir.join("user.md");
        match std::fs::read_to_string(&path) {
            Ok(content) => Some(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                tracing::warn!("Failed to read {}: {e}", path.display());
                None
            }
        }
    }

    pub fn memory_file_size(&self) -> u64 {
        let path = self.base_dir.join("MEMORY.md");
        std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
    }

    pub fn check_memory_size(&self, max_size: u64) {
        let size = self.memory_file_size();
        let threshold = max_size / 5 * 4;
        if size > threshold {
            tracing::warn!(
                "MEMORY.md is {size} bytes ({:.0}% of {max_size} limit)",
                if max_size == 0 {
                    0.0
                } else {
                    size as f64 / max_size as f64 * 100.0
                }
            );
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.base_dir)?;
        Ok(())
    }
}
