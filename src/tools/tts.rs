use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use crate::tts::TtsProvider;

use super::{Tool, ToolResult};

pub struct TtsTool {
    provider: Arc<dyn TtsProvider>,
}

const VOICE_TTL_SECS: u64 = 600;

impl TtsTool {
    pub fn new(provider: Arc<dyn TtsProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Tool for TtsTool {
    fn name(&self) -> &str {
        "tts"
    }

    fn description(&self) -> &str {
        "Convert text to speech audio. Returns a voice marker that the system will automatically deliver as a voice message. You MUST include the returned marker exactly as-is in your response text. NEVER fabricate or guess voice/image markers yourself."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to convert to speech"
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'text' parameter"))?;

        if text.is_empty() {
            return Ok(ToolResult {
                output: "Error: text is empty".to_string(),
                is_error: true,
            });
        }

        let output = match self.provider.synthesize(text).await {
            Ok(o) => o,
            Err(e) => {
                return Ok(ToolResult {
                    output: format!("TTS failed: {e}"),
                    is_error: true,
                });
            }
        };

        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let ext = "mp3";
        let raw_path = format!("/tmp/zerda_tts_{id}.{ext}");
        let mut raw_guard = TempPathGuard::new(raw_path.clone());
        tokio::fs::write(&raw_path, &output.audio_bytes).await?;

        let ogg_path = format!("/tmp/zerda_tts_{id}.ogg");
        let output_path = match tokio::process::Command::new("ffmpeg")
            .args([
                "-i", &raw_path, "-c:a", "libopus", "-b:a", "64k", &ogg_path, "-y",
            ])
            .output()
            .await
        {
            Ok(out) if out.status.success() => {
                raw_guard.disarm();
                if let Err(e) = tokio::fs::remove_file(&raw_path).await {
                    tracing::debug!("Failed to remove raw TTS file {raw_path}: {e}");
                }
                tracing::info!("TTS ffmpeg ok: {ogg_path}");
                ogg_path
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!("TTS ffmpeg failed, using {ext}: {stderr}");
                raw_guard.disarm();
                raw_path
            }
            Err(e) => {
                tracing::warn!("TTS ffmpeg not available, using {ext}: {e}");
                raw_guard.disarm();
                raw_path
            }
        };

        schedule_cleanup(output_path.clone(), Duration::from_secs(VOICE_TTL_SECS));

        tracing::info!("TTS done: {output_path}");
        Ok(ToolResult {
            output: format!("<voice>{output_path}</voice>"),
            is_error: false,
        })
    }
}

struct TempPathGuard {
    path: Option<String>,
}

impl TempPathGuard {
    fn new(path: String) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::debug!("Failed to remove guarded temp file {path}: {e}");
            }
        }
    }
}

fn schedule_cleanup(path: String, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::debug!("Failed to remove expired voice file {path}: {e}");
        }
    });
}
