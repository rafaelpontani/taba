# API de cliente

[← Índice](README.md) · [Visão geral das APIs](06-api.md) · [Convenções e DTOs padronizados](06d-api-convencoes.md)

## 8.1 API de cliente

Manual da API que as aplicações usam: criar e configurar tópicos, publicar, consumir e confirmar mensagens, e gerenciar tokens de acesso.

> **Porta:** `api.port` ([06-api.md](06-api.md), seção 8.0). **Autenticação:** opcional, por token (seção 8.1.3). A numeração das seções é a do conjunto das APIs: a 8.1.1, a API de operação, está em [06b-api-operacao.md](06b-api-operacao.md).
>
> **Formato, cabeçalhos, `ErrorResponse`, `Page<T>`, códigos de status e catálogo de erros** são comuns às três APIs e estão em [06d-api-convencoes.md](06d-api-convencoes.md). Este arquivo só traz o que é específico de cada rota.

### Resumo das rotas

| Verbo | Recurso | Permissão do token | Rate limit | Sucesso |
|---|---|---|---|---|
| `POST` | `/topics` | `create` | admin | `201` `Topic` |
| `GET` | `/topics` | qualquer token válido | admin | `200` `Page<Topic>` |
| `GET` | `/topics/{topic}` | qualquer token válido | admin | `200` `Topic` |
| `PATCH` | `/topics/{topic}` | `admin` | admin | `200` `Topic` |
| `DELETE` | `/topics/{topic}` | `admin` | admin | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | publish | `201` `PublishResponse` (ou `200`) |
| `GET` | `/topics/{topic}/messages` | `consume` (e `create` para o `autoCreate`) | consume | `200` `ConsumeResponse` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | consume | `200` `VisibilityResponse` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | consume | `204` |
| `POST` | `/tokens` | só o token raiz | admin | `201` `TokenCreated` |
| `GET` | `/tokens` | só o token raiz | admin | `200` `Page<Token>` |
| `GET` | `/tokens/{id}` | só o token raiz | admin | `200` `Token` |
| `PATCH` | `/tokens/{id}` | só o token raiz | admin | `200` `Token` |
| `DELETE` | `/tokens/{id}` | só o token raiz | admin | `204` |

A coluna "Permissão" só vale com a autenticação ligada; desligada, nenhuma é exigida. As permissões estão na seção 8.1.3, e os limites de taxa em [12-limites-api.md](12-limites-api.md).

## Tópicos

### Regras de nome, tags e limites

**Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.body`):

- de 1 a 100 caracteres na criação. Nome terminado no sufixo de dead-letter (`-dead-letter`) é **reservado** e rejeitado com `400` `validation.reserved-name`. A fila de dead-letter, de nome `{fila}` + sufixo, pode passar de 100 (até 112 com o sufixo padrão), e o `{topic}` das rotas a aceita;
- apenas letras ASCII (`a-z`, `A-Z`), dígitos (`0-9`), hífen (`-`) e sublinhado (`_`): regex `^[A-Za-z0-9_-]{1,100}$`;
- diferencia maiúsculas de minúsculas (`Pedidos` e `pedidos` são tópicos distintos);
- nome inválido é rejeitado na criação com `400` `validation.invalid-name`.

**Tags do tópico** (`tags`): pares chave/valor definidos na criação e alterados pelo `PATCH`. Servem para monitoramento: são enviadas como **atributos** nos traces e logs exportados via **OpenTelemetry** para as operações daquele tópico, e nas métricas de tópico conforme a configuração de exportação ([11-observabilidade.md](11-observabilidade.md)) (ex.: `{"time": "financeiro", "ambiente": "prod"}`).

- chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL);
- máximo de 20 tags por tópico;
- as tags não afetam o comportamento das mensagens, apenas a telemetria.

**Faixas dos campos da definição:**

| Campo | Faixa |
|---|---|
| `idleTimeout` | 60 a 604800 s (7 dias) |
| `ttl` | 60 a 31536000 s (365 dias) |
| `maxDeliveries` | 1 a 1000 |
| `maxMessageBytes` | 1 até `message.max-bytes.hard-limit` |
| `prefixes` (distribuidor) | até 20 prefixos de 1 a 100 caracteres (`A-Za-z0-9_-`); prefixo vazio é rejeitado |

### `POST /topics`: criar um tópico

Cria um tópico (fila ou distribuidor). Gera o `id` (GUID v7). A definição é propagada a **todos** os nós por registro + commit e persistida em disco ([02-arquitetura.md](02-arquitetura.md), seção 3.3), e a API só responde depois do commit. Toda **fila** ganha junto uma fila `{nome}-dead-letter` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10).

**Corpo: `TopicCreateRequest`**

| Campo | Tipo | Obrigatório | Padrão | Descrição |
|---|---|---|---|---|
| `name` | texto | sim | — | Nome do tópico (regras acima). |
| `type` | texto | sim | — | `queue` (fila) ou `distributor` (distribuidor). |
| `tags` | mapa texto→texto | não | `{}` | Tags de monitoramento. |
| `prefixes` | lista de texto | só `distributor` | `[]` | Prefixos de nome das filas para onde cada publicação é copiada ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). Com a lista vazia, o distribuidor não copia para ninguém. |
| `volatile` | booleano | só `queue` | `false` | Fila volátil: excluída sozinha, com as mensagens, se ficar sem consumidor por `idleTimeout`. |
| `idleTimeout` | inteiro (s) | só `queue` com `volatile` | `600` (`topics.volatile.idle-timeout.default`) | Tempo sem consumidor até a exclusão da fila volátil. |
| `fifo` | booleano | só `queue` | `false` | Fila FIFO: ordem por grupo e deduplicação ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16). **Imutável.** Exige `replication.factor` de pelo menos 2. |
| `maxMessageBytes` | inteiro (bytes) | só `queue` | `null` (vale `message.max-bytes`, 5 MB) | Tamanho máximo da mensagem **deste** tópico. Pode ser maior que o geral, até `message.max-bytes.hard-limit`. |
| `ttl` | inteiro (s) | só `queue` | `null` (vale `retention.ttl.default`, 14 dias) | Tempo de vida da mensagem desde a publicação. |
| `maxDeliveries` | inteiro | só `queue` | `null` (vale `retention.max-deliveries.default`, 10) | Entregas sem ack até a mensagem ir ao dead-letter. |

Um campo que não vale para o tipo (`prefixes` numa fila, `fifo` num distribuidor, `ttl` num distribuidor) é recusado com `400` `validation.not-applicable`.

**Resposta de sucesso: `201 Created`, `Topic`** (seção "DTOs"), com `Location: /topics/{name}`.

**Status:**

| Status | Código | Quando |
|---|---|---|
| `201` | — | Criado e efetivado em todos os nós. |
| `400` | `validation.failed`, `validation.invalid-name`, `validation.reserved-name`, `validation.not-applicable`, `validation.out-of-range`, `validation.max-bytes-above-hard-limit`, `validation.unknown-field` | Corpo inválido. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Sem token válido, sem `create`, ou nome (ou `prefixes`) fora dos prefixos do token. |
| `409` | `topic.already-exists` | Já existe um tópico com o nome. |
| `422` | `topic.fifo-needs-replication` | Fila FIFO com `replication.factor` abaixo de 2. |
| `429` | `rate-limit.exceeded` | Limite administrativo do token (ou do IP, sem token válido). |
| `503` | `cluster.registration-failed`, `cluster.commit-timeout`, `cluster.node-syncing` | Algum nó não confirmou o registro (nada foi criado), ou o commit está se completando em segundo plano ([06d](06d-api-convencoes.md), seção 8.3.5). |

**Exemplo:**

```http
POST /topics
Content-Type: application/json

{ "name": "pedidos", "type": "queue", "fifo": true, "tags": { "time": "financeiro" }, "ttl": 86400 }
```

```http
201 Created
Location: /topics/pedidos
X-Request-Id: 01927f3a-7c1e-7d2b-9f10-3a5b6c7d8e9f

{
  "id": "01927f3a-4a10-7b11-8c22-0d1e2f3a4b5c",
  "name": "pedidos",
  "type": "queue",
  "tags": { "time": "financeiro" },
  "createdAt": "2026-10-06T12:00:00.000Z",
  "volatile": false,
  "fifo": true,
  "maxMessageBytes": null,
  "ttl": 86400,
  "maxDeliveries": null,
  "effective": { "maxMessageBytes": 5242880, "ttl": 86400, "maxDeliveries": 10 }
}
```

### `GET /topics`: listar tópicos

Lista os tópicos, **paginada** (seção 8.1.2). Respondida **localmente**, sem consultar outros nós: a definição do tópico é replicada em todo nó (seção 3.3), ao contrário de `/metrics/topics`, que faz fan-out. Com a autenticação ligada, devolve só os tópicos dentro dos prefixos do token. Inclui as filas de dead-letter.

**Parâmetros de query:**

| Parâmetro | Tipo | Padrão | Descrição |
|---|---|---|---|
| `limit` | inteiro | a calibrar ([10-configuracao.md](10-configuracao.md)) | Itens por página. |
| `cursor` | texto | — | Cursor da página anterior (`nextCursor`). |
| `sort` | texto | `name` | `name`, `id` ou `createdAt`, com `-` na frente para decrescente (`-createdAt`). |
| `name` | texto | — | Devolve os tópicos cujo nome **contém** o texto (ex.: `?name=pedidos`). Diferencia maiúsculas de minúsculas. |

**Resposta de sucesso: `200 OK`, `Page<Topic>`.**

| Status | Código | Quando |
|---|---|---|
| `200` | — | Página devolvida (`items` vazio se não há tópicos). |
| `400` | `validation.out-of-range`, `validation.invalid-cursor`, `validation.failed` | `limit` fora da faixa, cursor inválido, `sort` desconhecido. |
| `401` / `429` | `auth.unauthorized` / `rate-limit.exceeded` | — |

**Exemplo:**

```http
GET /topics?name=pedidos&sort=-createdAt&limit=2
```

```json
{
  "items": [
    { "id": "01927f3a-4a10-7b11-8c22-0d1e2f3a4b5c", "name": "pedidos", "type": "queue", "...": "..." },
    { "id": "01927f3a-4a10-7b11-8c22-0d1e2f3a4b5d", "name": "pedidos-dead-letter", "type": "queue", "deadLetter": true, "source": "pedidos", "...": "..." }
  ],
  "nextCursor": "eyJrIjoiMDE5MjdmM2EtNGExMC03YjExLThjMjItMGQxZTJmM2E0YjVkIiwicyI6Ii1jcmVhdGVkQXQifQ"
}
```

### `GET /topics/{topic}`: ler um tópico

Devolve a definição e as configurações do tópico. Também respondida localmente. Na **dead-letter**, os valores são os da fila de origem, somente leitura, com `deadLetter: true` e `source`.

**Resposta de sucesso: `200 OK`, `Topic`.**

| Status | Código | Quando |
|---|---|---|
| `200` | — | — |
| `401` / `403` | `auth.unauthorized` / `auth.out-of-scope` | Sem token válido, ou tópico fora dos prefixos do token. |
| `404` | `topic.not-found` | O tópico não existe. |
| `429` | `rate-limit.exceeded` | — |

### `PATCH /topics/{topic}`: alterar um tópico

Altera a definição. Mesma propagação do `POST`. **Atualização parcial:** só os campos enviados mudam; `tags`, quando enviado, **substitui o mapa inteiro**; `null` em `maxMessageBytes`, `ttl` ou `maxDeliveries` **volta ao padrão do cluster**. Vale também para as mensagens já guardadas (o `ttl` novo vale já na próxima varredura), e a alteração de `maxMessageBytes` só vale para as publicações seguintes.

**Corpo: `TopicUpdateRequest`**

| Campo | Tipo | Aplica-se a | Descrição |
|---|---|---|---|
| `tags` | mapa texto→texto | todos | Substitui as tags. |
| `prefixes` | lista de texto | `distributor` | Substitui os prefixos. Vale a partir do commit, só para publicações posteriores. |
| `idleTimeout` | inteiro (s) | fila volátil | Vale na próxima verificação do gestor. |
| `maxMessageBytes` | inteiro ou `null` | fila | — |
| `ttl` | inteiro ou `null` | fila | — |
| `maxDeliveries` | inteiro ou `null` | fila | — |

**Imutáveis:** `name`, `id`, `type`, `volatile` e `fifo`. Enviá-los, mesmo com o valor atual, responde `400` `validation.immutable-field`.

**Resposta de sucesso: `200 OK`, `Topic`** (já com a alteração).

| Status | Código | Quando |
|---|---|---|
| `200` | — | — |
| `400` | `validation.immutable-field`, `validation.not-applicable`, `validation.out-of-range`, `validation.max-bytes-above-hard-limit`, `validation.unknown-field` | — |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `admin`; num distribuidor, os novos `prefixes` também precisam caber nos prefixos do token. |
| `404` | `topic.not-found` | — |
| `409` | `topic.read-only` | A fila `{nome}-dead-letter` é somente leitura e segue a fila de origem. |
| `429` / `503` | `rate-limit.exceeded` / `cluster.registration-failed`, `cluster.commit-timeout` | — |

### `DELETE /topics/{topic}`: remover um tópico

Remove o tópico e **todas** as suas mensagens, mesmo as pendentes (não entregues ou não confirmadas): **sempre funciona, sem checagem**. Remover uma fila remove também a sua dead-letter. Mesma propagação do `POST`: falha no registro **cancela** a remoção (rollback); falha no commit, já decidido, não cancela, e o coordenador insiste dentro do teto `write.commit.timeout`. Nunca há uma remoção parcial visível.

**Resposta de sucesso: `204 No Content`.**

| Status | Código | Quando |
|---|---|---|
| `204` | — | Removido em todos os nós. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `admin`. |
| `404` | `topic.not-found` | — |
| `409` | `topic.read-only` | Dead-letter: remova a fila de origem. |
| `429` / `503` | `rate-limit.exceeded` / `cluster.registration-failed`, `cluster.commit-timeout` | Em `cluster.commit-timeout` a remoção **vai se completar**; um `GET` posterior responde `404`. |

## Mensagens

### O corpo da mensagem

O corpo de uma mensagem pode ter **qualquer formato**: JSON, texto ou binário. O corpo da requisição de publicação são os **bytes da mensagem**, e o `Content-Type` da requisição diz o que eles são. O Taba guarda os **bytes como vieram** (sem reserializar) e o `Content-Type` como o metadado imutável `contentType` da mensagem, replicado e devolvido na entrega.

- **Sem `Content-Type`**, vale `application/json`. O valor é um media type de 1 a 255 caracteres; um valor inválido responde `400` `validation.invalid-content-type`. Os parâmetros (como `charset`) são guardados e devolvidos.
- **Como o `body` aparece na entrega** (e só aí; no disco ficam sempre os bytes crus):

| `contentType` | `body` na resposta | `bodyEncoding` |
|---|---|---|
| `application/json` ou `*/*+json` | O **JSON nativo**, exatamente como foi enviado (objeto, lista, texto, número, booleano ou `null`). | ausente |
| `text/*`, em UTF-8 (ou sem `charset`) | Uma **string** JSON com o texto. | ausente |
| Qualquer outro, incluindo `text/*` com outro `charset` | Os bytes em **base64** (uma string). | `base64` |

- **Validação na publicação:** um corpo `application/json` que não seja JSON válido, ou `text/*` que não seja UTF-8 válido, responde `400` `validation.invalid-body`. Para os demais tipos não há validação.
- **Tamanho:** o que conta são os **bytes enviados** (os originais, não a forma em base64), limitados por `maxMessageBytes` do tópico (ou, sem ele, `message.max-bytes`, 5 MB); acima disso, `413`. Na resposta, o binário em base64 ocupa cerca de 33% a mais, e isso pesa num lote de `maxMessages` grandes.
- **Entre nós** (`MessageRecord`, [06c-api-cluster.md](06c-api-cluster.md)): só há duas formas, o JSON nativo e o base64, para o limite de linha dos streams ser previsível.

### `POST /topics/{topic}/messages`: publicar

Publica uma mensagem. O nó que recebe a chamada é o **coordenador**: gera o `id`, faz o registro + commit nos nós responsáveis e responde quando o **quórum** confirma ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2). Numa **fila FIFO**, a publicação é repassada ao primário do grupo, que coordena ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.4).

**Parâmetros de query:**

| Parâmetro | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `groupId` | texto (1 a 128, `A-Za-z0-9_-.:`) | só na fila FIFO | O grupo da mensagem. Fora da FIFO, faz a entrega dar preferência aos grupos com menos mensagens ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11). Na FIFO, ordena a entrega. |
| `deduplicationId` | texto (1 a 128, `A-Za-z0-9_-.:`) | não, **só na fila FIFO** | Chave de deduplicação dentro do grupo. Sem ele, vale o SHA-256 do corpo. Fora da FIFO, `400` `validation.not-applicable`. |

**Corpo:** os bytes da mensagem, com o `Content-Type` correspondente (seção anterior). Esta é a única rota de cliente que aceita um `Content-Type` diferente de `application/json`.

**Resposta de sucesso numa fila: `PublishResponse`**

| Campo | Tipo | Descrição |
|---|---|---|
| `id` | texto | O `id` (GUID v7) da mensagem. Numa duplicata, o da **original**. |
| `duplicate` | booleano | **Só na fila FIFO.** `true` quando a publicação repetiu uma da janela de deduplicação (`fifo.dedup.window`): nenhuma mensagem foi criada. |

Status de sucesso: **`201`** quando a mensagem foi criada, **`200`** quando é uma duplicata (`duplicate: true`).

**Resposta de sucesso num distribuidor: `200 OK`, `DistributorPublishResponse`.** Cada fila que casou com um prefixo recebe uma **cópia independente**, com `id` próprio.

| Campo | Tipo | Descrição |
|---|---|---|
| `matched` | inteiro | Filas que casaram. Zero não é erro: a mensagem é descartada ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). |
| `deliveries` | lista de `DistributorDelivery` | Um item por fila que casou. |

`DistributorDelivery`: `topic` (texto, a fila) e **um** de `id` (a cópia foi criada) ou `error` (`{ "code": "cluster.quorum-unavailable", "message": "..." }`, sem status: o status do item é o do `code` no catálogo de [06d](06d-api-convencoes.md)). A **falha parcial não é erro do distribuidor**: a resposta é `200`, e as cópias que falharam trazem o `error`. Uma cópia sem `groupId` para uma fila FIFO falha só naquela fila, com `{ "code": "validation.group-id-required", "message": "..." }` (`400` no catálogo).

**Status:**

| Status | Código | Quando |
|---|---|---|
| `201` / `200` | — | Criada / duplicata na FIFO, ou resposta de um distribuidor. |
| `400` | `validation.group-id-required`, `validation.invalid-body`, `validation.invalid-content-type`, `validation.not-applicable`, `validation.failed` | `groupId` ausente na FIFO ou inválido, corpo que não confere com o `Content-Type` (JSON inválido, texto que não é UTF-8), `Content-Type` inválido, `deduplicationId` fora da FIFO. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `publish`. Num distribuidor, só ele é conferido, não as filas destino. |
| `404` | `topic.not-found` | — |
| `413` | `message.too-large` | Corpo acima do limite do tópico. |
| `422` | `distributor.too-many-targets` | Só num distribuidor, com `topics.distributor.max-targets` definido: a publicação é recusada **inteira**, sem nenhuma cópia. |
| `429` | `rate-limit.exceeded` | Limite de publicação do token (ou do IP, sem token válido). |
| `502` / `504` | `forward.bad-gateway`, `forward.timeout` | O repasse falhou depois de enviado o corpo. **Fora da FIFO a chamada não é repetida** (repetir duplicaria a mensagem): o cliente decide. Na FIFO é seguro repetir, pois o primário deduplica. |
| `503` | `cluster.quorum-unavailable`, `fifo.primary-unavailable`, `cluster.node-syncing`, `cluster.node-draining` | Sem quórum, ou o primário do grupo não pode coordenar agora. A mensagem não foi aceita. |

**Exemplos:**

```http
POST /topics/pedidos/messages?groupId=cliente-42&deduplicationId=pedido-9001
Content-Type: application/json

{ "pedido": 9001, "valor": 129.90, "itens": ["a", "b"] }
```

```http
201 Created

{ "id": "01927f3c-2b30-7a44-9d55-6e7f8091a2b3", "duplicate": false }
```

Repetindo a mesma chamada dentro da janela de deduplicação:

```http
200 OK

{ "id": "01927f3c-2b30-7a44-9d55-6e7f8091a2b3", "duplicate": true }
```

Publicação num distribuidor com uma cópia que falhou:

```http
200 OK

{
  "matched": 2,
  "deliveries": [
    { "topic": "pedidos-eu", "id": "01927f3a-7c1e-7d2b-9f10-3a5b6c7d8e9f" },
    { "topic": "pedidos-us", "error": { "code": "cluster.quorum-unavailable", "message": "Quórum de escrita indisponível." } }
  ]
}
```

### `GET /topics/{topic}/messages`: consumir

Entrega até `maxMessages` mensagens **prontas** e torna cada uma **invisível** a outros consumidores por `visibilityTimeout` segundos. Se não houver ack nesse prazo, a mensagem volta a ficar entregável (semântica *at-least-once*). Devolve o que estiver pronto, **sem esperar completar** `maxMessages` (com `waitTime > 0`, espera só a primeira). Cada mensagem passa pelo registro + commit de invisibilidade, e uma que falha não atrasa as demais ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.2). **Não há garantia de ordem**, exceto numa fila FIFO, que entrega cada grupo em ordem, uma mensagem por vez, com cada mensagem do lote de um grupo diferente (seção 16.6 de [17-topicos-fifo.md](17-topicos-fifo.md)).

Qualquer `GET` conta como consumidor para a ociosidade de uma fila volátil, com ou sem mensagem para entregar.

**Parâmetros de query:**

| Parâmetro | Tipo | Padrão | Descrição |
|---|---|---|---|
| `maxMessages` | inteiro | `10` (`delivery.max-messages.default`) | De 1 a `delivery.max-messages.max`. |
| `visibilityTimeout` | inteiro (s) | `120` (`delivery.visibility-timeout.default`) | Tempo de invisibilidade, de 0 a `delivery.visibility-timeout.max`. |
| `waitTime` | inteiro (s) | `0` | **Long polling**: de 0 a 20. Com valor maior que zero, aguarda mensagens novas antes de responder vazio. |
| `autoCreate` | booleano | `false` | Cria a fila se ela não existir. Os parâmetros abaixo só valem na criação; se o tópico existe e é uma fila, são **ignorados** ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.4). |
| `volatile` | booleano | `true` | Só com `autoCreate`. `false` cria uma fila persistente. |
| `idleTimeout` | inteiro (s) | `600` | Só com `autoCreate`. |
| `fifo` | booleano | `false` | Só com `autoCreate`. |
| `tag.<chave>` | texto | — | Só com `autoCreate`. Uma tag da fila por parâmetro (`tag.time=financeiro`). |

**Resposta de sucesso: `200 OK`, `ConsumeResponse`**

| Campo | Tipo | Descrição |
|---|---|---|
| `messages` | lista de `Message` | De 0 a `maxMessages` mensagens. Lista **vazia** quando não há nada pronto (sempre `200`, nunca `204`). |

**Status:**

| Status | Código | Quando |
|---|---|---|
| `200` | — | Com ou sem mensagens. |
| `400` | `validation.out-of-range`, `validation.reserved-name`, `validation.invalid-name`, `validation.failed` | Parâmetro fora da faixa; `autoCreate` com nome terminado no sufixo de dead-letter. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `consume`, e também `create` se o `autoCreate` for criar a fila. |
| `404` | `topic.not-found` | O tópico não existe e não há `autoCreate`. |
| `409` | `topic.wrong-type` | O tópico é um distribuidor (nunca é consumido). |
| `422` | `topic.volatile-limit`, `topic.fifo-needs-replication` | O `autoCreate` esbarra em `topics.volatile.max-count`, ou pede uma FIFO com fator de replicação abaixo de 2. |
| `429` | `rate-limit.exceeded` | Limite de consumo do token (ou do IP, sem token válido). |
| `503` | `delivery.unavailable`, `cluster.quorum-unavailable`, `cluster.node-syncing` | **Nenhuma** mensagem foi capturada e houve falha no registro de invisibilidade: nunca responde vazio nesse caso, que o cliente leria como fila sem mensagens. Se ao menos uma foi capturada, responde `200` com as que conseguiu. |

**Exemplo:**

```http
GET /topics/pedidos/messages?maxMessages=2&visibilityTimeout=60&waitTime=20
```

```json
{
  "messages": [
    {
      "id": "01927f3c-2b30-7a44-9d55-6e7f8091a2b3",
      "body": { "pedido": 9001, "valor": 129.90, "itens": ["a", "b"] },
      "contentType": "application/json",
      "groupId": "cliente-42",
      "deduplicationId": "pedido-9001",
      "receiptToken": "01927f3d-5c60-7e77-b188-92a3b4c5d6e7",
      "deliveryCount": 1,
      "publishedAt": "2026-10-06T12:01:00.000Z",
      "invisibleUntil": "2026-10-06T12:02:30.000Z"
    }
  ]
}
```

**Exemplo com corpo que não é JSON.** O `body` depende do `contentType` com que cada mensagem foi publicada (seção "O corpo da mensagem"). Aqui a fila recebeu três publicações:

```http
POST /topics/arquivos/messages
Content-Type: text/plain; charset=utf-8

Pedido 9001 aprovado
```

```http
POST /topics/arquivos/messages
Content-Type: image/png

<bytes do arquivo PNG>
```

```http
POST /topics/arquivos/messages
Content-Type: application/json

{ "pedido": 9001 }
```

```http
GET /topics/arquivos/messages?maxMessages=3
```

```json
{
  "messages": [
    {
      "id": "01927f40-1a00-7c10-8a01-1b2c3d4e5f60",
      "body": "Pedido 9001 aprovado",
      "contentType": "text/plain; charset=utf-8",
      "receiptToken": "01927f41-0001-7a00-9b00-aaaaaaaaaaa1",
      "deliveryCount": 1,
      "publishedAt": "2026-10-06T12:10:00.000Z",
      "invisibleUntil": "2026-10-06T12:12:00.000Z"
    },
    {
      "id": "01927f40-1a05-7c10-8a02-1b2c3d4e5f61",
      "body": "iVBORw0KGgo=",
      "contentType": "image/png",
      "bodyEncoding": "base64",
      "receiptToken": "01927f41-0002-7a00-9b00-aaaaaaaaaaa2",
      "deliveryCount": 1,
      "publishedAt": "2026-10-06T12:10:01.000Z",
      "invisibleUntil": "2026-10-06T12:12:00.000Z"
    },
    {
      "id": "01927f40-1a09-7c10-8a03-1b2c3d4e5f62",
      "body": { "pedido": 9001 },
      "contentType": "application/json",
      "receiptToken": "01927f41-0003-7a00-9b00-aaaaaaaaaaa3",
      "deliveryCount": 1,
      "publishedAt": "2026-10-06T12:10:02.000Z",
      "invisibleUntil": "2026-10-06T12:12:00.000Z"
    }
  ]
}
```

- **Texto:** `body` é uma string e não há `bodyEncoding`.
- **Binário:** `body` é a string em base64 dos bytes (no exemplo, os 8 primeiros bytes de um PNG), e `bodyEncoding` vale `base64`. O cliente decodifica e usa o `contentType` para saber o que é.
- **JSON:** `body` é o JSON nativo, como nos exemplos anteriores.

### `PUT /topics/{topic}/messages/{id}/visibility`: definir a invisibilidade

Define o prazo de invisibilidade de uma mensagem já entregue, por exemplo para ganhar mais tempo de processamento. A rota tem o nome da **única propriedade** que altera: cada propriedade alterável da mensagem, se surgir outra, terá a sua própria rota. `PUT` porque o valor **substitui** o anterior e repetir a chamada dá o mesmo resultado. Usa o mesmo registro + commit por quórum da entrega, **sem incrementar** `deliveryCount`. O valor é o novo tempo **a partir de agora** (`invisibleUntil = agora + visibilityTimeout`), **não um acréscimo** ao prazo atual. **`0` libera a mensagem na hora**, e o dono a recoloca na lista de prontos ([14-invisibilidade.md](14-invisibilidade.md), seção 13.3).

**Parâmetros de query:**

| Parâmetro | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `receiptToken` | texto | sim | O `receiptToken` da **última entrega** da mensagem. É a mesma credencial e o mesmo lugar do ack: em toda rota de mensagem, o `receiptToken` vai na query. |

**Corpo: `VisibilityRequest`**

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `visibilityTimeout` | inteiro (s) | sim | Novo tempo, de 0 a `delivery.visibility-timeout.max`. |

**Resposta de sucesso: `200 OK`, `VisibilityResponse`**

| Campo | Tipo | Descrição |
|---|---|---|
| `id` | texto | A mensagem. |
| `invisibleUntil` | data | O novo prazo. Com `visibilityTimeout: 0`, o instante atual. |

| Status | Código | Quando |
|---|---|---|
| `200` | — | — |
| `400` | `validation.required`, `validation.out-of-range`, `validation.unknown-field` | Falta `receiptToken` (query) ou `visibilityTimeout` (corpo), ou o tempo está fora da faixa. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `consume`. |
| `404` | `topic.not-found`, `message.not-found` | A mensagem já foi confirmada, venceu por `ttl` ou o tópico foi removido. |
| `409` | `message.stale-receipt` | A mensagem foi reentregue e o token ficou **obsoleto**. Nada muda. |
| `429` / `503` | `rate-limit.exceeded` / `cluster.quorum-unavailable` | — |

### `DELETE /topics/{topic}/messages/{id}`: confirmar (ack)

Confirma o consumo e **remove** a mensagem do tópico. Decide o commit por **quórum** (sempre com o primário; na FIFO, com o primário e o secundário, e o ack grava `deleteAt` em vez de apagar a linha, [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7). Falha antes do quórum **cancela** o ack (rollback) e a mensagem continua disponível; falha numa réplica fora do quórum, já decidido, não cancela ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.4). Se o prazo de invisibilidade venceu mas a mensagem **ainda não foi reentregue**, o token continua válido e o ack vale.

**Parâmetros de query:**

| Parâmetro | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `receiptToken` | texto | sim | O `receiptToken` da última entrega. |

**Resposta de sucesso: `204 No Content`.**

| Status | Código | Quando |
|---|---|---|
| `204` | — | Confirmada e removida. |
| `400` | `validation.required` | Falta `receiptToken`. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden`, `auth.out-of-scope` | Exige `consume`. |
| `404` | `topic.not-found`, `message.not-found` | Inexistente, **inclusive um segundo ack** da mesma mensagem, ou uma mensagem vencida por `ttl`. |
| `409` | `message.stale-receipt`, `topic.wrong-type` | Token obsoleto (a mensagem foi reentregue a outro consumidor); ou tópico distribuidor. |
| `429` / `503` | `rate-limit.exceeded` / `cluster.quorum-unavailable` | Sem quórum, o ack foi cancelado e a mensagem continua disponível. |

## Tokens de acesso

As rotas `/tokens` só existem com a autenticação ligada e só aceitam o **token raiz** (seção 8.1.3). No modo desenvolvimento, ou com a autenticação desligada, respondem `404` `route.not-found`. Criação, alteração e revogação usam o registro + commit de **todos** os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.4).

### `POST /tokens`: criar

**Corpo: `TokenCreateRequest`**

| Campo | Tipo | Obrigatório | Padrão | Descrição |
|---|---|---|---|---|
| `name` | texto (1 a 100, `A-Za-z0-9_-.:`) | sim | — | Nome para a pessoa identificar o token. Não precisa ser único. |
| `permissions` | lista de texto | sim | — | Ao menos uma de `publish`, `consume`, `create`, `admin`. |
| `topicPrefixes` | lista de texto | não | `[]` (todos os tópicos) | Até 20 prefixos de 1 a 100 caracteres (`A-Za-z0-9_-`). O token só alcança tópicos cujo nome **começa com** algum deles. |
| `expiresAt` | data | não | `null` (vale até ser revogado) | Prazo absoluto, no futuro. |

**Resposta de sucesso: `201 Created`, `TokenCreated`**, com `Location: /tokens/{id}`. É o `Token` mais o campo **`token`**, o segredo completo (`tb_<id>.<segredo>`), que **aparece só aqui** e não pode ser recuperado depois.

| Status | Código | Quando |
|---|---|---|
| `201` | — | — |
| `400` | `validation.failed`, `validation.required`, `validation.invalid-name`, `validation.out-of-range` | Permissão desconhecida, lista vazia, prefixo inválido, `expiresAt` no passado. |
| `401` / `403` | `auth.unauthorized` / `auth.forbidden` | Só o token raiz. |
| `404` | `route.not-found` | Autenticação desligada ou modo desenvolvimento. |
| `429` / `503` | `rate-limit.exceeded` / `cluster.registration-failed`, `cluster.commit-timeout` | Com algum nó fora do ar o registro falha; tente de novo. |

### `GET /tokens` e `GET /tokens/{id}`: listar e ler

`GET /tokens` é **paginada** (seção 8.1.2), respondida localmente, e **nunca devolve o segredo**. Parâmetros: `limit`, `cursor`, `sort` (`id` ou `createdAt`; **não** `name`, pois o nome é alterável) e `name` (o nome **contém** o texto). Resposta: `200`, `Page<Token>`. `GET /tokens/{id}` responde `200`, `Token`, ou `404` `token.not-found`.

### `PATCH /tokens/{id}`: alterar

Só `name` e `expiresAt` (`null` remove o prazo). Permissões e prefixos são **imutáveis**: para mudar o alcance, crie outro token e revogue este (`400` `validation.immutable-field`).

**Corpo: `TokenUpdateRequest`**: `name` (texto) e `expiresAt` (data ou `null`), ambos opcionais. Resposta: `200`, `Token`. Erros: `404` `token.not-found`, `400`, `401`/`403`, `503`.

### `DELETE /tokens/{id}`: revogar

Revoga o token: vale em **todos** os nós no commit, e as chamadas seguintes com ele respondem `401`. Resposta: `204`. Erros: `404` `token.not-found`, `401`/`403`, `503`.

## DTOs da API de cliente

### `Topic`

| Campo | Tipo | Presente em | Descrição |
|---|---|---|---|
| `id` | texto | todos | GUID v7 gerado na criação. Recriar o mesmo nome gera outro `id`. |
| `name` | texto | todos | O nome. |
| `type` | texto | todos | `queue` ou `distributor`. |
| `tags` | mapa texto→texto | todos | Tags (`{}` se não há). |
| `createdAt` | data | todos | Instante da criação. |
| `prefixes` | lista de texto | `distributor` | Prefixos de distribuição. |
| `volatile` | booleano | `queue` | Se é fila volátil. |
| `idleTimeout` | inteiro (s) | `queue` com `volatile` | Ociosidade até a exclusão. |
| `fifo` | booleano | `queue` | Se é fila FIFO. |
| `maxMessageBytes` | inteiro ou `null` | `queue` | O valor **definido** no tópico; `null` usa o padrão. |
| `ttl` | inteiro (s) ou `null` | `queue` | Idem. |
| `maxDeliveries` | inteiro ou `null` | `queue` | Idem. |
| `effective` | objeto | `queue` | Os valores **em vigor agora**, com o padrão do cluster aplicado: `maxMessageBytes`, `ttl` e `maxDeliveries`. O padrão vale na hora da conferência, então muda se o cluster mudar. |
| `deadLetter` | booleano | só dead-letter | Sempre `true` quando presente. |
| `source` | texto | só dead-letter | O nome da fila de origem. |

### `Message` (entregue pelo `GET`)

| Campo | Tipo | Presente em | Descrição |
|---|---|---|---|
| `id` | texto | sempre | GUID v7 da mensagem (a ordem do `id` acompanha a da publicação). |
| `body` | qualquer JSON | sempre | O corpo, na forma que o `contentType` manda (seção "O corpo da mensagem"): JSON nativo, string de texto ou string em base64. |
| `contentType` | texto | sempre | O `Content-Type` da publicação (`application/json` se não veio). Imutável. |
| `bodyEncoding` | texto | só binário | `base64`, quando o `body` é a string em base64 dos bytes. **Ausente** nos demais casos. |
| `receiptToken` | texto | sempre | Exigido no ack e na definição da invisibilidade, na query. Muda a **cada entrega**. |
| `deliveryCount` | inteiro | sempre | Quantas vezes a mensagem foi entregue, contando esta (começa em 1). |
| `publishedAt` | data | sempre | Instante da publicação (o do GUID v7). |
| `invisibleUntil` | data | sempre | Até quando a mensagem fica invisível a outros consumidores. |
| `groupId` | texto | se a mensagem tem grupo | O grupo informado na publicação. |
| `deduplicationId` | texto | fila FIFO | A chave de deduplicação (a informada ou o hash do corpo). |
| `deadLetteredAt` | data | fila de dead-letter | Quando a mensagem entrou na dead-letter. |
| `deadLetterReason` | texto | fila de dead-letter | Hoje só `max-deliveries`. |

### `Token`

| Campo | Tipo | Descrição |
|---|---|---|
| `id` | texto | GUID v7. É também o trecho que aparece em logs e traces como `token.id`. |
| `name` | texto | O nome. |
| `permissions` | lista de texto | `publish`, `consume`, `create`, `admin`. |
| `topicPrefixes` | lista de texto | Os prefixos (`[]` se alcança todos os tópicos). |
| `createdAt` | data | Criação. |
| `expiresAt` | data ou `null` | Vencimento, ou `null` se não vence. |

### 8.1.2 Paginação

Toda rota de **cliente ou de operação** que pode devolver um número ilimitado de itens é paginada, sempre com a mesma convenção e o envelope `Page<T>` ([06d](06d-api-convencoes.md), seção 8.3.4):

| Parâmetro | Descrição |
|---|---|
| `limit` | Itens por página. Tem padrão e máximo (a calibrar, [10-configuracao.md](10-configuracao.md)). |
| `cursor` | Token **opaco** devolvido em `nextCursor` pela página anterior. O cliente nunca o monta. |
| `sort` | Campo de ordenação, prefixo `-` para decrescente. Só nas rotas que o aceitam (hoje, `GET /topics` e `GET /tokens`), e só com campos imutáveis. Sem `sort`, a ordem é a do nome. |

A resposta traz os itens e `nextCursor`; sem `nextCursor`, acabou. Não há `page`/`offset` nem `total` (contar custa caro e o valor muda durante a leitura).

**Para não perder nem repetir itens entre páginas**, a paginação é por **chave** (*keyset*), não por deslocamento:

- A ordem é sempre **total e estável**: toda ordenação termina num campo único e imutável como desempate (o nome ou o `id`). `sort=-createdAt` vale, na prática, `(createdAt desc, id asc)`.
- O cursor guarda a **última chave lida**, mais o `sort` e os filtros em uso. A página seguinte pede "depois dessa chave", então inserir ou remover itens antes dela não desloca nada. Cursor com `sort` ou filtros diferentes dos atuais é rejeitado com `400` `validation.invalid-cursor`.
- Repetir um cursor devolve a mesma página enquanto os dados não mudarem, e o cursor **não expira** (não há estado no servidor).

A garantia é a de quem percorre tudo: recebe todo item que existia durante o percurso inteiro, sem repetição. Itens criados ou removidos durante o percurso podem ou não aparecer. É consistência fraca, suficiente para consulta; as rotas entre nós usam outro mecanismo (seção 8.2.1, [06c-api-cluster.md](06c-api-cluster.md)).

Por isso nenhuma rota ordena por valor que muda: `GET /metrics/topics` não tem `sort` e pagina sempre por nome.

### 8.1.3 Autenticação por token

A autenticação da API de cliente é **opcional e liga sozinha quando existe um token raiz**: o arquivo `root-token` na pasta de segredos, `auth.root-token-file` ou `auth.root-token` (veja "Token raiz" abaixo). Não há variável de ligar. Para manter a pasta montada (por causa do TLS do cluster) e ainda assim **não** exigir token, `auth.enabled=false` força a autenticação desligada ([10-configuracao.md](10-configuracao.md)). Sem token raiz, nenhuma rota de cliente exige nada. No **modo desenvolvimento** ela nunca é exigida, e as rotas `/tokens` respondem `404`. Ligada, toda rota da seção 8.1 exige `Authorization: Bearer <token>`. As rotas de operação (seção 8.1.1, [06b-api-operacao.md](06b-api-operacao.md)) e as `/cluster/*` ([06c-api-cluster.md](06c-api-cluster.md)) não usam token: têm proteção própria ([06-api.md](06-api.md), seção 8.0). O token trafega em claro se o balanceador não terminar o TLS, então use HTTPS na frente da API de cliente.

**Token de acesso.** Credencial opaca no formato `tb_<id>.<segredo>`, em que `id` é um GUID v7 e o segredo tem 256 bits aleatórios. O cluster guarda apenas o **SHA-256 do segredo**; o segredo completo só aparece na resposta da criação e não pode ser recuperado depois. O `id` do prefixo permite achar o token direto e identificá-lo em logs e traces **sem expor o segredo** (o token inteiro nunca é registrado). Como cada nó conhece todos os tokens, validar é uma consulta local em memória, sem chamada de rede.

**Permissões** (`permissions`, ao menos uma, combináveis):

| Permissão | Permite |
|---|---|
| `publish` | `POST /topics/{topic}/messages`. |
| `consume` | `GET` em `/topics/{topic}/messages`, `PUT` em `/topics/{topic}/messages/{id}/visibility` (invisibilidade) e `DELETE` em `/topics/{topic}/messages/{id}` (ack). |
| `create` | `POST /topics`, e o `autoCreate` do `GET` (que exige também `consume`). **Só cria**: alterar e remover tópicos é `admin`. |
| `admin` | `PATCH` e `DELETE` em `/topics/{topic}`. **Inclui `create`**, então um token só com `admin` também cria tópicos. |

Todo token válido consulta `GET /topics` e `GET /topics/{topic}` dentro dos seus prefixos. Exemplos de combinação: `publish` + `create` é um produtor que cria as próprias filas, e `consume` + `create`, um consumidor que usa `autoCreate`.

**Prefixos de tópico** (`topicPrefixes`, opcional): o token só alcança tópicos cujo nome **começa com** algum dos prefixos, o mesmo critério do Tópico Distribuidor ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). Sem prefixos, alcança todos. Até 20 prefixos de 1 a 100 caracteres (`A-Za-z0-9_-`).

- `GET /topics` devolve só os tópicos dentro dos prefixos do token, e um tópico fora deles responde `403` `auth.out-of-scope`.
- `POST /topics` com `create` só aceita um nome dentro dos prefixos, e, num Tópico Distribuidor, também cada um dos seus `prefixes` de distribuição: do contrário, um token restrito poderia injetar mensagens em filas fora do seu alcance.
- A **fila de dead-letter** é alcançada pelo mesmo prefixo da fila de origem, pois o nome dela começa com o da fila.
- Na publicação num **Tópico Distribuidor**, a permissão é conferida **só no distribuidor**, e as cópias nas filas destino não são conferidas de novo.

**Token raiz.** `auth.root-token` (ou `auth.root-token-file`) define um segredo, igual em todos os nós, que tem **acesso total**: todas as permissões, sem limite de prefixos, e é o **único** que gerencia os tokens (`/tokens`). Como vem da configuração e não da tabela de tokens, existe desde o primeiro instante, inclusive num nó que acabou de entrar. Por isso **não há um primeiro admin para criar**: o operador começa a trabalhar com o raiz e cria tokens de alcance menor para as aplicações, guardando o raiz num cofre. Nos logs e traces ele aparece como `token.id=root`.

- **Geração:** o `taba init` já cria o arquivo `root-token` na pasta de segredos ([06-api.md](06-api.md), seção 8.0.1); `taba token generate` imprime um segredo aleatório de 256 bits avulso. O token raiz precisa ter **ao menos 32 caracteres**, senão o nó não sobe. O padrão de `auth.root-token-file` é `<secrets.dir>/root-token`.
- **Troca:** o `auth.root-token-file` é relido a cada `auth.reload-interval` e aceita **duas linhas** (a atual e a anterior), para trocar o segredo sem reiniciar nenhum nó. Os tokens já criados seguem valendo, pois o raiz não os assina.
- `auth.enabled=true` sem token raiz impede o nó de subir. Um `auth.root-token-file` definido que não existe também.

**Primeiros passos:**

```
taba init --out ./secrets        # certificados do cluster + root-token
# em todos os nós: montar ./secrets em /etc/taba/secrets (a autenticação liga sozinha)
curl -H "Authorization: Bearer $(cat secrets/root-token)" -H "Content-Type: application/json" \
     -X POST https://taba.exemplo/topics -d '{"name":"pedidos","type":"queue"}'
```

**Criar um token** (a resposta é o `TokenCreated` da seção `POST /tokens`):

```http
POST /tokens
Authorization: Bearer <token raiz>
Content-Type: application/json

{
  "name": "servico-pedidos",
  "permissions": ["publish", "create"],
  "topicPrefixes": ["pedidos"],
  "expiresAt": "2027-06-01T00:00:00.000Z"
}
```

```http
201 Created
Location: /tokens/01927f3a-0000-7000-8000-000000000001

{
  "id": "01927f3a-0000-7000-8000-000000000001",
  "token": "tb_01927f3a-0000-7000-8000-000000000001.<segredo>",
  "name": "servico-pedidos",
  "permissions": ["publish", "create"],
  "topicPrefixes": ["pedidos"],
  "createdAt": "2026-10-06T12:00:00.000Z",
  "expiresAt": "2027-06-01T00:00:00.000Z"
}
```

**Validade.** `expiresAt` é opcional e absoluto; sem ele, o token vale até ser revogado. Cada nó ignora o token vencido e o apaga na varredura local (`retention.sweep.interval`), sem propagar nada, como no `ttl` das mensagens (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Depende do relógio dos nós (NTP).

**Respostas de autenticação.**

| Situação | Resposta |
|---|---|
| Sem token, mal formado, desconhecido, revogado ou vencido | `401` `auth.unauthorized`, com `WWW-Authenticate: Bearer`. A resposta não diz qual dos casos. |
| Token válido sem a permissão da rota | `403` `auth.forbidden`. |
| Token válido, mas o tópico está fora dos prefixos | `403` `auth.out-of-scope`. |

**Criação e revogação** usam o mesmo registro + commit de **todos** os nós da definição de tópico (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)): com algum nó fora do ar, a operação falha e o cliente tenta de novo. A revogação vale em todos os nós no commit. A conferência acontece no nó que recebe a chamada e, numa chamada repassada (seção 6.9 de [04-cluster.md](04-cluster.md)), de novo no destino, pois o cabeçalho `Authorization` segue como veio.
