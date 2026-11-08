#!/usr/bin/env bash
# The Terraform provider against a real node: gofmt, vet and the unit tests, then the acceptance tests (TF_ACC=1,
# internal/provider/acceptance_test.go) with Terraform and with OpenTofu, against one Taba node with authentication on
# (connectors/contract/lib.sh starts it from the taba:local image and removes it at the end).
#   connectors/terraform/scripts/test.sh                 # everything
#   connectors/terraform/scripts/test.sh unit            # gofmt, vet and unit tests only (no node)
#   connectors/terraform/scripts/test.sh docs            # docs/ regenerated (tfplugindocs) must match the committed one
#   connectors/terraform/scripts/test.sh lint            # golangci-lint (GOLANGCI_LINT_IMAGE)
#   TF_CLIS=terraform connectors/terraform/scripts/test.sh acc   # acceptance with Terraform only
#   GO_TEST_FLAGS=-v ...                                  # more flags of go test (acceptance)
#   TABA_CONTRACT_KEEP=1 ...                              # leaves the node running
# The CLIs and the Go modules are kept in Docker volumes (scripts/lib.sh).
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TF_CLIS="${TF_CLIS:-terraform tofu}"
mode="${1:-all}"

case "$mode" in
  all | unit | lint | docs | acc) ;;
  *) contract_die "unknown mode: $mode (all, unit, lint, docs, acc)" ;;
esac

if [ "$mode" = all ] || [ "$mode" = unit ]; then
  contract_step "gofmt, vet and unit tests ($GO_IMAGE)"
  tf_go -- 'test -z "$(gofmt -l .)" || { gofmt -l .; exit 1; }; go vet ./... && go test -race -count=1 ./...'
fi
[ "$mode" = unit ] && exit 0

# golangci-lint with its default linters (errcheck, govet, ineffassign, staticcheck, unused), with the module cache of
# tf_go. GOTOOLCHAIN=auto: the image may carry a Go older than the go line of go.mod.
if [ "$mode" = all ] || [ "$mode" = lint ]; then
  contract_step "golangci-lint ($GOLANGCI_LINT_IMAGE)"
  docker run --rm -e GOTOOLCHAIN=auto -e GOFLAGS=-mod=readonly -v taba-terraform-gomod:/go/pkg/mod \
    -v "$(contract_hostpath "$REPO_ROOT"):/repo" -w /repo/connectors/terraform "$GOLANGCI_LINT_IMAGE" golangci-lint run ./...
fi
[ "$mode" = lint ] && exit 0

tf_tools_install

# docs/ is what tfplugindocs generates from the schemas and examples/ (the registry shows it): regenerated here, it
# must come out the same as the committed one, like the published OpenAPI documents (tests/openapi.rs).
if [ "$mode" = all ] || [ "$mode" = docs ]; then
  contract_step "docs/ up to date (tfplugindocs, Terraform $TERRAFORM_VERSION)"
  tf_go -e PATH="/tools/terraform-$TERRAFORM_VERSION:/usr/local/go/bin:/go/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" -- '
    mkdir /tmp/c && cp -r /repo/connectors/go /repo/connectors/terraform /tmp/c/ && cd /tmp/c/terraform &&
    go generate ./... >/dev/null && diff -ru /repo/connectors/terraform/docs docs || { echo "docs/ is out of date: run go generate ./... (with terraform on the PATH)" >&2; exit 1; }'
  echo "  up to date"
fi
[ "$mode" = docs ] && exit 0

trap 'contract_node_down $?' EXIT
contract_node_up

for cli in $TF_CLIS; do
  # OpenTofu refuses the default provider address of the tests (namespace "-") outside its own registry, so it gets
  # the real one; Terraform keeps the default (with a namespace it looks the provider up in the registry on import).
  case "$cli" in
    terraform) path="/tools/terraform-$TERRAFORM_VERSION/terraform" address=() ;;
    tofu)
      path="/tools/tofu-$TOFU_VERSION/tofu"
      address=(-e TF_ACC_PROVIDER_HOST=registry.opentofu.org -e TF_ACC_PROVIDER_NAMESPACE=rafaelpontani)
      ;;
    *) contract_die "unknown CLI: $cli (terraform, tofu)" ;;
  esac
  contract_step "Acceptance tests with $cli against $TABA_URL"
  tf_go --network "$TABA_CONTRACT_NETWORK" -e TF_ACC=1 -e TF_ACC_TERRAFORM_PATH="$path" -e CHECKPOINT_DISABLE=1 \
    ${address[@]+"${address[@]}"} \
    -e TABA_URL="$TABA_URL" -e TABA_TOKEN="$TABA_TOKEN" -e GO_TEST_FLAGS="${GO_TEST_FLAGS:-}" \
    -- 'go test -count=1 -timeout 20m $GO_TEST_FLAGS -run "^TestAcc" ./internal/provider/'
done
