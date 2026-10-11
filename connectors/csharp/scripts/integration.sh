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

# Subir o cluster, esperar e criar tokens/filas vêm de docker/cluster.sh, que os testes dos outros
# conectores também podem usar (inclusive para derrubar nós no meio do teste).

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/../../../docker/cluster.sh"
cd "$TABA_ROOT"

TEST_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) TABA_KEEP=1 ;;
    --no-build) TABA_BUILD=0 ;;
    --) shift; TEST_ARGS=("$@"); break ;;
    -h|--help) sed -n '2,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "opção desconhecida: $1 (veja --help)" >&2; exit 2 ;;
  esac
  shift
done

URL="${TABA_URL:-http://localhost:8080}"
RUN_ID="$(date +%Y%m%d%H%M%S)"

command -v dotnet >/dev/null || taba_die "dotnet não encontrado (precisa do SDK do .NET 6 ou superior)"
taba_require_docker

TOKEN_IDS=()

cleanup() {
  local status=$?
  if [ "$TABA_KEEP" -eq 1 ]; then
    taba_step "Revogando os tokens destes testes"
    for id in "${TOKEN_IDS[@]:-}"; do
      [ -n "$id" ] && taba_revoke_token "$id"
    done
  fi
  taba_teardown "$status" || true
  exit "$status"
}
trap cleanup EXIT

# --- 1. o cluster -------------------------------------------------------------------------------

taba_up
taba_step "Conferindo o cluster pelo taba-cli"
taba_status

# --- 2. tokens e filas --------------------------------------------------------------------------

taba_step "Criando os tokens dos testes"
read -r APP_ID APP_TOKEN < <(taba_create_token "csharp-tests-app-$RUN_ID" \
  --permission publish,consume,create,admin --prefix csharp-)
read -r RO_ID RO_TOKEN < <(taba_create_token "csharp-tests-readonly-$RUN_ID" \
  --permission consume --prefix csharp-)
TOKEN_IDS=("$APP_ID" "$RO_ID")
echo "  token da aplicação: $APP_ID (publish, consume, create, admin em csharp-*)"
echo "  token de leitura:   $RO_ID (consume em csharp-*)"

taba_step "Criando as filas dos testes"
taba_ensure_topic csharp-shared --type queue
taba_ensure_topic csharp-shared-fifo --type queue --fifo
taba_ensure_topic csharp-fan-a --type queue
taba_ensure_topic csharp-fan-b --type queue --fifo
taba_ensure_topic csharp-fan --type distributor --prefix csharp-fan-

# A criação só responde depois do quórum, mas os nós sincronizam a definição por mensagens: dá um
# instante para o HAProxy poder cair em qualquer um deles.
sleep 2

# --- 3. os testes -------------------------------------------------------------------------------

taba_step "Rodando os testes do conector C# (unitários e de integração) contra $URL"
export TABA_URL="$URL"
export TABA_TOKEN="$APP_TOKEN"
export TABA_TOKEN_READONLY="$RO_TOKEN"
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1

dotnet test "$TABA_ROOT/connectors/csharp/Taba.Client.Tests" "${TEST_ARGS[@]+"${TEST_ARGS[@]}"}"

taba_step "Tudo passou"
