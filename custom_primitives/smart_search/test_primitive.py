from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
PRIMITIVES_ROOT = ROOT / "code_primitives" / "python"

for candidate in (ROOT, PRIMITIVES_ROOT):
    value = str(candidate)
    if value not in sys.path:
        sys.path.insert(0, value)

from custom_primitives.catalog import get_primitive_registry
from custom_primitives.smart_search import search


async def passthrough_run_with_guard(**kwargs):
    return kwargs["operation"]()


class FakeResponse:
    def __init__(self, payload: dict[str, object], status: int = 200) -> None:
        self.status = status
        self._payload = payload

    def read(self) -> bytes:
        return json.dumps(self._payload).encode("utf-8")

    def __enter__(self) -> FakeResponse:
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class SmartSearchPrimitiveTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.env_patch = patch.dict(
            os.environ,
            {
                "HOME": self.temp_dir.name,
                "PTC_SESSION_KEY": "cli:test-user",
            },
            clear=False,
        )
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)

    async def test_empty_query_returns_invalid_argument(self) -> None:
        result = await search.smart_search(query=" ")
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "invalid_argument")
        self.assertIn("query", result["error_message"])

    async def test_catalog_registers_primitive(self) -> None:
        registry = get_primitive_registry()
        self.assertIn("smart_search", registry)

    async def test_missing_endpoint_configuration_returns_dependency_missing(self) -> None:
        with patch.dict(
            os.environ,
            {
                "SMART_SEARCH_URL": "",
                "SMART_SEARCH_API_KEY": "",
                "SMART_SEARCH_MODEL": "",
            },
            clear=False,
        ):
            result = await search.smart_search(query="compare hermes agent and openclaw")

        self.assertEqual(result["status"], "dependency_missing")
        self.assertEqual(result["error_code"], "missing_dependency")
        self.assertIn("SMART_SEARCH_URL", result["error_message"])

    async def test_success_returns_answer_and_usage(self) -> None:
        captured: dict[str, object] = {}

        def fake_urlopen(request, timeout=0):
            headers = {key.lower(): value for key, value in request.header_items()}
            captured["url"] = request.full_url
            captured["timeout"] = timeout
            captured["authorization"] = headers.get("authorization")
            captured["content_type"] = headers.get("content-type")
            captured["payload"] = json.loads(request.data.decode("utf-8"))
            return FakeResponse(
                {
                    "id": "chatcmpl-123",
                    "object": "chat.completion",
                    "model": "grok-420",
                    "choices": [
                        {
                            "index": 0,
                            "finish_reason": "stop",
                            "message": {
                                "role": "assistant",
                                "content": "Hermes Agent focuses on workflow orchestration, while OpenClaw is more centered on tool-driven autonomy.",
                            },
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 12,
                        "completion_tokens": 18,
                        "total_tokens": 30,
                    },
                }
            )

        with patch.object(search, "run_with_guard", new=passthrough_run_with_guard):
            with patch.object(search, "urlopen", new=fake_urlopen):
                with patch.dict(
                    os.environ,
                    {
                        "SMART_SEARCH_URL": "https://grokkkk.oi-oi.de/v1/chat/completions",
                        "SMART_SEARCH_API_KEY": "sk-test",
                        "SMART_SEARCH_MODEL": "grok-420",
                    },
                    clear=False,
                ):
                    result = await search.smart_search(
                        query="hermes agent和openclaw有什么区别",
                        max_tokens=512,
                    )

        self.assertEqual(result["status"], "ok")
        self.assertEqual(
            result["data"]["answer"],
            "Hermes Agent focuses on workflow orchestration, while OpenClaw is more centered on tool-driven autonomy.",
        )
        self.assertEqual(result["data"]["model"], "grok-420")
        self.assertEqual(result["data"]["finish_reason"], "stop")
        self.assertEqual(result["data"]["usage"]["total_tokens"], 30)
        self.assertEqual(
            captured["payload"],
            {
                "model": "grok-420",
                "stream": False,
                "max_tokens": 512,
                "messages": [
                    {
                        "role": "user",
                        "content": "hermes agent和openclaw有什么区别",
                    }
                ],
            },
        )
        self.assertEqual(
            captured["url"],
            "https://grokkkk.oi-oi.de/v1/chat/completions",
        )
        self.assertEqual(captured["timeout"], 300.0)
        self.assertEqual(captured["authorization"], "Bearer sk-test")
        self.assertEqual(captured["content_type"], "application/json")

    async def test_uses_smart_search_timeout_for_guard(self) -> None:
        captured: dict[str, object] = {}

        async def fake_run_with_guard(**kwargs):
            captured["hard_timeout_secs"] = kwargs.get("hard_timeout_secs")
            return kwargs["operation"]()

        def fake_urlopen(request, timeout=0):
            del request
            del timeout
            return FakeResponse(
                {
                    "id": "chatcmpl-123",
                    "object": "chat.completion",
                    "model": "grok-420",
                    "choices": [
                        {
                            "index": 0,
                            "finish_reason": "stop",
                            "message": {
                                "role": "assistant",
                                "content": "ok",
                            },
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1,
                        "total_tokens": 2,
                    },
                }
            )

        with patch.object(search, "run_with_guard", new=fake_run_with_guard):
            with patch.object(search, "urlopen", new=fake_urlopen):
                with patch.dict(
                    os.environ,
                    {
                        "SMART_SEARCH_URL": "https://grokkkk.oi-oi.de/v1/chat/completions",
                        "SMART_SEARCH_API_KEY": "sk-test",
                        "SMART_SEARCH_MODEL": "grok-420",
                    },
                    clear=False,
                ):
                    result = await search.smart_search(query="hermes agent")

        self.assertEqual(result["status"], "ok")
        self.assertEqual(captured["hard_timeout_secs"], 300.0)


if __name__ == "__main__":
    unittest.main()
