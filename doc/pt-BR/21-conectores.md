---
título: Conectores
resumo: As bibliotecas de cliente do Taba — o contrato comum (as nove operações, repetição, erros, corpo), o estado real de cada linguagem (Rust e C# prontos; oito planejadas), como usar e como escrever um novo.
tipo: referência
status: parcial
verificado-em: 2026-10-13 @ 71183b9
fontes: [connectors/README.md, connectors/rust/client, connectors/rust/cli, connectors/csharp, exemplos/csharp-console]
---

# Conectores

> Leia antes: [16-api-cliente.md](16-api-cliente.md). Os conectores **não acrescentam protocolo**: a API é HTTP, e qualquer linguagem já fala com o Taba pelo cliente HTTP que tem. Um conector só poupa as linhas de montar a requisição, repetir chamadas, ler o `ErrorResponse` e decodificar o corpo.

## Estado real

| Linguagem | Pasta | Estado | Publicação |
|---|---|---|---|
| **Rust** (`taba-client`) | `connectors/rust/client/` | ✅ **Síncrono pronto.** A forma `async` falta | ⬜ crates.io |
| **`taba-cli`** (sobre o Rust) | `connectors/rust/cli/` | ✅ Pronto ([20-cli.md](20-cli.md)) | Binários e imagem Docker |
| **C#** (`Taba.Client`) | `connectors/csharp/` | ✅ **Pronto**: unitários e integração contra o Compose | ⬜ nuget.org |
| Java, C, C++, Python, PHP, Ruby, Go, Node.js | — | ⬜ **Planejados** (as pastas não existem) | — |

> A meta de **dez linguagens** é plano, não realidade: hoje **2 de 10** existem. O [roadmap](32-roadmap.md) tem a ordem. Como a API é HTTP, as demais linguagens usam o cliente HTTP delas até o conector sair.

## Os três princípios

1. **Simples de usar:** publicar, consumir e confirmar cabem em poucas linhas, sem configuração além do endereço (e do token).
2. **Simples de implementar:** pequeno o bastante para uma pessoa entender o código inteiro numa tarde. Por isso a superfície é mínima, **não há dependência externa** e o comportamento é o mesmo em todas as linguagens.
3. **Funciona em versões antigas** (até **15 anos**, de 2011 em diante), **só nas plataformas que falam TLS 1.2 ou superior**, fazem `PATCH` e conferem o certificado. O que não cumpre isso não é suportado: o Taba não abre porta HTTP simples nem TLS antigo para elas.

Fora disso, o conector **não tenta ser esperto**: não guarda estado, não faz buffer de publicações, não reordena. O servidor decide (quórum, FIFO, entrega justa).

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
| `429`, `503` | **Sim**, até `maxAttempts`, esperando o `Retry-After` (sem ele: 1 s, 2 s, 4 s). Em `503` de operação que cria ou apaga tópico, `409 topic.already-exists` e `404` da repetição contam como sucesso |
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

**Conexão e threads.** HTTP/1.1, com conexão persistente quando a biblioteca oferece sem custo; **não exige HTTP/2**. O cliente é **seguro para uso concorrente** em Java, C#, Go, Rust e Node; em C, C++, Python, PHP e Ruby vale **um cliente por thread**.

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
- **Testes:** `cd connectors/csharp && dotnet test Taba.Client.Tests` (NUnit; os de integração só rodam com `TABA_URL`). `connectors/csharp/scripts/integration.sh` sobe o Compose, cria tokens e filas e roda tudo.
- **Exemplo:** [`exemplos/csharp-console/`](../../exemplos/csharp-console/): cria um tópico, publica, consome com long polling e confirma; lê o endereço e o token raiz de variáveis de ambiente.

## Os planejados: versão-base e publicação

Valem como **plano** (propostas, sujeitas a D68, D70.1, D70.2). "Versão-base" é a mais antiga que fala TLS 1.2, tem cliente HTTP com `PATCH` e confere o certificado.

| Linguagem | Versão-base | Por quê | HTTP / JSON | Publicação |
|---|---|---|---|---|
| Java | **11** (2018) | O `HttpURLConnection` **nunca** fez `PATCH`; o `java.net.http` do 11 faz | `java.net.http.HttpClient` / leitor mínimo | Maven Central |
| Python | **2.7.9** e **3.4.3** | As primeiras que conferem o certificado por padrão e falam TLS 1.2 | `http.client` / `json` | PyPI |
| PHP | **5.6** | TLS 1.2 e verificação ligada por padrão | `curl` / `json_decode` | Packagist |
| Ruby | **2.0** | `Net::HTTP::Patch` e `json` na biblioteca-padrão | `Net::HTTP` / `json` | RubyGems |
| Node.js | **0.12** | `Promise` nativa e TLS 1.2 por padrão | `https` / `JSON` | npm |
| Go | **1.7** | `context.Context` e `net/http` com `PATCH` | `net/http` / `encoding/json` | Proxy de módulos (tag `connectors/go/vX`) |
| C | **C89** + `libcurl` ≥ 7.34 | A `libcurl` 7.34 é a primeira que exige TLS 1.2 e faz `PATCH` | `libcurl` / leitor mínimo | Releases do GitHub |
| C++ | **C++03** | Um cabeçalho sobre a `libtaba` (C) | via `libtaba` | Releases do GitHub |

**Regras de código para versões antigas:** nada de sintaxe ou biblioteca posterior à versão-base (sem `record` no Java 11, sem `?.` no C# 7.3, sem *f-strings* no Python 2.7, sem `let`/`const` no Node 0.12); o CI compila e testa **na versão-base e na mais recente**; texto em **UTF-8 sempre**; o conector **não desliga a verificação do certificado** e **não depende do relógio local**. O Taba **não** aceita `X-HTTP-Method-Override`.

## Como escrever um conector novo

1. **Leia** [16-api-cliente.md](16-api-cliente.md) e este contrato; **use o `taba-cli`** como referência executável (cada comando é uma chamada da API, e o que ele faz é o que o conector repete).
2. **Pasta autônoma** `connectors/<linguagem>/`: `README.md` (instalação, um exemplo de 10 linhas, versão-base), o empacotamento da linguagem, `src/`, `test/` e `exemplos/`. Nenhuma pasta importa código de outra (exceto `cpp/` sobre `c/`).
3. **Implemente** as nove operações, a repetição da tabela acima, o leitor de JSON mínimo e os erros `client.*`.
4. **Teste**: unitários com um transporte de mentira (o Rust tem `taba_client::testing`; o C# tem `ScriptedHandler`) conferindo a **requisição montada** e a leitura de respostas prontas; integração contra um Taba em modo desenvolvimento, ou o Compose.
5. **Cite os casos de uso** nos testes numa linha `UC: UC-CNT-NNN` (o `uc_coverage` do servidor lê as citações das pastas de conectores também) ([26-casos-de-uso.md](26-casos-de-uso.md)).

> Planejado e **ainda inexistente**: a suíte de conformidade compartilhada (`connectors/contrato/` com fixtures e cenários) e o contrato OpenAPI, que congelaria o contrato para os oito conectores restantes ([32-roadmap.md](32-roadmap.md)). Hoje a "fonte da verdade" do contrato é este texto, o `taba-cli` e os testes dos dois conectores.

## Pendências desta área

D68 (confirmar as versões-base das demais linguagens), D70.1 (organização e identificadores de pacote em cada registro), D70.2 (espelho do PHP; versões independentes das do servidor): [31-pendencias.md](31-pendencias.md).

## Fontes e provas

- `connectors/rust/client/src` (`client`, `http`, `json`, `base64`, `transport`, `testing`), `connectors/rust/client/README.md`, `connectors/csharp/Taba.Client*`, `connectors/csharp/Taba.Client.Tests`, `exemplos/csharp-console`, `connectors/README.md`.
- Casos de uso: UC-CNT (35 casos, nível `K`; conectores); veja [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md). Testes: `connectors/rust/client/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`.
- Histórico: `19-conectores.md`, `19a-conectores-linguagens.md`.
