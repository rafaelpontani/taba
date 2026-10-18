# taba-client (Rust)

The Taba Rust connector ([doc/pt-BR/21-conectores.md](../../../doc/pt-BR/21-conectores.md), section 18.10.3).

- **Its own HTTP/1.1** over `std::net::TcpStream` (building the request and reading the response are pure functions in `http`), **TLS 1.2 or later** through `rustls` with the system roots (plus an optional CA of your own) and its own **JSON** and **base64**. The only dependencies are `rustls` and `rustls-native-certs` (plus `serde`, only in the optional `serde` *feature*).
- **Thread-safe:** `Client` is `Send + Sync`; a single one serves many.
- **Call retries** of the common contract: `429` and `503` (with `Retry-After`), `502` and `504` only on FIFO, a network failure before sending; never a `4xx` nor a timeout.

```rust
use taba_client::{Client, Config, ConsumeOptions, PublishOptions, TopicRequest};

fn main() -> Result<(), taba_client::Error> {
    let mut config = Config::new("http://127.0.0.1:8080");
    config.token = std::env::var("TABA_TOKEN").ok();
    let client = Client::new(config)?;

    client.create_topic(&TopicRequest {
        name: "orders".into(),
        topic_type: "queue".into(),
        ..TopicRequest::default()
    })?;
    client.publish("orders", br#"{"order": 9001}"#, &PublishOptions::default())?;

    let options = ConsumeOptions { wait_time: Some(20), ..ConsumeOptions::default() };
    for message in client.consume("orders", &options)? {
        println!("{}", String::from_utf8_lossy(&message.body));
        client.ack("orders", &message.id, &message.receipt_token)?;
    }
    Ok(())
}
```

## Consuming with a *handler* and objects (D140)

The simplest use: publish and consume **objects**, without serializing or deleting anything by hand. With the `serde` *feature* (`taba-client = { features = ["serde"] }`; without it the connector stays with `rustls` only):

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Order { number: u32 }

client.publish_object("orders", &Order { number: 9001 })?;

client.consume_typed::<Order, _>("orders", &ConsumeOptions::default(), |d| {
    process_order(&d.body)?;              // d.body: Order already deserialized
    Ok(())
})?;
```

`consume_typed` reads with long polling, hands over the body already deserialized, **deletes the message if the *handler* returns `Ok`** and renews the invisibility while it runs. The details and customizations follow.

If the body is not a `T`, `consume_typed` returns `client.invalid-response` before the *handler* and the message is not deleted. Run the tests with the *feature*: `cargo test --workspace --all-features`.

### Without `serde`: `consume_with`

Besides `consume`, which returns the list, `consume_with` reads from the queue with **long polling** (20 s), **one message at a time** and **120 s of invisibility** (this mode's defaults; pass a `ConsumeOptions` to change them), hands each message to a `Delivery` and **deletes the message if the *handler* returns `Ok`** (`delivery.delete` is `true` by default). While the *handler* runs, a second *thread* **renews the invisibility** every third of it, so no other consumer takes the message. If the *handler* returns `Err`, the message is not deleted and the error propagates as `ConsumeError::Handler`.

Minimal:

```rust
loop {
    client.consume_with("orders", &ConsumeOptions::default(), |d| {
        println!("{}", String::from_utf8_lossy(&d.message.body));
        Ok(())
    })?;
}
```

Complete, annotated:

```rust
let stop = std::sync::atomic::AtomicBool::new(false); // elsewhere (a signal) sets true to end
while !stop.load(std::sync::atomic::Ordering::Relaxed) {
    // Each round is a long polling: with no messages it returns 0 after `wait_time`; with messages, it calls the handler.
    let handled = client.consume_with(
        "orders",
        &ConsumeOptions {
            max_messages: Some(1),        // default 1
            wait_time: Some(20),          // default 20 s (long polling)
            visibility_timeout: Some(120), // default 120 s; the renewal repeats it every 1/3
            ..ConsumeOptions::default()
        },
        |d| {
            // d.message carries everything: id, delivery_count, published_at, group_id, receipt_token, body (bytes)...
            process(&d.message.body)?;     // the `?` returns Err: the message is NOT deleted and goes back to the queue
            // Need more time in one go? This changes the invisibility and the base of the renewals:
            // d.change_visibility(600)?;
            d.delete = true;               // default true; false leaves the message in the queue
            if d.lease_lost() { /* the server refused a renewal: the message already went back to the queue */ }
            Ok(())
        },
    )?;                                    // returns how many messages were handled
}
```

`Client::call` and `Call` are the lower layer (route, query, body): it is what `taba-cli` uses to print the responses as the server returned them. `taba_client::testing` is a fake transport for the tests of whoever uses the connector.

**Still missing:** the `async` form (`async` *feature*, `tokio`), a `Transport` the application can swap, and publishing to crates.io.

## License

[Apache-2.0](https://github.com/rafaelpontani/taba/blob/main/connectors/LICENSE). The Taba server is AGPL-3.0-only; the connectors are Apache-2.0 so any application, open or closed, can embed them.
