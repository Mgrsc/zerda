# LLM Providers

## Supported Providers

### OpenAI Chat Completions (`openai_chat`)

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"   # Optional
```

- Endpoint: `{base_url}/chat/completions`
- Supports: text, images (vision), tools, streaming
- Compatible models: gpt-4o, gpt-4o-mini, gpt-5.2, and any OpenAI-compatible API

### OpenAI Responses (`openai_responses`)

```toml
[providers.openai_r]
type = "openai_responses"
api_key = "${OPENAI_API_KEY}"
```

- Endpoint: `{base_url}/responses`
- Extended reasoning support
- Same model compatibility as Chat Completions

### Anthropic Messages (`anthropic`)

```toml
[providers.anthropic]
type = "anthropic"
api_key = "${ANTHROPIC_API_KEY}"
# base_url = "https://api.anthropic.com"  # Optional
```

- Endpoint: `{base_url}/v1/messages`
- Supports: extended thinking (reasoning blocks), vision, tools, streaming
- Compatible models: claude-3-5-sonnet, claude-4-sonnet, claude-4-opus

## Model Configuration

Models are referenced as `provider_id@model_name`:

```toml
[agent.primary_model]
model = "openai@gpt-4o"
vision = true
temperature = 0.7
top_p = 1.0
max_tokens = 4096

[agent.fast_model]
model = "openai@gpt-4o-mini"
vision = true
```

### Parameters

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| `model` | - | required | `provider_id@model_name` |
| `vision` | bool | true | Enable image processing |
| `temperature` | 0.0-2.0 | provider default | Sampling temperature |
| `top_p` | 0.0-1.0 | provider default | Top-p sampling |
| `max_tokens` | >0 | provider default | Max output tokens |

## Primary vs Fast Model

- **Primary model**: Main reasoning agent (Planner). Handles user interaction, planning, tool orchestration.
- **Fast model**: Used for context compression and executor subagent. Falls back to primary model if not configured.

## Using OpenAI-Compatible APIs

Any API that implements the OpenAI Chat Completions protocol can be used by setting `base_url`:

```toml
[providers.local]
type = "openai_chat"
api_key = "not-needed"
base_url = "http://localhost:11434/v1"  # e.g., Ollama
```

## Custom Headers

```toml
[providers.custom]
type = "openai_chat"
api_key = "${API_KEY}"
extra_headers = { "X-Custom-Header" = "value" }
```

## Retry Configuration

All providers support retry with exponential backoff:

```toml
[providers.openai.retry]
max_retries = 3
base_delay_ms = 2000
max_delay_ms = 30000
connect_timeout_secs = 10
request_timeout_secs = 120
```

Rate-limited responses (HTTP 429) are automatically retried.
