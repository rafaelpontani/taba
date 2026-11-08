#!/usr/bin/env bash
# The Pulumi provider: the mapping tests, the check that the generated SDKs are up to date and the acceptance
# scenarios (examples/scenarios.sh) with the TypeScript and Python SDKs against one Taba node, all in Docker.
#   connectors/pulumi/scripts/test.sh                 # everything
#   connectors/pulumi/scripts/test.sh unit            # gofmt, vet and the mapping tests (no node)
#   connectors/pulumi/scripts/test.sh sdks            # generate.sh again: schema and SDKs must come out as committed
#   connectors/pulumi/scripts/test.sh acc             # plugin build, SDK compile (.NET, Go) and the scenarios
#   ACC_LANGS=python connectors/pulumi/scripts/test.sh acc   # one language of the scenarios
#   TABA_IMAGE=rafaelpontani/taba:0.3.8 ...           # the node image (default taba:local, built if missing)
#   TABA_CONTRACT_KEEP=1 ...                          # leaves the node running
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

ACC_LANGS="${ACC_LANGS:-typescript python}"
DOTNET_IMAGE="${DOTNET_IMAGE:-mcr.microsoft.com/dotnet/sdk:8.0}"
NODE_IMAGE="${NODE_IMAGE:-node:22-bookworm}"
PYTHON_IMAGE="${PYTHON_IMAGE:-python:3.12-bookworm}"
mode="${1:-all}"

case "$mode" in
  all | unit | sdks | acc) ;;
  *) contract_die "unknown mode: $mode (all, unit, sdks, acc)" ;;
esac

# Files created in a container belong to root: hand them back (the trap of generate.sh does the same).
CHOWN='chown -R $(stat -c %u:%g /repo) /repo/connectors/pulumi 2>/dev/null || true'

if [ "$mode" = all ] || [ "$mode" = unit ]; then
  contract_step "gofmt, vet and the mapping tests of the provider ($PULUMI_GO_IMAGE)"
  pulumi_go -- "trap '$CHOWN' EXIT; cd provider && test -z \"\$(gofmt -l .)\" || { gofmt -l .; exit 1; }
    go vet ./... && go test -race -count=1 ./...
    cd ../sdk && go vet ./... "
fi
[ "$mode" = unit ] && exit 0

# The schema and the four SDKs, regenerated, must be the committed ones (a tree hash, so it also works before the first commit).
tree_hash() {
  (cd "$(dirname "${BASH_SOURCE[0]}")/.." && find sdk provider/cmd/pulumi-resource-taba -type f | LC_ALL=C sort | xargs sha256sum | sha256sum)
}
if [ "$mode" = all ] || [ "$mode" = sdks ]; then
  contract_step "schema.json and the SDKs are up to date (generate.sh)"
  before="$(tree_hash)"
  "$(dirname "${BASH_SOURCE[0]}")/generate.sh" >/dev/null
  [ "$before" = "$(tree_hash)" ] || contract_die "schema.json or an SDK changed: the generated files were out of date (they are regenerated now; review and commit them)"
  echo "  up to date"
fi
[ "$mode" = sdks ] && exit 0

pulumi_tools_install

contract_step "Building the plugin pulumi-resource-taba"
pulumi_go -- "trap '$CHOWN' EXIT; mkdir -p /tools/acc && cd provider && go build -trimpath -o /tools/acc/pulumi-resource-taba ./cmd/pulumi-resource-taba"

contract_step "The .NET and Go SDKs compile"
docker run --rm -e DOTNET_CLI_TELEMETRY_OPTOUT=1 -e DOTNET_NOLOGO=1 -v "$(contract_hostpath "$REPO_ROOT"):/repo:ro" "$DOTNET_IMAGE" \
  sh -euc 'cp -r /repo/connectors/pulumi/sdk/dotnet /tmp/sdk && cd /tmp/sdk && echo 0.0.0-acc > version.txt && dotnet build -c Release -v q 2>&1 | tail -5'
pulumi_go -- "cd sdk && go build ./... && echo '  go sdk: ok'"

trap 'contract_node_down $?' EXIT
contract_node_up

for lang in $ACC_LANGS; do
  case "$lang" in
    typescript) image="$NODE_IMAGE" ;;
    python) image="$PYTHON_IMAGE" ;;
    *) contract_die "unknown language: $lang (typescript, python)" ;;
  esac
  contract_step "Scenarios with the $lang SDK against $TABA_URL"
  PULUMI_GO_IMAGE="$image" pulumi_go --network "$TABA_CONTRACT_NETWORK" -e TABA_URL="$TABA_URL" -e TABA_TOKEN="$TABA_TOKEN" -- \
    "bash /repo/connectors/pulumi/examples/scenarios.sh $lang"
done
