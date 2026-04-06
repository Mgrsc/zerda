use anyhow::Result;
use serde_json::json;
use uuid::Uuid;

use super::sqlite_store::{NewMemoryEntry, SqliteStore};
use super::types::{LinkType, MemoryEntry, MemoryKind, MemoryProposal, MemoryStatus};

pub struct ConflictOutcome {
    pub inserted: Option<MemoryEntry>,
    pub deactivated: Vec<MemoryEntry>,
}

pub fn apply_proposal(
    sqlite: &SqliteStore,
    entity_id: &str,
    source_turn_id: &str,
    source_session_id: &str,
    proposal: &MemoryProposal,
) -> Result<ConflictOutcome> {
    let version_group_id = proposal.version_key.trim();
    let axis = axis_from_version_key(version_group_id);
    let active = active_entries_for_axis(sqlite, entity_id, proposal, axis.as_deref())?;
    if matches!(proposal.status_hint, Some(MemoryStatus::Cancelled)) {
        for entry in &active {
            sqlite.update_entry_status(&entry.entry_id, MemoryStatus::Cancelled, None)?;
        }
        return Ok(ConflictOutcome {
            inserted: None,
            deactivated: active,
        });
    }

    if sqlite
        .find_entry_by_normalized(
            entity_id,
            proposal.kind.clone(),
            &proposal.normalized_content,
        )?
        .is_some()
    {
        return Ok(ConflictOutcome {
            inserted: None,
            deactivated: Vec::new(),
        });
    }

    let entry_id = Uuid::new_v4().to_string();
    let extra_json = json!({
        "version_key": proposal.version_key,
        "memory_scope": proposal.memory_scope,
        "axis": axis,
        "reason": proposal.reason,
        "source_excerpt": proposal.content,
        "evidence_quote": proposal.source_quote,
        "evidence_source": proposal.evidence_source,
        "evidence_verified": proposal.source_quote.is_some(),
    });
    let supersedes_entry_id = active.first().map(|entry| entry.entry_id.as_str());
    sqlite.insert_entry(NewMemoryEntry {
        entry_id: &entry_id,
        entity_id,
        kind: proposal.kind.clone(),
        status: MemoryStatus::Active,
        version_group_id: Some(version_group_id),
        supersedes_entry_id,
        superseded_by_entry_id: None,
        content: &proposal.content,
        normalized_content: Some(&proposal.normalized_content),
        importance: proposal.importance,
        confidence: proposal.confidence,
        source_turn_id: Some(source_turn_id),
        source_session_id: Some(source_session_id),
        valid_from: proposal.valid_from.as_deref(),
        valid_to: proposal.valid_to.as_deref(),
        event_start_at: proposal.event_start_at.as_deref(),
        event_end_at: proposal.event_end_at.as_deref(),
        timezone: proposal.timezone.as_deref(),
        extra_json: Some(&extra_json),
    })?;

    for entry in &active {
        let next_status = if matches!(proposal.kind, MemoryKind::Insight) {
            MemoryStatus::Obsolete
        } else {
            MemoryStatus::Superseded
        };
        sqlite.update_entry_status(&entry.entry_id, next_status, Some(&entry_id))?;
        sqlite.insert_link(
            &Uuid::new_v4().to_string(),
            &entry_id,
            &entry.entry_id,
            LinkType::Supersedes,
            1.0,
        )?;
    }

    let inserted = sqlite.get_entry(&entry_id)?;
    Ok(ConflictOutcome {
        inserted,
        deactivated: active,
    })
}

fn axis_from_version_key(version_key: &str) -> Option<String> {
    let trimmed = version_key.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts = trimmed.split(':').collect::<Vec<_>>();
    if parts.len() <= 2 {
        return Some(trimmed.to_string());
    }
    Some(parts[..parts.len() - 1].join(":"))
}

fn active_entries_for_axis(
    sqlite: &SqliteStore,
    entity_id: &str,
    proposal: &MemoryProposal,
    axis: Option<&str>,
) -> Result<Vec<MemoryEntry>> {
    if let Some(axis) = axis {
        let candidates = sqlite.list_active_entries_by_kinds(
            entity_id,
            std::slice::from_ref(&proposal.kind),
            128,
        )?;
        let scoped = candidates
            .into_iter()
            .filter(|entry| {
                entry
                    .extra
                    .as_ref()
                    .and_then(|extra| extra.axis.as_deref())
                    .or(entry.version_group_id.as_deref())
                    .map(|entry_axis| {
                        entry_axis == axis
                            || entry.version_group_id() == proposal.version_key.as_str()
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        return Ok(scoped);
    }
    sqlite.find_active_entries_by_version_group(entity_id, proposal.version_key.as_str())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::memory::sqlite_store::{NewMemoryEntry, SqliteStore};
    use crate::memory::types::MemoryScope;

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}.sqlite3", name, uuid::Uuid::new_v4()))
    }

    #[test]
    fn supersedes_previous_active_entry_on_same_version_group() {
        let path = temp_db_path("zerda-memory-conflict");
        let store = SqliteStore::new(path.clone()).unwrap();
        store
            .insert_entry(NewMemoryEntry {
                entry_id: "old-entry",
                entity_id: "entity-1",
                kind: MemoryKind::Preference,
                status: MemoryStatus::Active,
                version_group_id: Some("preference:food:spicy"),
                supersedes_entry_id: None,
                superseded_by_entry_id: None,
                content: "用户不爱吃辣",
                normalized_content: Some("用户不爱吃辣"),
                importance: 0.8,
                confidence: 0.9,
                source_turn_id: Some("turn-1"),
                source_session_id: Some("session-1"),
                valid_from: None,
                valid_to: None,
                event_start_at: None,
                event_end_at: None,
                timezone: None,
                extra_json: None,
            })
            .unwrap();

        let outcome = apply_proposal(
            &store,
            "entity-1",
            "turn-2",
            "session-1",
            &MemoryProposal {
                kind: MemoryKind::Preference,
                content: "用户爱吃辣".to_string(),
                normalized_content: "用户爱吃辣".to_string(),
                importance: 0.9,
                confidence: 0.95,
                memory_scope: Some(MemoryScope::Personal),
                valid_from: None,
                valid_to: None,
                event_start_at: None,
                event_end_at: None,
                timezone: None,
                version_key: "preference:food:spicy".to_string(),
                source_quote: Some("我爱吃辣".to_string()),
                evidence_source: Some("user_quote".to_string()),
                reason: Some("user updated preference".to_string()),
                status_hint: None,
            },
        )
        .unwrap();

        let old_entry = store.get_entry("old-entry").unwrap().unwrap();
        assert_eq!(old_entry.status, MemoryStatus::Superseded);
        assert_eq!(outcome.deactivated.len(), 1);
        assert!(outcome.inserted.is_some());

        std::fs::remove_file(path).ok();
    }
}
