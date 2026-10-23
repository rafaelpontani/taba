---
título: As três APIs — visão geral e convenções
resumo: As três APIs HTTP do nó (cliente, operação, cluster), quem usa cada uma, e as convenções comuns — formato, cabeçalhos, erro, paginação, status e repetição.
tipo: referência
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/http/mod.rs, crates/taba-node/src/http/common, crates/taba-node/src/core/error, crates/taba-node/src/core/pagination, crates/taba-node/src/core/time, crates/taba-node/src/core/id]
---

# As três APIs: visão geral e convenções

> Leia antes: [04-arquitetura.md](04-arquitetura.md) (as três portas). Depois: [16-api-cliente.md](16-api-cliente.md), [17-api-operacao.md](17-api-operacao.md), [18-api-cluster.md](18-api-cluster.md) e o catálogo de erros em [19-erros-e-limites.md](19-erros-e-limites.md).

> **Estado do contrato:** ainda **não há contrato OpenAPI** nem prefixo de versão nos caminhos (`/topics`, não `/v1/topics`). Este documento e os três seguintes são hoje a especificação. O `taba-cli version` mostra uma "versão do contrato" (1). ⬜ Contrato OpenAPI: [32-roadmap.md](32-roadmap.md).

## Quem usa o quê

| API | Porta (padrão) | Quem usa | Autenticação | Documento |
|---|---|---|---|---|
| **Cliente** | `api.port` (8080) | Aplicações (produtores e consumidores) | Token opcional | [16](16-api-cliente.md) |
| **Operação** | `operations.port` (ou a porta de cliente) | Operador, Prometheus, orquestrador | Nenhuma (rede) | [17](17-api-operacao.md) |
| **Cluster** | `cluster.port` (8081) | **Só os outros nós** | TLS mútuo | [18](18-api-cluster.md) |

Cada API tem **o seu público** e a rede restringe cada porta. A porta de cliente responde `404 route.not-found` a `/cluster/*`; a do cluster, a tudo que não seja `/cluster/*`.

![As três APIs do nó e suas rotas](../diagrams/img/pt-BR/node-ports.svg)

## Resumo das rotas

**Cliente** ([16](16-api-cliente.md)):

| Verbo | Rota | Permissão do token | Sucesso |
|---|---|---|---|
| `POST` | `/topics` | `create` | `201` |
| `GET` | `/topics` | qualquer token válido | `200` (paginada) |
| `GET` | `/topics/{topic}` | qualquer token válido | `200` |
| `PATCH` | `/topics/{topic}` | `manage` | `200` |
| `DELETE` | `/topics/{topic}` | `manage` | `204` |
| `POST` | `/topics/{topic}/messages` | `publish` | `201` (`200` em duplicata FIFO e em distribuidor) |
| `GET` | `/topics/{topic}/messages` | `consume` (+ `create` no `autoCreate`) | `200` |
| `PUT` | `/topics/{topic}/messages/{id}/visibility` | `consume` | `200` |
| `DELETE` | `/topics/{topic}/messages/{id}` | `consume` | `204` |
| `POST`, `GET`, `PATCH`, `DELETE` | `/tokens`, `/tokens/{id}` | só o token raiz | `201`, `200`, `200`, `204` |

**Operação** ([17](17-api-operacao.md)): `GET /health/live`, `GET /health/ready`, `GET /health/cluster`, `GET /metrics`, `GET /metrics/topics`, `GET /metrics/topics/{topic}`, `GET /metrics/topics/{topic}/groups`, `POST`/`GET`/`DELETE /node/drain`.

**Cluster** ([18](18-api-cluster.md)): `/cluster/*`, em seis grupos.

## Formato dos dados

| Item | Regra |
|---|---|
| Formato | JSON em UTF-8 (`Content-Type: application/json`), exceto: `/metrics` (texto do Prometheus), os streams do cluster (`application/x-ndjson`) e o **corpo** da publicação de mensagem (qualquer media type) |
| Nomes de campos | `camelCase`. Enumerações em minúsculas, com hífen quando compostas (`queue`, `max-deliveries`) |
| Datas | RFC 3339 em UTC com milissegundos: `2026-10-13T17:00:48.527Z`. Campos de data terminam em `At` (`createdAt`) ou são um prazo absoluto com nome próprio (`invisibleUntil`, `expiresAt`) |
| Durações | **Inteiro de segundos**, sem unidade no valor (`ttl`, `visibilityTimeout`, `idleTimeout`, `waitTime`) |
| Tamanhos | Inteiros, em bytes |
| Identificadores | **GUID v7** em minúsculas, forma canônica com hífens. Vale para tópico, mensagem, token e nó. Ordenável no tempo |
| Nome de tópico | `^[A-Za-z0-9_-]{1,100}$`, **sensível a maiúsculas** |
| Ausente × `null` | **Ausente** = não se aplica ao recurso (ex.: `prefixes` numa fila). **`null`** = aplica-se e está sem valor próprio (ex.: `ttl: null` = "usa o padrão do nó") |
| Campo desconhecido na requisição | `400 validation.unknown-field` (um erro de digitação não passa em silêncio). **Na resposta, o cliente deve ignorar campos que não conhece** |
| Campo obrigatório ausente | `400 validation.required`, com o campo em `details` |
| Corpo vazio | `POST`, `PATCH` e `DELETE` que não esperam corpo o ignoram; `204` nunca tem corpo |

## Cabeçalhos

**Na requisição:**

| Cabeçalho | Descrição |
|---|---|
| `Authorization: Bearer <token>` | API de cliente com a autenticação ligada |
| `Content-Type` | `application/json` nas rotas com corpo JSON (outro valor: `415 media.unsupported`). **Exceção:** a publicação de mensagem aceita qualquer media type, que vira o `contentType` da mensagem |
| `Accept` | `application/json` (padrão). No `/metrics`, `application/openmetrics-text` pede OpenMetrics |
| `traceparent` | Contexto de trace W3C; as operações do nó entram no mesmo trace |
| `X-Request-Id` | Opcional (1 a 128 caracteres `A-Za-z0-9_-.:`); sem ele, o nó gera um GUID v7 |

**Na resposta:**

| Cabeçalho | Quando |
|---|---|
| `X-Request-Id` | **Sempre** (o recebido ou o gerado); é o `requestId` do corpo de erro e liga a chamada aos logs e traces |
| `Retry-After` | `429`, `503` e `403` por freezing: segundos até valer a pena tentar de novo. **É a única forma** de informar o tempo |
| `WWW-Authenticate: Bearer` | `401` |
| `Location` | `201` |

## Erros

Toda resposta de erro (`4xx`, `5xx`) de rota JSON tem o formato `ErrorResponse`. O **status vai só na linha de status do HTTP**, o **tempo de espera só no `Retry-After`**.

```json
{
  "code": "topic.not-found",
  "message": "O tópico não existe.",
  "requestId": "01a13635-c00c-7a83-a4ad-5a194859cf74"
}
```

| Campo | Descrição |
|---|---|
| `code` | Código **estável** `grupo.motivo` (catálogo em [19](19-erros-e-limites.md)). É o que o programa do cliente deve testar |
| `message` | Texto para uma pessoa, em português; **pode mudar** e não deve entrar em lógica |
| `requestId` | O mesmo de `X-Request-Id` |
| `details` | Opcional, nos erros de validação: lista de `{ field, code, message }` (ex.: `field: "query.limit"`) |

Exemplo de validação (`limit=2000`):

```json
{"code":"validation.out-of-range","message":"limit deve estar entre 1 e 1000.","requestId":"…",
 "details":[{"field":"query.limit","code":"validation.out-of-range","message":"Entre 1 e 1000."}]}
```

**Exceções:** os health checks (`/health/live`, `/health/ready`) devolvem o próprio `HealthResponse`; uma falha **depois do `200`** num stream NDJSON usa a linha `{"error":"..."}`; num resultado parcial `200` (publicação num distribuidor), o item que falhou leva `error: { code, message }` **sem status**.

## Paginação

`GET /topics`, `GET /tokens`, `GET /metrics/topics` e `GET /metrics/topics/{topic}/groups` (sem `top`) respondem com:

```json
{ "items": [ … ], "nextCursor": "eyJrIjoi…" }
```

| Parâmetro | Descrição |
|---|---|
| `limit` | Itens por página: padrão **100** (`api.pagination.default-limit`), máximo **1000** (`api.pagination.max-limit`) |
| `cursor` | O `nextCursor` anterior, **opaco** (o cliente nunca o monta) |
| `sort` | Só onde a rota o aceita (`GET /topics`, `GET /tokens`), só com campos imutáveis; prefixo `-` para decrescente. Sem `sort`, a ordem é a do nome |

- `nextCursor` **ausente** = última página. **Não há `page`, `offset` nem `total`.**
- Paginação por **chave** (*keyset*): a ordem é total e estável (todo `sort` termina num campo único e imutável), o cursor guarda a última chave lida mais o `sort` e os filtros, e **não expira** (não há estado no servidor). Cursor com `sort` ou filtros diferentes do atual: `400 validation.invalid-cursor`.
- A garantia é de **consistência fraca**: quem percorre tudo recebe todo item que existia durante o percurso, sem repetição; itens criados ou removidos durante ele podem ou não aparecer.

## Códigos de status

| Status | Significado | Repetir? |
|---|---|---|
| `200` | Deu certo e há resultado | — |
| `201` | Recurso criado (com `Location`) | — |
| `202` | Aceito e **continua em segundo plano** (drain, volta do drain) | — |
| `204` | Deu certo, sem corpo (exclusão, ack) | — |
| `400` | Parâmetro, campo ou corpo inválido | **Não**, sem corrigir |
| `401` | Token ausente, inválido, revogado ou vencido | Não |
| `403` | Sem a permissão, tópico fora dos prefixos, IP fora da lista | Não |
| `404` | Tópico, mensagem ou token não existe, ou a rota não existe naquela porta | Não |
| `409` | O estado atual impede (nome já existe, recibo obsoleto, tipo errado, dead-letter somente leitura) | Não |
| `413` | Corpo maior que o limite do tópico | Não |
| `415` | `Content-Type` errado numa rota com corpo JSON | Não |
| `421` | **Só entre nós** (publicação FIFO no nó errado; nó errado no endereço) | — |
| `422` | O pedido é válido, mas o cluster não o atende nesta configuração (FIFO com fator < 2, teto de destinos, teto de filas voláteis) | Não |
| `429` | Rate limit | **Sim**, depois de `Retry-After` |
| `500` | Falha inesperada do nó | — |
| `502`, `504` | O **repasse** a outro nó falhou depois de enviado o corpo; **não foi repetido** | O cliente decide |
| `503` | O cluster não pôde fechar a operação **agora** (sem quórum, em sincronização, em drain) | **Sim**, depois de `Retry-After` |

> **Regra de ouro:** `5xx` e `429` podem ser repetidos depois de `Retry-After`; `4xx` (exceto `429`) não. Num `503` ou `504` de uma operação que **cria ou apaga** (tópico, token), o resultado pode já ter sido aplicado em parte do cluster: consulte o recurso antes de repetir, ou trate `409 topic.already-exists` e `404` na repetição como sucesso. Repetir uma **publicação** (fora da fila FIFO) **pode duplicar** a mensagem.

## O que está de fora

Não há versionamento nos caminhos, nem CORS ou compressão documentados. **O nó só termina TLS na porta do cluster**: o HTTPS da API de **cliente** (e da de operação) é do balanceador.

## Fontes e provas

- `http/mod.rs` (as três portas e camadas), `http/common` (`X-Request-Id`, `ErrorResponse`, `404`, `500`), `core/error` (catálogo e status fixos), `core/pagination` (cursor opaco), `core/id` (GUID v7), `core/time` (RFC 3339).
- Exemplos de resposta copiados de um nó real.
- Casos de uso: UC-CNV (convenções), UC-PAG (paginação), UC-TOP; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/topics.rs`, `messages.rs`.
- Histórico: `06-api.md`, `06d-api-convencoes.md`, `06a-api-cliente.md` (8.1.2).
