use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    apply_sampling_mode, build_openai_content_parts, initial_sampling_mode,
    is_dual_sampling_conflict_error, preferred_single_sampling_mode, sse_stream, truncate_for_log,
    ChatOptions, ConversationMessage, HttpClient, Provider, ProviderResponse, Role, SamplingMode,
    StreamEvent, StreamResult, ThinkingBlock, ToolCall, ToolSpec, Usage,
};
use crate::config::ProviderEndpoint;

pub struct OpenAiChatProvider {
    http: HttpClient,
}

#[derive(Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    extra_content: Option<Value>,
    pending_args: String,
    started: bool,
}

impl OpenAiChatProvider {
    pub fn new(endpoint: &ProviderEndpoint) -> Self {
        Self {
            http: HttpClient::new(endpoint, "https://api.openai.com/v1"),
        }
    }

    fn build_request(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Value {
        let mut api_messages: Vec<Value> = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    api_messages.push(json!({
                        "role": "system",
                        "content": msg.text_content()
                    }));
                }
                Role::User => {
                    let content = build_openai_content_parts(
                        &msg.content,
                        "text",
                        "image_url",
                        "image_url",
                        true,
                    );
                    api_messages.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
                Role::Assistant => {
                    let mut message = json!({
                        "role": "assistant",
                        "content": msg.text_content()
                    });
                    if let Some(reasoning_content) = &msg.reasoning_content {
                        message["reasoning_content"] = json!(reasoning_content);
                    }
                    if !msg.tool_calls.is_empty() {
                        let tool_calls: Vec<Value> = msg
                            .tool_calls
                            .iter()
                            .map(|tc| {
                                let mut call = json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments.to_string()
                                    }
                                });
                                if let Some(extra_content) = &tc.extra_content {
                                    call["extra_content"] = extra_content.clone();
                                }
                                tracing::debug!(
                                    tool_call_id = %tc.id,
                                    tool_name = %tc.name,
                                    has_extra_content = tc.extra_content.is_some(),
                                    "OpenAI Chat replaying assistant tool_call"
                                );
                                call
                            })
                            .collect();
                        message["tool_calls"] = json!(tool_calls);
                    }
                    api_messages.push(message);
                }
                Role::ToolResult {
                    tool_call_id,
                    is_error,
                } => {
                    let text = msg.text_content();
                    let content = if *is_error {
                        format!("[ERROR] {text}")
                    } else {
                        text
                    };
                    api_messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": content
                    }));
                }
            }
        }

        let mut body = json!({
            "model": opts.model,
            "messages": api_messages
        });

        if let Some(max_tokens) = opts.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        if !tools.is_empty() {
            let tool_defs: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tool_defs);
        }

        body
    }

    fn parse_response(body: &Value) -> Result<ProviderResponse> {
        let choice = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .context("No choices in OpenAI Chat response")?;

        let message = choice.get("message").context("No message in choice")?;

        let text = message
            .get("content")
            .and_then(|c| c.as_str())
            .map(std::string::ToString::to_string);
        let reasoning_content = message
            .get("reasoning_content")
            .and_then(|c| c.as_str())
            .map(std::string::ToString::to_string);

        let mut tool_calls: Vec<ToolCall> = Vec::new();
        if let Some(tcs) = message.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tcs {
                let id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let function = tc.get("function").context("No function in tool_call")?;
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments_str = function
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(arguments_str)
                    .context("Failed to parse tool call arguments")?;
                let extra_content = tc.get("extra_content").cloned();
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                    extra_content,
                });
            }
        }

        let usage = body
            .get("usage")
            .map(|u| Usage::from_json(u, "prompt_tokens", "completion_tokens"));

        if let Some(ref u) = usage {
            tracing::info!(
                "OpenAI Chat done: in={}, out={}, text={}, tools={}",
                u.input_tokens,
                u.output_tokens,
                text.as_ref().is_some_and(|t| !t.is_empty()),
                tool_calls.len()
            );
        }

        Ok(ProviderResponse {
            text,
            tool_calls,
            usage,
            reasoning_content,
            thinking_blocks: Vec::<ThinkingBlock>::new(),
        })
    }
}

#[async_trait]
impl Provider for OpenAiChatProvider {
    async fn chat(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<ProviderResponse> {
        let mut body = self.build_request(messages, tools, opts);
        let mut sampling_mode = initial_sampling_mode(&opts.model, opts);
        apply_sampling_mode(&mut body, opts, sampling_mode);
        let url = format!("{}/chat/completions", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];
        tracing::info!(
            "OpenAI Chat: model={}, sampling_mode={:?}, temperature={:?}, top_p={:?}",
            opts.model,
            sampling_mode,
            opts.temperature,
            opts.top_p
        );
        let started = Instant::now();
        let resp_body = match self
            .http
            .send_request(&url, &headers, &body, "OpenAI Chat")
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                if sampling_mode == SamplingMode::Both && is_dual_sampling_conflict_error(&err) {
                    sampling_mode = preferred_single_sampling_mode(opts);
                    apply_sampling_mode(&mut body, opts, sampling_mode);
                    tracing::warn!(
                        model = %opts.model,
                        sampling_mode = ?sampling_mode,
                        temperature = ?opts.temperature,
                        top_p = ?opts.top_p,
                        error = %err,
                        "OpenAI Chat dual-sampling conflict; retrying with single sampling parameter"
                    );
                    self.http
                        .send_request(&url, &headers, &body, "OpenAI Chat")
                        .await?
                } else {
                    return Err(err);
                }
            }
        };
        tracing::info!(
            "OpenAI Chat response received: model={}, elapsed_ms={}",
            opts.model,
            started.elapsed().as_millis()
        );
        Self::parse_response(&resp_body)
    }

    async fn chat_stream(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let mut body = self.build_request(messages, tools, opts);
        let mut sampling_mode = initial_sampling_mode(&opts.model, opts);
        apply_sampling_mode(&mut body, opts, sampling_mode);
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});

        let url = format!("{}/chat/completions", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];

        tracing::info!(
            "OpenAI Chat stream: model={}, sampling_mode={:?}, temperature={:?}, top_p={:?}",
            opts.model,
            sampling_mode,
            opts.temperature,
            opts.top_p
        );
        let started = Instant::now();
        let resp = match self
            .http
            .send_stream_request(&url, &headers, &body, "OpenAI Chat")
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                if sampling_mode == SamplingMode::Both && is_dual_sampling_conflict_error(&err) {
                    sampling_mode = preferred_single_sampling_mode(opts);
                    apply_sampling_mode(&mut body, opts, sampling_mode);
                    tracing::warn!(
                        model = %opts.model,
                        sampling_mode = ?sampling_mode,
                        temperature = opts.temperature,
                        top_p = opts.top_p,
                        error = %err,
                        "OpenAI Chat stream dual-sampling conflict; retrying with single sampling parameter"
                    );
                    self.http
                        .send_stream_request(&url, &headers, &body, "OpenAI Chat")
                        .await?
                } else {
                    return Err(err);
                }
            }
        };
        tracing::info!(
            "OpenAI Chat stream connected: model={}, elapsed_ms={}",
            opts.model,
            started.elapsed().as_millis()
        );

        Ok(sse_stream(
            resp.bytes_stream(),
            HashMap::<usize, PendingToolCall>::new(),
            parse_openai_chat_sse,
        ))
    }
}

fn parse_openai_chat_sse(
    block: &str,
    tool_calls_map: &mut HashMap<usize, PendingToolCall>,
) -> Vec<Result<StreamEvent>> {
    let mut payloads: Vec<&str> = block
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if payloads.is_empty() {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        payloads.push(trimmed);
    }

    let mut events = Vec::new();
    for data_str in payloads {
        if data_str == "[DONE]" {
            for (index, pending) in tool_calls_map.iter() {
                if !pending.started && !pending.pending_args.is_empty() {
                    tracing::warn!(
                        "OpenAI Chat stream ended with unresolved tool call at index {index}; dropping {} buffered argument bytes",
                        pending.pending_args.len()
                    );
                }
            }
            continue;
        }

        let data = match serde_json::from_str::<Value>(data_str) {
            Ok(v) => v,
            Err(e) => {
                events.push(Err(anyhow::anyhow!(
                    "OpenAI Chat SSE JSON parse error: {e}. raw={}",
                    truncate_for_log(data_str, 500)
                )));
                continue;
            }
        };

        events.extend(parse_openai_chat_stream_payload(&data, tool_calls_map));
    }

    events
}

fn parse_openai_chat_stream_payload(
    payload: &Value,
    tool_calls_map: &mut HashMap<usize, PendingToolCall>,
) -> Vec<Result<StreamEvent>> {
    if let Some(chunks) = payload.get("streamed_data").and_then(Value::as_array) {
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parse_openai_chat_stream_chunk(chunk, tool_calls_map));
        }
        if events.is_empty() {
            tracing::debug!(
                "OpenAI Chat stream payload contains streamed_data but produced no events"
            );
        }
        return events;
    }

    parse_openai_chat_stream_chunk(payload, tool_calls_map)
}

fn parse_openai_chat_stream_chunk(
    data: &Value,
    tool_calls_map: &mut HashMap<usize, PendingToolCall>,
) -> Vec<Result<StreamEvent>> {
    let mut events = Vec::new();

    let choice = data
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    let delta = choice.and_then(|c| c.get("delta"));

    if let Some(delta) = delta {
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                events.push(Ok(StreamEvent::TextDelta(content.to_string())));
            }
        }
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .or_else(|| delta.get("reasoning").and_then(Value::as_str))
        {
            if !reasoning.is_empty() {
                events.push(Ok(StreamEvent::AssistantMeta(json!({
                    "kind": "openai_reasoning_content_delta",
                    "delta": reasoning
                }))));
            }
        }

        if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
            for (fallback_index, tc) in tcs.iter().enumerate() {
                let index = if let Some(index) =
                    tc.get("index").and_then(Value::as_u64).map(|i| i as usize)
                {
                    index
                } else if let Some(id) = tc.get("id").and_then(Value::as_str) {
                    tool_calls_map
                        .iter()
                        .find_map(|(idx, pending)| {
                            (pending.id.as_deref() == Some(id)).then_some(*idx)
                        })
                        .unwrap_or(fallback_index)
                } else {
                    fallback_index
                };

                if tc.get("index").is_none() {
                    tracing::debug!(
                        fallback_index,
                        resolved_index = index,
                        has_id = tc.get("id").and_then(|v| v.as_str()).is_some(),
                        "OpenAI Chat stream tool_call missing index; applying compatibility fallback"
                    );
                }

                let pending = tool_calls_map.entry(index).or_default();

                if let Some(id) = tc.get("id").and_then(Value::as_str) {
                    pending.id = Some(id.to_string());
                    if pending.name.is_none() {
                        let initial_name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        pending.name = Some(initial_name);
                    }
                }
                if let Some(extra_content) = tc.get("extra_content") {
                    pending.extra_content = Some(extra_content.clone());
                }

                if let Some(name) = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                {
                    pending.name = Some(name.to_string());
                }

                if let Some(args) = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                {
                    if !args.is_empty() {
                        if pending.started {
                            if let Some(id) = &pending.id {
                                events.push(Ok(StreamEvent::ToolCallDelta {
                                    id: id.clone(),
                                    args_chunk: args.to_string(),
                                }));
                            } else {
                                pending.pending_args.push_str(args);
                            }
                        } else {
                            pending.pending_args.push_str(args);
                        }
                    }
                }

                if !pending.started {
                    if let (Some(id), Some(name)) = (pending.id.clone(), pending.name.clone()) {
                        pending.started = true;
                        tracing::info!("OpenAI Chat tool call start: {name}");
                        events.push(Ok(StreamEvent::ToolCallStart {
                            id: id.clone(),
                            name,
                            extra_content: pending.extra_content.clone(),
                        }));
                        if !pending.pending_args.is_empty() {
                            events.push(Ok(StreamEvent::ToolCallDelta {
                                id,
                                args_chunk: std::mem::take(&mut pending.pending_args),
                            }));
                        }
                    }
                }
            }
        }
    }

    if let Some(usage) = data.get("usage").filter(|u| !u.is_null()) {
        let u = Usage::from_json(usage, "prompt_tokens", "completion_tokens");
        tracing::info!(
            "OpenAI Chat stream done: in={}, out={}",
            u.input_tokens,
            u.output_tokens
        );
        events.push(Ok(StreamEvent::Done(u)));
    }

    events
}
