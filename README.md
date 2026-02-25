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

---

## 🧬 Technical Design

<details>
<summary><b>Expand Technical Design</b></summary>

### KV-Cache Friendly Architecture

Zerda's system prompt is fully static — identity, rules, and environment metadata are baked in at build time. All dynamic content (timestamps, task state, user context) is injected only at the tail of the user message, never into the system prompt. The built-in tool definition list (`shell → read → write → reload → memory → skill → todo → …`) is order-locked and never mutated at runtime, preventing tool-definition hash changes from invalidating the prefix cache. Conversation history follows an append-only discipline: messages are never retroactively edited — the history is only truncated from the head or appended at the tail, maximizing KV-cache prefix hits.

### File System Context

Large files (>10 MB) are never loaded in full; the tool returns a head/tail preview plus a file-path pointer. When any tool output exceeds `max_tool_output_chars`, the overflow is spilled to a temporary file and only the path reference is kept in context. The model re-accesses the full content on demand via `shell` / `read` tools (read-on-demand), avoiding upfront context bloat. During automatic compaction, the complete transcript is persisted to `memory/compaction/`; the resulting summary retains a recovery path so the model can trace back to the original content at any time — lossless recoverability with zero immediate inference overhead.

### ToDo Recitation

In long sessions, models are susceptible to the "Lost in the Middle" effect and attention-basin bias, causing attention to drop for instructions positioned in the middle of the context. To counteract this, `TodoTool` maintains a session-scoped task list. Each time a user turn is assembled, `pending_reminder()` automatically injects the outstanding items near the end of the user message. This continuously pushes global objectives into the model's recency attention window, enforcing periodic review and resisting attention collapse.

### Keep the Errors

Zerda does not scrub failed actions or tool errors. Every tool result, including `is_error` signals, is written back into conversation history and reused in subsequent reasoning as negative constraints for in-context learning. This enables implicit backtracking away from known-bad paths and reduces repeated failures. Even during auto-compaction, the full raw transcript is persisted before summarization so error context remains recoverable.

### Segmented Content Isolation

Mixing information from multiple sources into a single text block leads to "Instruction Dilution," where different semantics pollute each other. Zerda structures the `content` field of the user message as an array of independent text blocks: `[skills_index, todo_reminder, user_context, conversation_summary, timestamp, user_input]`. Each block is semantically self-contained and can be added or removed without affecting the integrity of others. Safety directives are injected as a standalone block for repeated reinforcement.

### System/User Prompt Layering (Experimental)

The prompt architecture is split into two layers. The **system prompt** serves as a static kernel: identity (role anchoring) → rules (negation-first constraints) → env (structured tags). The **user prompt** acts as a dynamic shell: `<system-reminder>` tags deliver elevated reminders, and content blocks are assembled dynamically based on the model's current phase (explore / plan / execute). Negation constraints (`NEVER` / `DO NOT`) are front-loaded to establish hard boundaries; structured tags (`<env>`, `<user-context>`) enable precise extraction. The identity text occupies the very first position in the system prompt — the opening sentence anchors the role, and all subsequent rules orbit around it.

</details>

---

## 📄 License

This project is open-sourced under the [MIT License](LICENSE).
