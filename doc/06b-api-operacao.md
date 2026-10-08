# API de operação

[← Índice](README.md) · [Visão geral das APIs](06-api.md) · [Convenções e DTOs padronizados](06d-api-convencoes.md)

## 8.1.1 API de operação (observabilidade)

Manual das rotas para **operadores, Prometheus e orquestradores**: métricas do nó, estado e condições do cluster, métricas dos tópicos, saúde e drain. Não são rotas para aplicações clientes. Detalhes, parâmetros e o significado de cada métrica estão em [11-observabilidade.md](11-observabilidade.md).

> **Porta:** `operations.port` ou, se não for definida, `api.port` ([06-api.md](06-api.md), seção 8.0). **Autenticação:** nenhuma; a proteção é de rede (porta própria, grupo de segurança) e, em complemento, `operations.allowed-ips`, e as rotas devem ficar numa camada de rede segura ([06-api.md](06-api.md), seção 8.0.1). Um IP fora da lista recebe `403` `ops.ip-not-allowed`. Sem `operations.port`, as rotas ficam na porta da API de cliente e, nesse caso, **não usam o token de acesso** ([06a](06a-api-cliente.md), seção 8.1.3).
>
> **Formato, cabeçalhos, `ErrorResponse`, `Page<T>`, códigos de status e catálogo de erros:** [06d-api-convencoes.md](06d-api-convencoes.md). As rotas `/node/drain` valem **só para o nó que recebe a chamada**, por isso o operador as chama no nó e não pelo balanceador.

### Resumo das rotas

| Verbo | Recurso | Rate limit | Sucesso |
|---|---|---|---|
| `GET` | `/metrics` | isenta | `200` texto Prometheus (só este nó) |
| `GET` | `/health/cluster` | isenta com `scope=node`; `ratelimit.ops.rps` com `scope=cluster` | `200` `ClusterStatus` |
| `GET` | `/metrics/topics` | `ratelimit.ops.rps` | `200` `Page<TopicStats>` (ou `Page<TopicStatsGroup>`) |
| `GET` | `/metrics/topics/{topic}` | `ratelimit.ops.rps` | `200` `TopicStatsDetail` |
| `GET` | `/metrics/topics/{topic}/groups` | `ratelimit.ops.rps` | `200` `Page<GroupStats>` |
| `POST` | `/node/drain` | admin | `202` `DrainStatus` (ou `200`) |
| `GET` | `/node/drain` | admin | `200` `DrainStatus` |
| `DELETE` | `/node/drain` | admin | `200` ou `202` `DrainStatus` |
| `GET` | `/health/live` | isenta | `200` `HealthResponse` |
| `GET` | `/health/ready` | isenta | `200` `HealthResponse` |

"Isenta" significa que a rota nunca responde `429`: são leves e chamadas por máquina em intervalo fixo, e um `429` no `/health/ready` faria o balanceador tirar o nó da rotação por engano ([12-limites-api.md](12-limites-api.md)).

## Métricas

### `GET /metrics`: endpoint Prometheus

Devolve as métricas **deste nó** no formato de texto do Prometheus (`text/plain; version=0.0.4`; OpenMetrics com `Accept: application/openmetrics-text`). É o modelo padrão de exporter: um alvo por nó, e o Prometheus ou o Collector descobre os nós (seção "Endpoint Prometheus" de [11-observabilidade.md](11-observabilidade.md)). O nó não consulta outros nós, e o total do cluster é feito pelo consumidor. As mesmas métricas saem por OTLP, que dispensa descoberta. Não há parâmetros.

| Status | Código | Quando |
|---|---|---|
| `200` | — | Corpo em texto; **não** é JSON. |
| `403` | `ops.ip-not-allowed` | IP fora de `operations.allowed-ips`. |
| `404` | `route.not-found` | `telemetry.prometheus.enabled` desligado. |

### `GET /metrics/topics`: métricas de tópicos, paginada

Lista **paginada** ([06a](06a-api-cliente.md), seção 8.1.2) com as métricas de cada tópico. A ordem é **sempre por nome**, e **não há `sort`**: as métricas mudam a todo instante e a paginação por chave não se sustentaria. Para achar os tópicos com problema, use `condition`.

**Parâmetros de query:**

| Parâmetro | Tipo | Padrão | Descrição |
|---|---|---|---|
| `limit` / `cursor` | — | — | Paginação por cursor. |
| `scope` | texto | `cluster` | `cluster` (consulta todos os nós e agrega, [11-observabilidade.md](11-observabilidade.md), "Visão do nó e visão do cluster") ou `node` (só os valores deste nó). |
| `tag.<chave>` | texto | — | Filtra por tag (`tag.time=financeiro`). |
| `condition` | texto | — | Filtra por condição do tópico (`backlog`, `stalled`...). É o caso de uso principal: "quais tópicos estão com problema?". |
| `fields` | lista separada por vírgula | todos | Restringe os campos de `TopicStats` devolvidos (`fields=name,depth,oldestAge`). `name` e `id` vêm sempre. |
| `groupBy` | texto | — | Nome de uma tag (`groupBy=time`). Devolve **uma linha por valor da tag**, em vez de uma por tópico (`TopicStatsGroup`). |

Resposta: `200`, `Page<TopicStats>` (ou `Page<TopicStatsGroup>` com `groupBy`). O resultado fica em cache por `stats.topics.cache-ttl` (a resposta traz `collectedAt`), e o filtro e o `limit` são aplicados **depois** do merge entre nós.

| Status | Código | Quando |
|---|---|---|
| `200` | — | — |
| `400` | `validation.invalid-cursor`, `validation.out-of-range`, `validation.failed`, `validation.unknown-field` | Inclusive `sort`, que a rota não aceita. |
| `403` / `429` | `ops.ip-not-allowed` / `rate-limit.exceeded` | — |

### `GET /metrics/topics/{topic}`: um tópico

Métricas detalhadas e condição de **um** tópico, pedindo aos nós só aquele tópico. Parâmetro: `scope` (`cluster` ou `node`). Resposta: `200`, `TopicStatsDetail`. Erros: `404` `topic.not-found`, `400`, `403`, `429`. Num Tópico Distribuidor, devolve só as métricas do distribuidor (`TopicStats.distributor`); as de fila se aplicam às filas destino ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.7).

### `GET /metrics/topics/{topic}/groups`: métricas por grupo

Métricas de **cada `groupId`** de um tópico, para achar o produtor barulhento ([11-observabilidade.md](11-observabilidade.md), "Métricas por grupo"). Cada nó conta só as mensagens de que é dono, então a soma entre nós é exata.

**Parâmetros de query:**

| Parâmetro | Tipo | Padrão | Descrição |
|---|---|---|---|
| `limit` / `cursor` | — | — | Paginação por cursor, ordenada por `groupId`, **sem `sort`**. |
| `groupId` | texto | — | Devolve só esse grupo. |
| `top` | inteiro | — | Os `N` maiores grupos por `messages`, **sem paginação** (resposta sem `nextCursor`). Não combina com `cursor`. |
| `scope` | texto | `cluster` | `cluster` ou `node`. |

Resposta: `200`, `Page<GroupStats>`. Erros: `404` `topic.not-found`, `400` (`validation.failed` em `top` com `cursor`), `403`, `429`. Um grupo sem mensagens e sem atividade na janela deixa de ser acompanhado e some da resposta.

## Drain

O **modo drain** prepara o nó para sair do cluster: grava todas as mensagens em disco, tira o nó da rotação e o **desregistra por conta própria**, sem votação ([04-cluster.md](04-cluster.md), seção 6.10). Estados (`drainState`): `active` → `draining` → `drained`.

### `POST /node/drain`: iniciar

Põe **este nó** em drain, sem encerrar o processo. O andamento fica em `GET /node/drain`. Já em drain (`draining` ou `drained`), responde `200` sem efeito.

| Status | Código | Quando |
|---|---|---|
| `202` | — | Drain iniciado. Corpo `DrainStatus` com `trigger: "api"`. |
| `200` | — | O nó já estava em drain; nada mudou. |
| `403` | `ops.ip-not-allowed` | — |

### `GET /node/drain`: consultar

Resposta: `200`, `DrainStatus`.

### `DELETE /node/drain`: cancelar ou voltar

Depende do estado:

| Estado do nó | Efeito | Status |
|---|---|---|
| `draining`, **antes** da saída do cluster | Cancela o drain. O nó volta a `active`, a gravar em memória, e continua membro. | `200` `DrainStatus` |
| `drained`, **depois** da saída | O nó **volta como nó novo**: ID novo, bootstrap completo, nova posição no anel. Antes de limpar o estado local, recupera as mensagens perdidas (seção 6.6 de [04-cluster.md](04-cluster.md)). `drainState` volta a `active`, e o estado do nó passa por `joining` e `syncing`. | `202` `DrainStatus` |
| `active` | Nada a cancelar. | `200` `DrainStatus` |
| Durante o desregistro | — | `409` `drain.conflict` (tente de novo quando estiver `drained`) |
| Processo já parando por sinal | — | `409` `drain.conflict` |

## Saúde

### `GET /health/live`

O processo está de pé e respondendo. Falhou, o orquestrador reinicia o processo. Resposta: `200`, `HealthResponse` com `health: "live"`.

### `GET /health/ready`

O nó está **apto a receber tráfego**: registrado no cluster (commit confirmado) e com **toda a sincronização concluída** (tópicos, tokens, blocos e banco de eventos, seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)). Em **drain** (`draining` ou `drained`) responde `503`, para o balanceador tirar o nó da rotação. Falhou, o balanceador deve tirar o nó da rotação.

| Status | Corpo | Quando |
|---|---|---|
| `200` | `HealthResponse` `{ "health": "ready" }` | Apto. |
| `503` | `HealthResponse` `{ "health": "not-ready", "reason": "..." }` | Não apto. `reason` diz por quê (abaixo). |

`/health/live` e `/health/ready` **não** devolvem `ErrorResponse`: o corpo é sempre `HealthResponse`, para o orquestrador poder ler o motivo sem tratar erros.

### `GET /health/cluster`: estado e condições

Estado do cluster, na família `/health`, para **pessoas e scripts**: condições, resumo e o estado de cada nó. **Não repete as métricas**, que se leem em `/metrics` e no OTLP.

| Parâmetro | Tipo | Padrão | Descrição |
|---|---|---|---|
| `scope` | texto | `cluster` | `cluster`: consulta **todos os nós** (pela rota interna `GET /cluster/status`, [06c](06c-api-cluster.md)) e deriva as condições do cluster. É *best-effort*: um nó que não responde aparece com `reachable: false` em vez de falhar a chamada inteira. `node`: leitura **local**, sem consultar outros nós, com as condições que o nó consegue derivar sozinho. |

No modo desenvolvimento devolve o mesmo nos dois `scope`, em formato de cluster de um nó só. Resposta: `200`, `ClusterStatus`.

| Status | Código | Quando |
|---|---|---|
| `200` | — | — |
| `400` | `validation.failed` | `scope` desconhecido. |
| `403` / `429` | `ops.ip-not-allowed` / `rate-limit.exceeded` | O `429` só vale com `scope=cluster`. |

## DTOs da API de operação

### `HealthResponse`

| Campo | Tipo | Descrição |
|---|---|---|
| `health` | texto | `live`, `ready` ou `not-ready`. |
| `reason` | texto | Só com `not-ready`: `bootstrapping` (ingresso em andamento), `syncing` (sincronização), `returning` (volta pós-exclusão), `draining`, `drained` ou `failed` (disco cheio ou banco corrompido, [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3). |

### `DrainStatus`

| Campo | Tipo | Descrição |
|---|---|---|
| `drainState` | texto | `active`, `draining` ou `drained`. |
| `trigger` | texto ou `null` | `signal` (`SIGTERM`/`SIGINT`) ou `api`; `null` em `active`. |
| `startedAt` | data ou `null` | Início do drain; `null` em `active`. |
| `pendingMessages` | inteiro | Mensagens **ainda em memória**. Chega a 0 quando o nó está `drained`. |

### `ClusterStatus`

Resposta de `GET /health/cluster`.

| Campo | Tipo | Descrição |
|---|---|---|
| `scope` | texto | `cluster` ou `node`, como pedido. |
| `collectedAt` | data | Instante da coleta. |
| `partial` | booleano | `true` se algum nó não respondeu (só em `scope=cluster`). |
| `worstCondition` | texto | A condição de **maior severidade** (`ok` se nenhuma). |
| `conditions` | lista de `ConditionEntry` | Todas as condições vigentes ([06d](06d-api-convencoes.md), seção 8.3.7; lista completa em [11-observabilidade.md](11-observabilidade.md), "Condição do cluster"). |
| `summary` | objeto | `members` (contagem por estado: `active`, `syncing`, `draining`, `suspect`), `membersVersion`, `quorumAvailable` (booleano) e `syncingBlocks`. Em `scope=node`, a visão deste nó. |
| `nodes` | lista | Um item por membro (um só em `scope=node`): `nodeId`, `reachable` (booleano) e `status` (`NodeStatus`, ou `null` se `reachable` é `false`). |

### `NodeStatus`

O estado de um nó, sem as métricas (essas ficam em `/metrics` e no OTLP). É também a resposta da rota interna `GET /cluster/status` ([06c](06c-api-cluster.md)), com a qual o cluster deriva as condições.

| Campo | Tipo | Descrição |
|---|---|---|
| `nodeId` | texto | ID do nó (GUID v7 gerado a cada início), o `service.instance.id` da telemetria. |
| `version` | texto | Versão do Taba. |
| `clusterName` / `environment` | texto | `service.namespace` e `deployment.environment.name` do recurso OpenTelemetry, quando configurados. |
| `mode` | texto | `production` ou `development`. |
| `state` | texto | `joining`, `active`, `syncing`, `returning` ou `failed` (disco cheio ou banco corrompido; `taba.node.state`). Sempre `active` em desenvolvimento, salvo `failed`. |
| `drainState` | texto | `active`, `draining` ou `drained`. |
| `startedAt`, `collectedAt` | data | Início do processo e instante da coleta. |
| `storage` | objeto | `memoryUsedBytes`, `memoryLimitBytes`, `memoryMessages`, `diskMessages`, `diskFreeBytes`. |
| `signals` | objeto | O que as condições do cluster precisam: `membersVersion`, `quorumAvailable`, `channels` (mapa `peer` → conectado, booleano), `syncingBlocks`, `blocksBelowReplication`, `blocksBelowQuorum` e `frozenNodes`. |

```json
{
  "scope": "cluster",
  "collectedAt": "2026-10-06T12:00:00.000Z",
  "partial": false,
  "worstCondition": "under-replicated",
  "conditions": [ { "condition": "under-replicated", "severity": "warning" } ],
  "summary": { "members": { "active": 3 }, "membersVersion": 7, "quorumAvailable": true, "syncingBlocks": 0 },
  "nodes": [
    {
      "nodeId": "01927f3a-0001-7000-8000-00000000000a",
      "reachable": true,
      "status": {
        "nodeId": "01927f3a-0001-7000-8000-00000000000a",
        "version": "0.9.0",
        "mode": "production",
        "state": "active",
        "drainState": "active",
        "startedAt": "2026-10-06T08:00:00.000Z",
        "collectedAt": "2026-10-06T12:00:00.000Z",
        "storage": { "memoryUsedBytes": 734003200, "memoryLimitBytes": 1073741824, "memoryMessages": 52110, "diskMessages": 0, "diskFreeBytes": 90194313216 }
      }
    }
  ]
}
```

### `TopicStats`

Os campos de cada grupo de [11-observabilidade.md](11-observabilidade.md), "Métricas por tópico". Os campos de fila **não** aparecem num distribuidor, e o inverso vale para `distributor`.

| Campo | Tipo | Descrição |
|---|---|---|
| `id`, `name`, `type`, `tags`, `createdAt` | — | Identificação. Em fila volátil, também `volatile` e `idleTimeout`. |
| `published`, `publishedBytes`, `publishErrors` | inteiro | Totais de publicação. |
| `publishRate` | número | Mensagens publicadas por segundo, na janela `memory.rate.window`. |
| `publishLatency`, `consumeLatency` | `{ p50, p95, p99 }` | Latências, em segundos. |
| `consumed`, `acked` | inteiro | Totais de entrega e de confirmação. |
| `consumeRate`, `ackRate` | número | Por segundo. |
| `visibleMessages`, `invisibleMessages` | inteiro | Prontas e em processamento. |
| `depth` | inteiro | `visibleMessages + invisibleMessages`. |
| `oldestMessageAt` | data | A mensagem mais antiga ainda não confirmada. **Ausente** se o tópico não tem mensagens. |
| `oldestAge` | número (s) | `agora − oldestMessageAt`. Ausente com `oldestMessageAt`. |
| `bytes` | inteiro | Volume acumulado. |
| `memoryMessages`, `diskMessages`, `memoryBytes`, `diskBytes`, `memoryShare` | número | Armazenamento. |
| `groups`, `noisyShare` | número | Grupos acompanhados e a parcela do maior. |
| `distributor` | objeto | **Só distribuidor:** `received` (publicações recebidas), `copies` (cópias geradas) e `copyErrors` (cópias que falharam). |
| `worstCondition` | texto | A condição do tópico de maior severidade (`ok`, `idle`, `backlog`, `stalled`, `noisy-group`, `group-stalled`, `memory-pressure`). |
| `conditions` | lista de `ConditionEntry` | Todas as condições vigentes. |
| `partial` | booleano | `true` se algum nó não respondeu: as somas viram **piso**. |
| `collectedAt` | data | Instante da coleta. |

### `TopicStatsDetail`

`TopicStats` mais `topGroups`: lista de `GroupStats` com os maiores grupos, até `stats.topics.top-groups` (padrão 10). Só em `GET /metrics/topics/{topic}`.

### `TopicStatsGroup` (resposta com `groupBy`)

| Campo | Tipo | Descrição |
|---|---|---|
| `tag` | texto | A chave usada em `groupBy`. |
| `value` | texto | O valor da tag que esta linha representa. |
| `topics` | inteiro | Quantidade de tópicos nesse valor. |
| demais campos de `TopicStats` | — | Quantidades **somadas** e datas pelo **menor** valor, como na visão do cluster. |

### `GroupStats`

| Campo | Tipo | Descrição |
|---|---|---|
| `groupId` | texto | O grupo. O contador compartilhado dos grupos acima de `delivery.fair.max-groups` aparece **sem** `groupId` e com `overflow: true`. |
| `messages`, `visibleMessages`, `invisibleMessages` | inteiro | Não confirmadas, prontas e em processamento. |
| `share` | número | Parcela do `depth` do tópico. |
| `band` | inteiro | Faixa de tamanho do grupo ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11). **Só** com `scope=node`. |
| `oldestMessageAt`, `oldestAge` | data, número | A mais antiga do grupo. |
| `publishRate`, `consumeRate`, `ackRate` | número | Taxas do grupo, na janela `memory.rate.window`. |
