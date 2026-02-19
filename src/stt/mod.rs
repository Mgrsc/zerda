use anyhow::Result;
use async_trait::async_trait;

pub mod groq;

#[async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(&self, audio: &[u8], file_name: &str) -> Result<String>;
}

type SttFactory = fn(&crate::config::SttConfig) -> Result<Box<dyn SttProvider>>;

const REGISTRY: &[(&str, SttFactory)] =
    &[("groq", |c| Ok(Box::new(groq::GroqSttProvider::new(c)?)))];

pub fn create_stt_provider(config: &crate::config::SttConfig) -> Result<Box<dyn SttProvider>> {
    let factory = REGISTRY
        .iter()
        .find(|(name, _)| *name == config.provider)
        .map(|(_, f)| f)
        .ok_or_else(|| anyhow::anyhow!("Unknown STT provider: {}", config.provider))?;
    factory(config)
}
