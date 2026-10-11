#!/usr/bin/env bash
# Alinha a versão de todos os componentes à da tag do release (sem commitar nada: vale só para o build).
#
#   .github/scripts/set-version.sh 0.1.0-rc.2
#
# Ajusta o Cargo.toml e o Cargo.lock do servidor, os Cargo.toml e o Cargo.lock dos conectores Rust
# e o <Version> dos pacotes do conector C#.

set -euo pipefail

version="${1:?uso: set-version.sh X.Y.Z[-rc.N]}"
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
  echo "versão inválida: $version (use X.Y.Z ou X.Y.Z-rc.N)" >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# Troca só a primeira linha `version = "..."` do arquivo (a do [package] ou do [workspace.package]).
set_toml_version() {
  sed -i "0,/^version = \".*\"/s//version = \"$version\"/" "$1"
}

# No Cargo.lock, a linha `version` que vem logo depois de `name = "<pacote>"`.
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

echo "versão $version aplicada"
