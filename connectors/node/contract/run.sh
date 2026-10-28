#!/usr/bin/env bash
# The Node.js connector against a real Taba (connectors/contract/README.md), on the oldest Node it supports (18).
#   connectors/node/contract/run.sh        # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker node:18-slim connectors/node -- node contract/driver.js "${args[@]}"
}

contract_main node "$@"
