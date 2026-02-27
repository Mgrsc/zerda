from __future__ import annotations

from html.parser import HTMLParser
import re
from typing import Any

from .base import invalid_argument_result, load_context, run_with_guard, validate_int_range
from .types import ActionStatus, PrimitiveResult

MAX_HTML_CHARS = 2_000_000
MIN_MAX_CHARS = 200
MAX_MAX_CHARS = 200_000
SKIP_TAGS = {
    "script",
    "style",
    "nav",
    "footer",
    "header",
    "aside",
    "noscript",
    "svg",
}


class MainTextParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self._skip_depth = 0
        self._context_depth = 0
        self._title_depth = 0
        self._chunks: list[str] = []
        self._title_parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        lower = tag.lower()
        if lower in SKIP_TAGS:
            self._skip_depth += 1
            return
        if lower == "title":
            self._title_depth += 1
        if lower in {"article", "main", "section"}:
            self._context_depth += 1
        if lower in {"p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "blockquote"}:
            self._chunks.append("\n")

    def handle_endtag(self, tag: str) -> None:
        lower = tag.lower()
        if lower in SKIP_TAGS and self._skip_depth > 0:
            self._skip_depth -= 1
            return
        if lower == "title" and self._title_depth > 0:
            self._title_depth -= 1
        if lower in {"article", "main", "section"} and self._context_depth > 0:
            self._context_depth -= 1
        if lower in {"p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "blockquote"}:
            self._chunks.append("\n")

    def handle_data(self, data: str) -> None:
        if self._skip_depth > 0:
            return
        text = data.strip()
        if not text:
            return
        if self._title_depth > 0:
            self._title_parts.append(text)
            return
        self._chunks.append(text if self._context_depth > 0 else f"{text} ")

    def result(self, max_chars: int) -> tuple[str, str]:
        title = re.sub(r"\s+", " ", " ".join(self._title_parts)).strip()
        merged = "".join(self._chunks)
        merged = re.sub(r"\n{3,}", "\n\n", merged)
        merged = re.sub(r"[ \t]{2,}", " ", merged)
        merged = merged.strip()
        if len(merged) > max_chars:
            merged = merged[:max_chars].rstrip()
        return title, merged


def _operation(html: str, max_chars: int) -> PrimitiveResult:
    parser = MainTextParser()
    parser.feed(html)
    parser.close()
    title, text = parser.result(max_chars)
    if not text:
        return PrimitiveResult(
            status=ActionStatus.UPSTREAM_ERROR,
            error_code="empty_extraction",
            error_message="HTML 中未提取到可读正文",
            retryable=False,
        )
    return PrimitiveResult(
        status=ActionStatus.OK,
        data={
            "title": title,
            "text": text,
            "chars": len(text),
        },
        retryable=False,
    )


async def extract_main_text_from_html(html: str, max_chars: int = 12000) -> dict[str, Any]:
    """
    [What it does]
    从 HTML 提取可读正文与标题。

    [Args]
    html: 原始 HTML 字符串
    max_chars: 返回文本最大长度 (200~200000，默认 12000)

    [Output Contract]
    res = await extract_main_text_from_html(html_string)
    assert res["status"] == "ok"           # 成功检查，唯一判断条件
    res["data"]["title"]                   # 页面标题
    res["data"]["text"]                    # 提取的正文
    res["data"]["chars"]                   # 正文字符数 (int)

    [When NOT to use]
    内容需要 JS 渲染后才出现时不要直接用。
    """
    ctx = load_context()
    try:
        payload = str(html or "").strip()
        if not payload:
            raise ValueError("参数 html 不能为空")
        if len(payload) > MAX_HTML_CHARS:
            raise ValueError(f"参数 html 超过长度限制 {MAX_HTML_CHARS}")
        parsed_max_chars = validate_int_range(
            max_chars, "max_chars", MIN_MAX_CHARS, MAX_MAX_CHARS
        )
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()

    result = await run_with_guard(
        primitive_name="extract_main_text_from_html",
        ctx=ctx,
        operation=lambda: _operation(payload, parsed_max_chars),
        max_retries=0,
    )
    return result.to_public_dict()
