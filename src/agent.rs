use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use futures::StreamExt as _;
use uuid::Uuid;

use crate::config::AgentConfig;
use crate::logging::{summarize_json, summarize_text};
use crate::providers::{
    create_provider, ChatOptions, ContentPart, ConversationMessage, Provider, Role, StreamEvent,
    ThinkingBlock, ToolCall, ToolSpec, Usage,
};
use crate::tools::{Tool, ToolResult};
use crate::util::fs::atomic_write_text;
use crate::util::text::TruncateForUi;

pub type ConfirmFn = dyn Fn(&str, &serde_json::Value) -> bool + Send + Sync;

struct TurnOutput {
    text: Option<String>,
    reasoning_content: Option<String>,
    thinking_blocks: Vec<ThinkingBlock>,
    tool_calls: Vec<ToolCall>,
    usage: Usage,
    parse_errors: Vec<(String, String)>,
}

enum CallStrategy<'a, F: Fn(&str)> {
    Blocking,
    Streaming { on_text: &'a F },
}

pub struct Agent {
    pub history: Vec<ConversationMessage>,
    pub total_usage: Usage,
    config: AgentConfig,
    confirm_fn: Option<Arc<ConfirmFn>>,
    compression_provider: Option<(Box<dyn Provider>, ChatOptions)>,
    conversation_summary: Option<String>,
    has_subagent: bool,
    temp_files: Vec<PathBuf>,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Self {
        let compression_provider = config.compression_model.as_ref().and_then(|comp_config| {
            let opts = ChatOptions::from_provider_config(&comp_config.provider);
            match create_provider(&comp_config.provider) {
                Ok(p) => Some((p, opts)),
                Err(e) => {
                    tracing::warn!("Failed to create compression provider: {e}");
                    None
                }
            }
        });

        Self {
            history: Vec::new(),
            total_usage: Usage::default(),
            config,
            confirm_fn: None,
            compression_provider,
            conversation_summary: None,
            has_subagent: false,
            temp_files: Vec::new(),
        }
    }

    pub fn set_has_subagent(&mut self, has: bool) {
        self.has_subagent = has;
    }

    pub fn set_confirm_fn(&mut self, f: Arc<ConfirmFn>) {
        self.confirm_fn = Some(f);
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.history.retain(|m| !matches!(m.role, Role::System));
        self.history.insert(0, ConversationMessage::system(prompt));
    }

    pub fn push_user_message(&mut self, text: String) {
        self.history.push(ConversationMessage::user(text));
    }

    pub fn take_conversation_summary(&mut self) -> Option<String> {
        self.conversation_summary.take()
    }

    pub fn has_subagent(&self) -> bool {
        self.has_subagent
    }

    pub fn confirm_fn(&self) -> Option<Arc<ConfirmFn>> {
        self.confirm_fn.clone()
    }

    pub async fn run_turn(
        &mut self,
        provider: &dyn Provider,
        tools: &[Box<dyn Tool>],
        opts: &ChatOptions,
    ) -> Result<String> {
        self.run_turn_inner(provider, tools, opts, CallStrategy::<fn(&str)>::Blocking)
            .await
    }

    pub async fn run_turn_stream(
        &mut self,
        provider: &dyn Provider,
        tools: &[Box<dyn Tool>],
        opts: &ChatOptions,
        on_text: impl Fn(&str),
    ) -> Result<String> {
        self.run_turn_inner(
            provider,
            tools,
            opts,
            CallStrategy::Streaming { on_text: &on_text },
        )
        .await
    }

    async fn run_turn_inner<F: Fn(&str)>(
        &mut self,
        provider: &dyn Provider,
        tools: &[Box<dyn Tool>],
        opts: &ChatOptions,
        strategy: CallStrategy<'_, F>,
    ) -> Result<String> {
        let tool_specs: Vec<ToolSpec> = tools.iter().map(|t| t.spec()).collect();

        let max_iterations = self.config.max_iterations;
        for iteration in 0..max_iterations {
            let request_history = self.sanitized_history_for_provider();
            let output = match &strategy {
                CallStrategy::Blocking => {
                    let response = provider.chat(&request_history, &tool_specs, opts).await?;
                    TurnOutput {
                        text: response.text,
                        reasoning_content: response.reasoning_content,
                        thinking_blocks: response.thinking_blocks,
                        tool_calls: response.tool_calls,
                        usage: response.usage.unwrap_or_default(),
                        parse_errors: Vec::new(),
                    }
                }
                CallStrategy::Streaming { on_text } => {
                    self.consume_stream(provider, &request_history, &tool_specs, opts, on_text)
                        .await?
                }
            };

            self.total_usage.input_tokens += output.usage.input_tokens;
            self.total_usage.output_tokens += output.usage.output_tokens;

            if output.tool_calls.is_empty() && output.parse_errors.is_empty() {
                let text = output.text.unwrap_or_default();
                self.history.push(ConversationMessage {
                    role: Role::Assistant,
                    content: if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![ContentPart::Text(text.clone())]
                    },
                    tool_calls: Vec::new(),
                    reasoning_content: output.reasoning_content,
                    thinking_blocks: output.thinking_blocks,
                });
                return Ok(text);
            }

            let error_tool_calls: Vec<ToolCall> = output
                .parse_errors
                .iter()
                .map(|(id, name)| ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: serde_json::json!({}),
                })
                .collect();

            let TurnOutput {
                text,
                reasoning_content,
                thinking_blocks,
                tool_calls,
                parse_errors,
                ..
            } = output;

            let (msg_tool_calls, exec_tool_calls) = if parse_errors.is_empty() {
                (tool_calls.clone(), tool_calls)
            } else {
                let exec = tool_calls.clone();
                let mut msg_calls = tool_calls;
                msg_calls.extend(error_tool_calls);
                (msg_calls, exec)
            };

            let mut assistant_msg = ConversationMessage {
                role: Role::Assistant,
                content: Vec::new(),
                tool_calls: msg_tool_calls,
                reasoning_content,
                thinking_blocks,
            };
            if let Some(ref text) = text {
                if !text.is_empty() {
                    assistant_msg.content.push(ContentPart::Text(text.clone()));
                }
            }
            self.history.push(assistant_msg);

            for (id, name) in &parse_errors {
                self.history.push(ConversationMessage::tool_result(
                    id,
                    format!("Failed to parse arguments for tool '{name}'. The arguments were malformed JSON."),
                    true,
                ));
            }

            self.process_tool_calls(&exec_tool_calls, tools, iteration, max_iterations)
                .await;
        }

        tracing::warn!(
            summary_kind = "iteration_fallback",
            max_iterations,
            history_messages = self.history.len(),
            "Iteration budget exhausted; generating fallback summary"
        );
        let summary = self.generate_iteration_fallback().await;
        self.history.push(ConversationMessage {
            role: Role::Assistant,
            content: vec![ContentPart::Text(summary.clone())],
            tool_calls: Vec::new(),
            reasoning_content: None,
            thinking_blocks: Vec::new(),
        });
        Ok(summary)
    }

    async fn generate_iteration_fallback(&self) -> String {
        let (comp_provider, comp_opts) = match self.compression_provider.as_ref() {
            Some(pair) => pair,
            None => {
                tracing::warn!(
                    summary_kind = "iteration_fallback",
                    reason = "missing_compression_provider",
                    "Compression provider not configured; using static fallback summary"
                );
                return self.static_fallback();
            }
        };

        let mut transcript = String::new();
        let recent: Vec<&ConversationMessage> = self
            .history
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let recent_messages = recent.len();

        for msg in &recent {
            let role_str = match &msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::ToolResult { .. } => "ToolResult",
                Role::System => continue,
            };
            let text = msg.text_content();
            let _ = writeln!(transcript, "{role_str}: {text}");
        }

        let transcript_chars = transcript.chars().count();
        tracing::info!(
            summary_kind = "iteration_fallback",
            model = %comp_opts.model,
            recent_messages,
            transcript_chars,
            "Generating iteration fallback summary with compression model"
        );

        let prompt = format!(
            "The agent has reached its maximum iteration limit ({max}). \
             Summarize what has been accomplished, what remains unfinished, \
             and suggest next steps for the user. \
             You MUST write the summary in the same language as the user's most recent message. \
             Be concise.\n\n{transcript}",
            max = self.config.max_iterations,
        );

        let messages = vec![ConversationMessage::user(prompt)];
        match comp_provider.chat(&messages, &[], comp_opts).await {
            Ok(response) => {
                let summary = response.text.unwrap_or_else(|| self.static_fallback());
                tracing::info!(
                    summary_kind = "iteration_fallback",
                    model = %comp_opts.model,
                    summary_chars = summary.chars().count(),
                    "Iteration fallback summary generated"
                );
                summary
            }
            Err(e) => {
                tracing::warn!(
                    summary_kind = "iteration_fallback",
                    model = %comp_opts.model,
                    "Failed to generate iteration fallback summary: {e}"
                );
                self.static_fallback()
            }
        }
    }

    fn static_fallback(&self) -> String {
        format!(
            "Reached the maximum iteration limit ({}). \
             Some work may be incomplete. Please review the conversation \
             and continue with a follow-up message if needed.",
            self.config.max_iterations,
        )
    }

    async fn consume_stream(
        &self,
        provider: &dyn Provider,
        messages: &[ConversationMessage],
        tool_specs: &[ToolSpec],
        opts: &ChatOptions,
        on_text: &dyn Fn(&str),
    ) -> Result<TurnOutput> {
        let mut stream = provider.chat_stream(messages, tool_specs, opts).await?;

        let mut text_buf = String::new();
        let mut reasoning_buf = String::new();
        let mut thinking_blocks: Vec<ThinkingBlock> = Vec::new();
        let mut tool_starts: Vec<(String, String)> = Vec::new();
        let mut tool_args: HashMap<String, String> = HashMap::new();
        let mut usage = Usage::default();

        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::TextDelta(delta) => {
                    on_text(&delta);
                    text_buf.push_str(&delta);
                }
                StreamEvent::ToolCallStart { id, name } => {
                    tool_starts.push((id.clone(), name));
                    tool_args.entry(id).or_default();
                }
                StreamEvent::ToolCallDelta { id, args_chunk } => {
                    tool_args.entry(id).or_default().push_str(&args_chunk);
                }
                StreamEvent::AssistantMeta(meta) => {
                    let kind = meta.get("kind").and_then(serde_json::Value::as_str);
                    match kind {
                        Some("openai_reasoning_content_delta") => {
                            if let Some(delta) = meta.get("delta").and_then(serde_json::Value::as_str)
                            {
                                reasoning_buf.push_str(delta);
                            }
                        }
                        Some("anthropic_thinking_block") => {
                            if let Some(block) = meta.get("block") {
                                match serde_json::from_value::<ThinkingBlock>(block.clone()) {
                                    Ok(tb) => thinking_blocks.push(tb),
                                    Err(e) => {
                                        tracing::warn!("Failed to parse thinking block from stream metadata: {e}");
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

        let mut tool_calls = Vec::new();
        let mut parse_errors = Vec::new();
        for (id, name) in tool_starts {
            let raw = tool_args.remove(&id).unwrap_or_default();
            match serde_json::from_str(&raw) {
                Ok(arguments) => tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                }),
                Err(e) => {
                    tracing::warn!(
                        tool = %name,
                        tool_call_id = %id,
                        raw = %summarize_text(&raw),
                        "Failed to parse tool args: {e}"
                    );
                    parse_errors.push((id, name));
                }
            }
        }

        let text = if text_buf.is_empty() {
            None
        } else {
            Some(text_buf)
        };
        let reasoning_content = if reasoning_buf.is_empty() {
            None
        } else {
            Some(reasoning_buf)
        };

        Ok(TurnOutput {
            text,
            reasoning_content,
            thinking_blocks,
            tool_calls,
            usage,
            parse_errors,
        })
    }

    async fn process_tool_calls(
        &mut self,
        tool_calls: &[ToolCall],
        tools: &[Box<dyn Tool>],
        iteration: usize,
        max_iterations: usize,
    ) {
        let tool_map: HashMap<&str, &dyn Tool> =
            tools.iter().map(|t| (t.name(), t.as_ref())).collect();
        let mut idx = 0;

        while idx < tool_calls.len() {
            let tc = &tool_calls[idx];
            let is_safe = tool_map
                .get(tc.name.as_str())
                .is_some_and(|tool| tool.is_safe_for_concurrent());

            if !is_safe {
                let result = self.execute_with_confirm(&tool_map, tc).await;
                self.push_tool_result(tc, result, iteration, max_iterations);
                idx += 1;
                continue;
            }

            let batch_start = idx;
            idx += 1;
            while idx < tool_calls.len() {
                let next = &tool_calls[idx];
                let next_safe = tool_map
                    .get(next.name.as_str())
                    .is_some_and(|tool| tool.is_safe_for_concurrent());
                if !next_safe {
                    break;
                }
                idx += 1;
            }

            let batch = &tool_calls[batch_start..idx];
            let futures: Vec<_> = batch
                .iter()
                .map(|call| self.execute_with_confirm(&tool_map, call))
                .collect();
            let results = futures::future::join_all(futures).await;
            for (call, result) in batch.iter().zip(results) {
                self.push_tool_result(call, result, iteration, max_iterations);
            }
        }
    }

    fn push_tool_result(
        &mut self,
        tc: &ToolCall,
        result: ToolResult,
        iteration: usize,
        max_iterations: usize,
    ) {
        let output = if result.output.len() > self.config.max_tool_output_chars {
            if self.has_subagent {
                let path = std::env::temp_dir().join(format!("zerda-tool-{}.txt", Uuid::new_v4()));
                match std::fs::write(&path, &result.output) {
                    Ok(()) => {
                        let len = result.output.len();
                        self.temp_files.push(path.clone());
                        format!(
                            "[Tool output too large ({len} chars), saved to {}. \
                             Use the `subagent` tool with this file_path to read and process the content.]",
                            path.display()
                        )
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save tool output to temp file: {e}");
                        result
                            .output
                            .truncate_for_ui(self.config.max_tool_output_chars)
                    }
                }
            } else {
                result
                    .output
                    .truncate_for_ui(self.config.max_tool_output_chars)
            }
        } else {
            result.output
        };

        let display_iter = iteration + 1;
        let mut tagged = format!("{output}\n[Iteration {display_iter}/{max_iterations}]");

        if max_iterations > 1 {
            if iteration + 1 == max_iterations * 9 / 10 {
                tagged.push_str(
                    "\n[Warning] Nearly out of iterations. Focus only on the most critical remaining work.",
                );
            } else if iteration + 1 == max_iterations * 3 / 4 {
                tagged.push_str(
                    "\n[Warning] 75% budget consumed. Prioritize essential steps and avoid exploratory actions.",
                );
            } else if iteration + 1 == max_iterations / 2 {
                tagged.push_str(
                    "\n[Warning] Half of iteration budget used. Consider reviewing your approach.",
                );
            }
        }

        self.history.push(ConversationMessage::tool_result(
            &tc.id,
            tagged,
            result.is_error,
        ));
    }

    pub async fn auto_compact(&mut self) -> Result<bool> {
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

        if self.compression_provider.is_none() {
            anyhow::bail!("History exceeds max_history but no compression provider configured");
        }

        self.compress_with_llm().await?;
        Ok(true)
    }

    pub async fn compress_with_llm(&mut self) -> Result<()> {
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
            let role_str = match &msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::ToolResult { .. } => "ToolResult",
                Role::System => continue,
            };
            let text = msg.text_content();
            let _ = writeln!(transcript, "{role_str}: {text}");
        }

        if transcript.len() > self.config.compression_transcript_max_chars {
            transcript.truncate(
                transcript.floor_char_boundary(self.config.compression_transcript_max_chars),
            );
        }

        let (comp_provider, comp_opts) = self
            .compression_provider
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No compression provider configured"))?;

        let prompt = format!(
            "Summarize this conversation into concise context for future turns. \
             Preserve: user preferences, commitments, decisions, unresolved tasks, key facts. \
             Omit: filler, repeated chit-chat, verbose tool logs. \
             Output plain text bullet points only.\n\n{transcript}"
        );

        tracing::info!(
            summary_kind = "history_compaction",
            model = %comp_opts.model,
            compacted_messages = indices_to_compact.len(),
            transcript_chars = transcript.chars().count(),
            "Generating history compaction summary with compression model"
        );

        let messages = vec![ConversationMessage::user(prompt)];
        let response = comp_provider.chat(&messages, &[], comp_opts).await?;

        let summary = response.text.unwrap_or_default();
        if summary.is_empty() {
            anyhow::bail!("Empty compression summary");
        }

        let summary = if summary.len() > self.config.compression_summary_max_chars {
            summary[..summary.floor_char_boundary(self.config.compression_summary_max_chars)]
                .to_string()
        } else {
            summary
        };

        let remove_set: std::collections::HashSet<usize> =
            indices_to_compact.iter().copied().collect();
        let mut idx = 0;
        self.history.retain(|_| {
            let keep = !remove_set.contains(&idx);
            idx += 1;
            keep
        });

        self.conversation_summary = Some(summary);

        tracing::info!(
            summary_kind = "history_compaction",
            model = %comp_opts.model,
            summary_chars = self
                .conversation_summary
                .as_ref()
                .map(|s| s.chars().count())
                .unwrap_or(0),
            remaining_history_messages = self.history.len(),
            "History compaction summary generated"
        );

        Ok(())
    }

    fn sanitized_history_for_provider(&self) -> Vec<ConversationMessage> {
        let mut sanitized = Vec::with_capacity(self.history.len());
        let mut pending: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut dropped_orphan_tool_results = 0usize;

        for msg in &self.history {
            match &msg.role {
                Role::System | Role::User => {
                    pending.clear();
                    sanitized.push(msg.clone());
                }
                Role::Assistant => {
                    pending.clear();
                    for tc in &msg.tool_calls {
                        pending.insert(tc.id.clone());
                    }
                    sanitized.push(msg.clone());
                }
                Role::ToolResult { tool_call_id, .. } => {
                    if pending.contains(tool_call_id) {
                        pending.remove(tool_call_id);
                        sanitized.push(msg.clone());
                    } else {
                        dropped_orphan_tool_results += 1;
                    }
                }
            }
        }

        if dropped_orphan_tool_results > 0 {
            tracing::warn!(
                dropped_orphan_tool_results,
                history_messages = self.history.len(),
                sanitized_messages = sanitized.len(),
                "Dropped orphan tool result messages before provider request"
            );
        }

        sanitized
    }

    async fn execute_with_confirm(
        &self,
        tool_map: &HashMap<&str, &dyn Tool>,
        call: &ToolCall,
    ) -> ToolResult {
        if self.config.confirm_tools.contains(&call.name) {
            if let Some(ref confirm) = self.confirm_fn {
                let name = call.name.clone();
                let args = call.arguments.clone();
                let confirm = Arc::clone(confirm);
                let allow = tokio::task::spawn_blocking(move || (confirm)(&name, &args))
                    .await
                    .unwrap_or(false);
                if !allow {
                    return ToolResult {
                        output: "Tool execution denied by user.".to_string(),
                        is_error: true,
                    };
                }
            }
        }
        let fut = execute_tool(tool_map, call);
        if let Some(timeout_secs) = self.config.tool_timeout {
            match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut).await {
                Ok(result) => result,
                Err(_) => ToolResult {
                    output: format!("Tool '{}' timed out after {timeout_secs}s", call.name),
                    is_error: true,
                },
            }
        } else {
            fut.await
        }
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

impl Drop for Agent {
    fn drop(&mut self) {
        for path in &self.temp_files {
            if let Err(e) = std::fs::remove_file(path) {
                tracing::debug!("Failed to clean up temp file {}: {e}", path.display());
            }
        }
    }
}

async fn execute_tool(tool_map: &HashMap<&str, &dyn Tool>, call: &ToolCall) -> ToolResult {
    let started = Instant::now();
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(
            tool = %call.name,
            tool_call_id = %call.id,
            args = %summarize_json(&call.arguments),
            "Tool start"
        );
    }
    if let Some(tool) = tool_map.get(call.name.as_str()) {
        match tool.execute(call.arguments.clone()).await {
            Ok(result) => {
                tracing::debug!(
                    tool = %call.name,
                    tool_call_id = %call.id,
                    is_error = result.is_error,
                    output_len = result.output.len(),
                    elapsed_ms = started.elapsed().as_millis(),
                    "Tool done"
                );
                result
            }
            Err(e) => {
                tracing::warn!(
                    tool = %call.name,
                    tool_call_id = %call.id,
                    elapsed_ms = started.elapsed().as_millis(),
                    "Tool execution error: {e}"
                );
                ToolResult {
                    output: format!("Tool execution error: {e}"),
                    is_error: true,
                }
            }
        }
    } else {
        ToolResult {
            output: format!("Unknown tool: {}", call.name),
            is_error: true,
        }
    }
}
