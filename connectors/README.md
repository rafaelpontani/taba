# Connectors

The Taba connectors ([doc/en-US/21-connectors.md](../doc/en-US/21-connectors.md)), one folder per language. Each one is self-contained and speaks the client API ([doc/en-US/16-client-api.md](../doc/en-US/16-client-api.md)).

| Folder | What it is | Status |
|---|---|---|
| [`rust/`](rust/) | `taba-client`: the nine operations, `subscribe` and call retries, with its own HTTP/1.1, TLS (`rustls`), JSON and base64 | Synchronous ready, on [crates.io](https://crates.io/crates/taba-client); the `async` form is missing; contract tests in [`rust/contract/`](rust/contract/) |
| [`cli/`](cli/) | `taba-cli`: the client command line ([doc/en-US/20-cli.md](../doc/en-US/20-cli.md)), on top of the Rust connector | Ready; the Docker image still needs checking on a real Docker, and so does the end-to-end script |
| [`csharp/`](csharp/) | `Taba.Client`: the nine operations as `...Async` and synchronous, `subscribe`, call retries, its own JSON, no dependencies; targets `net461`, `net5.0`, `net6.0`, `net8.0` and `net10.0` | Ready (unit tests, integration against Compose, `scripts/integration.sh`, and the contract tests; [console example](../examples/csharp-console/)); on [nuget.org](https://www.nuget.org/packages/Taba.Client) |
| [`java/`](java/) | `taba-client` for Java 11+: the nine operations (blocking and `CompletableFuture`), `subscribe`, call retries, its own JSON, no dependencies | Ready (unit tests, `mvn test`, and the contract tests); not published yet |
| [`python/`](python/) | `taba-client` for Python 3.9+: `taba.Client` and the `asyncio` `taba.aio.AsyncClient`, its own JSON, no dependencies | Ready (unit tests and the contract tests); not published yet |
| [`node/`](node/) | `taba-client` for Node.js 18+: the nine operations as `Promise`, `subscribe`, `AbortSignal`, types included, no dependencies | Ready (unit tests, `npm test`, and the contract tests); not published yet |
| [`omb/`](omb/) | The **OpenMessaging Benchmark driver** for Taba, on the Java connector (Java 11): `omb/build.sh`, `omb/test.sh` | Ready (unit and integration tests); a tool, not published |
| [`contract/`](contract/) | The **contract tests**: one scenario run against a real Taba by a driver of each connector above, and then the connectors reading each other's messages. `contract/run.sh` | Ready |
| [`go/`](go/) | `taba` for Go 1.7+: the nine operations with `context.Context`, `Subscribe`, consuming with a handler (automatic renewal of the invisibility), its own JSON reading, standard library only | Ready (unit tests, `go test`, and the contract tests); not published yet |
| [`php/`](php/) | `taba/client` for PHP 5.6+: the nine operations, `subscribe`, consuming with a handler, `curl`, no dependencies | Ready (unit tests, `php tests/run.php`, and the contract tests); not published yet. One thread: the invisibility is not renewed while a handler runs |
| [`ruby/`](ruby/) | `taba-client` for Ruby 2.1+: the nine operations, blocks, `subscribe`, consuming with a handler (automatic renewal), `Net::HTTP`, no dependencies | Ready (unit tests, minitest, and the contract tests); not published yet |
| `c/`, `cpp/` | C and C++ ([design kept](../doc/en-US/21-connectors.md)) | Deferred, no date: the folders do not exist |

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
