---
título: Configuração
resumo: Como configurar o nó (arquivo YAML e variáveis TABA_*), a precedência, e a tabela completa de parâmetros com o padrão efetivo e o que cada um faz.
tipo: referência
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src/config/params/mod.rs, crates/taba-node/src/config/mod.rs, crates/taba-node/src/config/source/mod.rs]
---

# Configuração

> Leia antes: [04-arquitetura.md](04-arquitetura.md). A tabela de chaves e padrões do código é `config/params/mod.rs`; **um teste confere que todo parâmetro da tabela é lido** pelo `Config`. Parâmetro novo entra na tabela **e** no `Config` ([24-convencoes.md](24-convencoes.md)).

## Como configurar

Há **duas formas**, que valem para **todos** os parâmetros, e podem ser usadas juntas:

1. **Arquivo `.yml`**: cada ponto da chave vira um nível de aninhamento. Indicado para o conjunto da configuração (versionado, montado como `ConfigMap` ou volume).
2. **Variáveis de ambiente**: cada parâmetro tem a sua. Indicado para o que muda por ambiente ou por nó, e para segredos.

**Precedência** (da maior para a menor): **variável de ambiente** → **arquivo `.yml`** → **padrão**.

**O arquivo.** O caminho vem de `--config <caminho>` ou de `TABA_CONFIG_FILE`; o padrão é `/etc/taba/taba.yml`. Sem o arquivo, o nó usa só variáveis e padrões. Um caminho **informado** que não existe **impede a subida**; o padrão ausente, não. **Uma chave que não é parâmetro** (erro de digitação) **impede a subida**, com o nome na mensagem; uma variável `TABA_*` que não é parâmetro só gera aviso no log.

**Nome da variável.** Prefixo `TABA_`, a chave em maiúsculas, com `.` e `-` trocados por `_` (`topics.distributor.max-targets` → `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`). As variáveis `OTEL_*` do OpenTelemetry são a exceção: não passam por aqui ([13-observabilidade.md](13-observabilidade.md)).

**Valores.** Listas (`operations.allowed-ips`, `client-ip.headers`, `client-ip.trusted-proxies`, `cluster.advertise-addresses`) são YAML nativo no arquivo e **texto separado por vírgula** na variável. **Toda duração leva unidade** (`ms`, `s`, `m`, `h`, `d`; `60` sozinho é **recusado**). Tamanhos usam unidades binárias (`KB`, `MB`, `GB`, `TB`; `5MB` = 5 242 880 bytes; um número sozinho são bytes).

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
# as mesmas chaves como variáveis
TABA_API_PORT=8080
TABA_CLUSTER_PORT=8081
TABA_CLUSTER_ENTRY_ADDRESS=taba-lb.interno:8081
TABA_REPLICATION_FACTOR=3
TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS=50
```

## Validações de partida

O nó **não sobe** (mensagem em português, uma por problema) quando:

- `storage.dir` não termina numa pasta chamada **`taba`**;
- as três portas se repetem ("usam a mesma porta; as três portas precisam ser diferentes"), ou uma está ocupada;
- `api.pagination.default-limit` está fora de 1 até `api.pagination.max-limit`;
- `message.max-bytes` é zero; `replication.factor` é menor que 1; `write.quorum` está fora de 1 até `replication.factor`;
- em produção, falta `cluster.entry-address` (exceto no primeiro nó, `cluster.first-node=true`), ou ele não é `host:porta`;
- `cluster.vote.quorum-percent` está fora de 1 a 100; `cluster.vote.min-voters`, `cluster.batch.max-items`, `ring.virtual-nodes` ou `write.retry.attempts` são menores que 1;
- `delivery.visibility-timeout.default` ou `delivery.max-messages.default` passam do respectivo `.max`; `delivery.fair.ready-weight` está fora de 0 a 1;
- `retention.ttl.default`, `retention.max-deliveries.default` ou `topics.volatile.idle-timeout.default` estão fora das faixas de [19-erros-e-limites.md](19-erros-e-limites.md);
- `cluster.tls.dir` definido não existe, ou a pasta existe sem nenhum par válido;
- `auth.enabled=true` sem token raiz; token raiz com menos de 32 caracteres; `auth.root-token-file` definido que não existe;
- `cluster.tls.enabled=false` ou `auth.dev-token` com `replication.factor` diferente de 1; `auth.dev-token` com menos de 32 caracteres ou com ponto;
- `operations.allowed-ips` ou `client-ip.trusted-proxies` têm uma entrada inválida;
- `backup.s3.access-key` sem `backup.s3.secret-key` (ou o contrário); `backup.segment.max-bytes` ou `backup.segment.max-messages` iguais a zero; a regra `backup.disk.evict-to` < `backup.disk.evict-above` < `backup.disk.pause-at` ≤ 100 desrespeitada; `backup.dead-after` que não passa de `backup.heartbeat.interval` (todo nó pareceria morto).


`mode` e `development.resume-on-start` **deixaram de existir** (o modo desenvolvimento saiu, [07-cluster.md](07-cluster.md), seção 8): um arquivo ou uma variável que ainda os defina continua subindo, com um aviso no log, e eles não fazem nada. Para rodar um nó só, use o perfil de um nó (`replication.factor=1`, e se quiser `cluster.tls.enabled=false` e `auth.dev-token`).

## Como ler as tabelas

- **Padrão em negrito** = o valor efetivo. **"(do código)"** marca um padrão que a tabela de parâmetros deixa sem valor mas o código aplica (a doc histórica o chamava "A definir").
- **Sem valor** = o recurso fica desligado ou sem teto (a coluna diz qual).
- ⚠️ **Aceito, mas sem efeito hoje:** o `Config` lê o valor e **nenhum código o usa**. São quatro (`cluster.sync.stream.max-line-bytes`, `cluster.sync.stream.idle-timeout`, `fifo.dedup.index.max-entries`, `stats.topics.cache-ttl`): pendência registrada em [31-pendencias.md](31-pendencias.md).
- As tabelas foram **montadas** a partir do código (`config/params/mod.rs`, os padrões e a leitura de cada campo) e das descrições de `10-configuracao.md`; o script `doc/pt-BR/scripts/verificar.mjs` ([27-ci-cd.md](27-ci-cd.md)) confere que **toda chave do código aparece aqui** e que nenhuma chave documentada deixou de existir.

## Modo de execução

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|

## Portas

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `api.port` | `TABA_API_PORT` | **8080** | Porta das rotas da API de cliente (seção 8.1), que inclui `/health/live` e `/health/ready` (sem token) e `/metrics` (qualquer token válido). Responde `404` a `/cluster/*`. |
| `cluster.port` | `TABA_CLUSTER_PORT` | **8081** | Porta das rotas `/cluster/*` e do canal WebSocket entre nós. É a que a tabela de membros guarda e a que o `cluster.entry-address` aponta. Só os outros nós devem alcançá-la; **não publicar** no balanceador de clientes. Um nó só (fator 1) a abre do mesmo jeito. |
| `operations.port` | `TABA_OPERATIONS_PORT` | **9090** | Porta da API de operação (monitoramento), **sempre aberta**. Ver a seção "Observabilidade" abaixo. |

### Segurança da porta do cluster

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `secrets.dir` | `TABA_SECRETS_DIR` | **/etc/taba/secrets** | Pasta que o operador cria uma vez com `taba init --out <pasta>` e monta em todos os nós, como segredo da plataforma. Tem `tls/` (certificados do cluster) e `root-token` (token raiz). Os dois parâmetros abaixo derivam dela, e quem quiser outros caminhos os define à parte. |
| `cluster.tls.dir` | `TABA_CLUSTER_TLS_DIR` | **<secrets.dir>/tls** (do código) | Pasta com os certificados do cluster, em pares `<nome>.crt` (PEM) e `<nome>.key`, gerados por `taba init` ou `taba cert generate`. O nó confia em todo certificado não expirado da pasta e apresenta o par de início mais recente que já começou a valer. O TLS mútuo liga **sozinho** quando a pasta existe, e o nó não sobe sem ao menos um par válido; se ela não existe, a porta atende em HTTP puro com aviso no log. Um valor definido que não existe impede a subida. |
| `cluster.tls.enabled` | `TABA_CLUSTER_TLS_ENABLED` | **true** | `false` desliga o TLS do cluster (sem pasta de segredos). **Só é aceito com `replication.factor=1`**: com mais de um nó o processo recusa partir. Parte do perfil de um nó, só para desenvolvimento ([07-cluster.md](07-cluster.md), seção 8). |
| `cluster.tls.reload-interval` | `TABA_CLUSTER_TLS_RELOAD_INTERVAL` | **60s** | De quanto em quanto tempo a pasta é conferida. Mudanças valem sem reiniciar o nó; pasta inválida mantém o conjunto anterior e registra `error`. |
| `cluster.tls.expiry-warning` | `TABA_CLUSTER_TLS_EXPIRY_WARNING` | **30d** | Antecedência com que o nó registra `warn` no log para o certificado em uso que está para expirar. |

## Autenticação da API de cliente

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `auth.enabled` | `TABA_AUTH_ENABLED` | **auto** | `auto`: liga **sozinha** quando existe um token raiz (o `root-token` da pasta de segredos, `auth.root-token-file` ou `auth.root-token`) e fica desligada sem ele. `false` força desligada, mesmo com a pasta montada. `true` exige o token raiz (o nó não sobe sem ele). Ligada, as rotas de cliente exigem `Authorization: Bearer`. |
| `auth.root-token` | `TABA_AUTH_ROOT_TOKEN` | vazio | Segredo, igual em todos os nós, com **acesso total** à API de cliente (todas as permissões e o único que gerencia `/tokens`). Criado pelo `taba init` (ou avulso, por `taba token generate`); no mínimo 32 caracteres. A existência de um token raiz (este, `auth.root-token-file` ou o `root-token` da pasta de segredos) é o que liga a autenticação em `auth.enabled=auto`. |
| `auth.root-token-file` | `TABA_AUTH_ROOT_TOKEN_FILE` | **<secrets.dir>/root-token** (do código) | Caminho de um arquivo com o token raiz, entregue como segredo da plataforma. Aceita **duas linhas** (a atual e a anterior), para trocar o segredo sem reiniciar. Prevalece sobre `auth.root-token`. |
| `auth.reload-interval` | `TABA_AUTH_RELOAD_INTERVAL` | **60s** | De quanto em quanto tempo `auth.root-token-file` é relido. |
| `auth.dev-token` | `TABA_AUTH_DEV_TOKEN` | vazio | Segredo de pelo menos 32 caracteres (sem ponto). A partida **cria** o token de trabalho `dev` (`admin`, `publish` e `consume`; só o hash é guardado) e liga a autenticação; o cliente usa `tb_dev.<segredo>`. Se o segredo mudou, a partida o atualiza. **Só é aceito com `replication.factor=1`**, e é só para desenvolvimento: não é o token raiz, então não para o cluster nem gerencia outros `admin`. |
| `health.single-node-condition` | `TABA_HEALTH_SINGLE_NODE_CONDITION` | **true** | Mostra a condição informativa `reduced-profile` no `/health/cluster` quando o nó roda com algo relaxado (`replication.factor=1`, `cluster.tls.enabled=false` ou `auth.dev-token`). `false` a esconde. Não derruba o `/health/ready`. |

## Parada e modo drain

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `drain.on-stop` | `TABA_DRAIN_ON_STOP` | **true** | Ao receber `SIGTERM` ou `SIGINT`, o nó entra em modo drain (todas as mensagens vão para o disco, o nó sai da rotação) e só então encerra. `false` encerra na hora, como uma queda. |
| `drain.timeout` | `TABA_DRAIN_TIMEOUT` | **25s** | Tempo máximo do drain inteiro: esvaziar a memória no disco e concluir o desregistro voluntário. Inclui esperar as chamadas repassadas terminarem. Esgotado, o nó registra o que sobrou e encerra (se foi por sinal), e o cluster cai na detecção e votação de sempre. Deve ser **menor** que o prazo de parada do orquestrador (`terminationGracePeriodSeconds`, 30 s por padrão no Kubernetes). |

## Armazenamento e memória

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `memory.limit` | `TABA_MEMORY_LIMIT` | sem limite (nada vai para o disco por overflow) | Espaço em memória destinado às mensagens (ex.: MB/GB). Ao atingir o limite, as mensagens excedentes vão para disco (overflow). |
| `memory.rate.window` | `TABA_MEMORY_RATE_WINDOW` | **5m** | Janela da média móvel de `consumeRate` e `publishRate`, que decide a divisão da memória entre tópicos. |
| `memory.topic.reserve` | `TABA_MEMORY_TOPIC_RESERVE` | **1000** | Memória garantida a cada tópico, fora da divisão por taxa de leitura. |
| `storage.dir` | `TABA_STORAGE_DIR` | **/var/lib/taba** | Raiz local do nó, onde ficam os arquivos `${topic}/${id}.body` do overflow, o banco do nó `_node.db` (definições de tópico, entre outras tabelas) e o banco de eventos da sincronização (`_sync/sync.db`). Cada nó tem a sua; nada é compartilhado. A última pasta deve se chamar `taba`. Se não existir, o nó a **cria ao iniciar**; se não conseguir criar ou gravar, o nó não sobe. |

## Paginação

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `api.pagination.default-limit` | `TABA_API_PAGINATION_DEFAULT_LIMIT` | **100** | Itens por página quando a rota paginada não recebe `limit`. |
| `api.pagination.max-limit` | `TABA_API_PAGINATION_MAX_LIMIT` | **1000** | Maior `limit` aceito; acima disso, `400` `validation.out-of-range`. O padrão não pode passar dele. |

## Mensagens

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `message.max-bytes` | `TABA_MESSAGE_MAX_BYTES` | **5MB** | Tamanho máximo do corpo de uma mensagem publicada, medido nos **bytes enviados** (não na forma em base64). Vale para todo tópico que não define o seu próprio `maxMessageBytes`. Acima disso, `POST /topics/{topic}/messages` responde `413`. |
| `message.hard-limit` | `TABA_MESSAGE_HARD_LIMIT` | sem teto | Teto absoluto para o `maxMessageBytes` de qualquer tópico, contra abuso de quem configura. O geral (`message.max-bytes`) também não pode passar dele. |

## Escrita e durabilidade

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `replication.factor` | `TABA_REPLICATION_FACTOR` | **3** | Número total de nós responsáveis (primário + réplicas) que devem acabar recebendo a mensagem — a durabilidade "plena", alcançada em segundo plano quando o quórum é menor que o total. Uma fila FIFO exige pelo menos **2** (seção 16.2 de [09-topicos.md](09-topicos.md)). |
| `write.quorum` | `TABA_WRITE_QUORUM` | **floor(replication.factor / 2) + 1** (do código) | Número de confirmações — **sempre incluindo o primário do bloco** (nunca um quórum só de réplicas) — suficiente para decidir o commit e responder ao cliente, sem esperar as demais réplicas. Só vale para **mensagens** (escrita, entrega/invisibilidade, ack); na fila FIFO inclui também o **secundário** (seção 16.5 de [09-topicos.md](09-topicos.md)); registro de nó e definição de tópico continuam exigindo todos os nós, sem quórum reduzido. **Valor único** para escrita, entrega e ack. Validação: de 1 até `replication.factor`; abaixo da maioria é aceito, mas permite perder dados se o primário e a réplica confirmada caírem juntos. Sem nós suficientes respondendo para o quórum, a operação falha antes de decidir e a API responde `503` com `Retry-After`. |
| `write.retry.attempts`<br>`write.retry.backoff` | `TABA_WRITE_RETRY_ATTEMPTS`<br>`TABA_WRITE_RETRY_BACKOFF` | **3**<br>**200ms** | Tentativas (e intervalo/backoff) ao repetir a chamada ao nó que falhou, tanto no registro quanto no commit. |
| `write.pending.ttl` | `TABA_WRITE_PENDING_TTL` | **30s** | Tempo após o qual um registro (fase 1) sem commit é descartado sozinho, se ainda não chegou rollback. Só se aplica **antes** do quórum decidir; depois disso não há mais descarte. |
| `write.commit.timeout` | `TABA_WRITE_COMMIT_TIMEOUT` | **10s** | Tempo máximo que o coordenador insiste no commit **nas réplicas fora do quórum**, depois de já ter respondido ao cliente. Esgotado o teto sem confirmação em alguma réplica, o coordenador **não faz rollback** (o quórum, com o primário, já efetivou a mensagem) e continua tentando em segundo plano — o bloco fica temporariamente abaixo do fator de replicação pleno, mas nunca abaixo do quórum. |

## Cluster

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `ring.virtual-nodes` | `TABA_RING_VIRTUAL_NODES` | **128** | Quantidade de posições que cada nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. |
| `cluster.channel.keepalive.interval` | `TABA_CLUSTER_CHANNEL_KEEPALIVE_INTERVAL` | **2s** (do código) | Periodicidade do sinal de vida (mensagem própria com data e hora, resumo de monitoramento e mensagens prontas por tópico) enviado em cada canal entre nós, e tempo sem sinal para considerá-lo caído. O sinal de vida leva o estado do nó e a versão da tabela de membros, então repete o estado a cada intervalo; `2 ×` este valor (no mínimo 500 ms) é também o prazo de uma chamada do canal (D109). |
| `cluster.channel.keepalive.max-topics` | `TABA_CLUSTER_CHANNEL_KEEPALIVE_MAX_TOPICS` | **1000** (do código) | Máximo de tópicos, com mensagens prontas, informados em cada sinal de vida. |
| `cluster.notify.window` | `TABA_CLUSTER_NOTIFY_WINDOW` | **100ms** | Tempo que o responsável pelos avisos do tópico acumula mensagens prontas antes de mandar um aviso só. |
| `cluster.notify.wait` | `TABA_CLUSTER_NOTIFY_WAIT` | **50ms** (do código) | Tempo que o responsável pelos avisos do tópico espera o nó avisado vir buscar as mensagens (`take`) antes de passar ao próximo nó com ouvinte. |
| `cluster.channel.reconnect.retries`<br>`cluster.channel.reconnect.backoff` | `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES`<br>`TABA_CLUSTER_CHANNEL_RECONNECT_BACKOFF` | **3** (do código)<br>**1s** (do código) | Tentativas de reconectar (e intervalo/backoff) antes de abrir a votação de exclusão de um nó cujo canal caiu. |
| `cluster.channel.queue.max-size`<br>`cluster.channel.queue.max-age` | `TABA_CLUSTER_CHANNEL_QUEUE_MAX_SIZE`<br>`TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE` | **10000** (do código)<br>**60s** (do código) | Tamanho e idade máximos da fila de saída para um nó com canal fora; ao passar, o nó com canal fora é tratado como falho e a votação de exclusão é aberta. |
| `cluster.vote.quorum-percent` | `TABA_CLUSTER_VOTE_QUORUM_PERCENT` | **51** | Percentual dos nós **saudáveis** (os que o votante enxerga e que respondem; os fora do ar não votam nem entram na base) que precisa concordar para efetivar a exclusão do nó: `ceil(S × percentual)`. |
| `cluster.vote.min-voters` | `TABA_CLUSTER_VOTE_MIN_VOTERS` | **2** | Número mínimo de nós saudáveis para uma votação valer; abaixo disso, nenhuma exclusão se efetiva. Impede que um nó isolado exclua todos os demais. |
| `cluster.batch.max-items` | `TABA_CLUSTER_BATCH_MAX_ITEMS` | **1000** | O máximo de operações de mensagem (`register`, `commit`, `rollback` de uma escrita, de uma invisibilidade ou de um ack) que uma chamada entre dois nós leva, e o máximo que um nó aceita numa só. O lote fecha quando uma vaga de envio abre, com o que estiver esperando, então com pouca carga ele tem um item; este é só o teto. `1` desliga o lote (cada operação vai sozinha). |
| `cluster.batch.max-bytes` | `TABA_CLUSTER_BATCH_MAX_BYTES` | **8MB** | O limite brando do tamanho dos corpos de um lote: quem envia para de acrescentar itens ao alcançá-lo, e um item maior que ele vai sozinho. Quem recebe aceita isto mais uma mensagem (`message.max-bytes`) e recusa mais que isso com `413 message.too-large`. |
| `cluster.freezing.duration` | `TABA_CLUSTER_FREEZING_DURATION` | **5m** (do código) | Tempo em que um nó desregistrado por falha fica impedido de se registrar novamente (anti-flapping). Durante o freezing o pedido de ingresso é rejeitado com `Retry-After`. |
| `cluster.entry-address` | `TABA_CLUSTER_ENTRY_ADDRESS` | obrigatório com `replication.factor` acima de 1 (exceto o primeiro nó) | Endereço (`host:porta`, a porta de 1 a 65535; uma forma inválida impede o nó de subir) do balanceador à frente do cluster, **na porta do cluster** (`cluster.port`, não a da API de cliente), usado pelo nó ao iniciar para listar os membros (`GET /cluster/nodes`) e entrar no cluster. Sem ele o nó não sobe, **exceto o primeiro nó e o perfil de um nó** (`replication.factor=1`), em que não é usado. |
| `cluster.advertise-addresses` | `TABA_CLUSTER_ADVERTISE_ADDRESSES` | **detectado (interface que sai para o entry-address)** (do código) | Lista de IPs (separados por vírgula) que o nó põe no `join` e que a tabela de membros guarda; os outros nós o reconhecem pelo IP de origem das conexões. Sem valor, o nó usa o endereço local de um soquete UDP "conectado" ao `cluster.entry-address` (nada é enviado), isto é, a interface que sai para o cluster. Quem tem NAT, várias interfaces ou um nome que não resolve para a interface certa define o parâmetro. |
| `cluster.first-node` | `TABA_CLUSTER_FIRST_NODE` | **false** | Só no primeiro nó de um cluster que **nunca existiu**: se a listagem não encontra nenhum membro, ele inicia o cluster sozinho. Um nó **com vida anterior** não precisa dela: ele procura o cluster também pelos IPs da tabela anterior e, passado `cluster.restore.cold-start.wait`, parte sozinho se for o de menor ID antigo entre os vivos. Nos demais, a listagem sem resposta é erro e o nó tenta de novo. |
| `cluster.restore.grace` | `TABA_CLUSTER_RESTORE_GRACE` | **5m** | Prazo `G` que a restauração das mensagens das vidas anteriores espera pelos nós esperados (os membros da maior tabela antiga). Passado o prazo, ela segue com um aviso e os nós presentes (ou fica retida, no modo estrito). Maior que zero. |
| `cluster.restore.strict` | `TABA_CLUSTER_RESTORE_STRICT` | **false** | Modo estrito: passado `G`, a restauração fica retida até a operação de forçar, em vez de seguir com os nós presentes. |
| `cluster.restore.clock-slack` | `TABA_CLUSTER_RESTORE_CLOCK_SLACK` | **5s** | Folga ao comparar o `last_seen` de uma vida (relógio do nó que a escreveu) com a data de corte do cluster (relógios de outros nós): só se restaura a vida vista depois da data **mais** a folga. |
| `cluster.restore.probe.timeout` | `TABA_CLUSTER_RESTORE_PROBE_TIMEOUT` | **2h** | Tempo total em que o nó sonda os IPs da tabela da vida anterior que ainda faltam no cluster. Não menor que o intervalo. |
| `cluster.restore.probe.interval` | `TABA_CLUSTER_RESTORE_PROBE_INTERVAL` | **10s** | Intervalo entre duas sondagens. Maior que zero. |
| `cluster.restore.cold-start.wait` | `TABA_CLUSTER_RESTORE_COLD_START_WAIT` | **15s** | Quanto tempo um nó **com vida anterior** procura o cluster (pelo endereço de entrada e pelos IPs da tabela da vida anterior) antes de poder partir sozinho. Passado o prazo, parte o de menor ID antigo entre os nós vivos. Um nó sem vida anterior nunca parte sozinho (só `cluster.first-node`). |
| `cluster.bootstrap.retry.interval` | `TABA_CLUSTER_BOOTSTRAP_RETRY_INTERVAL` | **5s** | Tempo de espera entre uma tentativa de entrar no cluster e a seguinte, depois de uma falha (listagem, registro, sincronização ou exclusão por falha no registro). |
| `cluster.bootstrap.retry.max-attempts` | `TABA_CLUSTER_BOOTSTRAP_RETRY_MAX_ATTEMPTS` | **0** | Número máximo de tentativas; `0` repete sem parar. Esgotado, o nó encerra com erro. |
| `cluster.message-loss.ttl` | `TABA_CLUSTER_MESSAGE_LOSS_TTL` | **14d** (do código) | Tempo depois do qual um registro da tabela `losses` expira; um nó que volta depois disso já não consegue recuperar a mensagem. |
| `cluster.reunion.interval` | `TABA_CLUSTER_REUNION_INTERVAL` | **10 × o sinal de vida (20 s), no mínimo 1 s** (do código) | De quanto em quanto tempo o nó que excluiu outros por votação consulta a tabela deles, para saber se as duas metades de uma partição simétrica se reencontraram; o lado perdedor refaz o ingresso (seção 6.7). |
| `cluster.recovery.timeout` | `TABA_CLUSTER_RECOVERY_TIMEOUT` | **5m** (do código) | Tempo máximo que um nó que retorna tenta consultar todos os nós da lista de perdas antes de limpar o estado local e refazer o bootstrap. |
| `cluster.sync.stream.max-items` | `TABA_CLUSTER_SYNC_STREAM_MAX_ITEMS` | **500** (do código) | Itens que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. |
| `cluster.sync.stream.max-bytes` | `TABA_CLUSTER_SYNC_STREAM_MAX_BYTES` | **4MiB** (do código) | Bytes que um stream NDJSON entre nós envia antes de fechar com `hasMore: true`. Conta só o corpo; a primeira mensagem de um trecho sempre entra. |
| `cluster.sync.stream.max-line-bytes` | `TABA_CLUSTER_SYNC_STREAM_MAX_LINE_BYTES` | — | Limite de uma linha (uma mensagem ou definição de tópico), que define o buffer de cada lado. Precisa caber a maior mensagem possível. ⚠️ **Aceito, mas sem efeito hoje** (o código lê o valor e não o usa). |
| `cluster.sync.stream.idle-timeout` | `TABA_CLUSTER_SYNC_STREAM_IDLE_TIMEOUT` | — | Tempo sem receber uma linha para o receptor considerar o stream falho e retomar pelo último ID lido. ⚠️ **Aceito, mas sem efeito hoje** (o código lê o valor e não o usa). |
| `cluster.sync.range.size` | `TABA_CLUSTER_SYNC_RANGE_SIZE` | **200** (do código) | Tamanho de cada faixa de IDs pedida a uma origem na sincronização de conteúdo de um bloco; relaciona-se com `cluster.sync.stream.max-items`. |
| `cluster.sync.log.max-bytes` | `TABA_CLUSTER_SYNC_LOG_MAX_BYTES` | **1GiB** (do código) | Tamanho máximo, em disco, do banco de eventos (`sync.db`) de um nó em sincronização. Passou do teto, o nó entra em falha e o cluster fica liberado para votar o desregistro dele. |
| `cluster.sync.timeout` | `TABA_CLUSTER_SYNC_TIMEOUT` | **60s** (do código) | Tempo que o nó em sincronização tenta de novo quando nenhum nó serve uma carga; esgotado, entra em falha como no disco cheio. Vale para cada chamada a uma origem (rede, `5xx`, `409`) e para a carga de definições. |

## Entrega de mensagens

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `delivery.visibility-timeout.default` | `TABA_DELIVERY_VISIBILITY_TIMEOUT_DEFAULT` | **120s** | Tempo em que a mensagem entregue fica invisível a outros consumidores quando o `GET` não informa `visibilityTimeout`. |
| `delivery.visibility-timeout.max` | `TABA_DELIVERY_VISIBILITY_TIMEOUT_MAX` | sem teto | Teto aceito em `visibilityTimeout` no `GET` e no `PUT .../visibility`. |
| `delivery.ready-list.size` | `TABA_DELIVERY_READY_LIST_SIZE` | **1000** | Número de IDs prontos que a lista (janela) mantém, por tópico, em cada nó; as contagens continuam exatas e a lista é reposta pelos cursores quando abre lugar. Mínimo efetivo 1. Na fila FIFO não se aplica (a lista tem uma entrada por grupo livre). |
| `delivery.invisible-list.size` | `TABA_DELIVERY_INVISIBLE_LIST_SIZE` | — | Número máximo de mensagens invisíveis (ID e prazo) que o dono guarda em memória, por tópico; o excedente vai para a tabela `expiry` do banco do tópico. Sem valor, a lista não tem teto. |
| `delivery.invisible.reload-threshold` | `TABA_DELIVERY_INVISIBLE_RELOAD_THRESHOLD` | — | Quando a lista de invisíveis em memória cai abaixo deste tamanho, a thread a recarrega com os próximos a vencer da tabela `expiry`. Sem valor, vale a metade de `delivery.invisible-list.size` (e `0`, só com a lista vazia, se esta também não tem valor). |
| `storage.db.synchronous` | `TABA_STORAGE_DB_SYNCHRONOUS` | **NORMAL** | Modo de sincronização do SQLite. A durabilidade vem da replicação entre nós, então `NORMAL` basta. |
| `storage.excluded.max-size` | `TABA_STORAGE_EXCLUDED_MAX_SIZE` | **100MB** | Tamanho máximo do log de mensagens excluídas (soma de todos os tópicos). Passando dele, os registros mais antigos saem primeiro e a data de início da gravação avança. Maior que zero. |
| `storage.excluded.max-age` | `TABA_STORAGE_EXCLUDED_MAX_AGE` | **24h** | Idade máxima de um registro do log de mensagens excluídas. Maior que zero. |
| `delivery.max-messages.default` | `TABA_DELIVERY_MAX_MESSAGES_DEFAULT` | **10** | Quantidade de mensagens do `GET` quando o cliente não informa `maxMessages`. |
| `delivery.max-messages.max` | `TABA_DELIVERY_MAX_MESSAGES_MAX` | sem teto | Maior `maxMessages` aceito. Não vale para os streams de sincronização entre nós (`cluster.sync.stream.*`). |
| `delivery.get.max-attempts` | `TABA_DELIVERY_GET_MAX_ATTEMPTS` | **3** (do código) | Quantos IDs da lista o `GET` tenta quando o registro de invisibilidade falha; esgotado, responde `503`. |
| `delivery.fair.ready-weight` | `TABA_DELIVERY_FAIR_READY_WEIGHT` | **0.1** | Quanto cada mensagem pronta pesa no tamanho do grupo, frente a uma em processamento (`tamanho = em processamento + peso × prontas`). De `0` a `1`: `0` equivale ao SQS (só o que está em processamento), `1` conta o backlog inteiro. |
| `delivery.fair.max-groups` | `TABA_DELIVERY_FAIR_MAX_GROUPS` | sem teto | Máximo de grupos de mensagens que cada dono acompanha, por tópico. Os grupos novos acima disso compartilham um contador único (seção 12.11). |
| `delivery.fair.normal-every` | `TABA_DELIVERY_FAIR_NORMAL_EVERY` | **10** | A cada tantas entregas, uma pega a mensagem mais antiga da lista, de qualquer grupo, para os grupos grandes não esperarem sem limite. `0` desliga a garantia. |
| `delivery.take.max-attempts` | `TABA_DELIVERY_TAKE_MAX_ATTEMPTS` | **3** (do código) | Teto de peers consultados, um por vez, quando a lista de prontos local está vazia (cada um escolhido ao acaso entre os que a tabela marca com mensagens prontas). Esgotado, o `GET` vira ouvinte local. |

## Retenção e dead-letter

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `retention.ttl.default` | `TABA_RETENTION_TTL_DEFAULT` | **14d** | `ttl` usado pelas filas que não definem o seu. Vale na hora da conferência: mudar o padrão afeta as filas sem `ttl` próprio. A mensagem mais velha que o TTL é descartada. |
| `retention.max-deliveries.default` | `TABA_RETENTION_MAX_DELIVERIES_DEFAULT` | **10** | `maxDeliveries` usado pelas filas que não definem o seu. Atingido sem ack, a mensagem vai para o dead-letter. |
| `topics.dead-letter.suffix` | `TABA_TOPICS_DEAD_LETTER_SUFFIX` | **-dead-letter** | Sufixo do nome da fila de dead-letter de cada fila. Reservado: tópico com nome terminado nele não pode ser criado (`400`). Mudar o sufixo com filas existentes exige migração; vale definir antes de criar tópicos. |
| `retention.sweep.interval` | `TABA_RETENTION_SWEEP_INTERVAL` | **60s** | De quanto em quanto tempo cada nó varre as próprias mensagens e apaga as vencidas pelo `ttl`. O dono já não entrega mensagem vencida entre uma varredura e outra. |

## Filas FIFO

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `fifo.dedup.window` | `TABA_FIFO_DEDUP_WINDOW` | **300s** | Tempo, a partir do instante do ID, em que uma publicação com o mesmo `deduplicationId` (ou o mesmo hash do corpo) no mesmo grupo é descartada como duplicata. Também é o prazo do `deleteAt` da mensagem confirmada. |
| `fifo.dedup.index.max-entries` | `TABA_FIFO_DEDUP_INDEX_MAX_ENTRIES` | — | Máximo de chaves da janela mantidas em memória, por tópico; o excedente é consultado no banco do tópico. ⚠️ **Aceito, mas sem efeito hoje** (o código lê o valor e não o usa). |
| `fifo.forward.max-attempts` | `TABA_FIFO_FORWARD_MAX_ATTEMPTS` | **3** (do código) | Quantas vezes o nó que recebeu a publicação a repassa de novo, com a tabela de membros atualizada, quando o primário responde `421`. Esgotado, `503` com `Retry-After`. |

## Tópicos voláteis e distribuidores

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `topics.volatile.idle-timeout.default` | `TABA_TOPICS_VOLATILE_IDLE_TIMEOUT_DEFAULT` | **600s** | `idleTimeout` usado quando a fila é criada (por `POST /topics` com `volatile` ou por `autoCreate`) sem informar o valor. |
| `topics.volatile.idle-check.retry-interval` | `TABA_TOPICS_VOLATILE_IDLE_CHECK_RETRY_INTERVAL` | **30s** (do código) | Intervalo para o nó gestor repetir a verificação final quando algum nó não respondeu. |
| `topics.distributor.max-targets` | `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS` | sem teto | Máximo de filas que uma publicação num Tópico Distribuidor pode casar; acima disso a publicação é recusada inteira (`422`). |
| `topics.volatile.max-count` | `TABA_TOPICS_VOLATILE_MAX_COUNT` | sem teto | Máximo de filas voláteis no cluster; atingido, o `autoCreate` é recusado (`422`). |
| `topics.volatile.takeover.jitter` | `TABA_TOPICS_VOLATILE_TAKEOVER_JITTER` | **2s** (do código) | Janela em que o nó que assume filas voláteis (entrada ou saída de nó) espalha suas primeiras consultas de ociosidade. |

## Observabilidade

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `telemetry.prometheus.enabled` | `TABA_TELEMETRY_PROMETHEUS_ENABLED` | **true** | Liga/desliga `GET /metrics` (formato Prometheus, só as métricas deste nó). |
| `operations.port` | `TABA_OPERATIONS_PORT` | **9090** | Porta da API de operação, **sem autenticação**: `/metrics`, `/metrics/topics/*`, `/health/*`, `/node/drain`, o backup em S3 e as rotas do cluster inteiro. Serve aos coletores e ao orquestrador **dentro** da infraestrutura (Prometheus, Telegraf, sondas), nunca a pessoas nem ao `taba-cli`. O nó sempre a abre; deve ser diferente de `api.port` e de `cluster.port`. A porta de cliente também serve `/metrics`, **com token**, e as duas sondas sem ele ([16-api-cliente.md](16-api-cliente.md)). |
| `operations.allowed-ips` | `TABA_OPERATIONS_ALLOWED_IPS` | sem restrição | Lista de IPs/faixas CIDR com acesso à porta de operação; os demais recebem `403`. Opcional. Sem lista, a rede é a única proteção. |
| `telemetry.topics.mode` | `TABA_TELEMETRY_TOPICS_MODE` | **off** | Vale para OTLP e `/metrics`: `off` (só rotas `/metrics/topics`), `by-tags` (agregado por tags) ou `per-topic` (uma série por tópico, com teto). |
| `telemetry.topics.group-by-tags` | `TABA_TELEMETRY_TOPICS_GROUP_BY_TAGS` | vazio | Chaves de tag usadas para agregar no modo `by-tags`. |
| `telemetry.topics.max-topics` | `TABA_TELEMETRY_TOPICS_MAX_TOPICS` | **1000** (do código) | Máximo de séries por tópico no modo `per-topic`; o excedente é agregado em `messaging.destination.name="_other"`. |
| `telemetry.logs.level` | `TABA_TELEMETRY_LOGS_LEVEL` | **info** (do código) | Nível mínimo dos logs estruturados: `trace`, `debug`, `info`, `warn` ou `error`. |
| `telemetry.logs.format` | `TABA_TELEMETRY_LOGS_FORMAT` | **text** | Formato dos logs no `stdout`: `text` (texto simples, sem cores), `color` (texto com cores ANSI, bom para terminal e `docker compose logs`) ou `json` (uma linha JSON por evento, para coletores de log). Não muda o que o OTLP exporta. O `docker-compose.yml` usa `color`. |
| `stats.topics.top-groups` | `TABA_STATS_TOPICS_TOP_GROUPS` | **10** | Quantos grupos (`topGroups`) o `GET /metrics/topics/{topic}` devolve. |
| `stats.topics.cache-ttl` | `TABA_STATS_TOPICS_CACHE_TTL` | — | Validade do resultado agregado de `/metrics/topics` (evita repetir a consulta a todos os nós). Com um nó só não há consulta a repetir: o parâmetro ainda não tem efeito. ⚠️ **Aceito, mas sem efeito hoje** (o código lê o valor e não o usa). |
| `stats.topics.backlog.depth`<br>`stats.topics.backlog.age` | `TABA_STATS_TOPICS_BACKLOG_DEPTH`<br>`TABA_STATS_TOPICS_BACKLOG_AGE` | condição desligada<br>condição desligada | `depth` e `oldestAge` acima dos quais o tópico entra na condição `backlog`. Sem valor, a condição não dispara. |
| `stats.topics.noisy-group-share` | `TABA_STATS_TOPICS_NOISY_GROUP_SHARE` | **50** | Parcela do `depth` do tópico acima da qual o maior grupo dispara a condição `noisy-group`. |
| `stats.topics.group-stalled-deliveries` | `TABA_STATS_TOPICS_GROUP_STALLED_DELIVERIES` | condição desligada | Número de entregas da cabeça de um grupo de fila FIFO, sem ack, acima do qual dispara a condição `group-stalled`. Sem valor, a condição não dispara. |
| `stats.topics.stalled-after` | `TABA_STATS_TOPICS_STALLED_AFTER` | condição desligada | Tempo sem consumo, com mensagens acumuladas, para a condição `stalled`. Sem valor, a condição não dispara. |
| `stats.topics.idle-after` | `TABA_STATS_TOPICS_IDLE_AFTER` | condição desligada | Tempo sem publicação nem consumo para a condição `idle`. Sem valor, a condição não dispara. |
| `stats.disk.free-min` | `TABA_STATS_DISK_FREE_MIN` | condição desligada | Abaixo disso o cluster entra na condição `storage-pressure`. Sem valor, a condição não dispara. |

## Rate limits

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `ratelimit.publish.rps` | `TABA_RATELIMIT_PUBLISH_RPS` | sem limite | Requisições por segundo, **por token** (ou por IP do cliente, sem token válido), para `POST /topics/{topic}/messages`. |
| `ratelimit.consume.rps` | `TABA_RATELIMIT_CONSUME_RPS` | sem limite | Requisições por segundo, por token (ou por IP, sem token válido), para `GET /topics/{topic}/messages`, `PUT /topics/{topic}/messages/{id}/visibility` (invisibilidade) e `DELETE /topics/{topic}/messages/{id}` (ack). |
| `ratelimit.admin.rps` | `TABA_RATELIMIT_ADMIN_RPS` | sem limite | Requisições por segundo, por token (ou por IP, sem token válido), para `POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`, e `/tokens`; e, por IP, para `/node/drain`, `/node/backup/flush`, `/cluster/backup/flush` e `/cluster/stop`. |
| `ratelimit.ops.rps` | `TABA_RATELIMIT_OPS_RPS` | sem limite | Requisições por segundo, por IP, para `/health/cluster?scope=cluster` e `/metrics/topics`. `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` são isentos. |
| `ratelimit.burst` | `TABA_RATELIMIT_BURST` | `ceil(rps)` de cada categoria | Tamanho do estouro permitido acima da taxa (ex.: token bucket), para absorver picos curtos sem rejeitar requisições. Vale para cada contador, de token ou de IP. |
| `client-ip.headers.enabled` | `TABA_CLIENT_IP_HEADERS_ENABLED` | **true** | Usa os cabeçalhos abaixo para achar o IP do cliente atrás de balanceador; `false` usa só o endereço da conexão. Só importa para as chamadas contadas por IP (sem token válido) e para `operations.allowed-ips`. |
| `client-ip.headers` | `TABA_CLIENT_IP_HEADERS` | **X-Forwarded-For** | Lista, em ordem, de cabeçalhos consultados; vale o primeiro presente e, nele, o primeiro endereço. |
| `client-ip.trusted-proxies` | `TABA_CLIENT_IP_TRUSTED_PROXIES` | vazia (cabeçalho aceito de qualquer origem) | Lista de IPs/CIDR dos balanceadores ou proxies cujos cabeçalhos de IP são aceitos. Vazia: aceita de qualquer origem (o serviço deve ficar em camada segura). Vale para o rate limit e para `operations.allowed-ips`. |


## Backup opcional em S3

O recurso liga com `backup.s3.bucket`; sem ele nada disto existe. Como funciona: [33-backup-s3.md](33-backup-s3.md).

| Chave | Variável | Padrão | O que faz |
|---|---|---|---|
| `backup.s3.bucket` | `TABA_BACKUP_S3_BUCKET` | sem valor (**recurso desligado**) | O bucket. Definido, liga o backup. |
| `backup.s3.region` | `TABA_BACKUP_S3_REGION` | sem valor | A região. Sem ela o SDK usa a cadeia padrão da AWS (`AWS_REGION`, perfil); com `backup.s3.endpoint` e sem região vale `us-east-1`. |
| `backup.s3.endpoint` | `TABA_BACKUP_S3_ENDPOINT` | sem valor (AWS) | O endereço de um S3 compatível (SeaweedFS, MinIO, Ceph), como `http://seaweedfs:8333`. |
| `backup.s3.prefix` | `TABA_BACKUP_S3_PREFIX` | **taba** | O prefixo base de todos os objetos no bucket. Vários clusters podem dividir um bucket com prefixos diferentes. |
| `backup.s3.access-key` | `TABA_BACKUP_S3_ACCESS_KEY` | sem valor | A chave de acesso. **Opcional:** sem ela (e sem a secreta), vale a cadeia padrão da AWS (IAM, perfil, papel da tarefa). Vai sempre com a secreta. |
| `backup.s3.secret-key` | `TABA_BACKUP_S3_SECRET_KEY` | sem valor | A chave secreta; nunca aparece em log nem em `Debug`. |
| `backup.s3.path-style` | `TABA_BACKUP_S3_PATH_STYLE` | (do código) **true** com `endpoint`, **false** sem | O bucket no caminho (`http://host/bucket/chave`) em vez de no nome do host. O MinIO exige. |
| `backup.delay` | `TABA_BACKUP_DELAY` | **60s** | A mensagem só vai ao S3 depois deste tempo no outbox: a confirmada e consumida antes **nunca vai**. |
| `backup.meta.interval` | `TABA_BACKUP_META_INTERVAL` | **60s** | De quanto em quanto tempo o estado mutável (entrega, invisibilidade, ack) vira um diário. É a defasagem (RPO) do estado. |
| `backup.segment.max-bytes` | `TABA_BACKUP_SEGMENT_MAX_BYTES` | **8MB** | O segmento fecha ao chegar a este tamanho (`MB` é 2²⁰ bytes, como em `message.max-bytes`). |
| `backup.segment.max-messages` | `TABA_BACKUP_SEGMENT_MAX_MESSAGES` | **5000** | O segmento fecha ao chegar a esta quantidade de mensagens. |
| `backup.segment.max-age` | `TABA_BACKUP_SEGMENT_MAX_AGE` | **60s** | O segmento fecha ao chegar a esta idade (do momento em que a primeira mensagem entrou nele). |
| `backup.heartbeat.interval` | `TABA_BACKUP_HEARTBEAT_INTERVAL` | **15s** | De quanto em quanto tempo o nó regrava `nodes/<id>.json` e observa os dos outros. |
| `backup.dead-after` | `TABA_BACKUP_DEAD_AFTER` | **60s** | Um nó é dado como morto para quem observa quando o batimento dele não muda por este tempo. Deve ser **maior** que `backup.heartbeat.interval` (com folga: vários batimentos). Também é o atraso da exclusão de um nó inalcançável. |
| `backup.drain.timeout` | `TABA_BACKUP_DRAIN_TIMEOUT` | **30s** | O quanto o drain, o flush e a pausa por disco cheio esperam o envio ao S3. Estourado, o nó segue sem o S3. O prazo de parada do orquestrador deve ser **maior** que `drain.timeout` + este valor. |
| `backup.disk.evict-above` | `TABA_BACKUP_DISK_EVICT_ABOVE` | **85** | Uso do disco (%) acima do qual o nó apaga o corpo das mensagens já salvas no S3. |
| `backup.disk.evict-to` | `TABA_BACKUP_DISK_EVICT_TO` | **70** | A evicção para quando o uso cai a este valor (%). |
| `backup.disk.pause-at` | `TABA_BACKUP_DISK_PAUSE_AT` | **90** | Durante o flush e o drain, a passagem da memória para o disco pausa a este uso (%) até a evicção liberar espaço. |
| `backup.reconcile.interval` | `TABA_BACKUP_RECONCILE_INTERVAL` | **60s** | De quanto em quanto tempo o gestor regrava as definições que divergem do bucket, apaga segmentos vencidos pelo `ttl` e o nó limpa batimentos mortos. |
| `backup.restore.lock-ttl` | `TABA_BACKUP_RESTORE_LOCK_TTL` | **60s** | A validade da trava de restauração: o titular a renova; parada por este tempo, outro nó a toma. |

## Limites fixos (não são configuração)

As faixas de `ttl`, `maxDeliveries`, `idleTimeout`, nomes, tags, prefixos e o `waitTime` de 0 a 20 s são **fixas** no código: ver [19-erros-e-limites.md](19-erros-e-limites.md).

## Fontes e provas

- `config/params/mod.rs` (todas as chaves e os padrões da tabela), `config/mod.rs` (o `Config` tipado e as validações), `config/source/mod.rs` (precedência e conversão de valores), `config/otlp.rs` (`OTEL_*`), `cluster/service/mod.rs` e `cluster/sync/mod.rs` (os padrões "do código"), `http/ratelimit/mod.rs` (o estouro padrão).
- Casos de uso: UC-CFG (configuração e validações de partida); veja [26a](26a-casos-de-uso-implementados.md). Testes: `config/tests.rs`, `config/params/tests.rs`, `config/source/tests.rs`, `tests/startup.rs`.
- Histórico: `10-configuracao.md`.
