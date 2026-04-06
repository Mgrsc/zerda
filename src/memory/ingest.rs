use std::sync::Arc;

use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, MessageMetadata, MessageOrigin, Provider, Role,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::chroma_store::{ChromaStore, ChromaUpsertItem};
use super::conflict::apply_proposal;
use super::embed_client::EmbeddingClient;
use super::sqlite_store::SqliteStore;
use super::types::{
    JournalMessage, MemoryEntry, MemoryExtractionOutput, MemoryKind, MemoryProposal, MemoryScope,
    PendingTurn,
};

pub async fn process_pending_turns(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
) -> Result<()> {
    let turns = sqlite.claim_pending_turns(8)?;
    for turn in turns {
        match process_single_turn(sqlite, embedder, chroma, analyzer, &turn).await {
            Ok(()) => {
                sqlite.mark_turn_processed(&turn.turn_id)?;
            }
            Err(error) => {
                sqlite.mark_turn_error(&turn.turn_id, &error.to_string())?;
                tracing::warn!(turn_id = %turn.turn_id, error = %error, "Memory ingest failed");
            }
        }
    }
    Ok(())
}

async fn process_single_turn(
    sqlite: &SqliteStore,
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    turn: &PendingTurn,
) -> Result<()> {
    let personal_proposals = extract_personal_turn_memories(analyzer, turn).await?;
    let operational_proposals = extract_operational_turn_memories(analyzer, turn).await?;
    let user_messages = collect_user_message_texts(&turn.messages);
    let operational_messages = collect_operational_message_texts(&turn.messages);
    for proposal in personal_proposals
        .memories
        .into_iter()
        .chain(operational_proposals.memories.into_iter())
    {
        let version_key = proposal.version_key.clone();
        let kind = proposal.kind.as_str().to_string();
        let proposal = match validate_proposal_against_turn(
            &proposal,
            &user_messages,
            &operational_messages,
        ) {
            Ok(proposal) => proposal,
            Err(error) => {
                tracing::warn!(
                    turn_id = %turn.turn_id,
                    kind,
                    version_key,
                    error = %error,
                    "Rejected memory proposal during evidence validation"
                );
                continue;
            }
        };
        let outcome = apply_proposal(
            sqlite,
            &turn.entity_id,
            &turn.turn_id,
            &turn.session_id,
            &proposal,
        )?;
        for entry in outcome.deactivated {
            remove_entry_from_index(chroma, &entry).await?;
        }
        if let Some(inserted) = outcome.inserted {
            sync_entry_to_index(embedder, chroma, &inserted).await?;
        }
    }
    Ok(())
}

pub async fn sync_entry_to_index(
    embedder: &EmbeddingClient,
    chroma: &ChromaStore,
    entry: &MemoryEntry,
) -> Result<()> {
    if !entry.status.is_active_like() {
        return Ok(());
    }
    let document = embedding_document_text(entry);
    let embedding = embedder.embed_text(&document).await?;
    let now = chrono::Utc::now().to_rfc3339();
    chroma
        .upsert(
            entry.kind.collection_name(),
            &[ChromaUpsertItem {
                entry_id: entry.entry_id.clone(),
                embedding,
                document,
                metadata: serde_json::json!({
                    "entry_id": entry.entry_id,
                    "entity_id": entry.entity_id,
                    "kind": entry.kind.as_str(),
                    "status": entry.status.as_str(),
                    "memory_scope": entry.extra.as_ref().and_then(|extra| extra.memory_scope.as_ref().map(|scope| scope.as_str())),
                    "axis": entry.extra.as_ref().and_then(|extra| extra.axis.as_deref()),
                    "importance": entry.importance,
                    "created_at": entry.created_at,
                    "event_start_at": entry.event_start_at,
                    "event_end_at": entry.event_end_at,
                    "is_future": entry.event_start_at.as_deref().is_some_and(|value| value > now.as_str()),
                    "version_group_id": entry.version_group_id,
                }),
            }],
        )
        .await
}

pub async fn remove_entry_from_index(chroma: &ChromaStore, entry: &MemoryEntry) -> Result<()> {
    chroma
        .delete_ids(
            entry.kind.collection_name(),
            std::slice::from_ref(&entry.entry_id),
        )
        .await
}

pub fn embedding_document_text(entry: &MemoryEntry) -> String {
    match entry.kind {
        super::types::MemoryKind::Event => format!(
            "event memory; time={}; timezone={}; content={}",
            entry.event_start_at.as_deref().unwrap_or(""),
            entry.timezone.as_deref().unwrap_or(""),
            entry.content
        ),
        super::types::MemoryKind::Commitment => {
            format!("commitment memory; content={}", entry.content)
        }
        super::types::MemoryKind::Preference => format!(
            "preference memory; axis={}; content={}",
            entry.version_group_id.as_deref().unwrap_or(""),
            entry.content
        ),
        super::types::MemoryKind::Constraint => format!(
            "high priority constraint; axis={}; content={}",
            entry.version_group_id.as_deref().unwrap_or(""),
            entry.content
        ),
        super::types::MemoryKind::ProfileFact => {
            format!("profile fact; content={}", entry.content)
        }
        super::types::MemoryKind::Procedure => {
            let scope = entry
                .extra
                .as_ref()
                .and_then(|extra| extra.memory_scope.as_ref())
                .map(MemoryScope::as_str)
                .unwrap_or("personal");
            format!("procedure memory; scope={scope}; content={}", entry.content)
        }
        super::types::MemoryKind::FailurePattern => {
            format!("failure pattern; content={}", entry.content)
        }
        super::types::MemoryKind::Insight => format!(
            "abstract insight; version_group={}; content={}",
            entry.version_group_id.as_deref().unwrap_or(""),
            entry.content
        ),
    }
}

async fn extract_personal_turn_memories(
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    turn: &PendingTurn,
) -> Result<MemoryExtractionOutput> {
    let rendered_messages = render_user_turn_messages(&turn.messages);
    if rendered_messages.is_empty() {
        return Ok(MemoryExtractionOutput {
            memories: Vec::new(),
        });
    }
    let system_prompt = r#"You extract durable memory proposals for an EMA memory system.
Return JSON only with this shape:
{"memories":[{"kind":"event|commitment|preference|profile_fact|constraint","content":"...","normalized_content":"...","importance":0.0,"confidence":0.0,"memory_scope":"personal","valid_from":null,"valid_to":null,"event_start_at":null,"event_end_at":null,"timezone":null,"version_key":"...","source_quote":"...","evidence_source":"user_quote","reason":"...","status_hint":"active|cancelled|null"}]}
Rules:
- Extract only durable memories useful for future turns.
- Extract only memories stated by the user. Ignore assistant self-description, roleplay, suggestions, and stylistic language.
- Facts that are clearly temporary and already finished should be skipped.
- Never emit procedure or failure_pattern for personal memory.
- Preferences and constraints must use stable version_key values, such as preference:food:spicy.
- source_quote must be an exact quote copied from the user conversation text that directly supports the memory.
- Use RFC3339 for event_start_at/event_end_at when the time is explicit.
- Return {"memories":[]} when nothing should be stored.
- No markdown fences."#;
    let user_prompt = format!(
        "session_id: {}\nentity_id: {}\nchannel: {}\nturn_created_at: {}\nconversation:\n{}",
        turn.session_id,
        turn.entity_id,
        turn.channel.as_deref().unwrap_or(""),
        turn.created_at,
        rendered_messages
    );
    let messages = vec![
        ConversationMessage {
            role: Role::System,
            content: vec![ContentPart::Text(system_prompt.to_string())],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata::default(),
        },
        ConversationMessage {
            role: Role::User,
            content: vec![ContentPart::Text(user_prompt)],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata {
                origin: MessageOrigin::Human,
                related_job_id: None,
                related_turn_id: Some(turn.turn_id.clone()),
                created_at: Some(chrono::Utc::now().to_rfc3339()),
            },
        },
    ];
    let response = analyzer
        .0
        .chat(&messages, &analyzer.1)
        .await
        .context("Memory extraction model request failed")?;
    let text = response
        .text
        .context("Memory extraction model returned no text")?;
    parse_json_response::<MemoryExtractionOutput>(&text)
}

async fn extract_operational_turn_memories(
    analyzer: &(Arc<dyn Provider>, ChatOptions),
    turn: &PendingTurn,
) -> Result<MemoryExtractionOutput> {
    if !should_extract_operational_memories(turn) {
        return Ok(MemoryExtractionOutput {
            memories: Vec::new(),
        });
    }
    let rendered_messages = render_operational_turn_messages(&turn.messages);
    if rendered_messages.is_empty() {
        return Ok(MemoryExtractionOutput {
            memories: Vec::new(),
        });
    }
    let system_prompt = r#"You extract operational memory proposals for an EMA memory system.
Return JSON only with this shape:
{"memories":[{"kind":"procedure|failure_pattern","content":"...","normalized_content":"...","importance":0.0,"confidence":0.0,"memory_scope":"operational","valid_from":null,"valid_to":null,"event_start_at":null,"event_end_at":null,"timezone":null,"version_key":"...","source_quote":"...","evidence_source":"assistant_quote|runtime_quote","reason":"...","status_hint":"active|cancelled|null"}]}
Rules:
- Extract only reusable operational knowledge from this completed turn.
- procedure means a reusable workflow or execution sequence that improves future task success.
- failure_pattern means a reusable failure trigger plus the first recovery check or action.
- Ignore one-off noise, transient network blips, and raw logs with no general value.
- Only emit a procedure when the turn reflects a successful or clearly recommended workflow.
- Only emit a failure_pattern when the turn clearly identifies a root cause or a reliable first recovery step.
- source_quote must be an exact quote copied from assistant or runtime text in the conversation.
- memory_scope must be operational.
- Return {"memories":[]} when nothing should be stored.
- No markdown fences."#;
    let user_prompt = format!(
        "session_id: {}\nentity_id: {}\nchannel: {}\nturn_created_at: {}\nconversation:\n{}",
        turn.session_id,
        turn.entity_id,
        turn.channel.as_deref().unwrap_or(""),
        turn.created_at,
        rendered_messages
    );
    let messages = vec![
        ConversationMessage {
            role: Role::System,
            content: vec![ContentPart::Text(system_prompt.to_string())],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata::default(),
        },
        ConversationMessage {
            role: Role::User,
            content: vec![ContentPart::Text(user_prompt)],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata {
                origin: MessageOrigin::Human,
                related_job_id: None,
                related_turn_id: Some(turn.turn_id.clone()),
                created_at: Some(chrono::Utc::now().to_rfc3339()),
            },
        },
    ];
    let response = analyzer
        .0
        .chat(&messages, &analyzer.1)
        .await
        .context("Operational memory extraction model request failed")?;
    let text = response
        .text
        .context("Operational memory extraction model returned no text")?;
    parse_json_response::<MemoryExtractionOutput>(&text)
}

pub(crate) fn parse_json_response<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let trimmed = text.trim();
    let raw = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|value| value.trim())
        .unwrap_or(trimmed);
    let raw = raw.strip_suffix("```").map(str::trim).unwrap_or(raw);
    let value: Value = serde_json::from_str(raw).context("Memory model returned invalid JSON")?;
    serde_json::from_value(value).context("Memory model JSON shape mismatch")
}

fn render_user_turn_messages(messages: &[JournalMessage]) -> String {
    messages
        .iter()
        .filter(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| format!("[{}] {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_operational_turn_messages(messages: &[JournalMessage]) -> String {
    messages
        .iter()
        .filter(|message| {
            matches!(
                message.role.as_str(),
                "user" | "assistant" | "runtime_ptc_result" | "runtime_ptc_notice"
            )
        })
        .map(|message| format!("[{}] {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_user_message_texts(messages: &[JournalMessage]) -> Vec<&str> {
    messages
        .iter()
        .filter(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| message.content.as_str())
        .collect()
}

fn collect_operational_message_texts(messages: &[JournalMessage]) -> Vec<(&str, &str)> {
    messages
        .iter()
        .filter_map(|message| match message.role.as_str() {
            "assistant" | "runtime_ptc_result" | "runtime_ptc_notice" => {
                Some((message.role.as_str(), message.content.as_str()))
            }
            _ => None,
        })
        .collect()
}

fn validate_proposal_against_turn(
    proposal: &MemoryProposal,
    user_messages: &[&str],
    operational_messages: &[(&str, &str)],
) -> Result<MemoryProposal> {
    if matches!(proposal.kind, super::types::MemoryKind::Insight) {
        return Ok(proposal.clone());
    }
    if proposal.content.trim().is_empty() {
        bail!("Memory proposal content must not be empty");
    }
    if proposal.version_key.trim().is_empty() {
        bail!("Memory proposal version_key must not be empty");
    }
    let Some(source_quote) = proposal.source_quote.as_deref().map(str::trim) else {
        bail!("Memory proposal source_quote must not be empty");
    };
    if source_quote.is_empty() {
        bail!("Memory proposal source_quote must not be empty");
    }
    match proposal
        .memory_scope
        .clone()
        .unwrap_or(MemoryScope::Personal)
    {
        MemoryScope::Personal => {
            validate_personal_proposal(proposal)?;
            if !user_messages
                .iter()
                .any(|message| message.contains(source_quote))
            {
                bail!("Memory proposal source_quote must match user-authored text");
            }
        }
        MemoryScope::Operational => {
            let expected_source = proposal.evidence_source.as_deref().unwrap_or("");
            let matched = operational_messages.iter().any(|(role, message)| {
                message.contains(source_quote)
                    && match expected_source {
                        "assistant_quote" => *role == "assistant",
                        "runtime_quote" => {
                            matches!(*role, "runtime_ptc_result" | "runtime_ptc_notice")
                        }
                        _ => true,
                    }
            });
            if !matched {
                bail!("Memory proposal source_quote must match assistant or runtime evidence");
            }
            validate_operational_proposal(proposal)?;
        }
    }
    Ok(proposal.clone())
}

fn validate_personal_proposal(proposal: &MemoryProposal) -> Result<()> {
    match proposal.kind {
        MemoryKind::Event
        | MemoryKind::Commitment
        | MemoryKind::Preference
        | MemoryKind::ProfileFact
        | MemoryKind::Constraint => Ok(()),
        MemoryKind::Procedure | MemoryKind::FailurePattern | MemoryKind::Insight => {
            bail!("Personal extractor emitted unsupported memory kind")
        }
    }
}

fn validate_operational_proposal(proposal: &MemoryProposal) -> Result<()> {
    match proposal.kind {
        MemoryKind::Procedure => {
            if proposal.importance < 0.4 || proposal.confidence < 0.5 {
                bail!("Operational procedure confidence is too low");
            }
        }
        MemoryKind::FailurePattern => {
            if proposal.importance < 0.55 || proposal.confidence < 0.65 {
                bail!("Failure pattern confidence is too low");
            }
            let content = proposal.content.to_ascii_lowercase();
            let has_trigger = ["when ", "if ", "当", "遇到", "返回", "报错"]
                .iter()
                .any(|probe| content.contains(probe));
            let has_symptom = [
                "because",
                "caused by",
                "症状",
                "现象",
                "显示",
                "提示",
                "returns",
                "返回",
                "报错",
            ]
            .iter()
            .any(|probe| content.contains(probe));
            let has_action = ["check", "verify", "retry", "先", "优先", "检查", "确认"]
                .iter()
                .any(|probe| content.contains(probe));
            if !(has_trigger && has_symptom && has_action) {
                bail!("Failure pattern must include trigger, symptom, and recovery guidance");
            }
        }
        _ => bail!("Operational extractor emitted unsupported memory kind"),
    }
    Ok(())
}

fn should_extract_operational_memories(turn: &PendingTurn) -> bool {
    let has_runtime_messages = turn.messages.iter().any(|message| {
        matches!(
            message.role.as_str(),
            "runtime_ptc_result" | "runtime_ptc_notice"
        )
    });
    let has_assistant = turn
        .messages
        .iter()
        .any(|message| message.role.eq_ignore_ascii_case("assistant"));
    has_assistant && (has_runtime_messages || user_query_looks_operational(&turn.messages))
}

fn user_query_looks_operational(messages: &[JournalMessage]) -> bool {
    let query = messages
        .iter()
        .filter(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    [
        "怎么", "如何", "流程", "步骤", "报错", "失败", "修复", "恢复", "排查", "debug", "error",
        "fix",
    ]
    .iter()
    .any(|probe| query.contains(probe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{MemoryKind, MemoryScope};

    #[test]
    fn renders_only_user_messages_for_memory_extraction() {
        let rendered = render_user_turn_messages(&[
            JournalMessage::new("user", "I like mild fish"),
            JournalMessage::new("assistant", "I am a desert fox with big ears"),
        ]);

        assert_eq!(rendered, "[user] I like mild fish");
    }

    #[test]
    fn returns_empty_output_when_turn_has_no_user_messages() {
        let rendered = render_user_turn_messages(&[JournalMessage::new(
            "assistant",
            "I am a desert fox with big ears",
        )]);

        assert_eq!(rendered, "");
    }

    #[test]
    fn rejects_proposal_without_source_quote() {
        let proposal = MemoryProposal {
            kind: MemoryKind::Preference,
            content: "User likes mild food".to_string(),
            normalized_content: "user likes mild food".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Personal),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "preference:food:spiciness".to_string(),
            source_quote: None,
            evidence_source: Some("user_quote".to_string()),
            reason: Some("direct user statement".to_string()),
            status_hint: None,
        };

        let error =
            validate_proposal_against_turn(&proposal, &["我喜欢吃不辣的"], &[]).unwrap_err();

        assert!(error.to_string().contains("source_quote"));
    }

    #[test]
    fn rejects_proposal_when_source_quote_is_not_in_user_messages() {
        let proposal = MemoryProposal {
            kind: MemoryKind::Preference,
            content: "User likes mild food".to_string(),
            normalized_content: "user likes mild food".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Personal),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "preference:food:spiciness".to_string(),
            source_quote: Some("微辣的最棒".to_string()),
            evidence_source: Some("user_quote".to_string()),
            reason: Some("direct user statement".to_string()),
            status_hint: None,
        };

        let error =
            validate_proposal_against_turn(&proposal, &["我喜欢吃不辣的"], &[]).unwrap_err();

        assert!(error.to_string().contains("user-authored"));
    }

    #[test]
    fn accepts_proposal_when_source_quote_matches_user_message_substring() {
        let proposal = MemoryProposal {
            kind: MemoryKind::Preference,
            content: "User likes mild food".to_string(),
            normalized_content: "user likes mild food".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Personal),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "preference:food:spiciness".to_string(),
            source_quote: Some("喜欢吃不辣".to_string()),
            evidence_source: Some("user_quote".to_string()),
            reason: Some("direct user statement".to_string()),
            status_hint: None,
        };

        let validated =
            validate_proposal_against_turn(&proposal, &["我喜欢吃不辣的"], &[]).unwrap();

        assert_eq!(validated.source_quote.as_deref(), Some("喜欢吃不辣"));
    }

    #[test]
    fn rejects_personal_procedure_even_if_user_quote_matches() {
        let proposal = MemoryProposal {
            kind: MemoryKind::Procedure,
            content: "Always sort the environment variables before reading them".to_string(),
            normalized_content: "always sort the environment variables before reading them"
                .to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Personal),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "procedure:user:env_read".to_string(),
            source_quote: Some("先把环境变量排一下".to_string()),
            evidence_source: Some("user_quote".to_string()),
            reason: Some("user described a workflow".to_string()),
            status_hint: None,
        };

        let error =
            validate_proposal_against_turn(&proposal, &["先把环境变量排一下"], &[]).unwrap_err();

        assert!(error
            .to_string()
            .contains("Personal extractor emitted unsupported memory kind"));
    }

    #[test]
    fn accepts_operational_procedure_backed_by_assistant_quote() {
        let proposal = MemoryProposal {
            kind: MemoryKind::Procedure,
            content: "Before sending a WeChat image, upload media first and then send the returned media id".to_string(),
            normalized_content: "before sending a wechat image upload media first and then send the returned media id".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Operational),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "procedure:wechat:image_send".to_string(),
            source_quote: Some("upload media first and then send the returned media id".to_string()),
            evidence_source: Some("assistant_quote".to_string()),
            reason: Some("validated successful workflow".to_string()),
            status_hint: None,
        };

        let validated = validate_proposal_against_turn(
            &proposal,
            &[],
            &[(
                "assistant",
                "We should upload media first and then send the returned media id.",
            )],
        )
        .unwrap();

        assert_eq!(validated.memory_scope, Some(MemoryScope::Operational));
    }

    #[test]
    fn rejects_failure_pattern_without_recovery_guidance() {
        let proposal = MemoryProposal {
            kind: MemoryKind::FailurePattern,
            content: "When the API returns 401".to_string(),
            normalized_content: "when the api returns 401".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Operational),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "failure:http:401_scope".to_string(),
            source_quote: Some("When the API returns 401".to_string()),
            evidence_source: Some("assistant_quote".to_string()),
            reason: Some("too vague".to_string()),
            status_hint: None,
        };

        let error = validate_proposal_against_turn(
            &proposal,
            &[],
            &[("assistant", "When the API returns 401")],
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("trigger, symptom, and recovery guidance"));
    }

    #[test]
    fn accepts_failure_pattern_with_trigger_symptom_and_recovery_guidance() {
        let proposal = MemoryProposal {
            kind: MemoryKind::FailurePattern,
            content: "When the API returns 401 and shows an invalid scope error, first check whether the token was issued with the required scope".to_string(),
            normalized_content: "when the api returns 401 and shows an invalid scope error first check whether the token was issued with the required scope".to_string(),
            importance: 0.8,
            confidence: 0.9,
            memory_scope: Some(MemoryScope::Operational),
            valid_from: None,
            valid_to: None,
            event_start_at: None,
            event_end_at: None,
            timezone: None,
            version_key: "failure:http:401_scope".to_string(),
            source_quote: Some("returns 401 and shows an invalid scope error".to_string()),
            evidence_source: Some("assistant_quote".to_string()),
            reason: Some("clear trigger and recovery".to_string()),
            status_hint: None,
        };

        let validated = validate_proposal_against_turn(
            &proposal,
            &[],
            &[(
                "assistant",
                "The endpoint returns 401 and shows an invalid scope error, so first check whether the token was issued with the required scope.",
            )],
        )
        .unwrap();

        assert_eq!(validated.kind, MemoryKind::FailurePattern);
    }
}
