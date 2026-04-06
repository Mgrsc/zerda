use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::{self, Stream};
use futures::StreamExt as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ProviderEndpoint, RetryConfig};
use crate::logging::{summarize_http_body, summarize_json, summarize_text, text_fingerprint};

pub mod anthropic;
pub mod openai_chat;
pub mod openai_responses;
pub const LIST_MODELS_UNSUPPORTED: &str = "listmodel interface is not supported by this provider";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentPart {
    Text(String),
    ImageUrl { url: String },
    ImageBase64 { media_type: String, data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    #[default]
    Human,
    RuntimePtcResult,
    RuntimePtcNotice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    #[serde(default)]
    pub origin: MessageOrigin,
    #[serde(default)]
    pub related_job_id: Option<String>,
    #[serde(default)]
    pub related_turn_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: Role,
    pub content: Vec<ContentPart>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub thinking_blocks: Vec<ThinkingBlock>,
    #[serde(default)]
    pub metadata: MessageMetadata,
}

impl ConversationMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentPart::Text(text.into())],
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata::default(),
        }
    }

    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: parts,
            reasoning_content: None,
            thinking_blocks: Vec::new(),
            metadata: MessageMetadata::default(),
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingBlock {
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
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
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<u32>,
}

impl ChatOptions {
    pub fn from_model_config(mc: &crate::config::ModelConfig, model_name: &str) -> Self {
        Self {
            model: model_name.to_string(),
            temperature: mc.temperature,
            top_p: mc.top_p,
            max_tokens: mc.max_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingMode {
    Both,
    TemperatureOnly,
    TopPOnly,
    None,
}

pub fn preferred_single_sampling_mode(opts: &ChatOptions) -> SamplingMode {
    let has_temp = opts.temperature.is_some();
    let has_top_p = opts.top_p.is_some();
    if !has_temp && has_top_p {
        SamplingMode::TopPOnly
    } else {
        SamplingMode::TemperatureOnly
    }
}

pub fn initial_sampling_mode(model: &str, opts: &ChatOptions) -> SamplingMode {
    let has_temp = opts.temperature.is_some();
    let has_top_p = opts.top_p.is_some();
    if !has_temp && !has_top_p {
        return SamplingMode::None;
    }
    if model.to_ascii_lowercase().contains("claude") {
        preferred_single_sampling_mode(opts)
    } else if has_temp && has_top_p {
        SamplingMode::Both
    } else if has_temp {
        SamplingMode::TemperatureOnly
    } else {
        SamplingMode::TopPOnly
    }
}

pub fn apply_sampling_mode(body: &mut Value, opts: &ChatOptions, mode: SamplingMode) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    match mode {
        SamplingMode::Both => {
            if let Some(t) = opts.temperature {
                obj.insert("temperature".to_string(), serde_json::json!(t));
            }
            if let Some(p) = opts.top_p {
                obj.insert("top_p".to_string(), serde_json::json!(p));
            }
        }
        SamplingMode::TemperatureOnly => {
            if let Some(t) = opts.temperature {
                obj.insert("temperature".to_string(), serde_json::json!(t));
            }
            obj.remove("top_p");
        }
        SamplingMode::TopPOnly => {
            if let Some(p) = opts.top_p {
                obj.insert("top_p".to_string(), serde_json::json!(p));
            }
            obj.remove("temperature");
        }
        SamplingMode::None => {
            obj.remove("temperature");
            obj.remove("top_p");
        }
    }
}

pub fn is_dual_sampling_conflict_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    if !(msg.contains("temperature") && msg.contains("top_p")) {
        return false;
    }
    msg.contains("cannot both")
        || msg.contains("not both")
        || msg.contains("only one")
        || msg.contains("either")
        || msg.contains("does not allow both")
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: Option<String>,
    pub usage: Option<Usage>,
    pub reasoning_content: Option<String>,
    pub thinking_blocks: Vec<ThinkingBlock>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    AssistantMeta(Value),
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
        opts: &ChatOptions,
    ) -> Result<ProviderResponse>;

    async fn chat_stream(
        &self,
        messages: &[ConversationMessage],
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let response = self.chat(messages, opts).await?;
        let ProviderResponse {
            text,
            usage,
            reasoning_content,
            thinking_blocks,
        } = response;
        let mut events: Vec<Result<StreamEvent>> = Vec::new();
        if let Some(text) = text {
            events.push(Ok(StreamEvent::TextDelta(text)));
        }
        if let Some(reasoning_content) = reasoning_content {
            events.push(Ok(StreamEvent::AssistantMeta(serde_json::json!({
                "kind": "openai_reasoning_content_delta",
                "delta": reasoning_content
            }))));
        }
        for block in thinking_blocks {
            events.push(Ok(StreamEvent::AssistantMeta(serde_json::json!({
                "kind": "anthropic_thinking_block",
                "block": block
            }))));
        }
        events.push(Ok(StreamEvent::Done(usage.unwrap_or_default())));
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Err(anyhow::anyhow!(LIST_MODELS_UNSUPPORTED))
    }
}

type ProviderFactory = fn(&ProviderEndpoint) -> Result<Box<dyn Provider>>;

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

pub fn create_provider(endpoint: &ProviderEndpoint) -> Result<Box<dyn Provider>> {
    let factory = REGISTRY
        .iter()
        .find(|(name, _)| *name == endpoint.kind)
        .map(|(_, f)| f)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider type: {}", endpoint.kind))?;
    factory(endpoint)
}

pub struct ProviderRegistry {
    endpoints: std::collections::HashMap<String, ProviderEndpoint>,
    cache: std::collections::HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new(endpoints: Vec<ProviderEndpoint>) -> Result<Self> {
        let map: std::collections::HashMap<String, ProviderEndpoint> = endpoints
            .into_iter()
            .map(|ep| (ep.id.clone(), ep))
            .collect();
        Ok(Self {
            endpoints: map,
            cache: std::collections::HashMap::new(),
        })
    }

    pub fn get_or_create(&mut self, provider_id: &str) -> Result<Arc<dyn Provider>> {
        if let Some(cached) = self.cache.get(provider_id) {
            return Ok(Arc::clone(cached));
        }
        let endpoint = self
            .endpoints
            .get(provider_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown provider id: '{provider_id}'"))?;
        let provider: Arc<dyn Provider> = Arc::from(create_provider(endpoint)?);
        self.cache
            .insert(provider_id.to_string(), Arc::clone(&provider));
        Ok(provider)
    }

    pub fn list_provider_ids(&self) -> Vec<&str> {
        self.endpoints.keys().map(String::as_str).collect()
    }
}

pub struct HttpClient {
    client: Client,
    pub api_key: String,
    pub base_url: String,
    retry_config: RetryConfig,
    extra_headers: std::collections::HashMap<String, String>,
}

impl HttpClient {
    pub fn new(endpoint: &ProviderEndpoint, default_base_url: &str) -> Self {
        let base_url = if endpoint.base_url.is_empty() {
            default_base_url.to_string()
        } else {
            endpoint.base_url.trim_end_matches('/').to_string()
        };

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(endpoint.retry.connect_timeout_secs))
            .timeout(Duration::from_secs(endpoint.retry.request_timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            api_key: endpoint.api_key.clone(),
            base_url,
            retry_config: endpoint.retry.clone(),
            extra_headers: endpoint.extra_headers.clone(),
        }
    }

    pub async fn send_request(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &Value,
        provider_name: &str,
    ) -> Result<Value> {
        let resp = self
            .send_with_retry(url, headers, body, provider_name)
            .await?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .await
            .with_context(|| format!("Failed to read {provider_name} response"))?;

        let resp_body: Value = serde_json::from_str(&raw_text).with_context(|| {
            format!(
                "Failed to parse {provider_name} response: {}",
                summarize_text(&raw_text)
            )
        })?;

        if !status.is_success() {
            let model = body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
            let payload = summarize_json(body);
            let error_msg = resp_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            anyhow::bail!(
                "{provider_name} API error ({status}) method=POST url={url} model={model} stream={stream} payload={payload}: {error_msg}"
            );
        }

        Ok(resp_body)
    }

    pub async fn send_get_request(
        &self,
        url: &str,
        headers: &[(&str, String)],
        provider_name: &str,
    ) -> Result<Value> {
        let resp = self
            .send_get_with_retry(url, headers, provider_name)
            .await?;

        let status = resp.status();
        let raw_text = resp
            .text()
            .await
            .with_context(|| format!("Failed to read {provider_name} response"))?;

        if !status.is_success() {
            let error_msg = serde_json::from_str::<Value>(&raw_text)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| summarize_http_body(&raw_text));
            anyhow::bail!("{provider_name} API error ({status}): {error_msg}");
        }

        let resp_body: Value = serde_json::from_str(&raw_text).with_context(|| {
            format!(
                "Failed to parse {provider_name} response: {}",
                summarize_text(&raw_text)
            )
        })?;

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
            let model = body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
            let payload = summarize_json(body);
            let msg = serde_json::from_str::<Value>(&raw_text)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| format!("Raw response: {}", summarize_http_body(&raw_text)));
            anyhow::bail!(
                "{provider_name} API error ({status}) method=POST url={url} model={model} stream={stream} payload={payload}: {msg}"
            );
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
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
        let payload = summarize_json(body);
        let request_fp = text_fingerprint(&format!("{url}|{payload}"));
        let header_keys = headers
            .iter()
            .map(|(k, _)| *k)
            .chain(self.extra_headers.keys().map(String::as_str))
            .collect::<Vec<_>>()
            .join(",");

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                event = "provider.chat.request.trace",
                provider = provider_name,
                url = %url,
                payload = %summarize_json(body),
                "Provider request"
            );
        }
        tracing::debug!(
            event = "provider.chat.request.dispatch",
            provider = provider_name,
            url = %url,
            model = %model,
            stream,
            request_fp = %request_fp,
            header_keys = %header_keys,
            payload = %payload,
            "Provider request dispatch"
        );

        for attempt in 0..=self.retry_config.max_retries {
            let attempt_started = Instant::now();
            if attempt > 0 {
                let delay = self.compute_backoff(attempt, None);
                tracing::info!(
                    event = "provider.chat.retry.scheduled",
                    provider = provider_name,
                    attempt,
                    max_retries = self.retry_config.max_retries,
                    delay_ms = delay,
                    "Provider retry scheduled"
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
                        tracing::warn!(
                            event = "provider.chat.request.error",
                            error_kind = "network",
                            provider = provider_name,
                            attempt,
                            model = %model,
                            stream,
                            url = %url,
                            request_fp = %request_fp,
                            elapsed_ms = attempt_started.elapsed().as_millis(),
                            "Provider network error: {e}"
                        );
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
                        format!("Rate limited. Raw: {}", summarize_http_body(&raw_text))
                    });
                tracing::warn!(
                    event = "provider.chat.response.error",
                    error_kind = "rate_limited",
                    provider = provider_name,
                    status = %status,
                    attempt,
                    model = %model,
                    stream,
                    url = %url,
                    request_fp = %request_fp,
                    elapsed_ms = attempt_started.elapsed().as_millis(),
                    "Provider rate limited: {error_msg}"
                );
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
                        format!("Server error. Raw: {}", summarize_http_body(&raw_text))
                    });
                tracing::warn!(
                    event = "provider.chat.response.error",
                    error_kind = "provider_5xx",
                    provider = provider_name,
                    status = %status,
                    attempt,
                    model = %model,
                    stream,
                    url = %url,
                    request_fp = %request_fp,
                    elapsed_ms = attempt_started.elapsed().as_millis(),
                    "Provider server error: {error_msg}"
                );
                last_error = Some(anyhow::anyhow!(
                    "{provider_name} API error ({status}) method=POST url={url} model={model} stream={stream} payload={payload}: {error_msg}"
                ));
                continue;
            }

            if status.is_client_error() {
                let raw_text = resp.text().await.unwrap_or_default();
                let error_msg = serde_json::from_str::<Value>(&raw_text)
                    .ok()
                    .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                    .unwrap_or_else(|| {
                        format!("Client error. Raw: {}", summarize_http_body(&raw_text))
                    });
                tracing::warn!(
                    event = "provider.chat.response.error",
                    error_kind = "provider_4xx",
                    provider = provider_name,
                    status = %status,
                    attempt,
                    model = %model,
                    stream,
                    url = %url,
                    request_fp = %request_fp,
                    payload = %payload,
                    elapsed_ms = attempt_started.elapsed().as_millis(),
                    "Provider client error response: {error_msg}"
                );
                return Err(anyhow::anyhow!(
                    "{provider_name} API error ({status}) method=POST url={url} model={model} stream={stream} payload={payload}: {error_msg}"
                ));
            }

            tracing::trace!(
                event = "provider.chat.response.ok",
                provider = provider_name,
                status = %status,
                attempt,
                model = %model,
                stream,
                url = %url,
                request_fp = %request_fp,
                elapsed_ms = attempt_started.elapsed().as_millis(),
                "Provider response"
            );
            return Ok(resp);
        }

        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("{provider_name} request failed after retries")))
    }

    async fn send_get_with_retry(
        &self,
        url: &str,
        headers: &[(&str, String)],
        provider_name: &str,
    ) -> Result<reqwest::Response> {
        let mut last_error: Option<anyhow::Error> = None;

        tracing::trace!(
            event = "provider.get.request.trace",
            provider = provider_name,
            url = %url,
            "Provider GET request"
        );

        for attempt in 0..=self.retry_config.max_retries {
            let attempt_started = Instant::now();
            if attempt > 0 {
                let delay = self.compute_backoff(attempt, None);
                tracing::info!(
                    event = "provider.get.retry.scheduled",
                    provider = provider_name,
                    attempt,
                    max_retries = self.retry_config.max_retries,
                    delay_ms = delay,
                    "Provider retry scheduled"
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }

            let mut req = self.client.get(url);
            for (key, value) in headers {
                req = req.header(*key, value);
            }
            for (key, value) in &self.extra_headers {
                req = req.header(key, value);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        tracing::warn!(
                            event = "provider.get.request.error",
                            error_kind = "network",
                            provider = provider_name,
                            attempt,
                            elapsed_ms = attempt_started.elapsed().as_millis(),
                            "Provider network error: {e}"
                        );
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
                        format!("Rate limited. Raw: {}", summarize_http_body(&raw_text))
                    });
                tracing::warn!(
                    event = "provider.get.response.error",
                    error_kind = "rate_limited",
                    provider = provider_name,
                    attempt,
                    elapsed_ms = attempt_started.elapsed().as_millis(),
                    "Provider rate limited: {error_msg}"
                );
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
                        format!("Server error. Raw: {}", summarize_http_body(&raw_text))
                    });
                tracing::warn!(
                    event = "provider.get.response.error",
                    error_kind = "provider_5xx",
                    provider = provider_name,
                    status = %status,
                    attempt,
                    elapsed_ms = attempt_started.elapsed().as_millis(),
                    "Provider server error: {error_msg}"
                );
                last_error = Some(anyhow::anyhow!(
                    "{provider_name} API error ({status}): {error_msg}"
                ));
                continue;
            }

            tracing::trace!(
                event = "provider.get.response.ok",
                provider = provider_name,
                status = %status,
                attempt,
                elapsed_ms = attempt_started.elapsed().as_millis(),
                "Provider response"
            );
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

pub fn extract_model_ids(body: &Value) -> Vec<String> {
    let mut model_ids = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("id")
                        .and_then(Value::as_str)
                        .map(std::string::ToString::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    model_ids.sort();
    model_ids.dedup();
    model_ids
}

fn simple_jitter(max: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    nanos % max
}
