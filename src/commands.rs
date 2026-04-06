use crate::agent::Agent;
use crate::config::ModelRef;
use crate::prompt;
use crate::providers::{Role, Usage, LIST_MODELS_UNSUPPORTED};
use crate::runner::{self, HotState, RunContext};

pub struct CommandInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub enum CommandResult {
    Handled(String),
    NotACommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPolicy {
    ImmediateRead,
    ImmediateJobControl,
    QueueWhileBusy,
    CancelThenRun,
    CancelCurrentTurn,
}

#[derive(Debug, Clone)]
pub enum Command {
    Clear,
    Compact,
    ModelShow,
    ModelList { provider_id: String },
    ModelSwitch { model_ref: ModelRef },
    ModelInvalid { raw_args: String, error: String },
    Status,
    Help,
    Cancel,
    Jobs,
    Job { job_id: String },
    CancelJob { job_id: String },
    Unknown { name: String },
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
        description: "Show, switch, or list models",
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
    CommandInfo {
        name: "jobs",
        description: "List PTC jobs",
    },
    CommandInfo {
        name: "job",
        description: "Inspect a PTC job",
    },
    CommandInfo {
        name: "cancel-job",
        description: "Cancel a running PTC job",
    },
];

pub fn command_infos() -> &'static [CommandInfo] {
    COMMANDS
}

pub fn parse_command(input: &str) -> Option<Command> {
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

    let command = match command_name {
        "clear" => Command::Clear,
        "compact" => Command::Compact,
        "model" => {
            if args.is_empty() {
                Command::ModelShow
            } else if let Some(provider_id) = parse_model_list_command_args(args) {
                Command::ModelList {
                    provider_id: provider_id.to_string(),
                }
            } else {
                match ModelRef::parse(args) {
                    Ok(model_ref) => Command::ModelSwitch { model_ref },
                    Err(e) => Command::ModelInvalid {
                        raw_args: args.to_string(),
                        error: e.to_string(),
                    },
                }
            }
        }
        "status" => Command::Status,
        "help" => Command::Help,
        "cancel" => Command::Cancel,
        "jobs" => Command::Jobs,
        "job" => Command::Job {
            job_id: args.to_string(),
        },
        "cancel-job" => Command::CancelJob {
            job_id: args.to_string(),
        },
        _ => Command::Unknown {
            name: command_name.to_string(),
        },
    };

    Some(command)
}

pub fn is_command(input: &str) -> bool {
    parse_command(input).is_some()
}

pub fn is_cancel_command(input: &str) -> bool {
    matches!(parse_command(input), Some(Command::Cancel))
}

impl Command {
    pub fn policy(&self) -> CommandPolicy {
        match self {
            Command::Help
            | Command::Status
            | Command::Jobs
            | Command::Job { .. }
            | Command::ModelShow
            | Command::ModelInvalid { .. }
            | Command::Unknown { .. } => CommandPolicy::ImmediateRead,
            Command::ModelList { .. } | Command::Compact => CommandPolicy::QueueWhileBusy,
            Command::CancelJob { .. } => CommandPolicy::ImmediateJobControl,
            Command::Clear | Command::ModelSwitch { .. } => CommandPolicy::CancelThenRun,
            Command::Cancel => CommandPolicy::CancelCurrentTurn,
        }
    }
}

pub async fn execute_immediate_command(
    command: &Command,
    agent: &Agent,
    hot: &HotState,
    session_key: Option<&str>,
) -> String {
    match command {
        Command::ModelShow => render_model_show(hot),
        Command::ModelList { provider_id } => {
            format!(
                "🕒 /model {provider_id} list queued and will run after the current turn finishes"
            )
        }
        Command::ModelInvalid { raw_args, error } => format!(
            "❌ Invalid model format: {error}\n🧭 Usage\n  • {}\n  • {}\n  • received: {raw_args}",
            model_usage_line_1(),
            model_usage_line_2(),
        ),
        Command::Status => render_status(agent, hot, session_key).await,
        Command::Jobs => render_jobs(hot, session_key).await,
        Command::Job { job_id } => render_job(hot, job_id, session_key).await,
        Command::CancelJob { job_id } => render_cancel_job(hot, job_id, session_key).await,
        Command::Help => render_help(),
        Command::Unknown { name } => {
            format!("❓ Unknown command: /{name}\n💡 Type /help for available commands")
        }
        Command::Cancel => "⏹️ No running turn to cancel".to_string(),
        Command::Compact => {
            "🕒 /compact queued and will run after the current turn finishes".to_string()
        }
        Command::Clear | Command::ModelSwitch { .. } => {
            "⏳ Command requires the current turn to be cancelled first".to_string()
        }
    }
}

pub async fn execute_stateful_command(
    command: &Command,
    agent: &mut Agent,
    hot: &mut HotState,
    _ctx: &RunContext,
    session_key: Option<&str>,
) -> String {
    match command {
        Command::Clear => {
            agent.history.clear();
            agent.total_usage = Usage::default();
            runner::refresh_prompt(agent, hot, None);
            "🧹 Context cleared".to_string()
        }
        Command::Compact => {
            let memory_dir = crate::config::resolve_path(crate::config::MEMORY_DIR);
            match agent.compress_with_llm(&memory_dir).await {
                Ok(()) => "🗜️ Context compressed with LLM".to_string(),
                Err(e) => format!("❌ Compression failed: {e}"),
            }
        }
        Command::ModelShow => render_model_show(hot),
        Command::ModelList { provider_id } => render_model_list(hot, provider_id).await,
        Command::ModelSwitch { model_ref } => {
            match hot.registry.get_or_create(&model_ref.provider_id) {
                Ok(provider) => {
                    hot.active_provider = provider;
                    hot.chat_opts.model = model_ref.model_name.clone();
                    hot.active_model_ref = model_ref.clone();
                    format!("✅ Model switched to: {}", hot.active_model_ref)
                }
                Err(e) => format!("❌ Failed to switch provider: {e}"),
            }
        }
        Command::ModelInvalid { raw_args, error } => format!(
            "❌ Invalid model format: {error}\n🧭 Usage\n  • {}\n  • {}\n  • received: {raw_args}",
            model_usage_line_1(),
            model_usage_line_2(),
        ),
        Command::Status => render_status(agent, hot, session_key).await,
        Command::Help => render_help(),
        Command::Cancel => "⏹️ No running turn to cancel".to_string(),
        Command::Jobs => render_jobs(hot, session_key).await,
        Command::Job { job_id } => render_job(hot, job_id, session_key).await,
        Command::CancelJob { job_id } => render_cancel_job(hot, job_id, session_key).await,
        Command::Unknown { name } => {
            format!("❓ Unknown command: /{name}\n💡 Type /help for available commands")
        }
    }
}

pub async fn try_handle_command(
    input: &str,
    agent: &mut Agent,
    hot: &mut HotState,
    ctx: &RunContext,
    session_key: Option<&str>,
) -> CommandResult {
    let Some(command) = parse_command(input) else {
        return CommandResult::NotACommand;
    };

    let response = match command.policy() {
        CommandPolicy::ImmediateRead | CommandPolicy::ImmediateJobControl => {
            execute_immediate_command(&command, agent, hot, session_key).await
        }
        CommandPolicy::QueueWhileBusy
        | CommandPolicy::CancelThenRun
        | CommandPolicy::CancelCurrentTurn => {
            execute_stateful_command(&command, agent, hot, ctx, session_key).await
        }
    };

    CommandResult::Handled(response)
}

async fn render_model_list(hot: &mut HotState, provider_id: &str) -> String {
    tracing::info!("Model list requested for provider: {provider_id}");
    match hot.registry.get_or_create(provider_id) {
        Ok(provider) => {
            match provider.list_models().await {
                Ok(mut models) => {
                    models.sort();
                    models.dedup();
                    if models.is_empty() {
                        format!("📭 Provider '{provider_id}' returned no models from listmodel interface.")
                    } else {
                        let mut lines = vec![format!(
                            "📚 Supported models for '{provider_id}' ({count})",
                            count = models.len()
                        )];
                        lines.extend(models.into_iter().map(|model| format!("  • {model}")));
                        lines.join("\n")
                    }
                }
                Err(e) => {
                    let message = e.to_string();
                    if is_list_models_unsupported_error(&message) {
                        format!("⚠️ Provider '{provider_id}' does not support listmodel interface.")
                    } else {
                        format!("❌ Failed to list models for '{provider_id}': {message}")
                    }
                }
            }
        }
        Err(e) => format!("❌ Failed to switch provider: {e}"),
    }
}

fn render_model_show(hot: &HotState) -> String {
    let mut lines = vec![
        "🤖 Current model".to_string(),
        format!("  • {}", hot.active_model_ref),
        String::new(),
        "🔌 Available providers".to_string(),
    ];
    for pid in hot.registry.list_provider_ids() {
        lines.push(format!("  • {pid}"));
    }
    lines.push(String::new());
    lines.push("🧭 Usage".to_string());
    lines.push(format!("  • {}", model_usage_line_1()));
    lines.push(format!("  • {}", model_usage_line_2()));
    lines.join("\n")
}

async fn render_status(agent: &Agent, hot: &HotState, session_key: Option<&str>) -> String {
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
    let provider_name = &hot.active_model_ref.provider_id;
    let model = &hot.active_model_ref.model_name;
    let temp = hot
        .chat_opts
        .temperature
        .map_or("auto".to_string(), |v| format!("{v}"));
    let top_p = hot
        .chat_opts
        .top_p
        .map_or("auto".to_string(), |v| format!("{v}"));
    let running_jobs = if let Some(manager) = &hot.job_manager {
        manager
            .list_jobs(session_key)
            .await
            .into_iter()
            .filter(|job| {
                matches!(
                    job.status,
                    crate::ptc::job_manager::PtcJobStatus::Queued
                        | crate::ptc::job_manager::PtcJobStatus::Running
                )
            })
            .count()
    } else {
        0
    };

    let has_assistant = agent
        .history
        .iter()
        .any(|m| matches!(m.role, Role::Assistant));
    let usage_warning = total == 0 && has_assistant;

    const W: usize = 44;
    let hl = format!("├{}┤", "─".repeat(W + 2));
    let cell = |s: &str| -> String { format!("│ {:width$} │", s, width = W) };
    let row = |label: &str, value: &str| -> String { cell(&format!("  {label:<9}: {value}")) };

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
    rows.push(row("PTC Jobs", &format!("{running_jobs} running")));
    rows.push(hl.clone());
    rows.push(cell("Tokens"));
    rows.push(hl.clone());
    rows.push(row("Input", &fmt_thousands(input_tokens)));
    rows.push(row("Output", &fmt_thousands(output_tokens)));
    if usage_warning {
        rows.push(row("", "⚠  provider may not report usage"));
    }
    rows.push(format!("└{}┘", "─".repeat(W + 2)));

    format!("📊 Session status\n```\n{}\n```", rows.join("\n"))
}

async fn render_jobs(hot: &HotState, session_key: Option<&str>) -> String {
    let Some(manager) = &hot.job_manager else {
        return "PTC runtime unavailable".to_string();
    };
    let jobs = manager.list_jobs(session_key).await;
    if jobs.is_empty() {
        "No PTC jobs".to_string()
    } else {
        let mut lines = vec!["PTC jobs".to_string()];
        for job in jobs {
            lines.push(format!(
                "- {} [{}] {}",
                job.job_id,
                format_job_status(&job.status),
                job.purpose
            ));
        }
        lines.join("\n")
    }
}

async fn render_job(hot: &HotState, job_id: &str, session_key: Option<&str>) -> String {
    let Some(manager) = &hot.job_manager else {
        return "PTC runtime unavailable".to_string();
    };
    let job_id = job_id.trim();
    if job_id.is_empty() {
        "Usage: /job <id>".to_string()
    } else if let Some(job) = manager.get_job(job_id).await {
        if session_key.is_some_and(|key| job.session_key != key) {
            return format!("PTC job not found: {job_id}");
        }
        [
            format!("job_id: {}", job.job_id),
            format!("status: {}", format_job_status(&job.status)),
            format!("purpose: {}", job.purpose),
            format!("session: {}", job.session_key),
            format!("pid: {:?}", job.pid),
            format!("artifact_dir: {}", job.artifact_dir.display()),
            format!("out: {}", job.out_path.display()),
            format!("log: {}", job.log_path.display()),
        ]
        .join("\n")
    } else {
        format!("PTC job not found: {job_id}")
    }
}

async fn render_cancel_job(hot: &HotState, job_id: &str, session_key: Option<&str>) -> String {
    let Some(manager) = &hot.job_manager else {
        return "PTC runtime unavailable".to_string();
    };
    let job_id = job_id.trim();
    if job_id.is_empty() {
        "Usage: /cancel-job <id>".to_string()
    } else {
        if let Some(job) = manager.get_job(job_id).await {
            if session_key.is_some_and(|key| job.session_key != key) {
                return format!("PTC job not running or not found: {job_id}");
            }
        } else {
            return format!("PTC job not running or not found: {job_id}");
        }
        match manager.cancel_job(job_id).await {
            Ok(true) => format!("PTC job cancelled: {job_id}"),
            Ok(false) => format!("PTC job not running or not found: {job_id}"),
            Err(e) => format!("Failed to cancel PTC job {job_id}: {e}"),
        }
    }
}

fn render_help() -> String {
    let mut lines = vec!["🧭 Available commands".to_string()];
    lines.extend(
        COMMANDS
            .iter()
            .map(|c| format!("  • /{:<9} - {}", c.name, c.description)),
    );
    lines.join("\n")
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

fn format_job_status(status: &crate::ptc::job_manager::PtcJobStatus) -> &'static str {
    match status {
        crate::ptc::job_manager::PtcJobStatus::Queued => "queued",
        crate::ptc::job_manager::PtcJobStatus::Running => "running",
        crate::ptc::job_manager::PtcJobStatus::Succeeded => "succeeded",
        crate::ptc::job_manager::PtcJobStatus::Failed => "failed",
        crate::ptc::job_manager::PtcJobStatus::TimedOut => "timed_out",
        crate::ptc::job_manager::PtcJobStatus::Cancelled => "cancelled",
    }
}

fn parse_model_list_command_args(args: &str) -> Option<&str> {
    let mut parts = args.split_whitespace();
    let provider_id = parts.next()?;
    let action = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if action.eq_ignore_ascii_case("list") {
        Some(provider_id)
    } else {
        None
    }
}

fn model_usage_line_1() -> &'static str {
    "/model <provider_id>@<model_name>"
}

fn model_usage_line_2() -> &'static str {
    "/model <provider_id> list"
}

fn is_list_models_unsupported_error(message: &str) -> bool {
    if message.contains(LIST_MODELS_UNSUPPORTED) {
        return true;
    }
    let normalized = message.to_ascii_lowercase();
    normalized.contains("(404")
        || normalized.contains("(405")
        || normalized.contains("not found")
        || normalized.contains("method not allowed")
}

#[cfg(test)]
mod tests {
    use super::{parse_command, Command, CommandPolicy};

    #[test]
    fn parse_model_switch_command() {
        let command = parse_command("/model openai@gpt-4o").unwrap();
        match command {
            Command::ModelSwitch { model_ref } => {
                assert_eq!(model_ref.provider_id, "openai");
                assert_eq!(model_ref.model_name, "gpt-4o");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parse_model_show_is_immediate_read() {
        let command = parse_command("/model").unwrap();
        assert!(matches!(command, Command::ModelShow));
        assert_eq!(command.policy(), CommandPolicy::ImmediateRead);
    }

    #[test]
    fn parse_compact_is_queued_while_busy() {
        let command = parse_command("/compact").unwrap();
        assert!(matches!(command, Command::Compact));
        assert_eq!(command.policy(), CommandPolicy::QueueWhileBusy);
    }

    #[test]
    fn parse_model_list_is_queued_while_busy() {
        let command = parse_command("/model openai list").unwrap();
        assert!(matches!(command, Command::ModelList { .. }));
        assert_eq!(command.policy(), CommandPolicy::QueueWhileBusy);
    }

    #[test]
    fn parse_clear_is_cancel_then_run() {
        let command = parse_command("/clear").unwrap();
        assert!(matches!(command, Command::Clear));
        assert_eq!(command.policy(), CommandPolicy::CancelThenRun);
    }

    #[test]
    fn parse_invalid_model_usage_stays_immediate() {
        let command = parse_command("/model invalid").unwrap();
        assert!(matches!(command, Command::ModelInvalid { .. }));
        assert_eq!(command.policy(), CommandPolicy::ImmediateRead);
    }
}
