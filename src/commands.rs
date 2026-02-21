use crate::agent::Agent;
use crate::providers::{Role, Usage};
use crate::runner::{self, HotState, RunContext};

pub struct CommandInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub enum CommandResult {
    Handled(String),
    NotACommand,
}

const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "clear",
        description: "Clear conversation history",
    },
    CommandInfo {
        name: "compact",
        description: "Compress context",
    },
    CommandInfo {
        name: "model",
        description: "Show or switch model",
    },
    CommandInfo {
        name: "status",
        description: "Show token usage and status",
    },
    CommandInfo {
        name: "help",
        description: "Show available commands",
    },
];

pub fn command_infos() -> &'static [CommandInfo] {
    COMMANDS
}

pub async fn try_handle_command(
    input: &str,
    agent: &mut Agent,
    hot: &mut HotState,
    _ctx: &RunContext<'_>,
) -> CommandResult {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return CommandResult::NotACommand;
    }

    let (raw_command, args) = match trimmed.split_once(' ') {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (trimmed, ""),
    };

    let command_name = raw_command
        .strip_prefix('/')
        .unwrap_or(raw_command)
        .split('@')
        .next()
        .unwrap_or("");

    let response = match command_name {
        "clear" => {
            agent.history.clear();
            agent.total_usage = Usage::default();
            runner::refresh_prompt(agent, hot, None);
            "Context cleared.".to_string()
        }
        "compact" => {
            let memory_dir = crate::config::resolve_path(crate::config::MEMORY_DIR);
            match agent.compress_with_llm(&memory_dir).await {
                Ok(()) => "Context compressed with LLM.".to_string(),
                Err(e) => format!("Compression failed: {e}"),
            }
        }
        "model" => {
            if args.is_empty() {
                format!("Current model: {}", hot.chat_opts.model)
            } else {
                hot.chat_opts.model = args.to_string();
                format!("Model switched to: {args}")
            }
        }
        "status" => {
            let non_system = agent
                .history
                .iter()
                .filter(|m| !matches!(m.role, Role::System))
                .count();
            let max_history = hot.cfg.agent.max_history;
            let input = agent.total_usage.input_tokens;
            let output = agent.total_usage.output_tokens;
            let total = input + output;
            let model = &hot.chat_opts.model;

            let mut lines = vec![
                format!("History: {non_system}/{max_history} messages"),
                format!("Tokens: in={input}, out={output}, total={total}"),
                format!("Model: {model}"),
            ];

            if let Some(budget) = hot.cfg.agent.max_budget_tokens {
                let remaining = budget.saturating_sub(total);
                lines.push(format!("Budget remaining: {remaining}"));
            }

            lines.join("\n")
        }
        "help" => COMMANDS
            .iter()
            .map(|c| format!("/{:<9} - {}", c.name, c.description))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => "Unknown command. Type /help for available commands.".to_string(),
    };

    CommandResult::Handled(response)
}
