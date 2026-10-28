#!/usr/bin/env bash
# The Taba driver against a real node: starts one Taba node in Docker (connectors/contract/lib.sh), runs the unit and
# the integration tests of connectors/omb on Java 11 on the node's network, and removes the node.
#   connectors/omb/test.sh                  # needs connectors/omb/build.sh to have run once (the Maven cache)
#   TABA_CONTRACT_KEEP=1 connectors/omb/test.sh   # leaves the node running
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
source "$OMB_REPO_ROOT/connectors/contract/lib.sh"

trap 'contract_node_down $?' EXIT
contract_node_up
contract_step "Driver tests against $TABA_URL"
omb_maven "mvn -B -f /repo/connectors/omb/pom.xml test" \
  --network "$TABA_CONTRACT_NETWORK" -e TABA_URL="$TABA_URL" -e TABA_TOKEN="$TABA_TOKEN"
