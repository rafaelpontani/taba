#!/usr/bin/env bash
# Shared pieces of the connector contract tests (connectors/contract/README.md): one Taba node in a container
# (the one-node profile, no cluster, no TLS, a development token) and a helper to run a driver in the container of
# the connector's base version, on the node's Docker network.
#
# For `source`; it does nothing by itself. Variables:
#   TABA_IMAGE        the node image (default taba:local, built from the root Dockerfile if missing).
#   TABA_CONTRACT_KEEP=1   leaves the node running at the end.
#   TABA_URL / TABA_TOKEN / TABA_CONTRACT_NETWORK   when set (by the central run.sh), the node already exists.

CONTRACT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONNECTORS_DIR="$(cd "$CONTRACT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$CONNECTORS_DIR/.." && pwd)"

# Git Bash on Windows converts paths such as /work.
export MSYS_NO_PATHCONV=1

contract_step() { printf '\n== %s\n' "$*"; }
contract_die() { echo "FAILED: $*" >&2; exit 1; }

# A path Docker accepts as the source of a bind mount (on Windows, C:/...).
contract_hostpath() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }

contract_require_docker() {
  command -v docker >/dev/null || contract_die "docker not found"
  docker info >/dev/null 2>&1 || contract_die "Docker is not running"
}

CONTRACT_OWNS_NODE=0
CONTRACT_NODE_NAME=""

# Starts the node unless one was handed in (TABA_URL). Exports TABA_URL, TABA_TOKEN and TABA_CONTRACT_NETWORK.
contract_node_up() {
  if [ -n "${TABA_URL:-}" ]; then
    : "${TABA_TOKEN:?TABA_TOKEN must go with TABA_URL}" "${TABA_CONTRACT_NETWORK:?TABA_CONTRACT_NETWORK must go with TABA_URL}"
    return
  fi
  contract_require_docker
  local image="${TABA_IMAGE:-taba:local}" suffix="$$"
  if ! docker image inspect "$image" >/dev/null 2>&1; then
    contract_step "Building the node image ($image)"
    docker build -t "$image" "$REPO_ROOT"
  fi
  CONTRACT_OWNS_NODE=1
  CONTRACT_NODE_NAME="taba-contract-$suffix"
  TABA_CONTRACT_NETWORK="taba-contract-net-$suffix"
  contract_step "Starting one Taba node ($image)"
  docker network create "$TABA_CONTRACT_NETWORK" >/dev/null
  # The one-node profile: factor 1 needs no entry address, and allows TLS off and a development token.
  docker run -d --name "$CONTRACT_NODE_NAME" --network "$TABA_CONTRACT_NETWORK" \
    -e TABA_REPLICATION_FACTOR=1 \
    -e TABA_CLUSTER_TLS_ENABLED=false \
    -e TABA_AUTH_DEV_TOKEN=contract-dev-secret-0123456789abcdef0123 \
    -e TABA_OPERATIONS_PORT=9090 \
    "$image" >/dev/null
  local deadline=$((SECONDS + 90))
  until docker exec "$CONTRACT_NODE_NAME" wget -q -O /dev/null http://127.0.0.1:9090/health/ready 2>/dev/null; do
    [ "$SECONDS" -lt "$deadline" ] || { docker logs --tail 30 "$CONTRACT_NODE_NAME" >&2; contract_die "the node did not become ready"; }
    sleep 1
  done
  echo "  ready"
  export TABA_URL="http://$CONTRACT_NODE_NAME:8080"
  export TABA_TOKEN="tb_dev.contract-dev-secret-0123456789abcdef0123"
  export TABA_CONTRACT_NETWORK
}

contract_node_down() {
  local status="${1:-$?}"
  [ "$CONTRACT_OWNS_NODE" -eq 1 ] || return "$status"
  if [ "$status" -ne 0 ]; then
    echo >&2; echo "Failed (code $status). Last lines of the node:" >&2
    docker logs --tail 25 "$CONTRACT_NODE_NAME" >&2 2>&1 || true
  fi
  if [ "${TABA_CONTRACT_KEEP:-0}" = "1" ]; then
    contract_step "Node kept: docker rm -f $CONTRACT_NODE_NAME; docker network rm $TABA_CONTRACT_NETWORK"
  else
    docker rm -f "$CONTRACT_NODE_NAME" >/dev/null 2>&1 || true
    docker network rm "$TABA_CONTRACT_NETWORK" >/dev/null 2>&1 || true
  fi
  return "$status"
}

# A prefix that makes the topics of one run unique (names: 1-100 of A-Za-z0-9_-).
contract_run_id() { date +%m%d%H%M%S; }

# docker run on the node's network with the repository mounted read-only at /repo and the cwd at the connector.
#   contract_docker <image> <workdir inside /repo> [docker options...] -- <command...>
contract_docker() {
  local image="$1" workdir="$2"; shift 2
  local opts=()
  while [ "$1" != "--" ]; do opts+=("$1"); shift; done
  shift
  docker run --rm --network "$TABA_CONTRACT_NETWORK" \
    -v "$(contract_hostpath "$REPO_ROOT"):/repo" -w "/repo/$workdir" \
    ${opts[@]+"${opts[@]}"} "$image" "$@"
}

# The arguments every driver takes: --url --token --prefix --lang --scenario...
#   contract_driver_args <lang> <prefix> <scenario>...
contract_driver_args() {
  local lang="$1" prefix="$2"; shift 2
  printf '%s\n' --url "$TABA_URL" --token "$TABA_TOKEN" --prefix "$prefix" --lang "$lang"
  local s
  for s in "$@"; do printf '%s\n' --scenario "/repo/connectors/contract/scenarios/$s"; done
}

# The entry point of a connector's contract/run.sh, which defines `contract_run_driver <lang> <prefix> <scenario>...`
# (runs the driver in the container of the connector's base version). Phases: core (default, the common scenario),
# setup, publish and drain (the cross-language scenario, driven by connectors/contract/run.sh).
#   contract_main <lang> [phase]
contract_main() {
  local lang="$1" phase="${2:-core}"
  local id="${TABA_CONTRACT_ID:-$(contract_run_id)}"
  trap 'contract_node_down $?' EXIT
  contract_node_up
  case "$phase" in
    core) contract_run_driver "$lang" "ct-$lang-$id" core.scn ;;
    setup) contract_run_driver "$lang" "ct-x-$id" cross-setup.scn ;;
    publish) contract_run_driver "$lang" "ct-x-$id" cross-publish.scn ;;
    drain) contract_run_driver "$lang" "ct-x-$id" cross-drain.scn ;;
    *) contract_die "unknown phase: $phase (core, setup, publish, drain)" ;;
  esac
}
