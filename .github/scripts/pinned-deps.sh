#!/usr/bin/env bash
# The OpenSSF Scorecard check Pinned-Dependencies on this checkout, the same that scorecard.dev runs on main: every
# image, action and package that a Dockerfile, a workflow or a shell script downloads must be pinned by hash (a digest,
# a commit SHA, `npm ci` with a lock, `pip install --require-hashes`). Fails when the check has any warning.
#   .github/scripts/pinned-deps.sh
# Runs on every pull request (job `pinned-dependencies` of ci.yml). Scans the tracked files and the new ones that git
# does not ignore (not target/ or node_modules/). Needs curl, tar and sha256sum; outside Linux, Docker as well (the
# binary of the Scorecard is only built for Linux here). How to fix each warning: doc/en-US/27-ci-cd.md#pinned-dependencies.
set -euo pipefail

SCORECARD_VERSION=5.6.0
# sha256 of the release files (scorecard_checksums.txt of the release).
SUM_AMD64=19bde07129450cb65a95c6e3b9f34e034341579e6f0e241546ec3ae6efb2dd9a
SUM_ARM64=3eacaceb3fdee6d0ba5df6ede45d1ada73321949029921bb63633802e8ec5951

root="$(git rev-parse --show-toplevel)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

native=1
[ "$(uname -s)" = Linux ] || native=0
if [ "$native" = 1 ]; then machine="$(uname -m)"; else machine="$(docker info --format '{{.Architecture}}')"; fi
case "$machine" in
  x86_64 | amd64) arch=amd64; sum="$SUM_AMD64" ;;
  aarch64 | arm64) arch=arm64; sum="$SUM_ARM64" ;;
  *) echo "pinned-deps: unsupported architecture $machine" >&2; exit 1 ;;
esac

file="scorecard_${SCORECARD_VERSION}_linux_${arch}.tar.gz"
curl -sSfL -o "$work/$file" "https://github.com/ossf/scorecard/releases/download/v${SCORECARD_VERSION}/$file"
echo "$sum  $work/$file" | sha256sum -c --quiet - || { echo "pinned-deps: checksum of $file does not match" >&2; exit 1; }
mkdir -p "$work/bin" && tar -xzf "$work/$file" -C "$work/bin" scorecard

# A copy of what git would commit: the Scorecard reads the folder as it is and would also count target/ and node_modules/.
mkdir -p "$work/repo"
(cd "$root" && git ls-files -z -c -o --exclude-standard | tar --null --ignore-failed-read -T - -cf - 2>/dev/null) | tar -xf - -C "$work/repo"

args=(--checks Pinned-Dependencies --show-details --format json)
if [ "$native" = 1 ]; then
  "$work/bin/scorecard" --local "$work/repo" "${args[@]}" >"$work/result.json"
else
  host() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
  MSYS_NO_PATHCONV=1 docker run --rm -v "$(host "$work/bin"):/sc:ro" -v "$(host "$work/repo"):/repo:ro" \
    alpine:3.22 /sc/scorecard --local /repo "${args[@]}" >"$work/result.json"
fi

score="$(grep -o '"score":[ ]*-\{0,1\}[0-9]*' "$work/result.json" | head -1 | grep -o -- '-\{0,1\}[0-9]*$')"
warnings="$(grep -oE '"Warn: ([^"\\]|\\.)*"' "$work/result.json" | sed 's/^"//; s/"$//' || true)"
echo "Pinned-Dependencies: $score/10 (Scorecard $SCORECARD_VERSION)"
if [ -n "$warnings" ]; then
  echo "$warnings" | while IFS= read -r line; do
    if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::error::$line"; else echo "  $line"; fi
  done
  echo "pinned-deps: pin these dependencies by hash before the pull request to release or main (doc/en-US/27-ci-cd.md#pinned-dependencies)" >&2
  exit 1
fi
echo "  no unpinned dependency"
