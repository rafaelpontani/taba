# Exemplo de console (C#)

Usa o conector [`Taba.Client`](../../connectors/csharp/): cria uma fila, publica três pedidos, consome com long polling, confirma cada mensagem e apaga a fila.

Com o cluster do `docker-compose.yml` da raiz de pé (`docker compose up --build -d`). No Git Bash do Windows, prefixe o `docker compose run` com `MSYS_NO_PATHCONV=1`, ou ele converte o caminho `/secrets/root-token`:

```
export TABA_TOKEN="$(docker compose run --rm -T --entrypoint cat taba-cli /secrets/root-token)"
dotnet run --project exemplos/csharp-console
```

`TABA_URL` (padrão `http://localhost:8080`) e `TABA_TOKEN` são as únicas configurações; o primeiro argumento muda o nome da fila. Precisa do SDK do .NET 8.
