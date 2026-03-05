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
mod reflection;
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

    let reload_signal = tools::reload::ReloadSignal::default();

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

    let compression_provider = (fast_provider.clone(), fast_chat_opts.clone());
    let subagent_provider = (fast_provider, fast_chat_opts);

    let reflection_engine = if cfg.reflection.enabled {
        if let Some(reflection_mc) = cfg.reflection.as_model_config() {
            match config::ModelRef::parse(&reflection_mc.model) {
                Ok(reflection_ref) => match registry.get_or_create(&reflection_ref.provider_id) {
                    Ok(reflection_provider) => {
                        let embedding_ref_result = match cfg.reflection.embedding_model.as_deref() {
                            Some(model_ref) => config::ModelRef::parse(model_ref),
                            None => Ok(config::ModelRef {
                                provider_id: reflection_ref.provider_id.clone(),
                                model_name: reflection::DEFAULT_EMBEDDING_MODEL.to_string(),
                            }),
                        };
                        match embedding_ref_result {
                            Ok(embedding_ref) => {
                                let embedding_provider = cfg
                                    .providers
                                    .iter()
                                    .find(|p| p.id == embedding_ref.provider_id.as_str());
                                match embedding_provider {
                                    Some(embedding_provider) => {
                                        let reflection_opts =
                                            providers::ChatOptions::from_model_config(
                                                &reflection_mc,
                                                &reflection_ref.model_name,
                                            );
                                        match reflection::ReflectionEngine::try_new(
                                            reflection_provider,
                                            reflection_opts,
                                            &cfg.reflection.qdrant_url,
                                            Some(&cfg.reflection.qdrant_api_key),
                                            cfg.reflection.embedding_dim,
                                            embedding_provider,
                                            &embedding_ref.model_name,
                                        ) {
                                            Some(engine) => {
                                                match engine.ensure_collection().await {
                                                    Ok(()) => Some(Arc::new(engine)),
                                                    Err(e) => {
                                                        tracing::warn!(
                                                        "REFLECTION: collection setup failed: {e}"
                                                    );
                                                        None
                                                    }
                                                }
                                            }
                                            None => None,
                                        }
                                    }
                                    None => {
                                        tracing::warn!(
                                            "REFLECTION: embedding provider '{}' not found",
                                            embedding_ref.provider_id
                                        );
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "REFLECTION: invalid reflection.embedding_model reference: {e}"
                                );
                                None
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "REFLECTION: provider '{}' init failed: {e}",
                            reflection_ref.provider_id
                        );
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("REFLECTION: invalid reflection.llm_model reference: {e}");
                    None
                }
            }
        } else {
            tracing::debug!(
                "REFLECTION: reflection.enabled=true but reflection.llm_model is empty"
            );
            None
        }
    } else {
        None
    };

    let docs_search_settings = if cfg.docs_search.enabled {
        match config::ModelRef::parse(&cfg.docs_search.embedding_model) {
            Ok(model_ref) => {
                match cfg
                    .providers
                    .iter()
                    .find(|provider| provider.id == model_ref.provider_id)
                {
                    Some(provider) => {
                        if provider.api_key.trim().is_empty() {
                            tracing::warn!(
                                "search_zerda_documents: embedding provider '{}' api_key is empty, tool disabled",
                                provider.id
                            );
                            None
                        } else {
                            Some(tools::search_docs::SearchDocsSettings {
                                qdrant_url: cfg.docs_search.qdrant_url.trim().to_string(),
                                qdrant_api_key: if cfg.docs_search.qdrant_api_key.trim().is_empty()
                                {
                                    None
                                } else {
                                    Some(cfg.docs_search.qdrant_api_key.trim().to_string())
                                },
                                collection: cfg.docs_search.collection.trim().to_string(),
                                docs_root: config::resolve_path(&cfg.docs_search.docs_dir),
                                embedding_api_key: provider.api_key.trim().to_string(),
                                embedding_base_url: provider.base_url.trim().to_string(),
                                embedding_model: model_ref.model_name,
                                embedding_dim: cfg.docs_search.embedding_dim,
                            })
                        }
                    }
                    None => {
                        tracing::warn!(
                            "search_zerda_documents: embedding provider '{}' not found, tool disabled",
                            model_ref.provider_id
                        );
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("search_zerda_documents: invalid docs_search.embedding_model: {e}");
                None
            }
        }
    } else {
        None
    };

    let tools_runtime = tools::BuiltinToolsRuntime {
        tool_timeout: cfg.agent.tool_timeout,
        config_path: cli.config.clone(),
        reload_signal: reload_signal.clone(),
        disabled_primitives: cfg.agent.disabled_primitives.clone(),
    };
    let tools_dependencies = tools::BuiltinToolsDependencies {
        tts_provider,
        skills: Arc::clone(&shared_skills),
        skill_cache: Arc::clone(&skill_cache),
        subagent_provider: Some(subagent_provider),
        reflection: reflection_engine,
        docs_search: docs_search_settings,
    };
    let (all_tools, todo_handle) = tools::builtin_tools((tools_runtime, tools_dependencies).into());

    let builtin_count = all_tools.len();

    let identity_path = config::resolve_path(&cfg.agent.identity_path);
    let identity_text = if identity_path.exists() {
        Some(identity::load_identity(&identity_path)?)
    } else {
        None
    };

    let system_prompt_parts = prompt::build_system_prompt_parts(identity_text.as_deref(), None);

    let mut agent = agent::Agent::new(
        cfg.agent.clone(),
        (
            Arc::clone(&compression_provider.0),
            compression_provider.1.clone(),
        ),
    );
    agent.set_system_prompt_parts(system_prompt_parts);

    let sessions_dir = config::resolve_path(config::MEMORY_DIR).join("sessions");

    let memory_client = if cfg.memory_service.enabled {
        match memory::MemoryServiceClient::new(&cfg.memory_service) {
            Ok(client) => {
                tracing::info!(
                    url = %cfg.memory_service.url,
                    tenant_id = %cfg.memory_service.tenant_id,
                    "Memory service client initialized"
                );
                Some(client)
            }
            Err(e) => {
                tracing::warn!("Failed to initialize memory service client: {e}");
                None
            }
        }
    } else {
        None
    };

    let run_ctx = runner::RunContext {
        config_path: cli.config.clone(),
        reload_signal,
        stt_provider,
        memory_client,
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
        registry,
        active_provider,
        active_model_ref: primary_ref,
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
                let recall_item_ids = runner::prepare_user_turn(
                    &mut agent,
                    &hot,
                    msg,
                    None,
                    None,
                    run_ctx.memory_client.as_ref(),
                    None,
                )
                .await;
                let response = agent
                    .run_turn(hot.active_provider.as_ref(), &hot.tools, &hot.chat_opts)
                    .await?;
                println!("{}", channels::cli::sanitize_terminal_text(&response));
                if let Some(client) = run_ctx.memory_client.as_ref() {
                    let messages = vec![
                        memory::IngestMessage {
                            role: "user".to_string(),
                            content: raw_msg,
                        },
                        memory::IngestMessage {
                            role: "assistant".to_string(),
                            content: response.clone(),
                        },
                    ];
                    if let Err(e) = client
                        .ingest(messages, None, None, Some(&turn_id), Some("cli"))
                        .await
                    {
                        tracing::warn!("Memory ingest failed for run command: {e}");
                    }
                    if !recall_item_ids.is_empty() {
                        if let Err(e) = client
                            .feedback(&recall_item_ids, None, Some(&turn_id))
                            .await
                        {
                            tracing::warn!("Memory feedback failed for run command: {e}");
                        }
                    }
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
