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
- [🧬 技术设计](#-技术设计)

---

## ✨ 核心特性

- 🧠 **多模型驱动**：无缝支持 OpenAI（同时兼容经典的 Chat Completions 接口与最新的 Responses 接口）和 Anthropic，并允许在运行时动态切换模型。
- 🔧 **PTC 执行路径**：机械执行统一通过 PTC 下推到异步 Python 作业，文件系统、进程、网页原语能力都通过受控工件链路运行。
- ~~🔌 **MCP 生态接入**~~：已移除，原有工具/MCP 路径已统一迁移到 PTC。
- ~~📜 **动态技能系统**~~：已移除，后续将迁移为 Playbook。
- 💬 **多端交互体验**：不仅支持直接通过 CLI 终端进行沉浸式对话，还支持通过 Telegram Bot 与 WeChat Gateway 远程接入；当前运行时仅支持 `telegram` 和 `wechat` 两种 channel。
- 🗜️ **智能上下文管理**：面对超长会话，系统会自动进行对话历史的 LLM 压缩与本地持久化存储，兼顾性能与记忆。
- 🧠 **EMA 记忆系统**：提供单用户全局长期记忆，支持偏好、约束、事件与操作经验的异步沉淀、召回与整理。当前 EMA 采用单一全局用户实体，所有会话天然共享同一套长期记忆。

---

## 🚀 快速开始

<details open>
<summary><b>🐳 Docker (推荐)</b></summary>

推荐直接用这条路径部署。

1. **准备环境目录**：
   ```bash
   mkdir zerda && cd zerda
   ```

2. **下载运行所需文件**：
   ```bash
   curl -fsSLO https://raw.githubusercontent.com/Mgrsc/zerda/main/{docker-compose.yml,.env.example,identity.md,zerda.toml.full} \
     && mv .env.example .env \
     && mv zerda.toml.full zerda.toml
   ```

3. **配置与启动服务**：
   先把 `zerda.toml.full` 改名为 `zerda.toml`，填好 `.env` 后启动：
   ```bash
   docker compose up -d
   ```

> 进阶配置请参考 [docker-compose.yml](docker-compose.yml)。
> 仓库自带的 Compose 栈会同时启动 Zerda、Chroma 和 `wechat-agent-gateway`。启动前先把 `zerda.toml.full` 改名为 `zerda.toml`。

</details>

---

## ⚙️ 配置说明

Zerda 使用 TOML 配置。用 [zerda.toml.full](zerda.toml.full) 作为完整模板，改名为 `zerda.toml` 后再启动。字段说明已经写在文件里。

### 🔑 环境变量
Zerda 会从进程环境变量中展开 TOML 里的 `${VAR}`。

- Docker 模式：`docker compose` 会自动加载 `.env`。
- 在 `zerda.toml` 里写 `${VAR}`，真实值放进 `.env`。
- 手动启动时，先把 `.env` 导出到当前 shell。
- 仓库维护的默认 embedding 配置会直接复用 `OPENAI_API_KEY` 与 `OPENAI_BASE_URL`；只有当 embedding 要走单独端点或凭据时，才需要额外改动。
- 当前 channel 仅支持 `telegram` 和 `wechat`。
- WeChat 通过 [`wechat-agent-gateway`](https://github.com/Mgrsc/wechat-agent-gateway) 接入，不直接处理微信协议。
- EMA memory 需要可访问的 Chroma；仓库自带 Compose 默认使用 `http://chroma:8000`。
- `ZERDA_PRIMITIVES_ROOT` 只有在你要覆盖默认原语发现路径时才需要设置。

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
