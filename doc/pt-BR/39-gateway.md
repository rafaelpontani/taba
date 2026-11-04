---
título: Gateway para os SDKs da AWS
resumo: Os gateways de SQS e SNS dentro do nó: uma aplicação que usa o SDK da AWS aponta o endpoint para uma porta do Taba e mantém o código. O que é suportado (filas padrão e FIFO, protocolos JSON e Query, tópicos do SNS que publicam em filas, SigV4), como uma chamada vira chamada à API do cliente, como funcionam o arquivo de credenciais e as permissões, onde ficam os atributos da mensagem, como um tópico e uma assinatura são guardados e cada diferença do SQS e do SNS que a aplicação precisa conhecer.
tipo: guia
status: parcial
verificado-em: 2026-11-04 @ a3bdab9
fontes: [crates/taba-node/src/gateway/mod.rs, crates/taba-node/src/gateway/backend/mod.rs, crates/taba-node/src/gateway/envelope/mod.rs, crates/taba-node/src/gateway/naming/mod.rs, crates/taba-node/src/gateway/aws/sigv4/mod.rs, crates/taba-node/src/gateway/aws/credentials/mod.rs, crates/taba-node/src/gateway/aws/errors/mod.rs, crates/taba-node/src/gateway/sqs/mod.rs, crates/taba-node/src/gateway/sqs/queues/mod.rs, crates/taba-node/src/gateway/sqs/messages/mod.rs, crates/taba-node/src/gateway/dispatch/mod.rs, crates/taba-node/src/gateway/sns/mod.rs, crates/taba-node/src/gateway/sns/protocol/mod.rs, crates/taba-node/src/gateway/sns/topics/mod.rs, crates/taba-node/src/gateway/sns/publish/mod.rs, crates/taba-node/src/gateway/sns/subscriptions/mod.rs, crates/taba-node/src/gateway/aws/auth/mod.rs, crates/taba-node/tests/gateway_sqs.rs, crates/taba-node/tests/gateway_sns.rs, connectors/gateway-contract/README.md]
---

# Gateway para os SDKs da AWS

> Leia antes: [16-api-cliente.md](16-api-cliente.md) e [14-seguranca.md](14-seguranca.md).

Uma aplicação que já envia e lê do Amazon SQS, ou publica em tópicos do Amazon SNS que alimentam filas SQS, pode usar o Taba **sem mudar o código**: ela aponta o endpoint do SDK (`AWS_ENDPOINT_URL_SQS` e `AWS_ENDPOINT_URL_SNS`, ou o parâmetro de endpoint do client) para o gateway de um nó do Taba. O gateway faz parte do nó e da imagem dele; vem **desligado** e o SQS e o SNS são atendidos em **uma porta só** (`gateway.aws.port`, 9324), cada um ligado pela sua chave ([22-configuracao.md](22-configuracao.md#gateway-para-os-sdks-da-aws)).

**Situação.** Hoje o gateway atende as **filas padrão e FIFO do SQS** e os **tópicos do SNS que publicam em filas SQS**. O RabbitMQ (AMQP) está planejado.

## Como uma chamada é atendida

```
SDK da AWS (SQS) ──JSON ou Query──┐
SDK da AWS (SNS) ──Query───────────────┴▶ gateway.aws.port ──▶ gateway/dispatch ──▶ gateway/sqs ┐
                                                                                    └─▶ gateway/sns ┴─▶ API do cliente (em processo) ──▶ motor do nó
```

1. A porta entrega o pedido ao SQS ou ao SNS pelo **serviço no escopo da credencial** da assinatura (`.../us-east-1/sns/aws4_request`), ou pelo cabeçalho `X-Amz-Target: AmazonSQS.<Ação>`, ou, numa chamada sem assinatura (só com a autenticação do Taba desligada), pela `Action` do formulário. Um protocolo desligado responde `404` com a chave que o liga. Depois o protocolo lê o pedido, confere a assinatura (abaixo) e o decodifica. No SQS, o protocolo **JSON** (`X-Amz-Target: AmazonSQS.<Ação>`, SDKs atuais) e o **Query** (um formulário, respostas em XML, SDKs mais antigos) são atendidos. O SNS só tem o protocolo Query.
2. Cada ação do SQS vira uma ou poucas chamadas à API do cliente, feitas **em processo** no mesmo roteador da `api.port`, com o token da chave de acesso. Assim, autenticação, permissões e prefixos do token, rate limit, drain (um nó em drain encaminha a chamada a outro nó, ou responde `ServiceUnavailable`), métricas e replicação são os de qualquer outro cliente. O rate limit continua contando pelo IP real do cliente (o endereço da conexão e o `X-Forwarded-For` do proxy são repassados).
3. A resposta sai no protocolo que o client usou, com os códigos de erro da AWS.

O gateway **não guarda estado**: todos os nós enxergam as mesmas filas e os mesmos tópicos, porque são filas e distribuidores do Taba.

## O que é suportado

| Área | Suportado |
|---|---|
| Filas | `CreateQueue`, `DeleteQueue`, `GetQueueUrl`, `ListQueues` (prefixo, `MaxResults`, `NextToken`), `PurgeQueue` |
| Atributos e tags | `GetQueueAttributes`, `SetQueueAttributes`, `TagQueue`, `UntagQueue`, `ListQueueTags` |
| Mensagens | `SendMessage`, `SendMessageBatch`, `ReceiveMessage` (long polling, até 10), `DeleteMessage`, `DeleteMessageBatch`, `ChangeMessageVisibility`, `ChangeMessageVisibilityBatch` |
| Recursos da mensagem | `MessageAttributes` (String, Number, Binary) com `MD5OfMessageAttributes`, `MD5OfMessageBody`, `DelaySeconds` por mensagem e por fila (filas padrão), `ApproximateReceiveCount`, `SentTimestamp`, `SenderId` |
| Filas FIFO | Nomes `.fifo` com `FifoQueue=true`, `MessageGroupId`, `MessageDeduplicationId`, `ContentBasedDeduplication`, `SequenceNumber`, `MessageGroupId` e `MessageDeduplicationId` de volta no recebimento, fila de dead-letter nomeada na `RedrivePolicy` |
| Não suportado (erro claro) | `AddPermission`, `RemovePermission`, as tarefas de mover mensagens, `DelaySeconds` em fila FIFO, uma fila de dead-letter compartilhada por várias filas de origem |
| Tópicos do SNS | `CreateTopic` (de novo com o mesmo nome responde o mesmo ARN), `DeleteTopic`, `ListTopics`, `GetTopicAttributes`, `SetTopicAttributes`, `TagResource`, `UntagResource`, `ListTagsForResource` |
| Assinaturas do SNS | `Subscribe` com o protocolo `sqs`, `Unsubscribe`, `ListSubscriptions`, `ListSubscriptionsByTopic`, `GetSubscriptionAttributes`, `SetSubscriptionAttributes` |
| Publicação no SNS | `Publish`, `PublishBatch` (até 10), `Subject`, `MessageAttributes` (String, Number, Binary), `MessageStructure=json` |
| SNS não suportado (erro claro) | `RawMessageDelivery=true`, `FilterPolicy` e `FilterPolicyScope`, `DeliveryPolicy` e a `RedrivePolicy` de uma assinatura, tópicos FIFO (`.fifo`, `FifoTopic`), os protocolos `http`, `https`, `email`, `email-json`, `sms`, `lambda`, `application` e `firehose`, SMS e push móvel (`PhoneNumber`, `TargetArn`, aplicações de plataforma), `ConfirmSubscription`, `AddPermission`, `RemovePermission`, as políticas de proteção de dados |
| Aceito e ignorado | A `Policy` da fila (uma assinatura do SNS sempre a grava), `SqsManagedSseEnabled`, `KmsMasterKeyId`, `KmsDataKeyReusePeriodSeconds`, `DeduplicationScope`, `FifoThroughputLimit`, `ReceiveRequestAttemptId`; `ListDeadLetterSourceQueues` responde uma lista vazia; o `DisplayName` e a `Policy` de um tópico do SNS |

## As chaves de acesso

O SigV4 nunca envia o segredo, então o nó não consegue conferir uma assinatura com o hash que o token do Taba guarda. O gateway lê um **arquivo de credenciais** (`gateway.aws.credentials-file`) que liga cada `accessKeyId` ao seu `secret` e ao `token` do Taba cujas permissões e prefixos valem:

```json
{ "AKIAEXEMPLO": { "secret": "wJalrXUtnFEMI...", "token": "tb_1a2b3c.xxxxxxxxxxxx" } }
```

- O arquivo é montado como segredo em cada nó e lido de novo a cada `auth.reload-interval`. Um arquivo inválido na partida impede o nó de subir; um que fica inválido depois é ignorado e as chaves anteriores continuam valendo.
- A assinatura precisa ser recente (15 minutos para cada lado), cobrir o corpo e nomear o serviço a que se destina (`sqs` ou `sns`, que é também o que a encaminha; o mesmo arquivo serve aos dois). Segredo errado é `SignatureDoesNotMatch`, chave desconhecida `InvalidClientTokenId`, assinatura ausente `MissingAuthenticationToken`, antiga `RequestExpired`.
- As permissões do token decidem o que a chave pode (`create` para criar filas, `publish` para enviar, `consume` para receber, apagar e esconder, `manage` para apagar e esvaziar); uma recusa é `AccessDenied` (`AuthorizationError` no SNS). Um token com prefixos de tópico confina a chave a essas filas, e o `ListQueues` mostra só elas. No SNS valem as mesmas permissões sobre o tópico: `create` para criá-lo, `manage` para apagá-lo, assinar e cancelar a assinatura, `publish` para publicar; uma assinatura também exige que o prefixo da fila esteja dentro dos prefixos do token, e o `ListTopics` mostra só os tópicos que o token alcança.
- **Com a autenticação do Taba desligada** não precisa de arquivo, qualquer credencial é aceita e nenhuma assinatura é conferida (o nó avisa na partida).
- **Com a autenticação ligada e sem chaves** toda chamada é recusada (o nó avisa na partida).
- Credenciais temporárias de uma IAM role não podem ser conferidas sem a AWS: a aplicação precisa de chaves estáticas.

**O exemplo do Compose.** O `docker compose up -d` sobe o cluster com os dois gateways **ligados** (porta `9324` no HAProxy, para o SQS e o SNS) e uma chave de acesso de demonstração: o serviço `init` grava o `aws-credentials.json` no volume `secrets` com a chave `taba` / segredo `taba-secret`, que age com o **token raiz**. Serve só para experimentar o Taba; para qualquer uso real troque o arquivo pelas chaves das suas aplicações, cada uma ligada a um token próprio. `TABA_GATEWAY_SQS_ENABLED=false` e `TABA_GATEWAY_SNS_ENABLED=false` desligam. O padrão do nó não muda: desligado.

## Como uma fila é guardada

Uma fila é uma fila do Taba com o **mesmo nome**, então um client nativo alcança as mesmas mensagens pelo mesmo nome. Uma fila **FIFO** é a fila FIFO do Taba (`fifo: true`) do nome **sem** `.fifo`: `orders.fifo` é o tópico `orders`, porque um nome de tópico não aceita ponto. O nome diz o tipo, então uma fila padrão `orders` e uma FIFO `orders.fifo` não podem existir juntas (o segundo `CreateQueue` é `QueueNameExists`) e cada nome só encontra a fila do próprio tipo.

| SQS | No Taba |
|---|---|
| `MessageRetentionPeriod` | `ttl` da fila |
| `MaximumMessageSize` | `maxMessageBytes` |
| `RedrivePolicy` (`maxReceiveCount`) | `maxDeliveries`; as dead letters ficam no Taba em `<tópico>-dead-letter`, e a fila nomeada em `deadLetterTargetArn` é um alias dela (abaixo) |
| `FifoQueue` | `fifo` da fila, vindo do nome `.fifo`; não pode ser mudado |
| `ContentBasedDeduplication` | A tag `sqs.cbd` |
| `VisibilityTimeout`, `DelaySeconds`, `ReceiveMessageWaitTimeSeconds` | Tags `sqs.vt`, `sqs.delay`, `sqs.wait`, que as ações de tag nunca mostram |
| Tags da fila | Tags da fila (limites abaixo) |
| `ApproximateNumberOfMessages*` | `GET /stats/topics/{fila}` |
| `MessageGroupId`, `MessageDeduplicationId` | `groupId`, `deduplicationId` da mensagem |
| `SequenceNumber` | O `id` da mensagem (um UUID que cresce dentro de cada grupo) como número decimal |
| `QueueUrl` | `<gateway.sqs.public-url ou Host>/000000000000/<nome>` |
| `QueueArn` | `arn:aws:sqs:<região da assinatura>:000000000000:<nome>` |

**A fila de dead-letter.** O Taba guarda as dead letters de `orders` em `orders-dead-letter`, uma por fila de origem, mas uma aplicação do SQS cria a própria fila de dead-letter e a nomeia na `RedrivePolicy`. O gateway faz dessa fila um **alias**: marca-a (tag `sqs.dlqof`) como a fila de dead-letter de `orders`, e toda ação sobre ela (receber, apagar, contagens, esvaziar) vai para `orders-dead-letter`; `orders` guarda o nome na tag `sqs.dlq` para mostrá-lo de volta na `RedrivePolicy`. A fila precisa existir, ser do mesmo tipo da origem (FIFO com FIFO) e não ser a fila de dead-letter de outra origem (`UnsupportedOperation`). Apagar a origem a libera. As mensagens da fila alias não são FIFO no Taba, então a ordem de uma fila FIFO se perde na dead-letter dela.

**O grupo e o id de deduplicação.** O SQS aceita mais pontuação que o Taba em `MessageGroupId` e `MessageDeduplicationId`. Um feito de `A-Za-z0-9_-.:` vai como está; outro vai como `h.` e 40 dígitos hexadecimais do seu SHA-256, e a mensagem é guardada num envelope que mantém o original, que o recebimento devolve. Com `ContentBasedDeduplication` e sem `MessageDeduplicationId`, o id de deduplicação é o SHA-256 do corpo.

Os padrões de uma fila (visibilidade, atraso, espera) ficam na memória do gateway por 2 segundos: uma mudança pelo gateway é vista na hora, uma pela API nativa ou por outro nó, dentro desse tempo.

**A mensagem.** O corpo de uma mensagem enviada sem atributos é guardado **como veio**, como `text/plain`: um consumidor nativo lê exatamente o que a aplicação enviou, e o que um client nativo publica é recebido pelo SQS. Com atributos, a mensagem é guardada como JSON, `{"tabaSqs":1,"body":"...","attributes":{"nome":{"type":"String","value":"..."}}}`, que um consumidor nativo consegue ler e reconhecer por `tabaSqs`. O servidor e os formatos em disco não mudam. O `ReceiptHandle` é `base64url("<id>:<receiptToken>")`, então apagar ou esconder uma mensagem não exige estado.

## SNS: tópicos e assinaturas

O SNS entrega uma publicação a todas as filas assinadas no tópico. O Taba já tem isso: o **distribuidor**, um tópico que não guarda mensagens e copia cada publicação para as filas cujo nome começa com um dos seus **prefixos**, em todos os nós. Assim, o gateway não faz fan-out e não guarda estado:

| SNS | No Taba |
|---|---|
| Tópico | Um distribuidor com o mesmo nome, criado pelo `CreateTopic`. O nome é um nome de tópico do Taba: letras, dígitos, `-` e `_`, até 100 caracteres, sem terminar em `-dead-letter` |
| `TopicArn` | `arn:aws:sns:<região da assinatura>:000000000000:<nome>` |
| Assinatura de uma fila | Um prefixo na lista do distribuidor: o nome da fila (sem `.fifo`) |
| `SubscriptionArn` | O `TopicArn` e o nome da fila: `arn:aws:sns:<região>:000000000000:<tópico>:<fila>`. É feito dos dois nomes, então não precisa de estado, e é sempre confirmada (nunca pendente) |
| `Endpoint` da assinatura | O ARN da fila (`arn:aws:sqs:...:<fila>`). A fila precisa existir, ser uma fila SQS deste gateway e, no `.fifo`, ser FIFO |
| Tags do tópico | Tags do distribuidor (mesmos limites das tags da fila) |
| `Publish` | Uma publicação no distribuidor; o nó faz as cópias |
| `GetTopicAttributes` | `TopicArn`, `Owner`, `DisplayName`, `SubscriptionsConfirmed` (o número de prefixos), `SubscriptionsPending` e `SubscriptionsDeleted` (0) |

O `Subscribe` é idempotente (a mesma fila dá o mesmo ARN), uma assinatura vale só para as publicações **depois** dela (o distribuidor aplica a mudança a partir do commit), um tópico guarda até **20** assinaturas (`SubscriptionLimitExceeded`; um prefixo comum aos nomes de várias filas cobre todas), e o `DeleteTopic` e o `Unsubscribe` são idempotentes. Um tópico também é um distribuidor para a API nativa: um prefixo posto com `PATCH /topics/{tópico}` é uma assinatura para o SNS, e o que um client nativo publica nele chega às filas sem o envelope do SNS.

**A notificação.** A fila recebe o que o SNS entrega quando o `RawMessageDelivery` está desligado, como o texto de um documento JSON guardado como `text/plain`:

```json
{"Type":"Notification","MessageId":"...","TopicArn":"arn:aws:sns:us-east-1:000000000000:orders","Subject":"...","Message":"...","Timestamp":"2026-11-04T10:00:00.000Z","MessageAttributes":{"color":{"Type":"String","Value":"blue"}}}
```

`Subject` e `MessageAttributes` só aparecem quando a publicação os tem. O `MessageId` é feito pelo gateway antes da publicação, porque as cópias são mensagens diferentes do Taba e quem publica precisa do id único que está em todas. Os campos da assinatura (`Signature`, `SigningCertURL`, `UnsubscribeURL`) **não** estão lá: não há certificado da AWS para assinar, então uma aplicação que verifica a assinatura da notificação falha. O `MessageStructure=json` é lido: a fila recebe o texto `sqs` da mensagem, ou o `default` (a chave `default` é obrigatória, como no SNS).

**O que é recusado, nunca aceito e ignorado.** O distribuidor copia e não faz mais nada, então o que exigiria mais é um erro que o nomeia: `RawMessageDelivery=true` (a fila sempre recebe a notificação, com a mensagem no campo `Message`; uma aplicação que lê o corpo cru precisa ler esse campo para migrar), `FilterPolicy` (aceitá-la entregaria tudo), qualquer protocolo que não seja `sqs`, tópicos FIFO (o distribuidor não é FIFO), `MessageGroupId` e `MessageDeduplicationId` numa publicação, e os atributos de tópico ou assinatura que não estão na lista acima. O `DisplayName` e a `Policy` de um tópico são aceitos e não têm efeito (não há entrega por SMS nem e-mail, e o Taba não aplica política; o lado do SQS faz o mesmo com a política de uma fila).

**Uma cópia que falha.** Se o nó recusa a publicação (mais destinos que `topics.distributor.max-targets`), nada é copiado e a chamada falha. Se só algumas cópias falham, a chamada responde `InternalError` (falha do lado do servidor, então o SDK repete) e as filas que tinham a cópia recebem uma duplicata, o que o "ao menos uma vez" permite. O `PublishBatch` publica cada entrada por conta própria (até 10, uma publicação cada) e responde quais falharam.

## Diferenças do SQS

| Tema | SQS | Gateway do Taba |
|---|---|---|
| Nomes de fila | Até 80 caracteres, `.fifo` para FIFO | Os mesmos; um nome terminado em `-dead-letter` (antes do `.fifo`) é recusado (pertence ao Taba); `orders` e `orders.fifo` são o mesmo tópico, então só um deles existe |
| Tags | Chaves e valores livres | Chaves de 1 a 64 e valores de até 128 caracteres de `A-Za-z0-9_-.:`; até 20 tags por fila, três delas usadas pelo gateway quando a fila define os três atributos acima; o prefixo `sqs.` é reservado |
| Mensagens atrasadas nas contagens | `ApproximateNumberOfMessagesDelayed` | Contam como não visíveis (PRB-093); `Delayed` é sempre 0 |
| Padrão de `MessageRetentionPeriod` | 4 dias | O padrão do nó (14 dias) enquanto a fila não define |
| Sem `RedrivePolicy` | Sem limite de recebimentos | `maxDeliveries` do nó (10 por padrão); depois disso a mensagem vai para `<nome>-dead-letter` |
| `ReceiveMessage` da mesma mensagem | `ApproximateFirstReceiveTimestamp` | Não devolvido |
| `ReceiveRequestAttemptId`, `MessageGroupId` e `MessageDeduplicationId` em fila padrão | Usados na FIFO | Ignorados |
| Deduplicação em fila FIFO | O id vale na fila toda (por padrão) por 5 minutos | O id vale **dentro do grupo** por `fifo.dedup.window` (5 minutos por padrão): o mesmo id em dois grupos são duas mensagens |
| Recebimento de fila FIFO | Várias mensagens do mesmo grupo numa chamada | Uma mensagem por grupo em andamento: a próxima de um grupo é entregue depois do delete da anterior. `MaxNumberOfMessages` traz mensagens de grupos diferentes |
| `DelaySeconds` em fila FIFO | Permitido na fila | O Taba não tem atraso em fila FIFO: uma mensagem com `DelaySeconds` é `InvalidParameterValue` e o atributo da fila precisa ser 0 (`UnsupportedOperation`) |
| Envio FIFO sem id de deduplicação | `InvalidParameterValue`, salvo `ContentBasedDeduplication` | O mesmo; sem `MessageGroupId`, `MissingParameter` |
| Fila de dead-letter | Qualquer fila do mesmo tipo, compartilhada por várias origens | Uma por fila de origem, criada por você e aliasada (acima) |
| Erros | `QueueDoesNotExist` com `AWS.SimpleQueueService.NonExistentQueue` | Os dois nomes, como a AWS: o protocolo JSON manda o primeiro em `__type` e o segundo em `x-amzn-query-error`; o Query manda o segundo |
| Rate limit | Por conta | Os `ratelimit.*` do nó, por chave; `ThrottlingException` / `RequestThrottled` |
| Nó em drain | n/a | A chamada é encaminhada a outro nó; sem nenhum, `ServiceUnavailable` com `Retry-After` (falha do lado do servidor, então o SDK tenta de novo) |

### Diferenças do SNS

| Tema | SNS | Gateway do Taba |
|---|---|---|
| Filas que uma assinatura alcança | Exatamente a fila assinada | Toda fila cujo nome **começa com** o nome assinado (PRB-098): assinar `orders` também copia para `orders-eu`. Nomeie as filas de modo que nenhum nome comece com outro |
| Nomes | Um tópico e uma fila podem ter o mesmo nome | Dividem o espaço de nomes do Taba: um tópico `orders` e uma fila `orders` não existem juntos (PRB-098); `CreateTopic` num nome de fila é `InvalidParameter` |
| Assinaturas por tópico | Milhões | 20 (`SubscriptionLimitExceeded`) |
| Assinantes | HTTP, e-mail, SMS, Lambda, SQS, ... | Só filas SQS deste gateway, que precisam existir ao assinar |
| Entrega a uma fila | A notificação, ou a mensagem crua com `RawMessageDelivery` | Sempre a notificação (`RawMessageDelivery=true` é recusado) |
| Assinatura da notificação | Assinada pela AWS | Sem assinatura |
| `FilterPolicy` | Filtra por assinatura | Recusada |
| Tópicos FIFO | `.fifo` | Recusados (PRB-100) |
| Mudanças simultâneas das assinaturas | Independentes | Dois `Subscribe` ou `Unsubscribe` do mesmo tópico no mesmo instante, em nós diferentes, podem perder um (PRB-099) |
| `PublishBatch` | Um pedido | Uma publicação por entrada, então cada entrada conta para o rate limit |
| Tamanho da mensagem | 256 KiB | O mesmo; a soma de um lote também |
| Erros | `NotFound`, `InvalidParameter`, `AuthorizationError`, `Throttled` | Os mesmos códigos, no XML do protocolo Query |

## Testes

Os SDKs nativos da AWS de seis linguagens (C#, Java v1 e v2, Rust, Go, Python, Node.js; uma versão atual e uma mais antiga, do protocolo Query, de cada) rodam os mesmos arquivos de cenário contra o gateway, e contra emuladores para provar que os cenários estão certos: [connectors/gateway-contract/README.md](../../connectors/gateway-contract/README.md). Os testes do próprio nó estão em `crates/taba-node/src/gateway/**/tests.rs` (unidade) e `crates/taba-node/tests/gateway_sqs.rs` e `crates/taba-node/tests/gateway_sns.rs` (um nó real, pedidos escritos à mão).

## Fontes e provas

- `crates/taba-node/src/gateway/` (o mapa de módulos está em [04-arquitetura.md](04-arquitetura.md)), `config/params/mod.rs` (as chaves).
- Casos de uso `UC-GTW-001` a `UC-GTW-030` em [26a](26a-casos-de-uso-implementados.md); testes em `crates/taba-node/src/gateway/**/tests.rs`, `crates/taba-node/tests/gateway_sqs.rs` e `crates/taba-node/tests/gateway_sns.rs`.
- A suíte de contrato dos SDKs nativos: `connectors/gateway-contract/`.
