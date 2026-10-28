#!/usr/bin/env bash
# The Rust connector against a real Taba (connectors/contract/README.md), on the current stable Rust.
#   connectors/rust/contract/run.sh        # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

# The build goes to a Docker volume (kept between runs), not to the repository.
contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker rust:1 connectors/rust/contract -v taba-contract-cargo:/cargo-cache \
    -e CARGO_HOME=/cargo-cache/home -e CARGO_TARGET_DIR=/cargo-cache/target -- \
    cargo run --release --quiet -- "${args[@]}"
}

contract_main rust "$@"
