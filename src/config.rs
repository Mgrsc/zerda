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
    pub reflection: ReflectionConfig,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub stt: SttConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub memory_service: MemoryServiceConfig,
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
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    #[serde(default = "default_max_tool_output_chars")]
    pub max_tool_output_chars: usize,
    #[serde(default = "default_identity_path")]
    pub identity_path: String,
    #[serde(default)]
    pub show_usage: bool,
    #[serde(default)]
    pub max_budget_tokens: Option<u64>,
    #[serde(default = "default_session_cleanup_days")]
    pub session_cleanup_days: u64,
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout: u64,
    #[serde(default)]
    pub disabled_primitives: Vec<String>,
}

const MIN_MAX_ITERATIONS: usize = 10;

const fn default_max_iterations() -> usize {
    10
}
const fn default_max_history() -> usize {
    30
}
const fn default_max_tool_output_chars() -> usize {
    30000
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
pub struct ReflectionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, alias = "model")]
    pub llm_model: String,
    #[serde(default = "default_reflection_max_tokens")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default)]
    pub embedding_dim: Option<u64>,
}

const fn default_reflection_max_tokens() -> Option<u32> {
    Some(2048)
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            llm_model: String::new(),
            max_tokens: default_reflection_max_tokens(),
            embedding_model: None,
            embedding_dim: None,
        }
    }
}

impl ReflectionConfig {
    pub fn as_model_config(&self) -> Option<ModelConfig> {
        let model = self.llm_model.trim();
        if model.is_empty() {
            return None;
        }
        Some(ModelConfig {
            model: model.to_string(),
            vision: false,
            temperature: Some(0.7),
            top_p: Some(0.95),
            max_tokens: self.max_tokens,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpFile {
    #[serde(default)]
    mcp: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    #[serde(flatten)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_tts_model")]
    pub model: String,
    #[serde(default)]
    pub voice_id: Option<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            api_key: None,
            model: default_tts_model(),
            voice_id: None,
        }
    }
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

fn default_tts_model() -> String {
    "speech-2.8-hd".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryServiceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_memory_service_url")]
    pub url: String,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default = "default_memory_tenant_id")]
    pub tenant_id: String,
    #[serde(default = "default_memory_entity_id")]
    pub default_entity_id: String,
    #[serde(default = "default_memory_process_id")]
    pub process_id: String,
    #[serde(default = "default_recall_timeout_ms")]
    pub recall_timeout_ms: u64,
    #[serde(default = "default_recall_top_k")]
    pub recall_top_k: u32,
    #[serde(default = "default_recall_min_score")]
    pub recall_min_score: f64,
    #[serde(default = "default_ingest_batch_turns")]
    pub ingest_batch_turns: u32,
    #[serde(default = "default_ingest_timeout_ms")]
    pub ingest_timeout_ms: u64,
    #[serde(default = "default_ingest_max_retries")]
    pub ingest_max_retries: u32,
}

impl Default for MemoryServiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_memory_service_url(),
            auth_token: String::new(),
            tenant_id: default_memory_tenant_id(),
            default_entity_id: default_memory_entity_id(),
            process_id: default_memory_process_id(),
            recall_timeout_ms: default_recall_timeout_ms(),
            recall_top_k: default_recall_top_k(),
            recall_min_score: default_recall_min_score(),
            ingest_batch_turns: default_ingest_batch_turns(),
            ingest_timeout_ms: default_ingest_timeout_ms(),
            ingest_max_retries: default_ingest_max_retries(),
        }
    }
}

fn default_memory_service_url() -> String {
    "http://localhost:8080".to_string()
}
fn default_memory_tenant_id() -> String {
    "default".to_string()
}
fn default_memory_entity_id() -> String {
    "user_default".to_string()
}
fn default_memory_process_id() -> String {
    "planner".to_string()
}
const fn default_recall_timeout_ms() -> u64 {
    3000
}
const fn default_recall_top_k() -> u32 {
    8
}
const fn default_recall_min_score() -> f64 {
    0.7
}
const fn default_ingest_batch_turns() -> u32 {
    3
}
const fn default_ingest_timeout_ms() -> u64 {
    10000
}
const fn default_ingest_max_retries() -> u32 {
    2
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
    let mut config: Config =
        serde_json::from_value(json_value).context("Failed to deserialize config")?;

    let mcp_path = config_path.with_file_name("mcp.toml");
    if mcp_path.exists() {
        let mcp_raw = std::fs::read_to_string(&mcp_path)
            .with_context(|| format!("Failed to read MCP config: {}", mcp_path.display()))?;
        let mcp_substituted = substitute_env_vars(&mcp_raw);
        let mcp_toml: toml::Value = toml::from_str(&mcp_substituted)
            .with_context(|| format!("Failed to parse {}", mcp_path.display()))?;
        let mcp_json = toml_to_json(mcp_toml);
        let mcp_file: McpFile = serde_json::from_value(mcp_json)
            .with_context(|| format!("Failed to deserialize {}", mcp_path.display()))?;
        config.mcp.extend(mcp_file.mcp);
    }

    if config.agent.max_iterations < MIN_MAX_ITERATIONS {
        tracing::warn!(
            "agent.max_iterations={} is below minimum {}; using {}",
            config.agent.max_iterations,
            MIN_MAX_ITERATIONS,
            MIN_MAX_ITERATIONS
        );
        config.agent.max_iterations = MIN_MAX_ITERATIONS;
    }

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
    let providers_by_id: std::collections::HashMap<&str, &ProviderEndpoint> = config
        .providers
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

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

    if config.reflection.enabled {
        anyhow::ensure!(
            config.reflection.as_model_config().is_some(),
            "reflection.enabled=true requires non-empty reflection.llm_model (provider_id@model_name)"
        );
    }

    if let Some(reflection_model) = config.reflection.as_model_config() {
        let reflection_ref =
            ModelRef::parse(&reflection_model.model).context("reflection.llm_model")?;
        anyhow::ensure!(
            provider_ids.contains(reflection_ref.provider_id.as_str()),
            "reflection.llm_model references unknown provider '{}'",
            reflection_ref.provider_id
        );
        validate_model_config(&reflection_model, "reflection.llm_model")?;
    }

    if config.reflection.enabled {
        if let Some(ref embedding_model) = config.reflection.embedding_model {
            let embedding_ref = ModelRef::parse(embedding_model)
                .context("reflection.embedding_model (expected provider_id@model_name)")?;
            anyhow::ensure!(
                provider_ids.contains(embedding_ref.provider_id.as_str()),
                "reflection.embedding_model references unknown provider '{}'",
                embedding_ref.provider_id
            );
            if let Some(provider) = providers_by_id.get(embedding_ref.provider_id.as_str()) {
                anyhow::ensure!(
                    supports_openai_embeddings(&provider.kind),
                    "reflection.embedding_model provider '{}' uses unsupported type '{}' for embeddings; expected openai_chat or openai_responses",
                    provider.id,
                    provider.kind
                );
            }
        } else if let Some(reflection_model) = config.reflection.as_model_config() {
            let reflection_ref = ModelRef::parse(&reflection_model.model)
                .context("reflection.llm_model (for default embedding provider)")?;
            if let Some(provider) = providers_by_id.get(reflection_ref.provider_id.as_str()) {
                anyhow::ensure!(
                    supports_openai_embeddings(&provider.kind),
                    "reflection.llm_model provider '{}' uses unsupported type '{}' for default embeddings; configure reflection.embedding_model with an OpenAI-compatible provider",
                    provider.id,
                    provider.kind
                );
            }
        }
    }

    anyhow::ensure!(
        config.agent.max_iterations >= MIN_MAX_ITERATIONS,
        "max_iterations must be greater than or equal to {}",
        MIN_MAX_ITERATIONS
    );

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

    validate_mcp_servers(&config.mcp)?;

    Ok(())
}

fn supports_openai_embeddings(provider_type: &str) -> bool {
    matches!(provider_type, "openai_chat" | "openai_responses")
}

fn validate_mcp_servers(servers: &[McpServerConfig]) -> Result<()> {
    for server in servers {
        anyhow::ensure!(!server.name.trim().is_empty(), "mcp.name must not be empty");
        anyhow::ensure!(
            !server.transport.trim().is_empty(),
            "mcp.{}.transport must not be empty",
            server.name
        );

        match server.transport.as_str() {
            "stdio" => {
                anyhow::ensure!(
                    server
                        .command
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty()),
                    "mcp.{} with stdio transport requires non-empty command",
                    server.name
                );
                anyhow::ensure!(
                    server.url.is_none() || server.url.as_deref().is_some_and(|u| u.is_empty()),
                    "mcp.{} with stdio transport must not set url",
                    server.name
                );
            }
            "streamable-http" => {
                anyhow::ensure!(
                    server.url.as_deref().is_some_and(|s| !s.trim().is_empty()),
                    "mcp.{} with streamable-http transport requires non-empty url",
                    server.name
                );
            }
            other => {
                anyhow::bail!(
                    "mcp.{} transport '{}' is unsupported; expected 'stdio' or 'streamable-http'",
                    server.name,
                    other
                );
            }
        }

        for arg in &server.args {
            anyhow::ensure!(
                !arg.is_empty(),
                "mcp.{} args must not contain empty items",
                server.name
            );
        }
        for key in server.env.keys() {
            anyhow::ensure!(
                !key.trim().is_empty(),
                "mcp.{} env keys must not be empty",
                server.name
            );
        }
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
