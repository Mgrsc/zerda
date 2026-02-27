from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any


class ActionStatus(str, Enum):
    OK = "ok"
    INVALID_ARGUMENT = "invalid_argument"
    TIMEOUT = "timeout"
    DEPENDENCY_MISSING = "dependency_missing"
    UPSTREAM_ERROR = "upstream_error"
    RATE_LIMITED = "rate_limited"
    INTERNAL_ERROR = "internal_error"


@dataclass(slots=True)
class PrimitiveResult:
    status: ActionStatus
    data: Any = None
    error_code: str | None = None
    error_message: str | None = None
    retryable: bool = False
    telemetry: dict[str, Any] | None = None

    def to_public_dict(self) -> dict[str, Any]:
        return {
            "status": self.status.value,
            "data": self.data,
            "error_code": self.error_code,
            "error_message": self.error_message,
            "retryable": self.retryable,
        }
