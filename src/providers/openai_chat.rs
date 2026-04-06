use std::time::Instant;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    apply_sampling_mode, build_openai_content_parts, extract_model_ids, initial_sampling_mode,
    is_dual_sampling_conflict_error, preferred_single_sampling_mode, sse_stream, truncate_for_log,
    ChatOptions, ConversationMessage, HttpClient, Provider, ProviderResponse, Role, SamplingMode,
    StreamEvent, StreamResult, ThinkingBlock, Usage,
};
use crate::config::ProviderEndpoint;

pub struct OpenAiChatProvider {
    http: HttpClient,
}

impl OpenAiChatProvider {
    pub fn new(endpoint: &ProviderEndpoint) -> Self {
        Self {
            http: HttpClient::new(endpoint, "https://api.openai.com/v1"),
        }
    }

    fn build_request(&self, messages: &[ConversationMessage], opts: &ChatOptions) -> Value {
        let mut api_messages: Vec<Value> = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    let content = build_openai_content_parts(
                        &msg.content,
                        "text",
                        "image_url",
                        "image_url",
                        true,
                    );
                    api_messages.push(json!({
                        "role": "system",
                        "content": content
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
                    api_messages.push(message);
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

        let usage = body
            .get("usage")
            .map(|u| Usage::from_json(u, "prompt_tokens", "completion_tokens"));

        if let Some(ref u) = usage {
            tracing::info!(
                "OpenAI Chat done: in={}, out={}, text={}",
                u.input_tokens,
                u.output_tokens,
                text.as_ref().is_some_and(|t| !t.is_empty())
            );
        }

        Ok(ProviderResponse {
            text,
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
        opts: &ChatOptions,
    ) -> Result<ProviderResponse> {
        let mut body = self.build_request(messages, opts);
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
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let mut body = self.build_request(messages, opts);
        let mut sampling_mode = initial_sampling_mode(&opts.model, opts);
        apply_sampling_mode(&mut body, opts, sampling_mode);
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage": true});

        let url = format!("{}/chat/completions", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];

        tracing::trace!(
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
        tracing::trace!(
            "OpenAI Chat stream connected: model={}, elapsed_ms={}",
            opts.model,
            started.elapsed().as_millis()
        );

        Ok(sse_stream(resp.bytes_stream(), (), parse_openai_chat_sse))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];
        tracing::info!("OpenAI Chat list models: base_url={}", self.http.base_url);
        let body = self
            .http
            .send_get_request(&url, &headers, "OpenAI Chat listmodel")
            .await?;
        let models = extract_model_ids(&body);
        tracing::info!("OpenAI Chat list models done: count={}", models.len());
        Ok(models)
    }
}

fn parse_openai_chat_sse(block: &str, _: &mut ()) -> Vec<Result<StreamEvent>> {
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

        events.extend(parse_openai_chat_stream_payload(&data));
    }

    events
}

fn parse_openai_chat_stream_payload(payload: &Value) -> Vec<Result<StreamEvent>> {
    if let Some(chunks) = payload.get("streamed_data").and_then(Value::as_array) {
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parse_openai_chat_stream_chunk(chunk));
        }
        if events.is_empty() {
            tracing::debug!(
                "OpenAI Chat stream payload contains streamed_data but produced no events"
            );
        }
        return events;
    }

    parse_openai_chat_stream_chunk(payload)
}

fn parse_openai_chat_stream_chunk(data: &Value) -> Vec<Result<StreamEvent>> {
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
    }

    if let Some(usage) = data.get("usage").filter(|u| !u.is_null()) {
        let u = Usage::from_json(usage, "prompt_tokens", "completion_tokens");
        tracing::debug!(
            event = "provider.chat.stream.done",
            provider = "openai_chat",
            input_tokens = u.input_tokens,
            output_tokens = u.output_tokens,
            "OpenAI Chat stream done"
        );
        events.push(Ok(StreamEvent::Done(u)));
    }

    events
}
