# zerda

Repository: `https://github.com/Mgrsc/zerda`

`zerda` is a lightweight Rust-based agent runtime with CLI and Telegram channels, MCP tool integration, long-session memory, streaming responses, and hot reload support.

## Features

- Multi-provider support: `openai_chat`, `openai_responses`, `anthropic`
- Built-in tools: `shell`, `read`, `write`, `memory`, `reload`, `skill`, `tts`, `subagent`
- Extensible tool layer via MCP (`stdio` / `streamable-http`)
- Channel support: CLI and Telegram (text/image/voice, optional STT/TTS)
- Session support: auto-save, resume, history compaction, old-session cleanup
- Config support: environment variable substitution, light reload, full restart reload

## Configuration Files

- Environment template: `.env.example`
- Main config template: `zerda.toml`
- MCP config template: `mcp.toml.example`

## Deployment

1. Copy and fill environment variables
```bash
cp .env.example .env
```

2. Update `zerda.toml` as needed. If you use MCP, copy `mcp.toml.example` to `mcp.toml`.
In Docker mode, also mount `mcp.toml` to `/root/.zerda/mcp.toml` (an example line is included in `docker-compose.yml`).

3. Start with Docker Compose
```bash
docker compose up -d --build
```
`docker-compose.yml` mounts a named volume `zerda-data` to `/root/.zerda`, so sessions, memory, and skills persist across container rebuilds.
The container reads config from `/root/.zerda/zerda.toml`.

4. Run directly on host
```bash
cargo run -- run
```

## Image Build

- Included GitHub Actions workflow: `.github/workflows/docker-image.yml`
- Builds multi-arch images and pushes to `ghcr.io/mgrsc/zerda` by default

## License

MIT. See `LICENSE`.
