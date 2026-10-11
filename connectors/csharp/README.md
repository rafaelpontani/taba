# Taba.Client (C#)

O conector C# do Taba ([doc/19-conectores.md](../../doc/19-conectores.md), [doc/19a](../../doc/19a-conectores-linguagens.md), seção "C#").

- **Alvos:** `net461` (.NET clássico, Framework 4.6.1 a 4.8), `net5.0`, `net6.0` e `net8.0`, e o `net10.0` quando o SDK é o 10 ou mais novo. O código é o mesmo, em **C# 7.3** nos alvos todos.
- **Sem dependências:** só a biblioteca-padrão (`HttpClient`, `Task`). O JSON é um leitor mínimo próprio (`Json.cs`); o base64 é o `Convert`.
- **Três pacotes:** `Taba.Client.Extensions.DependencyInjection` (o `AddTabaClient`, com `IOptions`; ver "Injeção de dependência"), e os dois abaixo.
- **Dois pacotes:** `Taba.Client.Abstractions` (a interface `ITabaClient`, os modelos, uma classe por arquivo, e a `TabaException`) e `Taba.Client` (o `TabaClient`, o `HttpClient` e o leitor de JSON). Aplicações e bibliotecas dependem de `ITabaClient` e simulam o cliente nos testes sem trazer o `HttpClient`.
- **Assíncrono primeiro:** as nove operações têm a forma `...Async` (a implementação) e a síncrona, que espera por ela. Todas aceitam um `CancellationToken`.
- **Seguro entre threads:** um só `TabaClient` serve a várias tarefas e threads. Crie um por aplicação e reuse.
- **Repetição de chamadas** do contrato comum: `429` e `503` (esperando o `Retry-After`, ou 1, 2 e 4 s), `502` e `504` só na publicação em fila FIFO (`PublishOptions.Fifo`), falha ao conectar. Nunca um `4xx`, nem um estouro de tempo, nem uma falha depois de enviar o corpo.

```csharp
using Taba;

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

`SubscribeAsync(topic, options, handler, cancellationToken)` faz o laço inteiro: long polling, chama o `handler` e confirma sozinho **só se ele terminar sem exceção** (a que falhar volta à fila ao fim do `visibilityTimeout`). Termina, sem exceção, quando o token é cancelado.

**Consumo com o objeto já deserializado (o uso mais comum):** `ConsumeAsync<T>` lê da fila (com long polling de 20 s, uma mensagem por vez e 120 s de invisibilidade por padrão), entrega o corpo como `T` em `context.Body` (mesmo serializador do `PublishJsonAsync`; a mensagem inteira segue em `context.Message`) e apaga a mensagem se o handler termina sem exceção. Enquanto o handler roda, a invisibilidade é renovada sozinha. Se o corpo não é um `T`, lança `client.invalid-response` antes de chamar o handler, e a mensagem não é apagada.

Mínimo:

```csharp
await client.ConsumeAsync<Pedido>("pedidos", async context =>
{
    await ProcessarAsync(context.Body);   // Pedido já deserializado
});
```

Padrões: `MaxMessages` 1, `WaitTime` 20 s (long polling) e `VisibilityTimeout` 120 s, que a renovação automática mantém (a cada um terço dele). Passe um `ConsumeOptions` para mudar. No handler: `context.Delete = false` deixa a mensagem na fila, `context.ChangeVisibilityAsync(segundos)` pede mais tempo de uma vez, e `context.LeaseLost` fica `true` se o servidor recusou uma renovação. Handler que lança: a mensagem não é apagada e a exceção sobe. Corpo que não é um `T`: `client.invalid-response` antes do handler, sem apagar. Um laço com `CancellationToken` está no exemplo do `BackgroundService`, na seção "Injeção de dependência".

Sem o objeto deserializado, use o `ConsumeAsync(topic, handler)` (ou `(topic, options, handler)`): o `context` é o mesmo, sem o `Body`, e o corpo vem em `context.Message.Body` (bytes) e `context.Message.BodyText`.

**Publicar objetos:** `PublishJsonAsync(topic, objeto)` serializa o objeto (`System.Text.Json` no .NET 5 em diante, `DataContractJsonSerializer` no .NET Framework, ou o `ITabaJsonSerializer` de `TabaConfig.JsonSerializer`); o JSON já pronto vai em `PublishRawJsonAsync`, e os bytes que você mesmo serializou, em `PublishAsync`.
 
## Injeção de dependência

O pacote **`Taba.Client.Extensions.DependencyInjection`** (à parte, para o `Taba.Client` seguir sem dependências) traz o `AddTabaClient`, que registra o `ITabaClient` como *singleton* (um só `TabaClient` por aplicação, descartado pelo contêiner) com o `TabaConfig` vindo do `IOptions<TabaConfig>`. Quem consome depende só de `ITabaClient` (pacote `Taba.Client.Abstractions`).

Mínimo (o caminho da seção no `appsettings.json`; o `IConfiguration` só é lido quando o `ITabaClient` é pedido pela primeira vez, então não há dependência dele na montagem dos serviços):

```csharp
// appsettings.json:  "Taba": { "BaseUrl": "http://localhost:8080", "Token": "tb_..." }
builder.Services.AddTabaClient("Taba");   // ou "Servicos:Taba", para uma seção aninhada

public class PedidoService
{
    private readonly ITabaClient _taba;

    public PedidoService(ITabaClient taba)
    {
        _taba = taba;
    }

    public Task PublicarAsync(Pedido pedido)
    {
        return _taba.PublishJsonAsync("pedidos", pedido);
    }
}
```

E o consumidor mínimo, um serviço de segundo plano (o token do host encerra o laço ao parar o serviço):

```csharp
builder.Services.AddHostedService<PedidosConsumer>();

public class PedidosConsumer : BackgroundService
{
    private readonly ITabaClient _taba;

    public PedidosConsumer(ITabaClient taba)
    {
        _taba = taba;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            // Long polling de 20 s; a mensagem é apagada ao fim do handler, se ele não lançar.
            await _taba.ConsumeAsync<Pedido>("pedidos", context => ProcessarAsync(context.Body), stoppingToken);
        }
    }
}
```

Completo, com todas as formas de configurar, o serializador do contêiner, o consumo em segundo plano e o teste:

```csharp
// appsettings.json (os nomes são os das propriedades do TabaConfig; TimeSpan como "hh:mm:ss"):
// "Taba": {
//   "BaseUrl": "https://taba.interno:8443",
//   "Token": "tb_...",                 // em produção, de um cofre de segredos (user-secrets, variável de ambiente, Key Vault)
//   "ConnectTimeout": "00:00:05",
//   "RequestTimeout": "00:00:30",
//   "MaxAttempts": 3,
//   "UserAgent": "taba-csharp/0.1.0 meu-servico/1.0"
// }

// 1) pelo caminho da seção (o IConfiguration é lido só ao resolver o ITabaClient):
builder.Services.AddTabaClient("Taba");

// 1b) ou passando a própria seção, se você já tem o IConfiguration em mãos:
builder.Services.AddTabaClient(builder.Configuration.GetSection("Taba"));

// 2) ou no código:
builder.Services.AddTabaClient(config =>
{
    config.BaseUrl = "https://taba.interno:8443";
    config.Token = Environment.GetEnvironmentVariable("TABA_TOKEN");
});

// 3) ou pelo IOptions de sempre (Configure, PostConfigure, IConfigureOptions<TabaConfig>...), e depois só registrar:
builder.Services.Configure<TabaConfig>(builder.Configuration.GetSection("Taba"));
builder.Services.PostConfigure<TabaConfig>(config => config.UserAgent += " meu-servico/1.0");
builder.Services.AddTabaClient();

// O serializador do PublishJson e do Consume<T>: o ITabaJsonSerializer registrado no contêiner vale
// (se o TabaConfig.JsonSerializer não foi informado). Sem ele, o padrão da plataforma.
builder.Services.AddSingleton<ITabaJsonSerializer, MeuSerializadorNewtonsoft>();

// O consumo vira um serviço de segundo plano: o token do host encerra o laço no Ctrl+C ou ao parar o serviço.
public class PedidosWorker : BackgroundService
{
    private readonly ITabaClient _taba;
    private readonly ILogger<PedidosWorker> _log;

    public PedidosWorker(ITabaClient taba, ILogger<PedidosWorker> log)
    {
        _taba = taba;
        _log = log;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                await _taba.ConsumeAsync<Pedido>("pedidos", async context =>
                {
                    _log.LogInformation("Pedido {Numero} ({Id})", context.Body.Numero, context.Message.Id);
                    await ProcessarAsync(context.Body, context.CancellationToken);
                }, stoppingToken);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                // o serviço está parando
            }
            catch (TabaException e)
            {
                _log.LogError(e, "Taba: {Code}", e.Code);       // teste sempre o Code, nunca a mensagem
                await Task.Delay(TimeSpan.FromSeconds(5), stoppingToken).ConfigureAwait(false);
            }
        }
    }
}

builder.Services.AddHostedService<PedidosWorker>();
```

O `BaseUrl` inválido só falha (`TabaException` com `client.invalid-config`) quando o `ITabaClient` é resolvido pela primeira vez; para falhar na partida, resolva-o ao iniciar (`app.Services.GetRequiredService<ITabaClient>()`).

Nos testes, como quem consome depende só da interface, troque o cliente por um simulado (Moq, NSubstitute...) ou por um `HttpMessageHandler` de mentira: `new TabaClient(config, handler)` aceita o seu handler (o cliente não o descarta). Sem o `Microsoft.Extensions.DependencyInjection`, registre o `TabaClient` como instância única da `ITabaClient` em qualquer contêiner (Autofac, Ninject, Unity...).

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
