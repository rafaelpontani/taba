# API do cluster

[← Índice](README.md) · [Visão geral das APIs](06-api.md) · [Convenções e DTOs padronizados](06d-api-convencoes.md)

## 8.2 API interna (cluster)

Manual das rotas que **os nós usam entre si**: registro e desregistro de nós, propagação de tópicos e tokens, replicação de mensagens, entrega entre nós, sincronização de blocos, recuperação e métricas. Nenhuma aplicação cliente deve chamá-las.

> **Porta:** todas as rotas desta seção ficam na **porta do cluster** (`cluster.port`, [06-api.md](06-api.md), seção 8.0) e nunca na da API de cliente, que responde `404` a `/cluster/*`. Só os outros nós devem alcançá-la. **Autenticação:** TLS mútuo com os certificados do cluster ([06-api.md](06-api.md), seção 8.0.1); uma conexão sem certificado aceito falha **no handshake**, antes de qualquer resposta HTTP.
>
> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a porta do cluster **não é aberta**, então nenhuma rota `/cluster/*` responde. Um nó em modo produção não consegue ingressar num nó em desenvolvimento.
>
> **Formato, cabeçalhos, `ErrorResponse`, códigos de status e catálogo de erros:** [06d-api-convencoes.md](06d-api-convencoes.md). Os DTOs desta API **não repetem o status HTTP**.

### Regras comuns às rotas do cluster

**Quem pode chamar.** Quem consta na **tabela de membros** do nó. Quem não consta recebe `403` `cluster.not-member`. Aceitam também quem **ainda não é membro**: `GET /cluster/nodes` e `POST /cluster/nodes/join` (o nó que entra), `GET /cluster/message-loss` e `POST /cluster/messages/recover` (o nó excluído que volta, seção 6.6 de [04-cluster.md](04-cluster.md)). O repasse (`/cluster/forward`) aceita também os nós que saíram por drain.

**Protocolo de duas fases.** As rotas `register`, `commit` e `rollback` implementam o registro + commit ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2):

- Cada operação tem um **`operationId`** (GUID v7 gerado pelo coordenador). As três fases levam o mesmo `operationId`, e **todas são idempotentes por ele**: repetir um `register`, um `commit` ou um `rollback` devolve o mesmo `Ack`, sem efeito novo.
- Cada rota `rollback` só descarta um registro (fase 1) **pendente, sem commit**: falha ou timeout de retries antes de fechar o quórum. Uma falha na chamada de `commit` numa réplica fora do quórum, já decidido, **nunca** aciona `rollback`: o coordenador insiste no commit dentro do teto `write.commit.timeout` até confirmar em todas as réplicas (seção 7.2).
- **Mensagens** (rotas com `messages/{id}`, `visibility` e `ack`) decidem o commit por **quórum** (`write.quorum`, sempre incluindo o primário do bloco): o `register` vai a todas as réplicas, mas o `commit` é decidido e respondido ao cliente assim que o quórum confirma. **Nó, tópico e token** exigem a confirmação de **todos** os nós, sem quórum reduzido.
- Um registro sem commit nem rollback expira sozinho após `write.pending.ttl`. Um `commit` ou `rollback` de uma operação que o nó não conhece (já expirou, ou nunca chegou) responde `404` `cluster.operation-unknown`.
- Um nó **em sincronização** confirma o `register` gravando-o no banco de eventos, e o aplica depois ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3); para o coordenador, a confirmação conta normalmente.

**Status comuns:**

| Status | Código | Quando |
|---|---|---|
| `200` | — | Operação aceita ou consulta atendida. |
| `400` | `validation.failed`, `validation.required`, `validation.unknown-field` | Corpo inválido. |
| `403` | `cluster.not-member` | O chamador não é membro. |
| `404` | `cluster.operation-unknown`, `topic.not-found`, `message.not-found` | Operação, tópico ou mensagem desconhecidos. |
| `409` | `cluster.conflict` | Há outra operação pendente para o mesmo recurso, ou o estado atual impede (mensagem já invisível, `receiptToken` diferente). |
| `421` | `fifo.misdirected` | Só na publicação FIFO repassada: o nó não é mais o primário do grupo. |
| `503` | `cluster.node-syncing`, `cluster.node-draining` | O nó não pode atender essa rota agora. |

### Resumo das rotas

| Grupo | Rotas |
|---|---|
| **Membros e canal** | `GET /cluster/nodes`, `POST /cluster/nodes/join`, `POST /cluster/nodes/register`, `POST /cluster/nodes/unregister`, `POST /cluster/nodes/commit`, `POST /cluster/vote-remove`, `GET /cluster/channel` |
| **Tópicos e tokens** | `POST /cluster/topics/register`, `commit`, `rollback`; `GET /cluster/topics`; `POST /cluster/tokens/register`, `commit`, `rollback`; `GET /cluster/tokens`; `GET /cluster/topics/{id}/idle` |
| **Mensagens** | `POST /cluster/topics/{topic}/messages/{id}/register`, `commit`, `rollback`; `.../visibility/register`, `commit`, `rollback`; `.../ack/register`, `commit`, `rollback`; `POST /cluster/topics/{topic}/messages/take` |
| **Sincronização e recuperação** | `GET /cluster/blocks/{id}/ids`, `GET /cluster/blocks/{id}/messages`, `GET /cluster/blocks/{id}/messages/{messageId}`, `GET /cluster/message-loss`, `POST /cluster/messages/recover` |
| **Observabilidade** | `GET /cluster/status`, `GET /cluster/metrics/topics` |
| **Repasse** | `* /cluster/forward/{caminho}` |

## Membros e canal

### `GET /cluster/nodes`: tabela de membros

É a **primeira chamada** de um nó ao iniciar, feita pelo endereço do balanceador (`cluster.entry-address`, que aponta para a porta do cluster); qualquer nó a responde. É a **única** rota desta seção que passa por balanceador. Aceita chamador que ainda não é membro.

**Resposta: `200`, `MemberTable`.** Erros: `503` `cluster.node-syncing` (o nó ainda não tem a tabela).

### `POST /cluster/nodes/join`: pedido de ingresso

Feito pelo nó novo a **um membro** da lista. O membro **testa os IPs** informados (conectando à porta do cluster), coordena o registro + commit com **todos** os nós (seção 6.2 de [04-cluster.md](04-cluster.md)) e só então responde. A chamada **pode demorar**: o pedido espera a vez se há outra mudança de membros ou uma votação em andamento (seção 6.8), e o chamador deve usar um timeout generoso.

**Corpo: `JoinRequest`**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `nodeId` | texto | sim | ID (GUID v7) que o nó gerou ao subir. |
| `addresses` | lista de texto | sim | Os IPs do nó, para os outros se conectarem. |
| `port` | inteiro | sim | A **porta do cluster** do nó. |
| `version` | texto | sim | Versão do Taba. |

**Resposta: `200`, `MemberTable`**, já com o commit confirmado em todo o cluster. A tabela de tópicos e a de tokens **não** vão aqui: o nó as puxa por `GET /cluster/topics` e `GET /cluster/tokens`.

| Status | Código | Quando |
|---|---|---|
| `403` | `cluster.node-frozen` | O nó está em período de freezing. Traz `Retry-After` com o tempo que falta (seção 6.7). |
| `422` | `cluster.join-unreachable` | O membro não conseguiu conectar de volta aos IPs informados. O nó é excluído e repete o bootstrap. |
| `503` | `cluster.registration-failed` | A fase de registro não foi confirmada por todos; o registro foi cancelado (rollback). |
| `503` | `cluster.commit-timeout` | O registro foi confirmado e o commit continua em segundo plano: o nó **foi aceito**. Antes de repetir, consulte `GET /cluster/nodes`. |

### `POST /cluster/nodes/register` e `POST /cluster/nodes/unregister`: fase 1

`register` propaga os dados do nó novo a todos os nós. `unregister` é o **desregistro voluntário** de um nó em drain, enviado por ele mesmo a todos os nós, **sem votação** (seção 6.10 de [04-cluster.md](04-cluster.md)). A fase 2 dos dois é `POST /cluster/nodes/commit`. Resposta: `200`, `Ack`. Erros: `409` `cluster.conflict`.

| DTO | Campos |
|---|---|
| `NodeRegisterRequest` | `operationId`, `coordinatorId` (o nó que coordena) e `node` (um `Member`). |
| `NodeUnregisterRequest` | `operationId`, `coordinatorId`, `nodeId` (quem sai) e `reason`: `vote` ou `drain`. |

### `POST /cluster/nodes/commit`: fase 2

Efetiva o registro **ou** o desregistro na tabela de membros do nó. Corpo: `OperationRef`. Resposta: `200`, `Ack`. Erros: `404` `cluster.operation-unknown`. Falha de commit não gera rollback (regra comum acima).

### `POST /cluster/vote-remove`: voto de exclusão

Pedido de voto para excluir um nó suspeito de falha, aberto quando o canal com ele fica fora após as tentativas de reconexão, ou quando a fila de saída estoura, ou quando o nó entra em falha (seção 6.3). Exige 51% dos nós saudáveis, com o mínimo de votantes.

**Corpo: `VoteRequest`**: `voteId` (GUID v7), `candidateNodeId` (o nó a excluir), `openedBy` (quem abriu) e `reason` (`channel-down`, `queue-overflow` ou `node-failed`).

**Resposta: `200`, `VoteResponse`**: `vote` (`yes` ou `no`, conforme o que o votante enxerga do candidato). Quem não responde não entra na base do quórum.

### `GET /cluster/channel`: canal permanente (WebSocket)

Abre (**upgrade para WebSocket**, `101 Switching Protocols`) o **canal permanente** entre dois nós: um por par, para todos os tópicos, aberto pelo nó que entra, volta ou detecta primeiro a queda. Em conexão dupla, o nó de menor hash do ID fecha a que abriu. Só aceita quem consta na tabela de membros (`403` `cluster.not-member` antes do upgrade). Carrega o **sinal de vida** que substitui o ping (seção 6.3) e os sinais de disponibilidade da entrega (seção 12.8). **Só sinais, nunca conteúdo de mensagens nem listas longas.**

Cada quadro é um objeto JSON em texto com o campo `type`:

| `type` | Campos | Descrição |
|---|---|---|
| `keepalive` | `at` (data), `load` (indicador de carga do nó), `monitoring` (resumo no estilo OpenTelemetry), `ready` (lista de `{ topic, count }`, até `cluster.channel.keepalive.max-topics`) | **Sinal de vida.** Sem `seq`, não é confirmado nem reenviado: o seguinte o substitui. |
| `listeners` | `seq`, `topic`, `hasListeners` | O tópico passou a ter (ou deixou de ter) ouvintes de long polling neste nó. |
| `ready` | `seq`, `topic`, `hasReady` | O nó passou a ter (ou deixou de ter) mensagens prontas do tópico. |
| `notify` | `seq`, `topic`, `count` | Aviso de mensagem nova: há `count` mensagens prontas; venha buscar com `take`. É uma dica. |
| `node-state` | `seq`, `state` (`draining`) | O nó entrou em drain: os pares deixam de pedir `take` a ele. |
| `confirm` | `seqs` (lista de inteiros) | Confirma sinais **já processados**. Pode agrupar vários. |

Todo quadro, exceto `keepalive` e `confirm`, leva um `seq` crescente por canal e por sentido; o emissor só o tira da fila de saída ao receber o `confirm`, e **reenvia na reconexão** o que ficou sem confirmação (o receptor ignora um `seq` que já processou). Ao abrir o canal, cada lado envia primeiro um **instantâneo**: um `listeners` e um `ready` para cada tópico em que o estado é verdadeiro, e só depois as mudanças (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).

## Tópicos e tokens

### `POST /cluster/topics/register`, `commit` e `rollback`

Propagam a criação, alteração ou remoção de um tópico a **todos** os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.3). O `commit` efetiva a definição na tabela de tópicos, e cada nó grava, atualiza ou remove o `_topics/${topico}.json` local. O `rollback` descarta a alteração pendente (sem commit) após esgotar os retries.

**`register`, corpo: `TopicRegisterRequest`**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `operationId`, `coordinatorId` | texto | sim | Identificam a operação e quem coordena. |
| `operation` | texto | sim | `create`, `update` ou `delete`. |
| `topic` | `TopicDefinition` | sim | A definição completa. Em `delete`, só `name` e `id` são lidos. |
| `expectedId` | texto | não | Em `delete` pelo **nó gestor** de uma fila volátil: o `id` que o tópico deve ter. Cada nó só aceita se o tópico atual com aquele nome ainda tiver esse `id`, para uma fila excluída e recriada nunca ser apagada por engano (seção 14.5 de [15-topicos-distribuidor.md](15-topicos-distribuidor.md)). |

`commit` e `rollback`: corpo `OperationRef`. Resposta de todas: `200`, `Ack`.

| Status | Código | Quando |
|---|---|---|
| `404` | `topic.not-found` | `update` ou `delete` de um tópico que o nó não conhece. |
| `409` | `topic.already-exists`, `cluster.conflict` | `create` de um nome que já existe; outra operação pendente no mesmo tópico; `expectedId` diferente do `id` atual. |
| `404` | `cluster.operation-unknown` | `commit` ou `rollback` de uma operação desconhecida. |

### `POST /cluster/tokens/register`, `commit` e `rollback`

O mesmo para os **tokens de acesso** ([02-arquitetura.md](02-arquitetura.md), seção 3.4): `_tokens/${id}.json` em cada nó. `register` leva o **hash** do segredo, **nunca o segredo**.

**`register`, corpo: `TokenRegisterRequest`**: `operationId`, `coordinatorId`, `operation` (`create`, `update` ou `revoke`) e `token` (`TokenRecord`). `commit` e `rollback`: `OperationRef`. Resposta: `200`, `Ack`. Erros: `409` `cluster.conflict`, `404` `token.not-found` (`update` ou `revoke` de um token desconhecido) e `404` `cluster.operation-unknown`.

### `GET /cluster/topics` e `GET /cluster/tokens`: streams para o bootstrap

Streams NDJSON (seção 8.2.1) com as definições de tópico e os tokens que o nó possui, **ordenados por `id`**; `from` e `to` opcionais. Usados pelo nó novo para montar as tabelas depois do registro, inclusive na verificação final (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)). Linhas de dados: `TopicDefinition` e `TokenRecord`.

### `GET /cluster/topics/{id}/idle`: ociosidade de uma fila volátil

Usada pelo **nó gestor** na verificação final da exclusão por ociosidade. Devolve há quanto tempo foi o último `GET` do tópico **neste nó**, como uma **duração decorrida** (não um instante, para não depender de relógios sincronizados). Leitura local, sem estado pendente ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5). `{id}` é o `id` (GUID v7) do tópico.

**Resposta: `200`, `IdleResponse`**: `topicId`, `idleSeconds` (número) e `sinceStart` (booleano: `true` quando o nó **não tem registro** de `GET` para o tópico, e `idleSeconds` é o tempo desde que o nó subiu). Erro: `404` `topic.not-found`.

## Mensagens

Todas levam o `topic` e o `id` da mensagem no caminho e o `operationId` no corpo. `register` vai a **todos** os nós responsáveis pelo bloco (primário e réplicas); o `commit` é decidido por quórum, com o primário incluso, e enviado às demais réplicas em segundo plano. `commit` e `rollback` recebem `OperationRef`, e respondem `200` `Ack`.

### `POST /cluster/topics/{topic}/messages/{id}/register`, `commit` e `rollback`: escrita

Fase 1, 2 e abortar da publicação de uma mensagem (seção 7.2 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)). O mesmo par de rotas faz o **movimento para o dead-letter** (uma única operação que cria a mensagem na dead-letter e remove da fila de origem, mantendo o mesmo `id`; seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).

**`register`, corpo: `MessageRegisterRequest`**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `operationId`, `coordinatorId` | texto | sim | — |
| `kind` | texto | sim | `publish` ou `dead-letter`. |
| `message` | `MessageRecord` | sim | A mensagem. No `kind: "dead-letter"`, com `deliveryCount: 0`, `invisibleUntil` nulo, `deadLetteredAt` e `deadLetterReason`. |
| `sourceTopic` | texto | só `dead-letter` | A fila de origem, de onde a mensagem sai no commit. |

Erros: `409` `cluster.conflict` (já há um registro pendente para o `id`, ou a mensagem já existe com conteúdo diferente).

### `POST /cluster/topics/{topic}/messages/{id}/visibility/register`, `commit` e `rollback`: invisibilidade

Fase 1, 2 e abortar de uma **entrega** ou de uma **alteração** de invisibilidade (seção 13.3 de [14-invisibilidade.md](14-invisibilidade.md)). No `commit` de uma entrega, o nó também remove o ID da sua lista de prontos, e o **primário sempre recebe** essa chamada antes da resposta ao cliente.

**`register`, corpo: `VisibilityRegisterRequest`**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `operationId`, `coordinatorId` | texto | sim | O coordenador é sempre o **dono** da entrega, para o relógio que fixa `invisibleUntil` ser o dele. |
| `kind` | texto | sim | `delivery` ou `change`. |
| `invisibleUntil` | data | sim | O novo prazo **absoluto**. As réplicas guardam o valor pronto e nunca o recalculam. |
| `receiptToken` | texto | sim | Em `delivery`, o token **novo** gerado pelo dono. Em `change`, o token que o cliente enviou, que deve conferir com o atual. |
| `deliveryCount` | inteiro | só `delivery` | O novo contador (o anterior mais 1). |

Erros: `404` `message.not-found`; `409` `cluster.conflict` (já há outro registro pendente para a mensagem, ou, numa `delivery`, ela já está invisível) e `409` `message.stale-receipt` (em `change`, o `receiptToken` não confere).

### `POST /cluster/topics/{topic}/messages/{id}/ack/register`, `commit` e `rollback`: ack

Fase 1, 2 e abortar do ack (seção 12.4 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). O `commit` remove a mensagem e seus metadados **sem tombstone**: a ausência do ID já basta para a sincronização (seção 7.3). Na fila FIFO grava `deleteAt` e apaga só o corpo e os metadados de entrega (seção 16.7 de [17-topicos-fifo.md](17-topicos-fifo.md)). O `rollback` mantém a mensagem disponível.

**`register`, corpo: `AckRegisterRequest`**: `operationId`, `coordinatorId`, `receiptToken` (deve conferir) e `deleteAt` (data, **só na fila FIFO**). Erros: `404` `message.not-found`, `409` `message.stale-receipt`, `409` `cluster.conflict`.

### `POST /cluster/topics/{topic}/messages/take`: entrega pelo dono

Pede ao nó **dono** que entregue até `maxMessages` mensagens prontas do tópico (a quantidade que ainda falta ao `GET` do cliente): ele faz o registro + commit de invisibilidade e devolve as mensagens ao nó que pediu (seção 12.8 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).

**Corpo: `TakeRequest`**: `maxMessages` (inteiro, o que falta) e `visibilityTimeout` (segundos, o que o cliente pediu).

**Resposta: `200`, `TakeResponse`**: `messages`, uma lista de `Message` (o mesmo DTO da API de cliente, [06a](06a-api-cliente.md), com `receiptToken`). **Lista vazia** quando o estado "tem mensagens prontas" era uma dica desatualizada, **ou quando o nó está em drain** (o pedido é respondido vazio). Um nó em sincronização não entrega: `503` `cluster.node-syncing`.

## Sincronização e recuperação

Todos os streams seguem o contrato da seção 8.2.1 (linha final obrigatória, teto por stream, retomada por `from`). O `{id}` do bloco é o identificador do bloco no anel de hash, que qualquer nó calcula da tabela de membros (seção 7.1).

### `GET /cluster/blocks/{id}/ids`: IDs do bloco

Stream NDJSON com os IDs de mensagens que o nó possui para o bloco, cada um com o nome do tópico, **ordenados por ID**. É o primeiro passo da sincronização distribuída, inclusive na verificação final (seção 7.3). Linha de dados: `BlockIdLine`: `id`, `topic` e, na fila FIFO, `deleteAt` para uma mensagem com exclusão lógica.

### `GET /cluster/blocks/{id}/messages`: mensagens do bloco

Stream NDJSON com as mensagens do bloco que o nó possui na faixa `from`–`to` (IDs, **inclusivos**). É a chamada **balanceada** da sincronização: o nó novo divide a lista unida de IDs em faixas e pede cada faixa a um nó de origem. Linha de dados: `MessageRecord`. Na fila FIFO, a mensagem com exclusão lógica vem **sem `body`** e com `deleteAt`.

### `GET /cluster/blocks/{id}/messages/{messageId}`: uma mensagem

Retorna o conteúdo de **uma** mensagem do bloco. Existe por consistência REST, para análise individual quando há problema com uma mensagem, e como último recurso para uma mensagem que faltou numa faixa. **Resposta: `200`, `MessageRecord`.** Erro: `404` `message.not-found`.

### `GET /cluster/message-loss`: tabela de perdas

Stream NDJSON com a tabela `perdas` do nó: os pares (`topic`, `id`) declarados perdidos na sincronização, **ordenados por ID**. Consultada em **todos** os nós por um nó que retorna (seção 6.6). Aceita chamador que ainda não é membro. Linha de dados: `LossLine`: `id` e `topic`.

### `POST /cluster/messages/recover`: recuperar mensagens

Usado por um nó que **retorna** ao cluster (seção 6.6) para enviar ao responsável pelo bloco as mensagens que encontrou na lista de perdas. O **corpo da requisição** é um stream NDJSON (`Content-Type: application/x-ndjson`) de `MessageRecord`, terminado pela linha `StreamEnd` ([06d](06d-api-convencoes.md), seção 8.3.7). O nó que recebe grava **direto nos nós responsáveis pelo bloco**, idempotente pelo ID, **sem registro pendente nem rollback**; a linha conta como aplicada quando `write.quorum` nós confirmam. A invisibilidade é zerada, e a mensagem volta a ficar pronta. Quem aceita apaga o registro de perda e avisa os demais. Aceita chamador que ainda não é membro.

**Resposta: `200`**, um objeto `RecoverResult`: `end` (`true`) e `count` (quantas linhas foram **aplicadas**). Se a conexão cair, quem envia **reenvia tudo**; mensagens já aplicadas são ignoradas.

## Observabilidade

### `GET /cluster/status`

Estado local do nó, para a agregação de `GET /health/cluster` com `scope=cluster`. **Resposta: `200`, `NodeStatus`** ([06b](06b-api-operacao.md)). As métricas não passam por aqui: cada nó as expõe no próprio `/metrics` e no OTLP.

### `GET /cluster/metrics/topics`

Contadores locais por tópico, **só das mensagens em que o nó é primário**, agregados por quem atende `/metrics/topics` (soma das quantidades, menor data). Parâmetro: `topic` (nome), opcional, para pedir só um tópico.

**Resposta: `200`, `TopicCounters`**: `collectedAt` (data) e `topics`, uma lista de `{ "topic", "visibleMessages", "invisibleMessages", "oldestMessageAt" }`. `oldestMessageAt` fica **ausente**, nunca zero, num tópico sem mensagens neste nó.

## Repasse

### `* /cluster/forward/{caminho}`: repasse de uma chamada de cliente

Repassa uma chamada de cliente a outro nó: de quem está **sincronizando** ou **em drain** (seção 6.9 de [04-cluster.md](04-cluster.md)) e, na fila FIFO, da publicação ao **primário do grupo** (seção 16.4 de [17-topicos-fifo.md](17-topicos-fifo.md)). Aceita **qualquer verbo**.

- `{caminho}` é o caminho original da API de cliente (`/cluster/forward/topics/pedidos/messages`). **Método, query, corpo e cabeçalhos seguem como vieram**, e a resposta volta **como o destino a deu**, inclusive `409`, `413` e `429`.
- Com a autenticação ligada, o destino **confere o token de novo**: o `Authorization` segue como veio, e um nó em sincronização ainda não tem a tabela de tokens ([06a](06a-api-cliente.md), seção 8.1.3).
- O IP do cliente já resolvido segue num cabeçalho interno, e o destino não conta o rate limit de novo (o nó que repassa contou por IP, pois não valida o token; [12-limites-api.md](12-limites-api.md)).
- Aceita só de membros da tabela e de nós que saíram por drain. A própria rota é o marcador de repasse: **quem a recebe nunca repassa de novo**; se estiver sincronizando, responde `503` `cluster.node-syncing`.
- **Falha do destino:** antes de o corpo ser enviado, o nó tenta outro destino, até um teto pequeno de tentativas. **Depois de enviado, não repete** (repetir um `POST` de mensagem a duplicaria, pois o `id` é gerado no coordenador): responde `502` `forward.bad-gateway` ou `504` `forward.timeout`. Na fila FIFO a repetição é segura, porque o primário deduplica.
- Sem nenhum destino disponível: `503` com `Retry-After` (`cluster.node-syncing` ou `cluster.node-draining`).
- Na fila FIFO, o primário recalcula o dono do grupo e responde `421` `fifo.misdirected` se não for ele; o nó repassador tenta de novo, com a tabela atualizada, até `fifo.forward.max-attempts`, e esgotado responde `503` `fifo.primary-unavailable`.

## DTOs da API do cluster

### `OperationRef`

| Campo | Tipo | Descrição |
|---|---|---|
| `operationId` | texto | O GUID v7 da operação, o mesmo do `register`. |

### `Member` e `MemberTable`

`Member`:

| Campo | Tipo | Descrição |
|---|---|---|
| `nodeId` | texto | ID do nó (GUID v7). |
| `addresses` | lista de texto | IPs internos do nó. |
| `port` | inteiro | A porta do cluster. |
| `state` | texto | `active`, `syncing`, `draining` ou `suspect`. |

`MemberTable`:

| Campo | Tipo | Descrição |
|---|---|---|
| `version` | inteiro | Versão (época) da tabela. Versões diferentes entre nós indicam propagação incompleta (`membership-divergent`). |
| `members` | lista de `Member` | Os membros. |

### `TopicDefinition`

A definição do tópico, **igual ao arquivo** `_topics/${topico}.json` ([02-arquitetura.md](02-arquitetura.md), seção 3.3): os campos de `Topic` ([06a](06a-api-cliente.md)) **sem** `effective`, `deadLetter` e `source` (a dead-letter não tem definição própria). Os valores `null` de `maxMessageBytes`, `ttl` e `maxDeliveries` são enviados como `null`, e cada nó aplica o padrão **na hora da conferência**.

### `TokenRecord`

| Campo | Tipo | Descrição |
|---|---|---|
| `id` | texto | GUID v7. |
| `name` | texto | O nome. |
| `permissions` | lista de texto | `publish`, `consume`, `create`, `admin`. |
| `topicPrefixes` | lista de texto | Os prefixos. |
| `createdAt` | data | Criação. |
| `expiresAt` | data ou `null` | Vencimento. |
| `secretHash` | texto | **SHA-256 do segredo**, em hexadecimal. O segredo nunca trafega entre nós. |

### `MessageRecord`

A mensagem **completa**, como os nós a trocam (nos streams e no registro): o conteúdo mais os metadados.

| Campo | Tipo | Presente em | Descrição |
|---|---|---|---|
| `id` | texto | sempre | GUID v7. |
| `topic` | texto | sempre | O tópico. |
| `body` | qualquer JSON | exceto exclusão lógica | O corpo: o **JSON nativo** quando `contentType` é `application/json` ou `*/*+json`; nos demais casos (texto e binário), os bytes em **base64** numa string, com `bodyEncoding: "base64"`. Entre nós não há a forma de string de texto da API de cliente, para o limite de linha ser previsível. **Ausente** na mensagem da fila FIFO com `deleteAt`. |
| `contentType` | texto | exceto exclusão lógica | O `Content-Type` da publicação. |
| `bodyEncoding` | texto | só binário e texto | `base64`, quando o `body` está em base64. |
| `groupId` | texto | se tem grupo | — |
| `deduplicationId` | texto | fila FIFO | — |
| `deliveryCount` | inteiro | sempre | Entregas até agora. |
| `invisibleUntil` | data ou `null` | sempre | Prazo de invisibilidade; `null` ou no passado: pronta. |
| `receiptToken` | texto ou `null` | sempre | Token da última entrega. |
| `deadLetteredAt`, `deadLetterReason` | data, texto | dead-letter | — |
| `deleteAt` | data | fila FIFO, mensagem confirmada | Exclusão lógica: a linha vale só para a deduplicação até essa data. |

### 8.2.1 Rotas de sincronização: streams NDJSON

As rotas entre nós que podem mover volume grande (`/cluster/topics`, `/cluster/tokens`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/messages`, `/cluster/message-loss` e o corpo do `/cluster/messages/recover`) **não usam a paginação do cliente** (seção 8.1.2, [06a-api-cliente.md](06a-api-cliente.md)). Quem sincroniza precisa saber que recebeu **tudo**, e a conexão é sempre direta entre dois nós, sem balanceador no caminho. O contrato é:

- **Formato:** `Content-Type: application/x-ndjson`, **um objeto JSON por linha**. Emissor e receptor trabalham com um buffer por linha, sem montar a resposta inteira em memória. O tamanho máximo de uma linha é limitado (`cluster.sync.stream.max-line-bytes`).
- **Ordem:** por **ID ascendente** (GUID v7, comparado como texto na forma canônica em minúsculas), que acompanha a ordem de criação. Mensagem e definição de tópico não mudam o `id`.
- **Faixa:** `from` e `to` (ambos **inclusivos**, ambos opcionais) delimitam os IDs pedidos. `limit` opcional reduz o teto de itens do stream.
- **Linha final obrigatória:** o stream termina com `StreamEnd` ([06d](06d-api-convencoes.md), seção 8.3.7): `{"end":true,"count":N,"last":"<id>","hasMore":false}`. **Sem essa linha o trecho é considerado falho**, mesmo que a conexão tenha fechado limpa: um corte entre duas linhas não se distingue de um fim normal sem ela.
- **Teto por stream:** o emissor fecha o stream ao atingir `cluster.sync.stream.max-items` ou `cluster.sync.stream.max-bytes`, com `hasMore: true`. O receptor abre outro com `from=<last>`.
- **Retomada:** depois de um corte ou de `hasMore`, o receptor repete a chamada com `from=<último ID lido>`. O emissor devolve de novo essa linha, e o receptor a **ignora** (aplicação idempotente por ID). Não há token nem estado no servidor, e a repetição devolve as mesmas linhas porque a mensagem não muda depois de gravada.
- **Erro depois do `200`:** o status já foi enviado, então o emissor escreve a linha `{"error":"..."}` e fecha, ou apenas fecha sem a linha final. Nos dois casos o receptor retoma pela regra acima. Essa linha não é um `ErrorResponse`.
- **Timeout:** vale um **timeout de leitura por linha** (`cluster.sync.stream.idle-timeout`), não um teto para o stream inteiro. Não há *heartbeat*: sem balanceador, só um nó parado ou lento fica sem escrever.
- **Contrapressão:** o receptor lê no próprio ritmo e o TCP segura o emissor. O emissor não mantém transação nem lista congelada aberta; ele lê o armazenamento em blocos pequenos.
- **Corpo de `POST`** (`/cluster/messages/recover`): o mesmo formato, com a mesma linha final no corpo. A resposta termina com `{"end":true,"count":N}`, com a quantidade aplicada. Se a conexão cair, quem envia reenvia tudo; mensagens já aplicadas são ignoradas.

Cada rota usa o mesmo formato com linhas diferentes:

| Rota | Linha de dados |
|---|---|
| `/cluster/topics` | `TopicDefinition`: a definição do tópico (a mesma de `_topics/${topico}.json`). |
| `/cluster/tokens` | `TokenRecord`: o token (o mesmo de `_tokens/${id}.json`, com o hash do segredo). |
| `/cluster/blocks/{id}/ids` | `{"id":"<id>","topic":"<topico>"}`; na fila FIFO, a linha de uma mensagem com exclusão lógica traz também `"deleteAt"` (seção 16.7 de [17-topicos-fifo.md](17-topicos-fifo.md)). |
| `/cluster/message-loss` | `{"id":"<id>","topic":"<topico>"}` |
| `/cluster/blocks/{id}/messages` | `MessageRecord`: a mensagem completa (`id`, conteúdo e metadados). Na fila FIFO, a mensagem com exclusão lógica vem **sem conteúdo** e com `"deleteAt"`. |

Quando faltar mensagem numa faixa (o `count` é menor que os IDs pedidos), o receptor busca só as que faltaram em outro nó de origem, e a rota por ID único serve de último recurso. Se nenhum nó a tem, vale o evento de perda da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md), registrado no log com o tópico e o ID.

Esse contrato **não cria tombstone nem log novo**: a remoção continua por ausência do ID, e a janela residual da seção 7.3 continua tratada como lá.
