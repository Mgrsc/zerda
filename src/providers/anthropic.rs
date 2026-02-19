use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    sse_stream, truncate_for_log, ChatOptions, ContentPart, ConversationMessage, HttpClient,
    Provider, ProviderResponse, Role, StreamEvent, StreamResult, ThinkingBlock, ToolCall,
    ToolSpec, Usage,
};
use crate::config::ProviderConfig;

pub struct AnthropicProvider {
    http: HttpClient,
}

impl AnthropicProvider {
    pub fn new(config: &ProviderConfig) -> Self {
        Self {
            http: HttpClient::new(config, "https://api.anthropic.com/v1"),
        }
    }

    fn build_request(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Value {
        let mut system_text = String::new();
        let mut api_messages: Vec<Value> = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    system_text = msg.text_content();
                }
                Role::User => {
                    let content = Self::build_content_parts(&msg.content);
                    api_messages.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
                Role::Assistant => {
                    let mut content: Vec<Value> =
                        msg.thinking_blocks.iter().map(|b| json!(b)).collect();
                    let text = msg.text_content();
                    if !text.is_empty() {
                        content.push(json!({ "type": "text", "text": text }));
                    }
                    for tc in &msg.tool_calls {
                        content.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments
                        }));
                    }
                    if !content.is_empty() {
                        api_messages.push(json!({
                            "role": "assistant",
                            "content": content
                        }));
                    }
                }
                Role::ToolResult {
                    tool_call_id,
                    is_error,
                } => {
                    let text = msg.text_content();
                    let mut block = json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": text
                    });
                    if *is_error {
                        block["is_error"] = json!(true);
                    }
                    api_messages.push(json!({
                        "role": "user",
                        "content": [block]
                    }));
                }
            }
        }

        let merged = Self::merge_consecutive_user_messages(api_messages);

        let mut body = json!({
            "model": opts.model,
            "max_tokens": opts.max_tokens,
            "temperature": opts.temperature,
            "messages": merged
        });

        if !system_text.is_empty() {
            body["system"] = json!(system_text);
        }

        if !tools.is_empty() {
            let tool_defs: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters
                    })
                })
                .collect();
            body["tools"] = json!(tool_defs);
        }

        body
    }

    fn merge_consecutive_user_messages(messages: Vec<Value>) -> Vec<Value> {
        let mut result: Vec<Value> = Vec::new();
        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "user" {
                if let Some(last) = result.last_mut() {
                    if last.get("role").and_then(|r| r.as_str()) == Some("user") {
                        if let (Some(existing), Some(new_content)) =
                            (last.get_mut("content"), msg.get("content"))
                        {
                            if let (Some(arr), Some(new_arr)) =
                                (existing.as_array_mut(), new_content.as_array())
                            {
                                arr.extend(new_arr.iter().cloned());
                            } else if let (Some(arr), Some(text)) =
                                (existing.as_array_mut(), new_content.as_str())
                            {
                                arr.push(json!({"type": "text", "text": text}));
                            }
                        }
                        continue;
                    }
                }
            }
            result.push(msg);
        }
        result
    }

    fn build_content_parts(parts: &[ContentPart]) -> Value {
        let blocks: Vec<Value> = parts
            .iter()
            .map(|p| match p {
                ContentPart::Text(t) => json!({"type": "text", "text": t}),
                ContentPart::ImageUrl { url } => json!({
                    "type": "image",
                    "source": { "type": "url", "url": url }
                }),
                ContentPart::ImageBase64 { media_type, data } => json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media_type, "data": data }
                }),
            })
            .collect();
        json!(blocks)
    }

    fn parse_response(body: &Value) -> ProviderResponse {
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut thinking_blocks: Vec<ThinkingBlock> = Vec::new();

        if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
            for block in content {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("thinking") => {
                        let thinking = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let signature = block
                            .get("signature")
                            .and_then(Value::as_str)
                            .map(std::string::ToString::to_string);
                        if !thinking.is_empty() || signature.is_some() {
                            thinking_blocks.push(ThinkingBlock::Thinking { thinking, signature });
                        }
                    }
                    Some("redacted_thinking") => {
                        let data = block
                            .get("data")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !data.is_empty() {
                            thinking_blocks.push(ThinkingBlock::RedactedThinking { data });
                        }
                    }
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = block.get("input").cloned().unwrap_or(json!({}));
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = body
            .get("usage")
            .map(|u| Usage::from_json(u, "input_tokens", "output_tokens"));

        let text = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        tracing::debug!(
            "Anthropic response: has_text={}, tool_calls={}",
            text.as_ref().is_some_and(|t| !t.is_empty()),
            tool_calls.len()
        );
        if let Some(ref u) = usage {
            tracing::info!(
                "Anthropic tokens: in={}, out={}",
                u.input_tokens,
                u.output_tokens
            );
        }

        ProviderResponse {
            text,
            tool_calls,
            usage,
            reasoning_content: None,
            thinking_blocks,
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<ProviderResponse> {
        let body = self.build_request(messages, tools, opts);
        let url = format!("{}/messages", self.http.base_url);
        let headers = [
            ("x-api-key", self.http.api_key.clone()),
            ("anthropic-version", "2023-06-01".to_string()),
        ];
        tracing::info!("Anthropic chat: model={}", opts.model);
        let resp_body = self
            .http
            .send_request(&url, &headers, &body, "Anthropic")
            .await?;
        Ok(Self::parse_response(&resp_body))
    }

    async fn chat_stream(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let mut body = self.build_request(messages, tools, opts);
        body["stream"] = json!(true);

        let url = format!("{}/messages", self.http.base_url);
        let headers = [
            ("x-api-key", self.http.api_key.clone()),
            ("anthropic-version", "2023-06-01".to_string()),
        ];

        tracing::info!("Anthropic stream: model={}", opts.model);
        let resp = self
            .http
            .send_stream_request(&url, &headers, &body, "Anthropic")
            .await?;

        Ok(sse_stream(
            resp.bytes_stream(),
            AnthropicStreamState::default(),
            parse_sse_event,
        ))
    }
}

#[derive(Default)]
struct AnthropicStreamState {
    current_tool_id: String,
    accumulated_input_tokens: u64,
    pending_thinking: HashMap<usize, PendingThinkingBlock>,
}

#[derive(Default)]
struct PendingThinkingBlock {
    kind: Option<PendingThinkingKind>,
    thinking: String,
    signature: Option<String>,
    data: String,
}

enum PendingThinkingKind {
    Thinking,
    RedactedThinking,
}

fn parse_sse_event(
    block: &str,
    state: &mut AnthropicStreamState,
) -> Vec<Result<StreamEvent>> {
    let mut event_type = "";
    let mut data_str = String::new();

    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = rest.trim();
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data_str = rest.to_string();
        }
    }

    if data_str.is_empty() {
        return Vec::new();
    }

    let data: Value = match serde_json::from_str(&data_str) {
        Ok(v) => v,
        Err(e) => {
            return vec![Err(anyhow::anyhow!(
                "Anthropic SSE JSON parse error: {e}. raw={}",
                truncate_for_log(&data_str, 500)
            ))];
        }
    };

    match event_type {
        "message_start" => {
            if let Some(input) = data
                .get("message")
                .and_then(|m| m.get("usage"))
                .and_then(|u| u.get("input_tokens"))
                .and_then(Value::as_u64)
            {
                state.accumulated_input_tokens = input;
            }
            Vec::new()
        }
        "content_block_start" => {
            let Some(cb) = data.get("content_block") else {
                return Vec::new();
            };
            let Some(index) = data.get("index").and_then(Value::as_u64).map(|v| v as usize) else {
                return Vec::new();
            };
            let Some(block_type) = cb.get("type").and_then(Value::as_str) else {
                return Vec::new();
            };
            match block_type {
                "tool_use" => {
                    let Some(id) = cb.get("id").and_then(Value::as_str) else {
                        return Vec::new();
                    };
                    let Some(name) = cb.get("name").and_then(Value::as_str) else {
                        return Vec::new();
                    };
                    let id = id.to_string();
                    let name = name.to_string();
                    state.current_tool_id.clone_from(&id);
                    tracing::info!("Anthropic tool call start: {name}");
                    vec![Ok(StreamEvent::ToolCallStart { id, name })]
                }
                "thinking" => {
                    let pending = state.pending_thinking.entry(index).or_default();
                    pending.kind = Some(PendingThinkingKind::Thinking);
                    if let Some(thinking) = cb.get("thinking").and_then(Value::as_str) {
                        pending.thinking.push_str(thinking);
                    }
                    if let Some(signature) = cb.get("signature").and_then(Value::as_str) {
                        pending.signature = Some(signature.to_string());
                    }
                    Vec::new()
                }
                "redacted_thinking" => {
                    let pending = state.pending_thinking.entry(index).or_default();
                    pending.kind = Some(PendingThinkingKind::RedactedThinking);
                    if let Some(data) = cb.get("data").and_then(Value::as_str) {
                        pending.data.push_str(data);
                    }
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        "content_block_delta" => {
            let Some(delta) = data.get("delta") else {
                return Vec::new();
            };
            let Some(index) = data.get("index").and_then(Value::as_u64).map(|v| v as usize) else {
                return Vec::new();
            };
            let Some(delta_type) = delta.get("type").and_then(Value::as_str) else {
                return Vec::new();
            };
            match delta_type {
                "text_delta" => {
                    let Some(text) = delta.get("text").and_then(Value::as_str) else {
                        return Vec::new();
                    };
                    vec![Ok(StreamEvent::TextDelta(text.to_string()))]
                }
                "input_json_delta" => {
                    let Some(chunk) = delta.get("partial_json").and_then(Value::as_str) else {
                        return Vec::new();
                    };
                    vec![Ok(StreamEvent::ToolCallDelta {
                        id: state.current_tool_id.clone(),
                        args_chunk: chunk.to_string(),
                    })]
                }
                "thinking_delta" => {
                    if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                        let pending = state.pending_thinking.entry(index).or_default();
                        if pending.kind.is_none() {
                            pending.kind = Some(PendingThinkingKind::Thinking);
                        }
                        pending.thinking.push_str(thinking);
                    }
                    Vec::new()
                }
                "signature_delta" => {
                    if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                        let pending = state.pending_thinking.entry(index).or_default();
                        pending.signature = Some(signature.to_string());
                    }
                    Vec::new()
                }
                "redacted_thinking_delta" => {
                    if let Some(chunk) = delta.get("data").and_then(Value::as_str) {
                        let pending = state.pending_thinking.entry(index).or_default();
                        if pending.kind.is_none() {
                            pending.kind = Some(PendingThinkingKind::RedactedThinking);
                        }
                        pending.data.push_str(chunk);
                    }
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        "content_block_stop" => {
            let Some(index) = data.get("index").and_then(Value::as_u64).map(|v| v as usize) else {
                return Vec::new();
            };
            let Some(pending) = state.pending_thinking.remove(&index) else {
                return Vec::new();
            };
            let PendingThinkingBlock {
                kind,
                thinking,
                signature,
                data,
            } = pending;
            let block = match kind {
                Some(PendingThinkingKind::Thinking) => Some(ThinkingBlock::Thinking {
                    thinking,
                    signature,
                }),
                Some(PendingThinkingKind::RedactedThinking) => {
                    Some(ThinkingBlock::RedactedThinking { data })
                }
                None => None,
            };
            if let Some(block) = block {
                vec![Ok(StreamEvent::AssistantMeta(json!({
                    "kind": "anthropic_thinking_block",
                    "block": block
                })))]
            } else {
                Vec::new()
            }
        }
        "message_delta" => {
            let output_tokens = data
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let usage = Usage {
                input_tokens: state.accumulated_input_tokens,
                output_tokens,
            };
            tracing::info!(
                "Anthropic stream done: in={}, out={}",
                usage.input_tokens,
                usage.output_tokens
            );
            vec![Ok(StreamEvent::Done(usage))]
        }
        _ => Vec::new(),
    }
}
