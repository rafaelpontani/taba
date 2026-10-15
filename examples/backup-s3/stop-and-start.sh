#!/usr/bin/env bash
# Stops the root Compose cluster WITH backup and starts it again, showing that everything comes back from
# S3 (doc/pt-BR/33-backup-s3.md, "Parar e iniciar o cluster"). Runs from scratch and wipes the nodes' data
# midway (the nodes' volumes), so the restore comes from MinIO and not from the disks.
#
#   examples/backup-s3/stop-and-start.sh
#
# Needs Docker with the compose plugin and port 8080 free. Exits 0 if the 20 published messages came back.

set -euo pipefail
export MSYS_NO_PATHCONV=1
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

cli() { docker compose run --rm -T taba-cli "$@" 2>/dev/null | tr -d '\r'; }
ops() { docker compose exec -T taba-0 wget -q -O - "$@"; }

echo "== starting the cluster from scratch (with MinIO)"
docker compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
docker compose up -d --wait

echo "== publishing 20 messages to 'orders'"
cli topic create orders --type queue >/dev/null
for n in $(seq 1 20); do cli publish orders --text "order-$n" --id-only >/dev/null; done

echo "== stopping the cluster: drains the nodes, sends everything to S3 and writes the stop list"
stop="$(ops --post-data='' http://127.0.0.1:9090/cluster/stop)"
echo "$stop"
case "$stop" in *'"stopList":true'*) ;; *) echo "the backup was not sent (see warnings)" >&2; exit 1 ;; esac

echo "== shutting the processes down and WIPING the nodes' disks (only the bucket keeps the messages)"
docker compose stop taba-0 taba
# -v also removes the replicas' anonymous volumes (the first node's `data-0` is a named volume).
docker compose rm -f -v taba-0 taba >/dev/null
for volume in $(docker volume ls -q | grep -E '(^|_)data-0$'); do docker volume rm "$volume" >/dev/null; done

echo "== starting again: the restore happens by itself"
docker compose up -d --wait

echo "== consuming"
got="$(cli consume orders --max 10 --wait 10 --ack -o raw; cli consume orders --max 10 --wait 10 --ack -o raw)"
count="$(printf '%s\n' "$got" | grep -c '^order-' || true)"
echo "$count messages came back"
[ "$count" -eq 20 ] || { echo "expected 20" >&2; exit 1; }
echo "ok: the cluster came back from S3 with the 20 messages"
