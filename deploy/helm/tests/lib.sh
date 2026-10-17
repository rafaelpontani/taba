#!/usr/bin/env bash
# Funções comuns dos testes do chart. Faça `source lib.sh` (não execute).
#
# Variáveis úteis:
#   TABA_TEST_TAG       tag das imagens taba e taba-cli (padrão: rc)
#   TABA_TEST_CONTEXT   usa este contexto do kubectl em vez de criar um cluster kind
#   TABA_TEST_CLUSTER   nome do cluster kind (padrão: taba-helm)
#   TABA_TEST_KEEP=1    não apaga namespace/release no fim (para investigar)
#   TABA_TEST_LOAD      imagens locais para `kind load docker-image` (separadas por espaço)

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# No Git Bash do Windows os binários nativos (helm, kind) precisam de C:/... em vez de /c/...
CHART=$(cd "$HERE/../taba" && { pwd -W 2>/dev/null || pwd; })
CI="$CHART/ci"
TAG=${TABA_TEST_TAG:-rc}
CLI_TAG=${TABA_TEST_CLI_TAG:-$TAG}
CLUSTER=${TABA_TEST_CLUSTER:-taba-helm}
UC=${UC:-adhoc}
NS="taba-${UC}"
REL=taba
PASSED=0
FAILED=0
SKIPPED=0
export MSYS_NO_PATHCONV=1

log()  { printf '\033[36m[%s]\033[0m %s\n' "$UC" "$*" >&2; }
pass() { PASSED=$((PASSED + 1)); printf '\033[32m  PASS\033[0m %s\n' "$1"; }
fail() { FAILED=$((FAILED + 1)); printf '\033[31m  FAIL\033[0m %s%s\n' "$1" "${2:+ — $2}"; }
skip() { SKIPPED=$((SKIPPED + 1)); printf '\033[33m  SKIP\033[0m %s%s\n' "$1" "${2:+ — $2}"; }

# check "nome do teste" comando...   → PASS se o comando sair com 0.
check() {
  local name=$1; shift
  local out
  if out=$("$@" 2>&1); then pass "$name"; else fail "$name" "$(printf '%s' "$out" | tail -n 3 | tr '\n' ' ')"; fi
}

# check_fails "nome" comando...      → PASS se o comando FALHAR (teste negativo).
check_fails() {
  local name=$1; shift
  if out=$("$@" 2>&1); then
    fail "$name" "deveria falhar e passou"
  elif printf '%s' "$out" | grep -Eq 'docker:|Unable to find image|Cannot connect'; then
    fail "$name" "falha de infraestrutura, não do chart: $(printf '%s' "$out" | head -n 1)"
  else
    pass "$name"
  fi
}

# check_out "nome" "regex" comando... → PASS se a saída casar com o regex.
check_out() {
  local name=$1 re=$2; shift 2
  local out
  out=$("$@" 2>&1) || true
  if printf '%s' "$out" | grep -Eq -- "$re"; then pass "$name"; else fail "$name" "saída não casa /$re/: $(printf '%s' "$out" | tail -n 2 | tr '\n' ' ')"; fi
}

need() {  # need ferramenta...  → sai com SKIP (código 77) se faltar alguma
  local t missing=()
  for t in "$@"; do command -v "$t" >/dev/null 2>&1 || missing+=("$t"); done
  if [ ${#missing[@]} -gt 0 ]; then
    skip "$UC" "faltam ferramentas: ${missing[*]}"
    exit 77
  fi
}

# helm local ou, sem ele, a imagem alpine/helm (só serve para os testes estáticos: não enxerga o cluster).
helm_static() {
  if command -v helm >/dev/null 2>&1; then helm "$@"; return; fi
  local base
  base=$(cd "$HERE/.." && { pwd -W 2>/dev/null || pwd; })
  docker run --rm -v "$base:/c" alpine/helm:3.19.0 "$@"
}

# Caminho do chart visto por helm_static (dentro do contêiner quando não há helm local).
chart_path() { if command -v helm >/dev/null 2>&1; then echo "$CHART"; else echo /c/taba; fi; }
ci_path()    { if command -v helm >/dev/null 2>&1; then echo "$CI/$1"; else echo "/c/taba/ci/$1"; fi; }

# ---- cluster -------------------------------------------------------------------------------------

K() { kubectl ${KCTX:+--context "$KCTX"} "$@"; }
H() { helm ${KCTX:+--kube-context "$KCTX"} "$@"; }

cluster_up() {
  need kubectl helm
  if [ -n "${TABA_TEST_CONTEXT:-}" ]; then KCTX=$TABA_TEST_CONTEXT; return; fi
  need kind docker
  if ! kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
    log "criando cluster kind $CLUSTER"
    kind create cluster --name "$CLUSTER" --wait 120s >&2 || { fail "kind create cluster"; exit 1; }
  fi
  KCTX="kind-$CLUSTER"
  local img
  for img in ${TABA_TEST_LOAD:-}; do kind load docker-image "$img" --name "$CLUSTER" >&2; done
}

ns_fresh() {
  K delete ns "$NS" --ignore-not-found --wait=true >/dev/null 2>&1
  K create ns "$NS" >/dev/null
}

cleanup() {
  [ "${TABA_TEST_KEEP:-0}" = 1 ] && return
  [ -n "${KCTX:-}" ] || return
  H uninstall "$REL" -n "$NS" --wait >/dev/null 2>&1 || true
  K delete ns "$NS" --ignore-not-found --wait=false >/dev/null 2>&1 || true
}

# install valores.yaml [args do helm...]
install() {
  local values=$1; shift
  log "helm install ($(basename "$values"))"
  H upgrade --install "$REL" "$CHART" -n "$NS" -f "$values" --set "image.tag=$TAG" "$@" --wait --timeout "${TABA_TEST_TIMEOUT:-10m}" >&2
}

# Release "taba" + chart "taba" → o nome não se repete (helper taba.fullname).
fullname() { echo "$REL"; }
pod0()     { echo "$(fullname)-0"; }

# wait_nodes N  → espera o StatefulSet ter N pods prontos e o cluster sem condições
wait_nodes() {
  local n=$1 t=${2:-600} i
  for ((i = 0; i < t; i += 5)); do
    local ready
    ready=$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.readyReplicas}' 2>/dev/null)
    if [ "${ready:-0}" = "$n" ] && [ "$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.replicas}')" = "$n" ]; then return 0; fi
    sleep 5
  done
  return 1
}

# ops POD CAMINHO  → GET na porta de operações do pod (de dentro dele)
ops() { K -n "$NS" exec "$1" -- wget -qO- "http://127.0.0.1:9090$2"; }

# cluster_healthy → sem condições em /health/cluster?scope=cluster
cluster_healthy() {
  local out
  out=$(ops "$(pod0)" "/health/cluster?scope=cluster") || return 1
  printf '%s' "$out" | grep -Eq '"conditions" *: *\[ *\]'
}

wait_healthy() {
  local t=${1:-300} i
  for ((i = 0; i < t; i += 5)); do cluster_healthy && return 0; sleep 5; done
  return 1
}

secrets_name() { echo "${TABA_SECRETS_NAME:-$(fullname)-secrets}"; }
root_token()   { K -n "$NS" get secret "$(secrets_name)" -o jsonpath='{.data.root-token}' | base64 -d; }

# tcli_up  → sobe o pod "tcli" (imagem taba-cli) apontado para o Service, as operações do pod-0 e o root token
tcli_up() {
  # O arquivo root-token termina com quebra de linha; na variável de ambiente ela estraga o cabeçalho.
  local tok tokenenv=""
  tok=$(root_token 2>/dev/null | tr -d '\n')
  [ -n "$tok" ] && tokenenv="        - { name: TABA_CLI_TOKEN, value: \"$tok\" }"
  K -n "$NS" delete pod tcli --ignore-not-found --wait=true >/dev/null 2>&1
  K -n "$NS" apply -f - >/dev/null <<YAML
apiVersion: v1
kind: Pod
metadata: { name: tcli }
spec:
  restartPolicy: Never
  enableServiceLinks: false
  terminationGracePeriodSeconds: 1
  containers:
    - name: tcli
      image: rafaelpontani/taba-cli:${CLI_TAG}
      command: ["sleep", "7200"]
      env:
        - { name: TABA_CLI_URL, value: "http://$(fullname):8080" }
        - { name: TABA_CLI_OPS_URL, value: "http://$(pod0).$(fullname)-headless:9090" }
${tokenenv}
YAML
  K -n "$NS" wait --for=condition=Ready pod/tcli --timeout=180s >/dev/null
}

# Sem -i: com stdin aberto o taba-cli lê o corpo do stdin. Para publicar via stdin use tcli_in.
tcli() { K -n "$NS" exec tcli -- taba-cli "$@"; }
tcli_in() { K -n "$NS" exec -i tcli -- taba-cli "$@"; }

# publish_n TOPICO N  → publica N mensagens {"n":i}
publish_n() {
  local topic=$1 n=$2 i
  for ((i = 1; i <= n; i++)); do
    tcli publish "$topic" "{\"n\":$i}" >/dev/null || return 1
  done
}

# consume_count TOPICO N  → consome (com ack) até N mensagens e imprime quantas vieram
consume_count() {
  local topic=$1 want=$2 got=0 out tries=0
  while [ "$got" -lt "$want" ] && [ "$tries" -lt 30 ]; do
    out=$(tcli consume "$topic" --max "$((want - got))" --wait 2 --ack -o raw 2>/dev/null | grep -c '"n"')
    got=$((got + ${out:-0}))
    tries=$((tries + 1))
  done
  echo "$got"
}

summary() {
  printf '\n%s: %d ok, %d falhas, %d ignorados\n' "$UC" "$PASSED" "$FAILED" "$SKIPPED"
  [ "$FAILED" -eq 0 ]
}

# ---- utilidades extras ---------------------------------------------------------------------------

topic_create() { tcli topic create "$1" --type queue >/dev/null 2>&1 || true; }

# winpath DIR → caminho que o Docker entende (Windows/Git Bash) ou o próprio caminho (Linux/macOS)
winpath() { cygpath -m "$1" 2>/dev/null || echo "$1"; }

# in_pod POD COMANDO... → executa no pod do namespace
in_pod() { local p=$1; shift; K -n "$NS" exec "$p" -- "$@"; }

# http_code POD URL [wget extra...] → código HTTP visto de dentro do pod (busybox wget)
http_code() {
  local pod=$1 url=$2; shift 2
  K -n "$NS" exec "$pod" -- wget -S -q -O /dev/null "$@" "$url" 2>&1 | grep -o 'HTTP/[0-9.]* [0-9]*' | tail -n1 | awk '{print $2}'
}

# wait_for SEGUNDOS comando... → repete o comando a cada 5 s até ele passar
wait_for() {
  local t=$1 i; shift
  for ((i = 0; i < t; i += 5)); do "$@" >/dev/null 2>&1 && return 0; sleep 5; done
  return 1
}

# tem_n_pods N → o StatefulSet tem exatamente N pods prontos
ready_is() { [ "$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.readyReplicas}')" = "$1" ]; }

# s3_ls ENDPOINT ACCESS SECRET CAMINHO → lista recursivamente s3://CAMINHO com o aws-cli (pod efêmero no namespace)
s3_ls() {
  K -n "$NS" run "s3ls-$RANDOM" --rm -i --restart=Never --quiet --image amazon/aws-cli:2.27.50     --env "AWS_ACCESS_KEY_ID=$2" --env "AWS_SECRET_ACCESS_KEY=$3" --env AWS_DEFAULT_REGION=us-east-1     --command -- aws --endpoint-url "$1" s3 ls --recursive "s3://$4"
}
