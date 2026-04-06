use std::sync::Arc;

use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, MessageMetadata, MessageOrigin, Provider, Role,
};
use anyhow::{Context, Result};

use super::conflict::apply_proposal;
use super::ingest::{parse_json_response, remove_entry_from_index, sync_entry_to_index};
use super::sqlite_store::SqliteStore;
use super::types::{
    InsightExtractionOutput, LinkType, MemoryEntry, MemoryKind, MemoryProposal, MemoryScope,
};
use super::{chroma_store::ChromaStore, embed_client::EmbeddingClient};

struct ConsolidationInput<'a> {
    entity_id: &'a str,
    payload: &'a str,
    system_prompt: &'a str,
    memory_scope: Option<MemoryScope>,
}

pub async fn consolidate_entity(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    entity_id: &str,
) -> Result<()> {
    consolidate_personal_entity(sqlite, embedder, chroma, analyzer, entity_id).await?;
    consolidate_operational_entity(sqlite, embedder, chroma, analyzer, entity_id).await?;
    Ok(())
}

async fn consolidate_personal_entity(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    entity_id: &str,
) -> Result<()> {
    let support_entries = verified_support_entries(
        sqlite.list_recent_active_entries(
            entity_id,
            &[
                MemoryKind::Event,
                MemoryKind::Commitment,
                MemoryKind::Preference,
                MemoryKind::ProfileFact,
                MemoryKind::Constraint,
            ],
            16,
        )?,
        Some(MemoryScope::Personal),
    );
    if support_entries.len() < 3 {
        return Ok(());
    }
    let payload = render_support_payload(&support_entries);
    let system_prompt = r#"You consolidate stable personal facts into high-level insights for EMA memory.
Return JSON only:
{"insights":[{"content":"...","normalized_content":"...","importance":0.0,"confidence":0.0,"version_key":"...","support_entry_ids":["id1"],"reason":"..."}]}
Rules:
- Generate at most 2 insights.
- Only create insights that are supported by multiple facts.
- Do not restate a future event as an insight.
- support_entry_ids must reference the provided ids.
- Return {"insights":[]} if there is no stable pattern."#;
    persist_insights(
        sqlite,
        embedder,
        chroma,
        analyzer,
        ConsolidationInput {
            entity_id,
            payload: &payload,
            system_prompt,
            memory_scope: Some(MemoryScope::Personal),
        },
    )
    .await
}

async fn consolidate_operational_entity(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    entity_id: &str,
) -> Result<()> {
    let support_entries = verified_support_entries(
        sqlite.list_recent_active_entries(
            entity_id,
            &[MemoryKind::Procedure, MemoryKind::FailurePattern],
            12,
        )?,
        Some(MemoryScope::Operational),
    );
    if support_entries.len() < 2 {
        return Ok(());
    }
    let payload = render_support_payload(&support_entries);
    let system_prompt = r#"You consolidate reusable operational lessons into high-level insights for EMA memory.
Return JSON only:
{"insights":[{"content":"...","normalized_content":"...","importance":0.0,"confidence":0.0,"version_key":"...","support_entry_ids":["id1"],"reason":"..."}]}
Rules:
- Generate at most 2 insights.
- Only create an insight when multiple support items indicate a stable reusable strategy.
- Do not restate a single procedure or a single failure pattern.
- Prefer concise operational heuristics that improve future execution quality.
- support_entry_ids must reference the provided ids.
- Return {"insights":[]} if there is no stable pattern."#;
    persist_insights(
        sqlite,
        embedder,
        chroma,
        analyzer,
        ConsolidationInput {
            entity_id,
            payload: &payload,
            system_prompt,
            memory_scope: Some(MemoryScope::Operational),
        },
    )
    .await
}

async fn persist_insights(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    input: ConsolidationInput<'_>,
) -> Result<()> {
    let messages = vec![
        ConversationMessage {
            role: Role::System,
            content: vec![ContentPart::Text(input.system_prompt.to_string())],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata::default(),
        },
        ConversationMessage {
            role: Role::User,
            content: vec![ContentPart::Text(format!(
                "entity_id: {}\nactive_facts:\n{}",
                input.entity_id, input.payload
            ))],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata {
                origin: MessageOrigin::Human,
                related_job_id: None,
                related_turn_id: None,
                created_at: Some(chrono::Utc::now().to_rfc3339()),
            },
        },
    ];
    let response = analyzer
        .0
        .chat(&messages, &analyzer.1)
        .await
        .context("Memory consolidation request failed")?;
    let text = response
        .text
        .context("Memory consolidation model returned no text")?;
    let output = parse_json_response::<InsightExtractionOutput>(&text)?;
    for insight in output.insights {
        let proposal = MemoryProposal {
            kind: MemoryKind::Insight,
            content: insight.content,
            normalized_content: insight.normalized_content,
            importance: insight.importance,
            confidence: insight.confidence,
            memory_scope: input.memory_scope.clone(),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: insight.version_key,
            source_quote: None,
            evidence_source: None,
            reason: insight.reason,
            status_hint: None,
        };
        let outcome = apply_proposal(
            sqlite,
            input.entity_id,
            "sleep-consolidation",
            "sleep",
            &proposal,
        )?;
        for entry in outcome.deactivated {
            remove_entry_from_index(chroma, &entry).await?;
        }
        if let Some(inserted) = outcome.inserted {
            sync_entry_to_index(embedder, chroma, &inserted).await?;
            for support_entry_id in insight.support_entry_ids {
                if sqlite.get_entry(&support_entry_id)?.is_some() {
                    sqlite.insert_link(
                        &uuid::Uuid::new_v4().to_string(),
                        &inserted.entry_id,
                        &support_entry_id,
                        LinkType::Supports,
                        1.0,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn render_support_payload(entries: &[MemoryEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "- id={} kind={} content={}",
                entry.entry_id,
                entry.kind.as_str(),
                entry.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn verified_support_entries(
    entries: Vec<MemoryEntry>,
    scope: Option<MemoryScope>,
) -> Vec<MemoryEntry> {
    entries
        .into_iter()
        .filter(|entry| {
            let Some(extra) = entry.extra.as_ref() else {
                return false;
            };
            if !extra.evidence_verified {
                return false;
            }
            match scope.as_ref() {
                Some(expected_scope) => extra.memory_scope.as_ref() == Some(expected_scope),
                None => true,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::verified_support_entries;
    use crate::memory::types::{
        MemoryEntry, MemoryEntryExtra, MemoryKind, MemoryScope, MemoryStatus,
    };

    fn memory_entry_with_verified_evidence(entry_id: &str, evidence_verified: bool) -> MemoryEntry {
        MemoryEntry {
            entry_id: entry_id.to_string(),
            entity_id: "self".to_string(),
            kind: MemoryKind::Preference,
            status: MemoryStatus::Active,
            version_group_id: Some(format!("preference:{entry_id}")),
            supersedes_entry_id: None,
            superseded_by_entry_id: None,
            content: "content".to_string(),
            normalized_content: Some("content".to_string()),
            importance: 0.5,
            confidence: 0.9,
            source_turn_id: Some("turn-1".to_string()),
            source_session_id: Some("session-1".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            last_accessed_at: None,
            access_count: 0,
            decay_score: 0.0,
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            extra: Some(MemoryEntryExtra {
                version_key: Some(format!("preference:{entry_id}")),
                memory_scope: Some(MemoryScope::Personal),
                axis: Some(format!("preference:{entry_id}")),
                reason: None,
                source_excerpt: None,
                evidence_quote: Some("我喜欢吃不辣的".to_string()),
                evidence_source: Some("user_quote".to_string()),
                evidence_verified,
                support_entry_ids: Vec::new(),
            }),
        }
    }

    #[test]
    fn verified_support_entries_excludes_entries_without_verified_evidence() {
        let entries = vec![
            memory_entry_with_verified_evidence("verified", true),
            memory_entry_with_verified_evidence("unverified", false),
            MemoryEntry {
                extra: None,
                ..memory_entry_with_verified_evidence("legacy", false)
            },
        ];

        let filtered = verified_support_entries(entries, Some(MemoryScope::Personal));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].entry_id, "verified");
    }

    #[test]
    fn verified_support_entries_filters_by_scope() {
        let mut operational = memory_entry_with_verified_evidence("operational", true);
        operational.extra = Some(MemoryEntryExtra {
            memory_scope: Some(MemoryScope::Operational),
            ..operational.extra.clone().unwrap()
        });
        let entries = vec![
            memory_entry_with_verified_evidence("personal", true),
            operational,
        ];

        let filtered = verified_support_entries(entries, Some(MemoryScope::Operational));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].entry_id, "operational");
    }
}
