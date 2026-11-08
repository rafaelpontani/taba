---
título: Conectores
resumo: As bibliotecas de cliente do Taba — o contrato comum (as nove operações, repetição, erros, corpo), o estado real de cada linguagem (Rust, C#, Java, Python, Node.js, Go, PHP e Ruby prontos; duas planejadas), como usar e como escrever um novo.
tipo: referência
status: parcial
verificado-em: 2026-11-07 @ 4fbb808
fontes: [connectors/README.md, connectors/rust/client, connectors/rust/cli, connectors/csharp, connectors/java, connectors/node, connectors/python, connectors/go, connectors/php, connectors/ruby, .github/scripts/package-connectors.sh, connectors/omb, connectors/contract, examples/csharp-console]
---

# Conectores

> Leia antes: [16-api-cliente.md](16-api-cliente.md). Os conectores **não acrescentam protocolo**: a API é HTTP, e qualquer linguagem já fala com o Taba pelo cliente HTTP que tem. Um conector só poupa as linhas de montar a requisição, repetir chamadas, ler o `ErrorResponse` e decodificar o corpo.

## Estado real

| Linguagem | Pasta | Estado | Publicação |
|---|---|---|---|
| **Rust** (`taba-client`) | `connectors/rust/client/` | ✅ **Síncrono pronto.** A forma `async` falta | ✅ [crates.io](https://crates.io/crates/taba-client) |
| **`taba-cli`** (sobre o Rust) | `connectors/rust/cli/` | ✅ Pronto ([20-cli.md](20-cli.md)) | ✅ [crates.io](https://crates.io/crates/taba-cli), binários e imagem Docker |
| **C#** (`Taba.Client`) | `connectors/csharp/` | ✅ **Pronto**: unitários, integração contra o Compose e os testes de contrato | ✅ [nuget.org](https://www.nuget.org/packages/Taba.Client) |
| **Java** (`taba-client`) | `connectors/java/` | ✅ **Pronto** (unitários e os testes de contrato) | ⬜ Não publicado (D70.1) |
| **Python** (`taba-client`) | `connectors/python/` | ✅ **Pronto** (unitários e os testes de contrato) | ⬜ Não publicado (D70.1) |
| **Node.js** (`taba-client`) | `connectors/node/` | ✅ **Pronto** (unitários e os testes de contrato) | ⬜ Não publicado (D70.1) |
| **Go** (`taba`) | `connectors/go/` | ✅ **Pronto** (unitários e os testes de contrato) | ⬜ Não publicado (D70.1) |
| **PHP** (`taba/client`) | `connectors/php/` | ✅ **Pronto** (unitários e os testes de contrato; sem renovação automática da invisibilidade, D68.3) | ⬜ Não publicado (D70.1, D70.2) |
| **Ruby** (`taba-client`) | `connectors/ruby/` | ✅ **Pronto** (unitários e os testes de contrato) | ⬜ Não publicado (D70.1) |
| **Driver do OpenMessaging Benchmark** (sobre o Java) | `connectors/omb/` | ✅ **Pronto** (testes unitários e de integração contra um nó) | ⬜ Não publicado (ferramenta do repositório) |
| **Provider Terraform e OpenTofu** (sobre o Go) | `connectors/terraform/` | ✅ **Pronto**: filas, distribuidores, tokens, `create_or_update`; testes unitários e de aceitação com Terraform e OpenTofu ([40-terraform.md](40-terraform.md)) | 🟡 Empacotado na Release; os registries esperam o repositório espelho (D211) |
| **Provider Pulumi** (o provider Terraform ligado ao Pulumi) | `connectors/pulumi/` | ✅ **Pronto**: filas, distribuidores, tokens, `createOrUpdate`; SDKs para TypeScript, Python, .NET e Go; testes unitários e de aceitação com TypeScript e Python ([41-pulumi.md](41-pulumi.md)) | 🟡 Empacotado na Release; o repositório espelho e os pacotes do npm, PyPI e NuGet esperam a configuração do mantenedor (D213) |
| C, C++ | — | ⏸ **Adiados**, sem data (D68.4): as pastas não existem | — |

> A meta de **dez linguagens** é plano, não realidade: hoje **8 de 10** existem, e as outras duas (C e C++) estão adiadas (D68.4). O [roadmap](32-roadmap.md) tem a ordem. Como a API é HTTP, as demais linguagens usam o cliente HTTP delas até o conector sair.

## Os três princípios

1. **Simples de usar:** publicar, consumir e confirmar cabem em poucas linhas, sem configuração além do endereço (e do token).
2. **Simples de implementar:** pequeno o bastante para uma pessoa entender o código inteiro numa tarde. Por isso a superfície é mínima, **não há dependência externa** e o comportamento é o mesmo em todas as linguagens.
3. **Funciona em versões antigas** (até **15 anos**, de 2011 em diante), **só nas plataformas que falam TLS 1.2 ou superior**, fazem `PATCH` e conferem o certificado. O que não cumpre isso não é suportado: o Taba não abre porta HTTP simples nem TLS antigo para elas.

Fora disso, o conector **não tenta ser esperto**: não guarda estado, não faz buffer de publicações, não reordena. O servidor decide (quórum, FIFO, entrega justa).

> **Alcance.** Os 15 anos valem para as linguagens ainda por fazer (C e C++) e para o C#, o Rust, o Go e o PHP (o Go 1.7 é de 2016 e o PHP 5.6 de 2014; o Ruby 2.1 é de 2013, e o 2.0 do primeiro plano não tem mais imagem Docker para testar, D68.3). Para **Java 11 (2018), Python 3.9 (2020) e Node.js 18 (2022)** a base foi posta mais alto (D68.2): a regra da tabela abaixo dava Python 2.7.9/3.4.3 e Node.js 0.12, que proibiriam `async`/`await`, `asyncio`, classes e anotações de tipo, e que as ferramentas de hoje já não rodam.

## A superfície: nove operações

A mesma em todas as linguagens, com os nomes adaptados (`publish`, `Publish`, `publish_message`…).

| Operação | Rota | Devolve |
|---|---|---|
| `createTopic(request)` | `POST /topics` | `Topic` |
| `getTopic(name)` | `GET /topics/{topic}` | `Topic` |
| `listTopics(limit, cursor, name)` | `GET /topics` | `Page<Topic>` (e um iterador sobre todas as páginas, onde a linguagem permite) |
| `updateTopic(name, request)` | `PATCH /topics/{topic}` | `Topic` |
| `deleteTopic(name)` | `DELETE /topics/{topic}` | — |
| `publish(topic, body, opções)` | `POST …/messages` | `PublishResponse` (ou `DistributorPublishResponse`) |
| `consume(topic, opções)` | `GET …/messages` | lista de `Message` (vazia se não há) |
| `ack(topic, message)` | `DELETE …/messages/{id}?receiptToken=` | — |
| `changeVisibility(topic, message, segundos)` | `PUT …/visibility?receiptToken=` | `VisibilityResponse` |

**Em lote** (opcional, ao lado das chamadas individuais, [D189](30-decisoes.md)): `publishBatch(topic, mensagens)`, `ackBatch(topic, mensagens)` e `changeVisibilityBatch(topic, mensagens, segundos)` chamam `POST …/messages/batch`, `/ack` e `/visibility` e devolvem um `BatchResult` por item, na ordem (`status`, `id`, `duplicate`, `matched`, `invisibleUntil`, `errorCode`, `errorMessage` e um `ok`/`isOk`); um item que falhou não lança erro. Uma mensagem de `publishBatch` (`BatchMessage`) tem os bytes do corpo e as mesmas opções de `publish` (`contentType`, `groupId`, `deduplicationId`); os bytes vão em base64, então qualquer tipo serve. Uma falha de rede não é repetida, como na publicação individual. No C# elas ficam na interface `ITabaBatchClient`, que o `TabaClient` também implementa, para não quebrar quem implementa `ITabaClient`. Nomes: `publish_batch`, `ack_batch`, `change_visibility_batch` no Rust, no Python e no Ruby; `publishBatch`, `ackBatch`, `changeVisibilityBatch` no PHP; `PublishBatch`, `AckBatch`, `ChangeVisibilityBatch` no Go; `PublishBatchAsync` (e `PublishBatch`) no C#.

**Declarar um tópico** (opcional, ao lado do `createTopic`, que não muda; [D210](30-decisoes.md#declarar-um-tópico-d210-d214-a-d216)): `createOrUpdateTopic(request, recreate)` chama [`PUT /topics/{topic}`](16-api-cliente.md#criar-ou-atualizar-um-tópico-put-topicstopic) com o nome no caminho codificado e o corpo do `createTopic` sem `name`, e devolve o `Topic` e o **desfecho** (`created` no `201`, `updated` no `200`, `recreated` no `201` com `Taba-Topic-Recreated: true`), de modo que quem chama não olha status nem cabeçalho. O que fica de fora volta ao padrão do servidor. `recreate` (`if-empty` ou `always`) vai na query só quando dado; outro valor é `client.invalid-request` e nada é enviado. `409 topic.immutable-mismatch` e `topic.not-empty` são erros com o código. Depois de um `503` o `PUT` é repetido como as outras chamadas, sem regra `recover` (é idempotente). A remoção "só se vazia" (`DELETE ?ifEmpty=true`) vem na forma de cada linguagem, sem mudar o `deleteTopic`. Nomes: `create_or_update_topic` e `delete_topic_if_empty` em Rust; `CreateOrUpdateTopicAsync` (e a síncrona) e `DeleteTopicIfEmptyAsync` em C#, na interface `ITabaTopicPutClient`, que o `TabaClient` implementa, para não quebrar quem implementa `ITabaClient`; `createOrUpdateTopic` e `deleteTopicIfEmpty` em Java (e as formas `…Async`), Go (`CreateOrUpdateTopic`, `DeleteTopicIfEmpty`) e PHP; `createOrUpdateTopic` e `deleteTopic(name, { ifEmpty: true })` em Node.js; `create_or_update_topic` e `delete_topic(name, if_empty=True)` em Python (também no `AsyncClient`) e Ruby (`if_empty: true`).

**Fora da superfície, de propósito:** tokens (`/tokens`: quem cria é o operador, com `curl` ou `taba-cli token`), a API de operação e a do cluster.

**Adicionais** (onde a linguagem permite): `subscribe(handler)` (long polling contínuo, chama o handler e confirma sozinho **se ele terminar sem erro**); `publishJson(topic, objeto)` (serializa e publica como `application/json`) e `publishRawJson(topic, texto)`; `consume` com *handler* (o `delete` vale `true` por padrão: o handler termina sem erro, a mensagem é apagada) e `consume<T>` (corpo já deserializado); renovação automática da invisibilidade enquanto o handler roda. **Assíncrono** onde há modelo assíncrono (`Task` no C#, `async`/`await` no Rust e no Python 3.5+…), com o mesmo contrato.

**`publish`** recebe os **bytes** e o `Content-Type`; as opções são `groupId`, `deduplicationId`, `contentType` e `requestId`. O conector **não valida o corpo**. **`consume`** tem `maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate` (com `volatile`, `idleTimeout`, `fifo`, `tags`); manda **só o que a pessoa informou** e **não aplica padrões próprios**: o que faltar vale o padrão do servidor.

### Configuração

Um objeto (sem variáveis de ambiente: `TABA_*` é do servidor):

| Campo | Padrão | Descrição |
|---|---|---|
| `baseUrl` | — (obrigatório) | Endereço do balanceador da API de cliente (`http://host:porta` ou `https://…`) |
| `token` | nenhum | Vai em `Authorization: Bearer` |
| `connectTimeout` | 5 s | Abrir a conexão |
| `requestTimeout` | 30 s | Chamada comum |
| `maxAttempts` | 3 | Tentativas, contando a primeira |
| `userAgent` | `taba-<linguagem>/<versão>` | — |

## O contrato comum

Idêntico em **todos** os conectores; é o que permite uma suíte de conformidade única.

**Long polling.** No `consume`, o tempo limite de leitura é **`waitTime + 10 s`**, nunca o `requestTimeout` (um `waitTime` de 20 s com limite de 30 s derrubaria a chamada à toa). Estourar o tempo no `consume` **não é seguro de repetir às cegas** (o nó pode ter capturado mensagens): o erro sobe e as mensagens voltam sozinhas ao fim do `visibilityTimeout`.

**Repetição de chamadas:**

| Resposta | Repete? |
|---|---|
| `429`, `503` | **Sim**, até `maxAttempts`, esperando o `Retry-After` (sem ele: 1 s, 2 s, 4 s). Em `503` de operação que cria ou apaga tópico, `409 topic.already-exists` e `404` da repetição contam como sucesso (não no `createOrUpdateTopic`, cujo `PUT` é só repetido) |
| `502`, `504` | **Só** na publicação em fila **FIFO** (o primário deduplica). Fora dela, **não** (duplicaria) |
| Falha de rede **antes** de enviar o corpo (recusa, DNS) | Sim, como um `503` |
| Falha de rede **depois** de enviar o corpo | **Não** (não se sabe se foi aplicado) |
| Qualquer `4xx`, menos `429` | **Nunca** |

**A mensagem entregue.** O conector sempre devolve o corpo como **bytes**:

| `contentType` / `bodyEncoding` | O que vem em `body` |
|---|---|
| `application/json`, `*/*+json` | O **texto JSON** do campo `body`, tal qual (a aplicação o decodifica) |
| `text/*` em UTF-8 | A string, em UTF-8 |
| `bodyEncoding: base64` | Os bytes já **decodificados** |

Mais `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt`, `invisibleUntil` e, quando vierem, `groupId`, `deduplicationId`, `deadLetteredAt`, `deadLetterReason`. **Campos desconhecidos na resposta são ignorados.**

**Erro.** Uma forma só por conector (exceção, `Result`, código de retorno), com os campos de `ErrorResponse`: `status`, `code`, `message`, `requestId`, `retryAfter`, `details`. A aplicação testa o **`code`**, nunca o `message`. As falhas do próprio cliente têm `status` zero e `code` **`client.network`**, **`client.timeout`**, **`client.invalid-config`**, **`client.invalid-response`** (e `client.invalid-request`, quando o objeto da aplicação não pode ser serializado): códigos que **só existem no cliente**.

**Cabeçalhos.** Envia `Authorization`, `Content-Type`, `Accept: application/json`, `User-Agent` e, se informados, `X-Request-Id` e `traceparent`; lê `Retry-After` e `X-Request-Id`.

**Conexão e threads.** HTTP/1.1, com conexão persistente quando a biblioteca oferece sem custo; **não exige HTTP/2**. O cliente é **seguro para uso concorrente** em Java, C#, Go, Rust e Node; em C, C++ e PHP vale **um cliente por thread** (por processo, no PHP); Python e Ruby mantêm uma conexão por thread, então um cliente pode ser compartilhado.

## Sem dependências: o leitor de JSON mínimo

Uma dependência externa é quase sempre o que quebra numa versão antiga. A regra: **só a biblioteca-padrão da linguagem**, salvo `libcurl` no C e `rustls` no Rust.

- **HTTP:** o cliente da própria linguagem. Três não têm: o **Rust** (implementação mínima de HTTP/1.1 sobre `TcpStream`, com `rustls`), o **C++** (usa a `libtaba`) e o **C** (`libcurl`).
- **JSON:** o que o conector lê é **pequeno e fixo** (os DTOs e o `ErrorResponse`). Em vez de depender de uma biblioteca, cada conector leva um **leitor de JSON mínimo** (150 a 250 linhas) que percorre o texto uma vez, sabe **pular** qualquer valor, entrega os campos de um objeto como pares nome → trecho do texto e decodifica *strings* (escapes e `\uXXXX`). Isso resolve o campo `body`, que é um JSON **qualquer**: o conector devolve o trecho **sem interpretá-lo**.
- **Base64:** biblioteca-padrão, ou ~30 linhas.

## Rust (`taba-client`)

- **HTTP/1.1 próprio** sobre `std::net::TcpStream`; **TLS 1.2+** pelo `rustls` com as raízes do sistema (e `ca-file`); JSON e base64 próprios. Dependências: `rustls`, `rustls-native-certs`; `serde` + `serde_json` **só** com a feature opcional `serde`.
- `Client` é `Send + Sync`. Repetição conforme o contrato.

```rust
use taba_client::{Client, Config, ConsumeOptions, PublishOptions, TopicRequest};

let mut config = Config::new("http://127.0.0.1:8080");
config.token = std::env::var("TABA_TOKEN").ok();
let client = Client::new(config)?;

client.create_topic(&TopicRequest { name: "pedidos".into(), topic_type: "queue".into(), ..Default::default() })?;
client.publish("pedidos", br#"{"pedido": 9001}"#, &PublishOptions::default())?;

let options = ConsumeOptions { wait_time: Some(20), ..Default::default() };
for message in client.consume("pedidos", &options)? {
    println!("{}", String::from_utf8_lossy(&message.body));
    client.ack("pedidos", &message.id, &message.receipt_token)?;
}
```

Com a feature `serde`: `publish_object` e `consume_typed::<T>` (long polling, corpo já deserializado, **apaga se o handler devolve `Ok`**, renova a invisibilidade enquanto roda). Sem `serde`: `consume_with`. Testes: `cd connectors/rust && cargo test --workspace --all-features`.

## C# (`Taba.Client`)

- **Alvos:** `net461` (.NET Framework 4.6.1 a 4.8), `net5.0`, `net6.0`, `net8.0` e, com SDK ≥ 10, `net10.0`. **C# 7.3** nos alvos todos. **Sem dependências** (`HttpClient`, `Task`); JSON mínimo próprio.
- **Pacotes:** `Taba.Client` (o `TabaClient`), `Taba.Client.Abstractions` (`ITabaClient`, modelos, `TabaException`) e `Taba.Client.Extensions.DependencyInjection` (o `AddTabaClient`, **à parte** para o principal seguir sem dependências).
- **Assíncrono primeiro:** as nove operações têm `…Async` (a implementação) e a síncrona. Todas aceitam `CancellationToken`. Um só `TabaClient` serve a várias tarefas.

```csharp
var config = new TabaConfig("http://localhost:8080") { Token = Environment.GetEnvironmentVariable("TABA_TOKEN") };
using (var client = new TabaClient(config))
{
    await client.CreateTopicAsync(new TopicRequest { Name = "pedidos", Type = "queue" });
    await client.PublishJsonAsync("pedidos", new { pedido = 9001 });

    foreach (Message message in await client.ConsumeAsync("pedidos", new ConsumeOptions { WaitTime = 20 }))
    {
        Console.WriteLine(message.BodyText);
        await client.AckAsync("pedidos", message);
    }
}
```

- **`ConsumeAsync<T>(topic, handler)`:** long polling de 20 s, uma mensagem por vez, 120 s de invisibilidade, corpo deserializado, **ack só se o handler terminar sem exceção**, renovação automática da invisibilidade (a cada um terço).
- **`SubscribeAsync`:** o laço inteiro.
- **Injeção de dependência:** `builder.Services.AddTabaClient("Taba")` lê `BaseUrl`, `Token`… da seção do `appsettings.json` e registra `ITabaClient` como *singleton*.
- **Testes:** `cd connectors/csharp && dotnet test Taba.Client.Tests` (NUnit; os de integração só rodam com `TABA_URL`). `connectors/csharp/scripts/integration.sh` sobe o Compose, cria tokens e filas e roda tudo. Além dele, todo conector (este também) roda os **testes de contrato**: `connectors/csharp/contract/run.sh`, ou `connectors/contract/run.sh` para os oito ([testes de contrato](#os-testes-de-contrato)).
- **Exemplo:** [`examples/csharp-console/`](../../examples/csharp-console/): cria um tópico, publica, consome com long polling e confirma; lê o endereço e o token raiz de variáveis de ambiente.

## Java (`io.github.rafaelpontani:taba-client`)

- **Java 11 ou superior** (`java.net.http.HttpClient`, HTTP/1.1, redirecionamentos desligados). **Sem dependências em tempo de execução**: JSON mínimo próprio; JUnit só nos testes. Projeto Maven em `connectors/java/`.
- **Bloqueante e `...Async`** (`CompletableFuture`, num pool de threads do próprio cliente ou no `TabaConfig.executor`). Uma chamada bloqueante é cancelada interrompendo a thread e uma assíncrona por `future.cancel(true)`; as duas terminam com `CancellationException`, nunca com `TabaException`. A `TabaException` é não verificada (*unchecked*). Os handlers são um `ThrowingConsumer` (uma exceção verificada sai como `HandlerFailedException`).
- **JSON:** o JDK não tem biblioteca de JSON, então `publishJson` e o `consume` tipado precisam de um `TabaJsonSerializer` na configuração (quatro linhas com o Jackson, no README); sem ele, falham com `client.invalid-config`.
- **`subscribe`** devolve uma `Subscription` (`AutoCloseable`) que roda o laço numa thread própria; `close()` a para, interrompendo um long polling e nunca o handler.

```java
TabaConfig config = TabaConfig.of("http://localhost:8080").token(System.getenv("TABA_TOKEN"));
try (TabaClient client = new TabaClient(config)) {
    client.createTopic(TopicRequest.queue("pedidos"));
    client.publishText("pedidos", "ola");

    for (Message message : client.consume("pedidos", new ConsumeOptions().waitTime(20))) {
        System.out.println(message.getBodyText());
        client.ack("pedidos", message);
    }
}
```

Testes: `cd connectors/java && mvn test` (JUnit 5 e o `com.sun.net.httpserver` do próprio JDK); o CI roda JDK 11 e 21.

## Driver do OpenMessaging Benchmark (`connectors/omb`)

- Um driver para o [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) escrito sobre o conector Java, em **Java 11**; é uma ferramenta para medir o Taba com os mesmos workloads que o OMB roda contra Kafka, NATS ou RabbitMQ, e não entra em nenhum pacote. O commit do OMB contra o qual é construído está em `connectors/omb/OMB_VERSION` (o último que ainda compila em Java 11).
- **Mapa:** um tópico do OMB é um **distribuidor** do Taba com o nome do tópico e o prefixo `<tópico>-`; cada assinatura é uma **fila** `<tópico>-<assinatura>` criada (volátil, com o maior `idleTimeout`, como rede de segurança) antes da primeira publicação e apagada quando o driver fecha. Os consumidores de uma assinatura disputam as mensagens dela. O número de partições do OMB não cria nada no Taba.
- **Chave e ordem:** a chave da mensagem do OMB é o `groupId`. `ordering: none` (padrão) usa fila comum; `ordering: per-key` usa fila FIFO, e a mensagem sem chave recebe `p0`..`p<partições-1>` em rodízio.
- **Lotes:** o produtor guarda as mensagens num buffer e as manda num `POST …/messages/batch` quando `producer.batchSize` (100) enchem ou `producer.lingerMs` (1 ms) passa, a mesma ideia do `linger.ms` do produtor do Kafka; a hora de uma mensagem é tomada quando ela entra no buffer, então a espera conta na latência dela. Como no Kafka, o linger só manda um lote parcial quando há uma requisição livre: com todas em voo o lote continua enchendo (senão um produtor saturado manda lotes cada vez menores, cada um custando uma requisição, e a taxa cai em espiral: medido, 600 mensagens por segundo contra um nó que recebe 45 mil). `producer.inFlight` (1000) conta requisições; o `bench/run.sh` usa 64, e o consumidor lê 500 por `GET`. O consumidor confirma o que um `GET` entregou num `POST …/messages/ack` (`consumer.ackBatch`, `true`). `consumer.pollers` (1) threads fazem o long polling da mesma fila ao mesmo tempo, cada uma com o seu `GET`, a ideia da busca em segundo plano do consumidor do Kafka: enquanto uma espera a resposta, outra já está pedindo; o `bench/run.sh` usa 16 (com 4, os quatro `GET`s em voo, e não o nó, seguravam um nó em cerca de 20 mil mensagens por segundo com núcleos sobrando, e o controlador de taxa do OMB tomava o resto como backlog). `batchSize: 1` e `ackBatch: false` voltam a uma requisição por mensagem.
- **Hora de publicação:** o Taba não guarda atributo além do corpo, então o produtor grava o relógio dele nos 8 primeiros bytes do corpo e o consumidor os retira; o corpo no servidor tem 8 bytes a mais que o payload do OMB, e o resultado precisa dizer isso.
- **Build e testes:** `connectors/omb/build.sh` (`driver-api` do OMB, o conector e o driver em um contêiner Java 11; um JAR com o conector dentro, `target/omb-driver-taba-<version>-all.jar`) e `connectors/omb/test.sh` (um nó em Docker e depois os testes unitários e de integração). O job `connector-omb` do CI roda o build e os testes unitários; o `connector-contract` também roda o `test.sh`. Configuração: `connectors/omb/driver-taba.yaml`. Casos: UC-CNT-036 a 041. O script que o roda contra outros sistemas é o `bench/run.sh` ([38-benchmarks.md](38-benchmarks.md)).

## Node.js (`taba-client`)

- **Node.js 18 ou superior**, CommonJS (funciona com `require` e `import`), tipos em `index.d.ts`, **sem dependências** (`node:http` e `node:https`).
- **Só `Promise`:** o Node não tem HTTP síncrono, então não existe a gêmea `...Sync`. O cancelamento é um `AbortSignal`; as opções são objetos; os nomes são `camelCase`. Num `PATCH`, `undefined` deixa o campo de fora e `null` volta ao padrão do servidor.
- `consumeWith(topic, options, handler)` e `consumeJson` (o `consume<T>` das outras linguagens: `context.body` é o corpo já lido); `subscribe(topic, options, handler, { signal, onError })` termina sem erro quando o sinal é abortado.
- Uma conexão do pool que o servidor fechou ociosa (`ECONNRESET` antes de qualquer byte da resposta) é repetida como uma conexão recusada (PRB-081).

```js
const { TabaClient } = require('taba-client');

const client = new TabaClient({ baseUrl: 'http://localhost:8080', token: process.env.TABA_TOKEN });
await client.createTopic({ name: 'pedidos', type: 'queue' });
await client.publishJson('pedidos', { pedido: 9001 });
await client.consumeWith('pedidos', { waitTime: 20 }, async ({ message }) => console.log(message.bodyText));
client.close();
```

Testes: `cd connectors/node && npm test` (`node:test` e um servidor HTTP local); o CI roda Node 18 e 24.

## Python (`taba-client`, `import taba`)

- **Python 3.9 ou superior**, **sem dependências**, tipado (`py.typed`). O `taba.Client` é síncrono (uma conexão por thread, então é seguro para uso concorrente); o `taba.aio.AsyncClient` é o do `asyncio`. A biblioteca-padrão não tem cliente HTTP assíncrono, então este roda o mesmo transporte em threads de trabalho (PRB-080).
- `snake_case`; as opções são argumentos nomeados (`client.consume("q", wait_time=20)`); `iter_topics()` é um gerador; `UNSET` e `None` dizem ao `TopicUpdate` para deixar um campo de fora ou enviar `null`. `consume_with` e `consume_json` são as formas com handler.
- **Cancelar:** o cliente síncrono recebe um `CancelToken` (fecha a conexão em andamento, então um long polling não espera o próprio tempo-limite) e levanta `taba.Cancelled`; o `subscribe` volta sem exceção. O cliente `asyncio` se cancela do jeito do `asyncio` (`task.cancel()`), que levanta `asyncio.CancelledError`.

```python
from taba import Client, TopicRequest

with Client("http://localhost:8080", token="...") as client:
    client.create_topic(TopicRequest("pedidos", "queue"))
    client.publish_json("pedidos", {"pedido": 9001})
    client.consume_with("pedidos", lambda context: print(context.message.body_text), wait_time=20)
```

Testes: `cd connectors/python && python -m unittest discover -s tests -t .` (o `unittest` padrão e um servidor HTTP local); o CI roda Python 3.9 e 3.13.

> **Java, Node.js e Python não são cópia do C#.** O contrato (nove operações, repetição, erros, corpo) é o mesmo; as formas são as de cada linguagem, registradas em D68.2. Os três rodam os testes de contrato contra um Taba real ([testes de contrato](#os-testes-de-contrato), D183), e nenhum está publicado (PRB-079).

## Go (`github.com/rafaelpontani/taba/connectors/go`, `package taba`)

- **Go 1.7 ou superior**, **só a biblioteca-padrão** (`net/http`, `encoding/json`); compila e testa na 1.7 e nas atuais. Nada do `go.mod` além da linha do módulo, então funciona também em um `GOPATH`.
- `taba.Client` é seguro para uso concorrente. **Toda chamada recebe um `context.Context`** e devolve `(valor, error)`; cancelar o contexto encerra a chamada em andamento (um long polling não espera o próprio timeout) e a chamada devolve o erro do próprio contexto, nunca um `*taba.Error`. `Subscribe` devolve `nil` quando o contexto é cancelado.
- Campos opcionais são ponteiros (`taba.Int(20)`, `taba.Bool(true)`), então `nil` quer dizer "não informado" e vale o padrão do servidor; em um `TopicUpdate`, `taba.SetInt(n)` / `taba.ClearInt()` / `nil` são "enviar `n`" / "enviar `null`" / "deixar de fora". O erro é um `*taba.Error` (`Status`, `Code`, `Message`, `RequestID`, `RetryAfter`, `Details`, `Body`).
- `ConsumeWith(ctx, topic, options, func(*MessageContext) error)`: devolver `nil` apaga a mensagem (a não ser com `Delete = false`), um erro a mantém e é devolvido; a invisibilidade é renovada por outra goroutine. O Go 1.7 não tem genéricos, então não há o `consume<T>` tipado: o handler lê o corpo com `Decode(&v)`.
- **Só no Go por enquanto** (o provider Terraform, e o Pulumi por meio dele, os usam; PRB-109): a administração dos tokens de acesso (`CreateToken`, `GetToken`, `ListTokens`/`ListAllTokens`, `UpdateToken`, `RevokeToken`; um `503 cluster.quorum-unavailable` no `CreateToken` volta sem nova tentativa, porque o dono pode ter criado o token), e `Config.RetryMaxWait`: o total das esperas entre as tentativas de uma chamada; não se faz a tentativa cuja espera passaria dele, e o `MaxAttempts` continua limitando (D220).

```go
client, _ := taba.NewClient(taba.Config{BaseURL: "http://localhost:8080", Token: os.Getenv("TABA_TOKEN")})
ctx := context.Background()
client.CreateTopic(ctx, &taba.TopicRequest{Name: "pedidos", Type: "queue"})
client.PublishJSON(ctx, "pedidos", map[string]int{"pedido": 9001})
client.ConsumeWith(ctx, "pedidos", &taba.ConsumeOptions{WaitTime: taba.Int(20)}, func(c *taba.MessageContext) error {
	fmt.Println(c.Message.BodyText())
	return nil
})
```

Testes: `cd connectors/go && go test -race .` (o `testing` padrão e o `httptest`).

## PHP (`taba/client`, namespace `Taba`)

- **PHP 5.6 ou superior**, só `ext-curl` e `ext-json`; o mesmo código roda na 5.6 e na 8.x. TLS 1.2 e a verificação do certificado ficam sempre ligados. Autoload pelo Composer (`classmap`) ou por `src/autoload.php`.
- **Uma thread só**, então **um cliente por processo**, e **sem renovação automática da invisibilidade** enquanto um handler roda: `consumeWith` toma a mensagem por `visibilityTimeout` (120 s por padrão) e o handler pode chamar `$context->changeVisibility()`. É o único ponto em que um conector faz menos do que o contrato permite (D68.3); o cenário de contrato imprime essa linha como `SKIP`.
- Métodos em `camelCase`; as opções são arrays (`array('waitTime' => 20)`) ou os objetos equivalentes, e uma chave desconhecida é erro `client.invalid-request`. O erro é uma `Taba\TabaException` com `status`, `errorCode` (o `getCode()` do PHP é inteiro), `requestId`, `retryAfter`, `details`, `body`.
- **Cancelar:** um `CancelToken` é verificado pela transferência do `curl` (cerca de uma vez por segundo) e entre as mensagens, e tem `cancelAfter($segundos)`, já que nada mais roda enquanto uma requisição está em andamento; uma chamada cancelada lança `Taba\Cancelled`.

```php
$client = new Taba\Client(new Taba\Config('http://localhost:8080', getenv('TABA_TOKEN')));
$client->createTopic(array('name' => 'pedidos', 'type' => 'queue'));
$client->publishJson('pedidos', array('pedido' => 9001));
$client->consumeWith('pedidos', function (Taba\MessageContext $context) {
    echo $context->message->bodyText();
}, array('waitTime' => 20));
```

Testes: `cd connectors/php && php tests/run.php` (um executor pequeno próprio, sem PHPUnit: um `curl` falso para as requisições, e o servidor embutido do PHP e sockets crus para o transporte real).

## Ruby (`taba-client`, `require 'taba'`)

- **Ruby 2.1 ou superior**, **só a biblioteca-padrão** (`net/http`, `json`); o mesmo código roda na 2.1 e na 3.x. `Taba::Client` é seguro para uso concorrente (uma conexão por thread). A 2.0 do primeiro plano não pôde ser testada (a imagem Docker dela não existe mais), então a base é a 2.1 (D68.3).
- `snake_case`; as opções são argumentos nomeados, os handlers são blocos (`consume_with`, `consume_json`, `subscribe`); `each_topic` é um `Enumerator` sobre as páginas. Em um `PATCH`, a chave que falta não é enviada e `nil` envia `null`. O erro é um `Taba::Error` (`status`, `code`, `reason`, `request_id`, `retry_after`, `details`, `body`).
- **Cancelar:** um `Taba::CancelToken` cancelado por outra thread encerra na hora a chamada em andamento (a troca roda em uma thread auxiliar quando há token, porque fechar um socket não acorda uma leitura bloqueada em toda versão do Ruby) e lança `Taba::Cancelled`. O `Net::HTTP` repete sozinho uma requisição idempotente depois de um timeout; o conector desliga isso, então só acontecem as repetições do contrato.

```ruby
client = Taba::Client.new('http://localhost:8080', token: ENV['TABA_TOKEN'])
client.create_topic(name: 'pedidos', type: 'queue')
client.publish_json('pedidos', pedido: 9001)
client.consume_with('pedidos', wait_time: 20) { |context| puts context.message.body_text }
```

Testes: `cd connectors/ruby && ruby -Ilib -Itest -e 'Dir["test/*_test.rb"].each { |f| require File.expand_path(f) }'` (minitest e um servidor TCP local).

> **Go, PHP e Ruby também não são cópia do C#** (D68.3): cada um usa as formas da própria linguagem, e os três rodam os testes de contrato contra um Taba real, em Go 1.7, PHP 5.6 e Ruby 2.1. Nenhum está publicado (PRB-079).

## Usar os conectores antes de publicados

Enquanto um conector não chega ao seu registro (PRB-079), o pacote dele é um **arquivo anexado a cada Release do GitHub** (D70.3), montado por `.github/scripts/package-connectors.sh`, listado no `SHA256SUMS` e atestado como os binários:

| Conector | Arquivo da [Release](https://github.com/rafaelpontani/taba/releases) | Instalação |
|---|---|---|
| Node.js | `taba-client-<v>.tgz` | `npm install <URL do arquivo>` |
| Python | `taba_client-<v>-py3-none-any.whl` | `pip install <URL do arquivo>` |
| Ruby | `taba-client-<v>.gem` | baixar e `gem install ./taba-client-<v>.gem` |
| Java | `taba-client-<v>.jar` | `mvn install:install-file`, ou o jar no classpath |
| PHP | `taba-client-php-<v>.zip` | um repositório `package` do Composer com o `dist.url` apontando para o arquivo, ou descompactar e `require src/autoload.php` |
| Go | sem arquivo | `go get github.com/rafaelpontani/taba/connectors/go@release` (o caminho do módulo é o repositório) |

`<v>` é a versão do **conector** (os conectores têm versão própria, D70.2), não a da Release. Sem uma Release, alguns também se instalam do código-fonte: Python (`pip install "git+https://github.com/rafaelpontani/taba.git@release#subdirectory=connectors/python"`), Ruby (Bundler, `glob: 'connectors/ruby/*.gemspec'`) e pnpm (`#path:/connectors/node`); o npm e o Composer não leem uma pasta de um repositório. Os comandos exatos estão no README de cada conector.

## Os adiados: versão-base e publicação

C e C++ estão **adiados, sem data** (D68.4): este é o desenho guardado para quando forem retomados, não um compromisso (propostas, sujeitas a D68, D70.1, D70.2). Java, Python e Node.js saíram desta tabela quando ficaram prontos (D68.2), e Go, PHP e Ruby depois deles (D68.3). "Versão-base" é a mais antiga que fala TLS 1.2, tem cliente HTTP com `PATCH` e confere o certificado.

| Linguagem | Versão-base | Por quê | HTTP / JSON | Publicação |
|---|---|---|---|---|
| C | **C89** + `libcurl` ≥ 7.34 | A `libcurl` 7.34 é a primeira que exige TLS 1.2 e faz `PATCH` | `libcurl` / leitor mínimo | Releases do GitHub |
| C++ | **C++03** | Um cabeçalho sobre a `libtaba` (C) | via `libtaba` | Releases do GitHub |

**Regras de código para versões antigas:** nada de sintaxe ou biblioteca posterior à versão-base (sem `?.` no C# 7.3; sem `record` nem bloco de texto no Java 11; sem `match` no Python 3.9; sem `Object.groupBy` no Node.js 18); o CI compila e testa **na versão-base e na mais recente**; texto em **UTF-8 sempre**; o conector **não desliga a verificação do certificado** e **não depende do relógio local**. O Taba **não** aceita `X-HTTP-Method-Override`.

## Como escrever um conector novo

1. **Leia** [16-api-cliente.md](16-api-cliente.md) e este contrato; **use o `taba-cli`** como referência executável (cada comando é uma chamada da API, e o que ele faz é o que o conector repete).
2. **Pasta autônoma** `connectors/<linguagem>/`: `README.md` (instalação, um exemplo de 10 linhas, versão-base), o empacotamento da linguagem, `src/`, `test/` e `exemplos/`. Nenhuma pasta importa código de outra (exceto `cpp/` sobre `c/`).
3. **Implemente** as nove operações, a repetição da tabela acima, o leitor de JSON mínimo e os erros `client.*`.
4. **Teste**: unitários com um transporte de mentira (o Rust tem `taba_client::testing`; o C# tem `ScriptedHandler`) conferindo a **requisição montada** e a leitura de respostas prontas; integração contra um Taba de um nó (`replication.factor=1`), ou o Compose; e um **driver** para os testes de contrato ([testes de contrato](#os-testes-de-contrato)): o cenário já está escrito, o conector só precisa executá-lo.
5. **Cite os casos de uso** nos testes numa linha `UC: UC-CNT-NNN` (o `uc_coverage` do servidor lê as citações das pastas de conectores também) ([26-casos-de-uso.md](26-casos-de-uso.md)).

> Planejado e **ainda inexistente**: as fixtures (pares requisição/resposta) e os documentos de teste do leitor de JSON da suíte de conformidade ([32-roadmap.md](32-roadmap.md)); os cenários e os drivers já existem ([testes de contrato](#os-testes-de-contrato)). Os documentos OpenAPI da API de cliente existem ([15-api-visao-geral.md](15-api-visao-geral.md)) e os conectores podem ser gerados a partir deles ou conferidos contra eles; além deles, a "fonte da verdade" do contrato é este texto, o `taba-cli` e os testes dos dois conectores.

## Os testes de contrato

O mesmo cenário roda contra um Taba real por **todos os conectores** (C#, Java, Node.js, Python, Rust, Go, PHP e Ruby), cada um com um *driver* pequeno escrito na sua própria linguagem que usa só a API pública ([connectors/contract/README.md](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md) tem a linguagem do cenário e as operações). `connectors/contract/run.sh` sobe **um nó Taba** no Docker (o perfil de um nó) e roda, um depois do outro, o driver de cada conector **no contêiner da menor versão que ele suporta** (Node 18, Python 3.9, JDK 11, .NET 8, Rust estável, Go 1.7, PHP 5.6, Ruby 2.1); cada conector também roda sozinho (`connectors/<linguagem>/contract/run.sh`).

- **`scenarios/core.scn`**, em todos os conectores: tópicos (e seus `404`, `409` e `400`), publicar, consumir, `changeVisibility` e `ack`, e o mesmo em lote (`publishBatch`, `settleBatch`: um resultado por item, um item ruim e um recibo velho falhando sozinhos, a ordem de um grupo FIFO); texto com acento, JSON e bytes voltando byte a byte; FIFO (duplicata e ordem); distribuidor; consumo com handler (apagada, falha, mantida e **renovação da invisibilidade** enquanto um handler lento segura a mensagem); `subscribe`; um long polling **cancelado** de fora (no Rust não, que é síncrono: a linha sai como `SKIP`; e o handler `slow`, que precisa da **renovação**, também é `SKIP` no PHP, que não tem threads); um token errado e uma conexão recusada.
- **`scenarios/cross-*.scn`**, os oito juntos: cada conector publica em um distribuidor e cada um lê a própria fila, achando o que os oito enviaram, exatamente.
- **Não cobre**: o `503` e as repetições com um nó caindo, e o comportamento do cluster (um nó só não mostra): ficam com o `docker/validate.sh` e os testes unitários de cada conector. O job `connector-contract` do `ci.yml` roda em todo pull request para `release`.

## Pendências desta área

D68 (confirmar as versões-base das linguagens ainda por fazer: C e C++), D70.1 (organização e identificadores de pacote em cada registro), D70.2 (espelho do PHP; versões independentes das do servidor): [31-pendencias.md](31-pendencias.md).

## Fontes e provas

- `connectors/rust/client/src` (`client`, `http`, `json`, `base64`, `transport`, `testing`), `connectors/rust/client/README.md`, `connectors/csharp/Taba.Client*`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src`, `connectors/node/src`, `connectors/python/taba`, `connectors/go`, `connectors/php/src`, `connectors/ruby/lib`, `examples/csharp-console`, `connectors/README.md`.
- Casos de uso: UC-CNT (43 casos, nível `K`; conectores; 042 e 043: declarar um tópico); veja [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md). Testes: `connectors/rust/client/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`, `connectors/java/src/test`, `connectors/node/test`, `connectors/python/tests`, `connectors/go/*_test.go`, `connectors/php/tests`, `connectors/ruby/test`, `connectors/omb/src/test`; testes de contrato: `connectors/contract` (`run.sh`, `scenarios/`) e os drivers em `connectors/{csharp,java,node,python,rust,go,php,ruby}/contract`.
- Histórico: `19-conectores.md`, `19a-conectores-linguagens.md`.
