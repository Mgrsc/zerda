use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt as _;

use crate::config::AgentConfig;
use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, MessageMetadata, MessageOrigin, Provider,
    ProviderResponse, Role, StreamEvent, ThinkingBlock, Usage,
};
use crate::ptc::parser::PtcRequest;
use crate::ptc::stream_interceptor::PtcStreamInterceptor;
use crate::util::fs::atomic_write_text;

pub struct AssistantTurnResult {
    pub visible_text: String,
    pub ptc_requests: Vec<PtcRequest>,
    pub ptc_parse_notice: Option<String>,
}

pub struct StreamedTurnOutput {
    visible_text: String,
    hidden_ptc: String,
    reasoning_content: Option<String>,
    thinking_blocks: Vec<ThinkingBlock>,
    usage: Usage,
}

pub struct TurnSnapshot {
    history: Vec<ConversationMessage>,
    total_usage: Usage,
    conversation_summary: Option<String>,
}

pub struct Agent {
    pub history: Vec<ConversationMessage>,
    pub total_usage: Usage,
    config: AgentConfig,
    compression_provider: (Arc<dyn Provider>, ChatOptions),
    conversation_summary: Option<String>,
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        compression_provider: (Arc<dyn Provider>, ChatOptions),
    ) -> Self {
        Self {
            history: Vec::new(),
            total_usage: Usage::default(),
            config,
            compression_provider,
            conversation_summary: None,
        }
    }

    pub fn set_system_prompt_parts(&mut self, parts: Vec<String>) {
        self.history.retain(|m| !matches!(m.role, Role::System));
        self.history.insert(
            0,
            ConversationMessage {
                role: Role::System,
                content: parts.into_iter().map(ContentPart::Text).collect(),
                reasoning_content: None,
                thinking_blocks: Vec::new(),
                metadata: MessageMetadata::default(),
            },
        );
    }

    pub fn snapshot_turn(&self) -> TurnSnapshot {
        TurnSnapshot {
            history: self.history.clone(),
            total_usage: self.total_usage.clone(),
            conversation_summary: self.conversation_summary.clone(),
        }
    }

    pub fn restore_turn(&mut self, snapshot: TurnSnapshot) {
        self.history = snapshot.history;
        self.total_usage = snapshot.total_usage;
        self.conversation_summary = snapshot.conversation_summary;
    }

    pub fn take_conversation_summary(&mut self) -> Option<String> {
        self.conversation_summary.take()
    }

    pub async fn run_turn(
        &mut self,
        provider: &dyn Provider,
        opts: &ChatOptions,
    ) -> Result<AssistantTurnResult> {
        let response = provider.chat(&self.history, opts).await?;
        Ok(self.finish_turn(self.process_blocking_response(response)))
    }

    pub async fn collect_turn_stream_output(
        &self,
        provider: &dyn Provider,
        opts: &ChatOptions,
        on_text: impl Fn(&str),
    ) -> Result<StreamedTurnOutput> {
        self.consume_stream(provider, &self.history, opts, &on_text)
            .await
    }

    pub fn finish_streamed_turn(
        &mut self,
        output: StreamedTurnOutput,
    ) -> Result<AssistantTurnResult> {
        Ok(self.finish_turn(output))
    }

    fn process_blocking_response(&self, response: ProviderResponse) -> StreamedTurnOutput {
        let mut interceptor = PtcStreamInterceptor::new();
        if let Some(text) = response.text.as_deref() {
            interceptor.push(text);
        }
        let (visible_text, hidden_ptc) = interceptor.finish();
        StreamedTurnOutput {
            visible_text,
            hidden_ptc,
            reasoning_content: response.reasoning_content,
            thinking_blocks: response.thinking_blocks,
            usage: response.usage.unwrap_or_default(),
        }
    }

    fn finish_turn(&mut self, output: StreamedTurnOutput) -> AssistantTurnResult {
        self.total_usage.input_tokens += output.usage.input_tokens;
        self.total_usage.output_tokens += output.usage.output_tokens;

        let parsed = crate::ptc::parser::parse_ptc_requests(&output.hidden_ptc);
        let (ptc_requests, ptc_parse_notice) = match parsed {
            Ok(requests) => (requests, None),
            Err(err) => (
                Vec::new(),
                Some(format!(
                    "<PTC_RUNTIME_NOTICE source=\"runtime\" status=\"error\">{err}</PTC_RUNTIME_NOTICE>"
                )),
            ),
        };

        self.history.push(ConversationMessage {
            role: Role::Assistant,
            content: if output.visible_text.is_empty() {
                Vec::new()
            } else {
                vec![ContentPart::Text(output.visible_text.clone())]
            },
            reasoning_content: output.reasoning_content,
            thinking_blocks: output.thinking_blocks,
            metadata: MessageMetadata::default(),
        });

        AssistantTurnResult {
            visible_text: output.visible_text,
            ptc_requests,
            ptc_parse_notice,
        }
    }

    async fn consume_stream(
        &self,
        provider: &dyn Provider,
        messages: &[ConversationMessage],
        opts: &ChatOptions,
        on_text: &dyn Fn(&str),
    ) -> Result<StreamedTurnOutput> {
        let mut stream = provider.chat_stream(messages, opts).await?;
        let mut interceptor = PtcStreamInterceptor::new();
        let mut reasoning_buf = String::new();
        let mut thinking_blocks: Vec<ThinkingBlock> = Vec::new();
        let mut usage = Usage::default();

        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::TextDelta(delta) => {
                    let visible = interceptor.push(&delta);
                    if !visible.is_empty() {
                        on_text(&visible);
                    }
                }
                StreamEvent::AssistantMeta(meta) => {
                    let kind = meta.get("kind").and_then(serde_json::Value::as_str);
                    match kind {
                        Some("openai_reasoning_content_delta") => {
                            if let Some(delta) =
                                meta.get("delta").and_then(serde_json::Value::as_str)
                            {
                                reasoning_buf.push_str(delta);
                            }
                        }
                        Some("anthropic_thinking_block") => {
                            if let Some(block) = meta.get("block") {
                                match serde_json::from_value::<ThinkingBlock>(block.clone()) {
                                    Ok(tb) => thinking_blocks.push(tb),
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to parse thinking block from stream metadata: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                StreamEvent::Done(u) => {
                    usage = u;
                }
            }
        }

        let (visible_text, hidden_ptc) = interceptor.finish();
        let reasoning_content = if reasoning_buf.is_empty() {
            None
        } else {
            Some(reasoning_buf)
        };

        Ok(StreamedTurnOutput {
            visible_text,
            hidden_ptc,
            reasoning_content,
            thinking_blocks,
            usage,
        })
    }

    pub async fn auto_compact(&mut self, memory_dir: &Path) -> Result<bool> {
        let non_system_count = self
            .history
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .count();

        if non_system_count <= self.config.max_history {
            return Ok(false);
        }

        tracing::info!(
            summary_kind = "history_compaction",
            non_system_count,
            max_history = self.config.max_history,
            "Auto-compact triggered"
        );

        self.compress_with_llm(memory_dir).await?;
        Ok(true)
    }

    pub async fn compress_with_llm(&mut self, memory_dir: &Path) -> Result<()> {
        let indices_to_compact: Vec<usize> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, m)| !matches!(m.role, Role::System))
            .map(|(i, _)| i)
            .collect();

        if indices_to_compact.is_empty() {
            return Ok(());
        }

        let mut transcript = String::new();
        for &i in &indices_to_compact {
            let msg = &self.history[i];
            let role_str = transcript_role(msg);
            let text = msg.text_content();
            let _ = writeln!(transcript, "{role_str}: {text}");
        }

        let compaction_dir = memory_dir.join("memory").join("compaction");
        std::fs::create_dir_all(&compaction_dir)?;
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let ms = duration.as_millis();
        let nanos = duration.subsec_nanos();
        let compaction_path = compaction_dir.join(format!("{ms}-{nanos:08x}.txt"));
        std::fs::write(&compaction_path, &transcript)?;

        let (comp_provider, comp_opts) = &self.compression_provider;
        let prompt = format!(
            "Summarize this conversation into concise context for future turns. \
             Preserve: user preferences, commitments, decisions, unresolved tasks, key facts. \
             Omit: filler, repeated chit-chat, raw XML protocol blocks, verbose logs. \
             Output plain text bullet points only.\n\n{transcript}"
        );
        let messages = vec![ConversationMessage::user(prompt)];
        let response = comp_provider.chat(&messages, comp_opts).await?;
        let summary = response.text.unwrap_or_default();
        if summary.is_empty() {
            anyhow::bail!("Empty compression summary");
        }

        let hint = format!(
            "\n\nThe full conversation transcript before compaction was saved to: {path}\n\
             Refer to that file via a later PTC task if specific detail is needed.",
            path = compaction_path.display()
        );

        let remove_set: std::collections::HashSet<usize> =
            indices_to_compact.iter().copied().collect();
        let mut idx = 0;
        self.history.retain(|_| {
            let keep = !remove_set.contains(&idx);
            idx += 1;
            keep
        });
        self.conversation_summary = Some(format!("{summary}{hint}"));
        Ok(())
    }

    pub fn save_session(&self, sessions_dir: &Path, id: Option<&str>) -> Result<String> {
        std::fs::create_dir_all(sessions_dir)?;
        let id = match id {
            Some(id) => id.to_string(),
            None => format!(
                "{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
        };
        let path = sessions_dir.join(format!("{id}.json"));
        let data = serde_json::to_string(&self.history)?;
        atomic_write_text(&path, &data)?;
        Ok(id)
    }

    pub fn load_session(
        sessions_dir: &Path,
        id: Option<&str>,
    ) -> Result<(String, Vec<ConversationMessage>)> {
        let (session_id, path) = if let Some(id) = id {
            (id.to_string(), sessions_dir.join(format!("{id}.json")))
        } else {
            let latest = Self::latest_session(sessions_dir)?;
            let name = latest
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            (name, latest)
        };
        let data = std::fs::read_to_string(&path)
            .map_err(|_| anyhow::anyhow!("Session not found: {session_id}"))?;
        let history: Vec<ConversationMessage> = serde_json::from_str(&data)?;
        Ok((session_id, history))
    }

    fn latest_session(sessions_dir: &Path) -> Result<PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(sessions_dir)?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        entries
            .first()
            .map(std::fs::DirEntry::path)
            .ok_or_else(|| anyhow::anyhow!("No sessions found"))
    }

    pub fn cleanup_old_sessions(sessions_dir: &Path, max_age_days: u64) -> usize {
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_days * 86400);
        let mut removed = 0;
        if let Ok(entries) = std::fs::read_dir(sessions_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified < cutoff {
                            match std::fs::remove_file(entry.path()) {
                                Ok(()) => removed += 1,
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to remove old session {}: {e}",
                                        entry.path().display()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        removed
    }
}

fn transcript_role(msg: &ConversationMessage) -> &'static str {
    match (&msg.role, &msg.metadata.origin) {
        (Role::System, _) => "System",
        (Role::Assistant, _) => "Assistant",
        (Role::User, MessageOrigin::Human) => "User",
        (Role::User, MessageOrigin::RuntimePtcResult) => "RuntimeResult",
        (Role::User, MessageOrigin::RuntimePtcNotice) => "RuntimeNotice",
    }
}
