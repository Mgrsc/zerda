from __future__ import annotations

import re
from typing import Any

from primitives.base import dependency_missing_result
from primitives.types import ActionStatus, PrimitiveResult

SCRAPLING_TIMEOUT_SECS = 15
DEFAULT_WAIT_AFTER_LOAD_MS = 2000
MIN_MAX_CHARS = 500
MAX_MAX_CHARS = 200_000
MIN_WAIT_AFTER_LOAD_MS = 0
MAX_WAIT_AFTER_LOAD_MS = 30_000
X_HOSTS = {"x.com", "www.x.com", "twitter.com", "www.twitter.com"}
WECHAT_HOSTS = {"mp.weixin.qq.com"}
STEALTH_FALLBACK_HOSTS = {
    "www.zhihu.com",
    "zhihu.com",
    "juejin.cn",
    "www.juejin.cn",
    "reddit.com",
    "www.reddit.com",
}
GENERIC_UI_NOISE_EXACT = {
    "Log in",
    "Login",
    "Sign up",
    "Post",
    "Menu",
    "Open menu",
    "Open navigation",
    "Skip to main content",
    "See new posts",
    "Conversation",
    "Home",
    "About",
    "More",
}
TITLE_PATTERNS = (
    re.compile(r'<meta[^>]+property=["\']og:title["\'][^>]+content=["\']([^"\']+)'),
    re.compile(r'<meta[^>]+name=["\']twitter:title["\'][^>]+content=["\']([^"\']+)'),
    re.compile(r'<h1[^>]+id=["\']activity-name["\'][^>]*>(.*?)</h1>', re.S),
    re.compile(r"<title>(.*?)</title>", re.S),
)


def clean_text(value: str) -> str:
    text = re.sub(r"<[^>]+>", " ", value)
    text = re.sub(r"\s+", " ", text)
    return text.strip()


def find_first(html: str, patterns: tuple[re.Pattern[str], ...]) -> str:
    for pattern in patterns:
        match = pattern.search(html)
        if not match:
            continue
        value = match.group(match.lastindex or 1)
        cleaned = clean_text(value)
        if cleaned:
            return cleaned
    return ""


def truncate(value: str, max_chars: int) -> str:
    if len(value) <= max_chars:
        return value
    return value[:max_chars].rstrip()


def normalize_extracted_text(
    text: str,
    extra_drop_exact: set[str] | None = None,
) -> str:
    drop_exact = set(GENERIC_UI_NOISE_EXACT)
    if extra_drop_exact:
        drop_exact.update(extra_drop_exact)
    cleaned_lines: list[str] = []
    seen_consecutive = ""
    for raw_line in text.splitlines():
        line = re.sub(r"\s+", " ", raw_line).strip()
        if not line:
            if cleaned_lines and cleaned_lines[-1] != "":
                cleaned_lines.append("")
            continue
        if line in drop_exact:
            continue
        if len(line) <= 2 and all(ch in ">|•·-" for ch in line):
            continue
        if line == seen_consecutive:
            continue
        cleaned_lines.append(line)
        seen_consecutive = line
    normalized = "\n".join(cleaned_lines)
    normalized = re.sub(r"\n{3,}", "\n\n", normalized)
    return normalized.strip()


def route_requires_stealth(host: str) -> bool:
    return host.lower() in X_HOSTS


def route_forbids_stealth(host: str) -> bool:
    return host.lower() in WECHAT_HOSTS


def route_allows_stealth_fallback(host: str) -> bool:
    return host.lower() in STEALTH_FALLBACK_HOSTS


def should_retry_with_stealth(
    host: str,
    title: str | None,
    text: str,
    status_code: int | None,
) -> bool:
    normalized_host = host.lower()
    if route_requires_stealth(normalized_host):
        return False
    if not route_allows_stealth_fallback(normalized_host):
        return False
    short_text = len(text.strip()) < 120
    title_missing = not title
    shell_markers = (
        "log in to reddit",
        "get the reddit app",
        "欢迎来到知乎",
        "知乎，让每一次点击都充满意义",
        "创作者中心",
        "登录",
        "注册",
    )
    lowered = text.lower()
    has_shell_marker = any(marker.lower() in lowered for marker in shell_markers)
    bad_status = status_code is not None and int(status_code) >= 400
    return short_text or (title_missing and has_shell_marker) or bad_status


def _load_fetcher_class(name: str):
    try:
        from scrapling import fetchers as scrapling_fetchers

        return getattr(scrapling_fetchers, name)
    except Exception:
        return None


def static_fetcher_missing_result() -> PrimitiveResult:
    return dependency_missing_result(
        "Missing Scrapling fetcher runtime; install scrapling[fetchers] before using scrapling_fetch_page"
    )


def stealth_fetcher_missing_result() -> PrimitiveResult:
    return dependency_missing_result(
        "Missing Scrapling stealth runtime; install Scrapling browser dependencies before using scrapling_stealth_fetch_page"
    )


def browser_runtime_missing_result(message: str) -> PrimitiveResult:
    return dependency_missing_result(message)


def extract_response_payload(
    response: Any,
    url: str,
    html_field: str = "html",
) -> PrimitiveResult:
    try:
        html = str(getattr(response, "html_content", "") or "")
        final_url = str(getattr(response, "url", url) or url)
        status_code = int(getattr(response, "status", 200) or 200)
        content_type = None
        response_headers = getattr(response, "headers", None)
        if hasattr(response_headers, "get"):
            content_type = response_headers.get("Content-Type") or response_headers.get(
                "content-type"
            )
        return PrimitiveResult(
            status=ActionStatus.OK,
            data={
                "url": url,
                "final_url": final_url,
                "status_code": status_code,
                "content_type": content_type,
                html_field: html,
                "response": response,
            },
            retryable=False,
        )
    except Exception as exc:
        return PrimitiveResult(
            status=ActionStatus.INTERNAL_ERROR,
            error_code="scrapling_response_invalid",
            error_message=f"Scrapling response normalization failed: {exc}",
            retryable=False,
        )
