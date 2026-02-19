use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use super::{Tool, ToolResult};

const DEFAULT_TIMEOUT_SECS: u64 = 300;

pub struct ShellTool {
    timeout: Duration,
}

impl ShellTool {
    pub const fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new(DEFAULT_TIMEOUT_SECS)
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: command"))?;

        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(result) => result?,
            Err(_) => {
                return Ok(ToolResult {
                    output: format!("Command timed out after {} seconds", self.timeout.as_secs()),
                    is_error: true,
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");

        Ok(ToolResult {
            output: combined,
            is_error: !output.status.success(),
        })
    }
}
