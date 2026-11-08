# taba (Go)

The Go client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API, `Subscribe`, consuming with a handler (with automatic renewal of the invisibility) and the call retries of the common contract. **No dependencies** (only the standard library).

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Go 1.7 or later** (it builds and tests on 1.7 and on current versions).
- `taba.Client` is safe for concurrent use. Every call takes a `context.Context`: cancelling it ends the call in flight, so a long polling does not wait for its timeout.

```
go get github.com/rafaelpontani/taba/connectors/go
```

```go
import taba "github.com/rafaelpontani/taba/connectors/go"

client, err := taba.NewClient(taba.Config{BaseURL: "http://localhost:8080", Token: token})
if err != nil {
	return err
}
defer client.Close()
ctx := context.Background()

client.CreateTopic(ctx, &taba.TopicRequest{Name: "orders", Type: "queue"})
client.PublishJSON(ctx, "orders", map[string]int{"order": 9001})

// Consume with a handler: the message is deleted if the handler returns nil.
_, err = client.ConsumeWith(ctx, "orders", &taba.ConsumeOptions{WaitTime: taba.Int(20)},
	func(c *taba.MessageContext) error {
		fmt.Println(c.Message.BodyText())
		return nil
	})

// Or the whole loop, until the context is cancelled (Subscribe then returns nil).
// client.Subscribe(ctx, "orders", nil, func(m *taba.Message) error { ... }, nil)
```

## Installing

Go reads the module straight from the repository, so there is nothing else to download: `go get github.com/rafaelpontani/taba/connectors/go@release` (a branch, or a commit; with Go 1.7, which has no modules, the plain `go get` above). A fixed version needs a tag `connectors/go/vX.Y.Z`, which is not created yet because the connector is not published (the identifiers are still open).

## Declaring a topic

`CreateOrUpdateTopic(ctx, request, recreate)` (`PUT /topics/{topic}`) creates the topic or brings it to the declared state, and returns a `TopicPut` with the `Topic` and the `Outcome` (`taba.TopicCreated`, `TopicUpdated` or `TopicRecreated`); `recreate` is `taba.RecreateNever`, `RecreateIfEmpty` or `RecreateAlways`. What is left out goes back to the server default (a full replacement, unlike the update); another type, `fifo` or `volatile` is the error `409 topic.immutable-mismatch` unless `recreate` is `if-empty` (only when the queue and its dead-letter are empty, `409 topic.not-empty` otherwise) or `always` (drops the messages; new `id`). After a `503` the call is repeated like the others. `DeleteTopicIfEmpty(ctx, name)` only removes a queue with no message. `CreateTopic` does not change (`409` when it exists). Details: [Create or update a topic](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#create-or-update-a-topic-put-topicstopic).

## Batches

Many messages in one request, next to the single calls (optional): `PublishBatch(ctx, topic, []taba.BatchMessage{...})`, `AckBatch(ctx, topic, messages)` and `ChangeVisibilityBatch(ctx, topic, messages, seconds)` return one `BatchResult` per item (`OK()`, `Status`, `ID`, `ErrorCode`). Each item is answered as its single call, in order, and an item that failed comes back with its `Status` and error code instead of an `error`. The bodies go in base64, so any type fits. Details: [Messages in batch](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#messages-in-batch-post-messagesbatch-ack-and-visibility).

## Tokens

With authentication on, the root token and the `admin` tokens administer the access tokens (only Go has this so far; the Terraform provider uses it): `CreateToken(ctx, &taba.TokenRequest{Name, Permissions, TopicPrefixes, ExpiresAt})` returns a `TokenCreated` whose `Secret` (`tb_<id>.<secret>`) the server shows **only here**; `GetToken`, `ListTokens`/`ListAllTokens(ctx, name)` (the name contains the text), `UpdateToken(ctx, id, &taba.TokenUpdate{Name, ExpiresAt})` (only the name and the deadline change; `taba.ClearString()` removes the deadline) and `RevokeToken`. An expired token is `404 token.not-found`, like a revoked one. A `503 cluster.quorum-unavailable` on `CreateToken` comes back without a new attempt: the owner of the definition may have created the token and the answer was lost, so look for it by name and revoke it before trying again. Details: [Tokens](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#tokens).

## Configuration

`taba.Config{...}`; a zero field takes the default. The client does not read environment variables: `TABA_*` belongs to the server.

| Field | Default | |
|---|---|---|
| `BaseURL` | required | `http://host:port` or `https://...` |
| `Token` | none | `Authorization: Bearer` |
| `ConnectTimeout` | `5s` | |
| `RequestTimeout` | `30s` | `Consume` uses `WaitTime + 10s` |
| `MaxAttempts` | `3` | counting the first |
| `RetryMaxWait` | none | total of the waits between the attempts of one call: no attempt is made whose wait (the `Retry-After`, or 1, 2, 4… up to 64 s) would pass it; `MaxAttempts` still bounds them |
| `UserAgent` | `taba-go/<version>` | |
| `TLSConfig` | system's roots, TLS 1.2+ | an own `*tls.Config` (a private CA, for example) |
| `HTTPClient` | the connector's own | replaces the HTTP client (then `ConnectTimeout` and `TLSConfig` are yours to set) |

## What differs from the other connectors

- Names are exported Go names (`PublishJSON`, `ConsumeWith`); the first argument is always a `context.Context`; a call returns `(value, error)`.
- Optional fields are pointers (`nil` is "not set", so the server default applies): `taba.Int(20)`, `taba.Bool(true)`, `taba.String("x")`. In a `TopicUpdate`, `taba.SetInt(n)` sends `n`, `taba.ClearInt()` sends `null` (back to the server default) and `nil` leaves the field out.
- `Ack` and `ChangeVisibility` take the `*Message` you received; `AckByID` and `ChangeVisibilityByID` take its id plus the receipt token.
- Errors are a `*taba.Error` with `Status`, `Code`, `Message`, `RequestID`, `RetryAfter`, `Details`, `Body`. Test the `Code`, never the `Message`. A cancelled context is **not** a `*taba.Error`: the call returns `context.Canceled` (or the context's own error), and `Subscribe` returns `nil`.
- `ListAllTopics` returns the list of all the pages. Go 1.7 has no generics, so there is no typed `ConsumeJSON`: read the body with `c.Decode(&v)` (or `message.JSON(&v)`).
- The handler of `ConsumeWith` returns an `error`: `nil` deletes the message (unless `c.Delete = false`), an error keeps it and is returned to the caller. `c.LeaseLost()` becomes `true` if the server refused a renewal.

## Tests

```
cd connectors/go
go test -race .
```

The tests start a local HTTP server that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`). The **contract tests** run the same scenario against a real Taba (one node in Docker), on Go 1.7: `contract/run.sh` (`../contract/run.sh` for all the connectors; [how it works](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md)).
