# Parâmetros de configuração

[← Índice](README.md)

Lista dos parâmetros configuráveis citados na documentação e seus valores padrão. **A definir** significa que o parâmetro é configurável, mas o padrão ainda não foi decidido (ver [pendências](08-pendencias-e-proximos-passos.md)).

> Os nomes da coluna "Chave sugerida" são propostas; ainda não há contrato de configuração definido.

## Armazenamento e memória

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Limite de memória por nó | `memory.limit` | A definir | Espaço em memória destinado às mensagens (ex.: MB/GB). Ao atingir o limite, as mensagens excedentes vão para disco (overflow). | 3.2 |
| Diretório raiz de disco | `storage.dir` | A definir | Raiz onde ficam os arquivos `${topico}/${id}.json` do overflow (relativo a quê: nó? bloco? ainda em aberto) e `_topics/${topico}.json` da definição do tópico. | 3.2, 3.3 |

## Escrita e durabilidade

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Fator de replicação | `replication.factor` | **3** | Número total de nós responsáveis (primário + réplicas) que devem acabar recebendo a mensagem — a durabilidade "plena", alcançada em segundo plano quando o quórum é menor que o total. | 3.1, 7.1, 7.2 |
| Quórum mínimo de escrita | `write.quorum` | Maioria de `replication.factor` (ex.: **2** em 3) | Número de confirmações — **sempre incluindo o primário do bloco** (nunca um quórum só de réplicas) — suficiente para decidir o commit e responder ao cliente, sem esperar as demais réplicas. Só vale para **mensagens** (escrita, entrega/invisibilidade, ack); registro de nó e definição de tópico continuam exigindo todos os nós, sem quórum reduzido. | 3.1, 7.2 |
| Retry de registro/commit de mensagem | `write.retry.attempts`, `write.retry.backoff` | A definir | Tentativas (e intervalo/backoff) ao repetir a chamada ao nó que falhou, tanto no registro quanto no commit. | 3.1, 7.2 |
| Expiração de registro pendente | `write.pending.ttl` | A definir | Tempo após o qual um registro (fase 1) sem commit é descartado sozinho, se ainda não chegou rollback. Só se aplica **antes** do quórum decidir; depois disso não há mais descarte. | 3.1 |
| Teto do commit (fase 2) | `write.commit.timeout` | A definir | Tempo máximo que o coordenador insiste no commit **nas réplicas fora do quórum**, depois de já ter respondido ao cliente. Esgotado o teto sem confirmação em alguma réplica, o coordenador **não faz rollback** (o quórum, com o primário, já efetivou a mensagem) e continua tentando em segundo plano — o bloco fica temporariamente abaixo do fator de replicação pleno, mas nunca abaixo do quórum. | 3.1, 7.2 |

## Cluster

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Nós virtuais por nó físico | `ring.virtual-nodes` | A definir | Quantidade de posições que cada nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. | 7.1 |
| Sinal de vida do canal | `cluster.channel.keepalive.interval` | A definir | Periodicidade do sinal de vida enviado em cada canal entre nós, e tempo sem sinal para considerá-lo caído. | 6.3 |
| Reconexão do canal | `cluster.channel.reconnect.retries`, `cluster.channel.reconnect.backoff` | A definir | Tentativas de reconectar (e intervalo/backoff) antes de abrir a votação de exclusão de um nó cujo canal caiu. | 6.3 |
| Limite da fila do canal | `cluster.channel.queue.max-size`, `cluster.channel.queue.max-age` | A definir | Tamanho e idade máximos da fila de saída para um nó com canal fora; ao passar, o nó com canal fora é tratado como falho e a votação de exclusão é aberta. | 12.9 |
| Quórum de votação | `cluster.vote.quorum` | **3** | Votos mínimos concordando com a exclusão para efetivar a remoção do nó. | 6.3 |
| Período de freezing | `cluster.freezing.duration` | A definir | Tempo em que um nó desregistrado por falha fica impedido de se registrar novamente (anti-flapping). Comportamento durante o freezing ainda em aberto. | 6.7 |
| Retry de bootstrap | `cluster.bootstrap.retry` | A definir | Comportamento (tentativa imediata, backoff, manual) após exclusão por falha no registro. | 6.5 |

> Tamanho mínimo do cluster (seção 6.4): 1 nó em desenvolvimento e 3 nós em produção. É uma recomendação de ambiente, não um parâmetro.

## Entrega de mensagens

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Invisibilidade padrão | `delivery.visibility-timeout.default` | **120 s** | Tempo em que a mensagem entregue fica invisível a outros consumidores quando o `GET` não informa `visibilityTimeout`. | 12.2 |
| Invisibilidade máxima | `delivery.visibility-timeout.max` | A definir | Teto aceito em `visibilityTimeout` no `GET` e no `PATCH`. | 12.3 |
| Tamanho da lista de prontos | `delivery.ready-list.size` | A definir | Número fixo de IDs prontos que a thread mantém, por tópico, em cada nó. | 12.6 |
| Tamanho da lista de invisíveis | `delivery.invisible-list.size` | A definir | Número máximo de mensagens invisíveis (ID e prazo) que o dono guarda em memória, por tópico; o excedente vai para o índice de expiração em disco. | 13.4 |
| Janela do índice de expiração | `delivery.invisible.spill.window` | A definir (sugestão: 60 s) | Intervalo de tempo coberto por cada arquivo do índice de expiração em disco (`${topico}/_expiracao/${janela}.log`). | 13.5 |
| Tentativas de `take` | `delivery.take.max-attempts` | A definir | Teto de peers consultados, um por vez, quando a lista de prontos local está vazia (cada um escolhido ao acaso entre os que a tabela marca com mensagens prontas). Esgotado, o `GET` vira ouvinte local. | 12.8 |

> Quórum, retry, expiração de pendente e teto de commit da invisibilidade reutilizam `write.quorum`, `write.retry.*`, `write.pending.ttl` e `write.commit.timeout`. Explicação do tema: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) e [14-invisibilidade.md](14-invisibilidade.md).

## Observabilidade

| Parâmetro | Chave sugerida | Padrão | Descrição | Seção |
|---|---|---|---|---|
| Endpoint OTLP | `telemetry.otlp.endpoint` | A definir | Destino da exportação OpenTelemetry (métricas, traces, logs). | 8.1 |
| Tags do tópico | `tags` (por tópico) | Nenhuma | Pares chave/valor enviados como atributos nos traces e logs do tópico (e nas métricas de tópico, conforme `telemetry.topics.mode`). Definidas na criação do tópico e alteradas via `PATCH`. | 8.1 |
| Endpoint Prometheus | `telemetry.prometheus.enabled` | A definir | Liga/desliga `GET /metrics` (formato Prometheus, local por nó). | 8.1.1 |
| Porta das rotas de operação | `operations.port` | Vazio (mesma porta da API) | Porta própria para `/metrics`, `/stats/*` e `/health/*`. Opcional; se definida, essas rotas deixam de existir na porta da API de cliente. | 8.1.1 |
| IPs autorizados nas rotas de operação | `operations.allowed-ips` | Vazio (sem restrição) | Lista de IPs/faixas CIDR com acesso a `/metrics`, `/stats/*` e `/health/*`; os demais recebem `403`. Opcional. Sem porta nem lista, essas rotas são tratadas como qualquer outra. | 8.1.1 |
| Modo de exportação de métricas de tópico | `telemetry.topics.mode` | `off` | Vale para OTLP e `/metrics`: `off` (só rotas `/stats/topics`), `by-tags` (agregado por tags) ou `per-topic` (uma série por tópico, com teto). | 8.1.1 |
| Tags de agrupamento | `telemetry.topics.group-by-tags` | Nenhuma | Chaves de tag usadas para agregar no modo `by-tags`. | 8.1.1 |
| Teto de tópicos exportados | `telemetry.topics.max-topics` | A definir | Máximo de séries por tópico no modo `per-topic`; o excedente é agregado em `topic="_other"`. | 8.1.1 |
| Amostragem de traces | `telemetry.traces.sample-ratio` | A definir | Fração de traces mantidos; falhas são sempre mantidas. | 8.1.1 |
| Nível de log | `telemetry.logs.level` | A definir | Nível mínimo dos logs estruturados. | 8.1.1 |
| Cache das métricas de tópico | `stats.topics.cache-ttl` | A definir | Validade do resultado agregado de `/stats/topics` (evita repetir a consulta a todos os nós). | 8.1.1 |
| Limiar de `backlog` | `stats.topics.backlog.depth`, `stats.topics.backlog.age` | A definir | `depth` e `oldestAge` acima dos quais o tópico entra na condição `backlog`. | 8.1.1 |
| Limiar de `stalled` | `stats.topics.stalled-after` | A definir | Tempo sem consumo, com mensagens acumuladas, para a condição `stalled`. | 8.1.1 |
| Limiar de `idle` | `stats.topics.idle-after` | A definir | Tempo sem publicação nem consumo para a condição `idle`. | 8.1.1 |
| Espaço livre mínimo em disco | `stats.disk.free-min` | A definir | Abaixo disso o cluster entra na condição `storage-pressure`. | 8.1.1 |

> Explicação do tema: [11-observabilidade.md](11-observabilidade.md).

## Rate limits

| Parâmetro | Chave sugerida | Padrão | Descrição |
|---|---|---|---|
| Rate limit de publicação | `ratelimit.publish.rps` | A definir | Requisições por segundo, por cliente/API key, para `POST /topics/{topic}/messages`. |
| Rate limit de consumo | `ratelimit.consume.rps` | A definir | Requisições por segundo, por cliente/API key, para `GET /topics/{topic}/messages`, `PATCH /topics/{topic}/messages/{id}` (invisibilidade) e `DELETE /topics/{topic}/messages/{id}` (ack). |
| Rate limit administrativo | `ratelimit.admin.rps` | A definir | Requisições por segundo, por cliente/API key, para `POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`. |
| Burst | `ratelimit.burst` | A definir | Tamanho do estouro permitido acima da taxa (ex.: token bucket), para absorver picos curtos sem rejeitar requisições. |

> Explicação do tema, incluindo a resposta ao exceder o limite e a pendência de identificação do cliente: [12-limites-api.md](12-limites-api.md).

## Limites fixos da API

Não são configuráveis; são regras de validação da API de cliente ([06-api.md](06-api.md)).

| Limite | Valor |
|---|---|
| Nome do tópico | 1 a 80 caracteres, regex `^[A-Za-z0-9_-]{1,80}$`, diferencia maiúsculas de minúsculas |
| Tags por tópico | máximo de 20 |
| Chave de tag | 1 a 64 caracteres, `A-Za-z0-9_-.:` |
| Valor de tag | 0 a 128 caracteres, `A-Za-z0-9_-.:` |
| Espera do long polling (`waitTime`) | 0 a 20 segundos (padrão 0) |

> Explicação do tema: [12-limites-api.md](12-limites-api.md).
