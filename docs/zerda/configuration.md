# Configuration Reference

## Providers

Define LLM providers under `[providers.<id>]`. Multiple providers can coexist.

```toml
[providers.openai]
type = "openai_chat"          # "openai_chat" | "openai_responses" | "anthropic"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"  # Optional, defaults to official endpoint
# extra_headers = { "X-Custom" = "value" }  # Optional

[providers.openai.retry]
max_retries = 3               # Default: 3
base_delay_ms = 2000          # Default: 2000
max_delay_ms = 30000          # Default: 30000
connect_timeout_secs = 10     # Default: 10
request_timeout_secs = 120    # Default: 120
```

### Supported Provider Types

| Type | Protocol | Example Models |
|------|----------|----------------|
| `openai_chat` | OpenAI Chat Completions API | gpt-4o, gpt-4o-mini, gpt-5.2 |
| `openai_responses` | OpenAI Responses API | Same, with extended reasoning support |
| `anthropic` | Anthropic Messages API | claude-3-5-sonnet, claude-4-sonnet |

## Agent Settings

```toml
[agent]
max_iterations = 10           # Max tool calls per turn (default: 10, min: 10)
max_history = 30              # Message count before auto-compression (default: 30)
max_tool_output_chars = 30000 # Truncate tool results exceeding this (default: 30000)
tool_timeout = 300            # Tool execution timeout in seconds (default: 300)
disabled_primitives = []      # Primitive blacklist; all enabled when empty
session_cleanup_days = 7      # Auto-clean sessions older than N days (default: 7)
show_usage = false            # Print token usage after each turn (default: false)
# max_budget_tokens = 100000  # Session token budget, unlimited by default
identity_path = "~/.zerda/identity.md"  # System role definition (default)

[agent.primary_model]
model = "openai@gpt-4o"       # required, format: provider_id@model_name
vision = true
# temperature = 0.7           # optional, range: 0.0-2.0
# top_p = 1.0                 # optional, range: 0.0-1.0
# max_tokens = 4096           # optional

[agent.fast_model]            # optional, falls back to primary model if omitted
model = "openai@gpt-4o-mini"
vision = true
# temperature = 0.7
# top_p = 1.0
# max_tokens = 4096
```

## Channels

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = ["*"]           # ["*"] = all users, or list specific user IDs
max_message_length = 4096       # Default: 4096
```

Currently only `telegram` channel is supported.

## MCP Servers

Defined in `mcp.toml` (must be in the same directory as the active `zerda.toml`).

```toml
# Stdio transport (local subprocess)
[[mcp]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
[mcp.env]
NODE_ENV = "production"

# Streamable HTTP transport (remote)
[[mcp]]
name = "remote-api"
transport = "streamable-http"
url = "https://example.com/mcp"
```

Changes to MCP servers can be reloaded with `reload` tool in `light` mode.

## TTS (Text-to-Speech)

```toml
[tts]
provider = "minimax"            # Currently only "minimax" supported
api_key = "${MINIMAX_API_KEY}"
model = "speech-2.8-hd"        # Default: "speech-2.8-hd"
# voice_id = "female-shaonv"   # Optional voice identifier
```

Leave `provider` empty or omit the section to disable TTS.

## STT (Speech-to-Text)

```toml
[stt]
provider = "groq"               # Currently only "groq" supported
api_key = "${GROQ_API_KEY}"
model = "whisper-large-v3-turbo" # Default: "whisper-large-v3-turbo"
```

Used for Telegram voice message transcription.

## Reflection Memory Loop

```toml
[reflection]
enabled = false
llm_model = "openai@${OPENAI_REFLECTION_MODEL}"
max_tokens = 2048
embedding_model = "openai@${OPENAI_EMBEDDING_MODEL}" # optional
embedding_dim = 1536                                 # optional
qdrant_url = "http://qdrant:6333"
qdrant_api_key = ""
```

- `llm_model`: Reflection analysis model (`provider_id@model_name`).
- `embedding_model`: Optional embedding model (`provider_id@model_name`), defaults to `<llm provider>@text-embedding-3-small`.
- `qdrant_url`: Qdrant endpoint for reflection guideline collection.
- `qdrant_api_key`: Optional Qdrant API key. Empty string means no API key header is sent.

## Memory Service (MemBurrow)

```toml
[memory_service]
enabled = true
url = "http://memory-service:8080" # Docker compose service DNS; use localhost in bare-metal runs
auth_token = "${MEMBURROW_AUTH_TOKEN}"
tenant_id = "default"
default_entity_id = "user_default"
process_id = "planner"
recall_timeout_ms = 3000
recall_top_k = 8
recall_min_score = 0.6
ingest_batch_turns = 3
ingest_timeout_ms = 10000
ingest_max_retries = 2
```

- `recall_min_score`: Memories below this score are excluded from LLM context injection.
- `ingest_batch_turns`: Batch multiple turns before one ingest request; set to `1` for strict per-turn idempotency.
- Memory API uses `Authorization: Bearer <auth_token>` and `X-Tenant-ID: <tenant_id>`.

## Docs Search (Qdrant)

```toml
[docs_search]
enabled = true
embedding_model = "openai@${OPENAI_EMBEDDING_MODEL}"
embedding_dim = 1536
qdrant_url = "http://qdrant:6333"
qdrant_api_key = ""
collection = "zerda_docs_index"
docs_dir = "docs/zerda"
```

- `embedding_model`: `provider_id@model_name`, provider is resolved from `[providers.<id>]`.
- `embedding_dim`: Embedding vector dimension used to create/query the collection.
- `qdrant_api_key`: Optional. If empty string, Zerda sends no API key header to Qdrant.
- `docs_dir`: Markdown source directory to index.
- On first startup, Zerda auto-indexes docs; later startups run incremental sync.

## Logging

```toml
[log]
level = "info"   # "trace" | "debug" | "info" | "warn" | "error"
```

## Environment Variable Expansion

Config values support `${VAR}` syntax for environment variable expansion. This is processed before TOML parsing, so it works in all string values.

```toml
api_key = "${OPENAI_API_KEY}"
token = "${TELEGRAM_BOT_TOKEN}"
```
