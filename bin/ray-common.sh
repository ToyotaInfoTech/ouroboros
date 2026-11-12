#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

# Common helpers for ray-up / ray-down scripts.
# This file is intended to be sourced.

# Project root directory (parent of the directory containing this file).
ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"

# Remote working directory on each node (kept as ~ so the remote shell expands it).
WORK="${WORK:-~/ouroboros}"

# SSH options: prefer agent forwarding; fall back to defaults if not set by caller.
SSH_OPTS="${SSH_OPTS:--o ForwardAgent=yes -o StrictHostKeyChecking=no}"

# Ray head node and worker nodes.
HEAD="${HEAD:-ouroworld}"
if ! declare -p WORKERS >/dev/null 2>&1; then
  WORKERS=("ouroplace-1" "ouroplace-2" "ouroplace-3")
fi

RAY_PORT="${RAY_PORT:-6379}"

ray_stop_host() {
  local host="$1"
  ssh ${SSH_OPTS} "ubuntu@${host}" "cd ${WORK} && source .venv/bin/activate && (ray stop --force >> /dev/null || true)"
}

ray_start_head() {
  ssh ${SSH_OPTS} "ubuntu@${HEAD}" "cd ${WORK} && source .venv/bin/activate && (ray stop --force >> /dev/null || true) && PYTHONPATH=python ray start --head --port=${RAY_PORT} --disable-usage-stats"
}

ray_start_worker() {
  local host="$1"
  ssh ${SSH_OPTS} "ubuntu@${host}" "cd ${WORK} && source .venv/bin/activate && (ray stop --force >> /dev/null || true) && PYTHONPATH=python ray start --address=${HEAD}:${RAY_PORT} --disable-usage-stats"
}

ray_up() {
  ray_start_head

  local worker
  for worker in "${WORKERS[@]}"; do
    ray_start_worker "${worker}"
  done
}

ray_down() {
  local worker
  for worker in "${WORKERS[@]}"; do
    ray_stop_host "${worker}"
  done

  ray_stop_host "${HEAD}"
}

ray_status() {
  ssh ${SSH_OPTS} "ubuntu@${HEAD}" "cd ${WORK} && source .venv/bin/activate && (ray status --address=${HEAD}:${RAY_PORT} || true)"
}