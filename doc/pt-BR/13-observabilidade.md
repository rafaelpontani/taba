---
título: Observabilidade
resumo: Como monitorar o Taba — OpenTelemetry (OTLP) e Prometheus, métricas de nó e de tópico, condições do cluster e do tópico, health checks, traces, logs e eventos.
tipo: referência
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/telemetry, crates/taba-node/src/http/metrics, crates/taba-node/src/http/topic_stats, crates/taba-node/src/http/operations.rs, crates/taba-node/src/cluster/health, crates/taba-node/src/config/otlp.rs]
---

# Observabilidade

> Leia antes: [04-arquitetura.md](04-arquitetura.md). Os parâmetros citados estão em [22-configuracao.md](22-configuracao.md); o que fazer diante de cada condição, em [23-operacao.md](23-operacao.md).

## Visão geral

A telemetria do Taba é **OpenTelemetry** (métricas, traces e logs). Há duas saídas para máquinas e uma leitura para pessoas:

| Saída | Modelo | Conteúdo | Uso típico |
|---|---|---|---|
| **OTLP** | *push*: cada nó envia o que é seu | Métricas, traces e logs | OpenTelemetry Collector ou qualquer backend OTLP (o Prometheus aceita OTLP) |
| **`GET /metrics`** | *pull*, **só deste nó** | Só métricas | Prometheus e compatíveis, ou o Collector com o receptor `prometheus` |
| **`GET /health/cluster`**, **`/metrics/topics/*`** | sob demanda, JSON | Estado, condições e métricas de tópico | Operador e scripts, diagnóstico sem backend |

OTLP e `/metrics` saem dos **mesmos instrumentos**: as métricas são as mesmas, só mudam os nomes no formato Prometheus.

**Nenhum nó consulta outro para montar uma métrica.** Cada nó reporta o seu trabalho e a sua visão do cluster; o total do cluster é feito pelo backend (PromQL ou o Collector). Não há lista de alvos central nem agregação no servidor. (A exceção é o JSON de `/health/cluster` e `/metrics/topics`, que consultam os nós sob demanda.)

![Caminhos da telemetria](../diagrams/img/pt-BR/observability-flow.svg)

A telemetria tem **dois níveis**, tratados de formas diferentes:

| Nível | Cardinalidade | OTLP e `/metrics` | Rotas de consulta |
|---|---|---|---|
| **Nó e cluster** | Limitada (nós, pares, rotas) | Sempre | `/health/cluster` |
| **Tópico** | **Ilimitada** (tópicos × valores de tags) | **Opt-in**, com agregação e teto | `/metrics/topics`, `/metrics/topics/{topic}` |

Por isso as métricas de tópico ficam em rotas próprias, paginadas: o custo de telemetria não deve crescer com o número de tópicos.

## Convenções (OpenTelemetry)

| Regra | Como o Taba cumpre |
|---|---|
| Namespace próprio | O que é do Taba começa com `taba.` |
| Convenção semântica quando existe | `http.server.*`, `process.*`, `messaging.*`. Só o que não tem nome padrão é `taba.*` |
| `taba.cluster.*` | A **visão do cluster** (membros, quórum, canais, votação): cada nó reporta a sua; no backend use `max`/`min`, **não** `sum`. As demais `taba.*` são trabalho do nó e **se somam** |
| Unidades (UCUM) | Sem unidade no nome: `s`, `By`, e contagens entre chaves (`{message}`, `{flag}`). `1` só para razões |
| Instrumentos | `updowncounter`: sobe e desce e se soma entre nós; `gauge`: não se soma (limite, idade, estado); `counter`: só cresce; `histogram`: distribuição |
| Recurso | `service.name=taba`, `service.namespace` (nome do cluster), `service.instance.id` (ID do nó, novo a cada partida), `service.version`, `deployment.environment.name` |
| Atributos | De cardinalidade limitada: `http.route` é sempre o **template** (`/topics/{topic}/messages`), nunca o caminho; o **ID da mensagem nunca é atributo de métrica** (só de trace e log) |

## Como ligar um coletor

Aponte o nó para o endpoint OTLP com as **variáveis padrão `OTEL_*`**. Sem `OTEL_EXPORTER_OTLP_ENDPOINT`, o OTLP fica **desligado**.

| Variável | Para quê |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Endereço do coletor (ex.: `http://coletor:4317`) |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` (padrão) ou `http/protobuf` |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_CERTIFICATE`… | Autenticação e TLS do backend |
| `OTEL_METRIC_EXPORT_INTERVAL` | Intervalo de envio das métricas, em ms (padrão 60000) |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | Amostragem de traces (padrão `parentbased_traceidratio`, fração **0,1**, provisório: D117) |
| `OTEL_RESOURCE_ATTRIBUTES` | `service.namespace=<cluster>,deployment.environment.name=<ambiente>` |

**Três formas de usar**, da mais simples à mais manual:

1. **Coletor (recomendada).** Cada nó empurra por OTLP; o Collector recebe e encaminha:

   ```yaml
   receivers:
     otlp:
       protocols: { grpc: {}, http: {} }
   processors:
     batch: {}
   exporters:
     otlphttp: { endpoint: https://backend.exemplo }
   service:
     pipelines:
       metrics: { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
       traces:  { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
       logs:    { receivers: [otlp], processors: [batch], exporters: [otlphttp] }
   ```

2. **Prometheus direto**: recebe OTLP (`--web.enable-otlp-receiver`) ou raspa `GET /metrics` de cada nó.
3. **Só `/health/cluster`**: sem nada instalado, o JSON basta para diagnóstico.

> **Limites da exportação OTLP (PRB-050):** o nó lê só o essencial das variáveis `OTEL_*`: o prazo de cada envio é fixo (10 s) e as variáveis por sinal (`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` etc.) podem não ser lidas. As **métricas OTLP são montadas à mão** (com `opentelemetry-proto`), porque o SDK não aceita dados agregados de fora.

## Métricas de nó

Exportadas por **cada nó**, sempre, via OTLP e em `GET /metrics`. A lista abaixo é gerada do registro do código (`telemetry/metrics/defs.rs`): é o que o nó **de fato** registra (84 métricas). Atributos usuais: `http.route`, `http.request.method`, `http.response.status_code`, `server.port` (qual das três portas) e os `taba.*` (`taba.phase`, `taba.outcome`, `taba.peer`…).


#### `process.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `process.uptime` | gauge | `s` | Tempo desde o início do processo. |
| `process.cpu.utilization` | gauge | `1` | Uso de CPU do processo, de 0 a 1 do total de núcleos, desde a leitura anterior. |
| `process.memory.usage` | updowncounter | `By` | Memória residente do processo. |
| `process.thread.count` | updowncounter | `{thread}` | Threads do sistema operacional do processo. |
| `process.open_file_descriptor.count` | updowncounter | `{file}` | Descritores de arquivo abertos. |

#### `taba.node.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.node.connections` | updowncounter | `{connection}` | Conexões HTTP abertas, por porta. |
| `taba.node.state` | gauge | `{flag}` | 1 no estado atual do nó. |
| `taba.node.drain.state` | gauge | `{flag}` | 1 no estado do drain do nó. |
| `taba.node.drain.pending` | updowncounter | `{message}` | Mensagens ainda em memória (chega a 0 quando o nó está drained). |

#### `taba.storage.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.storage.memory.usage` | updowncounter | `By` | Memória ocupada por corpos de mensagem. |
| `taba.storage.memory.limit` | gauge | `By` | Limite de memória de mensagens (memory.limit). |
| `taba.storage.memory.messages` | updowncounter | `{message}` | Mensagens mantidas em memória. |
| `taba.storage.disk.messages` | updowncounter | `{message}` | Mensagens em disco. |
| `taba.storage.disk.free` | gauge | `By` | Espaço livre no volume de storage.dir. |
| `taba.storage.overflow` | counter | `{message}` | Mensagens que foram da memória para o disco por falta de espaço. |
| `taba.storage.reads` | counter | `{read}` | Leituras de mensagem, por onde foram atendidas (memória ou disco). |
| `taba.storage.disk.errors` | counter | `{error}` | Falhas de entrada e saída em disco. |

#### `http.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `http.server.request.duration` | histogram | `s` | Latência das requisições recebidas; a contagem é o total de requisições. |
| `http.server.active_requests` | updowncounter | `{request}` | Requisições em andamento. |
| `http.server.request.body.size` | histogram | `By` | Tamanho do corpo das requisições (quando o tamanho é conhecido). |
| `http.server.response.body.size` | histogram | `By` | Tamanho do corpo das respostas (quando o tamanho é conhecido). |

#### `taba.ratelimit.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.ratelimit.rejected` | counter | `{request}` | Requisições recusadas com 429. |

#### `taba.auth.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.auth.requests` | counter | `{request}` | Autenticação por token: aceitas, recusadas com 401 e recusadas com 403. |
| `taba.auth.tokens` | gauge | `{token}` | Tokens de acesso que este nó conhece. |

#### `taba.delivery.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.delivery.ready` | updowncounter | `{message}` | Mensagens prontas para entrega neste nó (todos os tópicos). |
| `taba.delivery.fair.wait` | histogram | `s` | Tempo entre a publicação e a primeira entrega, por faixa do grupo. |
| `taba.delivery.redeliveries` | counter | `{message}` | Entregas de mensagens já entregues antes. |
| `taba.delivery.dead_lettered` | counter | `{message}` | Mensagens enviadas à dead-letter por atingirem maxDeliveries. |
| `taba.delivery.longpoll.waiting` | updowncounter | `{request}` | Chamadas de long polling aguardando neste nó. |
| `taba.delivery.longpoll.outcome` | counter | `{request}` | Como terminaram as chamadas de long polling. |
| `taba.delivery.take` | counter | `{request}` | Pedidos de entrega a outro dono (take), enviados e recebidos. |

#### `taba.retention.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.retention.expired` | counter | `{message}` | Mensagens descartadas por ttl. |

#### `taba.fifo.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.fifo.deduplicated` | counter | `{message}` | Publicações em fila FIFO descartadas por deduplicação. |
| `taba.fifo.groups.busy` | updowncounter | `{group}` | Grupos de filas FIFO com mensagem em voo neste nó. |
| `taba.fifo.forward` | counter | `{request}` | Publicações FIFO repassadas ao primário do grupo. |

#### `taba.cluster.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.cluster.members` | gauge | `{node}` | Membros na tabela deste nó, por estado. |
| `taba.cluster.members.version` | gauge | `{version}` | Versão (época) da tabela de membros; divergência entre nós indica propagação incompleta. |
| `taba.cluster.membership.changes` | counter | `{change}` | Registros e desregistros de nós, por fase. |
| `taba.cluster.bootstrap.failures` | counter | `{failure}` | Falhas de bootstrap deste nó, por motivo. |
| `taba.cluster.channel.notifications` | counter | `{message}` | Avisos de mensagem nova enviados e recebidos. |
| `taba.cluster.forwarded` | counter | `{request}` | Chamadas de cliente repassadas a outro nó (drain), por resultado. |
| `taba.cluster.channel.connections` | updowncounter | `{connection}` | Canais abertos com outros nós (esperado: nós do cluster − 1). |
| `taba.cluster.channel.state` | gauge | `{flag}` | 1 se o canal com o peer está conectado, 0 se caído. |
| `taba.cluster.channel.rtt` | histogram | `s` | Latência da chamada do canal (sinal de vida e quadros), por peer. |
| `taba.cluster.channel.reconnects` | counter | `{attempt}` | Tentativas de reconexão do canal e resultado. |
| `taba.cluster.channel.queue.out` | updowncounter | `{signal}` | Quadros na fila de saída para o peer. |
| `taba.cluster.channel.queue.in` | updowncounter | `{batch}` | Mensagens do peer lidas do canal e ainda não processadas (fila de entrada). |
| `taba.cluster.channel.queue.dropped` | counter | `{signal}` | Quadros substituídos, descartados por idade ou por estouro da fila de saída. |
| `taba.cluster.definitions.conflicts` | counter | `{conflict}` | Definições de tópico ou token recebidas com a mesma versão e conteúdo diferente do que o nó tem. |
| `taba.cluster.vote` | counter | `{vote}` | Votações de exclusão abertas ou votadas e o resultado. |
| `taba.cluster.freezing` | gauge | `{node}` | Nós atualmente em período de freezing. |
| `taba.cluster.tls.expires_in` | gauge | `s` | Tempo até a expiração (notAfter) do certificado do cluster em uso. |
| `taba.cluster.quorum.available` | gauge | `{flag}` | 1 se há nós saudáveis suficientes para votar (cluster.vote.min-voters). |
| `taba.cluster.condition` | gauge | `{flag}` | 1 para cada condição vigente do cluster. |
| `taba.cluster.recover` | counter | `{message}` | Mensagens recuperadas na volta de um nó, por direção. |

#### `taba.ring.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.ring.virtual_nodes` | gauge | `{vnode}` | Nós virtuais deste nó no anel (ring.virtual-nodes). |
| `taba.ring.blocks` | updowncounter | `{block}` | Blocos que o nó atende, por papel e integridade. |

#### `taba.write.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.write.coordinated` | counter | `{message}` | Operações sobre mensagens coordenadas por este nó (publicação, invisibilidade, ack, dead-letter). |
| `taba.write.retries` | counter | `{retry}` | Retries por fase e por nó de destino. |
| `taba.write.rollbacks` | counter | `{rollback}` | Rollbacks enviados por falha em fechar o quórum (fase 1). |
| `taba.write.commit_timeouts` | counter | `{timeout}` | Vezes que write.commit.timeout esgotou sem commit confirmado numa réplica. |
| `taba.write.received` | counter | `{message}` | Registros, commits e rollbacks recebidos. |
| `taba.write.pending` | updowncounter | `{message}` | Registros pendentes (sem commit). |
| `taba.write.pending.expired` | counter | `{message}` | Registros descartados por expiração (sem commit nem rollback). |

#### `taba.sync.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.sync.blocks.active` | updowncounter | `{block}` | Blocos em sincronização agora (entrega suspensa nesses blocos). |
| `taba.sync.duration` | histogram | `s` | Tempo para tornar um bloco íntegro. |
| `taba.sync.ids.listed` | counter | `{id}` | IDs consolidados (união) nas listas de origem. |
| `taba.sync.messages.pulled` | counter | `{message}` | Mensagens obtidas por nó de origem. |
| `taba.sync.bytes` | counter | `By` | Volume de corpo transferido na sincronização. |
| `taba.sync.log.events` | updowncounter | `{event}` | Eventos no banco de eventos (sync.db) ainda por processar. |
| `taba.sync.log.bytes` | gauge | `By` | Tamanho do banco de eventos em disco. |
| `taba.sync.phase` | gauge | `{flag}` | 1 na fase atual da sincronização do nó. |
| `taba.sync.failures` | counter | `{failure}` | Falhas de sincronização. |

#### `taba.recover.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.recover.messages` | counter | `{message}` | Mensagens enviadas, aplicadas ou ignoradas pela recuperação do nó que volta. |

#### `taba.topic.*`

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `taba.topic.published` | counter | `{message}` | Mensagens publicadas no tópico. |
| `taba.topic.consumed` | counter | `{message}` | Mensagens entregues em GET. |
| `taba.topic.acked` | counter | `{message}` | Confirmações (ack) do tópico. |
| `taba.topic.visible` | updowncounter | `{message}` | Mensagens prontas para entrega. |
| `taba.topic.invisible` | updowncounter | `{message}` | Mensagens em processamento. |
| `taba.topic.depth` | updowncounter | `{message}` | Mensagens ainda não confirmadas. |
| `taba.topic.oldest_timestamp` | gauge | `s` | Segundos Unix da mensagem não confirmada mais antiga. |
| `taba.topic.memory_bytes` | updowncounter | `By` | Bytes do tópico em memória. |
| `taba.topic.disk_bytes` | updowncounter | `By` | Bytes do tópico em disco. |
| `taba.topic.condition` | gauge | `{flag}` | 1 para cada condição vigente do tópico. |


> **Só no Linux:** `process.memory.usage`, `process.thread.count`, `process.open_file_descriptor.count` e `process.cpu.utilization` leem `/proc` (PRB-026).

### Nomes no Prometheus

| Regra | Exemplo |
|---|---|
| Pontos viram `_` | `taba.storage.memory.limit` → `taba_storage_memory_limit_bytes` |
| Unidade vira sufixo | `s` → `_seconds`; `By` → `_bytes`; `1` → `_ratio` |
| Unidade entre chaves some | `taba.cluster.quorum.available` → `taba_cluster_quorum_available` |
| Contadores ganham `_total` | `taba.write.retries` → `taba_write_retries_total` |
| Histogramas viram `_bucket`/`_sum`/`_count` | `http.server.request.duration` → `http_server_request_duration_seconds_bucket` |
| Recurso vira `job`, `instance`, `target_info` | `service.instance.id` → `instance` |

Um alvo por nó: o Prometheus guarda uma série por nó. O total sai por agregação (`sum(rate(taba_write_coordinated_total[5m]))` soma trabalho; `max(taba_cluster_members)` pega a visão do cluster). Somar no servidor quebraria os contadores.

### Exemplos de alertas (PromQL), a calibrar

| Alerta | Expressão |
|---|---|
| Nó fora do ar | `up{job="taba"} == 0` |
| Cluster sem quórum | `min(taba_cluster_quorum_available) == 0` |
| Tabelas de membros divergentes | `max(taba_cluster_members_version) != min(taba_cluster_members_version)` |
| Memória perto do limite | `taba_storage_memory_usage_bytes / taba_storage_memory_limit_bytes > 0.9` |
| Falhas de escrita | `sum(rate(taba_write_coordinated_total{taba_outcome="failed"}[5m])) > 0` |
| Certificado do cluster perto de expirar | `min(taba_cluster_tls_expires_in_seconds) < 30 * 86400` |

## Condições do cluster

`GET /health/cluster` devolve uma **lista de condições** (não um estado único), derivada dos sinais de cada nó. Também existe como a métrica `taba.cluster.condition`.

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo |
| `member-unreachable` | aviso | Canal com algum nó caído ou em reconexão, ainda antes da votação |
| `node-frozen` | aviso | Nó(s) em período de freezing |
| `syncing` | aviso | Blocos em sincronização; a entrega está suspensa neles |
| `under-replicated` | aviso | Blocos com menos réplicas íntegras que `replication.factor`, mas **no ou acima de** `write.quorum` — transitório e normal |
| `write-quorum-at-risk` | **crítica** | Blocos com réplicas íntegras **abaixo de** `write.quorum`, ou com o **primário** fora do ar e ainda na tabela: escritas podem falhar até a exclusão do nó |
| `quorum-lost` | **crítica** | Menos nós saudáveis que `cluster.vote.min-voters`: nenhuma exclusão se efetiva. **Não** é sobre durabilidade de mensagens |
| `membership-divergent` | **crítica** | Tabelas de membros com **versões diferentes** entre nós |
| `storage-pressure` | aviso | Espaço livre de `storage.dir` abaixo de `stats.disk.free-min` (sem valor, nunca dispara) |
| `upgrade-stalled` | aviso | A atualização que retira versões anteriores parou num nó que não terminou o drain em `drain.timeout` mais 30 s |

> **Como o nó calcula (provisório, D102–D104).** *Saudável* é este nó mais os pares com canal conectado. `quorum-lost` vale quando os saudáveis não alcançam `min(cluster.vote.min-voters, membros)` (com um nó só não há quem excluir). Uma réplica é *íntegra* quando o dono do bloco está `active` na tabela e o canal com ele está de pé. `under-replicated` aparece quando há mais blocos abaixo da replicação que abaixo do quórum; `syncing` quando algum dono de bloco está `syncing`. `membership-divergent` compara a versão da tabela de cada nó.

A degradação de replicação é propriedade do **cluster**, não do tópico: como os blocos vêm do ID da mensagem, as mensagens de um tópico se espalham por todos os blocos.

### Estado e resposta de `/health/cluster`

```bash
curl -s localhost:9090/health/cluster
# {"collectedAt":"…","conditions":[],"nodes":[{"nodeId":"…","reachable":true,"status":{
#    "drainState":"active","mode":"development","state":"active","version":"0.1.0",
#    "signals":{"blocksBelowQuorum":0,"blocksBelowReplication":0,"quorumAvailable":true,…},
#    "storage":{"diskFreeBytes":9322598400,"diskMessages":0,"memoryMessages":0,…}}}],
#  "partial":false,"scope":"cluster","summary":{"members":{"active":1},"membersVersion":0,
#  "quorumAvailable":true,"syncingBlocks":0},"worstCondition":"ok"}
```

- **`scope=cluster`** (padrão): consulta **todos os nós** (`GET /cluster/status`) e devolve o estado de cada um, um resumo e as condições. Nó que não responde aparece com `reachable: false` (*best-effort*).
- **`scope=node`:** só o nó que atende, sem consultar outros.
- `worstCondition` é a de maior severidade; `partial` indica nós que não responderam.

## Métricas de tópico

Ficam **fora** da exportação padrão e são lidas por rotas dedicadas ([17-api-operacao.md](17-api-operacao.md)):

| Rota | Descrição |
|---|---|
| `GET /metrics/topics` | Lista **paginada** com as métricas resumidas de cada tópico |
| `GET /metrics/topics/{topic}` | Detalhe de **um** tópico, com os maiores grupos |
| `GET /metrics/topics/{topic}/groups` | Um grupo por linha |

Exemplo (modo desenvolvimento):

```bash
curl -s localhost:9090/metrics/topics/pedidos
# {"ackRate":0.0033,"acked":1,"consumed":1,"depth":0,"invisibleMessages":0,"visibleMessages":0,
#  "memoryMessages":0,"diskMessages":0,"published":1,"publishedBytes":15,
#  "publishLatency":{"p50":0.01,"p95":0.01,"p99":0.01},"consumeLatency":{…},
#  "groups":0,"topGroups":[],"noisyShare":0.0,"conditions":[],"worstCondition":"ok",…}
```

### Visão do nó e visão do cluster

| Valor | Visão do **nó** | Visão do **cluster** |
|---|---|---|
| `visibleMessages`, `invisibleMessages`, `depth` | Só das mensagens de que **este nó é o primário** | **Soma** de todos os nós |
| `oldestMessageAt` | A mais antiga entre as de que este nó é primário | **A mais antiga** entre todos (nó sem mensagens do tópico não entra; ausente, nunca zero) |

**Só o primário conta**: senão a soma seria multiplicada pelo fator de replicação. O padrão é a visão do **cluster**; `scope=node` dá a do nó. Nó que não responde fica de fora e o resultado é marcado `partial` (as somas viram **piso**). Os valores são uma **fotografia aproximada**, não um valor transacional. Com **um nó só**, `scope=cluster` e `scope=node` coincidem e o resultado nunca é `partial`.

> ⚠️ **Limite atual (verificado num cluster de 3 nós):** a **agregação entre nós ainda não está implementada**. `scope=cluster` devolve **a visão do nó que atende** (e `partial` é sempre `false`), igual a `scope=node`. Num tópico com 12 mensagens publicadas, cada nó respondeu `depth: 4` (as de que ele é primário), e a resposta de cluster também deu 4, não 12. Para ter o total, **some no backend** a partir do `/metrics` de cada nó (com `telemetry.topics.mode` ligado) ou consulte **cada nó** com `scope=node` e some as quantidades. Os contadores (`published`, `consumed`, `acked`) são **por nó, desde a partida do processo**. A rota interna `GET /cluster/metrics/topics` e o parâmetro `stats.topics.cache-ttl` (sem efeito hoje) são a parte que falta. Registrado em [31-pendencias.md](31-pendencias.md).

### Condições do tópico

Também uma **lista**, com `worstCondition`. Os limites vêm de `stats.topics.*`; **um limite sem valor desliga a condição que depende dele**.

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo |
| `idle` | info | Sem publicação nem consumo há `stats.topics.idle-after` |
| `backlog` | aviso | `depth` > `stats.topics.backlog.depth` **ou** `oldestAge` > `stats.topics.backlog.age` |
| `stalled` | aviso | Há mensagens (`depth > 0`) mas **nenhum consumo** há `stats.topics.stalled-after`: consumidor parado ou ausente |
| `noisy-group` | info | Dois ou mais grupos e o maior ocupa mais de `stats.topics.noisy-group-share` do `depth` (padrão 50%). Na FIFO indica o **grupo quente** |
| `group-stalled` | aviso | Só FIFO: a cabeça de algum grupo foi entregue mais de `stats.topics.group-stalled-deliveries` vezes sem ack |
| `memory-pressure` | info | A maior parte das mensagens do tópico está em disco (`diskMessages` > `memoryMessages`) |

### Exportação das métricas de tópico (OTLP e Prometheus)

Opt-in por `telemetry.topics.mode`, com o **mesmo efeito** no OTLP e em `/metrics`:

| Modo | Padrão | O que exporta | Cardinalidade |
|---|---|---|---|
| `off` | ✔ | Nada por tópico | Nenhuma |
| `by-tags` | | Métricas agregadas **por combinação de tags** escolhidas em `telemetry.topics.group-by-tags`, no atributo `taba.topic.tag.<chave>` | Pelos valores das tags |
| `per-topic` | | Uma série por tópico (`messaging.destination.name`), até `telemetry.topics.max-topics` (1000 se ausente); o excedente vira `_other` | Pelo teto |

Métricas exportadas nesses modos: `taba.topic.published`, `consumed`, `acked` (counters), `visible`, `invisible`, `depth`, `memory_bytes`, `disk_bytes` (updowncounters), `oldest_timestamp` (gauge, segundos Unix; sem série no nó que não tem mensagens) e a condição `taba.topic.condition`. O SDK ainda limita a cardinalidade de qualquer métrica e marca o excesso com `otel.metric.overflow`.

**Tags de tópico** são pares chave/valor (`POST /topics`, `PATCH`). Nos **traces e logs** viram atributos `taba.topic.tag.<chave>`; nas métricas, só nas de tópico conforme o modo; **nunca** nas métricas de nó. Não afetam o comportamento das mensagens.

## Health checks

Rotas leves para orquestradores e balanceadores, **sem** fan-out:

| Rota | Significa | Falha → |
|---|---|---|
| `GET /health/live` | O processo está de pé e respondendo | Reiniciar o processo |
| `GET /health/ready` | O nó está **apto a receber tráfego**: registrado, **sincronização concluída**, fora de bootstrap e de retorno; **em drain responde `503`** | Tirar o nó da rotação |

`/health/ready` só responde `200` ao terminar a sincronização inteira ([07-cluster.md](07-cluster.md)). Enquanto sincroniza, o nó repassa as chamadas de cliente a outro nó, mas ainda assim deve ficar fora da rotação do balanceador.

## Acesso às rotas de operação

As rotas de operação são `/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain`. **Não têm autenticação** (decisão D63): quem instala as protege com a rede. Duas restrições opcionais e independentes:

| Parâmetro | Efeito |
|---|---|
| `operations.port` | Serve as rotas numa **porta própria**; elas **deixam de existir** na porta de cliente (`404`) |
| `operations.allowed-ips` | Lista de IPs e faixas CIDR autorizados. De fora da lista: `403 ops.ip-not-allowed`. Vale em qualquer porta que sirva as rotas |

| Configuração | Comportamento |
|---|---|
| Nenhuma (padrão) | As rotas ficam **na porta da API de cliente**, sem restrição |
| Só `operations.port` | Porta separada, sem filtro: quem alcança a porta acessa |
| Só `operations.allowed-ips` | Mesma porta da API, mas só os IPs listados acessam essas rotas |
| Ambas | Porta separada, acessível só pelos IPs listados |

- **Produção:** use ao menos a lista de IPs (Prometheus, balanceador, operadores) e, idealmente, a porta própria: `/metrics` expõe nomes de tópicos e tags e `/health/cluster` expõe a topologia.
- A lista deve incluir o **balanceador** que chama `/health/ready` e o **Prometheus ou Collector** que raspa.
- O IP avaliado é o **IP do cliente resolvido** pela mesma regra do rate limit: o da conexão ou, com `client-ip.headers.enabled`, o do cabeçalho (`X-Forwarded-For`), aceito só de `client-ip.trusted-proxies`. **Com a lista de proxies vazia, qualquer cliente forja o cabeçalho** e burla a lista de IPs (PRB-015).
- O envio OTLP é uma conexão **de saída** e não passa por essas restrições. As rotas `/cluster/*` ficam na porta do cluster e não são afetadas.
- O nó **não sobe** se `operations.port` estiver ocupada ou se a lista tiver uma entrada inválida.
- Pendência **D76:** o `kubelet` e o balanceador chamam `/health/*` de IPs que talvez não estejam na lista; hoje a lista filtra os health checks também.

## Traces

Um trace por operação de cliente, com spans internos por etapa. Os spans HTTP seguem a convenção (`{método} {rota}`, `http.*`); os de mensageria levam `messaging.system=taba`, `messaging.destination.name` (o tópico), `messaging.operation.type` (`send`, `receive`, `settle`) e `messaging.message.id`.

| Operação | Estrutura do trace |
|---|---|
| **Publicação** | Raiz `POST /topics/{topic}/messages` → `taba.write.register` (um por nó de destino) → `taba.write.commit` → `taba.write.rollback` se o quórum não fechou. A raiz **termina com sucesso quando o quórum confirma o commit**, sem esperar as demais réplicas |
| **Consumo e ack** | Raiz da requisição → leitura em memória ou disco |
| **Sincronização** | Um trace por bloco (`taba.sync.block`, `taba.block.id`), com `taba.sync.fetch` por chamada a uma origem |
| **Entrada, saída e votação** | `taba.cluster.join`, `taba.cluster.unregister` e `taba.vote.open` como raiz, com um span por nó consultado |

- O contexto é propagado entre nós pelo cabeçalho W3C `traceparent`: a escrita completa aparece num só trace mesmo passando por vários nós (D119).
- Atributos: `taba.topic.tag.<chave>`, `messaging.message.id` (**alta cardinalidade: só em traces e logs**), `taba.block.id`, `taba.node.id`, `taba.peer.id`, `taba.attempt`. O span que falha leva `otel.status_code=ERROR`.
- As rotas `/health`, `/metrics`, `/node` e `/cluster` não geram trace; o sinal de vida do canal também não.
- **Amostragem por cauda:** o SDK amostra por fração, no início. Para **manter sempre** os traces com falha ou evento de durabilidade (erro ao cliente, rollback, teto de commit, perda de mensagem), o nó marca esses spans com `taba.durability_event=true` e o **Collector** aplica `tail_sampling`.

## Logs

Logs **estruturados**: no `stdout` com `telemetry.logs.format` (`text`, padrão, `color` ou `json`) e, com OTLP, como *log records* com `trace_id` e `span_id`. O nível é `telemetry.logs.level` (`trace`, `debug`, `info`, `warn`, `error`; `info` se ausente). Logs por mensagem no caminho quente ficam em `debug`.

> O log no `stdout` ainda **não leva `trace_id`/`span_id`** (PRB-050); só o OTLP leva.

| Evento | Nível |
|---|---|
| Falha de registro/commit de mensagem (por tentativa) | `warn` |
| Escrita abortada com rollback (quórum não fechou) | `error` |
| Registro pendente expirado; teto de commit esgotado numa réplica | `warn` |
| Canal caiu/reconectou/esgotou; votação aberta, voto e resultado | `warn` / `info` |
| Nó registrado, desregistrado, rejeitado (freezing) | `info` |
| Token criado, alterado ou revogado (com `id`, **nunca** o segredo) | `info` |
| `401`/`403` (com o `id` do token, se houver; amostrado em volume) | `warn` / `debug` |
| Drain iniciado, concluído, cancelado, nó que volta como novo | `info` |
| Drain que estourou `drain.timeout` ou teve erro de I/O | `error` |
| Partida em modo desenvolvimento: retomada ou limpeza de `storage.dir` | `info` / `warn` |
| Falha de bootstrap | `error` |
| Início e fim de sincronização de bloco | `info` |
| **Perda de mensagem** (ID, bloco, nós consultados) | `error` |
| Retorno de nó e mensagens recuperadas | `info` |
| Resumo do overflow (`taba.storage.overflow`, a cada 60 s, só se houve; D118) | `info` |
| Erro de I/O em disco | `error` |

## Eventos

Eventos pontuais (**não** condições de saúde), emitidos como *log records* com `event.name`. Sempre levam o tópico em `messaging.destination.name`.

| `event.name` | Quando | Nível |
|---|---|---|
| `taba.distributor.no_target` | Publicação num distribuidor sem fila casando | `info` |
| `taba.distributor.partial` | Ao menos uma cópia num distribuidor falhou | `warn` |
| `taba.volatile.deleted` | Fila volátil excluída por ociosidade | `info` |
| `taba.volatile.check_blocked` | O gestor não concluiu a verificação final: algum nó não respondeu | — |
| `taba.message.dead_lettered` | Mensagem enviada à dead-letter ao atingir `maxDeliveries` | `warn` |
| `taba.message.deduplicated` | Publicação FIFO descartada por deduplicação (em volume, use `taba.fifo.deduplicated`) | `debug` |
| `taba.message.expired` | Mensagem descartada por `ttl` (em volume, use `taba.retention.expired`) | `info` |
| `taba.message.lost` | **Perda de mensagem** na sincronização. Não há métrica nem condição de saúde: quem quiser alertar usa o evento no backend de logs | `error` |
| `taba.message.recovered` | Mensagem devolvida por um nó que voltou | `info` |

O sistema **não envia notificações por conta própria**: alertas são do backend.

## Pendências desta área

D90 (padrões dos limiares das condições), D91 (teto e tags das métricas de tópico), D102 (`quorumAvailable`), PRB-026 (métricas de processo só no Linux), PRB-050 (OTLP e `stdout`): [31-pendencias.md](31-pendencias.md).

## Fontes e provas

- `telemetry/metrics/defs.rs` (a tabela de métricas acima é gerada daqui), `telemetry/otlp`, `telemetry/prometheus`, `telemetry/trace`, `telemetry/process`, `http/metrics`, `http/topic_stats`, `http/operations.rs`, `cluster/health/mod.rs` (as 8 condições e a severidade), `engine/messages/stats`.
- Exemplos de `/health/cluster` e `/metrics/topics/{topic}` copiados de um nó real.
- Casos de uso: UC-OPS (observabilidade, estatísticas, métricas e saúde), UC-CFG; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/metrics.rs`, `topic_stats.rs`, `otlp.rs`, `operations.rs`.
- Histórico: `11-observabilidade.md`, `06b-api-operacao.md`.
