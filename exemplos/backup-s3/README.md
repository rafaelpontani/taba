# Backup em S3: configuração e operação

Exemplos do backup opcional em S3 ([`doc/pt-BR/33-backup-s3.md`](../../doc/pt-BR/33-backup-s3.md)): a configuração com MinIO e com a AWS, e o passo a passo de **forçar o backup (flush)**, **parar** e **iniciar** o cluster.

| Arquivo | O que é |
|---|---|
| [`taba-minio.yml`](taba-minio.yml) | Configuração do nó com MinIO (ou outro S3 compatível), com todos os ajustes comentados |
| [`taba-aws.yml`](taba-aws.yml) | Configuração na AWS (credenciais pela cadeia padrão: papel, IAM, perfil) e as permissões mínimas |
| [`flush.sh`](flush.sh) | Força o backup e confere que nada ficou pendente |
| [`parar-e-iniciar.sh`](parar-e-iniciar.sh) | Para o cluster com backup, apaga os discos dos nós e inicia de novo: tudo volta do S3 |

O `docker-compose.yml` da raiz **já sobe um MinIO** e liga o backup nos três nós (`TABA_BACKUP_S3_*`). Console do MinIO: <http://localhost:9001> (`taba` / `taba-minio-secret`, só de demonstração).

## 1. Ligar

Com o Compose da raiz: `docker compose up --build -d`. Com o seu próprio cluster, defina o bucket (e, fora da AWS, o `endpoint` e as chaves) no arquivo de configuração ou nas variáveis:

```
TABA_BACKUP_S3_BUCKET=taba
TABA_BACKUP_S3_ENDPOINT=http://minio:9000      # só fora da AWS
TABA_BACKUP_S3_ACCESS_KEY=...                  # opcionais: sem elas vale a cadeia padrão da AWS
TABA_BACKUP_S3_SECRET_KEY=...
```

Confira num nó (pela porta de operação, que o Compose não publica; o `wget` do contêiner a alcança):

```
docker compose exec -T taba-1 wget -q -O - http://127.0.0.1:9090/node/backup
# {"s3Reachable":true,"pending":0,"outboxPending":0,…}
```

`s3Reachable: true` e `pending: 0` quer dizer que o que existe já está no bucket. **Com o S3 fora do ar o cluster funciona como sem o backup**; só se acumulam IDs no disco do nó.

## 2. Forçar o backup (flush), antes de uma manutenção

```
exemplos/backup-s3/flush.sh            # o cluster inteiro
exemplos/backup-s3/flush.sh --node     # só um nó
```

Por baixo é `POST /cluster/backup/flush` (ou `/node/backup/flush`). Cada nó faz **memória → disco → S3 → fila de envio (`outbox`) → S3**, sem desregistrar nem parar. Resposta de exemplo:

```json
{"complete": true, "nodes": [
  {"nodeId": "01a1…", "complete": true, "memoryMoved": 128, "segmentsUploaded": 4, "messagesUploaded": 128, "outboxPending": 0},
  {"nodeId": "01a2…", "complete": true, …},
  {"nodeId": "01a3…", "complete": true, …}]}
```

`"complete": false` com um nó em `"pending": true` quer dizer que ele estava lento ou fora do ar: **repita** o flush até `true`. Sem S3 configurado a resposta é `422 backup.not-configured`: ninguém acha que fez backup sem ter feito.

## 3. Parar e iniciar o cluster inteiro

```
docker compose exec -T taba-1 wget -q -O - --post-data='' http://127.0.0.1:9090/cluster/stop
# {"stopped":["…","…","…"],"stopList":true,"warnings":[], …}
docker compose stop                    # desliga os processos
…                                      # a manutenção
docker compose start                   # "iniciar" é subir os processos: a restauração é automática
```

- **`"stopList": true`** quer dizer que todos os nós mandaram tudo ao S3 e a lista dos nós vivos na parada foi gravada: na volta o primeiro nó restaura **sem esperar** o prazo de morte (`backup.dead-after`).
- **`"stopList": false`** com `warnings`: o S3 estava indisponível. O drain em disco foi feito, mas **o backup não foi enviado**; não desligue a infraestrutura até repetir o `stop` com o S3 de volta.
- Uma **queda** (sem `stop`) também se restaura: o primeiro nó espera os batimentos dos mortos vencerem (60 s) e restaura o que estava no bucket.

[`parar-e-iniciar.sh`](parar-e-iniciar.sh) faz o ciclo completo e **apaga os discos dos nós** no meio, para provar que as 20 mensagens voltam do bucket.

## O que esperar depois de restaurar

- Definições de tópico e tokens voltam por inteiro. As mensagens voltam **menos** as confirmadas (o diário as remove) e **mais** as que estavam no bucket.
- A defasagem (RPO) é a de `backup.delay` (mensagens ainda não enviadas) e `backup.meta.interval` (o estado da última janela): uma mensagem pode voltar entregável um pouco antes do prazo de invisibilidade. **O consumidor precisa ser idempotente** (*at-least-once*).
- Os nós voltam como **nós novos**, com id novo. A pasta de segredos (certificado do cluster e token raiz) **não** vai ao bucket: guarde-a à parte.

Mais: [`doc/pt-BR/23-operacao.md`](../../doc/pt-BR/23-operacao.md) (runbook) e [`doc/pt-BR/22-configuracao.md`](../../doc/pt-BR/22-configuracao.md) (todas as chaves).
