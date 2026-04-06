<div align="center">

# 🦊 Zerda

**Lightweight, highly modular, all-purpose AI agent framework**

[![License: MIT](https://img.shields.io/github/license/Mgrsc/zerda?style=flat-square&color=blue)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-blue.svg?style=flat-square&logo=docker)](https://www.docker.com/)

[**English**](./README.md) | [**简体中文**](./README.zh-CN.md)

</div>

---

## 📖 Introduction

**Zerda** is a high-performance AI agent framework built in Rust, focused on strong system interaction and modular extensibility. It supports mainstream LLM providers such as OpenAI and Anthropic. The runtime has now fully migrated from ~~provider-level tools~~ and ~~MCP (Model Context Protocol)~~ to **PTC (Programmatic Tool Calling)** as the async execution path, while ~~Skills~~ have been removed and will later be replaced by Playbook.

> [!IMPORTANT]
> **Project note**: this project is primarily built for the author's own use. If you deploy it yourself, no technical guidance or deployment support is provided here. The main goal of the project is to explore new technical and architectural ideas; the technical design section below is mainly for reference and learning.

> [!CAUTION]
> **Security warning**: the agent runtime has full system privileges, including shell execution, file access, and package management. For host safety, it is strongly recommended to run Zerda inside Docker or a constrained virtual machine.

---

## 🗂️ Quick Navigation

- [✨ Core Features](#-core-features)
- [🚀 Quick Start](#-quick-start)
- [⚙️ Configuration](#-configuration)
- [💻 CLI Guide](#-cli-guide)
- [🔌 Extension Capabilities](#-extension-capabilities)
- [🧬 Technical Design](#-technical-design)

---

## ✨ Core Features

- 🧠 **Multi-model runtime**: supports OpenAI, including both classic Chat Completions and newer Responses-style integration, plus Anthropic, with runtime model switching.
- 🔧 **PTC execution path**: mechanical work is pushed through PTC into async Python jobs, with filesystem, process, and web primitives executed through controlled artifacts.
- ~~🔌 **MCP integration**~~: removed. The previous tools/MCP path has been unified into PTC.
- ~~📜 **Dynamic Skills system**~~: removed. Future extensibility will move to Playbook instead.
- 💬 **Multi-channel interaction**: supports immersive CLI use as well as remote access through Telegram Bot and WeChat Gateway. Telegram supports voice transcription, and WeChat supports QR login on startup plus persistent login reuse.
- 🗜️ **Context management**: long sessions are compacted automatically with local persistence to balance performance and continuity.
- 🧠 **EMA memory**: provides single-user global long-term memory for asynchronously extracting, recalling, and consolidating preferences, constraints, events, and operational experience. All sessions currently share one global user memory space.

---

## 🚀 Quick Start

<details open>
<summary><b>🐳 Option 1: Docker (Recommended)</b></summary>

This is the fastest and safest way to deploy Zerda.

1. **Prepare a working directory**:
   ```bash
   mkdir zerda && cd zerda
   ```

2. **Download the core runtime files**:
   ```bash
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/docker-compose.yml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/.env.example && mv .env.example .env
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/zerda.toml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/identity.md
   ```

3. **Configure and start services**:
   edit `.env`, fill in your API keys, then start the stack:
   ```bash
   docker compose up -d
   ```

> For advanced setup, see [docker-compose.yml](docker-compose.yml).
> The bundled `docker-compose.yml` starts Zerda, Chroma, and `wechat-agent-gateway`; the bundled `zerda.toml` enables EMA memory and points Chroma to `http://chroma:8000`.

</details>

<details>
<summary><b>🔨 Option 2: Build from Source</b></summary>

Use this for local development or custom builds:

```bash
git clone https://github.com/Mgrsc/zerda.git && cd zerda
cargo build --release
./target/release/zerda --config zerda.toml.full
```

> [!NOTE]
> The runtime no longer provides a `reload` tool. After changing config, identity, or channel settings, restart the process directly.

</details>

---

## ⚙️ Configuration

Zerda uses TOML for runtime configuration and supports `${VAR}` expansion from environment variables.

### 📦 Config Files

- **[`zerda.toml`](zerda.toml)**: the Compose-ready config in this repository. It enables EMA memory and points Chroma to `http://chroma:8000`.
- **[`zerda.toml.full`](zerda.toml.full)**: the fuller template for bare-metal or custom deployments. It also enables EMA memory, but points Chroma to `http://127.0.0.1:8000` by default.
- ~~**`mcp.toml` (optional)**~~: removed. MCP is no longer supported.

### 🧭 Config Resolution Priority

Zerda resolves config files in this order:

1. `--config` / `-c`
2. `ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

### 🔑 Environment Variables

Zerda expands `${VAR}` placeholders inside TOML from the process environment.

- In Docker mode, `docker compose` loads `.env` automatically.
- In manual mode, load `.env` into your shell before starting Zerda.
- In most setups, one `.env` file plus TOML `${VAR}` references is enough.
- The maintained embedding defaults reuse `OPENAI_API_KEY` and `OPENAI_BASE_URL`; only change that path if embeddings must use a separate endpoint or credential.

```bash
set -a
source ~/.zerda/.env
set +a
./target/release/zerda --config ~/.zerda/zerda.toml
```

Recommended manual runtime layout:

- `~/.zerda/zerda.toml`
- `~/.zerda/identity.md`
- `~/.zerda/.env`

Optional WeChat channel config:

```toml
[[channels]]
name = "wechat"
gateway_url = "http://127.0.0.1:8080"
```

If Zerda and `wechat-agent-gateway` run in the same Docker Compose stack, use the service name:

```toml
[[channels]]
name = "wechat"
gateway_url = "http://wechat-agent-gateway:8080"
```

Additional notes:

- `zerda.toml` is the repository's Compose-ready config, while `zerda.toml.full` is for bare-metal or custom deployments.
- EMA memory is enabled in the maintained configs and requires a reachable Chroma instance.
- Bare-metal setups usually use `http://127.0.0.1:8000`; the bundled Compose stack uses `http://chroma:8000`.
- `ZERDA_PRIMITIVES_ROOT` is only needed if you want to override the default primitive discovery path.
- Built-in primitives live under `code_primitives/python/primitives/`; custom primitives live under `custom_primitives/`.
- WeChat integration goes through [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway), not the WeChat protocol directly.
- To avoid rescanning a QR code after restart, persist `WECHAT_GATEWAY_STATE_PATH` on the gateway side.
- WeChat voice input uses the transcript returned by the gateway and does not require Zerda's `[stt]` config.

---

## 💻 CLI Guide

Zerda provides a command-line interface with both interactive and service modes:

| Command | Description |
| :--- | :--- |
| `zerda` | Start an interactive chat session. |
| `zerda run -m "<message>"` | Execute a single prompt and exit. |
| `zerda run --resume [session_id]` | Resume the latest session or a specific saved session. |
| `zerda serve` | Start background services such as Telegram Bot or WeChat channel listeners. |
| `zerda config generate` | Print the full config template (`zerda.toml.full`). |
| `zerda config validate` | Validate the effective config and exit. |

### 🛠️ Interactive Commands

Inside interactive mode, you can use these slash commands:

- `/help`: show all available commands.
- `/model`: show the current model and available providers.
- `/model <provider_id>@<model_name>`: switch models immediately, for example `/model openai@gpt-4o`.
- `/model <provider_id> list`: list models supported by a provider, for example `/model openai list`.
- `/clear`: clear the current session history.
- `/compact`: trigger context compaction explicitly.
- `/status`: inspect token usage, budgets, and runtime state.
- `/jobs`: list PTC jobs in the current session.
- `/job <id>`: inspect a specific PTC job.
- `/cancel-job <id>`: cancel a running PTC job.
- `/cancel`: cancel the current running turn.
- `/exit` / `/quit`: leave interactive mode.

Busy-session behavior:

- While a reply is streaming, `/status`, `/jobs`, `/job <id>`, and `/cancel-job <id>` return immediately.
- `/compact` is queued and runs after the current turn finishes.
- `/clear` and `/model <provider>@<model>` cancel the current turn first, then run the requested command.

---

## 🔌 Extension Capabilities

### ~~📜 Skills~~

~~Skills used to be modular instruction bundles under `~/.zerda/skills/` for domain workflows and specialized guidance.~~

Removed. Future extensibility will move to Playbook instead of restoring the old Skills system.

### ~~🌐 MCP Integration~~

~~MCP used to connect the agent to external systems such as databases, codebases, or cloud APIs.~~

Removed. The old provider-level tools / MCP execution path has been fully consolidated into PTC.

### ~~🔍 Documentation Search~~

~~Zerda used to support semantic search over its own documentation through `search_zerda_documents`.~~

The old docs-search tool has been removed. If this capability returns, it will come back as a PTC primitive or workflow instead of the old tool interface.

---

## 🧬 Technical Design

<details>
<summary><b>Expand Technical Design</b></summary>

The current runtime keeps the same core ideas described in the Chinese README: KV-cache-friendly prompts, a single assistant dialogue loop with asynchronous PTC jobs, compiler-pattern delegation, prewritten Python primitives, local recoverable state, and EMA memory with structured extraction plus active recall.

For implementation-level operational details, see [AGENT_README.md](./AGENT_README.md).

</details>

---

## 🤖 For AI Agents

See [AGENT_README.md](./AGENT_README.md) for operational context.

Repository language convention:

- Code-facing assets stay in English.
- Localized end-user documentation lives in `README.zh-CN.md`.

---

## 📄 License

This project uses dual licensing:

- Open source use: [AGPL-3.0-only](LICENSE)
- Proprietary or closed-source use: contact the maintainer for commercial licensing
