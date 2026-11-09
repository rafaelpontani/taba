#!/usr/bin/env bash
# Checks the release files of the Pulumi provider as a user would get them (package.sh): the SHA256SUMS, then, for
# TypeScript and Python, the plugin installed with `pulumi plugin install resource taba <version> --file`, the SDK
# installed from the .tgz / .whl, and a small program: up (queue, distributor, token), preview with no changes, destroy
# against one Taba node. The .NET and Go packages are only checked to exist in the SHA256SUMS.
#   connectors/pulumi/scripts/verify-release.sh <folder with the files> <version>
#   TABA_IMAGE=rafaelpontani/taba:0.3.8 ...     # the node (default taba:local)
#   VERIFY_LANGS=python ...                     # only one language (default: typescript python)
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

dir="${1:?usage: verify-release.sh <folder> <version>}"
version="${2:?usage: verify-release.sh <folder> <version>}"
VERIFY_LANGS="${VERIFY_LANGS:-typescript python}"
NODE_IMAGE="${NODE_IMAGE:-node:22-bookworm}"
PYTHON_IMAGE="${PYTHON_IMAGE:-python:3.12-bookworm}"
dir="$(cd "$dir" && pwd)"
sums="pulumi-taba_${version}_SHA256SUMS"
pyversion="$(echo "$version" | sed -E 's/-rc\./rc/; s/-local/+local/')"

contract_step "Checksums of $sums"
[ -f "$dir/$sums" ] || contract_die "$sums not found in $dir"
(cd "$dir" && sha256sum -c "$sums")
for f in "Taba.Pulumi.$version.nupkg" "pulumi-taba-go-sdk-$version.tar.gz"; do
  grep -q "[ *]$f$" "$dir/$sums" || contract_die "$f is not in $sums"
done

# What nuget.org shows (and cannot be changed after the push): the id, the version and the README of the package.
nuspec="$(unzip -p "$dir/Taba.Pulumi.$version.nupkg" Taba.Pulumi.nuspec)"
for want in "<id>Taba.Pulumi</id>" "<version>$version</version>" "<readme>README.md</readme>"; do
  case "$nuspec" in *"$want"*) ;; *) contract_die "the nuspec of Taba.Pulumi.$version.nupkg has no $want" ;; esac
done
files="$(unzip -Z1 "$dir/Taba.Pulumi.$version.nupkg")"
case "$files" in *README.md*) ;; *) contract_die "README.md is not in Taba.Pulumi.$version.nupkg" ;; esac

pulumi_tools_install
if [ -n "${TABA_IMAGE:-}" ] && ! docker image inspect "$TABA_IMAGE" >/dev/null 2>&1; then
  contract_step "Pulling $TABA_IMAGE"
  docker pull --quiet "$TABA_IMAGE"
fi
trap 'contract_node_down $?' EXIT
contract_node_up

# Runs INSIDE the container of the language: the files are at /dist (read only), nothing from the repository is used.
inner='
set -euo pipefail
lang="$1"
export PULUMI_CONFIG_PASSPHRASE=verify PULUMI_BACKEND_URL=file:///work/state CI=true PULUMI_HOME=/work/home
rm -rf /work && mkdir -p /work/state /work/prog /work/home && cd /work/prog
case "$(uname -m)" in x86_64) arch=amd64 ;; aarch64) arch=arm64 ;; *) echo "unsupported arch" >&2; exit 1 ;; esac
q() { "$@" >/work/last 2>&1 || { cat /work/last >&2; echo "FAILED: $*" >&2; exit 1; }; }
echo "-- plugin $VERSION from the file"
q pulumi plugin install resource taba "$VERSION" --file "/dist/pulumi-resource-taba-v$VERSION-linux-$arch.tar.gz"
installed="$(pulumi plugin ls)"
case "$installed" in *taba*"$VERSION"*) ;; *) echo "$installed"; echo "FAILED: plugin not installed" >&2; exit 1 ;; esac
case "$lang" in
  typescript)
    printf "name: verify\nruntime: nodejs\n" > Pulumi.yaml
    printf "{\"name\":\"verify\",\"main\":\"index.ts\",\"dependencies\":{}}\n" > package.json
    q npm install --no-audit --no-fund "/dist/rafaelpontani-pulumi-taba-$VERSION.tgz" @pulumi/pulumi typescript@5 @types/node@20
    cat > index.ts <<EOT
import * as pulumi from "@pulumi/pulumi";
import * as taba from "@rafaelpontani/pulumi-taba";
const q = new taba.Queue("q", { name: "verify-$ID", ttl: 600 });
const t = new taba.Token("t", { name: "verify-$ID", permissions: ["consume"], topicPrefixes: ["verify-$ID"] });
export const queueName = q.name;
export const secret = t.secret;
EOT
    printf "{\"compilerOptions\":{\"target\":\"es2020\",\"module\":\"commonjs\",\"moduleResolution\":\"node\",\"strict\":true,\"esModuleInterop\":true,\"skipLibCheck\":true},\"files\":[\"index.ts\"]}\n" > tsconfig.json
    ;;
  python)
    printf "name: verify\nruntime:\n  name: python\n  options:\n    virtualenv: venv\n" > Pulumi.yaml
    python -m venv venv
    q venv/bin/pip install --quiet "/dist/pulumi_taba-$PYVERSION-py3-none-any.whl"
    cat > __main__.py <<EOT
import pulumi
import pulumi_taba as taba
q = taba.Queue("q", name="verify-$ID", ttl=600)
t = taba.Token("t", name="verify-$ID", permissions=["consume"], topic_prefixes=["verify-$ID"])
pulumi.export("queueName", q.name)
pulumi.export("secret", t.secret)
EOT
    ;;
esac
q pulumi stack init verify
echo "-- up"
q pulumi up --yes --skip-preview --non-interactive
grep -E "^Resources:|\+ [0-9]+ created" /work/last || true
[ "$(pulumi stack output queueName)" = "verify-$ID" ] || { echo "FAILED: queueName" >&2; exit 1; }
[ "$(pulumi stack output secret)" = "[secret]" ] || { echo "FAILED: the secret is not a secret" >&2; exit 1; }
echo "-- preview without changes"
q pulumi preview --expect-no-changes --non-interactive
echo "-- destroy"
q pulumi destroy --yes --skip-preview --non-interactive
'
id="$(contract_run_id)"
for lang in $VERIFY_LANGS; do
  case "$lang" in
    typescript) image="$NODE_IMAGE" ;;
    python) image="$PYTHON_IMAGE" ;;
    *) contract_die "unknown language: $lang (typescript, python)" ;;
  esac
  contract_step "$lang: plugin and SDK $version installed from the files"
  PULUMI_GO_IMAGE="$image" pulumi_go --network "$TABA_CONTRACT_NETWORK" -v "$(contract_hostpath "$dir"):/dist:ro" \
    -e TABA_URL="$TABA_URL" -e TABA_TOKEN="$TABA_TOKEN" -e VERSION="$version" -e PYVERSION="$pyversion" -e ID="$id$lang" -e INNER="$inner" -- \
    "printf '%s' \"\$INNER\" > /tmp/verify.sh; bash /tmp/verify.sh $lang"
done
echo "verified $version"
