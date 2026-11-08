---
title: Gateway for the AWS SDKs
summary: "The SQS and SNS gateways inside the node: an application that uses the AWS SDK points its endpoint at a Taba port and keeps its code. What is supported (standard and FIFO queues, JSON and Query protocols, SNS topics that publish to queues, SigV4), how a call becomes a call to the client API, how the credentials file and the permissions work, where the message attributes live, how a topic and a subscription are stored, and every difference from SQS and SNS that the application must know."
type: guide
status: partial
verified-on: 2026-11-04 @ a3bdab9
sources: [crates/taba-node/src/gateway/mod.rs, crates/taba-node/src/gateway/backend/mod.rs, crates/taba-node/src/gateway/envelope/mod.rs, crates/taba-node/src/gateway/naming/mod.rs, crates/taba-node/src/gateway/aws/sigv4/mod.rs, crates/taba-node/src/gateway/aws/credentials/mod.rs, crates/taba-node/src/gateway/aws/errors/mod.rs, crates/taba-node/src/gateway/sqs/mod.rs, crates/taba-node/src/gateway/sqs/queues/mod.rs, crates/taba-node/src/gateway/sqs/messages/mod.rs, crates/taba-node/src/gateway/dispatch/mod.rs, crates/taba-node/src/gateway/sns/mod.rs, crates/taba-node/src/gateway/sns/protocol/mod.rs, crates/taba-node/src/gateway/sns/topics/mod.rs, crates/taba-node/src/gateway/sns/publish/mod.rs, crates/taba-node/src/gateway/sns/subscriptions/mod.rs, crates/taba-node/src/gateway/aws/auth/mod.rs, crates/taba-node/tests/gateway_sqs.rs, crates/taba-node/tests/gateway_sns.rs, connectors/gateway-contract/README.md]
---

# Gateway for the AWS SDKs

> Read first: [16-client-api.md](16-client-api.md) and [14-security.md](14-security.md).

An application that already sends to and reads from Amazon SQS, or publishes to Amazon SNS topics that feed SQS queues, can use Taba **without changing its code**: it points the endpoint of the SDK (`AWS_ENDPOINT_URL_SQS` and `AWS_ENDPOINT_URL_SNS`, or the endpoint parameter of the client) at the gateway of a Taba node. The gateway is part of the node and of its image; it is **off by default** and SQS and SNS are served on **one port** (`gateway.aws.port`, 9324), each turned on by its own key ([22-configuration.md](22-configuration.md#gateway-for-the-aws-sdks)).

**Status.** Today the gateway serves **standard and FIFO SQS queues** and **SNS topics that publish to SQS queues**. RabbitMQ (AMQP) is planned.

## How a call is served

```
AWS SDK (SQS) ──JSON or Query──┐
AWS SDK (SNS) ──Query───────────┴▶ gateway.aws.port ──▶ gateway/dispatch ──▶ gateway/sqs ┐
                                                                                   └─▶ gateway/sns ┴─▶ client API (in the process) ──▶ node engine
```

1. The port hands the request to SQS or to SNS by the **service in the credential scope** of the signature (`.../us-east-1/sns/aws4_request`), or by the `X-Amz-Target: AmazonSQS.<Action>` header, or, for an unsigned call (only with the Taba authentication off), by the `Action` of the form. A protocol that is off answers `404` with the key that turns it on. Then the protocol reads the request, checks the signature (below) and decodes it. In SQS the **JSON** protocol (`X-Amz-Target: AmazonSQS.<Action>`, current SDKs) and the **Query** protocol (a form, XML answers, older SDKs) are both served. SNS only has the Query protocol.
2. Each SQS action becomes one or a few calls to the client API, made **in the process** against the same router of `api.port`, with the token of the access key. So authentication, permissions and prefixes of the token, rate limit, drain (a draining node forwards the call to another node, or answers `ServiceUnavailable`), metrics and replication are the ones of every other client. The rate limit still counts by the real client IP (the connection address and the `X-Forwarded-For` of the proxy are passed on).
3. The answer is written in the protocol the client used, with the error codes of AWS.

The gateway keeps **no state**: every node sees the same queues and topics, because they are Taba queues and distributors.

## What is supported

| Area | Supported |
|---|---|
| Queues | `CreateQueue`, `DeleteQueue`, `GetQueueUrl`, `ListQueues` (prefix, `MaxResults`, `NextToken`), `PurgeQueue` |
| Attributes and tags | `GetQueueAttributes`, `SetQueueAttributes`, `TagQueue`, `UntagQueue`, `ListQueueTags` |
| Messages | `SendMessage`, `SendMessageBatch`, `ReceiveMessage` (long polling, up to 10), `DeleteMessage`, `DeleteMessageBatch`, `ChangeMessageVisibility`, `ChangeMessageVisibilityBatch` |
| Message features | `MessageAttributes` (String, Number, Binary) with `MD5OfMessageAttributes`, `MD5OfMessageBody`, `DelaySeconds` per message and per queue (standard queues), `ApproximateReceiveCount`, `SentTimestamp`, `SenderId` |
| FIFO queues | `.fifo` names with `FifoQueue=true`, `MessageGroupId`, `MessageDeduplicationId`, `ContentBasedDeduplication`, `SequenceNumber`, `MessageGroupId` and `MessageDeduplicationId` back on receive, a dead-letter queue named in the `RedrivePolicy` |
| Not supported (clear error) | `AddPermission`, `RemovePermission`, the message move tasks, `DelaySeconds` on a FIFO queue, a dead-letter queue shared by several source queues |
| SNS topics | `CreateTopic` (again with the same name answers the same ARN), `DeleteTopic`, `ListTopics`, `GetTopicAttributes`, `SetTopicAttributes`, `TagResource`, `UntagResource`, `ListTagsForResource` |
| SNS subscriptions | `Subscribe` with protocol `sqs`, `Unsubscribe`, `ListSubscriptions`, `ListSubscriptionsByTopic`, `GetSubscriptionAttributes`, `SetSubscriptionAttributes` |
| SNS publishing | `Publish`, `PublishBatch` (up to 10), `Subject`, `MessageAttributes` (String, Number, Binary), `MessageStructure=json` |
| SNS not supported (clear error) | `RawMessageDelivery=true`, `FilterPolicy` and `FilterPolicyScope`, `DeliveryPolicy` and the `RedrivePolicy` of a subscription, FIFO topics (`.fifo`, `FifoTopic`), the protocols `http`, `https`, `email`, `email-json`, `sms`, `lambda`, `application` and `firehose`, SMS and mobile push (`PhoneNumber`, `TargetArn`, platform applications), `ConfirmSubscription`, `AddPermission`, `RemovePermission`, the data protection policies |
| Accepted and ignored | The queue `Policy` (an SNS subscription always writes it), `SqsManagedSseEnabled`, `KmsMasterKeyId`, `KmsDataKeyReusePeriodSeconds`, `DeduplicationScope`, `FifoThroughputLimit`, `ReceiveRequestAttemptId`; `ListDeadLetterSourceQueues` answers an empty list; the `DisplayName` and the `Policy` of an SNS topic |

## The access keys

SigV4 never sends the secret, so the node cannot check a signature with the hash that a Taba token keeps. The gateway reads a **credentials file** (`gateway.aws.credentials-file`) that maps each `accessKeyId` to its `secret` and to the Taba `token` whose permissions and prefixes apply:

```json
{ "AKIAEXAMPLE": { "secret": "wJalrXUtnFEMI...", "token": "tb_1a2b3c.xxxxxxxxxxxx" } }
```

- The file is mounted as a secret on every node and read again every `auth.reload-interval`. A file that is invalid when the node starts stops the start; one that becomes invalid later is ignored and the previous keys keep applying.
- The signature must be fresh (15 minutes either way), must cover the body, and must name the service it is for (`sqs` or `sns`, which is also what routes it; the same file serves both). A wrong secret is `SignatureDoesNotMatch`, an unknown key `InvalidClientTokenId`, a missing signature `MissingAuthenticationToken`, an old one `RequestExpired`.
- The permissions of the token decide what the key may do (`create` to create queues, `publish` to send, `consume` to receive, delete and hide, `manage` to delete and purge); a refusal is `AccessDenied` (`AuthorizationError` in SNS). A token with topic prefixes confines the key to those queues, and `ListQueues` shows only them. In SNS the same permissions apply to the topic: `create` to create it, `manage` to delete it, subscribe and unsubscribe, `publish` to publish; a subscription also needs the prefix of the queue to be inside the prefixes of the token, and `ListTopics` shows only the topics the token reaches.
- **With the Taba authentication off** no file is needed, any credential is accepted and no signature is checked (the node warns at the start).
- **With the authentication on and no keys** every call is refused (the node warns at the start).
- Temporary credentials of an IAM role cannot be checked without AWS: the application needs static keys.

**The Compose example.** `docker compose up -d` starts the cluster with both gateways **on** (port `9324` on HAProxy, for SQS and SNS) and a demo access key: the `init` service writes `aws-credentials.json` to the `secrets` volume with the key `taba` / secret `taba-secret`, which acts with the **root token**. It is only for trying Taba; for anything real replace the file with the keys of your applications, each mapped to a token of its own. `TABA_GATEWAY_SQS_ENABLED=false` and `TABA_GATEWAY_SNS_ENABLED=false` turn them off. The node default does not change: off.

## How a queue is stored

A queue is a Taba queue with the **same name**, so a native client reaches the same messages by the same name. A **FIFO** queue is the Taba FIFO queue (`fifo: true`) of the name **without** `.fifo`: `orders.fifo` is the topic `orders`, because a topic name cannot hold a dot. The name says the kind, so a standard `orders` and a FIFO `orders.fifo` cannot exist together (the second `CreateQueue` is `QueueNameExists`) and each name only finds the queue of its own kind.

| SQS | In Taba |
|---|---|
| `MessageRetentionPeriod` | `ttl` of the queue |
| `MaximumMessageSize` | `maxMessageBytes` |
| `RedrivePolicy` (`maxReceiveCount`) | `maxDeliveries`; the dead letters are kept by Taba in `<topic>-dead-letter`, and the queue named in `deadLetterTargetArn` is an alias of it (below) |
| `FifoQueue` | `fifo` of the queue, from the `.fifo` name; it cannot be changed |
| `ContentBasedDeduplication` | The tag `sqs.cbd` |
| `VisibilityTimeout`, `DelaySeconds`, `ReceiveMessageWaitTimeSeconds` | Tags `sqs.vt`, `sqs.delay`, `sqs.wait`, which the tag actions never show |
| Queue tags | Tags of the queue (see the limits below) |
| `ApproximateNumberOfMessages*` | `GET /stats/topics/{queue}` |
| `MessageGroupId`, `MessageDeduplicationId` | `groupId`, `deduplicationId` of the message |
| `SequenceNumber` | The message `id` (a UUID that grows inside each group) as a decimal number |
| `QueueUrl` | `<gateway.sqs.public-url or Host>/000000000000/<name>` |
| `QueueArn` | `arn:aws:sqs:<region of the signature>:000000000000:<name>` |

**The dead-letter queue.** Taba keeps the dead letters of `orders` in `orders-dead-letter`, one per source queue, but an SQS application creates its own dead-letter queue and names it in the `RedrivePolicy`. The gateway makes that queue an **alias**: it marks it (tag `sqs.dlqof`) as the dead-letter queue of `orders`, and every action on it (receive, delete, counts, purge) goes to `orders-dead-letter`; `orders` keeps the name in the tag `sqs.dlq` to show it back in its `RedrivePolicy`. The queue must exist, be of the same kind as the source (FIFO with FIFO) and not be the dead-letter queue of another source (`UnsupportedOperation`). Deleting the source frees it. The messages of the alias queue are not FIFO in Taba, so the order of a FIFO queue is lost in its dead-letter queue.

**The group and the deduplication id.** SQS accepts more punctuation than Taba in `MessageGroupId` and `MessageDeduplicationId`. One made of `A-Za-z0-9_-.:` goes as it is; another goes as `h.` and 40 hexadecimal digits of its SHA-256, and the message is stored as an envelope that keeps the original, which the receive gives back. With `ContentBasedDeduplication` and no `MessageDeduplicationId`, the deduplication id is the SHA-256 of the body.

The defaults of a queue (visibility, delay, wait) are remembered by the gateway for 2 seconds: a change through the gateway is seen at once, one through the native API or another node within that time.

**The message.** The body of a message sent without attributes is stored **as it is**, as `text/plain`: a native consumer reads exactly what the application sent, and what a native client publishes is received through SQS. With attributes the message is stored as JSON, `{"tabaSqs":1,"body":"...","attributes":{"name":{"type":"String","value":"..."}}}`, which a native consumer can read and recognise by `tabaSqs`. The server and the formats on disk do not change. The `ReceiptHandle` is `base64url("<id>:<receiptToken>")`, so deleting or hiding a message needs no state.

## SNS: topics and subscriptions

SNS delivers a publication to every queue subscribed to the topic. Taba already has that: the **distributor**, a topic that holds no messages and copies each publication to the queues whose name starts with one of its **prefixes**, on every node. So the gateway does no fan-out and keeps no state:

| SNS | In Taba |
|---|---|
| Topic | A distributor with the same name, made by `CreateTopic`. The name is a Taba topic name: letters, digits, `-` and `_`, up to 100 characters, not ending in `-dead-letter` |
| `TopicArn` | `arn:aws:sns:<region of the signature>:000000000000:<name>` |
| Subscription of a queue | A prefix in the list of the distributor: the name of the queue (without `.fifo`) |
| `SubscriptionArn` | The `TopicArn` and the name of the queue: `arn:aws:sns:<region>:000000000000:<topic>:<queue>`. It is made from the two names, so it needs no state, and it is always confirmed (never pending) |
| `Endpoint` of the subscription | The ARN of the queue (`arn:aws:sqs:...:<queue>`). The queue must exist, be an SQS queue of this gateway and, for `.fifo`, be FIFO |
| Topic tags | Tags of the distributor (same limits as the queue tags) |
| `Publish` | One publication to the distributor; the node makes the copies |
| `GetTopicAttributes` | `TopicArn`, `Owner`, `DisplayName`, `SubscriptionsConfirmed` (the number of prefixes), `SubscriptionsPending` and `SubscriptionsDeleted` (0) |

`Subscribe` is idempotent (the same queue gives the same ARN), a subscription counts only for the publications **after** it (the distributor applies a change from its commit), a topic holds up to **20** subscriptions (`SubscriptionLimitExceeded`; a prefix common to the names of several queues covers all of them), and `DeleteTopic` and `Unsubscribe` are idempotent. A topic is also a distributor for the native API: a prefix added with `PATCH /topics/{topic}` is a subscription for SNS, and what a native client publishes to it reaches the queues without the SNS envelope.

**The notification.** The queue receives what SNS delivers when `RawMessageDelivery` is off, as the text of a JSON document stored as `text/plain`:

```json
{"Type":"Notification","MessageId":"...","TopicArn":"arn:aws:sns:us-east-1:000000000000:orders","Subject":"...","Message":"...","Timestamp":"2026-11-04T10:00:00.000Z","MessageAttributes":{"color":{"Type":"String","Value":"blue"}}}
```

`Subject` and `MessageAttributes` appear only when the publication has them. The `MessageId` is made by the gateway before the publication, because the copies are different Taba messages and the publisher needs the one id that is in all of them. The fields of the signature (`Signature`, `SigningCertURL`, `UnsubscribeURL`) are **not** there: there is no certificate of AWS to sign with, so an application that verifies the signature of a notification fails. `MessageStructure=json` is read: the queue gets the `sqs` text of the message, or the `default` (the key `default` is required, as in SNS).

**What is refused, never accepted and ignored.** The distributor copies and does nothing else, so what would need more is an error that names it: `RawMessageDelivery=true` (the queue always gets the notification, with the message in its `Message` field; an application that reads the raw body has to read that field to migrate), `FilterPolicy` (accepting it would deliver everything), any protocol but `sqs`, FIFO topics (the distributor is not FIFO), `MessageGroupId` and `MessageDeduplicationId` on a publication, and the attributes of a topic or subscription not listed above. `DisplayName` and `Policy` of a topic are accepted and have no effect (SMS and e-mail are not delivered, and Taba applies no policy; the SQS side does the same with the policy of a queue).

**A failed copy.** If the node refuses the publication (more destinations than `topics.distributor.max-targets`) nothing is copied and the call fails. If only some copies fail the call answers `InternalError` (a receiver fault, so the SDK repeats it) and the queues that had the copy get a duplicate, which "at least once" allows. `PublishBatch` publishes each entry on its own (up to 10, one publication each) and answers which ones failed.

## Differences from SQS

| Subject | SQS | Taba gateway |
|---|---|---|
| Queue names | Up to 80 characters, `.fifo` for FIFO | The same; a name ending in `-dead-letter` (before `.fifo`) is refused (it belongs to Taba); `orders` and `orders.fifo` are the same topic, so only one of them exists |
| Tags | Free keys and values | Keys of 1 to 64 and values of up to 128 characters of `A-Za-z0-9_-.:`; up to 20 tags per queue, three of them used by the gateway when the queue sets the three attributes above; the prefix `sqs.` is reserved |
| Delayed messages in the counts | `ApproximateNumberOfMessagesDelayed` | They count as not visible (PRB-093); `Delayed` is always 0 |
| `MessageRetentionPeriod` default | 4 days | The node default (14 days) while the queue does not set it |
| Without a `RedrivePolicy` | No limit of receives | `maxDeliveries` of the node (10 by default); after it the message goes to `<name>-dead-letter` |
| `ReceiveMessage` of the same message | `ApproximateFirstReceiveTimestamp` | Not returned |
| `ReceiveRequestAttemptId`, `MessageGroupId` and `MessageDeduplicationId` on a standard queue | Used on FIFO | Ignored |
| Deduplication on a FIFO queue | The id counts in the whole queue (by default) for 5 minutes | The id counts **inside the group** for `fifo.dedup.window` (5 minutes by default): the same id in two groups is two messages |
| Receiving from a FIFO queue | Several messages of the same group in one call | One message per group in flight: the next of a group is delivered after the delete of the previous. `MaxNumberOfMessages` gives messages of different groups |
| `DelaySeconds` on a FIFO queue | Allowed on the queue | Taba has no delay on a FIFO queue: a message with `DelaySeconds` is `InvalidParameterValue` and the queue attribute must be 0 (`UnsupportedOperation`) |
| A FIFO send without a deduplication id | `InvalidParameterValue` unless the queue has `ContentBasedDeduplication` | The same; without a `MessageGroupId`, `MissingParameter` |
| Dead-letter queue | Any queue of the same kind, shared by many sources | One per source queue, created by you and aliased (above) |
| Errors | `QueueDoesNotExist` with `AWS.SimpleQueueService.NonExistentQueue` | Both names, as AWS: the JSON protocol sends the first in `__type` and the second in `x-amzn-query-error`; the Query protocol sends the second |
| Rate limit | Per account | The `ratelimit.*` of the node, per key; `ThrottlingException` / `RequestThrottled` |
| Node in drain | n/a | The call is forwarded to another node; with none, `ServiceUnavailable` with `Retry-After` (a receiver fault, so the SDK retries) |

### Differences from SNS

| Subject | SNS | Taba gateway |
|---|---|---|
| Queues a subscription reaches | Exactly the queue subscribed | Every queue whose name **starts with** the name subscribed (PRB-098): subscribing `orders` also copies to `orders-eu`. Name the queues so that no name starts with another |
| Names | A topic and a queue may share a name | They share the Taba name space: a topic `orders` and a queue `orders` cannot both exist (PRB-098); `CreateTopic` on a queue name is `InvalidParameter` |
| Subscriptions per topic | Millions | 20 (`SubscriptionLimitExceeded`) |
| Subscribers | HTTP, e-mail, SMS, Lambda, SQS, ... | Only SQS queues of this gateway, which must exist when subscribing |
| Delivery to a queue | The notification, or the raw message with `RawMessageDelivery` | Always the notification (`RawMessageDelivery=true` is refused) |
| Signature of the notification | Signed by AWS | Not signed |
| `FilterPolicy` | Filters by subscription | Refused |
| FIFO topics | `.fifo` | Refused (PRB-100) |
| Concurrent changes of the subscriptions | Independent | Two `Subscribe` or `Unsubscribe` of the same topic at the same instant, on different nodes, can lose one (PRB-099) |
| `PublishBatch` | One request | One publication per entry, so each entry counts for the rate limit |
| Message size | 256 KiB | The same; the sum of a batch too |
| Errors | `NotFound`, `InvalidParameter`, `AuthorizationError`, `Throttled` | The same codes, in the XML of the Query protocol |

## Tests

The native AWS SDKs of six languages (C#, Java v1 and v2, Rust, Go, Python, Node.js; a current and an older Query-protocol version of each) run the same scenario files against the gateway, and against emulators to prove the scenarios right: [connectors/gateway-contract/README.md](../../connectors/gateway-contract/README.md). The node's own tests are in `crates/taba-node/src/gateway/**/tests.rs` (units) and `crates/taba-node/tests/gateway_sqs.rs` and `crates/taba-node/tests/gateway_sns.rs` (a real node, requests written by hand).

## Sources and evidence

- `crates/taba-node/src/gateway/` (the module map is in [04-architecture.md](04-architecture.md)), `config/params/mod.rs` (the keys).
- Use cases `UC-GTW-001` to `UC-GTW-030` in [26a](26a-implemented-use-cases.md); tests in `crates/taba-node/src/gateway/**/tests.rs`, `crates/taba-node/tests/gateway_sqs.rs` and `crates/taba-node/tests/gateway_sns.rs`.
- The contract suite of the native SDKs: `connectors/gateway-contract/`.
