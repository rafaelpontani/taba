# Taba.Client (C#)

The Taba C# connector. Documentation: [Connectors, C#](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/21-connectors.md#c-tabaclient).

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md#c-tabaclient) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Targets:** `net461` (classic .NET, Framework 4.6.1 to 4.8), `net5.0`, `net6.0` and `net8.0`, plus `net10.0` when the SDK is 10 or newer. The code is the same, in **C# 7.3** on every target.
- **No dependencies:** only the standard library (`HttpClient`, `Task`). JSON is a minimal reader of its own (`Json.cs`); base64 is `Convert`.
- **Three packages:** `Taba.Client.Extensions.DependencyInjection` (`AddTabaClient`, with `IOptions`; see "Dependency injection"), and the two below.
- **Two packages:** `Taba.Client.Abstractions` (the `ITabaClient` interface, the models, one class per file, and `TabaException`) and `Taba.Client` (`TabaClient`, the `HttpClient` and the JSON reader). Applications and libraries depend on `ITabaClient` and mock the client in tests without pulling in the `HttpClient`.
- **Async first:** the nine operations have an `...Async` form (the implementation) and a synchronous one that waits for it. All accept a `CancellationToken`.
- **Thread-safe:** a single `TabaClient` serves several tasks and threads. Create one per application and reuse it.
- **Call retries** of the common contract: `429` and `503` (waiting for `Retry-After`, or 1, 2 and 4 s), `502` and `504` only when publishing to a FIFO queue (`PublishOptions.Fifo`), connection failures. Never a `4xx`, nor a timeout, nor a failure after the body was sent.

```csharp
using Taba;

var config = new TabaConfig("http://localhost:8080") { Token = Environment.GetEnvironmentVariable("TABA_TOKEN") };
using (var client = new TabaClient(config))
{
    await client.CreateTopicAsync(new TopicRequest { Name = "orders", Type = "queue" });
    await client.PublishJsonAsync("orders", new { order = 9001 });

    foreach (Message message in await client.ConsumeAsync("orders", new ConsumeOptions { WaitTime = 20 }))
    {
        Console.WriteLine(message.BodyText);
        await client.AckAsync("orders", message);
    }
}
```

`SubscribeAsync(topic, options, handler, cancellationToken)` runs the whole loop: long polling, calls the `handler` and acknowledges by itself **only if it finishes without an exception** (a failing one goes back to the queue at the end of the `visibilityTimeout`). It ends, without an exception, when the token is cancelled.

**Consuming with the object already deserialized (the most common use):** `ConsumeAsync<T>` reads from the queue (with 20 s long polling, one message at a time and 120 s of invisibility by default), hands the body over as `T` in `context.Body` (same serializer as `PublishJsonAsync`; the whole message stays in `context.Message`) and deletes the message if the handler finishes without an exception. While the handler runs, the invisibility is renewed automatically. If the body is not a `T`, it throws `client.invalid-response` before calling the handler, and the message is not deleted.

Minimal:

```csharp
await client.ConsumeAsync<Order>("orders", async context =>
{
    await ProcessAsync(context.Body);   // Order already deserialized
});
```

Defaults: `MaxMessages` 1, `WaitTime` 20 s (long polling) and `VisibilityTimeout` 120 s, which the automatic renewal keeps (every third of it). Pass a `ConsumeOptions` to change them. In the handler: `context.Delete = false` leaves the message in the queue, `context.ChangeVisibilityAsync(seconds)` asks for more time in one go, and `context.LeaseLost` becomes `true` if the server refused a renewal. A handler that throws: the message is not deleted and the exception propagates. A body that is not a `T`: `client.invalid-response` before the handler, without deleting. A loop with a `CancellationToken` is in the `BackgroundService` example, in the "Dependency injection" section.

Without the deserialized object, use `ConsumeAsync(topic, handler)` (or `(topic, options, handler)`): the `context` is the same, without `Body`, and the body comes in `context.Message.Body` (bytes) and `context.Message.BodyText`.

**Publishing objects:** `PublishJsonAsync(topic, obj)` serializes the object (`System.Text.Json` on .NET 5 and later, `DataContractJsonSerializer` on .NET Framework, or the `ITabaJsonSerializer` from `TabaConfig.JsonSerializer`); ready-made JSON goes in `PublishRawJsonAsync`, and bytes you serialized yourself, in `PublishAsync`.

## Dependency injection

The **`Taba.Client.Extensions.DependencyInjection`** package (separate, so `Taba.Client` stays dependency-free) brings `AddTabaClient`, which registers `ITabaClient` as a *singleton* (a single `TabaClient` per application, disposed by the container) with the `TabaConfig` coming from `IOptions<TabaConfig>`. Consumers depend only on `ITabaClient` (the `Taba.Client.Abstractions` package).

Minimal (the section path in `appsettings.json`; the `IConfiguration` is only read when the `ITabaClient` is requested for the first time, so there is no dependency on it while assembling the services):

```csharp
// appsettings.json:  "Taba": { "BaseUrl": "http://localhost:8080", "Token": "tb_..." }
builder.Services.AddTabaClient("Taba");   // or "Services:Taba", for a nested section

public class OrderService
{
    private readonly ITabaClient _taba;

    public OrderService(ITabaClient taba)
    {
        _taba = taba;
    }

    public Task PublishAsync(Order order)
    {
        return _taba.PublishJsonAsync("orders", order);
    }
}
```

And the minimal consumer, a background service (the host's token ends the loop when the service stops):

```csharp
builder.Services.AddHostedService<OrdersConsumer>();

public class OrdersConsumer : BackgroundService
{
    private readonly ITabaClient _taba;

    public OrdersConsumer(ITabaClient taba)
    {
        _taba = taba;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        while (!stoppingToken.IsCancellationRequested)
        {
            // 20 s long polling; the message is deleted at the end of the handler, if it does not throw.
            await _taba.ConsumeAsync<Order>("orders", context => ProcessAsync(context.Body), stoppingToken);
        }
    }
}
```

Complete, with every way of configuring, the container's serializer, background consuming and testing:

```csharp
// appsettings.json (the names are those of the TabaConfig properties; TimeSpan as "hh:mm:ss"):
// "Taba": {
//   "BaseUrl": "https://taba.internal:8443",
//   "Token": "tb_...",                 // in production, from a secret store (user-secrets, environment variable, Key Vault)
//   "ConnectTimeout": "00:00:05",
//   "RequestTimeout": "00:00:30",
//   "MaxAttempts": 3,
//   "UserAgent": "taba-csharp/0.1.0 my-service/1.0"
// }

// 1) by section path (the IConfiguration is only read when resolving the ITabaClient):
builder.Services.AddTabaClient("Taba");

// 1b) or passing the section itself, if you already have the IConfiguration at hand:
builder.Services.AddTabaClient(builder.Configuration.GetSection("Taba"));

// 2) or in code:
builder.Services.AddTabaClient(config =>
{
    config.BaseUrl = "https://taba.internal:8443";
    config.Token = Environment.GetEnvironmentVariable("TABA_TOKEN");
});

// 3) or through the usual IOptions (Configure, PostConfigure, IConfigureOptions<TabaConfig>...), then just register:
builder.Services.Configure<TabaConfig>(builder.Configuration.GetSection("Taba"));
builder.Services.PostConfigure<TabaConfig>(config => config.UserAgent += " my-service/1.0");
builder.Services.AddTabaClient();

// The serializer of PublishJson and Consume<T>: the ITabaJsonSerializer registered in the container applies
// (if TabaConfig.JsonSerializer was not set). Without it, the platform default.
builder.Services.AddSingleton<ITabaJsonSerializer, MyNewtonsoftSerializer>();

// Consuming becomes a background service: the host's token ends the loop on Ctrl+C or when the service stops.
public class OrdersWorker : BackgroundService
{
    private readonly ITabaClient _taba;
    private readonly ILogger<OrdersWorker> _log;

    public OrdersWorker(ITabaClient taba, ILogger<OrdersWorker> log)
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
                await _taba.ConsumeAsync<Order>("orders", async context =>
                {
                    _log.LogInformation("Order {Number} ({Id})", context.Body.Number, context.Message.Id);
                    await ProcessAsync(context.Body, context.CancellationToken);
                }, stoppingToken);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                // the service is stopping
            }
            catch (TabaException e)
            {
                _log.LogError(e, "Taba: {Code}", e.Code);       // always test the Code, never the message
                await Task.Delay(TimeSpan.FromSeconds(5), stoppingToken).ConfigureAwait(false);
            }
        }
    }
}

builder.Services.AddHostedService<OrdersWorker>();
```

An invalid `BaseUrl` only fails (`TabaException` with `client.invalid-config`) when the `ITabaClient` is resolved for the first time; to fail at startup, resolve it on start (`app.Services.GetRequiredService<ITabaClient>()`).

In tests, since consumers depend only on the interface, swap the client for a mock (Moq, NSubstitute...) or for a fake `HttpMessageHandler`: `new TabaClient(config, handler)` accepts your handler (the client does not dispose it). Without `Microsoft.Extensions.DependencyInjection`, register `TabaClient` as the single instance of `ITabaClient` in any container (Autofac, Ninject, Unity...).

## Errors

A single exception, `TabaException`, with `Status`, `Code`, `Message`, `RequestId`, `RetryAfter`, `Details` and `Body`. Always test the `Code` (`topic.not-found`, `auth.forbidden`...), never the message. Failures of the client itself have `Status` zero and `Code` `client.network`, `client.timeout`, `client.invalid-config` or `client.invalid-response`.

## The message body

`Message.Body` is always **bytes**: the JSON text as it came, the UTF-8 text, or the bytes already decoded from base64. `Message.BodyText` is the same in UTF-8. Dates stay as RFC 3339 text.

## TLS

The connector uses the platform's TLS and does **not** change it, and never turns certificate verification off. On .NET Framework 4.6.x the protocol is the one in `ServicePointManager.SecurityProtocol`: check that it includes TLS 1.2 (`ServicePointManager.SecurityProtocol |= SecurityProtocolType.Tls12`, at application startup). From 4.7 on, the system's applies. To trust your own CA, install it in the system certificate store.

`ConnectTimeout` only applies from .NET 5 on; on the Framework the `HttpClient` has no such limit, and the `RequestTimeout` (30 s) of the whole call applies.

## Tests

```
cd connectors/csharp
dotnet test Taba.Client.Tests          # the unit tests (the integration ones are ignored without TABA_URL)
scripts/integration.sh                  # everything, with a real cluster
```

- **Unit** (`ClientTests`, `JsonTests`, `PackagingTests`): no network, with a fake `HttpMessageHandler` (`ScriptedHandler`). They run on .NET 6, 8 and 10.
- **Contract tests** (`contract/run.sh`): the scenario every connector runs against a real node in Docker (`../contract/run.sh` for all of them, [how it works](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md)).
- **Integration** (`IntegrationTests`, category `Integration`): against a real Taba. [`scripts/integration.sh`](https://github.com/rafaelpontani/taba/blob/main/connectors/csharp/scripts/integration.sh) starts the root `docker-compose.yml` (the nodes and HAProxy), waits for the nodes to be healthy, creates through `taba-cli` the test tokens (one for the application and one read-only) and the queues (a plain one, a FIFO one and a distributor with two queues, one of them FIFO for the partial failure), runs `dotnet test` with `TABA_URL`, `TABA_TOKEN` and `TABA_TOKEN_READONLY` and tears everything down at the end. `--keep` leaves the cluster up, `--no-build` reuses the images, and what comes after `--` goes to `dotnet test`. You can also point the tests at a cluster of your own, by setting the three variables and the queues `csharp-shared`, `csharp-fan`, `csharp-fan-a` and `csharp-fan-b`.

The tests cite the use cases (`/// UC: UC-CNT-001`) like the server's, and `crates/taba-node/tests/uc_coverage.rs` reads them from here too.

**Still missing:** the shared fixtures and scenarios from `contrato/` and the check on .NET Framework 4.8 (the tests run on modern .NET; `net461` is only compiled).

## License

[Apache-2.0](https://github.com/rafaelpontani/taba/blob/main/connectors/LICENSE). The Taba server is AGPL-3.0-only; the connectors are Apache-2.0 so any application, open or closed, can embed them.
