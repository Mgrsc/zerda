use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use super::config::MemoryRuntimeConfig;

pub struct EmbeddingClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    dimensions: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    pub fn new(cfg: &MemoryRuntimeConfig) -> Result<Self> {
        let client = Client::builder().timeout(cfg.embedding_timeout).build()?;
        Ok(Self {
            client,
            base_url: cfg.embedding_base_url.clone(),
            api_key: cfg.embedding_api_key.clone(),
            model: cfg.embedding_model.clone(),
            dimensions: cfg.embedding_dimensions,
        })
    }

    pub async fn embed_text(&self, input: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = json!({
            "model": self.model,
            "input": input,
            "dimensions": self.dimensions,
        });
        let mut request = self.client.post(&url).json(&body);
        if !self.api_key.trim().is_empty() {
            request = request.bearer_auth(&self.api_key);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to request embeddings from {url}"))?;
        let response = response
            .error_for_status()
            .with_context(|| format!("Embedding endpoint returned error for {url}"))?;
        let payload: EmbeddingResponse = response
            .json()
            .await
            .context("Failed to parse embedding response JSON")?;
        let embedding = payload
            .data
            .into_iter()
            .next()
            .context("Embedding response contained no vectors")?
            .embedding;
        anyhow::ensure!(
            embedding.len() == self.dimensions,
            "Embedding dimension mismatch: expected {}, got {}",
            self.dimensions,
            embedding.len()
        );
        Ok(embedding)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
