# Gateway contract tests (SQS and SNS)

The native AWS SDK of six languages, in more than one version of each, against the Taba gateway (`gateway.sqs.*` and
`gateway.sns.*`) and against emulators. The same scenario files run in every language; each language has one small driver
that reads them and calls the SDK the way an application would, with no HTTP built by hand.

This suite is written **before** the gateway (phase 1 of the plan). Its two rules:

1. On **Taba**, before the gateway exists, every line of every driver must **fail because nothing answers**
   (`run.sh --expect-fail`). A driver that does not compile, aborts or skips a line makes the run bad, so a broken driver can
   not pass for a red test.
2. On the **emulators** (`run.sh --target emulator`), every line of `scenarios/aws/` must **pass**, in every language and
   version. That is what shows that the tests are right and that the failure on Taba is Taba's.

After the gateway is implemented, the same command with no flag (`run.sh`) must pass everything on Taba, without changing a
line of `scenarios/aws/`.

```
connectors/gateway-contract/
  run.sh              --expect-fail | --target emulator | (normal run); languages and --variant filter the drivers
  lib.sh              the targets (Taba node, ElasticMQ, moto), running a driver, judging a log
  emulator/           elasticmq.conf (the queue URLs carry the name of the container)
  scenarios/
    aws/              how AWS behaves: runs on Taba and on the emulators
      sqs-standard.scn  sqs-fifo.scn  sns.scn  cross-setup.scn cross-publish.scn cross-drain.scn
    taba/             what only exists or differs in Taba: runs only on Taba
      auth.scn  limits-sqs.scn  limits-fifo.scn  limits-sns.scn  native.scn
  drivers/
    run.sh            the container and the pinned SDK versions of each driver
    <language>/       the driver, `variants` (name and endpoint mode) and `build-and-run.sh` (inside the container)
```

## Drivers and versions

| Language | SDK | Variants (`drivers/run.sh` pins the versions) | Endpoint |
|---|---|---|---|
| C# | `AWSSDK.SQS`, `AWSSDK.SimpleNotificationService` | `current` 4.0.100.22; `old` 3.7.400.186 (Query protocol) | env / code |
| Java | `software.amazon.awssdk` v2, `aws-java-sdk` v1 | `v2` 2.57.8; `v2old` 2.20.162 (Query); `v1` 1.12.580 (Query) | env / code / code |
| Rust | `aws-sdk-sqs`, `aws-sdk-sns` | `current` (the newest of the day, printed in the result) | env |
| Go | `aws-sdk-go-v2` | `current`; `old` sqs v1.26.0 / sns v1.25.0 (Query) | env / code |
| Python | `boto3` | `current` 1.43.123; `old` 1.28.0 (Query) | env / code |
| Node | `@aws-sdk/client-sqs`, `client-sns` | `current` 3.1161.0; `old` 3.445.0 (Query) | env / code |

- **env** means the SDK reads `AWS_ENDPOINT_URL_SQS` and `AWS_ENDPOINT_URL_SNS`: the application code does not change.
  **code** means an older version, in which the endpoint is a parameter of the client (doc 39 will carry this matrix).
- The old versions are the ones before each SDK moved SQS to the JSON protocol, so they exercise the **Query** protocol, which
  the gateway has to speak as well.
- Not covered yet: Rust before its SQS JSON protocol (the 1.x series of `aws-sdk-sqs` that I tried did not build with the
  driver; the 0.x series needs another driver), PHP and Ruby (out of the first suite).

## The scenario language

One operation per line, `op key=value ... [as=name] [=> expectation ...]`. A line that starts with `#` is a comment.

- **Values** are percent-decoded (`%20` is a space). `{p}` is the prefix of the run (it makes the names unique), `{lang}` the
  language of the driver, `{prev}` the language before it in the cross-language ring, and `$name.field` a field of the result
  of an earlier line that ended with `as=name`.
- **Expectations** are separated by spaces: `field=value`, `field~contains`, `field!=value`, `field>=n`, `field<=n`,
  `field=*` (not empty), and `error=A|B` (the call must fail with one of these AWS error codes, compared **exactly**; the
  part of the code before `#` is ignored, because the JSON protocol sends `com.amazonaws.sqs#QueueDoesNotExist`; a driver also
  reads the code behind the name of the modeled exception class, because the old JS SDKs keep only `InvalidParameterException`
  for the wire code `InvalidParameter`). A field that does not exist is the empty string.
- **Fields** of the result: `url`, `arn`, `id`, `md5`, `md5Ok` (the MD5 the service returned matches the body), `count`,
  `body`, `bodies`, `sorted`, `handle`, `elapsedMs`, `attr_<Name>`, `tag_<key>`, `msgattr_<name>`, `msgattrtype_<name>` (a
  binary value is `b64:...`), `json_<field>` (the string fields of a JSON body, for the SNS envelope), `ok`/`failed` (batches).
- **Operations** (SQS): `createQueue`, `getQueueUrl`, `deleteQueue`, `purgeQueue`, `listQueues`, `getQueueAttributes`,
  `setQueueAttributes`, `tagQueue`, `untagQueue`, `listQueueTags`, `send`, `sendBig`, `sendBatch`, `sendBatchTooMany`,
  `receive`, `drainAll`, `deleteMessage`, `deleteAll`, `changeVisibility`, `changeVisibilityAll`, `allowSnsToSend` (the queue
  policy that SNS needs on AWS). (SNS): `createTopic`, `deleteTopic`, `listTopics`, `getTopicAttributes`, `subscribe`,
  `unsubscribe`, `listSubscriptions`, `getSubscriptionAttributes`, `setSubscriptionAttributes`, `publish`, `publishBatch`.
  Driver control: `sleep`, `useCredentials`. A change to an operation goes to **all six drivers**.

## What a driver prints

```
RESULT PASS sqs-standard.scn:12 createQueue
RESULT FAIL sqs-fifo.scn:30 receive :: count=1 but was '0'
RESULT CONNECT sns.scn:9 publish :: unreachable
SUMMARY lang=go variant=current sdk=... pass=100 fail=2 connect=1
```

`CONNECT` is a call that got no answer **while the driver could not open a TCP connection to the endpoint** (checked once, at
the start). Any other problem is `FAIL`. So an SDK error that is not an answer from the service counts as red only when the
gateway is really not there.

`run.sh` judges each log: the number of `RESULT` lines must equal the number of operation lines of its scenario files, a
`SUMMARY` must be there, and every line must be `PASS` (normal run, emulator) or `CONNECT` (`--expect-fail`).

## Targets

- **Taba**: one node (`TABA_IMAGE`, default `taba:local`) with `gateway.sqs.enabled`, `gateway.sns.enabled`, the port
  of the gateway (9324, SQS and SNS), `gateway.sqs.public-url` and `gateway.aws.credentials-file` set through `TABA_GATEWAY_*` variables. Today
  the node only warns about variables it does not know and starts, so the red run and the green run use the same command. The
  credentials file (a JSON with `accessKeyId -> {secret, token}`) holds the keys of the `ok`,
  `consumeonly` and `prefixed` tokens of `scenarios/taba/auth.scn`.
- **Emulators**: ElasticMQ 1.7.1 for the SQS files (it speaks JSON and Query) and moto 5.1.22 for `sns.scn` (SNS and SQS in the
  same server; ElasticMQ has no SNS). LocalStack was rejected: since 2026.03.0 it needs an account and a token.
- A line that an emulator does not do like AWS goes to `scenarios/taba/` with the reason written next to it, so that a line in
  `scenarios/aws/` is always something that passes there. A difference of Taba that is known and accepted goes to
  `scenarios/taba/limits-*.scn`, never loosens a line of `scenarios/aws/`.

## Native consumer

`scenarios/taba/native.scn` makes the SDK send, and `run.sh` then reads the queue with the Taba API (`GET
/topics/{queue}/messages`), as an application that does not use the SDK would. On the red run the queue must not exist.

## Cross-language

`cross-setup`, `cross-publish` and `cross-drain` run only with all six languages: each one sends to its own queue (text,
accented text, JSON, a message with attributes) and the next language in the ring (csharp, java, rust, go, python, node)
reads it, byte for byte.

## Run it

```
connectors/gateway-contract/run.sh --target emulator          # the proof that the tests are right
connectors/gateway-contract/run.sh --expect-fail              # before the gateway exists: every line fails
connectors/gateway-contract/run.sh                            # the goal: every line passes on Taba
connectors/gateway-contract/run.sh python --target emulator   # one language
```

Logs go to `.out/<run>/<driver>.log` (not versioned). The first run downloads the toolchain images and builds the drivers
(the Rust one takes minutes); the package caches live in Docker volumes (`gwpip`, `gwnpm`, `gwgocache`, `gwnuget`, `gwm2`,
`gwcargo`, `gwrusttarget`).
