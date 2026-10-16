---
título: API de cliente
resumo: Manual rota a rota da API que as aplicações usam — tópicos, mensagens (publicar, consumir, invisibilidade, ack, prune), tokens, quem é o token, os nós e as estatísticas, e as operações de cluster e de nó para o token raiz — com parâmetros, DTOs, status e exemplos.
tipo: referência
status: estável
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/src/http/topics, crates/taba-node/src/http/messages, crates/taba-node/src/http/tokens, crates/taba-node/src/http/auth, crates/taba-node/src/http/nodes, crates/taba-node/src/http/topic_stats, crates/taba-node/src/core/topic, crates/taba-node/src/core/validation, crates/taba-node/src/core/content]
---

# API de cliente

> Convenções comuns (formato, cabeçalhos, erro, paginação, status): [15-api-visao-geral.md](15-api-visao-geral.md). Catálogo de erros e limites: [19-erros-e-limites.md](19-erros-e-limites.md). O comportamento por trás de cada rota está em [09](09-topicos.md), [10](10-publicacao.md) e [12](12-entrega.md).

**Porta:** `api.port` (8080). **Autenticação:** token opcional ([14-seguranca.md](14-seguranca.md)). A coluna "Permissão" só vale com a autenticação ligada.

| Verbo | Rota | Permissão | Rate limit | Sucesso |
|---|---|---|---|---|
| `POST` | `/topics` | `create` | admin | `201` `Topic` |
| `GET` | `/topics` | token válido | admin | `200` `Page<Topic>` |
| `GET` | `/topics/{topic}` | token válido | admin | `200` `Topic` |
| `PATCH` | `/topics/{topic}` | `admin` | admin | `200` `Topic` |
| `DELETE` | `/topics/{topic}` | `admin` | admin | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | publicação | `201` / `200` |
| `GET` | `/topics/{topic}/messages` | `consume` | consumo | `200` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | consumo | `200` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | consumo | `204` |
| `DELETE` | `/topics/{topic}/messages` | `admin` | admin | `200` `PruneResult` |
| `POST` `GET` `PATCH` `DELETE` | `/tokens`, `/tokens/{id}` | só o raiz | admin | ver abaixo |
| `GET` | `/auth/me` | token válido | isento | `200` `Me` |
| `GET` | `/nodes` | token válido | ops | `200` `Page<Node>` |
| `GET` | `/stats/topics`, `/stats/topics/{topic}`, `/stats/topics/{topic}/groups` | token válido, **dentro dos seus prefixos** | ops | `200` |
| `GET` | `/cluster/status` | só o raiz | ops | `200` `ClusterStatus` |
| `GET` | `/nodes/{nodeId}/metrics`, `/nodes/{nodeId}/backup` | só o raiz | ops | `200` |
| `GET` | `/cluster/backup` | só o raiz | ops | `200` `ClusterBackup` |
| `GET` `POST` `DELETE` | `/nodes/{nodeId}/drain` | só o raiz | admin | `200` / `202` `DrainStatus` |
| `POST` | `/nodes/{nodeId}/backup/flush`, `/cluster/backup/flush`, `/cluster/stop` | só o raiz | admin | `200` |

Os exemplos usam `localhost:8080` sem autenticação (modo desenvolvimento). Com autenticação, acrescente `-H "Authorization: Bearer $TOKEN"`.

---

## Tópicos

### Regras de nome, tags e faixas

- **Nome:** `^[A-Za-z0-9_-]{1,100}$`, sensível a maiúsculas. Nome terminado em `-dead-letter` é **reservado** (`400 validation.reserved-name`); nome inválido, `400 validation.invalid-name`.
- **Tags:** até **20** por tópico; chave de 1 a 64 e valor de 0 a 128 caracteres `A-Za-z0-9_-.:`. Só para telemetria; não afetam o comportamento.

| Campo | Faixa |
|---|---|
| `idleTimeout` | 60 a 604 800 s (7 dias) |
| `ttl` | 60 a 31 536 000 s (365 dias) |
| `maxDeliveries` | 1 a 1000 |
| `maxMessageBytes` | 1 até `message.hard-limit` |
| `prefixes` | até 20 prefixos de 1 a 100 caracteres (`A-Za-z0-9_-`); vazio é recusado |

### `POST /topics`

Cria um tópico. Gera o `id`, propaga a definição a **todos os nós** (dono da definição, [08-armazenamento.md](08-armazenamento.md)) e **só responde depois**. Toda **fila** ganha junto sua `{nome}-dead-letter`.

**Corpo `TopicCreateRequest`**

| Campo | Tipo | Obrig. | Padrão | Descrição |
|---|---|---|---|---|
| `name` | texto | sim | — | Nome |
| `type` | texto | sim | — | `queue` ou `distributor` |
| `tags` | mapa texto→texto | não | `{}` | Tags de telemetria |
| `prefixes` | lista | só `distributor` | `[]` | Prefixos de destino. Vazia = o distribuidor não copia para ninguém |
| `volatile` | booleano | só `queue` | `false` | Fila volátil |
| `idleTimeout` | inteiro (s) | só volátil | `600` | Tempo sem consumidor até a exclusão |
| `fifo` | booleano | só `queue` | `false` | Fila FIFO. Imutável. Exige `replication.factor` ≥ 2 |
| `maxMessageBytes` | inteiro | só `queue` | `null` (`message.max-bytes`, 5 MB) | Máximo por mensagem; pode passar do geral, até `message.hard-limit` |
| `ttl` | inteiro (s) | só `queue` | `null` (14 dias) | Vida da mensagem |
| `maxDeliveries` | inteiro | só `queue` | `null` (10) | Entregas sem ack até a dead-letter |

Campo que não vale para o tipo (`prefixes` numa fila, `fifo` num distribuidor): `400 validation.not-applicable`.

**Resposta: `201`** com `Location: /topics/{name}` e o `Topic`:

```bash
curl -s -X POST localhost:8080/topics -H 'Content-Type: application/json' \
  -d '{"name":"pedidos","type":"queue"}'
```
```json
{"createdAt":"2026-10-13T17:00:48.527Z","effective":{"maxDeliveries":10,"maxMessageBytes":5242880,"ttl":1209600},
 "fifo":false,"id":"01a13635-a80f-7985-9ffa-8d0be5eda84f","maxDeliveries":null,"maxMessageBytes":null,
 "name":"pedidos","tags":{},"ttl":null,"type":"queue","volatile":false}
```

| Status | Códigos | Quando |
|---|---|---|
| `400` | `validation.failed`, `invalid-name`, `reserved-name`, `not-applicable`, `out-of-range`, `max-bytes-above-hard-limit`, `unknown-field` | Corpo inválido |
| `401`/`403` | `auth.unauthorized`, `auth.forbidden`, `auth.out-of-scope` | Sem `create`, ou nome/`prefixes` fora dos prefixos do token |
| `409` | `topic.already-exists` | Nome já existe |
| `422` | `topic.fifo-needs-replication` | FIFO com fator < 2 |
| `429` | `rate-limit.exceeded` | — |
| `503` | `cluster.registration-failed`, `cluster.commit-timeout`, `cluster.node-syncing` | Algum nó não confirmou (nada foi criado), ou o commit se completa em segundo plano |

### `GET /topics`

Lista **paginada**, respondida **localmente** (a definição é replicada em todo nó). Com autenticação, só os tópicos dentro dos prefixos do token. **Inclui as filas de dead-letter.**

| Parâmetro | Padrão | Descrição |
|---|---|---|
| `limit`, `cursor` | 100 / — | Paginação |
| `sort` | `name` | `name`, `id` ou `createdAt`; `-` para decrescente |
| `name` | — | Tópicos cujo nome **contém** o texto (sensível a maiúsculas) |

```bash
curl -s "localhost:8080/topics?name=pedidos&sort=-createdAt&limit=2"
# {"items":[{"id":"…","name":"pedidos","type":"queue",…},
#           {"id":"…","name":"pedidos-dead-letter","deadLetter":true,"source":"pedidos",…}],"nextCursor":"eyJr…"}
```

Erros: `400` (`out-of-range`, `invalid-cursor`, `failed`), `401`, `429`.

### `GET /topics/{topic}`

Devolve o `Topic`. Na dead-letter, os valores são os da fila de origem, somente leitura, com `deadLetter: true` e `source`. Erros: `401`/`403 auth.out-of-scope`, `404 topic.not-found`, `429`.

### `PATCH /topics/{topic}`

**Atualização parcial**: só o que foi enviado muda. `tags` **substitui o mapa inteiro**. `null` em `maxMessageBytes`, `ttl` ou `maxDeliveries` **volta ao padrão do nó**. Vale também para mensagens já guardadas (o `ttl` novo vale na próxima varredura); `maxMessageBytes` só para publicações seguintes.

| Campo | Aplica-se a |
|---|---|
| `tags` | todos |
| `prefixes` | `distributor` (vale a partir do commit, só para publicações posteriores) |
| `idleTimeout` | fila volátil (vale na próxima verificação do gestor) |
| `maxMessageBytes`, `ttl`, `maxDeliveries` | fila |

**Imutáveis:** `name`, `id`, `type`, `volatile`, `fifo`: enviá-los, **mesmo com o valor atual**, é `400 validation.immutable-field`. Erros: `404`, `409 topic.read-only` (a dead-letter segue a fila de origem), `503`.

### `DELETE /topics/{topic}`

Remove o tópico e **todas as suas mensagens, mesmo as pendentes**: **sempre funciona**, sem checagem. Remove também a dead-letter. Falha no registro cancela (rollback); falha no commit, já decidido, não cancela: o coordenador insiste. Nunca há remoção parcial visível. `204`. Em `503 cluster.commit-timeout` a remoção **vai se completar** (um `GET` depois dará `404`). `409 topic.read-only` na dead-letter.

---

## Mensagens

### O corpo da mensagem

O corpo pode ter **qualquer formato** (JSON, texto, binário). O corpo da requisição de publicação são os **bytes**; o `Content-Type` diz o que são. O Taba guarda os **bytes como vieram** e o `Content-Type` como o metadado imutável `contentType`.

- **Sem `Content-Type`**, vale `application/json`. Valor inválido: `400 validation.invalid-content-type` (media type de 1 a 255 caracteres; parâmetros como `charset` são guardados).
- **Validação na publicação:** `application/json` precisa ser JSON válido e `text/*` precisa ser UTF-8, senão `400 validation.invalid-body`. Para os demais tipos não há validação.
- **Como o `body` aparece na entrega** (só aí; no disco ficam os bytes crus):

| `contentType` | `body` na resposta | `bodyEncoding` |
|---|---|---|
| `application/json` ou `*/*+json` | O **JSON nativo**, exatamente como enviado | ausente |
| `text/*` em UTF-8 (ou sem `charset`) | Uma **string** com o texto | ausente |
| Qualquer outro (inclui `text/*` com outro `charset`) | Os bytes em **base64** | `base64` |

- **Tamanho:** o que conta são os **bytes enviados** (não o base64), limitados por `maxMessageBytes` do tópico (ou `message.max-bytes`, 5 MB); acima disso, `413 message.too-large`. O base64 na resposta ocupa cerca de 33% a mais.

### `POST /topics/{topic}/messages`: publicar

O nó que recebe é o **coordenador**: gera o `id`, faz o registro + commit e responde quando o **quórum** confirma ([10-publicacao.md](10-publicacao.md)). Na **FIFO**, repassa ao primário do grupo.

| Query | Obrig. | Descrição |
|---|---|---|
| `groupId` | só na FIFO | Grupo (1 a 128, `A-Za-z0-9_-.:`). Fora da FIFO, orienta a entrega justa |
| `deduplicationId` | não, **só na FIFO** | Chave de deduplicação no grupo; sem ela, o SHA-256 do corpo. Fora da FIFO: `400 validation.not-applicable` |

**Resposta numa fila — `PublishResponse`:**

| Campo | Descrição |
|---|---|
| `id` | O `id` da mensagem (numa duplicata, o da **original**) |
| `duplicate` | **Só na FIFO.** `true` se repetiu uma publicação da janela de deduplicação (nada foi criado) |

Status: **`201`** criada, **`200`** duplicata.

```bash
curl -s -i -X POST localhost:8080/topics/pedidos/messages -H 'Content-Type: application/json' -d '{"pedido":9001}'
# HTTP/1.1 201 Created
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}

curl -s -X POST "localhost:8080/topics/ordens/messages?groupId=g1&deduplicationId=d1" -d '{"n":1}'
# {"duplicate":false,"id":"01a1363a-5434-…"}   (a repetição: {"duplicate":true,"id":"01a1363a-5434-…"} com 200)
```

**Resposta num distribuidor — `200 DistributorPublishResponse`:** `matched` (filas que casaram; zero não é erro) e `deliveries`, um item por fila com `topic` e **um** de `id` (cópia criada) ou `error` (`{code, message}`, sem status):

```json
{"deliveries":[{"id":"01a1363a-4e96-…","topic":"pedidos-eu"},
               {"error":{"code":"cluster.quorum-unavailable","message":"…"},"topic":"pedidos-us"}],"matched":2}
```

| Status | Códigos | Quando |
|---|---|---|
| `400` | `validation.group-id-required`, `invalid-body`, `invalid-content-type`, `not-applicable`, `failed` | `groupId` ausente na FIFO, corpo que não confere com o `Content-Type`, `deduplicationId` fora da FIFO |
| `401`/`403` | `auth.*` | Exige `publish`. Num distribuidor só ele é conferido, não as filas destino |
| `404` | `topic.not-found` | — |
| `413` | `message.too-large` | Acima do limite do tópico |
| `422` | `distributor.too-many-targets` | Mais destinos que `topics.distributor.max-targets`: recusada **inteira** |
| `429` | `rate-limit.exceeded` | — |
| `502`/`504` | `forward.bad-gateway`, `forward.timeout` | Repasse falhou **depois** de enviado o corpo. **Fora da FIFO não é repetido** (duplicaria); na FIFO é seguro repetir |
| `503` | `cluster.quorum-unavailable`, `fifo.primary-unavailable`, `cluster.node-syncing`, `cluster.node-draining` | A mensagem **não foi aceita** |

### `GET /topics/{topic}/messages`: consumir

Entrega até `maxMessages` mensagens **prontas**, cada uma **invisível** por `visibilityTimeout`. Devolve o que estiver pronto, **sem esperar completar** `maxMessages` (com `waitTime > 0`, espera só a primeira). **Qualquer** `GET` conta como consumidor para a ociosidade de uma fila volátil. Sem garantia de ordem (exceto na FIFO, por grupo). Detalhes em [12-entrega.md](12-entrega.md).

| Query | Padrão | Descrição |
|---|---|---|
| `maxMessages` | `10` | 1 a `delivery.max-messages.max` |
| `visibilityTimeout` | `120` | 0 a `delivery.visibility-timeout.max` (s) |
| `waitTime` | `0` | **Long polling**: 0 a **20** s |
| `autoCreate` | `false` | Cria a **fila** se não existir; os parâmetros abaixo só valem na criação (se existe, são ignorados) |
| `volatile` | `true` | Só com `autoCreate` |
| `idleTimeout` | `600` | Só com `autoCreate` |
| `fifo` | `false` | Só com `autoCreate` |
| `tag.<chave>` | — | Só com `autoCreate` |

**Resposta: `200 ConsumeResponse`** `{ "messages": [ Message… ] }`. **Lista vazia** quando não há nada pronto: sempre `200`, **nunca `204`**.

`Message`:

| Campo | Descrição |
|---|---|
| `id` | GUID v7 |
| `body` | Conforme o `contentType` (tabela acima) |
| `bodyEncoding` | `base64`, se for o caso |
| `contentType` | O da publicação |
| `groupId`, `deduplicationId` | Se tem |
| `receiptToken` | **Guarde-o:** é exigido no ack e na alteração da invisibilidade |
| `deliveryCount` | Entregas até agora (esta inclusa) |
| `publishedAt` | Instante da publicação |
| `invisibleUntil` | Fim da invisibilidade desta entrega |
| `deadLetteredAt`, `deadLetterReason` | Só na dead-letter |

```bash
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[{"id":"01a13635-a913-…","body":{"pedido":9001},"contentType":"application/json",
#   "receiptToken":"01a13635-aa19-…","deliveryCount":1,"publishedAt":"2026-10-13T17:00:48.787Z",
#   "invisibleUntil":"2026-10-13T17:02:49.049Z"}]}
```

Exemplo com texto e binário (verificado):

```json
{"messages":[
 {"id":"…","body":"Pedido 9001 aprovado","contentType":"text/plain; charset=utf-8","receiptToken":"…",…},
 {"id":"…","body":"iVBORw0KGgo=","contentType":"image/png","bodyEncoding":"base64","receiptToken":"…",…}]}
```

| Status | Códigos | Quando |
|---|---|---|
| `400` | `validation.out-of-range`, `reserved-name`, `invalid-name` | Parâmetro fora da faixa; `autoCreate` com nome terminado em `-dead-letter` |
| `401`/`403` | `auth.*` | Exige `consume`, e `create` se o `autoCreate` for criar |
| `404` | `topic.not-found` | Não existe e sem `autoCreate` |
| `409` | `topic.wrong-type` | O tópico é um distribuidor |
| `422` | `topic.volatile-limit`, `topic.fifo-needs-replication` | Limites do `autoCreate` |
| `429` | `rate-limit.exceeded` | — |
| `503` | `delivery.unavailable`, `cluster.quorum-unavailable`, `cluster.node-syncing` | **Nenhuma** mensagem capturada e houve falha de registro: nunca responde vazio nesse caso. Se ao menos uma foi capturada, `200` com as que conseguiu |

### `PUT /topics/{topic}/messages/{id}/visibility`

Define o prazo de invisibilidade de uma mensagem **já entregue** (ex.: mais tempo de processamento). `PUT` porque o valor **substitui** e repetir dá o mesmo resultado. **Sem** incrementar `deliveryCount`. O valor é o novo tempo **a partir de agora**, não um acréscimo. **`0` libera a mensagem na hora.**

| Query | Obrig. | Descrição |
|---|---|---|
| `receiptToken` | sim | O token da **última entrega** |

Corpo `VisibilityRequest`: `{ "visibilityTimeout": <s> }` (0 a `delivery.visibility-timeout.max`). **`200 VisibilityResponse`:** `{ "id", "invisibleUntil" }` (com `0`, o instante atual).

```bash
curl -s -X PUT "localhost:8080/topics/dl/messages/<id>/visibility?receiptToken=<token>" \
  -H 'Content-Type: application/json' -d '{"visibilityTimeout":30}'
# {"id":"01a1363d-c5cb-…","invisibleUntil":"2026-10-13T17:10:13.601Z"}
```

Erros: `400` (`required`, `out-of-range`), `404` (`topic.not-found`, `message.not-found`: já confirmada, venceu por `ttl` ou o tópico sumiu), `409 message.stale-receipt`, `429`, `503`.

### `DELETE /topics/{topic}/messages/{id}`: confirmar (ack)

`?receiptToken=<token>` (obrigatório). **`204`**. Remove a mensagem com decisão por **quórum**; falha antes do quórum **cancela** o ack e a mensagem continua disponível. Se o prazo venceu mas a mensagem **ainda não foi reentregue**, o token continua válido e o ack vale.

Erros: `400 validation.required` (sem `receiptToken`), `404` (inexistente, **inclusive um segundo ack** ou mensagem vencida por `ttl`), `409` (`message.stale-receipt`: foi reentregue a outro; `topic.wrong-type`: distribuidor), `429`, `503 cluster.quorum-unavailable` (o ack foi cancelado).

### `DELETE /topics/{topic}/messages`: prune (esvaziar a fila)

Remove **todas as mensagens** da fila — visíveis e invisíveis, e as cópias que as réplicas guardam — em **todo o cluster**. Permissão `admin` sobre o tópico. O tópico, sua definição e seus contadores ficam. Só filas (um distribuidor dá `409 topic.wrong-type`).

O nó que recebe a chamada pede a **cada membro** (ele inclusive) que esvazie a própria cópia, todos ao mesmo tempo, sob um `pruneId` (o id torna a chamada idempotente em cada nó), e espera todos. **Só responde sucesso se todos tiverem conseguido.** É um ato raro, por isso não há nova tentativa nem desfazer.

```bash
curl -s -X DELETE localhost:8080/topics/orders/messages
```

**`PruneResult`**: `pruneId`, `removed` (mensagens removidas, cada uma contada **uma vez**: quem é dono da mensagem a conta, as réplicas não) e `nodes` (`nodeId` e `removed` de cada um).

Se algum nó falhar: `503 cluster.prune-incomplete`, com os nós na mensagem e em `details`. A fila pode ter ficado vazia só em alguns nós; **repetir a chamada é seguro**. Uma mensagem publicada enquanto o prune corre pode sobreviver a ele, e um nó que ainda está sincronizando não é coberto ([31-pendencias.md](31-pendencias.md)).

Erros: `403` (`auth.forbidden` com `required: "admin"`, `auth.out-of-scope`), `404 topic.not-found`, `409 topic.wrong-type`, `429`, `503`.

---

## Tokens

Só existem com a **autenticação ligada** e só aceitam o **token raiz**. Sem autenticação ou em modo desenvolvimento: `404 route.not-found`. Modelo e permissões em [14-seguranca.md](14-seguranca.md).

| Rota | Faz | Sucesso |
|---|---|---|
| `POST /tokens` | Cria. Corpo `{name, permissions, topicPrefixes?, expiresAt?}`. A resposta traz o segredo em **`token`** (`tb_<id>.<segredo>`), **só aqui** | `201`, `Location: /tokens/{id}` |
| `GET /tokens` | Lista, paginada. `sort`: `id` ou `createdAt` (não `name`); `name` filtra por trecho. **Nunca devolve o segredo** | `200` |
| `GET /tokens/{id}` | Lê | `200` / `404 token.not-found` |
| `PATCH /tokens/{id}` | Só `name` e `expiresAt` (`null` remove o prazo). Permissões e prefixos são imutáveis | `200` |
| `DELETE /tokens/{id}` | Revoga (vale em todos os nós no commit) | `204` |

`Token`: `id`, `name`, `permissions`, `topicPrefixes`, `createdAt`, `expiresAt` (`TokenCreated` = `Token` + `token`).

Erros: `400` (`validation.*`: permissão desconhecida, lista vazia, prefixo inválido, `expiresAt` no passado), `401`/`403` (só o raiz), `404`, `503` (registro não confirmado: tente de novo).

## Quem é o token, os nós e as estatísticas

Rotas para **qualquer token válido** (com a autenticação desligada, para todos). Mostram o que um token simples pode ver dentro do seu escopo, sem ser administrador.

| Rota | Responde |
|---|---|
| `GET /auth/me` | `Me`: `authEnabled`, `root`, `id` (`null` para o token raiz), `name`, `permissions`, `topicPrefixes`, `expiresAt`. Com a autenticação desligada o chamador é `open`, com todas as permissões. Um cliente a usa para habilitar ou desabilitar o que mostra **antes** de chamar |
| `GET /nodes` | `collectedAt`, `membersVersion`, `partial`, `missingNodes` e `items`: um `Node` por membro — `nodeId`, `self` (é o nó que respondeu), `reachable`, `state` (`active`, `syncing`, `drained`...), `drainState`, `version`, `memberState`. **Sem endereços.** Um nó que saiu por drain aparece com `state: "drained"` **na resposta de qualquer nó**: cada nó só lembra das saídas que viu, e um que volta de um drain começa com a memória vazia, então a lista une o que todos os membros sabem. Um nó drenado que voltou tem um id novo e deixa de aparecer com o id antigo |
| `GET /stats/topics`, `/stats/topics/{topic}`, `/stats/topics/{topic}/groups` | Os mesmos DTOs e parâmetros de `/metrics/topics*` da API de operação ([17-api-operacao.md](17-api-operacao.md)), mas **do cluster inteiro**, com `partial` e `missingNodes`. A lista só traz os tópicos **dentro dos prefixos do token** (como o `GET /topics`); `/stats/topics/{topic}` fora deles é `403 auth.out-of-scope`. `?scope=node` dá a visão só do nó que responde |

### Leituras consolidadas (`partial` e `missingNodes`)

`GET /nodes`, `GET /cluster/status`, `GET /cluster/backup` e `GET /stats/topics…` respondem pelo **cluster inteiro**. Cada nó diz o que **ele** vê, na API do cluster ([18-api-cluster.md](18-api-cluster.md)); o nó que recebe a chamada pergunta a todos os membros ao mesmo tempo (ele mesmo sem rede) e junta as respostas. Cada chamada tem prazo de 5 s (fixo). Um nó que não responde no prazo **não** derruba a leitura: a resposta é `200` e diz que está incompleta.

```json
{ "partial": true, "missingNodes": [{ "nodeId": "01a1…", "reason": "timeout" }] }
```

`reason` é `timeout`, `unreachable` (nenhuma resposta) ou `error` (o nó respondeu com um erro). Os números são então os dos nós que **responderam**, e o cliente deve mostrá-los com um aviso e continuar perguntando. Uma **escrita** que falha em algum nó é diferente: é um `503` que nomeia os nós (o prune).

**Como as estatísticas de tópico se juntam** (uma mensagem é contada pelo nó que é dono dela, então as contagens somam):

| Campo | Junção |
|---|---|
| contagens e taxas (`published`, `consumed`, `acked`, `visibleMessages`, `invisibleMessages`, `depth`, `bytes`, memória e disco...) | soma |
| `oldestMessageAt` | a mais antiga |
| `publishLatency`, `consumeLatency` (p50, p95, p99) | o **máximo** dos nós: percentis não se somam, e o máximo erra para o lado cauteloso |
| `groups`, `noisyShare` | `groups` e o maior grupo são o máximo dos nós (um grupo pode ter mensagens em vários nós, então somar o contaria duas vezes): um **limite inferior** |
| `conditions`, `worstCondition` | avaliadas de novo sobre os números juntos |
| `memoryShare` | a memória do tópico sobre a memória dos nós que responderam |
| `/stats/topics/{topic}/groups` | cada grupo uma vez, com suas mensagens de todos os nós |

## Cluster e nós (token raiz)

A API de operação ([17-api-operacao.md](17-api-operacao.md)) não tem token e serve ao monitoramento contínuo. Estas rotas são as cópias dela na API de cliente, para um cliente que tem uma URL e um token, e exigem o **token raiz**: a permissão `admin` é por tópico, e estas rotas mostram a topologia e o interior dos nós. Um token que não é o raiz recebe `403 auth.forbidden` com `"required": "root"` no corpo, para o cliente poder dizer o que falta.

**A regra do nó** (a mesma de uma mensagem). O nó que recebe `/nodes/{nodeId}/…` compara o `nodeId` com o seu. Se **é** esse nó, responde com os próprios dados; se **não é**, chama **uma vez** o nó que tem o dado, pela porta do cluster (`/cluster/node-call/{caminho}`, só membros, [18-api-cluster.md](18-api-cluster.md)), e devolve a resposta como veio. Nunca repassa uma chamada que já foi repassada. Um `nodeId` desconhecido é `404 node.not-found`; um nó que não responde é `502 forward.bad-gateway` (ou `504 forward.timeout`) que o nomeia, sem nova tentativa. Um nó que saiu por drain continua endereçável, para devolvê-lo.

| Rota | Faz |
|---|---|
| `GET /cluster/status` | O que o `GET /health/cluster` responde: as condições e o `NodeStatus` de cada nó (que agora traz `runtime`: `activeRequests`, `rateLimitRejected`, `channelRttP95Seconds`), com `partial` e `missingNodes`. Aceita `scope=node`. O `summary` diz também se o cluster está parado: `servingNodes` (nós ativos e fora de drain), `stopped` (todo nó que respondeu está em drain) e, com `stopped`, `plannedStop` e `stoppedAt` (a lista de parada em S3 do `POST /cluster/stop`). Um nó em drain responde esta rota (e `/nodes`) ele mesmo quando nenhum outro nó pode atender a chamada. |
| `GET /cluster/backup` | O estado do backup em S3 de **cada** nó, cada um como o `GET /nodes/{nodeId}/backup` o dá, com `partial` e `missingNodes` (`422 backup.not-configured` sem ele) |
| `GET /nodes/{nodeId}/metrics` | O texto Prometheus do nó (o que o `GET /metrics` dá) |
| `GET`, `POST`, `DELETE /nodes/{nodeId}/drain` | Estado, início e retorno do drain daquele nó: o que o `/node/drain` faz |
| `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush` | O backup em S3 daquele nó (`422 backup.not-configured` sem ele) |
| `POST /cluster/backup/flush`, `POST /cluster/stop` | O cluster inteiro: o mesmo da API de operação. Só montadas quando a API de operação tem porta própria; sem `operations.port` as rotas de operação já servem esses nomes na porta de cliente, sem token |

Ligar ou matar **processos** não é oferecido: é tarefa do orquestrador (o `drain` faz o nó parar de receber, e o retorno o traz de volta como um nó novo).

## Fontes e provas

- `http/nodes`, `http/topic_stats` (`client_routes`), `http/messages/prune.rs`, `http/topics`, `http/messages`, `http/tokens`, `http/auth`, `core/topic` (definição), `core/validation` (faixas), `core/content` (forma do corpo), `engine/topics`, `engine/messages`.
- Todos os exemplos acima foram executados contra um nó real; as respostas são cópias.
- Casos de uso: UC-TOP (prune: 044 a 048), UC-MSG (publicação e corpo), UC-CNS (consumo), UC-ACK, UC-VIS, UC-AUT (tokens; 037 a 039: `required`, `auth/me`, quem vê o quê), UC-OPS (046 a 051: os nós e as leituras consolidadas), UC-PAG; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/topics.rs`, `messages.rs`, `auth.rs`, `definitions.rs`, `nodes_api.rs`.
- Histórico: `06a-api-cliente.md`.
