# Telegram Integration

## Configuration

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = ["*"]           # ["*"] = all users, or list specific user IDs
max_message_length = 4096       # Default: 4096
draft_stream_update_interval_ms = 350  # Default: 350 (sendMessageDraft update interval)
```

## Starting the Bot

```bash
zerda serve
```

This starts the Telegram bot listener and runs indefinitely.

## Features

- Multi-user support with per-user session isolation
- Voice message transcription (requires STT configuration)
- Voice message synthesis (requires TTS configuration)
- Image handling (upload images for vision-enabled models)
- MarkdownV2 formatted responses
- Automatic message splitting for long responses (respects code blocks and links)
- Typing indicators during processing
- Streaming drafts in private chats (and private chat topics), with non-stream fallback in groups

## Access Control

- `allowed_users = ["*"]`: Accept messages from all users
- `allowed_users = ["123456", "789012"]`: Only accept from listed Telegram user IDs

## Rich Media

The agent can output special tags that Telegram renders as rich media:

- `<image>URL_OR_PATH</image>` - Sends an image
- `<voice>PATH</voice>` - Sends a voice message (from TTS tool)

These tags are only valid when produced by tools with actual file paths. The agent is instructed never to fabricate them.

## Voice Messages

### Receiving (STT)

When a user sends a voice message, it is automatically transcribed using the configured STT provider (Groq Whisper) and treated as text input.

### Sending (TTS)

The agent can use the `tts` tool to generate voice responses. The tool returns a `<voice>` tag that Telegram channel automatically renders as a voice message.

## Message Formatting

Telegram channel uses MarkdownV2 with safety normalization:

- No heading markdown (`#`, `##`, `###`)
- Inline bold/italic for emphasis
- Code blocks for logs, tables, structured output
- Concise, colloquial messaging style
- Progressive disclosure: summary first, full details on request
