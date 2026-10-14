#!/usr/bin/env bash
# A validação de ponta a ponta da versão 0.1 (doc/pt-BR/32-roadmap.md).
#
# Sobe o Compose da raiz do zero e confere, nesta ordem:
#   1. o cluster forma: três nós saudáveis e `taba-cli health cluster` sem condições;
#   2. o taba-cli publica e consome (tópico, publish, consume --ack, list);
#   3. a queda brusca de um nó (`docker compose kill`): é detectada, o nó é excluído por votação, as
#      publicações e o consumo seguem com dois nós, e o nó volta como nó novo (`docker compose start`);
#   4. a saída ordenada de outro nó (`docker compose stop`, que dispara o drain) e a volta dele;
#   5. em nenhum momento uma mensagem publicada (confirmada pelo quórum) se perde: ao fim, o conjunto
#      de mensagens confirmadas é exatamente o publicado;
#   6. o backup em S3 (o MinIO do Compose): `POST /cluster/stop`, os discos dos nós apagados, o
#      cluster sobe de novo e as mensagens publicadas voltam do bucket.
#
# Só usa o taba-cli: é o teste geral integrado do cluster, e não de um conector. Cada conector tem o seu
# teste (ex.: connectors/csharp/scripts/integration.sh). Subir, derrubar e provocar falhas no cluster
# está em docker/cluster.sh, que os testes dos conectores também usam (source ou linha de comando).
#
# Uso:
#   docker/validate.sh                  # tudo, do zero; apaga o cluster no fim
#   docker/validate.sh --keep           # deixa o cluster de pé no fim
#   docker/validate.sh --no-build       # reaproveita as imagens já construídas
#
# Precisa de Docker (com o plugin compose). A porta 8080 do host precisa estar livre.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/cluster.sh"
cd "$TABA_ROOT"

while [ $# -gt 0 ]; do
  case "$1" in
    --keep) TABA_KEEP=1 ;;
    --no-build) TABA_BUILD=0 ;;
    -h|--help) sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "opção desconhecida: $1 (veja --help)" >&2; exit 2 ;;
  esac
  shift
done

# Atalhos para as funções de docker/cluster.sh, a biblioteca que os testes dos conectores também usam.
step() { taba_step "$@"; }
ok() { taba_ok "$@"; }
die() { taba_die "$@"; }
cli() { taba_cli "$@"; }
active_members() { taba_active_members; }

taba_require_docker

PUBLISHED="$(mktemp)"; ACKED="$(mktemp)"
cleanup() {
  local status=$?
  rm -f "$PUBLISHED" "$ACKED"
  taba_teardown "$status" || true
  exit "$status"
}
trap cleanup EXIT

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

taba_up

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

# --- 3. queda brusca ----------------------------------------------------------------------------

step "Queda brusca do taba-3 (docker compose kill)"
cli topic create validacao-queda --type queue >/dev/null
publish_many antes 30 validacao-queda
ok "30 mensagens publicadas e confirmadas pelo quórum"

taba_kill taba-3
taba_wait_members 2 120

publish_many durante 10 validacao-queda
ok "10 mensagens publicadas com dois nós"
drain_topic validacao-queda 2
[ "$(wc -l < "$ACKED")" -ge 10 ] || die "o consumo com dois nós devolveu menos de 10 mensagens"
ok "o consumo segue com dois nós ($(wc -l < "$ACKED") confirmadas até aqui)"

taba_start taba-3
taba_wait_stable 240

# --- 4. saída ordenada --------------------------------------------------------------------------

step "Saída ordenada do taba-2 (docker compose stop, com drain)"
publish_many drain 10 validacao-queda
taba_stop taba-2
taba_wait_members 2 120
taba_start taba-2
taba_wait_stable 240

# --- 5. nada se perdeu --------------------------------------------------------------------------

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

# --- 6. backup em S3 (MinIO) ---------------------------------------------------------------------

step "Backup em S3: parar o cluster, apagar os discos dos nós e restaurar do MinIO"
cli topic create validacao-backup --type queue >/dev/null
BACKUP_PUBLISHED="$(mktemp)"
for n in $(seq 1 20); do
  cli publish validacao-backup --text "backup-$n" --id-only >/dev/null || die "publicar backup-$n"
  echo "backup-$n" >> "$BACKUP_PUBLISHED"
done
ok "20 mensagens publicadas em validacao-backup"

stop="$(taba_compose exec -T taba-1 wget -q -O - --post-data='' http://127.0.0.1:9090/cluster/stop)" || die "POST /cluster/stop"
case "$stop" in *'"stopList":true'*) ok "parada programada: a lista dos nós foi gravada no S3" ;; *) die "o /cluster/stop não gravou a lista: $stop" ;; esac

taba_compose stop taba-1 taba-2 taba-3 >/dev/null 2>&1
taba_compose rm -f taba-1 taba-2 taba-3 >/dev/null 2>&1
for volume in $(docker volume ls -q | grep -E '(^|_)data-[123]$'); do docker volume rm "$volume" >/dev/null; done
ok "os discos dos nós foram apagados: só o bucket guarda as mensagens"

taba_compose up -d
taba_wait_healthy 240
BACKUP_ACKED="$(mktemp)"
for round in $(seq 1 6); do
  cli consume validacao-backup --max 10 --wait 5 --ack -o raw >> "$BACKUP_ACKED" || true
done
sort -u "$BACKUP_PUBLISHED" > /tmp/taba-backup-published.txt
sort -u "$BACKUP_ACKED" > /tmp/taba-backup-acked.txt
if ! diff -q /tmp/taba-backup-published.txt /tmp/taba-backup-acked.txt >/dev/null; then
  echo "publicadas e não restauradas:" >&2
  comm -23 /tmp/taba-backup-published.txt /tmp/taba-backup-acked.txt >&2
  die "a restauração do S3 não devolveu tudo"
fi
rm -f "$BACKUP_PUBLISHED" "$BACKUP_ACKED"
ok "as 20 mensagens voltaram do S3 depois de a infraestrutura ser apagada"

step "Estado final"
taba_status | sed -n 1,6p

step "A validação passou"
