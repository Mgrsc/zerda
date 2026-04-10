# Configuration Reference

## Providers

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"

[providers.openai.retry]
max_retries = 3
base_delay_ms = 2000
max_delay_ms = 30000
connect_timeout_secs = 10
request_timeout_secs = 120
```

Supported provider types:

- `openai_chat`
- `openai_responses`
- `anthropic`

## Agent

```toml
[agent]
max_history = 30
tool_timeout = 300
primitive_timeout = 300
disabled_primitives = []
session_cleanup_days = 7
identity_path = "~/.zerda/identity.md"

[agent.primary_model]
model = "openai@gpt-4o"
vision = true

[agent.fast_model]
model = "openai@gpt-4o-mini"
vision = true
```

- `tool_timeout`: per-PTC-job timeout in seconds
- `primitive_timeout`: per-primitive hard timeout in seconds; defaults to `tool_timeout` when omitted
- `disabled_primitives`: blacklist of Python primitives
- `fast_model`: used for compression when configured

## Channels

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = ["*"]
max_message_length = 4096
draft_stream_update_interval_ms = 350
```

Currently supported channel kinds:

- `telegram`
- `wechat`

## STT

```toml
[stt]
provider = "groq"
api_key = "${GROQ_API_KEY}"
model = "whisper-large-v3-turbo"
```

## Memory

```toml
[memory]
enabled = true

[memory.embedding]
base_url = "${OPENAI_BASE_URL}"
api_key = "${OPENAI_API_KEY}"
model = "text-embedding-3-small"
dimensions = 1536
timeout_ms = 5000

[memory.sqlite]
path = "~/.zerda/memory/ema.sqlite3"

[memory.chroma]
url = "http://127.0.0.1:8000"
```

Current behavior:

- initializes the EMA SQLite schema and Chroma collections on demand
- recalls active memories into user turns
- journals completed turns, including runtime-backed problem-solving turns
- asynchronously extracts personal memories plus operational procedures/failure patterns, consolidates insights, and decays stale entries

Deployment note:

- Bare-metal default Chroma URL: `http://127.0.0.1:8000`
- Bundled Compose Chroma URL: `http://chroma:8000`
- The repository `zerda.toml` enables memory by default for the bundled Compose stack

## Logging

```toml
[log]
level = "info"
format = "json"
debug_plaintext = false
stream_progress_interval_ms = 2000
include_target = true
```
