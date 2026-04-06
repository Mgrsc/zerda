# Zerda

[English](./README.md) | [简体中文](./README.zh-CN.md)

## What is this project and why would I use it?

Zerda is a Rust AI agent runtime built around one assistant dialogue loop plus asynchronous Python-based PTC jobs. Use it when you want the model to keep responding while filesystem work, subprocesses, and web primitives continue in the background.

Migration note:

- ~~Provider-level tools~~ have been removed.
- ~~MCP integration~~ has been removed with the tool system.
- ~~Skills~~ have been removed and will be replaced later by Playbook.
- Zerda now uses PTC as the only built-in execution path for mechanical work.

## What are the prerequisites?

- Rust 1.75+ for source builds
- Python 3 on the host for PTC job execution
- At least one model provider credential such as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`
- `TELEGRAM_BOT_TOKEN` only if you enable the Telegram channel
- A separately deployed `wechat-agent-gateway` only if you enable the WeChat channel
- `GROQ_API_KEY` only if you want Telegram voice transcription
- A reachable Chroma server because EMA memory is enabled in the maintained configs; the bundled Compose stack starts it for you
- A separate embedding API key only if EMA should use a different endpoint or credential than `OPENAI_API_KEY`
- `FIRECRAWL_API_KEY` only if PTC jobs need external Firecrawl search primitives
- `scrapling[fetchers]` only if PTC jobs need `scrapling_fetch_page`
- Playwright browser binaries only if PTC jobs need Scrapling stealth fallback inside `scrapling_fetch_page`
- `agent-browser` plus `agent-browser install` only if PTC jobs need interactive browser or remote CDP primitives

Bundled Docker image note:

- The repository `Dockerfile` now provisions a dedicated Python virtual environment with `scrapling[fetchers]`, `playwright`, and Chromium so bundled Scrapling fetch primitives work inside locally built images without extra manual setup.

## How do I install/set up locally?

```bash
git clone https://github.com/Mgrsc/zerda.git
cd zerda
cargo build --release
```

Prepare runtime files:

```bash
mkdir -p ~/.zerda
cp zerda.toml.full ~/.zerda/zerda.toml
cp identity.md ~/.zerda/identity.md
cp .env.example ~/.zerda/.env
```

Generate the full config template if needed:

```bash
./target/release/zerda config generate > ~/.zerda/zerda.toml
```

## How do I configure it?

Zerda expands `${VAR}` placeholders in `zerda.toml` from the process environment.

In practice, keep secrets in `.env` and load that file before starting:

```bash
set -a
source ~/.zerda/.env
set +a
```

If you use Docker Compose, `.env` is loaded automatically.

Minimal runtime config:

```toml
[providers.openai]
type = "openai_chat"
api_key = "${OPENAI_API_KEY}"

[agent]
tool_timeout = 300
disabled_primitives = []

[agent.primary_model]
model = "openai@${OPENAI_MODEL}"

[agent.fast_model]
model = "openai@${OPENAI_FAST_MODEL}"

[memory]
enabled = true

[memory.embedding]
base_url = "${OPENAI_BASE_URL}"
api_key = "${OPENAI_API_KEY}"
model = "text-embedding-3-small"
dimensions = 1536

[memory.chroma]
url = "http://127.0.0.1:8000"
```

Optional Telegram channel:

```toml
[[channels]]
name = "telegram"
token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = []
max_message_length = 4096
draft_stream_update_interval_ms = 350
```

Optional WeChat channel:

```toml
[[channels]]
name = "wechat"
gateway_url = "http://127.0.0.1:8080"
```

If Zerda and `wechat-agent-gateway` run in the same Docker Compose stack, use the service name instead of `127.0.0.1`:

```toml
[[channels]]
name = "wechat"
gateway_url = "http://wechat-agent-gateway:8080"
```

Optional settings:

```bash
ZERDA_CONFIG=~/.zerda/zerda.toml
ZERDA_PRIMITIVES_ROOT=/absolute/path/to/code_primitives/python
```

Notes:

- `zerda.toml` is the Compose-ready config in this repo. `zerda.toml.full` is the fuller template for custom or bare-metal deployments.
- The maintained defaults reuse `OPENAI_API_KEY` and `OPENAI_BASE_URL` for embeddings, so most setups only need one `.env` file plus a matching TOML config.
- `agent.tool_timeout` controls the timeout for one background PTC job.
- EMA memory is enabled in the maintained configs and needs a reachable Chroma server.
- `ZERDA_PRIMITIVES_ROOT` is only needed if you want to override the default primitive discovery path.
- Custom primitives live under `custom_primitives/`; built-in ones live under `code_primitives/python/primitives/`.
- WeChat uses [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway), not the WeChat protocol directly.
- To avoid rescanning the QR code after restart, persist the gateway state with `WECHAT_GATEWAY_STATE_PATH`.
- WeChat voice input uses the transcript returned by the gateway; Zerda's `[stt]` config is not required for that path.

## How do I run it?

Interactive CLI:

```bash
./target/release/zerda --config ~/.zerda/zerda.toml
```

Single-turn execution:

```bash
./target/release/zerda --config ~/.zerda/zerda.toml run -m "List the Rust files in this repo"
```

Resume the latest saved session:

```bash
./target/release/zerda --config ~/.zerda/zerda.toml run --resume
```

Start configured remote channels:

```bash
./target/release/zerda --config ~/.zerda/zerda.toml serve
```

Inspect runtime job state during a session:

```text
/jobs
/job <id>
/cancel-job <id>
```

Busy-session command behavior:

- `/status`, `/jobs`, `/job <id>`, and `/cancel-job <id>` respond immediately while a reply is streaming.
- `/compact` is queued and runs after the current turn finishes.
- `/clear` and `/model <provider>@<model>` cancel the current turn first, then run.

Validate configuration before launch:

```bash
./target/release/zerda --config ~/.zerda/zerda.toml config validate
```

## How do I run the tests?

```bash
cargo test
```

```bash
cargo clippy --all-targets --all-features
```

## How do I deploy it?

Build a release binary and run it under a supervisor:

```bash
cargo build --release
./target/release/zerda --config /path/to/zerda.toml serve
```

Container deployment:

```bash
cp .env.example .env
docker compose up -d --build
```

Operational notes:

- PTC job artifacts are written under `~/.zerda/ptc_jobs/`.
- Detached jobs require Python 3 on the target host.
- Use `systemd`, `supervisord`, or another supervisor for long-running `serve` deployments.
- If you enable WeChat, deploy `wechat-agent-gateway` as a separate long-running service or sidecar in the same compose stack and keep its state path persistent.
- The repository `docker-compose.yml` starts Chroma and the repository `zerda.toml` enables EMA memory against `http://chroma:8000`.

## Where do I get help or report issues?

## For AI Agents

See [AGENT_README.md](./AGENT_README.md) for operational context.

Repository language convention:

- code-facing assets stay in English
- localized end-user documentation lives in `README.zh-CN.md`

Project docs and templates:

```bash
AGENT_README.md
zerda.toml.full
dev-file/design/
```

Technical implementation docs:

```bash
docs/zerda/architecture.md
docs/zerda/code-primitives.md
dev-file/design/ptc-async-runtime-redesign.md
dev-file/scratch/future-ptc-primitives.md
```

Issue tracker:

```bash
https://github.com/Mgrsc/zerda/issues
```
