# Observabilidade

[← Índice](README.md)

> Valores dos parâmetros e padrões: [10-configuracao.md](10-configuracao.md#observabilidade).

> Os nomes de métricas, rotas e parâmetros abaixo são **propostas**; ainda não há contrato definido (mesma ressalva de [10-configuracao.md](10-configuracao.md)).

## Visão geral

A saída de monitoramento do sistema é feita via **OpenTelemetry** (métricas, traces e logs), exportada (*push*) para um endpoint OTLP configurável (`telemetry.otlp.endpoint`). O cluster expõe também um endpoint **`GET /metrics`** no formato de texto do **Prometheus** (*pull*, um único alvo, com a visão de todos os nós), e **rotas de consulta** (`/stats/...`) para inspeção direta, sem depender de um backend de observabilidade.

| Saída | Modelo | Conteúdo | Uso típico |
|---|---|---|---|
| OTLP | push | Métricas, traces e logs | Backend compatível com OpenTelemetry |
| `GET /metrics` | pull, cluster inteiro (ou `scope=node`) | Somente métricas | Prometheus (e compatíveis: VictoriaMetrics, Grafana Agent, Mimir) |
| `GET /stats/*` | sob demanda | Métricas com visão de cluster e condições | Operador, scripts, diagnóstico sem backend |

OTLP e `/metrics` são alimentados pelos **mesmos instrumentos** (uma única instrumentação, dois exportadores): as métricas são as mesmas nas duas saídas, mudando só o formato de nomes (seção [Endpoint Prometheus](#endpoint-prometheus)).

A telemetria se divide em **dois níveis**, tratados de forma diferente:

| Nível | Cardinalidade | Exportação OTLP | Rotas de consulta |
|---|---|---|---|
| **Nó / cluster** | Limitada (nº de nós, de peers, de rotas) | Sempre | `/stats/node`, `/stats/cluster` |
| **Tópico** | **Ilimitada** (nº de tópicos × valores de tags) | Opt-in, com agregação e teto | `/stats/topics`, `/stats/topics/{topic}` |

**Por que separar:** métricas de nó são um conjunto fixo e pequeno, seguro de exportar sempre. Métricas de tópico crescem com o número de tópicos (e cada tag multiplica as séries); exportá-las por padrão faria o custo de telemetria escalar com o volume de tópicos, e o sistema de monitoramento poderia se tornar um problema maior que aquilo que monitora. Por isso ficam em **rotas próprias**, paginadas e sob demanda (seção [Métricas de tópico](#métricas-de-tópico-rotas-próprias)).

## Tags de tópico como atributos

Cada tópico pode ter **tags**: pares chave/valor definidos na criação (`POST /topics`) e alterados depois via `PATCH /topics/{topic}` (ver [06-api.md](06-api.md)).

- Nos **traces e logs** das operações daquele tópico (publicação, consumo, ack), as tags são enviadas como **atributos**. Traces e logs são eventos, não séries: não sofrem o mesmo problema de cardinalidade.
- Nas **métricas**, as tags só aparecem nas **métricas de tópico** e conforme a configuração de exportação (seção [Exportação das métricas de tópico](#exportação-das-métricas-de-tópico-otlp-e-prometheus)). As métricas de **nó nunca carregam tags nem o nome do tópico**.
- Exemplo: um tópico com `{"time": "financeiro", "ambiente": "prod"}` propaga esses dois atributos aos traces/logs dele, permitindo filtrar/agrupar por time ou ambiente no backend.
- As tags **não afetam o comportamento** das mensagens — servem só para telemetria.
- Limites de tamanho e caracteres das tags estão em [12-limites-api.md](12-limites-api.md), por serem regras fixas da API.

## Métricas de nó

Exportadas por **cada nó**, sempre, via OTLP e em `GET /metrics`, e consultáveis em `GET /stats/node` (local) e `GET /stats/cluster` (todos os nós). Prefixo sugerido: `msg.`.

**Atributos de recurso** (em toda métrica do nó): `service.name`, `service.instance.id` (ID do nó), `service.version`, nome do cluster, ambiente.

**Atributos de métrica:** somente de cardinalidade limitada — `route` (o **template** da rota, ex.: `/topics/{topic}/messages`, nunca o caminho real), `status_class` (`2xx`, `4xx`, `5xx`), `phase` (`register`/`commit`/`rollback`), `outcome`, `peer` (ID de outro nó, limitado pelo tamanho do cluster).

### Processo e host

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `msg.node.uptime` | gauge | s | Tempo desde o início do processo. |
| `msg.node.cpu.utilization` | gauge | 1 | Uso de CPU do processo. |
| `msg.node.process.memory` | gauge | By | Memória total do processo (inclui a parcela fora do limite de mensagens). |
| `msg.node.threads` | gauge | {thread} | Threads/tarefas ativas (conforme o runtime — ver [pendências](08-pendencias-e-proximos-passos.md)). |
| `msg.node.open_files` | gauge | {file} | Descritores/arquivos abertos (relevante pelo overflow em disco: um arquivo por mensagem). |
| `msg.node.connections` | gauge | {connection} | Conexões HTTP abertas (cliente e interna, separadas por atributo). |

### Armazenamento e overflow (seção 3.2)

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `msg.storage.memory.used` | gauge | By | Memória ocupada por mensagens. |
| `msg.storage.memory.limit` | gauge | By | Limite configurado (`memory.limit`). |
| `msg.storage.memory.messages` | gauge | {message} | Mensagens mantidas em memória. |
| `msg.storage.disk.messages` | gauge | {message} | Mensagens em disco (overflow). |
| `msg.storage.disk.used` | gauge | By | Espaço ocupado em disco pelas mensagens. |
| `msg.storage.disk.free` | gauge | By | Espaço livre no volume de `storage.dir`. |
| `msg.storage.overflow` | counter | {message} | Mensagens movidas de memória para disco. |
| `msg.storage.read.disk` | counter | {read} | Leituras que não encontraram a mensagem em memória e foram ao disco. |
| `msg.storage.read.memory` | counter | {read} | Leituras atendidas em memória (com a anterior, dá o *hit ratio*). |
| `msg.storage.disk.duration` | histogram | s | Latência de leitura/escrita em disco. |
| `msg.storage.disk.errors` | counter | {error} | Falhas de I/O em disco. |

### API de cliente

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.api.requests` | counter | {request} | `route`, `method`, `status_class` | Requisições recebidas. |
| `msg.api.duration` | histogram | s | `route`, `method` | Latência das requisições. |
| `msg.api.in_flight` | gauge | {request} | `route` | Requisições em andamento. |
| `msg.api.request.size` / `msg.api.response.size` | histogram | By | `route` | Tamanho de corpo de requisição/resposta. |
| `msg.api.rate_limited` | counter | {request} | `limit` (`publish`/`consume`/`admin`) | Requisições rejeitadas com `429` ([12-limites-api.md](12-limites-api.md)). |

### API interna (cluster)

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.cluster.rpc.requests` | counter | {request} | `route`, `peer`, `direction` (`in`/`out`), `outcome` | Chamadas entre nós. |
| `msg.cluster.rpc.duration` | histogram | s | `route`, `peer`, `direction` | Latência das chamadas entre nós. |

### Escrita de mensagens (seção 3.1 e 7.2)

Lado **coordenador** (nó que recebeu a publicação):

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.write.coordinated` | counter | {message} | `outcome` (`committed`/`failed`) | Publicações coordenadas por este nó. |
| `msg.write.duration` | histogram | s | `phase` (`register`/`commit`/`total`) | Duração de cada fase e da escrita completa. |
| `msg.write.retries` | counter | {retry} | `phase`, `peer` | Retries por fase e por nó de destino — o registro concentra a maior parte das falhas. |
| `msg.write.rollbacks` | counter | {rollback} | — | Rollbacks enviados por falha em fechar o **quórum** (fase 1), após esgotar os retries — nunca por falha em réplica fora do quórum. |
| `msg.write.commit_timeouts` | counter | {timeout} | — | Vezes que `write.commit.timeout` esgotou sem commit confirmado numa réplica fora do quórum; o commit já decidido **não** é desfeito, e o retry continua em segundo plano. |
| `msg.write.replicas_confirmed` | histogram | {node} | — | Réplicas com commit confirmado por escrita, medido em dois pontos: **na resposta ao cliente** (deve acompanhar `write.quorum`) e **eventualmente** (deve acompanhar `replication.factor`). |
| `msg.write.quorum_latency` | histogram | s | — | Tempo entre o início da escrita e o quórum confirmado — a latência que o cliente efetivamente espera. |

Lado **réplica** (nó responsável pelo bloco):

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.write.received` | counter | {message} | `phase` | Registros, commits e rollbacks recebidos. |
| `msg.write.pending` | gauge | {message} | — | Registros pendentes (sem commit). |
| `msg.write.pending.oldest_age` | gauge | s | — | Idade do registro pendente mais antigo (perto de `write.pending.ttl` indica commits não chegando). |
| `msg.write.pending.expired` | counter | {message} | — | Registros descartados por expiração (sem commit nem rollback). |

### Entrega e long polling ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md))

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.delivery.ready` | gauge | {message} | — | IDs nas listas de prontos deste nó (todos os tópicos). |
| `msg.delivery.redeliveries` | counter | {message} | — | Entregas de mensagens já entregues antes (`deliveryCount` > 1). |
| `msg.delivery.longpoll.waiting` | gauge | {request} | — | Chamadas de long polling aguardando neste nó. |
| `msg.delivery.longpoll.outcome` | counter | {request} | `outcome` (`delivered`/`empty`/`disconnected`) | Como terminaram as chamadas de long polling. |
| `msg.delivery.take` | counter | {request} | `direction` (`in`/`out`), `outcome` (`delivered`/`empty`/`error`) | Pedidos de entrega a outro dono (`/messages/take`). |
| `msg.cluster.channel.connections` | gauge | {connection} | — | Canais permanentes abertos com outros nós (esperado: nós do cluster − 1). |
| `msg.cluster.channel.notifications` | counter | {message} | `direction` (`in`/`out`), `outcome` | Avisos de mensagem nova enviados e recebidos (`outcome`: `fetched`/`empty` no lado que recebe). |

### Membros do cluster e falhas (seção 6)

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.cluster.members` | gauge | {node} | `state` (`active`/`frozen`/`suspect`) | Membros na tabela deste nó. |
| `msg.cluster.members.version` | gauge | 1 | — | Versão/época da tabela de membros; divergência entre nós indica propagação incompleta. |
| `msg.cluster.quorum.available` | gauge | 1 | — | `1` se há nós saudáveis suficientes para votar (`cluster.vote.min-voters`). |
| `msg.cluster.channel.state` | gauge | 1 | `peer` | `1` se o canal com o peer está conectado, `0` se caído (substitui o ping — seção 6.3). |
| `msg.cluster.channel.rtt` | histogram | s | `peer` | Latência do sinal de vida do canal, por peer. |
| `msg.cluster.channel.reconnects` | counter | {attempt} | `peer`, `outcome` | Tentativas de reconexão do canal e resultado; perto do limite de `cluster.channel.reconnect.retries` = votação iminente. |
| `msg.cluster.channel.queue.out` | gauge | {signal} | `peer` | Sinais na fila de saída para o peer; cresce enquanto o canal está fora (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). |
| `msg.cluster.channel.queue.in` | gauge | {signal} | `peer` | Sinais recebidos do peer ainda a processar; crescimento contínuo indica processamento mais lento que a chegada. |
| `msg.cluster.channel.queue.dropped` | counter | {signal} | `peer`, `reason` (`coalesced`/`expired`/`overflow`) | Sinais substituídos, descartados por idade ou descartados por estouro do limite da fila de saída (o estouro abre a votação de exclusão do peer). |
| `msg.cluster.vote` | counter | {vote} | `role` (`opened`/`cast`), `outcome` | Votações de exclusão abertas/votadas e seu resultado. |
| `msg.cluster.membership.changes` | counter | {change} | `operation` (`register`/`unregister`), `phase`, `outcome` | Registros e desregistros de nós, por fase. |
| `msg.cluster.bootstrap.failures` | counter | {failure} | `reason` | Falhas de bootstrap (teste de conexão, fase 1 ou fase 2 — seção 6.5). |
| `msg.cluster.freezing` | gauge | {node} | — | Nós atualmente em período de freezing (seção 6.7). |
| `msg.cluster.node.scrape_up` | gauge | 1 | `node` | `1` se a última atualização das métricas desse nó (thread de refresh, `telemetry.prometheus.node-timeout`) teve sucesso, `0` se falhou. Só existe na resposta de cluster. |
| `msg.cluster.node.metrics_age` | gauge | s | `node` | Idade dos valores mostrados para o nó na resposta de cluster. |
| `msg.cluster.node.state` | gauge | 1 | `state` (`joining`/`active`/`syncing`/`returning`) | Estado deste nó (incluindo retorno após exclusão — seção 6.6). |

### Anel, blocos e sincronização (seção 7)

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `msg.ring.virtual_nodes` | gauge | {vnode} | — | Nós virtuais deste nó (`ring.virtual-nodes`). |
| `msg.ring.blocks` | gauge | {block} | `role` (`primary`/`secondary`/`tertiary`/...), `state` (`healthy`/`syncing`) | Blocos que o nó atende, por papel e integridade. Desequilíbrio entre nós indica má distribuição do anel. |
| `msg.sync.blocks.active` | gauge | {block} | — | Blocos em sincronização agora (entrega suspensa nesses blocos). |
| `msg.sync.duration` | histogram | s | — | Tempo para tornar um bloco íntegro. |
| `msg.sync.ids.listed` | counter | {id} | — | IDs consolidados (união) nas listas de origem. |
| `msg.sync.messages.pulled` | counter | {message} | `peer` | Mensagens obtidas por nó de origem — mostra se o balanceamento entre origens está uniforme. |
| `msg.sync.bytes` | counter | By | `peer` | Volume transferido na sincronização. |
| `msg.sync.log.events` | gauge | {event} | — | Eventos no banco de eventos (`sync.db`) ainda por processar durante a sincronização. |
| `msg.sync.log.bytes` | gauge | By | — | Tamanho do banco de eventos em disco. |
| `msg.sync.phase` | gauge | 1 | `phase` (`topicos`/`blocos`/`replay`/`verificacao`/`pronto`) | Fase atual da sincronização do nó. |
| `msg.sync.streams` | counter | {stream} | `kind` (`topics`/`ids`/`messages`/`recover`), `direction` (`sent`/`received`), `outcome` (`complete`/`truncated`/`error`) | Streams NDJSON de sincronização e recuperação (seção 8.2.1 de [06-api.md](06-api.md)); `truncated` é o que fechou sem a linha final. |
| `msg.sync.stream.resumes` | counter | {resume} | `kind` | Retomadas depois de um corte (não conta a paginação normal por `hasMore`). |
| `msg.sync.stream.duration` | histogram | s | `kind` | Duração de cada stream. |
| `msg.sync.stream.bytes` | counter | By | `kind`, `direction` | Volume trafegado nos streams. |
| `msg.recover.messages` | counter | {message} | `outcome` (`sent`/`applied`/`skipped`) | Mensagens enviadas, aplicadas ou ignoradas (já existentes) pela recuperação da seção 6.6 de [04-cluster.md](04-cluster.md). |
| `msg.sync.failures` | counter | {failure} | `reason` | Falhas de sincronização de bloco. |
| `msg.cluster.recover` | counter | {message} | `direction` (`sent`/`received`) | Mensagens recuperadas na volta de um nó (seção 6.6). |

### Consulta

- `GET /stats/node` — métricas do **nó que atende a chamada**, leitura local, sem consulta a outros nós.
- `GET /stats/cluster` — visão do cluster: consulta **todos os nós** (pela rota interna `GET /cluster/stats/node`) e devolve a lista de métricas por nó mais um resumo (membros, quórum, blocos em sincronização, condição geral). Nós que não respondem aparecem como `unreachable` em vez de falhar a chamada inteira (*best-effort*).
- Toda resposta traz `collectedAt` (instante da coleta).

## Condição do cluster

Derivada das métricas de nó, exposta em `GET /stats/cluster` e `GET /health/ready`. **Condições, não um estado único** — o cluster pode ter mais de uma ao mesmo tempo:

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo. |
| `member-unreachable` | aviso | Canal com algum nó caído ou em reconexão, ainda antes da votação. |
| `node-frozen` | aviso | Nó(s) em período de freezing. |
| `syncing` | aviso | Blocos em sincronização; a entrega está suspensa neles (seção 7.3). |
| `under-replicated` | aviso | Blocos com menos réplicas íntegras que `replication.factor`, mas ainda **no ou acima do `write.quorum`** — estado normal e transitório (réplica alcançando o commit em segundo plano, seção 7.2), escritas continuam respondendo ao cliente normalmente. |
| `write-quorum-at-risk` | crítica | Blocos com réplicas íntegras **abaixo do `write.quorum`** — escritas podem falhar por não conseguir fechar quórum algum. |
| `quorum-lost` | crítica | Menos nós saudáveis que o mínimo de votantes (`cluster.vote.min-voters`); nenhuma exclusão de nó consegue ser efetivada. Não confundir com `write-quorum-at-risk`, que é sobre durabilidade de mensagens, não sobre votação de membros. |
| `membership-divergent` | crítica | Tabelas de membros com versões diferentes entre nós. |
| `storage-pressure` | aviso | Disco de `storage.dir` abaixo de um limite de espaço livre. |

> **Degradação de replicação é propriedade do cluster, não do tópico.** Os blocos são definidos pelo **ID da mensagem** (seção 7.2), não pelo tópico: as mensagens de um mesmo tópico se espalham por todos os blocos. Por isso `under-replicated` e `syncing` aparecem aqui e não na condição de tópico.

## Métricas de tópico (rotas próprias)

Ficam **fora** da exportação padrão e são consultadas por rotas dedicadas, pensadas para funcionar bem com **muitos tópicos**:

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/stats/topics` | Lista **paginada** com as métricas resumidas de cada tópico. |
| `GET` | `/stats/topics/{topic}` | Métricas detalhadas de **um** tópico. |

> Prefixo `/stats` (e não `/metrics`) porque `/metrics` é o endpoint Prometheus (seção [Endpoint Prometheus](#endpoint-prometheus)), que tem formato e semântica próprios.

### Parâmetros de `GET /stats/topics`

| Parâmetro | Descrição |
|---|---|
| `limit` / `cursor` | Paginação por cursor (o resultado devolve `nextCursor`). Padrão e máximo de `limit` a definir. A ordem é sempre por nome do tópico; **não há `sort`**, pois as métricas mudam a todo instante e a paginação por chave não se sustentaria. Para achar os tópicos com problema, use `condition`. |
| `scope` | `cluster` (padrão, com fan-out e agregação) ou `node` (só os valores deste nó, sem consultar outros — seção [Visão do nó e visão do cluster](#visão-do-nó-e-visão-do-cluster)). |
| `tag.<chave>=<valor>` | Filtra por tag. Ex.: `tag.time=financeiro`. |
| `condition` | Filtra por condição (ex.: `condition=backlog`) — o caso de uso principal: "quais tópicos estão com problema?". |
| `fields` | Restringe os campos devolvidos, para respostas menores. |
| `groupBy` | Agrega por tag (ex.: `groupBy=time`), devolvendo uma linha por valor de tag em vez de uma por tópico. |

### Métricas por tópico

| Grupo | Métrica | Descrição |
|---|---|---|
| **Publicação** | `published` | Total de mensagens publicadas. |
| | `publishRate` | Mensagens publicadas por segundo (janela recente). |
| | `publishedBytes` | Bytes publicados; com `published` dá o tamanho médio da mensagem. |
| | `publishErrors` | Publicações que falharam (erro devolvido ao cliente). |
| | `publishLatency` | p50/p95/p99 da latência de publicação. |
| **Consumo** | `consumed` | Total de mensagens entregues em `GET`. |
| | `acked` | Total de confirmações (`DELETE` da mensagem). |
| | `consumeRate` / `ackRate` | Mensagens consumidas / confirmadas por segundo. É a **taxa de leitura** usada na divisão dinâmica de memória (seção 3.2). |
| | `consumeLatency` | p50/p95/p99 da latência de consumo e de ack. |
| **Fila** | `visibleMessages` | Mensagens **visíveis**: prontas para entrega, sem `invisibleUntil` no futuro ([14-invisibilidade.md](14-invisibilidade.md)). |
| | `invisibleMessages` | Mensagens **invisíveis**: entregues, sem ack e com `invisibleUntil` no futuro (em processamento por algum consumidor). |
| | `depth` | Mensagens ainda não confirmadas: `visibleMessages + invisibleMessages`. |
| | `oldestMessageAt` | **Data de publicação da mensagem mais antiga** ainda não confirmada (visível ou invisível), tirada do timestamp do GUID v7 do ID. Ausente se o tópico não tem mensagens. |
| | `oldestAge` | Derivado: `agora − oldestMessageAt`. O melhor indicador de consumidor parado. |
| | `bytes` | Volume total acumulado. |
| **Armazenamento** | `memoryMessages` / `diskMessages` | Mensagens em memória e em disco. |
| | `memoryBytes` / `diskBytes` | Bytes em memória e em disco. |
| | `memoryShare` | Parcela da memória do cluster que o tópico ocupa (resultado da divisão dinâmica). |
| **Identificação** | `type`, `tags`, `createdAt` | Tipo (`queue`/`distributor`), `id`, tags e criação; em fila volátil, também `volatile` e `idleTimeout`. |

Um Tópico Distribuidor não guarda mensagens: as métricas de fila (profundidade, idade, armazenamento) se aplicam às **filas destino**, e o distribuidor expõe apenas publicações recebidas, cópias geradas e falhas por fila. Não há métricas por assinante. Ver [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.7.

### Condição do tópico

Também uma **lista de condições**, com o campo `status` sendo a de maior severidade. Limites configuráveis ([10-configuracao.md](10-configuracao.md#observabilidade)):

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo. |
| `idle` | info | Sem publicação nem consumo há `stats.topics.idle-after`. |
| `backlog` | aviso | `depth` ou `oldestAge` acima do limite. |
| `stalled` | aviso | Há mensagens (`depth > 0`) mas **nenhum consumo** há `stats.topics.stalled-after` — consumidor parado ou ausente. |
| `memory-pressure` | info | Maior parte das mensagens do tópico está em disco (`diskMessages` >> `memoryMessages`): leituras mais lentas. |

### Visão do nó e visão do cluster

Os valores de fila (`visibleMessages`, `invisibleMessages`, `depth`, `oldestMessageAt`) existem em dois pontos de vista, e o cluster **interpreta** os do nó, não os repete:

| Métrica | Visão do **nó** (local) | Visão do **cluster** (agregada) |
|---|---|---|
| `visibleMessages` | Visíveis das mensagens de que **este nó é o primário** | **Soma** dos valores de todos os nós |
| `invisibleMessages` | Invisíveis das mensagens de que este nó é o primário | **Soma** dos valores de todos os nós |
| `depth` | `visibleMessages + invisibleMessages` do nó | **Soma** dos valores de todos os nós |
| `oldestMessageAt` | Data da mais antiga entre as mensagens de que este nó é o primário | **A mais antiga** (menor data) entre todos os nós; nó sem mensagens do tópico não entra na conta |
| `oldestAge` | `agora − oldestMessageAt` do nó | `agora − oldestMessageAt` do cluster (é a maior idade entre os nós) |

- **Só o primário conta**, como no restante desta seção; senão a soma seria multiplicada pelo fator de replicação.
- Cada nó mantém **contadores** dessas quantidades, atualizados nas transições (publicação: +1 visível; entrega: −1 visível, +1 invisível; vencimento do prazo: −1 invisível, +1 visível; ack: −1 no estado em que estava), sem varrer mensagens a cada consulta. A data mais antiga é a menor entre a mais antiga das visíveis e a mais antiga das invisíveis; como achá-la sem varredura é o D35 ([08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md)).
- **Nó sem mensagens do tópico:** `oldestMessageAt` fica **ausente** (sem valor), nunca zero — um zero venceria o "mais antigo" do cluster.
- **Nó que não responde** fica de fora e o resultado é marcado `partial`: as somas viram **piso** (o cluster tem ao menos aquilo) e a data mais antiga pode ser mais recente que a real.
- **Como consultar cada visão:** `GET /stats/topics` e `GET /stats/topics/{topic}` devolvem a visão do **cluster** por padrão; com `scope=node` devolvem a visão **só do nó que atende**, sem consulta a outros nós. `groupBy` por tag aplica as mesmas regras (soma das quantidades, menor data) sobre os tópicos do grupo.

### Como o valor é calculado (custo)

- As mensagens de um tópico se espalham pelos blocos do anel; cada mensagem existe em `replication.factor` nós. Portanto o valor do tópico no cluster é a **soma entre nós**, contando apenas a **réplica primária** de cada mensagem — do contrário, tudo apareceria multiplicado pelo fator de replicação.
- Cada nó mantém contadores **locais** por tópico. Uma rota interna (`GET /cluster/stats/topics`) devolve esses parciais; o nó que atende `/stats/topics` **consulta todos os nós, soma e devolve**.
- Esse fan-out é justamente o custo que justifica isolar essas métricas: com muitos tópicos, o payload de cada nó cresce. Mitigações: o resultado é mantido em **cache** por `stats.topics.cache-ttl` (a resposta traz `collectedAt`); o filtro e o `limit` são aplicados depois do merge; `fields` reduz o tamanho; e `GET /stats/topics/{topic}` pede aos nós só aquele tópico.
- Os valores são uma **fotografia aproximada** (leitura sem lock global, nó que não responde fica de fora e é listado como `partial`), não um valor transacional.

## Exportação das métricas de tópico (OTLP e Prometheus)

Opt-in por `telemetry.topics.mode`, com o **mesmo efeito** no OTLP e em `GET /metrics`:

| Modo | Padrão | O que exporta | Cardinalidade |
|---|---|---|---|
| `off` | ✔ | Nada por tópico; só as rotas `/stats/topics`. | Nenhuma |
| `by-tags` | | Métricas agregadas **por combinação de tags** escolhidas em `telemetry.topics.group-by-tags` (ex.: `time`, `ambiente`). | Limitada pelos valores das tags, **não** pelo número de tópicos |
| `per-topic` | | Uma série por tópico, com o atributo `topic` (mais as tags). Limitada por `telemetry.topics.max-topics`; acima do teto, os tópicos excedentes são agregados em `topic="_other"`. | Limitada pelo teto |

Métricas exportadas nos modos `by-tags` e `per-topic` (subconjunto das métricas por tópico, escolhidas por serem contadores/gauges baratos): `msg.topic.published`, `msg.topic.consumed`, `msg.topic.acked`, `msg.topic.visible`, `msg.topic.invisible`, `msg.topic.depth`, `msg.topic.oldest_timestamp` (gauge em segundos Unix da mensagem mais antiga; **sem série** no nó que não tem mensagens do tópico), `msg.topic.memory_bytes`, `msg.topic.disk_bytes`.

## Endpoint Prometheus

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics` | Métricas do **cluster inteiro** (padrão, `scope=cluster`) ou **só deste nó** (`scope=node`), no formato de texto do Prometheus (`text/plain; version=0.0.4`; OpenMetrics quando pedido via `Accept`). |

### Modelo: um único alvo de scrape para o cluster

![Métricas do cluster em memória e coleta do Prometheus](diagramas/img/metricas-cluster.svg)

*Fonte: [metricas-cluster.json](diagramas/fonte/metricas-cluster.json).*

Seguindo o princípio do projeto (implementação simples e gestão automatizada), o Prometheus tem **um alvo só**: o **endereço do balanceador do cluster** (`cluster.entry-address`). Não é preciso descobrir nós nem manter lista de alvos; nós que entram ou saem aparecem e somem sozinhos.

- **Métricas do cluster em memória:** todo nó mantém em **memória** a última visão das métricas de todos os nós, e uma **thread dedicada** a mantém atualizada: a cada `telemetry.prometheus.refresh-interval` ela consulta **todos os nós em paralelo** (rota interna `GET /cluster/metrics`, que devolve as métricas locais de cada um) e troca o instantâneo inteiro de uma vez. O `GET /metrics` **só lê essa memória**: não faz fan-out no momento do scrape, e responde rápido mesmo com nós lentos ou fora do ar. Cada chamada do Prometheus pode cair num nó diferente do balanceador, e o resultado é praticamente o mesmo em qualquer um (a diferença é a defasagem de até um intervalo).
- **Uma série por nó, sem somar:** as séries de cada nó são mantidas separadas, com o rótulo `node` (o ID do nó), e a **soma é feita no PromQL**. Somar no servidor quebraria os contadores: se um nó falta numa chamada, a soma cai e o `rate()` lê um reinício falso.
- **Nó que não responde:** a thread espera cada nó por `telemetry.prometheus.node-timeout`. Se um nó falha, o instantâneo mantém os **últimos valores dele** por até `telemetry.prometheus.max-age`; passado esse prazo, as séries dele somem da resposta. A métrica `msg_cluster_node_scrape_up{node="..."}` vale `0` quando a última atualização desse nó falhou, e `msg_cluster_node_metrics_age_seconds{node="..."}` diz a idade do que está sendo mostrado. Assim se vê qual nó caiu, mesmo com o cluster degradado. Cluster inteiro fora do ar: o alvo único fica `down`.
- **Custo:** cada nó consulta todos os outros a cada intervalo, ou seja, **N × N chamadas por intervalo** no cluster, independente de haver scrape ou não; com poucos nós é irrelevante, e em clusters grandes o intervalo deve subir. Um nó que acaba de subir mostra só as métricas locais até a primeira atualização completa.
- **Métricas individuais por nó** continuam disponíveis com `scope=node`. Quem quiser um alvo por nó (por exemplo, com um *sidecar* ou agente ao lado de cada instância) usa essa chamada; é uma **escolha de implementação** da operação, não o caminho recomendado.
- **OTLP não muda:** cada nó continua enviando por *push* as suas próprias métricas, sem fan-out.
- A agregação de tópicos continua válida: os contadores de tópico contam **apenas as mensagens em que o nó é primário** (mesma regra de [Como o valor é calculado](#como-o-valor-é-calculado-custo)), então `sum(msg_topic_visible) by (topic)` e `sum(msg_topic_invisible) by (topic)` dão os valores do cluster sem multiplicar pelo fator de replicação, e `min(msg_topic_oldest_timestamp_seconds) by (topic)` dá a data da mensagem mais antiga.

### Conteúdo

- **Métricas de nó:** todas as da seção [Métricas de nó](#métricas-de-nó), sempre.
- **Métricas de tópico:** conforme `telemetry.topics.mode` — `off` (padrão) não emite nenhuma; `by-tags` e `per-topic` seguem as regras e o teto de cardinalidade já descritos. Ou seja, **o `/metrics` também não escala com o número de tópicos por padrão**.
- **Condições** (cluster e tópico, seções acima) como *gauges* `0/1`: `msg_cluster_condition{condition="under-replicated"} 1`; no tópico, apenas nos modos que exportam tópicos.
- `target_info` com os atributos de recurso (versão, nome do cluster, ambiente), no estilo OpenMetrics.

### Mapeamento de nomes (OTel → Prometheus)

| Regra | Exemplo (OTel → Prometheus) |
|---|---|
| Pontos viram `_` | `msg.storage.memory.used` → `msg_storage_memory_used_bytes` |
| Unidade vira sufixo | `s` → `_seconds`, `By` → `_bytes`, razão `1` → `_ratio` |
| Contadores ganham `_total` | `msg.write.retries` → `msg_write_retries_total` |
| Histogramas viram `_bucket`/`_sum`/`_count` | `msg.write.duration` → `msg_write_duration_seconds_bucket{phase="commit",le="0.05"}` |
| Atributos viram rótulos | `phase`, `route`, `peer`, `outcome`, `status_class` |

Regras de rótulo iguais às do OTLP: `route` é sempre o **template** (`/topics/{topic}/messages`), nunca o caminho real; o ID da mensagem nunca é rótulo; `topic` só aparece no modo `per-topic`.

### Configuração e exposição

- `telemetry.prometheus.enabled` liga/desliga o endpoint.
- Porta e restrição de acesso do `/metrics` são as das demais rotas de operação: ver [Acesso às rotas de operação](#acesso-às-rotas-de-operação).

### Exemplos de alertas (PromQL)


Sugestões de partida, a calibrar em produção:

| Alerta | Expressão |
|---|---|
| Nó fora do ar | `up{job="mensageria"} == 0` |
| Cluster sem quórum | `min(msg_cluster_quorum_available) == 0` |
| Tabelas de membros divergentes | `count(count by (version) (msg_cluster_members_version)) > 1` |
| Memória perto do limite | `msg_storage_memory_used_bytes / msg_storage_memory_limit_bytes > 0.9` |
| Falhas de escrita | `sum(rate(msg_write_coordinated_total{outcome="failed"}[5m])) > 0` |
| Registros pendentes envelhecendo | `max(msg_write_pending_oldest_age_seconds) > 30` |

## Acesso às rotas de operação

As **rotas de operação** são `GET /metrics`, `GET /stats/*` e `GET /health/*`. Ao contrário da API de cliente, são destinadas a operadores, ao Prometheus e a orquestradores/balanceadores. Duas restrições **opcionais e independentes** podem ser configuradas para elas:

| Parâmetro | Efeito |
|---|---|
| `operations.port` | Serve as rotas de operação em uma **porta própria**. Nessa configuração, elas **deixam de existir** na porta da API de cliente (que responde `404` para esses caminhos). |
| `operations.allowed-ips` | **Lista de IPs e faixas (CIDR)**, ex.: `10.0.0.0/8, 192.168.1.10`, autorizados a acessar essas rotas. Requisições de fora da lista recebem `403 Forbidden`. Vale em qualquer porta que sirva as rotas. |

As duas podem ser combinadas (porta própria **e** lista de IPs), usadas isoladas, ou nenhuma:

| Configuração | Comportamento |
|---|---|
| Nenhuma (padrão) | As rotas ficam na **mesma porta da API de cliente**, sem restrição de origem, e seguem as regras de rate limit próprias das rotas de operação ([12-limites-api.md](12-limites-api.md): `/health`, `/metrics` e `/stats/node` isentos, `/stats/cluster` e `/stats/topics` em categoria própria) e qualquer autenticação que venha a existir. |
| Só `operations.port` | Porta separada, sem filtro por IP: quem alcança a porta acessa. |
| Só `operations.allowed-ips` | Mesma porta da API, mas só os IPs da lista acessam essas rotas; a API de cliente segue aberta. |
| Ambas | Porta separada, acessível apenas pelos IPs/faixas listados. |

Detalhes:

- Recomendação para produção: ao menos a lista de IPs (Prometheus, balanceadores, faixa dos operadores); idealmente também a porta própria, pois o `/metrics` e o `/stats/topics` expõem nomes de tópicos e tags, e o `/stats/cluster` expõe a topologia do cluster.
- A lista deve incluir os IPs do **balanceador** que chama `/health/ready` e do **Prometheus** que faz o scrape.
- O IP avaliado é o **IP do cliente resolvido** pela mesma regra do rate limit ([12-limites-api.md](12-limites-api.md)): o endereço da conexão ou, com `client-ip.headers.enabled`, o cabeçalho (`X-Forwarded-For`), aceito só de quem estiver em `client-ip.trusted-proxies`. **Sem a lista configurada, o cabeçalho é aceito de qualquer origem**, então um cliente com acesso direto ao nó consegue forjá-lo e burlar a lista de IPs. Como o sistema não tem autenticação, essas rotas devem ficar numa **camada de rede segura** (rede privada, porta própria atrás de firewall); a lista de IPs é uma restrição a mais, não a proteção principal.
- As rotas internas entre nós (`/cluster/...`, inclusive `/cluster/stats/*`) **não** são rotas de operação: têm mecanismo próprio e não são afetadas por essas configurações.
- O nó **não sobe** se `operations.port` estiver ocupada ou se a lista tiver entrada inválida — falha explícita no início, em vez de expor as rotas sem restrição por engano.

## Traces

Um trace por operação de cliente, com spans internos para cada etapa relevante:

- **Publicação:** span raiz `POST /topics/{topic}/messages` → span de registro (um por nó de destino) → span de commit (um por nó de destino) → rollback, se a falha foi em fechar o **quórum**. O span raiz **termina com sucesso assim que o quórum confirma o commit** (sempre incluindo o primário) — não espera as demais réplicas. Os spans de registro/commit das réplicas fora do quórum continuam **fora do trace já encerrado**; se `write.commit.timeout` esgota numa delas, isso não reabre nem afeta o span raiz, só gera o evento de observabilidade correspondente (`msg.write.commit_timeouts`). O contexto de trace é propagado nas chamadas internas entre nós (cabeçalho W3C `traceparent`), de modo que a escrita completa aparece em um único trace mesmo passando por vários nós.
- **Consumo e ack:** span raiz da requisição → leitura em memória ou disco.
- **Sincronização de bloco:** um trace por bloco sincronizado, com um span por lote de `GET`, indicando o nó de origem.
- **Registro/desregistro de nó e votação:** um trace por operação, com um span por nó consultado.
- **Atributos dos spans:** tags do tópico, ID da mensagem, ID do bloco, IDs dos nós envolvidos, número da tentativa (retry). Como o ID da mensagem é um GUID v7 de alta cardinalidade, ele vai em **traces e logs, nunca em métricas**.
- **Amostragem:** `telemetry.traces.sample-ratio`; falhas e eventos de durabilidade (erro ao cliente, rollback, teto de commit esgotado numa réplica, perda de mensagem) devem ser sempre mantidos.

## Logs

Logs **estruturados** (JSON) com o mesmo contexto do trace (`trace_id`, `span_id`) e as tags do tópico quando houver. Eventos de log esperados:

| Evento | Nível | Origem |
|---|---|---|
| Falha de registro/commit de mensagem (por tentativa) | `warn` | Escrita (seção 7.2) |
| Escrita abortada e rollback enviado (falha em fechar o quórum) | `error` | Escrita (seção 3.1) |
| Registro pendente expirado | `warn` | Escrita (seção 3.1) |
| Teto de commit esgotado numa réplica fora do quórum, sem rollback — retry continua em segundo plano | `warn` | Escrita (seção 7.2) |
| Canal com um nó caiu, reconectou ou esgotou as tentativas; votação aberta, voto e resultado | `warn` / `info` | Cluster (seção 6.3) |
| Nó registrado, desregistrado, rejeitado (freezing) | `info` | Cluster (seções 6.2, 6.7) |
| Falha de bootstrap do nó | `error` | Cluster (seção 6.5) |
| Início e fim de sincronização de bloco | `info` | Sincronização (seção 7.3) |
| **Perda de mensagem** (ID, bloco, nós consultados) | `error` | Sincronização (seção 7.3) |
| Retorno de nó e mensagens recuperadas | `info` | Cluster (seção 6.6) |
| Overflow de memória para disco (resumo periódico, não por mensagem) | `info` | Armazenamento (seção 3.2) |
| Erro de I/O em disco | `error` | Armazenamento |

O nível de log é configurável (`telemetry.logs.level`). Logs por mensagem em caminho quente (publicação, consumo) ficam em `debug` para não pesar na latência.

## Health checks

Rotas leves para orquestradores e balanceadores, **sem** fan-out para outros nós:

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/health/live` | O processo está de pé e respondendo. Falha → reiniciar o processo. |
| `GET` | `/health/ready` | O nó está **apto a receber tráfego**: registrado no cluster (commit confirmado), com a **sincronização concluída** (tabela de tópicos, todos os blocos e o banco de eventos já processados, seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) e não em bootstrap nem em retorno pós-exclusão. Falha → o balanceador deve tirar o nó da rotação. |

`/health/ready` só passa a responder com sucesso quando o nó termina a sincronização inteira: enquanto sincroniza, o nó apenas guarda os eventos no banco de eventos e repassa as chamadas de cliente a outro nó (seção 6.9 de [04-cluster.md](04-cluster.md)); mesmo assim deve ficar fora da rotação do balanceador, para não gastar o salto extra à toa. Depois de pronto, ele volta a atender normalmente.

## Eventos de perda de mensagem (seção 7.3)

Decidido: o evento de perda gera uma linha no log da aplicação, em nível `error`, com o **nome do tópico**, o **ID da mensagem**, o bloco e os nós consultados, e um registro na tabela `perdas` do banco do nó, consultada pelo nó que volta de uma exclusão (seção 6.6 de [04-cluster.md](04-cluster.md)). Não há métrica nem condição de saúde por enquanto; quem quiser alertar usa a busca no log. O sistema não envia notificações por conta própria.

## Eventos de distribuidor e de fila volátil (seção 14)

Eventos pontuais, não condições de saúde do nó nem do cluster ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.7):

| Evento | Quando |
|---|---|
| `distributor-no-target` | Publicação num Tópico Distribuidor sem nenhuma fila casando (sucesso, mensagem descartada). |
| `distributor-partial` | Ao menos uma cópia de uma publicação em DT falhou. |
| `volatile-deleted` | Fila volátil excluída por ociosidade. |
| `volatile-check-blocked` | O gestor não concluiu a verificação final porque algum nó não respondeu. |

## Ainda em aberto

As pendências desta área ficam em [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md), no passo 10.
