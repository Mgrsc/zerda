use anyhow::Result;
use std::path::PathBuf;

use crate::util::fs::atomic_write_text;

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

    pub fn load_long_term_memory(&self, max_chars: usize) -> Option<String> {
        let path = self.base_dir.join("MEMORY.md");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!("Failed to read {}: {e}", path.display());
                return None;
            }
        };
        if content.is_empty() {
            return None;
        }
        if content.len() > max_chars {
            let start = content.len() - max_chars;
            let mut start = start;
            while start < content.len() && !content.is_char_boundary(start) {
                start += 1;
            }
            if let Some(nl) = content[start..].find('\n') {
                start += nl + 1;
            }
            Some(content[start..].to_string())
        } else {
            Some(content)
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

    pub fn memory_path(&self) -> PathBuf {
        self.base_dir.join("MEMORY.md")
    }

    pub fn append_memory(&self, content: &str) -> Result<()> {
        let path = self.memory_path();
        let existing = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        let mut next = existing;
        next.push('\n');
        next.push_str(content);
        next.push('\n');
        atomic_write_text(&path, &next)?;
        Ok(())
    }

    pub fn read_memory(&self, max_chars: usize) -> Option<String> {
        self.load_long_term_memory(max_chars)
    }
}
