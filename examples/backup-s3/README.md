# S3 backup: configuration and operation

Examples for the optional S3 backup ([`doc/pt-BR/33-backup-s3.md`](../../doc/pt-BR/33-backup-s3.md)): the configuration with MinIO and with AWS, and a walkthrough of **forcing a backup (flush)**, **stopping** and **starting** the cluster.

| File | What it is |
|---|---|
| [`taba-minio.yml`](taba-minio.yml) | Node configuration with MinIO (or another S3-compatible store), every setting commented |
| [`taba-aws.yml`](taba-aws.yml) | Configuration on AWS (credentials from the default chain: role, IAM, profile) and the minimum permissions |
| [`flush.sh`](flush.sh) | Forces the backup and checks that nothing is left pending |
| [`stop-and-start.sh`](stop-and-start.sh) | Stops the cluster with backup, wipes the nodes' disks and starts it again: everything comes back from S3 |

The `docker-compose.yml` at the repository root **already starts a MinIO** and turns the backup on in all nodes (`TABA_BACKUP_S3_*`). MinIO console: <http://localhost:9001> (`taba` / `taba-minio-secret`, demo only).

## 1. Turn it on

With the root Compose file: `docker compose up -d`. With your own cluster, set the bucket (and, outside AWS, the `endpoint` and the keys) in the configuration file or in the variables:

```
TABA_BACKUP_S3_BUCKET=taba
TABA_BACKUP_S3_ENDPOINT=http://minio:9000      # only outside AWS
TABA_BACKUP_S3_ACCESS_KEY=...                  # optional: without them the default AWS chain applies
TABA_BACKUP_S3_SECRET_KEY=...
```

Check it on a node (through the operations port, which Compose does not publish; the container's `wget` reaches it):

```
docker compose exec -T taba-0 wget -q -O - http://127.0.0.1:9090/node/backup
# {"s3Reachable":true,"pending":0,"outboxPending":0,…}
```

`s3Reachable: true` and `pending: 0` mean everything that exists is already in the bucket. **With S3 down the cluster works as if there were no backup**; only IDs pile up on the node's disk.

## 2. Force the backup (flush), before maintenance

```
examples/backup-s3/flush.sh            # the whole cluster
examples/backup-s3/flush.sh --node     # a single node
```

Underneath it is `POST /cluster/backup/flush` (or `/node/backup/flush`). Each node does **memory → disk → S3 → send queue (`outbox`) → S3**, without deregistering or stopping. Sample response:

```json
{"complete": true, "nodes": [
  {"nodeId": "01a1…", "complete": true, "memoryMoved": 128, "segmentsUploaded": 4, "messagesUploaded": 128, "outboxPending": 0},
  {"nodeId": "01a2…", "complete": true, …},
  {"nodeId": "01a3…", "complete": true, …}]}
```

`"complete": false` with a node at `"pending": true` means it was slow or down: **repeat** the flush until it is `true`. Without S3 configured the response is `422 backup.not-configured`: nobody believes they made a backup without having made one.

## 3. Stop and start the whole cluster

```
docker compose exec -T taba-0 wget -q -O - --post-data='' http://127.0.0.1:9090/cluster/stop
# {"stopped":["…","…","…"],"stopList":true,"warnings":[], …}
docker compose stop                    # shuts the processes down
…                                      # the maintenance
docker compose start                   # "start" means starting the processes: restoration is automatic
```

- **`"stopList": true`** means every node sent everything to S3 and the list of nodes alive at the stop was written: on the way back the first node restores **without waiting** for the death deadline (`backup.dead-after`).
- **`"stopList": false`** with `warnings`: S3 was unavailable. The drain to disk was done, but **the backup was not sent**; do not shut the infrastructure down until you repeat the `stop` with S3 back.
- A **crash** (no `stop`) is also restored: the first node waits for the dead nodes' heartbeats to expire (60 s) and restores what was in the bucket.

[`stop-and-start.sh`](stop-and-start.sh) runs the whole cycle and **wipes the nodes' disks** in the middle, to prove the 20 messages come back from the bucket.

## What to expect after a restore

- Topic definitions and tokens come back whole. Messages come back **minus** the acknowledged ones (the journal removes them) and **plus** the ones that were in the bucket.
- The lag (RPO) is that of `backup.delay` (messages not yet sent) and `backup.meta.interval` (the state of the last window): a message can come back deliverable a little before its invisibility deadline. **The consumer must be idempotent** (*at-least-once*).
- Nodes come back as **new nodes**, with a new id. The secrets folder (cluster certificate and root token) is **not** sent to the bucket: keep it separately.

More: [`doc/pt-BR/23-operacao.md`](../../doc/pt-BR/23-operacao.md) (runbook) and [`doc/pt-BR/22-configuracao.md`](../../doc/pt-BR/22-configuracao.md) (every key).
