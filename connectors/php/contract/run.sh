#!/usr/bin/env bash
# The PHP connector against a real Taba (connectors/contract/README.md), on the oldest PHP it supports (5.6).
#   connectors/php/contract/run.sh      # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker php:5.6-cli connectors/php -- php contract/driver.php "${args[@]}"
}

contract_main php "$@"
