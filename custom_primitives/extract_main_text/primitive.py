from __future__ import annotations

from typing import Any

from bs4 import BeautifulSoup

from primitives.base import invalid_argument_result, load_context, run_with_guard, validate_int_range
from primitives.types import ActionStatus, PrimitiveResult

BLOCK_TAGS = {
    "article",
    "main",
    "section",
    "p",
    "li",
    "blockquote",
    "pre",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
}


def _normalize_text(text: str) -> str:
    lines: list[str] = []
    for raw in text.splitlines():
        line = " ".join(raw.split()).strip()
        if line:
            lines.append(line)
    return "\n".join(lines)


def _collect_candidates(soup: BeautifulSoup) -> list[str]:
    candidates: list[str] = []
    root = soup.body or soup
    for tag in root.find_all(BLOCK_TAGS):
        text = tag.get_text(" ", strip=True)
        if len(text) >= 30:
            candidates.append(text)
    if not candidates:
        fallback = root.get_text("\n", strip=True)
        if fallback:
            candidates.append(fallback)
    return candidates


def _operation(html: str, max_chars: int) -> PrimitiveResult:
    try:
        soup = BeautifulSoup(html, "html.parser")
    except Exception as exc:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="html_parse_failed",
            error_message=f"Failed to parse HTML: {exc}",
            retryable=False,
        )
    candidates = _collect_candidates(soup)
    best = max(candidates, key=len) if candidates else ""
    normalized = _normalize_text(best)
    extracted = normalized[:max_chars]
    truncated = len(normalized) > len(extracted)
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "text": extracted,
            "char_count": len(extracted),
            "truncated": truncated,
        },
        retryable=False,
    )


async def extract_main_text_from_html(html: str, max_chars: int = 12000) -> dict[str, Any]:
    """
    [What it does]
    Extracts the most likely main readable text block from an HTML string.

    [Args]
    html: Raw HTML document string.
    max_chars: Maximum number of characters to keep in the extracted text.

    [Output Contract]
    res = await extract_main_text_from_html(html_string)
    assert res["status"] == "ok"
    res["data"]["text"]

    [When NOT to use]
    Do not use this when you need metadata, links, or full DOM traversal instead of readable body text.
    """
    ctx = load_context()
    try:
        html_value = str(html or "")
        if not html_value.strip():
            raise ValueError("Parameter html must not be empty")
        max_chars_value = validate_int_range(max_chars, "max_chars", 200, 100000)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="extract_main_text_from_html",
        ctx=ctx,
        operation=lambda: _operation(html_value, max_chars_value),
    )
    return result.to_public_dict()
