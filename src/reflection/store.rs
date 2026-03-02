use std::collections::HashMap;

use anyhow::{Context, Result};
use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, PointsIdsList,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde::Deserialize;

use super::types::Guideline;

const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
const DEFAULT_EMBEDDING_DIM: u64 = 1536;
const COLLECTION: &str = "zerda_executor_guidelines";
const DEFAULT_EMBEDDING_BASE_URL: &str = "https://api.openai.com/v1";

pub struct QdrantStore {
    qdrant: Qdrant,
    collection: String,
    embedding_dim: u64,
    http_client: reqwest::Client,
    embedding_base_url: String,
    embedding_api_key: String,
    embedding_model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl QdrantStore {
    pub fn try_from_env(embedding_dim_override: Option<u64>) -> Option<Self> {
        let qdrant_url = read_non_empty_env("QDRANT_URL")?;

        let embedding_api_key = read_non_empty_env("ACON_EMBEDDING_API_KEY")
            .or_else(|| read_non_empty_env("OPENAI_API_KEY"));
        let embedding_api_key = match embedding_api_key {
            Some(k) => k,
            None => {
                tracing::warn!(
                    "ACON: no embedding API key available (set ACON_EMBEDDING_API_KEY or OPENAI_API_KEY)"
                );
                return None;
            }
        };
        let embedding_base_url = read_non_empty_env("ACON_EMBEDDING_BASE_URL")
            .or_else(|| read_non_empty_env("OPENAI_BASE_URL"))
            .unwrap_or_else(|| DEFAULT_EMBEDDING_BASE_URL.to_string());
        let embedding_model = read_non_empty_env("ACON_EMBEDDING_MODEL")
            .unwrap_or_else(|| DEFAULT_EMBEDDING_MODEL.to_string());
        let embedding_dim = embedding_dim_override.unwrap_or(DEFAULT_EMBEDDING_DIM);

        let mut qdrant_config = QdrantConfig::from_url(&qdrant_url);
        if let Some(api_key) = read_non_empty_env("QDRANT_API_KEY") {
            qdrant_config = qdrant_config.api_key(api_key);
        }

        let qdrant = match Qdrant::new(qdrant_config) {
            Ok(client) => client,
            Err(e) => {
                tracing::warn!("ACON: failed to create Qdrant client: {e}");
                return None;
            }
        };

        tracing::info!(
            "ACON: Qdrant store initialized (collection={COLLECTION}, embedding_base_url={embedding_base_url}, model={embedding_model}, dim={embedding_dim})"
        );

        Some(Self {
            qdrant,
            collection: COLLECTION.to_string(),
            embedding_dim,
            http_client: reqwest::Client::new(),
            embedding_base_url,
            embedding_api_key,
            embedding_model,
        })
    }

    pub async fn ensure_collection(&self) -> Result<()> {
        let exists = self
            .qdrant
            .collection_exists(&self.collection)
            .await
            .context("ACON: check collection existence")?;

        if !exists {
            self.qdrant
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection).vectors_config(
                        VectorParamsBuilder::new(self.embedding_dim, Distance::Cosine),
                    ),
                )
                .await
                .context("ACON: create collection")?;
            tracing::info!("ACON: created Qdrant collection '{}'", self.collection);
        }

        Ok(())
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!(
            "{}/embeddings",
            self.embedding_base_url.trim_end_matches('/')
        );
        let body = serde_json::json!({
            "model": self.embedding_model,
            "input": text,
            "dimensions": self.embedding_dim,
        });

        let resp = self
            .http_client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.embedding_api_key),
            )
            .json(&body)
            .send()
            .await
            .context("ACON: embedding request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("ACON: embedding API returned {status}: {text}");
        }

        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .context("ACON: parse embedding response")?;
        parsed
            .data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow::anyhow!("ACON: empty embedding response"))
    }

    pub async fn query(&self, instruction: &str, top_k: u64) -> Result<Vec<Guideline>> {
        let vector = self.embed(instruction).await?;
        let response = self
            .qdrant
            .query(
                QueryPointsBuilder::new(&self.collection)
                    .query(vector)
                    .limit(top_k)
                    .with_payload(true),
            )
            .await
            .context("ACON: query points")?;

        let mut guidelines = Vec::new();
        for point in response.result {
            let payload: &HashMap<String, qdrant_client::qdrant::Value> = &point.payload;
            let guideline_text = payload
                .get("guideline_text")
                .and_then(|v| v.as_str())
                .map_or(String::new(), |s| s.to_string());
            if guideline_text.is_empty() {
                continue;
            }
            let id = match &point.id {
                Some(pid) => format!("{pid:?}"),
                None => continue,
            };
            guidelines.push(Guideline {
                id,
                guideline_text,
                score: point.score,
            });
        }

        Ok(guidelines)
    }

    pub async fn insert(&self, id: &str, instruction: &str, guideline: &str) -> Result<()> {
        let vector = self.embed(instruction).await?;
        let point_id: qdrant_client::qdrant::PointId = id.to_string().into();
        let payload: qdrant_client::Payload = serde_json::json!({
            "guideline_text": guideline,
            "instruction_digest": truncate(instruction, 200),
            "created_at": chrono::Utc::now().to_rfc3339(),
        })
        .try_into()
        .context("ACON: build payload")?;

        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(
                &self.collection,
                vec![PointStruct::new(point_id, vector, payload)],
            ))
            .await
            .context("ACON: upsert point")?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let point_id: qdrant_client::qdrant::PointId = id.to_string().into();
        self.qdrant
            .delete_points(
                DeletePointsBuilder::new(&self.collection).points(PointsIdsList {
                    ids: vec![point_id],
                }),
            )
            .await
            .context("ACON: delete point")?;

        Ok(())
    }
}

fn read_non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
