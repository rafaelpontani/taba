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
SDK_FLOORS="github.com/klauspost/compress@v1.20.4 golang.org/x/crypto@v0.59.0 go.opentelemetry.io/otel/sdk@v1.47.0 \
go.opentelemetry.io/otel/sdk/log@v1.47.0 go.opentelemetry.io/otel/trace@v1.47.0"
pulumi_go -e PULUMI_VERSION="$PULUMI_VERSION" -e SDK_FLOORS="$SDK_FLOORS" -- '
  set -eu
  trap "chown -R \$(stat -c %u:%g /repo) /repo/connectors/pulumi" EXIT
  # The steps that download modules are quiet when they pass and print what they said when they fail (KI-038: one of
  # them failed in CI with no message at all).
  log=/tmp/pulumi-generate.log
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
  # npm Trusted Publishing only accepts a package whose repository is the one that publishes it (rafaelpontani/taba,
  # release.yml); the bridge writes the Repository of the provider (the mirror, which the Pulumi Registry reads).
  sed -i "s|\"repository\": \"https://github.com/rafaelpontani/pulumi-taba\"|\"repository\": {\"type\": \"git\", \"url\": \"git+https://github.com/rafaelpontani/taba.git\", \"directory\": \"connectors/pulumi/sdk/nodejs\"}|" \
    sdk/nodejs/package.json
  grep -q "git+https://github.com/rafaelpontani/taba.git" sdk/nodejs/package.json
  # The Go SDK is its own module (tag sdk/vX.Y.Z of the mirror), with the Pulumi SDK pinned to the CLI version.
  cd sdk
  # The go line comes from the provider module, not from the image: golang:1.26 moves with every patch release, and
  # go mod init would write its own version (the check of the SDKs then failed on every new Go patch).
  go mod init github.com/rafaelpontani/pulumi-taba/sdk >/dev/null 2>&1
  go mod edit -go="$(sed -n "s/^go //p" ../provider/go.mod | tr -d "\r")"
  go get "github.com/pulumi/pulumi/sdk/v3@v$PULUMI_VERSION" >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
  # Floors for the indirect modules of the Pulumi SDK that have known vulnerabilities in the versions it asks for
  # (OSV, Scorecard Vulnerabilities). Raise a floor when a new advisory appears; drop it once PULUMI_VERSION asks for
  # a fixed version by itself.
  go get $SDK_FLOORS >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
  GOFLAGS="-mod=mod -buildvcs=false" go mod tidy >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
  cd ..; rm -rf sdk/dotnet/bin sdk/dotnet/obj'
echo "  generated"
