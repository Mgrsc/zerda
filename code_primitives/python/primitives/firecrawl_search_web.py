from __future__ import annotations

from typing import Any

from .base import (
    HARD_NETWORK_TIMEOUT_SECS,
    PrimitiveContext,
    firecrawl_post,
    invalid_argument_result,
    load_context,
    run_with_guard,
    validate_int_range,
)

MAX_QUERY_LENGTH = 500
MAX_LIMIT = 10
ALLOWED_SOURCES = {"web", "news", "images"}


def _as_dict(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    return {}


def _validate_query(raw: Any) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError("参数 query 不能为空")
    if len(value) > MAX_QUERY_LENGTH:
        raise ValueError(f"参数 query 超过长度限制 {MAX_QUERY_LENGTH}")
    return value


def _validate_sources(raw: Any) -> list[dict[str, str]]:
    if raw is None:
        return [{"type": "web"}]
    if not isinstance(raw, list):
        raise ValueError("参数 sources 必须是数组")
    out: list[dict[str, str]] = []
    for item in raw:
        source_type = ""
        if isinstance(item, str):
            source_type = item.strip()
        elif isinstance(item, dict):
            source_type = str(item.get("type", "")).strip()
        if source_type not in ALLOWED_SOURCES:
            raise ValueError(f"sources 包含不支持的 type: {source_type}")
        out.append({"type": source_type})
    if not out:
        raise ValueError("参数 sources 不能为空数组")
    return out


def _search_operation(
    ctx: PrimitiveContext,
    query: str,
    limit: int,
    sources: list[dict[str, str]],
):
    payload: dict[str, Any] = {
        "query": query,
        "limit": limit,
        "sources": sources,
    }
    return firecrawl_post(
        ctx=ctx,
        endpoint="/v1/search",
        payload=payload,
        timeout_secs=HARD_NETWORK_TIMEOUT_SECS,
    )


def _normalize_search_data(raw: Any) -> Any:
    envelope = _as_dict(raw)
    payload = _as_dict(envelope.get("result"))
    payload_data = payload.get("data")
    status_code = envelope.get("http_status")
    normalized: dict[str, Any] = {
        "success": bool(payload.get("success", True)),
        "http_status": status_code,
        "status_code": status_code,
        "result": payload,
    }
    if isinstance(payload_data, dict):
        normalized.update(payload_data)
        if "results" in payload_data and isinstance(payload_data.get("results"), list):
            normalized["results"] = payload_data["results"]
        return normalized
    if isinstance(payload_data, list):
        normalized["results"] = payload_data
        return normalized
    return envelope


async def firecrawl_search_web(
    query: str,
    limit: int = 5,
    sources: list[dict[str, str]] | list[str] | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    调用 Firecrawl search 接口进行网页搜索。

    [Args]
    query: 搜索词
    limit: 结果数量 (1~10，默认 5)
    sources: 数据源列表，支持 web/news/images

    [Output Contract]
    res = await firecrawl_search_web("query")
    assert res["status"] == "ok"           # 成功检查，唯一判断条件
    res["data"]["results"]                 # 搜索结果数组
    res["data"]["success"]                 # Firecrawl 成功标记 (bool)

    [When NOT to use]
    已有目标 URL 时用 firecrawl_scrape_page。
    """
    ctx = load_context()
    try:
        parsed_query = _validate_query(query)
        parsed_limit = validate_int_range(limit, "limit", 1, MAX_LIMIT)
        parsed_sources = _validate_sources(sources)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="firecrawl_search_web",
        ctx=ctx,
        operation=lambda: _search_operation(
            ctx=ctx,
            query=parsed_query,
            limit=parsed_limit,
            sources=parsed_sources,
        ),
    )
    if result.status.value == "ok":
        result.data = _normalize_search_data(result.data)
    return result.to_public_dict()
