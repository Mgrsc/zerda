use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Local;
use serde_json::json;

use super::execute_python_script::ExecutePythonScriptTool;
use super::shell::ShellTool;
use super::{Tool, ToolResult};
use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, Provider, Role, ToolCall, ToolSpec,
};
use crate::util::text::TruncateForUi;

const MAX_ITERATIONS: usize = 10;
const MAX_TOOL_OUTPUT_CHARS: usize = 10_000_000;
const MAX_TOKENS: u32 = 4096;
const EXECUTOR_DIR: &str = "~/.zerda/executor_jobs";
const MAX_KEY_OUTPUT_CHARS: usize = 6000;
const EXECUTOR_SYSTEM_PROMPT: &str = include_str!("../prompts/executor_system.md");
const EXECUTOR_DELEGATE_TEMPLATE: &str = include_str!("../prompts/executor_delegate.md");

pub struct SubAgentTool {
    provider: Arc<dyn Provider>,
    chat_opts: ChatOptions,
    tool_timeout: u64,
}

impl SubAgentTool {
    pub fn new(provider: Arc<dyn Provider>, mut chat_opts: ChatOptions, tool_timeout: u64) -> Self {
        chat_opts.max_tokens = Some(MAX_TOKENS);
        Self {
            provider,
            chat_opts,
            tool_timeout,
        }
    }
}

#[async_trait]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        "delegate_to_executor"
    }

    fn description(&self) -> &str {
        "Delegate mechanical execution to the executor. Use a compact structured brief and let the executor \
         generate and run an async Python script under ./.zerda/executor_jobs/. \
         Return only key results and artifact paths."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "brief": {
                    "type": "string",
                    "description": "Structured delegation brief in text format. Recommended fields: GOAL, INPUT, CONSTRAINTS, DONE_WHEN, RETURN."
                },
                "task_name": {
                    "type": "string",
                    "description": "Optional short name used in executor artifact file names"
                }
            },
            "required": ["brief"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let brief = args
            .get("brief")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: brief"))?;
        let task_name = args.get("task_name").and_then(|v| v.as_str()).unwrap_or("");
        let artifact = prepare_executor_artifacts(task_name, brief)?;
        let user_message = build_executor_user_message(brief, &artifact, self.tool_timeout);

        let inner_tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ExecutePythonScriptTool::new(
                artifact.script_path.clone(),
                artifact.log_path.clone(),
                artifact.out_path.clone(),
                self.tool_timeout,
            )),
            Box::new(ShellTool::new(self.tool_timeout)),
        ];
        let tool_specs: Vec<ToolSpec> = inner_tools.iter().map(|t| t.spec()).collect();
        let tool_map: HashMap<&str, &dyn Tool> =
            inner_tools.iter().map(|t| (t.name(), t.as_ref())).collect();

        let system = ConversationMessage::system(EXECUTOR_SYSTEM_PROMPT.trim_end());

        let mut history = vec![system, ConversationMessage::user(user_message)];
        let mut last_text = String::new();

        for _ in 0..MAX_ITERATIONS {
            let response = self
                .provider
                .chat(&history, &tool_specs, &self.chat_opts)
                .await?;
            if let Some(text) = &response.text {
                if !text.trim().is_empty() {
                    last_text = text.clone();
                }
            }

            if response.tool_calls.is_empty() {
                let failed = executor_result_failed(&artifact);
                let output = build_executor_result(
                    response.text.as_deref().unwrap_or(""),
                    &artifact,
                    failed,
                );
                return Ok(ToolResult {
                    output,
                    is_error: failed,
                });
            }

            let mut assistant_msg = ConversationMessage {
                role: Role::Assistant,
                content: Vec::new(),
                tool_calls: response.tool_calls.clone(),
                reasoning_content: response.reasoning_content.clone(),
                thinking_blocks: response.thinking_blocks.clone(),
            };
            if let Some(ref text) = response.text {
                if !text.is_empty() {
                    assistant_msg.content.push(ContentPart::Text(text.clone()));
                }
            }
            history.push(assistant_msg);

            for tc in &response.tool_calls {
                let result = execute_inner_tool(&tool_map, tc).await;
                history.push(ConversationMessage::tool_result(
                    &tc.id,
                    result.output.truncate_for_ui(MAX_TOOL_OUTPUT_CHARS),
                    result.is_error,
                ));
            }
        }

        let final_response = self.provider.chat(&history, &[], &self.chat_opts).await?;
        let final_text = final_response
            .text
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(last_text);
        let failed = executor_result_failed(&artifact);
        Ok(ToolResult {
            output: build_executor_result(final_text.as_str(), &artifact, failed),
            is_error: failed,
        })
    }
}

async fn execute_inner_tool(tool_map: &HashMap<&str, &dyn Tool>, call: &ToolCall) -> ToolResult {
    if let Some(tool) = tool_map.get(call.name.as_str()) {
        match tool.execute(call.arguments.clone()).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                output: format!("Tool execution error: {e}"),
                is_error: true,
            },
        }
    } else {
        ToolResult {
            output: format!("Unknown tool: {}", call.name),
            is_error: true,
        }
    }
}

struct ExecutorArtifacts {
    script_path: PathBuf,
    log_path: PathBuf,
    out_path: PathBuf,
    meta_path: PathBuf,
}

fn prepare_executor_artifacts(task_name: &str, brief: &str) -> Result<ExecutorArtifacts> {
    let root = crate::config::resolve_path(EXECUTOR_DIR);
    std::fs::create_dir_all(&root)?;

    let now = Local::now();
    let day = now.format("%Y%m%d").to_string();
    let time = now.format("%H%M%S").to_string();
    let basis = if task_name.trim().is_empty() {
        brief
    } else {
        task_name
    };
    let slug = sanitize_slug(basis);
    let task_dir = root.join(day).join(format!("{time}_{slug}"));
    std::fs::create_dir_all(&task_dir)?;

    let script_path = task_dir.join("script.py");
    let log_path = task_dir.join("run.log");
    let out_path = task_dir.join("result.out");
    let meta_path = task_dir.join("task.meta");

    let meta = format!(
        "created_at: {}\nscript: {}\nlog: {}\nout: {}\nbrief:\n{}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        script_path.display(),
        log_path.display(),
        out_path.display(),
        brief
    );
    std::fs::write(&meta_path, meta)?;

    Ok(ExecutorArtifacts {
        script_path,
        log_path,
        out_path,
        meta_path,
    })
}

fn build_executor_user_message(brief: &str, a: &ExecutorArtifacts, timeout_secs: u64) -> String {
    EXECUTOR_DELEGATE_TEMPLATE
        .replace("{{BRIEF}}", brief)
        .replace("{{SCRIPT_PATH}}", &a.script_path.display().to_string())
        .replace("{{TIMEOUT_SECS}}", &timeout_secs.to_string())
        .replace("{{LOG_PATH}}", &a.log_path.display().to_string())
        .replace("{{OUT_PATH}}", &a.out_path.display().to_string())
}

fn executor_result_failed(a: &ExecutorArtifacts) -> bool {
    let content = std::fs::read_to_string(&a.out_path).unwrap_or_default();
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.eq_ignore_ascii_case("timeout or error") {
        return true;
    }
    trimmed.contains("Traceback (most recent call last)")
}

fn build_executor_result(summary: &str, a: &ExecutorArtifacts, failed: bool) -> String {
    let out_content = std::fs::read_to_string(&a.out_path).unwrap_or_default();
    let primary = if !out_content.trim().is_empty() {
        out_content.truncate_for_ui(MAX_KEY_OUTPUT_CHARS)
    } else if !summary.trim().is_empty() {
        summary.trim().to_string()
    } else {
        "(executor returned empty content)".to_string()
    };
    let status_line = if failed {
        "[executor_status: partial]"
    } else {
        "[executor_status: ok]"
    };
    format!(
        "{status_line}\n{primary}\n\n[artifacts]\nscript: {script}\nresult: {out}\nlog: {log}\nmeta: {meta}",
        script = a.script_path.display(),
        out = a.out_path.display(),
        log = a.log_path.display(),
        meta = a.meta_path.display(),
    )
}

fn sanitize_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        let valid = ch.is_ascii_alphanumeric();
        if valid {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
        if out.len() >= 48 {
            break;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.to_string()
    }
}
