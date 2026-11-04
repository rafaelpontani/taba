#!/usr/bin/env bash
# Runs the driver of one language in the container of its toolchain: run.sh <lang> <variant> <driver arguments...>
# (the variants of each language are in drivers/<lang>/variants). Needs from lib.sh: GW_NETWORK, GW_DOCKER_ENV, REPO_ROOT.
#
# The SDK versions are pinned here. A variant that is "env" is a version that reads AWS_ENDPOINT_URL_SQS and
# AWS_ENDPOINT_URL_SNS, so the application code does not change; a variant that is "code" is an older version, in which
# the endpoint is a parameter of the client (plan section 7, phase 1). Each driver prints its SDK version in the SUMMARY.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

lang="$1"; variant="$2"; shift 2
inside="/repo/connectors/gateway-contract/drivers/$lang/build-and-run.sh"

case "$lang/$variant" in
  python/current) image=python:3.12-slim; env=(-e PACKAGES="boto3==1.43.123") ;;
  python/old) image=python:3.12-slim; env=(-e PACKAGES="boto3==1.28.0 botocore==1.31.0") ;;   # Query protocol, before 1.28.82
  node/current) image=node:22-slim; env=(-e SDK_VERSION=3.1161.0) ;;
  node/old) image=node:22-slim; env=(-e SDK_VERSION=3.445.0) ;;                              # Query protocol, before 3.447.0
  go/current) image=golang:1.25
    env=(-e GET="github.com/aws/aws-sdk-go-v2/config github.com/aws/aws-sdk-go-v2/credentials github.com/aws/aws-sdk-go-v2/service/sqs github.com/aws/aws-sdk-go-v2/service/sns github.com/aws/smithy-go") ;;
  go/old) image=golang:1.25                                                                   # Query protocol, sqs before v1.28.0
    env=(-e TAGS="-tags oldsdk" -e GET="github.com/aws/aws-sdk-go-v2/config@v1.25.0 github.com/aws/aws-sdk-go-v2/credentials@v1.16.0 github.com/aws/aws-sdk-go-v2/service/sqs@v1.26.0 github.com/aws/aws-sdk-go-v2/service/sns@v1.25.0") ;;
  csharp/current) image=mcr.microsoft.com/dotnet/sdk:8.0; env=(-e SQS_VERSION=4.0.100.22 -e SNS_VERSION=4.0.100.22) ;;
  csharp/old) image=mcr.microsoft.com/dotnet/sdk:8.0; env=(-e SQS_VERSION=3.7.400.186 -e SNS_VERSION=3.7.400.185) ;;   # Query protocol, before 3.7.681.0
  java/v2) image=maven:3.9-eclipse-temurin-17; env=(-e FLAVOR=v2 -e SDK_VERSION=2.57.8) ;;
  java/v2old) image=maven:3.9-eclipse-temurin-17; env=(-e FLAVOR=v2 -e SDK_VERSION=2.20.162) ;;   # Query protocol, before 2.21.19
  java/v1) image=maven:3.9-eclipse-temurin-17; env=(-e FLAVOR=v1 -e SDK_VERSION=1.12.580) ;;      # Query protocol, before 1.12.585
  rust/current) image=rust:1; env=() ;;   # the newest aws-sdk-sqs/sns of the day (JSON protocol): its version goes in the SUMMARY
  *) gw_die "unknown driver: $lang/$variant" ;;
esac

volumes=()
case "$lang" in
  python) volumes=(-v gwpip:/root/.cache/pip) ;;
  node) volumes=(-v gwnpm:/root/.npm) ;;
  go) volumes=(-v gwgocache:/go/pkg/mod) ;;
  csharp) volumes=(-v gwnuget:/root/.nuget/packages -e DOTNET_CLI_TELEMETRY_OPTOUT=1 -e DOTNET_NOLOGO=1) ;;
  java) volumes=(-v gwm2:/root/.m2) ;;
  rust) volumes=(-v gwcargo:/usr/local/cargo/registry -v gwrusttarget:/cache) ;;
esac

gw_docker "$image" "${volumes[@]}" "${env[@]+"${env[@]}"}" -- sh "$inside" "$@"
