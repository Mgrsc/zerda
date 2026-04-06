use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::config::MemoryRuntimeConfig;
use super::types::ChromaQueryResult;

const DEFAULT_TENANT: &str = "default_tenant";
const DEFAULT_DATABASE: &str = "default_database";

pub struct ChromaStore {
    client: Client,
    base_url: String,
    collection_cache: Mutex<HashMap<String, String>>,
}

pub struct ChromaUpsertItem {
    pub entry_id: String,
    pub embedding: Vec<f32>,
    pub document: String,
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
struct ChromaCollection {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChromaQueryResponse {
    #[serde(default)]
    ids: Vec<Vec<String>>,
    #[serde(default)]
    distances: Vec<Vec<f32>>,
}

impl ChromaStore {
    pub fn new(cfg: &MemoryRuntimeConfig) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: cfg.chroma_url.clone(),
            collection_cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn upsert(&self, collection: &str, items: &[ChromaUpsertItem]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let collection_id = self.ensure_collection(collection).await?;
        let url = format!(
            "{}/api/v2/tenants/{}/databases/{}/collections/{}/upsert",
            self.base_url, DEFAULT_TENANT, DEFAULT_DATABASE, collection_id
        );
        let body = json!({
            "ids": items.iter().map(|item| item.entry_id.as_str()).collect::<Vec<_>>(),
            "embeddings": items.iter().map(|item| item.embedding.clone()).collect::<Vec<_>>(),
            "documents": items.iter().map(|item| item.document.as_str()).collect::<Vec<_>>(),
            "metadatas": items.iter().map(|item| item.metadata.clone()).collect::<Vec<_>>(),
        });
        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to upsert Chroma records to {collection}"))?
            .error_for_status()
            .with_context(|| format!("Chroma upsert returned error for {collection}"))?;
        Ok(())
    }

    pub async fn query(
        &self,
        collection: &str,
        entity_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<ChromaQueryResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let collection_id = self.ensure_collection(collection).await?;
        let url = format!(
            "{}/api/v2/tenants/{}/databases/{}/collections/{}/query",
            self.base_url, DEFAULT_TENANT, DEFAULT_DATABASE, collection_id
        );
        let body = json!({
            "query_embeddings": [embedding],
            "n_results": limit,
            "include": ["distances"],
            "where": {
                "entity_id": entity_id,
            }
        });
        let response: ChromaQueryResponse = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to query Chroma collection {collection}"))?
            .error_for_status()
            .with_context(|| format!("Chroma query returned error for {collection}"))?
            .json()
            .await
            .context("Failed to parse Chroma query response")?;
        let ids = response.ids.into_iter().next().unwrap_or_default();
        let distances = response.distances.into_iter().next().unwrap_or_default();
        let mut out = Vec::new();
        for (index, entry_id) in ids.iter().enumerate() {
            out.push(ChromaQueryResult {
                entry_id: entry_id.clone(),
                distance: distances.get(index).copied().unwrap_or(1.0),
            });
        }
        Ok(out)
    }

    pub async fn delete_ids(&self, collection: &str, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let collection_id = self.ensure_collection(collection).await?;
        let url = format!(
            "{}/api/v2/tenants/{}/databases/{}/collections/{}/delete",
            self.base_url, DEFAULT_TENANT, DEFAULT_DATABASE, collection_id
        );
        let body = json!({ "ids": ids });
        self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to delete Chroma records from {collection}"))?
            .error_for_status()
            .with_context(|| format!("Chroma delete returned error for {collection}"))?;
        Ok(())
    }

    async fn ensure_collection(&self, collection: &str) -> Result<String> {
        if let Some(existing) = self.collection_cache.lock().await.get(collection).cloned() {
            return Ok(existing);
        }
        let url = format!(
            "{}/api/v2/tenants/{}/databases/{}/collections",
            self.base_url, DEFAULT_TENANT, DEFAULT_DATABASE
        );
        let body = json!({
            "name": collection,
            "get_or_create": true,
        });
        let created: ChromaCollection = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to ensure Chroma collection {collection}"))?
            .error_for_status()
            .with_context(|| format!("Chroma create collection returned error for {collection}"))?
            .json()
            .await
            .context("Failed to parse Chroma collection response")?;
        self.collection_cache
            .lock()
            .await
            .insert(collection.to_string(), created.id.clone());
        Ok(created.id)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}
