mod analyzer;
mod store;
pub mod types;

use std::sync::Arc;

use anyhow::Result;

use self::analyzer::ReflectionAnalyzer;
use self::store::QdrantStore;
use self::types::{Guideline, ReflectionContext};
use crate::config::ProviderEndpoint;
use crate::providers::{ChatOptions, Provider};

pub const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

pub struct ReflectionEngine {
    store: QdrantStore,
    analyzer: ReflectionAnalyzer,
}

impl ReflectionEngine {
    pub fn try_new(
        provider: Arc<dyn Provider>,
        chat_opts: ChatOptions,
        qdrant_url: &str,
        qdrant_api_key: Option<&str>,
        embedding_dim: Option<u64>,
        embedding_provider: &ProviderEndpoint,
        embedding_model: &str,
    ) -> Option<Self> {
        let store = QdrantStore::try_new(
            qdrant_url,
            qdrant_api_key,
            embedding_dim,
            embedding_provider,
            embedding_model,
        )?;
        let analyzer = ReflectionAnalyzer::new(provider, chat_opts);
        Some(Self { store, analyzer })
    }

    pub async fn ensure_collection(&self) -> Result<()> {
        self.store.ensure_collection().await
    }

    pub async fn query_guidelines(&self, instruction: &str, top_k: u64) -> Result<Vec<Guideline>> {
        self.store.query(instruction, top_k).await
    }

    pub fn spawn_reflection(self: &Arc<Self>, ctx: ReflectionContext) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(e) = run_reflection(&engine, ctx).await {
                tracing::warn!("REFLECTION: reflection task failed: {e}");
            }
        });
    }
}

async fn run_reflection(engine: &ReflectionEngine, ctx: ReflectionContext) -> Result<()> {
    if ctx.final_failed && !ctx.injected_guideline_ids.is_empty() {
        for id in &ctx.injected_guideline_ids {
            tracing::info!("REFLECTION: deleting unhelpful guideline id={id}");
            if let Err(e) = engine.store.delete(id).await {
                tracing::warn!("REFLECTION: failed to delete guideline {id}: {e}");
            }
        }
    }

    let total = ctx.iteration_outcomes.len();
    let failure_count = ctx
        .iteration_outcomes
        .iter()
        .filter(|o| o.had_tool_error || o.had_traceback)
        .count();
    let success_count = total - failure_count;

    tracing::debug!(
        "REFLECTION: iteration stats total={total} success={success_count} failure={failure_count} final_failed={}",
        ctx.final_failed
    );

    if total >= 2 && failure_count >= 1 && success_count >= 1 {
        match engine.analyzer.compress(&ctx).await {
            Ok(Some(guideline)) => {
                tracing::info!(
                    "REFLECTION: storing guideline id={} text=\"{}\"",
                    guideline.id,
                    guideline.guideline_text
                );
                engine
                    .store
                    .insert(&guideline.id, &ctx.instruction, &guideline.guideline_text)
                    .await?;
            }
            Ok(None) => {
                tracing::debug!("REFLECTION: analyzer produced no guideline");
            }
            Err(e) => {
                tracing::warn!("REFLECTION: compression failed: {e}");
            }
        }
    }

    Ok(())
}
