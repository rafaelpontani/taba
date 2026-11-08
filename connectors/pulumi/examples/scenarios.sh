#!/usr/bin/env bash
# The acceptance scenarios of the Pulumi provider, run INSIDE a container (scripts/test.sh acc) against a real node:
#   examples/scenarios.sh typescript|python
# It builds the generated SDK of the language, installs it into the example program and walks the life of the
# resources with the local file backend (no Pulumi account): up, no-op preview, in-place update, replacement refused
# and then forced, drift seen by refresh, createOrUpdate over an existing queue, rotation of the token with --replace,
# import, destroy. Every step that changes something is followed by `preview --expect-no-changes`.
# The node is reached with TABA_URL and TABA_TOKEN (the same variables the provider reads).
set -euo pipefail

lang="${1:?typescript or python}"
: "${TABA_URL:?}" "${TABA_TOKEN:?}"
export PATH="/tools/acc:$PATH"
export PULUMI_CONFIG_PASSPHRASE=acceptance PULUMI_BACKEND_URL=file:///work/state CI=true
prefix="acc-$lang"

step() { printf '\n-- %s\n' "$*"; }
fail() { echo "FAILED: $*" >&2; exit 1; }

# Runs a command quietly; on failure shows the output and stops. Output of the last command stays in /work/last.
quiet() { "$@" >/work/last 2>&1 || { cat /work/last >&2; fail "$*"; }; }
up() { quiet pulumi up --yes --skip-preview --non-interactive "$@"; }
noop() { quiet pulumi preview --expect-no-changes --non-interactive; echo "  preview without changes"; }

api() { # api METHOD PATH [JSON]
  local method="$1" path="$2" body="${3:-}"
  curl -sS -o /work/api.out -w '%{http_code}' -X "$method" -H "Authorization: Bearer $TABA_TOKEN" \
    ${body:+-H 'Content-Type: application/json' --data "$body"} "$TABA_URL$path"
}
expect_api() { # expect_api STATUS METHOD PATH [JSON]
  local want="$1"; shift
  local got; got="$(api "$@")"
  [ "$got" = "$want" ] || { cat /work/api.out >&2; fail "$* answered $got, wanted $want"; }
}
out() { pulumi stack output "$1" --non-interactive; }

rm -rf /work && mkdir -p /work/state /work/prog
cp -r "/repo/connectors/pulumi/examples/$lang/." /work/prog/

step "Building the $lang SDK and installing it into the program"
case "$lang" in
  typescript)
    cp -r /repo/connectors/pulumi/sdk/nodejs /work/sdk && cd /work/sdk
    sed -i 's/${VERSION}/0.0.0-acc/' package.json
    quiet npm install --no-audit --no-fund
    quiet npm run build
    cp package.json bin/
    cd /work/prog
    quiet npm install --no-audit --no-fund
    quiet npm install --no-audit --no-fund /work/sdk/bin typescript@5 @types/node@20
    ;;
  python)
    cp -r /repo/connectors/pulumi/sdk/python /work/sdk
    cd /work/prog
    python -m venv venv
    quiet venv/bin/pip install --quiet "pulumi>=3.269.0,<4.0.0" /work/sdk
    ;;
  *) fail "unknown language: $lang" ;;
esac

cd /work/prog
quiet pulumi stack init acc
pulumi config set prefix "$prefix" >/dev/null
orders="$prefix-orders"

step "up: queue, distributor, token and data sources"
up
noop
expect_api 200 GET "/topics/$orders"
expect_api 200 GET "/topics/$prefix-events"
[ "$(out queueName)" = "$orders" ] || fail "output queueName"
[ "$(out tokenSecret)" = "[secret]" ] || fail "the token must be a secret in the outputs, got: $(out tokenSecret)"
[ -n "$(pulumi stack output tokenSecret --show-secrets --non-interactive)" ] || fail "the token secret is empty"
pulumi stack output queueNames --non-interactive | grep -q "$orders" || fail "getTopics did not list $orders"

step "In-place update: maxDeliveries 5 -> 7"
pulumi config set maxDeliveries 7 >/dev/null
up
noop
expect_api 200 GET "/topics/$orders"
grep -q '"maxDeliveries":7' /work/api.out || fail "the node did not take maxDeliveries 7: $(cat /work/api.out)"

step "Immutable change on a queue with messages is refused, then forced with forceDestroy"
api POST "/topics/$orders/messages" '{"hello":"world"}' >/dev/null
pulumi config set volatile true >/dev/null
if pulumi up --yes --skip-preview --non-interactive >/work/last 2>&1; then cat /work/last >&2; fail "the replacement of a queue with messages must be refused"; fi
grep -q "still holds messages" /work/last || { cat /work/last >&2; fail "the refusal does not say the queue holds messages"; }
echo "  refused: the queue still holds messages"
pulumi config set volatile false >/dev/null
pulumi config set forceDestroy true >/dev/null
up
noop
echo "  forceDestroy applied first (it is read from the state, as the message says)"
pulumi config set volatile true >/dev/null
up
noop
expect_api 200 GET "/topics/$orders"
grep -q '"volatile":true' /work/api.out || fail "the queue was not replaced by a volatile one: $(cat /work/api.out)"

step "Drift made outside Pulumi is seen by refresh and corrected by up"
expect_api 200 PATCH "/topics/$orders" '{"maxDeliveries":9}'
quiet pulumi refresh --yes --non-interactive
if pulumi preview --expect-no-changes --non-interactive >/dev/null 2>&1; then fail "preview after refresh must show the drift"; fi
up
noop
expect_api 200 GET "/topics/$orders"
grep -q '"maxDeliveries":7' /work/api.out || fail "up did not restore maxDeliveries 7"

step "createOrUpdate takes over a queue that already exists"
expect_api 201 POST /topics "{\"name\":\"$prefix-adopted\",\"type\":\"queue\",\"maxDeliveries\":50}"
pulumi config set adopt true >/dev/null
up
noop
expect_api 200 GET "/topics/$prefix-adopted"
grep -q '"maxDeliveries":3' /work/api.out || fail "the existing queue was not corrected to maxDeliveries 3"

step "Rotation of the token: up --replace <urn>"
old_id="$(out tokenId)"
urn="$(pulumi stack --show-urns --non-interactive | grep 'taba:index/token:Token' | grep -o 'urn:pulumi:[^ ]*' | head -1)"
[ -n "$urn" ] || fail "could not find the urn of the token"
up --replace "$urn"
noop
new_id="$(out tokenId)"
[ "$old_id" != "$new_id" ] || fail "the token was not replaced"
expect_api 404 GET "/tokens/$old_id"
expect_api 200 GET "/tokens/$new_id"

step "import of a queue and of a token (they are not in the program: the state entry is dropped afterwards)"
expect_api 201 POST /topics "{\"name\":\"$prefix-imported\",\"type\":\"queue\"}"
quiet pulumi import --yes --non-interactive --out /work/imported.txt taba:index/queue:Queue imported "$prefix-imported"
grep -q "$prefix-imported" /work/imported.txt || fail "the generated code does not name the queue"
! grep -qi "forceDestroy\|force_destroy\|createOrUpdate\|create_or_update" /work/imported.txt || fail "the generated code carries provider-only fields: $(cat /work/imported.txt)"
echo "  queue imported"
expect_api 201 POST /tokens "{\"name\":\"$prefix-imported\",\"permissions\":[\"consume\"]}"
token_id="$(grep -o '"id":"[^"]*"' /work/api.out | head -1 | cut -d'"' -f4)"
[ -n "$token_id" ] || fail "no id in the token answer: $(cat /work/api.out)"
quiet pulumi import --yes --non-interactive --out /work/imported-token.txt taba:index/token:Token imported-token "$token_id"
echo "  token imported"
for r in imported imported-token; do
  u="$(pulumi stack --show-urns --non-interactive | grep -F "::$r" | grep -o 'urn:pulumi:[^ ]*' | head -1)"
  [ -n "$u" ] || fail "no urn for $r"
  quiet pulumi state delete --yes --force --non-interactive "$u"
done
expect_api 204 DELETE "/topics/$prefix-imported"
expect_api 204 DELETE "/tokens/$token_id"
noop

step "destroy leaves no topic or token of the program"
quiet pulumi destroy --yes --non-interactive
expect_api 200 GET /topics
if grep -q "\"$prefix-" /work/api.out; then cat /work/api.out >&2; fail "topics left after destroy"; fi
expect_api 200 GET /tokens
if grep -q "\"$prefix-app\"" /work/api.out; then fail "the token was left after destroy"; fi
echo
echo "scenarios of $lang: ok"
