#![allow(
    clippy::unnecessary_literal_bound,
    clippy::unused_self,
    clippy::option_if_let_else,
    clippy::cast_precision_loss
)]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::mpsc;

mod agent;
mod channels;
mod commands;
mod config;
mod identity;
mod logging;
mod memory;
mod prompt;
mod providers;
mod ptc;
mod rich_content;
mod runner;
mod stt;
mod util;

#[derive(Parser)]
#[command(name = "zerda", version, about = "Minimal AI agent framework")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long, env = "ZERDA_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        message: Option<String>,
        #[arg(long)]
        #[allow(clippy::option_option)]
        resume: Option<Option<String>>,
    },
    Serve,
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    Generate,
    Validate,
}

fn init_optional_provider<T: ?Sized + 'static>(
    label: &str,
    provider_name: &str,
    factory: impl FnOnce() -> anyhow::Result<Box<T>>,
) -> Option<Arc<T>> {
    if provider_name.is_empty() {
        return None;
    }
    match factory() {
        Ok(p) => Some(Arc::from(p)),
        Err(e) => {
            tracing::warn!("Failed to create {label} provider: {e}");
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Commands::Config { action }) = &cli.command {
        match action {
            ConfigAction::Generate => {
                print!("{}", include_str!("../zerda.toml.full"));
                return Ok(());
            }
            ConfigAction::Validate => match config::load_config(cli.config.as_deref()) {
                Ok(_) => {
                    println!("Configuration is valid.");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Configuration error: {e}");
                    std::process::exit(1);
                }
            },
        }
    }

    let cfg = config::load_config(cli.config.as_deref())?;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = &cfg.log.level;
        let filter = if level == "debug" || level == "trace" {
            format!("{level},hyper_util=warn,reqwest=warn,h2=warn,rustls=warn")
        } else {
            level.clone()
        };
        tracing_subscriber::EnvFilter::new(filter)
    });
    logging::set_runtime_log_options(cfg.log.debug_plaintext, cfg.log.stream_progress_interval_ms);
    let use_json = !cfg.log.format.eq_ignore_ascii_case("text");
    if use_json {
        tracing_subscriber::fmt()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_target(cfg.log.include_target)
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_target(cfg.log.include_target)
            .with_env_filter(env_filter)
            .init();
    }

    let mut registry = providers::ProviderRegistry::new(cfg.providers.clone())?;

    let primary_ref = config::ModelRef::parse(&cfg.agent.primary_model.model)?;
    let active_provider = registry.get_or_create(&primary_ref.provider_id)?;
    let chat_opts = providers::ChatOptions::from_model_config(
        &cfg.agent.primary_model,
        &primary_ref.model_name,
    );

    let fast_mc = cfg
        .agent
        .fast_model
        .as_ref()
        .unwrap_or(&cfg.agent.primary_model);
    let fast_ref = config::ModelRef::parse(&fast_mc.model)?;
    let fast_provider = registry.get_or_create(&fast_ref.provider_id)?;
    let fast_chat_opts = providers::ChatOptions::from_model_config(fast_mc, &fast_ref.model_name);

    let stt_provider: Option<Arc<dyn stt::SttProvider>> =
        init_optional_provider("STT", &cfg.stt.provider, || {
            stt::create_stt_provider(&cfg.stt)
        });

    let compression_provider = (fast_provider.clone(), fast_chat_opts.clone());

    let identity_path = config::resolve_path(&cfg.agent.identity_path);
    let identity_text = if identity_path.exists() {
        Some(identity::load_identity(&identity_path)?)
    } else {
        None
    };

    let system_prompt_parts = prompt::build_system_prompt_parts(
        &cfg.agent.disabled_primitives,
        identity_text.as_deref(),
        None,
    );

    let mut agent = agent::Agent::new(
        cfg.agent.clone(),
        (
            Arc::clone(&compression_provider.0),
            compression_provider.1.clone(),
        ),
    );
    agent.set_system_prompt_parts(system_prompt_parts);

    let sessions_dir = config::resolve_path(config::MEMORY_DIR).join("sessions");

    let memory_service = if cfg.memory.enabled {
        match memory::MemoryService::shared(&cfg.memory) {
            Ok(service) => {
                tracing::info!(
                    sqlite_path = %service.sqlite_path().display(),
                    embedding_base_url = %service.embedding_base_url(),
                    chroma_url = %service.chroma_url(),
                    "Memory service initialized"
                );
                Some(service)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize memory service: {e}");
                None
            }
        }
    } else {
        None
    };

    let run_ctx = runner::RunContext {
        stt_provider,
        memory_service,
    };

    let mut hot = runner::HotState {
        identity_text,
        cfg,
        chat_opts,
        compression_provider,
        registry,
        active_provider,
        active_model_ref: primary_ref,
        job_manager: None,
    };

    match cli.command {
        Some(Commands::Run { message, resume }) => {
            if let Some(resume_arg) = resume {
                let resume_id = resume_arg.as_deref();
                let (sid, history) = agent::Agent::load_session(&sessions_dir, resume_id)?;
                agent.history = history;
                tracing::info!("Resumed session: {sid}");
            }

            if let Some(msg) = message {
                let raw_msg = msg.clone();
                let turn_id = uuid::Uuid::new_v4().to_string();
                let entity_id = run_ctx
                    .memory_service
                    .as_ref()
                    .map(|service| service.entity_id());
                let recalled_memory = if let Some(service) = run_ctx.memory_service.as_ref() {
                    match service
                        .recall_prompt(entity_id.unwrap_or("self"), &raw_msg)
                        .await
                    {
                        Ok(Some((block, _))) => Some(block),
                        Ok(None) => None,
                        Err(error) => {
                            tracing::warn!("Memory recall failed for run mode: {error}");
                            None
                        }
                    }
                } else {
                    None
                };
                runner::prepare_user_turn(
                    &mut agent,
                    &hot,
                    runner::PrepareUserTurnInput {
                        content: msg,
                        content_parts: None,
                        channel_supplement: None,
                        session_key: None,
                        memory_block: recalled_memory,
                        origin: providers::MessageOrigin::Human,
                        related_job_id: None,
                    },
                )
                .await;
                let main_model_request = runner::current_main_model_request(&agent);
                let response = agent
                    .run_turn(hot.active_provider.as_ref(), &hot.chat_opts)
                    .await?;
                let visible_text = response.visible_text;
                let ptc_requests = response.ptc_requests;
                let ptc_parse_notice = response.ptc_parse_notice;
                println!("{}", channels::cli::sanitize_terminal_text(&visible_text));
                if !ptc_requests.is_empty() {
                    let (tx, _rx) = mpsc::channel(1);
                    let manager = ptc::job_manager::JobManager::new(
                        tx,
                        hot.cfg.agent.tool_timeout,
                        hot.cfg.agent.disabled_primitives.clone(),
                        (
                            Arc::clone(&hot.compression_provider.0),
                            hot.compression_provider.1.clone(),
                        ),
                    );
                    let session = ptc::job_manager::PtcSessionContext {
                        channel: "cli".to_string(),
                        session_id: format!("run-{turn_id}"),
                        sender: "user".to_string(),
                        main_model_request,
                    };
                    let jobs = manager.launch_requests(&session, ptc_requests).await;
                    if jobs.is_empty() {
                        eprintln!("PTC jobs failed to start in single-turn mode.");
                    } else {
                        eprintln!("Started detached PTC jobs:");
                        for job in jobs {
                            eprintln!(
                                "- {} [{}] {}",
                                job.job_id,
                                job.status_path.display(),
                                if job.purpose.is_empty() {
                                    "background task"
                                } else {
                                    &job.purpose
                                }
                            );
                        }
                    }
                }
                if let Some(notice) = ptc_parse_notice {
                    eprintln!("{notice}");
                }
                if let Some(memory_service) = run_ctx.memory_service.as_ref() {
                    let messages = [
                        memory::types::JournalMessage::new("user", raw_msg),
                        memory::types::JournalMessage::new("assistant", visible_text.clone()),
                    ];
                    if let Err(e) = memory_service.append_turn_messages(
                        &turn_id,
                        "cli-run",
                        entity_id.unwrap_or("self"),
                        Some("cli"),
                        &messages,
                    ) {
                        tracing::warn!("Failed to append memory journal for run mode: {e}");
                    }
                    memory_service.spawn_maintenance(
                        (
                            Arc::clone(&hot.compression_provider.0),
                            hot.compression_provider.1.clone(),
                        ),
                        entity_id.unwrap_or("self").to_string(),
                    );
                }
                if let Err(e) = agent.save_session(&sessions_dir, None) {
                    tracing::warn!("Failed to save session: {e}");
                }
            } else {
                runner::run_interactive(&mut agent, &run_ctx, &mut hot, &sessions_dir).await?;
            }
        }
        Some(Commands::Serve) => {
            runner::run_serve(&run_ctx, &mut hot, &sessions_dir).await?;
        }
        Some(Commands::Config { .. }) => unreachable!(),
        None => {
            runner::run_interactive(&mut agent, &run_ctx, &mut hot, &sessions_dir).await?;
        }
    }

    agent::Agent::cleanup_old_sessions(&sessions_dir, hot.cfg.agent.session_cleanup_days);

    Ok(())
}
