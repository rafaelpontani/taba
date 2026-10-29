---
título: API do cluster
resumo: As rotas que os nós usam entre si — membros e canal, definições, mensagens, sincronização, recuperação, observabilidade e repasse — com o protocolo de duas fases, os streams NDJSON e os DTOs.
tipo: referência
status: estável
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/src/cluster/protocol/backup.rs, crates/taba-node/src/cluster/protocol/http.rs, crates/taba-node/src/cluster/protocol, crates/taba-node/src/http/cluster_api, crates/taba-node/src/cluster/service, crates/taba-node/src/cluster/channel]
---

# API do cluster

> ⚠️ **Interna.** Nenhuma aplicação cliente deve chamar estas rotas, e **não há garantia de estabilidade** entre versões: todos os nós de um cluster devem rodar a mesma versão. Convenções comuns: [15-api-visao-geral.md](15-api-visao-geral.md). Como o cluster usa estas rotas: [07-cluster.md](07-cluster.md), [10-publicacao.md](10-publicacao.md), [12-entrega.md](12-entrega.md).

**Porta:** `cluster.port` (8081), **só** os outros nós. **Autenticação:** TLS mútuo ([14-seguranca.md](14-seguranca.md)); uma conexão sem certificado aceito falha **no handshake**.

O protocolo existe em duas formas que são **a mesma coisa**: o tipo `Request` (`cluster/protocol`) e a rota HTTP. O `Transport` real as traduz para HTTP; a rede em memória dos testes entrega o tipo direto (e, nos testes que pedem, passa cada chamada pela forma HTTP para provar o formato).

**Formatos.** As operações sobre mensagens trafegam em **protobuf** (`Content-Type: application/x-protobuf`, o esquema em [`crates/taba-node/proto/taba-cluster.proto`](https://github.com/rafaelpontani/taba/blob/main/crates/taba-node/proto/taba-cluster.proto)); as demais rotas `/cluster/*` ainda são JSON (e NDJSON nas listas de sincronização) até passarem a protobuf também. Um corpo de outro tipo numa rota que é protobuf: `415 media.unsupported`.

## Regras comuns

**Quem pode chamar.** Quem consta na **tabela de membros** do nó. Quem não consta: `403 cluster.not-member`. O chamador é reconhecido pelo **IP de origem** da conexão (todos usam o mesmo certificado), que tem de ser um `address` de algum membro. Um nó que ainda não tem a tabela responde `503 cluster.node-syncing`. **Aceitam chamador que ainda não é membro:** `GET /cluster/nodes`, e as de perdas (`message-loss`, `messages/recover`), porque quem volta de uma exclusão já não consta.

**Protocolo de duas fases.** As rotas `register`, `commit` e `rollback`:

- Cada operação tem um **`operationId`** (GUID v7 do coordenador). As três fases levam o mesmo, e **todas são idempotentes por ele**: repetir devolve o mesmo `Ack`, sem efeito novo.
- `rollback` só descarta um registro **pendente, sem commit**. Falha de `commit` numa réplica fora do quórum **nunca** aciona `rollback`: o coordenador insiste até `write.commit.timeout`.
- **Mensagens** decidem o commit por **quórum** (primário incluso); **nós** exigem **todos**. **Tópico e token** não usam estas fases: têm **dono** no anel (rotas `definitions`).
- Um registro sem commit nem rollback **expira** em `write.pending.ttl`. `commit` ou `rollback` de uma operação desconhecida: `404 cluster.operation-unknown`.
- Um nó **sincronizando** confirma o `register` **gravando-o no banco de eventos** e o aplica depois; para o coordenador, a confirmação conta normalmente.

**Status comuns:**

| Status | Código | Quando |
|---|---|---|
| `200` | — | Aceito ou consulta atendida |
| `400` | `validation.failed`, `required`, `unknown-field` | Corpo inválido |
| `403` | `cluster.not-member` | Chamador não é membro |
| `404` | `cluster.operation-unknown`, `topic.not-found`, `message.not-found` | Desconhecido |
| `409` | `cluster.conflict` | Outra operação pendente no mesmo recurso, ou o estado impede (mensagem já invisível, `receiptToken` diferente) |
| `421` | `fifo.misdirected` | Só na publicação FIFO repassada: o nó não é mais o primário do grupo |
| `421` | `cluster.wrong-node` | Só no canal: o nó neste endereço não é o que o `to` procura |
| `503` | `cluster.node-syncing`, `cluster.node-draining` | O nó não pode atender agora |

## Mapa das rotas

Extraído de `cluster/protocol/http.rs`, o tradutor entre `Request` e HTTP.

| Grupo | Rotas |
|---|---|
| **Membros e canal** | `GET /cluster/nodes`; `POST /cluster/nodes/join`, `register`, `unregister`, `commit`, `rollback`; `POST /cluster/vote-remove`; `GET /cluster/channel` (WebSocket) |
| **Definições** | `POST /cluster/definitions/propose`, `apply`; `GET /cluster/topics`, `GET /cluster/tokens`, `GET /cluster/definitions/tombstones`; `GET /cluster/topics/{id}/idle` |
| **Mensagens** | `POST /cluster/batch` (o `register`, `commit` e `rollback` das escritas, invisibilidades e acks, protobuf); `POST /cluster/topics/{topic}/messages/take`; `POST /cluster/topics/{topic}/fifo/publish`; `POST /cluster/topics/{topic}/prune` |
| **Sincronização e recuperação** | `GET /cluster/blocks/{id}/ids`, `GET /cluster/blocks/{id}/excluded`, `GET /cluster/blocks/{id}/messages`, `GET /cluster/blocks/{id}/messages/{messageId}`; `GET`/`POST /cluster/message-loss`; `DELETE /cluster/message-loss/{topic}/{id}`; `POST /cluster/messages/recover`; `POST /cluster/topics/{topic}/messages/restore`; `POST /cluster/topics/{topic}/messages/dead`; `GET /cluster/restore`; `POST /cluster/restore/decided`; `POST /cluster/restore/force` |
| **Observabilidade** | `GET /cluster/status`; `GET /cluster/stats/topics`; `GET /cluster/stats/topics/{topic}/groups` |
| **Backup em S3** | `POST /cluster/backup/flush`, `POST /cluster/backup/stop`, `POST /cluster/backup/saved`; `GET /cluster/backup/status`, `GET /cluster/departed` |
| **Repasse** | `* /cluster/forward/{caminho}`; `* /cluster/node-call/{caminho}` |

> A doc histórica também lista `GET /cluster/metrics/topics`; **essa rota não existe no código** (a agregação entre nós das métricas de tópico não foi implementada: [13-observabilidade.md](13-observabilidade.md)).

---

## Membros e canal

| Rota | Faz |
|---|---|
| `GET /cluster/nodes` | A **primeira chamada** de um nó ao iniciar, pelo balanceador (`cluster.entry-address`). Devolve a `MemberTable`. Qualquer nó a responde, mesmo de quem não é membro. **A única que passa por balanceador** |
| `POST /cluster/nodes/join` | O nó novo pede ingresso a **um membro**, que testa os IPs (conecta de volta), coordena o registro + commit com **todos** e só então responde com a `MemberTable`. **Pode demorar**: espera a vez se há outra mudança ou votação |
| `POST /cluster/nodes/register` / `unregister` | **Fase 1** do ingresso e do **desregistro** (`reason`: `vote` ou `drain`) |
| `POST /cluster/nodes/commit` | **Fase 2** (efetiva o registro **ou** o desregistro) |
| `POST /cluster/nodes/rollback` | Aborta a fase 1 pendente |
| `POST /cluster/vote-remove` | **Voto de exclusão**: corpo `VoteRequest` (`voteId`, `candidateNodeId`, `openedBy`, `reason`: `channel-down`, `queue-overflow` ou `node-failed`); resposta `VoteResponse` (`vote`: `yes`/`no`) |

`join`: corpo `JoinRequest` (`nodeId`, `addresses`, `port`, `version`). Erros: `403 cluster.node-frozen` (com `Retry-After`), `422 cluster.join-unreachable` (o membro não conseguiu conectar de volta: o nó é excluído e repete o bootstrap), `503 cluster.registration-failed` (fase 1 não confirmada por todos; rollback feito), `503 cluster.commit-timeout` (registro confirmado e commit em segundo plano: o nó **foi aceito**; consulte `GET /cluster/nodes` antes de repetir).

### O canal: `GET /cluster/channel` (WebSocket)

Abre o **canal permanente** entre dois nós (`101 Switching Protocols`), **um por par**, usado nos dois sentidos, para todos os tópicos. A query leva `from` (quem abre) e `to` (quem deve atender): um nó de **outro ID** no mesmo endereço responde `421 cluster.wrong-node`. **Só sinais, nunca conteúdo de mensagem.**

Cada mensagem de texto é um JSON de um de dois tipos:

| Tipo | Campos | O que é |
|---|---|---|
| **lote** | `from`, `to`, `keepalive?`, `frames` | O que um nó manda: o sinal de vida (quando é hora) e os quadros sem confirmação |
| **`confirm`** | `seqs` | A resposta a **cada** lote, depois de **processar** os quadros (vazia se só trouxe o sinal de vida) |

**Sinal de vida (`keepalive`):** `at`, `load` (chamadas repassadas em andamento), `version` (da tabela de membros), `state` (do nó), `monitoring` (**ainda não enviado**) e `ready` (lista de `{topic, count}`, até `cluster.channel.keepalive.max-topics`). Não leva `seq` e não é reenviado.

**Quadros (`frames`)**, cada um com `seq` (crescente por canal e por sentido) e `type`:

| `type` | Campos | Significa |
|---|---|---|
| `listeners` | `topic`, `hasListeners` | O tópico ganhou ou perdeu ouvintes de long polling neste nó |
| `ready` | `topic`, `hasReady` | O nó passou a ter (ou deixou de ter) mensagens prontas do tópico |
| `notify` | `topic`, `count` | Aviso de mensagem nova (dica): venha buscar com `take` |
| `node-state` | `state` | `draining`, `active` ou `failed`. Quem recebe `active` marca o membro como `active` na própria tabela, sem subir a `version` |
| `frozen` | `nodeId`, `frozenAt` | Um nó excluído por votação está em freezing. Não altera a tabela nem a `version` |
| `membership` | `operationId`, `version`, `change` | Uma mudança de membros **já efetivada** (`add`, `remove` por drain, `exclude` por votação), com a versão que a tabela passou a ter. Quem perdeu o registro ou o commit a aplica |
| `definition` | `kind`, `key`, `version`, `deleted`, `body` | O estado de uma definição (tópico ou token), com a mesma regra do `apply` |

A conexão é **dada por caída** quando fecha ou quando nada chega do par por `2 × cluster.channel.keepalive.interval`. Ao abrir, cada lado manda um **instantâneo** (`listeners`, `ready`, `frozen`) e só depois as mudanças.

---

## Definições (tópicos e tokens)

Cada definição tem um **dono** no anel (o primeiro nó ativo na posição de `topic:<nome>` ou `token:<id>`) e um **herdeiro** (o primeiro outro dono ativo que responde).

| Rota | Faz |
|---|---|
| `POST /cluster/definitions/propose` | Do nó que recebeu o cliente **ao dono**. Corpo `DefinitionProposal`: `kind` (`topic`/`token`), `operation` (`create`/`update`/`delete`), `key`, `body`, `expectedId?` (usado pelo gestor de fila volátil para só apagar o `id` esperado) |
| `POST /cluster/definitions/apply` | Do dono a cada nó e ao herdeiro. `DefinitionApply`: `kind`, `key`, `version`, `deleted`, `body`, `relay?` |
| `GET /cluster/topics`, `GET /cluster/tokens` | **Streams** das definições, ordenadas por `id`, para o bootstrap |
| `GET /cluster/definitions/tombstones` | **Stream** das lápides (definições removidas, a versão da remoção e, para um tópico, o `topicId` dele), para uma criação atrasada não ressuscitar o que foi removido |
| `GET /cluster/topics/{id}/idle` | Há quanto tempo foi o último `GET` do tópico **neste nó** (`IdleResponse`: `topicId`, `idleSeconds`, `sinceStart`). Usado pelo gestor da fila volátil |

- O dono confere (nome já existe, tópico desconhecido, `expectedId`), dá a **próxima versão**, grava no **herdeiro**, aplica em si, manda `apply` aos demais e responde depois dos que respondem em `write.commit.timeout`. Os outros recebem pelo quadro `definition` do canal.
- **Versão:** o nó só aplica versão **maior ou igual** à que tem. Na **mesma versão com conteúdo diferente**, vale a cópia de **menor impressão** (SHA-256 do corpo normalizado), em qualquer nó e ordem (D124).
- **Token:** o `TokenRecord` leva o **sal** e o **hash salgado**, **nunca o segredo**.
- Erros: `404` (`topic.not-found`, `token.not-found`), `409` (`topic.already-exists`, `cluster.conflict`), `503 cluster.quorum-unavailable` (dono fora do ar e na tabela, ou nenhum outro dono confirmou).

## Mensagens

### `POST /cluster/batch`: as operações sobre mensagens

As operações do protocolo de duas fases sobre mensagens (publicar e mover para a dead-letter, invisibilidade, ack) vão em **uma só rota**, em **lotes**: o corpo é um `BatchRequest` (`repeated Item items`) e a resposta um `BatchReply` (`repeated ItemResult results`) com **um resultado por item, na mesma ordem**. Um item diz o seu `kind` (`write`, `visibility`, `ack`), a sua `phase` (`register`, `commit`, `rollback`), o `topic`, o `id` da mensagem e o `operationId`; o `register` leva ainda o corpo do seu tipo. Um resultado tem o `status` que o item teria como chamada própria, o `operationId`, `accepted` e, se falhou, o erro com o mesmo `code` e `message` do [doc 19](19-erros-e-limites.md): **o erro de um item não toca os outros**, e um item de tipo que o nó não conhece é erro desse item.

Quando o lote fecha: cada nó tem uma fila de operações e 4 vagas de envio; quando há uma operação esperando **e** uma vaga livre, o lote fecha naquele instante com o que estiver na fila (até `cluster.batch.max-items` e `cluster.batch.max-bytes`, [doc 22](22-configuracao.md)). Não há temporizador: com pouca carga cada operação vai sozinha, com muita a fila cresce enquanto as vagas estão ocupadas e o lote seguinte sai cheio. Quem envia confere que o `operationId` de cada resultado é o do seu item, e uma resposta que não confere conta como falha da chamada inteira.

Erros da chamada inteira: `415 media.unsupported` (não é protobuf), `400 validation.failed` (um corpo que não é um lote), `413 message.too-large` (mais que `cluster.batch.max-items` itens, ou corpo maior que `cluster.batch.max-bytes` mais uma mensagem) e os comuns (`403 cluster.not-member`, `503 cluster.node-syncing`). O corpo de cada mensagem segue como os bytes com que chegou (sem base64, sem escape).

Os campos de cada tipo, como antes:

| Tipo | `register` (corpo) |
|---|---|
| `write` | `MessageRegisterRequest`: `operationId`, `coordinatorId`, `kind` (`publish` ou `dead-letter`), `message` (`MessageRecord`), `sourceTopic` (só `dead-letter`). O **mesmo tipo** faz o movimento para a dead-letter (cria lá e remove da origem, com o mesmo `id`) |
| `visibility` | `VisibilityRegisterRequest`: `kind` (`delivery`/`change`), `invisibleUntil` (**absoluto**), `receiptToken`, `deliveryCount` (só `delivery`). O coordenador é sempre o **dono** |
| `ack` | `AckRegisterRequest`: `receiptToken` (deve conferir), `deleteAt` (só FIFO) e `discard` (o primário do grupo descartando uma duplicata na liberação da restauração: não há recibo a conferir) |

`commit` e `rollback` de cada um só dizem o `operationId` e respondem `accepted`. Erros: `404 message.not-found`, `409 cluster.conflict` (já há registro pendente, ou a mensagem já está invisível), `409 message.stale-receipt`.

| Rota | Faz |
|---|---|
| `POST /cluster/topics/{topic}/messages/take` | Pede ao **dono** que entregue até `maxMessages` mensagens prontas (`TakeRequest`: `maxMessages`, `visibilityTimeout`). Ele faz o registro + commit de invisibilidade e devolve `TakeResponse` (`messages`: lista de `MessageRecord`). **Vazia** se a dica estava velha, ou se o nó está em **drain** |
| `POST /cluster/topics/{topic}/prune` | Esvazia a fila **neste nó** (corpo `{ "pruneId" }`, um GUID v7 que torna a chamada idempotente: o mesmo id responde a mesma contagem e não remove nada novo; o nó lembra os últimos 64). Responde `{ "removed" }`: quantas mensagens ele **era dono** e removeu (as réplicas também são removidas, mas não contadas). A distribuição do `DELETE /topics/{topic}/messages` ([16-api-cliente.md](16-api-cliente.md)). Erros: `404 topic.not-found`, `409 topic.wrong-type` |
| `POST /cluster/topics/{topic}/fifo/publish` | Repasse da publicação FIFO ao **primário do grupo** (corpo `MessageRecord`, com `id` marcador `00000000-0000-7000-8000-000000000000`; quem gera o `id` é o primário). `421 fifo.misdirected` se o nó não é o primário. Usado também pelas cópias de um distribuidor para uma fila FIFO |

## Sincronização e recuperação

Todos os streams seguem o contrato [NDJSON](#streams-ndjson).

| Rota | Faz |
|---|---|
| `GET /cluster/blocks/{id}/ids` | IDs de mensagens que o nó tem para o bloco, com o tópico, ordenados por ID (`BlockIdLine`: `id`, `topic`, e na FIFO `deleteAt`). **Primeiro passo** da sincronização |
| `GET /cluster/blocks/{id}/excluded?from=&to=` | O **log de excluídos** do nó para o bloco ([07-cluster.md](07-cluster.md), seção 10): uma linha por `id` que saiu do tópico morto (`id`, `topic`, `position`, `atMs`), ordenada por ID, com os IDs **inclusivos**. Quem sincroniza o bloco copia o log junto com as mensagens |
| `GET /cluster/blocks/{id}/messages?from=&to=` | As mensagens do bloco na faixa (IDs **inclusivos**): `MessageRecord`. É a chamada **balanceada** entre origens. Na FIFO, a mensagem com exclusão lógica vem **sem corpo** e com `deleteAt` |
| `GET /cluster/blocks/{id}/messages/{messageId}` | **Uma** mensagem; último recurso para uma que faltou numa faixa |
| `GET /cluster/message-loss` | A tabela `losses` do nó (`LossLine`: `id`, `topic`); aceita quem não é membro |
| `POST /cluster/message-loss` | Registra uma perda (`LossRecord`: `topic`, `id`, `block`, `detectedAt`); resposta `LossAck` (`changed`) |
| `DELETE /cluster/message-loss/{topic}/{id}` | Dá **baixa** numa perda |
| `POST /cluster/messages/recover[?replicate=false][&durable=true]` | Um nó que **volta** envia ao responsável as mensagens que achou na lista de perdas. **Corpo é um stream NDJSON** (`Content-Type: application/x-ndjson`) de `MessageRecord` com a linha final; resposta `RecoverResult` (`end`, `count` aplicadas). Com `replicate=false` só grava no disco local; com `durable=true` quem grava põe a mensagem **em disco**, e não só na memória (a restauração das vidas anteriores usa isso entre os donos). `415` com outro `Content-Type` |
| `POST /cluster/topics/{topic}/messages/restore[?topicId=]` | O nó que guarda a **pasta de uma vida anterior** envia ao **novo primário** do bloco as mensagens dela, com o ID original ([07-cluster.md](07-cluster.md), seção 10.6). Corpo NDJSON como o de `recover`; `topicId` é o do tópico quando a pasta foi criada. O primário confere o tópico (excluído, ou recriado com outro `topicId`, descarta), o log de excluídos dele, o dos outros nós presentes e o diário do S3, e ativa o que está vivo pelo caminho de `recover` (disco e quórum). Resposta `RecoverResult`: `count` é o que ficou **resolvido** (ativado, já existente ou descartado). Só membros |
| `POST /cluster/topics/{topic}/messages/dead` | Corpo `{"ids":[...]}`; responde `{"dead":[...]}` com os IDs que constam no **log de excluídos** deste nó. É a consulta, em lote, que o primário que restaura faz aos nós presentes. Só membros |
| `GET /cluster/restore` | O estado da restauração das vidas anteriores do nó ([07-cluster.md](07-cluster.md), seção 10.8): `status` (`unknown`, `restoring` ou `restored`), `finished` (enviou tudo o que as vidas dele guardam), `forced`, `missing` (os nós antigos ainda esperados) e `lives` (os manifestos das vidas que ele guarda, com a marca d'água). Os nós o consultam a cada 500 ms enquanto restauram. Só membros |
| `POST /cluster/restore/decided` | O coordenador (o de menor ID) avisa cada nó de que a votação fechou: o nó deduplica as filas FIFO, as libera e marca o cluster **restaurado do disco**. Idempotente. Só membros |
| `POST /cluster/restore/force` | O forçar do operador, espalhado pelo nó que o recebeu ([17-api-operacao.md](17-api-operacao.md)): o nó decide na hora, seja qual for a votação. Só membros |

`{id}` do bloco malformado: `400 validation.failed`.

## Backup em S3

Só existem com o backup configurado ([33-backup-s3.md](33-backup-s3.md)); sem ele respondem `422 backup.not-configured`. Aceitam só **membros**. São as chamadas por trás de `POST /cluster/backup/flush` e `POST /cluster/stop` da API de operação ([17-api-operacao.md](17-api-operacao.md)).

| Rota | Faz |
|---|---|
| `POST /cluster/backup/flush` | O flush do backup **neste** nó (as três etapas); responde o `FlushReport` em JSON |
| `POST /cluster/backup/stop` | O drain **deste** nó para a parada do cluster, com o envio ao S3; responde quando o nó concluiu: `{ "nodeId", "drained", "outboxPending" }` |
| `POST /cluster/backup/saved` | O primário avisa a réplica, em lote, de quais mensagens do tópico já estão no bucket: `{ "topic", "items": [{ "id", "segment", "offset", "len" }] }` (até 5000 itens). A réplica guarda a referência do que ela tem e ignora o resto; responde `Ack` |

## Observabilidade e repasse

| Rota | Faz |
|---|---|
| `GET /cluster/status` | O `NodeStatus` local ([17-api-operacao.md](17-api-operacao.md)), para o `GET /health/cluster?scope=cluster` agregar |
| `GET /cluster/stats/topics` | O que **este nó** vê de cada tópico: os números (contagens, taxas, profundidade, mensagem mais antiga, percentis de latência, memória e disco, grupos acompanhados) e a memória do nó em uso, não a apresentação. O `/stats/topics` da API de cliente pergunta a todos os membros e os junta ([16-api-cliente.md](16-api-cliente.md)). Responde `{ "nodeId", "memoryTotalBytes", "topics": [{ "name", "kind": "queue" \| "distributor", … }] }` |
| `GET /cluster/stats/topics/{topic}/groups` | Os grupos de uma fila como este nó os vê (`groupId`, `overflow`, `visible`, `invisible`, `band`, `oldestMs`). `404 topic.not-found`, `409 topic.wrong-type` |
| `GET /cluster/backup/status` | O estado do backup em S3 **deste** nó (o mesmo corpo do `GET /node/backup`). `422 backup.not-configured` |
| `GET /cluster/departed` | Os nÃ³s que saÃ­ram por drain, como **este** nÃ³ os lembra: uma lista de `{ nodeId, addresses, port }` (sÃ³ em memÃ³ria, atÃ© 64). O `GET /nodes` une as listas de todos os membros, entÃ£o a resposta nÃ£o depende de qual nÃ³ recebeu a chamada |
| `* /cluster/node-call/{caminho}` | Uma chamada da **API de operação** (`/metrics`, `/metrics/topics…`, `/node/drain`, `/node/backup`, `/node/backup/flush`) que o nó que recebe executa contra **si mesmo**, para uma chamada de cliente endereçada a ele (`/nodes/{nodeId}/…`, [16-api-cliente.md](16-api-cliente.md)). Aceita qualquer verbo; a resposta é a que a rota deu, com status e corpo como vieram. O token já foi conferido por quem enviou, e a chamada só vai às rotas de operação, nunca às de cliente. Funciona mesmo com o nó em drain (é assim que um nó drenado é devolvido). Aceita só de membros |
| `* /cluster/forward/{caminho}` | **Repasse** de uma chamada de cliente de um nó sincronizando, em drain ou em falha. Aceita **qualquer verbo**; método, query, corpo e cabeçalhos seguem como vieram e a resposta volta como o destino a deu. O IP do cliente já resolvido vai no cabeçalho `x-taba-client-ip`; o destino **confere o token de novo** e **não conta o rate limit**. Prazo de 60 s (cabe um long polling). **Quem recebe nunca repassa de novo**. Aceita só de membros e de nós que saíram por drain |

## Streams NDJSON

As rotas que movem volume (`/cluster/topics`, `/cluster/tokens`, `/cluster/definitions/tombstones`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/excluded`, `/cluster/blocks/{id}/messages`, `/cluster/message-loss` e o corpo de `/cluster/messages/recover`) **não usam a paginação do cliente**: quem sincroniza precisa saber que recebeu **tudo**.

| Regra | Efeito |
|---|---|
| **Formato** | `application/x-ndjson`, **um JSON por linha**; um buffer por linha, sem montar a resposta inteira. Tamanho máximo da linha limitado |
| **Ordem** | **ID ascendente** (GUID v7 como texto minúsculo) |
| **Faixa** | `from` e `to` (inclusivos, opcionais); `limit` opcional reduz o teto |
| **Linha final obrigatória** | `{"end":true,"count":N,"last":"<id>","hasMore":false}`. **Sem ela, o trecho é considerado falho** (um corte entre linhas não se distingue de um fim normal) |
| **Teto por stream** | O emissor fecha em `cluster.sync.stream.max-items` (padrão 500) ou `max-bytes` (padrão 4 MiB), com `hasMore: true`; o receptor abre outro com `from=<last>` |
| **Retomada** | Depois de corte ou `hasMore`, repete com `from=<último ID lido>`; a linha repetida é ignorada (idempotente por ID). Sem estado no servidor |
| **Erro depois do `200`** | O emissor escreve `{"error":"..."}` e fecha, ou apenas fecha: o receptor retoma pela regra acima. Não é `ErrorResponse` |
| **Contrapressão** | O receptor lê no próprio ritmo; o TCP segura o emissor |

A linha de dados: `TopicDefinition` (tópicos), `TokenRecord` (tokens), `{id, kind, key, version}` (lápides), `{id, topic}` (IDs e perdas) e `MessageRecord` (mensagens).

## DTOs principais

**`Member`:** `nodeId`, `addresses` (IPs internos), `port` (do cluster), `state` (`active`, `syncing`, `draining`, `suspect`). **`MemberTable`:** `version` (época; versões diferentes = `membership-divergent`), `members` e, só se há, `frozen` (lista de `{nodeId, frozenAt}`; não conta na igualdade nem sobe a `version`).

**`MessageRecord`** (a mensagem como os nós a trocam): `id`, `topic`, `body` (JSON nativo se `application/json`/`*/*+json`; senão **base64**), `contentType`, `bodyEncoding` (`base64`), `groupId`, `deduplicationId`, `deliveryCount`, `invisibleUntil`, `receiptToken`, `deadLetteredAt`, `deadLetterReason`, `deleteAt`. **Não há** a forma "string de texto" da API de cliente, para o limite de linha dos streams ser previsível.

**`Ack`:** `operationId`, `accepted`. **`StreamEnd`:** `end`, `count`, `last`, `hasMore`. **`OperationRef`:** `operationId`.

## Fontes e provas

- `cluster/protocol/http.rs` (a tabela de rotas é desta fonte), `cluster/protocol/{definitions,messages,recovery,signals,vote}.rs` (DTOs), `http/cluster_api`, `cluster/service` (atende as chamadas), `cluster/channel`, `cluster/members`, `cluster/replication`, `cluster/sync`, `cluster/recovery`.
- Casos de uso: UC-CLR (contrato das rotas), UC-STR (streams), UC-CAN (canal), UC-ING, UC-FAL, UC-SIN, UC-RTN, UC-FWD; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/cluster_api.rs`, `sync.rs`, `channel.rs`, `notify.rs`, `mtls.rs`.
- Histórico: `06c-api-cluster.md`.
