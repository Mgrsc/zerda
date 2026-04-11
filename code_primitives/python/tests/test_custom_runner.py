from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
PRIMITIVES_ROOT = ROOT / "code_primitives" / "python"

for candidate in (ROOT, PRIMITIVES_ROOT):
    value = str(candidate)
    if value not in sys.path:
        sys.path.insert(0, value)

import custom_runner


class CustomRunnerTests(unittest.IsolatedAsyncioTestCase):
    async def test_invoke_supports_positional_and_keyword_args(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            module_name = "custom_runner_demo_module"
            module_path = Path(temp_dir) / f"{module_name}.py"
            module_path.write_text(
                "\n".join(
                    [
                        "async def demo(value: str, suffix: str = '!') -> dict:",
                        "    return {",
                        "        'status': 'ok',",
                        "        'data': {'message': f'{value}{suffix}'},",
                        "        'error_code': None,",
                        "        'error_message': None,",
                        "        'retryable': False,",
                        "    }",
                    ]
                ),
                encoding="utf-8",
            )
            sys.path.insert(0, temp_dir)
            try:
                with patch.object(
                    sys,
                    "argv",
                    [
                        "custom_runner.py",
                        module_name,
                        "demo",
                        json.dumps(
                            {
                                "args": ["fox"],
                                "kwargs": {"suffix": "?"},
                            },
                            ensure_ascii=False,
                        ),
                    ],
                ):
                    result = await custom_runner._invoke()
            finally:
                sys.path.remove(temp_dir)
                sys.modules.pop(module_name, None)

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["message"], "fox?")

    async def test_invoke_accepts_legacy_keyword_only_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            module_name = "custom_runner_legacy_module"
            module_path = Path(temp_dir) / f"{module_name}.py"
            module_path.write_text(
                "\n".join(
                    [
                        "async def demo(value: str) -> dict:",
                        "    return {",
                        "        'status': 'ok',",
                        "        'data': {'message': value},",
                        "        'error_code': None,",
                        "        'error_message': None,",
                        "        'retryable': False,",
                        "    }",
                    ]
                ),
                encoding="utf-8",
            )
            sys.path.insert(0, temp_dir)
            try:
                with patch.object(
                    sys,
                    "argv",
                    [
                        "custom_runner.py",
                        module_name,
                        "demo",
                        json.dumps({"value": "legacy"}, ensure_ascii=False),
                    ],
                ):
                    result = await custom_runner._invoke()
            finally:
                sys.path.remove(temp_dir)
                sys.modules.pop(module_name, None)

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["message"], "legacy")


if __name__ == "__main__":
    unittest.main()
