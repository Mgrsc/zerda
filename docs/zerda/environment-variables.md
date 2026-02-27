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
| `OPENAI_BASE_URL` | OpenAI-compatible API base URL |
| `OPENAI_MODEL` | Default OpenAI model name |
| `OPENAI_COMPRESSION_MODEL` | Model for compression/subagent |
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

## Document Search (Cloudflare AI Search)

| Variable | Description |
|----------|-------------|
| `CF_AI_SEARCH_ACCOUNT_ID` | Cloudflare account ID |
| `CF_AI_SEARCH_API_TOKEN` | Cloudflare API token with AI Search permissions |
| `CF_AI_SEARCH_INSTANCE_NAME` | Cloudflare AutoRAG instance name |

When all three `CF_AI_SEARCH_*` variables are set, the `search_zerda_documents` tool becomes available, enabling semantic search over Zerda's documentation.

## Executor-Injected Variables

These are automatically set in executor Python scripts:

| Variable | Description |
|----------|-------------|
| `EXECUTOR_OUT_PATH` | Path for structured output JSON |
| `EXECUTOR_LOG_PATH` | Path for log file |
| `EXECUTOR_TELEMETRY_PATH` | Path for telemetry JSONL |
| `EXECUTOR_PRIMITIVES_PY_ROOT` | Code primitives root path |
| `EXECUTOR_ENABLE_FIRECRAWL_PRIMITIVES` | "0" or "1" |
