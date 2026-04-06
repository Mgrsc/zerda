# Environment Variables

## Core

| Variable | Description |
|---|---|
| `ZERDA_CONFIG` | Path to the active config file |
| `ZERDA_PRIMITIVES_ROOT` | Override code primitives root directory |

## Providers

| Variable | Description |
|---|---|
| `OPENAI_API_KEY` | OpenAI API key |
| `OPENAI_BASE_URL` | OpenAI-compatible base URL override for chat and the default EMA embedding endpoint |
| `OPENAI_MODEL` | Example primary model name |
| `OPENAI_FAST_MODEL` | Example fast model name |
| `ANTHROPIC_API_KEY` | Anthropic API key |

## Channels

| Variable | Description |
|---|---|
| `TELEGRAM_BOT_TOKEN` | Telegram bot token |

## Speech

| Variable | Description |
|---|---|
| `GROQ_API_KEY` | Groq API key for STT |

## Web

| Variable | Description |
|---|---|
| `FIRECRAWL_API_KEY` | Firecrawl API key for the search primitive |
| `FIRECRAWL_BASE_URL` | Firecrawl API base URL override |

Runtime note: `scrapling_fetch_page` does not use an environment variable, but it does require the Python runtime to have `scrapling[fetchers]` installed. It may also require Playwright browser binaries when its internal stealth path is used for selected dynamic domains. Otherwise the primitive returns `dependency_missing`.

Memory note: the maintained configs reuse `OPENAI_API_KEY` for the default EMA embedding endpoint. If you want EMA embeddings to use a different credential, edit `memory.embedding.api_key` in TOML directly.

## PTC-Injected Variables

| Variable | Description |
|---|---|
| `PTC_OUT_PATH` | Structured output JSON path |
| `PTC_LOG_PATH` | Log file path |
| `PTC_TELEMETRY_PATH` | Telemetry JSONL path |
| `PTC_PRIMITIVES_PY_ROOT` | Code primitives root path |
| `PTC_PRIMITIVES_PY_ROOTS` | JSON array of merged primitive package roots |
| `PTC_DISABLED_PRIMITIVES` | Disabled primitive list |
