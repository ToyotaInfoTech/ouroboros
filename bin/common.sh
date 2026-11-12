#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

# Common bootstrap for project scripts
# - Enables strict mode and tracing
# - Computes project root based on the caller script
# - cd to root and activate Python venv if present

set -xe

# Determine the directory of the caller script (index 1 in BASH_SOURCE inside a sourced file)
CALLER_DIR="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
ROOT="$(dirname "${CALLER_DIR}")"

cd "${ROOT}"

if [ -f "${ROOT}/.venv/bin/activate" ]; then
  . "${ROOT}/.venv/bin/activate"
fi

export PYTHONPATH="${ROOT}/python:${PYTHONPATH:-}"
export SSH_OPTS="-o ForwardAgent=yes -o StrictHostKeyChecking=no"
