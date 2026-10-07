# Parâmetros de configuração

[← Índice](README.md)

Lista dos parâmetros configuráveis citados na documentação e seus valores padrão. **A definir** significa que o parâmetro é configurável, mas o padrão ainda não foi decidido (ver [pendências](08b-pendencias.md)).

> Os nomes da coluna "Chave sugerida" são propostas; ainda não há contrato de configuração definido.

Todo parâmetro pode ser definido por **variável de ambiente**. Convenção: prefixo `TABA_`, chave em maiúsculas com `.` e `-` trocados por `_` (ex.: `topics.distributor.max-targets` vira `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`). **Sem limite** significa que o teto não existe enquanto a variável não for definida.

## Modo de execução

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Retomada do disco (desenvolvimento) | `development.resume-on-start` | **true** | Só em `mode=development`: na partida, o nó retoma mensagens, tópicos e estado salvos em `storage.dir` (o drain serve de save). `false` **apaga** o conteúdo de `storage.dir` e começa limpo. Ignorado em `production`. | 6.10 |
| Modo de execução | `mode` | **`production`** | `production` ou `development`. Em `development` o nó é um cluster de um só: sem bootstrap, canal entre nós, votação, freezing, replicação nem sincronização; `replication.factor` e `write.quorum` valem 1; `cluster.entry-address` não é exigido; as rotas `/cluster/*` ficam desabilitadas. Os parâmetros `cluster.*`, `ring.*`, `replication.*` e `write.quorum` são **ignorados** nesse modo (aviso no log). Não é para produção. | 6.4 |

## Parada e modo drain

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Drain ao receber sinal de parada | `drain.on-stop` | **true** | Ao receber `SIGTERM` ou `SIGINT`, o nó entra em modo drain (todas as mensagens vão para o disco, o nó sai da rotação) e só então encerra. `false` encerra na hora, como uma queda. | 6.10 |
| Prazo do drain | `drain.timeout` | A definir (sugestão: 25 s) | Tempo máximo do drain inteiro: esvaziar a memória no disco e concluir o desregistro voluntário. Inclui esperar as chamadas repassadas terminarem. Esgotado, o nó registra o que sobrou e encerra (se foi por sinal), e o cluster cai na detecção e votação de sempre. Deve ser **menor** que o prazo de parada do orquestrador (`terminationGracePeriodSeconds`, 30 s por padrão no Kubernetes). | 6.10 |

## Armazenamento e memória

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Limite de memória por nó | `memory.limit` | A definir | Espaço em memória destinado às mensagens (ex.: MB/GB). Ao atingir o limite, as mensagens excedentes vão para disco (overflow). | 3.2 |
| Janela da taxa de leitura | `memory.rate.window` | **5 minutos** | Janela da média móvel de `consumeRate` e `publishRate`, que decide a divisão da memória entre tópicos. | 3.2 |
| Reserva mínima de memória por tópico | `memory.topic.reserve` | **1000 mensagens** (o tamanho da lista de prontos) | Memória garantida a cada tópico, fora da divisão por taxa de leitura. | 3.2 |
| Diretório raiz de disco | `storage.dir` | **`/var/lib/taba`** (Linux) | Raiz local do nó, onde ficam os arquivos `${topico}/${id}.json` do overflow, `_topics/${topico}.json` da definição do tópico e o banco de eventos da sincronização (`_sync/sync.db`). Cada nó tem a sua; nada é compartilhado. A última pasta deve se chamar `taba`. Se não existir, o nó a **cria ao iniciar**; se não conseguir criar ou gravar, o nó não sobe. | 3.2, 3.3 |

## Mensagens

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Tamanho máximo da mensagem (geral) | `message.max-bytes` | **5 MB** (5.242.880 bytes) | Tamanho máximo do corpo de uma mensagem publicada. Vale para todo tópico que não define o seu próprio `maxMessageBytes`. Acima disso, `POST /topics/{topic}/messages` responde `413`. | 8.1 |
| Tamanho máximo da mensagem (por tópico) | `maxMessageBytes` (campo da definição do tópico) | Vazio (usa `message.max-bytes`) | Substitui o geral naquele tópico e **pode ser maior que ele**. Definido na criação e alterável por `PATCH`; não pode passar de `message.max-bytes.hard-limit`, senão `400`. | 8.1 |
| Limite rígido da mensagem | `message.max-bytes.hard-limit` | A definir | Teto absoluto para o `maxMessageBytes` de qualquer tópico, contra abuso de quem configura. O geral (`message.max-bytes`) também não pode passar dele. | 8.1 |

## Escrita e durabilidade

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Fator de replicação | `replication.factor` | **3** | Número total de nós responsáveis (primário + réplicas) que devem acabar recebendo a mensagem — a durabilidade "plena", alcançada em segundo plano quando o quórum é menor que o total. | 3.1, 7.1, 7.2 |
| Quórum mínimo de escrita | `write.quorum` | `floor(replication.factor / 2) + 1` (**2** em 3, **3** em 5, **1** com fator 1) | Número de confirmações — **sempre incluindo o primário do bloco** (nunca um quórum só de réplicas) — suficiente para decidir o commit e responder ao cliente, sem esperar as demais réplicas. Só vale para **mensagens** (escrita, entrega/invisibilidade, ack); registro de nó e definição de tópico continuam exigindo todos os nós, sem quórum reduzido. | 3.1, 7.2 | **Valor único** para escrita, entrega e ack. Validação: de 1 até `replication.factor`; abaixo da maioria é aceito, mas permite perder dados se o primário e a réplica confirmada caírem juntos. Sem nós suficientes respondendo para o quórum, a operação falha antes de decidir e a API responde `503` com `Retry-After`.
| Retry de registro/commit de mensagem | `write.retry.attempts`, `write.retry.backoff` | A definir | Tentativas (e intervalo/backoff) ao repetir a chamada ao nó que falhou, tanto no registro quanto no commit. | 3.1, 7.2 |
| Expiração de registro pendente | `write.pending.ttl` | A definir | Tempo após o qual um registro (fase 1) sem commit é descartado sozinho, se ainda não chegou rollback. Só se aplica **antes** do quórum decidir; depois disso não há mais descarte. | 3.1 |
| Teto do commit (fase 2) | `write.commit.timeout` | A definir | Tempo máximo que o coordenador insiste no commit **nas réplicas fora do quórum**, depois de já ter respondido ao cliente. Esgotado o teto sem confirmação em alguma réplica, o coordenador **não faz rollback** (o quórum, com o primário, já efetivou a mensagem) e continua tentando em segundo plano — o bloco fica temporariamente abaixo do fator de replicação pleno, mas nunca abaixo do quórum. | 3.1, 7.2 |

## Cluster

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Nós virtuais por nó físico | `ring.virtual-nodes` | A definir | Quantidade de posições que cada nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. | 7.1 |
| Sinal de vida do canal | `cluster.channel.keepalive.interval` | A definir | Periodicidade do sinal de vida (mensagem própria com data e hora, resumo de monitoramento e mensagens prontas por tópico) enviado em cada canal entre nós, e tempo sem sinal para considerá-lo caído. | 6.3 |
| Tópicos por sinal de vida | `cluster.channel.keepalive.max-topics` | A definir | Máximo de tópicos, com mensagens prontas, informados em cada sinal de vida. | 12.8 |
| Janela de agrupamento dos avisos | `cluster.notify.window` | **100 ms** | Tempo que o responsável pelos avisos do tópico acumula mensagens prontas antes de mandar um aviso só. | 12.8 |
| Espera pelo nó avisado | `cluster.notify.wait` | A definir (sugestão: alguns ms) | Tempo que o responsável espera o nó avisado vir buscar as mensagens antes de passar ao próximo nó com ouvinte. | 12.8 |
| Reconexão do canal | `cluster.channel.reconnect.retries`, `cluster.channel.reconnect.backoff` | A definir | Tentativas de reconectar (e intervalo/backoff) antes de abrir a votação de exclusão de um nó cujo canal caiu. | 6.3 |
| Limite da fila do canal | `cluster.channel.queue.max-size`, `cluster.channel.queue.max-age` | A definir | Tamanho e idade máximos da fila de saída para um nó com canal fora; ao passar, o nó com canal fora é tratado como falho e a votação de exclusão é aberta. | 12.9 |
| Quórum de votação | `cluster.vote.quorum-percent` | **51** (%) | Percentual dos nós **saudáveis** (os que o votante enxerga e que respondem; os fora do ar não votam nem entram na base) que precisa concordar para efetivar a exclusão do nó: `ceil(S × percentual)`. | 6.3 |
| Mínimo de votantes | `cluster.vote.min-voters` | **2** | Número mínimo de nós saudáveis para uma votação valer; abaixo disso, nenhuma exclusão se efetiva. Impede que um nó isolado exclua todos os demais. | 6.3 |
| Período de freezing | `cluster.freezing.duration` | A definir | Tempo em que um nó desregistrado por falha fica impedido de se registrar novamente (anti-flapping). Durante o freezing o pedido de ingresso é rejeitado com `Retry-After`. | 6.7 |
| Endereço do balanceador do cluster | `cluster.entry-address` | **Obrigatório** | Endereço (host:porta) do balanceador à frente do cluster, usado pelo nó ao iniciar para listar os membros (`GET /cluster/nodes`) e entrar no cluster. Sem ele o nó não sobe, **exceto em `mode=development`**, em que não é usado. | 6.2 |
| Primeiro nó do cluster | `cluster.first-node` | `false` | Só no primeiro nó de um cluster novo: se a listagem não encontra nenhum membro, ele inicia o cluster sozinho. Nos demais, a listagem sem resposta é erro e o nó tenta de novo. | 6.2 |
| Intervalo entre tentativas do bootstrap | `cluster.bootstrap.retry.interval` | A definir | Tempo de espera entre uma tentativa de entrar no cluster e a seguinte, depois de uma falha (listagem, registro, sincronização ou exclusão por falha no registro). | 6.5 |
| Máximo de tentativas do bootstrap | `cluster.bootstrap.retry.max-attempts` | `0` (sem limite) | Número máximo de tentativas; `0` repete sem parar. Esgotado, o nó encerra com erro. | 6.5 |
| Validade dos registros de perda | `cluster.message-loss.ttl` | A definir | Tempo depois do qual um registro da tabela `perdas` expira; um nó que volta depois disso já não consegue recuperar a mensagem. | 6.6 |
| Prazo da recuperação | `cluster.recovery.timeout` | A definir | Tempo máximo que um nó que retorna tenta consultar todos os nós da lista de perdas antes de limpar o estado local e refazer o bootstrap. | 6.6 |
| Teto de itens por stream de sincronização | `cluster.sync.stream.max-items` | A definir | Itens que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. | 7.3, 8.2.1 |
| Teto de bytes por stream de sincronização | `cluster.sync.stream.max-bytes` | A definir | Bytes que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. | 7.3, 8.2.1 |
| Tamanho máximo de linha do stream | `cluster.sync.stream.max-line-bytes` | A definir (maior que `message.max-bytes.hard-limit` mais a margem do envelope JSON) | Limite de uma linha (uma mensagem ou definição de tópico), que define o buffer de cada lado. Precisa caber a maior mensagem possível. | 8.2.1 |
| Timeout de leitura do stream | `cluster.sync.stream.idle-timeout` | A definir | Tempo sem receber uma linha para o receptor considerar o stream falho e retomar pelo último ID lido. | 8.2.1 |
| Tamanho da faixa de sincronização | `cluster.sync.range.size` | A definir | Tamanho de cada faixa de IDs pedida a uma origem na sincronização de conteúdo de um bloco; relaciona-se com `cluster.sync.stream.max-items`. | 7.3 |
| Teto do banco de eventos | `cluster.sync.log.max-bytes` | A definir | Tamanho máximo, em disco, do banco de eventos (`sync.db`) de um nó em sincronização. Passou do teto, o nó entra em falha e o cluster fica liberado para votar o desregistro dele. | 7.3 |
| Prazo da sincronização sem origem | `cluster.sync.timeout` | A definir | Tempo que o nó em sincronização tenta de novo quando nenhum nó serve uma carga; esgotado, entra em falha como no disco cheio. | 7.3 |

> Tamanho mínimo do cluster (seção 6.4): 1 nó em desenvolvimento (`mode=development`) e 3 nós em produção. O tamanho é uma recomendação de ambiente; o `mode` é o parâmetro que desliga o cluster (seção "Modo de execução").

## Entrega de mensagens

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Invisibilidade padrão | `delivery.visibility-timeout.default` | **120 s** | Tempo em que a mensagem entregue fica invisível a outros consumidores quando o `GET` não informa `visibilityTimeout`. | 12.2 |
| Invisibilidade máxima | `delivery.visibility-timeout.max` | A definir | Teto aceito em `visibilityTimeout` no `GET` e no `PATCH`. | 12.3 |
| Tamanho da lista de prontos | `delivery.ready-list.size` | **1000** | Número de IDs prontos que a thread mantém, por tópico, em cada nó. | 12.6 |
| Tamanho da lista de invisíveis | `delivery.invisible-list.size` | A definir | Número máximo de mensagens invisíveis (ID e prazo) que o dono guarda em memória, por tópico; o excedente vai para a tabela de expiração do banco local. | 13.4 |
| Limiar de recarga da lista de invisíveis | `delivery.invisible.reload-threshold` | A definir | Quando a lista de invisíveis em memória cai abaixo deste tamanho, a thread a recarrega com os próximos a vencer da tabela de expiração. | 13.5 |
| Durabilidade do banco local | `storage.db.synchronous` | `NORMAL` (com WAL) | Modo de sincronização do SQLite. A durabilidade vem da replicação entre nós, então `NORMAL` basta. | 3.2 |
| Mensagens por `GET` (padrão) | `delivery.max-messages.default` | **10** | Quantidade de mensagens do `GET` quando o cliente não informa `maxMessages`. | 12.2 |
| Mensagens por `GET` (teto) | `delivery.max-messages.max` | A definir | Maior `maxMessages` aceito. Não vale para os streams de sincronização entre nós (`cluster.sync.stream.*`). | 12.2 |
| Tentativas de IDs no `GET` | `delivery.get.max-attempts` | A definir | Quantos IDs da lista o `GET` tenta quando o registro de invisibilidade falha; esgotado, responde `503`. | 12.2 |
| Peso das prontas no tamanho do grupo | `delivery.fair.ready-weight` | **0,1** | Quanto cada mensagem pronta pesa no tamanho do grupo, frente a uma em processamento (`tamanho = em processamento + peso × prontas`). De `0` a `1`: `0` equivale ao SQS (só o que está em processamento), `1` conta o backlog inteiro. | 12.11 |
| Grupos acompanhados | `delivery.fair.max-groups` | A definir (sugestão: 100000) | Máximo de grupos de mensagens que cada dono acompanha, por tópico. Os grupos novos acima disso compartilham um contador único (seção 12.11). | 12.11 |
| Entrega pela ordem normal | `delivery.fair.normal-every` | **10** | A cada tantas entregas, uma pega a mensagem mais antiga da lista, de qualquer grupo, para os grupos grandes não esperarem sem limite. `0` desliga a garantia. | 12.11 |
| Tentativas de `take` | `delivery.take.max-attempts` | A definir | Teto de peers consultados, um por vez, quando a lista de prontos local está vazia (cada um escolhido ao acaso entre os que a tabela marca com mensagens prontas). Esgotado, o `GET` vira ouvinte local. | 12.8 |

> Quórum, retry, expiração de pendente e teto de commit da invisibilidade reutilizam `write.quorum`, `write.retry.*`, `write.pending.ttl` e `write.commit.timeout`. Explicação do tema: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) e [14-invisibilidade.md](14-invisibilidade.md).

## Retenção e dead-letter

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| TTL padrão da mensagem | `retention.ttl.default` | **14 dias** (1209600 s) | `ttl` usado pelas filas que não definem o seu. Vale na hora da conferência: mudar o padrão afeta as filas sem `ttl` próprio. A mensagem mais velha que o TTL é descartada. | 12.10 |
| TTL por tópico | `ttl` (campo da definição da fila) | Vazio (usa `retention.ttl.default`) | Substitui o padrão naquela fila (60 s a 365 dias). Definido na criação e alterável por `PATCH`; vale também para as mensagens já guardadas. | 12.10 |
| Limite de entregas padrão | `retention.max-deliveries.default` | **10** | `maxDeliveries` usado pelas filas que não definem o seu. Atingido sem ack, a mensagem vai para o dead-letter. | 12.10 |
| Limite de entregas por tópico | `maxDeliveries` (campo da definição da fila) | Vazio (usa `retention.max-deliveries.default`) | Substitui o padrão naquela fila (1 a 1000). Definido na criação e alterável por `PATCH`. | 12.10 |
| Sufixo da fila de dead-letter | `topics.dead-letter.suffix` | **`-dead-letter`** | Sufixo do nome da fila de dead-letter de cada fila. Reservado: tópico com nome terminado nele não pode ser criado (`400`). Mudar o sufixo com filas existentes exige migração; vale definir antes de criar tópicos. | 12.10 |
| Intervalo da varredura de expiração | `retention.sweep.interval` | **60 s** | De quanto em quanto tempo cada nó varre as próprias mensagens e apaga as vencidas pelo `ttl`. O dono já não entrega mensagem vencida entre uma varredura e outra. | 12.10 |

## Tópicos voláteis e distribuidores

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Ociosidade padrão da fila volátil | `topics.volatile.idle-timeout.default` | **600 s** (10 min) | `idleTimeout` usado quando a fila é criada (por `POST /topics` com `volatile` ou por `autoCreate`) sem informar o valor. | 14.3 |
| Nova tentativa da verificação | `topics.volatile.idle-check.retry-interval` | A definir (sugestão: 30 s) | Intervalo para o nó gestor repetir a verificação final quando algum nó não respondeu. | 14.5 |
| Teto de destinos por publicação | `topics.distributor.max-targets` | **Sem limite** | Máximo de filas que uma publicação num Tópico Distribuidor pode casar; acima disso a publicação é recusada inteira (`422`). | 14.2 |
| Teto de filas voláteis | `topics.volatile.max-count` | **Sem limite** | Máximo de filas voláteis no cluster; atingido, o `autoCreate` é recusado (`422`). | 14.4 |
| Espalhamento ao assumir tópicos | `topics.volatile.takeover.jitter` | A definir | Janela em que o nó que assume filas voláteis (entrada ou saída de nó) espalha suas primeiras consultas de ociosidade. | 14.6 |

## Observabilidade

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Endpoint OTLP | `telemetry.otlp.endpoint` | A definir | Destino da exportação OpenTelemetry (métricas, traces, logs). | 8.1 |
| Tags do tópico | `tags` (por tópico) | Nenhuma | Pares chave/valor enviados como atributos nos traces e logs do tópico (e nas métricas de tópico, conforme `telemetry.topics.mode`). Definidas na criação do tópico e alteradas via `PATCH`. | 8.1 |
| Endpoint Prometheus | `telemetry.prometheus.enabled` | A definir | Liga/desliga `GET /metrics` (formato Prometheus; cluster inteiro por padrão, `scope=node` para só o nó). | 8.1.1 |
| Prazo por nó no refresh do `/metrics` | `telemetry.prometheus.node-timeout` | A definir | Tempo que a thread de atualização espera cada nó. Quem não responde fica com `msg_cluster_node_scrape_up = 0`. | 8.1.1 |
| Intervalo de atualização do `/metrics` | `telemetry.prometheus.refresh-interval` | A definir (sugestão: 15 s) | Periodicidade com que cada nó atualiza em memória as métricas de todos os nós; o `/metrics` de cluster só lê essa memória. | 8.1.1 |
| Validade dos valores de um nó | `telemetry.prometheus.max-age` | A definir | Quanto tempo o instantâneo mantém os últimos valores de um nó que não responde; depois disso as séries dele somem. | 8.1.1 |
| Porta das rotas de operação | `operations.port` | Vazio (mesma porta da API) | Porta própria para `/metrics`, `/stats/*` e `/health/*`. Opcional; se definida, essas rotas deixam de existir na porta da API de cliente. | 8.1.1 |
| IPs autorizados nas rotas de operação | `operations.allowed-ips` | Vazio (sem restrição) | Lista de IPs/faixas CIDR com acesso a `/metrics`, `/stats/*` e `/health/*`; os demais recebem `403`. Opcional. Sem porta nem lista, essas rotas são tratadas como qualquer outra. | 8.1.1 |
| Modo de exportação de métricas de tópico | `telemetry.topics.mode` | `off` | Vale para OTLP e `/metrics`: `off` (só rotas `/stats/topics`), `by-tags` (agregado por tags) ou `per-topic` (uma série por tópico, com teto). | 8.1.1 |
| Tags de agrupamento | `telemetry.topics.group-by-tags` | Nenhuma | Chaves de tag usadas para agregar no modo `by-tags`. | 8.1.1 |
| Teto de tópicos exportados | `telemetry.topics.max-topics` | A definir | Máximo de séries por tópico no modo `per-topic`; o excedente é agregado em `topic="_other"`. | 8.1.1 |
| Amostragem de traces | `telemetry.traces.sample-ratio` | A definir | Fração de traces mantidos; falhas são sempre mantidas. | 8.1.1 |
| Nível de log | `telemetry.logs.level` | A definir | Nível mínimo dos logs estruturados. | 8.1.1 |
| Maiores grupos no detalhe do tópico | `stats.topics.top-groups` | **10** | Quantos grupos (`topGroups`) o `GET /stats/topics/{topic}` devolve. | 8.1.1 |
| Cache das métricas de tópico | `stats.topics.cache-ttl` | A definir | Validade do resultado agregado de `/stats/topics` (evita repetir a consulta a todos os nós). | 8.1.1 |
| Limiar de `backlog` | `stats.topics.backlog.depth`, `stats.topics.backlog.age` | A definir | `depth` e `oldestAge` acima dos quais o tópico entra na condição `backlog`. | 8.1.1 |
| Parcela do grupo barulhento | `stats.topics.noisy-group-share` | **50** (%) | Parcela do `depth` do tópico acima da qual o maior grupo dispara a condição `noisy-group`. | 8.1.1 |
| Limiar de `stalled` | `stats.topics.stalled-after` | A definir | Tempo sem consumo, com mensagens acumuladas, para a condição `stalled`. | 8.1.1 |
| Limiar de `idle` | `stats.topics.idle-after` | A definir | Tempo sem publicação nem consumo para a condição `idle`. | 8.1.1 |
| Espaço livre mínimo em disco | `stats.disk.free-min` | A definir | Abaixo disso o cluster entra na condição `storage-pressure`. | 8.1.1 |

> Explicação do tema: [11-observabilidade.md](11-observabilidade.md).

## Rate limits

| Parâmetro | Chave sugerida | Padrão | Descrição |
|---|---|---|---|
| Rate limit de publicação | `ratelimit.publish.rps` | A definir | Requisições por segundo, por IP do cliente, para `POST /topics/{topic}/messages`. |
| Rate limit de consumo | `ratelimit.consume.rps` | A definir | Requisições por segundo, por IP do cliente, para `GET /topics/{topic}/messages`, `PATCH /topics/{topic}/messages/{id}` (invisibilidade) e `DELETE /topics/{topic}/messages/{id}` (ack). |
| Rate limit administrativo | `ratelimit.admin.rps` | A definir | Requisições por segundo, por IP do cliente, para `POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`. |
| Rate limit das consultas de operação | `ratelimit.ops.rps` | A definir (sugestão: 2) | Requisições por segundo, por IP, para `/stats/cluster` e `/stats/topics`. `/health/*`, `/metrics` e `/stats/node` são isentos. |
| Burst | `ratelimit.burst` | A definir | Tamanho do estouro permitido acima da taxa (ex.: token bucket), para absorver picos curtos sem rejeitar requisições. |
| IP do cliente por cabeçalho | `client-ip.headers.enabled` | **true** | Usa os cabeçalhos abaixo para achar o IP do cliente atrás de balanceador; `false` usa só o endereço da conexão. |
| Cabeçalhos do IP do cliente | `client-ip.headers` | `X-Forwarded-For` | Lista, em ordem, de cabeçalhos consultados; vale o primeiro presente e, nele, o primeiro endereço. |
| Proxies confiáveis do IP do cliente | `client-ip.trusted-proxies` | Vazia | Lista de IPs/CIDR dos balanceadores ou proxies cujos cabeçalhos de IP são aceitos. Vazia: aceita de qualquer origem (o serviço deve ficar em camada segura). Vale para o rate limit e para `operations.allowed-ips`. |

> Explicação do tema, incluindo a resposta ao exceder o limite e a identificação do cliente por IP: [12-limites-api.md](12-limites-api.md).

## Limites fixos da API

Não são configuráveis; são regras de validação da API de cliente ([06-api.md](06-api.md)).

| Limite | Valor |
|---|---|
| Nome do tópico | 1 a 100 caracteres, regex `^[A-Za-z0-9_-]{1,100}$`, diferencia maiúsculas de minúsculas; nome terminado no sufixo de dead-letter é reservado (a dead-letter, `{fila}` + sufixo, passa de 100: até 112 por padrão) |
| `ttl` de fila | 60 s a 365 dias |
| `maxDeliveries` de fila | 1 a 1000 |
| Tags por tópico | máximo de 20 |
| Chave de tag | 1 a 64 caracteres, `A-Za-z0-9_-.:` |
| Valor de tag | 0 a 128 caracteres, `A-Za-z0-9_-.:` |
| Espera do long polling (`waitTime`) | 0 a 20 segundos (padrão 0) |

> Explicação do tema: [12-limites-api.md](12-limites-api.md).
