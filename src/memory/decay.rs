use anyhow::Result;
use chrono::{DateTime, Duration, Local, Utc};

use super::chroma_store::ChromaStore;
use super::sqlite_store::SqliteStore;
use super::types::{MemoryEntry, MemoryKind, MemoryStatus};

const ACTIVE_DECAY_GRACE_DAYS: i64 = 7;
const ACTIVE_DECAY_LOOKBACK_DAYS: i64 = 21;
const ACTIVE_DECAY_LIMIT: usize = 256;

pub async fn run_decay(sqlite: &SqliteStore, chroma: &ChromaStore) -> Result<()> {
    let expired = sqlite.expire_due_entries(Local::now())?;
    for entry in &expired {
        remove_from_index(chroma, entry).await?;
    }
    let cooled = archive_cold_active_entries(sqlite)?;
    for entry in &cooled {
        remove_from_index(chroma, entry).await?;
    }
    let archived = sqlite.archive_cold_entries(14)?;
    for entry in &archived {
        remove_from_index(chroma, entry).await?;
    }
    Ok(())
}

fn archive_cold_active_entries(sqlite: &SqliteStore) -> Result<Vec<MemoryEntry>> {
    let now = Utc::now();
    let candidates = sqlite.list_active_entries_by_kinds(
        "self",
        &[
            MemoryKind::Preference,
            MemoryKind::ProfileFact,
            MemoryKind::Constraint,
            MemoryKind::Procedure,
            MemoryKind::FailurePattern,
            MemoryKind::Insight,
        ],
        ACTIVE_DECAY_LIMIT,
    )?;
    let mut archived = Vec::new();
    for entry in candidates {
        if !should_archive_active_entry(&entry, now) {
            continue;
        }
        sqlite.update_entry_status(&entry.entry_id, MemoryStatus::Archived, None)?;
        archived.push(entry);
    }
    Ok(archived)
}

async fn remove_from_index(chroma: &ChromaStore, entry: &MemoryEntry) -> Result<()> {
    chroma
        .delete_ids(
            entry.kind.collection_name(),
            std::slice::from_ref(&entry.entry_id),
        )
        .await
}

fn should_archive_active_entry(entry: &MemoryEntry, now: DateTime<Utc>) -> bool {
    if !entry.status.is_active_like() {
        return false;
    }
    let created_at = parse_rfc3339_utc(&entry.created_at).unwrap_or(now);
    if now - created_at < Duration::days(ACTIVE_DECAY_GRACE_DAYS) {
        return false;
    }
    let last_touch = entry
        .last_accessed_at
        .as_deref()
        .and_then(parse_rfc3339_utc)
        .unwrap_or(created_at);
    if now - last_touch < Duration::days(ACTIVE_DECAY_LOOKBACK_DAYS) {
        return false;
    }
    retention_score(entry, now) < archive_threshold(entry.kind.clone())
}

fn retention_score(entry: &MemoryEntry, now: DateTime<Utc>) -> f32 {
    let age_anchor = entry
        .last_accessed_at
        .as_deref()
        .and_then(parse_rfc3339_utc)
        .or_else(|| parse_rfc3339_utc(&entry.created_at))
        .unwrap_or(now);
    let age_days = (now - age_anchor).num_days().max(0) as f32;
    let access_score = ((entry.access_count as f32 + 1.0).ln() / 9.0_f32.ln()).clamp(0.0, 1.0);
    let recency_score = (1.0 / (1.0 + age_days / 21.0)).clamp(0.0, 1.0);
    entry.importance.clamp(0.0, 1.0) * 0.4
        + entry.confidence.clamp(0.0, 1.0) * 0.2
        + access_score * 0.25
        + recency_score * 0.15
}

fn archive_threshold(kind: MemoryKind) -> f32 {
    match kind {
        MemoryKind::Insight => 0.32,
        MemoryKind::Constraint => 0.34,
        MemoryKind::ProfileFact => 0.36,
        MemoryKind::Preference => 0.38,
        MemoryKind::Procedure => 0.4,
        MemoryKind::FailurePattern => 0.42,
        MemoryKind::Event | MemoryKind::Commitment => 1.0,
    }
}

fn parse_rfc3339_utc(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{MemoryEntryExtra, MemoryScope};

    #[test]
    fn keeps_recent_active_entries_even_with_low_score() {
        let now = Utc::now();
        let entry = test_entry(
            now - Duration::days(2),
            None,
            0,
            0.2,
            0.2,
            MemoryKind::Procedure,
        );

        assert!(!should_archive_active_entry(&entry, now));
    }

    #[test]
    fn archives_old_low_value_failure_pattern() {
        let now = Utc::now();
        let entry = test_entry(
            now - Duration::days(40),
            Some(now - Duration::days(30)),
            0,
            0.2,
            0.2,
            MemoryKind::FailurePattern,
        );

        assert!(should_archive_active_entry(&entry, now));
    }

    #[test]
    fn keeps_old_high_access_insight() {
        let now = Utc::now();
        let entry = test_entry(
            now - Duration::days(60),
            Some(now - Duration::days(25)),
            12,
            0.8,
            0.9,
            MemoryKind::Insight,
        );

        assert!(!should_archive_active_entry(&entry, now));
    }

    fn test_entry(
        created_at: DateTime<Utc>,
        last_accessed_at: Option<DateTime<Utc>>,
        access_count: i64,
        importance: f32,
        confidence: f32,
        kind: MemoryKind,
    ) -> MemoryEntry {
        MemoryEntry {
            entry_id: "entry-1".to_string(),
            entity_id: "self".to_string(),
            kind,
            status: MemoryStatus::Active,
            version_group_id: Some("group-1".to_string()),
            supersedes_entry_id: None,
            superseded_by_entry_id: None,
            content: "content".to_string(),
            normalized_content: Some("content".to_string()),
            importance,
            confidence,
            source_turn_id: Some("turn-1".to_string()),
            source_session_id: Some("session-1".to_string()),
            created_at: created_at.to_rfc3339(),
            updated_at: created_at.to_rfc3339(),
            last_accessed_at: last_accessed_at.map(|value| value.to_rfc3339()),
            access_count,
            decay_score: 0.0,
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            extra: Some(MemoryEntryExtra {
                version_key: Some("group-1".to_string()),
                memory_scope: Some(MemoryScope::Operational),
                axis: Some("group".to_string()),
                reason: None,
                source_excerpt: None,
                evidence_quote: Some("quote".to_string()),
                evidence_source: Some("assistant_quote".to_string()),
                evidence_verified: true,
                support_entry_ids: Vec::new(),
            }),
        }
    }
}
