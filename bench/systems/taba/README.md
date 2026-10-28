# Taba

Imagem: a do commit atual (`docker build` na raiz) ou a informada em `--taba-image`. Um nó, fator 1, sem TLS.
Driver: `connectors/omb` (distribuidor + uma fila por assinatura; chave = `groupId`). O workload `keyed-1kb` usa
`ordering: per-key`.

- `default`: `storage.db.synchronous=NORMAL`, sem limite de memória.
- `fsync`: `storage.db.synchronous=FULL` e `memory.limit=64MB` (o excesso vai para o disco).
