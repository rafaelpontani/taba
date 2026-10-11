# taba-client (Rust)

O conector Rust do Taba ([doc/19-conectores.md](../../../doc/19-conectores.md), [doc/19a](../../../doc/19a-conectores-linguagens.md), seção 18.10.3).

- **HTTP/1.1 próprio** sobre `std::net::TcpStream` (montar a requisição e ler a resposta são funções puras em `http`), **TLS 1.2 ou superior** pelo `rustls` com as raízes do sistema (mais uma CA própria opcional) e **JSON** e **base64** próprios. As únicas dependências são o `rustls` e o `rustls-native-certs`.
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

`Client::call` e `Call` são a camada de baixo (rota, query, corpo): é o que o `taba-cli` usa para imprimir as respostas como o servidor as devolveu. `taba_client::testing` é um transporte de mentira para os testes de quem usa o conector.

**Ainda falta:** a forma `async` (*feature* `async`, `tokio`), o `Transport` trocável pela aplicação e a publicação no crates.io.
