use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::{self, Stream};
use futures::StreamExt as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ProviderConfig, RetryConfig};
use crate::logging::Redacted;

pub mod anthropic;
pub mod openai_chat;
pub mod openai_responses;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    ToolResult {
        tool_call_id: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentPart {
    Text(String),
    ImageUrl { url: String },
    ImageBase64 { media_type: String, data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: Role,
    pub content: Vec<ContentPart>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

impl ConversationMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: vec![ContentPart::Text(text.into())],
            tool_calls: Vec::new(),
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentPart::Text(text.into())],
            tool_calls: Vec::new(),
        }
    }

    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: parts,
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentPart::Text(text.into())],
            tool_calls: Vec::new(),
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        text: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::ToolResult {
                tool_call_id: tool_call_id.into(),
                is_error,
            },
            content: vec![ContentPart::Text(text.into())],
            tool_calls: Vec::new(),
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn from_json(value: &Value, input_key: &str, output_key: &str) -> Self {
        Self {
            input_tokens: value.get(input_key).and_then(Value::as_u64).unwrap_or(0),
            output_tokens: value.get(output_key).and_then(Value::as_u64).unwrap_or(0),
        }
    }
}

pub fn truncate_for_log(text: &str, max_len: usize) -> &str {
    if text.len() > max_len {
        &text[..text.floor_char_boundary(max_len)]
    } else {
        text
    }
}

pub fn build_openai_content_parts(
    parts: &[ContentPart],
    text_type: &str,
    image_type: &str,
    image_url_key: &str,
    nest_url: bool,
) -> Value {
    if parts.len() == 1 {
        if let ContentPart::Text(t) = &parts[0] {
            return serde_json::json!(t);
        }
    }

    let wrap_url = |url: String| -> Value {
        if nest_url {
            serde_json::json!({ "url": url })
        } else {
            serde_json::json!(url)
        }
    };

    let blocks: Vec<Value> = parts
        .iter()
        .map(|p| match p {
            ContentPart::Text(t) => serde_json::json!({"type": text_type, "text": t}),
            ContentPart::ImageUrl { url } => serde_json::json!({
                "type": image_type,
                image_url_key: wrap_url(url.clone())
            }),
            ContentPart::ImageBase64 { media_type, data } => serde_json::json!({
                "type": image_type,
                image_url_key: wrap_url(format!("data:{media_type};base64,{data}"))
            }),
        })
        .collect();
    serde_json::json!(blocks)
}

#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub model: String,
    pub temperature: f64,
    pub max_tokens: u32,
}

impl ChatOptions {
    pub fn from_provider_config(config: &ProviderConfig) -> Self {
        Self {
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_chunk: String },
    Done(Usage),
}

pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

pub fn sse_stream<S, State, F>(byte_stream: S, initial_state: State, parse_fn: F) -> StreamResult
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + Unpin + 'static,
    State: Send + 'static,
    F: Fn(&str, &mut State) -> Vec<Result<StreamEvent>> + Send + Sync + 'static,
{
    let parse_fn = std::sync::Arc::new(parse_fn);
    let event_stream = stream::unfold(
        (
            byte_stream,
            String::new(),
            initial_state,
            std::collections::VecDeque::<Result<StreamEvent>>::new(),
            parse_fn,
        ),
        |(mut byte_stream, mut buffer, mut state, mut pending, parse_fn)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((event, (byte_stream, buffer, state, pending, parse_fn)));
                }

                if let Some(line_end) = buffer.find("\n\n") {
                    let block = buffer[..line_end].to_string();
                    buffer.drain(..line_end + 2);

                    let mut events = parse_fn(&block, &mut state);
                    if !events.is_empty() {
                        let first = events.remove(0);
                        pending.extend(events);
                        return Some((first, (byte_stream, buffer, state, pending, parse_fn)));
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(anyhow::anyhow!("Stream error: {e}")),
                            (byte_stream, buffer, state, pending, parse_fn),
                        ));
                    }
                    None => {
                        if buffer.trim().is_empty() {
                            return None;
                        }
                        let block = std::mem::take(&mut buffer);
                        let mut events = parse_fn(&block, &mut state);
                        if !events.is_empty() {
                            let first = events.remove(0);
                            pending.extend(events);
                            return Some((first, (byte_stream, buffer, state, pending, parse_fn)));
                        }
                        return Some((
                            Err(anyhow::anyhow!(
                                "Stream ended with unparsed SSE buffer: {}",
                                truncate_for_log(&block, 500)
                            )),
                            (byte_stream, buffer, state, pending, parse_fn),
                        ));
                    }
                }
            }
        },
    );
    Box::pin(event_stream)
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<ProviderResponse>;

    async fn chat_stream(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let response = self.chat(messages, tools, opts).await?;
        let mut events: Vec<Result<StreamEvent>> = Vec::new();
        if let Some(text) = response.text {
            events.push(Ok(StreamEvent::TextDelta(text)));
        }
        for tc in &response.tool_calls {
            events.push(Ok(StreamEvent::ToolCallStart {
                id: tc.id.clone(),
                name: tc.name.clone(),
            }));
            events.push(Ok(StreamEvent::ToolCallDelta {
                id: tc.id.clone(),
                args_chunk: tc.arguments.to_string(),
            }));
        }
        events.push(Ok(StreamEvent::Done(response.usage.unwrap_or_default())));
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

type ProviderFactory = fn(&ProviderConfig) -> Result<Box<dyn Provider>>;

const REGISTRY: &[(&str, ProviderFactory)] = &[
    ("anthropic", |c| {
        Ok(Box::new(anthropic::AnthropicProvider::new(c)))
    }),
    ("openai_chat", |c| {
        Ok(Box::new(openai_chat::OpenAiChatProvider::new(c)))
    }),
    ("openai_responses", |c| {
        Ok(Box::new(openai_responses::OpenAiResponsesProvider::new(c)))
    }),
];

pub fn create_provider(config: &ProviderConfig) -> Result<Box<dyn Provider>> {
    let factory = REGISTRY
        .iter()
        .find(|(name, _)| *name == config.name)
        .map(|(_, f)| f)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", config.name))?;
    factory(config)
}

pub struct HttpClient {
    client: Client,
    pub api_key: String,
    pub base_url: String,
    retry_config: RetryConfig,
    extra_headers: std::collections::HashMap<String, String>,
}

impl HttpClient {
    pub fn new(config: &ProviderConfig, default_base_url: &str) -> Self {
        let base_url = if config.base_url.is_empty() {
            default_base_url.to_string()
        } else {
            config.base_url.trim_end_matches('/').to_string()
        };

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(config.retry.connect_timeout_secs))
            .timeout(Duration::from_secs(config.retry.request_timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            api_key: config.api_key.clone(),
            base_url,
            retry_config: config.retry.clone(),
            extra_headers: config.extra_headers.clone(),
        }
    }

    pub async fn send_request(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        provider_name: &str,
    ) -> Result<Value> {
        tracing::debug!("{provider_name} request body: {:?}", Redacted::new(body));
        let resp = self
            .send_with_retry(url, headers, body, provider_name)
            .await?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .await
            .with_context(|| format!("Failed to read {provider_name} response"))?;

        let resp_body: Value = serde_json::from_str(&raw_text).with_context(|| {
            let truncated = &raw_text[..raw_text.floor_char_boundary(500.min(raw_text.len()))];
            format!("Failed to parse {provider_name} response: {truncated}")
        })?;

        tracing::debug!("{provider_name} response: {:?}", Redacted::new(&resp_body));

        if !status.is_success() {
            let error_msg = resp_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            anyhow::bail!("{provider_name} API error ({status}): {error_msg}");
        }

        Ok(resp_body)
    }

    pub async fn send_stream_request(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        provider_name: &str,
    ) -> Result<reqwest::Response> {
        let resp = self
            .send_with_retry(url, headers, body, provider_name)
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let raw_text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&raw_text)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| {
                    let truncated =
                        &raw_text[..raw_text.floor_char_boundary(500.min(raw_text.len()))];
                    format!("Raw response: {truncated}")
                });
            anyhow::bail!("{provider_name} API error ({status}): {msg}");
        }

        Ok(resp)
    }

    async fn send_with_retry(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        provider_name: &str,
    ) -> Result<reqwest::Response> {
        let mut last_error: Option<anyhow::Error> = None;

        tracing::debug!("{provider_name} request to {url}");

        for attempt in 0..=self.retry_config.max_retries {
            if attempt > 0 {
                let delay = self.compute_backoff(attempt, None);
                tracing::warn!(
                    "{provider_name} retry {attempt}/{} after {delay}ms",
                    self.retry_config.max_retries
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }

            let mut req = self
                .client
                .post(url)
                .header("Content-Type", "application/json");
            for (key, value) in headers {
                req = req.header(*key, value);
            }
            for (key, value) in &self.extra_headers {
                req = req.header(key, value);
            }

            let resp = match req.json(body).send().await {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        tracing::warn!("{provider_name} network error (attempt {attempt}): {e}");
                        last_error = Some(e.into());
                        continue;
                    }
                    return Err(e).with_context(|| format!("{provider_name} API request failed"));
                }
            };

            let status = resp.status();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());
                let raw_text = resp.text().await.unwrap_or_default();
                let error_msg = serde_json::from_str::<Value>(&raw_text)
                    .ok()
                    .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                    .unwrap_or_else(|| {
                        let truncated =
                            &raw_text[..raw_text.floor_char_boundary(500.min(raw_text.len()))];
                        format!("Rate limited. Raw: {truncated}")
                    });
                tracing::warn!("{provider_name} rate limited (attempt {attempt}): {error_msg}");
                let delay = self.compute_backoff(attempt + 1, retry_after.map(|s| s * 1000));
                tokio::time::sleep(Duration::from_millis(delay)).await;
                last_error = Some(anyhow::anyhow!(
                    "{provider_name} API error ({status}): {error_msg}"
                ));
                continue;
            }

            if status.is_server_error() {
                let raw_text = resp.text().await.unwrap_or_default();
                let error_msg = serde_json::from_str::<Value>(&raw_text)
                    .ok()
                    .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                    .unwrap_or_else(|| {
                        let truncated =
                            &raw_text[..raw_text.floor_char_boundary(500.min(raw_text.len()))];
                        format!("Server error. Raw: {truncated}")
                    });
                tracing::warn!(
                    "{provider_name} server error {status} (attempt {attempt}): {error_msg}"
                );
                last_error = Some(anyhow::anyhow!(
                    "{provider_name} API error ({status}): {error_msg}"
                ));
                continue;
            }

            return Ok(resp);
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("{provider_name} request failed after retries")))
    }

    fn compute_backoff(&self, attempt: u32, retry_after_ms: Option<u64>) -> u64 {
        if let Some(ra) = retry_after_ms {
            return ra.min(self.retry_config.max_delay_ms);
        }
        let base = self.retry_config.base_delay_ms;
        let delay = base * 2u64.pow(attempt.saturating_sub(1));
        let jitter = simple_jitter(delay / 4 + 1);
        (delay + jitter).min(self.retry_config.max_delay_ms)
    }
}

fn simple_jitter(max: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos % max
}
