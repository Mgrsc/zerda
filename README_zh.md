<div align="center">

# 🦊 Zerda

**轻量级、高度模块化的全能型 AI Agent 框架**

[![License: MIT](https://img.shields.io/github/license/Mgrsc/zerda?style=flat-square&color=blue)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-blue.svg?style=flat-square&logo=docker)](https://www.docker.com/)

[**English**](./README.md) | [**简体中文**](./README_zh.md)

</div>

---

## 📖 简介

**Zerda** 是一个基于 Rust 开发的高性能 AI Agent 框架，专注于提供强大的系统交互能力与灵活的模块化扩展。它原生支持主流 LLM 提供商 (OpenAI, Anthropic)，并深度集成了 **MCP (Model Context Protocol)** 与动态 **技能系统 (Skill System)**。

> [!CAUTION]
> **安全警告**：Agent 运行时拥有完整的系统权限（Shell 执行、文件读写、包管理等）。为了您的宿主机安全，**强烈建议**在 Docker 容器或受限的虚拟机环境中运行 Zerda。

---

## ✨ 核心特性

- 🧠 **多模型驱动**：无缝支持 OpenAI（同时兼容经典的 Chat Completions 接口与最新的 Responses 接口）和 Anthropic，并允许在运行时动态切换模型。
- 🔧 **全能工具箱**：内置 Shell 执行、文件系统读写、长期记忆管理、TTS/STT 语音交互及子 Agent 调度等核心能力。
- 🔌 **MCP 生态接入**：完美支持 [Model Context Protocol (MCP)](https://modelcontextprotocol.io)，可动态加载外部工具、数据库和专有数据源。
- 📜 **动态技能系统**：采用 Markdown 定义技能，支持热重载。Agent 具备自主搜索、安装和自我优化技能的能力。
- 💬 **多端交互体验**：不仅支持直接通过 CLI 终端进行沉浸式对话，还支持通过 Telegram Bot 远程操控（支持语音消息）。
- 🗜️ **智能上下文管理**：面对超长会话，系统会自动进行对话历史的 LLM 压缩与本地持久化存储，兼顾性能与记忆。

---

## 🚀 快速开始

<details open>
<summary><b>🐳 方式一：Docker (推荐)</b></summary>

这是最快捷且最安全的部署方式。

1. **准备环境目录**：
   ```bash
   mkdir zerda && cd zerda
   ```

2. **下载核心配置文件**：
   ```bash
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/docker-compose.yml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/.env.example && mv .env.example .env
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/zerda.toml
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/identity.md
   ```

3. **配置与启动服务**：
   编辑 `.env` 文件，填入你的 API Key，然后启动容器：
   ```bash
   docker compose up -d
   ```

> 进阶配置请参考 [docker-compose.yml](docker-compose.yml)。

</details>

<details>
<summary><b>🔨 方式二：从源码构建</b></summary>

适合进行本地开发或定制化编译：

```bash
# 克隆项目仓库
git clone https://github.com/Mgrsc/zerda.git && cd zerda

# 编译 release 版本
cargo build --release

# 运行 Zerda
./target/release/zerda --config zerda.toml
```

> [!NOTE]
> 当 Agent 调用内置的 `reload` 工具时，进程会执行硬重启并退出。Docker 部署通过 `restart: unless-stopped` 策略自动恢复，而裸二进制启动需要借助外部进程守护工具（如 `systemd`、`supervisord`）来实现自动拉起。

</details>

---

## ⚙️ 配置说明

Zerda 采用灵活的 TOML 格式进行配置，并支持通过 `${VAR}` 语法注入环境变量。

### 📦 配置文件
- **[`zerda.toml`](zerda.toml)**：最小化配置模板，仅包含运行所需的最核心参数，适合快速上手。
- **[`zerda.toml.full`](zerda.toml.full)**：完整配置示例，包含所有可选参数、详细注释及进阶功能（如 TTS、STT、日志等级等）。
- **`mcp.toml`（可选）**：如果存在，必须与当前生效的 `zerda.toml` 位于同一目录；启动时会合并其中的 `[[mcp]]` 配置。

### 🧭 配置解析优先级
Zerda 启动时按以下顺序确定配置文件：
1. `--config` / `-c`
2. 环境变量 `ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

### 🔑 环境变量
Zerda 会从进程环境变量中展开 TOML 内的 `${VAR}` 占位符。

- Docker 模式：`docker compose` 通过 `env_file` 自动加载 `.env`。
- 手动启动：Zerda 不会自动读取 `.env`，需要先在 shell 中加载。

```bash
set -a
source ~/.zerda/.env
set +a
./target/release/zerda --config ~/.zerda/zerda.toml
```

手动启动推荐目录结构：
- `~/.zerda/zerda.toml`
- `~/.zerda/mcp.toml`（可选）
- `~/.zerda/identity.md`
- `~/.zerda/.env`

---

## 💻 CLI 使用手册

Zerda 提供了功能丰富的命令行接口：

| 命令 | 描述 |
| :--- | :--- |
| `zerda` | 直接进入交互式对话模式。 |
| `zerda run -m "<消息>"` | 执行单条指令后立即退出。 |
| `zerda run --resume` | 恢复并继续上一次未完成的会话。 |
| `zerda serve` | 启动后台服务（例如 Telegram Bot 监听）。 |

### 🛠️ 交互模式命令

在交互式对话模式中，你可以输入以下 `/` 开头的快捷命令：

- `/help`：显示所有可用的快捷命令。
- `/model [name]`：查看当前使用的模型，或即时切换到新模型。
- `/clear`：清空当前会话的历史记录。
- `/compact`：强制触发 LLM 进行上下文压缩。
- `/status`：查看当前的 Token 用量、预算限制及系统状态。

---

## 🔌 扩展能力

### 📜 技能系统 (Skills)
Skills 是存放在 `~/.zerda/skills/` 目录下的模块化指令集。它们用于定义 Agent 的专业工作流和垂直领域知识。
- **编写规范**：使用 Markdown 编写，遵循 [Claude Skills 文档](https://code.claude.com/docs/en/skills) 的风格规范。
- **零配置管理**：你可以直接通过自然语言让 Agent 根据需求自行编写、搜索、安装和配置技能，完全无需手动干预。当然，也完全支持手动创建和微调。

### 🌐 MCP 集成
通过 Model Context Protocol (MCP)，安全地将 Agent 连接到外部生态系统（如本地数据库、代码仓库或云端 API）。你可以直接吩咐 Agent 自行编写配置并接入所需的 MCP 服务器，无需手动编辑配置文件；同时，也保留了手动配置的灵活性。

```toml
[[mcp]]
name = "my-local-tools"
transport = "stdio"
command = "npx"
args = ["-y", "@scope/server"]
```

---

## 🧬 技术设计

### KV-Cache 友好架构

Zerda 的 system prompt 完全静态化——identity、rules、环境元数据在构建时固定写入，所有动态内容（时间戳、任务状态、用户上下文）仅注入到 user message 的末端，绝不侵入 system prompt。内置工具定义列表（`shell → read → write → reload → memory → skill → todo → …`）顺序锁定，运行时不增删，防止 tool definitions hash 变化导致前缀缓存失效。会话历史遵循 append-only 原则：消息不做回溯修改，仅从头部截断或尾部追加，最大化 KV-Cache 前缀命中率。


### 文件系统上下文

大文件（>10 MB）从不全量加载，工具仅返回头尾预览和文件路径指针。当任意工具输出超过 `max_tool_output_chars` 阈值时，溢出内容写入临时文件，上下文中只保留路径引用。模型按需通过 `shell` / `read` 工具重新读取完整内容（read-on-demand），避免预载造成的上下文膨胀。自动压缩时，完整对话转录持久化到 `memory/compaction/` 目录，摘要中保留恢复路径，模型可随时追溯原始内容——在零即时推理开销下实现无损可恢复性。


### ToDo Recitation（待办事项背诵）

长会话中，模型易受"Lost in the Middle"效应和注意力盆地偏差影响，对位于上下文中部的指令关注度显著下降。为此，`TodoTool` 维护一个 session 级待办列表，每轮用户消息构建时通过 `pending_reminder()` 将未完成事项自动注入 user message 靠近末端的位置。这一机制持续将全局目标推入模型的近期注意力窗口，强制周期性复习，有效对抗注意力坍缩。


### Segmented Content Isolation（分段式内容隔离）

多来源信息混入同一文本块会导致"Instruction Dilution"（提示词稀释），不同语义相互污染。Zerda 将 user message 的 `content` 字段组织为独立 text block 数组：`[skills_index, todo_reminder, user_context, conversation_summary, timestamp, user_input]`。各块语义独立，可按需增删而不影响其他块的完整性。安全准则作为独立块注入，实现反复强化。


### System/User Prompt Layering（提示词分层架构）(Experimental)

提示词架构分为两层。**System prompt** 作为静态内核：identity（角色锚定）→ rules（否定式约束前置）→ env（结构化标签）。**User prompt** 作为动态外壳：通过 `<system-reminder>` 标签实现越级提醒，content block 根据模型当前阶段（探索 / 规划 / 执行）动态组装。否定式约束（`NEVER` / `DO NOT`）前置以划定硬性禁区；结构化标签（`<env>`、`<user-context>`）便于精准提取。identity 文本位于 system prompt 的最前位，首句锚定身份，后续所有规则围绕该身份展开。


---

## 📄 开源协议

本项目基于 [MIT License](LICENSE) 开源。
