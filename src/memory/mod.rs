pub mod chroma_store;
pub mod config;
pub mod conflict;
pub mod consolidation;
pub mod decay;
pub mod embed_client;
pub mod ingest;
pub mod recall;
pub mod sqlite_store;
pub mod types;

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use self::chroma_store::ChromaStore;
use self::config::MemoryRuntimeConfig;
use self::embed_client::EmbeddingClient;
use self::sqlite_store::{PendingTurnBacklog, SqliteStore};
use self::types::{JournalMessage, RecallResult};
use crate::providers::{ChatOptions, Provider};

const DEFAULT_MEMORY_ENTITY_ID: &str = "self";
const MAINTENANCE_COUNT_THRESHOLD: usize = 3;
const MAINTENANCE_AGE_THRESHOLD_SECS: i64 = 300;

pub struct MemoryService {
    config: MemoryRuntimeConfig,
    sqlite: SqliteStore,
    embedding: EmbeddingClient,
    chroma: ChromaStore,
    maintenance_lock: Mutex<()>,
}

impl MemoryService {
    pub fn new(cfg: &crate::config::MemoryConfig) -> Result<Self> {
        let config = MemoryRuntimeConfig::from_config(cfg);
        let sqlite = SqliteStore::new(config.sqlite_path.clone())?;
        let embedding = EmbeddingClient::new(&config)?;
        let chroma = ChromaStore::new(&config)?;
        Ok(Self {
            config,
            sqlite,
            embedding,
            chroma,
            maintenance_lock: Mutex::new(()),
        })
    }

    pub fn shared(cfg: &crate::config::MemoryConfig) -> Result<Arc<Self>> {
        Ok(Arc::new(Self::new(cfg)?))
    }

    pub fn append_turn_messages(
        &self,
        turn_id: &str,
        session_id: &str,
        entity_id: &str,
        channel: Option<&str>,
        messages: &[JournalMessage],
    ) -> Result<()> {
        self.sqlite
            .append_turn_messages(turn_id, session_id, entity_id, channel, messages)
    }

    pub async fn recall_prompt(
        &self,
        entity_id: &str,
        query: &str,
    ) -> Result<Option<(String, RecallResult)>> {
        let Some((blocks, result)) = recall::recall(
            &self.sqlite,
            &self.chroma,
            &self.embedding,
            entity_id,
            query,
        )
        .await?
        else {
            return Ok(None);
        };
        let Some(rendered) = blocks.render() else {
            return Ok(None);
        };
        Ok(Some((rendered, result)))
    }

    pub fn spawn_maintenance(
        self: &Arc<Self>,
        analyzer: (Arc<dyn Provider>, ChatOptions),
        entity_id: String,
    ) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let backlog = match service.sqlite.pending_turn_backlog() {
                Ok(backlog) => backlog,
                Err(error) => {
                    tracing::warn!(entity_id = %entity_id, error = %error, "Failed to inspect memory backlog");
                    return;
                }
            };
            let Some(trigger_reason) = should_run_maintenance(&backlog, Utc::now()) else {
                tracing::info!(
                    entity_id = %entity_id,
                    pending_turn_count = backlog.count,
                    oldest_pending_turn_age_secs = oldest_pending_turn_age_secs(&backlog, Utc::now()),
                    count_threshold = MAINTENANCE_COUNT_THRESHOLD,
                    age_threshold_secs = MAINTENANCE_AGE_THRESHOLD_SECS,
                    "Memory maintenance skipped by buffer policy"
                );
                return;
            };
            let Ok(_guard) = service.maintenance_lock.try_lock() else {
                return;
            };
            tracing::info!(
                entity_id = %entity_id,
                pending_turn_count = backlog.count,
                oldest_pending_turn_age_secs = oldest_pending_turn_age_secs(&backlog, Utc::now()),
                count_threshold = MAINTENANCE_COUNT_THRESHOLD,
                age_threshold_secs = MAINTENANCE_AGE_THRESHOLD_SECS,
                trigger_reason,
                "Memory maintenance triggered by buffer policy"
            );
            if let Err(error) = service.run_maintenance(analyzer, &entity_id).await {
                tracing::warn!(entity_id = %entity_id, error = %error, "Memory maintenance failed");
            }
        });
    }

    async fn run_maintenance(
        &self,
        analyzer: (Arc<dyn Provider>, ChatOptions),
        entity_id: &str,
    ) -> Result<()> {
        ingest::process_pending_turns(&self.sqlite, &self.embedding, &self.chroma, &analyzer)
            .await?;
        consolidation::consolidate_entity(
            &self.sqlite,
            &self.embedding,
            &self.chroma,
            &analyzer,
            entity_id,
        )
        .await?;
        decay::run_decay(&self.sqlite, &self.chroma).await?;
        Ok(())
    }

    pub fn entity_id(&self) -> &'static str {
        DEFAULT_MEMORY_ENTITY_ID
    }

    pub fn sqlite_path(&self) -> &std::path::Path {
        &self.config.sqlite_path
    }

    pub fn embedding_base_url(&self) -> &str {
        self.embedding.base_url()
    }

    pub fn chroma_url(&self) -> &str {
        self.chroma.base_url()
    }
}

fn should_run_maintenance(
    backlog: &PendingTurnBacklog,
    now: DateTime<Utc>,
) -> Option<&'static str> {
    if backlog.count >= MAINTENANCE_COUNT_THRESHOLD {
        return Some("count_threshold");
    }
    let oldest_age_secs = oldest_pending_turn_age_secs(backlog, now)?;
    if oldest_age_secs >= MAINTENANCE_AGE_THRESHOLD_SECS {
        return Some("age_threshold");
    }
    None
}

fn oldest_pending_turn_age_secs(backlog: &PendingTurnBacklog, now: DateTime<Utc>) -> Option<i64> {
    let oldest = backlog.oldest_created_at.as_deref()?;
    let oldest = DateTime::parse_from_rfc3339(oldest)
        .ok()?
        .with_timezone(&Utc);
    Some((now - oldest).num_seconds())
}

#[cfg(test)]
mod tests {
    use super::{
        oldest_pending_turn_age_secs, should_run_maintenance, PendingTurnBacklog,
        MAINTENANCE_AGE_THRESHOLD_SECS,
    };
    use chrono::{Duration, Utc};

    #[test]
    fn skips_maintenance_when_backlog_is_below_both_thresholds() {
        let now = Utc::now();
        let backlog = PendingTurnBacklog {
            count: 2,
            oldest_created_at: Some((now - Duration::seconds(60)).to_rfc3339()),
        };

        let trigger = should_run_maintenance(&backlog, now);

        assert_eq!(trigger, None);
    }

    #[test]
    fn triggers_maintenance_when_pending_turn_count_reaches_threshold() {
        let now = Utc::now();
        let backlog = PendingTurnBacklog {
            count: 3,
            oldest_created_at: Some((now - Duration::seconds(10)).to_rfc3339()),
        };

        let trigger = should_run_maintenance(&backlog, now);

        assert_eq!(trigger, Some("count_threshold"));
    }

    #[test]
    fn triggers_maintenance_when_oldest_pending_turn_exceeds_age_threshold() {
        let now = Utc::now();
        let backlog = PendingTurnBacklog {
            count: 1,
            oldest_created_at: Some(
                (now - Duration::seconds(MAINTENANCE_AGE_THRESHOLD_SECS + 1)).to_rfc3339(),
            ),
        };

        let trigger = should_run_maintenance(&backlog, now);

        assert_eq!(trigger, Some("age_threshold"));
    }

    #[test]
    fn computes_oldest_pending_turn_age_secs_from_backlog_timestamp() {
        let now = Utc::now();
        let backlog = PendingTurnBacklog {
            count: 1,
            oldest_created_at: Some((now - Duration::seconds(42)).to_rfc3339()),
        };

        let age_secs = oldest_pending_turn_age_secs(&backlog, now);

        assert_eq!(age_secs, Some(42));
    }
}
