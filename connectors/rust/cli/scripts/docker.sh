#!/bin/sh
# Builds the taba-cli Docker image (doc/pt-BR/20-cli.md) with buildx.
#
#   docker.sh                       taba-cli image for this machine's platform (stays in the local Docker)
#   docker.sh --multi               linux/amd64 and linux/arm64 (needs --push or --output)
#   docker.sh --tag my/taba-cli:1   another name for the image
#
# The context is the repository root; the Dockerfile copies only the connectors/rust/ folder (the Cargo workspace
# of the connectors).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)   # the repository root: the build context

tag="taba-cli"
platform=""
extra=""
while [ $# -gt 0 ]; do
  case "$1" in
    --multi) platform="linux/amd64,linux/arm64" ;;
    --tag) shift; tag="${1:?--tag needs a name}" ;;
    --push) extra="$extra --push" ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "docker.sh: unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -n "$platform" ]; then
  case "$extra" in
    *--push*) ;;
    *) echo "docker.sh: --multi does not fit in the local Docker; use --push (or run buildx with --output)" >&2; exit 2 ;;
  esac
  # shellcheck disable=SC2086
  exec docker buildx build --platform "$platform" -f "$here/../Dockerfile" -t "$tag" $extra "$root"
fi
# shellcheck disable=SC2086
exec docker buildx build --load -f "$here/../Dockerfile" -t "$tag" $extra "$root"
