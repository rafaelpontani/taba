#!/usr/bin/env bash
# The C# connector's integration tests against a real Taba cluster (doc 19, section 18.7).
#
# What it does:
#   1. starts the cluster from the root docker-compose.yml (nodes behind HAProxy) and waits for
#      the nodes to be healthy;
#   2. with the root token (which lives in the `secrets` volume), creates through taba-cli the test tokens
#      (one for the application and one read-only) and the queues the tests use;
#   3. runs `dotnet test` with TABA_URL and the tokens in the environment;
#   4. tears the cluster down and deletes the data (unless asked to keep it).
#
# Usage:
#   connectors/csharp/scripts/integration.sh                 # everything, from scratch
#   connectors/csharp/scripts/integration.sh --keep          # leaves the cluster up at the end
#   connectors/csharp/scripts/integration.sh --no-build      # reuses the images already built
#   connectors/csharp/scripts/integration.sh -- -f net8.0 --filter "Name~fifo"
#                                                            # what comes after -- goes to dotnet test
#
# Needs Docker (with the compose plugin) and the .NET 6 SDK or later. The host's port 8080
# must be free (it is HAProxy's).

# Starting the cluster, waiting and creating tokens/queues come from docker/cluster.sh, which the other
# connectors' tests can also use (including to take nodes down in the middle of a test).

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../docker/cluster.sh"
cd "$TABA_ROOT"

TEST_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) TABA_KEEP=1 ;;
    --no-build) TABA_BUILD=0 ;;
    --) shift; TEST_ARGS=("$@"); break ;;
    -h|--help) sed -n '2,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

URL="${TABA_URL:-http://localhost:8080}"
RUN_ID="$(date +%Y%m%d%H%M%S)"

command -v dotnet >/dev/null || taba_die "dotnet not found (needs the .NET 6 SDK or later)"
taba_require_docker

TOKEN_IDS=()

cleanup() {
  local status=$?
  if [ "$TABA_KEEP" -eq 1 ]; then
    taba_step "Revoking this run's test tokens"
    for id in "${TOKEN_IDS[@]:-}"; do
      [ -n "$id" ] && taba_revoke_token "$id"
    done
  fi
  taba_teardown "$status" || true
  exit "$status"
}
trap cleanup EXIT

# --- 1. the cluster -----------------------------------------------------------------------------

taba_up
taba_step "Checking the cluster through taba-cli"
taba_status

# --- 2. tokens and queues -----------------------------------------------------------------------

taba_step "Creating the test tokens"
read -r APP_ID APP_TOKEN < <(taba_create_token "csharp-tests-app-$RUN_ID" \
  --permission publish,consume,create,admin --prefix csharp-)
read -r RO_ID RO_TOKEN < <(taba_create_token "csharp-tests-readonly-$RUN_ID" \
  --permission consume --prefix csharp-)
TOKEN_IDS=("$APP_ID" "$RO_ID")
echo "  application token: $APP_ID (publish, consume, create, admin on csharp-*)"
echo "  read-only token:   $RO_ID (consume on csharp-*)"

taba_step "Creating the test queues"
taba_ensure_topic csharp-shared --type queue
taba_ensure_topic csharp-shared-fifo --type queue --fifo
taba_ensure_topic csharp-fan-a --type queue
taba_ensure_topic csharp-fan-b --type queue --fifo
taba_ensure_topic csharp-fan --type distributor --prefix csharp-fan-

# Creation only responds after the quorum, but the nodes sync the definition through messages: give it a
# moment so HAProxy can land on any of them.
sleep 2

# --- 3. the tests -------------------------------------------------------------------------------

taba_step "Running the C# connector tests (unit and integration) against $URL"
export TABA_URL="$URL"
export TABA_TOKEN="$APP_TOKEN"
export TABA_TOKEN_READONLY="$RO_TOKEN"
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1

dotnet test connectors/csharp/Taba.Client.Tests "${TEST_ARGS[@]+"${TEST_ARGS[@]}"}"

taba_step "Everything passed"
