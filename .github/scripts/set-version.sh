#!/usr/bin/env bash
# Aligns the version of every component with the release tag (committing nothing: it only applies to the build).
#
#   .github/scripts/set-version.sh 0.1.0-rc.2
#
# Adjusts the server Cargo.toml and Cargo.lock, the Rust connectors' Cargo.toml and Cargo.lock,
# the <Version> of the C# connector packages, the version of the Node.js, Python, Ruby and Java connectors and of
# the OpenMessaging Benchmark driver.

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
# crates.io requires the version of the path dependency: taba-cli depends on the taba-client of the same tag.
sed -i "s|^taba-client = { path = \"../client\", version = \"[^\"]*\" }|taba-client = { path = \"../client\", version = \"=$version\" }|" \
  connectors/rust/cli/Cargo.toml

# The chart goes out with the exact version: `version` (the package) and `appVersion` (the default image tag).
sed -i -e "s|^version: .*|version: $version|" -e "s|^appVersion: .*|appVersion: \"$version\"|" \
  deploy/helm/taba/Chart.yaml

# The Node.js connector (taba-client on npm) goes out with the release version too, like the C# and Rust ones.
sed -i "0,/\"version\": \"[^\"]*\"/s//\"version\": \"$version\"/" connectors/node/package.json

# The Python connector (taba-client on PyPI) too, in the PEP 440 form: 0.3.9-rc.2 is 0.3.9rc2 (D230).
pyversion="$(echo "$version" | sed -E 's/-rc\./rc/')"
sed -i "s|^VERSION = \"[^\"]*\"|VERSION = \"$pyversion\"|" connectors/python/taba/_version.py

# The Ruby connector (taba-client on RubyGems) too, in the RubyGems form: a letter makes a pre-release, and
# 0.3.9-rc.2 is 0.3.9.rc2 (D232).
gemversion="$(echo "$version" | sed -E 's/-rc\./.rc/')"
sed -i "s|^  VERSION = '[^']*'.freeze|  VERSION = '$gemversion'.freeze|" connectors/ruby/lib/taba/version.rb

# The Java connector (io.github.rafaelpontani:taba-client on Maven Central) too, as it is: Maven sorts 0.3.9-rc.2 before
# 0.3.9 (D242). The <version> right after the project's <artifactId>, and the VERSION of the User-Agent.
sed -i "/^  <artifactId>taba-client<\/artifactId>/{n;s|<version>[^<]*</version>|<version>$version</version>|}" \
  connectors/java/pom.xml
sed -i "s|public static final String VERSION = \"[^\"]*\";|public static final String VERSION = \"$version\";|" \
  connectors/java/src/main/java/io/github/rafaelpontani/taba/TabaClient.java
# The OpenMessaging Benchmark driver (its -all jar goes to the Release, D243): its own version and the version of the
# Java connector it is built on, which must be the one above.
sed -i -e "/^  <artifactId>omb-driver-taba<\/artifactId>/{n;s|<version>[^<]*</version>|<version>$version</version>|}" \
  -e "s|<taba.version>[^<]*</taba.version>|<taba.version>$version</taba.version>|" connectors/omb/pom.xml

for csproj in connectors/csharp/Taba.Client*/*.csproj; do
  if grep -q '<Version>' "$csproj"; then
    sed -i "s|<Version>[^<]*</Version>|<Version>$version</Version>|" "$csproj"
  fi
done

echo "version $version applied"
