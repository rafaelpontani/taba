#!/usr/bin/env bash
# The Ruby connector against a real Taba (connectors/contract/README.md), on the oldest Ruby it supports (2.1).
#   connectors/ruby/contract/run.sh      # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker ruby:2.1 connectors/ruby -e LANG=C.UTF-8 -- ruby contract/driver.rb "${args[@]}"
}

contract_main ruby "$@"
