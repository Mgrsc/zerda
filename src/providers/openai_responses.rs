use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use std::collections::HashMap;

use super::{
    apply_sampling_mode, build_openai_content_parts, initial_sampling_mode,
    is_dual_sampling_conflict_error, preferred_single_sampling_mode, sse_stream, truncate_for_log,
    ChatOptions, ConversationMessage, HttpClient, Provider, ProviderResponse, Role, SamplingMode,
    StreamEvent, StreamResult, ThinkingBlock, ToolCall, ToolSpec, Usage,
};
use crate::config::ProviderEndpoint;

pub struct OpenAiResponsesProvider {
    http: HttpClient,
}

impl OpenAiResponsesProvider {
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
        let mut instructions: Option<String> = None;
        let mut input: Vec<Value> = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    instructions = Some(msg.text_content());
                }
                Role::User => {
                    let content = build_openai_content_parts(
                        &msg.content,
                        "input_text",
                        "input_image",
                        "image_url",
                        false,
                    );
                    input.push(json!({
                        "role": "user",
                        "content": content
                    }));
                }
                Role::Assistant => {
                    let mut content: Vec<Value> = Vec::new();
                    let text = msg.text_content();
                    if !text.is_empty() {
                        content.push(json!({
                            "type": "output_text",
                            "text": text
                        }));
                    }
                    if !content.is_empty() {
                        input.push(json!({
                            "role": "assistant",
                            "content": content
                        }));
                    }
                    for tc in &msg.tool_calls {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": tc.id,
                            "name": tc.name,
                            "arguments": tc.arguments.to_string()
                        }));
                    }
                }
                Role::ToolResult {
                    tool_call_id,
                    is_error,
                } => {
                    let text = msg.text_content();
                    let output = if *is_error {
                        format!("[ERROR] {text}")
                    } else {
                        text
                    };
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": tool_call_id,
                        "output": output
                    }));
                }
            }
        }

        let mut body = json!({
            "model": opts.model,
            "input": input
        });

        if let Some(max_tokens) = opts.max_tokens {
            body["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(inst) = instructions {
            body["instructions"] = json!(inst);
        }

        if !tools.is_empty() {
            let tool_defs: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    })
                })
                .collect();
            body["tools"] = json!(tool_defs);
        }

        body
    }

    fn parse_response(body: &Value) -> Result<ProviderResponse> {
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(output) = body.get("output").and_then(|o| o.as_array()) {
            for item in output {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("message") => {
                        if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                            for block in content {
                                if block.get("type").and_then(|t| t.as_str()) == Some("output_text")
                                {
                                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                        text_parts.push(t.to_string());
                                    }
                                }
                            }
                        }
                    }
                    Some("function_call") => {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments_str = item
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let arguments: Value = serde_json::from_str(arguments_str)
                            .context("Failed to parse function_call arguments")?;
                        tool_calls.push(ToolCall {
                            id: call_id,
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
            "OpenAI Responses response: has_text={}, tool_calls={}",
            text.as_ref().is_some_and(|t| !t.is_empty()),
            tool_calls.len()
        );
        if let Some(ref u) = usage {
            tracing::info!(
                "OpenAI Responses tokens: in={}, out={}",
                u.input_tokens,
                u.output_tokens
            );
        }

        Ok(ProviderResponse {
            text,
            tool_calls,
            usage,
            reasoning_content: None,
            thinking_blocks: Vec::<ThinkingBlock>::new(),
        })
    }
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
    async fn chat(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<ProviderResponse> {
        let mut body = self.build_request(messages, tools, opts);
        let mut sampling_mode = initial_sampling_mode(&opts.model, opts);
        apply_sampling_mode(&mut body, opts, sampling_mode);
        let url = format!("{}/responses", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];
        tracing::info!(
            "OpenAI Responses: model={}, sampling_mode={:?}, temperature={:?}, top_p={:?}",
            opts.model,
            sampling_mode,
            opts.temperature,
            opts.top_p
        );
        let resp_body = match self
            .http
            .send_request(&url, &headers, &body, "OpenAI Responses")
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
                        "OpenAI Responses dual-sampling conflict; retrying with single sampling parameter"
                    );
                    self.http
                        .send_request(&url, &headers, &body, "OpenAI Responses")
                        .await?
                } else {
                    return Err(err);
                }
            }
        };
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

        let url = format!("{}/responses", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];

        tracing::info!(
            "OpenAI Responses stream: model={}, sampling_mode={:?}, temperature={:?}, top_p={:?}",
            opts.model,
            sampling_mode,
            opts.temperature,
            opts.top_p
        );
        let resp = match self
            .http
            .send_stream_request(&url, &headers, &body, "OpenAI Responses")
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
                        "OpenAI Responses stream dual-sampling conflict; retrying with single sampling parameter"
                    );
                    self.http
                        .send_stream_request(&url, &headers, &body, "OpenAI Responses")
                        .await?
                } else {
                    return Err(err);
                }
            }
        };

        Ok(sse_stream(
            resp.bytes_stream(),
            HashMap::<String, String>::new(),
            |block, state| parse_responses_sse(block, state).into_iter().collect(),
        ))
    }
}

fn parse_responses_sse(
    block: &str,
    state: &mut HashMap<String, String>,
) -> Option<Result<StreamEvent>> {
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
        return None;
    }

    let data: Value = match serde_json::from_str(&data_str) {
        Ok(v) => v,
        Err(e) => {
            return Some(Err(anyhow::anyhow!(
                "OpenAI Responses SSE JSON parse error: {e}. raw={}",
                truncate_for_log(&data_str, 500)
            )));
        }
    };

    match event_type {
        "response.output_text.delta" => {
            let text = data.get("delta")?.as_str()?.to_string();
            Some(Ok(StreamEvent::TextDelta(text)))
        }
        "response.output_item.added" => {
            let item = data.get("item")?;
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                return None;
            }
            let call_id = item.get("call_id")?.as_str()?.to_string();
            let item_id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !item_id.is_empty() {
                state.insert(item_id, call_id.clone());
            }
            state.insert("_last".to_string(), call_id.clone());
            tracing::info!("OpenAI Responses tool call start: {name}");
            Some(Ok(StreamEvent::ToolCallStart { id: call_id, name }))
        }
        "response.function_call_arguments.delta" => {
            let chunk = data.get("delta")?.as_str()?.to_string();
            let item_id = data
                .get("item_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let id = if item_id.is_empty() {
                state.get("_last").cloned().unwrap_or_default()
            } else {
                state.get(&item_id).cloned().unwrap_or(item_id)
            };
            Some(Ok(StreamEvent::ToolCallDelta {
                id,
                args_chunk: chunk,
            }))
        }
        "response.completed" => {
            let usage = data
                .get("response")
                .and_then(|r| r.get("usage"))
                .map(|u| Usage::from_json(u, "input_tokens", "output_tokens"))
                .unwrap_or_default();
            tracing::info!(
                "OpenAI Responses stream done: in={}, out={}",
                usage.input_tokens,
                usage.output_tokens
            );
            Some(Ok(StreamEvent::Done(usage)))
        }
        _ => None,
    }
}
