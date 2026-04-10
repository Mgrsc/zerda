#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CUSTOM_ROOT="$ROOT_DIR/custom_primitives"
CACHE_ROOT="${ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR:-${HOME:-$ROOT_DIR}/.zerda/custom_primitives}"
STAMP_PATH="$CACHE_ROOT/requirements.sha256"
PYTHON_REQUEST="${ZERDA_CUSTOM_PRIMITIVES_PYTHON:-${ZERDA_CUSTOM_PRIMITIVES_PYTHON_VERSION:-3.13}}"
VENV_PATH="${ZERDA_CUSTOM_PRIMITIVES_VENV_PATH:-$CACHE_ROOT/venv}"
PYTHON_REQUEST_STAMP="$CACHE_ROOT/python-request.txt"
MERGED_REQUIREMENTS_PATH="$CACHE_ROOT/requirements.merged.txt"
STATE_PATH="$CACHE_ROOT/install.state"
LOG_PATH="$CACHE_ROOT/install.log"
PID_PATH="$CACHE_ROOT/install.pid"
STATUS_FIFO_PATH="$CACHE_ROOT/install.status"

ensure_cache_root() {
    mkdir -p "$CACHE_ROOT"
}

write_state() {
    printf '%s\n' "$1" > "$STATE_PATH"
}

activate_custom_venv() {
    export ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR="$CACHE_ROOT"
    export ZERDA_CUSTOM_PRIMITIVES_VENV_PATH="$VENV_PATH"
    export VIRTUAL_ENV="$VENV_PATH"
    export PATH="$VENV_PATH/bin:$PATH"
}

needs_install() {
    previous_request=""
    if [ -f "$PYTHON_REQUEST_STAMP" ]; then
        previous_request="$(tr -d '\n' < "$PYTHON_REQUEST_STAMP")"
    fi
    if [ "$previous_request" != "$PYTHON_REQUEST" ]; then
        return 0
    fi
    if [ ! -x "$VENV_PATH/bin/python" ]; then
        return 0
    fi
    previous_digest=""
    if [ -f "$STAMP_PATH" ]; then
        previous_digest="$(tr -d '\n' < "$STAMP_PATH")"
    fi
    if [ "$previous_digest" != "$1" ]; then
        return 0
    fi
    return 1
}

install_requirements_background() {
    if [ -f "$PID_PATH" ]; then
        existing_pid="$(tr -d '\n' < "$PID_PATH" 2>/dev/null || true)"
        if [ -n "$existing_pid" ] && kill -0 "$existing_pid" 2>/dev/null; then
            return 0
        fi
    fi

    (
        set -euo pipefail
        ensure_cache_root
        printf '%s\n' "$$" > "$PID_PATH"
        write_state "installing"
        {
            if [ -d "$VENV_PATH" ]; then
                previous_request=""
                if [ -f "$PYTHON_REQUEST_STAMP" ]; then
                    previous_request="$(tr -d '\n' < "$PYTHON_REQUEST_STAMP")"
                fi
                if [ "$previous_request" != "$PYTHON_REQUEST" ]; then
                    rm -rf "$VENV_PATH"
                fi
            fi
            uv venv --allow-existing --python "$PYTHON_REQUEST" "$VENV_PATH"
            uv pip install --python "$VENV_PATH/bin/python" -r "$MERGED_REQUIREMENTS_PATH"
            printf '%s\n' "$PYTHON_REQUEST" > "$PYTHON_REQUEST_STAMP"
            printf '%s\n' "$1" > "$STAMP_PATH"
            write_state "ready"
            printf '%s\n' "[custom-primitives] dependency install ready" >/proc/1/fd/1
        } || {
            write_state "failed"
            printf '%s\n' "[custom-primitives] dependency install failed" >/proc/1/fd/2
        }
        rm -f "$PID_PATH"
    ) >>"$LOG_PATH" 2>&1 &
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
activate_custom_venv

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
        if needs_install "$digest"; then
            echo "[custom-primitives] dependency install started in background"
            install_requirements_background "$digest"
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
