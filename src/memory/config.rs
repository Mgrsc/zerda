use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct MemoryRuntimeConfig {
    pub embedding_base_url: String,
    pub embedding_api_key: String,
    pub embedding_model: String,
    pub embedding_dimensions: usize,
    pub embedding_timeout: Duration,
    pub sqlite_path: PathBuf,
    pub chroma_url: String,
}

impl MemoryRuntimeConfig {
    pub fn from_config(cfg: &crate::config::MemoryConfig) -> Self {
        Self {
            embedding_base_url: cfg.embedding.base_url.trim_end_matches('/').to_string(),
            embedding_api_key: cfg.embedding.api_key.clone(),
            embedding_model: cfg.embedding.model.clone(),
            embedding_dimensions: cfg.embedding.dimensions,
            embedding_timeout: Duration::from_millis(cfg.embedding.timeout_ms),
            sqlite_path: crate::config::resolve_path(&cfg.sqlite.path),
            chroma_url: cfg.chroma.url.trim_end_matches('/').to_string(),
        }
    }
}
