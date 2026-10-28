#!/usr/bin/env bash
# The contract tests of all the connectors (connectors/contract/README.md): one Taba node in Docker, then every
# connector runs the same scenario through its own driver, in the container of its oldest supported runtime, and
# then the connectors talk to each other (what one publishes, the others read back, byte for byte).
#
#   connectors/contract/run.sh                      # all: csharp java node python rust
#   connectors/contract/run.sh node python          # only these
#   connectors/contract/run.sh --keep               # leaves the node running at the end
#   connectors/contract/run.sh --rebuild            # rebuilds the node image (taba:local) first
#   TABA_IMAGE=rafaelpontani/taba:latest connectors/contract/run.sh   # against a published image
#
# Needs Docker. Each connector can also run alone: connectors/<language>/contract/run.sh.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

LANGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) export TABA_CONTRACT_KEEP=1 ;;
    --rebuild) docker rmi -f "${TABA_IMAGE:-taba:local}" >/dev/null 2>&1 || true ;;
    -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    csharp|java|node|python|rust) LANGS+=("$1") ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done
[ ${#LANGS[@]} -gt 0 ] || LANGS=(csharp java node python rust)

trap 'contract_node_down $?' EXIT
contract_node_up
export TABA_CONTRACT_ID="$(contract_run_id)"

FAILED=()
run() { # run <lang> <phase>
  local lang="$1" phase="$2"
  contract_step "$lang: $phase"
  "$CONNECTORS_DIR/$lang/contract/run.sh" "$phase" || FAILED+=("$lang/$phase")
}

for lang in "${LANGS[@]}"; do run "$lang" core; done

if [ ${#LANGS[@]} -eq 5 ]; then
  # The cross-language scenario expects what all five connectors published, so it only runs with all of them.
  run "${LANGS[0]}" setup
  for lang in "${LANGS[@]}"; do run "$lang" publish; done
  for lang in "${LANGS[@]}"; do run "$lang" drain; done
fi

contract_step "Result"
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "FAILED: ${FAILED[*]}" >&2
  exit 1
fi
echo "all the connectors passed the contract tests"
