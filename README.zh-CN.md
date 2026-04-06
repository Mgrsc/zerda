<div align="center">

# 🦊 Zerda

**轻量级、高度模块化的全能型 AI Agent 框架**

[![License: MIT](https://img.shields.io/github/license/Mgrsc/zerda?style=flat-square&color=blue)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/Docker-Ready-blue.svg?style=flat-square&logo=docker)](https://www.docker.com/)

[**English**](./README.md) | [**简体中文**](./README.zh-CN.md)

</div>

---

## 📖 简介

**Zerda** 是一个基于 Rust 开发的高性能 AI Agent 框架，专注于提供强大的系统交互能力与灵活的模块化扩展。它支持主流 LLM 提供商 (OpenAI, Anthropic)，当前运行时已经把 ~~provider-level tools~~ 与 ~~MCP (Model Context Protocol)~~ 全面迁移为 **PTC (Programmatic Tool Calling)** 异步执行路径；同时，~~Skills~~ 已移除，后续会由 Playbook 替代。

> [!IMPORTANT]
> **项目声明**：本项目主要是作者写给自己使用的。如果你要自行部署，这里不提供任何技术指导或部署支持。这个项目的主要目的，是实验新的技术和架构思路；下方的技术设计内容仅作为参考学习使用。

> [!CAUTION]
> **安全警告**：Agent 运行时拥有完整的系统权限（Shell 执行、文件读写、包管理等）。为了您的宿主机安全，**强烈建议**在 Docker 容器或受限的虚拟机环境中运行 Zerda。

---

## 🗂️ 快速导航

- [✨ 核心特性](#-核心特性)
- [🚀 快速开始](#-快速开始)
- [⚙️ 配置说明](#-配置说明)
- [💻 CLI 使用手册](#-cli-使用手册)
- [🔌 扩展能力](#-扩展能力)
- [🧬 技术设计](#-技术设计)

---

## ✨ 核心特性

- 🧠 **多模型驱动**：无缝支持 OpenAI（同时兼容经典的 Chat Completions 接口与最新的 Responses 接口）和 Anthropic，并允许在运行时动态切换模型。
- 🔧 **PTC 执行路径**：机械执行统一通过 PTC 下推到异步 Python 作业，文件系统、进程、网页原语能力都通过受控工件链路运行。
- ~~🔌 **MCP 生态接入**~~：已移除，原有工具/MCP 路径已统一迁移到 PTC。
- ~~📜 **动态技能系统**~~：已移除，后续将迁移为 Playbook。
- 💬 **多端交互体验**：不仅支持直接通过 CLI 终端进行沉浸式对话，还支持通过 Telegram Bot 与 WeChat Gateway 远程接入；其中 Telegram 支持语音消息转写，WeChat 支持启动时扫码登录与登录态复用。
- 🗜️ **智能上下文管理**：面对超长会话，系统会自动进行对话历史的 LLM 压缩与本地持久化存储，兼顾性能与记忆。
- 🧠 **EMA 记忆系统**：提供单用户全局长期记忆，支持偏好、约束、事件与操作经验的异步沉淀、召回与整理。当前 EMA 采用单一全局用户实体，所有会话天然共享同一套长期记忆。

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
> 仓库自带的 `docker-compose.yml` 会同时启动 Zerda、Chroma 和 `wechat-agent-gateway`；配套的 `zerda.toml` 已默认启用 EMA memory，并把 Chroma 地址指向 `http://chroma:8000`。

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
./target/release/zerda --config zerda.toml.full
```

> [!NOTE]
> 当前运行时不再提供 `reload` 工具。修改配置、身份文件或频道设置后，请直接重启进程。

</details>

---

## ⚙️ 配置说明

Zerda 采用灵活的 TOML 格式进行配置，并支持通过 `${VAR}` 语法注入环境变量。

### 📦 配置文件
- **[`zerda.toml`](zerda.toml)**：仓库内的 Compose 就绪最小配置，默认启用 EMA memory，并把 Chroma 地址指向 Compose 服务 `http://chroma:8000`。
- **[`zerda.toml.full`](zerda.toml.full)**：完整配置模板，适合裸机或自定义部署。它同样默认启用 EMA memory，但 Chroma 地址默认写成宿主机本地的 `http://127.0.0.1:8000`。
- ~~**`mcp.toml`（可选）**~~：已移除，MCP 路径不再受支持。

### 🧭 配置解析优先级
Zerda 启动时按以下顺序确定配置文件：
1. `--config` / `-c`
2. 环境变量 `ZERDA_CONFIG`
3. `~/.zerda/zerda.toml`

### 🔑 环境变量
Zerda 会从进程环境变量中展开 TOML 内的 `${VAR}` 占位符。

- Docker 模式：`docker compose` 通过 `env_file` 自动加载 `.env`。
- 手动启动：Zerda 不会自动读取 `.env`，需要先在 shell 中加载。
- 仓库内的 `docker-compose.yml` 只使用一份 `.env`，供 Zerda 自身与同栈部署的 gateway 使用。
- 仓库维护的默认 embedding 配置会直接复用 `OPENAI_API_KEY` 与 `OPENAI_BASE_URL`；只有当 embedding 要走单独的兼容端点或不同凭据时，才需要手工改 `memory.embedding.api_key`。
- 核心原语位于 `code_primitives/python/primitives/`。
- `code_primitives/python/primitives/catalog.py` 是内置原语的注册入口。
- 所有非核心原语统一位于 `custom_primitives/`。
- `custom_primitives/catalog.py` 是自定义原语的注册入口；具体实现可以按能力分组放在 `custom_primitives/agent_browser/`、`custom_primitives/firecrawl/` 这类子目录里。
- 仓库自带的 Compose 部署里，这个目录要挂到容器内的 `/root/.zerda/`，因为运行时会按 Zerda 当前工作目录去发现它。
- 最前面的 system 提示词块会动态注入 `<PTC_AVALIABLE_PRIMITIVES>`，里面只列当前启用的顶层公开原语名。
- 这个列表同时包含内置原语和自定义原语，比如 `fs_read`、`process_run`、`firecrawl_search_web`、`scrapling_fetch_page`、`agent_browser`。
- 当前网页能力分工是：Firecrawl 负责搜索和发现 URL，Scrapling 负责页面抓取，`agent_browser` 负责交互式验证与测试。
- `scrapling_fetch_page` 遇到 `mp.weixin.qq.com` 文章链接时会自动切到微信公众号专用提取；遇到 `x.com` / `twitter.com` 时会自动切到 stealth 抓取路径，并优先提取 tweet 正文容器，再做一层轻量 UI 噪音过滤。
- 对 Reddit、知乎、掘金这类已知动态站点，`scrapling_fetch_page` 现在会先走静态抓取；如果静态结果明显像壳页或正文不足，会自动再尝试一次 stealth 抓取。
- `scrapling_fetch_page` 依赖 Python 运行时已安装 `scrapling[fetchers]`；如果没装，这个原语会返回 `dependency_missing`。
- `scrapling_fetch_page` 在部分动态站点上会内部切到 stealth 浏览器抓取，因此当这条内部路径被需要时，也依赖 Playwright 浏览器二进制。

仓库内 Docker 镜像说明：

- 当前仓库里的 `Dockerfile` 会在镜像内创建专用 Python 虚拟环境，并安装 `scrapling[fetchers]`、`playwright` 和 Chromium 浏览器，因此本地 build 出来的镜像可以直接使用 Scrapling 抓取原语，不需要再进容器手工安装。
- prompt 里可见的原语名会和 PTC 作业 bootstrap 使用同一套 primitive roots 解析规则，尽量保证“启动时看到的”和“运行时实际可用的”一致。
- 内置 `shell` 原语的新代码推荐用 `command=`，同时也兼容模型常见会写的 `cmd=` 别名。
- 模型应先查看 `<PTC_AVALIABLE_PRIMITIVES>`，如果不清楚某个原语或命名空间怎么调用，再用 `help("name")` 查看方法、参数、默认值和返回约定。
- 原语发现现在主要依赖清晰的公开名字，加上 `help(...)` 的结构化说明，而不是把大段签名直接塞进首屏提示词。
- PTC 原语除了执行能力，也可以通过 `get_workflow` 暴露渐进式工作流说明，用来指导安装、初始化和推荐的整体操作顺序。
- 对安装敏感、依赖连接或状态复用、或包含三步以上依赖关系的任务，模型应先看 `get_workflow`，再按步骤逐段执行，而不是一口气写一大段依赖脚本。
- 通用执行规则应保持与具体工具无关：后续步骤只能建立在前一步已经成功且产出了所需状态、标识符或句柄的前提上。
- 仓库内置的浏览器能力对外公开为 `agent_browser` 命名空间，常规调用应优先写成 `agent_browser.connect_cdp(...)`、`agent_browser.snapshot()`、`agent_browser.get_title()` 这类方法。
- 当浏览器能力可能还没安装或流程不熟时，可以先看 `agent_browser.get_workflow()`；它会返回一份独立维护的 Markdown workflow，描述安装步骤以及 `connect_cdp -> snapshot -> interaction -> wait -> re-snapshot -> read` 的循环流程。
- `connect_cdp` 成功后，`agent_browser` 会按 Zerda 会话保存一个默认浏览器 session，后续浏览器动作即使没显式传 `session` 也会优先复用同一个已连接浏览器。
- 浏览器相关的参数错误现在可能带有纠错数据，比如允许的 `kind` 取值、缺失的必填参数和示例调用。
- `agent_browser.close()` 只是显式清理动作，不应被当成默认收尾，因为连上的浏览器通常是用户自己打开的。
- 通过 `agent_browser` 生成的浏览器截图会写入当前 PTC 作业的 artifact 目录。
- Python 执行协议已收敛为单个 `<PTC_TOOL_CALLING>` 块，代码直接放在 body 里，不再额外嵌套 `<PYTHON>` 包装层。
- PTC body 本身已经运行在 runtime 的事件循环里，应该直接 `await`，不要再写 `asyncio.run()`。
- 只要不是非常简单的一步调用，都应该显式给 `result` 赋值，避免 runtime 落盘 `null`。
- 运行时用于常规执行时只接受精确的 `<PTC_TOOL_CALLING>` 标签；错误的 provider 风格包装会被当成提示词错误，而不会再做兼容归一化。
- 当 PTC 的 `out.json` 结果超过 8,000 字符时，Zerda 会把“原始主模型请求上下文 + PTC 结果”交给 `agent.fast_model` 进行压缩后再回注；完整原始结果仍保留在回注消息里的 `OUT_PATH` 所指向工件中。

```bash
set -a
source ~/.zerda/.env
set +a
./target/release/zerda --config ~/.zerda/zerda.toml
```

手动启动推荐目录结构：
- `~/.zerda/zerda.toml`
- `~/.zerda/identity.md`
- `~/.zerda/.env`

可选 WeChat 通道配置：

```toml
[[channels]]
name = "wechat"
gateway_url = "http://127.0.0.1:8080"
```

如果 Zerda 和 `wechat-agent-gateway` 跑在同一个 Docker Compose 栈里，应改成：

```toml
[[channels]]
name = "wechat"
gateway_url = "http://wechat-agent-gateway:8080"
```

EMA memory 现在在仓库维护的配置里默认开启。

- 裸机运行时，`[memory.chroma].url` 应保持为 `http://127.0.0.1:8000`，并确保本机已有 Chroma。
- 使用仓库自带 Compose 时，直接使用 `zerda.toml`，其中 `[memory.chroma].url` 已指向 `http://chroma:8000`。

WeChat 接入说明：

- Zerda 不直接实现微信协议，而是通过 [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway) 的 HTTP 接口接入。
- 在仓库自带的 Compose 部署里，Zerda 访问 gateway 应使用服务名 `http://wechat-agent-gateway:8080`，不要写 `127.0.0.1`。
- Zerda 把每个 WeChat gateway 实例都当成单账号入口处理；如果 gateway 里已经存在多个已配置账号，启动会直接报错，不会隐式挑一个。
- 启用 WeChat 通道后，如果 gateway 里还没有可复用的登录账号，`zerda serve` 启动时会在日志里打印终端二维码并等待扫码确认。
- WeChat 轮询在空结果时会保留上一次的拉取游标，避免下一轮把旧消息重新拉回来。
- WeChat 回复切分现在会尽量让短中回复保持单气泡，并避免出现单独以“比如：”这类连接词结尾的气泡。
- 想避免每次重启都重新扫码，必须给 gateway 配稳定的 `WECHAT_GATEWAY_STATE_PATH` 持久化目录；登录态保存在 gateway，不保存在 Zerda。
- 微信语音消息直接使用 gateway 已返回的转写文本，不依赖 Zerda 的 `[stt]` 配置。
- WeChat 出站图片现在支持和 Telegram 一样的富文本标记：`<image>/绝对路径.png</image>`。Zerda 会先把本地文件上传到 `wechat-agent-gateway`，再通过 `send_media` 发出去。

---

## 💻 CLI 使用手册

Zerda 提供了功能丰富的命令行接口：

| 命令 | 描述 |
| :--- | :--- |
| `zerda` | 直接进入交互式对话模式。 |
| `zerda run -m "<消息>"` | 执行单条指令后立即退出。 |
| `zerda run --resume [session_id]` | 恢复最近会话，或按会话 ID 恢复指定会话。 |
| `zerda serve` | 启动后台服务（例如 Telegram Bot 或 WeChat 通道监听）。 |
| `zerda config generate` | 输出完整配置模板（`zerda.toml.full`）。 |
| `zerda config validate` | 校验当前生效配置并退出。 |

### 🛠️ 交互模式命令

在交互式对话模式中，你可以输入以下 `/` 开头的快捷命令：

- `/help`：显示所有可用的快捷命令。
- `/model`：查看当前激活模型与可用 provider 列表。
- `/model <provider_id>@<model_name>`：即时切换到新模型（例如 `/model openai@gpt-4o`）。
- `/model <provider_id> list`：列出指定 provider 支持的模型（例如 `/model openai list`）。
- `/clear`：清空当前会话的历史记录。
- `/compact`：强制触发 LLM 进行上下文压缩。
- `/status`：查看当前的 Token 用量、预算限制及系统状态。
- `/jobs`：查看当前会话中的 PTC 作业。
- `/job <id>`：查看指定 PTC 作业详情。
- `/cancel-job <id>`：取消运行中的 PTC 作业。
- `/cancel`：取消当前正在执行的轮次。
- `/exit` / `/quit`：退出交互模式（CLI 会话）。

忙时行为：

- 在模型流式回复期间，`/status`、`/jobs`、`/job <id>`、`/cancel-job <id>` 会立即返回。
- `/compact` 不打断当前轮次，会进入队列并在当前轮次结束后执行。
- `/clear` 与 `/model <provider>@<model>` 会先取消当前轮次，再执行命令本身。

---

## 🔌 扩展能力

### ~~📜 技能系统 (Skills)~~
~~Skills 是存放在 `~/.zerda/skills/` 目录下的模块化指令集。它们用于定义 Agent 的专业工作流和垂直领域知识。~~

已移除。后续扩展能力会迁移到 Playbook，而不是恢复旧 Skills 机制。

### ~~🌐 MCP 集成~~
~~通过 Model Context Protocol (MCP)，安全地将 Agent 连接到外部生态系统（如本地数据库、代码仓库或云端 API）。~~

已移除。旧的 provider-level tools / MCP 执行路径已经统一迁移为 PTC。

### ~~🔍 文档搜索~~
~~Zerda 通过 `search_zerda_documents` 工具支持对自身项目文档的语义搜索。~~

旧 docs search 工具已移除。后续如果恢复，会以 PTC 原语或 PTC 工作流的形式重建，而不是恢复旧工具接口。

---

## 🧬 技术设计

<details>
<summary><b>展开技术设计</b></summary>

### KV-Cache 友好架构

Zerda 的系统提示词（System Prompt）完全静态化。identity、rules、环境元数据在构建时固定写入，所有动态内容（时间戳、任务状态、记忆召回上下文）仅注入到用户消息（User Message）的末端，绝不侵入系统提示词。会话历史遵循仅追加（Append-Only）原则：消息不做回溯修改，仅从头部截断或尾部追加，最大化 KV-Cache 前缀命中率。

### ~~Planner-Executor 解耦架构~~

~~Zerda 已从单体 ReAct 循环迁移为双层职责架构。高层负责理解用户意图与收敛回答，机械执行通过 PTC 异步作业完成。通过策略层与执行层的物理隔离，主链路上下文中的低层噪声显著减少，长对话下稳定性更好。~~

当前运行时已经收敛为单助手对话主链路加异步 PTC 作业，`Planner-Executor` 仅作为迁移历史保留。

### 编译器模式（Compiler Pattern）

在高层推理与执行之间，Zerda 采用编译器模式（Compiler Pattern）：把用户请求和环境反馈压缩为高密度结构化指令，再交给执行层处理。指令格式依然保持 `ACTION(params) -> {return_fields}` 的紧凑形式，降低委托阶段的解释成本。

### 程序化工具调用（PTC）

Zerda 现已把 ~~provider-level tools~~ 与 ~~MCP~~ 路径统一迁移为程序化工具调用（PTC）。执行能力通过 `<PTC_TOOL_CALLING>` 下推为异步 Python 作业，脚本、日志、结果和状态文件全部落盘为工件，由运行时回注结果而不是依赖 provider 原生工具调用。

PTC 不再只是“执行某个工具”的薄封装。对于复杂原语，PTC 还允许原语自身暴露 `get_workflow` 这类渐进式文档入口：模型先读取该原语返回的工作流 Markdown，再进入实际操作阶段。这样一来，过去由 Skills 承担的“先看指导，再开始工作”的链路，可以逐步迁移为原语本地携带的工作说明，而不用重新引入独立的 Skills 子系统。

### 预写原语层（Code Primitives）

在 PTC 之上，Zerda 提供“预写原语”层：把高频、易错、可复用的环境交互封装为 Python 异步函数，并在执行时注入给模型直接调用。原语负责参数校验、错误分类、超时控制和遥测落盘；任务级逻辑则由 PTC 作业组合这些原语完成。

这意味着原语现在承担两类职责：

1. 执行职责：真正访问文件系统、进程、网络或浏览器
2. 指导职责：通过 `get_workflow` 提供与该原语强绑定的渐进式使用说明

这种设计把执行能力与工作指导都收拢到同一个注册原语中，使文档版本、参数契约和实际行为保持同步，也让复杂能力可以按“查看 workflow -> 开始调用”的方式稳定工作。

### 本地状态与上下文压缩

当前运行时只保留本地可恢复状态，不再依赖外部长时记忆服务。保留的状态层包括：

1. 会话历史：保存在 `~/.zerda/sessions/`，用于恢复对话。
2. 上下文压缩摘要：当历史过长时由 fast model 压缩，用于后续轮次续接。
3. 压缩前转录快照：落在 `~/.zerda/memory/compaction/`，便于回查细节。
4. PTC 工件：落在 `~/.zerda/ptc_jobs/...`，保存脚本、日志、结果和状态。
5. 浏览器会话状态：`agent_browser` 会按 Zerda 会话持久化默认浏览器 session，便于后续复用。

### EMA 记忆系统设计

EMA（Essentialist Memory Architecture）的目标不是“把聊天记录全部记下来”，而是只保留那些会在未来继续影响回答质量或执行质量的长期信息。

#### 记录入口

EMA 不会把一整段对话直接写进向量库，而是先把每一轮已完成对话记到本地 journal，再由后台任务异步抽取结构化记忆。

完整链路是：

1. 用户和 assistant 完成一轮对话
2. 这一轮的 `user / assistant / runtime` 消息先原样写入 SQLite journal
3. 每轮结束后都会尝试触发一次后台维护检查
4. 只有当 pending turn 积攒到阈值，或最老 pending turn 超过年龄阈值时，后台任务才真正开始批量处理 journal
5. 后台任务再从这批 pending turn 中抽取可能值得长期保存的结构化 memory
6. 校验证据、处理冲突、写入正式 memory entry
7. 当前仍然 active 的 entry 再同步到 Chroma，供后续低延迟召回

也就是说，journal 是每轮都先写进去的；真正的 extraction / consolidation / decay 不是每轮都立刻执行，而是按 backlog 阈值批量触发。journal 负责留原始记录，memory entry 才是经过筛选和结构化后的长期记忆。

#### 记忆分类

EMA 当前把长期记忆分成两类：

1. personal memory：用户自己的长期信息，包括 `event`、`commitment`、`preference`、`profile fact`、`constraint`，以及从这些稳定事实里巩固出来的 `personal insight`
2. operational memory：从已完成执行链路中提炼出的可复用操作经验，包括 `procedure`、`failure pattern`，以及进一步巩固出的 `operational insight`

这两类记忆分开存，是为了避免“用户是谁”和“系统该怎么做事”混在一起，导致召回时上下文污染。

#### 入库规则

EMA 不是“模型觉得有用就存”，而是有明确证据门槛：

1. `personal memory` 必须能被当前回合里的用户原话直接引用证明
2. `operational memory` 必须能被当前回合里的 assistant 或 runtime 输出直接证明
3. 没有直接引文证据的 proposal 会被拒绝
4. `personal memory` 不存 `procedure`

所以它不是模糊印象记忆，而是“有来源、可追溯、可复核”的长期记忆。

#### 存储结构

EMA 使用两层存储：

1. SQLite：保存 turn journal、memory entry、状态变化、entry link 和 recall log
2. Chroma：只保存当前 active memory 的 embedding 和检索 metadata

这样分层的原因是：

1. SQLite 适合保留完整时序、证据和状态变化，便于审计和回放
2. Chroma 适合做当前活跃记忆的语义近邻检索，保证召回速度

前者负责“记得住且能回查”，后者负责“问的时候能快速想起来”。

#### 召回流程

当用户发来一条新消息时，EMA 不会把所有长期记忆都塞回 prompt，而是先做意图分流，再做分类型召回。

当前大致规则是：

1. 普通开放式提问：优先召回 `profile fact / commitment / preference / constraint / event / insight`
2. troubleshooting、恢复、排错类问题：额外提高 `failure pattern / procedure` 的权重
3. 时间相关问题：优先走 `event / commitment` 的时间窗口召回

实际召回链路是：

1. 先分析当前问题更像什么类型
2. 从 SQLite 取精确候选
3. 从 Chroma 取向量候选
4. 用确定性规则做重排
5. 只把最相关的少量记忆块注入当前用户消息

这样设计的目的，是让不同问题只带回最需要的那部分记忆，而不是把所有历史都重新灌进上下文。

#### 后台维护

EMA 的复杂整理不放在前台回复主链路里，而放在异步后台维护任务里完成。后台主要做四件事：

1. extraction：从 journal 中抽取结构化 memory proposal
2. conflict handling：处理同一语义轴上的更新、替换、取消与失效
3. consolidation：把多条稳定事实或多次重复经验巩固成 insight
4. decay：让长期不用且价值低的 active memory 自动冷却归档

这就是 EMA 的基本原则：

1. 热路径简单，只做判断、检索、重排、注入
2. 冷路径异步，把抽取、巩固、冲突处理、衰减都放到后台
3. 证据优先，尽量避免模型凭感觉制造长期记忆

所以从原理上说，EMA 不是“聊天记录检索”，而是“带证据约束的结构化长期记忆系统”。

### 抗上下文腐败（Context Rot）

该架构针对 Context Rot 做了显式设计：异常栈、重试细节、机械噪声主要沉淀在 PTC 工件与日志中，主对话只接收决策级结果。这样可以避免复杂任务中过多失败轨迹污染高层上下文。

### Token 效率观测

在工具密集型场景中，PTC 相比旧的 provider-level tools 路径更容易把执行噪声隔离在工件中，从而降低主上下文的膨胀速度。实际收益会随任务形态、输出长度和原语组合方式变化。

### 文件系统上下文

大文件（>10 MB）从不全量加载。执行工件采用分层目录持久化：`~/.zerda/ptc_jobs/<YYYYMMDD>/<HHMMSS>_<task_slug>/`，脚本、日志、结果、状态分离，便于复盘且降低主链路上下文污染。自动压缩时，完整对话转录持久化到 `memory/compaction/` 目录，摘要中保留恢复路径。

### Segmented Content Isolation（分段式内容隔离）

多来源信息混入同一文本块会导致提示词稀释。Zerda 将用户消息的 `content` 字段组织为独立文本块数组，当前主要包括：运行时状态、会话摘要、时间戳与用户输入。各块语义独立，可按需增删而不影响其他块的完整性。

### System/User Prompt Layering（提示词分层架构）(Experimental)

提示词架构分为两层。**系统提示词** 作为静态内核：identity → rules → env。**用户提示词** 作为动态外壳：通过结构化块按当前阶段注入运行时状态、摘要与用户输入。

</details>

---

## 🤖 For AI Agents

请查看 [AGENT_README.md](./AGENT_README.md) 获取运行时与代码结构说明。

仓库语言约定：

- 面向代码的资产保持英文
- 面向最终用户的本地化文档保留在 `README.zh-CN.md`


---

## 📄 开源协议

本项目采用双许可证：
- 开源使用：[AGPL-3.0-only](LICENSE)
- 专有/闭源使用：请联系维护者获取商业授权
