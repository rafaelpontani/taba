#!/usr/bin/env bash
# The provider against the three-node Compose cluster (docker/cluster.sh) while a node is down: a node killed just
# before an apply is still a member for a few seconds, and the calls that reach it (or whose definition it owns) get
# 503 until it is voted out. The apply must still finish, with every queue and token created exactly once (no token
# left orphaned by a 503 of POST /tokens), the next plan empty, and again empty after the node comes back.
#   connectors/terraform/scripts/cluster.sh          # builds the images, starts the cluster, removes it at the end
#   TABA_BUILD=0 ...                                 # reuses the images; TABA_KEEP=1 keeps the cluster
#   TF_CLI=tofu ...                                  # OpenTofu instead of Terraform
# About 5 minutes. Not run by CI (it needs the whole cluster); run it before a release that changes the provider.
set -euo pipefail
here="$(dirname "${BASH_SOURCE[0]}")"
source "$here/../../contract/lib.sh"
source "$here/lib.sh"
source "$here/../../../docker/cluster.sh"

TF_CLI="${TF_CLI:-terraform}"
BEFORE="${BEFORE:-10}"
DURING="${DURING:-40}"

taba_require_docker
tf_tools_install
case "$TF_CLI" in
  terraform) cli="/tools/terraform-$TERRAFORM_VERSION/terraform" ;;
  tofu) cli="/tools/tofu-$TOFU_VERSION/tofu" ;;
  *) taba_die "unknown TF_CLI: $TF_CLI (terraform, tofu)" ;;
esac

work="$(mktemp -d)"
mkdir -p "$work/plugins"
cleanup() {
  local status=$?
  taba_teardown "$status"
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

taba_step "Building the provider"
tf_go -v "$(contract_hostpath "$work/plugins"):/out" -e CGO_ENABLED=0 -- 'go build -o /out/terraform-provider-taba .'

cat >"$work/dev.tfrc" <<'EOF'
provider_installation {
  dev_overrides {
    "rafaelpontani/taba" = "/work/plugins"
  }
  direct {}
}
EOF

cat >"$work/main.tf" <<'EOF'
terraform {
  required_providers {
    taba = { source = "rafaelpontani/taba" }
  }
}

variable "n" { type = number }

resource "taba_queue" "q" {
  count = var.n
  name  = "tfc-q-${count.index}"
  ttl   = 600
}

resource "taba_token" "t" {
  count          = var.n
  name           = "tfc-t-${count.index}"
  permissions    = ["consume"]
  topic_prefixes = ["tfc-q-${count.index}"]
}

# Read after the resources: what the cluster has, not what the state says.
data "taba_topics" "queues" {
  name_contains = "tfc-q-"
  type          = "queue"
  depends_on    = [taba_queue.q]
}

data "taba_tokens" "tokens" {
  name_contains = "tfc-t-"
  depends_on    = [taba_token.t]
}

output "queues" { value = length(data.taba_topics.queues.names) }
output "tokens" { value = length(data.taba_tokens.tokens.ids) }
EOF

taba_up
root_token="$(taba_compose run --rm -T --entrypoint cat taba-cli /secrets/root-token 2>/dev/null | tr -d '\r\n')"
[ -n "$root_token" ] || taba_die "could not read the root token"
network="$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}' "$(taba_compose ps -q haproxy)")"

# The CLI in a container on the cluster network, with the work folder (configuration, state, provider) at /work.
tf() {
  docker run --rm --network "$network" -v "$(contract_hostpath "$work"):/work" -v taba-terraform-tools:/tools:ro \
    -w /work -e TF_CLI_CONFIG_FILE=/work/dev.tfrc -e TF_IN_AUTOMATION=1 -e CHECKPOINT_DISABLE=1 \
    -e TABA_URL=http://haproxy:8080 -e TABA_TOKEN="$root_token" alpine:3.20 "$cli" "$@"
}

# The plan of `-var n=<n>` must be empty (exit code 0 with -detailed-exitcode).
plan_is_empty() {
  local n="$1" status=0
  tf plan -input=false -no-color -detailed-exitcode -var "n=$n" >"$work/plan.log" 2>&1 || status=$?
  [ "$status" -eq 0 ] || { cat "$work/plan.log" >&2; taba_die "the plan with n=$n is not empty (exit $status)"; }
  taba_ok "plan with n=$n is empty"
}

# The outputs (read from the cluster) must be <n> queues and <n> tokens.
counts_are() {
  local n="$1" queues tokens
  queues="$(tf output -raw queues)"
  tokens="$(tf output -raw tokens)"
  [ "$queues" = "$n" ] && [ "$tokens" = "$n" ] || taba_die "expected $n queues and $n tokens in the cluster, got $queues and $tokens"
  taba_ok "$queues queues and $tokens tokens in the cluster"
}

apply() {
  local n="$1"
  local started=$SECONDS
  if ! tf apply -input=false -no-color -auto-approve -var "n=$n" >"$work/apply.log" 2>&1; then
    grep -A12 "^Error" "$work/apply.log" >&2 || tail -40 "$work/apply.log" >&2
    echo "  (after $((SECONDS - started)) s)" >&2
    taba_die "apply with n=$n failed"
  fi
  grep -E '^Apply complete' "$work/apply.log" | sed 's/^/  /'
}

taba_step "$TF_CLI: $BEFORE queues and tokens with the three nodes"
apply "$BEFORE"
counts_are "$BEFORE"

taba_step "$TF_CLI: up to $DURING queues and tokens right after taba-2 is killed"
taba_kill taba-2
started=$SECONDS
apply "$DURING"
echo "  apply took $((SECONDS - started)) s"
counts_are "$DURING"
plan_is_empty "$DURING"

taba_step "taba-2 back"
taba_start taba-2
taba_wait_stable 240
plan_is_empty "$DURING"

taba_step "$TF_CLI: removing everything"
apply 0
counts_are 0
tf destroy -input=false -no-color -auto-approve -var n=0 >/dev/null
taba_ok "destroyed"
