# taba-cli

The Taba client command line ([documentation](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/20-cli.md#taba-cli-client)): topics, publish, consume, `subscribe`, ack, invisibility, tokens, health, metrics and drain, plus the **connection profiles** (with an interactive screen: `taba-cli profile`).

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/20-cli.md#taba-cli-cliente) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

```
taba-cli profile add local --url http://127.0.0.1:8080 --token-file ./secrets/root-token
taba-cli topic create orders --type queue --fifo
echo '{"order": 9001}' | taba-cli publish orders --stdin --group-id customer-42
taba-cli consume orders --wait 20 --ack
taba-cli health cluster -o table
```

Without a profile, `http://127.0.0.1:8080` applies, with no token. `taba-cli help` lists the commands.

## Build and test

```
cd connectors/rust
cargo test -p taba-cli
cli/scripts/build.sh                  # Linux and macOS: this machine's binary in dist/<target>/
cli/scripts/build.ps1                 # Windows
cli/scripts/docker.sh                 # the Docker image (Alpine)
```

`build.sh --all` and `build.ps1 -All` build every target the machine reaches ([targets](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/20-cli.md#docker-image-and-binaries)). The image uses the repository root as context: `docker buildx build -f connectors/rust/cli/Dockerfile -t taba-cli .`.

## License

[Apache-2.0](https://github.com/rafaelpontani/taba/blob/main/connectors/LICENSE). The Taba server is AGPL-3.0-only; the connectors are Apache-2.0 so any application, open or closed, can embed them.
