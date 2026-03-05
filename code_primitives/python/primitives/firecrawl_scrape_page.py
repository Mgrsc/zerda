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
        raise ValueError("Parameter formats must be a list of strings")
    cleaned: list[str] = []
    for item in raw:
        value = str(item).strip()
        if not value:
            continue
        if value not in ALLOWED_FORMATS:
            raise ValueError(f"formats contains an unsupported value: {value}")
        cleaned.append(value)
    if not cleaned:
        raise ValueError("Parameter formats must not be an empty list")
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
    links_value = payload_data.get("links")
    key_links: list[str] = []
    if isinstance(links_value, list):
        key_links = [str(item) for item in links_value if str(item).strip()]
    elif isinstance(links_value, dict):
        internal_links = links_value.get("internal_links")
        external_links = links_value.get("external_links")
        if isinstance(internal_links, list):
            key_links.extend(str(item) for item in internal_links if str(item).strip())
        if isinstance(external_links, list):
            key_links.extend(str(item) for item in external_links if str(item).strip())
    normalized["key_links"] = key_links
    normalized["links_raw_type"] = type(links_value).__name__ if links_value is not None else "none"
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
    Calls the Firecrawl scrape API to crawl a single page's content.

    [Args]
    url: Target page URL (http/https)
    formats: List of output formats, default is ["markdown"]. Supported formats: markdown, html, rawHtml, links, screenshot, json
    only_main_content: Only retain main content (default True)
    max_age: Cache duration in seconds (0~86400)
    wait_for_ms: Milliseconds to wait for JS rendering (0~20000)

    [Output Contract]
    res = await firecrawl_scrape_page("https://example.com")
    assert res["status"] == "ok"           # Success check, the only judgment condition
    res["data"]["markdown"]                # Main content in Markdown (default format)
    res["data"]["metadata"]["title"]       # Page title
    res["data"]["metadata"]["description"] # Page description
    res["data"]["html"]                    # HTML (requires "html" to be included in formats)
    res["data"]["key_links"]               # Stable merged links list (list[str])
    res["data"]["links_raw_type"]          # Original links field type: list|dict|none
    res["data"]["success"]                 # Firecrawl success flag (bool)
    res["data"]["http_status"]             # HTTP status code (int)

    [When NOT to use]
    Use firecrawl_search_web when you need to discover URLs via search.

    [Common Mistakes]
    Do not assume res["data"]["links"] is always a dict.
    Use res["data"]["key_links"] as the stable link list field.
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
            raise ValueError("Parameter only_main_content must be a boolean")
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
