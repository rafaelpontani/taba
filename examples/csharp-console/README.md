# Console example (C#)

Uses the [`Taba.Client`](../../connectors/csharp/) connector: creates a queue, publishes three orders, consumes with long polling, acknowledges each message and deletes the queue.

With the cluster from the root `docker-compose.yml` up (`docker compose up -d`). On Windows Git Bash, prefix the `docker compose run` with `MSYS_NO_PATHCONV=1`, or it converts the `/secrets/root-token` path:

```
export TABA_TOKEN="$(docker compose run --rm -T --entrypoint cat taba-cli /secrets/root-token)"
dotnet run --project examples/csharp-console
```

`TABA_URL` (default `http://localhost:8080`) and `TABA_TOKEN` are the only settings; the first argument changes the queue name. Needs the .NET 8 SDK.
