#!/usr/bin/env bash
# Shared pieces of the gateway contract tests (connectors/gateway-contract/README.md): the targets (one Taba node with the
# gateway on, or the emulators ElasticMQ and moto), the way to run a driver in the container of its language, and the
# rules that tell a good red run from a bad one.
#
# For `source`; it does nothing by itself.

GW_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$GW_DIR/../.." && pwd)"

# Git Bash on Windows converts paths such as /repo.
export MSYS_NO_PATHCONV=1

# The images of the emulators. ElasticMQ answers SQS in both protocols; moto answers SQS and SNS together.
ELASTICMQ_IMAGE="${ELASTICMQ_IMAGE:-softwaremill/elasticmq-native:1.7.1}"
MOTO_IMAGE="${MOTO_IMAGE:-motoserver/moto:5.1.22}"

gw_step() { printf '\n== %s\n' "$*"; }
gw_die() { echo "FAILED: $*" >&2; exit 1; }

# A path Docker accepts as the source of a bind mount (on Windows, C:/...).
gw_hostpath() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }

gw_require_docker() {
  command -v docker >/dev/null || gw_die "docker not found"
  docker info >/dev/null 2>&1 || gw_die "Docker is not running"
}

GW_NETWORK="${GW_NETWORK:-}"
GW_CONTAINERS=()
GW_TMP=""

gw_network_up() {
  GW_NETWORK="gw-net-$$"
  GW_TMP="$(mktemp -d)"
  docker network create "$GW_NETWORK" >/dev/null
}

gw_down() {
  local status="${1:-$?}"
  if [ "${GW_KEEP:-0}" = "1" ]; then
    echo "kept: containers ${GW_CONTAINERS[*]:-} and network $GW_NETWORK (docker rm -f ...; docker network rm $GW_NETWORK)"
  else
    [ ${#GW_CONTAINERS[@]} -eq 0 ] || docker rm -f "${GW_CONTAINERS[@]}" >/dev/null 2>&1 || true
    [ -z "$GW_NETWORK" ] || docker network rm "$GW_NETWORK" >/dev/null 2>&1 || true
    [ -z "$GW_TMP" ] || rm -rf "$GW_TMP"
  fi
  return "$status"
}

# docker run on the target's network with the repository mounted read-only at /repo.
#   gw_docker <image> [docker options...] -- <command...>
gw_docker() {
  local image="$1"; shift
  local opts=()
  while [ "$1" != "--" ]; do opts+=("$1"); shift; done
  shift
  # shellcheck disable=SC2086  # GW_DOCKER_ENV is a list of -e options on purpose
  docker run --rm --network "$GW_NETWORK" -v "$(gw_hostpath "$REPO_ROOT"):/repo:ro" \
    $GW_DOCKER_ENV ${opts[@]+"${opts[@]}"} "$image" "$@"
}

# ---- the Taba target --------------------------------------------------------------------------------------------------

# A call to the Taba API with the development token, from a throwaway container: gw_taba_api METHOD PATH [JSON]
gw_taba_api() {
  docker run --rm --network "$GW_NETWORK" -e URL="http://$GW_NODE:8080" -e TOKEN="$GW_ADMIN_TOKEN" -e METHOD="$1" -e API_PATH="$2" -e BODY="${3:-}" node:22-slim node -e '
    (async () => {
      const r = await fetch(process.env.URL + process.env.API_PATH, { method: process.env.METHOD,
        headers: { Authorization: "Bearer " + process.env.TOKEN, "Content-Type": "application/json" },
        body: process.env.BODY || undefined });
      process.stdout.write(r.status + " " + (await r.text()));
    })().catch((e) => { process.stdout.write("0 " + e.message); });'
}

GW_NODE=""
GW_ADMIN_TOKEN=""
GW_RUN_ID=""
GW_SQS_URL=""
GW_SNS_URL=""
GW_CREDS=()          # name=accessKey:secretKey for the drivers (--cred)
GW_DEFAULT_KEY=""
GW_DEFAULT_SECRET=""

# Creates a Taba token and prints "token" (the secret shows up only once).
gw_make_token() { # gw_make_token <name> <permissions json> [prefixes json]
  local out
  out="$(gw_taba_api POST /tokens "{\"name\":\"$1\",\"permissions\":$2,\"topicPrefixes\":${3:-[]}}")"
  case "$out" in 20*|201*) ;; *) gw_die "could not create the token $1: $out" ;; esac
  printf '%s' "$out" | sed -n 's/.*"token":"\([^"]*\)".*/\1/p'
}

gw_random() { LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c "$1"; }

# Starts one Taba node with the gateway port on and the credentials file (the keys are the ones of the plan, section 4.1).
gw_target_taba_up() {
  local image="${TABA_IMAGE:-taba:local}"
  docker image inspect "$image" >/dev/null 2>&1 || { gw_step "Building the node image ($image)"; docker build -t "$image" "$REPO_ROOT"; }
  GW_NODE="gw-taba-$$"
  GW_CONTAINERS+=("$GW_NODE")
  GW_ADMIN_TOKEN="tb_dev.gatewaycontractdevsecret0123456789abcdef"
  mkdir -p "$GW_TMP/gateway"
  echo '{}' >"$GW_TMP/gateway/credentials.json"
  gw_step "Starting one Taba node ($image) with gateway.sqs and gateway.sns on (one port)"
  docker run -d --name "$GW_NODE" --network "$GW_NETWORK" \
    -v "$(gw_hostpath "$GW_TMP/gateway"):/gateway" \
    -e TABA_REPLICATION_FACTOR=1 -e TABA_CLUSTER_TLS_ENABLED=false \
    -e TABA_AUTH_DEV_TOKEN="${GW_ADMIN_TOKEN#tb_dev.}" -e TABA_OPERATIONS_PORT=9090 \
    -e TABA_GATEWAY_AWS_PORT=9324 -e TABA_GATEWAY_SQS_ENABLED=true -e "TABA_GATEWAY_SQS_PUBLIC_URL=http://$GW_NODE:9324" \
    -e TABA_GATEWAY_SNS_ENABLED=true \
    -e TABA_GATEWAY_AWS_CREDENTIALS_FILE=/gateway/credentials.json \
    "$image" >/dev/null
  gw_wait_ready
  GW_RUN_ID="$(date +%m%d%H%M%S)"
  # Credentials: access key -> secret + the Taba token whose permissions apply (provisional format, plan section 4.2).
  local ok consume prefixed key_ok key_consume key_prefixed secret_ok secret_consume secret_prefixed
  ok="$(gw_make_token gw-ok '["publish","consume","create","manage"]')"
  consume="$(gw_make_token gw-consume '["consume"]')"
  prefixed="$(gw_make_token gw-prefixed '["publish","consume","create","manage"]' "[\"gw$GW_RUN_ID\"]")"
  key_ok="GWOK$(gw_random 16)"; key_consume="GWCO$(gw_random 16)"; key_prefixed="GWPR$(gw_random 16)"
  secret_ok="$(gw_random 40)"; secret_consume="$(gw_random 40)"; secret_prefixed="$(gw_random 40)"
  cat >"$GW_TMP/gateway/credentials.json" <<EOF
{
  "$key_ok": {"secret": "$secret_ok", "token": "$ok"},
  "$key_consume": {"secret": "$secret_consume", "token": "$consume"},
  "$key_prefixed": {"secret": "$secret_prefixed", "token": "$prefixed"}
}
EOF
  docker restart "$GW_NODE" >/dev/null
  gw_wait_ready
  GW_SQS_URL="http://$GW_NODE:9324"
  GW_SNS_URL="$GW_SQS_URL"   # SQS and SNS are served on the same port
  GW_DEFAULT_KEY="$key_ok"; GW_DEFAULT_SECRET="$secret_ok"
  GW_CREDS=("ok=$key_ok:$secret_ok" "badsecret=$key_ok:$(gw_random 40)" "unknown=GWNO$(gw_random 16):$(gw_random 40)"
            "consumeonly=$key_consume:$secret_consume" "prefixed=$key_prefixed:$secret_prefixed")
}

gw_wait_ready() {
  local deadline=$((SECONDS + 90))
  until docker exec "$GW_NODE" wget -q -O /dev/null http://127.0.0.1:9090/health/ready 2>/dev/null; do
    [ "$SECONDS" -lt "$deadline" ] || { docker logs --tail 30 "$GW_NODE" >&2; gw_die "the node did not become ready"; }
    sleep 1
  done
}

# Reads the queue of scenarios/taba/native.scn with the Taba API, the way a native consumer does, and prints "ok" if both
# messages are there, "missing" if the topic does not exist, or what went wrong.
gw_native_check() { # gw_native_check <topic> <lang>
  local out
  out="$(gw_taba_api GET "/topics/$1/messages?maxMessages=10&waitTime=2&visibilityTimeout=30")"
  case "$out" in
    404*) echo missing ;;
    200*) if printf '%s' "$out" | grep -q "native-from-$2" && printf '%s' "$out" | grep -q "caf.*-native-$2"; then echo ok; else echo "unexpected messages: ${out:0:200}"; fi ;;
    *) echo "unexpected answer: ${out:0:200}" ;;
  esac
}

# ---- the emulator target ------------------------------------------------------------------------------------------------

GW_ELASTICMQ_URL=""
GW_MOTO_URL=""

gw_target_emulator_up() {
  GW_RUN_ID="$(date +%m%d%H%M%S)"
  gw_step "Starting ElasticMQ ($ELASTICMQ_IMAGE) and moto ($MOTO_IMAGE)"
  docker run -d --name "gw-elasticmq-$$" --network "$GW_NETWORK" --network-alias elasticmq \
    -v "$(gw_hostpath "$GW_DIR/emulator")/elasticmq.conf:/opt/elasticmq.conf" "$ELASTICMQ_IMAGE" >/dev/null
  docker run -d --name "gw-moto-$$" --network "$GW_NETWORK" --network-alias moto "$MOTO_IMAGE" >/dev/null
  GW_CONTAINERS+=("gw-elasticmq-$$" "gw-moto-$$")
  local deadline=$((SECONDS + 60))
  until docker logs "gw-elasticmq-$$" 2>&1 | grep -q "started in"; do
    [ "$SECONDS" -lt "$deadline" ] || gw_die "ElasticMQ did not start"
    sleep 1
  done
  deadline=$((SECONDS + 60))
  until docker run --rm --network "$GW_NETWORK" node:22-slim node -e 'fetch("http://moto:5000/moto-api/").then(()=>process.exit(0)).catch(()=>process.exit(1))' 2>/dev/null; do
    [ "$SECONDS" -lt "$deadline" ] || gw_die "moto did not start"
    sleep 1
  done
  GW_ELASTICMQ_URL="http://elasticmq:9324"
  GW_MOTO_URL="http://moto:5000"
  GW_DEFAULT_KEY="test"; GW_DEFAULT_SECRET="test"
  GW_CREDS=()
}

# ---- running a driver ------------------------------------------------------------------------------------------------------

# The drivers of a language and their variants: drivers/<lang>/variants has one "name mode" per line (mode: env, when
# the SDK reads AWS_ENDPOINT_URL_*, or code, when the endpoint goes in the code).
gw_variants() { grep -v '^#' "$GW_DIR/drivers/$1/variants" | awk 'NF {print $1}'; }
gw_variant_mode() { grep -v '^#' "$GW_DIR/drivers/$1/variants" | awk -v v="$2" '$1 == v {print $2}'; }

# gw_run_driver <lang> <variant> <prefix> <sqs url> <sns url> <extra driver args...> -- <scenario files (relative to scenarios/)...>
# Prints the driver output (RESULT lines and SUMMARY).
gw_run_driver() {
  local lang="$1" variant="$2" prefix="$3" sqs="$4" sns="$5"; shift 5
  local extra=()
  while [ "$1" != "--" ]; do extra+=("$1"); shift; done
  shift
  local mode; mode="$(gw_variant_mode "$lang" "$variant")"
  local args=(--sqs-endpoint "$sqs" --sns-endpoint "$sns" --endpoint-mode "$mode" --prefix "$prefix" --lang "$lang" --variant "$variant")
  local c f
  for c in ${GW_CREDS[@]+"${GW_CREDS[@]}"}; do args+=(--cred "$c"); done
  for f in "$@"; do args+=(--scenario "/repo/connectors/gateway-contract/scenarios/$f"); done
  GW_DOCKER_ENV="-e AWS_ACCESS_KEY_ID=$GW_DEFAULT_KEY -e AWS_SECRET_ACCESS_KEY=$GW_DEFAULT_SECRET -e AWS_REGION=us-east-1"
  if [ "$mode" = env ]; then
    GW_DOCKER_ENV="$GW_DOCKER_ENV -e AWS_ENDPOINT_URL_SQS=$sqs -e AWS_ENDPOINT_URL_SNS=$sns"
  fi
  export GW_DOCKER_ENV GW_NETWORK REPO_ROOT
  "$GW_DIR/drivers/run.sh" "$lang" "$variant" "${args[@]}" ${extra[@]+"${extra[@]}"}
}

# ---- reading the result of a driver --------------------------------------------------------------------------------------------

# The lines of a scenario file that are operations (not blank, not comments).
gw_scenario_lines() {
  local f total=0 n
  for f in "$@"; do
    n="$(grep -cvE '^[[:space:]]*(#|$)' "$GW_DIR/scenarios/$f" || true)"
    total=$((total + n))
  done
  echo "$total"
}

# gw_judge <log> <expected line count> <want: pass|connect> <name>
# pass:    every RESULT line is PASS.
# connect: every RESULT line is CONNECT (the red run: nothing answers), except the control lines (`sleep`,
#          `useCredentials`), which never talk to the gateway and so are PASS. A line that passes, fails for any other
#          reason, or is missing, makes the run bad, and so does a driver that did not print its SUMMARY.
# Prints a short verdict; returns 0 when the run is as wanted.
gw_judge() {
  local log="$1" expected="$2" want="$3" name="$4"
  local results pass fail connect control
  results="$(grep -c '^RESULT ' "$log" || true)"
  pass="$(grep -c '^RESULT PASS ' "$log" || true)"
  fail="$(grep -c '^RESULT FAIL ' "$log" || true)"
  connect="$(grep -c '^RESULT CONNECT ' "$log" || true)"
  control="$(grep -cE '^RESULT PASS [^ ]+ (sleep|useCredentials)( |$)' "$log" || true)"
  if ! grep -q '^SUMMARY ' "$log"; then
    echo "  BAD  $name: the driver did not finish (no SUMMARY); the last lines of its log:"
    tail -n 8 "$log" | sed 's/^/        /'
    return 1
  fi
  if [ "$results" -ne "$expected" ]; then
    echo "  BAD  $name: $results lines answered, $expected expected (a line was skipped or repeated)"
    return 1
  fi
  if [ "$want" = pass ]; then
    if [ "$pass" -eq "$expected" ]; then echo "  ok   $name: $pass/$expected lines pass"; return 0; fi
    echo "  BAD  $name: $pass/$expected pass ($fail failed, $connect unreachable)"
    grep -E '^RESULT (FAIL|CONNECT) ' "$log" | head -n 12 | sed 's/^RESULT /        /'
    return 1
  fi
  if [ "$pass" -eq "$control" ] && [ "$connect" -eq $((expected - control)) ]; then
    echo "  red  $name: $connect/$((expected - control)) lines fail because nothing answers ($control control lines do not call the gateway)"
    return 0
  fi
  echo "  BAD  $name: not all red: $((pass - control)) pass, $fail failed for another reason, $connect unreachable"
  grep -E '^RESULT (PASS|FAIL) ' "$log" | grep -vE ' (sleep|useCredentials)( |$)' | head -n 12 | sed 's/^RESULT /        /'
  return 1
}
