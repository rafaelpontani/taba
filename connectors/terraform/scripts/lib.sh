#!/usr/bin/env bash
# Shared pieces of the provider scripts (test.sh, cluster.sh): the Go container and the Terraform and OpenTofu CLIs.
# For `source`, after connectors/contract/lib.sh (REPO_ROOT, contract_hostpath); it does nothing by itself.
#   GO_IMAGE (golang:1.25), TERRAFORM_VERSION (1.10.5), TOFU_VERSION (1.9.1), GOLANGCI_LINT_IMAGE.
# The CLIs are downloaded once, checked against their SHA256SUMS, into the Docker volume taba-terraform-tools; the Go
# modules are kept in taba-terraform-gomod.

GO_IMAGE="${GO_IMAGE:-golang:1.25}"
TERRAFORM_VERSION="${TERRAFORM_VERSION:-1.10.5}"
TOFU_VERSION="${TOFU_VERSION:-1.9.1}"
GOLANGCI_LINT_IMAGE="${GOLANGCI_LINT_IMAGE:-golangci/golangci-lint:v2.5.0}"

# Runs a shell command in the Go container with the repository at /repo and the cwd at the provider.
#   tf_go [docker options...] -- <shell command>
tf_go() {
  local opts=()
  while [ "$1" != "--" ]; do opts+=("$1"); shift; done
  shift
  docker run --rm -e GOTOOLCHAIN=local -e GOFLAGS=-mod=readonly \
    -v taba-terraform-gomod:/go/pkg/mod -v taba-terraform-tools:/tools:ro \
    -v "$(contract_hostpath "$REPO_ROOT"):/repo" -w /repo/connectors/terraform \
    ${opts[@]+"${opts[@]}"} "$GO_IMAGE" sh -c "$1"
}

# Downloads a CLI into the tools volume: tf_tool <name> <version> <zip url base> <zip name with ARCH> <sums name>.
tf_tool() {
  local name="$1" version="$2" base="$3" zip="$4" sums="$5"
  docker run --rm -v taba-terraform-tools:/tools -e NAME="$name" -e VERSION="$version" -e BASE="$base" \
    -e ZIP="$zip" -e SUMS="$sums" alpine:3.20 sh -euc '
      dir="/tools/$NAME-$VERSION"
      [ -x "$dir/$NAME" ] && exit 0
      case "$(uname -m)" in x86_64) arch=amd64 ;; aarch64) arch=arm64 ;; *) echo "unsupported arch" >&2; exit 1 ;; esac
      zip="$(echo "$ZIP" | sed "s/ARCH/$arch/")"
      tmp="$(mktemp -d)"; cd "$tmp"
      wget -q "$BASE/$zip" "$BASE/$SUMS"
      grep " $zip\$" "$SUMS" | sha256sum -c -
      mkdir -p "$dir" && unzip -q -o "$zip" "$NAME" -d "$dir" && chmod +x "$dir/$NAME"'
}

# Both CLIs in the tools volume: /tools/terraform-<version>/terraform and /tools/tofu-<version>/tofu.
tf_tools_install() {
  contract_step "Terraform $TERRAFORM_VERSION and OpenTofu $TOFU_VERSION"
  tf_tool terraform "$TERRAFORM_VERSION" "https://releases.hashicorp.com/terraform/$TERRAFORM_VERSION" \
    "terraform_${TERRAFORM_VERSION}_linux_ARCH.zip" "terraform_${TERRAFORM_VERSION}_SHA256SUMS"
  tf_tool tofu "$TOFU_VERSION" "https://github.com/opentofu/opentofu/releases/download/v$TOFU_VERSION" \
    "tofu_${TOFU_VERSION}_linux_ARCH.zip" "tofu_${TOFU_VERSION}_SHA256SUMS"
}
