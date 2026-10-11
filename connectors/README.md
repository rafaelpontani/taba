# Conectores

Os conectores do Taba ([doc/19-conectores.md](../doc/19-conectores.md)), uma pasta por linguagem. Cada um é autônomo e fala a API de cliente ([doc/06a-api-cliente.md](../doc/06a-api-cliente.md)).

| Pasta | O que é | Estado |
|---|---|---|
| [`rust/`](rust/) | `taba-client`: as nove operações, o `subscribe` e a repetição de chamadas, com HTTP/1.1, TLS (`rustls`), JSON e base64 próprios | Síncrono pronto; a forma `async` e a publicação no crates.io faltam |
| [`cli/`](cli/) | `taba-cli`: a linha de comando do cliente ([doc/19b-cli.md](../doc/19b-cli.md)), sobre o conector Rust | Pronto; falta conferir a imagem Docker num Docker de verdade e o roteiro de ponta a ponta |
| [`csharp/`](csharp/) | `Taba.Client`: as nove operações em `...Async` e síncronas, o `subscribe`, a repetição de chamadas, JSON próprio, sem dependências; alvos `net461`, `net5.0`, `net6.0`, `net8.0` e `net10.0` | Pronto (unitários e integração contra o Compose, `scripts/integration.sh`; [exemplo de console](../exemplos/csharp-console/)); falta a publicação no nuget.org |
| `java/`, `c/`, `cpp/`, `python/`, `php/`, `ruby/`, `go/`, `node/` | Os demais ([plano](../doc/19-conectores.md), seção 18.8) | A fazer |

## Rust

Os dois crates formam um workspace do Cargo próprio (`Cargo.toml` desta pasta), fora do workspace do servidor:

```
cd connectors/rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Os testes citam os casos de uso (`/// UC: UC-CLI-001`) como os do servidor, e o `crates/taba-node/tests/uc_coverage.rs` os lê também daqui.
