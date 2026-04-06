from __future__ import annotations

import difflib
import inspect
import json
import os
import sys
from pathlib import Path


def _resolve_roots() -> list[Path]:
    raw_many = os.environ.get("PTC_PRIMITIVES_PY_ROOTS", "").strip()
    if raw_many:
        try:
            parsed = json.loads(raw_many)
            if isinstance(parsed, list):
                roots = [
                    Path(str(item)).expanduser().resolve()
                    for item in parsed
                    if str(item).strip()
                ]
                if roots:
                    return roots
        except json.JSONDecodeError:
            pass

    roots: list[Path] = []
    root = os.environ.get("PTC_PRIMITIVES_PY_ROOT", "").strip()
    if root:
        roots.append(Path(root).expanduser().resolve())
    return roots


for root in _resolve_roots():
    resolved = str(root)
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
    os.environ.get("PTC_DISABLED_PRIMITIVES", "")
)

NAMESPACE_RULES = {
    "agent_browser": "agent_browser_",
}


def write_ptc_result(payload):
    out_path_raw = os.environ.get("PTC_OUT_PATH", "").strip()
    out_path = Path(out_path_raw) if out_path_raw else Path("out.json")
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)


def read_ptc_result():
    out_path_raw = os.environ.get("PTC_OUT_PATH", "").strip()
    out_path = Path(out_path_raw) if out_path_raw else Path("out.json")
    if not out_path.exists():
        return None
    return json.loads(out_path.read_text(encoding="utf-8"))


def _first_non_empty_line(text: str) -> str:
    for raw in text.splitlines():
        line = raw.strip()
        if line:
            return line
    return ""


def _extract_doc_sections(docstring: str) -> dict[str, str]:
    sections: dict[str, str] = {}
    current: str | None = None
    lines: list[str] = []
    for raw in docstring.splitlines():
        line = raw.strip()
        if line.startswith("[") and line.endswith("]") and len(line) > 2:
            if current is not None:
                value = "\n".join(item for item in lines if item).strip()
                if value:
                    sections[current] = value
            current = line[1:-1]
            lines = []
            continue
        if current is not None:
            lines.append(line)
    if current is not None:
        value = "\n".join(item for item in lines if item).strip()
        if value:
            sections[current] = value
    return sections


def _parse_args_doc(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    current: str | None = None
    for raw in text.splitlines():
        line = raw.strip()
        if not line:
            continue
        if ":" in line:
            name, description = line.split(":", 1)
            candidate = name.strip()
            if candidate and " " not in candidate:
                current = candidate
                result[current] = description.strip()
                continue
        if current is not None:
            result[current] = f"{result[current]} {line}".strip()
    return result


def _annotation_text(annotation) -> str | None:
    if annotation is inspect.Signature.empty:
        return None
    if isinstance(annotation, str):
        return annotation
    name = getattr(annotation, "__name__", None)
    if isinstance(name, str) and name:
        return name
    return str(annotation).replace("typing.", "")


def _default_text(value) -> str | None:
    if value is inspect.Signature.empty:
        return None
    return repr(value)


def _parameter_kind_name(parameter: inspect.Parameter) -> str:
    if parameter.kind is inspect.Parameter.POSITIONAL_ONLY:
        return "positional_only"
    if parameter.kind is inspect.Parameter.POSITIONAL_OR_KEYWORD:
        return "positional_or_keyword"
    if parameter.kind is inspect.Parameter.KEYWORD_ONLY:
        return "keyword_only"
    if parameter.kind is inspect.Parameter.VAR_POSITIONAL:
        return "variadic_positional"
    return "variadic_keyword"


def _call_shape(name: str, fn) -> str:
    return f"{name}{inspect.signature(fn)}"


def _infer_requirements(name: str, fn) -> list[str]:
    requirements: list[str] = []
    if name == "agent_browser" or name.startswith("agent_browser."):
        requirements.append("agent-browser")
    try:
        source = inspect.getsource(fn)
    except (OSError, TypeError):
        source = ""
    if "FIRECRAWL_API_KEY" in source:
        requirements.append("FIRECRAWL_API_KEY")
    requirements.sort()
    return requirements


def _describe_callable(name: str, fn, *, workflow_entry: str | None = None) -> dict:
    docstring = inspect.getdoc(fn) or ""
    sections = _extract_doc_sections(docstring)
    args_doc = _parse_args_doc(sections.get("Args", ""))
    signature = inspect.signature(fn)
    parameters = []
    required_parameters = []
    defaults = {}
    for parameter in signature.parameters.values():
        required = (
            parameter.default is inspect.Signature.empty
            and parameter.kind
            not in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD)
        )
        if required:
            required_parameters.append(parameter.name)
        default_value = _default_text(parameter.default)
        if default_value is not None:
            defaults[parameter.name] = default_value
        parameters.append(
            {
                "name": parameter.name,
                "kind": _parameter_kind_name(parameter),
                "required": required,
                "default": default_value,
                "annotation": _annotation_text(parameter.annotation),
                "description": args_doc.get(parameter.name),
            }
        )
    summary = (
        sections.get("What it does")
        or _first_non_empty_line(docstring)
        or name
    )
    data = {
        "name": name,
        "kind": "method" if "." in name else "primitive",
        "summary": summary,
        "call_shape": _call_shape(name, fn),
        "parameters": parameters,
        "required_parameters": required_parameters,
        "defaults": defaults,
        "returns": sections.get("Output Contract", ""),
        "when_not_to_use": sections.get("When NOT to use", ""),
        "common_mistakes": sections.get("Common Mistakes", ""),
        "requirements": _infer_requirements(name, fn),
        "workflow_available": workflow_entry is not None,
        "workflow_entry": workflow_entry,
    }
    return data


class _NamespaceProxy:
    def __init__(self, name: str, methods: dict[str, object], legacy_callable=None):
        self._name = name
        self._methods = methods
        self._legacy_callable = legacy_callable

    async def __call__(self, *args, **kwargs):
        if self._legacy_callable is None:
            raise TypeError(
                f"{self._name} is a namespace, not a callable primitive. "
                f'Use help("{self._name}") and then call one of its methods'
            )
        return await self._legacy_callable(*args, **kwargs)

    def __getattr__(self, item: str):
        if item in self._methods:
            return self._methods[item]
        raise AttributeError(f"{self._name} has no method {item}")

    def __dir__(self):
        return sorted(set(super().__dir__()) | set(self._methods.keys()))


def _build_public_surface(registry: dict[str, object]):
    top_level: dict[str, object] = {}
    namespaces: dict[str, dict[str, object]] = {}
    for namespace, prefix in NAMESPACE_RULES.items():
        namespaces[namespace] = {
            "name": namespace,
            "legacy": registry.get(namespace),
            "methods": {},
        }
    for name, primitive in registry.items():
        matched_namespace = None
        for namespace, prefix in NAMESPACE_RULES.items():
            if name.startswith(prefix):
                matched_namespace = namespace
                method_name = name[len(prefix):]
                namespaces[namespace]["methods"][method_name] = primitive
                break
        if matched_namespace is not None:
            continue
        if name in namespaces:
            continue
        top_level[name] = primitive
    return top_level, namespaces


def _namespace_summary(namespace: str, legacy_callable, methods: dict[str, object]) -> str:
    docstring = inspect.getdoc(legacy_callable) or ""
    if not docstring:
        preferred = methods.get("get_workflow") or methods.get("connect_cdp")
        if preferred is not None:
            docstring = inspect.getdoc(preferred) or ""
    sections = _extract_doc_sections(docstring)
    return (
        sections.get("What it does")
        or _first_non_empty_line(docstring)
        or namespace
    )


def _public_names(top_level: dict[str, object], namespaces: dict[str, dict[str, object]]) -> list[str]:
    names = ["help", *top_level.keys(), *namespaces.keys()]
    names = sorted(set(names))
    return names


def _all_help_targets(top_level: dict[str, object], namespaces: dict[str, dict[str, object]]) -> list[str]:
    targets = ["help", *top_level.keys(), *namespaces.keys()]
    for namespace, entry in namespaces.items():
        for method_name in entry["methods"]:
            targets.append(f"{namespace}.{method_name}")
    return sorted(set(targets))

try:
    from primitives.catalog import get_primitive_registry
    from primitives.types import ActionStatus, PrimitiveResult
    from custom_primitives.catalog import (
        get_primitive_registry as get_custom_primitive_registry,
    )

    registry = {}
    registry.update(get_primitive_registry(disabled_primitives=disabled_primitives))
    registry.update(get_custom_primitive_registry(disabled_primitives=disabled_primitives))
    top_level, namespaces = _build_public_surface(registry)
    public_names = _public_names(top_level, namespaces)
    help_targets = _all_help_targets(top_level, namespaces)

    async def primitive_help(name: str | None = None):
        value = str(name or "").strip()
        if not value or value == "help":
            return PrimitiveResult(
                status=ActionStatus.OK,
                data={
                    "name": "help",
                    "kind": "primitive",
                    "summary": "Inspect public primitive names and callable contracts before writing PTC code",
                    "call_shape": "help(name=None)",
                    "parameters": [
                        {
                            "name": "name",
                            "kind": "positional_or_keyword",
                            "required": False,
                            "default": "None",
                            "annotation": "str | None",
                            "description": "Top-level primitive name, namespace name, or dotted method name",
                        }
                    ],
                    "required_parameters": [],
                    "defaults": {"name": "None"},
                    "returns": "Returns top-level public names or one structured callable/namespace description",
                    "examples": [
                        'help("firecrawl_search_web")',
                        'help("agent_browser")',
                        'help("agent_browser.connect_cdp")',
                    ],
                    "available_primitives": public_names,
                },
                retryable=False,
            ).to_public_dict()
        if value in namespaces:
            entry = namespaces[value]
            workflow_available = "get_workflow" in entry["methods"]
            workflow_entry = f"{value}.get_workflow" if workflow_available else None
            methods = []
            for method_name, primitive in sorted(entry["methods"].items()):
                public_name = f"{value}.{method_name}"
                methods.append(
                    {
                        "name": public_name,
                        "summary": _describe_callable(
                            public_name,
                            primitive,
                            workflow_entry=workflow_entry,
                        )["summary"],
                    }
                )
            return PrimitiveResult(
                status=ActionStatus.OK,
                data={
                    "name": value,
                    "kind": "namespace",
                    "summary": _namespace_summary(
                        value,
                        entry["legacy"],
                        entry["methods"],
                    ),
                    "methods": methods,
                    "workflow_available": workflow_available,
                    "workflow_entry": workflow_entry,
                    "requirements": ["agent-browser"] if value == "agent_browser" else [],
                },
                retryable=False,
            ).to_public_dict()
        if "." in value:
            namespace, method_name = value.split(".", 1)
            entry = namespaces.get(namespace)
            if entry is not None and method_name in entry["methods"]:
                workflow_available = "get_workflow" in entry["methods"] and method_name != "get_workflow"
                workflow_entry = f"{namespace}.get_workflow" if workflow_available else None
                return PrimitiveResult(
                    status=ActionStatus.OK,
                    data=_describe_callable(
                        value,
                        entry["methods"][method_name],
                        workflow_entry=workflow_entry,
                    ),
                    retryable=False,
                ).to_public_dict()
        if value in top_level:
            return PrimitiveResult(
                status=ActionStatus.OK,
                data=_describe_callable(value, top_level[value]),
                retryable=False,
            ).to_public_dict()
        suggestions = difflib.get_close_matches(value, help_targets, n=5, cutoff=0.35)
        return PrimitiveResult(
            status=ActionStatus.INVALID_ARGUMENT,
            data={
                "name": value,
                "did_you_mean": suggestions,
                "available_primitives": public_names,
            },
            error_code="not_found",
            error_message=f"No primitive or method named {value}",
            retryable=False,
        ).to_public_dict()

    globals()["help"] = primitive_help
    for name, primitive in top_level.items():
        globals()[name] = primitive
    for namespace, entry in namespaces.items():
        globals()[namespace] = _NamespaceProxy(
            namespace,
            methods=entry["methods"],
            legacy_callable=entry["legacy"],
        )
    globals()["__available_primitives__"] = public_names
    globals()["__help_targets__"] = help_targets
    globals()["__disabled_primitives__"] = sorted(disabled_primitives)
    globals()["write_ptc_result"] = write_ptc_result
    globals()["read_ptc_result"] = read_ptc_result
except Exception as exc:
    globals()["__available_primitives__"] = []
    globals()["__disabled_primitives__"] = sorted(disabled_primitives)
    globals()["__primitives_bootstrap_error__"] = str(exc)
    globals()["write_ptc_result"] = write_ptc_result
    globals()["read_ptc_result"] = read_ptc_result
