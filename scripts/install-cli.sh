#!/bin/sh
# Installs the taba-cli (the Taba command line).
#
#   curl -fsSL https://raw.githubusercontent.com/rafaelpontani/taba/main/scripts/install-cli.sh | sh
#
# Linux (x86_64 and aarch64): downloads the static binary of the latest final GitHub Release and checks its SHA256.
# macOS and Windows (Git Bash / MINGW64, MSYS, Cygwin): the Releases have no binary for them, so it uses
#   1. `cargo install taba-cli --locked`, when cargo exists; otherwise
#   2. a `taba-cli` shell shim that runs the image rafaelpontani/taba-cli in Docker.
# On native Windows (PowerShell) use scripts/install-cli.ps1.
#
# Environment:
#   TABA_CLI_VERSION   version to install, for example 0.1.1 (default: the latest final version)
#   TABA_CLI_DIR       destination folder (default: $HOME/.local/bin)
#   TABA_CLI_DOCKER=1  forces the Docker shim even when cargo exists
set -eu

REPO=rafaelpontani/taba
DEST=${TABA_CLI_DIR:-$HOME/.local/bin}
version=${TABA_CLI_VERSION:-}
version=${version#v}

fail() { echo "install-cli: $*" >&2; exit 1; }

warn_path() {
  case ":$PATH:" in
    *":$1:"*) ;;
    *) echo "Note: $1 is not in your PATH; add: export PATH=\"$1:\$PATH\"" ;;
  esac
}

install_with_cargo() {
  echo "cargo install taba-cli --locked${version:+ --version $version}"
  # shellcheck disable=SC2086
  cargo install taba-cli --locked ${version:+--version $version}
  warn_path "$HOME/.cargo/bin"
  taba-cli --version 2>/dev/null || "$HOME/.cargo/bin/taba-cli" --version || true
}

install_docker_shim() {
  tag=${version:-latest}
  mkdir -p "$DEST"
  # Inside the container 127.0.0.1 is not the host; MSYS_NO_PATHCONV keeps Git Bash from rewriting arguments.
  cat > "$DEST/taba-cli" <<SHIM
#!/bin/sh
export MSYS_NO_PATHCONV=1
exec docker run --rm -i -e TABA_CLI_URL="\${TABA_CLI_URL:-http://host.docker.internal:8080}" -e TABA_CLI_TOKEN rafaelpontani/taba-cli:$tag "\$@"
SHIM
  chmod 755 "$DEST/taba-cli"
  echo "Installed: $DEST/taba-cli (runs rafaelpontani/taba-cli:$tag in Docker)"
  warn_path "$DEST"
}

os=$(uname -s)
case "$os" in
  Linux)
    case "$(uname -m)" in
      x86_64|amd64)  target=x86_64-unknown-linux-musl ;;
      aarch64|arm64) target=aarch64-unknown-linux-musl ;;
      *) target= ;;
    esac ;;
  *) target= ;;
esac

if [ -z "$target" ] || [ "${TABA_CLI_DOCKER:-}" = 1 ]; then
  echo "No Release binary for $os $(uname -m)."
  if command -v cargo >/dev/null 2>&1 && [ "${TABA_CLI_DOCKER:-}" != 1 ]; then
    install_with_cargo
  elif command -v docker >/dev/null 2>&1; then
    install_docker_shim
  else
    fail "install Rust (https://rustup.rs) and run: cargo install taba-cli --locked  (or install Docker)"
  fi
  exit 0
fi

if [ -z "$version" ]; then
  # /releases/latest redirects to the newest final Release (candidates are pre-releases and are skipped).
  url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
    || fail "could not find the latest version"
  version=${url##*/v}
fi

base="https://github.com/$REPO/releases/download/v$version"
file="taba-cli-$version-$target"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading taba-cli $version ($target)..."
curl -fsSL -o "$tmp/$file" "$base/$file" || fail "download failed: $base/$file"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || fail "download failed: $base/SHA256SUMS"

expected=$(grep " $file\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)
[ -n "$expected" ] || fail "$file is not in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$file" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/$file" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || fail "SHA256 does not match (expected $expected, got $actual)"

mkdir -p "$DEST"
install -m 755 "$tmp/$file" "$DEST/taba-cli"
echo "Installed: $DEST/taba-cli"
warn_path "$DEST"
"$DEST/taba-cli" --version || true
