# Telegram Integration

## Configuration

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = ["*"]
max_message_length = 4096
draft_stream_update_interval_ms = 350
```

## Start

```bash
zerda serve
```

## Features

- Per-user session isolation
- Voice message transcription through configured STT
- Image input forwarding for vision-capable models
- MarkdownV2-safe rendering
- Long-message splitting
- Typing indicators
- Streaming drafts in supported chat contexts

## Rich Media Tags

The channel renderer understands:

- `<image>URL_OR_PATH</image>`
- `<voice>PATH</voice>`

These tags must only reference real artifact paths or URLs produced by the runtime. They must never be fabricated in prose.
