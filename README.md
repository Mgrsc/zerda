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
- [🧬 Technical Design](#-technical-design)

---

## ✨ Core Features

- 🧠 **Multi-model runtime**: supports OpenAI, including both classic Chat Completions and newer Responses-style integration, plus Anthropic, with runtime model switching.
- 🔧 **PTC execution path**: mechanical work is pushed through PTC into async Python jobs, with filesystem, process, and web primitives executed through controlled artifacts.
- ~~🔌 **MCP integration**~~: removed. The previous tools/MCP path has been unified into PTC.
- ~~📜 **Dynamic Skills system**~~: removed. Future extensibility will move to Playbook instead.
- 💬 **Multi-channel interaction**: supports immersive CLI use as well as remote access through Telegram Bot and WeChat Gateway. The runtime currently supports only `telegram` and `wechat` channels.
- 🗜️ **Context management**: long sessions are compacted automatically with local persistence to balance performance and continuity.
- 🧠 **EMA memory**: provides single-user global long-term memory for asynchronously extracting, recalling, and consolidating preferences, constraints, events, and operational experience. All sessions currently share one global user memory space.

---

## 🚀 Quick Start

<details open>
<summary><b>🐳 Docker (Recommended)</b></summary>

Recommended deployment path.

1. **Prepare a working directory**:
   ```bash
   mkdir zerda && cd zerda
   ```

2. **Download the required files**:
   ```bash
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/{docker-compose.yml,.env.example,identity.md,zerda.toml.full} \
     && mv .env.example .env \
     && mv zerda.toml.full zerda.toml
   ```

3. **Configure and start services**:
   rename `zerda.toml.full` to `zerda.toml`, fill in `.env`, then start:
   ```bash
   docker compose up -d
   ```

> For advanced setup, see [docker-compose.yml](docker-compose.yml).
> The bundled stack starts Zerda, Chroma, and `wechat-agent-gateway`. Rename `zerda.toml.full` to `zerda.toml` before startup.

</details>

---

## ⚙️ Configuration

Zerda uses TOML. Use [zerda.toml.full](zerda.toml.full) as the full template, rename it to `zerda.toml`, then start. Field-level notes are already in the file.

### 🔑 Environment Variables

Zerda expands `${VAR}` in TOML from the process environment.

- In Docker mode, `docker compose` loads `.env` automatically.
- Put `${VAR}` in `zerda.toml` and keep the real values in `.env`.
- For manual startup, export `.env` before launching Zerda.
- The maintained embedding defaults reuse `OPENAI_API_KEY` and `OPENAI_BASE_URL`; only change that path if embeddings must use a separate endpoint or credential.
- Current channel support is limited to `telegram` and `wechat`.
- WeChat integration goes through [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway), not the WeChat protocol directly.
- EMA memory requires a reachable Chroma instance. The bundled Compose stack uses `http://chroma:8000`.
- `ZERDA_PRIMITIVES_ROOT` is only needed if you want to override the default primitive discovery path.
- Custom primitive package environments are synced automatically during startup. Use `zerda primitives sync` for explicit maintenance.

---

## 💻 CLI Guide

Zerda provides a command-line interface with both interactive and service modes:

| Command | Description |
| :--- | :--- |
| `zerda` | Start an interactive chat session. |
| `zerda run -m "<message>"` | Execute a single prompt and exit. |
| `zerda run --resume [session_id]` | Resume the latest session or a specific saved session. |
| `zerda serve` | Start background services such as Telegram Bot or WeChat channel listeners. |
| `zerda primitives sync` | Sync isolated environments for custom primitive packages. |
| `zerda primitives doctor` | Show readiness for custom primitive packages. |
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

## 🧬 Technical Design

<details>
<summary><b>Expand Technical Design</b></summary>

### Current Runtime

The runtime is a single-assistant dialogue loop plus asynchronous PTC jobs. The model handles dialogue directly. When mechanical work is needed, it emits `<PTC_TOOL_CALLING>` blocks, which the host executes as detached Python jobs and later reinjects into the same session as runtime results.

### Programmatic Tool Calling

PTC replaces both provider-level tool calling and MCP. The execution model is protocol-driven rather than provider-driven: the model writes `<PTC_TOOL_CALLING>`, and the host executes it as a bounded Python job. Execution artifacts are kept locally for inspection and recovery.

Runtime discovery is unified too: the model first inspects `<PTC_AVALIABLE_PRIMITIVES>`, then uses `help("name")` to learn parameter shapes and return contracts. Some complex primitives may additionally expose `get_workflow` for setup-sensitive or multi-step guidance.

### Code Primitives

On top of PTC, Zerda provides prewritten async Python primitives for common environment interactions. These primitives handle validation, error classification, hard timeouts, and telemetry, while PTC jobs compose them into task-level execution.

The primitive layer has two sources:

1. Core primitives in `code_primitives/python/primitives/`, executed directly inside the current PTC Python process.
2. Custom primitives in `custom_primitives/`, executed through package-isolated environments behind the same PTC surface.

This keeps one visible protocol surface for the model while separating custom-package dependencies from the main runtime.

### Local Recoverable State

The current runtime keeps local recoverable state for sessions, compaction artifacts, PTC artifacts, and EMA memory. This preserves replayability and troubleshooting without depending on provider-native tool history.

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
