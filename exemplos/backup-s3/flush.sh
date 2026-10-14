#!/usr/bin/env bash
# Força o backup agora e confere que nada ficou pendente (doc/pt-BR/33-backup-s3.md, "Operação").
#
#   exemplos/backup-s3/flush.sh               # o cluster inteiro, pelo Compose da raiz
#   exemplos/backup-s3/flush.sh --node        # só o taba-1
#   OPS_URL=http://10.0.0.5:9090 exemplos/backup-s3/flush.sh   # um cluster qualquer, pela porta de operação
#
# O flush é seguro em produção: não desregistra nem para o nó. Termina com código 0 só se a resposta
# disser `"complete":true` (nada pendente em nenhum nó); com 1, o nó lento ou fora do ar aparece
# em `"pending":true` e a chamada pode ser repetida.

set -euo pipefail

scope="cluster"
[ "${1:-}" = "--node" ] && scope="node"
path="/cluster/backup/flush"
[ "$scope" = "node" ] && path="/node/backup/flush"

# No Windows com Git Bash, o Docker converte caminhos como /secrets/root-token.
export MSYS_NO_PATHCONV=1

call() {
  if [ -n "${OPS_URL:-}" ]; then
    curl -sS -X POST "$OPS_URL$path"
  else
    # A porta de operação do Compose não é publicada: o wget do próprio contêiner chama o nó.
    (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && \
      docker compose exec -T taba-1 wget -q -O - --post-data='' "http://127.0.0.1:9090$path")
  fi
}

echo "POST $path"
response="$(call)"
echo "$response"

case "$response" in
  *'"code":"backup.not-configured"'*)
    echo "o backup em S3 não está configurado (backup.s3.bucket)" >&2
    exit 2 ;;
  *'"complete":true'*)
    echo "ok: tudo foi para o S3" ;;
  *)
    echo "incompleto: repita o flush e confira GET /node/backup (pending deve chegar a 0)" >&2
    exit 1 ;;
esac
