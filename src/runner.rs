use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;
use uuid::Uuid;

use crate::agent;
use crate::channels::{self, Channel};
use crate::commands;
use crate::config::{self, Config, ModelRef};
use crate::identity;
use crate::memory::Memory;
use crate::prompt::build_system_prompt;
use crate::providers::{self, ChatOptions, ContentPart, ProviderRegistry};
use crate::rich_content;
use crate::skills::{self, Skill};
use crate::stt::SttProvider;
use crate::tools;
use crate::tools::reload::ReloadSignal;

pub struct RunContext<'a> {
    pub mem: &'a Memory,
    pub config_path: Option<PathBuf>,
    pub reload_signal: ReloadSignal,
    pub stt_provider: Option<Arc<dyn SttProvider>>,
}

pub struct HotState {
    pub tools: Vec<Box<dyn tools::Tool>>,
    pub todo: tools::todo::TodoHandle,
    pub identity_text: Option<String>,
    pub skills: Vec<skills::Skill>,
    pub shared_skills: Arc<RwLock<Vec<skills::Skill>>>,
    pub skill_cache: Arc<RwLock<HashMap<String, String>>>,
    pub cfg: Config,
    pub builtin_count: usize,
    pub chat_opts: ChatOptions,
    pub compression_provider: (Arc<dyn providers::Provider>, ChatOptions),
    pub registry: ProviderRegistry,
    pub active_provider: Arc<dyn providers::Provider>,
    pub active_model_ref: ModelRef,
}

pub(crate) fn refresh_prompt(
    agent: &mut agent::Agent,
    hot: &HotState,
    channel_supplement: Option<&str>,
) {
    let system_prompt = build_system_prompt(hot.identity_text.as_deref(), channel_supplement);
    agent.set_system_prompt(system_prompt);
}

fn build_user_message(
    content: String,
    content_parts: Option<Vec<ContentPart>>,
    vision_enabled: bool,
    skills_list: &[Skill],
    user_context: Option<&str>,
    conversation_summary: Option<&str>,
    todo_reminder: Option<String>,
) -> providers::ConversationMessage {
    let mut parts: Vec<ContentPart> = Vec::new();

    let skills_idx = skills::skills_index(skills_list);
    if !skills_idx.is_empty() {
        parts.push(ContentPart::Text(skills_idx));
    }

    if let Some(reminder) = todo_reminder {
        parts.push(ContentPart::Text(reminder));
    }

    if let Some(ctx) = user_context {
        if !ctx.trim().is_empty() {
            parts.push(ContentPart::Text(format!(
                "<user-context>\n{ctx}\n</user-context>"
            )));
        }
    }

    if let Some(summary) = conversation_summary {
        parts.push(ContentPart::Text(format!(
            "<conversation-summary>\n{summary}\n</conversation-summary>"
        )));
    }

    let now = chrono::Local::now();
    parts.push(ContentPart::Text(format!(
        "current_time: {}",
        now.format("%Y-%m-%d %H:%M:%S")
    )));

    if let Some(cp) = content_parts {
        let has_image = cp.iter().any(|p| {
            matches!(
                p,
                ContentPart::ImageUrl { .. } | ContentPart::ImageBase64 { .. }
            )
        });

        if has_image && !vision_enabled {
            let caption = cp
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text(t) if !t.trim().is_empty() => Some(t.trim()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let text = if caption.is_empty() {
                "[system: The user sent an image, but the current model has no vision capability and cannot view image content. Clearly inform the user that image viewing is unavailable and ask for a text description or extracted text from the image.]".to_string()
            } else {
                format!(
                    "[system: The user sent an image, but the current model has no vision capability and cannot view image content. Clearly inform the user that image viewing is unavailable and ask for a text description or extracted text from the image.]\n[image_caption]: {caption}"
                )
            };
            parts.push(ContentPart::Text(text));
        } else {
            parts.extend(cp);
        }
    } else {
        parts.push(ContentPart::Text(content));
    }

    providers::ConversationMessage::user_parts(parts)
}

pub(crate) fn prepare_user_turn(
    agent: &mut agent::Agent,
    hot: &HotState,
    mem: &Memory,
    content: String,
    content_parts: Option<Vec<ContentPart>>,
    channel_supplement: Option<&str>,
) {
    let user_ctx = mem.load_user_context();
    let summary = agent.take_conversation_summary();
    let todo_reminder = hot.todo.pending_reminder();
    let user_msg = build_user_message(
        content,
        content_parts,
        hot.cfg.agent.primary_model.vision,
        &hot.skills,
        user_ctx.as_deref(),
        summary.as_deref(),
        todo_reminder,
    );
    agent.history.push(user_msg);
    refresh_prompt(agent, hot, channel_supplement);
}

enum BudgetStatus {
    Ok,
    Exhausted,
}

async fn post_turn(agent: &mut agent::Agent, hot: &HotState) -> BudgetStatus {
    if hot.cfg.agent.show_usage {
        let msg = format!(
            "tokens: in={}, out={}, total={}",
            agent.total_usage.input_tokens,
            agent.total_usage.output_tokens,
            agent.total_usage.input_tokens + agent.total_usage.output_tokens,
        );
        tracing::info!("{msg}");
    }

    if let Some(budget) = hot.cfg.agent.max_budget_tokens {
        let total = agent.total_usage.input_tokens + agent.total_usage.output_tokens;
        if total >= budget {
            return BudgetStatus::Exhausted;
        }
    }

    if let Err(e) = agent
        .auto_compact(&config::resolve_path(config::MEMORY_DIR))
        .await
    {
        tracing::warn!("Auto-compact failed: {e}");
    }
    BudgetStatus::Ok
}

fn retry_config_equal(a: &config::RetryConfig, b: &config::RetryConfig) -> bool {
    a.max_retries == b.max_retries
        && a.base_delay_ms == b.base_delay_ms
        && a.max_delay_ms == b.max_delay_ms
        && a.connect_timeout_secs == b.connect_timeout_secs
        && a.request_timeout_secs == b.request_timeout_secs
}

fn provider_endpoints_equal(
    a: &[config::ProviderEndpoint],
    b: &[config::ProviderEndpoint],
) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).all(|(x, y)| {
        x.id == y.id
            && x.kind == y.kind
            && x.api_key == y.api_key
            && x.base_url == y.base_url
            && x.extra_headers == y.extra_headers
            && retry_config_equal(&x.retry, &y.retry)
    })
}

fn agent_model_fields_equal(a: &config::AgentConfig, b: &config::AgentConfig) -> bool {
    a.primary_model == b.primary_model && a.fast_model == b.fast_model
}

fn channel_config_equal(a: &config::ChannelConfig, b: &config::ChannelConfig) -> bool {
    a.name == b.name && a.params == b.params
}

fn channels_equal(a: &[config::ChannelConfig], b: &[config::ChannelConfig]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| channel_config_equal(x, y))
}

fn tts_config_equal(a: &config::TtsConfig, b: &config::TtsConfig) -> bool {
    a.provider == b.provider
        && a.api_key == b.api_key
        && a.model == b.model
        && a.voice_id == b.voice_id
}

fn stt_config_equal(a: &config::SttConfig, b: &config::SttConfig) -> bool {
    a.provider == b.provider && a.api_key == b.api_key && a.model == b.model
}

fn log_config_equal(a: &config::LogConfig, b: &config::LogConfig) -> bool {
    a.level == b.level
}

fn light_reload_blockers(old_cfg: &Config, new_cfg: &Config) -> Vec<String> {
    let mut blockers = Vec::new();

    if !provider_endpoints_equal(&old_cfg.providers, &new_cfg.providers) {
        blockers.push("providers.*".to_string());
    }
    if !agent_model_fields_equal(&old_cfg.agent, &new_cfg.agent) {
        blockers.push("agent.model/sampling".to_string());
    }
    if !channels_equal(&old_cfg.channels, &new_cfg.channels) {
        blockers.push("channels".to_string());
    }
    if !tts_config_equal(&old_cfg.tts, &new_cfg.tts) {
        blockers.push("tts.*".to_string());
    }
    if !stt_config_equal(&old_cfg.stt, &new_cfg.stt) {
        blockers.push("stt.*".to_string());
    }
    if !log_config_equal(&old_cfg.log, &new_cfg.log) {
        blockers.push("log.level".to_string());
    }

    if old_cfg.agent.max_iterations != new_cfg.agent.max_iterations {
        blockers.push("agent.max_iterations".to_string());
    }
    if old_cfg.agent.max_history != new_cfg.agent.max_history {
        blockers.push("agent.max_history".to_string());
    }
    if old_cfg.agent.max_tool_output_chars != new_cfg.agent.max_tool_output_chars {
        blockers.push("agent.max_tool_output_chars".to_string());
    }
    if old_cfg.agent.max_memory_tokens != new_cfg.agent.max_memory_tokens {
        blockers.push("agent.max_memory_tokens".to_string());
    }
    if old_cfg.agent.show_usage != new_cfg.agent.show_usage {
        blockers.push("agent.show_usage".to_string());
    }
    if old_cfg.agent.max_budget_tokens != new_cfg.agent.max_budget_tokens {
        blockers.push("agent.max_budget_tokens".to_string());
    }
    if old_cfg.agent.max_memory_file_size != new_cfg.agent.max_memory_file_size {
        blockers.push("agent.max_memory_file_size".to_string());
    }
    if old_cfg.agent.session_cleanup_days != new_cfg.agent.session_cleanup_days {
        blockers.push("agent.session_cleanup_days".to_string());
    }
    if old_cfg.agent.tool_timeout != new_cfg.agent.tool_timeout {
        blockers.push("agent.tool_timeout".to_string());
    }

    blockers
}

async fn perform_light_reload(
    agent: &mut agent::Agent,
    ctx: &RunContext<'_>,
    hot: &mut HotState,
    channel_supplement: Option<&str>,
) {
    tracing::info!("Performing light reload...");

    match config::load_config(ctx.config_path.as_deref()) {
        Ok(new_cfg) => {
            let blockers = light_reload_blockers(&hot.cfg, &new_cfg);
            if !blockers.is_empty() {
                let details = blockers.join(", ");
                let msg = format!(
                    "[System] Light reload rejected. These config changes require full reload: {details}"
                );
                agent.push_user_message(msg);
                tracing::warn!("Light reload rejected due to full-reload-only changes: {details}");
                return;
            }

            hot.tools.truncate(hot.builtin_count);

            let mcp_tools = tools::mcp::connect_mcp_servers(&new_cfg.mcp).await;
            let mcp_count = mcp_tools.len();
            hot.tools.extend(mcp_tools);

            let skills_dir = config::resolve_path(config::MEMORY_DIR).join("skills");
            hot.skills = skills::load_skills(&skills_dir);
            *hot.shared_skills.write().await = hot.skills.clone();
            hot.skill_cache.write().await.clear();
            let skill_count = hot.skills.len();

            let identity_path = config::resolve_path(&new_cfg.agent.identity_path);
            hot.identity_text = if identity_path.exists() {
                match identity::load_identity(&identity_path) {
                    Ok(text) => Some(text),
                    Err(e) => {
                        tracing::warn!("Failed to reload identity: {e}");
                        hot.identity_text.take()
                    }
                }
            } else {
                None
            };

            hot.cfg.mcp = new_cfg.mcp.clone();
            hot.cfg.agent.identity_path = new_cfg.agent.identity_path.clone();

            refresh_prompt(agent, hot, channel_supplement);

            let msg = format!(
                "[System] Light reload completed successfully. \
                 {mcp_count} MCP server(s), {skill_count} skill(s) loaded."
            );
            agent.push_user_message(msg);

            tracing::info!("Light reload completed successfully");
        }
        Err(e) => {
            let msg = format!("[System] Light reload failed: {e}");
            agent.push_user_message(msg);
            tracing::error!("Light reload failed (config error): {e}");
        }
    }
}

async fn check_reload(
    agent: &mut agent::Agent,
    ctx: &RunContext<'_>,
    hot: &mut HotState,
    channel_supplement: Option<&str>,
) {
    if ctx.reload_signal.take().is_some() {
        perform_light_reload(agent, ctx, hot, channel_supplement).await;
    }
    refresh_skills(hot).await;
}

async fn refresh_skills(hot: &mut HotState) {
    let skills_dir = config::resolve_path(config::MEMORY_DIR).join("skills");
    hot.skills = skills::load_skills(&skills_dir);
    *hot.shared_skills.write().await = hot.skills.clone();
    hot.skill_cache.write().await.clear();
}

fn create_channel_registry(
    cfg: &Config,
    stt_provider: Option<Arc<dyn SttProvider>>,
) -> std::collections::HashMap<String, Arc<dyn Channel>> {
    channels::create_channel_registry(cfg, stt_provider)
}

pub async fn run_interactive(
    agent: &mut agent::Agent,
    ctx: &RunContext<'_>,
    hot: &mut HotState,
    sessions_dir: &std::path::Path,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<channels::ChannelMessage>(32);
    let cli_channel = channels::cli::CliChannel;
    let mut pending: Vec<channels::ChannelMessage> = Vec::new();

    tokio::spawn(async move {
        if let Err(e) = cli_channel.listen(tx).await {
            tracing::error!("CLI listener error: {e}");
        }
    });

    print!("zerda> ");
    if let Err(e) = std::io::stdout().flush() {
        tracing::debug!("Failed to flush prompt: {e}");
    }

    loop {
        let msg = if let Some(idx) = pending.iter().position(|_| true) {
            pending.remove(idx)
        } else {
            match rx.recv().await {
                Some(m) => m,
                None => break,
            }
        };

        match commands::try_handle_command(&msg.content, agent, hot, ctx).await {
            commands::CommandResult::Handled(feedback) => {
                eprintln!("{feedback}");
                print!("zerda> ");
                if let Err(e) = std::io::stdout().flush() {
                    tracing::debug!("Failed to flush prompt: {e}");
                }
                continue;
            }
            commands::CommandResult::NotACommand => {}
        }
        let snapshot = agent.snapshot_turn();
        prepare_user_turn(agent, hot, ctx.mem, msg.content, msg.content_parts, None);

        let mut cancelled = false;
        let mut run_result = None;
        {
            let mut rx_closed = false;
            let run_turn = agent.run_turn_stream(
                hot.active_provider.as_ref(),
                &hot.tools,
                &hot.chat_opts,
                |delta| {
                    let safe = channels::cli::sanitize_terminal_text(delta);
                    print!("{safe}");
                    if let Err(e) = std::io::stdout().flush() {
                        tracing::debug!("Failed to flush stream output: {e}");
                    }
                },
                |_, _| {},
            );
            tokio::pin!(run_turn);

            loop {
                tokio::select! {
                    result = &mut run_turn => {
                        run_result = Some(result);
                        break;
                    }
                    incoming = rx.recv(), if !rx_closed => {
                        match incoming {
                            Some(next) => {
                                if commands::is_cancel_command(&next.content) {
                                    cancelled = true;
                                    tracing::info!("Interactive turn cancel requested by user");
                                eprintln!("\nCurrent turn cancelled\n");
                                    break;
                                }
                                pending.push(next);
                            }
                            None => rx_closed = true,
                        }
                    }
                }
            }
        }

        if cancelled {
            agent.restore_turn(snapshot);
            tracing::info!("Interactive turn cancelled and rolled back");
            print!("zerda> ");
            if let Err(e) = std::io::stdout().flush() {
                tracing::debug!("Failed to flush prompt: {e}");
            }
            continue;
        }

        if let Some(result) = run_result {
            match result {
                Ok(_) => println!("\n"),
                Err(e) => eprintln!("\nError: {e}\n"),
            }
        } else {
            agent.restore_turn(snapshot);
            eprintln!("\nError: turn did not complete.\n");
            print!("zerda> ");
            if let Err(e) = std::io::stdout().flush() {
                tracing::debug!("Failed to flush prompt: {e}");
            }
            continue;
        }

        if hot.cfg.agent.show_usage {
            eprintln!(
                "[tokens: in={}, out={}, total={}]",
                agent.total_usage.input_tokens,
                agent.total_usage.output_tokens,
                agent.total_usage.input_tokens + agent.total_usage.output_tokens,
            );
        }

        check_reload(agent, ctx, hot, None).await;

        if matches!(post_turn(agent, hot).await, BudgetStatus::Exhausted) {
            eprintln!("Token budget exhausted. Ending session.");
            break;
        }

        if let Err(e) = agent.save_session(sessions_dir, Some("latest")) {
            tracing::debug!("Failed to save session: {e}");
        }

        print!("zerda> ");
        if let Err(e) = std::io::stdout().flush() {
            tracing::debug!("Failed to flush prompt: {e}");
        }
    }

    Ok(())
}

pub async fn run_serve(
    ctx: &RunContext<'_>,
    hot: &mut HotState,
    sessions_dir: &std::path::Path,
) -> Result<()> {
    let channels = create_channel_registry(&hot.cfg, ctx.stt_provider.clone());

    if channels.is_empty() {
        anyhow::bail!("No channels configured. Add channel entries to your config file.");
    }

    let (tx, mut rx) = mpsc::channel::<channels::ChannelMessage>(32);

    for (name, ch) in &channels {
        let ch = Arc::clone(ch);
        let ch_tx = tx.clone();
        tracing::info!("Starting channel: {name}");
        tokio::spawn(async move {
            if let Err(e) = ch.listen(ch_tx).await {
                tracing::error!("Channel listener error: {e}");
            }
        });
    }

    drop(tx);

    tracing::info!("Serving {} channel(s)...", channels.len());
    let mut pending: Vec<channels::ChannelMessage> = Vec::new();
    let mut session_agents: HashMap<String, agent::Agent> = HashMap::new();
    let mut session_recipients: HashMap<String, String> = HashMap::new();

    let memory_dir = config::resolve_path(config::MEMORY_DIR);
    let reload_marker = memory_dir.join(".reload-pending");
    let reply_ctx_path = memory_dir.join(".reply-context");
    if reload_marker.exists() {
        if let Err(e) = std::fs::remove_file(&reload_marker) {
            tracing::warn!(
                "Failed to remove reload marker {}: {e}",
                reload_marker.display()
            );
        }
        if let Some(target) = load_reply_target(&reply_ctx_path) {
            match target {
                ReplyTarget::Unicast {
                    channel,
                    sender,
                    session_id,
                } => {
                    let session_key = format!("{channel}:{session_id}");
                    let storage_id = format!("{channel}-{}", hex::encode(session_id.as_bytes()));
                    let mut notify_agent =
                        if let Some(existing) = session_agents.remove(&session_key) {
                            existing
                        } else {
                            let mut fresh = agent::Agent::new(
                                hot.cfg.agent.clone(),
                                (
                                    Arc::clone(&hot.compression_provider.0),
                                    hot.compression_provider.1.clone(),
                                ),
                            );
                            if let Ok((loaded_id, history)) =
                                agent::Agent::load_session(sessions_dir, Some(&storage_id))
                            {
                                tracing::info!("Resumed session {loaded_id} for {session_key}");
                                fresh.history = history;
                            }
                            fresh
                        };
                    let mcp_count = hot.cfg.mcp.len();
                    let notify_msg = format!(
                        "[System] Configuration reloaded successfully. {mcp_count} MCP server(s) configured. \
                         Briefly inform the user that the reload completed and services are back online."
                    );
                    notify_agent.push_user_message(notify_msg);
                    let supplement = channels.get(&channel).and_then(|ch| ch.prompt_supplement());
                    refresh_prompt(&mut notify_agent, hot, supplement.as_deref());

                    let response = match notify_agent
                        .run_turn(hot.active_provider.as_ref(), &hot.tools, &hot.chat_opts)
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("Failed to generate reload notification: {e}");
                            "Configuration reloaded successfully.".to_string()
                        }
                    };

                    if let Some(ch) = channels.get(&channel) {
                        if let Err(e) = ch.send(&response, &sender).await {
                            tracing::warn!("Failed to send reload notification: {e}");
                        }
                    } else {
                        tracing::info!("Reload notification: {response}");
                    }
                    if let Err(e) = notify_agent.save_session(sessions_dir, Some(&storage_id)) {
                        tracing::warn!("Failed to save session {storage_id}: {e}");
                    }
                    session_recipients.insert(session_key.clone(), sender);
                    session_agents.insert(session_key, notify_agent);
                }
            }
        }
    }

    loop {
        let msg = if let Some(idx) = pending.iter().position(|_| true) {
            pending.remove(idx)
        } else {
            match rx.recv().await {
                Some(m) => m,
                None => break,
            }
        };

        let mut content = msg.content;
        let mut content_parts = msg.content_parts;
        let sender = msg.sender;
        let session_id = msg.session_id;
        let channel_name = msg.channel;
        let turn_id = Uuid::new_v4().to_string();
        let turn_started = Instant::now();
        let mut queued_cancel = false;

        while let Ok(next) = rx.try_recv() {
            if next.session_id == session_id
                && next.channel == channel_name
                && commands::is_cancel_command(&next.content)
            {
                queued_cancel = true;
                continue;
            }
            if next.session_id == session_id && next.channel == channel_name {
                if commands::is_command(&next.content) {
                    pending.push(next);
                } else {
                    content.push('\n');
                    content.push_str(&next.content);
                    if let Some(mut extra) = next.content_parts {
                        content_parts
                            .get_or_insert_with(Vec::new)
                            .append(&mut extra);
                    }
                }
            } else {
                pending.push(next);
            }
        }

        let turn_span = tracing::info_span!(
            "turn",
            turn_id = %turn_id,
            channel = %channel_name,
            session_id = %session_id,
            sender = %sender
        );
        tracing::info!(
            parent: &turn_span,
            content_chars = content.chars().count(),
            has_content_parts = content_parts.as_ref().is_some_and(|parts| !parts.is_empty()),
            "Turn start"
        );

        let memory_dir = config::resolve_path(config::MEMORY_DIR);
        let reply_target = ReplyTarget::Unicast {
            channel: channel_name.clone(),
            sender: sender.clone(),
            session_id: session_id.clone(),
        };
        if let Err(e) = save_reply_target(&memory_dir.join(".reply-context"), &reply_target) {
            tracing::warn!("Failed to save reply context: {e}");
        }

        let ch = channels.get(&channel_name).cloned();
        let session_key = format!("{channel_name}:{session_id}");
        let storage_id = format!("{channel_name}-{}", hex::encode(session_id.as_bytes()));
        session_recipients.insert(session_key.clone(), sender.clone());

        let mut session_agent = if let Some(existing) = session_agents.remove(&session_key) {
            existing
        } else {
            let mut fresh = agent::Agent::new(
                hot.cfg.agent.clone(),
                (
                    Arc::clone(&hot.compression_provider.0),
                    hot.compression_provider.1.clone(),
                ),
            );
            if let Ok((loaded_id, history)) =
                agent::Agent::load_session(sessions_dir, Some(&storage_id))
            {
                tracing::info!("Resumed session {loaded_id} for {session_key}");
                fresh.history = history;
            }
            fresh
        };

        match commands::try_handle_command(&content, &mut session_agent, hot, ctx).await {
            commands::CommandResult::Handled(feedback) => {
                if let Some(ref ch) = ch {
                    if let Err(e) = ch.send(&feedback, &sender).await {
                        tracing::warn!("Failed to send command feedback: {e}");
                    }
                }
                if !session_agent.history.is_empty() {
                    if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
                        tracing::warn!("Failed to save session {storage_id}: {e}");
                    }
                }
                tracing::info!(
                    parent: &turn_span,
                    elapsed_ms = turn_started.elapsed().as_millis(),
                    response_chars = feedback.chars().count(),
                    "Turn done"
                );
                session_agents.insert(session_key, session_agent);
                continue;
            }
            commands::CommandResult::NotACommand => {}
        }

        let supplement = ch.as_ref().and_then(|c| c.prompt_supplement());
        let snapshot = session_agent.snapshot_turn();
        hot.todo.set_session(&session_key);
        prepare_user_turn(
            &mut session_agent,
            hot,
            ctx.mem,
            content,
            content_parts,
            supplement.as_deref(),
        );

        let typing_cancel = CancellationToken::new();
        if let Some(ref ch) = ch {
            let ch = Arc::clone(ch);
            let sender = sender.clone();
            let token = typing_cancel.clone();
            let span = turn_span.clone();
            tokio::spawn(
                async move {
                    loop {
                        if token.is_cancelled() {
                            break;
                        }
                        if let Err(e) = ch.send_typing(&sender).await {
                            tracing::debug!("Failed to send typing event: {e}");
                        }
                        tokio::select! {
                            () = token.cancelled() => break,
                            () = tokio::time::sleep(std::time::Duration::from_secs(4)) => {}
                        }
                    }
                }
                .instrument(span),
            );
        }

        let mut tool_phase_handle = None;
        let mut tool_phase_buffer = None;
        let tool_phase_tx = if let Some(ref ch_ref) = ch {
            let (tx, mut rx) = mpsc::unbounded_channel::<String>();
            let ch = Arc::clone(ch_ref);
            let sender = sender.clone();
            let span = turn_span.clone();
            tool_phase_handle = Some(tokio::spawn(
                async move {
                    while let Some(message) = rx.recv().await {
                        if let Err(e) = ch.send(&message, &sender).await {
                            tracing::warn!("Failed to send tool-phase message: {e}");
                        }
                    }
                }
                .instrument(span),
            ));
            Some(tx)
        } else {
            tool_phase_buffer = Some(Arc::new(std::sync::Mutex::new(Vec::<String>::new())));
            None
        };
        let captured_messages = tool_phase_buffer.as_ref().map(Arc::clone);
        let tool_phase_tx_for_cb = tool_phase_tx.clone();
        let mut cancelled = queued_cancel;
        let mut rx_closed = false;
        let mut response = String::new();
        if !cancelled {
            let run_turn = session_agent
                .run_turn_stream(
                    hot.active_provider.as_ref(),
                    &hot.tools,
                    &hot.chat_opts,
                    |_| {},
                    move |text, has_tool_calls| {
                        if has_tool_calls {
                            if let Some(message) = normalize_tool_phase_message(text) {
                                let chars = message.chars().count();
                                tracing::debug!(chars, "Captured tool-phase assistant content");
                                if let Some(tx) = &tool_phase_tx_for_cb {
                                    if tx.send(message).is_err() {
                                        tracing::warn!(
                                            "Tool-phase sender channel closed before delivery"
                                        );
                                    }
                                } else if let Some(buffer) = &captured_messages {
                                    buffer.lock().unwrap().push(message);
                                }
                            }
                        }
                    },
                )
                .instrument(turn_span.clone());
            tokio::pin!(run_turn);

            loop {
                tokio::select! {
                    result = &mut run_turn => {
                        response = match result {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::error!(channel = %channel_name, sender = %sender, "Turn failed: {e}");
                                format!("Error: {e}")
                            }
                        };
                        break;
                    }
                    incoming = rx.recv(), if !rx_closed => {
                        match incoming {
                            Some(next) => {
                                if next.session_id == session_id
                                    && next.channel == channel_name
                                    && commands::is_cancel_command(&next.content)
                                {
                                    cancelled = true;
                                    tracing::info!(
                                        parent: &turn_span,
                                        "Turn cancel requested by user"
                                    );
                                    break;
                                }
                                pending.push(next);
                            }
                            None => rx_closed = true,
                        }
                    }
                }
            }
        } else {
            tracing::info!(
                parent: &turn_span,
                "Turn cancel requested before execution started"
            );
        }

        typing_cancel.cancel();
        drop(tool_phase_tx);
        if let Some(handle) = tool_phase_handle {
            if let Err(e) = handle.await {
                tracing::warn!("Tool-phase sender task panicked: {e}");
            }
        }

        if cancelled {
            session_agent.restore_turn(snapshot);
            if let Some(ref ch) = ch {
                if let Err(e) = ch.send("Current turn cancelled", &sender).await {
                    tracing::warn!("Failed to send cancel feedback via {}: {e}", channel_name);
                }
            } else {
                println!("Current turn cancelled");
            }
            tracing::info!(
                parent: &turn_span,
                elapsed_ms = turn_started.elapsed().as_millis(),
                "Turn cancelled"
            );
            if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
                tracing::warn!("Failed to save session {storage_id}: {e}");
            }
            session_agents.insert(session_key, session_agent);
            continue;
        }

        if let Some(ref ch) = ch {
            tracing::debug!(
                final_response_chars = response.chars().count(),
                "Sending assistant messages"
            );
            if let Some(final_message) = normalize_final_message(&response) {
                if let Err(e) = send_final_message_streamed(ch, &sender, &final_message).await {
                    tracing::warn!("Failed to send response via {}: {e}", channel_name);
                }
            }
        } else {
            if let Some(buffer) = tool_phase_buffer {
                let captured_tool_phase = {
                    let mut guard = buffer.lock().unwrap();
                    std::mem::take(&mut *guard)
                };
                for message in captured_tool_phase {
                    println!("{}", channels::cli::sanitize_terminal_text(&message));
                }
            }
            if let Some(final_message) = normalize_final_message(&response) {
                println!("{}", channels::cli::sanitize_terminal_text(&final_message));
            }
        }

        tracing::info!(
            parent: &turn_span,
            elapsed_ms = turn_started.elapsed().as_millis(),
            response_chars = response.chars().count(),
            "Turn done"
        );

        check_reload(&mut session_agent, ctx, hot, supplement.as_deref()).await;

        if matches!(
            post_turn(&mut session_agent, hot).await,
            BudgetStatus::Exhausted
        ) {
            tracing::warn!("Token budget exhausted for session {session_key}");
            if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
                tracing::warn!("Failed to save session {storage_id}: {e}");
            }
            session_agents.insert(session_key, session_agent);
            continue;
        }

        if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
            tracing::warn!("Failed to save session {storage_id}: {e}");
        }
        session_agents.insert(session_key, session_agent);
    }

    Ok(())
}

fn normalize_tool_phase_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let no_trailing_ws = trimmed.trim_end();
    let no_colon = no_trailing_ws
        .strip_suffix('：')
        .or_else(|| no_trailing_ws.strip_suffix(':'))
        .unwrap_or(no_trailing_ws)
        .trim_end();
    if no_colon.is_empty() {
        None
    } else {
        Some(no_colon.to_string())
    }
}

fn normalize_final_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn build_stream_prefixes(text: &str, step_chars: usize) -> Vec<String> {
    let step = step_chars.max(1);
    let mut prefixes = Vec::new();
    let mut acc = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        acc.push(ch);
        count += 1;
        if count.is_multiple_of(step) {
            prefixes.push(acc.clone());
        }
    }
    if prefixes.last() != Some(&acc) {
        prefixes.push(acc);
    }
    prefixes
}

async fn send_final_message_streamed(
    channel: &Arc<dyn Channel>,
    recipient: &str,
    message: &str,
) -> Result<()> {
    let clean_text = rich_content::strip_rich_markers(message);
    let markers = rich_content::rich_content_markers(message);

    if clean_text.is_empty() {
        for marker in markers {
            channel.send(&marker, recipient).await?;
        }
        return Ok(());
    }

    if clean_text.chars().count() > config::STREAM_OVERFLOW_CHARS {
        channel.send(&clean_text, recipient).await?;
    } else {
        let prefixes = build_stream_prefixes(&clean_text, 80);
        let mut fallback_plain = true;
        if let Some(first) = prefixes.first() {
            let start_text = format!("{first}▌");
            if let Some(message_id) = channel.send_stream_start(recipient, &start_text).await? {
                fallback_plain = false;
                for prefix in prefixes.iter().skip(1) {
                    let update = format!("{prefix}▌");
                    channel
                        .send_stream_update(recipient, &message_id, &update)
                        .await?;
                }
                channel
                    .send_stream_update(recipient, &message_id, &clean_text)
                    .await?;
            }
        }
        if fallback_plain {
            channel.send(&clean_text, recipient).await?;
        }
    }

    for marker in markers {
        channel.send(&marker, recipient).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum ReplyTarget {
    Unicast {
        channel: String,
        sender: String,
        session_id: String,
    },
}

fn save_reply_target(path: &Path, target: &ReplyTarget) -> Result<()> {
    let data = serde_json::to_vec(target)?;
    std::fs::write(path, data)?;
    Ok(())
}

fn load_reply_target(path: &Path) -> Option<ReplyTarget> {
    let raw = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<ReplyTarget>(&raw) {
        Ok(target) => Some(target),
        Err(e) => {
            tracing::warn!("Invalid reply context at {}: {e}", path.display());
            None
        }
    }
}
