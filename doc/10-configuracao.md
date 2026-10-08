# Parâmetros de configuração

[← Índice](README.md)

Lista dos parâmetros configuráveis citados na documentação e seus valores padrão. **A definir** significa que o parâmetro é configurável, mas o padrão ainda não foi decidido (ver [pendências](08b-pendencias.md)).

> Os nomes das colunas "Variável de ambiente" e "Chave no YAML" são propostas; ainda não há contrato de configuração definido.

## Como configurar

O Taba pode ser configurado de **duas formas**, que valem para **todos** os parâmetros deste documento:

1. **Arquivo `.yml`**: um arquivo YAML, em que cada ponto da chave vira um nível de aninhamento. É o jeito indicado para o conjunto da configuração, versionado e montado como `ConfigMap` ou volume.
2. **Variáveis de ambiente**: cada parâmetro tem uma variável própria. É o jeito indicado para valores que mudam por ambiente ou por nó, e para o que é segredo.

Podem ser usadas juntas. Ordem de precedência, da maior para a menor:

1. variável de ambiente;
2. arquivo `.yml`;
3. valor padrão da tabela.

**Arquivo.** O caminho vem de `--config <caminho>` ou da variável `TABA_CONFIG_FILE` (padrão: `/etc/taba/taba.yml`). Sem o arquivo, o nó usa só as variáveis e os padrões. Um caminho informado que não existe impede a subida, e o padrão ausente, não. Uma chave que não é parâmetro (um erro de digitação) impede a subida, com o nome dela na mensagem; uma variável `TABA_*` que não é parâmetro só gera um aviso no log.

**Nome da variável.** Prefixo `TABA_`, chave em maiúsculas com `.` e `-` trocados por `_` (ex.: `topics.distributor.max-targets` vira `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`). As exceções são as variáveis padrão do OpenTelemetry (`OTEL_*`), que não levam o prefixo e não têm chave no YAML (ver "Observabilidade").

**Valores.** Listas (`operations.allowed-ips`, `client-ip.headers`, `client-ip.trusted-proxies`) são YAML nativo no arquivo e texto separado por vírgula na variável. Durações e tamanhos usam a mesma forma nos dois (ex.: `60s`, `5MB`). Toda duração leva unidade (`ms`, `s`, `m`, `h` ou `d`; `60` sozinho é recusado), e os tamanhos usam unidades binárias (`KB`, `MB`, `GB` e `TB`; `5MB` são 5.242.880 bytes, e um número sozinho são bytes). Uma variável vazia vale como não definida. **Sem limite** significa que o teto não existe enquanto o parâmetro não for definido.

**Exemplo.** A mesma configuração nos dois formatos:

```yaml
# taba.yml
mode: production
api:
  port: 8080
cluster:
  port: 8081
  entry-address: taba-lb.interno:8081
replication:
  factor: 3
topics:
  distributor:
    max-targets: 50
```

```bash
# variáveis de ambiente equivalentes
TABA_MODE=production
TABA_API_PORT=8080
TABA_CLUSTER_PORT=8081
TABA_CLUSTER_ENTRY_ADDRESS=taba-lb.interno:8081
TABA_REPLICATION_FACTOR=3
TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS=50
```

Nas tabelas abaixo, a coluna **Chave no YAML** mostra o caminho com pontos; `cluster.entry-address` é escrito no arquivo como `entry-address` dentro de `cluster`. Linhas com `—` nas duas colunas descrevem campos da definição de um tópico (criados pela API), não configuração do nó.

## Modo de execução

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Retomada do disco (desenvolvimento) | `TABA_DEVELOPMENT_RESUME_ON_START` | `development.resume-on-start` | **true** | Só em `mode=development`: na partida, o nó retoma mensagens, tópicos e estado salvos em `storage.dir` (o drain serve de save). `false` **apaga** o conteúdo de `storage.dir` e começa limpo. Ignorado em `production`. | 6.10 |
| Modo de execução | `TABA_MODE` | `mode` | **`production`** | `production` ou `development`. Em `development` o nó é um cluster de um só: sem bootstrap, canal entre nós, votação, freezing, replicação nem sincronização; `replication.factor` e `write.quorum` valem 1; `cluster.entry-address` não é exigido; a porta do cluster não é aberta. Os parâmetros `cluster.*`, `ring.*`, `replication.*` e `write.quorum` são **ignorados** nesse modo (aviso no log). Não é para produção. | 6.4 |

## Portas

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Porta da API de cliente | `TABA_API_PORT` | `api.port` | **8080** | Porta das rotas da API de cliente (seção 8.1). Sem `operations.port`, serve também as rotas de operação. Responde `404` a `/cluster/*`. | 8.0 |
| Porta do cluster | `TABA_CLUSTER_PORT` | `cluster.port` | **8081** | Porta das rotas `/cluster/*` e do canal WebSocket entre nós. É a que a tabela de membros guarda e a que o `cluster.entry-address` aponta. Só os outros nós devem alcançá-la; **não publicar** no balanceador de clientes. Ignorada em `mode=development`, em que não é aberta. | 8.0, 8.2 |
| Porta das rotas de operação | `TABA_OPERATIONS_PORT` | `operations.port` | Vazio (mesma porta da API) | Ver a seção "Observabilidade" abaixo. | 8.0, 8.1.1 |

### Segurança da porta do cluster

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Pasta de segredos | `TABA_SECRETS_DIR` | `secrets.dir` | **`/etc/taba/secrets`** | Pasta que o operador cria uma vez com `taba init --out <pasta>` e monta em todos os nós, como segredo da plataforma. Tem `tls/` (certificados do cluster) e `root-token` (token raiz). Os dois parâmetros abaixo derivam dela, e quem quiser outros caminhos os define à parte. | 8.0.1, 8.1.3 |
| Pasta dos certificados do cluster | `TABA_CLUSTER_TLS_DIR` | `cluster.tls.dir` | **`<secrets.dir>/tls`** | Pasta com os certificados do cluster, em pares `<nome>.crt` (PEM) e `<nome>.key`, gerados por `taba init` ou `taba cert generate`. O nó confia em todo certificado não expirado da pasta e apresenta o par de início mais recente que já começou a valer. O TLS mútuo liga **sozinho** quando a pasta existe, e o nó não sobe sem ao menos um par válido; se ela não existe, a porta atende em HTTP puro com aviso no log. Um valor definido que não existe impede a subida. Ignorado em `mode=development`. | 8.0.1 |
| Intervalo de recarga dos certificados | `TABA_CLUSTER_TLS_RELOAD_INTERVAL` | `cluster.tls.reload-interval` | **60 s** | De quanto em quanto tempo a pasta é conferida. Mudanças valem sem reiniciar o nó; pasta inválida mantém o conjunto anterior e registra `error`. | 8.0.1 |
| Aviso de expiração do certificado | `TABA_CLUSTER_TLS_EXPIRY_WARNING` | `cluster.tls.expiry-warning` | **30 dias** | Antecedência com que o nó registra `warn` no log para o certificado em uso que está para expirar. | 8.0.1 |

> As três portas precisam ser diferentes entre si. Valor repetido ou porta ocupada impede o nó de subir. Explicação: [06-api.md](06-api.md), seção 8.0.

## Autenticação da API de cliente

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Autenticação por token | `TABA_AUTH_ENABLED` | `auth.enabled` | **`auto`** | `auto`: liga **sozinha** quando existe um token raiz (o `root-token` da pasta de segredos, `auth.root-token-file` ou `auth.root-token`) e fica desligada sem ele. `false` força desligada, mesmo com a pasta montada. `true` exige o token raiz (o nó não sobe sem ele). Ligada, as rotas de cliente exigem `Authorization: Bearer`. Ignorada (sempre desligada) em `mode=development`. | 8.1.3 |
| Token raiz | `TABA_AUTH_ROOT_TOKEN` | `auth.root-token` | Vazio | Segredo, igual em todos os nós, com **acesso total** à API de cliente (todas as permissões e o único que gerencia `/tokens`). Criado pelo `taba init` (ou avulso, por `taba token generate`); no mínimo 32 caracteres. A existência de um token raiz (este, `auth.root-token-file` ou o `root-token` da pasta de segredos) é o que liga a autenticação em `auth.enabled=auto`. | 8.1.3 |
| Arquivo do token raiz | `TABA_AUTH_ROOT_TOKEN_FILE` | `auth.root-token-file` | **`<secrets.dir>/root-token`** | Caminho de um arquivo com o token raiz, entregue como segredo da plataforma. Aceita **duas linhas** (a atual e a anterior), para trocar o segredo sem reiniciar. Prevalece sobre `auth.root-token`. | 8.1.3 |
| Intervalo de recarga do token raiz | `TABA_AUTH_RELOAD_INTERVAL` | `auth.reload-interval` | **60 s** | De quanto em quanto tempo `auth.root-token-file` é relido. | 8.1.3 |

> Os tokens de trabalho não são configuração: são criados por `POST /tokens` e replicados na tabela `tokens` do `_node.db` (seção 3.4 de [02-arquitetura.md](02-arquitetura.md)). A expiração usa `retention.sweep.interval`.

## Parada e modo drain

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Drain ao receber sinal de parada | `TABA_DRAIN_ON_STOP` | `drain.on-stop` | **true** | Ao receber `SIGTERM` ou `SIGINT`, o nó entra em modo drain (todas as mensagens vão para o disco, o nó sai da rotação) e só então encerra. `false` encerra na hora, como uma queda. | 6.10 |
| Prazo do drain | `TABA_DRAIN_TIMEOUT` | `drain.timeout` | **25 s** | Tempo máximo do drain inteiro: esvaziar a memória no disco e concluir o desregistro voluntário. Inclui esperar as chamadas repassadas terminarem. Esgotado, o nó registra o que sobrou e encerra (se foi por sinal), e o cluster cai na detecção e votação de sempre. Deve ser **menor** que o prazo de parada do orquestrador (`terminationGracePeriodSeconds`, 30 s por padrão no Kubernetes). | 6.10 |

## Armazenamento e memória

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Limite de memória por nó | `TABA_MEMORY_LIMIT` | `memory.limit` | A definir | Espaço em memória destinado às mensagens (ex.: MB/GB). Ao atingir o limite, as mensagens excedentes vão para disco (overflow). | 3.2 |
| Janela da taxa de leitura | `TABA_MEMORY_RATE_WINDOW` | `memory.rate.window` | **5 minutos** | Janela da média móvel de `consumeRate` e `publishRate`, que decide a divisão da memória entre tópicos. | 3.2 |
| Reserva mínima de memória por tópico | `TABA_MEMORY_TOPIC_RESERVE` | `memory.topic.reserve` | **1000 mensagens** (o tamanho da lista de prontos) | Memória garantida a cada tópico, fora da divisão por taxa de leitura. | 3.2 |
| Diretório raiz de disco | `TABA_STORAGE_DIR` | `storage.dir` | **`/var/lib/taba`** (Linux) | Raiz local do nó, onde ficam os arquivos `${topic}/${id}.body` do overflow, o banco do nó `_node.db` (definições de tópico, entre outras tabelas) e o banco de eventos da sincronização (`_sync/sync.db`). Cada nó tem a sua; nada é compartilhado. A última pasta deve se chamar `taba`. Se não existir, o nó a **cria ao iniciar**; se não conseguir criar ou gravar, o nó não sobe. | 3.2, 3.3 |

## Paginação

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| `limit` padrão | `TABA_API_PAGINATION_DEFAULT_LIMIT` | `api.pagination.default-limit` | **100** | Itens por página quando a rota paginada não recebe `limit`. | 8.1.2 |
| `limit` máximo | `TABA_API_PAGINATION_MAX_LIMIT` | `api.pagination.max-limit` | **1000** | Maior `limit` aceito; acima disso, `400` `validation.out-of-range`. O padrão não pode passar dele. | 8.1.2 |

## Mensagens

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Tamanho máximo da mensagem (geral) | `TABA_MESSAGE_MAX_BYTES` | `message.max-bytes` | **5 MB** (5.242.880 bytes) | Tamanho máximo do corpo de uma mensagem publicada, medido nos **bytes enviados** (não na forma em base64). Vale para todo tópico que não define o seu próprio `maxMessageBytes`. Acima disso, `POST /topics/{topic}/messages` responde `413`. | 8.1 |
| Tamanho máximo da mensagem (por tópico) | — | — (campo `maxMessageBytes` da definição do tópico) | Vazio (usa `message.max-bytes`) | Substitui o geral naquele tópico e **pode ser maior que ele**. Definido na criação e alterável por `PATCH`; não pode passar de `message.hard-limit`, senão `400`. | 8.1 |
| Limite rígido da mensagem | `TABA_MESSAGE_HARD_LIMIT` | `message.hard-limit` | A definir | Teto absoluto para o `maxMessageBytes` de qualquer tópico, contra abuso de quem configura. O geral (`message.max-bytes`) também não pode passar dele. | 8.1 |

## Escrita e durabilidade

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Fator de replicação | `TABA_REPLICATION_FACTOR` | `replication.factor` | **3** | Número total de nós responsáveis (primário + réplicas) que devem acabar recebendo a mensagem — a durabilidade "plena", alcançada em segundo plano quando o quórum é menor que o total. Uma fila FIFO exige pelo menos **2** (seção 16.2 de [17-topicos-fifo.md](17-topicos-fifo.md)). | 3.1, 7.1, 7.2 |
| Quórum mínimo de escrita | `TABA_WRITE_QUORUM` | `write.quorum` | `floor(replication.factor / 2) + 1` (**2** em 3, **3** em 5, **1** com fator 1) | Número de confirmações — **sempre incluindo o primário do bloco** (nunca um quórum só de réplicas) — suficiente para decidir o commit e responder ao cliente, sem esperar as demais réplicas. Só vale para **mensagens** (escrita, entrega/invisibilidade, ack); na fila FIFO inclui também o **secundário** (seção 16.5 de [17-topicos-fifo.md](17-topicos-fifo.md)); registro de nó e definição de tópico continuam exigindo todos os nós, sem quórum reduzido. **Valor único** para escrita, entrega e ack. Validação: de 1 até `replication.factor`; abaixo da maioria é aceito, mas permite perder dados se o primário e a réplica confirmada caírem juntos. Sem nós suficientes respondendo para o quórum, a operação falha antes de decidir e a API responde `503` com `Retry-After`. | 3.1, 7.2 |
| Retry de registro/commit de mensagem | `TABA_WRITE_RETRY_ATTEMPTS`<br>`TABA_WRITE_RETRY_BACKOFF` | `write.retry.attempts`<br>`write.retry.backoff` | **3**<br>**200 ms** | Tentativas (e intervalo/backoff) ao repetir a chamada ao nó que falhou, tanto no registro quanto no commit. | 3.1, 7.2 |
| Expiração de registro pendente | `TABA_WRITE_PENDING_TTL` | `write.pending.ttl` | **30 s** | Tempo após o qual um registro (fase 1) sem commit é descartado sozinho, se ainda não chegou rollback. Só se aplica **antes** do quórum decidir; depois disso não há mais descarte. | 3.1 |
| Teto do commit (fase 2) | `TABA_WRITE_COMMIT_TIMEOUT` | `write.commit.timeout` | **10 s** | Tempo máximo que o coordenador insiste no commit **nas réplicas fora do quórum**, depois de já ter respondido ao cliente. Esgotado o teto sem confirmação em alguma réplica, o coordenador **não faz rollback** (o quórum, com o primário, já efetivou a mensagem) e continua tentando em segundo plano — o bloco fica temporariamente abaixo do fator de replicação pleno, mas nunca abaixo do quórum. | 3.1, 7.2 |

## Cluster

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Nós virtuais por nó físico | `TABA_RING_VIRTUAL_NODES` | `ring.virtual-nodes` | **128** | Quantidade de posições que cada nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. | 7.1 |
| Sinal de vida do canal | `TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL` | `cluster.channel.keepalive.interval` | **2 s** | Periodicidade do sinal de vida (mensagem própria com data e hora, resumo de monitoramento e mensagens prontas por tópico) enviado em cada canal entre nós, e tempo sem sinal para considerá-lo caído. Enquanto o canal WebSocket não existe, é também de quanto em quanto tempo o nó repete os sinais de estado (tópicos com mensagens prontas e com ouvintes). | 6.3 |
| Tópicos por sinal de vida | `TABA_CLUSTER_CHANNEL_KEEPALIVE_MAX_TOPICS` | `cluster.channel.keepalive.max-topics` | A definir | Máximo de tópicos, com mensagens prontas, informados em cada sinal de vida. | 12.8 |
| Janela de agrupamento dos avisos | `TABA_CLUSTER_NOTIFY_WINDOW` | `cluster.notify.window` | **100 ms** | Tempo que o responsável pelos avisos do tópico acumula mensagens prontas antes de mandar um aviso só. | 12.8 |
| Espera pelo nó avisado | `TABA_CLUSTER_NOTIFY_WAIT` | `cluster.notify.wait` | A definir (sugestão: alguns ms) | Tempo que o responsável espera o nó avisado vir buscar as mensagens antes de passar ao próximo nó com ouvinte. | 12.8 |
| Reconexão do canal | `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES`<br>`TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF` | `cluster.channel.reconnect.retries`<br>`cluster.channel.reconnect.backoff` | A definir | Tentativas de reconectar (e intervalo/backoff) antes de abrir a votação de exclusão de um nó cujo canal caiu. | 6.3 |
| Limite da fila do canal | `TABA_CLUSTER_CHANNEL_QUEUE_MAX_SIZE`<br>`TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE` | `cluster.channel.queue.max-size`<br>`cluster.channel.queue.max-age` | A definir | Tamanho e idade máximos da fila de saída para um nó com canal fora; ao passar, o nó com canal fora é tratado como falho e a votação de exclusão é aberta. | 12.9 |
| Quórum de votação | `TABA_CLUSTER_VOTE_QUORUM_PERCENT` | `cluster.vote.quorum-percent` | **51** (%) | Percentual dos nós **saudáveis** (os que o votante enxerga e que respondem; os fora do ar não votam nem entram na base) que precisa concordar para efetivar a exclusão do nó: `ceil(S × percentual)`. | 6.3 |
| Mínimo de votantes | `TABA_CLUSTER_VOTE_MIN_VOTERS` | `cluster.vote.min-voters` | **2** | Número mínimo de nós saudáveis para uma votação valer; abaixo disso, nenhuma exclusão se efetiva. Impede que um nó isolado exclua todos os demais. | 6.3 |
| Período de freezing | `TABA_CLUSTER_FREEZING_DURATION` | `cluster.freezing.duration` | A definir | Tempo em que um nó desregistrado por falha fica impedido de se registrar novamente (anti-flapping). Durante o freezing o pedido de ingresso é rejeitado com `Retry-After`. | 6.7 |
| Endereço do balanceador do cluster | `TABA_CLUSTER_ENTRY_ADDRESS` | `cluster.entry-address` | **Obrigatório** | Endereço (`host:porta`, a porta de 1 a 65535; uma forma inválida impede o nó de subir) do balanceador à frente do cluster, **na porta do cluster** (`cluster.port`, não a da API de cliente), usado pelo nó ao iniciar para listar os membros (`GET /cluster/nodes`) e entrar no cluster. Sem ele o nó não sobe, **exceto em `mode=development`**, em que não é usado. | 6.2 |
| Primeiro nó do cluster | `TABA_CLUSTER_FIRST_NODE` | `cluster.first-node` | `false` | Só no primeiro nó de um cluster novo: se a listagem não encontra nenhum membro, ele inicia o cluster sozinho. Nos demais, a listagem sem resposta é erro e o nó tenta de novo. | 6.2 |
| Intervalo entre tentativas do bootstrap | `TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL` | `cluster.bootstrap.retry.interval` | **5 s** | Tempo de espera entre uma tentativa de entrar no cluster e a seguinte, depois de uma falha (listagem, registro, sincronização ou exclusão por falha no registro). | 6.5 |
| Máximo de tentativas do bootstrap | `TABA_CLUSTER_BOOTSTRAP_RETRY_MAX_ATTEMPTS` | `cluster.bootstrap.retry.max-attempts` | `0` (sem limite) | Número máximo de tentativas; `0` repete sem parar. Esgotado, o nó encerra com erro. | 6.5 |
| Validade dos registros de perda | `TABA_CLUSTER_MESSAGE_LOSS_TTL` | `cluster.message-loss.ttl` | A definir | Tempo depois do qual um registro da tabela `losses` expira; um nó que volta depois disso já não consegue recuperar a mensagem. | 6.6 |
| Prazo da recuperação | `TABA_CLUSTER_RECOVERY_TIMEOUT` | `cluster.recovery.timeout` | A definir | Tempo máximo que um nó que retorna tenta consultar todos os nós da lista de perdas antes de limpar o estado local e refazer o bootstrap. | 6.6 |
| Teto de itens por stream de sincronização | `TABA_CLUSTER_SYNC_STREAM_MAX_ITEMS` | `cluster.sync.stream.max-items` | **500** | Itens que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. | 7.3, 8.2.1 |
| Teto de bytes por stream de sincronização | `TABA_CLUSTER_SYNC_STREAM_MAX_BYTES` | `cluster.sync.stream.max-bytes` | **4MB** | Bytes que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. Conta só o corpo; a primeira mensagem de um trecho sempre entra. | 7.3, 8.2.1 |
| Tamanho máximo de linha do stream | `TABA_CLUSTER_SYNC_STREAM_MAX_LINE_BYTES` | `cluster.sync.stream.max-line-bytes` | A definir (maior que `message.hard-limit` × 4/3, o base64, mais a margem do envelope JSON) | Limite de uma linha (uma mensagem ou definição de tópico), que define o buffer de cada lado. Precisa caber a maior mensagem possível. | 8.2.1 |
| Timeout de leitura do stream | `TABA_CLUSTER_SYNC_STREAM_IDLE_TIMEOUT` | `cluster.sync.stream.idle-timeout` | A definir | Tempo sem receber uma linha para o receptor considerar o stream falho e retomar pelo último ID lido. | 8.2.1 |
| Tamanho da faixa de sincronização | `TABA_CLUSTER_SYNC_RANGE_SIZE` | `cluster.sync.range.size` | **200** | Tamanho de cada faixa de IDs pedida a uma origem na sincronização de conteúdo de um bloco; relaciona-se com `cluster.sync.stream.max-items`. | 7.3 |
| Teto do banco de eventos | `TABA_CLUSTER_SYNC_LOG_MAX_BYTES` | `cluster.sync.log.max-bytes` | **1GB** | Tamanho máximo, em disco, do banco de eventos (`sync.db`) de um nó em sincronização. Passou do teto, o nó entra em falha e o cluster fica liberado para votar o desregistro dele. | 7.3 |
| Prazo da sincronização sem origem | `TABA_CLUSTER_SYNC_TIMEOUT` | `cluster.sync.timeout` | **60 s** | Tempo que o nó em sincronização tenta de novo quando nenhum nó serve uma carga; esgotado, entra em falha como no disco cheio. Vale para cada chamada a uma origem (rede, `5xx`, `409`) e para a carga de definições. | 7.3 |

> Tamanho mínimo do cluster (seção 6.4): 1 nó em desenvolvimento (`mode=development`) e 3 nós em produção. O tamanho é uma recomendação de ambiente; o `mode` é o parâmetro que desliga o cluster (seção "Modo de execução").

## Entrega de mensagens

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Invisibilidade padrão | `TABA_DELIVERY_VISIBILITY_TIMEOUT_DEFAULT` | `delivery.visibility-timeout.default` | **120 s** | Tempo em que a mensagem entregue fica invisível a outros consumidores quando o `GET` não informa `visibilityTimeout`. | 12.2 |
| Invisibilidade máxima | `TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX` | `delivery.visibility-timeout.max` | A definir | Teto aceito em `visibilityTimeout` no `GET` e no `PUT .../visibility`. | 12.3 |
| Tamanho da lista de prontos | `TABA_DELIVERY_READY_LIST_SIZE` | `delivery.ready-list.size` | **1000** | Número de IDs prontos que a thread mantém, por tópico, em cada nó. | 12.6 |
| Tamanho da lista de invisíveis | `TABA_DELIVERY_INVISIBLE_LIST_SIZE` | `delivery.invisible-list.size` | A definir | Número máximo de mensagens invisíveis (ID e prazo) que o dono guarda em memória, por tópico; o excedente vai para a tabela de expiração do banco local. | 13.4 |
| Limiar de recarga da lista de invisíveis | `TABA_DELIVERY_INVISIBLE_RELOAD_THRESHOLD` | `delivery.invisible.reload-threshold` | A definir | Quando a lista de invisíveis em memória cai abaixo deste tamanho, a thread a recarrega com os próximos a vencer da tabela de expiração. | 13.5 |
| Durabilidade do banco local | `TABA_STORAGE_DB_SYNCHRONOUS` | `storage.db.synchronous` | `NORMAL` (com WAL) | Modo de sincronização do SQLite. A durabilidade vem da replicação entre nós, então `NORMAL` basta. | 3.2 |
| Mensagens por `GET` (padrão) | `TABA_DELIVERY_MAX_MESSAGES_DEFAULT` | `delivery.max-messages.default` | **10** | Quantidade de mensagens do `GET` quando o cliente não informa `maxMessages`. | 12.2 |
| Mensagens por `GET` (teto) | `TABA_DELIVERY_MAX_MESSAGES_MAX` | `delivery.max-messages.max` | A definir | Maior `maxMessages` aceito. Não vale para os streams de sincronização entre nós (`cluster.sync.stream.*`). | 12.2 |
| Tentativas de IDs no `GET` | `TABA_DELIVERY_GET_MAX_ATTEMPTS` | `delivery.get.max-attempts` | A definir | Quantos IDs da lista o `GET` tenta quando o registro de invisibilidade falha; esgotado, responde `503`. | 12.2 |
| Peso das prontas no tamanho do grupo | `TABA_DELIVERY_FAIR_READY_WEIGHT` | `delivery.fair.ready-weight` | **0,1** | Quanto cada mensagem pronta pesa no tamanho do grupo, frente a uma em processamento (`tamanho = em processamento + peso × prontas`). De `0` a `1`: `0` equivale ao SQS (só o que está em processamento), `1` conta o backlog inteiro. | 12.11 |
| Grupos acompanhados | `TABA_DELIVERY_FAIR_MAX_GROUPS` | `delivery.fair.max-groups` | A definir (sugestão: 100000) | Máximo de grupos de mensagens que cada dono acompanha, por tópico. Os grupos novos acima disso compartilham um contador único (seção 12.11). | 12.11 |
| Entrega pela ordem normal | `TABA_DELIVERY_FAIR_NORMAL_EVERY` | `delivery.fair.normal-every` | **10** | A cada tantas entregas, uma pega a mensagem mais antiga da lista, de qualquer grupo, para os grupos grandes não esperarem sem limite. `0` desliga a garantia. | 12.11 |
| Tentativas de `take` | `TABA_DELIVERY_TAKE_MAX_ATTEMPTS` | `delivery.take.max-attempts` | **3** | Teto de peers consultados, um por vez, quando a lista de prontos local está vazia (cada um escolhido ao acaso entre os que a tabela marca com mensagens prontas). Esgotado, o `GET` vira ouvinte local. | 12.8 |

> Quórum, retry, expiração de pendente e teto de commit da invisibilidade reutilizam `write.quorum`, `write.retry.*`, `write.pending.ttl` e `write.commit.timeout`. Explicação do tema: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) e [14-invisibilidade.md](14-invisibilidade.md).

## Retenção e dead-letter

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| TTL padrão da mensagem | `TABA_RETENTION_TTL_DEFAULT` | `retention.ttl.default` | **14 dias** (1209600 s) | `ttl` usado pelas filas que não definem o seu. Vale na hora da conferência: mudar o padrão afeta as filas sem `ttl` próprio. A mensagem mais velha que o TTL é descartada. | 12.10 |
| TTL por tópico | — | — (campo `ttl` da definição do tópico) | Vazio (usa `retention.ttl.default`) | Substitui o padrão naquela fila (60 s a 365 dias). Definido na criação e alterável por `PATCH`; vale também para as mensagens já guardadas. | 12.10 |
| Limite de entregas padrão | `TABA_RETENTION_MAX_DELIVERIES_DEFAULT` | `retention.max-deliveries.default` | **10** | `maxDeliveries` usado pelas filas que não definem o seu. Atingido sem ack, a mensagem vai para o dead-letter. | 12.10 |
| Limite de entregas por tópico | — | — (campo `maxDeliveries` da definição do tópico) | Vazio (usa `retention.max-deliveries.default`) | Substitui o padrão naquela fila (1 a 1000). Definido na criação e alterável por `PATCH`. | 12.10 |
| Sufixo da fila de dead-letter | `TABA_TOPICS_DEAD_LETTER_SUFFIX` | `topics.dead-letter.suffix` | **`-dead-letter`** | Sufixo do nome da fila de dead-letter de cada fila. Reservado: tópico com nome terminado nele não pode ser criado (`400`). Mudar o sufixo com filas existentes exige migração; vale definir antes de criar tópicos. | 12.10 |
| Intervalo da varredura de expiração | `TABA_RETENTION_SWEEP_INTERVAL` | `retention.sweep.interval` | **60 s** | De quanto em quanto tempo cada nó varre as próprias mensagens e apaga as vencidas pelo `ttl`. O dono já não entrega mensagem vencida entre uma varredura e outra. | 12.10 |

## Filas FIFO

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Janela de deduplicação | `TABA_FIFO_DEDUP_WINDOW` | `fifo.dedup.window` | **300 s** (5 min) | Tempo, a partir do instante do ID, em que uma publicação com o mesmo `deduplicationId` (ou o mesmo hash do corpo) no mesmo grupo é descartada como duplicata. Também é o prazo do `deleteAt` da mensagem confirmada. | 16.7 |
| Índice de deduplicação em memória | `TABA_FIFO_DEDUP_INDEX_MAX_ENTRIES` | `fifo.dedup.index.max-entries` | A definir | Máximo de chaves da janela mantidas em memória, por tópico; o excedente é consultado no banco do tópico. | 16.7 |
| Tentativas do repasse ao primário | `TABA_FIFO_FORWARD_MAX_ATTEMPTS` | `fifo.forward.max-attempts` | A definir | Quantas vezes o nó que recebeu a publicação a repassa de novo, com a tabela de membros atualizada, quando o primário responde `421`. Esgotado, `503` com `Retry-After`. | 16.4 |

> A fila FIFO exige `replication.factor` de pelo menos 2 e usa `write.quorum` sempre com o primário e o secundário (seção 16.5). A varredura do `deleteAt` usa `retention.sweep.interval`.

## Tópicos voláteis e distribuidores

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Ociosidade padrão da fila volátil | `TABA_TOPICS_VOLATILE_IDLE_TIMEOUT_DEFAULT` | `topics.volatile.idle-timeout.default` | **600 s** (10 min) | `idleTimeout` usado quando a fila é criada (por `POST /topics` com `volatile` ou por `autoCreate`) sem informar o valor. | 14.3 |
| Nova tentativa da verificação | `TABA_TOPICS_VOLATILE_IDLE_CHECK_RETRY_INTERVAL` | `topics.volatile.idle-check.retry-interval` | A definir (sugestão: 30 s) | Intervalo para o nó gestor repetir a verificação final quando algum nó não respondeu. | 14.5 |
| Teto de destinos por publicação | `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS` | `topics.distributor.max-targets` | **Sem limite** | Máximo de filas que uma publicação num Tópico Distribuidor pode casar; acima disso a publicação é recusada inteira (`422`). | 14.2 |
| Teto de filas voláteis | `TABA_TOPICS_VOLATILE_MAX_COUNT` | `topics.volatile.max-count` | **Sem limite** | Máximo de filas voláteis no cluster; atingido, o `autoCreate` é recusado (`422`). | 14.4 |
| Espalhamento ao assumir tópicos | `TABA_TOPICS_VOLATILE_TAKEOVER_JITTER` | `topics.volatile.takeover.jitter` | A definir | Janela em que o nó que assume filas voláteis (entrada ou saída de nó) espalha suas primeiras consultas de ociosidade. | 14.6 |

## Observabilidade

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição | Seção |
|---|---|---|---|---|---|
| Exportação OTLP (variáveis padrão `OTEL_*`) | `OTEL_EXPORTER_OTLP_ENDPOINT`<br>`OTEL_EXPORTER_OTLP_PROTOCOL`<br>`OTEL_EXPORTER_OTLP_HEADERS`<br>`OTEL_EXPORTER_OTLP_CERTIFICATE`<br>`OTEL_METRIC_EXPORT_INTERVAL` | — | Sem endpoint, o OTLP fica desligado; protocolo `grpc` | Destino e opções da exportação OpenTelemetry (métricas, traces, logs), pelas **variáveis padrão do SDK**, sem o prefixo `TABA_` (exceção à convenção do início deste documento), para um coletor ser ligado como em qualquer serviço OpenTelemetry. | 8.1 |
| Atributos de recurso | `OTEL_SERVICE_NAME`<br>`OTEL_RESOURCE_ATTRIBUTES` | — | `service.name=taba` | `service.namespace` (nome do cluster) e `deployment.environment.name` (ambiente) vão em `OTEL_RESOURCE_ATTRIBUTES`. `service.instance.id` (ID do nó) e `service.version` o nó define sozinho. | 8.1 |
| Tags do tópico | — | — (campo `tags` da definição do tópico) | Nenhuma | Pares chave/valor enviados como atributos nos traces e logs do tópico (e nas métricas de tópico, conforme `telemetry.topics.mode`). Definidas na criação do tópico e alteradas via `PATCH`. | 8.1 |
| Endpoint Prometheus | `TABA_TELEMETRY_PROMETHEUS_ENABLED` | `telemetry.prometheus.enabled` | **true** | Liga/desliga `GET /metrics` (formato Prometheus, só as métricas deste nó). | 8.1.1 |
| Porta das rotas de operação | `TABA_OPERATIONS_PORT` | `operations.port` | Vazio (mesma porta da API) | Porta própria para `/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain`. Opcional; se definida, essas rotas deixam de existir na porta da API de cliente. Deve ser diferente de `api.port` e de `cluster.port`. | 8.0, 8.1.1 |
| IPs autorizados nas rotas de operação | `TABA_OPERATIONS_ALLOWED_IPS` | `operations.allowed-ips` | Vazio (sem restrição) | Lista de IPs/faixas CIDR com acesso a `/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain`; os demais recebem `403`. Opcional. Sem porta nem lista, essas rotas são tratadas como qualquer outra. | 8.1.1 |
| Modo de exportação de métricas de tópico | `TABA_TELEMETRY_TOPICS_MODE` | `telemetry.topics.mode` | `off` | Vale para OTLP e `/metrics`: `off` (só rotas `/metrics/topics`), `by-tags` (agregado por tags) ou `per-topic` (uma série por tópico, com teto). | 8.1.1 |
| Tags de agrupamento | `TABA_TELEMETRY_TOPICS_GROUP_BY_TAGS` | `telemetry.topics.group-by-tags` | Nenhuma | Chaves de tag usadas para agregar no modo `by-tags`. | 8.1.1 |
| Teto de tópicos exportados | `TABA_TELEMETRY_TOPICS_MAX_TOPICS` | `telemetry.topics.max-topics` | **1000** (provisório, D91) | Máximo de séries por tópico no modo `per-topic`; o excedente é agregado em `messaging.destination.name="_other"`. | 8.1.1 |
| Amostragem de traces | `OTEL_TRACES_SAMPLER`<br>`OTEL_TRACES_SAMPLER_ARG` | — | `parentbased_traceidratio`, fração a definir | Amostragem por fração no SDK. Manter sempre as falhas é amostragem por cauda, feita no Collector ([11-observabilidade.md](11-observabilidade.md), "Como ligar um coletor"). | 8.1.1 |
| Nível de log | `TABA_TELEMETRY_LOGS_LEVEL` | `telemetry.logs.level` | A definir (até lá, `info`) | Nível mínimo dos logs estruturados: `trace`, `debug`, `info`, `warn` ou `error`. | 8.1.1 |
| Maiores grupos no detalhe do tópico | `TABA_STATS_TOPICS_TOP_GROUPS` | `stats.topics.top-groups` | **10** | Quantos grupos (`topGroups`) o `GET /metrics/topics/{topic}` devolve. | 8.1.1 |
| Cache das métricas de tópico | `TABA_STATS_TOPICS_CACHE_TTL` | `stats.topics.cache-ttl` | A definir (D90) | Validade do resultado agregado de `/metrics/topics` (evita repetir a consulta a todos os nós). Com um nó só não há consulta a repetir: o parâmetro ainda não tem efeito. | 8.1.1 |
| Limiar de `backlog` | `TABA_STATS_TOPICS_BACKLOG_DEPTH`<br>`TABA_STATS_TOPICS_BACKLOG_AGE` | `stats.topics.backlog.depth`<br>`stats.topics.backlog.age` | A definir (D90) | `depth` e `oldestAge` acima dos quais o tópico entra na condição `backlog`. Sem valor, a condição não dispara. | 8.1.1 |
| Parcela do grupo barulhento | `TABA_STATS_TOPICS_NOISY_GROUP_SHARE` | `stats.topics.noisy-group-share` | **50** (%) | Parcela do `depth` do tópico acima da qual o maior grupo dispara a condição `noisy-group`. | 8.1.1 |
| Entregas para `group-stalled` | `TABA_STATS_TOPICS_GROUP_STALLED_DELIVERIES` | `stats.topics.group-stalled-deliveries` | A definir (sugestão: 5; D90) | Número de entregas da cabeça de um grupo de fila FIFO, sem ack, acima do qual dispara a condição `group-stalled`. Sem valor, a condição não dispara. | 8.1.1 |
| Limiar de `stalled` | `TABA_STATS_TOPICS_STALLED_AFTER` | `stats.topics.stalled-after` | A definir (D90) | Tempo sem consumo, com mensagens acumuladas, para a condição `stalled`. Sem valor, a condição não dispara. | 8.1.1 |
| Limiar de `idle` | `TABA_STATS_TOPICS_IDLE_AFTER` | `stats.topics.idle-after` | A definir (D90) | Tempo sem publicação nem consumo para a condição `idle`. Sem valor, a condição não dispara. | 8.1.1 |
| Espaço livre mínimo em disco | `TABA_STATS_DISK_FREE_MIN` | `stats.disk.free-min` | A definir (D90) | Abaixo disso o cluster entra na condição `storage-pressure`. Sem valor, a condição não dispara. | 8.1.1 |

> Explicação do tema: [11-observabilidade.md](11-observabilidade.md).

## Rate limits

| Parâmetro | Variável de ambiente | Chave no YAML | Padrão | Descrição |
|---|---|---|---|---|
| Rate limit de publicação | `TABA_RATELIMIT_PUBLISH_RPS` | `ratelimit.publish.rps` | A definir | Requisições por segundo, **por token** (ou por IP do cliente, sem token válido), para `POST /topics/{topic}/messages`. |
| Rate limit de consumo | `TABA_RATELIMIT_CONSUME_RPS` | `ratelimit.consume.rps` | A definir | Requisições por segundo, por token (ou por IP, sem token válido), para `GET /topics/{topic}/messages`, `PUT /topics/{topic}/messages/{id}/visibility` (invisibilidade) e `DELETE /topics/{topic}/messages/{id}` (ack). |
| Rate limit administrativo | `TABA_RATELIMIT_ADMIN_RPS` | `ratelimit.admin.rps` | A definir | Requisições por segundo, por token (ou por IP, sem token válido), para `POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`, e `/tokens`. |
| Rate limit das consultas de operação | `TABA_RATELIMIT_OPS_RPS` | `ratelimit.ops.rps` | A definir (sugestão: 2) | Requisições por segundo, por IP, para `/health/cluster?scope=cluster` e `/metrics/topics`. `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` são isentos. |
| Burst | `TABA_RATELIMIT_BURST` | `ratelimit.burst` | A definir | Tamanho do estouro permitido acima da taxa (ex.: token bucket), para absorver picos curtos sem rejeitar requisições. Vale para cada contador, de token ou de IP. |
| IP do cliente por cabeçalho | `TABA_CLIENT_IP_HEADERS_ENABLED` | `client-ip.headers.enabled` | **true** | Usa os cabeçalhos abaixo para achar o IP do cliente atrás de balanceador; `false` usa só o endereço da conexão. Só importa para as chamadas contadas por IP (sem token válido) e para `operations.allowed-ips`. |
| Cabeçalhos do IP do cliente | `TABA_CLIENT_IP_HEADERS` | `client-ip.headers` | `X-Forwarded-For` | Lista, em ordem, de cabeçalhos consultados; vale o primeiro presente e, nele, o primeiro endereço. |
| Proxies confiáveis do IP do cliente | `TABA_CLIENT_IP_TRUSTED_PROXIES` | `client-ip.trusted-proxies` | Vazia | Lista de IPs/CIDR dos balanceadores ou proxies cujos cabeçalhos de IP são aceitos. Vazia: aceita de qualquer origem (o serviço deve ficar em camada segura). Vale para o rate limit e para `operations.allowed-ips`. |

> Explicação do tema, incluindo a resposta ao exceder o limite e a identificação do cliente por token ou por IP: [12-limites-api.md](12-limites-api.md).

## Limites fixos da API

Não são configuráveis; são regras de validação da API de cliente ([06a-api-cliente.md](06a-api-cliente.md)).

| Limite | Valor |
|---|---|
| Nome do tópico | 1 a 100 caracteres, regex `^[A-Za-z0-9_-]{1,100}$`, diferencia maiúsculas de minúsculas; nome terminado no sufixo de dead-letter é reservado (a dead-letter, `{fila}` + sufixo, passa de 100: até 112 por padrão) |
| `ttl` de fila | 60 s a 365 dias |
| `maxDeliveries` de fila | 1 a 1000 |
| Tags por tópico | máximo de 20 |
| Chave de tag | 1 a 64 caracteres, `A-Za-z0-9_-.:` |
| Valor de tag | 0 a 128 caracteres, `A-Za-z0-9_-.:` |
| Espera do long polling (`waitTime`) | 0 a 20 segundos (padrão 0) |
| `contentType` da mensagem | Media type válido de 1 a 255 caracteres (padrão `application/json`) |
| Nome do token | 1 a 100 caracteres, `A-Za-z0-9_-.:` |
| Permissões do token | Ao menos uma de `publish`, `consume`, `create`, `admin` |
| Prefixos de tópico do token | Até 20, de 1 a 100 caracteres cada, `A-Za-z0-9_-` |
| `expiresAt` do token | Data futura (opcional) |

> Explicação do tema: [12-limites-api.md](12-limites-api.md).
