#!/usr/bin/env bash
# Control of the root Compose Taba cluster, to reuse in the integration tests.
#
# Two ways to use it:
#
#   1. As a library, in a test script (a connector's or the general one):
#
#        source "$(dirname "${BASH_SOURCE[0]}")/../../docker/cluster.sh"   # adjust the path
#        taba_require_docker
#        trap taba_teardown EXIT        # tears the cluster down (or keeps it, with TABA_KEEP=1)
#        taba_up                        # starts it and waits for the three healthy nodes
#        read -r ID TOKEN < <(taba_create_token my-test --permission publish,consume --prefix my-)
#        taba_ensure_topic my-queue --type queue
#        taba_kill taba-2               # abrupt crash
#        taba_wait_members 2 120        # the node was excluded by vote
#        taba_start taba-2
#        taba_wait_stable 240           # back to 3 members, no conditions besides node-frozen
#
#   2. As a command, to set up the scenario by hand or from another language:
#
#        docker/cluster.sh up [--no-build]
#        docker/cluster.sh down
#        docker/cluster.sh kill|stop|start|restart <node>   # taba-0 (the seed), taba-1 or taba-2 (taba-N
#                                                           # is replica N of the `taba` service)
#        docker/cluster.sh wait-healthy|wait-stable [seconds]
#        docker/cluster.sh wait-members <n> [seconds]
#        docker/cluster.sh status
#        docker/cluster.sh token <name> [options of 'taba-cli token create']   # prints "id secret"
#        docker/cluster.sh topic <name> [options of 'taba-cli topic create']   # creates if missing
#        docker/cluster.sh cli <taba-cli arguments>
#
# Variables: TABA_NODES (default "taba-0 taba-1 taba-2"), TABA_KEEP=1 (taba_teardown keeps the
# cluster), TABA_BUILD=0 (taba_up reuses the images), TABA_CLI_VERBOSE=1 (shows the stderr of
# docker compose run).
#
# Needs Docker (with the compose plugin). The host's port 8080 (HAProxy) must be free.

TABA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TABA_NODES="${TABA_NODES:-taba-0 taba-1 taba-2}"
TABA_KEEP="${TABA_KEEP:-0}"
TABA_BUILD="${TABA_BUILD:-1}"

# Git Bash on Windows converts paths such as /secrets/root-token.
export MSYS_NO_PATHCONV=1

taba_step() { printf '\n== %s\n' "$*"; }
taba_ok() { printf '  ok: %s\n' "$*"; }
taba_die() { echo "FAILED: $*" >&2; exit 1; }

# Runs the root docker compose, from any directory. It uses the development override: these tests
# validate the image built from this repository, not the published one.
taba_compose() { (cd "$TABA_ROOT" && docker compose -f docker-compose.yml -f docker-compose.dev.yml "$@"); }

# A node's container. `taba-0` is the first node's service (the seed); `taba-N`, with N >= 1, is replica
# N of the `taba` service (Compose numbers replicas from 1). The names still hold for whoever uses
# `--scale taba=<n>`: with more replicas, `taba-3`, `taba-4`...
taba_container() {
  local node="$1" n id
  case "$node" in
    taba-0) taba_compose ps -aq taba-0 2>/dev/null ;;
    taba-[0-9]*)
      n="${node#taba-}"
      for id in $(taba_compose ps -aq taba 2>/dev/null); do
        if [ "$(docker inspect -f '{{index .Config.Labels "com.docker.compose.container-number"}}' "$id" 2>/dev/null)" = "$n" ]; then
          echo "$id"
          return
        fi
      done
      ;;
    *) taba_compose ps -aq "$node" 2>/dev/null ;;
  esac
}

taba_require_docker() {
  command -v docker >/dev/null || taba_die "docker not found"
  docker compose version >/dev/null 2>&1 || taba_die "the 'docker compose' plugin is not installed"
  docker info >/dev/null 2>&1 || taba_die "Docker is not running"
}

# The Compose taba-cli, already pointed at the cluster and with the root token from the volume.
taba_cli() {
  if [ "${TABA_CLI_VERBOSE:-0}" = "1" ]; then
    taba_compose run --rm -T taba-cli "$@" | tr -d '\r'
  else
    taba_compose run --rm -T taba-cli "$@" 2>/dev/null | tr -d '\r'
  fi
}

# --- lifecycle -------------------------------------------------------------------------------

# Starts the cluster from scratch (deleting whatever exists) and waits for the nodes to be healthy.
taba_up() {
  taba_step "Starting the cluster from scratch"
  taba_compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
  if [ "$TABA_BUILD" -eq 1 ]; then
    taba_compose --profile tools build
  fi
  # The root Compose defaults to four nodes (taba-0 plus three replicas); the tests assume the three of TABA_NODES.
  taba_compose up -d --scale taba=2
  taba_wait_healthy "${1:-240}"
}

taba_down() {
  taba_compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
}

# For use in `trap taba_teardown EXIT`: shows the logs if it failed and tears the cluster down (or keeps it).
taba_teardown() {
  local status="${1:-$?}"   # the exit code, if the caller has already lost sight of it
  if [ "$status" -ne 0 ]; then
    echo >&2
    echo "Failed (code $status). Last lines of the nodes:" >&2
    taba_compose logs --tail 25 taba-0 taba 2>/dev/null >&2 || true
  fi
  if [ "$TABA_KEEP" -eq 1 ]; then
    taba_step "Cluster kept (docker/cluster.sh down, or docker compose down -v, to delete it)"
  else
    taba_step "Tearing the cluster down and deleting the data"
    taba_down
  fi
  return "$status"
}

# --- failures --------------------------------------------------------------------------------

# `docker stop` honors Compose's stop_grace_period (it becomes the container's stop timeout).
taba_kill() { docker kill "$(taba_container "$1")" >/dev/null 2>&1; }    # abrupt crash
taba_stop() { docker stop "$(taba_container "$1")" >/dev/null 2>&1; }    # orderly exit (drain)
taba_start() { docker start "$(taba_container "$1")" >/dev/null 2>&1; }
taba_restart() { taba_stop "$1"; taba_start "$1"; }

# --- state -----------------------------------------------------------------------------------

taba_healthy_nodes() {
  local n=0 id state node
  for node in $TABA_NODES; do
    id="$(taba_container "$node")"
    [ -n "$id" ] || continue
    state="$(docker inspect -f '{{if .State.Running}}{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}{{else}}stopped{{end}}' "$id" 2>/dev/null || echo none)"
    [ "$state" = "healthy" ] && n=$((n + 1))
  done
  echo "$n"
}

taba_node_count() { set -- $TABA_NODES; echo "$#"; }

# How many active members the cluster sees (through the load balancer; if it is down, empty).
taba_active_members() {
  taba_cli cluster status -o json | grep -o '"active": *[0-9]*' | head -n 1 | grep -o '[0-9]*$' || true
}

# The cluster conditions, minus the `node-frozen` warning: the old ID of a node that crashed stays frozen
# for cluster.freezing.duration (5 min); the restarted node has a new ID and is not affected.
taba_other_conditions() {
  taba_cli cluster status -o json | grep -o '"condition": *"[^"]*"' | grep -v node-frozen || true
}

taba_status() { taba_cli cluster status -o table; }

# --- waits -----------------------------------------------------------------------------------

# Waits for a condition (a command that returns 0), with a time limit in seconds.
taba_wait_for() {
  local what="$1" limit="$2"; shift 2
  local deadline=$((SECONDS + limit))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || taba_die "$what: did not happen within ${limit} s"
    sleep 3
  done
  taba_ok "$what (${SECONDS}s in total)"
}

taba_all_healthy() { [ "$(taba_healthy_nodes)" -eq "$(taba_node_count)" ]; }
taba_wait_healthy() { taba_wait_for "all nodes become healthy" "${1:-240}" taba_all_healthy; }

_taba_has_members() { [ "$(taba_active_members)" = "$1" ]; }
taba_wait_members() { taba_wait_for "the cluster has $1 active members" "${2:-120}" _taba_has_members "$1"; }

_taba_stable() {
  [ "$(taba_active_members)" = "$(taba_node_count)" ] && [ -z "$(taba_other_conditions)" ] && taba_all_healthy
}
taba_wait_stable() {
  taba_wait_for "all members are back, with no conditions (only node-frozen is accepted)" "${1:-240}" _taba_stable
}

# --- data ------------------------------------------------------------------------------------

# Creates a token and prints "id secret" (the secret only appears in the creation response).
#   read -r ID TOKEN < <(taba_create_token my-test --permission publish,consume --prefix my-)
taba_create_token() {
  local name="$1"; shift
  local out id secret
  out="$(taba_cli token create --name "$name" "$@" -o json)"
  id="$(printf '%s' "$out" | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' | head -n 1)"
  secret="$(printf '%s' "$out" | sed -n 's/.*"token": *"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$id" ] && [ -n "$secret" ] || taba_die "could not read the created token: $out"
  echo "$id $secret"
}

taba_revoke_token() { taba_cli token revoke "$1" --yes >/dev/null 2>&1 || true; }

# Creates the topic if it does not exist yet.
taba_ensure_topic() {
  local name="$1"; shift
  if taba_cli topic get "$name" >/dev/null 2>&1; then
    echo "  $name already exists"
  else
    taba_cli topic create "$name" "$@" >/dev/null
    echo "  $name created"
  fi
}

# --- as a command ----------------------------------------------------------------------------

_taba_main() {
  local cmd="${1:-}"; shift || true
  taba_require_docker
  cd "$TABA_ROOT"
  case "$cmd" in
    up) [ "${1:-}" = "--no-build" ] && TABA_BUILD=0; taba_up ;;
    down) taba_down ;;
    kill|stop|start|restart) [ -n "${1:-}" ] || taba_die "usage: cluster.sh $cmd <node>"; "taba_$cmd" "$1" ;;
    wait-healthy) taba_wait_healthy "${1:-240}" ;;
    wait-stable) taba_wait_stable "${1:-240}" ;;
    wait-members) [ -n "${1:-}" ] || taba_die "usage: cluster.sh wait-members <n> [seconds]"; taba_wait_members "$1" "${2:-120}" ;;
    status) taba_status ;;
    token) taba_create_token "$@" ;;
    topic) taba_ensure_topic "$@" ;;
    cli) TABA_CLI_VERBOSE=1 taba_cli "$@" ;;
    ""|-h|--help) sed -n '2,35p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' ;;
    *) echo "unknown command: $cmd (see --help)" >&2; return 2 ;;
  esac
}

# Run directly (not with `source`): it becomes the command.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  _taba_main "$@"
fi
