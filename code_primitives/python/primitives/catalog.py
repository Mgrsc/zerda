from __future__ import annotations

from typing import Any, Awaitable, Callable

from .fs_delete import fs_delete
from .fs_list import fs_list
from .fs_move import fs_move
from .fs_read import fs_read
from .fs_replace import fs_replace
from .fs_write import fs_write
from .process_poll import process_poll
from .process_run import process_run
from .process_spawn import process_spawn
from .process_terminate import process_terminate
from .shell import shell

PrimitiveCallable = Callable[..., Awaitable[dict[str, Any]]]


def get_primitive_registry(
    disabled_primitives: set[str] | None = None,
) -> dict[str, PrimitiveCallable]:
    disabled = disabled_primitives or set()
    registry: dict[str, PrimitiveCallable] = {
        "fs_delete": fs_delete,
        "fs_list": fs_list,
        "fs_move": fs_move,
        "fs_read": fs_read,
        "fs_replace": fs_replace,
        "fs_write": fs_write,
        "process_poll": process_poll,
        "process_run": process_run,
        "process_spawn": process_spawn,
        "process_terminate": process_terminate,
        "shell": shell,
    }
    return {name: fn for name, fn in registry.items() if name not in disabled}
