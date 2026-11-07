# taba-client (Ruby)

The Ruby client of [Taba](https://github.com/rafaelpontani/taba): the nine operations of the client API, `subscribe`, consuming with a handler (with automatic renewal of the invisibility) and the call retries of the common contract. **No dependencies** (only the standard library: `net/http`, `json`).

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/21-conectores.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

- **Ruby 2.1 or later** (it runs the same code on 2.1 and on 3.x).
- `Taba::Client` is safe for concurrent use: each thread has its own connection.

```ruby
require 'taba'

client = Taba::Client.new('http://localhost:8080', token: ENV['TABA_TOKEN'])

client.create_topic(name: 'orders', type: 'queue')
client.publish_json('orders', order: 9001)

# Consume with a block: the message is deleted if the block does not raise.
client.consume_with('orders', wait_time: 20) do |context|
  puts context.message.body_text
end

# Or the whole loop, until the token is cancelled.
# token = Taba::CancelToken.new
# client.subscribe('orders', cancel: token) { |message| puts message.json }
```

## Installing (not in a registry yet)

Not in a registry yet (the identifiers are still open). Until then, each [Release](https://github.com/rafaelpontani/taba/releases) has the installable file, with its checksum in `SHA256SUMS` and its provenance (`gh attestation verify <file> -R rafaelpontani/taba`). `<release>` below is the version of the Release; the version in the file name is the connector's own.

```
curl -LO https://github.com/rafaelpontani/taba/releases/download/v<release>/taba-client-0.1.0.gem
gem install ./taba-client-0.1.0.gem
```

With Bundler, straight from the source (a branch, a tag or a commit):

```ruby
gem 'taba-client', git: 'https://github.com/rafaelpontani/taba.git', branch: 'release',
                   glob: 'connectors/ruby/*.gemspec'
```

## Declaring a topic

`create_or_update_topic(request, recreate: nil)` (`PUT /topics/{topic}`; the fields can also go as options) creates the topic or brings it to the declared state, and returns a `Taba::TopicPut` with `topic` and `outcome` (`created`, `updated` or `recreated`). What is left out goes back to the server default (a full replacement, unlike the update); another type, `fifo` or `volatile` is the error `409 topic.immutable-mismatch` unless `recreate` is `if-empty` (only when the queue and its dead-letter are empty, `409 topic.not-empty` otherwise) or `always` (drops the messages; new `id`). After a `503` the call is repeated like the others. `delete_topic(name, if_empty: true)` only removes a queue with no message. `create_topic` does not change (`409` when it exists). Details: [Create or update a topic](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#create-or-update-a-topic-put-topicstopic).

## Batches

Many messages in one request, next to the single calls (optional): `publish_batch(topic, [Taba::BatchMessage.new(body, content_type: ...)])`, `ack_batch(topic, messages)` and `change_visibility_batch(topic, messages, seconds)` return one `BatchResult` per item (`ok?`, `status`, `id`, `error_code`). Each item is answered as its single call, in order, and an item that failed comes back with its `status` and error code instead of raising. The bodies go in base64, so any type fits. Details: [Messages in batch](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/16-client-api.md#messages-in-batch-post-messagesbatch-ack-and-visibility).

## Configuration

`Taba::Client.new(config)` with a `Taba::Config`, or `Taba::Client.new(base_url, **options)` with the same fields. The client does not read environment variables: `TABA_*` belongs to the server.

| Field | Default | |
|---|---|---|
| `base_url` | required | `http://host:port` or `https://...` |
| `token` | none | `Authorization: Bearer` |
| `connect_timeout` | `5.0` | seconds |
| `request_timeout` | `30.0` | seconds; `consume` uses `wait_time + 10` |
| `max_attempts` | `3` | counting the first |
| `user_agent` | `taba-ruby/<version>` | |
| `ca_file`, `ca_path` | system's | an own CA bundle (a private CA, for example); TLS 1.2 or later, the certificate is always verified |

## What differs from the other connectors

- Names are `snake_case` and predicates end in `?` (`ok?`, `lease_lost?`); options are keyword arguments (`client.consume('q', max_messages: 5, wait_time: 20)`; an unknown one is a `client.invalid-request` error). Handlers are blocks. `ack` and `change_visibility` take the `Message` you received, or its id plus the receipt token.
- A `TopicUpdate` field left out is not sent; `nil` sends `null` (back to the server default) for `max_message_bytes`, `ttl` and `max_deliveries`: `client.update_topic('q', ttl: nil)`.
- Errors are a `Taba::Error` with `status`, `code`, `reason` (the message of the `ErrorResponse`; `message` and `to_s` also carry the code and the request id), `request_id`, `retry_after`, `details`, `body`. Test the `code`, never the text.
- Cancelling: pass `cancel: token` (a `Taba::CancelToken`) and call `token.cancel` from another thread; the call in flight ends at once with `Taba::Cancelled` (not a `Taba::Error`), so a long polling does not wait for its timeout. `subscribe` returns without an exception.
- `each_topic` walks all the pages (an `Enumerator` without a block); `list_all_topics` returns the array. `consume_json` hands over the body already parsed in `context.body`.
- `request_timeout` limits each wait for the response (Ruby's `Net::HTTP` has no total time limit), and `Net::HTTP`'s own automatic retry is switched off, so only the retries of the contract happen.

## Tests

```
cd connectors/ruby
ruby -Ilib -Itest -e 'Dir["test/*_test.rb"].each { |f| require File.expand_path(f) }'
```

The tests start a local HTTP server that records every request and answers from a script (the equivalent of the C# `ScriptedHandler`). The **contract tests** run the same scenario against a real Taba (one node in Docker), on Ruby 2.1: `contract/run.sh` (`../contract/run.sh` for all the connectors; [how it works](https://github.com/rafaelpontani/taba/blob/main/connectors/contract/README.md)).
