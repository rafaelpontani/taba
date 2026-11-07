#!/usr/bin/env bash
# Checks the release files of the provider as a user would get them: the SHA256SUMS (and its GPG signature, when the
# .sig and the public key are given), then Terraform and OpenTofu install the provider from those files (a
# filesystem mirror, with the exact version) and apply a queue and a token against one node, plan with no difference
# and destroy.
#   connectors/terraform/scripts/verify-release.sh <folder with the files> <version>
#   TABA_IMAGE=rafaelpontani/taba:0.2.0 ...      # the node of the same version (default taba:local)
#   GPG_PUBLIC_KEY=key.asc ...                    # also check the signature of the SHA256SUMS (required if a .sig exists)
#   TF_CLIS=terraform ...                         # only one CLI
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/../../contract/lib.sh"
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

dir="${1:?usage: verify-release.sh <folder> <version>}"
version="${2:?usage: verify-release.sh <folder> <version>}"
TF_CLIS="${TF_CLIS:-terraform tofu}"
dir="$(cd "$dir" && pwd)"
sums="terraform-provider-taba_${version}_SHA256SUMS"

contract_step "Checksums of $sums"
[ -f "$dir/$sums" ] || contract_die "$sums not found in $dir"
(cd "$dir" && sha256sum -c "$sums")
if [ -f "$dir/$sums.sig" ]; then
  [ -n "${GPG_PUBLIC_KEY:-}" ] || contract_die "$sums.sig exists: set GPG_PUBLIC_KEY to the public key to check it"
  contract_step "Signature of $sums"
  key="$(cd "$(dirname "$GPG_PUBLIC_KEY")" && pwd)/$(basename "$GPG_PUBLIC_KEY")"
  docker run --rm -v "$(contract_hostpath "$dir"):/dist:ro" -v "$(contract_hostpath "$(dirname "$key")"):/key:ro" \
    -e SUMS="$sums" -e KEY="$(basename "$key")" "$GO_IMAGE" sh -euc '
      export GNUPGHOME="$(mktemp -d)"
      gpg --batch --quiet --import "/key/$KEY"
      gpg --batch --verify "/dist/$SUMS.sig" "/dist/$SUMS"'
fi

tf_tools_install
# A published image is pulled (contract_node_up would build a missing image from this checkout instead).
if [ -n "${TABA_IMAGE:-}" ] && ! docker image inspect "$TABA_IMAGE" >/dev/null 2>&1; then
  contract_step "Pulling $TABA_IMAGE"
  docker pull --quiet "$TABA_IMAGE"
fi
trap 'contract_node_down $?' EXIT
contract_node_up

work="$(mktemp -d)"
# The packed layout of a filesystem mirror, under the host each CLI uses for "rafaelpontani/taba".
for host in registry.terraform.io registry.opentofu.org; do
  mkdir -p "$work/mirror/$host/rafaelpontani/taba"
  cp "$dir"/terraform-provider-taba_"${version}"_*.zip "$work/mirror/$host/rafaelpontani/taba/"
done
cat >"$work/cli.tfrc" <<'EOF'
provider_installation {
  filesystem_mirror {
    path    = "/work/mirror"
    include = ["registry.terraform.io/rafaelpontani/taba", "registry.opentofu.org/rafaelpontani/taba"]
  }
}
EOF
id="$(contract_run_id)"
cat >"$work/main.tf" <<EOF
terraform {
  required_providers {
    taba = {
      source  = "rafaelpontani/taba"
      version = "$version"
    }
  }
}

resource "taba_queue" "q" {
  name = "verify-$id"
  ttl  = 600
}

resource "taba_token" "t" {
  name           = "verify-$id"
  permissions    = ["consume"]
  topic_prefixes = ["verify-$id"]
}
EOF

for cli in $TF_CLIS; do
  case "$cli" in
    terraform) path="/tools/terraform-$TERRAFORM_VERSION/terraform" ;;
    tofu) path="/tools/tofu-$TOFU_VERSION/tofu" ;;
    *) contract_die "unknown CLI: $cli (terraform, tofu)" ;;
  esac
  contract_step "$cli: install $version from the files, apply, plan, destroy"
  rm -rf "$work/.terraform" "$work/.terraform.lock.hcl" "$work"/terraform.tfstate*
  run() {
    docker run --rm --network "$TABA_CONTRACT_NETWORK" -v "$(contract_hostpath "$work"):/work" \
      -v taba-terraform-tools:/tools:ro -w /work -e TF_CLI_CONFIG_FILE=/work/cli.tfrc -e TF_IN_AUTOMATION=1 \
      -e CHECKPOINT_DISABLE=1 -e TABA_URL="$TABA_URL" -e TABA_TOKEN="$TABA_TOKEN" alpine:3.20 "$path" "$@"
  }
  run init -input=false -no-color >/dev/null
  run version -json | grep -q "\"registry.[a-z.]*/rafaelpontani/taba\": \"$version\"" \
    || contract_die "$cli did not install rafaelpontani/taba $version"
  run apply -input=false -no-color -auto-approve | grep -E '^Apply complete'
  run plan -input=false -no-color -detailed-exitcode >/dev/null || contract_die "$cli: the plan after the apply is not empty"
  echo "  plan after the apply: no changes"
  run destroy -input=false -no-color -auto-approve | grep -E '^Destroy complete'
done
rm -rf "$work"
