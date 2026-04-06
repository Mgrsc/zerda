# Providers

## Supported Providers

### OpenAI Chat Completions

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"
```

- Endpoint: `{base_url}/chat/completions`
- Supports: text, images, streaming

### OpenAI Responses

```toml
[providers.openai_r]
type = "openai_responses"
api_key = "${OPENAI_API_KEY}"
```

- Endpoint: `{base_url}/responses`
- Supports: text, images, streaming

### Anthropic Messages

```toml
[providers.anthropic]
type = "anthropic"
api_key = "${ANTHROPIC_API_KEY}"
```

- Endpoint: `{base_url}/messages`
- Supports: text, vision, streaming, thinking blocks

## Model References

Models use `provider_id@model_name`:

```toml
[agent.primary_model]
model = "openai@gpt-4o"
vision = true
```

## Notes

- Primary model handles user-facing turns.
- Fast model is used for compression.
- Providers do not receive tool schemas; PTC is emitted as text and intercepted by the host runtime.
