# zerda

A lightweight Rust-based agent runtime — CLI and Telegram channels, MCP tool integration, long-session memory, streaming responses, and hot reload support.

## Features

**Providers**

- `openai_chat`, `openai_responses`, `anthropic`

**Built-in Tools**

- `shell`, `read`, `write`, `memory`, `reload`, `skill`, `tts`, `subagent`

**Extensibility**

- MCP tool layer via `stdio` and `streamable-http` transports

**Channels**

- CLI and Telegram (text / image / voice, optional STT / TTS)

**Sessions**

- Auto-save, resume, history compaction, old-session cleanup

**Configuration**

- Environment variable substitution, light reload, full restart reload

## Quick Start

1. Copy and fill environment variables:
```bash
cp .env.example .env
```

2. Update `zerda.toml` as needed. If you use MCP, copy `mcp.toml.example` to `mcp.toml`.
   In Docker mode, also mount `mcp.toml` to `/root/.zerda/mcp.toml` (an example line is included in `docker-compose.yml`).

3. Start with Docker Compose:
```bash
docker compose up -d --build
```
`docker-compose.yml` mounts a named volume `zerda-data` to `/root/.zerda`, so sessions, memory, and skills persist across container rebuilds.
The container reads config from `/root/.zerda/zerda.toml`.

4. Or run directly on host:
```bash
cargo run -- run
```

## Configuration

| File | Description |
|------|-------------|
| `.env.example` | Environment template |
| `zerda.toml` | Main config template |
| `mcp.toml.example` | MCP config template |

## Docker Image

- GitHub Actions workflow: `.github/workflows/docker-image.yml`
- Builds multi-arch images and pushes to `ghcr.io/mgrsc/zerda` by default

## License

MIT. See `LICENSE`.
