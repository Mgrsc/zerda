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
| `zerda run --resume [session_id]` | 恢复最近会话，或按会话 ID 恢复指定会话。 |
| `zerda serve` | 启动后台服务（例如 Telegram Bot 监听）。 |
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
- `/cancel`：取消当前正在执行的轮次。
- `/exit` / `/quit`：退出交互模式（CLI 会话）。

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

### 🔍 文档搜索

Zerda 通过 `search_zerda_documents` 工具支持对自身项目文档的语义搜索，使 Agent 能够按需查阅配置指南、命令参考和架构说明。

**当前支持的后端：[Cloudflare AutoRAG (AI Search)](https://developers.cloudflare.com/autorag/)**

配置步骤：

1. 将 `docs/zerda/` 目录下的文档文件上传到 Cloudflare R2 存储桶。
2. 在 Cloudflare 控制台创建一个 AutoRAG 实例并关联该 R2 存储桶。
3. 创建 Cloudflare API Token，权限选择 **Account → AI Search Index Engine → Run**。
4. 设置以下环境变量：
   ```env
   CF_AI_SEARCH_ACCOUNT_ID=<你的账户ID>
   CF_AI_SEARCH_API_TOKEN=<你的API Token>
   CF_AI_SEARCH_INSTANCE_NAME=<你的AutoRAG实例名>
   ```

三个变量全部设置后，`search_zerda_documents` 工具会自动注册。缺少任一变量时，该工具将被静默跳过，不会报错。

---

## 🧬 技术设计

<details>
<summary><b>展开技术设计</b></summary>

### KV-Cache 友好架构

Zerda 的系统提示词（System Prompt）完全静态化——identity、rules、环境元数据在构建时固定写入，所有动态内容（时间戳、任务状态、记忆召回上下文）仅注入到用户消息（User Message）的末端，绝不侵入系统提示词（System Prompt）。在 Planner 主循环中，内置工具定义列表（`reload → skill → todo → tts → delegate_to_executor`）顺序锁定，运行时不增删，防止工具定义哈希（Tool Definitions Hash）变化导致前缀缓存失效。会话历史遵循仅追加（Append-Only）原则：消息不做回溯修改，仅从头部截断或尾部追加，最大化 KV-Cache 前缀命中率。

### Planner-Executor 解耦架构

Zerda 已从单体 ReAct 循环迁移为双智能体架构。Planner 负责意图理解、任务降维拆解与最终综合；Executor 负责环境交互与机械执行。通过策略层与执行层的物理隔离，主链路上下文中低层工具噪声显著减少，长对话下高维推理稳定性更好。

该分层同时改善了并发扩展特性。工程上，Planner 可以扇出多个相互独立的执行节点，同时保持单一且清洁的高层推理线程。配合可横向扩展的 Executor worker，任务扇出速度可显著高于单体 ReAct 回路，而不会同比例污染主脑上下文。

### 编译器模式（Compiler Pattern）

在 Planner 与 Executor 之间，Zerda 引入了编译器模式（Compiler Pattern）：Planner 充当前端编译器，将用户冗杂的自然语言请求和环境反馈"编译"为高密度结构化指令后再传递给 Executor。指令采用 `ACTION(params) -> {return_fields}` 格式——紧凑、机器可读，消除叙述性开销，使 Executor 的执行目标无歧义化。

这与编译器将人类可读源码转换为优化中间表示（IR）的过程同构：Planner 吸收上下文、消解歧义，输出一条最小化指令；能力较弱的 Executor 模型只需忠实执行即可。效果是：委托阶段的 token 浪费大幅降低，Executor 误读意图的错误率下降，"理解做什么"（Planner）与"执行怎么做"（Executor）的职责分离更加彻底。

### 程序化工具调用（PTC）

Executor 采用程序化工具调用（PTC）进行计算下推。`execute_python_script` 以结构化字段接收纯 Python 代码，自动完成脚本落盘、执行、日志与结果输出，并返回标准化状态。这样可把原本多步工具链压缩为单个受控执行块，减少主循环中的工具调用冗余。

### 预写原语层（Code Primitives）

在 PTC 之上，Zerda 增加了“预写原语”机制：将高频、易错、可复用的环境交互预先实现为 Python 异步函数，并在 Executor 运行时注入给模型直接 `await` 调用。该层用于降低临时脚本拼装成本与字段路径猜测错误。

当前原语遵循统一契约：固定返回 `status/data/error_code/error_message/retryable`，并在 docstring 中显式声明 `[Output Contract]` 的成功判定和关键字段路径。对于 Firecrawl 类原语，返回结构采用扁平化优先（例如 `data.markdown`、`data.html`、`data.metadata`、`data.results` 可直接访问），同时保留上游原始 payload 兼容字段，兼顾稳定读取与向后兼容。

这一层本质上把“工具能力”与“任务级代码”解耦：原语负责参数校验、超时重试、错误分类和遥测落盘；模型只需组合调用顺序与业务逻辑。对应地，复杂条件约束尽量在运行时代码中校验，避免把不兼容的复杂 schema 直接压给模型侧工具定义。

### 轻量关系型混合记忆系统（[MemBurrow](https://github.com/Mgrsc/MemBurrow)）

Zerda 引入了轻量外部记忆服务 [MemBurrow](https://github.com/Mgrsc/MemBurrow)，主要解决长会话 Agent 在生产环境中的四类问题：

- 上下文反复回放：为了保留偏好与规则而反复喂长历史，导致 token 成本持续膨胀。
- 召回正确性漂移：仅靠向量相似度会召回“语义相近但操作上不正确”的记忆。
- 硬约束丢失：纯语义检索容易漏掉规则、偏好、安全约束等高优先信息。
- 召回链路脆弱：向量层波动时，记忆注入稳定性明显下降。

该记忆管线通过以下设计进行缓解：

1. SQL 作为事实真相层，向量层作为语义加速索引。
2. 意图路由：规则/偏好/约束类请求走 SQL-first，其它请求走混合召回。
3. Outbox 异步写入：API 快速返回，抽取/嵌入/索引在后台 worker 处理。
4. 多因子重排：综合语义相关性、重要度、置信度、新鲜度、作用域。
5. 优雅降级与修复：向量检索失败时回退 SQL，并通过周期性 reconciliation 降低 SQL-向量漂移。

对 Zerda 的直接收益是：减少历史回放造成的提示词膨胀，提升可执行约束的保留率，并在部分依赖异常时保持更稳定的记忆召回行为。

### Executor 反思闭环（ACON-inspired）

Zerda 在 Executor 路径实现的是 ACON（Agent Context Optimization）启发式反思闭环。目标是把记忆优化从“持续喂任务知识”转为“沉淀可复用的方法论与教训”（`How to act / What to avoid`）。每次执行前，系统会对委托指令做向量化检索，从 Qdrant 召回最相似的历史指南，并以精简的 `<system-reminder>` 注入到 Executor 提示词中。

执行过程中，系统按迭代记录工具错误和 traceback 信号。执行结束后，异步反思任务会做失败驱动对比：在同一轨迹内对照失败迭代与成功迭代，压缩出一条可迁移的操作指南。压缩提示词强约束输出为方法级经验（非领域事实）、短文本、祈使句，并要求可泛化到相似任务。

提炼后的指南会写回向量库，作为后续相似指令的先验。系统同时包含负反馈回收机制：若注入指南后任务最终仍失败，会删除本轮注入的指南条目，避免无效经验在后续任务中被持续放大。

边界说明：该实现不是论文中完整的 ACON 流水线。Zerda 当前聚焦在线 Executor 指导记忆，不包含论文里的完整离线 UT/CO 优化流程、专用 history/observation 压缩器训练链路，以及 compressor/agent 蒸馏流水线。

### 抗上下文腐败（Context Rot）

该架构针对 Context Rot 做了显式设计：异常栈、重试细节、机械噪声主要沉淀在 Executor 工件与日志中，Planner 仅接收决策级结果。在线索已充分时（包括 `links=[]` 这类负信息证据）可直接收敛；在线索不足时，再由 Planner 重置局部策略并分配新任务节点，避免在复杂任务中过早收敛。

### Token 效率观测

在 `example-docs/some-file/` 的网站调查样本对比中，Planner-Executor + PTC 相比旧的直接工具路径表现出显著降耗。

| 指标 | 传统 ReAct（单回路） | Planner-Executor + PTC |
| :--- | :--- | :--- |
| 主上下文中的工具轨迹暴露 | 高 | 低 |
| 主上下文中的机械报错噪声 | 高 | 主要隔离在 Executor 工件 |
| 典型工具链长度 | 更长、更碎 | 压缩为受控执行块 |
| Token 消耗（首轮样本） | 基线 | 样本观测约下降 80% |
| 多轮稳定性 | 轨迹累积后更快劣化 | 策略/执行分离后更稳定 |

以上表格属于初始第一轮测试与样本观测结果，不是通用基准。实际收益会随任务形态、工具扇出和输出长度而变化。
在持续多轮工具调用场景下，传统 ReAct 往往因推理、执行、重试和诊断信息共线累积而更快膨胀上下文；Planner-Executor 将大部分执行残留沉淀在 Executor 工件/日志中，因此 Planner 上下文增速通常更慢、稳定性更高。

### 文件系统上下文

大文件（>10 MB）从不全量加载，工具仅返回头尾预览和文件路径指针。当任意工具输出超过 `max_tool_output_chars` 阈值时，溢出内容写入临时文件，上下文中只保留路径引用。Executor 工件采用分层目录持久化：`~/.zerda/executor_jobs/<YYYYMMDD>/<HHMMSS>_<task_slug>/`，脚本/日志/结果/元数据分离，便于复盘且降低主链路上下文污染。自动压缩时，完整对话转录持久化到 `memory/compaction/` 目录，摘要中保留恢复路径，模型可随时追溯原始内容——在零即时推理开销下实现无损可恢复性。

### ToDo Recitation（待办事项背诵）

长会话中，模型易受”迷失在中间（Lost in the Middle）”效应和注意力盆地（Attention Basin）偏差影响，对位于上下文中部的指令关注度显著下降。为此，`TodoTool` 维护一个会话级（Session-Scoped）待办列表，每轮用户消息构建时通过 `pending_reminder()` 将未完成事项自动注入用户消息（User Message）靠近末端的位置。这一机制持续将全局目标推入模型的近因偏好（Recency Bias）注意力窗口，强制周期性复习，有效对抗注意力坍缩。

除了注意力锚定，`TodoTool` 同时承担复杂任务的编排职责。面对多步请求时，Planner 先通过 `todo(add)` 分解子任务，再逐个以编译后指令 delegate 给 Executor，每完成一个即 `todo(done)` 标记，形成可审计的执行轨迹。`TodoTool` 内部 `Mutex` 保护，支持单次迭代批量创建，典型 4 子任务工作流约 6 次迭代完成。

### Keep the Errors（保留错误现场）

Zerda 不会清理失败动作（Failed Actions）和工具报错（Tool Errors）。每次工具调用结果（含 `is_error` 标记）都会写回会话历史，并在后续推理中作为负面约束（Negative Constraints）参与上下文内学习（In-Context Learning）。这让模型能够利用已失败路径进行隐式回溯（Backtracking），减少同类错误的重复尝试；即使触发自动压缩，完整原始转录也会先落盘保存，确保错误现场可追溯。

### Segmented Content Isolation（分段式内容隔离）

多来源信息混入同一文本块会导致提示词稀释（Instruction Dilution），不同语义相互污染。Zerda 将用户消息（User Message）的 `content` 字段组织为独立文本块（Text Block）数组：`[skills_index, todo_reminder, memory_recall, conversation_summary, timestamp, user_input]`。各块语义独立，可按需增删而不影响其他块的完整性。安全准则作为独立块注入，实现反复强化。

### System/User Prompt Layering（提示词分层架构）(Experimental)

提示词架构分为两层。**系统提示词（System Prompt）** 作为静态内核：identity（角色锚定）→ rules（否定式约束前置）→ env（结构化标签）。**用户提示词（User Prompt）** 作为动态外壳：通过 `<system-reminder>` 标签实现越级提醒，内容块（Content Block）根据模型当前阶段（探索 / 规划 / 执行）动态组装。否定式约束（`NEVER` / `DO NOT`）前置以划定硬性禁区；结构化标签（`<env>`、`<memory-recall>`）便于精准提取。identity 文本位于系统提示词（System Prompt）的最前位，首句锚定身份，后续所有规则围绕该身份展开。

</details>


---

## 📄 开源协议

本项目基于 [MIT License](LICENSE) 开源。
