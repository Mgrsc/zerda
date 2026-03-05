<div align="center">

# 🦊 Zerda

**A lightweight, highly modular, and powerful AI Agent framework.**

[![License: MIT](https://img.shields.io/github/license/Mgrsc/zerda?style=flat-square&color=blue)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-blue.svg?style=flat-square&logo=docker)](https://www.docker.com/)

[**English**](./README.md) | [**简体中文**](./README_zh.md)

</div>

---

## 📖 Introduction

**Zerda** is an AI Agent framework developed in Rust, focusing on delivering robust system interaction capabilities alongside flexible modular extensions. It supports major LLM providers (OpenAI, Anthropic) and deeply integrates with the **MCP (Model Context Protocol)** and a dynamic **Skill System**.

> [!CAUTION]
> **Security Warning**: The Agent operates with full system permissions (Shell execution, file R/W, package management, etc.). For your security, it is **strongly recommended** to run Zerda within a Docker container or a restricted virtual machine.

---

## 🗂️ Quick Navigation

- [✨ Key Features](#-key-features)
- [🚀 Getting Started](#-getting-started)
- [⚙️ Configuration](#-configuration)
- [💻 CLI Usage](#-cli-usage)
- [🔌 Extension Capabilities](#-extension-capabilities)
- [🧬 Technical Design](#-technical-design)

---

## ✨ Key Features

- 🧠 **Multi-Model Support**: Seamlessly switch between OpenAI (Chat Completions API and the new Responses API) and Anthropic models at runtime.
- 🔧 **Versatile Toolset**: Built-in tools for Shell execution, file management, memory control, TTS/STT, and sub-agent scheduling.
- 🔌 **MCP Integration**: Native support for the [Model Context Protocol (MCP)](https://modelcontextprotocol.io), allowing dynamic integration of external tools and data sources.
- 📜 **Dynamic Skill System**: Markdown-based skill definitions with hot-reloading. The Agent can autonomously search, install, and optimize its own skills.
- 💬 **Multi-Channel Interaction**: Engage via the direct CLI interface or remotely through a Telegram Bot (with voice message support).
- 🗜️ **Smart Context Management**: Automated compression and persistent storage of conversation history to effectively handle long-running sessions.

---

## 🚀 Getting Started

<details open>
<summary><b>🐳 Option 1: Docker (Recommended)</b></summary>

Deploying with Docker is the fastest and most secure method.

1. **Prepare the environment**:
   ```bash
   mkdir zerda && cd zerda
   ```

2. **Download core configuration files**:
   ```bash
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/docker-compose.yml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/.env.example && mv .env.example .env
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/zerda.toml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/identity.md
   ```

3. **Configure and Start**:
   Edit the `.env` file to include your API keys, then start the services:
   ```bash
   docker compose up -d
   ```

> For advanced setups, refer to [docker-compose.yml](docker-compose.yml).

</details>

<details>
<summary><b>🔨 Option 2: Build from Source</b></summary>

Ideal for local development or custom builds:

```bash
# Clone the repository
git clone https://github.com/Mgrsc/zerda.git && cd zerda

# Build the release binary
cargo build --release

# Run Zerda
./target/release/zerda --config zerda.toml
```

> [!NOTE]
> When the Agent invokes the built-in `reload` tool, the process performs a hard restart and exits. Unlike Docker (which auto-restarts via `restart: unless-stopped`), a bare binary requires an external process supervisor (e.g., `systemd`, `supervisord`) to bring it back up automatically.

</details>

---

## ⚙️ Configuration

Zerda utilizes a flexible TOML configuration format and supports `${VAR}` environment variable expansion.

### 📦 Configuration Files
- **[`zerda.toml`](zerda.toml)**: A minimal configuration containing only the essential parameters for a quick start.
- **[`zerda.toml.full`](zerda.toml.full)**: A comprehensive configuration example including all optional parameters, detailed comments, and advanced settings (TTS, STT, log levels, etc.).
- **`mcp.toml` (optional)**: If present, it must be in the same directory as the active `zerda.toml`, and its `[[mcp]]` entries are merged at startup.

### 🧭 Config Resolution Order
When Zerda starts, config is resolved in this order:
1. `--config` / `-c`
2. `ZERDA_CONFIG` environment variable
3. `~/.zerda/zerda.toml`

### 🔑 Environment Variables
Zerda expands `${VAR}` values in TOML from process environment variables.

- Docker mode: `docker compose` loads `.env` automatically via `env_file`.
- Manual mode: Zerda does not auto-load `.env`. You need to load it in your shell first.
- In the integrated compose setup, `memory-service` uses a separate env file (`.env.distributed.example`) and does not merge its variables into Zerda's `.env`.

```bash
set -a
source ~/.zerda/.env
set +a
./target/release/zerda --config ~/.zerda/zerda.toml
```

Recommended manual layout:
- `~/.zerda/zerda.toml`
- `~/.zerda/mcp.toml` (optional)
- `~/.zerda/identity.md`
- `~/.zerda/.env`

---

## 💻 CLI Usage

Zerda provides a powerful command-line interface:

| Command | Description |
| :--- | :--- |
| `zerda` | Enter the interactive chat mode. |
| `zerda run -m "<message>"` | Execute a single instruction and exit. |
| `zerda run --resume [session_id]` | Resume the latest session or a specific session by ID. |
| `zerda serve` | Start background services (e.g., Telegram Bot). |
| `zerda config generate` | Print the full example config template (`zerda.toml.full`). |
| `zerda config validate` | Validate the active config file and exit. |

### 🛠️ Interactive Mode Commands

While in the interactive chat mode, you can use the following commands:

- `/help`: Show available commands.
- `/model`: View the current active model and available providers.
- `/model <provider_id>@<model_name>`: Switch to a new model instantly (e.g., `/model openai@gpt-4o`).
- `/model <provider_id> list`: List models supported by the target provider (e.g., `/model openai list`).
- `/clear`: Clear the current session history.
- `/compact`: Force context compression using the LLM.
- `/status`: Display token usage, budget, and system status.
- `/cancel`: Cancel the current running turn.
- `/exit` / `/quit`: Exit interactive mode (CLI session).

---

## 🔌 Extension Capabilities

### 📜 Skill System
Skills are modular instruction sets located in `~/.zerda/skills/`. They define the Agent's specialized workflows and knowledge bases.
- **Specification**: Written in Markdown, following the [Claude Skills documentation](https://code.claude.com/docs/en/skills) style.
- **Zero-Touch Management**: Simply ask the Agent to write, search, install, and configure its own skills based on your needs. Manual authoring and tweaking are also fully supported.

### 🌐 MCP Integration
Connect your Agent to external ecosystems like local databases, code repositories, or cloud APIs securely through the Model Context Protocol (MCP). The Agent is fully capable of writing configurations and integrating MCP servers autonomously upon request, while manual configuration via `mcp.toml` or `zerda.toml` remains available.

```toml
[[mcp]]
name = "my-local-tools"
transport = "stdio"
command = "npx"
args = ["-y", "@scope/server"]
```

### 🔍 Document Search

Zerda supports semantic search over its own project documentation via the `search_zerda_documents` tool, enabling the Agent to look up configuration guides, command references, and architectural details on demand.

**Backend: Qdrant (local/shared vector store)**

Setup:

1. Configure `docs_search` in `zerda.toml`:
   ```toml
   [docs_search]
   enabled = true
   embedding_model = "openai@${OPENAI_EMBEDDING_MODEL}"
   embedding_dim = 1536
   qdrant_url = "http://qdrant:6333"
   qdrant_api_key = ""
   collection = "zerda_docs_index"
   docs_dir = "docs/zerda"
   ```
2. Ensure the embedding provider in `[providers.<id>]` has valid `api_key` and optional `base_url` (used as embedding API base URL).
3. Start Zerda. On first startup, Zerda automatically vectorizes all Markdown files under `docs/zerda/` into `docs_search.collection`.
4. Later startups perform incremental sync and only re-embed changed files.

When `docs_search.enabled=true` and configuration is valid, the `search_zerda_documents` tool is registered automatically.
`docs_search.qdrant_api_key = ""` means no API key is attached to Qdrant requests, which is correct for default local Compose deployments without Qdrant auth.

---

## 🧬 Technical Design

<details>
<summary><b>Expand Technical Design</b></summary>

### KV-Cache Friendly Architecture

Zerda's system prompt is fully static — identity, rules, and environment metadata are baked in at build time. All dynamic content (timestamps, task state, memory recall context) is injected only at the tail of the user message, never into the system prompt. In the Planner loop, the built-in tool definition list (`reload → skill → todo → tts → delegate_to_executor`) is order-locked and never mutated at runtime, preventing tool-definition hash changes from invalidating the prefix cache. Conversation history follows an append-only discipline: messages are never retroactively edited — the history is only truncated from the head or appended at the tail, maximizing KV-cache prefix hits.

### Planner-Executor Decoupling

Zerda has migrated from a monolithic ReAct loop to a dual-agent Planner-Executor architecture. The Planner focuses on intent understanding, task decomposition, and final synthesis, while the Executor focuses on environment interaction and mechanical execution. This hard separation significantly reduces direct low-level tool traces in the Planner's context and keeps high-level reasoning cleaner over long multi-turn sessions.

The split also improves concurrency scaling characteristics. In practice, a Planner can fan out multiple independent execution nodes while keeping one clean reasoning thread. With horizontally scalable Executor workers, task fan-out can grow much faster than in a single mixed ReAct loop, without proportionally polluting the Planner context.

### Compiler Pattern (Intent Compilation)

Between Planner and Executor, Zerda applies a Compiler Pattern: the Planner acts as a front-end compiler that translates verbose, ambiguous user requests and environmental context into high-density structured instructions before passing them to the Executor. Instructions follow the form `ACTION(params) -> {return_fields}` — a compact, machine-readable format that eliminates narrative overhead and makes the Executor's job unambiguous.

This mirrors how a compiler transforms human-readable source code into optimized intermediate representation: the Planner absorbs context, resolves ambiguity, and emits a minimal instruction that the weaker Executor model can follow reliably. The result is fewer wasted tokens on verbose delegation briefs, lower error rates from the Executor misinterpreting intent, and a clean separation between "understanding what to do" (Planner) and "doing it" (Executor).

### Programmatic Tool Calling (PTC)

Instead of repeatedly composing shell heredoc payloads in-context, the Executor uses programmatic tool calling for compute pushdown. The `execute_python_script` tool accepts pure Python code in a structured field, writes/runs scripts in a managed artifact directory, and returns standardized execution status plus compact findings. This converts many multi-step tool chains into one bounded execution block and reduces tool-call chatter in the main loop.

### Prewritten Primitive Layer (Code Primitives)

On top of PTC, Zerda adds a prewritten primitive layer: frequently used, failure-prone, and reusable environment interactions are implemented as async Python functions and injected into the Executor runtime for direct `await` calls. This reduces ad-hoc script assembly and field-path guessing failures.

Primitives follow a strict shared contract: `status/data/error_code/error_message/retryable`, and each primitive docstring defines an explicit `[Output Contract]` with success criteria and key field paths. For Firecrawl-oriented primitives, responses are normalized for flat access first (for example `data.markdown`, `data.html`, `data.metadata`, `data.results`) while preserving a compatible raw upstream payload field for backward compatibility.

This layer decouples tool capability from task-level scripting: primitives own argument validation, timeout/retry policy, error typing, and telemetry persistence; the model focuses on orchestration and business logic. For compatibility, complex conditional constraints are enforced in runtime checks rather than pushed into top-level tool schemas.

### Lightweight Relational-Hybrid Memory ([MemBurrow](https://github.com/Mgrsc/MemBurrow))

Zerda integrates a lightweight external memory service ([MemBurrow](https://github.com/Mgrsc/MemBurrow)) to address practical failure modes in long-running agent sessions:

- Repeated context replay: preserving preferences/rules by replaying long histories inflates token cost.
- Retrieval correctness drift: vector-only recall may return semantically similar but operationally wrong memories.
- Constraint loss: hard rules and user preferences are easy to miss in pure similarity search.
- Recall fragility: when vector infrastructure degrades, memory injection becomes unstable.

The memory pipeline mitigates these issues with several design choices:

1. SQL as source of truth, vector as acceleration index.
2. Intent-aware routing: rule/preference/constraint-style intents go SQL-first; other intents use hybrid recall.
3. Outbox-based async ingest: API returns quickly while extraction/embedding/indexing run in background workers.
4. Multi-factor rerank: semantic relevance, importance, confidence, freshness, and scope.
5. Graceful degradation and repair: SQL fallback when vector search fails, plus periodic reconciliation to reduce SQL-vector drift.

For Zerda, this reduces prompt bloat from history replay, improves retention of actionable constraints, and keeps recall behavior stable under partial dependency failures.

In the default integrated deployment, MemBurrow and Zerda reflection share the same Qdrant instance. Collection names are isolated by design, so data does not collide:
- MemBurrow collection: `memburrow_agent_memory`
- Zerda reflection collection: `zerda_executor_guidelines`

### Executor Reflection Memory Loop (Heuristic)

Zerda implements a heuristic executor reflection memory loop that is conceptually inspired by ACON (Agent Context Optimization). The goal is to shift memory usage from "feeding more task facts" to "feeding reusable methodology and lessons" (`How to act / What to avoid`). Before an execution run, the system embeds the delegated instruction, retrieves top-matched historical guidelines from Qdrant, and injects them into the Executor prompt as concise system reminders.

Configuration note: all reflection settings live under `[reflection]` (for example `llm_model`, `max_tokens`, `embedding_model`, `embedding_dim`, `qdrant_url`, `qdrant_api_key`). Both `llm_model` and `embedding_model` use `provider_id@model_name` and resolve `base_url` / `api_key` from `[providers.<id>]`. `embedding_model` is optional and defaults to the same provider as `llm_model` with `text-embedding-3-small`. Reflection sampling is fixed at `temperature=0.7` and `top_p=0.95`.

During execution, Zerda records iteration outcomes (tool errors and traceback signals). After the run, a reflection worker asynchronously performs failure-driven contrast: it compares failed and successful iterations from the same trajectory, then compresses one reusable guideline in imperative form. The compression prompt explicitly enforces method-level lessons (not domain facts), short output, and generalizability to similar tasks.

Extracted guidelines are written back into a vector store and become reusable priors for future similar instructions. Zerda also includes a negative-feedback guardrail: if a run still ends in failure after guideline injection, those injected guideline entries are removed to avoid reinforcing unhelpful heuristics.

Scope note: this is not the full ACON research pipeline from the paper. Zerda currently focuses on online executor guidance memory and does not implement the paper's full offline UT/CO optimization workflow, dedicated history/observation compressor training loop, or compressor/agent distillation pipeline.

### Context Rot Resistance

The architecture explicitly mitigates Context Rot. Mechanical failures, stack traces, and retry noise are retained in Executor artifacts/logs, while the Planner receives reduced, decision-grade outputs. When evidence is sufficient (including negative evidence such as empty link sets), the Planner can converge immediately; when evidence is insufficient, the Planner can re-decompose the task with a fresh local strategy without inheriting excessive execution residue.

### Empirical Token Efficiency

In the website-investigation samples under `example-docs/some-file/`, the Planner-Executor + PTC workflow showed substantial token reduction versus the prior direct-tooling path.

| Metric | Traditional ReAct (single loop) | Planner-Executor + PTC |
| :--- | :--- | :--- |
| Tool trace exposure in main context | High | Low |
| Mechanical error noise in main context | High | Mostly isolated to Executor artifacts |
| Typical tool-call chain length | Longer, chatty | Compressed into bounded execution blocks |
| Token usage (round-1 sample) | Baseline | ~80% lower in observed sample |
| Multi-turn stability | Degrades faster as traces accumulate | More stable due to strategy/execution separation |

This table reflects an initial first-round test and sample observation, not a universal benchmark. Actual gains vary by task shape, tool fan-out, and output verbosity.
For sustained multi-round tool usage, traditional ReAct often experiences faster context expansion because reasoning, execution traces, retries, and diagnostics co-reside in one thread; Planner-Executor keeps most execution residue in Executor artifacts/logs, so Planner context tends to grow slower and remain more stable.

### File System Context

Large files (>10 MB) are never loaded in full; the tool returns a head/tail preview plus a file-path pointer. When any tool output exceeds `max_tool_output_chars`, the overflow is spilled to a temporary file and only the path reference is kept in context. Executor artifacts are persisted under `~/.zerda/executor_jobs/<YYYYMMDD>/<HHMMSS>_<task_slug>/` with separated script/log/result/meta files, which makes replay and postmortem analysis deterministic while minimizing Planner context pollution. During automatic compaction, the complete transcript is persisted to `memory/compaction/`; the resulting summary retains a recovery path so the model can trace back to the original content at any time — lossless recoverability with zero immediate inference overhead.

### ToDo Recitation

In long sessions, models are susceptible to the "Lost in the Middle" effect and attention-basin bias, causing attention to drop for instructions positioned in the middle of the context. To counteract this, `TodoTool` maintains a session-scoped task list. Each time a user turn is assembled, `pending_reminder()` automatically injects the outstanding items near the end of the user message. This continuously pushes global objectives into the model's recency attention window, enforcing periodic review and resisting attention collapse.

Beyond attention anchoring, `TodoTool` doubles as the task orchestration backbone for complex requests. The Planner decomposes multi-step work via `todo(add)`, delegates each sub-task with a compiled instruction, and marks `todo(done)` upon completion — forming an auditable execution trace. `TodoTool` is concurrent-safe (internally `Mutex`-protected), allowing batch creation in a single iteration; a typical 4-subtask workflow completes in ~6 iterations.

### Keep the Errors

Zerda does not scrub failed actions or tool errors. Every tool result, including `is_error` signals, is written back into conversation history and reused in subsequent reasoning as negative constraints for in-context learning. This enables implicit backtracking away from known-bad paths and reduces repeated failures. Even during auto-compaction, the full raw transcript is persisted before summarization so error context remains recoverable.

### Segmented Content Isolation

Mixing information from multiple sources into a single text block leads to "Instruction Dilution," where different semantics pollute each other. Zerda structures the `content` field of the user message as an array of independent text blocks: `[skills_index, todo_reminder, memory_recall, conversation_summary, timestamp, user_input]`. Each block is semantically self-contained and can be added or removed without affecting the integrity of others. Safety directives are injected as a standalone block for repeated reinforcement.

### System/User Prompt Layering (Experimental)

The prompt architecture is split into two layers. The **system prompt** serves as a static kernel: identity (role anchoring) → rules (negation-first constraints) → env (structured tags). The **user prompt** acts as a dynamic shell: `<system-reminder>` tags deliver elevated reminders, and content blocks are assembled dynamically based on the model's current phase (explore / plan / execute). Negation constraints (`NEVER` / `DO NOT`) are front-loaded to establish hard boundaries; structured tags (`<env>`, `<memory-recall>`) enable precise extraction. The identity text occupies the very first position in the system prompt — the opening sentence anchors the role, and all subsequent rules orbit around it.

</details>

---

## 📄 License

This project is open-sourced under the [MIT License](LICENSE).
