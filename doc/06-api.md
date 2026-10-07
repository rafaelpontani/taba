# API

[← Índice](README.md)

## 8. API (rascunho inicial)
 
> Contrato formal (OpenAPI) ainda não definido — esta seção lista os verbos e recursos previstos até o momento.
 
### 8.1 API de cliente
 
| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/topics` | Cria um tópico (nome, tipo `queue` ou `distributor`, e opcionalmente `tags`; `prefixes` para `distributor`; `volatile` e `idleTimeout` para `queue`; `maxMessageBytes` opcional, tamanho máximo da mensagem do tópico, padrão `message.max-bytes` = 5 MB, podendo ser maior, até `message.max-bytes.hard-limit`; `ttl` e `maxDeliveries` opcionais para `queue`, padrões `retention.ttl.default` = 14 dias e `retention.max-deliveries.default` = 10). Toda fila ganha junto uma fila `{nome}-dead-letter` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10). Gera o `id` (GUID v7) do tópico. Propagado a todos os nós por registro + commit; persistido em disco ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `GET` | `/topics` | Lista os tópicos existentes, **paginada** (seção 8.1.2), com `sort` (`name`, `id`, `createdAt`) e o filtro `name`, que devolve os tópicos cujo nome **contém** o texto informado (ex.: `?name=pedidos`), para ajudar na busca; diferencia maiúsculas de minúsculas, como o nome do tópico. Respondida **localmente**, sem consultar outros nós: a definição do tópico é replicada em todo nó (seção 3.3), diferente de `/stats/topics`, que faz fan-out para métricas de consumo. |
| `GET` | `/topics/{topic}` | Retorna as informações e configurações do tópico (tipo, tags, `ttl`, `maxDeliveries` etc.; na dead-letter, os valores da fila de origem, só leitura, com `deadLetter: true` e `source`). Também respondida localmente, pelo mesmo motivo. |
| `PATCH` | `/topics/{topic}` | Altera informações do tópico (tags; `prefixes` de um `distributor`; `idleTimeout` de uma fila volátil; `maxMessageBytes`; `ttl` e `maxDeliveries` de uma fila). Nome, `id`, tipo e `volatile` são imutáveis. Na fila `{nome}-dead-letter`, `PATCH` e `DELETE` respondem `409`: ela é somente leitura e segue a fila de origem. Mesma propagação do `POST`. |
| `DELETE` | `/topics/{topic}` | Remove o tópico e **todas** as suas mensagens, mesmo as pendentes (ainda não entregues ou não confirmadas) — sempre funciona, sem checagem. Mesma propagação do `POST`; falha no registro **cancela** a remoção (rollback); falha no commit, já decidido, não cancela — o coordenador insiste dentro do teto `write.commit.timeout`. Nunca uma remoção parcial visível. |
| `POST` | `/topics/{topic}/messages` | Publica uma mensagem no tópico. Corpo acima do tamanho máximo do tópico (`maxMessageBytes` ou, sem ele, `message.max-bytes`) retorna `413`. Num `distributor`, copia a mensagem para as filas cujo nome começa com algum prefixo e responde com o resultado por fila; sem fila casando, responde sucesso e descarta ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). |
| `GET` | `/topics/{topic}/messages` | Consome mensagem(ns) do tópico. Rejeitado com `409` num `distributor`. Aceita `autoCreate=true` (cria a fila se não existir, volátil por padrão, com `idleTimeout`, `volatile` e `tag.<chave>` opcionais; seção 14.4), `visibilityTimeout` (segundos, padrão 120): a mensagem entregue fica invisível a outros consumidores por esse tempo; e `waitTime` (segundos, 0 a 20, padrão 0): long polling, aguarda novas mensagens antes de responder vazio; e `maxMessages` (padrão **10**, configurável, com teto): a resposta traz de 1 a `maxMessages` mensagens, as que estiverem prontas ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Sem garantia de ordem entre as mensagens (seção 12.7). |
| `PATCH` | `/topics/{topic}/messages/{id}` | Altera a invisibilidade de uma mensagem já entregue (`{"visibilityTimeout": 300, "receiptToken": "..."}`), por exemplo para ganhar mais tempo de processamento. O `receiptToken` (devolvido na entrega) é obrigatório; token obsoleto retorna `409`. O valor é o novo tempo **a partir de agora**, não um acréscimo; `0` libera a mensagem na hora. |
| `DELETE` | `/topics/{topic}/messages/{id}` | Confirma o consumo (ack) de uma mensagem, removendo-a do tópico. Exige `receiptToken` (query string), o da última entrega; token obsoleto retorna `409`. Decide o commit por **quórum** (sempre com o primário): falha antes do quórum **cancela** o ack (rollback); falha numa réplica fora do quórum, já decidido, não cancela — insiste dentro do teto `write.commit.timeout` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.4). |

**Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`):
- de 1 a 100 caracteres na criação; nome terminado no sufixo de dead-letter (`-dead-letter`) é reservado e rejeitado com `400`. A fila de dead-letter, de nome `{fila}` + sufixo, pode passar de 100 (até 112 com o sufixo padrão), e o `{topic}` das rotas a aceita;
- apenas letras ASCII (`a-z`, `A-Z`), dígitos (`0-9`), hífen (`-`) e sublinhado (`_`) — regex `^[A-Za-z0-9_-]{1,100}$`;
- diferencia maiúsculas de minúsculas (`Pedidos` e `pedidos` são tópicos distintos);
- nomes inválidos são rejeitados na criação (`POST /topics`) com erro `400`.

**Tags do tópico** (`tags`): pares chave/valor definidos na criação (`POST /topics`) e alterados via `PATCH /topics/{topic}`. Servem para monitoramento: são enviadas como **atributos** nos traces e logs exportados via **OpenTelemetry** para as operações daquele tópico, e nas métricas de tópico conforme a configuração de exportação ([11-observabilidade.md](11-observabilidade.md)) (ex.: `{"time": "financeiro", "ambiente": "prod"}`).
- chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL);
- máximo de 20 tags por tópico;
- as tags não afetam o comportamento das mensagens, apenas a telemetria.
 
### 8.1.1 API de operação (observabilidade)

Rotas para operadores, não para aplicações clientes. Detalhes, parâmetros e métricas em [11-observabilidade.md](11-observabilidade.md). Opcionalmente podem ser servidas em **porta própria** e/ou restritas a uma **lista de IPs/faixas** (`operations.port`, `operations.allowed-ips`); sem essa configuração, ficam na porta da API de cliente e são tratadas como qualquer outra rota ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics` | Métricas do cluster inteiro (padrão, uma série por nó) ou só deste nó (`scope=node`), no formato Prometheus. Alvo único de scrape: o balanceador. Lê só uma cópia em memória, atualizada por uma thread (seção "Endpoint Prometheus" de [11-observabilidade.md](11-observabilidade.md)). |
| `GET` | `/stats/node` | Métricas do nó que atende a chamada (leitura local). |
| `GET` | `/stats/cluster` | Métricas de todos os nós e condição geral do cluster (consulta todos os nós, *best-effort*). |
| `GET` | `/stats/topics` | Métricas de tópicos, **paginada** (`limit`, `cursor`, `tag.*`, `condition`, `fields`, `groupBy`, `scope`), **sem `sort`**: a ordem é sempre por nome do tópico. Visão do cluster por padrão; `scope=node` devolve só os valores do nó. |
| `GET` | `/stats/topics/{topic}` | Métricas detalhadas e condição de um tópico. |
| `GET` | `/health/live` | O processo está de pé. |
| `GET` | `/health/ready` | O nó está apto a receber tráfego (registrado no cluster e com toda a sincronização concluída: tópicos, blocos e banco de eventos; seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)). |

### 8.1.2 Paginação

Toda rota de **cliente ou de operação** que pode devolver um número ilimitado de itens é paginada, sempre com a mesma convenção:

| Parâmetro | Descrição |
|---|---|
| `limit` | Itens por página. Tem padrão e máximo (a calibrar, [10-configuracao.md](10-configuracao.md)). |
| `cursor` | Token **opaco** devolvido em `nextCursor` pela página anterior. O cliente nunca o monta. |
| `sort` | Campo de ordenação, prefixo `-` para decrescente. Só nas rotas que o aceitam (hoje, `GET /topics`), e só com campos imutáveis. Sem `sort`, a ordem é a do nome. |

A resposta traz os itens e `nextCursor`; sem `nextCursor`, acabou. Não há `page`/`offset` nem `total` (contar custa caro e o valor muda durante a leitura).

**Para não perder nem repetir itens entre páginas**, a paginação é por **chave** (*keyset*), não por deslocamento:

- A ordem é sempre **total e estável**: toda ordenação termina num campo único e imutável como desempate (o nome ou o `id`). `sort=-createdAt` vale, na prática, `(createdAt desc, id asc)`.
- O cursor guarda a **última chave lida**, mais o `sort` e os filtros em uso. A página seguinte pede "depois dessa chave", então inserir ou remover itens antes dela não desloca nada. Cursor com `sort` ou filtros diferentes dos atuais é rejeitado com `400`.
- Repetir um cursor devolve a mesma página enquanto os dados não mudarem, e o cursor **não expira** (não há estado no servidor).

A garantia é a de quem percorre tudo: recebe todo item que existia durante o percurso inteiro, sem repetição. Itens criados ou removidos durante o percurso podem ou não aparecer. É consistência fraca, suficiente para consulta; as rotas entre nós usam outro mecanismo (seção 8.2.1).

Por isso nenhuma rota ordena por valor que muda: `GET /stats/topics` não tem `sort` e pagina sempre por nome.

### 8.2 API interna (cluster)

> Cada rota `rollback` só descarta um registro (fase 1) **pendente, sem commit** — falha ou timeout de retries antes de fechar o quórum. Uma falha na chamada de `commit` (fase 2) numa réplica fora do quórum, já decidido, nunca aciona `rollback`: o coordenador insiste no commit dentro do teto `write.commit.timeout` até confirmar em todas as réplicas ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).
>
> **Mensagens (rotas abaixo com `messages/{id}`, `visibility` e `ack`) decidem o commit por quórum** (`write.quorum`, sempre incluindo o primário do bloco) — a chamada de `register` ainda vai a todas as réplicas, mas o `commit` é decidido e respondido ao cliente assim que o quórum confirma, sem esperar as demais. **Nó e tópico (`nodes`, `topics`) continuam exigindo confirmação de todos os nós**, sem quórum reduzido (seção 7.2).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/cluster/nodes` | Lista a tabela de membros (ID, IPs internos, porta, estado). É a primeira chamada de um nó ao iniciar, feita pelo endereço do balanceador (`cluster.entry-address`); qualquer nó a responde. |
| `POST` | `/cluster/nodes/join` | Pedido de ingresso do nó novo, com seus IPs e porta, feito a um membro da lista. O membro testa os IPs e coordena o registro + commit (seção 6.2 de [04-cluster.md](04-cluster.md)); responde com a tabela de nós. |
| `POST` | `/cluster/nodes/register` | Fase 1 (registro) de um novo nó: propaga os dados do novo nó (IP/porta) a todos os nós. |
| `POST` | `/cluster/nodes/commit` | Fase 2 (commit) de registro **ou** desregistro de nó: efetiva a operação na tabela de membros de todos os nós. |
| `POST` | `/cluster/topics/register` | Fase 1 (registro) de uma criação, alteração ou remoção de tópico: propaga a definição a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `POST` | `/cluster/topics/commit` | Fase 2 (commit): efetiva a definição na tabela de tópicos de todos os nós, e cada um grava/atualiza/remove o `${topico}.json` local. |
| `POST` | `/cluster/topics/rollback` | Descarta a alteração de tópico pendente (sem commit) após esgotar os retries. |
| `GET` | `/cluster/topics/{id}/idle` | Usado pelo **nó gestor** de uma fila volátil: devolve há quanto tempo foi o último `GET` do tópico neste nó (duração decorrida; sem registro, o tempo desde que o nó subiu). Leitura local, sem estado pendente ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5). |
| `POST` | `/cluster/vote-remove` | Usado para votar a remoção de um nó suspeito de falha (51% dos nós saudáveis, com mínimo de votantes — aberta quando o canal com o nó fica fora após as tentativas de reconexão, antes do commit de desregistro). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/register` | Fase 1 (registro) de uma mensagem, enviado ao(s) nó(s) responsável(is) pelo bloco (primário/réplicas). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/commit` | Fase 2 (commit) da entrega de uma mensagem já registrada nos nós responsáveis; decidido assim que o quórum confirma (primário incluso), enviado às demais réplicas em paralelo/segundo plano. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/rollback` | Descarta o registro pendente (sem commit) de uma mensagem cujo registro falhou após os retries. |
| `GET` | `/cluster/channel` | Abre (upgrade para **WebSocket**) o **canal permanente** entre dois nós, aberto pelo nó que entra, volta ou detecta primeiro a queda; em conexão dupla, o nó de menor hash do ID fecha a que abriu (um por par, para todos os tópicos): carrega o **sinal de vida** que substitui o ping (seção 6.3) e os sinais de disponibilidade da entrega — tópicos com ouvintes, tópicos com mensagens prontas e avisos de mensagem nova (seção 12.8). Só sinais, nunca conteúdo de mensagens nem listas longas. |
| `POST` | `/cluster/topics/{topic}/messages/take` | Pede ao nó dono que entregue até `maxMessages` mensagens prontas do tópico (a quantidade que ainda falta ao `GET` do cliente), com `visibilityTimeout`: ele faz o registro + commit de invisibilidade e devolve as mensagens ao nó que pediu (seção 12.8). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/register` | Fase 1 (registro) de uma entrega ou de uma alteração de invisibilidade: grava como pendente o novo `invisibleUntil` (e o `deliveryCount`, se for entrega) nos nós responsáveis. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/commit` | Fase 2 (commit): efetiva a invisibilidade; no caso de entrega, o nó também remove o ID da sua lista de prontos. Decidido por quórum (primário incluso) — o primário sempre recebe essa chamada antes da resposta ao cliente. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/rollback` | Descarta a alteração de invisibilidade pendente após esgotar os retries. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/register` | Fase 1 (registro) do ack: pedido de remoção da mensagem a todos os nós responsáveis pelo bloco. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/commit` | Fase 2 (commit): remove a mensagem (e seus metadados); decidido por quórum (primário incluso), replicado às demais réplicas em segundo plano. Sem tombstone: a ausência do ID já basta para a sincronização (seção 7.3). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/rollback` | Descarta o registro de ack pendente após esgotar os retries; a mensagem permanece disponível. |
| `GET` | `/cluster/topics` | **Stream** (NDJSON, seção 8.2.1) com as definições de tópico que o nó possui, ordenadas por `id`; `from`/`to` opcionais. Usado pelo nó novo para montar a tabela de tópicos depois do registro, inclusive na verificação final (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)). |
| `GET` | `/cluster/metrics` | Métricas **locais** do nó, no formato Prometheus. É o que a thread de atualização das métricas do cluster consulta em cada nó (seção "Endpoint Prometheus" de [11-observabilidade.md](11-observabilidade.md)). |
| `GET` | `/cluster/blocks/{id}/ids` | **Stream** (NDJSON) com os IDs de mensagens que o nó possui para o bloco, cada um com o nome do tópico, ordenados por ID; `from`/`to` opcionais. Usado na sincronização distribuída, inclusive na verificação final (seção 7.3). |
| `GET` | `/cluster/blocks/{id}/messages` | **Stream** (NDJSON) com as mensagens do bloco que o nó possui na faixa `from`–`to` (IDs, inclusivos). É a chamada balanceada da sincronização: o nó novo divide a lista unida de IDs em faixas e pede cada faixa a um nó de origem. |
| `GET` | `/cluster/blocks/{id}/messages/{messageId}` | Retorna o conteúdo de **uma** mensagem do bloco. Mantida por consistência REST e para análise individual quando há problema com uma mensagem; também serve de reforço para uma mensagem que faltou numa faixa. |
| `GET` | `/cluster/message-loss` | **Stream** (NDJSON) com a tabela `perdas` do nó: os pares (`topic`, `id`) declarados perdidos na sincronização, ordenados por ID; `from`/`to` opcionais. Consultada em todos os nós por um nó que retorna (seção 6.6). Aceita chamadas de nó que ainda não é membro. |
| `POST` | `/cluster/messages/recover` | Usado por um nó que retorna ao cluster (seção 6.6) para enviar ao responsável pelo bloco as mensagens que encontrou na lista de perdas (corpo e `deliveryCount`). O **corpo da requisição** é um stream NDJSON (seção 8.2.1). O nó que recebe grava **direto nos nós responsáveis pelo bloco**, idempotente pelo ID, sem fase de registro pendente nem rollback; a linha conta como aplicada quando `write.quorum` nós confirmam. Quem aceita apaga o registro de perda e avisa os demais nós. Aceita chamadas de nó que ainda não é membro. |
| `GET` | `/cluster/stats/node` | Métricas locais do nó, para a agregação de `/stats/cluster`. |
| `GET` | `/cluster/stats/topics` | Contadores locais por tópico (apenas mensagens em que o nó é primário: visíveis, invisíveis e data da mais antiga), agregados por quem atende `/stats/topics` (soma das quantidades, menor data). |

### 8.2.1 Rotas de sincronização: streams NDJSON

As rotas entre nós que podem mover volume grande (`/cluster/topics`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/messages` e o corpo do `/cluster/messages/recover`) **não usam a paginação do cliente** (seção 8.1.2). Quem sincroniza precisa saber que recebeu **tudo**, e a conexão é sempre direta entre dois nós, sem balanceador no caminho. O contrato é:

- **Formato:** `Content-Type: application/x-ndjson`, **um objeto JSON por linha**. Emissor e receptor trabalham com um buffer por linha, sem montar a resposta inteira em memória. O tamanho máximo de uma linha é limitado (`cluster.sync.stream.max-line-bytes`).
- **Ordem:** por **ID ascendente** (GUID v7, comparado como texto na forma canônica em minúsculas), que acompanha a ordem de criação. Mensagem e definição de tópico não mudam o `id`.
- **Faixa:** `from` e `to` (ambos **inclusivos**, ambos opcionais) delimitam os IDs pedidos. `limit` opcional reduz o teto de itens do stream.
- **Linha final obrigatória:** o stream termina com `{"end":true,"count":N,"last":"<id>","hasMore":false}`. **Sem essa linha o trecho é considerado falho**, mesmo que a conexão tenha fechado limpa: um corte entre duas linhas não se distingue de um fim normal sem ela.
- **Teto por stream:** o emissor fecha o stream ao atingir `cluster.sync.stream.max-items` ou `cluster.sync.stream.max-bytes`, com `hasMore: true`. O receptor abre outro com `from=<last>`.
- **Retomada:** depois de um corte ou de `hasMore`, o receptor repete a chamada com `from=<último ID lido>`. O emissor devolve de novo essa linha, e o receptor a **ignora** (aplicação idempotente por ID). Não há token nem estado no servidor, e a repetição devolve as mesmas linhas porque a mensagem não muda depois de gravada.
- **Erro depois do `200`:** o status já foi enviado, então o emissor escreve `{"error":"..."}` e fecha, ou apenas fecha sem a linha final. Nos dois casos o receptor retoma pela regra acima.
- **Timeout:** vale um **timeout de leitura por linha** (`cluster.sync.stream.idle-timeout`), não um teto para o stream inteiro. Não há *heartbeat*: sem balanceador, só um nó parado ou lento fica sem escrever.
- **Contrapressão:** o receptor lê no próprio ritmo e o TCP segura o emissor. O emissor não mantém transação nem lista congelada aberta; ele lê o armazenamento em blocos pequenos.
- **Corpo de `POST`** (`/cluster/messages/recover`): o mesmo formato, com a mesma linha final no corpo. A resposta também termina com `{"end":true,"count":N}`, com a quantidade aplicada. Se a conexão cair, quem envia reenvia tudo; mensagens já aplicadas são ignoradas.

Cada rota usa o mesmo formato com linhas diferentes:

| Rota | Linha de dados |
|---|---|
| `/cluster/topics` | A definição do tópico (a mesma de `_topics/${topico}.json`). |
| `/cluster/blocks/{id}/ids` | `{"id":"<id>","topic":"<topico>"}` |
| `/cluster/message-loss` | `{"id":"<id>","topic":"<topico>"}` |
| `/cluster/blocks/{id}/messages` | A mensagem completa (`id`, conteúdo e metadados). |

Quando faltar mensagem numa faixa (o `count` é menor que os IDs pedidos), o receptor busca só as que faltaram em outro nó de origem, e a rota por ID único serve de último recurso. Se nenhum nó a tem, vale o evento de perda da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md), registrado no log com o tópico e o ID.

Esse contrato **não cria tombstone nem log novo**: a remoção continua por ausência do ID, e a janela residual da seção 7.3 continua tratada como lá.

