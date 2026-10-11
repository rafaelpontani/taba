# taba-client (Rust)

O conector Rust do Taba ([doc/19-conectores.md](../../../doc/19-conectores.md), [doc/19a](../../../doc/19a-conectores-linguagens.md), seção 18.10.3).

- **HTTP/1.1 próprio** sobre `std::net::TcpStream` (montar a requisição e ler a resposta são funções puras em `http`), **TLS 1.2 ou superior** pelo `rustls` com as raízes do sistema (mais uma CA própria opcional) e **JSON** e **base64** próprios. As únicas dependências são o `rustls` e o `rustls-native-certs` (mais o `serde`, só na *feature* opcional `serde`).
- **Seguro entre threads:** o `Client` é `Send + Sync`; um só serve a vários.
- **Repetição de chamadas** do contrato comum: `429` e `503` (com o `Retry-After`), `502` e `504` só na FIFO, falha de rede antes de enviar; nunca um `4xx` nem um estouro de tempo.

```rust
use taba_client::{Client, Config, ConsumeOptions, PublishOptions, TopicRequest};

fn main() -> Result<(), taba_client::Error> {
    let mut config = Config::new("http://127.0.0.1:8080");
    config.token = std::env::var("TABA_TOKEN").ok();
    let client = Client::new(config)?;

    client.create_topic(&TopicRequest {
        name: "pedidos".into(),
        topic_type: "queue".into(),
        ..TopicRequest::default()
    })?;
    client.publish("pedidos", br#"{"pedido": 9001}"#, &PublishOptions::default())?;

    let options = ConsumeOptions { wait_time: Some(20), ..ConsumeOptions::default() };
    for message in client.consume("pedidos", &options)? {
        println!("{}", String::from_utf8_lossy(&message.body));
        client.ack("pedidos", &message.id, &message.receipt_token)?;
    }
    Ok(())
}
```

## Consumo com *handler* e objetos (D140)

O uso mais simples: publicar e consumir **objetos**, sem serializar nem apagar nada à mão. Com a *feature* `serde` (`taba-client = { features = ["serde"] }`; fora dela o conector segue só com o `rustls`):

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Pedido { numero: u32 }

client.publish_object("pedidos", &Pedido { numero: 9001 })?;

client.consume_typed::<Pedido, _>("pedidos", &ConsumeOptions::default(), |d| {
    processar_pedido(&d.body)?;           // d.body: Pedido já deserializado
    Ok(())
})?;
```

O `consume_typed` lê com long polling, entrega o corpo já deserializado, **apaga a mensagem se o *handler* devolve `Ok`** e renova a invisibilidade enquanto ele roda. Os detalhes e as personalizações vêm a seguir.

Se o corpo não é um `T`, o `consume_typed` devolve `client.invalid-response` antes do *handler* e a mensagem não é apagada. Rodar os testes com a *feature*: `cargo test --workspace --all-features`.

### Sem o `serde`: `consume_with`

Além do `consume` que devolve a lista, o `consume_with` lê da fila com **long polling** (20 s), **uma mensagem por vez** e **120 s de invisibilidade** (os padrões deste modo; passe um `ConsumeOptions` para mudar), entrega cada mensagem a um `Delivery` e **apaga a mensagem se o *handler* devolve `Ok`** (`delivery.delete` é `true` por padrão). Enquanto o *handler* roda, uma segunda *thread* **renova a invisibilidade** a cada um terço dela, para nenhum outro consumidor pegar a mensagem. Se o *handler* devolve `Err`, a mensagem não é apagada e o erro sobe como `ConsumeError::Handler`.

Mínimo:

```rust
loop {
    client.consume_with("pedidos", &ConsumeOptions::default(), |d| {
        println!("{}", String::from_utf8_lossy(&d.message.body));
        Ok(())
    })?;
}
```

Completo, com anotações:

```rust
let stop = std::sync::atomic::AtomicBool::new(false); // outro lugar (um sinal) põe true para encerrar
while !stop.load(std::sync::atomic::Ordering::Relaxed) {
    // Cada volta é um long polling: sem mensagens devolve 0 após `wait_time`; com mensagens, chama o handler.
    let handled = client.consume_with(
        "pedidos",
        &ConsumeOptions {
            max_messages: Some(1),        // padrão 1
            wait_time: Some(20),          // padrão 20 s (long polling)
            visibility_timeout: Some(120), // padrão 120 s; a renovação o repete a cada 1/3
            ..ConsumeOptions::default()
        },
        |d| {
            // d.message traz tudo: id, delivery_count, published_at, group_id, receipt_token, body (bytes)...
            processar(&d.message.body)?;   // o `?` devolve Err: a mensagem NÃO é apagada e volta à fila
            // Mais tempo de uma vez? Isto muda a invisibilidade e a base das renovações:
            // d.change_visibility(600)?;
            d.delete = true;               // padrão true; false deixa a mensagem na fila
            if d.lease_lost() { /* o servidor recusou uma renovação: a mensagem já voltou à fila */ }
            Ok(())
        },
    )?;                                    // devolve quantas mensagens foram tratadas
}
```

`Client::call` e `Call` são a camada de baixo (rota, query, corpo): é o que o `taba-cli` usa para imprimir as respostas como o servidor as devolveu. `taba_client::testing` é um transporte de mentira para os testes de quem usa o conector.

**Ainda falta:** a forma `async` (*feature* `async`, `tokio`), o `Transport` trocável pela aplicação e a publicação no crates.io.
