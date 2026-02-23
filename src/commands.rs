use crate::agent::Agent;
use crate::prompt;
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
    CommandInfo {
        name: "cancel",
        description: "Cancel current running turn",
    },
];

pub fn command_infos() -> &'static [CommandInfo] {
    COMMANDS
}

fn parse_command(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
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

    Some((command_name, args))
}

pub fn is_cancel_command(input: &str) -> bool {
    parse_command(input)
        .map(|(name, _)| name == "cancel")
        .unwrap_or(false)
}

pub async fn try_handle_command(
    input: &str,
    agent: &mut Agent,
    hot: &mut HotState,
    _ctx: &RunContext<'_>,
) -> CommandResult {
    let Some((command_name, args)) = parse_command(input) else {
        return CommandResult::NotACommand;
    };

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
            let input_tokens = agent.total_usage.input_tokens;
            let output_tokens = agent.total_usage.output_tokens;
            let total = input_tokens + output_tokens;

            let os_name = prompt::read_os_pretty_name();
            let shell = prompt::read_default_shell();
            let platform = std::env::consts::OS;
            let provider_name = &hot.cfg.provider.name;
            let model = &hot.chat_opts.model;
            let temp = hot.chat_opts.temperature;
            let top_p = hot.chat_opts.top_p;
            let tool_total = hot.tools.len();
            let builtin = hot.builtin_count;
            let mcp = tool_total - builtin;
            let skills = hot.skills.len();
            let pending = hot.todo.pending_count();

            let has_assistant = agent
                .history
                .iter()
                .any(|m| matches!(m.role, Role::Assistant));
            let usage_warning = total == 0 && has_assistant;

            const W: usize = 44;
            let hl = format!("├{}┤", "─".repeat(W + 2));
            let cell = |s: &str| -> String { format!("│ {:width$} │", s, width = W) };
            let row =
                |label: &str, value: &str| -> String { cell(&format!("  {label:<9}: {value}")) };

            let mut rows: Vec<String> = Vec::new();
            rows.push(format!("┌{}┐", "─".repeat(W + 2)));
            rows.push(cell("System"));
            rows.push(hl.clone());
            rows.push(row("Version", env!("ZERDA_VERSION")));
            rows.push(row("Platform", &format!("{platform} ({os_name})")));
            rows.push(row("Shell", &shell));
            rows.push(hl.clone());
            rows.push(cell("Provider"));
            rows.push(hl.clone());
            rows.push(row("Provider", provider_name));
            rows.push(row("Model", model));
            rows.push(row("Temp/TopP", &format!("{temp} / {top_p}")));
            rows.push(hl.clone());
            rows.push(cell("Session"));
            rows.push(hl.clone());
            rows.push(row(
                "History",
                &format!("{non_system} / {max_history} messages"),
            ));
            rows.push(row(
                "Tools",
                &format!("{tool_total}  ({builtin} builtin, {mcp} mcp)"),
            ));
            rows.push(row("Skills", &format!("{skills} loaded")));
            rows.push(row("Todos", &format!("{pending} pending")));
            rows.push(hl.clone());
            rows.push(cell("Tokens"));
            rows.push(hl.clone());
            rows.push(row("Input", &fmt_thousands(input_tokens)));
            rows.push(row("Output", &fmt_thousands(output_tokens)));
            if usage_warning {
                rows.push(row("", "⚠  provider may not report usage"));
            }
            rows.push(format!("└{}┘", "─".repeat(W + 2)));

            format!("```\n{}\n```", rows.join("\n"))
        }
        "help" => COMMANDS
            .iter()
            .map(|c| format!("/{:<9} - {}", c.name, c.description))
            .collect::<Vec<_>>()
            .join("\n"),
        "cancel" => "No running turn to cancel".to_string(),
        _ => "Unknown command. Type /help for available commands.".to_string(),
    };

    CommandResult::Handled(response)
}

fn fmt_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}
