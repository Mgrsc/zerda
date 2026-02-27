from __future__ import annotations

from typing import Any

from .base import (
    HARD_NETWORK_TIMEOUT_SECS,
    PrimitiveContext,
    firecrawl_post,
    invalid_argument_result,
    load_context,
    run_with_guard,
    validate_http_url,
    validate_int_range,
)

ALLOWED_FORMATS = {"markdown", "html", "rawHtml", "links", "screenshot", "json"}
MAX_WAIT_FOR_MS = 20000
MAX_MAX_AGE = 86400


def _as_dict(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    return {}


def _validate_formats(raw: Any) -> list[str]:
    if raw is None:
        return ["markdown"]
    if not isinstance(raw, list):
        raise ValueError("参数 formats 必须是字符串数组")
    cleaned: list[str] = []
    for item in raw:
        value = str(item).strip()
        if not value:
            continue
        if value not in ALLOWED_FORMATS:
            raise ValueError(f"formats 包含不支持的值: {value}")
        cleaned.append(value)
    if not cleaned:
        raise ValueError("参数 formats 不能为空数组")
    return cleaned


def _scrape_operation(
    ctx: PrimitiveContext,
    url: str,
    formats: list[str],
    only_main_content: bool,
    max_age: int | None,
    wait_for_ms: int | None,
):
    payload: dict[str, Any] = {
        "url": url,
        "formats": formats,
        "onlyMainContent": only_main_content,
    }
    if max_age is not None:
        payload["maxAge"] = max_age
    if wait_for_ms is not None:
        payload["waitFor"] = wait_for_ms
    return firecrawl_post(
        ctx=ctx,
        endpoint="/v1/scrape",
        payload=payload,
        timeout_secs=HARD_NETWORK_TIMEOUT_SECS,
    )


def _normalize_scrape_data(raw: Any) -> Any:
    envelope = _as_dict(raw)
    payload = _as_dict(envelope.get("result"))
    payload_data = payload.get("data")
    if not isinstance(payload_data, dict):
        return envelope
    metadata = _as_dict(payload_data.get("metadata"))
    status_code = envelope.get("http_status")
    if status_code is None:
        status_code = metadata.get("statusCode")
    normalized: dict[str, Any] = {
        "success": bool(payload.get("success", True)),
        "http_status": status_code,
        "status_code": status_code,
        "metadata": metadata,
        "result": payload,
    }
    normalized.update(payload_data)
    return normalized


async def firecrawl_scrape_page(
    url: str,
    formats: list[str] | None = None,
    only_main_content: bool = True,
    max_age: int | None = None,
    wait_for_ms: int | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    调用 Firecrawl scrape 接口抓取单个页面内容。

    [Args]
    url: 目标页面 URL (http/https)
    formats: 输出格式列表，默认 ["markdown"]。支持: markdown, html, rawHtml, links, screenshot, json
    only_main_content: 仅保留正文 (默认 True)
    max_age: 缓存秒数 (0~86400)
    wait_for_ms: JS 渲染等待毫秒 (0~20000)

    [Output Contract]
    res = await firecrawl_scrape_page("https://example.com")
    assert res["status"] == "ok"           # 成功检查，唯一判断条件
    res["data"]["markdown"]                # 正文 Markdown (默认格式)
    res["data"]["metadata"]["title"]       # 页面标题
    res["data"]["metadata"]["description"] # 页面描述
    res["data"]["html"]                    # HTML (需 formats 含 "html")
    res["data"]["success"]                 # Firecrawl 成功标记 (bool)
    res["data"]["http_status"]             # HTTP 状态码 (int)

    [When NOT to use]
    需要搜索发现 URL 时用 firecrawl_search_web。
    """
    ctx = load_context()
    try:
        parsed_url = validate_http_url(url, "url")
        parsed_formats = _validate_formats(formats)
        parsed_wait_for_ms = (
            validate_int_range(wait_for_ms, "wait_for_ms", 0, MAX_WAIT_FOR_MS)
            if wait_for_ms is not None
            else None
        )
        parsed_max_age = (
            validate_int_range(max_age, "max_age", 0, MAX_MAX_AGE)
            if max_age is not None
            else None
        )
        if not isinstance(only_main_content, bool):
            raise ValueError("参数 only_main_content 必须是布尔值")
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="firecrawl_scrape_page",
        ctx=ctx,
        operation=lambda: _scrape_operation(
            ctx=ctx,
            url=parsed_url,
            formats=parsed_formats,
            only_main_content=only_main_content,
            max_age=parsed_max_age,
            wait_for_ms=parsed_wait_for_ms,
        ),
    )
    if result.status.value == "ok":
        result.data = _normalize_scrape_data(result.data)
    return result.to_public_dict()
