from __future__ import annotations

from datetime import datetime, timezone
from html import escape
from html.parser import HTMLParser
import re
from typing import Any
from urllib.parse import urlparse

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
    MAX_MAX_CHARS,
    MIN_MAX_CHARS,
    SCRAPLING_TIMEOUT_SECS,
    TITLE_PATTERNS,
    clean_text,
    extract_response_payload,
    find_first,
    _load_fetcher_class,
    route_allows_stealth_fallback,
    route_requires_stealth,
    should_retry_with_stealth,
    static_fetcher_missing_result,
    truncate,
)

MAX_BODY_CHARS = 500_000
WECHAT_HOSTS = {"mp.weixin.qq.com"}
WECHAT_CONTENT_IDS = {"js_content", "js_content_container"}
WECHAT_SKIP_TAGS = {
    "script",
    "style",
    "svg",
    "noscript",
    "iframe",
    "form",
    "input",
    "button",
}
BLOCK_TAGS = {
    "article",
    "blockquote",
    "br",
    "dd",
    "div",
    "dt",
    "figcaption",
    "figure",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "li",
    "ol",
    "p",
    "section",
    "table",
    "tr",
    "ul",
}
VOID_TAGS = {
    "area",
    "base",
    "br",
    "col",
    "embed",
    "hr",
    "img",
    "input",
    "link",
    "meta",
    "param",
    "source",
    "track",
    "wbr",
}
AUTHOR_PATTERNS = (
    re.compile(r'<span[^>]+id=["\']js_name["\'][^>]*>(.*?)</span>', re.S),
    re.compile(r"var\s+nickname\s*=\s*htmlDecode\((['\"])(.*?)\1\)", re.S),
)
PUBLISH_TIME_PATTERNS = (
    re.compile(r'<em[^>]+id=["\']publish_time["\'][^>]*>(.*?)</em>', re.S),
    re.compile(r"var\s+publish_time\s*=\s*(['\"])(.*?)\1", re.S),
    re.compile(r"\bct\s*=\s*(['\"]?)(\d{10})\1"),
)
WECHAT_TEXT_STOP_MARKERS = (
    "\n推荐阅读",
    "\n相关阅读",
    "\n延伸阅读",
    "\n校 审 |",
    "\n校  审 |",
    "\n校\t审 |",
    "\n编 辑 |",
    "\n编  辑 |",
    "\n编辑 |",
    "\n作者 |",
)
WECHAT_DROP_LINE_PATTERNS = (
    re.compile(r"^\d{2,}$"),
    re.compile(r"^[A-Z][A-Z0-9 _-]{2,24}$"),
    re.compile(r"^SECTION$"),
    re.compile(r"^COUNT$"),
    re.compile(r"^CHANNEL\s*:.*AUTHOR\s*:.*$", re.I),
    re.compile(r"^file\.textSYSTEM_V\.[0-9.]+$", re.I),
    re.compile(r"^[↓↘↙→←]+$"),
    re.compile(r"^推荐点赞[收藏在看转发分享]*$"),
    re.compile(r"^[点赞在看收藏转发分享推荐]{2,12}$"),
)
WECHAT_HTML_STOP_PATTERNS = (
    re.compile(r'<section[^>]+class=["\']mp_profile_iframe_wrp["\'][^>]*>', re.I),
    re.compile(r"<mp-common-profile\b", re.I),
    re.compile(r'powered-by=["\']xiumi\.us["\']', re.I),
    re.compile(r">推荐阅读<", re.I),
    re.compile(r">相关阅读<", re.I),
)

def _find_publish_time(html: str) -> str | None:
    for pattern in PUBLISH_TIME_PATTERNS:
        match = pattern.search(html)
        if not match:
            continue
        value = match.group(match.lastindex or 1).strip()
        if not value:
            continue
        if value.isdigit() and len(value) == 10:
            try:
                return datetime.fromtimestamp(
                    int(value), tz=timezone.utc
                ).isoformat()
            except (OSError, OverflowError, ValueError):
                return value
        return clean_text(value)
    return None


def _looks_like_wechat_article(url: str) -> bool:
    parsed = urlparse(url)
    return parsed.netloc.lower() in WECHAT_HOSTS and parsed.path.startswith("/s")


def _load_fetcher():
    return _load_fetcher_class("Fetcher")


def _fetch_html(
    url: str,
    headers: dict[str, str] | None,
) -> PrimitiveResult:
    Fetcher = _load_fetcher()
    if Fetcher is None:
        return static_fetcher_missing_result()
    request_args: dict[str, Any] = {
        "follow_redirects": True,
        "timeout": SCRAPLING_TIMEOUT_SECS,
        "retries": 1,
    }
    if headers:
        request_args["headers"] = headers
    try:
        response = Fetcher.get(url, **request_args)
    except Exception as exc:
        message = str(exc)
        if "certificate" in message.lower():
            try:
                response = Fetcher.get(url, verify=False, **request_args)
            except Exception as retry_exc:
                return PrimitiveResult(
                    status=ActionStatus.UPSTREAM_ERROR,
                    error_code="scrapling_fetch_failed",
                    error_message=f"Scrapling fetch failed: {retry_exc}",
                    retryable=False,
                )
        else:
            return PrimitiveResult(
                status=ActionStatus.UPSTREAM_ERROR,
                error_code="scrapling_fetch_failed",
                error_message=f"Scrapling fetch failed: {exc}",
                retryable=False,
            )
    return extract_response_payload(response=response, url=url)


def _render_start_tag(tag: str, attrs: list[tuple[str, str | None]]) -> str:
    rendered = []
    for key, value in attrs:
        if value is None:
            rendered.append(key)
            continue
        rendered.append(f'{key}="{escape(value, quote=True)}"')
    if rendered:
        return f"<{tag} {' '.join(rendered)}>"
    return f"<{tag}>"


class _WechatFragmentExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._capturing = False
        self._depth = 0
        self._chunks: list[str] = []
        self._target_id: str | None = None

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        lower = tag.lower()
        attr_map = {key: value or "" for key, value in attrs}
        target_id = attr_map.get("id", "")
        if not self._capturing and target_id in WECHAT_CONTENT_IDS:
            self._capturing = True
            self._depth = 1
            self._target_id = target_id
            return
        if not self._capturing:
            return
        self._chunks.append(_render_start_tag(tag, attrs))
        if lower not in VOID_TAGS:
            self._depth += 1

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if not self._capturing:
            return
        rendered = _render_start_tag(tag, attrs)
        if rendered.endswith(">"):
            rendered = rendered[:-1] + " />"
        self._chunks.append(rendered)

    def handle_endtag(self, tag: str) -> None:
        if not self._capturing:
            return
        self._depth -= 1
        if self._depth <= 0:
            self._capturing = False
            return
        self._chunks.append(f"</{tag}>")

    def handle_data(self, data: str) -> None:
        if self._capturing:
            self._chunks.append(escape(data))

    def result(self) -> tuple[str | None, str]:
        return self._target_id, "".join(self._chunks).strip()


class _WechatContentParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._skip_depth = 0
        self._texts: list[str] = []
        self._images: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        lower = tag.lower()
        if lower in WECHAT_SKIP_TAGS:
            self._skip_depth += 1
            return
        if self._skip_depth > 0:
            return
        attr_map = {key: value or "" for key, value in attrs}
        if lower == "img":
            src = (
                attr_map.get("data-src")
                or attr_map.get("src")
                or attr_map.get("data-croporisrc")
            ).strip()
            if src and src not in self._images:
                self._images.append(src)
        if lower in BLOCK_TAGS:
            self._texts.append("\n")

    def handle_endtag(self, tag: str) -> None:
        lower = tag.lower()
        if lower in WECHAT_SKIP_TAGS and self._skip_depth > 0:
            self._skip_depth -= 1
            return
        if self._skip_depth > 0:
            return
        if lower in BLOCK_TAGS:
            self._texts.append("\n")

    def handle_data(self, data: str) -> None:
        if self._skip_depth > 0:
            return
        text = data.strip()
        if text:
            self._texts.append(text)

    def result(self, max_chars: int) -> tuple[str, list[str]]:
        text = "".join(self._texts)
        text = re.sub(r"\n{3,}", "\n\n", text)
        text = re.sub(r"[ \t]{2,}", " ", text)
        text = re.sub(r" *\n *", "\n", text)
        text = text.strip()
        if len(text) > max_chars:
            text = text[:max_chars].rstrip()
        return text, self._images


def _extract_wechat_fragment(html: str) -> tuple[str | None, str]:
    parser = _WechatFragmentExtractor()
    parser.feed(html)
    parser.close()
    return parser.result()


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
def _trim_wechat_noise_text(text: str) -> str:
    trimmed = text
    for marker in WECHAT_TEXT_STOP_MARKERS:
        index = trimmed.find(marker)
        if index != -1:
            trimmed = trimmed[:index].rstrip()
    metadata_match = re.search(r"\n校\s*审\s*\|", trimmed)
    if metadata_match:
        trimmed = trimmed[:metadata_match.start()].rstrip()
    metadata_match = re.search(r"\n编\s*辑\s*\|", trimmed)
    if metadata_match:
        trimmed = trimmed[:metadata_match.start()].rstrip()
    cleaned_lines: list[str] = []
    for raw_line in trimmed.splitlines():
        line = raw_line.strip()
        line = re.sub(r"(?<![A-Za-z])(SECTION|COUNT)(?![A-Za-z])", "", line)
        line = re.sub(r"file\.textSYSTEM_V\.[0-9.]+", "", line, flags=re.I)
        line = re.sub(r"\s{2,}", " ", line).strip()
        if not line:
            cleaned_lines.append("")
            continue
        if any(pattern.fullmatch(line) for pattern in WECHAT_DROP_LINE_PATTERNS):
            continue
        cleaned_lines.append(line)
    trimmed = "\n".join(cleaned_lines)
    trimmed = trimmed.rstrip('”"')
    trimmed = re.sub(r"\n{3,}", "\n\n", trimmed)
    return trimmed.strip()


def _trim_wechat_noise_html(html: str) -> str:
    end = len(html)
    for pattern in WECHAT_HTML_STOP_PATTERNS:
        match = pattern.search(html)
        if match:
            end = min(end, match.start())
    return html[:end].rstrip()


def _general_fetch(
    url: str,
    headers: dict[str, str] | None,
    max_chars: int,
) -> PrimitiveResult:
    fetched = _fetch_html(url, headers=headers)
    if fetched.status != ActionStatus.OK:
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
    if response is not None and hasattr(response, "get_all_text"):
        try:
            text = str(
                response.get_all_text(ignore_tags=("script", "style", "noscript"))
                or ""
            )
        except Exception:
            text = clean_text(html)
    else:
        text = clean_text(html)
    title = find_first(html, TITLE_PATTERNS)
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "url": payload.get("url"),
            "final_url": payload.get("final_url"),
            "status_code": payload.get("status_code"),
            "content_type": payload.get("content_type"),
            "route": "default",
            "title": title or None,
            "text": truncate(text, max_chars),
            "html": truncate(html, min(MAX_BODY_CHARS, max_chars * 3)),
        },
        retryable=False,
    )


def _general_fetch_with_stealth_fallback(
    url: str,
    headers: dict[str, str] | None,
    max_chars: int,
) -> PrimitiveResult:
    result = _general_fetch(url, headers=headers, max_chars=max_chars)
    if result.status != ActionStatus.OK:
        return result
    payload = dict(result.data or {})
    title = payload.get("title")
    text = str(payload.get("text") or "")
    status_code = payload.get("status_code")
    host = urlparse(url).netloc
    if not should_retry_with_stealth(
        host=host,
        title=title,
        text=text,
        status_code=status_code if isinstance(status_code, int) else None,
    ):
        return result
    from .stealth_fetch_page import _stealth_operation

    stealth = _stealth_operation(
        url=url,
        headers=headers,
        max_chars=max_chars,
        wait_after_load_ms=3000,
    )
    if stealth.status == ActionStatus.DEPENDENCY_MISSING:
        return result
    return stealth


def _wechat_fetch(
    url: str,
    headers: dict[str, str] | None,
    max_chars: int,
) -> PrimitiveResult:
    fetched = _fetch_html(url, headers=headers)
    if fetched.status != ActionStatus.OK:
        return fetched
    payload = dict(fetched.data or {})
    html = str(payload.get("html") or "")
    if not html:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="empty_body",
            error_message="Fetched page returned an empty body",
            retryable=False,
        )
    if "环境异常" in html and "去验证" in html:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="wechat_verification_required",
            error_message="WeChat returned a verification page instead of article content",
            retryable=False,
            data={
                "url": payload.get("url"),
                "final_url": payload.get("final_url"),
                "status_code": payload.get("status_code"),
                "route": "wechat_mp",
                "verification_required": True,
            },
        )
    content_id, fragment = _extract_wechat_fragment(html)
    if not fragment:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="wechat_content_not_found",
            error_message="Failed to locate WeChat article content container",
            retryable=False,
            data={
                "url": payload.get("url"),
                "final_url": payload.get("final_url"),
                "status_code": payload.get("status_code"),
                "route": "wechat_mp",
            },
        )
    parser = _WechatContentParser()
    parser.feed(fragment)
    parser.close()
    text, images = parser.result(max_chars=max_chars)
    text = _trim_wechat_noise_text(text)
    fragment = _trim_wechat_noise_html(fragment)
    if not text:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="wechat_text_empty",
            error_message="WeChat article content container was found but no readable text was extracted",
            retryable=False,
        )
    title = find_first(html, TITLE_PATTERNS)
    author = find_first(html, AUTHOR_PATTERNS) or None
    publish_time = _find_publish_time(html)
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "url": payload.get("url"),
            "final_url": payload.get("final_url"),
            "status_code": payload.get("status_code"),
            "content_type": payload.get("content_type"),
            "route": "wechat_mp",
            "container_id": content_id,
            "title": title or None,
            "author": author,
            "publish_time": publish_time,
            "text": text,
            "images": images,
            "content_html": truncate(fragment, min(MAX_BODY_CHARS, max_chars * 3)),
        },
        retryable=False,
    )


def _operation(
    url: str,
    headers: dict[str, str] | None,
    max_chars: int,
) -> PrimitiveResult:
    if _looks_like_wechat_article(url):
        return _wechat_fetch(url, headers=headers, max_chars=max_chars)
    if route_requires_stealth(urlparse(url).netloc):
        from .stealth_fetch_page import _stealth_operation

        return _stealth_operation(
            url=url,
            headers=headers,
            max_chars=max_chars,
            wait_after_load_ms=2000,
        )
    if route_allows_stealth_fallback(urlparse(url).netloc):
        return _general_fetch_with_stealth_fallback(
            url=url,
            headers=headers,
            max_chars=max_chars,
        )
    return _general_fetch(url, headers=headers, max_chars=max_chars)


async def scrapling_fetch_page(
    url: str,
    headers: dict[str, str] | None = None,
    max_chars: int = 20000,
) -> dict[str, Any]:
    """
    [What it does]
    Fetch one page and return cleaned page content. WeChat Official Account article URLs are routed automatically to a dedicated extractor that reads the article body from the WeChat content container and filters page noise.

    [Args]
    url: Target page URL (http/https).
    headers: Optional request headers as a string-to-string dictionary.
    max_chars: Maximum text length returned in data.text (500~200000, default 20000).

    [Output Contract]
    res = await scrapling_fetch_page("https://example.com")
    assert res["status"] == "ok"
    res["data"]["route"]
    res["data"]["title"]
    res["data"]["text"]

    [When NOT to use]
    Do not use this for URL discovery. Use firecrawl_search_web when you do not know the target page URL. Do not use this for click-driven flows or browser testing; use agent_browser for that.

    [Common Mistakes]
    For WeChat article URLs, read data.text and data.images instead of trying to consume the full raw page HTML. If the response reports verification_required, the page is blocked by WeChat risk control and needs a different execution path.
    """
    ctx = load_context()
    try:
        parsed_url = validate_http_url(url, "url")
        parsed_max_chars = validate_int_range(
            max_chars, "max_chars", MIN_MAX_CHARS, MAX_MAX_CHARS
        )
        parsed_headers = _normalize_headers(headers)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="scrapling_fetch_page",
        ctx=ctx,
        operation=lambda: _operation(
            url=parsed_url,
            headers=parsed_headers,
            max_chars=parsed_max_chars,
        ),
        max_retries=0,
        hard_timeout_secs=HARD_OPERATION_TIMEOUT_SECS,
    )
    return result.to_public_dict()
