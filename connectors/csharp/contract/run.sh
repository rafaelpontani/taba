#!/usr/bin/env bash
# The C# connector against a real Taba (connectors/contract/README.md), on .NET 8 (its net8.0 target).
#   connectors/csharp/contract/run.sh      # starts its own Taba node in Docker, runs the common scenario, removes it
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"

# It builds a clean copy of the connector inside the container (the bin and obj folders of the host are not
# carried over and nothing is written to the repository); the NuGet cache stays in a Docker volume.
contract_run_driver() {
  local lang="$1" prefix="$2"; shift 2
  local args
  mapfile -t args < <(contract_driver_args "$lang" "$prefix" "$@")
  contract_docker mcr.microsoft.com/dotnet/sdk:8.0 connectors/csharp -v taba-contract-nuget:/root/.nuget \
    -e DOTNET_CLI_TELEMETRY_OPTOUT=1 -e DOTNET_NOLOGO=1 -- sh -c '
      cp -r /repo/connectors/csharp /tmp/csharp &&
      find /tmp/csharp -type d \( -name bin -o -name obj \) -prune -exec rm -rf {} + &&
      cd /tmp/csharp/contract &&
      dotnet run -c Release -- "$@"' sh "${args[@]}"
}

contract_main csharp "$@"
