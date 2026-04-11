from __future__ import annotations

import json
import os
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

import bootstrap


class BootstrapTests(unittest.IsolatedAsyncioTestCase):
    async def test_custom_proxy_forwards_positional_args(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            runner_path = Path(temp_dir) / "runner.py"
            runner_path.write_text(
                "\n".join(
                    [
                        "import json",
                        "import sys",
                        "payload = json.loads(sys.argv[3])",
                        "print(json.dumps({",
                        "    'status': 'ok',",
                        "    'data': payload,",
                        "    'error_code': None,",
                        "    'error_message': None,",
                        "    'retryable': False,",
                        "}, ensure_ascii=False))",
                    ]
                ),
                encoding="utf-8",
            )
            entry = {
                "name": "get_lunar_info",
                "module": "custom_primitives.divination.primitive",
                "callable": "get_lunar_info",
                "python_executable": sys.executable,
            }
            with patch.dict(
                os.environ,
                {bootstrap.CUSTOM_RUNNER_ENV: str(runner_path)},
                clear=False,
            ):
                proxy = bootstrap._build_custom_proxy(entry)
                result = await proxy("2002-10-16 07:25:00")

        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["args"], ["2002-10-16 07:25:00"])
        self.assertEqual(result["data"]["kwargs"], {})


if __name__ == "__main__":
    unittest.main()
