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
    pub provider: ProviderConfig,
    pub agent: AgentConfig,
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
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_true")]
    pub vision: bool,
    #[serde(default)]
    pub extra_headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub retry: RetryConfig,
}

const fn default_base_url() -> String {
    String::new()
}
const fn default_temperature() -> f64 {
    1.0
}
const fn default_top_p() -> f64 {
    0.95
}
const fn default_max_tokens() -> u32 {
    4096
}
const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    #[serde(default = "default_max_tool_output_chars")]
    pub max_tool_output_chars: usize,
    #[serde(default = "default_max_memory_tokens")]
    pub max_memory_tokens: usize,
    #[serde(default = "default_identity_path")]
    pub identity_path: String,
    pub fast_model: Option<FastModelConfig>,
    #[serde(default)]
    pub show_usage: bool,
    #[serde(default)]
    pub max_budget_tokens: Option<u64>,
    #[serde(default = "default_max_memory_file_size")]
    pub max_memory_file_size: u64,
    #[serde(default = "default_session_cleanup_days")]
    pub session_cleanup_days: u64,
    #[serde(default = "default_tool_timeout")]
    pub tool_timeout: u64,
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
const fn default_max_memory_tokens() -> usize {
    2000
}
fn default_identity_path() -> String {
    "~/.zerda/identity.md".to_string()
}
const fn default_max_memory_file_size() -> u64 {
    102_400
}
const fn default_session_cleanup_days() -> u64 {
    7
}
const fn default_tool_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Deserialize)]
pub struct FastModelConfig {
    #[serde(flatten)]
    pub provider: ProviderConfig,
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

fn validate_config(config: &Config) -> Result<()> {
    anyhow::ensure!(
        !config.provider.api_key.is_empty(),
        "provider.api_key must not be empty"
    );
    anyhow::ensure!(
        !config.provider.model.is_empty(),
        "provider.model must not be empty"
    );
    anyhow::ensure!(
        (0.0..=2.0).contains(&config.provider.temperature),
        "temperature must be between 0.0 and 2.0, got {}",
        config.provider.temperature
    );
    anyhow::ensure!(
        (0.0..=1.0).contains(&config.provider.top_p),
        "top_p must be between 0.0 and 1.0, got {}",
        config.provider.top_p
    );
    anyhow::ensure!(
        config.provider.max_tokens > 0,
        "max_tokens must be greater than 0"
    );
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
