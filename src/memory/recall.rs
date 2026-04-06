use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Instant;

use anyhow::Result;
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Weekday,
};
use regex::Regex;
use uuid::Uuid;

use crate::logging::text_fingerprint;

use super::chroma_store::ChromaStore;
use super::embed_client::EmbeddingClient;
use super::sqlite_store::{RecallLogRecord, SqliteStore};
use super::types::{
    CandidateSource, IntentAnalysis, MemoryEntry, MemoryKind, MemoryPromptBlocks, RecallCandidate,
    RecallCandidateDebug, RecallDebugInfo, RecallResult, RecallTemplate, TemporalWindow,
};

static ISO_DATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<y>\d{4})-(?P<m>\d{1,2})-(?P<d>\d{1,2})").expect("valid iso date regex")
});
static MONTH_DAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<m>\d{1,2})月(?P<d>\d{1,2})日").expect("valid month day regex")
});
static TIME_COLON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<h>\d{1,2}):(?P<m>\d{2})").expect("valid time regex"));
static CN_HOUR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(上午|中午|下午|晚上)?(?P<h>\d{1,2})点(半)?").expect("valid chinese hour regex")
});

const DEFAULT_FACT_CANDIDATES: usize = 8;
const DEFAULT_INSIGHT_CANDIDATES: usize = 4;
const DEFAULT_FAILURE_CANDIDATES: usize = 4;
const DEFAULT_PROCEDURE_CANDIDATES: usize = 4;

pub async fn recall(
    sqlite: &SqliteStore,
    chroma: &ChromaStore,
    embedder: &EmbeddingClient,
    entity_id: &str,
    query: &str,
) -> Result<Option<(MemoryPromptBlocks, RecallResult)>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    let started = Instant::now();
    let now = Local::now();
    let intent = analyze_intent(query, now);
    let query_embedding_text = build_query_embedding_text(query, &intent);
    let query_embedding = embedder.embed_text(&query_embedding_text).await?;

    let mut candidates = Vec::new();
    if let Some(window) = &intent.temporal_window {
        candidates.extend(
            sqlite
                .query_temporal_candidates(entity_id, window, DEFAULT_FACT_CANDIDATES)?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.85)),
        );
    }
    if matches!(intent.template, RecallTemplate::Open) {
        candidates.extend(
            sqlite
                .list_recent_active_entries(
                    entity_id,
                    &[
                        MemoryKind::Commitment,
                        MemoryKind::Preference,
                        MemoryKind::ProfileFact,
                        MemoryKind::Constraint,
                    ],
                    DEFAULT_FACT_CANDIDATES,
                )?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.8)),
        );
    }
    if intent.asks_constraints {
        candidates.extend(
            sqlite
                .query_active_constraints(entity_id, DEFAULT_FACT_CANDIDATES)?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.82)),
        );
        candidates.extend(
            sqlite
                .query_active_preferences(entity_id, DEFAULT_FACT_CANDIDATES)?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.78)),
        );
    }
    if intent.asks_procedure {
        candidates.extend(
            sqlite
                .query_active_constraints(entity_id, DEFAULT_PROCEDURE_CANDIDATES)?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.76)),
        );
    }
    if intent.asks_procedure {
        for result in chroma
            .query(
                MemoryKind::Procedure.collection_name(),
                entity_id,
                &query_embedding,
                DEFAULT_PROCEDURE_CANDIDATES,
            )
            .await?
        {
            if let Some(entry) = sqlite.get_entry(&result.entry_id)? {
                candidates.push(build_candidate(
                    entry,
                    CandidateSource::ChromaProcedures,
                    &intent,
                    semantic_from_distance(result.distance),
                ));
            }
        }
    }
    if intent.asks_debug_recovery {
        candidates.extend(
            sqlite
                .query_active_failure_patterns(entity_id, DEFAULT_FAILURE_CANDIDATES)?
                .into_iter()
                .map(|entry| build_candidate(entry, CandidateSource::SqliteExact, &intent, 0.86)),
        );
        for result in chroma
            .query(
                MemoryKind::FailurePattern.collection_name(),
                entity_id,
                &query_embedding,
                DEFAULT_FAILURE_CANDIDATES,
            )
            .await?
        {
            if let Some(entry) = sqlite.get_entry(&result.entry_id)? {
                candidates.push(build_candidate(
                    entry,
                    CandidateSource::ChromaFailures,
                    &intent,
                    semantic_from_distance(result.distance),
                ));
            }
        }
    }
    for result in chroma
        .query(
            MemoryKind::Event.collection_name(),
            entity_id,
            &query_embedding,
            DEFAULT_FACT_CANDIDATES,
        )
        .await?
    {
        if let Some(entry) = sqlite.get_entry(&result.entry_id)? {
            candidates.push(build_candidate(
                entry,
                CandidateSource::ChromaFacts,
                &intent,
                semantic_from_distance(result.distance),
            ));
        }
    }
    for result in chroma
        .query(
            MemoryKind::Insight.collection_name(),
            entity_id,
            &query_embedding,
            DEFAULT_INSIGHT_CANDIDATES,
        )
        .await?
    {
        if let Some(entry) = sqlite.get_entry(&result.entry_id)? {
            candidates.push(build_candidate(
                entry,
                CandidateSource::ChromaInsights,
                &intent,
                semantic_from_distance(result.distance),
            ));
        }
    }

    let candidates = expand_relations(sqlite, &intent, candidates)?;
    let ranked = rerank(intent.clone(), candidates);

    let mut facts = Vec::new();
    let mut insights = Vec::new();
    let mut failures = Vec::new();
    let mut procedures = Vec::new();
    let mut selected_ids = Vec::new();
    for candidate in &ranked {
        match candidate.entry.kind {
            MemoryKind::Insight => {
                if insights.len() < 2 {
                    insights.push(candidate.entry.clone());
                    selected_ids.push(candidate.entry.entry_id.clone());
                }
            }
            MemoryKind::FailurePattern => {
                if failures.len() < 3 {
                    failures.push(candidate.entry.clone());
                    selected_ids.push(candidate.entry.entry_id.clone());
                }
            }
            MemoryKind::Procedure => {
                if procedures.len() < 3 {
                    procedures.push(candidate.entry.clone());
                    selected_ids.push(candidate.entry.entry_id.clone());
                }
            }
            _ => {
                if facts.len() < 6 {
                    facts.push(candidate.entry.clone());
                    selected_ids.push(candidate.entry.entry_id.clone());
                }
            }
        }
    }
    if facts.is_empty() && insights.is_empty() && failures.is_empty() && procedures.is_empty() {
        return Ok(None);
    }

    sqlite.increment_access(&selected_ids)?;
    let debug = RecallDebugInfo {
        template: match intent.template {
            RecallTemplate::Temporal => "temporal".to_string(),
            RecallTemplate::Preference => "preference".to_string(),
            RecallTemplate::Troubleshooting => "troubleshooting".to_string(),
            RecallTemplate::Open => "open".to_string(),
        },
        query: query.to_string(),
        temporal_window: intent
            .temporal_window
            .as_ref()
            .map(|window| format!("{} -> {}", window.start, window.end)),
        candidates: ranked.iter().map(to_debug_candidate).collect(),
    };
    let candidate_entry_ids = ranked
        .iter()
        .map(|candidate| candidate.entry.entry_id.clone())
        .collect::<Vec<_>>();
    let recall_id = Uuid::new_v4().to_string();
    let query_embedding_hash = text_fingerprint(&query_embedding_text);
    sqlite.log_recall(RecallLogRecord {
        recall_id: &recall_id,
        entity_id,
        query_text: query,
        query_embedding_hash: &query_embedding_hash,
        candidate_entry_ids: &candidate_entry_ids,
        selected_entry_ids: &selected_ids,
        latency_ms: started.elapsed().as_millis(),
        debug: &debug,
    })?;

    let prompt = MemoryPromptBlocks {
        facts: facts.iter().map(render_entry).collect(),
        insights: insights.iter().map(render_entry).collect(),
        failures: failures.iter().map(render_entry).collect(),
        procedures: procedures.iter().map(render_entry).collect(),
    };
    Ok(Some((
        prompt,
        RecallResult {
            facts,
            insights,
            failures,
            procedures,
            debug,
        },
    )))
}

pub fn analyze_intent(query: &str, now: DateTime<Local>) -> IntentAnalysis {
    let temporal_window = detect_temporal_window(query, now);
    let asks_constraints = contains_preference_probe(query);
    let asks_procedure = contains_procedure_probe(query);
    let asks_debug_recovery = contains_debug_probe(query);
    let template = if temporal_window.is_some() {
        RecallTemplate::Temporal
    } else if asks_debug_recovery {
        RecallTemplate::Troubleshooting
    } else if asks_constraints {
        RecallTemplate::Preference
    } else {
        RecallTemplate::Open
    };
    IntentAnalysis {
        template,
        temporal_window,
        asks_constraints,
        asks_procedure,
        asks_debug_recovery,
    }
}

fn contains_preference_probe(query: &str) -> bool {
    [
        "喜欢",
        "爱吃",
        "爱喝",
        "讨厌",
        "忌口",
        "过敏",
        "能不能吃",
        "适不适合",
        "偏好",
        "口味",
    ]
    .iter()
    .any(|probe| query.contains(probe))
}

fn contains_procedure_probe(query: &str) -> bool {
    ["怎么", "如何", "步骤", "流程", "安排", "报销", "处理"]
        .iter()
        .any(|probe| query.contains(probe))
}

fn contains_debug_probe(query: &str) -> bool {
    [
        "报错",
        "错误",
        "失败",
        "修复",
        "恢复",
        "排查",
        "为什么不行",
        "why failed",
        "debug",
        "error",
        "fix",
        "recover",
        "troubleshoot",
    ]
    .iter()
    .any(|probe| query.to_ascii_lowercase().contains(probe))
}

fn detect_temporal_window(query: &str, now: DateTime<Local>) -> Option<TemporalWindow> {
    if query.contains("今天") {
        return Some(day_window("今天", now.date_naive(), query, now));
    }
    if query.contains("明天") {
        return Some(day_window(
            "明天",
            now.date_naive() + Duration::days(1),
            query,
            now,
        ));
    }
    if query.contains("后天") {
        return Some(day_window(
            "后天",
            now.date_naive() + Duration::days(2),
            query,
            now,
        ));
    }
    if query.contains("这周") || query.contains("本周") {
        if let Some(weekday) = detect_weekday(query) {
            return Some(day_window(
                "这周",
                resolve_weekday_in_week(now.date_naive(), weekday, 0),
                query,
                now,
            ));
        }
        return Some(week_window("这周", now, 0));
    }
    if query.contains("下周") {
        if let Some(weekday) = detect_weekday(query) {
            return Some(day_window(
                "下周",
                resolve_weekday_in_week(now.date_naive(), weekday, 1),
                query,
                now,
            ));
        }
        return Some(week_window("下周", now, 1));
    }
    if let Some(date) = parse_iso_or_month_day(query, now) {
        return Some(day_window("绝对日期", date, query, now));
    }
    if let Some(weekday) = detect_weekday(query) {
        return Some(day_window(
            "周内日期",
            resolve_next_weekday(now.date_naive(), weekday),
            query,
            now,
        ));
    }
    None
}

fn day_window(label: &str, date: NaiveDate, query: &str, now: DateTime<Local>) -> TemporalWindow {
    let base_start = localize(
        date.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("midnight")),
        now,
    );
    let base_end = base_start + Duration::days(1);
    if let Some(hour) = parse_hour_hint(query) {
        let start = localize(date.and_time(hour), now);
        return TemporalWindow {
            label: label.to_string(),
            start,
            end: start + Duration::hours(2),
        };
    }
    TemporalWindow {
        label: label.to_string(),
        start: base_start,
        end: base_end,
    }
}

fn week_window(label: &str, now: DateTime<Local>, offset_weeks: i64) -> TemporalWindow {
    let monday = now.date_naive() - Duration::days(now.weekday().num_days_from_monday() as i64)
        + Duration::weeks(offset_weeks);
    let start = localize(
        monday.and_time(NaiveTime::from_hms_opt(0, 0, 0).expect("midnight")),
        now,
    );
    TemporalWindow {
        label: label.to_string(),
        start,
        end: start + Duration::days(7),
    }
}

fn parse_iso_or_month_day(query: &str, now: DateTime<Local>) -> Option<NaiveDate> {
    if let Some(captures) = ISO_DATE_RE.captures(query) {
        return NaiveDate::from_ymd_opt(
            captures.name("y")?.as_str().parse().ok()?,
            captures.name("m")?.as_str().parse().ok()?,
            captures.name("d")?.as_str().parse().ok()?,
        );
    }
    let captures = MONTH_DAY_RE.captures(query)?;
    NaiveDate::from_ymd_opt(
        now.year(),
        captures.name("m")?.as_str().parse().ok()?,
        captures.name("d")?.as_str().parse().ok()?,
    )
}

fn parse_hour_hint(query: &str) -> Option<NaiveTime> {
    if let Some(captures) = TIME_COLON_RE.captures(query) {
        return NaiveTime::from_hms_opt(
            captures.name("h")?.as_str().parse().ok()?,
            captures.name("m")?.as_str().parse().ok()?,
            0,
        );
    }
    let captures = CN_HOUR_RE.captures(query)?;
    let mut hour: u32 = captures.name("h")?.as_str().parse().ok()?;
    let prefix = captures.get(1).map(|v| v.as_str()).unwrap_or("");
    if matches!(prefix, "下午" | "晚上") && hour < 12 {
        hour += 12;
    }
    let minute = if captures.get(3).is_some() { 30 } else { 0 };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

fn detect_weekday(query: &str) -> Option<Weekday> {
    [
        ("周一", Weekday::Mon),
        ("星期一", Weekday::Mon),
        ("周二", Weekday::Tue),
        ("星期二", Weekday::Tue),
        ("周三", Weekday::Wed),
        ("星期三", Weekday::Wed),
        ("周四", Weekday::Thu),
        ("星期四", Weekday::Thu),
        ("周五", Weekday::Fri),
        ("星期五", Weekday::Fri),
        ("周六", Weekday::Sat),
        ("星期六", Weekday::Sat),
        ("周日", Weekday::Sun),
        ("星期日", Weekday::Sun),
        ("周天", Weekday::Sun),
        ("星期天", Weekday::Sun),
    ]
    .iter()
    .find_map(|(probe, weekday)| query.contains(probe).then_some(*weekday))
}

fn resolve_next_weekday(date: NaiveDate, weekday: Weekday) -> NaiveDate {
    let current = date.weekday().num_days_from_monday() as i64;
    let target = weekday.num_days_from_monday() as i64;
    let mut delta = target - current;
    if delta < 0 {
        delta += 7;
    }
    date + Duration::days(delta)
}

fn resolve_weekday_in_week(date: NaiveDate, weekday: Weekday, week_offset: i64) -> NaiveDate {
    let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64)
        + Duration::weeks(week_offset);
    monday + Duration::days(weekday.num_days_from_monday() as i64)
}

fn localize(naive: chrono::NaiveDateTime, fallback: DateTime<Local>) -> DateTime<Local> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => fallback,
    }
}

fn build_query_embedding_text(query: &str, intent: &IntentAnalysis) -> String {
    match intent.template {
        RecallTemplate::Temporal => format!("temporal memory query: {query}"),
        RecallTemplate::Preference => format!("preference or constraint query: {query}"),
        RecallTemplate::Troubleshooting => format!("troubleshooting query: {query}"),
        RecallTemplate::Open => format!("open memory query: {query}"),
    }
}

fn semantic_from_distance(distance: f32) -> f32 {
    (1.0 - distance).clamp(0.0, 1.0)
}

fn build_candidate(
    entry: MemoryEntry,
    source: CandidateSource,
    intent: &IntentAnalysis,
    semantic_score: f32,
) -> RecallCandidate {
    let type_match = score_type_match(&entry, intent);
    let temporal_match = score_temporal_match(&entry, intent.temporal_window.as_ref());
    let importance_boost = entry.importance.clamp(0.0, 1.0) * 0.15;
    let recency_boost = score_recency(&entry, intent);
    let relation_boost = 0.0;
    let stale_penalty = score_stale_penalty(&entry);
    RecallCandidate {
        entry,
        source,
        semantic_score,
        type_match,
        temporal_match,
        importance_boost,
        recency_boost,
        relation_boost,
        stale_penalty,
        final_score: 0.0,
    }
}

fn expand_relations(
    sqlite: &SqliteStore,
    intent: &IntentAnalysis,
    candidates: Vec<RecallCandidate>,
) -> Result<Vec<RecallCandidate>> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    let mut to_expand = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.entry.entry_id.clone()) {
            if matches!(candidate.entry.kind, MemoryKind::Insight) {
                to_expand.push(candidate.entry.entry_id.clone());
            }
            merged.push(candidate);
        }
    }
    for entry_id in to_expand {
        for related in
            sqlite.get_related_entries(&entry_id, super::types::LinkType::Supports, true)?
        {
            if seen.insert(related.entry_id.clone()) {
                let mut candidate =
                    build_candidate(related, CandidateSource::RelationExpansion, intent, 0.66);
                candidate.relation_boost = 0.2;
                merged.push(candidate);
            }
        }
    }
    Ok(merged)
}

fn rerank(intent: IntentAnalysis, candidates: Vec<RecallCandidate>) -> Vec<RecallCandidate> {
    let mut dedup = HashMap::<String, RecallCandidate>::new();
    for mut candidate in candidates {
        if !candidate.entry.status.is_active_like() {
            continue;
        }
        if matches!(
            candidate.entry.status,
            super::types::MemoryStatus::Cancelled | super::types::MemoryStatus::Expired
        ) {
            continue;
        }
        candidate.final_score = match intent.template {
            RecallTemplate::Temporal => {
                0.25 * candidate.semantic_score
                    + 0.40 * candidate.temporal_match
                    + 0.20 * candidate.type_match
                    + 0.05 * candidate.importance_boost
                    + 0.05 * candidate.recency_boost
                    + 0.05 * candidate.relation_boost
                    - candidate.stale_penalty
            }
            RecallTemplate::Preference => {
                0.40 * candidate.semantic_score
                    + 0.25 * candidate.type_match
                    + 0.15 * candidate.importance_boost
                    + 0.10 * candidate.recency_boost
                    + 0.10 * candidate.relation_boost
                    - candidate.stale_penalty
            }
            RecallTemplate::Troubleshooting => {
                0.38 * candidate.semantic_score
                    + 0.25 * candidate.type_match
                    + 0.12 * candidate.importance_boost
                    + 0.10 * candidate.recency_boost
                    + 0.10 * candidate.relation_boost
                    + 0.05 * candidate.temporal_match
                    - candidate.stale_penalty
            }
            RecallTemplate::Open => {
                0.45 * candidate.semantic_score
                    + 0.20 * candidate.relation_boost
                    + 0.10 * candidate.type_match
                    + 0.10 * candidate.importance_boost
                    + 0.10 * candidate.recency_boost
                    + 0.05 * candidate.temporal_match
                    - candidate.stale_penalty
            }
        };
        if matches!(intent.template, RecallTemplate::Temporal)
            && matches!(candidate.entry.kind, MemoryKind::Event)
            && candidate.temporal_match >= 1.0
        {
            candidate.final_score += 0.15;
        }
        if matches!(intent.template, RecallTemplate::Temporal)
            && matches!(candidate.entry.kind, MemoryKind::Commitment)
            && candidate.temporal_match >= 1.0
        {
            candidate.final_score += 0.10;
        }
        if matches!(candidate.entry.kind, MemoryKind::Constraint) {
            candidate.final_score += 0.20;
        }
        if matches!(candidate.entry.kind, MemoryKind::Preference) {
            candidate.final_score += 0.08;
        }
        if matches!(intent.template, RecallTemplate::Troubleshooting)
            && matches!(candidate.entry.kind, MemoryKind::FailurePattern)
        {
            candidate.final_score += 0.22;
        }
        if matches!(intent.template, RecallTemplate::Troubleshooting)
            && matches!(candidate.entry.kind, MemoryKind::Procedure)
        {
            candidate.final_score += 0.10;
        }
        dedup
            .entry(candidate.entry.entry_id.clone())
            .and_modify(|existing| {
                if candidate.final_score > existing.final_score {
                    *existing = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    let mut ranked = dedup.into_values().collect::<Vec<_>>();
    ranked.sort_by(compare_candidates);
    ranked
}

fn score_type_match(entry: &MemoryEntry, intent: &IntentAnalysis) -> f32 {
    match intent.template {
        RecallTemplate::Temporal => match entry.kind {
            MemoryKind::Event => 1.0,
            MemoryKind::Commitment => 0.9,
            MemoryKind::Constraint => 0.2,
            MemoryKind::Insight => 0.1,
            MemoryKind::Procedure => 0.1,
            _ => 0.3,
        },
        RecallTemplate::Preference => match entry.kind {
            MemoryKind::Constraint => 1.0,
            MemoryKind::Preference => 0.9,
            MemoryKind::ProfileFact => 0.5,
            MemoryKind::Insight => 0.25,
            _ => 0.2,
        },
        RecallTemplate::Troubleshooting => match entry.kind {
            MemoryKind::FailurePattern => 1.0,
            MemoryKind::Procedure => 0.8,
            MemoryKind::Constraint => 0.3,
            MemoryKind::Insight => 0.2,
            _ => 0.15,
        },
        RecallTemplate::Open => match entry.kind {
            MemoryKind::Insight => 1.0,
            MemoryKind::ProfileFact => 0.95,
            MemoryKind::Commitment => 0.85,
            MemoryKind::Constraint => 0.82,
            MemoryKind::Preference => 0.78,
            MemoryKind::Procedure => 0.35,
            MemoryKind::FailurePattern => 0.2,
            MemoryKind::Event => 0.58,
        },
    }
}

fn score_temporal_match(entry: &MemoryEntry, window: Option<&TemporalWindow>) -> f32 {
    let Some(window) = window else {
        return 0.0;
    };
    let Some(event_start_at) = entry.event_start_at.as_deref() else {
        return 0.0;
    };
    let Ok(event_start) = DateTime::parse_from_rfc3339(event_start_at) else {
        return 0.0;
    };
    let event_start = event_start.with_timezone(&Local);
    if event_start >= window.start && event_start < window.end {
        return 1.0;
    }
    let delta = (event_start - window.start).num_hours().unsigned_abs();
    if delta <= 24 {
        0.7
    } else if event_start > Local::now() {
        0.3
    } else {
        0.0
    }
}

fn score_recency(entry: &MemoryEntry, intent: &IntentAnalysis) -> f32 {
    let Ok(created_at) = DateTime::parse_from_rfc3339(&entry.created_at) else {
        return 0.0;
    };
    let age_days = (Local::now() - created_at.with_timezone(&Local))
        .num_days()
        .max(0) as f32;
    let base = (1.0 / (1.0 + age_days / 14.0)).clamp(0.0, 1.0);
    match intent.template {
        RecallTemplate::Preference => {
            if matches!(entry.kind, MemoryKind::Preference | MemoryKind::Constraint) {
                base
            } else {
                base * 0.25
            }
        }
        RecallTemplate::Troubleshooting => {
            if matches!(
                entry.kind,
                MemoryKind::FailurePattern | MemoryKind::Procedure
            ) {
                base
            } else {
                base * 0.2
            }
        }
        RecallTemplate::Open => base * 0.5,
        RecallTemplate::Temporal => base * 0.4,
    }
}

fn score_stale_penalty(entry: &MemoryEntry) -> f32 {
    if matches!(entry.kind, MemoryKind::Insight) && entry.access_count == 0 {
        0.04
    } else {
        0.0
    }
}

fn render_entry(entry: &MemoryEntry) -> String {
    match entry.kind {
        MemoryKind::Event => {
            let stamp = entry
                .event_start_at
                .as_deref()
                .map(format_event_stamp)
                .unwrap_or_else(|| "event".to_string());
            format!("[event][{stamp}] {}", entry.content)
        }
        MemoryKind::Commitment => format!("[commitment] {}", entry.content),
        MemoryKind::Preference => format!("[preference] {}", entry.content),
        MemoryKind::ProfileFact => format!("[profile_fact] {}", entry.content),
        MemoryKind::Constraint => format!("[constraint] {}", entry.content),
        MemoryKind::Procedure => format!("[procedure] {}", entry.content),
        MemoryKind::FailurePattern => format!("[failure_pattern] {}", entry.content),
        MemoryKind::Insight => format!("[insight] {}", entry.content),
    }
}

fn format_event_stamp(raw: &str) -> String {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M %Z")
                .to_string()
        })
        .unwrap_or_else(|_| raw.to_string())
}

fn to_debug_candidate(candidate: &RecallCandidate) -> RecallCandidateDebug {
    RecallCandidateDebug {
        entry_id: candidate.entry.entry_id.clone(),
        kind: candidate.entry.kind.as_str().to_string(),
        status: candidate.entry.status.as_str().to_string(),
        content: candidate.entry.content.clone(),
        source: match candidate.source {
            CandidateSource::SqliteExact => "sqlite_exact",
            CandidateSource::ChromaFacts => "chroma_facts",
            CandidateSource::ChromaInsights => "chroma_insights",
            CandidateSource::ChromaProcedures => "chroma_procedures",
            CandidateSource::ChromaFailures => "chroma_failures",
            CandidateSource::RelationExpansion => "relation_expansion",
        }
        .to_string(),
        semantic_score: candidate.semantic_score,
        type_match: candidate.type_match,
        temporal_match: candidate.temporal_match,
        importance_boost: candidate.importance_boost,
        recency_boost: candidate.recency_boost,
        relation_boost: candidate.relation_boost,
        stale_penalty: candidate.stale_penalty,
        final_score: candidate.final_score,
    }
}

fn compare_candidates(a: &RecallCandidate, b: &RecallCandidate) -> Ordering {
    b.final_score
        .partial_cmp(&a.final_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            b.entry
                .importance
                .partial_cmp(&a.entry.importance)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| a.entry.created_at.cmp(&b.entry.created_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::MemoryStatus;

    #[test]
    fn uses_temporal_template_when_query_contains_explicit_time() {
        let analysis = analyze_intent("我下周五有什么安排", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Temporal);
        assert!(analysis.temporal_window.is_some());
    }

    #[test]
    fn uses_preference_template_when_query_contains_preference_probe() {
        let analysis = analyze_intent("我喜欢吃什么", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Preference);
    }

    #[test]
    fn uses_preference_template_for_food_preference_question_with_ai_chi() {
        let analysis = analyze_intent("你还记得我爱吃什么吗", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Preference);
    }

    #[test]
    fn temporal_probe_overrides_preference_probe_in_mixed_query() {
        let analysis = analyze_intent("帮我安排一下下周五吃火锅的事", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Temporal);
    }

    #[test]
    fn uses_troubleshooting_template_when_query_mentions_failure() {
        let analysis = analyze_intent("这个接口为什么报错了，应该怎么修复", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Troubleshooting);
        assert!(analysis.asks_debug_recovery);
    }

    #[test]
    fn uses_open_template_when_query_is_general_memory_probe() {
        let analysis = analyze_intent("你还记得我是谁吗", Local::now());
        assert_eq!(analysis.template, RecallTemplate::Open);
    }

    #[test]
    fn open_template_prioritizes_profile_facts_over_procedures() {
        let intent = IntentAnalysis {
            template: RecallTemplate::Open,
            temporal_window: None,
            asks_constraints: false,
            asks_procedure: false,
            asks_debug_recovery: false,
        };

        let profile_score =
            score_type_match(&test_entry(MemoryKind::ProfileFact, "profile"), &intent);
        let procedure_score =
            score_type_match(&test_entry(MemoryKind::Procedure, "procedure"), &intent);

        assert!(profile_score > procedure_score);
    }

    fn test_entry(kind: MemoryKind, content: &str) -> MemoryEntry {
        MemoryEntry {
            entry_id: format!("{content}-id"),
            entity_id: "self".to_string(),
            kind,
            status: MemoryStatus::Active,
            version_group_id: Some(format!("{content}-group")),
            supersedes_entry_id: None,
            superseded_by_entry_id: None,
            content: content.to_string(),
            normalized_content: Some(content.to_string()),
            importance: 0.5,
            confidence: 0.8,
            source_turn_id: Some("turn-1".to_string()),
            source_session_id: Some("session-1".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            last_accessed_at: None,
            access_count: 0,
            decay_score: 0.0,
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            extra: None,
        }
    }
}
