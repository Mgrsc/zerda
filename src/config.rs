use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::Mutex;

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

pub const STREAM_OVERFLOW_CHARS: usize = 4000;
pub const MAX_PROMPT_CHARS: usize = 32000;
pub const MEMORY_DIR: &str = "~/.zerda";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(deserialize_with = "deserialize_providers")]
    pub providers: Vec<ProviderEndpoint>,
    pub agent: AgentConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    #[serde(default)]
    pub stt: SttConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            base_delay_ms: default_base_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
            connect_timeout_secs: default_connect_timeout_secs(),
            request_timeout_secs: default_request_timeout_secs(),
        }
    }
}

const fn default_max_retries() -> u32 {
    3
}
const fn default_base_delay_ms() -> u64 {
    2000
}
const fn default_max_delay_ms() -> u64 {
    30000
}
const fn default_connect_timeout_secs() -> u64 {
    10
}
const fn default_request_timeout_secs() -> u64 {
    120
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderEndpoint {
    #[serde(skip)]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub extra_headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub retry: RetryConfig,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    #[serde(default = "default_true")]
    pub vision: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub primary_model: ModelConfig,
    #[serde(default)]
    pub fast_model: Option<ModelConfig>,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    #[serde(default = "default_identity_path")]
    pub identity_path: String,
    #[serde(default = "default_session_cleanup_days")]
    pub session_cleanup_days: u64,
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout: u64,
    #[serde(default)]
    pub primitive_timeout: Option<u64>,
    #[serde(default)]
    pub disabled_primitives: Vec<String>,
}

impl AgentConfig {
    pub fn effective_primitive_timeout(&self) -> u64 {
        self.primitive_timeout.unwrap_or(self.tool_timeout)
    }
}

const fn default_max_history() -> usize {
    30
}
fn default_identity_path() -> String {
    "~/.zerda/identity.md".to_string()
}
const fn default_session_cleanup_days() -> u64 {
    7
}
const fn default_tool_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    #[serde(flatten)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SttConfig {
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_stt_model")]
    pub model: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            api_key: None,
            model: default_stt_model(),
        }
    }
}

fn default_stt_model() -> String {
    "whisper-large-v3-turbo".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default)]
    pub debug_plaintext: bool,
    #[serde(default = "default_stream_progress_interval_ms")]
    pub stream_progress_interval_ms: u64,
    #[serde(default = "default_true")]
    pub include_target: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "json".to_string()
}

const fn default_stream_progress_interval_ms() -> u64 {
    2000
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub embedding: MemoryEmbeddingConfig,
    #[serde(default)]
    pub sqlite: MemorySqliteConfig,
    #[serde(default)]
    pub chroma: MemoryChromaConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryEmbeddingConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub dimensions: usize,
    #[serde(default = "default_memory_embedding_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for MemoryEmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            dimensions: 0,
            timeout_ms: default_memory_embedding_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemorySqliteConfig {
    #[serde(default = "default_memory_sqlite_path")]
    pub path: String,
}

impl Default for MemorySqliteConfig {
    fn default() -> Self {
        Self {
            path: default_memory_sqlite_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryChromaConfig {
    #[serde(default = "default_memory_chroma_url")]
    pub url: String,
}

impl Default for MemoryChromaConfig {
    fn default() -> Self {
        Self {
            url: default_memory_chroma_url(),
        }
    }
}

const fn default_memory_embedding_timeout_ms() -> u64 {
    5000
}

fn default_memory_sqlite_path() -> String {
    "~/.zerda/memory/ema.sqlite3".to_string()
}

fn default_memory_chroma_url() -> String {
    "http://127.0.0.1:8000".to_string()
}

fn substitute_env_vars(input: &str) -> String {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").expect("invalid regex"));
    static MISSING_VARS: LazyLock<Mutex<std::collections::HashSet<String>>> =
        LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
    RE.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        match std::env::var(var_name) {
            Ok(val) => val,
            Err(_) => {
                if let Ok(mut missing) = MISSING_VARS.lock() {
                    if missing.insert(var_name.to_string()) {
                        tracing::warn!(
                            "Environment variable '{var_name}' is not set, using empty string"
                        );
                    }
                }
                String::new()
            }
        }
    })
    .to_string()
}

#[derive(Debug, Clone)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_name: String,
}

impl ModelRef {
    pub fn parse(s: &str) -> Result<Self> {
        let (provider_id, model_name) = s.split_once('@').ok_or_else(|| {
            anyhow::anyhow!("Invalid model reference '{s}': expected 'provider_id@model_name'")
        })?;
        anyhow::ensure!(
            !provider_id.is_empty(),
            "provider_id must not be empty in '{s}'"
        );
        anyhow::ensure!(
            !model_name.is_empty(),
            "model_name must not be empty in '{s}'"
        );
        Ok(Self {
            provider_id: provider_id.to_string(),
            model_name: model_name.to_string(),
        })
    }
}

impl fmt::Display for ModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.provider_id, self.model_name)
    }
}

pub fn resolve_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

pub fn load_config(path: Option<&Path>) -> Result<Config> {
    let config_path = if let Some(p) = path {
        p.to_path_buf()
    } else if let Ok(env_path) = std::env::var("ZERDA_CONFIG") {
        PathBuf::from(env_path)
    } else {
        resolve_path("~/.zerda/zerda.toml")
    };

    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config: {}", config_path.display()))?;

    let substituted = substitute_env_vars(&raw);
    let toml_value: toml::Value =
        toml::from_str(&substituted).context("Failed to parse TOML config")?;
    let json_value = toml_to_json(toml_value);
    let config: Config =
        serde_json::from_value(json_value).context("Failed to deserialize config")?;

    validate_config(&config)?;

    Ok(config)
}

fn deserialize_providers<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<ProviderEndpoint>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let map: std::collections::HashMap<String, ProviderEndpoint> =
        serde::Deserialize::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(id, mut ep)| {
            ep.id = id;
            ep
        })
        .collect())
}

fn validate_config(config: &Config) -> Result<()> {
    anyhow::ensure!(
        !config.providers.is_empty(),
        "At least one [providers.*] entry is required"
    );

    for ep in &config.providers {
        anyhow::ensure!(
            !ep.api_key.is_empty(),
            "providers.{}.api_key must not be empty",
            ep.id
        );
    }

    let provider_ids: std::collections::HashSet<&str> =
        config.providers.iter().map(|p| p.id.as_str()).collect();

    let primary =
        ModelRef::parse(&config.agent.primary_model.model).context("agent.primary_model.model")?;
    anyhow::ensure!(
        provider_ids.contains(primary.provider_id.as_str()),
        "agent.primary_model references unknown provider '{}'",
        primary.provider_id
    );
    validate_model_config(&config.agent.primary_model, "agent.primary_model")?;

    if let Some(ref fm) = config.agent.fast_model {
        let fast = ModelRef::parse(&fm.model).context("agent.fast_model.model")?;
        anyhow::ensure!(
            provider_ids.contains(fast.provider_id.as_str()),
            "agent.fast_model references unknown provider '{}'",
            fast.provider_id
        );
        validate_model_config(fm, "agent.fast_model")?;
    }

    for ch in &config.channels {
        if ch.name == "telegram" {
            let has_token = ch
                .params
                .get("token")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            anyhow::ensure!(has_token, "Telegram channel requires a non-empty 'token'");
        }
    }

    for name in &config.agent.disabled_primitives {
        anyhow::ensure!(
            !name.trim().is_empty(),
            "agent.disabled_primitives must not contain empty names"
        );
    }

    anyhow::ensure!(
        config.agent.tool_timeout > 0,
        "agent.tool_timeout must be greater than 0"
    );
    if let Some(primitive_timeout) = config.agent.primitive_timeout {
        anyhow::ensure!(
            primitive_timeout > 0,
            "agent.primitive_timeout must be greater than 0"
        );
    }

    if config.memory.enabled {
        anyhow::ensure!(
            !config.memory.embedding.base_url.trim().is_empty(),
            "memory.embedding.base_url must not be empty when memory.enabled = true"
        );
        anyhow::ensure!(
            !config.memory.embedding.model.trim().is_empty(),
            "memory.embedding.model must not be empty when memory.enabled = true"
        );
        anyhow::ensure!(
            config.memory.embedding.dimensions > 0,
            "memory.embedding.dimensions must be greater than 0 when memory.enabled = true"
        );
        anyhow::ensure!(
            !config.memory.sqlite.path.trim().is_empty(),
            "memory.sqlite.path must not be empty when memory.enabled = true"
        );
        anyhow::ensure!(
            !config.memory.chroma.url.trim().is_empty(),
            "memory.chroma.url must not be empty when memory.enabled = true"
        );
    }

    Ok(())
}

fn validate_model_config(mc: &ModelConfig, label: &str) -> Result<()> {
    if let Some(temp) = mc.temperature {
        anyhow::ensure!(
            (0.0..=2.0).contains(&temp),
            "{label}.temperature must be between 0.0 and 2.0, got {temp}"
        );
    }
    if let Some(top_p) = mc.top_p {
        anyhow::ensure!(
            (0.0..=1.0).contains(&top_p),
            "{label}.top_p must be between 0.0 and 1.0, got {top_p}"
        );
    }
    if let Some(max_tokens) = mc.max_tokens {
        anyhow::ensure!(max_tokens > 0, "{label}.max_tokens must be greater than 0");
    }
    Ok(())
}

fn toml_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(tbl) => {
            let map = tbl.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect();
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let unique = format!("{}-{}.toml", name, uuid::Uuid::new_v4());
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn loads_memory_config_with_embedding_and_chroma() {
        let path = temp_config_path("zerda-memory-config");
        std::fs::write(
            &path,
            r#"
[providers.openai]
type = "openai_chat"
api_key = "test-key"

[agent.primary_model]
model = "openai@gpt-4o"

[memory]
enabled = true

[memory.embedding]
base_url = "https://embed.example.com/v1"
api_key = "embed-key"
model = "text-embedding-custom"
dimensions = 1024

[memory.sqlite]
path = "/tmp/ema.sqlite3"

[memory.chroma]
url = "http://127.0.0.1:8000"
"#,
        )
        .unwrap();

        let loaded = load_config(Some(&path)).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(loaded.memory.enabled);
        assert_eq!(
            loaded.memory.embedding.base_url,
            "https://embed.example.com/v1"
        );
        assert_eq!(loaded.memory.embedding.model, "text-embedding-custom");
        assert_eq!(loaded.memory.embedding.dimensions, 1024);
        assert_eq!(loaded.memory.sqlite.path, "/tmp/ema.sqlite3");
        assert_eq!(loaded.memory.chroma.url, "http://127.0.0.1:8000");
    }

    #[test]
    fn rejects_memory_config_without_embedding_dimensions() {
        let path = temp_config_path("zerda-memory-config-invalid");
        std::fs::write(
            &path,
            r#"
[providers.openai]
type = "openai_chat"
api_key = "test-key"

[agent.primary_model]
model = "openai@gpt-4o"

[memory]
enabled = true

[memory.embedding]
base_url = "https://embed.example.com/v1"
model = "text-embedding-custom"
dimensions = 0

[memory.sqlite]
path = "/tmp/ema.sqlite3"

[memory.chroma]
url = "http://127.0.0.1:8000"
"#,
        )
        .unwrap();

        let err = load_config(Some(&path)).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(err
            .to_string()
            .contains("memory.embedding.dimensions must be greater than 0"));
    }

    #[test]
    fn agent_primitive_timeout_defaults_to_tool_timeout() {
        let path = temp_config_path("zerda-agent-primitive-timeout-default");
        std::fs::write(
            &path,
            r#"
[providers.openai]
type = "openai_chat"
api_key = "test-key"

[agent]
tool_timeout = 300

[agent.primary_model]
model = "openai@gpt-4o"
"#,
        )
        .unwrap();

        let loaded = load_config(Some(&path)).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.agent.effective_primitive_timeout(), 300);
    }

    #[test]
    fn agent_primitive_timeout_allows_explicit_override() {
        let path = temp_config_path("zerda-agent-primitive-timeout-override");
        std::fs::write(
            &path,
            r#"
[providers.openai]
type = "openai_chat"
api_key = "test-key"

[agent]
tool_timeout = 300
primitive_timeout = 45

[agent.primary_model]
model = "openai@gpt-4o"
"#,
        )
        .unwrap();

        let loaded = load_config(Some(&path)).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.agent.effective_primitive_timeout(), 45);
    }
}
