use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use base64::Engine as _;
use tokio::sync::mpsc;

use super::{Channel, ChannelMessage};
use crate::logging::{summarize_text, text_fingerprint};
use crate::providers::ContentPart;
use crate::rich_content::{self, RichSegment};
use crate::stt::SttProvider;

const POLLING_TIMEOUT: u64 = 30;
const SPLIT_DELAY_MS: u64 = 100;

fn split_message(message: &str, max_len: usize) -> Vec<String> {
    let max_len = max_len.max(1);
    if message.is_empty() {
        return vec![message.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        let mut split_byte = remaining.len();
        let mut found_limit = false;
        let mut char_count = 0usize;
        for (idx, ch) in remaining.char_indices() {
            char_count += 1;
            if char_count == max_len {
                split_byte = idx + ch.len_utf8();
                found_limit = true;
                break;
            }
        }

        if !found_limit {
            chunks.push(remaining.to_string());
            break;
        }

        let window = &remaining[..split_byte];
        let split_at = find_markdown_safe_split(window).unwrap_or(split_byte);

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
    }

    chunks
}

fn find_markdown_safe_split(window: &str) -> Option<usize> {
    let mut last_split = None;
    let mut escaped = false;
    let mut in_link_title = false;
    let mut pending_link_url = false;
    let mut link_url_depth = 0usize;
    let mut in_code_block = false;
    let mut backtick_run = 0usize;

    for (idx, ch) in window.char_indices() {
        if ch == '`' {
            backtick_run += 1;
            if backtick_run == 3 {
                in_code_block = !in_code_block;
                backtick_run = 0;
            }
            continue;
        } else {
            backtick_run = 0;
        }

        if in_code_block {
            continue;
        }

        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }

        if link_url_depth > 0 {
            if ch == '(' {
                link_url_depth += 1;
            } else if ch == ')' {
                link_url_depth -= 1;
            }
        } else if in_link_title {
            if ch == ']' {
                in_link_title = false;
                pending_link_url = true;
            }
        } else {
            if pending_link_url {
                if ch == '(' {
                    link_url_depth = 1;
                    pending_link_url = false;
                    continue;
                }
                pending_link_url = false;
            }
            if ch == '[' {
                in_link_title = true;
                continue;
            }
            if ch == '\n' || ch == ' ' {
                last_split = Some(idx + ch.len_utf8());
            }
        }
    }

    last_split.filter(|&p| p > 0)
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.len() > 2
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|')
        && t.ends_with('|')
        && t.len() > 2
        && t.chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

fn wrap_tables(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut i = 0;
    while i < lines.len() {
        if i + 1 < lines.len() && is_table_row(lines[i]) && is_table_separator(lines[i + 1]) {
            out.push_str("```\n");
            while i < lines.len() && is_table_row(lines[i]) {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }
            out.push_str("```\n");
        } else {
            out.push_str(lines[i]);
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;
        }
    }
    out
}

fn normalize_telegram_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(after_open) = rest.strip_prefix("```") {
            if let Some(close_offset) = find_closing_code_fence(after_open) {
                out.push_str(&rest[..3 + close_offset + 3]);
                i += 3 + close_offset + 3;
                continue;
            }
        }
        if let Some(after_tick) = rest.strip_prefix('`') {
            if let Some(close_offset) = after_tick.find('`') {
                out.push_str(&rest[..2 + close_offset]);
                i += 2 + close_offset;
                continue;
            }
        }
        if rest.starts_with("**") {
            out.push('*');
            i += 2;
            continue;
        }
        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn escape_markdown_v2_char(ch: char, out: &mut String) {
    if matches!(
        ch,
        '\\'
            | '_'
            | '*'
            | '['
            | ']'
            | '('
            | ')'
            | '~'
            | '`'
            | '>'
            | '#'
            | '+'
            | '-'
            | '='
            | '|'
            | '{'
            | '}'
            | '.'
            | '!'
    ) {
        out.push('\\');
    }
    out.push(ch);
}

fn escape_markdown_v2_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for ch in text.chars() {
        escape_markdown_v2_char(ch, &mut out);
    }
    out
}

fn parse_markdown_link(text: &str) -> Option<(usize, &str, &str)> {
    if !text.starts_with('[') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut i = 1usize;
    let mut escaped = false;
    let mut title_end = None;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b']' {
            title_end = Some(i);
            break;
        }
        i += 1;
    }
    let title_end = title_end?;
    if title_end + 1 >= bytes.len() || bytes[title_end + 1] != b'(' {
        return None;
    }

    let url_start = title_end + 2;
    let mut depth = 1usize;
    let mut j = url_start;
    escaped = false;
    let mut url_end = None;
    while j < bytes.len() {
        let b = bytes[j];
        if escaped {
            escaped = false;
            j += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            j += 1;
            continue;
        }
        if b == b'(' {
            depth += 1;
            j += 1;
            continue;
        }
        if b == b')' {
            depth -= 1;
            if depth == 0 {
                url_end = Some(j);
                break;
            }
        }
        j += 1;
    }
    let url_end = url_end?;
    let consumed = url_end + 1;
    Some((consumed, &text[1..title_end], &text[url_start..url_end]))
}

fn find_closing_code_fence(s: &str) -> Option<usize> {
    let mut search = 0;
    loop {
        let rel = s[search..].find("```")?;
        let abs = search + rel;
        if abs == 0 || s.as_bytes()[abs - 1] == b'\n' {
            return Some(abs);
        }
        search = abs + 3;
    }
}

fn escape_code_content(content: &str, out: &mut String) {
    for ch in content.chars() {
        if ch == '`' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
}

fn escape_url_content(url: &str, out: &mut String) {
    for ch in url.chars() {
        if ch == ')' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
}

fn render_markdown_v2_safe(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut i = 0usize;
    let mut at_line_start = true;
    let mut prev_char: Option<char> = None;
    while i < text.len() {
        let rest = &text[i..];

        if let Some(after_open) = rest.strip_prefix("```") {
            if let Some(close_offset) = find_closing_code_fence(after_open) {
                let content = &after_open[..close_offset];
                out.push_str("```");
                escape_code_content(content, &mut out);
                out.push_str("```");
                i += 3 + close_offset + 3;
                at_line_start = false;
                prev_char = Some('`');
                continue;
            }
        }

        if let Some(after_tick) = rest.strip_prefix('`') {
            if let Some(close_offset) = after_tick.find('`') {
                let content = &after_tick[..close_offset];
                if !content.is_empty() && !content.contains('\n') {
                    out.push('`');
                    escape_code_content(content, &mut out);
                    out.push('`');
                    i += 1 + close_offset + 1;
                    at_line_start = false;
                    prev_char = Some('`');
                    continue;
                }
            }
        }

        if let Some(after_open) = rest.strip_prefix("||") {
            if let Some(close_offset) = after_open.find("||") {
                let content = &after_open[..close_offset];
                if !content.is_empty() && !content.contains('\n') {
                    out.push_str("||");
                    out.push_str(&escape_markdown_v2_text(content));
                    out.push_str("||");
                    i += 2 + close_offset + 2;
                    at_line_start = false;
                    prev_char = Some('|');
                    continue;
                }
            }
        }

        if let Some(inner) = rest.strip_prefix("**").and_then(|t| {
            t.find("**").map(|end| &t[..end]).filter(|t| !t.is_empty())
        }) {
            out.push('*');
            out.push_str(&escape_markdown_v2_text(inner));
            out.push('*');
            i += inner.len() + 4;
            at_line_start = false;
            prev_char = Some('*');
            continue;
        }

        if let Some(inner) = rest.strip_prefix('*').and_then(|t| {
            t.find('*').map(|end| &t[..end]).filter(|t| {
                !t.is_empty() && !t.starts_with(' ') && !t.ends_with(' ') && !t.contains('\n')
            })
        }) {
            out.push('*');
            out.push_str(&escape_markdown_v2_text(inner));
            out.push('*');
            i += inner.len() + 2;
            at_line_start = false;
            prev_char = Some('*');
            continue;
        }

        let prev_is_word = prev_char.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !prev_is_word && rest.starts_with("__") {
            let after_open = &rest[2..];
            if let Some(close_offset) = after_open.find("__") {
                let content = &after_open[..close_offset];
                if !content.is_empty() && !content.contains('\n') {
                    out.push_str("__");
                    out.push_str(&escape_markdown_v2_text(content));
                    out.push_str("__");
                    i += 2 + close_offset + 2;
                    at_line_start = false;
                    prev_char = Some('_');
                    continue;
                }
            }
        }

        let prev_is_word = prev_char.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !prev_is_word {
            if let Some(inner) = rest.strip_prefix('_').and_then(|t| {
                t.find('_').map(|end| &t[..end]).filter(|t| {
                    !t.is_empty() && !t.starts_with(' ') && !t.ends_with(' ') && !t.contains('\n')
                })
            }) {
                out.push('_');
                out.push_str(&escape_markdown_v2_text(inner));
                out.push('_');
                i += inner.len() + 2;
                at_line_start = false;
                prev_char = Some('_');
                continue;
            }
        }

        let prev_is_tilde = prev_char.is_some_and(|c| c.is_alphanumeric() || c == '~');
        if !prev_is_tilde {
            if let Some(inner) = rest.strip_prefix('~').and_then(|t| {
                t.find('~').map(|end| &t[..end]).filter(|t| {
                    !t.is_empty() && !t.starts_with(' ') && !t.ends_with(' ') && !t.contains('\n')
                })
            }) {
                out.push('~');
                out.push_str(&escape_markdown_v2_text(inner));
                out.push('~');
                i += inner.len() + 2;
                at_line_start = false;
                prev_char = Some('~');
                continue;
            }
        }

        if let Some((consumed, title, url)) = parse_markdown_link(rest) {
            out.push('[');
            out.push_str(&escape_markdown_v2_text(title));
            out.push_str("](");
            escape_url_content(url, &mut out);
            out.push(')');
            i += consumed;
            at_line_start = false;
            prev_char = Some(')');
            continue;
        }

        if let Some(ch) = rest.chars().next() {
            if ch == '\n' {
                out.push('\n');
                at_line_start = true;
            } else if ch == '>' && at_line_start {
                out.push('>');
                at_line_start = false;
            } else {
                escape_markdown_v2_char(ch, &mut out);
                at_line_start = false;
            }
            prev_char = Some(ch);
            i += ch.len_utf8();
            continue;
        }
        break;
    }
    out
}

fn markdown_v2_candidates(text: &str) -> Vec<String> {
    let table_wrapped = wrap_tables(text);
    let normalized = normalize_telegram_markdown(&table_wrapped);
    let rendered = render_markdown_v2_safe(&normalized);
    let mut candidates = Vec::new();
    for candidate in [text.to_string(), table_wrapped, normalized, rendered] {
        if !candidates.iter().any(|c| c == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

#[derive(Clone)]
pub struct TelegramChannel {
    token: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
    stt_provider: Option<Arc<dyn SttProvider>>,
    max_message_length: usize,
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
        Ok(Self {
            token,
            allowed_users,
            client: reqwest::Client::new(),
            stt_provider,
            max_message_length,
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
            let candidates = markdown_v2_candidates(chunk);
            let mut markdown_errors = Vec::new();
            let mut markdown_ok = false;
            for (idx, candidate) in candidates.iter().enumerate() {
                if markdown_ok {
                    break;
                }
                let body = serde_json::json!({
                    "chat_id": chat_id,
                    "text": candidate,
                    "parse_mode": "MarkdownV2"
                });
                let resp = self
                    .client
                    .post(self.api_url("sendMessage"))
                    .json(&body)
                    .send()
                    .await?;
                if resp.status().is_success() {
                    markdown_ok = true;
                    break;
                }
                let status = resp.status();
                let err = resp.text().await.unwrap_or_default();
                tracing::debug!(
                    candidate_idx = idx,
                    candidate = %summarize_text(candidate),
                    status = %status,
                    "sendMessage MarkdownV2 candidate rejected: {err}"
                );
                markdown_errors.push(format!("{status}: {err}"));
            }
            if !markdown_ok {
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
                    let md_err = markdown_errors.join(" | ");
                    anyhow::bail!(
                        "Telegram sendMessage failed (markdown: {md_err}; plain: {plain_err})"
                    );
                }
            }

            if i < chunks.len() - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(SPLIT_DELAY_MS)).await;
            }
        }
        Ok(())
    }

    async fn send_text_msg(&self, chat_id: &str, text: &str) -> Result<serde_json::Value> {
        let candidates = markdown_v2_candidates(text);
        for candidate in &candidates {
            let body = serde_json::json!({
                "chat_id": chat_id,
                "text": candidate,
                "parse_mode": "MarkdownV2"
            });
            let resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await?;
            if resp.status().is_success() {
                let data: serde_json::Value = resp.json().await?;
                return Ok(data);
            }
        }
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
             - <image>URL_OR_ABSOLUTE_PATH</image> — Send an image by URL or absolute file path\n\
             - <voice>PATH</voice> — Send a voice message from a file path\n\
             Response format contract:\n\
             1. Put the conclusion in the first line.\n\
             2. Then use a numbered list with 2-4 key points.\n\
             3. Keep each point short (no more than 2 sentences).\n\
             4. Default length: 180-450 Chinese characters unless the user explicitly asks for detail.\n\
             5. If details are long, send a compact summary first, then ask whether to continue.\n\
             Formatting rules for Telegram MarkdownV2:\n\
             1. Format text for Telegram MarkdownV2. Do NOT use Markdown tables.\n\
             2. Use Telegram-compatible bold as *bold* (single asterisks), not **bold**.\n\
             3. Hyperlinks are allowed as [title](https://example.com), but bare URLs are preferred for reliability.\n\
             4. Escape MarkdownV2 special characters when needed to avoid rendering errors.\n\
             5. For tabular or aligned content, use fenced code blocks instead.\n\
             6. Avoid fragile nested Markdown; keep formatting robust under message splitting.\n\
             7. If MarkdownV2 rendering fails after splitting, plain text fallback is acceptable.\n\
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

        if is_intermediate {
            let body = serde_json::json!({
                "chat_id": recipient,
                "message_id": message_id,
                "text": text,
            });
            let resp = self
                .client
                .post(self.api_url("editMessageText"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                if err_text.contains("message is not modified") {
                    return Ok(());
                }
                anyhow::bail!("editMessageText failed: {err_text}");
            }
            return Ok(());
        }

        let candidates = markdown_v2_candidates(text);
        let mut markdown_errors = Vec::new();
        for (idx, candidate) in candidates.iter().enumerate() {
            let body = serde_json::json!({
                "chat_id": recipient,
                "message_id": message_id,
                "text": candidate,
                "parse_mode": "MarkdownV2"
            });
            let resp = self
                .client
                .post(self.api_url("editMessageText"))
                .json(&body)
                .send()
                .await?;
            if resp.status().is_success() {
                return Ok(());
            }
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            if err_text.contains("message is not modified") {
                return Ok(());
            }
            tracing::debug!(
                candidate_idx = idx,
                candidate_chars = candidate.chars().count(),
                candidate_fp = %text_fingerprint(candidate),
                status = %status,
                "editMessageText MarkdownV2 candidate rejected: {err_text}"
            );
            markdown_errors.push(format!("{status}: {err_text}"));
        }

        tracing::debug!(
            "editMessageText with MarkdownV2 failed: {}",
            markdown_errors.join(" | ")
        );
        let plain_body = serde_json::json!({
            "chat_id": recipient,
            "message_id": message_id,
            "text": text,
        });
        let plain_resp = self
            .client
            .post(self.api_url("editMessageText"))
            .json(&plain_body)
            .send()
            .await?;
        if !plain_resp.status().is_success() {
            let plain_err = plain_resp.text().await.unwrap_or_default();
            if plain_err.contains("message is not modified") {
                return Ok(());
            }
            anyhow::bail!(
                "editMessageText failed (markdown: {}; plain: {plain_err})",
                markdown_errors.join(" | ")
            );
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
                "timeout": POLLING_TIMEOUT,
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
