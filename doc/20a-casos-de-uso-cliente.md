# Casos de uso: cliente

[← Índice](README.md) · [Convenções e níveis de teste](20-casos-de-uso.md)

## 19.A Casos de uso do cliente

O que uma aplicação cliente faz com o Taba: cria tópicos, publica, consome, confirma, e usa os recursos de cada tipo de tópico. Convenções, níveis (`U`, `I`, `C`, `K`) e a forma de citar os códigos nos testes: [20-casos-de-uso.md](20-casos-de-uso.md). Todos os casos de `I` rodam contra um nó em modo desenvolvimento; os que dependem de mais de um nó, de falha ou de tempo pedem `C`.

### TOP: tópicos

Criar, listar, ler, alterar e remover. Rotas e regras: [06a-api-cliente.md](06a-api-cliente.md), seção 8.1 ("Tópicos"). Propagação no cluster: [02-arquitetura.md](02-arquitetura.md), seção 3.3 (e os casos `DEF`).

#### Criar (`POST /topics`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-TOP-001 | Criar uma fila (`type: queue`) com nome válido: `201`, `Location: /topics/{name}`, corpo `Topic` com `id` GUID v7, `createdAt`, `tags: {}`, `volatile: false`, `fifo: false` e `effective` com os padrões do cluster. | 06a | I |
| UC-TOP-002 | Criar um distribuidor (`type: distributor`) com `prefixes`: `201`, e o `Topic` traz `prefixes` e não traz os campos de fila (`volatile`, `fifo`, `ttl`, `maxDeliveries`, `maxMessageBytes`, `effective`). | 06a | I |
| UC-TOP-003 | Criar fila volátil (`volatile: true`) sem `idleTimeout`: vale o padrão de 600 s (`topics.volatile.idle-timeout.default`); com `idleTimeout` informado, vale o informado. | 06a, 15 §14.3 | I |
| UC-TOP-004 | Criar fila FIFO (`fifo: true`) com `replication.factor` de pelo menos 2: `201` e `fifo: true` no `Topic`. | 06a, 17 §16.2 | I, C |
| UC-TOP-005 | Criar fila com `ttl`, `maxDeliveries` e `maxMessageBytes` próprios: o `Topic` traz os valores definidos e `effective` os mesmos; sem eles, os três são `null` e `effective` traz os padrões (14 dias, 10, 5 MB). | 06a | I |
| UC-TOP-006 | Toda fila criada ganha junto a fila `{nome}-dead-letter`, que aparece em `GET /topics` com `deadLetter: true` e `source`. | 06a, 13 §12.10 | I |
| UC-TOP-007 | Criar com tags válidas (até 20; chave de 1 a 64 e valor de 0 a 128 caracteres de `A-Za-z0-9_-.:`): `201` e as tags voltam no `Topic`. | 06a | I |
| UC-TOP-008 | Criar com tags inválidas (mais de 20; chave vazia ou com mais de 64; valor com mais de 128; caractere fora do conjunto): `400` `validation.failed`, com o campo em `details`. | 06a, 12 | U, I |
| UC-TOP-009 | Nome fora de `^[A-Za-z0-9_-]{1,100}$` (vazio, 101 caracteres, espaço, acento, `/`): `400` `validation.invalid-name`. | 06a, 12 | U, I |
| UC-TOP-010 | Nome terminado em `-dead-letter`: `400` `validation.reserved-name`. | 06a, 13 §12.10 | U, I |
| UC-TOP-011 | O nome diferencia maiúsculas de minúsculas: `Pedidos` e `pedidos` coexistem como tópicos distintos. | 06a | I |
| UC-TOP-012 | Criar com nome que já existe: `409` `topic.already-exists`. | 06a | I |
| UC-TOP-013 | Campo que não vale para o tipo (`prefixes` numa fila, `fifo`, `volatile`, `ttl`, `maxDeliveries` ou `maxMessageBytes` num distribuidor): `400` `validation.not-applicable`. | 06a | U, I |
| UC-TOP-014 | `idleTimeout` fora de 60 a 604800, `ttl` fora de 60 a 31536000 ou `maxDeliveries` fora de 1 a 1000: `400` `validation.out-of-range`. Os limites exatos (59 e 60, 604800 e 604801 etc.) são aceitos e recusados como definido. | 06a, 12 | U, I |
| UC-TOP-015 | `maxMessageBytes` acima de `message.hard-limit`: `400` `validation.max-bytes-above-hard-limit`. Acima do geral (5 MB) e dentro do limite rígido: aceito. | 06a, 12 | U, I |
| UC-TOP-016 | Distribuidor com mais de 20 prefixos, prefixo vazio, de mais de 100 caracteres ou com caractere fora de `A-Za-z0-9_-`: `400`. | 06a, 12, 15 §14.2 | U, I |
| UC-TOP-017 | Distribuidor com `prefixes` vazio é aceito: não copia a publicação para ninguém. | 06a | I |
| UC-TOP-018 | Corpo com campo desconhecido: `400` `validation.unknown-field`. Sem `name` ou sem `type`: `400` `validation.required`, com o campo em `details`. | 06a, 06d | U, I |
| UC-TOP-019 | Criar fila FIFO com `replication.factor` abaixo de 2: `422` `topic.fifo-needs-replication`. | 06a, 17 §16.2 | C |
| UC-TOP-020 | Recriar um tópico com o mesmo nome (depois de removê-lo) gera um `id` diferente. | 06a, 15 §14.3 | I |

#### Listar (`GET /topics`) e ler (`GET /topics/{topic}`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-TOP-021 | Listar sem tópicos: `200` com `items: []` e sem `nextCursor`. | 06a | I |
| UC-TOP-022 | A listagem vem ordenada por nome por padrão e inclui as filas de dead-letter. | 06a | I |
| UC-TOP-023 | `sort` por `name`, `id`, `createdAt` e cada um com `-` (decrescente); `sort` desconhecido: `400` `validation.failed`. | 06a | I |
| UC-TOP-024 | O filtro `name` devolve os tópicos cujo nome **contém** o texto, diferenciando maiúsculas de minúsculas. | 06a | I |
| UC-TOP-025 | A listagem e a leitura são respondidas localmente, sem chamar outros nós (a definição está em todo nó). | 06a, 02 §3.3 | C |
| UC-TOP-026 | `GET /topics/{topic}` devolve a definição; tópico inexistente: `404` `topic.not-found`. | 06a | I |
| UC-TOP-027 | `GET` da dead-letter devolve `deadLetter: true`, `source` e os valores da fila de origem, somente leitura. | 06a, 13 §12.10 | I |

#### Alterar (`PATCH /topics/{topic}`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-TOP-028 | `PATCH` parcial: só os campos enviados mudam. `tags`, quando enviado, **substitui o mapa inteiro**. | 06a | I |
| UC-TOP-029 | `null` em `ttl`, `maxDeliveries` ou `maxMessageBytes` volta ao padrão do cluster (`effective` acompanha o padrão). | 06a | I |
| UC-TOP-030 | Novo `ttl` vale também para as mensagens já guardadas, a partir da próxima varredura. | 06a, 13 §12.10 | I |
| UC-TOP-031 | Novo `maxMessageBytes` só vale para as publicações seguintes; mensagens já aceitas não são rejeitadas. | 06a, 12 | I |
| UC-TOP-032 | `PATCH` de `prefixes` num distribuidor vale a partir do commit e só para publicações posteriores. | 06a, 15 §14.2 | I |
| UC-TOP-033 | `PATCH` de `idleTimeout` numa fila volátil vale a partir da próxima verificação do gestor. | 06a, 15 §14.3 | I, C |
| UC-TOP-034 | `PATCH` com campo imutável (`name`, `id`, `type`, `volatile`, `fifo`), **mesmo com o valor atual**: `400` `validation.immutable-field`. | 06a | U, I |
| UC-TOP-035 | `PATCH` com campo que não vale para o tipo (`ttl` num distribuidor): `400` `validation.not-applicable`. Fora da faixa ou acima do limite rígido: `400`. | 06a | U, I |
| UC-TOP-036 | `PATCH` na dead-letter: `409` `topic.read-only`. | 06a | I |
| UC-TOP-037 | `PATCH` em tópico inexistente: `404` `topic.not-found`. | 06a | I |

#### Remover (`DELETE /topics/{topic}`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-TOP-038 | Remover um tópico vazio: `204`; um `GET` posterior responde `404`. | 06a | I |
| UC-TOP-039 | Remover uma fila com mensagens prontas, invisíveis (em processamento) e sem ack: `204`, sempre, sem checagem. Todas as mensagens somem (memória, disco e metadados). | 06a, 02 §3.3 | I, C |
| UC-TOP-040 | Remover uma fila remove também a sua dead-letter, com as mensagens dela. | 06a, 13 §12.10 | I |
| UC-TOP-041 | `DELETE` na dead-letter: `409` `topic.read-only`. | 06a | I |
| UC-TOP-042 | `DELETE` de tópico inexistente: `404` `topic.not-found` (inclusive na repetição depois de um `503` `cluster.commit-timeout`, que o cliente trata como sucesso). | 06a, 06d | I, C |
| UC-TOP-043 | Remover um distribuidor: `204`; as filas que ele copiava não são afetadas. | 06a, 15 §14.2 | I |

### MSG: publicação e o corpo da mensagem

`POST /topics/{topic}/messages`. Regras do corpo: [06a-api-cliente.md](06a-api-cliente.md), "O corpo da mensagem". Coordenação e quórum no cluster: casos `ESC`.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-MSG-001 | Publicar JSON válido, sem `Content-Type`: `201`, `PublishResponse` com `id` GUID v7, e a mensagem fica com `contentType: application/json`. | 06a | I |
| UC-MSG-002 | Publicar texto (`text/plain; charset=utf-8`): `201`, e o `contentType` guarda o parâmetro `charset`. | 06a | I |
| UC-MSG-003 | Publicar binário (`image/png`): `201`, e os bytes ficam guardados exatamente como vieram, sem reserializar nem codificar. | 06a, 02 §3.2 | I |
| UC-MSG-004 | `Content-Type` que não é um media type válido, ou com mais de 255 caracteres: `400` `validation.invalid-content-type`. | 06a | U, I |
| UC-MSG-005 | Corpo `application/json` (ou `*/*+json`) que não é JSON válido: `400` `validation.invalid-body`. | 06a | U, I |
| UC-MSG-006 | Corpo `text/*` que não é UTF-8 válido: `400` `validation.invalid-body`. | 06a | U, I |
| UC-MSG-007 | Corpo de qualquer outro tipo não é validado (bytes arbitrários são aceitos). | 06a | I |
| UC-MSG-008 | O tamanho conta os **bytes enviados** (não o base64). Corpo com exatamente `maxMessageBytes` é aceito; um byte a mais: `413` `message.too-large`. | 06a, 12 | U, I |
| UC-MSG-009 | Sem `maxMessageBytes` no tópico vale `message.max-bytes` (5 MB); um tópico com `maxMessageBytes` maior que o geral aceita mensagens maiores, até o limite rígido. | 06a, 12 | I |
| UC-MSG-010 | Publicar em tópico inexistente: `404` `topic.not-found`. | 06a | I |
| UC-MSG-011 | `groupId` válido (1 a 128 caracteres de `A-Za-z0-9_-.:`) numa fila comum: aceito e devolvido no `GET`. Inválido (vazio, mais de 128, caractere fora do conjunto): `400`. | 06a, 12 | U, I |
| UC-MSG-012 | `deduplicationId` numa fila que não é FIFO: `400` `validation.not-applicable`. | 06a, 17 §16.2 | I |
| UC-MSG-013 | O `id` é gerado pelo nó coordenador antes do registro, é um GUID v7 em minúsculas e ordenável por tempo: duas publicações sequenciais têm `id` crescente. | 02 §3.1, 05 §7.2 | U, I |
| UC-MSG-014 | A resposta só sai depois de o commit estar confirmado no quórum (a mensagem já é entregável no instante em que o cliente recebe o `id`). | 05 §7.2, 02 §3.1 | C |
| UC-MSG-015 | Publicar por qualquer nó do cluster funciona: o nó que recebe é o coordenador, e a mensagem acaba nos nós responsáveis pelo bloco. | 02 §3.1 | C |
| UC-MSG-016 | Sem quórum: `503` `cluster.quorum-unavailable` com `Retry-After`, e a mensagem **não** foi aceita (nenhum `GET` a vê). | 06a, 05 §7.2 | C |
| UC-MSG-017 | `502` ou `504` num repasse depois de enviado o corpo, fora da FIFO: a chamada não é repetida e o erro sobe ao cliente. | 06a, 04 §6.9 | C |
| UC-MSG-018 | `PublishResponse` numa fila comum não traz `duplicate`; o campo só existe na fila FIFO. | 06a | I |
| UC-MSG-019 | Publicar com `X-Request-Id`: a resposta devolve o mesmo valor. | 06d §8.3.2 | I |
| UC-MSG-020 | A mensagem publicada preserva o `groupId` e o `contentType` por todo o ciclo (publicação, entrega, reentrega e dead-letter). | 06a, 13 §12.11 | I |

### CNS: consumo (`GET /topics/{topic}/messages`)

Entrega interna entre nós: casos `ENT`. Long polling entre nós: casos `ENT` e `CAN`.

#### Entrega e corpo

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNS-001 | `GET` numa fila com mensagens: `200`, `messages` com `id`, `body`, `contentType`, `receiptToken`, `deliveryCount: 1`, `publishedAt` e `invisibleUntil`. | 06a | I |
| UC-CNS-002 | `GET` sem mensagens prontas: `200` com `messages: []`, **nunca** `204`. | 06a | I |
| UC-CNS-003 | `maxMessages` sem informar vale 10 (`delivery.max-messages.default`); informado, devolve até esse número; fora de 1 a `delivery.max-messages.max`: `400` `validation.out-of-range`. | 06a, 13 §12.2 | I |
| UC-CNS-004 | O `GET` devolve o que estiver pronto, sem esperar completar `maxMessages` (com `waitTime` 0). | 06a, 13 §12.2 | I |
| UC-CNS-005 | `visibilityTimeout` sem informar vale 120 s; informado vale o informado; fora de 0 a `delivery.visibility-timeout.max`: `400`. O `invisibleUntil` devolvido é `agora + visibilityTimeout`. | 06a, 13 §12.2 | I |
| UC-CNS-006 | Uma mensagem entregue fica invisível: um segundo `GET` dentro do prazo não a devolve. | 13 §12.2 | I |
| UC-CNS-007 | Sem ack até o fim do prazo, a mensagem volta a ser entregue, com `deliveryCount` incrementado e um `receiptToken` **novo** (o anterior não é mais aceito). | 13 §12.4, 14 §13.2 | I |
| UC-CNS-008 | Corpo JSON sai como **JSON nativo** (objeto, lista, texto, número, booleano, `null`), exatamente como foi enviado, sem `bodyEncoding`. | 06a | I |
| UC-CNS-009 | Corpo `text/*` em UTF-8 (ou sem `charset`) sai como **string**, sem `bodyEncoding`. | 06a | I |
| UC-CNS-010 | Corpo de qualquer outro tipo, e `text/*` com outro `charset`, sai em **base64**, com `bodyEncoding: "base64"`. | 06a | I |
| UC-CNS-011 | `*/*+json` (por exemplo, `application/vnd.api+json`) é tratado como JSON. | 06a | U, I |
| UC-CNS-012 | Num lote com JSON, texto e binário, cada mensagem sai na forma do seu `contentType`. | 06a | I |
| UC-CNS-013 | Não há garantia de ordem entre mensagens de uma fila comum; o teste não pode exigir ordem de entrega. As duas pontas do GUID v7 são consumidas de forma intercalada (efeito sanfona). | 13 §12.6 e 12.7 | U |
| UC-CNS-014 | A mensagem só chega ao cliente depois de a invisibilidade estar commitada no quórum. | 13 §12.2 | C |
| UC-CNS-015 | Mensagem vencida por `ttl` (mesmo antes da varredura) não é entregue. | 13 §12.10 | I |
| UC-CNS-016 | Consumir da dead-letter: `GET` normal, com `deadLetteredAt` e `deadLetterReason` nas mensagens. | 06a, 13 §12.10 | I |
| UC-CNS-017 | `GET` num distribuidor: `409` `topic.wrong-type`. | 06a | I |
| UC-CNS-018 | `GET` num tópico que não existe, sem `autoCreate`: `404` `topic.not-found`. | 06a | I |

#### Falha parcial e total na entrega

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNS-019 | O registro de invisibilidade de uma mensagem falha e há outras prontas: o rollback segue em outra thread, o `GET` passa ao próximo ID sem esperar, e a mensagem que falhou **nunca** entra na resposta. | 13 §12.2 | C |
| UC-CNS-020 | Ao menos uma mensagem capturada e outras falharam: `200` com as que conseguiu (menos que `maxMessages`). | 06a, 13 §12.2 | C |
| UC-CNS-021 | **Nenhuma** mensagem capturada e houve falha no registro: `503` `delivery.unavailable` com `Retry-After`, nunca `200` vazio. | 06a, 13 §12.2 | C |
| UC-CNS-022 | O número de falhas toleradas por `GET` é limitado por `delivery.get.max-attempts`; esgotado, aplica-se UC-CNS-020 ou UC-CNS-021. | 13 §12.2 | C |
| UC-CNS-023 | Se o rollback não chega a uma réplica, o pendente expira por `write.pending.ttl` e a mensagem segue pronta como estava. | 13 §12.2 | C |

#### Long polling

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNS-024 | `waitTime` de 0 a 20 é aceito (padrão 0, responde na hora); fora da faixa: `400` `validation.out-of-range`. | 06a, 12 | U, I |
| UC-CNS-025 | Com `waitTime > 0` e fila vazia, o `GET` espera; uma mensagem publicada durante a espera é devolvida **sem** esperar o fim do tempo. | 13 §12.8 | I |
| UC-CNS-026 | Com `waitTime > 0` e nenhuma mensagem chegando, responde `200` com `messages: []` ao fim do tempo. | 13 §12.8 | I |
| UC-CNS-027 | Com long polling, o `GET` devolve a primeira mensagem que chegar, sem esperar completar `maxMessages`. | 06a | I |
| UC-CNS-028 | O cliente desconecta durante a espera: o ouvinte é removido (nenhuma mensagem é entregue a uma conexão morta, e o recurso é liberado). | 13 §12.8 | I |
| UC-CNS-029 | Vários `GET` em espera no mesmo tópico: cada mensagem nova vai ao primeiro da fila de ouvintes, na ordem de chegada. | 13 §12.8 | I |
| UC-CNS-030 | Long polling em espera num nó e mensagem publicada em outro nó do cluster: o consumidor recebe a mensagem (aviso entre nós + `take`). | 13 §12.8 | C |

#### `autoCreate`

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNS-031 | `autoCreate=true` num nome que não existe cria uma fila **volátil** por padrão (`volatile` padrão `true`), com `idleTimeout` 600 s, e já conta como consumidor. | 15 §14.4 | I |
| UC-CNS-032 | `autoCreate` com `volatile=false`, `idleTimeout`, `fifo=true` e `tag.<chave>=<valor>` cria a fila com esses atributos. | 06a, 15 §14.4 | I |
| UC-CNS-033 | `autoCreate` num tópico que já existe e é fila: os parâmetros de criação são **ignorados** e o `GET` é um consumo normal. | 15 §14.4 | I |
| UC-CNS-034 | `autoCreate` num nome que pertence a um distribuidor: `409` `topic.wrong-type`. | 15 §14.4 | I |
| UC-CNS-035 | `autoCreate` com nome terminado em `-dead-letter`: `400` `validation.reserved-name`. Nome inválido: `400` `validation.invalid-name`. | 06a, 15 §14.4 | I |
| UC-CNS-036 | `autoCreate` que esbarra em `topics.volatile.max-count`: `422` `topic.volatile-limit`. Sem o teto definido, não há limite. | 15 §14.4 | I, C |
| UC-CNS-037 | `autoCreate` de fila FIFO com `replication.factor` abaixo de 2: `422` `topic.fifo-needs-replication`. | 06a, 17 §16.2 | C |
| UC-CNS-038 | Vários consumidores com `autoCreate` ao mesmo tempo no mesmo nome: um cria, os demais tratam como "já existe" e seguem como consumo normal, sem erro `409`. | 15 §14.4 | C |
| UC-CNS-039 | O primeiro `GET` com `autoCreate` paga o registro + commit em todos os nós; os seguintes não. | 15 §14.4 | C |
| UC-CNS-040 | `autoCreate` exige as permissões `consume` **e** `create` (com autenticação ligada). | 06a §8.1.3 | I |

### ACK: confirmação do consumo (`DELETE /topics/{topic}/messages/{id}`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-ACK-001 | Ack com o `receiptToken` da última entrega: `204`, e a mensagem some do tópico (nenhum `GET` a devolve, nem depois do prazo). | 06a, 13 §12.4 | I |
| UC-ACK-002 | Ack sem `receiptToken` na query: `400` `validation.required`. | 06a | I |
| UC-ACK-003 | Um segundo ack da mesma mensagem: `404` `message.not-found`. | 06a | I |
| UC-ACK-004 | Ack com token obsoleto (a mensagem foi reentregue a outro consumidor): `409` `message.stale-receipt`, e **nada muda** (a mensagem continua do segundo consumidor). | 06a, 14 §13.2 | I |
| UC-ACK-005 | Ack depois de vencido o prazo mas **antes** da reentrega: o token continua válido e o ack vale. | 06a, 14 §13.2 | I |
| UC-ACK-006 | Ack de mensagem vencida por `ttl`: `404` `message.not-found`. | 06a, 13 §12.10 | I |
| UC-ACK-007 | Ack em distribuidor: `409` `topic.wrong-type`. Em tópico inexistente: `404` `topic.not-found`. | 06a | I |
| UC-ACK-008 | Sem quórum, o ack é cancelado (rollback): `503` `cluster.quorum-unavailable` e a mensagem **continua disponível** (volta ao fim do prazo). | 06a, 13 §12.4 | C |
| UC-ACK-009 | O ack é decidido por quórum **com o primário**: com o primário fora do quórum o ack não se efetiva. | 13 §12.4 | C |
| UC-ACK-010 | Falha numa réplica fora do quórum, já decidido o ack: não cancela; o coordenador insiste até `write.commit.timeout` e, esgotado, o bloco fica abaixo do fator pleno (evento, não falha de saúde). | 13 §12.4 | C |
| UC-ACK-011 | Na fila comum o ack remove a mensagem e os metadados sem deixar tombstone: a ausência do ID basta. | 13 §12.4 | I, C |
| UC-ACK-012 | Ack depois de a fila volátil ter sido excluída: `404` `topic.not-found`. | 15 §14.5 | I |
| UC-ACK-013 | Ack de uma mensagem em disco (overflow) remove a linha do banco e o arquivo `.body`, nessa ordem. | 02 §3.2 | I |

### VIS: alteração da invisibilidade (`PUT .../visibility`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-VIS-001 | `PUT` com `receiptToken` válido e `visibilityTimeout` de 0 a `delivery.visibility-timeout.max`: `200` `VisibilityResponse` com `id` e o novo `invisibleUntil`. | 06a | I |
| UC-VIS-002 | O novo prazo é `agora + visibilityTimeout`, **não** um acréscimo ao prazo atual (encurtar também funciona). | 06a, 14 §13.3 | I |
| UC-VIS-003 | `visibilityTimeout: 0` libera a mensagem na hora: o `invisibleUntil` devolvido é o instante atual e o próximo `GET` a entrega. | 06a, 13 §12.3 | I |
| UC-VIS-004 | O `PUT` **não** incrementa `deliveryCount` e mantém o mesmo `receiptToken` (só uma nova entrega gera outro). | 14 §13.2 e 13.3 | I |
| UC-VIS-005 | Sem `receiptToken` (query) ou sem `visibilityTimeout` (corpo): `400` `validation.required`. Tempo fora da faixa: `400` `validation.out-of-range`. Campo desconhecido no corpo: `400` `validation.unknown-field`. | 06a | U, I |
| UC-VIS-006 | `PUT` com token obsoleto (mensagem reentregue): `409` `message.stale-receipt`, nada muda. | 06a | I |
| UC-VIS-007 | `PUT` de mensagem já confirmada, vencida por `ttl` ou de tópico removido: `404` (`message.not-found` ou `topic.not-found`). | 06a | I |
| UC-VIS-008 | `PUT` sem quórum: `503` `cluster.quorum-unavailable`, e o prazo anterior continua valendo. | 06a, 14 §13.3 | C |
| UC-VIS-009 | Repetir o mesmo `PUT` dá o mesmo resultado (idempotente, por substituir o valor). | 06a | I |
| UC-VIS-010 | Com o prazo estendido, a mensagem **não** é reentregue enquanto o novo prazo não vence. | 14 §13.3 | I |
| UC-VIS-011 | `PUT` com `0` numa mensagem que já está no `maxDeliveries`: vai para a dead-letter em vez de voltar à lista de prontos. | 13 §12.10 | I |
| UC-VIS-012 | O `PUT` é decidido pelo mesmo registro + commit por quórum da entrega, sem tocar na lista de prontos (exceto com `0`). | 13 §12.3, 14 §13.3 | C |

### RET: expiração (`ttl`), limite de entregas e dead-letter

Regras: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-RET-001 | Mensagem com mais de `ttl` desde a publicação é descartada, entregue ou não, e **não** vai para a dead-letter. | 13 §12.10 | I |
| UC-RET-002 | O prazo é `instante do ID + ttl` calculado com o `ttl` **atual** do tópico: reduzir o `ttl` por `PATCH` expira mensagens já guardadas. | 13 §12.10 | I |
| UC-RET-003 | O padrão do `ttl` (14 dias) vale na hora da conferência: mudar `retention.ttl.default` afeta as filas sem `ttl` próprio, e não as que o definiram. | 13 §12.10 | U, I |
| UC-RET-004 | A expiração vale também para mensagem **invisível** (em processamento): ela some, e o ack seguinte recebe `404`. | 13 §12.10 | I |
| UC-RET-005 | O dono não entrega mensagem vencida entre duas varreduras. | 13 §12.10 | I |
| UC-RET-006 | A varredura de cada nó (`retention.sweep.interval`) apaga as cópias vencidas em memória e em disco, sem propagar nada. | 13 §12.10 | C |
| UC-RET-007 | Cada mensagem expirada gera o evento `taba.message.expired` e conta em `taba.retention.expired`. | 13 §12.10, 11 | I |
| UC-RET-008 | `maxDeliveries: N`: ao vencer a invisibilidade da N-ésima entrega sem ack, a mensagem vai para a dead-letter e a (N+1)-ésima entrega **nunca acontece**. | 13 §12.10 | I |
| UC-RET-009 | Na dead-letter a mensagem mantém o **mesmo `id`**, com `deliveryCount: 0`, `invisibleUntil` zerado, `deadLetteredAt` e `deadLetterReason: max-deliveries`. | 13 §12.10 | I |
| UC-RET-010 | A mensagem na dead-letter cai no mesmo bloco e nos mesmos nós da mensagem original (mesmo `id`). | 13 §12.10 | U, C |
| UC-RET-011 | O movimento para a dead-letter é **uma operação** de registro + commit (cria na dead-letter e remove da fila de origem). Falha antes do quórum: rollback, a mensagem segue na fila e o dono tenta de novo na próxima volta. | 13 §12.10 | C |
| UC-RET-012 | A dead-letter não tem definição própria: não há linha na tabela `topics` e as configurações são as da fila de origem. | 13 §12.10, 02 §3.3 | I |
| UC-RET-013 | A dead-letter não tem dead-letter: a mensagem lá só sai por ack ou por `ttl` (contado de `deadLetteredAt`). | 13 §12.10 | I |
| UC-RET-014 | A dead-letter é um tópico comum para consumo, ack e publicação: dá para devolver uma mensagem corrigida à fila de origem. | 13 §12.10 | I |
| UC-RET-015 | O sufixo da dead-letter é configurável (`topics.dead-letter.suffix`) e passa a ser o nome reservado. | 10 | U, I |
| UC-RET-016 | A dead-letter nunca é destino de um distribuidor, mesmo que o nome comece com o prefixo. | 13 §12.10, 15 §14.2 | I |
| UC-RET-017 | O `GET` na dead-letter não conta como consumidor da fila volátil de origem. | 13 §12.10, 15 §14.3 | I |
| UC-RET-018 | Cada mensagem enviada à dead-letter gera o evento `taba.message.dead_lettered` (fila, ID, `deliveryCount`) e conta em `taba.delivery.dead_lettered`. | 13 §12.10, 11 | I |
| UC-RET-019 | Na fila FIFO, a mensagem que atinge `maxDeliveries` vai para a dead-letter e o grupo segue com a próxima; a vencida por `ttl` na cabeça também libera o grupo (casos `FIF`). | 13 §12.10, 17 §16.6 | I |
| UC-RET-020 | A sincronização de bloco descarta o que já venceu por `ttl` em vez de copiar. | 13 §12.10, 05 §7.3 | C |

### DST: Tópico Distribuidor

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-DST-001 | Publicar num distribuidor copia a mensagem para **cada** fila cujo nome começa com um dos prefixos: `200` `DistributorPublishResponse` com `matched` e uma entrada em `deliveries` por fila, cada cópia com `id` próprio. | 06a, 15 §14.2 | I |
| UC-DST-002 | Cada cópia é independente: o mesmo conteúdo e `contentType`, `id` diferente, e consumo e ack de uma não afetam as outras. | 15 §14.2 | I |
| UC-DST-003 | Nenhuma fila casa: `200` com `matched: 0` e `deliveries: []`, a mensagem é descartada, e o evento `taba.distributor.no_target` é registrado. | 06a, 15 §14.2 | I |
| UC-DST-004 | Uma fila que casa com **dois ou mais prefixos do mesmo** distribuidor recebe **uma** cópia. | 15 §14.2 | I |
| UC-DST-005 | Uma fila que casa com prefixos de **distribuidores diferentes** recebe uma cópia de cada publicação em cada um. | 15 §14.2 | I |
| UC-DST-006 | Só filas são destino: um distribuidor nunca recebe cópia (sem laços). | 15 §14.2 | I |
| UC-DST-007 | O casamento é feito na hora da publicação: uma fila criada depois só recebe o que for publicado depois. | 15 §14.2 | I |
| UC-DST-008 | Falha parcial: uma cópia falha (por exemplo, sem quórum) e as outras seguem; a resposta é `200`, com `error` (`code`, `message`) no item que falhou e `id` nos demais, e o evento `taba.distributor.partial`. Nada é desfeito. | 06a, 15 §14.2 | C |
| UC-DST-009 | Cópia sem `groupId` para uma fila FIFO falha só naquela fila, com `validation.group-id-required` no item; as outras cópias seguem. | 06a, 15 §14.2, 17 §16.8 | I |
| UC-DST-010 | O `groupId` e o `deduplicationId` informados vão para todas as cópias; sem `deduplicationId`, cada cópia usa o hash do corpo. | 15 §14.2, 17 §16.8 | I |
| UC-DST-011 | Com `topics.distributor.max-targets` definido e a publicação casando mais filas: `422` `distributor.too-many-targets`, e **nenhuma** cópia é feita. Sem o teto, não há limite. | 06a, 15 §14.2 | I |
| UC-DST-012 | Num distribuidor vale o `message.max-bytes` geral, e as cópias não são conferidas de novo contra o `maxMessageBytes` de cada fila. | 12 | I |
| UC-DST-013 | O `PATCH` de `prefixes` vale a partir do commit, só para publicações posteriores. | 06a, 15 §14.2 | I |
| UC-DST-014 | O distribuidor não guarda mensagens: não há mensagem em memória nem em disco para ele, e nenhuma lista de prontos. | 15 §14.2 | I |
| UC-DST-015 | Não há ordem entre as filas destino. O teste não pode exigir ordem. | 15 §14.2 | I |
| UC-DST-016 | `GET` e ack num distribuidor: `409` `topic.wrong-type`. | 15 §14.1 | I |
| UC-DST-017 | A permissão `publish` é conferida só no distribuidor; as filas destino não são conferidas de novo (nem os prefixos do token). | 06a §8.1.3 | I |
| UC-DST-018 | As métricas do distribuidor são só `received`, `copies` e `copyErrors` (as de fila se aplicam às filas destino). | 15 §14.7, 06b | I |

### VOL: fila volátil e nó gestor

Os casos com mais de um nó pedem `C`. No modo desenvolvimento o próprio nó é o gestor ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5).

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-VOL-001 | Fila volátil sem nenhum `GET` por `idleTimeout` segundos é excluída, com as mensagens e a dead-letter, e o evento `taba.volatile.deleted` é registrado. | 15 §14.3 e 14.5 | I, C |
| UC-VOL-002 | O relógio começa na **criação**: o prazo é `max(createdAt, último GET em qualquer nó) + idleTimeout`. | 15 §14.3 | U, I |
| UC-VOL-003 | **Qualquer** `GET` conta como consumidor (com ou sem mensagem, com ou sem long polling), e o instante é registrado no **início** da chamada. | 15 §14.3 | I |
| UC-VOL-004 | Publicar **não** conta como consumidor: uma fila volátil só com produtores é excluída, e as mensagens se perdem. | 15 §14.3 | I |
| UC-VOL-005 | O `GET` na dead-letter da fila volátil não conta como consumidor. | 15 §14.3 | I |
| UC-VOL-006 | O gestor é o nó primário do hash do `id` do tópico no anel; qualquer nó chega ao mesmo gestor a partir das tabelas de tópicos e de membros. | 15 §14.5 | U, C |
| UC-VOL-007 | A lista de tópicos gerenciados é derivada, não guardada: cada nó a calcula localmente e ela muda sozinha com a tabela de membros ou de tópicos. | 15 §14.5 | U, C |
| UC-VOL-008 | No vencimento o gestor pergunta a **todos os nós** (`GET /cluster/topics/{id}/idle`) há quanto tempo foi o último `GET`; cada nó responde uma duração decorrida, não um instante. | 15 §14.5, 06c | C |
| UC-VOL-009 | Se algum nó teve `GET` mais recente que o `idleTimeout`, a fila **não** é excluída: o gestor reagenda para `idleTimeout` menos a menor duração recebida. | 15 §14.5 | C |
| UC-VOL-010 | Se algum nó não responde, o gestor **não exclui** e tenta de novo depois de `topics.volatile.idle-check.retry-interval`; o evento `taba.volatile.check_blocked` é registrado. | 15 §14.5 | C |
| UC-VOL-011 | Um nó sem registro de `GET` para o tópico (subiu agora ou reiniciou) responde o tempo desde que subiu, com `sinceStart: true`: um reinício só pode **adiar** a exclusão. | 15 §14.5, 06c | C |
| UC-VOL-012 | A exclusão usa o registro + commit de todos os nós com o `id` do tópico (`expectedId`): uma fila excluída e **recriada** no meio do caminho não é apagada por engano. | 15 §14.5, 06c | C |
| UC-VOL-013 | Corrida residual: um `GET` depois da verificação final e antes do commit recebe `404`, ou recria a fila (com `autoCreate`) com `id` novo e sem as mensagens antigas. | 15 §14.5 | C |
| UC-VOL-014 | Um consumidor com mensagem invisível recebe `404` ao tentar o ack depois da exclusão. | 15 §14.5 | I |
| UC-VOL-015 | Gestor fora do ar e ainda não desregistrado: nenhuma fila dele é excluída; a fila continua recebendo mensagens e consumidores. Desregistrado o nó, o novo gestor assume. | 15 §14.5 | C |
| UC-VOL-016 | Entrada de nó: o nó novo só gerencia depois de ficar pronto; até lá nenhuma fila desse trecho do anel é excluída. Os nós existentes abandonam as filas cuja posição passou ao novo. | 15 §14.6 | C |
| UC-VOL-017 | Saída de nó (commit do desregistro): o novo primário das posições do nó removido assume as filas voláteis dele, sem transferir estado. | 15 §14.6, 04 §6.3 | C |
| UC-VOL-018 | Falha no registro de nó e retorno de nó excluído: o conjunto de filas gerenciadas de cada nó volta ao anterior. Tópico criado ou excluído atualiza o conjunto ao aplicar o commit. | 15 §14.6 | C |
| UC-VOL-019 | Ao assumir uma fila, o novo gestor faz **imediatamente** a consulta de ociosidade e calcula o prazo real; muitas filas trocando de gestor espalham as consultas no tempo (`topics.volatile.takeover.jitter`). | 15 §14.6 | C |
| UC-VOL-020 | Exclusão em andamento durante a troca de gestor: as duas tentativas são inofensivas (a segunda é rejeitada sem efeito pelo `id`). | 15 §14.6 | C |
| UC-VOL-021 | `PATCH` de `idleTimeout` vale na próxima verificação; `volatile` e o tipo são imutáveis. | 15 §14.3 | I |
| UC-VOL-022 | Uma fila FIFO pode ser volátil, com as mesmas regras. | 15 §14.3, 17 §16.8 | I |
| UC-VOL-023 | Exclusões de filas voláteis e mudanças de membros podem correr em paralelo, sem espera entre elas. | 15 §14.5 | C |

### FIF: fila FIFO

Regras: [17-topicos-fifo.md](17-topicos-fifo.md). A parte de sincronização e failover está em `SIN`, `FAL` e `INV`.

#### Publicação e deduplicação

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-FIF-001 | `fifo` é imutável: `PATCH` com `fifo` responde `400` `validation.immutable-field`. Um distribuidor não pode ser FIFO (`400` `validation.not-applicable`). | 17 §16.2, 06a | I |
| UC-FIF-002 | Publicar numa FIFO sem `groupId`: `400` `validation.group-id-required`. `groupId` inválido: `400`. | 17 §16.2, 06a | U, I |
| UC-FIF-003 | Publicar com `groupId` válido: `201` `PublishResponse` com `id` e `duplicate: false`. | 06a | I |
| UC-FIF-004 | Publicação duplicada (mesmo grupo, mesmo `deduplicationId`, dentro da janela): `200` (não `201`) com o `id` da **original** e `duplicate: true`, e nenhuma mensagem nova é criada. | 06a, 17 §16.7 | I |
| UC-FIF-005 | Sem `deduplicationId`, vale o SHA-256 dos **bytes** do corpo, em hexadecimal; o `contentType` não entra no hash. Duas mensagens idênticas no mesmo grupo viram uma só. | 17 §16.7 | U, I |
| UC-FIF-006 | O `deduplicationId` é devolvido no `GET` (o informado ou o hash). | 06a, 17 §16.7 | I |
| UC-FIF-007 | Depois da janela (`fifo.dedup.window`, padrão 300 s, contada do instante do ID da original), a mesma chave cria uma mensagem nova. | 17 §16.7 | I |
| UC-FIF-008 | O escopo é o grupo: o mesmo `deduplicationId` em grupos diferentes **não** é duplicata. | 17 §16.7 | I |
| UC-FIF-009 | Duas publicações simultâneas com a mesma chave são serializadas pelo primário: a segunda espera. Se a primeira commitou, devolve o mesmo `id`; se sofreu rollback, segue como nova. | 17 §16.7 | C |
| UC-FIF-010 | Repetir uma publicação que deu `502` ou `504` é seguro: o primário deduplica e a duplicata devolve o `id` da original. | 17 §16.4, 06a | C |
| UC-FIF-011 | A deduplicação continua valendo **depois do ack** da original, enquanto a janela não passa (a linha com `deleteAt` ainda existe). | 17 §16.7 | I |

#### Ordem e entrega

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-FIF-012 | A mensagem seguinte de um grupo só é entregue depois do **ack** da anterior (ou da saída dela por `ttl` ou dead-letter). | 17 §16.1 e 16.6 | I |
| UC-FIF-013 | Grupos diferentes andam em paralelo: com dois grupos livres, o `GET` entrega uma mensagem de cada. | 17 §16.1 | I |
| UC-FIF-014 | Cada mensagem do lote do `GET` vem de um grupo **diferente**; nunca duas do mesmo grupo no mesmo lote. | 17 §16.6 | I |
| UC-FIF-015 | Dentro do grupo a ordem de entrega é a ordem do `id`. Um produtor que publica em sequência (espera a resposta) tem a ordem preservada; produtores concorrentes ficam na ordem em que chegam ao primário. | 17 §16.1 e 16.4 | I, C |
| UC-FIF-016 | Reentrega: vencida a invisibilidade sem ack, a **mesma** mensagem volta como cabeça do grupo (`deliveryCount` sobe) e o grupo volta a ser livre; a ordem se mantém. | 17 §16.6 | I |
| UC-FIF-017 | `PUT .../visibility` com `0` na cabeça libera o grupo, como um vencimento. | 17 §16.6 | I |
| UC-FIF-018 | O ack da cabeça libera o grupo, e a mensagem seguinte entra na lista de prontos. | 17 §16.6 | I |
| UC-FIF-019 | Entre grupos livres vale a cabeça mais antiga; a entrega justa por faixas **não** vale na FIFO. | 17 §16.6, 13 §12.11 | U, I |
| UC-FIF-020 | A lista de prontos tem uma entrada por grupo livre (a cabeça), ordenada pelo ID; um grupo com mensagem em voo não está na lista. | 17 §16.6, 13 §12.6 | U |
| UC-FIF-021 | `maxDeliveries` atingido pela cabeça: vai para a dead-letter e o grupo segue com a próxima; a ordem das restantes é mantida. A dead-letter de uma FIFO **não é FIFO**. | 17 §16.6 e 16.8 | I |
| UC-FIF-022 | A cabeça vencida por `ttl` é descartada e o grupo é liberado para a próxima. | 17 §16.6 | I |

#### Colocação, coordenação e failover

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-FIF-023 | A posição no anel é `hash(id do tópico + ":" + groupId)`: todas as mensagens de um grupo caem no mesmo bloco, com o mesmo primário e as mesmas réplicas; grupos diferentes têm posições diferentes. | 17 §16.3 | U |
| UC-FIF-024 | Recriar o tópico (outro `id`) muda a posição dos grupos. | 17 §16.3 | U |
| UC-FIF-025 | A dead-letter de uma FIFO usa o `id` da fila de origem (e o `groupId`) para a posição, para o movimento continuar sendo uma operação só. | 17 §16.3 | U, C |
| UC-FIF-026 | O nó que recebe a publicação a repassa ao **primário do grupo**, que coordena (gera o ID, confere a deduplicação, faz o registro + commit). Se o próprio nó é o primário, não há repasse. | 17 §16.4 | C |
| UC-FIF-027 | ID monotônico por grupo: `max(agora, último ID do grupo + 1)`, usando os bits de contador do GUID v7 no mesmo milissegundo; com o relógio do primário atrasado, os IDs ainda crescem. | 17 §16.4 | U |
| UC-FIF-028 | O primário recalcula o dono do grupo ao receber a publicação repassada: se não for ele, responde `421` `fifo.misdirected`; o nó que repassa tenta de novo com a tabela atualizada, até `fifo.forward.max-attempts`, e esgotado o cliente recebe `503` `fifo.primary-unavailable` com `Retry-After`. | 17 §16.4, 06c | C |
| UC-FIF-029 | O primário do grupo ainda em sincronização (nó novo): as publicações do grupo respondem `503` com `Retry-After`; o primário antigo **não** coordena. | 17 §16.4 | C |
| UC-FIF-030 | Primário fora do ar e ainda não desregistrado: as publicações do grupo respondem `503` até o desregistro. | 17 §16.4 | C |
| UC-FIF-031 | O quórum de escrita, de entrega e de ack **sempre inclui o primário e o secundário**, e nunca fica abaixo de 2; `write.quorum` maior que 2 também os inclui. | 17 §16.5 | U, C |
| UC-FIF-032 | Secundário fora do ar e ainda não desregistrado: escrita, entrega e ack do grupo respondem `503`, mesmo com o terciário saudável; some quando ele é desregistrado. | 17 §16.5 | C |
| UC-FIF-033 | Failover sem reordenação: o primário confirma `m1`, libera `m2`, e cai; o novo primário (o secundário) já tem o ack de `m1`, e o consumidor **não** vê `m1, m2, m1`. | 17 §16.5 | C |
| UC-FIF-034 | O novo primário herda a deduplicação sem perda (os registros existem em todos os nós). | 17 §16.7 | C |
| UC-FIF-035 | Ao assumir um bloco, o novo primário refaz por grupo a cabeça, o estado livre ou ocupado (grupo com invisível no futuro entra ocupado) e a base do ID monotônico. | 17 §16.6, 14 §13.7 | C |

#### Exclusão lógica (`deleteAt`)

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-FIF-036 | O ack na FIFO **não apaga a linha**: grava `deleteAt` (= instante do ID + `fifo.dedup.window`) e apaga o corpo (`.body` e a cópia em memória) e os metadados de entrega; a linha curta mantém `id`, posição, `group_id`, `deduplicationId` e `deleteAt`. | 17 §16.7 | I |
| UC-FIF-037 | Se `deleteAt` já passou no ack, a linha é apagada na hora, como na fila comum. O mesmo vale para a mensagem que vai à dead-letter. | 17 §16.7 | I |
| UC-FIF-038 | A linha com `deleteAt` existe em todos os nós, replicada pelo registro + commit do ack, decidido por quórum. | 17 §16.7 | C |
| UC-FIF-039 | A varredura de cada nó apaga as linhas vencidas pelo índice de `deleteAt` a cada `retention.sweep.interval`, sem propagar nada. | 17 §16.7 | I |
| UC-FIF-040 | A linha com `deleteAt` é ignorada pela lista de prontos, pelos contadores de fila e de grupo, por `depth`, pelo `ttl`, pelo `MIN(id)` de `oldestMessageAt`, pelo `GET`, pelo `PUT .../visibility` e por um segundo ack (que respondem como mensagem inexistente). Só a deduplicação e a varredura a enxergam. | 17 §16.7 | U, I |
| UC-FIF-041 | A listagem de IDs e o stream de mensagens do bloco incluem a linha com `deleteAt`, **sem corpo**, e o banco de eventos grava o `deleteAt` de forma idempotente. | 17 §16.7, 05 §7.3 | C |

#### Outros

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-FIF-042 | `autoCreate` aceita `fifo=true`; vale a exigência de `replication.factor` 2 (UC-CNS-037). | 17 §16.2, 15 §14.4 | I, C |
| UC-FIF-043 | Um distribuidor com FIFO entre os destinos mantém a ordem por grupo em cada fila (cada cópia é coordenada pelo primário do grupo da fila destino). | 17 §16.8 | C |
| UC-FIF-044 | Métricas e condições da FIFO: `taba.fifo.deduplicated`, `taba.fifo.forward` (`ok`, `misdirected`, `error`), `taba.fifo.groups.busy`, evento `taba.message.deduplicated`, e a condição `group-stalled` quando a cabeça passa de `stats.topics.group-stalled-deliveries` entregas sem ack. | 17 §16.11, 11 | I, C |
| UC-FIF-045 | A condição `noisy-group` indica o grupo quente de uma FIFO. | 17 §16.10, 11 | I |
| UC-FIF-046 | Em modo desenvolvimento a FIFO funciona igual: o nó é o primário de todos os grupos (sem repasse), o quórum vale 1 e `replication.factor` 1 é aceito. | 17 §16.12 | I |

### GRP: grupos de mensagens e entrega justa

Vale nas filas comuns. Na FIFO o `groupId` ordena e a entrega justa não se aplica (UC-FIF-019). Regras: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-GRP-001 | O `groupId` é opcional; a mensagem sem grupo pertence a um grupo vazio. Um tópico em que ninguém usa `groupId` tem um grupo só e se comporta como a lista de prontos simples. | 13 §12.11 | U, I |
| UC-GRP-002 | O `groupId` é imutável, guardado com a mensagem (memória ou coluna `group_id`) e devolvido no `GET`; na dead-letter a mensagem mantém o grupo. | 13 §12.11 | I |
| UC-GRP-003 | Numa fila comum o grupo **não** ordena nem serializa a entrega: duas mensagens do mesmo grupo podem estar em voo ao mesmo tempo. | 13 §12.11 | I |
| UC-GRP-004 | Cada dono mantém por grupo os contadores de **prontas** e **em processamento**. Publicação: +1 pronta. Entrega: pronta vira em processamento. Vencimento da invisibilidade (e `PUT` com `0`): volta a pronta. Ack, expiração por `ttl` e dead-letter descontam. | 13 §12.11 | U |
| UC-GRP-005 | `tamanho = em processamento + peso × prontas` com o peso de `delivery.fair.ready-weight` (padrão 0,1). Com peso `0` só conta o que está em processamento; com `1` conta o backlog inteiro. | 13 §12.11 | U |
| UC-GRP-006 | A faixa é `⌊log2(tamanho)⌋`, e tamanho menor que 1 conta como 1: tamanho 1 é a faixa 0, 2 e 3 a faixa 1, 4 a 7 a faixa 2. | 13 §12.11 | U |
| UC-GRP-007 | A reposição da lista de prontos percorre os grupos do menor para o maior e toma **uma** mensagem de cada grupo por rodada, até encher a lista. | 13 §12.11 | U |
| UC-GRP-008 | A lista fica ordenada pela faixa e, dentro dela, pela sanfona. Uma mensagem nova entra na posição da faixa do seu grupo, à frente das de grupos maiores. | 13 §12.11 | U |
| UC-GRP-009 | Ao tirar um ID, o `GET` confere a faixa atual do grupo: se mudou desde que o ID entrou na lista, o ID é reposicionado e o próximo é considerado. | 13 §12.11 | U |
| UC-GRP-010 | Efeito observável: um grupo com 10 mil mensagens e outro com 5 mensagens publicados juntos: as 5 do grupo pequeno são entregues antes de o grande ser esgotado. | 13 §12.11 | I |
| UC-GRP-011 | Progresso garantido: a cada `delivery.fair.normal-every` entregas (padrão 10), uma pega a mensagem mais antiga da lista, de qualquer grupo; o grupo grande nunca espera sem limite. Com `0`, a garantia é desligada. | 13 §12.11 | U, I |
| UC-GRP-012 | Acima de `delivery.fair.max-groups`, os grupos novos compartilham um contador único (tratados como um grupo só), que aparece em `GroupStats` com `overflow: true` e sem `groupId`. | 13 §12.11, 06b | U |
| UC-GRP-013 | As contagens por grupo são derivadas: refeitas dos metadados e da coluna `group_id` quando o nó assume blocos ou reinicia. | 13 §12.11 | C |
| UC-GRP-014 | Num distribuidor, o `groupId` é repassado a todas as cópias. | 13 §12.11, 15 §14.2 | I |
| UC-GRP-015 | `GET /metrics/topics/{topic}/groups` devolve mensagens, parcela, idade e taxas por `groupId`; `top=N` devolve os `N` maiores sem paginação; `groupId=` devolve só esse; um grupo sem mensagens e sem atividade some. | 06b, 11 | I |
| UC-GRP-016 | A condição `noisy-group` dispara quando há dois ou mais grupos e o maior ocupa mais de `stats.topics.noisy-group-share` do `depth`. | 11 | U, I |
| UC-GRP-017 | A métrica `taba.delivery.fair.wait` mede a espera por faixa: as faixas baixas esperam menos que as altas. | 11 | I |

### AUT: autenticação e tokens de acesso

Regras: [06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3, e [02-arquitetura.md](02-arquitetura.md), seção 3.4.

#### Quando a autenticação liga

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-AUT-001 | Sem token raiz (sem `root-token` na pasta de segredos, sem `auth.root-token-file` e sem `auth.root-token`), nenhuma rota de cliente exige nada. | 06a §8.1.3 | I |
| UC-AUT-002 | Com um token raiz (pela pasta de segredos, pelo arquivo ou por `auth.root-token`), a autenticação **liga sozinha** e as rotas de cliente exigem `Authorization: Bearer`. | 06a §8.1.3, 10 | I |
| UC-AUT-003 | `auth.enabled=false` força a autenticação desligada, mesmo com a pasta montada. | 10 | I |
| UC-AUT-004 | `auth.enabled=true` sem token raiz impede o nó de subir; um `auth.root-token-file` definido que não existe também. | 06a §8.1.3, 10 | I |
| UC-AUT-005 | Token raiz com menos de 32 caracteres: o nó não sobe. | 06a §8.1.3 | I |
| UC-AUT-006 | O `auth.root-token-file` aceita duas linhas (a atual e a anterior) e é relido a cada `auth.reload-interval`: trocar o segredo não reinicia o nó, os dois valem durante a troca, e os tokens já criados seguem valendo. | 06a §8.1.3, 10 | I |
| UC-AUT-007 | No modo desenvolvimento a autenticação nunca é exigida. | 06a §8.1.3, 04 §6.4 | I |

#### Verificação de uma chamada

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-AUT-008 | Sem token, mal formado, desconhecido, revogado ou vencido: `401` `auth.unauthorized` com `WWW-Authenticate: Bearer`; a resposta é igual nos cinco casos (não diz qual). | 06a §8.1.3 | U, I |
| UC-AUT-009 | `publish` permite só `POST /topics/{topic}/messages`; `consume` permite `GET`, `PUT .../visibility` e `DELETE` de mensagens; `create` permite `POST /topics` e o `autoCreate` (junto de `consume`); `admin` permite `PATCH` e `DELETE` em `/topics/{topic}` **e inclui `create`**. Cada rota fora da permissão: `403` `auth.forbidden`. | 06a §8.1.3 | U, I |
| UC-AUT-010 | `create` só **cria**: alterar e remover tópicos exige `admin`. | 06a §8.1.3 | I |
| UC-AUT-011 | Todo token válido consulta `GET /topics` e `GET /topics/{topic}` dentro dos seus prefixos. | 06a §8.1.3 | I |
| UC-AUT-012 | Prefixos de tópico: o token só alcança tópicos cujo nome **começa com** algum prefixo; um tópico fora: `403` `auth.out-of-scope`. Sem prefixos, alcança todos. | 06a §8.1.3 | U, I |
| UC-AUT-013 | `GET /topics` com token de prefixos devolve só os tópicos dentro deles. | 06a §8.1.3 | I |
| UC-AUT-014 | `POST /topics` com `create` só aceita nome dentro dos prefixos; num distribuidor, também cada um dos seus `prefixes` de distribuição (senão o token injetaria mensagens em filas fora do alcance). O `PATCH` de `prefixes` segue a mesma regra. | 06a §8.1.3 | I |
| UC-AUT-015 | A dead-letter é alcançada pelo mesmo prefixo da fila de origem. | 06a §8.1.3 | I |
| UC-AUT-016 | Num distribuidor, a permissão é conferida só nele; as cópias nas filas destino não são conferidas de novo. | 06a §8.1.3 | I |
| UC-AUT-017 | O token raiz tem acesso total (todas as permissões, sem prefixos) e é o único que usa `/tokens`; nos logs e traces aparece como `token.id=root`. | 06a §8.1.3 | I |
| UC-AUT-018 | Token com `expiresAt` vencido é recusado com `401`; cada nó o ignora e o apaga na varredura local (`retention.sweep.interval`), sem propagar. | 06a §8.1.3, 02 §3.4 | C |
| UC-AUT-019 | A validação é uma consulta local em memória, sem chamada de rede: com os outros nós fora do ar, um token válido continua aceito. | 06a §8.1.3, 02 §3.4 | C |
| UC-AUT-020 | O formato do token é `tb_<id>.<segredo>`, com `id` GUID v7 e segredo de 256 bits aleatórios; o cluster guarda só o hash salgado do segredo (`SHA-256(sal ‖ segredo)`, sal aleatório por token). | 06a §8.1.3 | U |
| UC-AUT-021 | O token completo e o segredo nunca vão para logs nem traces; só o `id` (`token.id`). O `id` do token nunca é atributo de métrica. | 06a §8.1.3, 11 | I |

#### Rotas `/tokens`

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-AUT-022 | `POST /tokens` com o token raiz: `201` `TokenCreated`, com `Location: /tokens/{id}` e o campo `token` (o segredo completo), que aparece **só aqui**. | 06a | I |
| UC-AUT-023 | `POST /tokens` inválido (`permissions` vazia ou desconhecida, `name` fora do padrão, prefixo inválido ou acima de 20, `expiresAt` no passado): `400`. | 06a, 12 | U, I |
| UC-AUT-024 | `/tokens` com um token que não é o raiz: `403` `auth.forbidden`. Sem token: `401`. | 06a | I |
| UC-AUT-025 | `/tokens` com a autenticação desligada ou no modo desenvolvimento: `404` `route.not-found`. | 06a | I |
| UC-AUT-026 | `GET /tokens` é paginada, nunca devolve o segredo, aceita `sort` por `id` e `createdAt` (não `name`, que é alterável: `400`) e o filtro `name` por trecho. | 06a | I |
| UC-AUT-027 | `GET /tokens/{id}`: `200` `Token`, ou `404` `token.not-found`. | 06a | I |
| UC-AUT-028 | `PATCH /tokens/{id}` altera só `name` e `expiresAt` (`null` remove o prazo); permissões e prefixos são imutáveis (`400` `validation.immutable-field`). | 06a | I |
| UC-AUT-029 | `DELETE /tokens/{id}`: `204`; as chamadas seguintes com o token respondem `401`, em **todos** os nós, a partir do commit; um segundo `DELETE`: `404`. | 06a | I, C |
| UC-AUT-030 | Criar, alterar e revogar usam o registro + commit com confirmação de **todos** os nós; com algum nó fora do ar: `503` `cluster.registration-failed` (e `cluster.commit-timeout` conforme o caso). | 06a, 02 §3.4 | C |
| UC-AUT-031 | O segredo nunca trafega entre nós: o `register` leva só o `salt` e o `secretHash`. | 06c, 02 §3.4 | C |
| UC-AUT-032 | Cada token é uma linha da tabela `tokens` do `_node.db`, gravada no commit em cada nó, com o sal e o hash do segredo (nunca o segredo). | 02 §3.4 | I, C |
| UC-AUT-033 | Um nó que entra puxa os tokens (`GET /cluster/tokens`) junto com a tabela de tópicos; até a carga terminar o nó não atende rotas de cliente, então nunca valida com a tabela incompleta. | 02 §3.4 | C |
| UC-AUT-034 | Uma chamada repassada (nó em sincronização ou drain) leva o `Authorization` como veio, e o **destino** confere o token de novo. | 06a, 04 §6.9 | C |
| UC-AUT-035 | As rotas de operação e do cluster não usam o token de acesso. | 06a, 06 §8.0 | I |
| UC-AUT-036 | Eventos: token criado, alterado e revogado (`info`, com o `id`); chamada recusada por `401`/`403` (`warn`, amostrada em volume). `taba.auth.requests` conta por `outcome` (`ok`, `unauthorized`, `forbidden`) e `taba.auth.tokens` traz quantos o nó conhece. | 11 | I |

### LIM: limites e rate limit

Limites fixos de validação estão nos casos de cada área (`TOP`, `MSG`, `AUT`). Aqui, o rate limit e o IP do cliente. Regras: [12-limites-api.md](12-limites-api.md).

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-LIM-001 | Excedido o limite de publicação, consumo ou administrativo: `429` `rate-limit.exceeded` com `Retry-After`. | 12, 06d | I |
| UC-LIM-002 | As categorias são independentes: publicação (`POST .../messages`), consumo (`GET`, `PUT .../visibility`, `DELETE` de mensagem) e administrativo (`/topics`, `/tokens`). Estourar uma não afeta as outras. | 12 | I |
| UC-LIM-003 | O limite é taxa mais estouro (`ratelimit.burst`): picos curtos dentro do burst não são rejeitados. | 12 | U, I |
| UC-LIM-004 | Com token válido (inclusive o raiz), a chave do contador é o `id` do token em cada categoria, mesmo mudando de IP; um `403` também conta no token. | 12 | I |
| UC-LIM-005 | Sem token válido (autenticação desligada, modo desenvolvimento, `401`, rotas de operação), a chave é o IP do cliente. Tentar adivinhar tokens não fica sem limite. | 12 | I |
| UC-LIM-006 | A autenticação vem antes do rate limit, e um token só vira chave **depois de validado** (ninguém esgota o limite de outro citando o `id` dele). | 12 | U, I |
| UC-LIM-007 | Todos os tokens têm o mesmo limite; não há limite próprio por token. | 12 | I |
| UC-LIM-008 | Os contadores são locais a cada nó, sem troca entre nós. | 12 | C |
| UC-LIM-009 | `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` nunca respondem `429`. | 12, 06b | I |
| UC-LIM-010 | `/health/cluster?scope=cluster` e `/metrics/topics` têm categoria própria (`ratelimit.ops.rps`, por IP, com o mesmo burst). | 12, 06b | I |
| UC-LIM-011 | O IP do cliente vem do primeiro endereço do primeiro cabeçalho presente de `client-ip.headers` (padrão `X-Forwarded-For`), só se a conexão vier de `client-ip.trusted-proxies` (vazia: aceita de qualquer origem). Cabeçalho ausente ou inválido: vale o endereço da conexão. | 12 | U, I |
| UC-LIM-012 | Com `client-ip.headers.enabled=false` vale sempre o endereço da conexão. | 12, 10 | U, I |
| UC-LIM-013 | O IP resolvido vale também para `operations.allowed-ips`. | 12, 11 | I |
| UC-LIM-014 | Nó em sincronização ou drain: conta a chamada por IP e a repassa; o destino não conta de novo. A porta do cluster não tem rate limit. | 12, 04 §6.9 | C |
| UC-LIM-015 | `taba.ratelimit.rejected` conta as rejeições por `category` (`publish`, `consume`, `admin`, `ops`) e `key` (`token` ou `ip`). | 11 | I |

### PAG: paginação

Regras: [06a-api-cliente.md](06a-api-cliente.md), seção 8.1.2, e [06d-api-convencoes.md](06d-api-convencoes.md), seção 8.3.4. Vale para `GET /topics`, `GET /tokens`, `GET /metrics/topics` e `GET /metrics/topics/{topic}/groups`.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-PAG-001 | A resposta é `Page<T>`: `items` e, havendo mais, `nextCursor`; **na última página `nextCursor` não existe**. | 06d §8.3.4 | I |
| UC-PAG-002 | `limit` fora da faixa: `400` `validation.out-of-range`. `cursor` malformado: `400` `validation.invalid-cursor`. | 06a, 06d | U, I |
| UC-PAG-003 | Percorrer todas as páginas devolve cada item que existia durante o percurso inteiro, **sem repetir**. | 06a §8.1.2 | I |
| UC-PAG-004 | Itens inseridos ou removidos antes da chave do cursor durante o percurso não deslocam a página seguinte (paginação por chave, não por deslocamento). | 06a §8.1.2 | I |
| UC-PAG-005 | Cursor usado com `sort` ou filtros diferentes dos que o geraram: `400` `validation.invalid-cursor`. | 06a §8.1.2 | U, I |
| UC-PAG-006 | Repetir um cursor devolve a mesma página enquanto os dados não mudarem, e o cursor não expira. | 06a §8.1.2 | I |
| UC-PAG-007 | A ordem é total e estável: toda ordenação termina num campo único e imutável (nome ou `id`); `sort=-createdAt` vale `(createdAt desc, id asc)`; itens com o mesmo `createdAt` não se perdem entre páginas. | 06a §8.1.2 | U, I |
| UC-PAG-008 | Não há `page`, `offset` nem `total`. | 06a §8.1.2 | I |
| UC-PAG-009 | `GET /metrics/topics` não aceita `sort` (`400`) e pagina sempre por nome. | 06b, 11 | I |

### CNV: convenções comuns da API

Regras: [06d-api-convencoes.md](06d-api-convencoes.md). Valem para as três APIs.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNV-001 | JSON em UTF-8, campos em `camelCase`, enumerações em minúsculas com hífen, datas RFC 3339 em UTC com milissegundos (`2026-10-06T12:00:00.000Z`), durações em segundos inteiros, tamanhos em bytes, identificadores GUID v7 em minúsculas. | 06d §8.3.1 | U, I |
| UC-CNV-002 | Campo ausente = não se aplica ao recurso; campo `null` = aplica-se e está sem valor próprio (`ttl: null` usa o padrão). | 06d §8.3.1 | I |
| UC-CNV-003 | Requisição com corpo JSON e `Content-Type` diferente de `application/json`: `415` `media.unsupported`. A exceção é a publicação de mensagem, que aceita qualquer media type. | 06d §8.3.2 | I |
| UC-CNV-004 | `X-Request-Id` de 1 a 128 caracteres de `A-Za-z0-9_-.:`: devolvido igual na resposta. Sem ele, o nó gera um GUID v7. Está **sempre** na resposta e igual ao `requestId` do corpo de erro. | 06d §8.3.2 e 8.3.3 | I |
| UC-CNV-005 | `traceparent` na requisição: as operações do nó entram no mesmo trace. | 06d §8.3.2, 11 | I |
| UC-CNV-006 | Toda resposta de erro (`4xx` e `5xx`) de rota JSON é `ErrorResponse`: `code` (formato `grupo.motivo`), `message`, `requestId` e, quando há mais de um problema, `details` (`field`, `code`, `message`). O status vai só na linha de status. | 06d §8.3.3 | U, I |
| UC-CNV-007 | `Retry-After` só no cabeçalho (nunca no corpo), em `429`, `503` e `403` por freezing; `WWW-Authenticate: Bearer` no `401`; `Location` no `201`. | 06d §8.3.2 | I |
| UC-CNV-008 | Cada `code` do catálogo tem um status HTTP fixo; o catálogo inteiro é conferido por teste de tabela (código → status). | 06d §8.3.5 e 8.3.6 | U |
| UC-CNV-009 | Campo desconhecido na requisição: `400` `validation.unknown-field`. Campo obrigatório ausente: `400` `validation.required`, com o campo em `details`. | 06d §8.3.1 | U, I |
| UC-CNV-010 | `POST`, `PATCH` e `DELETE` que não esperam corpo ignoram o que vier; `204` nunca tem corpo. | 06d §8.3.1 | I |
| UC-CNV-011 | A porta de cliente responde `404` `route.not-found` a `/cluster/*`; a porta do cluster responde `404` a tudo que não seja `/cluster/*`. | 06d, 06 §8.0 | I, C |
| UC-CNV-012 | Falha inesperada do nó: `500` `internal.error` com `ErrorResponse`. | 06d §8.3.5 | U, I |
| UC-CNV-013 | As rotas `/health/*` devolvem `HealthResponse` (nunca `ErrorResponse`), e uma falha **depois do `200`** de um stream NDJSON usa a linha `{"error":"..."}`. | 06d §8.3.3 | I, C |
| UC-CNV-014 | Erro dentro de um resultado parcial (`200` do distribuidor) leva `error` com `code` e `message`, **sem status**: o status do item é o do `code` no catálogo. | 06d §8.3.3 | I |
| UC-CNV-015 | Um `503` ou `504` de operação que cria ou apaga (tópico, token) pode já ter sido aplicado em parte do cluster: ao repetir, `409` `topic.already-exists` (criação) ou `404` (remoção) significam que a operação já tinha sido aplicada. | 06d §8.3.5 | C |
| UC-CNV-016 | `Accept: application/openmetrics-text` no `/metrics` pede OpenMetrics; o padrão é o texto do Prometheus. | 06d §8.3.2, 06b | I |
| UC-CNV-017 | Os DTOs reutilizados `Ack`, `StreamEnd` e `ConditionEntry` têm os campos e tipos definidos em 06d §8.3.7 (`severity` de `ConditionEntry` só vale `info`, `warning` ou `critical`). | 06d §8.3.7 | U |

### CNT: conectores

Regras: [19-conectores.md](19-conectores.md) e [19a-conectores-linguagens.md](19a-conectores-linguagens.md). O nível é `K` para todos: cada caso vale para **cada um dos dez conectores** (C#, Java, Rust, C, C++, Python, PHP, Ruby, Go e Node.js), rodando as fixtures e os cenários de `connectors/contrato/`. Um conector só está coberto num caso quando o teste existe **naquele conector**.

| Código | Caso de uso | Fonte | Nível |
|---|---|---|---|
| UC-CNT-001 | O conector oferece as nove operações (`createTopic`, `getTopic`, `listTopics`, `updateTopic`, `deleteTopic`, `publish`, `consume`, `ack`, `changeVisibility`), cada uma montando o verbo, o caminho, a query, os cabeçalhos e o corpo da rota correspondente. | 19 §18.3 | K |
| UC-CNT-002 | `baseUrl` é obrigatório. Os padrões são `connectTimeout` 5 s, `requestTimeout` 30 s, `maxAttempts` 3 e `userAgent` `taba-<linguagem>/<versão>`. | 19 §18.3 | K |
| UC-CNT-003 | Com `token`, todas as chamadas levam `Authorization: Bearer`; sem ele, nenhuma leva o cabeçalho. | 19 §18.3 | K |
| UC-CNT-004 | `publish` recebe os bytes e o `Content-Type`; `publishJson` e `publishText` só escolhem o `Content-Type`; as opções `groupId`, `deduplicationId`, `contentType` e `requestId` viram query e cabeçalhos corretos. O conector não valida o corpo. | 19 §18.3 | K |
| UC-CNT-005 | `consume` envia só os valores que a pessoa informou (`maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate`, `volatile`, `idleTimeout`, `fifo`, `tags` como `tag.<chave>`) e **não aplica padrões próprios**. | 19 §18.3 | K |
| UC-CNT-006 | `ack` e `changeVisibility` mandam o `receiptToken` na query. | 19 §18.3 | K |
| UC-CNT-007 | `listTopics` aceita `limit`, `cursor` e `name`, e oferece um iterador sobre todas as páginas onde a linguagem permite. | 19 §18.3 | K |
| UC-CNT-008 | O tempo limite de leitura do `consume` é `waitTime + 10 s`, nunca o `requestTimeout`: um `waitTime` de 20 s com `requestTimeout` de 30 s não derruba a chamada. | 19 §18.4 | K |
| UC-CNT-009 | Estouro de tempo no `consume` **não** é repetido: o conector devolve o erro de tempo (`client.timeout`). | 19 §18.4 | K |
| UC-CNT-010 | `429` e `503` são repetidos até `maxAttempts` esperando o `Retry-After`; sem ele, esperam 1 s, 2 s, 4 s. | 19 §18.4 | K |
| UC-CNT-011 | Num `503` de operação que cria ou apaga tópico, o `409` `topic.already-exists` e o `404` da repetição contam como sucesso. | 19 §18.4 | K |
| UC-CNT-012 | `502` e `504` só são repetidos na publicação em fila FIFO; fora dela o erro sobe sem repetir. | 19 §18.4 | K |
| UC-CNT-013 | Falha de rede **antes** de enviar o corpo (recusa de conexão, DNS) é repetida como um `503`; **depois** de enviar o corpo não é repetida. | 19 §18.4 | K |
| UC-CNT-014 | Qualquer `4xx` (menos `429`) nunca é repetido. | 19 §18.4 | K |
| UC-CNT-015 | O `body` da mensagem sai como **bytes**: JSON vira o texto JSON tal qual veio; `text/*` em UTF-8 vira a string em UTF-8; `bodyEncoding: base64` vira os bytes decodificados. | 19 §18.4 | K |
| UC-CNT-016 | A `Message` traz `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt`, `invisibleUntil` e, quando vierem, `groupId`, `deduplicationId`, `deadLetteredAt` e `deadLetterReason`; campos desconhecidos na resposta são ignorados. | 19 §18.4 | K |
| UC-CNT-017 | Uma só forma de erro por conector, com `status`, `code`, `message`, `requestId`, `retryAfter` e `details`; erro de rede e de tempo têm `status` zero e `code` `client.network` ou `client.timeout`. | 19 §18.4 | K |
| UC-CNT-018 | O conector envia `Accept: application/json`, `User-Agent` e, se informados, `X-Request-Id` e `traceparent`, e lê `Retry-After` e `X-Request-Id` da resposta. | 19 §18.4 | K |
| UC-CNT-019 | HTTP/1.1 com conexão persistente quando a biblioteca HTTP oferece sem custo; sem HTTP/2. | 19 §18.4 | K |
| UC-CNT-020 | Segurança de thread: o cliente é seguro para uso concorrente em Java, C#, Go, Rust e Node; em C, C++, Python, PHP e Ruby vale um cliente por thread, e o README de cada pasta diz como. | 19 §18.4 | K |
| UC-CNT-021 | O leitor de JSON mínimo passa o conjunto comum de documentos de `contrato/fixtures/` (escapes, Unicode e pares substitutos, números grandes, aninhamento profundo, espaços e quebras de linha, valores vazios) e entrega o `body` como trecho de texto sem reserializar. | 19 §18.5 | K |
| UC-CNT-022 | O base64 próprio (C, C++, Rust) codifica e decodifica como o padrão, inclusive com preenchimento (`=`) e bytes arbitrários. | 19 §18.5 | K |
| UC-CNT-023 | Sem dependências externas além da biblioteca-padrão da linguagem (no C, só a `libcurl`); no C++ o cabeçalho único embrulha a `libtaba`. | 19 §18.5 | K |
| UC-CNT-024 | O código compila e passa os testes **na versão-base e na mais recente** da linguagem, sem usar recurso posterior à versão-base. | 19 §18.6 e 18.7 | K |
| UC-CNT-025 | O texto vai e volta sempre em UTF-8, de forma explícita, qualquer que seja o *charset* local. | 19 §18.6 | K |
| UC-CNT-026 | As fixtures de cada rota conferem a requisição montada e a leitura de uma resposta pronta: `ErrorResponse`, `Page<T>`, corpo JSON, texto e base64, e o `Retry-After`. | 19 §18.7 | K |
| UC-CNT-027 | Os cenários de integração passam contra um Taba em modo desenvolvimento: criar tópico, publicar, consumir com long polling, ack, invisibilidade, fila FIFO com duplicata, distribuidor com falha parcial e token sem permissão. | 19 §18.7 | K |
| UC-CNT-028 | Cada conector declara a versão do contrato da API que implementa, e ignora campos novos que o servidor acrescente. | 19 §18.7 | K |
| UC-CNT-029 | `updateTopic` sai como `PATCH` de verdade, sem `X-HTTP-Method-Override`, em cada conector, e o conector não faz conexão HTTP própria para esse verbo. | 19 §18.6, 08a D72 | K |
| UC-CNT-030 | O conector usa a TLS da plataforma, sem pilha de TLS própria (no Rust, o `rustls`), exige TLS 1.2 como mínimo (onde a biblioteca permite configurar) e **nunca** desliga a verificação do certificado. | 19 §18.6, 08a D69 | K |
| UC-CNT-031 | O `subscribe(handler)` (long polling, chamada do handler e ack automático). **(pendente: D71.1)** | 19 §18.3, 08b | K |
| UC-CNT-032 | Nas linguagens com modelo assíncrono (C#, Java, Python 3.5+, Node.js, Go e Rust), as nove operações têm a forma assíncrona, com a mesma montagem de requisição, a mesma repetição de chamadas, o mesmo tempo limite do `consume` e os mesmos erros da síncrona. C, C++, PHP e Ruby só têm a síncrona. | 19 §18.3, 08a D71 | K |
