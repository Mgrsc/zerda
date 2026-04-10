# AGENT_README

## Purpose

This file is the operational entry point for agents working in the Zerda repository. It describes the current runtime after the full migration from provider-level tools and MCP to PTC.

Code-facing repository assets should stay in English. Localized end-user documentation remains in `README.zh-CN.md`.

## System Overview

Zerda is a single-assistant runtime.

- Providers receive only `system`, `user`, and `assistant` messages.
- The assistant requests Python-based mechanical work by emitting `<PTC_TOOL_CALLING>` blocks.
- Primitive availability is injected into the first system prompt part as `<PTC_AVALIABLE_PRIMITIVES>`.
- The host intercepts those blocks, launches detached Python jobs, and injects results back as runtime-originated `user` messages.
- There is no provider-level tool calling, no MCP integration, and no Skills system.
- Primitive discovery now starts from prompt-visible top-level public names plus runtime `help(...)`.
- Tools may still expose `get_workflow` for end-to-end setup guidance, installation steps, and recommended operational order.
- Telegram and gateway-backed WeChat are the currently supported remote channels.
- EMA memory is implemented as one global single-user entity: hot-path recall uses embeddings plus deterministic reranking, ordinary recall prioritizes profile facts/commitments/preferences/constraints/events/insights, troubleshooting recall additionally prioritizes failure patterns and procedures, completed turns are buffered before async extraction and consolidation with the fast model, personal durable memory only accepts exact user-authored quotes for events/commitments/preferences/profile facts/constraints, operational durable memory only accepts reusable procedures or failure patterns backed by exact assistant/runtime quotes, operational maintenance can synthesize higher-level operational insights, and low-value long-unused active memories may be archived automatically.

## Architecture And Component Map

| Component | File(s) | Purpose | Inputs | Outputs | Dependencies | Failure behavior | Verification | Debugging |
|---|---|---|---|---|---|---|---|---|
| Bootstrap | `src/main.rs` | Parse CLI, load config, initialize runtime, dispatch mode | CLI args, env vars, config | interactive loop, serve loop, single turn | `clap`, config, providers, runner | exits on invalid config or provider init | `zerda config validate` | inspect startup logs |
| Turn orchestrator | `src/runner.rs` | Build user turns, stream responses, inject runtime events | channel messages, provider | assistant replies, saved sessions, runtime result messages | channels, agent, job manager | logs and keeps serving; sends text error to user on turn failure | interactive session or `serve` | inspect `runner.*` logs |
| Assistant core | `src/agent.rs` | One-turn provider call, compaction, PTC extraction | history, system prompt, provider response | visible assistant text, parsed PTC requests, parse notices | provider trait, fast model sidecar | malformed hidden payload becomes runtime notice | one prompt in CLI | inspect saved session JSON |
| PTC parser | `src/ptc/parser.rs` | Parse exact `<PTC_TOOL_CALLING>` payloads | hidden XML payload | `Vec<PtcRequest>` | regex | malformed payload becomes runtime notice | feed sample XML | inspect runtime notice text |
| PTC stream interceptor | `src/ptc/stream_interceptor.rs` | Hide hidden PTC XML after first tag | streaming text deltas | visible text + hidden XML | none | partial malformed tag can leak only if provider never completes it | streamed PTC reply | inspect hidden payload handling |
| Primitive index | `src/ptc/primitive_index.rs` | Scan internal primitive files plus custom primitive registrations, parse callable metadata, and build prompt-visible public primitive listings | primitive files, `custom_primitives/catalog.py`, disabled list | indexed primitive metadata and available public names | filesystem, regex | prompt-visible primitive list can degrade to empty | inspect prompt output and primitive paths | inspect primitive paths and metadata |
| PTC job manager | `src/ptc/job_manager.rs` | Launch detached Python jobs, track status, compress oversized inline results, inject completion | parsed requests, session context | artifacts, status files, runtime result messages | `python3`, filesystem, primitives roots, primitive index, fast-model sidecar | start failures become notices; timeout becomes timed_out result | `/jobs`, `/job <id>` | inspect `~/.zerda/ptc_jobs/*` |
| Provider adapters | `src/providers/*` | Chat and stream integration for supported APIs | history, model options | `ProviderResponse` or stream events | provider HTTP APIs | request errors bubble up to runner | send one prompt | inspect provider logs |
| Channel adapters | `src/channels/*` | CLI, Telegram, and gateway-backed WeChat transport | stdin, Telegram updates, or WeChat gateway events | `ChannelMessage` and outbound replies | terminal, Telegram API, or WeChat gateway HTTP API; optional STT | init/send failures logged | local CLI, Telegram message, or WeChat message | inspect channel logs |
| EMA memory runtime | `src/memory/*` | Recall active personal/operational memory, journal completed turns, extract structured memories, consolidate insights, decay stale entries | memory config, completed turn content, fast-model sidecar | recalled prompt blocks, SQLite state, Chroma active index | rusqlite, reqwest, filesystem, fast model provider | warns and degrades gracefully when recall or maintenance fails | inspect SQLite file and logs | inspect memory recall/maintenance logs |
| Prompt builder | `src/prompt.rs`, `src/prompts/system_ptc_rules.md` | Build system prompt and env block | identity text, channel supplement | prompt parts | filesystem, env | oversized prompt warns | inspect built prompt | verify included cwd and rules |
| Python primitives | `code_primitives/python/*`, `custom_primitives/*` | Async filesystem, process, shell, web lookup, and browser-interaction primitives for PTC jobs | Python calls from PTC code | structured JSON results | Python 3, OS tools, optional Firecrawl, optional `agent-browser` | primitive returns structured error object | run through PTC | inspect `out.json`, `log.txt`, `telemetry.jsonl` |

## Public Interfaces

### CLI commands

| Command | Purpose | Outputs | Failure behavior | Verification | Debugging |
|---|---|---|---|---|---|
| `zerda` | interactive chat | streamed assistant replies | exits on startup failure | start local session | inspect terminal and logs |
| `zerda run -m "<text>"` | one-shot turn | one assistant response and optional detached PTC jobs | exits on fatal startup/provider error | compare stdout and artifacts | inspect stderr and job files |
| `zerda run --resume [id]` | resume saved session | continued session history | returns error if session missing or corrupt | resume latest session | inspect `~/.zerda/sessions/*.json` |
| `zerda serve` | start configured channels | long-running listeners | bails if no channels configured | start service and send message | inspect channel logs |
| `zerda config generate` | print full template | `zerda.toml.full` to stdout | none expected | compare output to template | inspect stdout |
| `zerda config validate` | validate config | success text or field error | exits `1` on invalid config | run before deployment | inspect reported field |

### In-session slash commands

| Command | Purpose | Failure behavior | Verification | Debugging |
|---|---|---|---|---|
| `/clear` | clear history and token counters | none expected | history resets | inspect next saved session |
| `/compact` | force fast-model compression | queued while a turn is running; returns error text on compression failure | summary appears | inspect `~/.zerda/memory/compaction/` |
| `/model` | show current model | none expected; still available during streaming | compare with config | inspect `/status` |
| `/model <provider>@<model>` | switch model in current session | cancels active turn first; returns error text if invalid | next turn uses new model | inspect `/status` |
| `/model <provider> list` | list provider models | queued while a turn is running; provider may not support it | compare returned list | inspect provider logs |
| `/status` | show session and runtime state | none expected; still available during streaming | inspect token usage and running jobs | compare with job files |
| `/jobs` | list PTC jobs | returns empty list if none | job ids appear | compare with `~/.zerda/ptc_jobs` |
| `/job <id>` | inspect one PTC job | returns not found if wrong session or id; still available during streaming | artifact paths appear | inspect `status.json` and `log.txt` |
| `/cancel-job <id>` | terminate a running job | returns not found if wrong session or id; still available during streaming | status becomes cancelled | compare `status.json` and result notice |
| `/help` | list commands | none expected | command list is printed | compare with `src/commands.rs` |
| `/cancel` | cancel current running turn | no effect when idle | current turn is rolled back | inspect cancel logs |

## PTC Runtime Contract

### Assistant-visible protocol

- The first protocol tag such as `<PTC_TOOL_CALLING` ends visible output for that assistant message.
- Hidden payload may contain one or more `<PTC_TOOL_CALLING>` blocks.
- `<PTC_TOOL_CALLING>` carries async Python body code directly.
- The first system prompt part is `<PTC_AVALIABLE_PRIMITIVES>`, generated from the currently enabled top-level public primitive names.
- Optional `purpose="..."` on `<PTC_TOOL_CALLING>` is used for job listing and runtime state.
- The runtime only accepts those exact tags. Wrong wrappers are treated as malformed payload.
- The assistant should inspect `<PTC_AVALIABLE_PRIMITIVES>` first, call `help("name")` when method or parameter details are unclear, and only then write the PTC call.
- If `help(...)` reveals a `get_workflow` entry, the assistant should use it when the tool is unfamiliar or setup-sensitive.
- If a task needs installation, connection setup, state reuse, resource attachment, or three or more dependent operations, the assistant should inspect `get_workflow` before writing operational code.
- Multi-step tasks should normally be executed as several small PTC calls with runtime feedback between steps, not as one large dependent script.
- PTC bodies already run inside the runtime event loop. They should use direct `await`, must not call `asyncio.run()`, and should explicitly assign the final payload to `result`.
- Generic execution guidance should remain tool-agnostic: later steps should run only after earlier steps have succeeded and produced the state or identifiers they depend on.
- For `agent_browser`, the workflow is Markdown loaded from a standalone file and describes installation plus an iterative browser loop rather than an automatic open-then-close lifecycle. The runtime keeps a default browser session scoped to the current Zerda conversation after a successful connect.
- This makes PTC both an execution layer and a progressive documentation layer: `help(...)` teaches callable contracts while `get_workflow` can teach tool-level usage patterns in-band before work begins.

### Python execution environment

PTC jobs run as `python3 <artifact_dir>/script.py` with:

- current working directory set to the Zerda process cwd
- stdout and stderr appended to `log.txt`
- result expected in `out.json`

Runtime-injected environment variables:

- `PTC_OUT_PATH`
- `PTC_LOG_PATH`
- `PTC_TELEMETRY_PATH`
- `PTC_ARTIFACT_DIR`
- `PTC_WORKING_DIR`
- `PTC_JOB_ID`
- `PTC_SESSION_KEY`
- `PTC_PRIMITIVES_PY_ROOT`
- `PTC_PRIMITIVES_PY_ROOTS`
- `PTC_DISABLED_PRIMITIVES`

PTC artifact notes:

- `out.json` keeps the full raw result.
- `request.txt` stores the original main-model request context used when the job was launched.
- If `out.json` exceeds 8,000 characters, the runtime asks `[agent.fast_model]` to compress the inline reinjected `<RESULT>` using `request.txt` plus the raw result.
- The emergency inline ceiling is 100,000 characters; if compression cannot produce an acceptable inline payload above that ceiling, the runtime falls back to a short pointer telling the assistant to inspect `OUT_PATH`.

### Primitive discovery and sources

Primitive sources:

- internal: `code_primitives/python/primitives/`
- custom: `custom_primitives/`

Deployment note:

- Runtime discovery resolves custom primitive roots relative to the Zerda working directory.
- In the bundled Compose deployment, mount that directory into `/root/.zerda/custom_primitives/`.

Custom primitive registration:

- `custom_primitives/catalog.py` is the registration point and source of truth for exposed custom primitives.
- `code_primitives/python/primitives/catalog.py` is the registration point and source of truth for exposed built-in primitives.
- Custom implementation files may be grouped into subpackages such as `custom_primitives/agent_browser/`, `custom_primitives/firecrawl/`, and `custom_primitives/smart_search/`.
- Prompt-visible primitives are included only if they are registered in their source catalog.

Prompt exposure policy:

- The earliest system prompt block is `<PTC_AVALIABLE_PRIMITIVES>`.
- That block lists all currently enabled top-level public primitive names from both core and custom sources, including built-ins such as `fs_read`.
- Prompt discovery uses the same primitive-root resolution rules as PTC job bootstrap so prompt-visible names and runtime availability stay aligned.
- The built-in `shell` primitive keeps `command` as the canonical parameter name but also accepts `cmd` as a compatibility alias.
- The model should inspect `<PTC_AVALIABLE_PRIMITIVES>` first, then use `help("name")` to discover methods and callable details before writing code.
- Clear public names plus `help(...)` are now the main discovery surface.
- Web routing is: `firecrawl_search_web` for URL discovery, `smart_search` for answer-style retrieval against a configured OpenAI-compatible `chat/completions` endpoint, Scrapling primitives for page fetching, and `agent_browser` for interactive validation and test flows.
- `scrapling_fetch_page` routes `mp.weixin.qq.com` article URLs to a WeChat-specific extractor and `x.com` / `twitter.com` URLs to the stealth fetch path, where tweet-body container extraction is preferred over full-page text.
- `scrapling_fetch_page` also performs one automatic static-to-stealth retry for selected dynamic-content domains when the static result looks like a shell page or lacks usable body text.
- `scrapling_fetch_page` depends on `scrapling[fetchers]` and `playwright` in the custom-primitives Python runtime; otherwise it returns `dependency_missing`.
- `scrapling_fetch_page` may internally switch to a stealth browser-backed path for selected dynamic domains; `run_zerda_with_deps.sh` installs Chromium automatically when `playwright` is declared in custom primitive requirements.
- Optional docstring metadata still helps indexing and maintenance, but primary discoverability no longer depends on a separate search-first flow.

## Configuration Reference

### File resolution

1. `--config`
2. `ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

### Active config sections

| Section / key | Type | Required | Default | Purpose | What breaks if wrong | Verification | Debugging |
|---|---|---|---|---|---|---|---|
| `[providers.<id>].type` | string | yes | none | provider adapter kind | startup fails | `config validate` | inspect provider kind |
| `[providers.<id>].api_key` | string | yes | none | provider auth | requests fail | `config validate` | inspect env expansion |
| `[providers.<id>].base_url` | string | no | provider default | endpoint override | requests hit wrong host | send one prompt | inspect provider logs |
| `[providers.<id>].extra_headers` | map | no | empty | custom headers | auth or routing mismatch | send one prompt | inspect HTTP logs |
| `[providers.<id>].retry.*` | integers | no | see template | request retry and timeout control | poor resiliency or long waits | provider request | inspect retry logs |
| `[agent].max_history` | integer | no | `30` | auto-compaction threshold | long sessions bloat history | chat until threshold | inspect compaction logs |
| `[agent].identity_path` | string | no | `~/.zerda/identity.md` | prepend identity to system prompt | wrong identity or missing file | startup and first prompt | inspect prompt parts |
| `[agent].session_cleanup_days` | integer | no | `7` | old session retention | sessions grow or are deleted too soon | inspect session dir | compare timestamps |
| `[agent].tool_timeout` | integer | no | `300` | per-PTC-job timeout seconds | jobs hang or terminate too early | launch long-running PTC | inspect `status.json` |
| `[agent].disabled_primitives` | list[string] | no | `[]` | disable named Python primitives | assistant loses primitive access | launch PTC job | inspect bootstrap globals |
| `[agent.primary_model].model` | string | yes | none | active provider@model | startup fails | `config validate` | inspect active model |
| `[agent.primary_model].vision` | bool | no | `true` | image handling | image messages degrade to text warning | send image | inspect turn payload |
| `[agent.primary_model].temperature/top_p/max_tokens` | numeric | no | unset | sampling/token control | provider may reject invalid values | one prompt | inspect provider error |
| `[agent.fast_model].*` | same as primary | no | fallback to primary | compaction and oversized-PTC-result compression sidecar model | `/compact` or long PTC reinjection may fail | `/compact` and long PTC job completion | inspect compaction logs and PTC runtime logs |
| `[[channels]].name` | string | no | none | channel kind | `serve` cannot start channel | `serve` | inspect channel registry |
| `[[channels]].token` | string | Telegram only | none | Telegram auth | channel init fails | `serve` | inspect Telegram logs |
| `[[channels]].allowed_users` | list | no | `[]` | Telegram filtering | unexpected user access or denial | send allowed and denied message | inspect Telegram filter logs |
| `[[channels]].max_message_length` | integer | no | channel default | Telegram message splitting | truncation or over-splitting | send long reply | inspect Telegram send logs |
| `[[channels]].draft_stream_update_interval_ms` | integer | no | `350` | Telegram streaming cadence | slow or noisy drafts | streamed reply | inspect update logs |
| `[[channels]].gateway_url` | string | WeChat only | `http://127.0.0.1:8080` | WeChat gateway base URL for the single-account integration; when Zerda and the gateway share one Compose stack, use `http://wechat-agent-gateway:8080` | channel cannot reach gateway, login flow, or startup rejects ambiguous multi-account state | `serve` with WeChat enabled | inspect bootstrap logs, gateway health, and `/v1/accounts` |
| `[stt].provider` | string | no | empty | enable voice transcription | voice transcription unavailable | send voice message | inspect STT init logs |
| `[stt].api_key` | string | provider-specific | unset | STT auth | STT request fails | send voice message | inspect provider logs |
| `[stt].model` | string | no | `whisper-large-v3-turbo` | transcription model | STT request may fail | send voice message | inspect STT logs |
| `[memory].enabled` | bool | no | `false` | enable EMA recall, journaling, and async maintenance | no EMA recall or maintenance | inspect logs and sqlite file | inspect init and recall logs |
| `[memory.embedding].base_url/api_key/model/dimensions/timeout_ms` | mixed | if memory enabled | see template | embedding endpoint configuration for EMA | memory init fails or future retrieval setup is invalid | startup with memory enabled | inspect init logs |
| `[memory.sqlite].path` | string | if memory enabled | `~/.zerda/memory/ema.sqlite3` | SQLite backing store path | schema/journal writes fail | inspect created file | inspect filesystem and logs |
| `[memory.chroma].url` | string | if memory enabled | `http://127.0.0.1:8000` | Chroma backing store URL; the repository `zerda.toml` uses `http://chroma:8000` for bundled Compose | memory init fails | startup with memory enabled | inspect init logs |
| `[log].level/format/debug_plaintext/stream_progress_interval_ms/include_target` | mixed | no | see template | runtime observability | too noisy or opaque logs | run any mode | inspect log output |

## Environment Variables

| Variable | Type | Required | Default | Used by | What breaks if wrong |
|---|---|---|---|---|---|
| `ZERDA_CONFIG` | path | no | unset | config discovery | wrong path prevents startup |
| `OPENAI_API_KEY` | string | no | unset | OpenAI providers | OpenAI requests fail |
| `OPENAI_MODEL` | string | no | unset | example config | startup fails if referenced and empty |
| `OPENAI_FAST_MODEL` | string | no | unset | fast-model example | compaction config invalid if referenced and empty |
| `ANTHROPIC_API_KEY` | string | no | unset | Anthropic provider | Anthropic requests fail |
| `TELEGRAM_BOT_TOKEN` | string | no | unset | Telegram channel | Telegram channel cannot start |
| `GROQ_API_KEY` | string | no | unset | STT | voice transcription unavailable |
| `FIRECRAWL_API_KEY` | string | no | unset | external Firecrawl search primitive | web primitive calls fail |
| `FIRECRAWL_BASE_URL` | URL | no | provider default | Firecrawl search primitive | requests hit wrong endpoint |
| `SMART_SEARCH_URL` | URL | no | unset | `smart_search` custom primitive | answer-style retrieval primitive cannot reach the configured endpoint |
| `SMART_SEARCH_API_KEY` | string | no | unset | `smart_search` custom primitive | answer-style retrieval primitive authentication fails |
| `SMART_SEARCH_MODEL` | string | no | unset | `smart_search` custom primitive | answer-style retrieval primitive cannot build a valid request |
| `AGENT_BROWSER_EXECUTABLE_PATH` | path | no | unset | external `agent-browser` runtime, when supported by the host installation | browser primitives may fail to launch the intended executable |
| `ZERDA_PRIMITIVES_ROOT` | path | no | auto-discovery | primitive bootstrap | primitives may not load |

Repository-maintained config template:

- `zerda.toml`
  - minimal Compose-ready config in the repository root
  - enables EMA memory and points Chroma to `http://chroma:8000`
- `zerda.toml.full`
  - full template for bare-metal or custom deployments

Runtime dependency note:

- `scrapling_fetch_page` does not use an environment variable, but it does require `scrapling[fetchers]` and `playwright` in the custom-primitives Python runtime. If those dependencies are absent, the primitive returns `dependency_missing`.
- `run_zerda_with_deps.sh` installs Chromium automatically into the custom-primitives cache when `playwright` is declared in merged custom primitive requirements.
- The repository Dockerfile also provisions `scrapling[fetchers]`, `playwright`, and Chromium for locally built images, so bundled Scrapling primitives should still be available there even before custom runtime bootstrapping runs.
  - runtime default resolution still targets `~/.zerda/zerda.toml`
  - typical bare-metal deployment copies `zerda.toml.full` to that runtime path

External WeChat gateway environment, configured on the gateway service rather than in Zerda:

- `WECHAT_GATEWAY_STATE_PATH`
  - must point to persistent storage if you do not want a fresh QR scan after each gateway restart
- `WECHAT_GATEWAY_URL`
  - should match the base URL Zerda uses in `[[channels]].gateway_url`
  - in the bundled Compose stack, Zerda should use `http://wechat-agent-gateway:8080` while the gateway container may still keep its own internal default

## Logging

- Process logs go to stdout or stderr.
- Supported formats:
  - `json`
  - `text`
- Key log families:
  - `runner.*`
  - provider request and retry warnings
  - `telegram.*`
  - WeChat bootstrap, login, and pull-loop logs from `src/channels/wechat.rs`

Files on disk:

- sessions: `~/.zerda/sessions/*.json`
- compaction snapshots: `~/.zerda/memory/compaction/*.txt`
- EMA SQLite: `~/.zerda/memory/ema.sqlite3`
- PTC artifacts: `~/.zerda/ptc_jobs/<date>/<job>/`
  - includes `out.json` for the raw result and `request.txt` for the originating main-model request context
- browser session state: `~/.zerda/agent_browser_state/*.json`

## Error Taxonomy

| Error class | Typical root cause | Recovery |
|---|---|---|
| Config validation error | bad provider ref, empty Telegram token, malformed model ref | fix config and rerun `config validate` |
| Provider request failure | bad API key, wrong base URL, timeout, rate limit | verify credentials and endpoint |
| PTC parse notice | malformed assistant XML or stray text after first PTC tag | inspect hidden payload logic and prompt rules |
| PTC job start failure | missing `python3`, bad primitives root, filesystem permission issue | inspect runtime notice and host environment |
| PTC job timeout | long-running process exceeded `agent.tool_timeout` | increase timeout or split work |
| PTC primitive error | invalid primitive args or upstream command failure | inspect `out.json` and `log.txt` |
| Browser primitive dependency missing | `agent-browser` CLI missing from PATH or Chrome not installed for it | install `agent-browser`, run `agent-browser install`, then retry |
| Channel startup or send failure | invalid Telegram token or network issue | verify token and inspect logs |
| WeChat gateway bootstrap failure | gateway unreachable, bad `gateway_url`, or gateway unhealthy | inspect gateway service and startup logs |
| Repeated WeChat QR login | ephemeral or reset `WECHAT_GATEWAY_STATE_PATH` on the gateway | persist gateway state and restart gateway once |
| Repeated WeChat replies after idle polls | local WeChat pull cursor rewound after an empty gateway response | keep the last non-empty cursor and inspect `events pull` logs for `from_cursor=0` resets |
| WeChat replies feel spammy or end with dangling `:` / `：` | outbound bubble splitting is too aggressive for short chat replies | tune `DEFAULT_BUBBLE_SOFT_LIMIT`, `MAX_SENTENCES_PER_BUBBLE`, and connector handling in `src/channels/wechat.rs` |
| WeChat image marker does not arrive as an image | outbound message lacked dispatch context, local file path was unreadable, or gateway upload failed | inspect `src/channels/wechat.rs` logs for outbound upload/send warnings, confirm absolute local path, and verify gateway `/v1/media` works |
| Session persistence failure | filesystem permission or disk issue | inspect `~/.zerda` ownership and free space |

## Troubleshooting

### Assistant says work started but nothing comes back

1. Run `/jobs`.
2. Inspect `/job <id>`.
3. Open `status.json` and `log.txt`.
4. Verify `python3` exists and `ZERDA_PRIMITIVES_ROOT` resolves correctly.

### Relative paths inside PTC read the wrong files

1. Inspect the process cwd in startup logs.
2. Check `PTC_WORKING_DIR` in the job environment.
3. Prefer absolute paths in the generated Python block if the task spans multiple roots.

### Telegram voice messages are not transcribed

1. Verify `[stt]` is configured.
2. Check `GROQ_API_KEY`.
3. Inspect Telegram and STT logs for request failures.

### WeChat voice messages are not transcribed

1. Inspect the raw event payload from `wechat-agent-gateway`.
2. Verify the gateway returned either `event.text` or `media[].transcript`.
3. If both are empty, this is a gateway-side limitation for that message; Zerda does not run external STT for WeChat.

### WeChat asks for a QR code after every restart

1. Inspect the gateway deployment rather than Zerda first.
2. Verify the gateway uses a stable `WECHAT_GATEWAY_STATE_PATH`.
3. Make sure that path is backed by a persistent host path or Docker volume.
4. Check `GET /v1/accounts` on the gateway after restart; if it is empty, the gateway state was not restored.

## Test Strategy

- Validate config parsing with `zerda config validate`.
- Exercise one CLI turn with a prompt that does not emit PTC.
- Exercise one CLI or Telegram turn that emits one PTC job.
- Exercise one WeChat text turn through `wechat-agent-gateway`.
- Verify `/jobs`, `/job <id>`, and `/cancel-job <id>` against artifact files.
- Verify `/compact` still works with the configured fast model.

## Codebase Conventions And File Responsibility Map

| Path | Responsibility |
|---|---|
| `src/main.rs` | bootstrap and mode dispatch |
| `src/runner.rs` | turn orchestration and runtime-event injection |
| `src/agent.rs` | assistant turn execution, compaction, PTC extraction |
| `src/config.rs` | config loading and validation |
| `src/providers/*` | provider adapters |
| `src/ptc/*` | PTC parsing, interception, job lifecycle |
| `src/channels/*` | CLI, Telegram, and WeChat transports |
| `src/channels/wechat.rs` | gateway-backed WeChat transport, QR login bootstrap, event polling, and short-message delivery |
| `src/prompt.rs` | system prompt assembly |
| `code_primitives/python/*` | runtime Python primitives |

## Operational Constraints And Sharp Edges

- Only the first protocol tag is visible-output terminating; anything after it must remain valid PTC/Rust protocol XML.
- PTC jobs are detached from the main assistant loop; users may continue chatting while jobs run.
- Runtime job results are injected as `user` messages with metadata, not as assistant text.
- There is no supported path for reintroducing provider-level tools or MCP without redesigning the runtime contract and this file.
- Future feature work should prefer bounded Python primitives over reviving tool-era abstractions.
