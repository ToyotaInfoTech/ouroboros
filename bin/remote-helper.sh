#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0
# Remote execution helper for cargo-make tasks.
# Source this file in task scripts and call remote_guard at the top.
#
# When REMOTE is set (hostname or role), the current cargo-make task is
# re-executed on the remote host via SSH and the local script exits.
#
# Usage in Makefile.toml task scripts:
#   . bin/remote-helper.sh
#   remote_guard && exit 0
#   # ... local task logic follows ...
#
# Environment:
#   REMOTE                          Hostname (e.g. "ourocloud") defined in etc/hosts.toml
#   CARGO_MAKE_CURRENT_TASK_NAME    Set automatically by cargo-make

REMOTE_HELPER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REMOTE_HELPER_ROOT="$(dirname "${REMOTE_HELPER_DIR}")"

# Activate project venv if available (for parse-hosts.py / tomllib)
if [[ -f "${REMOTE_HELPER_ROOT}/.venv/bin/activate" ]]; then
    . "${REMOTE_HELPER_ROOT}/.venv/bin/activate"
fi

# SSH options: prefer caller-provided value; fall back to ForwardAgent=yes.
SSH_OPTS="${SSH_OPTS:--o ForwardAgent=yes -o StrictHostKeyChecking=no}"

remote_guard() {
    if [[ -z "${REMOTE:-}" ]]; then
        return 1  # No REMOTE set; continue with local execution
    fi

    local task="${CARGO_MAKE_CURRENT_TASK_NAME:-}"
    if [[ -z "${task}" ]]; then
        echo "Error: CARGO_MAKE_CURRENT_TASK_NAME is not set" >&2
        return 2
    fi

    local info ssh_target remote_dir
    info=$(python3 "${REMOTE_HELPER_DIR}/parse-hosts.py" info "${REMOTE}")
    ssh_target=$(echo "${info}" | cut -d' ' -f1)
    remote_dir=$(echo "${info}" | cut -d' ' -f2)

    echo "[remote] Executing '${task}' on ${REMOTE} (${ssh_target}:${remote_dir})"

    ssh ${SSH_OPTS} "${ssh_target}" \
        "cd ${remote_dir} && . \"\$HOME/.cargo/env\" && nohup cargo make ${task} &"

    return 0
}
