# Taba.Client (C#)

O conector C# do Taba ([doc/19-conectores.md](../../doc/19-conectores.md), [doc/19a](../../doc/19a-conectores-linguagens.md), seção "C#").

- **Alvos:** `net461` (.NET clássico, Framework 4.6.1 a 4.8), `net5.0`, `net6.0` e `net8.0`, e o `net10.0` quando o SDK é o 10 ou mais novo. O código é o mesmo, em **C# 7.3** nos alvos todos.
- **Sem dependências:** só a biblioteca-padrão (`HttpClient`, `Task`). O JSON é um leitor mínimo próprio (`Json.cs`); o base64 é o `Convert`.
- **Assíncrono primeiro:** as nove operações têm a forma `...Async` (a implementação) e a síncrona, que espera por ela. Todas aceitam um `CancellationToken`.
- **Seguro entre threads:** um só `TabaClient` serve a várias tarefas e threads. Crie um por aplicação e reuse.
- **Repetição de chamadas** do contrato comum: `429` e `503` (esperando o `Retry-After`, ou 1, 2 e 4 s), `502` e `504` só na publicação em fila FIFO (`PublishOptions.Fifo`), falha ao conectar. Nunca um `4xx`, nem um estouro de tempo, nem uma falha depois de enviar o corpo.

```csharp
using Taba;

var config = new TabaConfig("http://localhost:8080") { Token = Environment.GetEnvironmentVariable("TABA_TOKEN") };
using (var client = new TabaClient(config))
{
    await client.CreateTopicAsync(new TopicRequest { Name = "pedidos", Type = "queue" });
    await client.PublishJsonAsync("pedidos", "{\"pedido\": 9001}");

    foreach (Message message in await client.ConsumeAsync("pedidos", new ConsumeOptions { WaitTime = 20 }))
    {
        Console.WriteLine(message.BodyText);
        await client.AckAsync("pedidos", message);
    }
}
```

`SubscribeAsync(topic, options, handler, cancellationToken)` faz o laço inteiro: long polling, chama o `handler` e confirma sozinho **só se ele terminar sem exceção** (a que falhar volta à fila ao fim do `visibilityTimeout`). Termina, sem exceção, quando o token é cancelado.

## Erros

Uma só exceção, `TabaException`, com `Status`, `Code`, `Message`, `RequestId`, `RetryAfter`, `Details` e `Body`. Teste sempre o `Code` (`topic.not-found`, `auth.forbidden`...), nunca a mensagem. Falhas do próprio cliente têm `Status` zero e `Code` `client.network`, `client.timeout`, `client.invalid-config` ou `client.invalid-response`.

## O corpo da mensagem

`Message.Body` são sempre **bytes**: o texto JSON tal qual veio, o texto UTF-8, ou os bytes já decodificados do base64. `Message.BodyText` é o mesmo em UTF-8. As datas ficam como texto RFC 3339.

## TLS

O conector usa a TLS da plataforma e **não** a altera, e nunca desliga a verificação do certificado. No .NET Framework 4.6.x o protocolo é o de `ServicePointManager.SecurityProtocol`: confira que inclui o TLS 1.2 (`ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12`, na inicialização do aplicativo). Do 4.7 em diante vale o do sistema. Para confiar numa CA própria, instale-a no repositório de certificados do sistema.

O `ConnectTimeout` só vale do .NET 5 em diante; no Framework o `HttpClient` não tem esse limite, e vale o `RequestTimeout` (30 s) da chamada inteira.

## Testes

```
cd connectors/csharp
dotnet test Taba.Client.Tests          # os unitários (os de integração são ignorados sem TABA_URL)
scripts/integration.sh                  # tudo, com um cluster de verdade
```

- **Unitários** (`ClientTests`, `JsonTests`, `PackagingTests`): sem rede, com um `HttpMessageHandler` de mentira (`ScriptedHandler`). Rodam no .NET 6, 8 e 10.
- **Integração** (`IntegrationTests`, categoria `Integration`): contra um Taba de verdade. O [`scripts/integration.sh`](scripts/integration.sh) sobe o `docker-compose.yml` da raiz (três nós e o HAProxy), espera os nós ficarem saudáveis, cria pelo `taba-cli` os tokens dos testes (um da aplicação e um só de leitura) e as filas (uma comum, uma FIFO e um distribuidor com duas filas, uma delas FIFO para a falha parcial), roda o `dotnet test` com `TABA_URL`, `TABA_TOKEN` e `TABA_TOKEN_READONLY` e derruba tudo no fim. `--keep` deixa o cluster de pé, `--no-build` reaproveita as imagens, e o que vem depois de `--` vai para o `dotnet test`. Também dá para apontar os testes para um cluster seu, definindo as três variáveis e as filas `csharp-shared`, `csharp-fan`, `csharp-fan-a` e `csharp-fan-b`.

Os testes citam os casos de uso (`/// UC: UC-CNT-001`) como os do servidor, e o `crates/taba-node/tests/uc_coverage.rs` os lê também daqui.

**Ainda falta:** a publicação no nuget.org, as fixtures e os cenários compartilhados de `contrato/` e a conferência no .NET Framework 4.8 (os testes rodam no .NET moderno; o `net461` só é compilado).
