# taba-cli

A linha de comando do cliente do Taba ([doc/19b-cli.md](../../../doc/19b-cli.md)): tópicos, publicar, consumir, `subscribe`, ack, invisibilidade, tokens, saúde, métricas e drain, mais os **perfis de conexão** (com uma tela interativa: `taba-cli profile`).

```
taba-cli profile add local --url http://127.0.0.1:8080 --token-file ./secrets/root-token
taba-cli topic create pedidos --type queue --fifo
echo '{"pedido": 9001}' | taba-cli publish pedidos --stdin --group-id cliente-42
taba-cli consume pedidos --wait 20 --ack
taba-cli health cluster -o table
```

Sem perfil vale `http://127.0.0.1:8080`, sem token. `taba-cli help` lista os comandos.

## Construir e testar

```
cd connectors/rust
cargo test -p taba-cli
cli/scripts/build.sh                  # Linux e macOS: o binário desta máquina em dist/<alvo>/
cli/scripts/build.ps1                 # Windows
cli/scripts/docker.sh                 # a imagem Docker (Alpine)
```

`build.sh --all` e `build.ps1 -All` constroem todos os alvos que a máquina alcança (seção 18.11.8 do doc). A imagem usa a raiz do repositório como contexto: `docker buildx build -f connectors/rust/cli/Dockerfile -t taba-cli .`.
