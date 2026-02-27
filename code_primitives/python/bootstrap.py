from __future__ import annotations

import os
import sys
from pathlib import Path

root = os.environ.get("EXECUTOR_PRIMITIVES_PY_ROOT", "").strip()
if root:
    resolved = str(Path(root).expanduser().resolve())
    if resolved not in sys.path:
        sys.path.insert(0, resolved)

enable_firecrawl = os.environ.get("EXECUTOR_ENABLE_FIRECRAWL_PRIMITIVES", "0").strip() == "1"

try:
    from primitives.catalog import get_primitive_registry

    registry = get_primitive_registry(enable_firecrawl=enable_firecrawl)
    for name, primitive in registry.items():
        globals()[name] = primitive
    globals()["__available_primitives__"] = sorted(registry.keys())
except Exception as exc:
    globals()["__available_primitives__"] = []
    globals()["__primitives_bootstrap_error__"] = str(exc)
