#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CUSTOM_ROOT="$ROOT_DIR/custom_primitives"
CACHE_ROOT="${ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR:-${HOME:-$ROOT_DIR}/.zerda/custom_primitives}"
STAMP_PATH="$CACHE_ROOT/requirements.sha256"
BROWSER_STAMP_PATH="$CACHE_ROOT/playwright.sha256"
PYTHON_BIN="${ZERDA_PTC_PYTHON:-/opt/zerda-python/bin/python}"
MERGED_REQUIREMENTS_PATH="$CACHE_ROOT/requirements.merged.txt"
STATE_PATH="$CACHE_ROOT/install.state"
LOG_PATH="$CACHE_ROOT/install.log"
PLAYWRIGHT_BROWSERS_DIR="${PLAYWRIGHT_BROWSERS_PATH:-$CACHE_ROOT/ms-playwright}"

ensure_cache_root() {
    mkdir -p "$CACHE_ROOT"
}

write_state() {
    printf '%s\n' "$1" > "$STATE_PATH"
}

prepare_runtime_env() {
    export ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR="$CACHE_ROOT"
    export ZERDA_PTC_PYTHON="$PYTHON_BIN"
    export PLAYWRIGHT_BROWSERS_PATH="$PLAYWRIGHT_BROWSERS_DIR"
}

needs_playwright_install() {
    previous_digest=""
    if [ -f "$BROWSER_STAMP_PATH" ]; then
        previous_digest="$(tr -d '\n' < "$BROWSER_STAMP_PATH")"
    fi
    if [ "$previous_digest" != "$1" ]; then
        return 0
    fi
    if [ ! -d "$PLAYWRIGHT_BROWSERS_DIR" ]; then
        return 0
    fi
    if ! find "$PLAYWRIGHT_BROWSERS_DIR" -mindepth 1 -maxdepth 1 -type d -name 'chromium-*' | grep -q .; then
        return 0
    fi
    return 1
}

needs_install() {
    previous_digest=""
    if [ -f "$STAMP_PATH" ]; then
        previous_digest="$(tr -d '\n' < "$STAMP_PATH")"
    fi
    if [ "$previous_digest" != "$1" ]; then
        return 0
    fi
    return 1
}

merged_requirements_need_playwright() {
    grep -Eiq '^[[:space:]]*playwright([[:space:]]*(\[.*\])?)?([<>=!~].*)?$' "$MERGED_REQUIREMENTS_PATH"
}

require_python_bin() {
    if [ ! -x "$PYTHON_BIN" ]; then
        echo "Unified PTC Python runtime not found at $PYTHON_BIN" >&2
        exit 1
    fi
}

install_requirements() {
    write_state "installing"
    if {
        set -euo pipefail
        ensure_cache_root
        uv pip install --python "$PYTHON_BIN" -r "$MERGED_REQUIREMENTS_PATH"
        if merged_requirements_need_playwright && needs_playwright_install "$1"; then
            "$PYTHON_BIN" -m playwright install chromium
            printf '%s\n' "$1" > "$BROWSER_STAMP_PATH"
        fi
        printf '%s\n' "$1" > "$STAMP_PATH"
    } >>"$LOG_PATH" 2>&1; then
        write_state "ready"
        echo "[custom-primitives] dependency install ready"
        return 0
    fi
    write_state "failed"
    echo "[custom-primitives] dependency install failed" >&2
    return 1
}

if ! command -v uv >/dev/null 2>&1; then
    echo "uv is required for custom primitive dependency installation" >&2
    exit 1
fi

if ! command -v zerda >/dev/null 2>&1; then
    echo "zerda executable not found in PATH" >&2
    exit 1
fi

mapfile -t requirement_files < <(
    find "$CUSTOM_ROOT" -mindepth 2 -maxdepth 2 -type f -name requirements.txt | sort
)

ensure_cache_root
prepare_runtime_env
require_python_bin

if [ "${#requirement_files[@]}" -gt 0 ]; then
    awk '
        {
            sub(/[[:space:]]*#.*/, "", $0)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", $0)
            if ($0 != "") print $0
        }
    ' "${requirement_files[@]}" | sort -u > "$MERGED_REQUIREMENTS_PATH"

    if [ -s "$MERGED_REQUIREMENTS_PATH" ]; then
        digest="$(sha256sum "$MERGED_REQUIREMENTS_PATH" | awk '{print $1}')"
        should_install=0
        if needs_install "$digest"; then
            should_install=1
        elif merged_requirements_need_playwright && needs_playwright_install "$digest"; then
            should_install=1
        fi
        if [ "$should_install" -eq 1 ]; then
            echo "[custom-primitives] installing runtime dependencies"
            install_requirements "$digest"
        else
            write_state "ready"
            echo "[custom-primitives] dependency install already satisfied"
        fi
    else
        write_state "ready"
        echo "[custom-primitives] no custom primitive dependencies declared"
    fi
else
    write_state "ready"
    echo "[custom-primitives] no custom primitive dependencies declared"
fi

exec zerda "$@"
