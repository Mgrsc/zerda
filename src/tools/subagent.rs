use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::read::ReadTool;
use super::shell::ShellTool;
use super::{Tool, ToolResult};
use crate::providers::{
    ChatOptions, ContentPart, ConversationMessage, Provider, Role, ToolCall, ToolSpec,
};
use crate::util::text::TruncateForUi;

const MAX_ITERATIONS: usize = 10;
const MAX_TOOL_OUTPUT_CHARS: usize = 10_000_000;
const MAX_TOKENS: u32 = 4096;

pub struct SubAgentTool {
    provider: Arc<dyn Provider>,
    chat_opts: ChatOptions,
    tool_timeout: u64,
}

impl SubAgentTool {
    pub fn new(provider: Arc<dyn Provider>, mut chat_opts: ChatOptions, tool_timeout: u64) -> Self {
        chat_opts.max_tokens = MAX_TOKENS;
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
        "subagent"
    }

    fn description(&self) -> &str {
        "Run a sub-agent (smaller model) with shell and read tools to process data, extract information, summarize content, or perform auxiliary tasks. The sub-agent can autonomously use tools to complete the given task."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Instructions for the sub-agent describing what to do"
                },
                "content": {
                    "type": "string",
                    "description": "Text content to pass directly to the sub-agent (mutually exclusive with file_path)"
                },
                "file_path": {
                    "type": "string",
                    "description": "Path to a file for the sub-agent to process (mutually exclusive with content)"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prompt"))?;

        let content = args.get("content").and_then(|v| v.as_str());
        let file_path = args.get("file_path").and_then(|v| v.as_str());

        let user_message = match (content, file_path) {
            (Some(c), _) => format!("{prompt}\n\n<content>\n{c}\n</content>"),
            (_, Some(path)) => format!(
                "{prompt}\n\nThe content is in the file: {path}\nUse the `read` tool to read it."
            ),
            _ => prompt.to_string(),
        };

        let inner_tools: Vec<Box<dyn Tool>> = vec![
            Box::new(ShellTool::new(self.tool_timeout)),
            Box::new(ReadTool),
        ];
        let tool_specs: Vec<ToolSpec> = inner_tools.iter().map(|t| t.spec()).collect();
        let tool_map: HashMap<&str, &dyn Tool> =
            inner_tools.iter().map(|t| (t.name(), t.as_ref())).collect();

        let system = ConversationMessage::system(
            "You are a focused assistant. Complete the given task using available tools. \
             Be concise and return only the requested information.",
        );

        let mut history = vec![system, ConversationMessage::user(user_message)];

        for _ in 0..MAX_ITERATIONS {
            let response = self
                .provider
                .chat(&history, &tool_specs, &self.chat_opts)
                .await?;

            if response.tool_calls.is_empty() {
                return Ok(ToolResult {
                    output: response.text.unwrap_or_default(),
                    is_error: false,
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
        Ok(ToolResult {
            output: final_response.text.unwrap_or_else(|| {
                "Sub-agent reached max iterations without final answer.".to_string()
            }),
            is_error: false,
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
