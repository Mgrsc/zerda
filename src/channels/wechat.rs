use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::{Channel, ChannelMessage, ChannelMessageOrigin};
use crate::providers::ContentPart;
use crate::rich_content::{self, RichSegment};

const DEFAULT_POLL_WAIT_MS: u64 = 30_000;
const DEFAULT_POLL_LIMIT: usize = 50;
const DEFAULT_LOGIN_POLL_INTERVAL_MS: u64 = 1_000;
const DEFAULT_LOGIN_MAX_POLLS: usize = 120;
const DEFAULT_LOGIN_RETRY_BACKOFF_MS: u64 = 5_000;
const DEFAULT_ACCOUNT_LABEL: &str = "default";
const DEFAULT_SEND_DELAY_MS: u64 = 350;
const DEFAULT_BUBBLE_SOFT_LIMIT: usize = 160;
const MAX_SENTENCES_PER_BUBBLE: usize = 4;
const WECHAT_PROMPT_SUPPLEMENT: &str = include_str!("../prompts/wechat_supplement.md");

#[derive(Clone)]
struct ConversationDispatch {
    conversation_id: String,
    account_id: String,
    context_token: Option<String>,
}

#[derive(Clone)]
pub struct WechatChannel {
    gateway_url: String,
    client: reqwest::Client,
    dispatch_by_recipient: Arc<RwLock<HashMap<String, ConversationDispatch>>>,
}

#[derive(Debug, Clone, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    account_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct AccountsResponse {
    accounts: Vec<Account>,
}

#[derive(Debug, Clone, Deserialize)]
struct Account {
    account_id: String,
    enabled: bool,
    configured: bool,
    base_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct LoginStartRequest {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LoginStartResponse {
    login_id: String,
    qrcode_url: String,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LoginStatusResponse {
    login_id: String,
    status: String,
    account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PullEventsRequest {
    cursor: Option<String>,
    account_id: String,
    wait_ms: u64,
    limit: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct PullEventsResponse {
    events: Vec<InboundEvent>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EventKind {
    Text,
    Image,
    Voice,
    File,
    Video,
}

#[derive(Debug, Clone, Deserialize)]
struct MediaDescriptor {
    media_id: String,
    kind: EventKind,
    filename: Option<String>,
    mime: Option<String>,
    transcript: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct InboundEvent {
    conversation_id: String,
    account_id: String,
    peer_id: String,
    context_token: Option<String>,
    kind: EventKind,
    text: Option<String>,
    quoted_text: Option<String>,
    media: Vec<MediaDescriptor>,
}

#[derive(Debug, Clone, Serialize)]
struct ActionBatch {
    conversation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_token: Option<String>,
    actions: Vec<OutboundAction>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutboundAction {
    Typing {
        status: TypingStatus,
    },
    SendText {
        text: String,
    },
    SendMedia {
        media_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum TypingStatus {
    Start,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionBatchResponse {
    ok: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct UploadMediaResponse {
    media_id: String,
}

impl WechatChannel {
    pub fn from_config(
        params: &serde_json::Value,
        _stt_provider: Option<Arc<dyn crate::stt::SttProvider>>,
    ) -> Result<Self> {
        let gateway_url = params
            .get("gateway_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("http://127.0.0.1:8080")
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            gateway_url,
            client: reqwest::Client::new(),
            dispatch_by_recipient: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.gateway_url, path);
        let response = self.client.get(&url).send().await?;
        parse_json_response(response, "wechat gateway get").await
    }

    async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.gateway_url, path);
        let response = self.client.post(&url).json(body).send().await?;
        parse_json_response(response, "wechat gateway post").await
    }

    async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T> {
        let url = format!("{}{}", self.gateway_url, path);
        let response = self.client.post(&url).multipart(form).send().await?;
        parse_json_response(response, "wechat gateway multipart post").await
    }

    async fn get_media_bytes(&self, media_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/v1/media/{media_id}", self.gateway_url);
        let response = self.client.get(url).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("wechat gateway media download failed ({status}): {body}");
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn wait_for_account(&self) -> Result<Account> {
        loop {
            match self.bootstrap_once().await {
                Ok(account) => return Ok(account),
                Err(error) => {
                    tracing::warn!(
                        gateway_url = %self.gateway_url,
                        "WeChat bootstrap failed: {error}"
                    );
                    tokio::time::sleep(Duration::from_millis(DEFAULT_LOGIN_RETRY_BACKOFF_MS)).await;
                }
            }
        }
    }

    async fn bootstrap_once(&self) -> Result<Account> {
        let health: HealthResponse = self.get_json("/v1/health").await?;
        tracing::info!(
            gateway_url = %self.gateway_url,
            gateway_status = %health.status,
            gateway_version = %health.version,
            account_count = health.account_count,
            "WeChat gateway reachable"
        );

        let accounts = self.fetch_accounts().await?;
        if let Some(account) = select_configured_account(&accounts)? {
            tracing::info!(
                gateway_url = %self.gateway_url,
                account_id = %account.account_id,
                base_url = %account.base_url,
                "Reusing persisted WeChat account"
            );
            return Ok(account);
        }

        let login: LoginStartResponse = self
            .post_json(
                "/v1/accounts/login/start",
                &LoginStartRequest {
                    label: DEFAULT_ACCOUNT_LABEL.to_string(),
                    base_url: None,
                },
            )
            .await?;
        self.emit_login_qr(&login)?;

        for _ in 0..DEFAULT_LOGIN_MAX_POLLS {
            tokio::time::sleep(Duration::from_millis(DEFAULT_LOGIN_POLL_INTERVAL_MS)).await;
            let status: LoginStatusResponse = self
                .get_json(&format!("/v1/accounts/login/{}", login.login_id))
                .await?;
            tracing::info!(
                login_id = %status.login_id,
                status = %status.status,
                "WeChat login status updated"
            );
            if status.status == "confirmed" {
                let account_id = status
                    .account_id
                    .as_deref()
                    .context("wechat login confirmed without account_id")?;
                let accounts = self.fetch_accounts().await?;
                if let Some(account) = accounts
                    .into_iter()
                    .find(|account| account.account_id == account_id)
                {
                    tracing::info!(
                        account_id = %account.account_id,
                        "WeChat login confirmed"
                    );
                    return Ok(account);
                }
                anyhow::bail!("wechat login confirmed but account not found in account list");
            }
            if status.status == "expired" {
                anyhow::bail!("wechat login expired before confirmation");
            }
        }

        anyhow::bail!(
            "wechat login polling exceeded max polls ({})",
            DEFAULT_LOGIN_MAX_POLLS
        );
    }

    async fn fetch_accounts(&self) -> Result<Vec<Account>> {
        let response: AccountsResponse = self.get_json("/v1/accounts").await?;
        Ok(response.accounts)
    }

    fn emit_login_qr(&self, login: &LoginStartResponse) -> Result<()> {
        let qrcode = render_qrcode(&login.qrcode_url)?;
        tracing::info!(
            login_id = %login.login_id,
            login_status = %login.status,
            qrcode_url = %login.qrcode_url,
            "WeChat login started; scan the QR code below"
        );
        eprintln!(
            "\n[wechat] login_id: {}\n[wechat] qrcode_url: {}\n\n{}\n",
            login.login_id, login.qrcode_url, qrcode
        );
        Ok(())
    }

    fn update_dispatch(
        &self,
        recipient: &str,
        conversation_id: &str,
        account_id: &str,
        context_token: Option<String>,
    ) {
        if let Ok(mut dispatch) = self.dispatch_by_recipient.write() {
            dispatch.insert(
                recipient.to_string(),
                ConversationDispatch {
                    conversation_id: conversation_id.to_string(),
                    account_id: account_id.to_string(),
                    context_token,
                },
            );
        }
    }

    fn resolve_dispatch(&self, recipient: &str) -> Result<ConversationDispatch> {
        let dispatch = self
            .dispatch_by_recipient
            .read()
            .ok()
            .and_then(|map| map.get(recipient).cloned())
            .with_context(|| {
                format!("WeChat conversation context not found for recipient '{recipient}'")
            })?;
        Ok(dispatch)
    }

    async fn send_action(&self, recipient: &str, action: OutboundAction) -> Result<()> {
        let dispatch = self.resolve_dispatch(recipient)?;
        let context_token = dispatch
            .context_token
            .context("WeChat dispatch context is missing context_token")?;
        let response: ActionBatchResponse = self
            .post_json(
                "/v1/actions",
                &ActionBatch {
                    conversation_id: dispatch.conversation_id,
                    context_token: Some(context_token),
                    actions: vec![action],
                },
            )
            .await?;
        if !response.ok {
            anyhow::bail!("wechat gateway action batch was not acknowledged as ok");
        }
        Ok(())
    }

    async fn send_text_bubbles(&self, recipient: &str, text: &str) -> Result<()> {
        let normalized = normalize_wechat_text(text);
        if normalized.is_empty() {
            return Ok(());
        }
        let bubbles = split_wechat_bubbles(&normalized);
        for (index, bubble) in bubbles.iter().enumerate() {
            self.send_action(
                recipient,
                OutboundAction::SendText {
                    text: bubble.clone(),
                },
            )
            .await?;
            if index + 1 < bubbles.len() {
                tokio::time::sleep(Duration::from_millis(DEFAULT_SEND_DELAY_MS)).await;
            }
        }
        Ok(())
    }

    async fn upload_media(&self, recipient: &str, path: &str) -> Result<String> {
        let dispatch = self.resolve_dispatch(recipient)?;
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read outbound WeChat media at '{path}'"))?;
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image")
            .to_string();
        let form = reqwest::multipart::Form::new()
            .text("account_id", dispatch.account_id)
            .text("kind", "image")
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes).file_name(filename),
            );
        let response: UploadMediaResponse = self.post_multipart("/v1/media", form).await?;
        Ok(response.media_id)
    }

    async fn send_rich_segments(&self, recipient: &str, message: &str) -> Result<()> {
        for segment in rich_content::extract_rich_segments(message) {
            match segment {
                RichSegment::Text(text) => self.send_text_bubbles(recipient, &text).await?,
                RichSegment::Image(source) => {
                    if source.starts_with("http://") || source.starts_with("https://") {
                        tracing::warn!(
                            recipient,
                            source = %source,
                            "WeChat outbound image URL is not supported yet; falling back to text"
                        );
                        self.send_text_bubbles(recipient, &format!("[image: {source}]"))
                            .await?;
                        continue;
                    }
                    match self.upload_media(recipient, &source).await {
                        Ok(media_id) => {
                            self.send_action(
                                recipient,
                                OutboundAction::SendMedia {
                                    media_id,
                                    caption: None,
                                },
                            )
                            .await?;
                        }
                        Err(error) => {
                            tracing::warn!(
                                recipient,
                                source = %source,
                                "WeChat outbound image upload failed: {error}"
                            );
                            self.send_text_bubbles(recipient, &format!("[image: {source}]"))
                                .await?;
                        }
                    }
                }
                RichSegment::Voice(path) => {
                    self.send_text_bubbles(recipient, &format!("[voice: {path}]"))
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn inbound_event_to_message(
        &self,
        event: InboundEvent,
    ) -> Result<Option<ChannelMessage>> {
        let recipient = format!("{}:{}", event.account_id, event.peer_id);
        self.update_dispatch(
            &recipient,
            &event.conversation_id,
            &event.account_id,
            event.context_token.clone(),
        );

        let quoted_prefix = event
            .quoted_text
            .as_deref()
            .map(str::trim)
            .filter(|quoted| !quoted.is_empty())
            .map(|quoted| format!("[quoted_message]: {quoted}\n"))
            .unwrap_or_default();

        let (content, content_parts) = match event.kind {
            EventKind::Text => {
                let text = event.text.unwrap_or_default();
                (format!("{quoted_prefix}{text}").trim().to_string(), None)
            }
            EventKind::Image => {
                if let Some(media) = event
                    .media
                    .iter()
                    .find(|media| matches!(media.kind, EventKind::Image))
                {
                    let bytes = match self.get_media_bytes(&media.media_id).await {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            tracing::warn!(media_id = %media.media_id, "Failed to download WeChat image: {error}");
                            return Ok(Some(ChannelMessage {
                                sender: recipient,
                                session_id: event.conversation_id,
                                content: "[system: The user sent an image through WeChat, but the image could not be loaded. Ask the user to resend it or describe it in text.]".to_string(),
                                content_parts: None,
                                channel: "wechat".to_string(),
                                origin: ChannelMessageOrigin::Human,
                                related_job_id: None,
                            }));
                        }
                    };
                    let mut parts = Vec::new();
                    let caption = event.text.unwrap_or_default();
                    let merged_caption = format!("{quoted_prefix}{caption}").trim().to_string();
                    if !merged_caption.is_empty() {
                        parts.push(ContentPart::Text(merged_caption));
                    }
                    parts.push(ContentPart::ImageBase64 {
                        media_type: media
                            .mime
                            .clone()
                            .unwrap_or_else(|| "image/jpeg".to_string()),
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                    });
                    ("[image_message]".to_string(), Some(parts))
                } else {
                    (
                        "[system: The user sent an image through WeChat, but no image payload was available. Ask the user to resend it.]".to_string(),
                        None,
                    )
                }
            }
            EventKind::Voice => {
                if let Some(transcript) = event
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    (
                        format!("{quoted_prefix}[voice_message]: {transcript}"),
                        None,
                    )
                } else if let Some(transcript) = event
                    .media
                    .iter()
                    .find(|media| matches!(media.kind, EventKind::Voice))
                    .and_then(|media| media.transcript.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    (
                        format!("{quoted_prefix}[voice_message]: {transcript}"),
                        None,
                    )
                } else {
                    (
                        "[system: The user sent a voice message through WeChat, but no transcript was provided by the gateway. Ask the user to type the message instead.]".to_string(),
                        None,
                    )
                }
            }
            EventKind::File => {
                let file_name = event
                    .media
                    .first()
                    .and_then(|media| media.filename.as_deref())
                    .unwrap_or("unnamed file");
                (
                    format!(
                        "{quoted_prefix}[system: The user sent a file through WeChat: {file_name}. The file is not automatically loaded into the model context. Ask whether they want specific help with it.]"
                    ),
                    None,
                )
            }
            EventKind::Video => {
                let file_name = event
                    .media
                    .first()
                    .and_then(|media| media.filename.as_deref())
                    .unwrap_or("video");
                (
                    format!(
                        "{quoted_prefix}[system: The user sent a video through WeChat: {file_name}. The video is not automatically loaded into the model context. Ask the user for a description or the exact part they need help with.]"
                    ),
                    None,
                )
            }
        };

        let parts_empty = content_parts.as_ref().is_none_or(Vec::is_empty);
        if content.is_empty() && parts_empty {
            return Ok(None);
        }

        Ok(Some(ChannelMessage {
            sender: recipient,
            session_id: event.conversation_id,
            content,
            content_parts,
            channel: "wechat".to_string(),
            origin: ChannelMessageOrigin::Human,
            related_job_id: None,
        }))
    }
}

#[async_trait]
impl Channel for WechatChannel {
    fn name(&self) -> &str {
        "wechat"
    }

    fn prompt_supplement(&self) -> Option<String> {
        Some(WECHAT_PROMPT_SUPPLEMENT.to_string())
    }

    async fn send_typing(&self, recipient: &str) -> Result<()> {
        self.send_action(
            recipient,
            OutboundAction::Typing {
                status: TypingStatus::Start,
            },
        )
        .await
    }

    async fn send(&self, message: &str, recipient: &str) -> Result<()> {
        if !rich_content::has_rich_markers(message) {
            return self.send_text_bubbles(recipient, message).await;
        }
        self.send_rich_segments(recipient, message).await
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        let mut cursor = None;
        let account = self.wait_for_account().await?;
        tracing::info!(
            gateway_url = %self.gateway_url,
            account_id = %account.account_id,
            "WeChat channel listener started"
        );

        loop {
            let response: PullEventsResponse = match self
                .post_json(
                    "/v1/events/pull",
                    &PullEventsRequest {
                        cursor: cursor.clone(),
                        account_id: account.account_id.clone(),
                        wait_ms: DEFAULT_POLL_WAIT_MS,
                        limit: DEFAULT_POLL_LIMIT,
                    },
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    tracing::warn!(
                        gateway_url = %self.gateway_url,
                        account_id = %account.account_id,
                        "WeChat event pull failed: {error}"
                    );
                    tokio::time::sleep(Duration::from_millis(DEFAULT_LOGIN_RETRY_BACKOFF_MS)).await;
                    continue;
                }
            };

            cursor = advance_pull_cursor(cursor, response.next_cursor.clone());

            for event in response.events {
                let Some(message) = self.inbound_event_to_message(event).await? else {
                    continue;
                };
                if let Err(error) = tx.send(message).await {
                    return Err(anyhow::anyhow!("WeChat receiver channel closed: {error}"));
                }
            }
        }
    }
}

fn parse_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    context: &str,
) -> impl std::future::Future<Output = Result<T>> {
    let context = context.to_string();
    async move {
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("{context} failed ({status}): {body}");
        }
        Ok(response.json::<T>().await?)
    }
}

fn select_configured_account(accounts: &[Account]) -> Result<Option<Account>> {
    let configured = accounts
        .iter()
        .filter(|account| account.enabled && account.configured)
        .cloned()
        .collect::<Vec<_>>();

    match configured.len() {
        0 => Ok(None),
        1 => Ok(configured.into_iter().next()),
        _ => anyhow::bail!(
            "multiple configured WeChat accounts found; Zerda supports only one account per gateway instance"
        ),
    }
}

fn render_qrcode(content: &str) -> Result<String> {
    let code = QrCode::new(content.as_bytes())?;
    Ok(code.render::<unicode::Dense1x2>().quiet_zone(false).build())
}

fn advance_pull_cursor(current: Option<String>, next: Option<String>) -> Option<String> {
    next.or(current)
}

fn normalize_wechat_text(text: &str) -> String {
    let mut normalized_lines = Vec::new();
    let mut last_blank = false;

    for raw_line in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let mut line = raw_line.trim().to_string();
        if line.is_empty() {
            if !last_blank {
                normalized_lines.push(String::new());
                last_blank = true;
            }
            continue;
        }
        last_blank = false;
        if let Some(stripped) = line.strip_prefix('#') {
            line = stripped.trim_start_matches('#').trim().to_string();
        }
        if let Some(stripped) = line.strip_prefix("- ") {
            line = stripped.trim().to_string();
        }
        if let Some(stripped) = line.strip_prefix("* ") {
            line = stripped.trim().to_string();
        }
        let mut cleaned = line
            .replace("```", "")
            .replace("**", "")
            .replace("__", "")
            .replace("~~", "")
            .replace("||", "")
            .replace('`', "");
        cleaned = cleaned.trim().to_string();
        if !cleaned.is_empty() {
            normalized_lines.push(cleaned);
        }
    }

    normalized_lines.join("\n").trim().to_string()
}

fn split_wechat_bubbles(text: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        fragments.extend(split_fragments(trimmed));
    }

    let mut bubbles = Vec::new();
    let mut current = String::new();
    let mut sentence_count = 0usize;

    for fragment in fragments {
        let fragment = fragment.trim();
        if fragment.is_empty() {
            continue;
        }
        let fragment_len = fragment.chars().count();
        let current_len = current.chars().count();
        let needs_new_bubble = !current.is_empty()
            && (sentence_count >= MAX_SENTENCES_PER_BUBBLE
                || current_len + fragment_len > DEFAULT_BUBBLE_SOFT_LIMIT);

        if needs_new_bubble {
            if ends_with_connector(&current) {
                current.push(' ');
                current.push_str(fragment);
                sentence_count += 1;
                continue;
            }
            bubbles.push(current.trim().to_string());
            current.clear();
            sentence_count = 0;
        }

        current.push_str(fragment);
        sentence_count += 1;
    }

    if !current.trim().is_empty() {
        bubbles.push(current.trim().to_string());
    }

    if bubbles.is_empty() {
        vec![text.trim().to_string()]
    } else {
        bubbles
    }
}

fn ends_with_connector(text: &str) -> bool {
    text.trim_end().ends_with(':') || text.trim_end().ends_with('：')
}

fn split_fragments(text: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | ';' | '；') {
            push_fragment(&mut fragments, &mut current);
        }
    }

    push_fragment(&mut fragments, &mut current);

    let mut expanded = Vec::new();
    for fragment in fragments {
        if fragment.chars().count() <= DEFAULT_BUBBLE_SOFT_LIMIT {
            expanded.push(fragment);
        } else {
            expanded.extend(split_long_fragment(&fragment, DEFAULT_BUBBLE_SOFT_LIMIT));
        }
    }
    expanded
}

fn push_fragment(fragments: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        fragments.push(trimmed.to_string());
    }
    current.clear();
}

fn split_long_fragment(text: &str, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for token in split_by_soft_delimiters(text) {
        let token_len = token.chars().count();
        let current_len = current.chars().count();
        if current_len > 0 && current_len + token_len > limit {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(&token);
        if current.chars().count() >= limit {
            chunks.push(current.trim().to_string());
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    chunks
}

fn split_by_soft_delimiters(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '，' | '、' | ',' | '：' | ':' | ' ') {
            tokens.push(current.clone());
            current.clear();
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    if tokens.len() == 1 {
        let single = tokens.pop().unwrap_or_default();
        let mut chars = String::new();
        let mut fallback = Vec::new();
        for ch in single.chars() {
            chars.push(ch);
            if chars.chars().count() >= DEFAULT_BUBBLE_SOFT_LIMIT {
                fallback.push(chars.clone());
                chars.clear();
            }
        }
        if !chars.is_empty() {
            fallback.push(chars);
        }
        return fallback;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    use crate::channels::Channel;

    use super::{
        advance_pull_cursor, ends_with_connector, normalize_wechat_text, select_configured_account,
        split_wechat_bubbles, Account, WechatChannel,
    };

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        path: String,
        body: Vec<u8>,
        content_type: Option<String>,
    }

    #[derive(Clone)]
    struct StubResponse {
        status_line: &'static str,
        content_type: &'static str,
        body: &'static str,
    }

    #[tokio::test]
    async fn send_image_marker_uploads_media_then_sends_media_action() {
        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let responses = vec![
            StubResponse {
                status_line: "HTTP/1.1 200 OK",
                content_type: "application/json",
                body: r#"{"media_id":"med_out_123","kind":"image","size":7,"status":"ready"}"#,
            },
            StubResponse {
                status_line: "HTTP/1.1 200 OK",
                content_type: "application/json",
                body: r#"{"ok":true}"#,
            },
        ];
        let gateway_url = spawn_stub_server(requests.clone(), responses)
            .await
            .expect("stub server should start");

        let image_path = write_temp_image(b"PNGTEST1");
        let channel =
            WechatChannel::from_config(&serde_json::json!({ "gateway_url": gateway_url }), None)
                .expect("channel should build");
        channel.update_dispatch(
            "acct-1:peer-1",
            "wechat:acct-1:peer-1",
            "acct-1",
            Some("ctx-1".to_string()),
        );

        channel
            .send(
                &format!("<image>{}</image>", image_path.display()),
                "acct-1:peer-1",
            )
            .await
            .expect("image send should succeed");

        let requests = requests.lock().await.clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/v1/media");
        assert!(requests[0]
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("multipart/form-data;")));
        assert!(body_contains(&requests[0].body, "name=\"account_id\""));
        assert!(body_contains(&requests[0].body, "acct-1"));
        assert!(body_contains(&requests[0].body, "name=\"kind\""));
        assert!(body_contains(&requests[0].body, "image"));
        assert!(body_contains(&requests[0].body, "filename=\""));
        assert_eq!(requests[1].path, "/v1/actions");
        assert!(body_contains(&requests[1].body, "\"type\":\"send_media\""));
        assert!(body_contains(
            &requests[1].body,
            "\"media_id\":\"med_out_123\""
        ));
        assert!(body_contains(
            &requests[1].body,
            "\"conversation_id\":\"wechat:acct-1:peer-1\""
        ));
        assert!(body_contains(
            &requests[1].body,
            "\"context_token\":\"ctx-1\""
        ));
    }

    #[tokio::test]
    async fn send_image_marker_falls_back_to_text_when_upload_fails() {
        let requests = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));
        let responses = vec![
            StubResponse {
                status_line: "HTTP/1.1 500 Internal Server Error",
                content_type: "text/plain",
                body: "upload failed",
            },
            StubResponse {
                status_line: "HTTP/1.1 200 OK",
                content_type: "application/json",
                body: r#"{"ok":true}"#,
            },
        ];
        let gateway_url = spawn_stub_server(requests.clone(), responses)
            .await
            .expect("stub server should start");

        let image_path = write_temp_image(b"PNGTEST2");
        let channel =
            WechatChannel::from_config(&serde_json::json!({ "gateway_url": gateway_url }), None)
                .expect("channel should build");
        channel.update_dispatch(
            "acct-2:peer-2",
            "wechat:acct-2:peer-2",
            "acct-2",
            Some("ctx-2".to_string()),
        );

        channel
            .send(
                &format!("<image>{}</image>", image_path.display()),
                "acct-2:peer-2",
            )
            .await
            .expect("send should fall back to text");

        let requests = requests.lock().await.clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].path, "/v1/actions");
        assert!(body_contains(&requests[1].body, "\"type\":\"send_text\""));
        assert!(body_contains(
            &requests[1].body,
            &format!("[image: {}]", image_path.display())
        ));
    }

    fn write_temp_image(bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zerda-wechat-test-{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        std::fs::write(&path, bytes).expect("temp image should be written");
        path
    }

    fn body_contains(body: &[u8], needle: &str) -> bool {
        String::from_utf8_lossy(body).contains(needle)
    }

    async fn spawn_stub_server(
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        responses: Vec<StubResponse>,
    ) -> anyhow::Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.expect("connection should arrive");
                let request = read_http_request(&mut stream)
                    .await
                    .expect("request should parse");
                requests.lock().await.push(request);
                let reply = format!(
                    "{}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.status_line,
                    response.content_type,
                    response.body.len(),
                    response.body
                );
                stream
                    .write_all(reply.as_bytes())
                    .await
                    .expect("response should write");
            }
        });
        Ok(format!("http://{addr}"))
    }

    async fn read_http_request(
        stream: &mut tokio::net::TcpStream,
    ) -> anyhow::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut header_end = None;
        while header_end.is_none() {
            let mut chunk = [0u8; 1024];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                anyhow::bail!("unexpected EOF while reading headers");
            }
            buffer.extend_from_slice(&chunk[..read]);
            header_end = buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4);
        }
        let header_end = header_end.expect("header end should exist");
        let header_bytes = &buffer[..header_end];
        let header_text = String::from_utf8_lossy(header_bytes);
        let mut lines = header_text.lines();
        let request_line = lines.next().expect("request line should exist");
        let path = request_line
            .split_whitespace()
            .nth(1)
            .expect("path should exist")
            .to_string();
        let mut content_length = 0usize;
        let mut content_type = None;
        for line in lines {
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            if let Some(length) = lower.strip_prefix("content-length:") {
                content_length = length.trim().parse().expect("content-length should parse");
            } else if let Some(value) = trimmed
                .split_once(':')
                .filter(|(name, _)| name.trim().eq_ignore_ascii_case("content-type"))
            {
                content_type = Some(value.1.trim().to_string());
            }
        }
        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let mut chunk = vec![0u8; content_length - body.len()];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                anyhow::bail!("unexpected EOF while reading body");
            }
            body.extend_from_slice(&chunk[..read]);
        }
        Ok(RecordedRequest {
            path,
            body,
            content_type,
        })
    }

    #[test]
    fn select_configured_account_returns_single_configured_account() {
        let accounts = vec![
            Account {
                account_id: "a".to_string(),
                enabled: true,
                configured: true,
                base_url: "https://example.com".to_string(),
            },
            Account {
                account_id: "b".to_string(),
                enabled: true,
                configured: false,
                base_url: "https://example.com".to_string(),
            },
        ];

        let selected = select_configured_account(&accounts)
            .expect("selection should succeed")
            .expect("account");
        assert_eq!(selected.account_id, "a");
    }

    #[test]
    fn select_configured_account_rejects_multiple_configured_accounts() {
        let accounts = vec![
            Account {
                account_id: "a".to_string(),
                enabled: true,
                configured: true,
                base_url: "https://example.com".to_string(),
            },
            Account {
                account_id: "b".to_string(),
                enabled: true,
                configured: true,
                base_url: "https://example.com".to_string(),
            },
        ];

        let error = select_configured_account(&accounts).expect_err("should reject ambiguity");
        assert!(error
            .to_string()
            .contains("multiple configured WeChat accounts found"));
    }

    #[test]
    fn advance_pull_cursor_keeps_existing_cursor_when_gateway_returns_none() {
        let next = advance_pull_cursor(Some("4".to_string()), None);
        assert_eq!(next, Some("4".to_string()));
    }

    #[test]
    fn advance_pull_cursor_uses_new_cursor_when_gateway_returns_one() {
        let next = advance_pull_cursor(Some("4".to_string()), Some("5".to_string()));
        assert_eq!(next, Some("5".to_string()));
    }

    #[test]
    fn normalize_wechat_text_removes_markdown_style_noise() {
        let text = "# Heading\n\n**Bold**\n- First item\n```bash\nls\n```";
        let normalized = normalize_wechat_text(text);
        assert!(!normalized.contains('#'));
        assert!(!normalized.contains("**"));
        assert!(!normalized.contains("```"));
        assert!(normalized.contains("Heading"));
        assert!(normalized.contains("Bold"));
        assert!(normalized.contains("First item"));
        assert!(normalized.contains("ls"));
    }

    #[test]
    fn split_wechat_bubbles_prefers_short_messages() {
        let bubbles = split_wechat_bubbles(
            "Hmm? What happened? Let me check. There might be two issues. One is config. One is the state directory.",
        );
        assert!(bubbles.len() >= 2);
        assert!(bubbles.iter().all(|bubble| !bubble.trim().is_empty()));
    }

    #[test]
    fn split_wechat_bubbles_does_not_leave_a_colon_at_the_end_of_a_bubble() {
        let bubbles = split_wechat_bubbles(
            "可以呀，我能帮你查资料。比如： 扫描文件夹找特定文件、读取代码、整理搜索结果。",
        );
        assert!(bubbles.iter().all(|bubble| !ends_with_connector(bubble)));
    }
}
