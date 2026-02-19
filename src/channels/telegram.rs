use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use base64::Engine as _;
use tokio::sync::mpsc;

use super::{Channel, ChannelMessage};
use crate::providers::ContentPart;
use crate::rich_content::{self, RichSegment};
use crate::stt::SttProvider;

fn split_message(message: &str, max_len: usize) -> Vec<String> {
    let max_len = max_len.max(1);
    if message.chars().count() <= max_len {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        if remaining.chars().count() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let mut split_byte = remaining.len();
        let mut char_count = 0usize;
        for (idx, ch) in remaining.char_indices() {
            char_count += 1;
            if char_count >= max_len {
                split_byte = idx + ch.len_utf8();
                break;
            }
        }

        let window = &remaining[..split_byte];
        let split_at = window
            .rfind('\n')
            .map(|p| p + 1)
            .or_else(|| window.rfind(' ').map(|p| p + 1))
            .unwrap_or(split_byte);

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
    }

    chunks
}

#[derive(Clone)]
pub struct TelegramChannel {
    token: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
    stt_provider: Option<Arc<dyn SttProvider>>,
    max_message_length: usize,
    polling_timeout: u64,
    split_delay_ms: u64,
}

impl TelegramChannel {
    pub fn from_config(
        params: &serde_json::Value,
        stt_provider: Option<Arc<dyn SttProvider>>,
    ) -> anyhow::Result<Self> {
        let token = params
            .get("token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Telegram channel missing 'token'"))?
            .to_string();
        let allowed_users: Vec<String> = params
            .get("allowed_users")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let max_message_length = params
            .get("max_message_length")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(4096)
            .max(1);
        let polling_timeout = params
            .get("polling_timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);
        let split_delay_ms = params
            .get("split_delay_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(100);
        Ok(Self {
            token,
            allowed_users,
            client: reqwest::Client::new(),
            stt_provider,
            max_message_length,
            polling_timeout,
            split_delay_ms,
        })
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.token)
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.is_empty() || self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<()> {
        let chunks = split_message(text, self.max_message_length);
        for (i, chunk) in chunks.iter().enumerate() {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "Markdown"
            });

            let resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err = resp.text().await.unwrap_or_default();

                let plain_body = serde_json::json!({
                    "chat_id": chat_id,
                    "text": chunk,
                });
                let plain_resp = self
                    .client
                    .post(self.api_url("sendMessage"))
                    .json(&plain_body)
                    .send()
                    .await?;

                if !plain_resp.status().is_success() {
                    let plain_err = plain_resp.text().await.unwrap_or_default();
                    anyhow::bail!(
                        "Telegram sendMessage failed (markdown {status}: {err}; plain: {plain_err})"
                    );
                }
            }

            if i < chunks.len() - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(self.split_delay_ms)).await;
            }
        }
        Ok(())
    }

    async fn send_text_msg(&self, chat_id: &str, text: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        });

        let resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let plain_body = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            });
            let plain_resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&plain_body)
                .send()
                .await?;
            let plain_status = plain_resp.status();
            let plain_raw = plain_resp.text().await?;
            if !plain_status.is_success() {
                anyhow::bail!("Telegram sendMessage fallback failed ({plain_status}): {plain_raw}");
            }
            let data: serde_json::Value = serde_json::from_str(&plain_raw)?;
            return Ok(data);
        }

        let data: serde_json::Value = resp.json().await?;
        Ok(data)
    }

    async fn send_photo(&self, chat_id: &str, url_or_path: &str) -> Result<()> {
        if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "photo": url_or_path
            });
            let resp = self
                .client
                .post(self.api_url("sendPhoto"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let err = resp.text().await.unwrap_or_default();
                anyhow::bail!("Telegram sendPhoto failed: {err}");
            }
        } else {
            let file_bytes = tokio::fs::read(url_or_path).await?;
            let file_name = std::path::Path::new(url_or_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("photo.jpg")
                .to_string();
            let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name);
            let form = reqwest::multipart::Form::new()
                .text("chat_id", chat_id.to_string())
                .part("photo", part);
            let resp = self
                .client
                .post(self.api_url("sendPhoto"))
                .multipart(form)
                .send()
                .await?;
            if !resp.status().is_success() {
                let err = resp.text().await.unwrap_or_default();
                anyhow::bail!("Telegram sendPhoto (upload) failed: {err}");
            }
        }
        Ok(())
    }

    async fn send_voice(&self, chat_id: &str, path: &str) -> Result<()> {
        let file_bytes = tokio::fs::read(path).await?;
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("voice.ogg")
            .to_string();
        let part = reqwest::multipart::Part::bytes(file_bytes.clone()).file_name(file_name.clone());
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);
        let resp = self
            .client
            .post(self.api_url("sendVoice"))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            tracing::debug!("sendVoice failed, trying sendAudio");
            let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name);
            let form = reqwest::multipart::Form::new()
                .text("chat_id", chat_id.to_string())
                .part("audio", part);
            let resp = self
                .client
                .post(self.api_url("sendAudio"))
                .multipart(form)
                .send()
                .await?;
            if !resp.status().is_success() {
                let err = resp.text().await.unwrap_or_default();
                anyhow::bail!("Telegram sendAudio failed: {err}");
            }
        }
        if let Err(e) = tokio::fs::remove_file(path).await {
            tracing::debug!("Failed to remove sent voice file {path}: {e}");
        }
        Ok(())
    }

    async fn download_telegram_file(&self, file_id: &str) -> Result<Vec<u8>> {
        let body = serde_json::json!({ "file_id": file_id });
        let resp = self
            .client
            .post(self.api_url("getFile"))
            .json(&body)
            .send()
            .await?;
        let data: serde_json::Value = resp.json().await?;
        let file_path = data
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing file_path in getFile response"))?;
        let download_url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.token, file_path
        );
        let bytes = self.client.get(&download_url).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub async fn register_commands(&self, commands: &[crate::commands::CommandInfo]) {
        let cmds: Vec<serde_json::Value> = commands
            .iter()
            .map(|c| serde_json::json!({ "command": c.name, "description": c.description }))
            .collect();
        let body = serde_json::json!({ "commands": cmds });
        match self
            .client
            .post(self.api_url("setMyCommands"))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Telegram bot commands registered");
            }
            Ok(resp) => {
                let err = resp.text().await.unwrap_or_default();
                tracing::warn!("Failed to register Telegram commands: {err}");
            }
            Err(e) => {
                tracing::warn!("Failed to register Telegram commands: {e}");
            }
        }
    }

    async fn transcribe_audio(&self, audio_bytes: Vec<u8>, file_name: &str) -> Result<String> {
        let stt = self
            .stt_provider
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("STT not configured"))?;
        stt.transcribe(&audio_bytes, file_name).await
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn prompt_supplement(&self) -> Option<String> {
        Some(
            "You are responding via Telegram. Rich content markers:\n\
             - <image>URL</image> — Send an image by URL\n\
             - <voice>PATH</voice> — Send a voice message from a file path\n\
             Formatting rules for Telegram:\n\
             1. Telegram Markdown rendering is limited. Do NOT use Markdown tables.\n\
             2. For tabular or aligned content, use fenced code blocks instead.\n\
             3. For simple comparisons, prefer concise bullet lists over wide layouts.\n\
             4. Keep formatting robust under message splitting; avoid fragile nested Markdown.\n\
             CRITICAL RULES:\n\
             1. NEVER fabricate or guess these markers. Only use paths/URLs returned by tools.\n\
             2. When the tts tool returns a marker like <voice>/tmp/zerda_tts_xxx.ogg</voice>, include it EXACTLY as-is in your response.\n\
             3. Never output these markers as examples, in explanations, or when a tool has failed.\n\
             4. Place each marker on its own line.\n\
             5. If you receive a system note about voice messages being unsupported (STT not configured), kindly inform the user that voice message recognition is currently unavailable."
                .to_string(),
        )
    }

    async fn send_typing(&self, recipient: &str) -> Result<()> {
        let body = serde_json::json!({
            "chat_id": recipient,
            "action": "typing"
        });
        self.client
            .post(self.api_url("sendChatAction"))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    async fn send(&self, message: &str, recipient: &str) -> Result<()> {
        if !rich_content::has_rich_markers(message) {
            return self.send_text(recipient, message).await;
        }

        for segment in rich_content::extract_rich_segments(message) {
            match segment {
                RichSegment::Text(text) => self.send_text(recipient, &text).await?,
                RichSegment::Image(url) => {
                    if let Err(e) = self.send_photo(recipient, &url).await {
                        tracing::warn!("Failed to send photo: {e}");
                        self.send_text(recipient, &format!("[image: {url}]"))
                            .await?;
                    }
                }
                RichSegment::Voice(path) => {
                    if let Err(e) = self.send_voice(recipient, &path).await {
                        tracing::warn!("Failed to send voice: {e}");
                        self.send_text(recipient, &format!("[voice: {path}]"))
                            .await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_stream_start(&self, recipient: &str, text: &str) -> Result<Option<String>> {
        let data = self.send_text_msg(recipient, text).await?;
        let message_id = data
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .map(|id| id.to_string());
        Ok(message_id)
    }

    async fn send_stream_update(
        &self,
        recipient: &str,
        message_id: &str,
        text: &str,
    ) -> Result<()> {
        let is_intermediate = text.ends_with('▌');

        let body = if is_intermediate {
            serde_json::json!({
                "chat_id": recipient,
                "message_id": message_id,
                "text": text,
            })
        } else {
            serde_json::json!({
                "chat_id": recipient,
                "message_id": message_id,
                "text": text,
                "parse_mode": "Markdown"
            })
        };

        let resp = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            if is_intermediate {
                anyhow::bail!("editMessageText failed: {err_text}");
            }
            tracing::debug!("editMessageText with Markdown failed: {err_text}");
            let plain_body = serde_json::json!({
                "chat_id": recipient,
                "message_id": message_id,
                "text": text,
            });
            let retry_resp = self
                .client
                .post(self.api_url("editMessageText"))
                .json(&plain_body)
                .send()
                .await?;
            if !retry_resp.status().is_success() {
                let plain_err = retry_resp.text().await.unwrap_or_default();
                anyhow::bail!("editMessageText failed (markdown: {err_text}; plain: {plain_err})");
            }
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()> {
        self.register_commands(crate::commands::command_infos())
            .await;

        let mut offset: i64 = 0;

        tracing::info!("Telegram channel listening for messages...");

        loop {
            let url = self.api_url("getUpdates");
            let body = serde_json::json!({
                "offset": offset,
                "timeout": self.polling_timeout,
                "allowed_updates": ["message"]
            });

            let resp = match self.client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Telegram poll error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Telegram parse error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let Some(results) = data.get("result").and_then(serde_json::Value::as_array) else {
                continue;
            };

            for update in results {
                if let Some(uid) = update.get("update_id").and_then(serde_json::Value::as_i64) {
                    offset = uid + 1;
                }

                let Some(message) = update.get("message") else {
                    continue;
                };

                let user_id = message
                    .get("from")
                    .and_then(|f| f.get("id"))
                    .and_then(serde_json::Value::as_i64)
                    .map_or_else(|| "unknown".to_string(), |id| id.to_string());

                if !self.is_user_allowed(&user_id) {
                    tracing::warn!("Telegram: ignoring message from unauthorized user: {user_id}");
                    continue;
                }

                let chat_id = message
                    .get("chat")
                    .and_then(|c| c.get("id"))
                    .and_then(serde_json::Value::as_i64)
                    .map(|id| id.to_string());

                let Some(chat_id) = chat_id else {
                    tracing::warn!("Telegram: missing chat_id in message, skipping");
                    continue;
                };

                let (content, content_parts) = if let Some(voice) =
                    message.get("voice").or_else(|| message.get("audio"))
                {
                    let file_id = voice.get("file_id").and_then(serde_json::Value::as_str);
                    let Some(file_id) = file_id else {
                        tracing::warn!("Telegram: voice message missing file_id");
                        continue;
                    };
                    if self.stt_provider.is_none() {
                        ("[system: The user sent a voice message, but speech-to-text (STT) is not configured, so the audio cannot be transcribed. Please inform the user that voice message recognition is currently unavailable.]".to_string(), None)
                    } else {
                        let file_name = voice
                            .get("file_name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(if message.get("voice").is_some() {
                                "voice.ogg"
                            } else {
                                "audio.mp3"
                            });
                        match self.download_telegram_file(file_id).await {
                            Ok(audio_bytes) => {
                                match self.transcribe_audio(audio_bytes, file_name).await {
                                    Ok(text) if !text.is_empty() => {
                                        tracing::debug!(
                                            "STT transcribed, chars={}",
                                            text.chars().count()
                                        );
                                        (format!("[voice_message]: {text}"), None)
                                    }
                                    Ok(_) => {
                                        tracing::warn!("STT returned empty transcription");
                                        continue;
                                    }
                                    Err(e) => {
                                        tracing::error!("STT transcription failed: {e}");
                                        continue;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to download voice file: {e}");
                                continue;
                            }
                        }
                    }
                } else if message
                    .get("photo")
                    .and_then(serde_json::Value::as_array)
                    .is_some()
                {
                    let Some(file_id) = message
                        .get("photo")
                        .and_then(serde_json::Value::as_array)
                        .and_then(|arr| arr.last())
                        .and_then(|v| v.get("file_id"))
                        .and_then(serde_json::Value::as_str)
                    else {
                        tracing::warn!("Telegram: photo message missing file_id");
                        continue;
                    };
                    let image_bytes = match self.download_telegram_file(file_id).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            tracing::error!("Failed to download image file: {e}");
                            continue;
                        }
                    };
                    let mut parts = Vec::new();
                    let caption = message
                        .get("caption")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if !caption.is_empty() {
                        parts.push(ContentPart::Text(caption.to_string()));
                    }
                    parts.push(ContentPart::ImageBase64 {
                        media_type: "image/jpeg".to_string(),
                        data: base64::engine::general_purpose::STANDARD.encode(image_bytes),
                    });
                    ("[image_message]".to_string(), Some(parts))
                } else if let Some(text) = message.get("text").and_then(serde_json::Value::as_str) {
                    (text.to_string(), None)
                } else {
                    continue;
                };

                if let Err(e) = tx
                    .send(ChannelMessage {
                        sender: chat_id.clone(),
                        session_id: format!("{chat_id}:{user_id}"),
                        content,
                        content_parts,
                        channel: "telegram".to_string(),
                    })
                    .await
                {
                    return Err(anyhow::anyhow!("Telegram receiver channel closed: {e}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_message;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn split_message_preserves_content_and_boundaries(input in proptest::collection::vec(any::<char>(), 0..300).prop_map(|v| v.into_iter().collect::<String>()), max_len in 1usize..80) {
            let chunks = split_message(&input, max_len);
            prop_assert!(!chunks.is_empty());
            prop_assert_eq!(chunks.concat(), input);
            for chunk in chunks {
                prop_assert!(chunk.chars().count() <= max_len.max(1));
                prop_assert!(chunk.is_char_boundary(chunk.len()));
            }
        }
    }

    #[test]
    fn split_message_handles_zero_limit() {
        let input = "你好 world 😀";
        let chunks = split_message(input, 0);
        assert!(!chunks.is_empty());
        assert_eq!(chunks.concat(), input);
    }
}
