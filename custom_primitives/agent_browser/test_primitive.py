from __future__ import annotations

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

from primitives.types import ActionStatus, PrimitiveResult

from custom_primitives.agent_browser import common, primitive


async def passthrough_run_with_guard(**kwargs):
    return kwargs["operation"]()


class AgentBrowserPrimitiveTests(unittest.IsolatedAsyncioTestCase):
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

    async def test_open_without_session_returns_missing_browser_session(self) -> None:
        result = await primitive.agent_browser(
            action="open",
            url="https://example.com",
        )
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "missing_browser_session")

    async def test_get_workflow_returns_markdown_without_unlocking_step(self) -> None:
        workflow_result = await primitive.agent_browser(action="get_workflow")
        self.assertEqual(workflow_result["status"], "ok")
        self.assertEqual(workflow_result["data"]["format"], "markdown")
        self.assertIn("npm install agent-browser", workflow_result["data"]["workflow"])
        self.assertIn("agent-browser install", workflow_result["data"]["workflow"])

        result = await primitive.agent_browser(action="get")
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "missing_required_parameter")
        self.assertEqual(
            result["data"]["allowed_kind_values"],
            list(common.SUPPORTED_GET_KINDS),
        )

    async def test_rejects_invalid_get_kind_with_allowed_values(self) -> None:
        result = await primitive.agent_browser(action="get", kind="bogus")
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "invalid_parameter_value")
        self.assertEqual(
            result["data"]["allowed_kind_values"],
            list(common.SUPPORTED_GET_KINDS),
        )

    async def test_attr_kind_requires_name(self) -> None:
        result = await primitive.agent_browser(
            action="get",
            kind="attr",
            target="@e1",
        )
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "missing_required_parameter")
        self.assertEqual(result["data"]["required_parameters"], ["name"])

    async def test_connect_persists_default_session_for_later_calls(self) -> None:
        connect_payload = PrimitiveResult(
            status=ActionStatus.OK,
            data={"parsed": {"data": {"cdpUrl": "ws://127.0.0.1/devtools/browser/test"}}},
            retryable=False,
        )
        open_payload = PrimitiveResult(
            status=ActionStatus.OK,
            data={"parsed": {"data": {"url": "https://example.com", "title": "Example"}}},
            retryable=False,
        )
        seen_sessions: list[str | None] = []
        payloads = [connect_payload, open_payload]

        def fake_run_agent_browser(command_argv, *, session=None, headed=False, timeout_secs=20.0):
            seen_sessions.append(session)
            return payloads.pop(0)

        with patch.object(primitive, "run_with_guard", new=passthrough_run_with_guard):
            with patch.object(primitive, "run_agent_browser", new=fake_run_agent_browser):
                connect_result = await primitive.agent_browser(
                    action="connect_cdp",
                    target="http://127.0.0.1:9222",
                )
                self.assertEqual(connect_result["status"], "ok")
                resolved_session = connect_result["data"]["session"]
                self.assertTrue(resolved_session)
                self.assertEqual(
                    common.load_browser_state().get("default_browser_session"),
                    resolved_session,
                )

                open_result = await primitive.agent_browser(
                    action="open",
                    url="https://example.com",
                )
                self.assertEqual(open_result["status"], "ok")
                self.assertEqual(seen_sessions, [resolved_session, resolved_session])


if __name__ == "__main__":
    unittest.main()
