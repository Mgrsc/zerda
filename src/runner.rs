use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;
use uuid::Uuid;

use crate::agent;
use crate::channels::{self, Channel, ChannelMessageOrigin};
use crate::commands;
use crate::config::{self, Config, ModelRef};
use crate::logging::stream_progress_interval_ms;
use crate::memory;
use crate::prompt::build_system_prompt_parts;
use crate::providers::{
    self, ChatOptions, ContentPart, MessageMetadata, MessageOrigin, ProviderRegistry,
};
use crate::ptc::job_manager::{JobManager, PtcSessionContext};
use crate::rich_content;
use crate::stt::SttProvider;

pub struct RunContext {
    pub stt_provider: Option<Arc<dyn SttProvider>>,
    pub memory_service: Option<Arc<memory::MemoryService>>,
}

pub struct HotState {
    pub identity_text: Option<String>,
    pub cfg: Config,
    pub chat_opts: ChatOptions,
    pub compression_provider: (Arc<dyn providers::Provider>, ChatOptions),
    pub registry: ProviderRegistry,
    pub active_provider: Arc<dyn providers::Provider>,
    pub active_model_ref: ModelRef,
    pub job_manager: Option<Arc<JobManager>>,
}

pub(crate) struct PrepareUserTurnInput {
    pub content: String,
    pub content_parts: Option<Vec<ContentPart>>,
    pub channel_supplement: Option<String>,
    pub session_key: Option<String>,
    pub memory_block: Option<String>,
    pub origin: MessageOrigin,
    pub related_job_id: Option<String>,
}

struct PostTurnInput {
    memory_service: Option<Arc<memory::MemoryService>>,
    memory_analyzer: Option<(Arc<dyn providers::Provider>, ChatOptions)>,
    turn_id: String,
    session_id: String,
    entity_id: String,
    channel: Option<String>,
    input_origin: MessageOrigin,
    turn_input_content: Option<String>,
    assistant_response: Option<String>,
}

pub(crate) fn refresh_prompt(
    agent: &mut agent::Agent,
    hot: &HotState,
    channel_supplement: Option<&str>,
) {
    let system_prompt_parts = build_system_prompt_parts(
        &hot.cfg.agent.disabled_primitives,
        hot.identity_text.as_deref(),
        channel_supplement,
    );
    agent.set_system_prompt_parts(system_prompt_parts);
}

fn provider_message_origin(origin: ChannelMessageOrigin) -> MessageOrigin {
    match origin {
        ChannelMessageOrigin::Human => MessageOrigin::Human,
        ChannelMessageOrigin::RuntimePtcResult => MessageOrigin::RuntimePtcResult,
        ChannelMessageOrigin::RuntimePtcNotice => MessageOrigin::RuntimePtcNotice,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_user_message(
    content: String,
    content_parts: Option<Vec<ContentPart>>,
    vision_enabled: bool,
    conversation_summary: Option<&str>,
    runtime_state: Option<&str>,
    memory_block: Option<&str>,
    origin: MessageOrigin,
    related_job_id: Option<String>,
) -> providers::ConversationMessage {
    let mut parts: Vec<ContentPart> = Vec::new();

    if let Some(runtime_state) = runtime_state {
        if !runtime_state.trim().is_empty() {
            parts.push(ContentPart::Text(runtime_state.to_string()));
        }
    }

    if let Some(memory_block) = memory_block {
        if !memory_block.trim().is_empty() {
            parts.push(ContentPart::Text(memory_block.to_string()));
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

    let mut message = providers::ConversationMessage::user_parts(parts);
    message.metadata = MessageMetadata {
        origin,
        related_job_id,
        related_turn_id: None,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    message
}

pub(crate) async fn prepare_user_turn(
    agent: &mut agent::Agent,
    hot: &HotState,
    input: PrepareUserTurnInput,
) {
    let summary = agent.take_conversation_summary();

    let runtime_state = if let (Some(manager), Some(session_key)) =
        (&hot.job_manager, input.session_key.as_deref())
    {
        let jobs = manager.running_jobs_for_session(session_key).await;
        manager.render_runtime_state_block(&jobs)
    } else {
        None
    };

    let user_msg = build_user_message(
        input.content,
        input.content_parts,
        hot.cfg.agent.primary_model.vision,
        summary.as_deref(),
        runtime_state.as_deref(),
        input.memory_block.as_deref(),
        input.origin,
        input.related_job_id,
    );
    agent.history.push(user_msg);
    refresh_prompt(agent, hot, input.channel_supplement.as_deref());
}

pub(crate) fn current_main_model_request(agent: &agent::Agent) -> String {
    agent
        .history
        .last()
        .map(|message| message.text_content())
        .unwrap_or_default()
}

enum BusyTurnAction {
    Continue,
    CancelOnly,
    CancelAndRun(commands::Command),
}

struct BusyCommandDispatch {
    feedback: Vec<String>,
    action: BusyTurnAction,
}

async fn dispatch_busy_command(
    command: commands::Command,
    agent: &agent::Agent,
    hot: &HotState,
    session_key: Option<&str>,
    queued_commands: &mut VecDeque<commands::Command>,
) -> BusyCommandDispatch {
    match command.policy() {
        commands::CommandPolicy::ImmediateRead | commands::CommandPolicy::ImmediateJobControl => {
            BusyCommandDispatch {
                feedback: vec![
                    commands::execute_immediate_command(&command, agent, hot, session_key).await,
                ],
                action: BusyTurnAction::Continue,
            }
        }
        commands::CommandPolicy::QueueWhileBusy => {
            queued_commands.push_back(command.clone());
            BusyCommandDispatch {
                feedback: vec![
                    commands::execute_immediate_command(&command, agent, hot, session_key).await,
                ],
                action: BusyTurnAction::Continue,
            }
        }
        commands::CommandPolicy::CancelThenRun => BusyCommandDispatch {
            feedback: Vec::new(),
            action: BusyTurnAction::CancelAndRun(command),
        },
        commands::CommandPolicy::CancelCurrentTurn => BusyCommandDispatch {
            feedback: Vec::new(),
            action: BusyTurnAction::CancelOnly,
        },
    }
}

async fn drain_queued_commands(
    queued_commands: &mut VecDeque<commands::Command>,
    agent: &mut agent::Agent,
    hot: &mut HotState,
    ctx: &RunContext,
    session_key: Option<&str>,
) -> Vec<String> {
    let mut feedback = Vec::new();
    while let Some(command) = queued_commands.pop_front() {
        feedback
            .push(commands::execute_stateful_command(&command, agent, hot, ctx, session_key).await);
    }
    feedback
}

async fn post_turn(agent: &mut agent::Agent, input: PostTurnInput) {
    let msg = format!(
        "tokens: in={}, out={}, total={}",
        agent.total_usage.input_tokens,
        agent.total_usage.output_tokens,
        agent.total_usage.input_tokens + agent.total_usage.output_tokens,
    );
    tracing::info!("{msg}");

    if let Err(e) = agent
        .auto_compact(&config::resolve_path(config::MEMORY_DIR))
        .await
    {
        tracing::warn!("Auto-compact failed: {e}");
    }

    if let Some(memory_service) = input
        .memory_service
        .as_ref()
        .filter(|_| input.turn_input_content.is_some() || input.assistant_response.is_some())
    {
        let mut messages = Vec::new();
        if let Some(text) = input.turn_input_content.as_deref() {
            if !text.trim().is_empty() {
                let role = match input.input_origin {
                    MessageOrigin::Human => "user",
                    MessageOrigin::RuntimePtcResult => "runtime_ptc_result",
                    MessageOrigin::RuntimePtcNotice => "runtime_ptc_notice",
                };
                messages.push(memory::types::JournalMessage::new(role, text));
            }
        }
        if let Some(text) = input.assistant_response.as_deref() {
            if !text.trim().is_empty() {
                messages.push(memory::types::JournalMessage::new("assistant", text));
            }
        }
        if let Err(e) = memory_service.append_turn_messages(
            &input.turn_id,
            &input.session_id,
            &input.entity_id,
            input.channel.as_deref(),
            &messages,
        ) {
            tracing::warn!("Failed to append memory journal: {e}");
        }
        if let Some(analyzer) = input.memory_analyzer {
            memory_service.spawn_maintenance(analyzer, input.entity_id.clone());
        }
    }
}

async fn recall_memory_block(
    memory_service: Option<&Arc<memory::MemoryService>>,
    entity_id: Option<&str>,
    query: &str,
) -> Option<String> {
    let memory_service = memory_service?;
    let entity_id = entity_id?;
    match memory_service.recall_prompt(entity_id, query).await {
        Ok(Some((block, result))) => {
            tracing::debug!(
                entity_id = %entity_id,
                facts = result.facts.len(),
                insights = result.insights.len(),
                failures = result.failures.len(),
                procedures = result.procedures.len(),
                template = %result.debug.template,
                "Memory recall hit"
            );
            Some(block)
        }
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(entity_id = %entity_id, error = %error, "Memory recall failed");
            None
        }
    }
}

fn create_channel_registry(
    cfg: &Config,
    stt_provider: Option<Arc<dyn SttProvider>>,
) -> std::collections::HashMap<String, Arc<dyn Channel>> {
    channels::create_channel_registry(cfg, stt_provider)
}

pub async fn run_interactive(
    agent: &mut agent::Agent,
    ctx: &RunContext,
    hot: &mut HotState,
    sessions_dir: &std::path::Path,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<channels::ChannelMessage>(32);
    let cli_channel = channels::cli::CliChannel;
    let mut pending: Vec<channels::ChannelMessage> = Vec::new();
    let mut queued_commands: VecDeque<commands::Command> = VecDeque::new();
    hot.job_manager = Some(Arc::new(JobManager::new(
        tx.clone(),
        hot.cfg.agent.tool_timeout,
        hot.cfg.agent.disabled_primitives.clone(),
        (
            Arc::clone(&hot.compression_provider.0),
            hot.compression_provider.1.clone(),
        ),
    )));

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
        for feedback in
            drain_queued_commands(&mut queued_commands, agent, hot, ctx, Some("cli:cli-user")).await
        {
            eprintln!("{feedback}");
        }

        let msg = if let Some(idx) = pending.iter().position(|_| true) {
            pending.remove(idx)
        } else {
            match rx.recv().await {
                Some(m) => m,
                None => break,
            }
        };

        if matches!(msg.origin, ChannelMessageOrigin::Human) {
            match commands::try_handle_command(&msg.content, agent, hot, ctx, Some("cli:cli-user"))
                .await
            {
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
        }
        let turn_id = Uuid::new_v4().to_string();
        let snapshot = agent.snapshot_turn();
        let session_key = "cli:cli-user".to_string();
        let related_job_id = msg.related_job_id;
        let cli_entity_id = ctx
            .memory_service
            .as_ref()
            .map(|service| service.entity_id());
        let turn_input_content = Some(msg.content.clone());
        let memory_block = if let Some(text) = turn_input_content
            .as_deref()
            .filter(|_| matches!(msg.origin, ChannelMessageOrigin::Human))
        {
            recall_memory_block(ctx.memory_service.as_ref(), cli_entity_id, text).await
        } else {
            None
        };
        prepare_user_turn(
            agent,
            hot,
            PrepareUserTurnInput {
                content: msg.content,
                content_parts: msg.content_parts,
                channel_supplement: None,
                session_key: Some(session_key.clone()),
                memory_block,
                origin: provider_message_origin(msg.origin),
                related_job_id,
            },
        )
        .await;
        let main_model_request = current_main_model_request(agent);

        let mut cancelled = false;
        let mut post_cancel_command = None;
        let mut run_output = None;
        {
            let mut rx_closed = false;
            let run_turn = agent.collect_turn_stream_output(
                hot.active_provider.as_ref(),
                &hot.chat_opts,
                |delta| {
                    let safe = channels::cli::sanitize_terminal_text(delta);
                    print!("{safe}");
                    if let Err(e) = std::io::stdout().flush() {
                        tracing::debug!("Failed to flush stream output: {e}");
                    }
                },
            );
            tokio::pin!(run_turn);

            loop {
                tokio::select! {
                    result = &mut run_turn => {
                        run_output = Some(result);
                        break;
                    }
                    incoming = rx.recv(), if !rx_closed => {
                        match incoming {
                            Some(next) => {
                                if matches!(next.origin, ChannelMessageOrigin::Human) {
                                    if let Some(command) = commands::parse_command(&next.content) {
                                        let dispatch = dispatch_busy_command(
                                            command,
                                            agent,
                                            hot,
                                            Some("cli:cli-user"),
                                            &mut queued_commands,
                                        )
                                        .await;
                                        for feedback in dispatch.feedback {
                                            eprintln!("\n{feedback}\n");
                                        }
                                        match dispatch.action {
                                            BusyTurnAction::Continue => {}
                                            BusyTurnAction::CancelOnly => {
                                                cancelled = true;
                                                tracing::info!("Interactive turn cancel requested by user");
                                                break;
                                            }
                                            BusyTurnAction::CancelAndRun(command) => {
                                                cancelled = true;
                                                post_cancel_command = Some(command);
                                                tracing::info!("Interactive turn cancel requested by command");
                                                break;
                                            }
                                        }
                                        continue;
                                    }
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
            eprintln!("\nCurrent turn cancelled\n");
            if let Some(command) = post_cancel_command.take() {
                let feedback = commands::execute_stateful_command(
                    &command,
                    agent,
                    hot,
                    ctx,
                    Some("cli:cli-user"),
                )
                .await;
                eprintln!("{feedback}");
            }
            for feedback in
                drain_queued_commands(&mut queued_commands, agent, hot, ctx, Some("cli:cli-user"))
                    .await
            {
                eprintln!("{feedback}");
            }
            print!("zerda> ");
            if let Err(e) = std::io::stdout().flush() {
                tracing::debug!("Failed to flush prompt: {e}");
            }
            continue;
        }

        let mut ptc_requests = Vec::new();
        let mut ptc_parse_notice = None;
        if let Some(result) = run_output {
            match result {
                Ok(output) => {
                    let resp = match agent.finish_streamed_turn(output) {
                        Ok(resp) => resp,
                        Err(e) => {
                            eprintln!("\nError: {e}\n");
                            agent.restore_turn(snapshot);
                            print!("zerda> ");
                            if let Err(e) = std::io::stdout().flush() {
                                tracing::debug!("Failed to flush prompt: {e}");
                            }
                            continue;
                        }
                    };
                    ptc_requests = resp.ptc_requests;
                    ptc_parse_notice = resp.ptc_parse_notice;
                    println!("\n");
                }
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

        if let Some(manager) = &hot.job_manager {
            if !ptc_requests.is_empty() {
                let session = PtcSessionContext {
                    channel: "cli".to_string(),
                    session_id: "cli-user".to_string(),
                    sender: "user".to_string(),
                    main_model_request: main_model_request.clone(),
                };
                manager.launch_requests(&session, ptc_requests).await;
            }
        }
        if let Some(notice) = ptc_parse_notice {
            pending.push(channels::ChannelMessage {
                sender: "__ptc_notice__".to_string(),
                session_id: "cli-user".to_string(),
                content: notice,
                content_parts: None,
                channel: "cli".to_string(),
                origin: ChannelMessageOrigin::RuntimePtcNotice,
                related_job_id: None,
            });
        }

        eprintln!(
            "[tokens: in={}, out={}, total={}]",
            agent.total_usage.input_tokens,
            agent.total_usage.output_tokens,
            agent.total_usage.input_tokens + agent.total_usage.output_tokens,
        );

        let assistant_response = agent.history.last().map(|message| message.text_content());
        post_turn(
            agent,
            PostTurnInput {
                memory_service: ctx.memory_service.clone(),
                memory_analyzer: Some((
                    Arc::clone(&hot.compression_provider.0),
                    hot.compression_provider.1.clone(),
                )),
                turn_id,
                session_id: "cli-user".to_string(),
                entity_id: cli_entity_id.unwrap_or("self").to_string(),
                channel: Some("cli".to_string()),
                input_origin: provider_message_origin(msg.origin),
                turn_input_content: turn_input_content.clone(),
                assistant_response,
            },
        )
        .await;

        if let Err(e) = agent.save_session(sessions_dir, Some("latest")) {
            tracing::debug!("Failed to save session: {e}");
        }

        for feedback in
            drain_queued_commands(&mut queued_commands, agent, hot, ctx, Some("cli:cli-user")).await
        {
            eprintln!("{feedback}");
        }

        print!("zerda> ");
        if let Err(e) = std::io::stdout().flush() {
            tracing::debug!("Failed to flush prompt: {e}");
        }
    }

    Ok(())
}

pub async fn run_serve(
    ctx: &RunContext,
    hot: &mut HotState,
    sessions_dir: &std::path::Path,
) -> Result<()> {
    let channels = create_channel_registry(&hot.cfg, ctx.stt_provider.clone());

    if channels.is_empty() {
        anyhow::bail!("No channels configured. Add channel entries to your config file.");
    }

    let (tx, mut rx) = mpsc::channel::<channels::ChannelMessage>(32);
    hot.job_manager = Some(Arc::new(JobManager::new(
        tx.clone(),
        hot.cfg.agent.tool_timeout,
        hot.cfg.agent.disabled_primitives.clone(),
        (
            Arc::clone(&hot.compression_provider.0),
            hot.compression_provider.1.clone(),
        ),
    )));

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
        let origin = msg.origin;
        let turn_id = Uuid::new_v4().to_string();
        let trace_id = Uuid::new_v4().to_string();
        let turn_started = Instant::now();
        let mut queued_cancel = false;

        while let Ok(next) = rx.try_recv() {
            if next.session_id == session_id
                && next.channel == channel_name
                && matches!(next.origin, ChannelMessageOrigin::Human)
                && commands::is_cancel_command(&next.content)
            {
                queued_cancel = true;
                continue;
            }
            if next.session_id == session_id && next.channel == channel_name {
                if !matches!(origin, ChannelMessageOrigin::Human)
                    || !matches!(next.origin, ChannelMessageOrigin::Human)
                    || commands::is_command(&next.content)
                {
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
            trace_id = %trace_id,
            channel = %channel_name,
            session_id = %session_id,
            sender = %sender
        );
        tracing::info!(
            parent: &turn_span,
            event = "runner.turn.start",
            content_chars = content.chars().count(),
            has_content_parts = content_parts.as_ref().is_some_and(|parts| !parts.is_empty()),
            "Turn start"
        );

        let ch = channels.get(&channel_name).cloned();
        let session_key = format!("{channel_name}:{session_id}");
        let storage_id = format!("{channel_name}-{}", hex::encode(session_id.as_bytes()));

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

        if matches!(origin, ChannelMessageOrigin::Human) {
            match commands::try_handle_command(
                &content,
                &mut session_agent,
                hot,
                ctx,
                Some(&session_key),
            )
            .await
            {
                commands::CommandResult::Handled(feedback) => {
                    if let Some(ref ch) = ch {
                        if let Err(e) = ch.send(&feedback, &sender).await {
                            tracing::warn!("Failed to send command feedback: {e}");
                        }
                    }
                    if !session_agent.history.is_empty() {
                        if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id))
                        {
                            tracing::warn!("Failed to save session {storage_id}: {e}");
                        }
                    }
                    tracing::info!(
                        parent: &turn_span,
                        event = "runner.turn.done",
                        elapsed_ms = turn_started.elapsed().as_millis(),
                        response_chars = feedback.chars().count(),
                        "Turn done"
                    );
                    session_agents.insert(session_key, session_agent);
                    continue;
                }
                commands::CommandResult::NotACommand => {}
            }
        }

        let supplement = ch.as_ref().and_then(|c| c.prompt_supplement());
        let snapshot = session_agent.snapshot_turn();
        let mut queued_commands: VecDeque<commands::Command> = VecDeque::new();
        let related_job_id = msg.related_job_id;
        let entity_id = ctx
            .memory_service
            .as_ref()
            .map(|service| service.entity_id());
        let turn_input_content = Some(content.clone());
        let memory_block = if let Some(text) = turn_input_content
            .as_deref()
            .filter(|_| matches!(origin, ChannelMessageOrigin::Human))
        {
            recall_memory_block(ctx.memory_service.as_ref(), entity_id, text).await
        } else {
            None
        };
        prepare_user_turn(
            &mut session_agent,
            hot,
            PrepareUserTurnInput {
                content,
                content_parts,
                channel_supplement: supplement,
                session_key: Some(session_key.clone()),
                memory_block,
                origin: provider_message_origin(origin),
                related_job_id,
            },
        )
        .await;
        let main_model_request = current_main_model_request(&session_agent);

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

        let mut live_stream_handle = None;
        let live_stream_tx = if let Some(ref ch_ref) = ch {
            let (tx, rx) = mpsc::unbounded_channel::<LiveStreamEvent>();
            let ch = Arc::clone(ch_ref);
            let sender = sender.clone();
            let span = turn_span.clone();
            live_stream_handle = Some(tokio::spawn(
                async move { dispatch_live_stream(ch, sender, rx).await }.instrument(span),
            ));
            Some(tx)
        } else {
            None
        };
        let live_stream_tx_for_delta = live_stream_tx.clone();
        let mut cancelled = queued_cancel;
        let mut post_cancel_command = None;
        let mut rx_closed = false;
        let mut turn_output = None;
        let mut turn_error: Option<String> = None;
        if !cancelled {
            let run_turn = session_agent
                .collect_turn_stream_output(
                    hot.active_provider.as_ref(),
                    &hot.chat_opts,
                    move |delta| {
                        if let Some(tx) = &live_stream_tx_for_delta {
                            let _ = tx.send(LiveStreamEvent::Delta(delta.to_string()));
                        }
                    },
                )
                .instrument(turn_span.clone());
            tokio::pin!(run_turn);

            loop {
                tokio::select! {
                    result = &mut run_turn => {
                        turn_output = match result {
                            Ok(r) => Some(r),
                            Err(e) => {
                                tracing::error!(
                                    event = "runner.turn.error",
                                    error_kind = "provider",
                                    channel = %channel_name,
                                    sender = %sender,
                                    "Turn failed: {e}"
                                );
                                turn_error = Some(format!("{e}"));
                                None
                            }
                        };
                        break;
                    }
                    incoming = rx.recv(), if !rx_closed => {
                        match incoming {
                            Some(next) => {
                                if next.session_id == session_id
                                    && next.channel == channel_name
                                    && matches!(next.origin, ChannelMessageOrigin::Human)
                                {
                                    if let Some(command) = commands::parse_command(&next.content) {
                                        let dispatch = dispatch_busy_command(
                                            command,
                                            &session_agent,
                                            hot,
                                            Some(&session_key),
                                            &mut queued_commands,
                                        )
                                        .await;
                                        for feedback in dispatch.feedback {
                                            if let Some(ref ch) = ch {
                                                if let Err(e) = ch.send(&feedback, &sender).await {
                                                    tracing::warn!("Failed to send command feedback: {e}");
                                                }
                                            }
                                        }
                                        match dispatch.action {
                                            BusyTurnAction::Continue => {}
                                            BusyTurnAction::CancelOnly => {
                                                cancelled = true;
                                                tracing::info!(
                                                    parent: &turn_span,
                                                    event = "runner.turn.cancel.requested",
                                                    "Turn cancel requested by user"
                                                );
                                                break;
                                            }
                                            BusyTurnAction::CancelAndRun(command) => {
                                                cancelled = true;
                                                post_cancel_command = Some(command);
                                                tracing::info!(
                                                    parent: &turn_span,
                                                    event = "runner.turn.cancel.requested",
                                                    "Turn cancel requested by command"
                                                );
                                                break;
                                            }
                                        }
                                        continue;
                                    }
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
                event = "runner.turn.cancel.pre_execution",
                "Turn cancel requested before execution started"
            );
        }

        typing_cancel.cancel();
        drop(live_stream_tx);
        let live_stream_summary = if let Some(handle) = live_stream_handle {
            match handle.await {
                Ok(summary) => summary,
                Err(e) => {
                    tracing::warn!("Live stream sender task panicked: {e}");
                    LiveStreamDispatchSummary::default()
                }
            }
        } else {
            LiveStreamDispatchSummary::default()
        };

        if cancelled {
            session_agent.restore_turn(snapshot);
            if let Some(ref ch) = ch {
                if let Err(e) = ch.send("Current turn cancelled", &sender).await {
                    tracing::warn!("Failed to send cancel feedback via {}: {e}", channel_name);
                }
                if let Some(command) = post_cancel_command.take() {
                    let feedback = commands::execute_stateful_command(
                        &command,
                        &mut session_agent,
                        hot,
                        ctx,
                        Some(&session_key),
                    )
                    .await;
                    if let Err(e) = ch.send(&feedback, &sender).await {
                        tracing::warn!("Failed to send command feedback via {}: {e}", channel_name);
                    }
                }
                for feedback in drain_queued_commands(
                    &mut queued_commands,
                    &mut session_agent,
                    hot,
                    ctx,
                    Some(&session_key),
                )
                .await
                {
                    if let Err(e) = ch.send(&feedback, &sender).await {
                        tracing::warn!(
                            "Failed to send queued command feedback via {}: {e}",
                            channel_name
                        );
                    }
                }
            } else {
                println!("Current turn cancelled");
            }
            tracing::info!(
                parent: &turn_span,
                event = "runner.turn.cancelled",
                elapsed_ms = turn_started.elapsed().as_millis(),
                "Turn cancelled"
            );
            if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
                tracing::warn!("Failed to save session {storage_id}: {e}");
            }
            session_agents.insert(session_key, session_agent);
            continue;
        }

        let Some(turn_output) = turn_output else {
            session_agent.restore_turn(snapshot);
            if let Some(ref ch) = ch {
                let msg = match turn_error {
                    Some(detail) => format!("Error: {detail}"),
                    None => "Error: turn did not complete".to_string(),
                };
                let _ = ch.send(&msg, &sender).await;
            }
            session_agents.insert(session_key, session_agent);
            continue;
        };
        let agent::AssistantTurnResult {
            visible_text: response,
            ptc_requests,
            ptc_parse_notice,
        } = match session_agent.finish_streamed_turn(turn_output) {
            Ok(result) => result,
            Err(e) => {
                session_agent.restore_turn(snapshot);
                if let Some(ref ch) = ch {
                    let _ = ch.send(&format!("Error: {e}"), &sender).await;
                }
                session_agents.insert(session_key, session_agent);
                continue;
            }
        };
        if let Some(manager) = &hot.job_manager {
            if !ptc_requests.is_empty() {
                let session = PtcSessionContext {
                    channel: channel_name.clone(),
                    session_id: session_id.clone(),
                    sender: sender.clone(),
                    main_model_request: main_model_request.clone(),
                };
                manager.launch_requests(&session, ptc_requests).await;
            }
        }
        if let Some(notice) = ptc_parse_notice {
            pending.push(channels::ChannelMessage {
                sender: sender.clone(),
                session_id: session_id.clone(),
                content: notice,
                content_parts: None,
                channel: channel_name.clone(),
                origin: ChannelMessageOrigin::RuntimePtcNotice,
                related_job_id: None,
            });
        }

        if let Some(ref ch) = ch {
            tracing::debug!(
                event = "runner.turn.response.dispatch",
                final_response_chars = response.chars().count(),
                "Sending assistant messages"
            );
            if let Some(final_message) = normalize_final_message(&response) {
                if let Err(e) = send_final_message(
                    ch,
                    &sender,
                    &final_message,
                    live_stream_summary.stream_message_id.as_deref(),
                )
                .await
                {
                    tracing::warn!("Failed to send response via {}: {e}", channel_name);
                }
            }
        } else {
            if let Some(final_message) = normalize_final_message(&response) {
                println!("{}", channels::cli::sanitize_terminal_text(&final_message));
            }
        }

        tracing::info!(
            parent: &turn_span,
            event = "runner.turn.done",
            elapsed_ms = turn_started.elapsed().as_millis(),
            response_chars = response.chars().count(),
            "Turn done"
        );

        post_turn(
            &mut session_agent,
            PostTurnInput {
                memory_service: ctx.memory_service.clone(),
                memory_analyzer: Some((
                    Arc::clone(&hot.compression_provider.0),
                    hot.compression_provider.1.clone(),
                )),
                turn_id,
                session_id,
                entity_id: entity_id.unwrap_or("self").to_string(),
                channel: Some(channel_name.clone()),
                input_origin: provider_message_origin(origin),
                turn_input_content,
                assistant_response: Some(response),
            },
        )
        .await;

        if let Err(e) = session_agent.save_session(sessions_dir, Some(&storage_id)) {
            tracing::warn!("Failed to save session {storage_id}: {e}");
        }
        if let Some(ref ch) = ch {
            for feedback in drain_queued_commands(
                &mut queued_commands,
                &mut session_agent,
                hot,
                ctx,
                Some(&session_key),
            )
            .await
            {
                if let Err(e) = ch.send(&feedback, &sender).await {
                    tracing::warn!(
                        "Failed to send queued command feedback via {}: {e}",
                        channel_name
                    );
                }
            }
        }
        session_agents.insert(session_key, session_agent);
    }

    Ok(())
}

fn normalize_final_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

enum LiveStreamEvent {
    Delta(String),
}

#[derive(Default)]
struct LiveStreamDispatchSummary {
    stream_message_id: Option<String>,
    overflowed: bool,
}

fn apply_live_stream_event(event: LiveStreamEvent, stream_text: &mut String) -> bool {
    match event {
        LiveStreamEvent::Delta(delta) => {
            stream_text.push_str(&delta);
            true
        }
    }
}

async fn dispatch_live_stream(
    channel: Arc<dyn Channel>,
    recipient: String,
    mut rx: mpsc::UnboundedReceiver<LiveStreamEvent>,
) -> LiveStreamDispatchSummary {
    let mut summary = LiveStreamDispatchSummary::default();
    let mut stream_text = String::new();
    let mut disabled = false;
    let started = Instant::now();
    let mut last_progress = Instant::now();
    let mut delta_chunks = 0u64;
    let progress_interval_ms = stream_progress_interval_ms();
    tracing::debug!(
        event = "runner.stream.start",
        progress_interval_ms,
        "Live stream dispatch started"
    );

    while let Some(event) = rx.recv().await {
        if apply_live_stream_event(event, &mut stream_text) {
            delta_chunks += 1;
        }
        while let Ok(event) = rx.try_recv() {
            if apply_live_stream_event(event, &mut stream_text) {
                delta_chunks += 1;
            }
        }

        if disabled {
            continue;
        }

        let clean_text = rich_content::strip_rich_markers(&stream_text);
        if clean_text.is_empty() {
            continue;
        }
        let clean_chars = clean_text.chars().count();
        if last_progress.elapsed().as_millis() >= u128::from(progress_interval_ms) {
            tracing::debug!(
                event = "runner.stream.progress",
                elapsed_ms = started.elapsed().as_millis(),
                delta_chunks,
                clean_chars,
                "Live stream progress"
            );
            last_progress = Instant::now();
        }
        if clean_chars > config::STREAM_OVERFLOW_CHARS {
            if !summary.overflowed {
                tracing::debug!(
                    event = "runner.stream.overflow",
                    clean_chars,
                    max_chars = config::STREAM_OVERFLOW_CHARS,
                    "Live stream overflow reached; skip intermediate updates"
                );
            }
            summary.overflowed = true;
            continue;
        }

        let update = format!("{clean_text}▌");
        if let Some(message_id) = summary.stream_message_id.clone() {
            if let Err(e) = channel
                .send_stream_update(&recipient, &message_id, &update)
                .await
            {
                tracing::warn!(
                    event = "runner.stream.update.error",
                    "Failed to send live stream update: {e}"
                );
                summary.stream_message_id = None;
                disabled = true;
            }
            continue;
        }

        match channel.send_stream_start(&recipient, &update).await {
            Ok(Some(message_id)) => {
                summary.stream_message_id = Some(message_id);
            }
            Ok(None) => {
                disabled = true;
            }
            Err(e) => {
                tracing::warn!(
                    event = "runner.stream.start.error",
                    "Failed to start live stream: {e}"
                );
                disabled = true;
            }
        }
    }

    tracing::debug!(
        event = "runner.stream.done",
        elapsed_ms = started.elapsed().as_millis(),
        delta_chunks,
        overflowed = summary.overflowed,
        has_stream_message = summary.stream_message_id.is_some(),
        "Live stream dispatch done"
    );

    summary
}

async fn send_final_message(
    channel: &Arc<dyn Channel>,
    recipient: &str,
    message: &str,
    stream_message_id: Option<&str>,
) -> Result<()> {
    let clean_text = rich_content::strip_rich_markers(message);
    let markers = rich_content::rich_content_markers(message);

    if clean_text.is_empty() {
        for marker in markers {
            channel.send(&marker, recipient).await?;
        }
        return Ok(());
    }

    if let Some(message_id) = stream_message_id {
        if let Err(e) = channel
            .send_stream_update(recipient, message_id, &clean_text)
            .await
        {
            tracing::warn!(
                event = "runner.stream.finalize.error",
                "Failed to finalize stream update, falling back to send: {e}"
            );
            channel.send(&clean_text, recipient).await?;
        }
    } else {
        channel.send(&clean_text, recipient).await?;
    }

    for marker in markers {
        channel.send(&marker, recipient).await?;
    }
    Ok(())
}
