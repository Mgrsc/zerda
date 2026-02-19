#!/bin/bash
set -e

CONFIG="${ZERDA_CONFIG:-/root/.zerda/zerda.toml}"
BACKUP="${CONFIG}.bak"
PACKAGES_FILE="/root/.zerda/packages.txt"
CRASH_THRESHOLD=15
MAX_BACKOFF=60
MAX_RESTARTS=20
STABLE_RUNTIME=300
backoff=1
restart_count=0

if [ -f "$PACKAGES_FILE" ] && [ -s "$PACKAGES_FILE" ]; then
    echo "[supervisor] Restoring packages from $PACKAGES_FILE ..."
    sudo -u builder paru -S --needed --noconfirm $(cat "$PACKAGES_FILE" | tr '\n' ' ') || true
    echo "[supervisor] Package restoration complete."
fi

BUNDLED_SKILLS="/usr/local/share/zerda/skills"
USER_SKILLS="/root/.zerda/skills"
if [ -d "$BUNDLED_SKILLS" ]; then
    mkdir -p "$USER_SKILLS"
    for skill_dir in "$BUNDLED_SKILLS"/*/; do
        skill_name=$(basename "$skill_dir")
        if [ ! -e "$USER_SKILLS/$skill_name" ]; then
            cp -r "$skill_dir" "$USER_SKILLS/$skill_name"
            echo "[supervisor] Installed bundled skill: $skill_name"
        fi
    done
fi

child_pid=0

forward_signal() {
    if [ "$child_pid" -ne 0 ]; then
        kill -TERM "$child_pid" 2>/dev/null
        wait "$child_pid" 2>/dev/null
    fi
    exit 0
}

trap forward_signal SIGTERM SIGINT

while true; do
    if [ -f "$CONFIG" ]; then
        cp "$CONFIG" "$BACKUP"
        echo "[supervisor] Backed up config to ${BACKUP}"
    fi

    start_time=$(date +%s)
    echo "[supervisor] Starting zerda $@ ..."

    zerda "$@" &
    child_pid=$!
    wait "$child_pid" 2>/dev/null
    exit_code=$?
    child_pid=0

    end_time=$(date +%s)
    runtime=$((end_time - start_time))

    if [ "$exit_code" -eq 0 ]; then
        echo "[supervisor] zerda exited normally."
        exit 0
    fi

    if [ "$exit_code" -eq 75 ]; then
        echo "[supervisor] Reload requested (exit 75). Restarting immediately..."
        backoff=1
        restart_count=0
        continue
    fi

    if [ "$runtime" -ge "$STABLE_RUNTIME" ]; then
        restart_count=0
    fi

    restart_count=$((restart_count + 1))
    if [ "$restart_count" -ge "$MAX_RESTARTS" ]; then
        echo "[supervisor] Reached max restarts ($MAX_RESTARTS). Giving up."
        exit 1
    fi

    if [ "$runtime" -lt "$CRASH_THRESHOLD" ]; then
        echo "[supervisor] zerda crashed after ${runtime}s (exit ${exit_code}). Likely bad config."
        if [ -f "$BACKUP" ]; then
            cp "$BACKUP" "$CONFIG"
            echo "[supervisor] Rolled back config from ${BACKUP}"
        fi
        sleep 2
        backoff=1
        continue
    fi

    echo "[supervisor] zerda exited with code ${exit_code} after ${runtime}s. Restarting in ${backoff}s..."
    sleep "$backoff"
    backoff=$((backoff * 2))
    if [ "$backoff" -gt "$MAX_BACKOFF" ]; then
        backoff=$MAX_BACKOFF
    fi
done
