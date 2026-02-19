use anyhow::Result;
use async_trait::async_trait;

use super::{TtsOutput, TtsProvider};
use crate::config::TtsConfig;

pub struct MinimaxTtsProvider {
    api_key: String,
    model: String,
    voice_id: String,
    client: reqwest::Client,
}

impl MinimaxTtsProvider {
    pub fn new(config: &TtsConfig) -> Result<Self> {
        Ok(Self {
            api_key: config
                .api_key
                .clone()
                .ok_or_else(|| anyhow::anyhow!("TTS api_key is required"))?,
            model: config.model.clone(),
            voice_id: config.voice_id.clone().unwrap_or_default(),
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl TtsProvider for MinimaxTtsProvider {
    async fn synthesize(&self, text: &str) -> Result<TtsOutput> {
        let mut voice_setting = serde_json::json!({
            "speed": 1,
            "vol": 1,
            "pitch": 0
        });
        if !self.voice_id.is_empty() {
            voice_setting["voice_id"] = serde_json::json!(self.voice_id);
        }

        let body = serde_json::json!({
            "model": self.model,
            "text": text,
            "stream": false,
            "output_format": "hex",
            "voice_setting": voice_setting,
            "audio_setting": {
                "sample_rate": 32000,
                "bitrate": 128000,
                "format": "mp3",
                "channel": 1
            }
        });

        tracing::info!("TTS request: text_len={}, model={}", text.len(), self.model);

        let resp = self
            .client
            .post("https://api.minimaxi.com/v1/t2a_v2")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("MiniMax TTS API failed ({status}): {err}");
        }

        let data: serde_json::Value = resp.json().await?;

        if let Some(status_code) = data
            .get("base_resp")
            .and_then(|r| r.get("status_code"))
            .and_then(|c| c.as_i64())
        {
            if status_code != 0 {
                let status_msg = data
                    .get("base_resp")
                    .and_then(|r| r.get("status_msg"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown");
                anyhow::bail!("MiniMax TTS error ({status_code}): {status_msg}");
            }
        }

        let hex_audio = data
            .get("data")
            .and_then(|d| d.get("audio"))
            .and_then(|a| a.as_str())
            .filter(|h| !h.is_empty())
            .ok_or_else(|| {
                let preview = serde_json::to_string(&data)
                    .unwrap_or_default()
                    .chars()
                    .take(500)
                    .collect::<String>();
                anyhow::anyhow!("MiniMax TTS: unexpected response format: {preview}")
            })?;

        let audio_bytes = hex::decode(hex_audio)?;
        tracing::info!("TTS audio decoded: {} bytes", audio_bytes.len());

        Ok(TtsOutput { audio_bytes })
    }
}
