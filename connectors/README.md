# Connectors

The Taba connectors ([doc/pt-BR/21-conectores.md](../doc/pt-BR/21-conectores.md)), one folder per language. Each one is self-contained and speaks the client API ([doc/pt-BR/16-api-cliente.md](../doc/pt-BR/16-api-cliente.md)).

| Folder | What it is | Status |
|---|---|---|
| [`rust/`](rust/) | `taba-client`: the nine operations, `subscribe` and call retries, with its own HTTP/1.1, TLS (`rustls`), JSON and base64 | Synchronous ready; the `async` form and publishing to crates.io are missing |
| [`cli/`](cli/) | `taba-cli`: the client command line ([doc/pt-BR/20-cli.md](../doc/pt-BR/20-cli.md)), on top of the Rust connector | Ready; the Docker image still needs checking on a real Docker, and so does the end-to-end script |
| [`csharp/`](csharp/) | `Taba.Client`: the nine operations as `...Async` and synchronous, `subscribe`, call retries, its own JSON, no dependencies; targets `net461`, `net5.0`, `net6.0`, `net8.0` and `net10.0` | Ready (unit tests and integration against Compose, `scripts/integration.sh`; [console example](../examples/csharp-console/)); publishing to nuget.org is missing |
| `java/`, `c/`, `cpp/`, `python/`, `php/`, `ruby/`, `go/`, `node/` | The rest ([plan](../doc/pt-BR/21-conectores.md)) | To do |

## Rust

The two crates form their own Cargo workspace (the `Cargo.toml` in this folder), outside the server workspace:

```
cd connectors/rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

The tests cite the use cases (`/// UC: UC-CLI-001`) like the server's, and `crates/taba-node/tests/uc_coverage.rs` reads them from here too.

## License

[Apache-2.0](LICENSE). The Taba server is AGPL-3.0-only; the connectors are Apache-2.0 so any application, open or closed, can embed them.
