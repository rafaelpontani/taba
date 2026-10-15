#!/bin/sh
# Builds taba-cli (doc/pt-BR/20-cli.md) on Linux and macOS.
#
#   build.sh                  this machine's platform
#   build.sh --target <target>  one target from the table below
#   build.sh --all            every target this machine can build
#
# Targets: x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, x86_64-apple-darwin,
#          aarch64-apple-darwin (the Windows ones: build.ps1).
# The result goes to connectors/rust/cli/dist/<target>/, with the binary and a SHA256SUMS.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
workspace=$(cd "$here/../.." && pwd)   # connectors/rust/, the Cargo workspace of the connectors
dist="$here/../dist"
dist=$(mkdir -p "$dist" && cd "$dist" && pwd)

# Small, reproducible binary: Cargo.lock applies (--locked), no symbols, optimized for size.
export CARGO_PROFILE_RELEASE_OPT_LEVEL=z
export CARGO_PROFILE_RELEASE_LTO=true
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export CARGO_PROFILE_RELEASE_PANIC=abort
export CARGO_PROFILE_RELEASE_STRIP=symbols

host=$(rustc -vV | sed -n 's/^host: //p')

case "$host" in
  *-apple-darwin) reachable="x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl" ;;
  *-linux-*)      reachable="x86_64-unknown-linux-musl aarch64-unknown-linux-musl" ;;
  *) echo "build.sh: unsupported system ($host); on Windows use build.ps1" >&2; exit 2 ;;
esac

# The target of the same architecture and the same system as this machine: builds with no extra tool.
host_arch="${host%%-*}"
native() {
  case "$1" in
    "$host_arch"-unknown-linux-musl) case "$host" in *-linux-*) return 0 ;; esac ;;
    "$host_arch"-apple-darwin) case "$host" in *-apple-darwin) return 0 ;; esac ;;
  esac
  return 1
}

has() { command -v "$1" >/dev/null 2>&1; }

sha256() {
  if has sha256sum; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

build_one() {
  target="$1"
  echo "==> $target"
  rustup target add "$target" >/dev/null 2>&1 || true
  cd "$workspace"
  case "$target" in
    *-linux-musl)
      # Static, linked against musl. Outside the same system and architecture, it crosses with zigbuild or cross.
      if native "$target"; then
        RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" cargo build --release --locked -p taba-cli --target "$target"
      elif has cargo-zigbuild; then
        cargo zigbuild --release --locked -p taba-cli --target "$target"
      elif has cross; then
        cross build --release --locked -p taba-cli --target "$target"
      else
        echo "build.sh: for $target install cargo-zigbuild (cargo install cargo-zigbuild) or cross" >&2
        return 1
      fi
      ;;
    *-apple-darwin)
      cargo build --release --locked -p taba-cli --target "$target"
      ;;
  esac
  out="$dist/$target"
  mkdir -p "$out"
  cp "$workspace/target/$target/release/taba-cli" "$out/taba-cli"
  (cd "$out" && sha256 taba-cli > SHA256SUMS)
  echo "    $out/taba-cli"
}

mode=host
target=""
while [ $# -gt 0 ]; do
  case "$1" in
    --all) mode=all ;;
    --target) shift; target="${1:?--target needs a target}"; mode=one ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "build.sh: unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

case "$mode" in
  host)
    case "$host" in
      *-apple-darwin) build_one "$host_arch-apple-darwin" ;;
      *) build_one "$host_arch-unknown-linux-musl" ;;
    esac ;;
  one)
    ok=no; for t in $reachable; do [ "$t" = "$target" ] && ok=yes; done
    [ "$ok" = yes ] || { echo "build.sh: this machine ($host) does not build $target (it reaches: $reachable)" >&2; exit 2; }
    build_one "$target" ;;
  all)
    failed=0
    for t in $reachable; do build_one "$t" || failed=1; done
    exit $failed ;;
esac
