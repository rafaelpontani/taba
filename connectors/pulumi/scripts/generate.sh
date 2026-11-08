#!/usr/bin/env bash
# Generates the schema and the SDKs of the Pulumi provider from the Terraform provider (pulumi-tfgen-taba), in a
# container, so the output is the same on Windows and on Linux.
#   connectors/pulumi/scripts/generate.sh
# Writes provider/cmd/pulumi-resource-taba/{schema.json,schema-embed.json,bridge-metadata.json} and
# sdk/{nodejs,python,dotnet,go}. The generated files are versioned; scripts/test.sh sdks runs this again and fails if
# anything changed.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

pulumi_tools_install
contract_step "schema and SDKs ($PULUMI_GO_IMAGE)"
# The sdk folder is emptied first (a removed resource must not leave its file behind). Root owns what the container
# writes, so the work is done inside it and the files go back to the owner of the repository at the end (also on
# failure).
pulumi_go -e PULUMI_VERSION="$PULUMI_VERSION" -- '
  set -eu
  trap "chown -R \$(stat -c %u:%g /repo) /repo/connectors/pulumi" EXIT
  cmd=provider/cmd/pulumi-resource-taba
  tfgen=/tmp/pulumi-tfgen-taba
  (cd provider && go build -o "$tfgen" ./cmd/pulumi-tfgen-taba)
  rm -rf sdk && mkdir -p sdk
  # The docs of the resources are read from ../terraform/docs, relative to the provider folder.
  (cd provider && "$tfgen" schema --out cmd/pulumi-resource-taba >/dev/null)
  rm -f "$cmd/Pulumi.yaml"
  cp "$cmd/schema.json" "$cmd/schema-embed.json"
  for lang in nodejs python dotnet go; do
    "$tfgen" "$lang" --out "sdk/$lang" >/dev/null
  done
  # The Go SDK is its own module (tag sdk/vX.Y.Z of the mirror), with the Pulumi SDK pinned to the CLI version.
  cd sdk
  go mod init github.com/rafaelpontani/pulumi-taba/sdk >/dev/null 2>&1
  go get "github.com/pulumi/pulumi/sdk/v3@v$PULUMI_VERSION" >/dev/null 2>&1
  GOFLAGS="-mod=mod -buildvcs=false" go mod tidy >/dev/null 2>&1
  cd ..; rm -rf sdk/dotnet/bin sdk/dotnet/obj'
echo "  generated"
