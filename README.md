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

</details>

---

## ⚙️ Configuration

Zerda utilizes a flexible TOML configuration format and supports `${VAR}` environment variable expansion.

### 📦 Configuration Files
- **[`zerda.toml`](zerda.toml)**: A minimal configuration containing only the essential parameters for a quick start.
- **[`zerda.toml.full`](zerda.toml.full)**: A comprehensive configuration example including all optional parameters, detailed comments, and advanced settings (TTS, STT, log levels, etc.).

### 🔑 Environment Variables
We highly recommend using a `.env` file to manage sensitive information securely. See [`.env.example`](.env.example) for a template.

---

## 💻 CLI Usage

Zerda provides a powerful command-line interface:

| Command | Description |
| :--- | :--- |
| `zerda` | Enter the interactive chat mode. |
| `zerda run -m "<message>"` | Execute a single instruction and exit. |
| `zerda run --resume` | Resume the most recent session. |
| `zerda serve` | Start background services (e.g., Telegram Bot). |

### 🛠️ Interactive Mode Commands

While in the interactive chat mode, you can use the following commands:

- `/help`: Show available commands.
- `/model [name]`: View the current model or switch to a new one instantly.
- `/clear`: Clear the current session history.
- `/compact`: Force context compression using the LLM.
- `/status`: Display token usage, budget, and system status.

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

## 📄 License

This project is open-sourced under the [MIT License](LICENSE).
