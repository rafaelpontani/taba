#!/usr/bin/env bash
# The release files of the Pulumi provider, built in containers from this checkout (nothing is published):
#   pulumi-resource-taba-v<version>-<os>-<arch>.tar.gz   the plugin, one per platform (-trimpath, no cgo)
#   rafaelpontani-pulumi-taba-<version>.tgz              the npm package (TypeScript/JavaScript SDK)
#   pulumi_taba-<python version>-py3-none-any.whl and pulumi_taba-<python version>.tar.gz   (PyPI)
#   Taba.Pulumi.<version>.nupkg                          the NuGet package (.NET SDK)
#   pulumi-taba-go-sdk-<version>.tar.gz                  the Go SDK module (published as a tag of the mirror)
#   pulumi-taba_<version>_SHA256SUMS
# The generated SDKs carry a placeholder version: it is replaced here, in a copy, so the tree stays as committed.
#   connectors/pulumi/scripts/package.sh <version> <output folder>        # 0.3.9, 0.3.9-rc.2, 0.0.0-local
#   PLATFORMS="linux/amd64 windows/amd64" connectors/pulumi/scripts/package.sh 0.0.0-local /tmp/pulumi   # a subset
#   SDKS="nodejs python" ...                                              # only some SDKs (default: nodejs python dotnet go)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

version="${1:?usage: package.sh <version> <output folder>}"
out="${2:?usage: package.sh <version> <output folder>}"
PLATFORMS="${PLATFORMS:-linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64 windows/arm64}"
SDKS="${SDKS:-nodejs python dotnet go}"
NODE_IMAGE="${NODE_IMAGE:-node:22-bookworm}"
PYTHON_IMAGE="${PYTHON_IMAGE:-python:3.12-bookworm}"
DOTNET_IMAGE="${DOTNET_IMAGE:-mcr.microsoft.com/dotnet/sdk:8.0}"
echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+|-local)?$' \
  || contract_die "version $version is not X.Y.Z, X.Y.Z-rc.N or X.Y.Z-local"
[ ! -e "$out" ] || contract_die "$out already exists"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
# PEP 440: 0.3.9-rc.2 is 0.3.9rc2 and 0.0.0-local is 0.0.0+local.
pyversion="$(echo "$version" | sed -E 's/-rc\./rc/; s/-local/+local/')"
uid="$(id -u)"
has() { case " $SDKS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
sdk_root="$(contract_hostpath "$REPO_ROOT")"

contract_step "Plugin $version for $PLATFORMS ($PULUMI_GO_IMAGE)"
pulumi_go -v "$(contract_hostpath "$out"):/out" -e VERSION="$version" -e PLATFORMS="$PLATFORMS" -e HOST_UID="$uid" -- '
  set -eu
  pkg=github.com/rafaelpontani/taba/connectors/pulumi/provider/pkg/version
  cd provider
  for platform in $PLATFORMS; do
    os="${platform%/*}"; arch="${platform#*/}"
    bin=pulumi-resource-taba; [ "$os" = windows ] && bin=pulumi-resource-taba.exe
    dir="$(mktemp -d)"
    CGO_ENABLED=0 GOOS="$os" GOARCH="$arch" go build -trimpath -buildvcs=false -ldflags "-s -w -X $pkg.Version=$VERSION" -o "$dir/$bin" ./cmd/pulumi-resource-taba
    # Fixed owner, order and date: the same commit gives the same archive.
    tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="2020-01-01 00:00:00Z" -C "$dir" -cf - "$bin" | gzip -n > "/out/pulumi-resource-taba-v$VERSION-$os-$arch.tar.gz"
    rm -rf "$dir"
  done
  chown "$HOST_UID" /out/* 2>/dev/null || true'

if has nodejs; then
  contract_step "npm package ($NODE_IMAGE)"
  docker run --rm -v "$sdk_root:/repo:ro" -v "$(contract_hostpath "$out"):/out" -e VERSION="$version" -e HOST_UID="$uid" "$NODE_IMAGE" bash -euc '
    cp -r /repo/connectors/pulumi/sdk/nodejs /tmp/sdk && cd /tmp/sdk
    sed -i "s/\${VERSION}/$VERSION/" package.json
    npm install --no-audit --no-fund --loglevel=error
    npm run build --silent
    # The README of the package is the one of connectors/pulumi with only the TypeScript example (the generator writes none).
    cp package.json bin/ && sh /repo/connectors/pulumi/scripts/readme.sh nodejs > bin/README.md && cd bin
    npm pack --silent --pack-destination /out >/dev/null
    chown "$HOST_UID" /out/* 2>/dev/null || true'
fi

if has python; then
  contract_step "Python wheel and sdist ($PYTHON_IMAGE)"
  docker run --rm -v "$sdk_root:/repo:ro" -v "$(contract_hostpath "$out"):/out" -e PYVERSION="$pyversion" -e HOST_UID="$uid" "$PYTHON_IMAGE" bash -euc '
    cp -r /repo/connectors/pulumi/sdk/python /tmp/sdk && cd /tmp/sdk
    # pyproject.toml names README.md at the root of the SDK: the one of connectors/pulumi, with only the Python example.
    sh /repo/connectors/pulumi/scripts/readme.sh python > README.md
    sed -i "s/^  version = .*/  version = \"$PYVERSION\"/" pyproject.toml
    pip install --quiet build
    python -m build --outdir /out >/dev/null
    chown "$HOST_UID" /out/* 2>/dev/null || true'
fi

if has dotnet; then
  contract_step "NuGet package ($DOTNET_IMAGE)"
  docker run --rm -e DOTNET_CLI_TELEMETRY_OPTOUT=1 -e DOTNET_NOLOGO=1 -v "$sdk_root:/repo:ro" -v "$(contract_hostpath "$out"):/out" \
    -e VERSION="$version" -e HOST_UID="$uid" "$DOTNET_IMAGE" sh -euc '
    cp -r /repo/connectors/pulumi/sdk/dotnet /tmp/sdk && cd /tmp/sdk
    rm -rf bin obj
    # The version is read from version.txt (an embedded resource) and used by the project as the package version.
    echo "$VERSION" > version.txt
    # The README of the package is the one of connectors/pulumi with only the C# example (the generator writes a
    # one-line file): nuget.org shows it on the page of the package.
    sh /repo/connectors/pulumi/scripts/readme.sh dotnet > README.md
    cat > Directory.Build.props <<"PROPS"
<Project>
  <PropertyGroup>
    <PackageReadmeFile>README.md</PackageReadmeFile>
  </PropertyGroup>
  <ItemGroup>
    <None Include="$(MSBuildThisFileDirectory)README.md" Pack="true" PackagePath="/" />
  </ItemGroup>
</Project>
PROPS
    # The generated project packs on build (GeneratePackageOnBuild); the .nupkg lands in bin/Release.
    dotnet build -c Release -v q -p:Version="$VERSION" 2>&1 | grep -E " error " || true
    cp bin/Release/*.nupkg /out/
    chown "$HOST_UID" /out/* 2>/dev/null || true'
fi

if has go; then
  contract_step "Go SDK module"
  tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="2020-01-01 00:00:00Z" -C "$REPO_ROOT/connectors/pulumi" \
    --exclude=bin --exclude=obj -cf - sdk/go sdk/go.mod sdk/go.sum | gzip -n > "$out/pulumi-taba-go-sdk-$version.tar.gz"
fi

contract_step "SHA256SUMS"
(cd "$out" && ls | grep -v SHA256SUMS | LC_ALL=C sort | xargs sha256sum > "pulumi-taba_${version}_SHA256SUMS")
ls -l "$out"
