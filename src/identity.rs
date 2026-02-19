use anyhow::{Context, Result};
use std::path::Path;

pub fn load_identity(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read identity file: {}", path.display()))
}
