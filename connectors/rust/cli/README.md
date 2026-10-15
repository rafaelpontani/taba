# taba-cli

The Taba client command line ([doc/pt-BR/20-cli.md](../../../doc/pt-BR/20-cli.md)): topics, publish, consume, `subscribe`, ack, invisibility, tokens, health, metrics and drain, plus the **connection profiles** (with an interactive screen: `taba-cli profile`).

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

`build.sh --all` and `build.ps1 -All` build every target the machine reaches (section 18.11.8 of the doc). The image uses the repository root as context: `docker buildx build -f connectors/rust/cli/Dockerfile -t taba-cli .`.
