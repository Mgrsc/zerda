# Environment Variables

## Core

| Variable | Description |
|----------|-------------|
| `ZERDA_CONFIG` | Path to zerda.toml config file |
| `ZERDA_PRIMITIVES_ROOT` | Override code primitives root directory |

## LLM Providers

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | OpenAI API key |
| `OPENAI_BASE_URL` | OpenAI-compatible API base URL (used when referenced by provider `base_url`) |
| `OPENAI_MODEL` | Default OpenAI model name |
| `OPENAI_FAST_MODEL` | Fast model name for compression/subagent |
| `ANTHROPIC_API_KEY` | Anthropic API key |

## Channels

| Variable | Description |
|----------|-------------|
| `TELEGRAM_BOT_TOKEN` | Telegram bot token from @BotFather |

## TTS / STT

| Variable | Description |
|----------|-------------|
| `MINIMAX_API_KEY` | MiniMax API key for TTS |
| `GROQ_API_KEY` | Groq API key for STT (Whisper) |

## Web & Search

| Variable | Description |
|----------|-------------|
| `FIRECRAWL_API_KEY` | Firecrawl API key for web scraping |
| `FIRECRAWL_BASE_URL` | Firecrawl API base URL (default: https://api.firecrawl.dev) |

## Document Search (Qdrant)

Document search is configured in `zerda.toml` under `[docs_search]`.

- `qdrant_url`, `qdrant_api_key`, `collection`, `docs_dir`, `embedding_model`, and `embedding_dim` are TOML fields.
- Embedding provider credentials come from `[providers.<id>]` referenced by `docs_search.embedding_model`.

## MemBurrow Memory Service

| Variable | Description |
|----------|-------------|
| `MEMBURROW_AUTH_TOKEN` | Bearer token for MemBurrow API |

## Executor-Injected Variables

These are automatically set in executor Python scripts:

| Variable | Description |
|----------|-------------|
| `EXECUTOR_OUT_PATH` | Path for structured output JSON |
| `EXECUTOR_LOG_PATH` | Path for log file |
| `EXECUTOR_TELEMETRY_PATH` | Path for telemetry JSONL |
| `EXECUTOR_PRIMITIVES_PY_ROOT` | Code primitives root path |
| `EXECUTOR_DISABLED_PRIMITIVES` | JSON array or comma-separated primitive blacklist |
