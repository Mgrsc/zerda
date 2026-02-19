use anyhow::Result;
use async_trait::async_trait;

use super::SttProvider;
use crate::config::SttConfig;

pub struct GroqSttProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GroqSttProvider {
    pub fn new(config: &SttConfig) -> Result<Self> {
        Ok(Self {
            api_key: config
                .api_key
                .clone()
                .ok_or_else(|| anyhow::anyhow!("STT api_key is required"))?,
            model: config.model.clone(),
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl SttProvider for GroqSttProvider {
    async fn transcribe(&self, audio: &[u8], file_name: &str) -> Result<String> {
        let mime = match file_name
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "ogg" | "oga" => "audio/ogg",
            "mp3" => "audio/mpeg",
            "m4a" | "mp4" => "audio/mp4",
            "wav" => "audio/wav",
            "flac" => "audio/flac",
            "webm" => "audio/webm",
            _ => "application/octet-stream",
        };

        let part = reqwest::multipart::Part::bytes(audio.to_vec())
            .file_name(file_name.to_string())
            .mime_str(mime)?;
        let form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("response_format", "text")
            .part("file", part);

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Groq STT failed: {err}");
        }

        let text = resp.text().await?.trim().to_string();
        Ok(text)
    }
}
