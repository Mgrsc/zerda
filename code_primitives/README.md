# Code Primitives 规范

本目录用于存放可被 Executor 直接注入并调用的 Python 原语。

目标是让模型在机械执行阶段优先复用稳定原语，减少临时拼接脚本、降低参数幻觉和上下文噪声。

## 依赖策略

- 原语实现默认优先 Python 标准库。
- 只有在无法满足需求时才引入第三方依赖。
- 若引入第三方依赖，必须在容器镜像中预装并在本 README 明确记录，禁止让 Executor 在任务中临时安装依赖。
- 缺失依赖必须返回 `DEPENDENCY_MISSING`，不能让原语直接崩溃。

## 目录结构

- `python/bootstrap.py`
  - 运行时注入入口，把原语注册到脚本全局命名空间
- `python/primitives/types.py`
  - `ActionStatus`、`PrimitiveResult` 统一类型
- `python/primitives/base.py`
  - 通用能力：硬超时、重试、输入校验、遥测落盘、Firecrawl HTTP 封装
- `python/primitives/catalog.py`
  - 原语注册表
- `python/primitives/*.py`
  - 具体原语实现文件

## 原语设计硬约束

### 1. 强类型与结构化返回

每个原语必须返回 `PrimitiveResult.to_public_dict()` 结构：

- `status`
- `data`
- `error_code`
- `error_message`
- `retryable`

状态码使用 `ActionStatus`，禁止返回随意字符串。

注入给 Executor 的原语函数统一为 `async def`。调用方必须使用 `await`，不能按同步函数直接取值。

### 2. 强制硬超时

任何网络 I/O 或重计算操作都必须使用内部硬超时常量，不信任模型传入的 timeout 参数。

超时必须返回：

- `status = ActionStatus.TIMEOUT`
- `error_code = "operation_timeout"` 或 `"network_timeout"`

### 3. 深度输入校验

仅有类型注解不够。必须在原语内部做边界检查和格式检查。

示例：

- URL 必须 `http/https`
- `limit` 必须在定义区间
- `sources/formats` 必须在白名单

校验失败必须返回：

- `status = ActionStatus.INVALID_ARGUMENT`
- 明确 `error_message`（可指导模型下轮自修复）

### 4. 防御性错误处理

原语内部不可把底层异常直接抛给上层导致进程失败。

要求：

- 捕获依赖缺失、网络异常、上游 HTTP 异常
- 转换为标准状态码（`DEPENDENCY_MISSING`、`UPSTREAM_ERROR`、`RATE_LIMITED` 等）
- 标注是否可重试（`retryable`）

### 5. 幂等优先

原语设计默认幂等。相同参数重复调用，不应造成状态污染。

避免：

- append 语义写文件
- 非必要副作用

### 6. 静默遥测与结果隔离

遥测信息只写任务工件内 `telemetry.jsonl`，不直接塞给 Planner 上下文。

遥测最少包含：

- primitive 名称
- 状态码
- 耗时
- 重试次数
- 关键错误码

## Docstring 规范（面向 LLM）

每个对外原语必须包含以下段落：

- `[What it does]`
- `[Args]`
- `[Returns]`
- `[Output Contract]`
- `[When NOT to use]`
- `[Common Mistakes]`

重点：必须明确“不要怎么用”，用于降低模型误选原语概率。

`[Output Contract]` 必须写清楚成功判定与关键字段路径，供 Executor 严格按契约读取，禁止自由猜测键名。

## 命名规范

- 原语函数名必须精确、语义完整，避免缩写和模糊名称
- 使用动词+对象，例如：
  - `firecrawl_scrape_page`
  - `firecrawl_search_web`
- 不使用“网页爬取”这类自然语言函数名

## 环境变量门控

Firecrawl 原语只在具备配置时可用：

- `FIRECRAWL_API_KEY`（主）
- `FIRECRAWL_KEY`（兼容）
- `FIRECRAWL_BASE_URL`（可选）

如果 key 缺失，原语应返回 `DEPENDENCY_MISSING`，并由上层避免继续暴力重试。

## 新增原语步骤

1. 在 `python/primitives/` 新建文件并实现 async 函数
2. 按本规范补齐校验、硬超时、重试、Docstring、结构化返回
3. 在 `python/primitives/catalog.py` 注册函数
4. 确认函数签名与 Docstring 可被扫描器识别（会注入到 Executor catalog）

## 当前内置原语

- `extract_main_text_from_html`
  - 标准库原语，从 HTML 提取正文与标题
- `firecrawl_scrape_page`
  - 需 `FIRECRAWL_API_KEY`
- `firecrawl_search_web`
  - 需 `FIRECRAWL_API_KEY`

## 原语模板

```python
from __future__ import annotations

from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard
from .types import PrimitiveResult


def _operation(...) -> PrimitiveResult:
    ...


async def your_primitive(...) -> dict[str, Any]:
    """
    [What it does]
    ...

    [Args]
    ...

    [Returns]
    PrimitiveResult 公共字典: status/data/error_code/error_message/retryable

    [When NOT to use]
    ...

    [Common Mistakes]
    ...
    """
    ctx = load_context()
    try:
        ...
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="your_primitive",
        ctx=ctx,
        operation=lambda: _operation(...),
    )
    return result.to_public_dict()
```
