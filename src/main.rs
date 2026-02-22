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

mod agent;
mod channels;
mod commands;
mod config;
mod identity;
mod logging;
mod memory;
mod prompt;
mod providers;
mod rich_content;
mod runner;
mod skills;
mod stt;
mod tools;
mod tts;
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

    let cfg = config::load_config(cli.config.as_deref())?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                let level = &cfg.log.level;
                let filter = if level == "debug" || level == "trace" {
                    format!("{level},hyper_util=warn,reqwest=warn,h2=warn,rustls=warn,rmcp=info")
                } else {
                    level.to_string()
                };
                tracing_subscriber::EnvFilter::new(filter)
            }),
        )
        .init();

    let provider = providers::create_provider(&cfg.provider)?;
    let chat_opts = providers::ChatOptions::from_provider_config(&cfg.provider);

    let memory_dir = config::resolve_path(config::MEMORY_DIR);
    let mem = Arc::new(memory::Memory::new(memory_dir.join("memory")));
    mem.ensure_dirs()?;
    mem.check_memory_size(cfg.agent.max_memory_file_size);

    let reload_signal = tools::reload::ReloadSignal::default();
    let max_memory_chars = cfg.agent.max_memory_tokens * 4;

    let skills_dir = config::resolve_path(config::MEMORY_DIR).join("skills");
    let skills_list = skills::load_skills(&skills_dir);
    let shared_skills = Arc::new(tokio::sync::RwLock::new(skills_list.clone()));
    let skill_cache = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

    let tts_provider: Option<Arc<dyn tts::TtsProvider>> =
        init_optional_provider("TTS", &cfg.tts.provider, || {
            tts::create_tts_provider(&cfg.tts)
        });

    let stt_provider: Option<Arc<dyn stt::SttProvider>> =
        init_optional_provider("STT", &cfg.stt.provider, || {
            stt::create_stt_provider(&cfg.stt)
        });

    let fast_model_provider: (
        std::sync::Arc<dyn providers::Provider>,
        providers::ChatOptions,
    ) = if let Some(ref fm_cfg) = cfg.agent.fast_model {
        match providers::create_provider(&fm_cfg.provider) {
            Ok(p) => {
                let opts = providers::ChatOptions::from_provider_config(&fm_cfg.provider);
                (Arc::from(p), opts)
            }
            Err(e) => {
                tracing::warn!("Failed to create fast_model provider, falling back to main provider: {e}");
                let p = providers::create_provider(&cfg.provider)
                    .expect("main provider already validated");
                (Arc::from(p), chat_opts.clone())
            }
        }
    } else {
        let p = providers::create_provider(&cfg.provider)
            .expect("main provider already validated");
        (Arc::from(p), chat_opts.clone())
    };

    let subagent_provider = (Arc::clone(&fast_model_provider.0), fast_model_provider.1.clone());

    let tools_runtime = tools::BuiltinToolsRuntime {
        tool_timeout: cfg.agent.tool_timeout,
        max_memory_chars,
        config_path: cli.config.clone(),
        reload_signal: reload_signal.clone(),
    };
    let tools_dependencies = tools::BuiltinToolsDependencies {
        memory: Arc::clone(&mem),
        tts_provider,
        skills: Arc::clone(&shared_skills),
        skill_cache: Arc::clone(&skill_cache),
        subagent_provider: Some(subagent_provider),
    };
    let (mut all_tools, todo_handle) = tools::builtin_tools((tools_runtime, tools_dependencies).into());

    let builtin_count = all_tools.len();
    let mcp_tools = tools::mcp::connect_mcp_servers(&cfg.mcp).await;
    all_tools.extend(mcp_tools);

    let identity_path = config::resolve_path(&cfg.agent.identity_path);
    let identity_text = if identity_path.exists() {
        Some(identity::load_identity(&identity_path)?)
    } else {
        None
    };

    let system_prompt =
        prompt::build_system_prompt(identity_text.as_deref(), None);

    let compression_provider = (fast_model_provider.0, fast_model_provider.1);
    let mut agent = agent::Agent::new(cfg.agent.clone(), (Arc::clone(&compression_provider.0), compression_provider.1.clone()));
    agent.set_system_prompt(system_prompt);

    let sessions_dir = config::resolve_path(config::MEMORY_DIR).join("sessions");

    let run_ctx = runner::RunContext {
        provider: provider.as_ref(),
        mem: &mem,
        config_path: cli.config.clone(),
        reload_signal,
        stt_provider,
    };

    let mut hot = runner::HotState {
        tools: all_tools,
        todo: todo_handle,
        identity_text,
        skills: skills_list,
        shared_skills,
        skill_cache,
        cfg,
        builtin_count,
        chat_opts,
        compression_provider,
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
                runner::prepare_user_turn(&mut agent, &hot, mem.as_ref(), msg, None, None);
                let response = agent
                    .run_turn(provider.as_ref(), &hot.tools, &hot.chat_opts)
                    .await?;
                println!("{}", channels::cli::sanitize_terminal_text(&response));
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
        None => {
            runner::run_interactive(&mut agent, &run_ctx, &mut hot, &sessions_dir).await?;
        }
    }

    agent::Agent::cleanup_old_sessions(&sessions_dir, hot.cfg.agent.session_cleanup_days);

    Ok(())
}
