#!/usr/bin/env bash
# The OSV scanner on this checkout, the same scan that the OpenSSF Scorecard check Vulnerabilities runs on main: every
# lock file (Cargo.lock, go.mod, package-lock.json, requirements*.txt, pom.xml...) against the advisories of osv.dev,
# honouring the osv-scanner.toml next to each lock file (the advisories with no fix, D227). Fails on any open advisory.
#   .github/scripts/vulnerabilities.sh
# Runs on every pull request (job `vulnerabilities` of ci.yml) and every day on main (vulnerabilities.yml), because an
# advisory published after the merge lowers the score without any change to the repository. Scans the tracked files
# and the new ones that git does not ignore (not target/ or node_modules/). Needs curl and sha256sum; outside Linux,
# Docker as well. How to fix: doc/en-US/27-ci-cd.md#known-vulnerabilities.
set -euo pipefail

OSV_SCANNER_VERSION=2.7.0
# sha256 of the release files (osv-scanner_SHA256SUMS of the release).
SUM_AMD64=160c6253e2922e9e0a6a9771c5c4c40a4b3c6090b34a3cdd5e80c96cc2e4218b
SUM_ARM64=4061ef4c021ed3c47f203b818f337e351336cc5cd67c0274fc36bd1dfc728dd7

root="$(git rev-parse --show-toplevel)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

native=1
[ "$(uname -s)" = Linux ] || native=0
if [ "$native" = 1 ]; then machine="$(uname -m)"; else machine="$(docker info --format '{{.Architecture}}')"; fi
case "$machine" in
  x86_64 | amd64) arch=amd64; sum="$SUM_AMD64" ;;
  aarch64 | arm64) arch=arm64; sum="$SUM_ARM64" ;;
  *) echo "vulnerabilities: unsupported architecture $machine" >&2; exit 1 ;;
esac

file="osv-scanner_linux_${arch}"
mkdir -p "$work/bin"
curl -sSfL -o "$work/bin/osv-scanner" "https://github.com/google/osv-scanner/releases/download/v${OSV_SCANNER_VERSION}/$file"
echo "$sum  $work/bin/osv-scanner" | sha256sum -c --quiet - || { echo "vulnerabilities: checksum of $file does not match" >&2; exit 1; }
chmod +x "$work/bin/osv-scanner"

# A copy of what git would commit: the scanner reads the folder as it is and would also count target/ and node_modules/.
mkdir -p "$work/repo"
(cd "$root" && git ls-files -z -c -o --exclude-standard | tar --null --ignore-failed-read -T - -cf - 2>/dev/null) | tar -xf - -C "$work/repo"

# Exit codes of the scanner: 0 nothing found, 1 open advisories, anything else an error of the scan itself.
# --no-resolve: the pom.xml of the OMB driver depends on the Java connector of this repository, which is not in Maven
# Central under that version, and the transitive resolution of it fails the whole scan (exit 127). The lock files are
# read as they are; only the manifests lose their transitive dependencies.
args=(scan source -r --no-resolve --format table)
set +e
if [ "$native" = 1 ]; then
  "$work/bin/osv-scanner" "${args[@]}" "$work/repo"
else
  host() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
  MSYS_NO_PATHCONV=1 docker run --rm -v "$(host "$work/bin"):/osv:ro" -v "$(host "$work/repo"):/repo:ro" \
    alpine:3.22 /osv/osv-scanner "${args[@]}" /repo
fi
status=$?
set -e

case "$status" in
  0) echo "  no known vulnerability (OSV scanner $OSV_SCANNER_VERSION)" ;;
  1)
    msg="vulnerabilities: update the dependencies above to a fixed version, or, when the lock file cannot take one, list the advisory with its reason in the osv-scanner.toml next to it (doc/en-US/27-ci-cd.md#known-vulnerabilities)"
    if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::error::$msg"; else echo "$msg" >&2; fi
    exit 1 ;;
  *) echo "vulnerabilities: the OSV scanner failed (exit $status)" >&2; exit "$status" ;;
esac
