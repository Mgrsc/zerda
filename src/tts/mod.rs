use anyhow::Result;
use async_trait::async_trait;

pub mod minimax;

pub struct TtsOutput {
    pub audio_bytes: Vec<u8>,
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn synthesize(&self, text: &str) -> Result<TtsOutput>;
}

type TtsFactory = fn(&crate::config::TtsConfig) -> Result<Box<dyn TtsProvider>>;

const REGISTRY: &[(&str, TtsFactory)] = &[("minimax", |c| {
    Ok(Box::new(minimax::MinimaxTtsProvider::new(c)?))
})];

pub fn create_tts_provider(config: &crate::config::TtsConfig) -> Result<Box<dyn TtsProvider>> {
    let factory = REGISTRY
        .iter()
        .find(|(name, _)| *name == config.provider)
        .map(|(_, f)| f)
        .ok_or_else(|| anyhow::anyhow!("Unknown TTS provider: {}", config.provider))?;
    factory(config)
}
