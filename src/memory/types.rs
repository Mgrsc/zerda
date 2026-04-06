use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalMessage {
    pub role: String,
    pub content: String,
}

impl JournalMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Event,
    Commitment,
    Preference,
    ProfileFact,
    Constraint,
    Procedure,
    FailurePattern,
    Insight,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Commitment => "commitment",
            Self::Preference => "preference",
            Self::ProfileFact => "profile_fact",
            Self::Constraint => "constraint",
            Self::Procedure => "procedure",
            Self::FailurePattern => "failure_pattern",
            Self::Insight => "insight",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "event" => Some(Self::Event),
            "commitment" => Some(Self::Commitment),
            "preference" => Some(Self::Preference),
            "profile_fact" => Some(Self::ProfileFact),
            "constraint" => Some(Self::Constraint),
            "procedure" => Some(Self::Procedure),
            "failure_pattern" => Some(Self::FailurePattern),
            "insight" => Some(Self::Insight),
            _ => None,
        }
    }

    pub fn collection_name(&self) -> &'static str {
        match self {
            Self::Insight => "ema_insights",
            Self::Procedure => "ema_procedures",
            Self::FailurePattern => "ema_failures",
            _ => "ema_facts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Personal,
    Operational,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Operational => "operational",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Superseded,
    Obsolete,
    Expired,
    Cancelled,
    Archived,
}

impl MemoryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Obsolete => "obsolete",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::Archived => "archived",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "obsolete" => Some(Self::Obsolete),
            "expired" => Some(Self::Expired),
            "cancelled" => Some(Self::Cancelled),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    pub fn is_active_like(&self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    Supports,
    DerivedFrom,
    Contradicts,
    RelatedTo,
    Explains,
    Supersedes,
}

impl LinkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::DerivedFrom => "derived_from",
            Self::Contradicts => "contradicts",
            Self::RelatedTo => "related_to",
            Self::Explains => "explains",
            Self::Supersedes => "supersedes",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingTurn {
    pub turn_id: String,
    pub session_id: String,
    pub entity_id: String,
    pub channel: Option<String>,
    pub created_at: String,
    pub messages: Vec<JournalMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntryExtra {
    #[serde(default)]
    pub version_key: Option<String>,
    #[serde(default)]
    pub memory_scope: Option<MemoryScope>,
    #[serde(default)]
    pub axis: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub source_excerpt: Option<String>,
    #[serde(default)]
    pub evidence_quote: Option<String>,
    #[serde(default)]
    pub evidence_source: Option<String>,
    #[serde(default)]
    pub evidence_verified: bool,
    #[serde(default)]
    pub support_entry_ids: Vec<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MemoryEntry {
    pub entry_id: String,
    pub entity_id: String,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub version_group_id: Option<String>,
    pub supersedes_entry_id: Option<String>,
    pub superseded_by_entry_id: Option<String>,
    pub content: String,
    pub normalized_content: Option<String>,
    pub importance: f32,
    pub confidence: f32,
    pub source_turn_id: Option<String>,
    pub source_session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
    pub access_count: i64,
    pub decay_score: f32,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub event_start_at: Option<String>,
    pub event_end_at: Option<String>,
    pub timezone: Option<String>,
    pub extra: Option<MemoryEntryExtra>,
}

impl MemoryEntry {
    pub fn version_group_id(&self) -> &str {
        self.version_group_id
            .as_deref()
            .unwrap_or(self.entry_id.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub kind: MemoryKind,
    pub content: String,
    pub normalized_content: String,
    pub importance: f32,
    pub confidence: f32,
    #[serde(default)]
    pub memory_scope: Option<MemoryScope>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub event_start_at: Option<String>,
    #[serde(default)]
    pub event_end_at: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    pub version_key: String,
    #[serde(default)]
    pub source_quote: Option<String>,
    #[serde(default)]
    pub evidence_source: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub status_hint: Option<MemoryStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryExtractionOutput {
    #[serde(default)]
    pub memories: Vec<MemoryProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightProposal {
    pub content: String,
    pub normalized_content: String,
    pub importance: f32,
    pub confidence: f32,
    pub version_key: String,
    #[serde(default)]
    pub support_entry_ids: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightExtractionOutput {
    #[serde(default)]
    pub insights: Vec<InsightProposal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecallTemplate {
    Temporal,
    Preference,
    Troubleshooting,
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalWindow {
    pub label: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentAnalysis {
    pub template: RecallTemplate,
    pub temporal_window: Option<TemporalWindow>,
    pub asks_constraints: bool,
    pub asks_procedure: bool,
    pub asks_debug_recovery: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateSource {
    SqliteExact,
    ChromaFacts,
    ChromaInsights,
    ChromaProcedures,
    ChromaFailures,
    RelationExpansion,
}

#[derive(Debug, Clone)]
pub struct RecallCandidate {
    pub entry: MemoryEntry,
    pub source: CandidateSource,
    pub semantic_score: f32,
    pub type_match: f32,
    pub temporal_match: f32,
    pub importance_boost: f32,
    pub recency_boost: f32,
    pub relation_boost: f32,
    pub stale_penalty: f32,
    pub final_score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallCandidateDebug {
    pub entry_id: String,
    pub kind: String,
    pub status: String,
    pub content: String,
    pub source: String,
    pub semantic_score: f32,
    pub type_match: f32,
    pub temporal_match: f32,
    pub importance_boost: f32,
    pub recency_boost: f32,
    pub relation_boost: f32,
    pub stale_penalty: f32,
    pub final_score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallDebugInfo {
    pub template: String,
    pub query: String,
    pub temporal_window: Option<String>,
    pub candidates: Vec<RecallCandidateDebug>,
}

#[derive(Debug, Clone)]
pub struct RecallResult {
    pub facts: Vec<MemoryEntry>,
    pub insights: Vec<MemoryEntry>,
    pub failures: Vec<MemoryEntry>,
    pub procedures: Vec<MemoryEntry>,
    pub debug: RecallDebugInfo,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryPromptBlocks {
    pub facts: Vec<String>,
    pub insights: Vec<String>,
    pub failures: Vec<String>,
    pub procedures: Vec<String>,
}

impl MemoryPromptBlocks {
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
            && self.insights.is_empty()
            && self.failures.is_empty()
            && self.procedures.is_empty()
    }

    pub fn render(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::new();
        if !self.facts.is_empty() {
            out.push_str("<memory-facts>\n");
            for item in &self.facts {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
            out.push_str("</memory-facts>\n");
        }
        if !self.insights.is_empty() {
            out.push_str("<memory-insights>\n");
            for item in &self.insights {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
            out.push_str("</memory-insights>\n");
        }
        if !self.failures.is_empty() {
            out.push_str("<memory-failures>\n");
            for item in &self.failures {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
            out.push_str("</memory-failures>\n");
        }
        if !self.procedures.is_empty() {
            out.push_str("<memory-procedures>\n");
            for item in &self.procedures {
                out.push_str("- ");
                out.push_str(item);
                out.push('\n');
            }
            out.push_str("</memory-procedures>\n");
        }
        Some(out.trim_end().to_string())
    }
}

#[derive(Debug, Clone)]
pub struct ChromaQueryResult {
    pub entry_id: String,
    pub distance: f32,
}

#[cfg(test)]
mod tests {
    use super::MemoryPromptBlocks;

    #[test]
    fn renders_memory_blocks_in_fact_insight_procedure_order() {
        let rendered = MemoryPromptBlocks {
            facts: vec!["[event] test".to_string()],
            insights: vec!["[insight] test".to_string()],
            failures: vec!["[failure_pattern] test".to_string()],
            procedures: vec!["[procedure] test".to_string()],
        }
        .render()
        .unwrap();

        assert!(rendered.contains("<memory-facts>"));
        assert!(rendered.contains("<memory-insights>"));
        assert!(rendered.contains("<memory-failures>"));
        assert!(rendered.contains("<memory-procedures>"));
        assert!(rendered.find("<memory-facts>") < rendered.find("<memory-insights>"));
        assert!(rendered.find("<memory-insights>") < rendered.find("<memory-failures>"));
        assert!(rendered.find("<memory-failures>") < rendered.find("<memory-procedures>"));
    }
}
