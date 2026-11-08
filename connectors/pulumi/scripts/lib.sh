#!/usr/bin/env bash
# Shared pieces of the Pulumi provider scripts (generate.sh, test.sh, ...): the Go container and the Pulumi CLI.
# For `source`, after connectors/contract/lib.sh (REPO_ROOT, contract_hostpath, contract_step); it does nothing by itself.
#   PULUMI_GO_IMAGE (golang:1.26), PULUMI_VERSION (3.274.0), PULUMI_CONVERTER_VERSION (1.4.0).
# The Pulumi CLI is downloaded once, checked against its published checksums, into the Docker volume taba-pulumi-tools
# (together with PULUMI_HOME and the Terraform converter that tfgen needs to turn the HCL examples of the Terraform
# provider into the other languages); the Go modules and the build cache are kept in taba-pulumi-gomod/-gocache.

PULUMI_GO_IMAGE="${PULUMI_GO_IMAGE:-golang:1.26}"
PULUMI_VERSION="${PULUMI_VERSION:-3.274.0}"
PULUMI_CONVERTER_VERSION="${PULUMI_CONVERTER_VERSION:-1.4.0}"

# Runs a shell command in the Go container with the repository at /repo and the cwd at the provider.
#   pulumi_go [docker options...] -- <shell command>
pulumi_go() {
  local opts=()
  while [ "$1" != "--" ]; do opts+=("$1"); shift; done
  shift
  docker run --rm -e GOTOOLCHAIN=local -e GOFLAGS=-mod=readonly \
    -e PULUMI_HOME=/tools/pulumi-home -e PULUMI_SKIP_UPDATE_CHECK=true -e PULUMI_AUTOMATION_API_SKIP_VERSION_CHECK=true \
    -e PATH="/tools/pulumi-$PULUMI_VERSION:/usr/local/go/bin:/go/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    -v taba-pulumi-gomod:/go/pkg/mod -v taba-pulumi-gocache:/root/.cache/go-build -v taba-pulumi-tools:/tools \
    -v "$(contract_hostpath "$REPO_ROOT"):/repo" -w /repo/connectors/pulumi \
    ${opts[@]+"${opts[@]}"} "$PULUMI_GO_IMAGE" sh -c "$1"
}

# The Pulumi CLI and the Terraform converter, in the tools volume (idempotent).
pulumi_tools_install() {
  contract_step "Pulumi CLI $PULUMI_VERSION and the Terraform converter $PULUMI_CONVERTER_VERSION"
  docker run --rm -v taba-pulumi-tools:/tools -e VERSION="$PULUMI_VERSION" alpine:3.20 sh -euc '
    dir="/tools/pulumi-$VERSION"
    [ -x "$dir/pulumi" ] && exit 0
    case "$(uname -m)" in x86_64) arch=x64 ;; aarch64) arch=arm64 ;; *) echo "unsupported arch" >&2; exit 1 ;; esac
    base="https://github.com/pulumi/pulumi/releases/download/v$VERSION"
    tar="pulumi-v$VERSION-linux-$arch.tar.gz"
    tmp="$(mktemp -d)"; cd "$tmp"
    wget -q "$base/$tar" "$base/pulumi-$VERSION-checksums.txt"
    grep " $tar\$" "pulumi-$VERSION-checksums.txt" | sha256sum -c -
    tar -xzf "$tar" && mkdir -p "$dir" && cp pulumi/* "$dir"/'
  pulumi_go -- "pulumi plugin ls 2>/dev/null | grep -q 'terraform.*$PULUMI_CONVERTER_VERSION' || pulumi plugin install converter terraform $PULUMI_CONVERTER_VERSION >/dev/null"
}
