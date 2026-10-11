#!/usr/bin/env bash
# Os testes de integração do conector C# contra um cluster Taba de verdade (doc 19, seção 18.7).
#
# O que faz:
#   1. sobe o cluster do docker-compose.yml da raiz (três nós atrás do HAProxy) e espera os três
#      ficarem saudáveis;
#   2. com o token raiz (que está no volume `secrets`), cria pelo taba-cli os tokens dos testes
#      (um da aplicação e um só de leitura) e as filas que os testes usam;
#   3. roda `dotnet test` com TABA_URL e os tokens no ambiente;
#   4. derruba o cluster e apaga os dados (a não ser que se peça para manter).
#
# Uso:
#   connectors/csharp/scripts/integration.sh                 # tudo, do zero
#   connectors/csharp/scripts/integration.sh --keep          # deixa o cluster de pé no fim
#   connectors/csharp/scripts/integration.sh --no-build      # reaproveita as imagens já construídas
#   connectors/csharp/scripts/integration.sh -- -f net8.0 --filter "Name~fifo"
#                                                            # o que vem depois de -- vai para o dotnet test
#
# Precisa de Docker (com o plugin compose) e do SDK do .NET 6 ou superior. A porta 8080 do host
# precisa estar livre (é a do HAProxy).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

KEEP=0
BUILD=1
TEST_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) KEEP=1 ;;
    --no-build) BUILD=0 ;;
    --) shift; TEST_ARGS=("$@"); break ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "opção desconhecida: $1 (veja --help)" >&2; exit 2 ;;
  esac
  shift
done

URL="${TABA_URL:-http://localhost:8080}"
RUN_ID="$(date +%Y%m%d%H%M%S)"

log() { printf '\n== %s\n' "$*"; }
die() { echo "erro: $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker não encontrado"
docker compose version >/dev/null 2>&1 || die "o plugin 'docker compose' não está instalado"
command -v dotnet >/dev/null || die "dotnet não encontrado (precisa do SDK do .NET 6 ou superior)"
docker info >/dev/null 2>&1 || die "o Docker não está rodando"

# O taba-cli do Compose, já apontado para o cluster e com o token raiz do volume.
cli() { docker compose run --rm -T taba-cli "$@"; }

TOKEN_IDS=()

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    echo
    echo "Os testes falharam (código $status). Últimas linhas dos nós:" >&2
    docker compose logs --tail 20 taba-1 taba-2 taba-3 2>/dev/null >&2 || true
  fi
  if [ "$KEEP" -eq 1 ]; then
    log "Cluster mantido (docker compose down -v para apagar). Revogando os tokens destes testes"
    for id in "${TOKEN_IDS[@]:-}"; do
      [ -n "$id" ] && cli token revoke "$id" --yes >/dev/null 2>&1 || true
    done
  else
    log "Derrubando o cluster e apagando os dados"
    docker compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup EXIT

# --- 1. o cluster -------------------------------------------------------------------------------

log "Subindo o cluster (três nós e o HAProxy)"
if [ "$BUILD" -eq 1 ]; then
  docker compose --profile tools build
fi
docker compose up -d

log "Esperando os três nós ficarem saudáveis"
deadline=$((SECONDS + 240))
while true; do
  healthy=0
  for node in taba-1 taba-2 taba-3; do
    id="$(docker compose ps -q "$node" 2>/dev/null || true)"
    [ -n "$id" ] || continue
    state="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$id" 2>/dev/null || echo none)"
    [ "$state" = "healthy" ] && healthy=$((healthy + 1))
  done
  echo "  nós saudáveis: $healthy de 3"
  [ "$healthy" -eq 3 ] && break
  [ "$SECONDS" -lt "$deadline" ] || die "o cluster não ficou saudável em 240 s"
  sleep 3
done

log "Conferindo o cluster pelo taba-cli"
cli health cluster -o table

# --- 2. tokens e filas --------------------------------------------------------------------------

# Cria um token e devolve "id segredo" (a resposta é um JSON, e o segredo aparece só nela).
create_token() {
  local name="$1"; shift
  local out
  out="$(cli token create --name "$name" "$@" -o json)"
  local id secret
  id="$(printf '%s' "$out" | tr -d '\r' | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' | head -n 1)"
  secret="$(printf '%s' "$out" | tr -d '\r' | sed -n 's/.*"token": *"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$id" ] && [ -n "$secret" ] || die "não consegui ler o token criado: $out"
  echo "$id $secret"
}

log "Criando os tokens dos testes"
read -r APP_ID APP_TOKEN < <(create_token "csharp-tests-app-$RUN_ID" \
  --permission publish,consume,create,admin --prefix csharp-)
read -r RO_ID RO_TOKEN < <(create_token "csharp-tests-readonly-$RUN_ID" \
  --permission consume --prefix csharp-)
TOKEN_IDS=("$APP_ID" "$RO_ID")
echo "  token da aplicação: $APP_ID (publish, consume, create, admin em csharp-*)"
echo "  token de leitura:   $RO_ID (consume em csharp-*)"

ensure_topic() {
  local name="$1"; shift
  if cli topic get "$name" >/dev/null 2>&1; then
    echo "  $name já existe"
  else
    cli topic create "$name" "$@" >/dev/null
    echo "  $name criado"
  fi
}

log "Criando as filas dos testes"
ensure_topic csharp-shared --type queue
ensure_topic csharp-shared-fifo --type queue --fifo
ensure_topic csharp-fan-a --type queue
ensure_topic csharp-fan-b --type queue --fifo
ensure_topic csharp-fan --type distributor --prefix csharp-fan-

# A criação só responde depois do quórum, mas os nós sincronizam a definição por mensagens: dá um
# instante para o HAProxy poder cair em qualquer um deles.
sleep 2

# --- 3. os testes -------------------------------------------------------------------------------

log "Rodando os testes do conector C# (unitários e de integração) contra $URL"
export TABA_URL="$URL"
export TABA_TOKEN="$APP_TOKEN"
export TABA_TOKEN_READONLY="$RO_TOKEN"
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1

dotnet test "$ROOT/connectors/csharp/Taba.Client.Tests" "${TEST_ARGS[@]+"${TEST_ARGS[@]}"}"

log "Tudo passou"
