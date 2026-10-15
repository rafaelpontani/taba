#!/usr/bin/env bash
# Forces the backup now and checks that nothing is left pending (doc/pt-BR/33-backup-s3.md, "Operação").
#
#   examples/backup-s3/flush.sh               # the whole cluster, through the root Compose file
#   examples/backup-s3/flush.sh --node        # only taba-0
#   OPS_URL=http://10.0.0.5:9090 examples/backup-s3/flush.sh   # any cluster, through the operations port
#
# The flush is safe in production: it neither deregisters nor stops the node. It exits 0 only if the
# response says `"complete":true` (nothing pending on any node); with 1, the slow or down node shows up
# as `"pending":true` and the call can be repeated.

set -euo pipefail

scope="cluster"
[ "${1:-}" = "--node" ] && scope="node"
path="/cluster/backup/flush"
[ "$scope" = "node" ] && path="/node/backup/flush"

# On Windows with Git Bash, Docker converts paths such as /secrets/root-token.
export MSYS_NO_PATHCONV=1

call() {
  if [ -n "${OPS_URL:-}" ]; then
    curl -sS -X POST "$OPS_URL$path"
  else
    # Compose does not publish the operations port: the container's own wget calls the node.
    (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && \
      docker compose exec -T taba-0 wget -q -O - --post-data='' "http://127.0.0.1:9090$path")
  fi
}

echo "POST $path"
response="$(call)"
echo "$response"

case "$response" in
  *'"code":"backup.not-configured"'*)
    echo "the S3 backup is not configured (backup.s3.bucket)" >&2
    exit 2 ;;
  *'"complete":true'*)
    echo "ok: everything went to S3" ;;
  *)
    echo "incomplete: repeat the flush and check GET /node/backup (pending must reach 0)" >&2
    exit 1 ;;
esac
