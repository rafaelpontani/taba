# taba-client (Java)

The Java client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API in a blocking form and as `CompletableFuture`, `subscribe`, consuming with a handler (with automatic renewal of the invisibility) and the call retries of the common contract. **No runtime dependencies**: the JDK's `java.net.http.HttpClient` and its own minimal JSON reader.

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Java 11 or later.** Maven coordinates `io.github.rafaelpontani:taba-client` (provisional until the organization and package identifiers are decided).
- Safe for concurrent use: one `TabaClient` serves several threads.

```java
TabaConfig config = TabaConfig.of("http://localhost:8080").token(System.getenv("TABA_TOKEN"));

try (TabaClient client = new TabaClient(config)) {
    client.createTopic(TopicRequest.queue("orders"));
    client.publishText("orders", "hello");

    // Consume with a handler: the message is deleted if the handler does not throw.
    client.consume("orders", new ConsumeOptions().waitTime(20), context -> {
        System.out.println(context.getMessage().getBodyText());
    });

    // Or the whole loop, on its own thread, until it is closed.
    try (Subscription subscription = client.subscribe("orders", null, message -> {
        System.out.println(message.getBodyText());
    })) {
        Thread.sleep(60_000);
    }

    // Asynchronous: every operation has a ...Async form.
    CompletableFuture<Topic> topic = client.getTopicAsync("orders");
}
```

## JSON

The JDK has no JSON library, so the connector does not guess one. `publishJson` and the typed `consume(topic, Order.class, handler)` use the `TabaJsonSerializer` you put in the configuration; without it they fail with `client.invalid-config`. With Jackson:

```java
ObjectMapper mapper = new ObjectMapper();
config.jsonSerializer(new TabaJsonSerializer() {
    public String serialize(Object value) throws Exception { return mapper.writeValueAsString(value); }
    public <T> T deserialize(String json, Class<T> type) throws Exception { return mapper.readValue(json, type); }
});
```

`publish(topic, bytes)`, `publishRawJson` and `publishText` need no serializer.

## Configuration

One object (the client does not read environment variables: `TABA_*` belongs to the server).

| Method | Default | |
|---|---|---|
| `baseUrl` | required | `http://host:port` or `https://...` |
| `token` | none | `Authorization: Bearer` |
| `connectTimeout` | 5 s | `Duration` |
| `requestTimeout` | 30 s | `Duration`, up to the response headers; `consume` uses `waitTime + 10 s` |
| `maxAttempts` | 3 | counting the first |
| `userAgent` | `taba-java/<version>` | |
| `jsonSerializer` | none | see above |
| `executor` | daemon threads of the client | where the `...Async` calls run |
| `sslContext` | the JVM's | an own `SSLContext` (a private CA, for example) |

## What differs from the other connectors

- `TabaException` is **unchecked** and carries `getStatus()`, `getCode()`, `getMessage()`, `getRequestId()`, `getRetryAfter()`, `getDetails()`. Test the code (`client.network`, `client.timeout`, `topic.not-found`...), never the message.
- **Cancelling:** interrupt the thread of a blocking call, or `future.cancel(true)` on an async one; either ends with a `CancellationException` (never a `TabaException`) and leaves the interrupted flag set. `Subscription.close()` stops the loop and waits for it; `Subscription.completion()` ends exceptionally if the server failed while consuming.
- Handlers are a `ThrowingConsumer`: they may throw a checked exception, which comes out of `consume` as `TabaClient.HandlerFailedException`.
- `Message.getBody()` is a copy of the bytes; `getBodyText()` is the UTF-8 text.
- TLS is the JVM's; the certificate is always verified.

## Tests

```
cd connectors/java
mvn test
```

The tests (JUnit 5) start a local server with the JDK's `com.sun.net.httpserver` that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`). There are no integration tests against a real Taba yet.
