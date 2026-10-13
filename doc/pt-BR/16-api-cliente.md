---
título: API de cliente
resumo: Manual rota a rota da API que as aplicações usam — tópicos, mensagens (publicar, consumir, invisibilidade, ack) e tokens — com parâmetros, DTOs, status e exemplos.
tipo: referência
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/http/topics, crates/taba-node/src/http/messages, crates/taba-node/src/http/tokens, crates/taba-node/src/http/auth, crates/taba-node/src/core/topic, crates/taba-node/src/core/validation, crates/taba-node/src/core/content]
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
| `POST` `GET` `PATCH` `DELETE` | `/tokens`, `/tokens/{id}` | só o raiz | admin | ver abaixo |

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

## Fontes e provas

- `http/topics`, `http/messages`, `http/tokens`, `http/auth`, `core/topic` (definição), `core/validation` (faixas), `core/content` (forma do corpo), `engine/topics`, `engine/messages`.
- Todos os exemplos acima foram executados contra um nó real; as respostas são cópias.
- Casos de uso: UC-TOP, UC-MSG (publicação e corpo), UC-CNS (consumo), UC-ACK, UC-VIS, UC-AUT (tokens), UC-PAG; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/topics.rs`, `messages.rs`, `auth.rs`, `definitions.rs`.
- Histórico: `06a-api-cliente.md`.
