---
título: API de operação
resumo: Manual das rotas para operador, Prometheus e orquestrador — saúde, métricas do nó e de tópicos, drain e backup em S3 — com parâmetros, DTOs e exemplos.
tipo: referência
status: estável
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/src/http/backup.rs, crates/taba-node/src/http/operations.rs, crates/taba-node/src/http/metrics, crates/taba-node/src/http/topic_stats, crates/taba-node/src/cluster/drain, crates/taba-node/src/cluster/health, crates/taba-node/src/cluster/state]
---

# API de operação

> Convenções comuns: [15-api-visao-geral.md](15-api-visao-geral.md). O significado das métricas e condições: [13-observabilidade.md](13-observabilidade.md). O que fazer diante delas: [23-operacao.md](23-operacao.md).

**Porta:** `operations.port` ou, se ausente, a **porta de cliente**. **Autenticação: nenhuma**; a proteção é de rede (porta própria, `operations.allowed-ips`). IP fora da lista: `403 ops.ip-not-allowed`. Sem `operations.port`, essas rotas **não usam o token** de cliente.

> As rotas `/node/drain` valem **só para o nó que recebe a chamada**: o operador as chama no nó (porta de operação direta), **não pelo balanceador**.

> **Um cliente com token tem cópias destas rotas na API de cliente** ([16-api-cliente.md](16-api-cliente.md)): `GET /cluster/status`, `/nodes/{nodeId}/metrics`, `/nodes/{nodeId}/drain`, `/nodes/{nodeId}/backup`..., endereçadas pelo `nodeId` (funcionam pelo balanceador) e para o **token raiz**. As rotas deste documento não mudam: sem token, para o monitoramento contínuo.

| Verbo | Rota | Rate limit | Sucesso |
|---|---|---|---|
| `GET` | `/health/live` | isenta | `200` `HealthResponse` |
| `GET` | `/health/ready` | isenta | `200` / `503` `HealthResponse` |
| `GET` | `/health/cluster` | isenta com `scope=node`; `ratelimit.ops.rps` com `scope=cluster` | `200` `ClusterStatus` |
| `GET` | `/metrics` | isenta | `200` texto Prometheus (só este nó) |
| `GET` | `/metrics/topics` | `ratelimit.ops.rps` | `200` `Page<TopicStats>` |
| `GET` | `/metrics/topics/{topic}` | idem | `200` `TopicStatsDetail` |
| `GET` | `/metrics/topics/{topic}/groups` | idem | `200` `Page<GroupStats>` |
| `POST` | `/node/drain` | admin | `202` (ou `200` se já em drain) |
| `GET` | `/node/drain` | admin | `200` `DrainStatus` |
| `DELETE` | `/node/drain` | admin | `200` ou `202` `DrainStatus` |
| `GET` | `/node/backup` | isenta | `200` `BackupStatus` (ou `422 backup.not-configured`) |
| `POST` | `/node/backup/flush` | admin | `200` `FlushReport` (ou `422 backup.not-configured`) |
| `POST` | `/cluster/backup/flush` | admin | `200` `ClusterFlushReport` (ou `422 backup.not-configured`) |
| `POST` | `/cluster/stop` | admin | `200` `StopReport` |

"Isenta" = nunca responde `429`: são leves e chamadas por máquina; um `429` no `/health/ready` faria o balanceador tirar o nó da rotação por engano.

---

## Saúde

### `GET /health/live`

O processo está de pé e respondendo. Falhou → o orquestrador **reinicia** o processo. `200 {"health":"live"}`.

### `GET /health/ready`

O nó está **apto a receber tráfego**: registrado, com a **sincronização concluída**, fora de bootstrap e de retorno. **Em drain** (`draining` ou `drained`) responde `503`.

| Status | Corpo |
|---|---|
| `200` | `{"health":"ready"}` |
| `503` | `{"health":"not-ready","reason":"…"}` |

`reason`: `bootstrapping` (entrando), `syncing` (sincronizando), `returning` (voltando depois de excluído), `draining`, `drained` ou `failed` (disco cheio ou banco corrompido). Os dois endpoints **não** devolvem `ErrorResponse`: o corpo é sempre `HealthResponse`, para o orquestrador ler o motivo sem tratar erros.

### `GET /health/cluster`

Estado do cluster para **pessoas e scripts**: condições, resumo e o estado de cada nó. **Não repete as métricas.**

| Parâmetro | Padrão | Descrição |
|---|---|---|
| `scope` | `cluster` | `cluster`: consulta **todos os nós** (`GET /cluster/status`), *best-effort*: nó que não responde aparece com `reachable: false`. `node`: leitura **local**, sem consultar outros nós |

Num nó só (modo desenvolvimento) os dois escopos são iguais. Erros: `400 validation.failed` (`scope` desconhecido), `403`, `429` (só com `scope=cluster`).

```json
{"collectedAt":"2026-10-13T17:13:10.812Z","partial":false,"scope":"cluster","worstCondition":"ok","conditions":[],
 "summary":{"members":{"active":3},"membersVersion":3,"quorumAvailable":true,"syncingBlocks":0},
 "nodes":[{"nodeId":"01a13640-7774-…","reachable":true,"status":{ "…": "NodeStatus" }}, …]}
```

Durante uma falha real (um nó derrubado com `kill`), a mesma rota mostrou: `conditions: [write-quorum-at-risk, member-unreachable, under-replicated]`, depois `[node-frozen]` (nó excluído).

**`ClusterStatus`:**

| Campo | Descrição |
|---|---|
| `scope`, `collectedAt` | Como pedido; instante da coleta |
| `partial` | `true` se algum nó não respondeu |
| `worstCondition` | A de maior severidade (`ok` se nenhuma) |
| `conditions` | Lista de `{condition, severity}` ([13-observabilidade.md](13-observabilidade.md)) |
| `summary` | `members` (contagem por estado), `membersVersion`, `quorumAvailable`, `syncingBlocks` |
| `nodes` | Um por membro: `nodeId`, `reachable`, `status` (`NodeStatus`, ou `null`) |

**`NodeStatus`:** `nodeId`, `version`, `clusterName`/`environment` (se configurados), `mode` (`production`/`development`), `state` (`joining`, `active`, `syncing`, `returning`, `failed`), `drainState`, `startedAt`, `collectedAt`, `storage` (`memoryUsedBytes`, `memoryLimitBytes`, `memoryMessages`, `diskMessages`, `diskFreeBytes`) e `signals` (`membersVersion`, `quorumAvailable`, `channels`, `syncingBlocks`, `blocksBelowReplication`, `blocksBelowQuorum`, `frozenNodes`).

---

## Métricas

### `GET /metrics`

As métricas **deste nó**, em texto do Prometheus (`text/plain; version=0.0.4`; `Accept: application/openmetrics-text` pede OpenMetrics). **Um alvo por nó**: o nó não consulta outros. Não há parâmetros. Erros: `403 ops.ip-not-allowed`; `404 route.not-found` se `telemetry.prometheus.enabled` for `false`. A lista de métricas está em [13-observabilidade.md](13-observabilidade.md).

### `GET /metrics/topics`

Lista **paginada** de métricas por tópico, **sempre por nome** (não há `sort`: as métricas mudam o tempo todo).

| Parâmetro | Descrição |
|---|---|
| `limit`, `cursor` | Paginação |
| `scope` | `cluster` (padrão) ou `node`. ⚠️ **Hoje `cluster` não agrega entre nós**: devolve a visão do nó que atende ([13-observabilidade.md](13-observabilidade.md)) |
| `tag.<chave>` | Filtra por tag |
| `condition` | Filtra por condição do tópico (`backlog`, `stalled`…): "quais tópicos estão com problema?" |
| `fields` | Restringe os campos (`fields=name,depth,oldestAge`); `name` e `id` vêm sempre |
| `groupBy` | Nome de uma tag: **uma linha por valor da tag** (`TopicStatsGroup`) em vez de uma por tópico |

### `GET /metrics/topics/{topic}`

Um tópico, em detalhe: `TopicStats` mais `topGroups` (até `stats.topics.top-groups`, padrão 10). Parâmetro `scope`. `404 topic.not-found`. Num distribuidor, só as métricas dele (`distributor`: `received`, `copies`, `copyErrors`).

### `GET /metrics/topics/{topic}/groups`

Métricas **por `groupId`** para achar o produtor barulhento. Paginada por `groupId` (sem `sort`).

| Parâmetro | Descrição |
|---|---|
| `limit`, `cursor` | Paginação |
| `groupId` | Só esse grupo |
| `top` | Os `N` maiores por `messages`, **sem paginação** (não combina com `cursor`: `400`) |
| `scope` | `cluster` ou `node` |

Um grupo sem mensagens e sem atividade na janela some da resposta. O contador compartilhado dos grupos acima de `delivery.fair.max-groups` vem **sem** `groupId` e com `overflow: true`.

**`TopicStats` (campos principais):** `id`, `name`, `type`, `tags`, `createdAt`, `published`, `publishedBytes`, `publishErrors`, `publishRate`, `publishLatency`/`consumeLatency` (`{p50,p95,p99}` em s), `consumed`, `acked`, `consumeRate`, `ackRate`, `visibleMessages`, `invisibleMessages`, `depth`, `oldestMessageAt`/`oldestAge` (ausentes sem mensagens), `bytes`, `memoryMessages`, `diskMessages`, `memoryBytes`, `diskBytes`, `memoryShare`, `groups`, `noisyShare`, `worstCondition`, `conditions`, `partial`, `collectedAt`.

**`GroupStats`:** `groupId`, `messages`, `visibleMessages`, `invisibleMessages`, `share`, `band` (só com `scope=node`), `oldestMessageAt`/`oldestAge`, `publishRate`, `consumeRate`, `ackRate`.

---

## Drain

Prepara o nó para **sair do cluster**: grava tudo em disco, sai da rotação e **se desregistra sozinho, sem votação** ([07-cluster.md](07-cluster.md), seção 7). Estados: `active` → `draining` → `drained`.

**`DrainStatus`:**

| Campo | Descrição |
|---|---|
| `drainState` | `active`, `draining` ou `drained` |
| `trigger` | `signal` (`SIGTERM`/`SIGINT`) ou `api`; `null` em `active` |
| `startedAt` | Início do drain; `null` em `active` |
| `pendingMessages` | Mensagens **ainda em memória**; chega a 0 quando `drained` |

### `POST /node/drain`: iniciar

Põe **este nó** em drain, **sem encerrar** o processo. `202` com `trigger: "api"`; `200` se já estava em drain (sem efeito).

### `GET /node/drain`: consultar

```bash
taba-cli drain status -o json     # ou GET http://nó:9090/node/drain
# {"drainState":"active","trigger":null,"startedAt":null,"pendingMessages":13}
# depois do start (4 s): {"drainState":"drained","trigger":"api","startedAt":"2026-10-13T17:24:55.438Z","pendingMessages":0}
```

### `DELETE /node/drain`: cancelar ou voltar

| Estado do nó | Efeito | Status |
|---|---|---|
| `draining`, antes da saída do cluster | **Cancela** o drain; o nó volta a `active` e continua membro | `200` |
| `drained`, depois da saída | O nó **volta como nó novo** (ID novo, bootstrap completo, recupera mensagens perdidas antes de limpar). `drainState` volta a `active` | `202` |
| `active` | Nada a cancelar | `200` |
| Durante o desregistro, ou processo já parando por sinal | — | `409 drain.conflict` |

Verificado num cluster de 3 nós: `drain start` levou o nó a `drained` em ~4 s e `/health/ready` passou a `503`; `drain cancel` o devolveu como nó novo, e em ~20 s o cluster voltou a **3 ativos, sem condições**.

> Enquanto em drain, o nó **recusa toda chamada de cliente** (inclusive ack): repassa a outro nó ou, sem outro, `503 cluster.node-draining` (PRB-005). Pendência **D75:** drain pedido pela API que estoura `drain.timeout` fica `draining` até alguém chamar `DELETE`. **D77:** o `Retry-After` do `503` do drain é fixo em 5 s.

## Backup em S3

O recurso é opcional ([33-backup-s3.md](33-backup-s3.md)). Estas rotas, como `/node/drain`, não usam token: a proteção é de rede. Sem `backup.s3.bucket`, `GET /node/backup` e os dois *flush* respondem `422` `backup.not-configured`; `POST /cluster/stop` ainda drena os nós.

> Estas rotas moram na **API de operação** (a porta de operação, ou a de cliente sem `operations.port`), **não** na porta do cluster: as chamadas entre nós usam `/cluster/backup/flush`, `/cluster/backup/stop` e `/cluster/backup/saved` na porta do cluster (mTLS), descritas em [18-api-cluster.md](18-api-cluster.md).

### `GET /node/backup`: o estado

```bash
curl -s http://nó:9090/node/backup
# {"s3Reachable":true,"lastError":null,"lastSuccessAt":"2026-10-14T15:00:03.512Z",
#  "outboxPending":0,"journalPending":0,"definitionsPending":0,"pending":0,
#  "segments":3,"segmentBytes":182011,"saved":412,"segmentsUploaded":9,
#  "messagesUploaded":412,"bodiesEvicted":0,"errors":0}
```

| Campo | Significado |
|---|---|
| `s3Reachable` / `lastError` / `lastSuccessAt` | O S3 respondeu na última chamada? Se não, o motivo e quando foi a última que deu certo |
| `outboxPending` | Mensagens que ainda não estão num segmento no bucket (inclui as que esperam `backup.delay`) |
| `journalPending` | Mudanças de estado ainda fora do diário |
| `definitionsPending` | Definições de tópico ou token ainda por gravar |
| `pending` | A soma dos três: **zero é "tudo salvo"** |
| `segments` / `segmentBytes` | Segmentos que **este nó** gravou e mantém no bucket |
| `saved` | Mensagens que este nó (dono ou réplica) sabe ler do bucket |
| `bodiesEvicted` | Corpos que a evicção apagou do disco desde a partida |

### `POST /node/backup/flush`: forçar o backup neste nó

Faz, em sequência, as três etapas: **memória → disco** (pelo caminho do drain, sem parar o nó), **disco → S3** (segmentos e diário, com prioridade) e **`outbox` → S3**. Responde quando as três terminam **ou** quando `backup.drain.timeout` estoura. Não desregistra nem para o nó: é seguro em produção. Durante o flush o nó grava as mensagens novas em disco (o limite de memória cai a zero) e volta ao normal ao terminar.

```bash
curl -s -X POST http://nó:9090/node/backup/flush
# {"complete":true,"memoryMoved":128,"memoryRemaining":0,"segmentsUploaded":4,
#  "messagesUploaded":128,"journalsUploaded":2,"outboxPending":0,"journalPending":0,
#  "timedOut":false,"s3Reachable":true,"nodeId":"01a1…"}
```

`complete: true` só quando nada ficou para trás (`memoryRemaining`, `outboxPending` e `journalPending` em zero e sem estouro de prazo).

### `POST /cluster/backup/flush`: forçar em todos os nós

O nó que recebe a chamada a repassa a todos os outros (pela porta do cluster) e junta as respostas. **Não muda a ordem de nenhum bloco.**

```bash
curl -s -X POST http://nó:9090/cluster/backup/flush
# {"complete":false,"nodes":[
#   {"nodeId":"01a1…","complete":true,"outboxPending":0, …},
#   {"nodeId":"01a2…","complete":false,"pending":true,"error":"prazo esgotado"}]}
```

Um nó lento ou fora do ar aparece com `"pending": true` e o motivo; `complete` do conjunto é `false` e o operador repete a chamada.

### `POST /cluster/stop`: parar o cluster com backup

Faz o drain em **todos** os nós pela mesma etapa de envio do encerramento por sinal; os outros em paralelo, e o nó que recebeu a chamada por último. O coordenador grava `cluster/stop.json` com os nós que pararam e a ordem em que pararam. Os processos **continuam rodando** em `drained`: parar os processos é com o orquestrador (`docker compose stop`).

```bash
curl -s -X POST http://nó:9090/cluster/stop
# {"stopped":["01a2…","01a3…","01a1…"],"stopList":true,"warnings":[],
#  "nodes":[{"nodeId":"01a2…","drained":true,"outboxPending":0}, …]}
```

`stopList: false` com `warnings` quando o S3 estava indisponível: o drain em disco foi feito, mas o backup **não foi enviado**. Um nó que não respondeu fica fora da lista e é tratado como morto ao subir.

## Fontes e provas

- `http/backup.rs` (as rotas do backup), `http/operations.rs` (as rotas e `HealthResponse`), `http/metrics`, `http/topic_stats`, `cluster/drain` (estados e transições), `cluster/health` (condições), `cluster/state`.
- Respostas copiadas de um nó real (um nó em desenvolvimento e um cluster de 3 nós pelo Compose).
- Casos de uso: UC-OPS (saúde, métricas e estatísticas), UC-DRN (drain), UC-LIM (limites das rotas de operação); veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/operations.rs`, `metrics.rs`, `topic_stats.rs`, `drain_cluster.rs`.
- Histórico: `06b-api-operacao.md`, `11-observabilidade.md`.
