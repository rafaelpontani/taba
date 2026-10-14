#!/usr/bin/env bash
# Para o cluster do Compose da raiz COM backup e o inicia de novo, mostrando que tudo volta do S3
# (doc/pt-BR/33-backup-s3.md, "Parar e iniciar o cluster"). Roda do zero e apaga os dados dos nós no
# meio do caminho (os volumes `data-N`), para a restauração vir do MinIO e não dos discos.
#
#   exemplos/backup-s3/parar-e-iniciar.sh
#
# Precisa de Docker com o plugin compose e da porta 8080 livre. Termina com 0 se as 20 mensagens
# publicadas voltaram.

set -euo pipefail
export MSYS_NO_PATHCONV=1
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

cli() { docker compose run --rm -T taba-cli "$@" 2>/dev/null | tr -d '\r'; }
ops() { docker compose exec -T taba-1 wget -q -O - "$@"; }

echo "== subindo o cluster do zero (com MinIO)"
docker compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
docker compose up -d --build --wait

echo "== publicando 20 mensagens em 'pedidos'"
cli topic create pedidos --type queue >/dev/null
for n in $(seq 1 20); do cli publish pedidos --text "pedido-$n" --id-only >/dev/null; done

echo "== parando o cluster: drena os nós, manda tudo ao S3 e grava a lista da parada"
stop="$(ops --post-data='' http://127.0.0.1:9090/cluster/stop)"
echo "$stop"
case "$stop" in *'"stopList":true'*) ;; *) echo "o backup não foi enviado (veja warnings)" >&2; exit 1 ;; esac

echo "== desligando os processos e APAGANDO os discos dos nós (só o bucket guarda as mensagens)"
docker compose stop taba-1 taba-2 taba-3
docker compose rm -f taba-1 taba-2 taba-3 >/dev/null
for volume in $(docker volume ls -q | grep -E '(^|_)data-[123]$'); do docker volume rm "$volume" >/dev/null; done

echo "== iniciando de novo: a restauração acontece sozinha"
docker compose up -d --wait

echo "== consumindo"
got="$(cli consume pedidos --max 10 --wait 10 --ack -o raw; cli consume pedidos --max 10 --wait 10 --ack -o raw)"
count="$(printf '%s\n' "$got" | grep -c '^pedido-' || true)"
echo "$count mensagens voltaram"
[ "$count" -eq 20 ] || { echo "esperava 20" >&2; exit 1; }
echo "ok: o cluster voltou do S3 com as 20 mensagens"
