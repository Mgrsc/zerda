from __future__ import annotations

import builtins
import json
import os
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None


def _resolve_primitives_root() -> Path:
    raw = (
        os.environ.get("EXECUTOR_PRIMITIVES_PY_ROOT", "").strip()
        or os.environ.get("ZERDA_PRIMITIVES_ROOT", "").strip()
    )
    if raw:
        return Path(raw).expanduser().resolve()
    return Path(__file__).resolve().parent


def _parse_disabled(raw: str) -> set[str]:
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


def _disabled_from_config() -> set[str]:
    if tomllib is None:
        return set()
    config_raw = os.environ.get("ZERDA_CONFIG", "").strip()
    config_path = Path(config_raw).expanduser() if config_raw else Path("~/.zerda/zerda.toml").expanduser()
    if not config_path.exists():
        return set()
    try:
        data = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except Exception:
        return set()
    agent = data.get("agent")
    if not isinstance(agent, dict):
        return set()
    values = agent.get("disabled_primitives")
    if not isinstance(values, list):
        return set()
    return {str(item).strip() for item in values if str(item).strip()}


def _inject() -> None:
    root = _resolve_primitives_root()
    root_str = str(root)
    if root_str not in sys.path:
        sys.path.insert(0, root_str)
    disabled = _disabled_from_config()
    disabled.update(_parse_disabled(os.environ.get("EXECUTOR_DISABLED_PRIMITIVES", "")))
    try:
        from primitives.catalog import get_primitive_registry

        registry = get_primitive_registry(disabled_primitives=disabled)
    except Exception:
        return
    for name, primitive in registry.items():
        setattr(builtins, name, primitive)
    setattr(builtins, "__available_primitives__", sorted(registry.keys()))
    setattr(builtins, "__disabled_primitives__", sorted(disabled))


_inject()
