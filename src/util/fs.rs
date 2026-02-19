use std::io::Write;
use std::path::Path;

use anyhow::Result;

pub fn atomic_write_text(path: &Path, data: &str) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid path without parent: {}", path.display()))?;
    std::fs::create_dir_all(dir)?;

    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid file name: {}", path.display()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = dir.join(format!(".{file_name}.{stamp}.tmp"));

    {
        let mut tmp = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)?;
        tmp.write_all(data.as_bytes())?;
        tmp.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    if let Ok(dir_fd) = std::fs::File::open(dir) {
        let _ = dir_fd.sync_all();
    }
    Ok(())
}
