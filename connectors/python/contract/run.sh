#!/usr/bin/env bash
# The Python connector against a real Taba (connectors/contract/README.md), on the oldest Python it supports (3.9).
#   connectors/python/contract/run.sh      # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker python:3.9-slim connectors/python -e PYTHONIOENCODING=utf-8 -e PYTHONDONTWRITEBYTECODE=1 -- \
    python contract/driver.py "${args[@]}"
}

contract_main python "$@"
