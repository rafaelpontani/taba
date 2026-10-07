# Convenções e DTOs padronizados

[← Índice](README.md) · [Visão geral das APIs](06-api.md)

## 8.3 Convenções comuns às três APIs

Este arquivo define o que **todas** as rotas das três APIs ([cliente](06a-api-cliente.md), [operação](06b-api-operacao.md) e [cluster](06c-api-cluster.md)) compartilham: o formato dos dados, os cabeçalhos, o DTO de erro, o envelope de paginação, os códigos de status e o catálogo de códigos de erro. Os arquivos de cada API só descrevem o que é específico da rota e apontam para cá.

> Como no resto da documentação, os nomes de campos e de códigos são **propostas** até o contrato OpenAPI ser fechado (passo 1 de [08c-proximos-passos.md](08c-proximos-passos.md)). Não há prefixo de versão nos caminhos por ora; a política de versionamento entra com o contrato.

### 8.3.1 Formato dos dados

| Item | Regra |
|---|---|
| **Formato** | JSON em UTF-8. Requisição e resposta usam `Content-Type: application/json`, exceto onde a rota diz o contrário (`/metrics` em texto do Prometheus; os streams do cluster em `application/x-ndjson`). |
| **Nomes de campos** | `camelCase`. Valores de enumeração em minúsculas, com hífen quando compostos (`queue`, `max-deliveries`). |
| **Datas e horas** | RFC 3339, em UTC, com milissegundos: `2026-10-06T12:00:00.000Z`. Todo campo de data termina em `At` (`createdAt`) ou é um prazo absoluto com nome próprio (`invisibleUntil`, `expiresAt`). |
| **Durações** | Número **inteiro de segundos**, sem unidade no valor (`ttl`, `visibilityTimeout`, `idleTimeout`, `waitTime`). Latências e idades em métricas são números reais em segundos. |
| **Tamanhos** | Inteiros, em bytes (`maxMessageBytes`, `bytes`). |
| **Identificadores** | GUID v7 em minúsculas, na forma canônica com hífens (`01927f3a-7c1e-7d2b-9f10-3a5b6c7d8e9f`). O mesmo vale para `id` de tópico, mensagem, token e nó. |
| **Nome de tópico** | `^[A-Za-z0-9_-]{1,100}$`, diferenciando maiúsculas de minúsculas ([06a](06a-api-cliente.md)). |
| **Ausente × `null`** | Campo **ausente** na resposta: não se aplica àquele recurso (por exemplo, `prefixes` num tópico `queue`). Campo **`null`**: aplica-se e está sem valor próprio (por exemplo, `ttl: null` significa "usa o padrão do cluster"). |
| **Campos desconhecidos** | Uma requisição com campo que a rota não define é recusada com `400` `validation.unknown-field`, para um erro de digitação não passar em silêncio. Na resposta, o cliente deve **ignorar** campos que não conhece (o servidor pode acrescentar campos). |
| **Campo obrigatório ausente** | `400` `validation.required`, com o campo em `details`. |
| **Corpo vazio** | `POST`, `PATCH` e `DELETE` que não esperam corpo ignoram o que vier; `204` nunca tem corpo. |

### 8.3.2 Cabeçalhos

**Na requisição:**

| Cabeçalho | Quando | Descrição |
|---|---|---|
| `Authorization: Bearer <token>` | API de cliente com a autenticação ligada ([06a](06a-api-cliente.md), seção 8.1.3) | Token de acesso. Não vale nas APIs de operação e do cluster. |
| `Content-Type` | Requisição com corpo | `application/json`. Outro valor responde `415`. **Exceção:** `POST /topics/{topic}/messages` aceita qualquer media type, que vira o `contentType` da mensagem ([06a](06a-api-cliente.md), "O corpo da mensagem"). |
| `Accept` | Opcional | `application/json` (padrão). No `/metrics`, `application/openmetrics-text` pede OpenMetrics. |
| `traceparent` | Opcional | Contexto de trace W3C. Se vier, as operações do nó entram no mesmo trace ([11-observabilidade.md](11-observabilidade.md), seção "Traces"). |
| `X-Request-Id` | Opcional | Identificador da requisição escolhido pelo cliente (1 a 128 caracteres, `A-Za-z0-9_-.:`). Sem ele, o nó gera um GUID v7. |

**Na resposta:**

| Cabeçalho | Quando | Descrição |
|---|---|---|
| `X-Request-Id` | **Sempre** | O valor recebido ou o gerado. É o mesmo `requestId` do corpo de erro e o atributo que liga a chamada aos logs e traces do nó. |
| `Content-Type` | Quando há corpo | `application/json` (ou o do stream ou do `/metrics`). |
| `Retry-After` | `429`, `503` e `403` por freezing | Segundos até valer a pena tentar de novo. É a **única** forma de informar esse tempo: o corpo não o repete. |
| `WWW-Authenticate: Bearer` | `401` | Indica que a rota espera um token. |
| `Location` | `201` | Caminho do recurso criado (por exemplo, `/topics/pedidos`). |

### 8.3.3 DTO de erro: `ErrorResponse`

Toda resposta de erro (`4xx` e `5xx`) de uma rota JSON tem este corpo. Há duas exceções: os endpoints de saúde (`/health/*`, [06b](06b-api-operacao.md)) devolvem o seu próprio DTO, e uma falha **depois do `200`** de um stream NDJSON usa a linha `{"error":"..."}` ([06c](06c-api-cluster.md), seção 8.2.1).

O **código de status vai só na linha de status do HTTP**, nunca no corpo, e o tempo de espera vai só no cabeçalho `Retry-After`: o DTO não repete o que o protocolo já diz. Cada `code` do catálogo (seção 8.3.6) tem um status HTTP fixo.

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `code` | texto | sim | Código **estável** do erro (catálogo da seção 8.3.6), no formato `grupo.motivo`. É o que o programa do cliente deve testar, junto com o status HTTP. |
| `message` | texto | sim | Explicação para uma pessoa. Pode mudar entre versões e **não deve ser usada em lógica**. |
| `requestId` | texto | sim | O mesmo valor do cabeçalho `X-Request-Id`, para achar a chamada nos logs e traces. |
| `details` | lista de `ErrorDetail` | não | Presente nos erros de validação e quando há mais de um problema. |

`ErrorDetail`:

| Campo | Tipo | Descrição |
|---|---|---|
| `field` | texto | O campo, ou o parâmetro de query, com o problema (`name`, `tags.ambiente`, `groupId`, `query.maxMessages`). |
| `code` | texto | Código do problema, do mesmo catálogo. |
| `message` | texto | Explicação para uma pessoa. |

Exemplo (`400 Bad Request`):

```json
{
  "code": "validation.failed",
  "message": "A requisição tem campos inválidos.",
  "requestId": "01927f3a-7c1e-7d2b-9f10-3a5b6c7d8e9f",
  "details": [
    { "field": "name", "code": "validation.invalid-name", "message": "Use de 1 a 100 caracteres entre A-Za-z0-9_-." },
    { "field": "ttl", "code": "validation.out-of-range", "message": "ttl deve estar entre 60 e 31536000 segundos." }
  ]
}
```

Exemplo (`503 Service Unavailable`, com o tempo de espera no cabeçalho):

```http
HTTP/1.1 503 Service Unavailable
Retry-After: 5
X-Request-Id: 01927f3b-0a42-7c3d-8e21-4b6c7d8e9fa0
Content-Type: application/json

{
  "code": "cluster.quorum-unavailable",
  "message": "Não há nós suficientes para fechar o quórum de escrita.",
  "requestId": "01927f3b-0a42-7c3d-8e21-4b6c7d8e9fa0"
}
```

**Erro dentro de um resultado parcial.** Quando uma resposta `200` traz itens que falharam (a publicação num distribuidor, [06a](06a-api-cliente.md)), cada item com problema leva um objeto `error` com `code` e `message`, **sem status**: o status do item é o do `code` no catálogo.

### 8.3.4 Envelope de paginação: `Page<T>`

As rotas que devolvem listas ilimitadas (`GET /topics`, `GET /tokens`, `GET /metrics/topics` e `GET /metrics/topics/{topic}/groups` sem `top`) respondem com:

| Campo | Tipo | Obrigatório | Descrição |
|---|---|---|---|
| `items` | lista de `T` | sim | Os itens da página, na ordem total e estável da rota. Vazia se não há nada. |
| `nextCursor` | texto | não | Cursor **opaco** da página seguinte. **Ausente na última página.** |

Parâmetros de query comuns: `limit` (inteiro, padrão e máximo em [10-configuracao.md](10-configuracao.md)), `cursor` (o `nextCursor` anterior) e, onde a rota o aceita, `sort`. `limit` fora da faixa, `cursor` malformado, e `cursor` usado com `sort` ou filtros diferentes dos que o geraram, respondem `400` (`validation.out-of-range` e `validation.invalid-cursor`). As regras de consistência estão na seção 8.1.2 ([06a](06a-api-cliente.md)).

### 8.3.5 Códigos de status

| Status | Significado no Taba | Corpo |
|---|---|---|
| `200 OK` | A operação deu certo e há um resultado a devolver. | DTO da rota. |
| `201 Created` | Um recurso foi criado. Traz `Location`. | DTO do recurso. |
| `202 Accepted` | A operação foi aceita e **continua em segundo plano** (drain, volta do drain). | DTO de estado. |
| `204 No Content` | A operação deu certo e não há o que devolver (exclusão, ack). | Nenhum. |
| `400 Bad Request` | Parâmetro, campo ou corpo inválido. **Não repita sem corrigir.** | `ErrorResponse` com `details`. |
| `401 Unauthorized` | Token ausente, inválido, revogado ou vencido. | `ErrorResponse`, com `WWW-Authenticate`. |
| `403 Forbidden` | Token válido sem a permissão da rota, ou tópico fora dos prefixos do token. Também, nas rotas de operação, IP fora de `operations.allowed-ips`. | `ErrorResponse`. |
| `404 Not Found` | O tópico, a mensagem ou o token não existe, ou a rota não existe naquela porta. | `ErrorResponse`. |
| `409 Conflict` | O estado atual do recurso impede a operação (nome já existe, `receiptToken` obsoleto, tópico de tipo errado, dead-letter somente leitura). | `ErrorResponse`. |
| `413 Content Too Large` | Corpo da mensagem acima do limite do tópico. | `ErrorResponse`. |
| `415 Unsupported Media Type` | `Content-Type` diferente de `application/json` numa rota com corpo JSON (todas, menos a publicação de mensagem). | `ErrorResponse`. |
| `421 Misdirected Request` | **Só entre nós:** o nó que recebeu uma publicação FIFO já não é o primário do grupo. Nunca chega ao cliente ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.4). | `ErrorResponse`. |
| `422 Unprocessable Content` | O pedido é válido, mas o cluster não pode atendê-lo nesta configuração (fila FIFO com `replication.factor` abaixo de 2, teto de destinos ou de filas voláteis). | `ErrorResponse`. |
| `429 Too Many Requests` | Rate limit do token, ou do IP do cliente quando não há token válido, excedido ([12-limites-api.md](12-limites-api.md)). | `ErrorResponse`, com `Retry-After`. |
| `500 Internal Server Error` | Falha inesperada do nó. | `ErrorResponse`. |
| `502 Bad Gateway` / `504 Gateway Timeout` | Um repasse a outro nó falhou depois de o corpo ter sido enviado, e a chamada **não foi repetida** (seção 6.9 de [04-cluster.md](04-cluster.md)). O cliente decide se repete. | `ErrorResponse`. |
| `503 Service Unavailable` | O cluster não pôde fechar a operação **agora** (sem quórum, nó em sincronização ou em drain, registro de tópico sem confirmação). Tente de novo depois de `Retry-After`. | `ErrorResponse`, com `Retry-After`. |

**Quando repetir.** `5xx` e `429` podem ser repetidos depois de `Retry-After`. `4xx` (exceto `429`) não: o pedido precisa mudar. Num `503` ou `504` de uma operação que **cria ou apaga** (tópico, token), o resultado pode já ter sido aplicado em parte do cluster: consulte o recurso antes de repetir, ou trate `409` `topic.already-exists` e `404` na repetição como sucesso.

### 8.3.6 Catálogo de códigos de erro

Os códigos são estáveis dentro de uma versão do contrato. Uma coluna mostra a rota onde o erro costuma aparecer.

**Validação (`400`):**

| Código | Quando |
|---|---|
| `validation.failed` | Erro genérico de validação; os problemas estão em `details`. |
| `validation.required` | Campo ou parâmetro obrigatório ausente. |
| `validation.unknown-field` | Campo que a rota não define. |
| `validation.out-of-range` | Número fora da faixa (`ttl`, `maxDeliveries`, `idleTimeout`, `waitTime`, `maxMessages`, `limit`, `visibilityTimeout`). |
| `validation.invalid-name` | Nome de tópico fora do padrão, ou nome de token fora do padrão. |
| `validation.reserved-name` | Nome terminado no sufixo de dead-letter (`-dead-letter`), na criação e no `autoCreate`. |
| `validation.immutable-field` | `PATCH` que tenta mudar nome, `id`, tipo, `volatile`, `fifo` ou, num token, permissões e prefixos. |
| `validation.not-applicable` | Campo que não vale para o tipo do tópico (`prefixes` numa fila, `ttl` num distribuidor, `deduplicationId` fora da FIFO, `fifo` num distribuidor). |
| `validation.invalid-body` | O corpo de uma mensagem não confere com o `Content-Type`: JSON inválido em `application/json`, ou texto que não é UTF-8 em `text/*`. |
| `validation.invalid-content-type` | O `Content-Type` da publicação de uma mensagem não é um media type válido (1 a 255 caracteres). |
| `validation.group-id-required` | Publicação numa fila FIFO sem `groupId`. |
| `validation.invalid-cursor` | `cursor` malformado, ou usado com `sort` ou filtros diferentes dos que o geraram. |
| `validation.max-bytes-above-hard-limit` | `maxMessageBytes` acima de `message.max-bytes.hard-limit`. |

**Autenticação e autorização (`401` e `403`):**

| Código | Status | Quando |
|---|---|---|
| `auth.unauthorized` | `401` | Sem token, mal formado, desconhecido, revogado ou vencido. A resposta não diz qual. |
| `auth.forbidden` | `403` | Token válido sem a permissão da rota (inclui as rotas `/tokens`, só do token raiz). |
| `auth.out-of-scope` | `403` | O tópico, ou um dos `prefixes` de um distribuidor, está fora dos prefixos do token. |
| `ops.ip-not-allowed` | `403` | Rota de operação chamada de um IP fora de `operations.allowed-ips`. |
| `cluster.not-member` | `403` | Rota do cluster chamada por quem não consta na tabela de membros. |
| `cluster.node-frozen` | `403` | Pedido de ingresso de um nó em período de freezing. Traz `Retry-After`. |

**Recurso não encontrado (`404`):**

| Código | Quando |
|---|---|
| `topic.not-found` | O tópico não existe (e não há `autoCreate`). |
| `message.not-found` | A mensagem não existe: já confirmada, vencida por `ttl` ou removida com o tópico. |
| `token.not-found` | O token não existe ou já foi revogado. |
| `route.not-found` | Caminho que não existe naquela porta (por exemplo, `/cluster/*` na porta de cliente, ou `/tokens` com a autenticação desligada). |

**Conflito (`409`):**

| Código | Quando |
|---|---|
| `topic.already-exists` | Já existe um tópico com o nome. |
| `topic.wrong-type` | `GET` ou ack de mensagem num distribuidor; ou `autoCreate` num nome que pertence a um distribuidor. |
| `topic.read-only` | `PATCH` ou `DELETE` numa fila de dead-letter. |
| `message.stale-receipt` | O `receiptToken` não é o da última entrega (a mensagem foi reentregue). |
| `drain.conflict` | `DELETE /node/drain` durante o desregistro, ou com o processo já parando por sinal. |
| `cluster.conflict` | Entre nós: já há outra operação pendente para o mesmo recurso, ou a mensagem já está invisível. |

**Limites e configuração (`413`, `415` e `422`):**

| Código | Status | Quando |
|---|---|---|
| `message.too-large` | `413` | Corpo acima de `maxMessageBytes` do tópico (ou de `message.max-bytes`). |
| `media.unsupported` | `415` | `Content-Type` diferente de `application/json` numa rota com corpo JSON (não vale na publicação de mensagem). |
| `topic.fifo-needs-replication` | `422` | Criar fila FIFO com `replication.factor` abaixo de 2. |
| `distributor.too-many-targets` | `422` | A publicação casaria mais filas que `topics.distributor.max-targets`. Nenhuma cópia é feita. |
| `topic.volatile-limit` | `422` | O `autoCreate` esbarra em `topics.volatile.max-count`. |
| `cluster.join-unreachable` | `422` | Entre nós: o nó que coordena o ingresso não conseguiu conectar de volta ao nó novo. |
| `rate-limit.exceeded` | `429` | Rate limit do token (ou do IP, sem token válido) excedido. |

**Disponibilidade e falhas (`5xx`):**

| Código | Status | Quando |
|---|---|---|
| `internal.error` | `500` | Falha inesperada. |
| `forward.bad-gateway` | `502` | O repasse a outro nó falhou depois de enviado o corpo. |
| `forward.timeout` | `504` | O repasse a outro nó estourou o tempo depois de enviado o corpo. |
| `cluster.quorum-unavailable` | `503` | Sem nós suficientes para fechar o quórum de escrita, de entrega ou de ack. |
| `cluster.registration-failed` | `503` | Um registro (tópico, token, nó) não foi confirmado por todos os nós e foi cancelado (rollback). Nada foi aplicado. |
| `cluster.commit-timeout` | `503` | O registro foi confirmado, mas o commit não chegou a todos os nós dentro de `write.commit.timeout`. **A operação continua em segundo plano e vai se completar.** |
| `cluster.node-syncing` | `503` | O nó está sincronizando e não achou outro nó para repassar. |
| `cluster.node-draining` | `503` | O nó está em drain e não achou outro nó para repassar (no modo desenvolvimento, sempre). |
| `fifo.primary-unavailable` | `503` | O primário do grupo FIFO está fora do ar, sincronizando, ou esgotou `fifo.forward.max-attempts`. |
| `delivery.unavailable` | `503` | O `GET` não capturou nenhuma mensagem e houve falhas no registro de invisibilidade (nunca devolve vazio nesse caso). |
| `fifo.misdirected` | `421` | Entre nós: o nó não é o primário do grupo. |

**Códigos só do cluster** (respostas das rotas `/cluster/*`): `cluster.not-member`, `cluster.node-frozen`, `cluster.join-unreachable`, `cluster.conflict`, `fifo.misdirected` e `cluster.operation-unknown` (`404`: commit ou rollback de uma operação que o nó não conhece, por já ter expirado).

### 8.3.7 DTOs reutilizados

Estes DTOs aparecem em mais de uma API e são definidos aqui uma vez.

**`Ack`** (resposta de `register`, `commit` e `rollback` entre nós):

| Campo | Tipo | Descrição |
|---|---|---|
| `operationId` | texto | O mesmo `operationId` da requisição. |
| `accepted` | booleano | `true` quando o nó gravou (`register`), efetivou (`commit`) ou descartou (`rollback`). |

**`StreamEnd`** (linha final de todo stream NDJSON, [06c](06c-api-cluster.md), seção 8.2.1):

| Campo | Tipo | Descrição |
|---|---|---|
| `end` | booleano | Sempre `true`. |
| `count` | inteiro | Linhas de dados enviadas neste stream. |
| `last` | texto | O último ID enviado (ausente se `count` é 0). |
| `hasMore` | booleano | `true` quando o emissor fechou no teto e há mais a pedir. |

**`ConditionEntry`** (condição do cluster ou de um tópico, [11-observabilidade.md](11-observabilidade.md)):

| Campo | Tipo | Descrição |
|---|---|---|
| `condition` | texto | Nome da condição (`under-replicated`, `backlog`, `noisy-group`...). |
| `severity` | texto | `info`, `warning` ou `critical`. |
