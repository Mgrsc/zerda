use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{
    apply_sampling_mode, build_openai_content_parts, extract_model_ids, initial_sampling_mode,
    is_dual_sampling_conflict_error, preferred_single_sampling_mode, sse_stream, truncate_for_log,
    ChatOptions, ContentPart, ConversationMessage, HttpClient, Provider, ProviderResponse, Role,
    SamplingMode, StreamEvent, StreamResult, ThinkingBlock, Usage,
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

    fn build_request(&self, messages: &[ConversationMessage], opts: &ChatOptions) -> Value {
        let mut instructions: Option<String> = None;
        let mut input: Vec<Value> = Vec::new();

        for msg in messages {
            match &msg.role {
                Role::System => {
                    let parts_text: Vec<&str> = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect();
                    instructions = Some(parts_text.join("\n\n"));
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

        body
    }

    fn parse_response(body: &Value) -> Result<ProviderResponse> {
        let mut text_parts: Vec<String> = Vec::new();

        if let Some(output) = body.get("output").and_then(|o| o.as_array()) {
            for item in output {
                if let Some("message") = item.get("type").and_then(|t| t.as_str()) {
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            }
                        }
                    }
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
            "OpenAI Responses response: has_text={}",
            text.as_ref().is_some_and(|t| !t.is_empty())
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
        opts: &ChatOptions,
    ) -> Result<ProviderResponse> {
        let mut body = self.build_request(messages, opts);
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
        opts: &ChatOptions,
    ) -> Result<StreamResult> {
        let mut body = self.build_request(messages, opts);
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

        Ok(sse_stream(resp.bytes_stream(), (), |block, state| {
            parse_responses_sse(block, state).into_iter().collect()
        }))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.http.base_url);
        let headers = [("Authorization", format!("Bearer {}", self.http.api_key))];
        tracing::info!(
            "OpenAI Responses list models: base_url={}",
            self.http.base_url
        );
        let body = self
            .http
            .send_get_request(&url, &headers, "OpenAI Responses listmodel")
            .await?;
        let models = extract_model_ids(&body);
        tracing::info!("OpenAI Responses list models done: count={}", models.len());
        Ok(models)
    }
}

fn parse_responses_sse(block: &str, _: &mut ()) -> Option<Result<StreamEvent>> {
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
        "response.completed" => {
            let usage = data
                .get("response")
                .and_then(|r| r.get("usage"))
                .map(|u| Usage::from_json(u, "input_tokens", "output_tokens"))
                .unwrap_or_default();
            tracing::info!(
                event = "provider.chat.stream.done",
                provider = "openai_responses",
                input_tokens = usage.input_tokens,
                output_tokens = usage.output_tokens,
                "OpenAI Responses stream done"
            );
            Some(Ok(StreamEvent::Done(usage)))
        }
        _ => None,
    }
}
