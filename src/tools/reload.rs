use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Tool, ToolResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadMode {
    Light,
}

#[derive(Clone, Default)]
pub struct ReloadSignal(Arc<Mutex<Option<ReloadMode>>>);

impl ReloadSignal {
    pub fn request(&self, mode: ReloadMode) {
        *self.0.lock().unwrap() = Some(mode);
    }

    pub fn take(&self) -> Option<ReloadMode> {
        self.0.lock().unwrap().take()
    }
}

pub struct ReloadTool {
    config_path: Option<PathBuf>,
    signal: ReloadSignal,
}

impl ReloadTool {
    pub fn new(config_path: Option<PathBuf>, signal: ReloadSignal) -> Self {
        Self {
            config_path,
            signal,
        }
    }
}

#[async_trait]
impl Tool for ReloadTool {
    fn name(&self) -> &str {
        "reload"
    }

    fn description(&self) -> &str {
        "Validate and reload zerda.toml configuration. mode='light' reloads MCP servers, skills, and prompt/identity context without restarting. mode='full' (default) restarts the process."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["light", "full"],
                    "description": "light: reload MCP/skills/identity and prompt context in-place. full: restart process (required for provider/channel/stt/tts and most agent runtime settings)."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("light");

        let config_result = crate::config::load_config(self.config_path.as_deref());

        match mode {
            "light" => match config_result {
                Ok(cfg) => {
                    let mcp_count = cfg.mcp.len();
                    self.signal.request(ReloadMode::Light);
                    Ok(ToolResult {
                        output: format!(
                            "Config validated. Light reload scheduled: {mcp_count} MCP server(s). \
                             Reload will complete between turns. If non-light-safe fields changed, \
                             the system will reject light reload and ask for mode='full'.",
                        ),
                        is_error: false,
                    })
                }
                Err(e) => Ok(ToolResult {
                    output: format!("Config validation failed: {e}. Fix the config and try again."),
                    is_error: true,
                }),
            },
            "full" => match config_result {
                Ok(cfg) => {
                    let memory_dir = crate::config::resolve_path(&cfg.agent.memory_dir);
                    if let Err(e) = std::fs::write(memory_dir.join(".reload-pending"), "") {
                        tracing::warn!("Failed to write reload marker: {e}");
                    }
                    tracing::info!("Config validated successfully. Triggering reload (exit 75)...");
                    std::process::exit(75);
                }
                Err(e) => Ok(ToolResult {
                    output: format!("Config validation failed: {e}. Fix the config and try again."),
                    is_error: true,
                }),
            },
            other => Ok(ToolResult {
                output: format!("Unknown mode: {other}. Use 'light' or 'full'."),
                is_error: true,
            }),
        }
    }
}
