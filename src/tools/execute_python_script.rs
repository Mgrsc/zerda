use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::{Tool, ToolResult};
use crate::util::text::TruncateForUi;

const MAX_SCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUT_PREVIEW_CHARS: usize = 1200;
const MAX_LOG_PREVIEW_CHARS: usize = 1200;
const MAX_INLINE_MODEL_CHARS: usize = 12_000;

pub struct ExecutePythonScriptTool {
    script_path: PathBuf,
    log_path: PathBuf,
    out_path: PathBuf,
    timeout_secs: u64,
}

impl ExecutePythonScriptTool {
    pub fn new(
        script_path: PathBuf,
        log_path: PathBuf,
        out_path: PathBuf,
        timeout_secs: u64,
    ) -> Self {
        Self {
            script_path,
            log_path,
            out_path,
            timeout_secs,
        }
    }
}

#[async_trait]
impl Tool for ExecutePythonScriptTool {
    fn name(&self) -> &str {
        "execute_python_script"
    }

    fn description(&self) -> &str {
        "Write Python code to the pre-allocated script path and execute it. Returns standardized execution status, exit code, artifact paths, and output/log previews."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Pure Python code. Must write final structured results to the pre-allocated out path."
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let code = args
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: code"))?;

        if code.trim().is_empty() {
            return Ok(ToolResult {
                output: json!({
                    "status": "error",
                    "reason": "empty_code",
                    "script_path": self.script_path,
                    "log_path": self.log_path,
                    "out_path": self.out_path,
                })
                .to_string(),
                is_error: true,
            });
        }

        let code_bytes = code.len();
        if code_bytes > MAX_SCRIPT_BYTES {
            return Ok(ToolResult {
                output: json!({
                    "status": "error",
                    "reason": "script_too_large",
                    "script_bytes": code_bytes,
                    "max_script_bytes": MAX_SCRIPT_BYTES,
                    "script_path": self.script_path,
                    "log_path": self.log_path,
                    "out_path": self.out_path,
                })
                .to_string(),
                is_error: true,
            });
        }

        if let Some(parent) = self.script_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = self.log_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = self.out_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&self.script_path, code).await?;
        tracing::info!(
            script = %self.script_path.display(),
            timeout_secs = self.timeout_secs,
            script_bytes = code_bytes,
            "execute_python_script start"
        );

        let child = Command::new("python3")
            .arg(&self.script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let (mut status, exit_code, stdout_text, stderr_text) = match tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        {
            Ok(Ok(output)) => {
                let exit_code = output.status.code();
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout_text = stdout.to_string();
                let stderr_text = stderr.to_string();
                if !output.status.success() {
                    ("error".to_string(), exit_code, stdout_text, stderr_text)
                } else {
                    ("ok".to_string(), exit_code, stdout_text, stderr_text)
                }
            }
            Ok(Err(e)) => (
                "error".to_string(),
                None,
                String::new(),
                format!("failed_to_wait_process: {e}"),
            ),
            Err(_) => (
                "timeout".to_string(),
                None,
                String::new(),
                format!("process_timeout_after_{}s", self.timeout_secs),
            ),
        };

        let combined_log = format!(
            "=== STDOUT ===\n{}\n\n=== STDERR ===\n{}",
            stdout_text, stderr_text
        );
        tokio::fs::write(&self.log_path, &combined_log).await?;

        let out_content = tokio::fs::read_to_string(&self.out_path)
            .await
            .unwrap_or_default();
        let out_trimmed = out_content.trim();
        if out_trimmed.is_empty() && status == "ok" {
            status = "partial".to_string();
        }

        let out_size = tokio::fs::metadata(&self.out_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let log_size = tokio::fs::metadata(&self.log_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        tracing::info!(
            script = %self.script_path.display(),
            out = %self.out_path.display(),
            log = %self.log_path.display(),
            status = %status,
            exit_code = ?exit_code,
            out_size,
            log_size,
            "execute_python_script done"
        );

        let out_inline = if out_content.chars().count() <= MAX_INLINE_MODEL_CHARS {
            Some(out_content.clone())
        } else {
            None
        };
        let model_output = if let Some(full) = &out_inline {
            full.clone()
        } else if !out_trimmed.is_empty() {
            out_content.truncate_for_ui(MAX_OUT_PREVIEW_CHARS)
        } else if !stdout_text.trim().is_empty() {
            stdout_text.truncate_for_ui(MAX_OUT_PREVIEW_CHARS)
        } else {
            String::new()
        };

        let response = json!({
            "status": status,
            "exit_code": exit_code,
            "script_path": self.script_path,
            "log_path": self.log_path,
            "out_path": self.out_path,
            "out_size": out_size,
            "log_size": log_size,
            "stdout_preview": stdout_text.truncate_for_ui(MAX_OUT_PREVIEW_CHARS),
            "stderr_preview": stderr_text.truncate_for_ui(MAX_LOG_PREVIEW_CHARS),
            "out_preview": out_content.truncate_for_ui(MAX_OUT_PREVIEW_CHARS),
            "log_preview": combined_log.truncate_for_ui(MAX_LOG_PREVIEW_CHARS),
            "out_inline": out_inline,
            "model_output": model_output,
        });

        let output = serde_json::to_string_pretty(&response)?;
        Ok(ToolResult {
            output,
            is_error: status != "ok",
        })
    }
}
