# API do cluster

[← Índice](README.md) · [Visão geral das APIs](06-api.md)

## 8.2 API interna (cluster)

> **Porta:** todas as rotas desta seção ficam na **porta do cluster** (`cluster.port`, [06-api.md](06-api.md), seção 8.0) e nunca na da API de cliente, que responde `404` a `/cluster/*`. Só os outros nós devem alcançá-la. **Autenticação:** TLS mútuo com os certificados do cluster ([06-api.md](06-api.md), seção 8.0.1).

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a porta do cluster **não é aberta**, então nenhuma rota `/cluster/*` responde. Um nó em modo produção não consegue ingressar num nó em desenvolvimento.

> Cada rota `rollback` só descarta um registro (fase 1) **pendente, sem commit** — falha ou timeout de retries antes de fechar o quórum. Uma falha na chamada de `commit` (fase 2) numa réplica fora do quórum, já decidido, nunca aciona `rollback`: o coordenador insiste no commit dentro do teto `write.commit.timeout` até confirmar em todas as réplicas ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).
>
> **Mensagens (rotas abaixo com `messages/{id}`, `visibility` e `ack`) decidem o commit por quórum** (`write.quorum`, sempre incluindo o primário do bloco) — a chamada de `register` ainda vai a todas as réplicas, mas o `commit` é decidido e respondido ao cliente assim que o quórum confirma, sem esperar as demais. **Nó e tópico (`nodes`, `topics`) continuam exigindo confirmação de todos os nós**, sem quórum reduzido (seção 7.2).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/cluster/nodes` | Lista a tabela de membros (ID, IPs internos, porta do cluster, estado). É a primeira chamada de um nó ao iniciar, feita pelo endereço do balanceador (`cluster.entry-address`, que aponta para a porta do cluster); qualquer nó a responde. É a **única** rota desta seção que passa por balanceador. |
| `POST` | `/cluster/nodes/join` | Pedido de ingresso do nó novo, com seus IPs e a porta do cluster, feito a um membro da lista. O membro testa os IPs (conectando à porta do cluster informada) e coordena o registro + commit (seção 6.2 de [04-cluster.md](04-cluster.md)); responde com a tabela de nós. |
| `POST` | `/cluster/nodes/register` | Fase 1 (registro) de um novo nó: propaga os dados do novo nó (IP e porta do cluster) a todos os nós. |
| `POST` | `/cluster/nodes/unregister` | Fase 1 (registro) do **desregistro voluntário** de um nó em drain, enviada por ele a todos os nós; sem votação. O commit usa `/cluster/nodes/commit` ([04-cluster.md](04-cluster.md), seção 6.10). |
| `POST` | `/cluster/nodes/commit` | Fase 2 (commit) de registro **ou** desregistro de nó: efetiva a operação na tabela de membros de todos os nós. |
| `POST` | `/cluster/topics/register` | Fase 1 (registro) de uma criação, alteração ou remoção de tópico: propaga a definição a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `POST` | `/cluster/topics/commit` | Fase 2 (commit): efetiva a definição na tabela de tópicos de todos os nós, e cada um grava/atualiza/remove o `${topico}.json` local. |
| `POST` | `/cluster/topics/rollback` | Descarta a alteração de tópico pendente (sem commit) após esgotar os retries. |
| `POST` | `/cluster/tokens/register` | Fase 1 (registro) da criação, alteração ou revogação de um token de acesso: propaga o registro (com o hash do segredo, nunca o segredo) a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.4). |
| `POST` | `/cluster/tokens/commit` | Fase 2 (commit): efetiva o token na tabela de todos os nós, e cada um grava, atualiza ou apaga o `_tokens/${id}.json` local. |
| `POST` | `/cluster/tokens/rollback` | Descarta a alteração de token pendente (sem commit) após esgotar os retries. |
| `GET` | `/cluster/tokens` | **Stream** (NDJSON, seção 8.2.1) com os tokens que o nó possui, ordenados por `id`; `from`/`to` opcionais. Usado pelo nó novo, junto com `/cluster/topics`, no bootstrap. |
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
| `*` | `/cluster/forward/{caminho}` | **Repasse** de uma chamada de cliente a outro nó: de quem está sincronizando ou em drain (seção 6.9 de [04-cluster.md](04-cluster.md)) e, na fila FIFO, da publicação ao primário do grupo (seção 16.4 de [17-topicos-fifo.md](17-topicos-fifo.md)). `{caminho}` é o caminho original da API de cliente (ex.: `/cluster/forward/topics/pedidos/messages`); método, query, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu. Com a autenticação ligada, o destino **confere o token de novo** (o `Authorization` segue como veio; um nó em sincronização ainda não tem a tabela de tokens, [06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3). Aceita só de membros da tabela e de nós que saíram por drain; a própria rota é o marcador de repasse, e quem a recebe nunca repassa de novo. |
| `GET` | `/cluster/stats/node` | Métricas locais do nó, para a agregação de `/stats/cluster`. |
| `GET` | `/cluster/stats/topics` | Contadores locais por tópico (apenas mensagens em que o nó é primário: visíveis, invisíveis e data da mais antiga), agregados por quem atende `/stats/topics` (soma das quantidades, menor data). |

### 8.2.1 Rotas de sincronização: streams NDJSON

As rotas entre nós que podem mover volume grande (`/cluster/topics`, `/cluster/tokens`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/messages` e o corpo do `/cluster/messages/recover`) **não usam a paginação do cliente** (seção 8.1.2, [06a-api-cliente.md](06a-api-cliente.md)). Quem sincroniza precisa saber que recebeu **tudo**, e a conexão é sempre direta entre dois nós, sem balanceador no caminho. O contrato é:

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
| `/cluster/tokens` | O token (o mesmo de `_tokens/${id}.json`, com o hash do segredo). |
| `/cluster/blocks/{id}/ids` | `{"id":"<id>","topic":"<topico>"}`; na fila FIFO, a linha de uma mensagem com exclusão lógica traz também `"deleteAt"` (seção 16.7 de [17-topicos-fifo.md](17-topicos-fifo.md)). |
| `/cluster/message-loss` | `{"id":"<id>","topic":"<topico>"}` |
| `/cluster/blocks/{id}/messages` | A mensagem completa (`id`, conteúdo e metadados). Na fila FIFO, a mensagem com exclusão lógica vem **sem conteúdo** e com `"deleteAt"`. |

Quando faltar mensagem numa faixa (o `count` é menor que os IDs pedidos), o receptor busca só as que faltaram em outro nó de origem, e a rota por ID único serve de último recurso. Se nenhum nó a tem, vale o evento de perda da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md), registrado no log com o tópico e o ID.

Esse contrato **não cria tombstone nem log novo**: a remoção continua por ausência do ID, e a janela residual da seção 7.3 continua tratada como lá.
