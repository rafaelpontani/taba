---
title: Quick start
summary: Start Taba (one node or the three-node cluster), create a topic, publish, consume, confirm, and watch the cluster react to a node going down.
type: tutorial
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [docker-compose.yml, docker/haproxy.cfg, Dockerfile, crates/taba-node/src/cli/mod.rs, connectors/rust/cli]
---

# Quick start

In about ten minutes you start Taba, publish and consume a message, and see what happens when a node goes down. There are two paths:

| Path | When to use | What starts |
|---|---|---|
| **A. One node** (one-node profile) | Get to know the API, develop a client | 1 process, no replicas |
| **B. Three-node cluster** (Docker Compose) | See replication, failure and recovery | 3 nodes + a sample HAProxy |

> Prerequisites: **A** needs the `taba` binary (see [05-dependencies.md](05-dependencies.md) to build it) or Docker. **B** needs Docker with Compose.

## A. One node, in the one-node profile

```bash
# The data folder must end in "taba" (the node refuses any other name).
mkdir -p /tmp/taba-dev/taba
TABA_REPLICATION_FACTOR=1 TABA_STORAGE_DIR=/tmp/taba-dev/taba \
TABA_API_PORT=8080 TABA_OPERATIONS_PORT=9090 taba
```

The log warns that this is the one-node profile, for development and not for production. With no root token there is **no authentication**.

```bash
# 1. Create a queue topic
curl -s -X POST localhost:8080/topics -H 'Content-Type: application/json' \
  -d '{"name":"pedidos","type":"queue"}'

# 2. Publish: the response carries the message id (HTTP 201)
curl -s -X POST localhost:8080/topics/pedidos/messages -H 'Content-Type: application/json' \
  -d '{"pedido":9001}'
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}

# 3. Consume, waiting up to 1 s for a message (long polling)
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[{"id":"01a13635-a913-…","body":{"pedido":9001},"contentType":"application/json",
#   "receiptToken":"01a13635-aa19-…","deliveryCount":1,"publishedAt":"…","invisibleUntil":"…"}]}

# 4. Confirm (ack): HTTP 204. Use the id and the receiptToken from the previous response.
curl -s -i -X DELETE "localhost:8080/topics/pedidos/messages/<id>?receiptToken=<receiptToken>"

# 5. Consume again: the queue is empty
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[]}
```

If you do **not** do step 4, the message becomes available again when `invisibleUntil` passes (default: 120 s after delivery), with `deliveryCount` 2. This is the central delivery mechanism ([12-delivery.md](12-delivery.md)).

Questions to the node (**operations** port, 9090):

```bash
curl -s localhost:9090/health/ready          # {"health":"ready"}
curl -s localhost:9090/health/cluster        # state and conditions; "worstCondition":"ok"
curl -s localhost:9090/metrics/topics/pedidos # topic metrics (published, consumed, queue)
```

An error always has the same format ([19-errors-and-limits.md](19-errors-and-limits.md)):

```bash
curl -s localhost:8080/topics/nao-existe
# {"code":"topic.not-found","message":"O tópico não existe.","requestId":"…"}   (HTTP 404)
```

(The `message` text is the one the server returns today, in Portuguese; the `code` is what clients must use.)

### The same with `taba-cli`

`taba-cli` is the client command line ([20-cli.md](20-cli.md)). Point it at the node with variables:

```bash
export TABA_CLI_URL=http://localhost:8080
taba-cli topic create fila2 --type queue
echo '{"x":1}' | taba-cli publish fila2 --stdin
taba-cli consume fila2 --wait 1 --ack        # receives and confirms
taba-cli cluster status -o table
```

## B. Three-node cluster with Docker Compose

At the repository root:

```bash
docker compose up -d                # pulls the public images (rafaelpontani/taba) and starts everything
docker compose ps                   # the three nodes become "healthy" when they finish synchronizing
```

To build the image from the repository (for developers), add the `docker-compose.dev.yml` override:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d
```

The tag of the public images comes from `TABA_VERSION` (default `latest`): `TABA_VERSION=1.2 docker compose up -d`.

What Compose does, in order (`docker-compose.yml`):

1. The **`init`** service runs `taba init --out /secrets` once and creates, in a volume, the **cluster certificate** (mutual TLS between nodes) and the **root token**. On later runs it does nothing.
2. **HAProxy** (a sample) listens on port `8080` and balances the client API across the nodes that are ready. Any load balancer will do.
3. **`taba-0`** starts with `cluster.first-node=true`: the first node forms the cluster by itself.
4. The **replicas of the `taba` service** (two by default, `taba-1` and `taba-2`) wait for `taba-0` to become healthy, discover the cluster through HAProxy (`cluster.entry-address`), join and synchronize. For more nodes: `docker compose up -d --scale taba=4`; HAProxy discovers the replicas through DNS. Each replica has an anonymous data volume, deleted by `docker compose down -v`.

Use the Compose `taba-cli`, already pointed at the cluster and with the root token:

```bash
docker compose run --rm taba-cli cluster status -o table            # three nodes, no conditions
docker compose run --rm taba-cli topic create pedidos --type queue
echo '{"pedido": 9001}' | docker compose run --rm -T taba-cli publish pedidos --stdin
docker compose run --rm taba-cli consume pedidos --wait 5 --ack
```

The client API is also at `http://localhost:8080` for any HTTP client, now **with authentication**: send `Authorization: Bearer <root token>`. The token is in the `secrets` volume:

```bash
docker compose run --rm --entrypoint cat taba-cli /secrets/root-token
```

### Bring a node down and watch the cluster react

There are **two** ways to take a node away, and they are different. The replicas belong to the same service, so `docker compose stop` does not tell them apart; `docker/cluster.sh` accepts the names `taba-0` (the first node), `taba-1` and `taba-2` (`taba-N` is replica N of the `taba` service):

```bash
# 1. Normal stop: SIGTERM -> the node does DRAIN and leaves the cluster on its own (no vote)
docker/cluster.sh stop taba-2
docker compose run --rm taba-cli cluster status -o table   # already shows 2 active nodes, membersVersion went up
docker/cluster.sh start taba-2                               # comes back as a new node and synchronizes

# 2. Failure: SIGKILL -> the cluster has to DETECT the loss and vote
docker/cluster.sh kill taba-2
docker compose run --rm taba-cli cluster status              # in ~4 s: write-quorum-at-risk, member-unreachable, under-replicated
docker compose run --rm taba-cli cluster status              # in ~8 s: taba-2 excluded; the node-frozen condition remains
docker/cluster.sh start taba-2                               # comes back as a new node (new ID: no freezing) and synchronizes
```

What happens underneath ([07-cluster.md](07-cluster.md)): on a **failure**, the other nodes notice that the permanent channel with `taba-3` dropped, try to reconnect, open an exclusion **vote** and remove it from the member table; its messages already existed on other nodes (replication factor 3), so nothing that was confirmed is lost. On a **normal stop**, the node writes everything to disk and deregisters itself. In both cases, when it returns, the node joins **as a new node**, synchronizes the blocks and starts serving.

> ⚠️ **Do not bring down a second node before the first one returns.** With 3 nodes, losing two in quick succession leaves a single survivor (fewer than `cluster.vote.min-voters`, 2): the cluster goes into `quorum-lost` and the restarted nodes cannot rejoin. See [23-operations.md](23-operations.md).

![What docker compose up creates](../diagrams/img/en-US/compose-deployment.svg)

### Cleanup

```bash
docker compose down -v      # deletes containers and volumes (including the root token)
```

## Next steps

- Understand the vocabulary: [03-concepts.md](03-concepts.md).
- Choose the topic type: [09-topics.md](09-topics.md).
- All the routes: [16-client-api.md](16-client-api.md).
- Run in production: [23-operations.md](23-operations.md).

## Sources and evidence

- `docker-compose.yml`, `docker/haproxy.cfg`, `Dockerfile`, `crates/taba-node/src/cli/mod.rs` (`taba init`).
- The examples of path A were run against a real one-node profile (responses copied from the run).
- `docker/validate.sh` automates the path B script end to end.
- Use cases: UC-DEV (one-node profile), UC-ING (node join), UC-CLI (`taba-cli`); see [26a](26a-implemented-use-cases.md).
