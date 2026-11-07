#!/usr/bin/env bash
# The release files of the provider (scripts/package.go): one zip per platform, the manifest and the SHA256SUMS, in the
# layout of the Terraform and OpenTofu registries. Built in the Go container; the signature is made by release.yml.
#   connectors/terraform/scripts/package.sh <version> <output folder>          # 0.2.0 dist/terraform
#   PLATFORMS=linux/amd64 connectors/terraform/scripts/package.sh 0.0.0-rc.1 /tmp/tf   # one platform (a quick check)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

version="${1:?usage: package.sh <version> <output folder>}"
out="${2:?usage: package.sh <version> <output folder>}"
[ ! -e "$out" ] || contract_die "$out already exists"
mkdir -p "$out"

contract_step "Provider $version for ${PLATFORMS:-every platform} ($GO_IMAGE)"
tf_go -v "$(contract_hostpath "$out"):/out" -e VERSION="$version" -e PLATFORMS="${PLATFORMS:-}" -e HOST_UID="$(id -u)" -- '
  go run scripts/package.go -version "$VERSION" -out /tmp/dist -platforms "$PLATFORMS" &&
  cp /tmp/dist/* /out/ && chown "$HOST_UID" /out/* 2>/dev/null; true'
ls "$out"
