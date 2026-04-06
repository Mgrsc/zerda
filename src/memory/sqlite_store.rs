use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;

use super::types::{
    JournalMessage, LinkType, MemoryEntry, MemoryEntryExtra, MemoryKind, MemoryStatus, PendingTurn,
    RecallDebugInfo, TemporalWindow,
};

pub struct SqliteStore {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTurnBacklog {
    pub count: usize,
    pub oldest_created_at: Option<String>,
}

pub struct NewMemoryEntry<'a> {
    pub entry_id: &'a str,
    pub entity_id: &'a str,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub version_group_id: Option<&'a str>,
    pub supersedes_entry_id: Option<&'a str>,
    pub superseded_by_entry_id: Option<&'a str>,
    pub content: &'a str,
    pub normalized_content: Option<&'a str>,
    pub importance: f32,
    pub confidence: f32,
    pub source_turn_id: Option<&'a str>,
    pub source_session_id: Option<&'a str>,
    pub valid_from: Option<&'a str>,
    pub valid_to: Option<&'a str>,
    pub event_start_at: Option<&'a str>,
    pub event_end_at: Option<&'a str>,
    pub timezone: Option<&'a str>,
    pub extra_json: Option<&'a Value>,
}

impl SqliteStore {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create memory directory: {}", parent.display())
            })?;
        }
        let store = Self { path };
        store.init_schema()?;
        Ok(store)
    }

    pub fn append_turn_messages(
        &self,
        turn_id: &str,
        session_id: &str,
        entity_id: &str,
        channel: Option<&str>,
        messages: &[JournalMessage],
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        for message in messages {
            if message.content.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO memory_turn_journal (
                    turn_id,
                    session_id,
                    entity_id,
                    channel,
                    role,
                    content,
                    created_at,
                    processed_at,
                    extract_status
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 'pending')",
                params![
                    turn_id,
                    session_id,
                    entity_id,
                    channel,
                    message.role,
                    message.content,
                    now
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn claim_pending_turns(&self, limit: usize) -> Result<Vec<PendingTurn>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction()?;
        let mut turn_ids = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT turn_id
                 FROM memory_turn_journal
                 WHERE extract_status = 'pending'
                 ORDER BY id ASC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;
            for row in rows {
                turn_ids.push(row?);
            }
        }
        if turn_ids.is_empty() {
            tx.commit()?;
            return Ok(Vec::new());
        }
        let now = Utc::now().to_rfc3339();
        for turn_id in &turn_ids {
            tx.execute(
                "UPDATE memory_turn_journal
                 SET extract_status = 'processing', processed_at = ?2
                 WHERE turn_id = ?1 AND extract_status = 'pending'",
                params![turn_id, now],
            )?;
        }
        let turns = self.load_turns_with_status(&tx, &turn_ids)?;
        tx.commit()?;
        Ok(turns)
    }

    pub fn pending_turn_backlog(&self) -> Result<PendingTurnBacklog> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT COUNT(*), MIN(created_at)
             FROM (
                 SELECT turn_id, MIN(created_at) AS created_at
                 FROM memory_turn_journal
                 WHERE extract_status = 'pending'
                 GROUP BY turn_id
             )",
        )?;
        let backlog = stmt.query_row([], |row| {
            Ok(PendingTurnBacklog {
                count: row.get::<_, i64>(0)? as usize,
                oldest_created_at: row.get(1)?,
            })
        })?;
        Ok(backlog)
    }

    pub fn mark_turn_processed(&self, turn_id: &str) -> Result<()> {
        self.open()?.execute(
            "UPDATE memory_turn_journal
             SET extract_status = 'done', processed_at = ?2
             WHERE turn_id = ?1",
            params![turn_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn mark_turn_error(&self, turn_id: &str, message: &str) -> Result<()> {
        self.open()?.execute(
            "UPDATE memory_turn_journal
             SET extract_status = ?2, processed_at = ?3
             WHERE turn_id = ?1",
            params![
                turn_id,
                format!("error:{}", truncate_status(message)),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn insert_entry(&self, new_entry: NewMemoryEntry<'_>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let extra_json = new_entry.extra_json.map(Value::to_string);
        self.open()?.execute(
            "INSERT INTO memory_entries (
                entry_id,
                entity_id,
                kind,
                status,
                version_group_id,
                supersedes_entry_id,
                superseded_by_entry_id,
                content,
                normalized_content,
                importance,
                confidence,
                source_turn_id,
                source_session_id,
                created_at,
                updated_at,
                last_accessed_at,
                access_count,
                decay_score,
                valid_from,
                valid_to,
                event_start_at,
                event_end_at,
                timezone,
                extra_json
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?14, NULL, 0, 0.0, ?15, ?16, ?17, ?18, ?19, ?20
            )",
            params![
                new_entry.entry_id,
                new_entry.entity_id,
                new_entry.kind.as_str(),
                new_entry.status.as_str(),
                new_entry.version_group_id,
                new_entry.supersedes_entry_id,
                new_entry.superseded_by_entry_id,
                new_entry.content,
                new_entry.normalized_content,
                new_entry.importance,
                new_entry.confidence,
                new_entry.source_turn_id,
                new_entry.source_session_id,
                now,
                new_entry.valid_from,
                new_entry.valid_to,
                new_entry.event_start_at,
                new_entry.event_end_at,
                new_entry.timezone,
                extra_json,
            ],
        )?;
        Ok(())
    }

    pub fn insert_link(
        &self,
        link_id: &str,
        from_entry_id: &str,
        to_entry_id: &str,
        link_type: LinkType,
        weight: f32,
    ) -> Result<()> {
        self.open()?.execute(
            "INSERT OR IGNORE INTO memory_links (
                link_id,
                from_entry_id,
                to_entry_id,
                link_type,
                weight,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                link_id,
                from_entry_id,
                to_entry_id,
                link_type.as_str(),
                weight,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_entry_status(
        &self,
        entry_id: &str,
        status: MemoryStatus,
        superseded_by_entry_id: Option<&str>,
    ) -> Result<()> {
        self.open()?.execute(
            "UPDATE memory_entries
             SET status = ?2, superseded_by_entry_id = COALESCE(?3, superseded_by_entry_id), updated_at = ?4
             WHERE entry_id = ?1",
            params![
                entry_id,
                status.as_str(),
                superseded_by_entry_id,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn find_active_entries_by_version_group(
        &self,
        entity_id: &str,
        version_group_id: &str,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND version_group_id = ?2 AND status = 'active'
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![entity_id, version_group_id], map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn find_entry_by_normalized(
        &self,
        entity_id: &str,
        kind: MemoryKind,
        normalized_content: &str,
    ) -> Result<Option<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND kind = ?2 AND normalized_content = ?3 AND status = 'active'
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let entry = stmt
            .query_row(
                params![entity_id, kind.as_str(), normalized_content],
                map_memory_entry,
            )
            .optional()?;
        Ok(entry)
    }

    pub fn get_entry(&self, entry_id: &str) -> Result<Option<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT * FROM memory_entries WHERE entry_id = ?1 LIMIT 1")?;
        let entry = stmt
            .query_row(params![entry_id], map_memory_entry)
            .optional()?;
        Ok(entry)
    }

    pub fn query_temporal_candidates(
        &self,
        entity_id: &str,
        window: &TemporalWindow,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1
               AND status = 'active'
               AND kind IN ('event', 'commitment')
               AND event_start_at IS NOT NULL
               AND event_start_at >= ?2
               AND event_start_at < ?3
             ORDER BY event_start_at ASC, importance DESC
             LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                entity_id,
                window.start.to_rfc3339(),
                window.end.to_rfc3339(),
                limit as i64
            ],
            map_memory_entry,
        )?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn query_active_constraints(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND status = 'active' AND kind = 'constraint'
             ORDER BY importance DESC, created_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![entity_id, limit as i64], map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn query_active_preferences(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let entries = self.list_active_entries_by_kinds(
            entity_id,
            &[MemoryKind::Preference, MemoryKind::Constraint],
            limit.saturating_mul(3).max(limit),
        )?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for entry in entries {
            if seen.insert(entry.version_group_id().to_string()) {
                out.push(entry);
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn query_active_failure_patterns(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND status = 'active' AND kind = 'failure_pattern'
             ORDER BY importance DESC, created_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![entity_id, limit as i64], map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn list_active_entries_by_kinds(
        &self,
        entity_id: &str,
        kinds: &[MemoryKind],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if kinds.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND status = 'active' AND kind IN ({placeholders})
             ORDER BY importance DESC, created_at DESC
             LIMIT {limit}",
        );
        let mut bind_values = vec![entity_id.to_string()];
        bind_values.extend(kinds.iter().map(|kind| kind.as_str().to_string()));
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let params = bind_values.iter().map(String::as_str);
        let rows = stmt.query_map(rusqlite::params_from_iter(params), map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn list_recent_active_entries(
        &self,
        entity_id: &str,
        kinds: &[MemoryKind],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if kinds.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT * FROM memory_entries
             WHERE entity_id = ?1 AND status = 'active' AND kind IN ({placeholders})
             ORDER BY created_at DESC, importance DESC
             LIMIT {limit}",
        );
        let mut bind_values = vec![entity_id.to_string()];
        bind_values.extend(kinds.iter().map(|kind| kind.as_str().to_string()));
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let params = bind_values.iter().map(String::as_str);
        let rows = stmt.query_map(rusqlite::params_from_iter(params), map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn get_related_entries(
        &self,
        entry_id: &str,
        link_type: LinkType,
        outgoing: bool,
    ) -> Result<Vec<MemoryEntry>> {
        let (join_col, filter_col) = if outgoing {
            ("l.to_entry_id", "l.from_entry_id")
        } else {
            ("l.from_entry_id", "l.to_entry_id")
        };
        let sql = format!(
            "SELECT e.* FROM memory_entries e
             JOIN memory_links l ON e.entry_id = {join_col}
             WHERE {filter_col} = ?1 AND l.link_type = ?2
             ORDER BY l.weight DESC, e.created_at DESC"
        );
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![entry_id, link_type.as_str()], map_memory_entry)?;
        let entries = collect_entries(rows)?;
        Ok(entries)
    }

    pub fn increment_access(&self, entry_ids: &[String]) -> Result<()> {
        if entry_ids.is_empty() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.open()?;
        for entry_id in entry_ids {
            conn.execute(
                "UPDATE memory_entries
                 SET access_count = access_count + 1, last_accessed_at = ?2, updated_at = ?2
                 WHERE entry_id = ?1",
                params![entry_id, now],
            )?;
        }
        Ok(())
    }

    pub fn log_recall(&self, record: RecallLogRecord<'_>) -> Result<()> {
        self.open()?.execute(
            "INSERT INTO memory_recall_log (
                recall_id,
                entity_id,
                query_text,
                query_embedding_hash,
                query_time,
                candidate_entry_ids,
                selected_entry_ids,
                latency_ms,
                debug_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.recall_id,
                record.entity_id,
                record.query_text,
                record.query_embedding_hash,
                Utc::now().to_rfc3339(),
                serde_json::to_string(record.candidate_entry_ids)?,
                serde_json::to_string(record.selected_entry_ids)?,
                record.latency_ms as i64,
                serde_json::to_string(record.debug)?,
            ],
        )?;
        Ok(())
    }

    pub fn expire_due_entries(&self, now: DateTime<Local>) -> Result<Vec<MemoryEntry>> {
        let threshold = now.to_rfc3339();
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE status = 'active'
               AND (
                    (
                        kind IN ('event', 'commitment')
                        AND COALESCE(event_end_at, event_start_at) IS NOT NULL
                        AND COALESCE(event_end_at, event_start_at) < ?1
                    )
                 OR (valid_to IS NOT NULL AND valid_to < ?1)
               )",
        )?;
        let candidates = collect_entries(stmt.query_map(params![threshold], map_memory_entry)?)?;
        for entry in &candidates {
            conn.execute(
                "UPDATE memory_entries SET status = 'expired', updated_at = ?2 WHERE entry_id = ?1",
                params![entry.entry_id, Utc::now().to_rfc3339()],
            )?;
        }
        Ok(candidates)
    }

    pub fn archive_cold_entries(&self, older_than_days: i64) -> Result<Vec<MemoryEntry>> {
        let conn = self.open()?;
        let threshold = (Utc::now() - chrono::Duration::days(older_than_days)).to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT * FROM memory_entries
             WHERE status IN ('superseded', 'obsolete', 'expired')
               AND updated_at < ?1",
        )?;
        let candidates = collect_entries(stmt.query_map(params![threshold], map_memory_entry)?)?;
        for entry in &candidates {
            conn.execute(
                "UPDATE memory_entries SET status = 'archived', updated_at = ?2 WHERE entry_id = ?1",
                params![entry.entry_id, Utc::now().to_rfc3339()],
            )?;
        }
        Ok(candidates)
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn load_turns_with_status(
        &self,
        conn: &Connection,
        turn_ids: &[String],
    ) -> Result<Vec<PendingTurn>> {
        if turn_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = turn_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT turn_id, session_id, entity_id, channel, role, content, created_at
             FROM memory_turn_journal
             WHERE turn_id IN ({placeholders})
             ORDER BY id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(turn_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut grouped = BTreeMap::<String, PendingTurn>::new();
        for row in rows {
            let (turn_id, session_id, entity_id, channel, role, content, created_at) = row?;
            grouped
                .entry(turn_id.clone())
                .or_insert_with(|| PendingTurn {
                    turn_id: turn_id.clone(),
                    session_id,
                    entity_id,
                    channel,
                    created_at,
                    messages: Vec::new(),
                })
                .messages
                .push(JournalMessage { role, content });
        }
        Ok(grouped.into_values().collect())
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.open()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_turn_journal (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                channel TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                processed_at TEXT,
                extract_status TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_turn_journal_extract_status
                ON memory_turn_journal(extract_status, created_at);
            CREATE INDEX IF NOT EXISTS idx_memory_turn_journal_turn_id
                ON memory_turn_journal(turn_id);

            CREATE TABLE IF NOT EXISTS memory_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entry_id TEXT NOT NULL UNIQUE,
                entity_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                version_group_id TEXT,
                supersedes_entry_id TEXT,
                superseded_by_entry_id TEXT,
                content TEXT NOT NULL,
                normalized_content TEXT,
                importance REAL NOT NULL DEFAULT 0.0,
                confidence REAL NOT NULL DEFAULT 0.0,
                source_turn_id TEXT,
                source_session_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_accessed_at TEXT,
                access_count INTEGER NOT NULL DEFAULT 0,
                decay_score REAL NOT NULL DEFAULT 0.0,
                valid_from TEXT,
                valid_to TEXT,
                event_start_at TEXT,
                event_end_at TEXT,
                timezone TEXT,
                extra_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memory_entries_entity_status_kind
                ON memory_entries(entity_id, status, kind);
            CREATE INDEX IF NOT EXISTS idx_memory_entries_version_group
                ON memory_entries(entity_id, version_group_id, status);
            CREATE INDEX IF NOT EXISTS idx_memory_entries_event_start
                ON memory_entries(entity_id, event_start_at, status);

            CREATE TABLE IF NOT EXISTS memory_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                link_id TEXT NOT NULL UNIQUE,
                from_entry_id TEXT NOT NULL,
                to_entry_id TEXT NOT NULL,
                link_type TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_links_from
                ON memory_links(from_entry_id, link_type);
            CREATE INDEX IF NOT EXISTS idx_memory_links_to
                ON memory_links(to_entry_id, link_type);

            CREATE TABLE IF NOT EXISTS memory_recall_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                recall_id TEXT NOT NULL UNIQUE,
                entity_id TEXT NOT NULL,
                query_text TEXT NOT NULL,
                query_embedding_hash TEXT,
                query_time TEXT NOT NULL,
                candidate_entry_ids TEXT NOT NULL,
                selected_entry_ids TEXT NOT NULL,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                debug_json TEXT
            );

            CREATE TABLE IF NOT EXISTS memory_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id TEXT NOT NULL UNIQUE,
                job_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                status TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                error_text TEXT
            );
            ",
        )?;
        Ok(())
    }

    fn open(&self) -> Result<Connection> {
        Ok(Connection::open(&self.path)?)
    }
}

fn truncate_status(message: &str) -> String {
    let trimmed = message.trim();
    let max_chars = 80;
    let count = trimmed.chars().count();
    if count <= max_chars {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(max_chars).collect::<String>())
    }
}

fn collect_entries<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<MemoryEntry>>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<MemoryEntry>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn map_memory_entry(row: &Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let extra_json: Option<String> = row.get("extra_json")?;
    let extra = extra_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<MemoryEntryExtra>(raw).ok());
    Ok(MemoryEntry {
        entry_id: row.get("entry_id")?,
        entity_id: row.get("entity_id")?,
        kind: MemoryKind::from_str(&row.get::<_, String>("kind")?).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid memory kind",
                )),
            )
        })?,
        status: MemoryStatus::from_str(&row.get::<_, String>("status")?).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid memory status",
                )),
            )
        })?,
        version_group_id: row.get("version_group_id")?,
        supersedes_entry_id: row.get("supersedes_entry_id")?,
        superseded_by_entry_id: row.get("superseded_by_entry_id")?,
        content: row.get("content")?,
        normalized_content: row.get("normalized_content")?,
        importance: row.get("importance")?,
        confidence: row.get("confidence")?,
        source_turn_id: row.get("source_turn_id")?,
        source_session_id: row.get("source_session_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_accessed_at: row.get("last_accessed_at")?,
        access_count: row.get("access_count")?,
        decay_score: row.get("decay_score")?,
        valid_from: row.get("valid_from")?,
        valid_to: row.get("valid_to")?,
        event_start_at: row.get("event_start_at")?,
        event_end_at: row.get("event_end_at")?,
        timezone: row.get("timezone")?,
        extra,
    })
}

pub struct RecallLogRecord<'a> {
    pub recall_id: &'a str,
    pub entity_id: &'a str,
    pub query_text: &'a str,
    pub query_embedding_hash: &'a str,
    pub candidate_entry_ids: &'a [String],
    pub selected_entry_ids: &'a [String],
    pub latency_ms: u128,
    pub debug: &'a RecallDebugInfo,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::JournalMessage;

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}.sqlite3", name, uuid::Uuid::new_v4()))
    }

    #[test]
    fn creates_memory_sqlite_schema_on_init() {
        let path = temp_db_path("zerda-memory-schema");
        let store = SqliteStore::new(path.clone()).unwrap();
        let conn = Connection::open(store.path()).unwrap();

        for table in [
            "memory_turn_journal",
            "memory_entries",
            "memory_links",
            "memory_recall_log",
            "memory_jobs",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1);
        }

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn appends_turn_into_memory_turn_journal() {
        let path = temp_db_path("zerda-memory-journal");
        let store = SqliteStore::new(path.clone()).unwrap();
        let messages = vec![
            JournalMessage::new("user", "hello"),
            JournalMessage::new("assistant", "hi"),
        ];

        store
            .append_turn_messages("turn-1", "session-1", "entity-1", Some("cli"), &messages)
            .unwrap();

        let conn = Connection::open(store.path()).unwrap();
        let rows: Vec<(String, String, String)> = conn
            .prepare(
                "SELECT turn_id, role, extract_status
                 FROM memory_turn_journal
                 ORDER BY id ASC",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            rows,
            vec![
                (
                    "turn-1".to_string(),
                    "user".to_string(),
                    "pending".to_string()
                ),
                (
                    "turn-1".to_string(),
                    "assistant".to_string(),
                    "pending".to_string()
                )
            ]
        );

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn claims_pending_turns_grouped_by_turn() {
        let path = temp_db_path("zerda-memory-claim");
        let store = SqliteStore::new(path.clone()).unwrap();
        store
            .append_turn_messages(
                "turn-1",
                "session-1",
                "entity-1",
                Some("cli"),
                &[
                    JournalMessage::new("user", "hello"),
                    JournalMessage::new("assistant", "hi"),
                ],
            )
            .unwrap();
        store
            .append_turn_messages(
                "turn-2",
                "session-1",
                "entity-1",
                Some("cli"),
                &[JournalMessage::new("user", "bye")],
            )
            .unwrap();

        let claimed = store.claim_pending_turns(1).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].turn_id, "turn-1");
        assert_eq!(claimed[0].messages.len(), 2);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn counts_pending_user_turns_instead_of_journal_rows() {
        let path = temp_db_path("zerda-memory-backlog-count");
        let store = SqliteStore::new(path.clone()).unwrap();
        store
            .append_turn_messages(
                "turn-1",
                "session-1",
                "entity-1",
                Some("cli"),
                &[
                    JournalMessage::new("user", "hello"),
                    JournalMessage::new("assistant", "hi"),
                ],
            )
            .unwrap();
        store
            .append_turn_messages(
                "turn-2",
                "session-1",
                "entity-1",
                Some("cli"),
                &[JournalMessage::new("assistant", "only assistant row")],
            )
            .unwrap();
        store
            .append_turn_messages(
                "turn-3",
                "session-1",
                "entity-1",
                Some("cli"),
                &[JournalMessage::new("user", "bye")],
            )
            .unwrap();

        let backlog = store.pending_turn_backlog().unwrap();

        assert_eq!(backlog.count, 2);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn returns_oldest_pending_user_turn_timestamp() {
        let path = temp_db_path("zerda-memory-backlog-age");
        let store = SqliteStore::new(path.clone()).unwrap();
        store
            .append_turn_messages(
                "turn-1",
                "session-1",
                "entity-1",
                Some("cli"),
                &[JournalMessage::new("user", "first")],
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        store
            .append_turn_messages(
                "turn-2",
                "session-1",
                "entity-1",
                Some("cli"),
                &[JournalMessage::new("user", "second")],
            )
            .unwrap();

        let backlog = store.pending_turn_backlog().unwrap();
        let conn = Connection::open(store.path()).unwrap();
        let oldest_from_sql: String = conn
            .query_row(
                "SELECT MIN(created_at)
                 FROM memory_turn_journal
                 WHERE extract_status = 'pending' AND role = 'user'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(
            backlog.oldest_created_at.as_deref(),
            Some(oldest_from_sql.as_str())
        );

        std::fs::remove_file(path).ok();
    }
}
