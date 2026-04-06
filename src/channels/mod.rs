use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::ContentPart;
use crate::stt::SttProvider;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

pub mod cli;
pub mod telegram;
pub mod wechat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMessageOrigin {
    Human,
    RuntimePtcResult,
    RuntimePtcNotice,
}

#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub sender: String,
    pub session_id: String,
    pub content: String,
    pub content_parts: Option<Vec<ContentPart>>,
    pub channel: String,
    pub origin: ChannelMessageOrigin,
    pub related_job_id: Option<String>,
}

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    fn prompt_supplement(&self) -> Option<String> {
        None
    }
    async fn send(&self, message: &str, recipient: &str) -> Result<()>;
    async fn send_typing(&self, _recipient: &str) -> Result<()> {
        Ok(())
    }
    async fn send_stream_start(&self, _recipient: &str, _text: &str) -> Result<Option<String>> {
        Ok(None)
    }
    async fn send_stream_update(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<()> {
        Ok(())
    }
    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<()>;
}

type ChannelFactory =
    fn(&crate::config::ChannelConfig, Option<Arc<dyn SttProvider>>) -> Result<Arc<dyn Channel>>;

const REGISTRY: &[(&str, ChannelFactory)] = &[
    ("telegram", |config, stt| {
        let tg = telegram::TelegramChannel::from_config(&config.params, stt)?;
        Ok(Arc::new(tg))
    }),
    ("wechat", |config, stt| {
        let wechat = wechat::WechatChannel::from_config(&config.params, stt)?;
        Ok(Arc::new(wechat))
    }),
];

pub fn create_channel(
    config: &crate::config::ChannelConfig,
    stt_provider: Option<Arc<dyn SttProvider>>,
) -> Result<Arc<dyn Channel>> {
    let factory = REGISTRY
        .iter()
        .find(|(name, _)| *name == config.name)
        .map(|(_, f)| f)
        .ok_or_else(|| anyhow::anyhow!("Unknown channel type: {}", config.name))?;
    factory(config, stt_provider)
}

pub fn create_channel_registry(
    app_config: &crate::config::Config,
    stt_provider: Option<Arc<dyn SttProvider>>,
) -> HashMap<String, Arc<dyn Channel>> {
    let mut registry = HashMap::new();
    for ch_cfg in &app_config.channels {
        match create_channel(ch_cfg, stt_provider.clone()) {
            Ok(ch) => {
                tracing::info!("Registered channel: {}", ch.name());
                registry.insert(ch_cfg.name.clone(), ch);
            }
            Err(e) => tracing::warn!("Failed to create channel '{}': {e}", ch_cfg.name),
        }
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::REGISTRY;

    #[test]
    fn registry_lists_supported_remote_channels() {
        let names = REGISTRY.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        assert_eq!(names, vec!["telegram", "wechat"]);
    }
}
