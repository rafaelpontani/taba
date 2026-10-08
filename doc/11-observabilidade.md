# Observabilidade

[← Índice](README.md)

> Valores dos parâmetros e padrões: [10-configuracao.md](10-configuracao.md#observabilidade).

> Os nomes de métricas, rotas e parâmetros abaixo são **propostas**; ainda não há contrato definido (mesma ressalva de [10-configuracao.md](10-configuracao.md)). Seguem as **convenções semânticas do OpenTelemetry** (seção [Aderência ao OpenTelemetry](#aderência-ao-opentelemetry)).

## Visão geral

A telemetria do Taba é **OpenTelemetry** (métricas, traces e logs), e há duas saídas para máquinas e uma leitura para pessoas:

| Saída | Modelo | Conteúdo | Uso típico |
|---|---|---|---|
| **OTLP** | *push*, cada nó envia o que é seu | Métricas, traces e logs | OpenTelemetry Collector, ou qualquer backend OTLP (inclusive o Prometheus, que recebe OTLP) |
| **`GET /metrics`** | *pull*, **só deste nó** | Somente métricas | Prometheus e compatíveis (VictoriaMetrics, Grafana Agent, Mimir), ou o Collector com o receptor `prometheus` |
| **`GET /health/cluster`**, **`/metrics/topics/*`** | sob demanda, JSON | Estado, condições e métricas de tópico | Operador, scripts, diagnóstico sem backend |

OTLP e `/metrics` são alimentados pelos **mesmos instrumentos** (uma única instrumentação, dois exportadores): as métricas são as mesmas, mudando só o formato dos nomes ([Endpoint Prometheus](#endpoint-prometheus)). As rotas `/health/cluster` **não repetem** as métricas: devolvem estado e condições para pessoas.

**Nenhum nó consulta outro para montar métrica.** Cada nó reporta o seu trabalho e a sua visão do cluster, e o total do cluster é feito pelo backend (PromQL, ou o Collector). Isso dispensa lista de alvos centralizada, fan-out entre nós e agregação no servidor.

A telemetria se divide em **dois níveis**, tratados de forma diferente:

| Nível | Cardinalidade | Exportação OTLP e `/metrics` | Rotas de consulta |
|---|---|---|---|
| **Nó / cluster** | Limitada (nº de nós, de peers, de rotas) | Sempre | `/health/cluster` |
| **Tópico** | **Ilimitada** (nº de tópicos × valores de tags) | Opt-in, com agregação e teto | `/metrics/topics`, `/metrics/topics/{topic}` |

**Por que separar:** métricas de nó são um conjunto fixo e pequeno, seguro de exportar sempre. Métricas de tópico crescem com o número de tópicos (e cada tag multiplica as séries); exportá-las por padrão faria o custo de telemetria escalar com o volume de tópicos. Por isso ficam em **rotas próprias**, paginadas e sob demanda, e só entram no OTLP e no `/metrics` por opção (seção [Exportação das métricas de tópico](#exportação-das-métricas-de-tópico-otlp-e-prometheus)).

## Aderência ao OpenTelemetry

| Regra | Como o Taba a cumpre |
|---|---|
| **Namespace próprio** | Tudo que é do Taba começa com `taba.` (métricas, atributos, eventos). Nomes genéricos como `msg.` não são usados. |
| **Convenção semântica quando existe** | HTTP usa `http.server.*` e `http.client.*`, processo usa `process.*`, mensageria usa o atributo `messaging.*`. Só o que não tem nome padrão é `taba.*`. Dashboards prontos dos backends funcionam sem trabalho. |
| **Prefixo `taba.cluster.*`** | Marca a **visão do cluster** (membros, quórum, canais, votação). Cada nó reporta a sua visão; o total sai do backend, com `max`/`min` (todos veem o mesmo cluster) e não `sum`. As demais métricas `taba.*` são do **trabalho do nó** e se somam. |
| **Unidades (UCUM)** | Sem a unidade no nome. `s`, `By`, e unidades anotadas entre chaves (`{message}`, `{flag}`) para contagens. A unidade `1` só vale para razões de verdade (0 a 1). |
| **Instrumentos** | Contagem atual que sobe e desce e se soma entre nós (mensagens em memória, prontas, pendentes) é `updowncounter`; valor que não se soma (limite, idade, versão, estado) é `gauge`; só cresce, `counter`; distribuição, `histogram`. |
| **Sem contador duplicado** | O número de requisições é o `_count` do histograma de duração, não há contador à parte. |
| **Recurso** | `service.name` (`taba`), `service.namespace` (nome do cluster), `service.instance.id` (ID do nó, GUID v7 novo a cada início), `service.version` e `deployment.environment.name`. Os três primeiros viram `job` e `instance` no Prometheus. |
| **Atributos** | De cardinalidade limitada, nos nomes padrão (`http.route`, `http.request.method`, `http.response.status_code`, `error.type`, `server.port`) ou `taba.*`. |
| **Eventos** | *Log records* com `event.name` (seção [Eventos](#eventos)). |
| **Configuração** | As variáveis padrão `OTEL_*` do SDK (seção [Como ligar um coletor](#como-ligar-um-coletor)). |

**Atributos próprios:** nas tabelas abaixo, o atributo de nome curto leva o prefixo `taba.` (por exemplo, `phase` é `taba.phase`, `peer` é `taba.peer.id`, `outcome` é `taba.outcome`). No Prometheus viram `taba_phase`, `taba_peer_id`, `taba_outcome`.

## Como ligar um coletor

Basta apontar o nó para o endpoint OTLP, com as **variáveis padrão do OpenTelemetry**, sem lista de alvos e sem descoberta:

| Variável | Para quê |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Endereço do coletor (ex.: `http://coletor:4317`). Sem ela, o OTLP fica desligado. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` (padrão) ou `http/protobuf`. |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_CERTIFICATE`... | Autenticação e TLS do backend. |
| `OTEL_METRIC_EXPORT_INTERVAL` | Intervalo de envio das métricas, em milissegundos (padrão 60000). |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | Amostragem de traces (`parentbased_traceidratio` e a fração). |
| `OTEL_RESOURCE_ATTRIBUTES` | Atributos de recurso: `service.namespace=<cluster>,deployment.environment.name=<ambiente>`. |

**Três formas de usar**, da mais simples à mais manual:

1. **Coletor (recomendada).** Cada nó empurra por OTLP; o Collector recebe, trata e encaminha. Exemplo mínimo:

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

2. **Prometheus direto.** O Prometheus recebe OTLP (`--web.enable-otlp-receiver`), com `OTEL_EXPORTER_OTLP_ENDPOINT` apontando para ele, ou raspa o `GET /metrics` de cada nó (descoberta por DNS de um *Service headless*, ou pelo Kubernetes).
3. **Só `/health/cluster`.** Sem nada instalado, as rotas JSON bastam para diagnóstico.

**Amostragem de traces.** O SDK decide a amostragem **no início** do trace, antes de saber se houve falha, então ele só faz amostragem por fração. Manter **sempre** os traces com falha e com evento de durabilidade (erro ao cliente, rollback, teto de commit esgotado, perda de mensagem) é **amostragem por cauda**, feita no Collector (processador `tail_sampling`, com política "status de erro" e "atributo `taba.durability_event`"). O nó marca esses spans com o atributo.

## Tags de tópico como atributos

Cada tópico pode ter **tags**: pares chave/valor definidos na criação (`POST /topics`) e alterados depois via `PATCH /topics/{topic}` (ver [06a-api-cliente.md](06a-api-cliente.md)).

- Nos **traces e logs** das operações daquele tópico (publicação, consumo, ack), as tags são enviadas como **atributos** `taba.topic.tag.<chave>`. O nome do tópico vai em `messaging.destination.name`. Traces e logs são eventos, não séries: não sofrem o mesmo problema de cardinalidade.
- Nas **métricas**, as tags só aparecem nas **métricas de tópico** e conforme a configuração de exportação (seção [Exportação das métricas de tópico](#exportação-das-métricas-de-tópico-otlp-e-prometheus)). As métricas de **nó nunca carregam tags nem o nome do tópico**.
- Exemplo: um tópico com `{"time": "financeiro", "ambiente": "prod"}` propaga `taba.topic.tag.time=financeiro` e `taba.topic.tag.ambiente=prod` aos traces e logs dele, permitindo filtrar e agrupar por time ou ambiente no backend.
- As tags **não afetam o comportamento** das mensagens — servem só para telemetria.
- Limites de tamanho e caracteres das tags estão em [12-limites-api.md](12-limites-api.md), por serem regras fixas da API.

## Métricas de nó

Exportadas por **cada nó**, sempre, via OTLP e em `GET /metrics`. Cada uma descreve **este nó**; o cluster se obtém somando (ou, nas `taba.cluster.*`, tomando `max`/`min`) no backend. Atributos de recurso: seção [Aderência ao OpenTelemetry](#aderência-ao-opentelemetry).

**Atributos de métrica:** somente de cardinalidade limitada — `http.route` (o **template** da rota, ex.: `/topics/{topic}/messages`, nunca o caminho real), `http.request.method`, `http.response.status_code`, `server.port` (diz qual das três portas: cliente, cluster ou operação), `error.type`, e os `taba.*` das tabelas (`phase`, `outcome`, `peer`...). `peer` é o ID de outro nó, limitado pelo tamanho do cluster.

### Processo e host

Nomes padrão de `process.*`.

| Métrica | Tipo | Unidade | Descrição |
|---|---|---|---|
| `process.uptime` | gauge | s | Tempo desde o início do processo. |
| `process.cpu.utilization` | gauge | 1 | Uso de CPU do processo. |
| `process.memory.usage` | updowncounter | By | Memória total do processo (inclui a parcela fora do limite de mensagens). |
| `process.thread.count` | updowncounter | {thread} | Threads do sistema operacional do processo (runtime, pool de bloqueio e escritores SQLite; as tasks do tokio não entram — [18-estrutura-codigo.md](18-estrutura-codigo.md), seção 17.6). |
| `process.open_file_descriptor.count` | updowncounter | {file} | Descritores/arquivos abertos (relevante pelo overflow em disco: um arquivo por mensagem). |
| `taba.node.connections` | updowncounter | {connection} | Conexões HTTP abertas, por `server.port`. |

### Armazenamento e overflow (seção 3.2)

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.storage.memory.usage` | updowncounter | By | — | Memória ocupada por mensagens. |
| `taba.storage.memory.limit` | gauge | By | — | Limite configurado (`memory.limit`). |
| `taba.storage.memory.messages` | updowncounter | {message} | — | Mensagens mantidas em memória. |
| `taba.storage.disk.messages` | updowncounter | {message} | — | Mensagens em disco (overflow). |
| `taba.storage.disk.usage` | updowncounter | By | — | Espaço ocupado em disco pelas mensagens. |
| `taba.storage.disk.free` | gauge | By | — | Espaço livre no volume de `storage.dir`. |
| `taba.storage.overflow` | counter | {message} | — | Mensagens movidas de memória para disco. |
| `taba.storage.reads` | counter | {read} | `tier` (`memory`/`disk`) | Leituras por onde foram atendidas; `disk` é a leitura que não achou a mensagem em memória. Com as duas séries dá o *hit ratio*. |
| `taba.storage.disk.duration` | histogram | s | `operation` (`read`/`write`) | Latência de leitura/escrita em disco. |
| `taba.storage.disk.errors` | counter | {error} | — | Falhas de I/O em disco. |

### API HTTP (as três portas)

Convenção semântica de HTTP, para as **três portas** (cliente, cluster e operação), distintas por `server.port`. Não há contador de requisições: o número é o `_count` do histograma.

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `http.server.request.duration` | histogram | s | `http.request.method`, `http.route`, `http.response.status_code`, `error.type`, `url.scheme`, `server.port` | Latência das requisições recebidas; o `_count` é o total de requisições. Faixas (*buckets*) sugeridas pela convenção: 5 ms a 10 s. |
| `http.server.active_requests` | updowncounter | {request} | `http.request.method`, `url.scheme`, `server.port` | Requisições em andamento. |
| `http.server.request.body.size`, `http.server.response.body.size` | histogram | By | como a duração | Tamanho de corpo de requisição e de resposta. |
| `http.client.request.duration` | histogram | s | `http.request.method`, `http.route`, `http.response.status_code`, `error.type`, `peer` | Chamadas **deste nó a outros** (rotas `/cluster/*`); `peer` é o nó de destino. As chamadas recebidas pela porta do cluster aparecem em `http.server.*` com a porta do cluster. |
| `taba.ratelimit.rejected` | counter | {request} | `category` (`publish`/`consume`/`admin`/`ops`), `key` (`token`/`ip`) | Requisições rejeitadas com `429` ([12-limites-api.md](12-limites-api.md)). `key` diz se o contador que estourou era o do token ou o do IP; o `id` do token nunca é atributo de métrica. |
| `taba.auth.requests` | counter | {request} | `outcome` (`ok`/`unauthorized`/`forbidden`) | Autenticação por token ([06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3): requisições aceitas, recusadas com `401` e recusadas com `403`. Só existe com a autenticação ligada. O `id` do token é atributo de trace e de log, nunca de métrica. |
| `taba.auth.tokens` | gauge | {token} | — | Tokens de acesso que este nó conhece (devem ser iguais em todos os nós; agregue com `max`). |
| `taba.cluster.tls.expires_in` | gauge | s | — | Tempo até a expiração (`notAfter`) do certificado do cluster em uso. Só existe com `cluster.tls.dir` definido. Alerte antes de chegar a zero ([06-api.md](06-api.md), seção 8.0.1). |

### Escrita de mensagens (seção 3.1 e 7.2)

Lado **coordenador** (nó que recebeu a publicação):

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.write.coordinated` | counter | {message} | `outcome` (`committed`/`failed`) | Publicações coordenadas por este nó. |
| `taba.write.duration` | histogram | s | `phase` (`register`/`commit`/`total`) | Duração de cada fase e da escrita completa. |
| `taba.write.retries` | counter | {retry} | `phase`, `peer` | Retries por fase e por nó de destino — o registro concentra a maior parte das falhas. |
| `taba.write.rollbacks` | counter | {rollback} | — | Rollbacks enviados por falha em fechar o **quórum** (fase 1), após esgotar os retries — nunca por falha em réplica fora do quórum. |
| `taba.write.commit_timeouts` | counter | {timeout} | — | Vezes que `write.commit.timeout` esgotou sem commit confirmado numa réplica fora do quórum; o commit já decidido **não** é desfeito, e o retry continua em segundo plano. |
| `taba.write.replicas_confirmed` | histogram | {node} | `stage` (`response`/`eventual`) | Réplicas com commit confirmado por escrita, medido **na resposta ao cliente** (`response`, deve acompanhar `write.quorum`) e **eventualmente** (`eventual`, deve acompanhar `replication.factor`). |
| `taba.write.quorum_latency` | histogram | s | — | Tempo entre o início da escrita e o quórum confirmado — a latência que o cliente efetivamente espera. |

Lado **réplica** (nó responsável pelo bloco):

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.write.received` | counter | {message} | `phase` | Registros, commits e rollbacks recebidos. |
| `taba.write.pending` | updowncounter | {message} | — | Registros pendentes (sem commit). |
| `taba.write.pending.oldest_age` | gauge | s | — | Idade do registro pendente mais antigo (perto de `write.pending.ttl` indica commits não chegando). |
| `taba.write.pending.expired` | counter | {message} | — | Registros descartados por expiração (sem commit nem rollback). |

### Entrega e long polling ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md))

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.delivery.ready` | updowncounter | {message} | — | IDs nas listas de prontos deste nó (todos os tópicos). |
| `taba.delivery.fair.wait` | histogram | s | `band` (`0` a `14`, e `15+`) | Tempo entre a publicação e a **primeira entrega**, por faixa de tamanho do grupo da mensagem na hora da entrega (seção 12.11 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). É a prova de que a entrega justa funciona: as faixas baixas devem esperar menos que as altas. |
| `taba.delivery.redeliveries` | counter | {message} | — | Entregas de mensagens já entregues antes (`deliveryCount` > 1). |
| `taba.delivery.dead_lettered` | counter | {message} | — | Mensagens enviadas ao dead-letter por terem atingido `maxDeliveries` (seção 12.10). Crescimento contínuo indica mensagem que nenhum consumidor consegue confirmar. |
| `taba.retention.expired` | counter | {message} | — | Mensagens descartadas por `ttl` (varredura local, ou descartadas na entrega ou na sincronização). |
| `taba.fifo.deduplicated` | counter | {message} | — | Publicações em fila FIFO descartadas por deduplicação ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7). |
| `taba.fifo.forward` | counter | {request} | `outcome` (`ok`/`misdirected`/`error`) | Publicações FIFO repassadas ao primário do grupo (seção 16.4). `misdirected` é o `421` do primário que não é mais o dono; `error` é o primário que não respondeu. Conta no nó que repassa, uma vez por tentativa. Existe desde a etapa 15d. |
| `taba.fifo.groups.busy` | updowncounter | {group} | — | Grupos de filas FIFO com mensagem em voo neste nó (seção 16.6). |
| `taba.delivery.longpoll.waiting` | updowncounter | {request} | — | Chamadas de long polling aguardando neste nó. |
| `taba.delivery.longpoll.outcome` | counter | {request} | `outcome` (`delivered`/`empty`/`disconnected`) | Como terminaram as chamadas de long polling. |
| `taba.delivery.take` | counter | {request} | `direction` (`in`/`out`), `outcome` (`delivered`/`empty`/`error`) | Pedidos de entrega a outro dono (`/messages/take`). |
| `taba.cluster.channel.connections` | updowncounter | {connection} | — | Canais permanentes abertos com outros nós (esperado: nós do cluster − 1). |
| `taba.cluster.channel.notifications` | counter | {message} | `direction` (`in`/`out`), `outcome` | Avisos de mensagem nova enviados e recebidos (`outcome`: `sent`/`received` hoje; `fetched`/`empty` no lado que recebe, com o canal). |
| `taba.cluster.forwarded` | counter | {request} | `outcome` (`ok`/`timeout`/`error`/`no-destination`) | Chamadas de cliente que este nó, em drain, repassou a outro (seção 6.9 de [04-cluster.md](04-cluster.md)). |

### Membros do cluster e falhas (seção 6)

Visão do cluster: **todos os nós reportam a mesma coisa** (ou a sua visão dela), então o total é `max` ou `min`, não `sum`.

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.cluster.members` | gauge | {node} | `state` (`active`/`syncing`/`draining`/`suspect`, os estados do `Member`; os nós em freezing estão em `taba.cluster.freezing`) | Membros na tabela deste nó. |
| `taba.cluster.members.version` | gauge | {version} | — | Versão/época da tabela de membros; divergência entre nós indica propagação incompleta. |
| `taba.cluster.quorum.available` | gauge | {flag} | — | `1` se há nós saudáveis suficientes para votar (`cluster.vote.min-voters`). |
| `taba.cluster.channel.state` | gauge | {flag} | `peer` | `1` se o canal com o peer está conectado, `0` se caído (substitui o ping — seção 6.3). |
| `taba.cluster.channel.rtt` | histogram | s | `peer` | Latência do sinal de vida do canal, por peer. |
| `taba.cluster.channel.reconnects` | counter | {attempt} | `peer`, `outcome` | Tentativas de reconexão do canal e resultado; perto do limite de `cluster.channel.reconnect.retries` = votação iminente. |
| `taba.cluster.channel.queue.out` | updowncounter | {signal} | `peer` | Sinais na fila de saída para o peer; cresce enquanto o canal está fora (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Não há fila de entrada: a chamada do canal processa os quadros e só então devolve o `confirm`, então o trabalho lento aparece em `rtt`. |
| `taba.cluster.channel.queue.dropped` | counter | {signal} | `peer`, `reason` (`coalesced`/`expired`/`overflow`) | Sinais substituídos, descartados por idade ou descartados por estouro do limite da fila de saída (o estouro abre a votação de exclusão do peer). |
| `taba.cluster.vote` | counter | {vote} | `role` (`opened`/`cast`), `outcome` | Votações de exclusão abertas/votadas e seu resultado. |
| `taba.cluster.membership.changes` | counter | {change} | `operation` (`register`/`unregister`), `reason` (`vote`/`drain`, no desregistro), `phase`, `outcome` | Registros e desregistros de nós, por fase. |
| `taba.cluster.bootstrap.failures` | counter | {failure} | `reason` (`connectivity`, `phase1`, `phase2`, `frozen`; e `list`, `join` e `config`, do lado do nó que tenta entrar) | Falhas de bootstrap deste nó (teste de conexão, fase 1 ou fase 2 — seção 6.5). |
| `taba.cluster.freezing` | gauge | {node} | — | Nós atualmente em período de freezing (seção 6.7). |
| `taba.cluster.condition` | gauge | {flag} | `taba.condition` | `1` para cada condição vigente na visão **deste nó** (uma série por condição, `0` quando não vigora). `membership-divergent` só se vê comparando nós (`scope=cluster`), então aqui fica sempre `0` (D104). |
| `taba.node.state` | gauge | {flag} | `state` (`joining`/`active`/`syncing`/`returning`/`failed`) | `1` no estado atual **deste nó** (incluindo retorno após exclusão — seção 6.6, e `failed`: disco cheio ou banco corrompido, seção 7.3 de [05](05-particionamento-replicacao.md)). |
| `taba.node.drain.state` | gauge | {flag} | `state` (`active`/`draining`/`drained`) | `1` no estado do drain deste nó (seção 6.10 de [04-cluster.md](04-cluster.md)). |
| `taba.node.drain.pending` | updowncounter | {message} | — | Mensagens ainda em memória durante o drain; chega a 0 quando o nó está `drained`. |

A saúde de **cada nó** se vê pelas métricas do próprio nó (`up` no modo *pull*, ou a ausência de `process.uptime` no modo *push*) e por `taba.cluster.channel.state` dos vizinhos. Não há métrica que um nó emita sobre a coleta de outro.

### Anel, blocos e sincronização (seção 7)

| Métrica | Tipo | Unidade | Atributos | Descrição |
|---|---|---|---|---|
| `taba.ring.virtual_nodes` | gauge | {vnode} | — | Nós virtuais deste nó (`ring.virtual-nodes`). |
| `taba.ring.blocks` | updowncounter | {block} | `role` (`primary`/`secondary`/`tertiary`/...), `state` (`healthy`/`syncing`) | Blocos que o nó atende, por papel e integridade. Desequilíbrio entre nós indica má distribuição do anel. |
| `taba.sync.blocks.active` | updowncounter | {block} | — | Blocos em sincronização agora (entrega suspensa nesses blocos). Existem, desde a etapa 15a: `blocks.active`, `duration`, `ids.listed`, `messages.pulled`, `bytes` (só o corpo), `log.events`, `log.bytes`, `phase` e `failures` (`reason`: `no-source`, `disk`, `log-overflow`, `cancelled`); os `streams`, as retomadas e `taba.recover.*` esperam o `Transport` HTTP e a recuperação. |
| `taba.sync.duration` | histogram | s | — | Tempo para tornar um bloco íntegro. |
| `taba.sync.ids.listed` | counter | {id} | — | IDs consolidados (união) nas listas de origem. |
| `taba.sync.messages.pulled` | counter | {message} | `peer` | Mensagens obtidas por nó de origem — mostra se o balanceamento entre origens está uniforme. |
| `taba.sync.bytes` | counter | By | `peer` | Volume transferido na sincronização. |
| `taba.sync.log.events` | updowncounter | {event} | — | Eventos no banco de eventos (`sync.db`) ainda por processar durante a sincronização. |
| `taba.sync.log.bytes` | gauge | By | — | Tamanho do banco de eventos em disco. |
| `taba.sync.phase` | gauge | {flag} | `phase` (`topics`/`blocks`/`replay`/`verification`/`ready`) | `1` na fase atual da sincronização do nó. |
| `taba.sync.streams` | counter | {stream} | `kind` (`topics`/`ids`/`messages`/`recover`), `direction` (`sent`/`received`), `outcome` (`complete`/`truncated`/`error`) | Streams NDJSON de sincronização e recuperação (seção 8.2.1 de [06c-api-cluster.md](06c-api-cluster.md)); `truncated` é o que fechou sem a linha final. |
| `taba.sync.stream.resumes` | counter | {resume} | `kind` | Retomadas depois de um corte (não conta a paginação normal por `hasMore`). |
| `taba.sync.stream.duration` | histogram | s | `kind` | Duração de cada stream. |
| `taba.sync.stream.bytes` | counter | By | `kind`, `direction` | Volume trafegado nos streams. |
| `taba.recover.messages` | counter | {message} | `outcome` (`sent`/`applied`/`skipped`) | Mensagens enviadas, aplicadas ou ignoradas (já existentes) pela recuperação da seção 6.6 de [04-cluster.md](04-cluster.md). `sent` conta no nó que volta; `applied` e `skipped`, em cada nó que grava. Existe desde a etapa 15c. |
| `taba.sync.failures` | counter | {failure} | `reason` | Falhas de sincronização de bloco. |
| `taba.cluster.recover` | counter | {message} | `direction` (`sent`/`received`) | Mensagens recuperadas na volta de um nó (seção 6.6). `received` conta as aplicadas e as ignoradas. Existe desde a etapa 15c. |

### Equivalência com os nomes anteriores

Os nomes `msg.*` da primeira versão da proposta foram substituídos. Ao ler um documento ou um caso de uso antigo, vale:

| Antes | Agora |
|---|---|
| `msg.node.uptime`, `cpu.utilization`, `process.memory`, `threads`, `open_files` | `process.uptime`, `process.cpu.utilization`, `process.memory.usage`, `process.thread.count`, `process.open_file_descriptor.count` |
| `msg.node.connections` | `taba.node.connections` |
| `msg.api.requests` | o `_count` de `http.server.request.duration` |
| `msg.api.duration`, `in_flight`, `request.size`, `response.size` | `http.server.request.duration`, `http.server.active_requests`, `http.server.request.body.size`, `http.server.response.body.size` |
| `msg.cluster.rpc.requests`, `rpc.duration` | `http.client.request.duration` (saída) e `http.server.*` com a porta do cluster (entrada) |
| `msg.api.rate_limited` | `taba.ratelimit.rejected` |
| `msg.storage.read.disk`, `read.memory` | `taba.storage.reads` com `tier` |
| `msg.cluster.node.scrape_up`, `metrics_age` | removidas: não há coleta de um nó por outro |
| `msg.cluster.node.state` | `taba.node.state` |
| `msg.<área>.*` (demais) | `taba.<área>.*`; as de visão do cluster ficam em `taba.cluster.*` |
| atributos `route`, `method`, `status_class` | `http.route`, `http.request.method`, `http.response.status_code` |
| `msg.sync.phase` com `topics`, `blocos`, `verificacao`, `pronto` | `taba.sync.phase` com `topics`, `blocks`, `verification`, `ready` |

### Consulta

As métricas acima **não têm rota JSON própria**: leem-se em `GET /metrics` (este nó) e no OTLP. Para ver o estado e as condições sem backend, use `GET /health/cluster`:

- `GET /health/cluster` — com `scope=cluster` (padrão), consulta **todos os nós** (pela rota interna `GET /cluster/status`) e devolve o estado de cada um, um resumo (membros, quórum, blocos em sincronização) e as **condições** do cluster. Nós que não respondem aparecem como `reachable: false` em vez de falhar a chamada inteira (*best-effort*).
- `GET /health/cluster?scope=node` — o **nó que atende a chamada**, leitura local, sem consulta a outros nós, e as condições que ele mesmo consegue derivar.
- Toda resposta traz `collectedAt` (instante da coleta). O DTO está em [06b-api-operacao.md](06b-api-operacao.md).

## Condição do cluster

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): há um só nó; `GET /health/cluster` devolve a mesma coisa nos dois `scope`, e as métricas `taba.cluster.*` (canal, votação, membros, freezing) não existem.

Derivada do estado de cada nó, exposta em `GET /health/cluster` (não em `GET /health/ready`, que segue o [doc 06b](06b-api-operacao.md)) e também como métrica `taba.cluster.condition` ([Endpoint Prometheus](#endpoint-prometheus)). **Condições, não um estado único** — o cluster pode ter mais de uma ao mesmo tempo:

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo. |
| `member-unreachable` | aviso | Canal com algum nó caído ou em reconexão, ainda antes da votação. |
| `node-frozen` | aviso | Nó(s) em período de freezing. |
| `syncing` | aviso | Blocos em sincronização; a entrega está suspensa neles (seção 7.3). |
| `under-replicated` | aviso | Blocos com menos réplicas íntegras que `replication.factor`, mas ainda **no ou acima do `write.quorum`** — estado normal e transitório (réplica alcançando o commit em segundo plano, seção 7.2), escritas continuam respondendo ao cliente normalmente. |
| `write-quorum-at-risk` | crítica | Blocos com réplicas íntegras **abaixo do `write.quorum`**, ou com o **primário** fora do ar e ainda na tabela (o quórum sempre o inclui, seção 7.2 de [05](05-particionamento-replicacao.md); D107) — escritas podem falhar por não conseguir fechar quórum algum, até a exclusão do nó. |
| `quorum-lost` | crítica | Menos nós saudáveis que o mínimo de votantes (`cluster.vote.min-voters`); nenhuma exclusão de nó consegue ser efetivada. Não confundir com `write-quorum-at-risk`, que é sobre durabilidade de mensagens, não sobre votação de membros. |
| `membership-divergent` | crítica | Tabelas de membros com versões diferentes entre nós. |
| `storage-pressure` | aviso | Disco de `storage.dir` abaixo de um limite de espaço livre. |

> **Como o nó calcula (provisório, D102 a D104).** *Saudável* é este nó mais os pares com o canal conectado. `quorum-lost` vale quando os saudáveis não alcançam `min(cluster.vote.min-voters, membros)` (com um nó só não há quem excluir, então não é `quorum-lost`). Uma réplica é *íntegra* quando o dono do bloco está `active` na tabela e o canal com ele está de pé (o próprio nó conta); o esperado é o número de donos que o anel dá, `min(replication.factor, membros)`. `under-replicated` aparece quando há mais blocos abaixo da replicação que abaixo do `write.quorum`, e `syncing`, quando algum dono de bloco está `syncing`. `write-quorum-at-risk` vale também quando o primário do bloco está `active` na tabela mas com o canal fora (D107); um primário que só sincroniza não conta. `membership-divergent` compara a versão da tabela de cada nó em `GET /health/cluster?scope=cluster`: o sinal de vida chega com até `keepalive.interval` de atraso e faria a condição piscar a cada mudança de membros. **Limite conhecido:** o quórum de escrita sempre inclui o primário, então os blocos cujo primário está fora do ar (e ainda não foi excluído) não escrevem, mas a condição só diz `under-replicated` (PRB-044, D107).

> **Degradação de replicação é propriedade do cluster, não do tópico.** Os blocos são definidos pelo **ID da mensagem** (seção 7.2), não pelo tópico: as mensagens de um mesmo tópico se espalham por todos os blocos. Por isso `under-replicated` e `syncing` aparecem aqui e não na condição de tópico.

## Métricas de tópico (rotas próprias)

Ficam **fora** da exportação padrão e são consultadas por rotas dedicadas, pensadas para funcionar bem com **muitos tópicos**:

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics/topics` | Lista **paginada** com as métricas resumidas de cada tópico. |
| `GET` | `/metrics/topics/{topic}` | Métricas detalhadas de **um** tópico. |

> A fila de dead-letter de cada fila é um tópico comum para monitoramento: aparece aqui e em `/topics`, com `deadLetter: true` e `source`, e herda as `tags` da fila de origem (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).

> `/metrics` devolve o texto do Prometheus (só este nó, sem parâmetros) e as subrotas `/metrics/topics/*` devolvem JSON paginado: o Prometheus raspa apenas `/metrics` e não segue as subrotas.

### Parâmetros de `GET /metrics/topics`

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
| | `oldestMessageAt` | **Data de publicação da mensagem mais antiga** ainda não confirmada (visível ou invisível); na dead-letter, a data em que a mensagem entrou nela (de onde o `ttl` conta). Ausente se o tópico não tem mensagens. |
| | `oldestAge` | Derivado: `agora − oldestMessageAt`. O melhor indicador de consumidor parado. |
| | `bytes` | Bytes das mensagens ainda não confirmadas (`memoryBytes + diskBytes`). |
| **Armazenamento** | `memoryMessages` / `diskMessages` | Mensagens em memória e em disco. |
| | `memoryBytes` / `diskBytes` | Bytes em memória e em disco. |
| | `memoryShare` | Parcela da memória do cluster que o tópico ocupa (resultado da divisão dinâmica). |
| **Grupos** | `groups` | Grupos de mensagens acompanhados (seção 12.11 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Na visão do cluster, é a soma dos nós, um limite superior (o mesmo grupo pode ter mensagens em vários nós). |
| | `topGroups` | Os maiores grupos, com os campos de grupo abaixo, até `stats.topics.top-groups`. Só em `GET /metrics/topics/{topic}`. |
| | `noisyShare` | Parcela de `depth` que o maior grupo ocupa. Base da condição `noisy-group`. |
| **Identificação** | `type`, `tags`, `createdAt` | Tipo (`queue`/`distributor`), `id`, tags e criação; em fila volátil, também `volatile` e `idleTimeout`. |

Um Tópico Distribuidor não guarda mensagens: as métricas de fila (profundidade, idade, armazenamento) se aplicam às **filas destino**, e o distribuidor expõe apenas publicações recebidas, cópias geradas e falhas por fila. Não há métricas por assinante. Ver [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.7.

### Métricas por grupo

Para tópicos com `groupId` (seção 12.11 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)), a rota `GET /metrics/topics/{topic}/groups` devolve as métricas **de cada grupo**. Cada nó conta só as mensagens de que é dono, e os donos de um tópico não se repetem, então a **soma dos nós é exata** para as contagens e as taxas, e a data mais antiga é a menor entre os nós.

| Campo | Descrição |
|---|---|
| `groupId` | O grupo. O contador compartilhado dos grupos acima de `delivery.fair.max-groups` aparece como uma linha sem `groupId` e com `overflow: true`. |
| `messages`, `visibleMessages`, `invisibleMessages` | Mensagens do grupo ainda não confirmadas, prontas e em processamento. |
| `share` | Parcela do `depth` do tópico que o grupo ocupa. |
| `band` | Faixa de tamanho do grupo (seção 12.11); só na visão `scope=node`, pois a faixa é local de cada dono. |
| `oldestMessageAt`, `oldestAge` | Data e idade da mensagem mais antiga do grupo. |
| `publishRate`, `consumeRate`, `ackRate` | Taxas do grupo, na janela `memory.rate.window`. |

A mensagem sem `groupId` pertence ao grupo de nome vazio (`groupId: ""`). Parâmetros: `limit` e `cursor` (paginação por cursor, ordenada por `groupId`, sem `sort`, pelo mesmo motivo de `/metrics/topics`), `groupId` (um grupo exato), `top=N` (os `N` maiores por `messages`, **sem paginação**, é a forma de achar o barulhento), `scope` (`cluster` por padrão, ou `node`). Um grupo sem mensagens e sem atividade na janela deixa de ser acompanhado e some da resposta.

**Sem série por grupo no OTLP nem no Prometheus:** o número de grupos é ilimitado, e uma série por grupo estouraria a cardinalidade (mesmo motivo da seção "Exportação das métricas de tópico"). A justiça é medida por faixa, que tem cardinalidade pequena: `taba.delivery.fair.wait`, acima.

### Condição do tópico

Também uma **lista de condições**, com o campo `worstCondition` (na API, [06b-api-operacao.md](06b-api-operacao.md)) sendo a de maior severidade. Limites configuráveis ([10-configuracao.md](10-configuracao.md#observabilidade)):

| Condição | Severidade | Quando |
|---|---|---|
| `ok` | — | Nenhuma das abaixo. |
| `idle` | info | Sem publicação nem consumo há `stats.topics.idle-after`. |
| `backlog` | aviso | `depth` ou `oldestAge` acima do limite. |
| `stalled` | aviso | Há mensagens (`depth > 0`) mas **nenhum consumo** há `stats.topics.stalled-after` — consumidor parado ou ausente. |
| `noisy-group` | info | Há dois ou mais grupos e o maior ocupa mais de `stats.topics.noisy-group-share` do `depth` (padrão 50%). O grupo está em `topGroups`. Numa fila FIFO indica também o **grupo quente**, que concentra a fila num só primário ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.10). |
| `group-stalled` | aviso | Só em fila FIFO: a cabeça de algum grupo já foi entregue mais de `stats.topics.group-stalled-deliveries` vezes sem ack, travando o grupo. O grupo está em `topGroups`. |
| `memory-pressure` | info | Maior parte das mensagens do tópico está em disco (`diskMessages` > `memoryMessages`; a razão exata é a D90): leituras mais lentas. |

Um limite de `stats.topics.*` que não tem valor desliga a condição que depende dele (`backlog`, `stalled`, `idle`, `group-stalled`); `noisy-group` e `memory-pressure` não dependem de limite próprio (D90).

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
- Cada nó mantém **contadores** dessas quantidades, atualizados nas transições (publicação: +1 visível; entrega: −1 visível, +1 invisível; vencimento do prazo: −1 invisível, +1 visível; ack: −1 no estado em que estava), sem varrer mensagens a cada consulta. A data mais antiga é a menor entre a mais antiga das visíveis e a mais antiga das invisíveis; como achá-la sem varredura é o D35 ([08b-pendencias.md](08b-pendencias.md)).
- **Nó sem mensagens do tópico:** `oldestMessageAt` fica **ausente** (sem valor), nunca zero — um zero venceria o "mais antigo" do cluster.
- **Nó que não responde** fica de fora e o resultado é marcado `partial`: as somas viram **piso** (o cluster tem ao menos aquilo) e a data mais antiga pode ser mais recente que a real.
- **Como consultar cada visão:** `GET /metrics/topics` e `GET /metrics/topics/{topic}` devolvem a visão do **cluster** por padrão; com `scope=node` devolvem a visão **só do nó que atende**, sem consulta a outros nós. `groupBy` por tag aplica as mesmas regras (soma das quantidades, menor data) sobre os tópicos do grupo.

### Como o valor é calculado (custo)

- As mensagens de um tópico se espalham pelos blocos do anel; cada mensagem existe em `replication.factor` nós. Portanto o valor do tópico no cluster é a **soma entre nós**, contando apenas a **réplica primária** de cada mensagem — do contrário, tudo apareceria multiplicado pelo fator de replicação.
- Cada nó mantém contadores **locais** por tópico. Uma rota interna (`GET /cluster/metrics/topics`) devolve esses parciais; o nó que atende `/metrics/topics` **consulta todos os nós, soma e devolve**.
- Esse fan-out é justamente o custo que justifica isolar essas métricas: com muitos tópicos, o payload de cada nó cresce. Mitigações: o resultado é mantido em **cache** por `stats.topics.cache-ttl` (a resposta traz `collectedAt`); o filtro e o `limit` são aplicados depois do merge; `fields` reduz o tamanho; e `GET /metrics/topics/{topic}` pede aos nós só aquele tópico.
- Os valores são uma **fotografia aproximada** (leitura sem lock global, nó que não responde fica de fora e é listado como `partial`), não um valor transacional.

## Exportação das métricas de tópico (OTLP e Prometheus)

Opt-in por `telemetry.topics.mode`, com o **mesmo efeito** no OTLP e em `GET /metrics`:

| Modo | Padrão | O que exporta | Cardinalidade |
|---|---|---|---|
| `off` | ✔ | Nada por tópico; só as rotas `/metrics/topics`. | Nenhuma |
| `by-tags` | | Métricas agregadas **por combinação de tags** escolhidas em `telemetry.topics.group-by-tags` (ex.: `time`, `ambiente`), no atributo `taba.topic.tag.<chave>`. | Limitada pelos valores das tags, **não** pelo número de tópicos |
| `per-topic` | | Uma série por tópico, com o atributo `messaging.destination.name` (mais as tags). Limitada por `telemetry.topics.max-topics`; acima do teto, os tópicos excedentes são agregados em `messaging.destination.name="_other"`. | Limitada pelo teto |

Métricas exportadas nos modos `by-tags` e `per-topic` (subconjunto das métricas por tópico, escolhidas por serem contadores/gauges baratos): `taba.topic.published`, `taba.topic.consumed`, `taba.topic.acked` (counters, `{message}`), `taba.topic.visible`, `taba.topic.invisible`, `taba.topic.depth` (updowncounters, `{message}`), `taba.topic.oldest_timestamp` (gauge, `s`, segundos Unix da mensagem mais antiga; **sem série** no nó que não tem mensagens do tópico), `taba.topic.memory_bytes`, `taba.topic.disk_bytes` (updowncounters, `By`). Além do teto do Taba, o próprio SDK do OpenTelemetry limita a cardinalidade de qualquer métrica e marca o excesso com `otel.metric.overflow`.

## Endpoint Prometheus

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics` | Métricas **deste nó**, no formato de texto do Prometheus (`text/plain; version=0.0.4`; OpenMetrics quando pedido via `Accept`). |

### Modelo: um alvo por nó

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): o Prometheus lê o único nó, sem métricas `taba.cluster.*`.

É o modelo padrão de qualquer exporter: **cada nó expõe as suas métricas**, e quem coleta (Prometheus ou Collector) descobre os nós. O nó **não consulta outros nós** para montar a resposta, e não existe métrica agregada do cluster no servidor.

- **Descoberta.** Em Kubernetes, por um *Service headless* ou pelos pods (`ServiceMonitor` ou anotações); em Docker ou VMs, por DNS ou por lista. O alvo é a porta que serve o `/metrics` (`operations.port`, ou `api.port` se não houver porta de operação; [06-api.md](06-api.md), seção 8.0), **não** a porta do cluster.
- **Cluster inteiro.** O Prometheus guarda uma série por nó, com `instance` (o ID do nó, de `service.instance.id`) e `job`. O total sai por agregação: `sum(rate(taba_write_coordinated_total[5m]))` soma o trabalho dos nós; `max(taba_cluster_members)` pega a visão do cluster (todos os nós a veem igual). Somar no servidor quebraria os contadores (se um nó faltasse, o `rate()` leria um reinício falso), por isso a soma é sempre do consumidor.
- **Nó fora do ar.** É o `up` do próprio alvo e, para os vizinhos, `taba_cluster_channel_state == 0`.
- **Quem prefere *push*.** O OTLP envia a mesma coisa sem lista de alvos ([Como ligar um coletor](#como-ligar-um-coletor)).
- A agregação de tópicos continua válida: os contadores de tópico contam **apenas as mensagens em que o nó é primário** (mesma regra de [Como o valor é calculado](#como-o-valor-é-calculado-custo)), então `sum(taba_topic_visible) by (messaging_destination_name)` e `sum(taba_topic_invisible) by (messaging_destination_name)` dão os valores do cluster sem multiplicar pelo fator de replicação, e `min(taba_topic_oldest_timestamp_seconds) by (messaging_destination_name)` dá a data da mensagem mais antiga.

### Conteúdo

- **Métricas de nó:** todas as da seção [Métricas de nó](#métricas-de-nó), sempre.
- **Métricas de tópico:** conforme `telemetry.topics.mode` — `off` (padrão) não emite nenhuma; `by-tags` e `per-topic` seguem as regras e o teto de cardinalidade já descritos. Ou seja, **o `/metrics` também não escala com o número de tópicos por padrão**.
- **Condições** como *gauges* `0/1` com a unidade `{flag}`: `taba_cluster_condition{taba_condition="under-replicated"} 1`; no tópico (`taba_topic_condition`), apenas nos modos que exportam tópicos.
- `target_info` com os atributos de recurso (versão, ambiente), no estilo OpenMetrics.

### Mapeamento de nomes (OTel → Prometheus)

Segue a especificação de compatibilidade do OpenTelemetry com o Prometheus.

| Regra | Exemplo (OTel → Prometheus) |
|---|---|
| Pontos viram `_` | `taba.storage.memory.limit` → `taba_storage_memory_limit_bytes` |
| Unidade vira sufixo | `s` → `_seconds`, `By` → `_bytes`; razão `1` → `_ratio` (**só** para razões de verdade, como `process.cpu.utilization`) |
| Unidade anotada entre chaves some | `{message}`, `{flag}`, `{node}` não geram sufixo: `taba.cluster.quorum.available` → `taba_cluster_quorum_available` |
| Contadores ganham `_total` | `taba.write.retries` → `taba_write_retries_total` |
| Histogramas viram `_bucket`/`_sum`/`_count` | `taba.write.duration` → `taba_write_duration_seconds_bucket{taba_phase="commit",le="0.05"}` |
| Atributos viram rótulos | `taba.phase` → `taba_phase`; `http.route` → `http_route` |
| Recurso vira `job`, `instance` e `target_info` | `service.namespace/service.name` → `job`; `service.instance.id` → `instance` |

Regras de rótulo iguais às do OTLP: `http.route` é sempre o **template** (`/topics/{topic}/messages`), nunca o caminho real; o ID da mensagem nunca é rótulo; o tópico (`messaging.destination.name`) só aparece no modo `per-topic`.

### Configuração e exposição

- `telemetry.prometheus.enabled` liga/desliga o endpoint.
- Porta e restrição de acesso do `/metrics` são as das demais rotas de operação: ver [Acesso às rotas de operação](#acesso-às-rotas-de-operação).

### Exemplos de alertas (PromQL)

Sugestões de partida, a calibrar em produção:

| Alerta | Expressão |
|---|---|
| Nó fora do ar | `up{job="taba"} == 0` (ou o `job` do cluster) |
| Cluster sem quórum | `min(taba_cluster_quorum_available) == 0` |
| Tabelas de membros divergentes | `max(taba_cluster_members_version) != min(taba_cluster_members_version)` |
| Memória perto do limite | `taba_storage_memory_usage_bytes / taba_storage_memory_limit_bytes > 0.9` |
| Falhas de escrita | `sum(rate(taba_write_coordinated_total{taba_outcome="failed"}[5m])) > 0` |
| Registros pendentes envelhecendo | `max(taba_write_pending_oldest_age_seconds) > 30` |
| Certificado do cluster perto de expirar | `min(taba_cluster_tls_expires_in_seconds) < 30 * 86400` |

## Acesso às rotas de operação

As **rotas de operação** são `GET /metrics`, `GET /metrics/topics/*`, `GET /health/*` (inclusive `/health/cluster`) e `/node/drain` (`POST`, `GET` e `DELETE`). Ao contrário da API de cliente, são destinadas a operadores, ao Prometheus e a orquestradores/balanceadores. Duas restrições **opcionais e independentes** podem ser configuradas para elas:

| Parâmetro | Efeito |
|---|---|
| `operations.port` | Serve as rotas de operação em uma **porta própria**. Nessa configuração, elas **deixam de existir** na porta da API de cliente (que responde `404` para esses caminhos). |
| `operations.allowed-ips` | **Lista de IPs e faixas (CIDR)**, ex.: `10.0.0.0/8, 192.168.1.10`, autorizados a acessar essas rotas. Requisições de fora da lista recebem `403 Forbidden`. Vale em qualquer porta que sirva as rotas. |

As duas podem ser combinadas (porta própria **e** lista de IPs), usadas isoladas, ou nenhuma:

| Configuração | Comportamento |
|---|---|
| Nenhuma (padrão) | As rotas ficam na **mesma porta da API de cliente**, sem restrição de origem, e seguem as regras de rate limit próprias das rotas de operação ([12-limites-api.md](12-limites-api.md): `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` isentos, `/health/cluster?scope=cluster` e `/metrics/topics` em categoria própria) e qualquer autenticação que venha a existir. |
| Só `operations.port` | Porta separada, sem filtro por IP: quem alcança a porta acessa. |
| Só `operations.allowed-ips` | Mesma porta da API, mas só os IPs da lista acessam essas rotas; a API de cliente segue aberta. |
| Ambas | Porta separada, acessível apenas pelos IPs/faixas listados. |

Detalhes:

- Recomendação para produção: ao menos a lista de IPs (Prometheus, balanceadores, faixa dos operadores); idealmente também a porta própria, pois o `/metrics` e o `/metrics/topics` expõem nomes de tópicos e tags, e o `/health/cluster` expõe a topologia do cluster.
- A lista deve incluir os IPs do **balanceador** que chama `/health/ready` e do **Prometheus ou Collector** que faz o scrape.
- O IP avaliado é o **IP do cliente resolvido** pela mesma regra do rate limit ([12-limites-api.md](12-limites-api.md)): o endereço da conexão ou, com `client-ip.headers.enabled`, o cabeçalho (`X-Forwarded-For`), aceito só de quem estiver em `client-ip.trusted-proxies`. **Sem a lista configurada, o cabeçalho é aceito de qualquer origem**, então um cliente com acesso direto ao nó consegue forjá-lo e burlar a lista de IPs. Essas rotas **não têm autenticação** (decisão D63): quem instala as protege com a porta própria e a rede (grupo de segurança). Elas devem ficar numa **camada de rede segura** (rede privada, porta própria atrás de firewall); a lista de IPs é uma restrição a mais, não a proteção principal.
- O envio por OTLP é uma conexão **de saída** do nó para o coletor e não passa por essas restrições.
- As rotas internas entre nós (`/cluster/...`, inclusive `/cluster/status`) **não** são rotas de operação: ficam na **porta do cluster** (`cluster.port`, [06-api.md](06-api.md), seção 8.0), que é separada da API de cliente e da porta de operação, e não são afetadas por essas configurações.
- O nó **não sobe** se `operations.port` estiver ocupada ou se a lista tiver entrada inválida — falha explícita no início, em vez de expor as rotas sem restrição por engano.

## Traces

Um trace por operação de cliente, com spans internos para cada etapa relevante. Os spans HTTP seguem a convenção semântica (nome `{método} {rota}`, atributos `http.*`), e os de mensageria levam `messaging.system=taba`, `messaging.destination.name` (o tópico), `messaging.operation.type` (`send`, `receive`, `settle`) e `messaging.message.id`.

- **Publicação:** span raiz `POST /topics/{topic}/messages` → span de registro (um por nó de destino) → span de commit (um por nó de destino) → rollback, se a falha foi em fechar o **quórum**. O span raiz **termina com sucesso assim que o quórum confirma o commit** (sempre incluindo o primário) — não espera as demais réplicas. Os spans de registro/commit das réplicas fora do quórum continuam **fora do trace já encerrado**; se `write.commit.timeout` esgota numa delas, isso não reabre nem afeta o span raiz, só gera o evento de observabilidade correspondente (`taba.write.commit_timeouts`). O contexto de trace é propagado nas chamadas internas entre nós (cabeçalho W3C `traceparent`), de modo que a escrita completa aparece em um único trace mesmo passando por vários nós.
- **Consumo e ack:** span raiz da requisição → leitura em memória ou disco.
- **Sincronização de bloco:** um trace por bloco sincronizado, com um span por lote de `GET`, indicando o nó de origem.
- **Registro/desregistro de nó e votação:** um trace por operação, com um span por nó consultado.
- **Atributos dos spans:** `taba.topic.tag.<chave>`, `messaging.message.id`, ID do bloco (`taba.block.id`), IDs dos nós envolvidos (`taba.node.id`, `taba.peer.id`), número da tentativa (`taba.attempt`). Como o ID da mensagem é um GUID v7 de alta cardinalidade, ele vai em **traces e logs, nunca em métricas**.
- **Nomes dos spans do nó:** `{método} {rota}` na requisição de cliente (as rotas `/health`, `/metrics`, `/node` e `/cluster` não geram trace); `taba.write.register`, `taba.write.commit` e `taba.write.rollback` (um por nó de destino, com `taba.peer.id` e, no registro, `taba.attempt`); `taba.sync.block` (um trace por bloco, com `taba.block.id`) e `taba.sync.fetch` (uma chamada a uma origem, com `taba.peer.id`); `taba.cluster.join`, `taba.cluster.unregister` e `taba.vote.open` como raiz da operação, com `taba.cluster.register`, `taba.cluster.commit`, `taba.cluster.rollback` e `taba.vote.request` por nó consultado (`taba.node.id` é o nó que entra, sai ou é votado). O span que falha leva `otel.status_code=ERROR`. O `traceparent` de uma chamada de cliente é respeitado, e o `Transport` HTTP leva o contexto do span corrente ao outro nó no mesmo cabeçalho: a chamada recebida (`{método} {caminho}`, só quando chega com `traceparent`) continua o mesmo trace. O sinal de vida e as demais tarefas de fundo, sem span, não geram trace (D119).
- **Amostragem:** o SDK usa `OTEL_TRACES_SAMPLER` (padrão `parentbased_traceidratio`, fração 0,1: D117). Os spans de falha e de evento de durabilidade (erro ao cliente, rollback, teto de commit esgotado numa réplica, perda de mensagem) levam o atributo `taba.durability_event=true`, e o **Collector** os mantém sempre (amostragem por cauda, seção [Como ligar um coletor](#como-ligar-um-coletor)).

## Logs

Logs **estruturados** (JSON, ou OTLP) com o mesmo contexto do trace (`trace_id`, `span_id`) e as tags do tópico quando houver. A severidade usa os níveis do OpenTelemetry (`debug`, `info`, `warn`, `error`). Eventos de log esperados:

| Evento | Nível | Origem |
|---|---|---|
| Falha de registro/commit de mensagem (por tentativa) | `warn` | Escrita (seção 7.2) |
| Escrita abortada e rollback enviado (falha em fechar o quórum) | `error` | Escrita (seção 3.1) |
| Registro pendente expirado | `warn` | Escrita (seção 3.1) |
| Teto de commit esgotado numa réplica fora do quórum, sem rollback — retry continua em segundo plano | `warn` | Escrita (seção 7.2) |
| Canal com um nó caiu, reconectou ou esgotou as tentativas; votação aberta, voto e resultado | `warn` / `info` | Cluster (seção 6.3) |
| Nó registrado, desregistrado, rejeitado (freezing) | `info` | Cluster (seções 6.2, 6.7) |
| Token criado, alterado ou revogado (com o `id`, nunca o segredo) | `info` | API de cliente (seção 8.1.3 de [06a-api-cliente.md](06a-api-cliente.md)) |
| Chamada recusada por falta ou excesso de permissão (`401`/`403`), com o `id` do token quando houver; em volume, amostrada | `warn` / `debug` | API de cliente (seção 8.1.3) |
| Drain iniciado (por sinal ou API), concluído, cancelado, ou nó que volta do drain como novo | `info` | Cluster (seção 6.10) |
| Partida em modo desenvolvimento: retomada do disco (e se havia save concluído) ou limpeza de `storage.dir` | `info` / `warn` | Cluster (seção 6.10) |
| Drain que estourou `drain.timeout`, com o que sobrou em memória, ou com erro de I/O | `error` | Cluster (seção 6.10) |
| Falha de bootstrap do nó | `error` | Cluster (seção 6.5) |
| Início e fim de sincronização de bloco | `info` | Sincronização (seção 7.3) |
| **Perda de mensagem** (ID, bloco, nós consultados) | `error` | Sincronização (seção 7.3) |
| Retorno de nó e mensagens recuperadas | `info` | Cluster (seção 6.6) |
| Overflow de memória para disco (resumo periódico, não por mensagem): evento `taba.storage.overflow`, com `messages` e `window_secs`, a cada 60 s e só quando houve overflow (D118) | `info` | Armazenamento (seção 3.2) |
| Erro de I/O em disco | `error` | Armazenamento |

Pelo OTLP cada log vai como *log record* com `trace_id` e `span_id` do span em que aconteceu, severidade do OpenTelemetry e os campos do evento (inclusive `event.name`) como atributos; o log em JSON no `stdout` ainda não leva `trace_id` nem `span_id` (PRB-050). O nível de log é configurável (`telemetry.logs.level`). Logs por mensagem em caminho quente (publicação, consumo) ficam em `debug` para não pesar na latência.

## Health checks

Rotas leves para orquestradores e balanceadores, **sem** fan-out para outros nós:

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/health/live` | O processo está de pé e respondendo. Falha → reiniciar o processo. |
| `GET` | `/health/ready` | O nó está **apto a receber tráfego**: registrado no cluster (commit confirmado), com a **sincronização concluída** (tabela de tópicos, todos os blocos e o banco de eventos já processados, seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) e não em bootstrap nem em retorno pós-exclusão. **Em drain (`draining` ou `drained`, seção 6.10 de [04-cluster.md](04-cluster.md)) também responde `503`**, para o balanceador tirar o nó da rotação. Falha → o balanceador deve tirar o nó da rotação. |

`/health/ready` só passa a responder com sucesso quando o nó termina a sincronização inteira: enquanto sincroniza, o nó apenas guarda os eventos no banco de eventos e repassa as chamadas de cliente a outro nó (seção 6.9 de [04-cluster.md](04-cluster.md)); mesmo assim deve ficar fora da rotação do balanceador, para não gastar o salto extra à toa. Depois de pronto, ele volta a atender normalmente.

## Eventos

Eventos pontuais, **não** condições de saúde do nó nem do cluster. São **log records** do OpenTelemetry com o campo `event.name` (nome em minúsculas, com pontos), que o backend filtra e agrega. Um evento sempre leva o tópico em `messaging.destination.name`. Os eventos de distribuidor e de fila volátil estão em [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.7.

| `event.name` | Quando |
|---|---|
| `taba.distributor.no_target` | Publicação num Tópico Distribuidor sem nenhuma fila casando (sucesso, mensagem descartada). |
| `taba.distributor.partial` | Ao menos uma cópia de uma publicação em DT falhou. |
| `taba.volatile.deleted` | Fila volátil excluída por ociosidade. |
| `taba.volatile.check_blocked` | O gestor não concluiu a verificação final porque algum nó não respondeu. |
| `taba.message.dead_lettered` | Mensagem enviada ao dead-letter ao atingir `maxDeliveries`. Atributos: fila, ID, `deliveryCount`. |
| `taba.message.deduplicated` | Publicação em fila FIFO descartada por deduplicação. Atributos: fila, `groupId`, `deduplicationId`, ID da mensagem original. Em volume, use a métrica `taba.fifo.deduplicated`. |
| `taba.message.expired` | Mensagem descartada por `ttl`. Atributos: fila, ID, idade. Em volume, pode ser amostrado ou agregado pela métrica `taba.retention.expired`. |
| `taba.message.lost` | **Perda de mensagem** (seção 7.3): decidido que vai para o log em `error`, com o **nome do tópico**, o **ID da mensagem**, o bloco e os nós consultados, e para a tabela `losses` do banco do nó, consultada pelo nó que volta de uma exclusão (seção 6.6 de [04-cluster.md](04-cluster.md)). Não há métrica nem condição de saúde por enquanto; quem quiser alertar usa o evento no backend de logs. O sistema não envia notificações por conta própria. |

## Ainda em aberto

As pendências desta área ficam em [08b-pendencias.md](08b-pendencias.md), no passo 10.
