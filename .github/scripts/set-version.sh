#!/usr/bin/env bash
# Aligns the version of every component with the release tag (committing nothing: it only applies to the build).
#
#   .github/scripts/set-version.sh 0.1.0-rc.2
#
# Adjusts the server Cargo.toml and Cargo.lock, the Rust connectors' Cargo.toml and Cargo.lock
# and the <Version> of the C# connector packages.

set -euo pipefail

version="${1:?usage: set-version.sh X.Y.Z[-rc.N]}"
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
  echo "invalid version: $version (use X.Y.Z or X.Y.Z-rc.N)" >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Replaces only the first `version = "..."` line of the file (the one in [package] or [workspace.package]).
set_toml_version() {
  sed -i "0,/^version = \".*\"/s//version = \"$version\"/" "$1"
}

# In Cargo.lock, the `version` line that comes right after `name = "<package>"`.
set_lock_version() {
  local file="$1"; shift
  local name tmp
  for name in "$@"; do
    tmp="$(mktemp)"
    awk -v name="$name" -v version="$version" '
      found && /^version = / { print "version = \"" version "\""; found = 0; next }
      { found = ($0 == "name = \"" name "\""); print }
    ' "$file" > "$tmp"
    cat "$tmp" > "$file"
    rm -f "$tmp"
  done
}

set_toml_version Cargo.toml
set_lock_version Cargo.lock taba-node

set_toml_version connectors/rust/client/Cargo.toml
set_toml_version connectors/rust/cli/Cargo.toml
set_lock_version connectors/rust/Cargo.lock taba-client taba-cli

for csproj in connectors/csharp/Taba.Client*/*.csproj; do
  if grep -q '<Version>' "$csproj"; then
    sed -i "s|<Version>[^<]*</Version>|<Version>$version</Version>|" "$csproj"
  fi
done

echo "version $version applied"
