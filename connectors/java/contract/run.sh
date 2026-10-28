#!/usr/bin/env bash
# The Java connector against a real Taba (connectors/contract/README.md), on the oldest Java it supports (11).
#   connectors/java/contract/run.sh        # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

# The connector has no dependencies, so the driver is compiled with plain javac next to its sources.
contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker maven:3.9-eclipse-temurin-11 connectors/java -- sh -c \
    'javac -d /tmp/out $(find src/main/java contract -name "*.java") && java -cp /tmp/out ContractDriver "$@"' sh "${args[@]}"
}

contract_main java "$@"
