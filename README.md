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

Load environment variables before starting Zerda:

```bash
set -a
source ~/.zerda/.env
set +a
```

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

If Zerda and `wechat-agent-gateway` run in the same Docker Compose stack, use:

```toml
[[channels]]
name = "wechat"
gateway_url = "http://wechat-agent-gateway:8080"
```

Bundled Compose config:

- The repository `zerda.toml` is the Compose-ready config.
- It enables EMA memory by default.
- It points Chroma to `http://chroma:8000`.

Optional settings:

```bash
ZERDA_CONFIG=~/.zerda/zerda.toml
ZERDA_PRIMITIVES_ROOT=/absolute/path/to/code_primitives/python
```

Notes:

- `agent.tool_timeout` is the timeout for one background PTC job.
- `memory` enables EMA memory: hot-path recall uses embeddings plus local reranking, while completed turns are buffered before asynchronous extraction and consolidation with the fast model.
- The maintained default embedding path reuses `OPENAI_API_KEY` and `OPENAI_BASE_URL`; switch `memory.embedding.api_key` only if embeddings should use a separate provider credential.
- EMA keeps both personal memory and operational memory. Personal durable memory stores events, commitments, preferences, profile facts, and constraints backed by exact user-authored quotes; operational durable memory stores reusable procedures and failure patterns only when backed by exact assistant/runtime evidence from a completed turn.
- Ordinary EMA recall prioritizes profile facts, commitments, preferences, constraints, events, and insights; troubleshooting queries additionally prioritize failure patterns and procedures.
- Personal durable memory does not store procedures, and operational maintenance can consolidate repeated procedures or failure patterns into higher-level operational insights.
- Failure patterns are recalled only for troubleshooting-style queries, not for ordinary memory prompts.
- Low-value active memories that stay unused for long enough can be archived automatically, while frequently reused memories decay more slowly.
- EMA currently uses one global single-user memory entity, so all sessions share the same long-term memory space.
- `agent.disabled_primitives` disables named Python primitives such as `shell` or `process_spawn`.
- Core primitives ship under `code_primitives/python/primitives/`.
- `code_primitives/python/primitives/catalog.py` is the registration point for exposed built-in primitives.
- All non-core primitives live under `custom_primitives/`.
- `custom_primitives/catalog.py` is the registration point for exposed custom primitives, and implementations may be grouped under subdirectories such as `custom_primitives/agent_browser/` and `custom_primitives/firecrawl/`.
- In the bundled Compose deployment, mount that directory into `/root/.zerda/` because runtime discovery resolves it relative to Zerda's working directory.
- The first system prompt block is `<PTC_AVALIABLE_PRIMITIVES>`, generated dynamically at startup from the currently enabled top-level public primitive names.
- That block includes both core and custom public names such as `fs_read`, `process_run`, `firecrawl_search_web`, `scrapling_fetch_page`, and `agent_browser`.
- The intended web split is: Firecrawl for search and URL discovery, Scrapling for page fetching, and `agent_browser` for interactive validation and testing.
- `scrapling_fetch_page` automatically routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body extraction and light UI-noise filtering are applied before returning text.
- For selected dynamic-content domains such as Reddit, Zhihu, and Juejin, `scrapling_fetch_page` now tries static fetch first and automatically retries with stealth fetch when the static result looks like a shell page or lacks usable content.
- `scrapling_fetch_page` requires the Python runtime to have `scrapling[fetchers]`. Without it, the primitive returns `dependency_missing`.
- `scrapling_fetch_page` may internally use a stealth browser-backed path for selected dynamic domains and therefore also needs Playwright browser binaries when that internal fallback path is required.
- Prompt-visible primitive names are resolved from the same primitive root settings used by PTC job bootstrap, so startup discovery and runtime availability stay aligned.
- The built-in `shell` primitive uses `command=` as its primary argument and also accepts the common alias `cmd=` for compatibility.
- The model is expected to inspect `<PTC_AVALIABLE_PRIMITIVES>` first, then call `help("name")` whenever callable shape, method list, or parameter meaning is unclear.
- PTC primitives can also expose progressive guidance through `get_workflow`, which is intended for whole-tool setup and operational flow rather than raw parameter discovery.
- For setup-sensitive or multi-step tasks, the assistant should inspect `get_workflow` before operational code and then execute the workflow step by step rather than writing one large dependent script.
- Generic execution guidance should stay tool-agnostic: later steps should only run after earlier steps have succeeded and produced the state or identifiers they depend on.
- `agent_browser` is the bundled browser namespace. Public PTC usage should prefer methods such as `agent_browser.connect_cdp(...)`, `agent_browser.snapshot()`, and `agent_browser.get_title()`.
- `agent_browser.get_workflow()` is optional but recommended when browser setup may be missing. It returns a standalone Markdown workflow that includes installation and the loop of CDP attachment, snapshot, interaction, wait, re-snapshot, and data reads.
- `agent_browser` now keeps a default browser session per Zerda conversation after a successful `connect_cdp`, so later browser actions can reuse the same attached browser even when the model omits `session`.
- Browser-specific invalid-argument responses may now include corrective data such as allowed `kind` values, missing required parameters, and example calls.
- `agent_browser.close()` is explicit cleanup only and should not be treated as the default end of a browser task, because the connected browser often belongs to the user.
- Browser screenshots written through `agent_browser` go into the current PTC artifact directory.
- Python execution protocol now uses a single `<PTC_TOOL_CALLING>` block with direct body code instead of nesting `<PYTHON>` inside it.
- PTC bodies already run inside the runtime event loop; they should use `await` directly and must not call `asyncio.run()`.
- Every non-trivial PTC body should assign the final value to `result` explicitly so the runtime does not persist `null`.
- The runtime accepts only the exact `<PTC_TOOL_CALLING>` tag for normal execution; malformed provider-style wrappers are treated as prompt errors rather than normalized.
- When a PTC `out.json` payload exceeds 8,000 characters, Zerda asks `agent.fast_model` to compress the inline reinjected result using the original main-model request context; the full raw artifact remains on disk at the reported `OUT_PATH`.
- The WeChat channel does not speak the WeChat protocol directly. It talks to [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway) over HTTP.
- In the bundled Compose deployment, Zerda should reach the gateway by service name (`http://wechat-agent-gateway:8080`), not `127.0.0.1`.
- Zerda treats each WeChat gateway instance as single-account. If the gateway contains multiple configured accounts, startup fails instead of choosing one implicitly.
- When the WeChat channel starts and no persisted account exists, Zerda prints a terminal QR code in startup logs and waits for scan confirmation.
- Empty WeChat poll responses keep the last pull cursor so older inbound messages are not replayed on the next poll.
- WeChat reply chunking is now tuned to keep short and medium replies in one bubble when possible and avoid dangling endings such as `for example:`.
- To avoid scanning again after each restart, run the gateway with a stable `WECHAT_GATEWAY_STATE_PATH` volume or host path. Zerda does not store WeChat login state itself.
- WeChat voice messages use the transcript already returned by the gateway. Zerda's `[stt]` config is not required for WeChat voice input.
- WeChat outbound image replies support the same rich marker format as Telegram: `<image>/absolute/path.png</image>`. Zerda uploads the local file to `wechat-agent-gateway` and sends it through `send_media`.

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
