#!/usr/bin/env bash
# The Go connector against a real Taba (connectors/contract/README.md), on the oldest Go it supports (1.7).
#   connectors/go/contract/run.sh      # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  # Go 1.7 has no modules: the repository is mounted where GOPATH expects the package.
  docker run --rm --network "$TABA_CONTRACT_NETWORK" \
    -v "$(contract_hostpath "$REPO_ROOT"):/go/src/github.com/rafaelpontani/taba:ro" \
    -v "$(contract_hostpath "$REPO_ROOT"):/repo:ro" \
    -w /go/src/github.com/rafaelpontani/taba/connectors/go golang:1.7 \
    go run contract/main.go "${args[@]}"
}

contract_main go "$@"
