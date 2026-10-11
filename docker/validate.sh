#!/usr/bin/env bash
# A validação de ponta a ponta da versão 0.1 (doc/08c-proximos-passos.md, passo 5).
#
# Sobe o Compose da raiz do zero e confere, nesta ordem:
#   1. o cluster forma: três nós saudáveis e `taba-cli health cluster` sem condições;
#   2. o taba-cli publica e consome (tópico, publish, consume --ack, list);
#   3. o exemplo de console em C# publica e consome;
#   4. a queda brusca de um nó (`docker compose kill`): é detectada, o nó é excluído por votação, as
#      publicações e o consumo seguem com dois nós, e o nó volta como nó novo (`docker compose start`);
#   5. a saída ordenada de outro nó (`docker compose stop`, que dispara o drain) e a volta dele;
#   6. em nenhum momento uma mensagem publicada (confirmada pelo quórum) se perde: ao fim, o conjunto
#      de mensagens confirmadas é exatamente o publicado.
#
# Uso:
#   docker/validate.sh                  # tudo, do zero; apaga o cluster no fim
#   docker/validate.sh --keep           # deixa o cluster de pé no fim
#   docker/validate.sh --no-build       # reaproveita as imagens já construídas
#   docker/validate.sh --skip-csharp    # sem o exemplo em C# (não precisa do SDK do .NET)
#
# Precisa de Docker (com o plugin compose). A porta 8080 do host precisa estar livre.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

KEEP=0
BUILD=1
CSHARP=1
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) KEEP=1 ;;
    --no-build) BUILD=0 ;;
    --skip-csharp) CSHARP=0 ;;
    -h|--help) sed -n '2,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "opção desconhecida: $1 (veja --help)" >&2; exit 2 ;;
  esac
  shift
done

# O Git Bash do Windows converte caminhos como /secrets/root-token.
export MSYS_NO_PATHCONV=1
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1

step() { printf '\n== %s\n' "$*"; }
ok() { printf '  ok: %s\n' "$*"; }
die() { echo "FALHOU: $*" >&2; exit 1; }

command -v docker >/dev/null || die "docker não encontrado"
docker compose version >/dev/null 2>&1 || die "o plugin 'docker compose' não está instalado"
docker info >/dev/null 2>&1 || die "o Docker não está rodando"

PUBLISHED="$(mktemp)"; ACKED="$(mktemp)"

cli() { docker compose run --rm -T taba-cli "$@" 2>/dev/null | tr -d '\r'; }

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    echo >&2
    echo "A validação falhou. Últimas linhas dos nós:" >&2
    docker compose logs --tail 25 taba-1 taba-2 taba-3 2>/dev/null >&2 || true
  fi
  if [ "$KEEP" -eq 1 ]; then
    step "Cluster mantido (docker compose down -v para apagar)"
  else
    step "Derrubando o cluster e apagando os dados"
    docker compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
  fi
  rm -f "$PUBLISHED" "$ACKED"
  exit "$status"
}
trap cleanup EXIT

healthy_nodes() {
  local n=0 id state node
  for node in taba-1 taba-2 taba-3; do
    id="$(docker compose ps -q "$node" 2>/dev/null || true)"
    [ -n "$id" ] || continue
    state="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$id" 2>/dev/null || echo none)"
    [ "$state" = "healthy" ] && n=$((n + 1))
  done
  echo "$n"
}

# Quantos membros ativos o cluster enxerga (pelo taba-1; se ele estiver fora, 0).
active_members() {
  cli health cluster -o json | grep -o '"active": *[0-9]*' | head -n 1 | grep -o '[0-9]*$' || true
}

worst_condition() {
  cli health cluster -o table | awk '/^worstCondition/ {print $2}' || true
}

# Espera uma condição (um comando que devolve 0), com limite de tempo.
wait_for() {
  local what="$1" limit="$2"; shift 2
  local deadline=$((SECONDS + limit))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || die "$what: não aconteceu em ${limit} s"
    sleep 3
  done
  ok "$what (${SECONDS}s no total)"
}

three_healthy() { [ "$(healthy_nodes)" -eq 3 ]; }
two_members() { [ "$(active_members)" = "2" ]; }
# O ID antigo do nó que caiu fica congelado por cluster.freezing.duration (5 min), e isso aparece como o
# aviso `node-frozen`; o nó reiniciado tem ID novo e não é afetado. Qualquer outra condição é falha.
other_conditions() { cli health cluster -o json | grep -o '"condition": *"[^"]*"' | grep -v node-frozen || true; }
three_members_ok() { [ "$(active_members)" = "3" ] && [ -z "$(other_conditions)" ] && three_healthy; }

# Publica "$2" mensagens de texto "$1-N" no tópico "$3" e anota cada uma em $PUBLISHED.
publish_many() {
  local prefix="$1" count="$2" topic="$3" n
  for n in $(seq 1 "$count"); do
    cli publish "$topic" --text "$prefix-$n" --id-only >/dev/null || die "publicar $prefix-$n em $topic"
    echo "$prefix-$n" >> "$PUBLISHED"
  done
}

# Consome e confirma o que houver (até esvaziar), anotando em $ACKED.
drain_topic() {
  local topic="$1" max_rounds="${2:-20}" round out
  for round in $(seq 1 "$max_rounds"); do
    out="$(cli consume "$topic" --max 10 --wait 3 --ack -o raw)"
    [ -n "$out" ] || return 0
    printf '%s\n' "$out" >> "$ACKED"
  done
}

# --- 1. o cluster forma ------------------------------------------------------------------------

step "Subindo o Compose do zero"
docker compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
if [ "$BUILD" -eq 1 ]; then
  docker compose --profile tools build
fi
docker compose up -d
wait_for "os três nós ficam saudáveis" 240 three_healthy

step "O cluster formou: taba-cli health cluster"
[ "$(active_members)" = "3" ] || die "esperava 3 membros ativos, achei '$(active_members)'"
cli health cluster -o json | grep -q '"conditions": \[\]' || die "o cluster tem condições: $(cli health cluster -o table)"
ok "3 membros ativos e nenhuma condição"

# --- 2. taba-cli --------------------------------------------------------------------------------

step "taba-cli: criar tópico, publicar e consumir"
cli topic create validacao-cli --type queue >/dev/null
cli publish validacao-cli --text "ola do taba-cli" --id-only >/dev/null
got="$(cli consume validacao-cli --max 1 --wait 5 --ack -o raw)"
[ "$got" = "ola do taba-cli" ] || die "o consume devolveu '$got'"
cli topic list --name validacao-cli | grep -q validacao-cli || die "o topic list não mostra o tópico"
ok "publish, consume --ack e topic list"

# --- 3. exemplo C# ------------------------------------------------------------------------------

if [ "$CSHARP" -eq 1 ]; then
  step "Exemplo de console em C#"
  command -v dotnet >/dev/null || die "dotnet não encontrado (use --skip-csharp para pular)"
  TABA_URL="http://localhost:8080" \
  TABA_TOKEN="$(docker compose run --rm -T --entrypoint cat taba-cli /secrets/root-token 2>/dev/null | tr -d '\r\n')" \
    dotnet run --project exemplos/csharp-console | tee /tmp/taba-csharp-example.log
  grep -q "3 mensagens recebidas e confirmadas" /tmp/taba-csharp-example.log || die "o exemplo C# não recebeu as 3 mensagens"
  ok "o exemplo C# publicou, consumiu e confirmou"
fi

# --- 4. queda brusca ----------------------------------------------------------------------------

step "Queda brusca do taba-3 (docker compose kill)"
cli topic create validacao-queda --type queue >/dev/null
publish_many antes 30 validacao-queda
ok "30 mensagens publicadas e confirmadas pelo quórum"

docker compose kill taba-3 >/dev/null 2>&1
wait_for "a queda é detectada e o nó é excluído por votação (2 membros)" 120 two_members

publish_many durante 10 validacao-queda
ok "10 mensagens publicadas com dois nós"
drain_topic validacao-queda 2
[ "$(wc -l < "$ACKED")" -ge 10 ] || die "o consumo com dois nós devolveu menos de 10 mensagens"
ok "o consumo segue com dois nós ($(wc -l < "$ACKED") confirmadas até aqui)"

docker compose start taba-3 >/dev/null 2>&1
wait_for "o nó volta como nó novo e o cluster fica com 3 membros (só o aviso node-frozen é aceito)" 240 three_members_ok

# --- 5. saída ordenada --------------------------------------------------------------------------

step "Saída ordenada do taba-2 (docker compose stop, com drain)"
publish_many drain 10 validacao-queda
docker compose stop taba-2 >/dev/null 2>&1
wait_for "o taba-2 sai do cluster (2 membros)" 120 two_members
docker compose start taba-2 >/dev/null 2>&1
wait_for "o taba-2 volta e o cluster fica com 3 membros" 240 three_members_ok

# --- 6. nada se perdeu --------------------------------------------------------------------------

step "Nenhuma mensagem confirmada se perdeu"
drain_topic validacao-queda 30
sort -u "$PUBLISHED" > /tmp/taba-published.txt
sort -u "$ACKED" > /tmp/taba-acked.txt
if ! diff -q /tmp/taba-published.txt /tmp/taba-acked.txt >/dev/null; then
  echo "publicadas e não recebidas:" >&2
  comm -23 /tmp/taba-published.txt /tmp/taba-acked.txt >&2
  die "o conjunto recebido difere do publicado"
fi
ok "$(wc -l < /tmp/taba-published.txt) mensagens publicadas, todas recebidas e confirmadas"

step "Estado final"
cli health cluster -o table | sed -n 1,6p

step "A validação passou"
