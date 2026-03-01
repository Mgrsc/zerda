use std::sync::Arc;

use anyhow::Result;

use super::types::{Guideline, ReflectionContext};
use crate::providers::{ChatOptions, ConversationMessage, Provider, Role};

const COMPRESS_PROMPT: &str = "\
Analyze this executor task that initially failed but eventually succeeded.

<instruction>{instruction}</instruction>
<execution_history>{history}</execution_history>

Failed at iterations: {error_indices}

Extract ONE reusable lesson (max 2 sentences, under 50 words):
- What wrong approach/assumption caused the failure
- What fix or method led to success

Rules:
- Imperative voice, start with a verb
- Focus on methodology (How to act / What to avoid), NOT domain facts
- Must generalize to similar tasks
- Output ONLY the lesson text";

const DEFAULT_ACON_MAX_TOKENS: u32 = 1024;

pub struct ReflectionAnalyzer {
    provider: Arc<dyn Provider>,
    chat_opts: ChatOptions,
}

impl ReflectionAnalyzer {
    pub fn new(provider: Arc<dyn Provider>, mut chat_opts: ChatOptions) -> Self {
        if chat_opts.max_tokens.is_none() {
            chat_opts.max_tokens = Some(DEFAULT_ACON_MAX_TOKENS);
        }
        Self {
            provider,
            chat_opts,
        }
    }

    pub async fn compress(&self, ctx: &ReflectionContext) -> Result<Option<Guideline>> {
        let error_indices: Vec<String> = ctx
            .iteration_outcomes
            .iter()
            .enumerate()
            .filter(|(_, o)| o.had_tool_error || o.had_traceback)
            .map(|(i, _)| i.to_string())
            .collect();

        if error_indices.is_empty() {
            return Ok(None);
        }

        let history_text = serialize_history(&ctx.history);
        let prompt = COMPRESS_PROMPT
            .replace("{instruction}", &ctx.instruction)
            .replace("{history}", &history_text)
            .replace("{error_indices}", &error_indices.join(", "));

        let messages = vec![ConversationMessage::user(prompt)];
        let response = self
            .provider
            .chat(&messages, &[], &self.chat_opts)
            .await?;

        let text = response
            .text
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().to_string());

        match text {
            Some(guideline_text) => Ok(Some(Guideline {
                id: uuid::Uuid::new_v4().to_string(),
                guideline_text,
                score: 0.0,
            })),
            None => Ok(None),
        }
    }
}

fn serialize_history(history: &[ConversationMessage]) -> String {
    let mut parts = Vec::new();
    for msg in history {
        let role_tag = match &msg.role {
            Role::System => continue,
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::ToolResult { is_error, .. } => {
                if *is_error {
                    "tool_error"
                } else {
                    "tool_result"
                }
            }
        };
        let text = msg.text_content();
        if text.is_empty() && msg.tool_calls.is_empty() {
            continue;
        }
        let mut entry = format!("[{role_tag}]");
        if !text.is_empty() {
            let truncated = if text.len() > 500 {
                format!("{}...", &text[..text.floor_char_boundary(500)])
            } else {
                text
            };
            entry.push_str(&format!(" {truncated}"));
        }
        for tc in &msg.tool_calls {
            entry.push_str(&format!(" -> call {}({})", tc.name, tc.arguments));
        }
        parts.push(entry);
    }
    parts.join("\n")
}
