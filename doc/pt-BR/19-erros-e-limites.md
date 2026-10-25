---
título: Erros e limites
resumo: O catálogo completo de códigos de erro (cada um com o status HTTP fixo), os limites fixos de validação, os limites configuráveis e os rate limits.
tipo: referência
status: estável
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/src/core/error/mod.rs, crates/taba-node/src/core/validation/mod.rs, crates/taba-node/src/http/ratelimit, crates/taba-node/src/config/params/mod.rs]
---

# Erros e limites

> Formato do erro e política de repetição: [15-api-visao-geral.md](15-api-visao-geral.md). Os valores configuráveis: [22-configuracao.md](22-configuracao.md).

## Regra do catálogo

Todo erro de API é um **`ErrorCode`** do catálogo (`core/error`), com **status HTTP fixo**: o mesmo código sempre vem com o mesmo status. O catálogo tem **53 códigos**. Código novo exige a linha neste documento **e** o caso na tabela de teste (regra de [24-convencoes.md](24-convencoes.md)). Os códigos são estáveis; a `message` **não** (não a use em lógica).

## Validação — `400`

| Código | Quando |
|---|---|
| `validation.failed` | Erro genérico; os problemas vêm em `details` |
| `validation.required` | Campo ou parâmetro obrigatório ausente |
| `validation.unknown-field` | Campo que a rota não define |
| `validation.out-of-range` | Número fora da faixa (`ttl`, `maxDeliveries`, `idleTimeout`, `waitTime`, `maxMessages`, `limit`, `visibilityTimeout`) |
| `validation.invalid-name` | Nome de tópico (ou de token) fora do padrão |
| `validation.reserved-name` | Nome terminado em `-dead-letter`, na criação e no `autoCreate` |
| `validation.immutable-field` | `PATCH` tenta mudar `name`, `id`, `type`, `volatile`, `fifo` (ou, num token, permissões e prefixos) |
| `validation.not-applicable` | Campo que não vale para o tipo (`prefixes` numa fila, `ttl` num distribuidor, `deduplicationId` fora da FIFO, `fifo` num distribuidor) |
| `validation.invalid-body` | O corpo não confere com o `Content-Type` (JSON inválido, texto que não é UTF-8) |
| `validation.invalid-content-type` | O `Content-Type` da publicação não é um media type válido |
| `validation.group-id-required` | Publicação numa fila FIFO sem `groupId` |
| `validation.invalid-cursor` | `cursor` malformado, ou usado com `sort`/filtros diferentes dos que o geraram |
| `validation.max-bytes-above-hard-limit` | `maxMessageBytes` acima de `message.hard-limit` |

## Autenticação e autorização — `401`, `403`

| Código | Status | Quando |
|---|---|---|
| `auth.unauthorized` | 401 | Sem token, mal formado, desconhecido, revogado ou vencido. **A resposta não diz qual** |
| `auth.forbidden` | 403 | Token válido sem a permissão da rota (inclui `/tokens`, o cluster e os nós: `admin`; parar o cluster e os tokens `admin`: só o raiz). O corpo traz **`required`**: `root`, ou o nome da permissão que faltou (`manage`, `admin`, `publish`…), para o cliente poder dizer o que o token precisa |
| `auth.out-of-scope` | 403 | O tópico (ou um `prefix` de distribuidor) está fora dos prefixos do token |
| `ops.ip-not-allowed` | 403 | Rota de operação chamada de um IP fora de `operations.allowed-ips` |
| `cluster.not-member` | 403 | Rota do cluster chamada por quem não consta na tabela de membros |
| `cluster.node-frozen` | 403 | Pedido de ingresso de um nó em freezing (traz `Retry-After`) |

## Não encontrado — `404`

| Código | Quando |
|---|---|
| `topic.not-found` | O tópico não existe (e não há `autoCreate`) |
| `message.not-found` | A mensagem não existe: já confirmada, vencida por `ttl` ou removida com o tópico. **Um segundo ack** dá este erro |
| `token.not-found` | O token não existe ou já foi revogado |
| `node.not-found` | `/nodes/{nodeId}/…` com um `nodeId` que não está no cluster (nem entre os nós que saíram por drain) |
| `route.not-found` | Caminho que não existe naquela porta (`/cluster/*` na porta de cliente; `/tokens` com autenticação desligada) |
| `cluster.operation-unknown` | Entre nós: `commit`/`rollback` de uma operação que o nó não conhece (expirou) |

## Conflito — `409`

| Código | Quando |
|---|---|
| `topic.already-exists` | Já existe um tópico com o nome |
| `topic.wrong-type` | `GET`/ack num distribuidor; ou `autoCreate` num nome que pertence a um distribuidor |
| `topic.read-only` | `PATCH`/`DELETE` numa fila de dead-letter |
| `message.stale-receipt` | O `receiptToken` não é o da última entrega (a mensagem foi reentregue) |
| `drain.conflict` | `DELETE /node/drain` durante o desregistro, ou com o processo já parando por sinal |
| `cluster.conflict` | Entre nós: outra operação pendente no mesmo recurso, ou a mensagem já está invisível |

## Limites e configuração — `413`, `415`, `421`, `422`, `429`

| Código | Status | Quando |
|---|---|---|
| `message.too-large` | 413 | Corpo acima de `maxMessageBytes` do tópico (ou de `message.max-bytes`) |
| `media.unsupported` | 415 | `Content-Type` ≠ `application/json` numa rota com corpo JSON (não vale na publicação) |
| `fifo.misdirected` | 421 | **Entre nós:** o nó não é o primário do grupo |
| `cluster.wrong-node` | 421 | **Entre nós, no canal:** outro processo no mesmo endereço |
| `distributor.too-many-targets` | 422 | A publicação casaria mais filas que `topics.distributor.max-targets` (nenhuma cópia é feita) |
| `topic.volatile-limit` | 422 | O `autoCreate` esbarra em `topics.volatile.max-count` |
| `cluster.join-unreachable` | 422 | Entre nós: o coordenador do ingresso não conseguiu conectar de volta ao nó novo |
| `backup.not-configured` | 422 | `POST /node/backup/flush` ou `POST /cluster/backup/flush` num nó sem `backup.s3.bucket` |
| `rate-limit.exceeded` | 429 | Rate limit excedido (traz `Retry-After`) |

## Disponibilidade e falhas — `5xx`

| Código | Status | Quando |
|---|---|---|
| `internal.error` | 500 | Falha inesperada (um *panic* é convertido neste erro) |
| `forward.bad-gateway` | 502 | O repasse falhou depois de enviado o corpo |
| `forward.timeout` | 504 | O repasse estourou o tempo depois de enviado o corpo |
| `cluster.quorum-unavailable` | 503 | Sem nós suficientes para fechar o quórum de escrita, entrega ou ack |
| `cluster.registration-failed` | 503 | Um registro (nó, mensagem) não foi confirmado por todos e foi cancelado. **Nada foi aplicado** |
| `cluster.prune-incomplete` | 503 | O prune de uma fila falhou em alguns nós (nomeados na mensagem e em `details`). A fila pode estar vazia nos outros; repetir a chamada é seguro |
| `cluster.commit-timeout` | 503 | O registro foi confirmado, mas o commit não chegou a todos em `write.commit.timeout`. **A operação continua em segundo plano e vai se completar** |
| `cluster.node-syncing` | 503 | O nó está sincronizando e não achou outro para repassar |
| `cluster.node-draining` | 503 | O nó está em drain e não achou outro para repassar (num nó só, sempre) |
| `node.failed` | 503 | O nó está em falha (disco cheio ou banco corrompido) e não achou outro para repassar |
| `fifo.primary-unavailable` | 503 | O primário do grupo FIFO está fora do ar, sincronizando, ou esgotou `fifo.forward.max-attempts` |
| `delivery.unavailable` | 503 | O `GET` não capturou nenhuma mensagem e houve falha no registro de invisibilidade (nunca devolve vazio nesse caso) |

Os `503` trazem `Retry-After` (hoje 5 s no drain, na sincronização e na falta de quórum). Os `502`/`504` **não** foram repetidos pelo nó: o cliente decide.

## Limites fixos (validação)

Regras de formato, **não configuráveis**, aplicadas na criação e na alteração (`core/validation`):

| Item | Limite |
|---|---|
| Nome de tópico | `^[A-Za-z0-9_-]{1,100}$`, sensível a maiúsculas. A dead-letter pode ter até **112** (com `-dead-letter`) |
| Tags | até **20** por tópico; chave 1 a 64, valor 0 a 128, `A-Za-z0-9_-.:` |
| `prefixes` (distribuidor) | até **20**, de 1 a 100 caracteres, `A-Za-z0-9_-`; vazio é recusado |
| `idleTimeout` | 60 a 604 800 s |
| `ttl` | 60 a 31 536 000 s |
| `maxDeliveries` | 1 a 1000 |
| `groupId`, `deduplicationId` | 1 a 128, `A-Za-z0-9_-.:` |
| Token: `name` | 1 a 100, `A-Za-z0-9_-.:` |
| Token: `permissions` | ao menos uma de `publish`, `consume`, `create`, `manage`, `admin`; `admin` não aceita `topicPrefixes` |
| Token: `topicPrefixes` | até 20, de 1 a 100, `A-Za-z0-9_-` |
| Token: `expiresAt` | no futuro |
| `contentType` | media type de 1 a 255 caracteres; padrão `application/json` |
| `waitTime` (long polling) | 0 a 20 s |
| `X-Request-Id` | 1 a 128, `A-Za-z0-9_-.:` |
| `limit` de paginação | 1 a `api.pagination.max-limit` (1000) |

## Limites configuráveis, sem teto por padrão

| Chave | Efeito |
|---|---|
| `message.max-bytes` (**5 MB**) | Tamanho máximo da mensagem para todo tópico. Cada tópico pode ter o seu `maxMessageBytes`, **inclusive maior**, até `message.hard-limit` (sem padrão: sem teto). Maior que o limite do tópico: `413`. O `PATCH` só vale para publicações seguintes. Num distribuidor vale o geral e as cópias não são conferidas de novo |
| `topics.distributor.max-targets` | Destinos por publicação de distribuidor; acima, `422` e **nenhuma** cópia |
| `topics.volatile.max-count` | Filas voláteis no cluster; atingido, o `autoCreate` dá `422` |
| `delivery.max-messages.max` | Teto de `maxMessages` |
| `delivery.visibility-timeout.max` | Teto de `visibilityTimeout` |

## Rate limits

**Desligados por padrão** (sem `ratelimit.*.rps`, a categoria não tem limite). Token bucket, **por nó**, por categoria e por chave; detalhes em [14-seguranca.md](14-seguranca.md).

| Categoria | Chave de configuração | Rotas |
|---|---|---|
| Publicação | `ratelimit.publish.rps` | `POST /topics/{topic}/messages` |
| Consumo | `ratelimit.consume.rps` | `GET`, `PUT .../visibility`, `DELETE` de mensagens |
| Administrativo | `ratelimit.admin.rps` | `/topics*`, `/tokens*` |
| Operação | `ratelimit.ops.rps` | `/health/cluster?scope=cluster`, `/metrics/topics*` |
| Estouro | `ratelimit.burst` | Vale para todas; ausente = `ceil(rps)` |

Isentas: `/health/live`, `/health/ready`, `/metrics`, `/health/cluster?scope=node`, e a porta do cluster inteira. Estouro: `429 rate-limit.exceeded` com `Retry-After`.

## Fontes e provas

- `core/error/mod.rs` (a macro com todos os `ErrorCode` e status; **o catálogo acima foi conferido contra ela**), `core/validation/mod.rs` (as faixas), `http/ratelimit`, `config/params/mod.rs`.
- Casos de uso: UC-CNV (convenções: erros e status), UC-LIM (rate limit), UC-CFG; veja [26a](26a-casos-de-uso-implementados.md). O teste que confere cada código e o seu status está em `core/error/tests.rs`.
- Histórico: `06d-api-convencoes.md` (8.3.6), `12-limites-api.md`.
