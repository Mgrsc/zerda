use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::MemoryServiceConfig;

#[derive(Debug, Serialize)]
pub struct RecallRequest {
    pub tenant_id: String,
    pub entity_id: String,
    pub process_id: String,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct RecallResponse {
    pub items: Vec<RecallItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecallItem {
    #[serde(rename = "type")]
    pub kind: String,
    pub content: String,
    pub score: f64,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct IngestRequest {
    pub tenant_id: String,
    pub entity_id: String,
    pub process_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub messages: Vec<IngestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<IngestContext>,
}

#[derive(Debug, Serialize)]
pub struct IngestContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_time: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IngestMessage {
    pub role: String,
    pub content: String,
}

pub struct MemoryServiceClient {
    client: Client,
    base_url: String,
    auth_token: String,
    tenant_id: String,
    default_entity_id: String,
    process_id: String,
    recall_top_k: u32,
    recall_min_score: f64,
    recall_timeout: Duration,
    ingest_timeout: Duration,
    ingest_max_retries: u32,
}

impl MemoryServiceClient {
    pub fn new(cfg: &MemoryServiceConfig) -> Result<Self> {
        let client = Client::builder().build()?;
        let recall_min_score = cfg.recall_min_score.clamp(0.0, 1.0);
        if (recall_min_score - cfg.recall_min_score).abs() > f64::EPSILON {
            tracing::warn!(
                configured = cfg.recall_min_score,
                clamped = recall_min_score,
                "memory_service.recall_min_score out of range, clamped into [0.0, 1.0]"
            );
        }
        Ok(Self {
            client,
            base_url: cfg.url.trim_end_matches('/').to_string(),
            auth_token: cfg.auth_token.clone(),
            tenant_id: cfg.tenant_id.clone(),
            default_entity_id: cfg.default_entity_id.clone(),
            process_id: cfg.process_id.clone(),
            recall_top_k: cfg.recall_top_k,
            recall_min_score,
            recall_timeout: Duration::from_millis(cfg.recall_timeout_ms),
            ingest_timeout: Duration::from_millis(cfg.ingest_timeout_ms),
            ingest_max_retries: cfg.ingest_max_retries,
        })
    }

    pub async fn recall(&self, query: &str, entity_id: Option<&str>) -> Result<Vec<RecallItem>> {
        let entity = entity_id.unwrap_or(&self.default_entity_id);
        let req = RecallRequest {
            tenant_id: self.tenant_id.clone(),
            entity_id: entity.to_string(),
            process_id: self.process_id.clone(),
            query: query.to_string(),
            intent: None,
            top_k: Some(self.recall_top_k),
        };

        let resp = self
            .client
            .post(format!("{}/v1/memory/recall", self.base_url))
            .bearer_auth(&self.auth_token)
            .header("X-Tenant-ID", &self.tenant_id)
            .timeout(self.recall_timeout)
            .json(&req)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("recall failed ({status}): {body}");
        }

        let recall_resp: RecallResponse = resp.json().await?;
        tracing::debug!(
            items = recall_resp.items.len(),
            entity_id = entity,
            "Memory recall completed"
        );
        Ok(recall_resp.items)
    }

    pub async fn ingest(
        &self,
        messages: Vec<IngestMessage>,
        entity_id: Option<&str>,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        channel: Option<&str>,
    ) -> Result<()> {
        let entity = entity_id.unwrap_or(&self.default_entity_id);
        let context = IngestContext {
            channel: channel.map(String::from),
            current_time: Some(Utc::now().to_rfc3339()),
        };
        let req = IngestRequest {
            tenant_id: self.tenant_id.clone(),
            entity_id: entity.to_string(),
            process_id: self.process_id.clone(),
            session_id: session_id.map(String::from),
            turn_id: turn_id.map(String::from),
            messages,
            context: Some(context),
        };

        let body = serde_json::to_value(&req)?;
        let url = format!("{}/v1/memory/ingest", self.base_url);

        for attempt in 0..=self.ingest_max_retries {
            let resp = self
                .client
                .post(&url)
                .bearer_auth(&self.auth_token)
                .header("X-Tenant-ID", &self.tenant_id)
                .timeout(self.ingest_timeout)
                .json(&body)
                .send()
                .await?;

            let status = resp.status();
            if status.is_success() {
                tracing::debug!(entity_id = entity, attempt, "Memory ingest accepted");
                return Ok(());
            }

            let resp_body = resp.text().await.unwrap_or_default();

            if status.is_client_error() {
                anyhow::bail!("ingest failed ({status}): {resp_body}");
            }

            if attempt < self.ingest_max_retries {
                let delay = Duration::from_millis(500 * 2u64.pow(attempt));
                tracing::warn!(
                    status = %status,
                    attempt,
                    delay_ms = delay.as_millis(),
                    "Ingest 5xx, retrying"
                );
                tokio::time::sleep(delay).await;
            } else {
                anyhow::bail!(
                    "ingest failed after {} retries ({status}): {resp_body}",
                    self.ingest_max_retries
                );
            }
        }

        unreachable!()
    }

    pub fn format_recall_context(items: &[RecallItem]) -> Option<String> {
        if items.is_empty() {
            return None;
        }
        let mut buf = String::from("<memory-recall>\n");
        for item in items {
            let timestamp = recall_item_timestamp(item)
                .map(|v| format!("; time={v}"))
                .unwrap_or_default();
            let source = item
                .source
                .as_deref()
                .map(|v| format!("; source={v}"))
                .unwrap_or_default();
            buf.push_str(&format!(
                "- [{}] (score={:.2}{}{source}) {}\n",
                item.kind, item.score, timestamp, item.content
            ));
        }
        buf.push_str("</memory-recall>");
        Some(buf)
    }

    pub fn filter_recall_items(&self, items: Vec<RecallItem>) -> Vec<RecallItem> {
        let total = items.len();
        let filtered: Vec<RecallItem> = items
            .into_iter()
            .filter(|item| item.score >= self.recall_min_score)
            .collect();
        tracing::debug!(
            total,
            kept = filtered.len(),
            dropped = total.saturating_sub(filtered.len()),
            min_score = self.recall_min_score,
            "Memory recall items filtered"
        );
        filtered
    }

    pub async fn feedback(
        &self,
        recall_item_ids: &[String],
        entity_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Result<()> {
        let entity = entity_id.unwrap_or(&self.default_entity_id);
        let body = FeedbackRequest {
            tenant_id: self.tenant_id.clone(),
            entity_id: entity.to_string(),
            process_id: self.process_id.clone(),
            turn_id: turn_id.map(String::from),
            used_items: recall_item_ids.to_vec(),
            helpful: Vec::new(),
            harmful: Vec::new(),
            note: None,
        };
        let url = format!("{}/v1/memory/feedback", self.base_url);

        for attempt in 0..=self.ingest_max_retries {
            let resp = self
                .client
                .post(&url)
                .bearer_auth(&self.auth_token)
                .header("X-Tenant-ID", &self.tenant_id)
                .timeout(self.ingest_timeout)
                .json(&body)
                .send()
                .await?;

            let status = resp.status();
            let resp_body = resp.text().await.unwrap_or_default();
            if status.is_success() {
                tracing::debug!(
                    item_count = recall_item_ids.len(),
                    attempt,
                    "Memory feedback accepted"
                );
                return Ok(());
            }

            if status.is_client_error() {
                anyhow::bail!("feedback failed ({status}): {resp_body}");
            }

            if attempt < self.ingest_max_retries {
                let delay = Duration::from_millis(500 * 2u64.pow(attempt));
                tracing::warn!(
                    status = %status,
                    attempt,
                    delay_ms = delay.as_millis(),
                    "Feedback 5xx, retrying"
                );
                tokio::time::sleep(delay).await;
            } else {
                anyhow::bail!(
                    "feedback failed after {} retries ({status}): {resp_body}",
                    self.ingest_max_retries
                );
            }
        }

        unreachable!()
    }
}

#[derive(Debug, Serialize)]
struct FeedbackRequest {
    tenant_id: String,
    entity_id: String,
    process_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    used_items: Vec<String>,
    helpful: Vec<String>,
    harmful: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

fn recall_item_timestamp(item: &RecallItem) -> Option<String> {
    if let Some(v) = item.created_at.as_deref() {
        return Some(v.to_string());
    }
    if let Some(v) = item.updated_at.as_deref() {
        return Some(v.to_string());
    }
    if let Some(v) = item.expires_at.as_deref() {
        return Some(v.to_string());
    }
    let props = item.properties.as_ref()?.as_object()?;
    for key in [
        "event_end_at",
        "event_date",
        "event_start_at",
        "occurred_at",
        "observed_at",
        "current_time",
    ] {
        if let Some(value) = props.get(key) {
            if let Some(text) = value.as_str() {
                return Some(format!("{key}:{text}"));
            }
            if let Some(ts) = value.as_i64() {
                return Some(format!("{key}:{ts}"));
            }
        }
    }
    None
}

pub struct IngestBuffer {
    turns: Vec<(Vec<IngestMessage>, String)>,
    batch_size: u32,
}

impl IngestBuffer {
    pub fn new(batch_size: u32) -> Self {
        Self {
            turns: Vec::new(),
            batch_size: batch_size.max(1),
        }
    }

    pub fn push(&mut self, user: Option<&str>, assistant: Option<&str>, turn_id: &str) {
        let mut turn_messages = Vec::new();
        if let Some(text) = user {
            if !text.trim().is_empty() {
                turn_messages.push(IngestMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                });
            }
        }
        if let Some(text) = assistant {
            if !text.trim().is_empty() {
                turn_messages.push(IngestMessage {
                    role: "assistant".to_string(),
                    content: text.to_string(),
                });
            }
        }
        if !turn_messages.is_empty() {
            self.turns.push((turn_messages, turn_id.to_string()));
        }
        tracing::debug!(
            turn_count = self.turns.len(),
            batch_size = self.batch_size,
            buffered_messages = self
                .turns
                .iter()
                .map(|(messages, _)| messages.len())
                .sum::<usize>(),
            "Ingest buffer updated"
        );
    }

    pub fn should_flush(&self) -> bool {
        self.turns.len() >= self.batch_size as usize
    }

    pub fn take(&mut self) -> Option<Vec<(Vec<IngestMessage>, String)>> {
        if self.turns.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.turns))
    }

    pub fn has_pending(&self) -> bool {
        !self.turns.is_empty()
    }
}
