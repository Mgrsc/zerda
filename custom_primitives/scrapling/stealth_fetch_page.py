from __future__ import annotations

import re
from urllib.parse import urlparse
from typing import Any

from primitives.base import (
    HARD_OPERATION_TIMEOUT_SECS,
    invalid_argument_result,
    load_context,
    run_with_guard,
    validate_http_url,
    validate_int_range,
)
from primitives.types import ActionStatus, PrimitiveResult

from .common import (
    DEFAULT_WAIT_AFTER_LOAD_MS,
    MAX_MAX_CHARS,
    MAX_WAIT_AFTER_LOAD_MS,
    MIN_MAX_CHARS,
    MIN_WAIT_AFTER_LOAD_MS,
    SCRAPLING_TIMEOUT_SECS,
    TITLE_PATTERNS,
    _load_fetcher_class,
    browser_runtime_missing_result,
    clean_text,
    extract_response_payload,
    find_first,
    normalize_extracted_text,
    route_forbids_stealth,
    route_requires_stealth,
    stealth_fetcher_missing_result,
    truncate,
)

MAX_BODY_CHARS = 500_000
X_UI_NOISE_EXACT = {
    "Don’t miss what’s happening",
    "Don't miss what's happening",
    "People on X are the first to know.",
    "People on X are the first to know",
    "Log in",
    "Sign up",
    "Post",
    "See new posts",
    "Conversation",
}


def _normalize_headers(raw: Any) -> dict[str, str] | None:
    if raw is None:
        return None
    if not isinstance(raw, dict):
        raise ValueError("Parameter headers must be a dictionary of string pairs")
    normalized: dict[str, str] = {}
    for key, value in raw.items():
        name = str(key).strip()
        if not name:
            continue
        normalized[name] = str(value)
    return normalized or None


def _load_stealth_fetcher():
    return _load_fetcher_class("StealthyFetcher")


def _fetch_html(
    url: str,
    headers: dict[str, str] | None,
    wait_after_load_ms: int,
) -> PrimitiveResult:
    StealthyFetcher = _load_stealth_fetcher()
    if StealthyFetcher is None:
        return stealth_fetcher_missing_result()
    request_args: dict[str, Any] = {
        "headless": True,
        "network_idle": True,
        "wait": wait_after_load_ms,
        "timeout": SCRAPLING_TIMEOUT_SECS * 1000,
    }
    if route_requires_stealth(urlparse(url).netloc):
        request_args["wait_selector"] = '[data-testid="tweetText"]'
    if headers:
        request_args["extra_headers"] = headers
    try:
        response = StealthyFetcher.fetch(url, **request_args)
    except Exception as exc:
        message = str(exc)
        lowered = message.lower()
        if "playwright was just installed or updated" in lowered or "executable doesn't exist" in lowered:
            return browser_runtime_missing_result(
                "Missing Playwright browser runtime for scrapling_stealth_fetch_page; run playwright install"
            )
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="scrapling_stealth_fetch_failed",
            error_message=f"Scrapling stealth fetch failed: {exc}",
            retryable=False,
        )
    return extract_response_payload(response=response, url=url)


def _looks_like_x_error_shell(text: str) -> bool:
    lowered = text.lower()
    return "something went wrong" in lowered and "try again" in lowered


def _selector_text(node: Any) -> str:
    if hasattr(node, "get_all_text"):
        try:
            return str(
                node.get_all_text(ignore_tags=("script", "style", "noscript")) or ""
            )
        except Exception:
            pass
    return str(getattr(node, "text", "") or "")


def _extract_x_text(response: Any, html: str) -> str:
    text_blocks: list[str] = []
    if response is not None and hasattr(response, "css"):
        try:
            nodes = response.css('[data-testid="tweetText"]')
        except Exception:
            nodes = []
        for node in nodes[:3]:
            block = normalize_extracted_text(
                _selector_text(node),
                extra_drop_exact=X_UI_NOISE_EXACT,
            )
            if block and block not in text_blocks:
                text_blocks.append(block)
        if text_blocks:
            combined = "\n\n".join(text_blocks)
            return re.sub(r"\n{3,}", "\n\n", combined).strip()
        try:
            articles = response.css("article")
        except Exception:
            articles = []
        best = ""
        for node in articles[:3]:
            candidate = normalize_extracted_text(
                _selector_text(node),
                extra_drop_exact=X_UI_NOISE_EXACT,
            )
            if len(candidate) > len(best):
                best = candidate
        if best:
            return best
    return normalize_extracted_text(clean_text(html), extra_drop_exact=X_UI_NOISE_EXACT)


def _stealth_operation(
    url: str,
    headers: dict[str, str] | None,
    max_chars: int,
    wait_after_load_ms: int,
) -> PrimitiveResult:
    host = urlparse(url).netloc
    if route_forbids_stealth(host):
        return PrimitiveResult(
            status=ActionStatus.INVALID_ARGUMENT,
            error_code="stealth_forbidden_for_route",
            error_message="WeChat article URLs must use scrapling_fetch_page default routing instead of stealth fetch",
            retryable=False,
            data={
                "url": url,
                "route": "wechat_mp",
            },
        )
    route = "stealth_dynamic" if route_requires_stealth(host) else "stealth"
    fetched = _fetch_html(url, headers=headers, wait_after_load_ms=wait_after_load_ms)
    if fetched.status != ActionStatus.OK:
        if fetched.data is None:
            fetched.data = {}
        if isinstance(fetched.data, dict):
            fetched.data.setdefault("url", url)
            fetched.data.setdefault("route", route)
            fetched.data.setdefault("render_mode", "stealth")
        return fetched
    payload = dict(fetched.data or {})
    response = payload.get("response")
    html = str(payload.get("html") or "")
    if not html:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="empty_body",
            error_message="Fetched page returned an empty body",
            retryable=False,
        )
    if route_requires_stealth(urlparse(url).netloc):
        text = _extract_x_text(response, html)
    elif response is not None and hasattr(response, "get_all_text"):
        try:
            text = normalize_extracted_text(
                str(
                    response.get_all_text(ignore_tags=("script", "style", "noscript"))
                    or ""
                )
            )
        except Exception:
            text = normalize_extracted_text(clean_text(html))
    else:
        text = normalize_extracted_text(clean_text(html))
    title = find_first(html, TITLE_PATTERNS)
    if _looks_like_x_error_shell(text):
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="stealth_shell_page",
            error_message="Stealth fetch returned a shell/error page instead of article content",
            retryable=False,
            data={
                "url": payload.get("url"),
                "final_url": payload.get("final_url"),
                "status_code": payload.get("status_code"),
                "content_type": payload.get("content_type"),
                "route": route,
                "title": title or None,
                "text": truncate(text, max_chars),
            },
        )
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "url": payload.get("url"),
            "final_url": payload.get("final_url"),
            "status_code": payload.get("status_code"),
            "content_type": payload.get("content_type"),
            "route": route,
            "render_mode": "stealth",
            "title": title or None,
            "text": truncate(text, max_chars),
            "html": truncate(html, min(MAX_BODY_CHARS, max_chars * 3)),
        },
        retryable=False,
    )


async def scrapling_stealth_fetch_page(
    url: str,
    headers: dict[str, str] | None = None,
    max_chars: int = 20000,
    wait_after_load_ms: int = DEFAULT_WAIT_AFTER_LOAD_MS,
) -> dict[str, Any]:
    """
    [What it does]
    Fetch one page through Scrapling's browser-backed stealth fetcher and return normalized rendered content. This is intended for JavaScript-heavy or anti-bot-protected pages such as X/Twitter.

    [Args]
    url: Target page URL (http/https).
    headers: Optional request headers as a string-to-string dictionary.
    max_chars: Maximum text length returned in data.text (500~200000, default 20000).
    wait_after_load_ms: Extra wait time after load completion in milliseconds (0~30000, default 2000).

    [Output Contract]
    res = await scrapling_stealth_fetch_page("https://x.com/example/status/1")
    res["data"]["route"]
    res["data"]["render_mode"]
    res["data"]["text"]

    [When NOT to use]
    Do not use this for URL discovery. Use firecrawl_search_web first when the target URL is not known. Do not use this for interactive click flows; use agent_browser for that.
    """
    ctx = load_context()
    try:
        parsed_url = validate_http_url(url, "url")
        parsed_max_chars = validate_int_range(
            max_chars, "max_chars", MIN_MAX_CHARS, MAX_MAX_CHARS
        )
        parsed_wait_after_load_ms = validate_int_range(
            wait_after_load_ms,
            "wait_after_load_ms",
            MIN_WAIT_AFTER_LOAD_MS,
            MAX_WAIT_AFTER_LOAD_MS,
        )
        parsed_headers = _normalize_headers(headers)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="scrapling_stealth_fetch_page",
        ctx=ctx,
        operation=lambda: _stealth_operation(
            url=parsed_url,
            headers=parsed_headers,
            max_chars=parsed_max_chars,
            wait_after_load_ms=parsed_wait_after_load_ms,
        ),
        max_retries=0,
        hard_timeout_secs=HARD_OPERATION_TIMEOUT_SECS,
    )
    return result.to_public_dict()
