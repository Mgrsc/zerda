from __future__ import annotations

import json
import os
import sys
from pathlib import Path

root = os.environ.get("EXECUTOR_PRIMITIVES_PY_ROOT", "").strip()
if root:
    resolved = str(Path(root).expanduser().resolve())
    if resolved not in sys.path:
        sys.path.insert(0, resolved)


def _parse_disabled_primitives(raw: str) -> set[str]:
    text = raw.strip()
    if not text:
        return set()
    try:
        parsed = json.loads(text)
        if isinstance(parsed, list):
            return {str(item).strip() for item in parsed if str(item).strip()}
    except json.JSONDecodeError:
        pass
    return {item.strip() for item in text.split(",") if item.strip()}


disabled_primitives = _parse_disabled_primitives(
    os.environ.get("EXECUTOR_DISABLED_PRIMITIVES", "")
)

try:
    from primitives.catalog import get_primitive_registry

    registry = get_primitive_registry(disabled_primitives=disabled_primitives)
    for name, primitive in registry.items():
        globals()[name] = primitive
    globals()["__available_primitives__"] = sorted(registry.keys())
    globals()["__disabled_primitives__"] = sorted(disabled_primitives)
except Exception as exc:
    globals()["__available_primitives__"] = []
    globals()["__disabled_primitives__"] = sorted(disabled_primitives)
    globals()["__primitives_bootstrap_error__"] = str(exc)
