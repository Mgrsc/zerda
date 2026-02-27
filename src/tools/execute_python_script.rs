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
    telemetry_path: PathBuf,
    timeout_secs: u64,
    primitives_py_root: Option<PathBuf>,
    bootstrap_path: Option<PathBuf>,
    firecrawl_enabled: bool,
}

impl ExecutePythonScriptTool {
    pub fn new(
        script_path: PathBuf,
        log_path: PathBuf,
        out_path: PathBuf,
        telemetry_path: PathBuf,
        timeout_secs: u64,
        primitives_py_root: Option<PathBuf>,
        bootstrap_path: Option<PathBuf>,
        firecrawl_enabled: bool,
    ) -> Self {
        Self {
            script_path,
            log_path,
            out_path,
            telemetry_path,
            timeout_secs,
            primitives_py_root,
            bootstrap_path,
            firecrawl_enabled,
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
        if let Some(parent) = self.telemetry_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let final_code = build_bootstrapped_code(
            code,
            self.bootstrap_path.as_ref(),
            &self.out_path,
            &self.log_path,
            &self.telemetry_path,
        );
        tokio::fs::write(&self.script_path, final_code).await?;
        tracing::info!(
            script = %self.script_path.display(),
            timeout_secs = self.timeout_secs,
            script_bytes = code_bytes,
            firecrawl_enabled = self.firecrawl_enabled,
            "execute_python_script start"
        );

        let mut command = Command::new("python3");
        command
            .arg(&self.script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("EXECUTOR_OUT_PATH", &self.out_path)
            .env("EXECUTOR_LOG_PATH", &self.log_path)
            .env("EXECUTOR_TELEMETRY_PATH", &self.telemetry_path)
            .env(
                "EXECUTOR_ENABLE_FIRECRAWL_PRIMITIVES",
                if self.firecrawl_enabled { "1" } else { "0" },
            );

        if let Some(root) = &self.primitives_py_root {
            command.env("EXECUTOR_PRIMITIVES_PY_ROOT", root);
        }
        if let Some(parent) = self.script_path.parent() {
            command.current_dir(parent);
        }

        let child = command.spawn()?;

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
            "telemetry_path": self.telemetry_path,
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

fn build_bootstrapped_code(
    user_code: &str,
    bootstrap_path: Option<&PathBuf>,
    out_path: &PathBuf,
    log_path: &PathBuf,
    telemetry_path: &PathBuf,
) -> String {
    let mut lines = Vec::new();
    lines.push("import os".to_string());
    lines.push(format!(
        "os.environ.setdefault(\"EXECUTOR_OUT_PATH\", {})",
        to_py_string(out_path.display().to_string())
    ));
    lines.push(format!(
        "os.environ.setdefault(\"EXECUTOR_LOG_PATH\", {})",
        to_py_string(log_path.display().to_string())
    ));
    lines.push(format!(
        "os.environ.setdefault(\"EXECUTOR_TELEMETRY_PATH\", {})",
        to_py_string(telemetry_path.display().to_string())
    ));
    if let Some(path) = bootstrap_path {
        lines.push(format!(
            "_BOOTSTRAP_PATH = {}",
            to_py_string(path.display().to_string())
        ));
        lines.push("if os.path.exists(_BOOTSTRAP_PATH):".to_string());
        lines.push("    with open(_BOOTSTRAP_PATH, \"r\", encoding=\"utf-8\") as _bf:".to_string());
        lines.push("        _bootstrap_src = _bf.read()".to_string());
        lines.push(
            "    exec(compile(_bootstrap_src, _BOOTSTRAP_PATH, \"exec\"), globals(), globals())"
                .to_string(),
        );
    }
    lines.push(String::new());
    lines.push(user_code.to_string());
    lines.join("\n")
}

fn to_py_string(value: String) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "\"\"".to_string())
}
