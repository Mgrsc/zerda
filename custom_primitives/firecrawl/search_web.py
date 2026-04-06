from __future__ import annotations

from typing import Any

from primitives.base import (
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


def _as_dict(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    return {}


def _validate_query(raw: Any) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError("Parameter query must not be empty")
    if len(value) > MAX_QUERY_LENGTH:
        raise ValueError(f"Parameter query exceeds max length {MAX_QUERY_LENGTH}")
    return value


def _search_operation(
    ctx: PrimitiveContext,
    query: str,
    limit: int,
    lang: str | None,
):
    payload: dict[str, Any] = {
        "query": query,
        "limit": limit,
    }
    if lang:
        payload["lang"] = lang
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
    lang: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Calls the Firecrawl search API for web search.

    [Args]
    query: Search query text.
    limit: Number of results (1~10, default 5).
    lang: Language code (optional, for example "zh", "en").

    [Output Contract]
    res = await firecrawl_search_web("query")
    assert res["status"] == "ok"
    res["data"]["results"]
    res["data"]["success"]

    [When NOT to use]
    Do not use this when you already have the target URL and only need page fetching.
    """
    ctx = load_context()
    try:
        parsed_query = _validate_query(query)
        parsed_limit = validate_int_range(limit, "limit", 1, MAX_LIMIT)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="firecrawl_search_web",
        ctx=ctx,
        operation=lambda: _search_operation(
            ctx=ctx,
            query=parsed_query,
            limit=parsed_limit,
            lang=lang,
        ),
    )
    if result.status.value == "ok":
        result.data = _normalize_search_data(result.data)
    return result.to_public_dict()
