<p align="center">
  <img src="doc/diagramas/img/hero-en.gif" width="100%" alt="Taba: open-source messaging that replaces SQS, SNS, Kafka and RabbitMQ with a single Docker image. Animation: 3 nodes, each drawn as a 3-story building. Several producers come and go and publish messages; each message has a primary node, which replicates it to the other two, and every node is the primary of some of them. Consumers arrive later, read asynchronously and confirm with an ack.">
</p>

🇺🇸 **English** | 🇧🇷 [Português (Brasil)](README-pt-BR.md)

**Taba** is a **lightweight, fast, open-source** messaging system that runs from **a single Docker image**: 1 node to try it out, 3 for production. It puts **queues** (each message goes to one consumer, like SQS) and **pub/sub** (each publication is copied to many queues, like SNS) behind the same HTTP API, and scales by adding nodes, with no coordinator and no complex configuration.

If you want to get away from per-message billing, from a cluster of brokers to operate, or from depending on a cloud, Taba is for you.

## What you get

- **One image, no dependencies:** every node runs the same simple, small Docker image. Start 1 node to test and 3 for production, with no separate broker, coordinator or database. It runs on one machine, on a VM or on Kubernetes.
- **Fast:** messages live in memory, replicated across the nodes. The disk only comes in when memory fills up.
- **Always available:** every topic is spread out and replicated. If a node goes down the cluster keeps running, and the node rejoins by itself when it recovers.
- **Scales effortlessly:** a new node joins the cluster, discovers it on its own and takes over its share. There is no coordinator to configure.
- **Almost no cost:** no per-message billing and no cloud lock-in. You only pay for the machine it runs on.
- **FIFO queues:** per-group delivery order and publish deduplication, for when sequence matters.
- **Simple API:** a few HTTP routes, no mandatory SDK and no new protocol to learn.
- **Simple connectors in the main languages:** C#, Java, Rust, C, C++, Python, PHP, Ruby, Go and Node.js, including old versions (see [Connectors](#connectors)).
- **Open source (AGPLv3):** the code is open and, if someone offers Taba as a service with modifications, they must share them under the same license.

> Speed and cost are **project goals**. There are no published benchmarks yet (see [Status and limits](#status-and-limits)).

## Taba vs. the alternatives

| | **Taba** | SQS + SNS | Kafka | RabbitMQ |
|---|---|---|---|---|
| **Where it runs** | Anywhere: machine, VM, Docker, Kubernetes | AWS only | Wherever you host it, or as a managed service | Wherever you host it, or as a managed service |
| **What to run** | The same image on 3 nodes (1 for dev) | Nothing (managed) | Brokers and metadata coordination | A broker (more nodes for high availability) |
| **Queue and pub/sub** | In the same HTTP API | Two separate services | Topics and consumer groups | Queues and exchanges |
| **Billing** | No per-message billing: infrastructure only | Per request | Infrastructure, or a managed plan | Infrastructure, or a managed plan |
| **Scaling** | Add a node | Automatic (managed) | Brokers and partitions | Nodes in the cluster |

## Try it in a minute

With Docker, a three-node cluster (behind an example HAProxy) starts with one command. An init container creates the cluster certificate and the root token, the first node forms the cluster and the other two join:

```bash
docker compose up --build -d      # three "healthy" nodes in about a minute
```

Then, with the [`taba-cli`](doc/pt-BR/20-cli.md), which already ships in the Compose file pointed at the cluster (and with the root token):

```bash
docker compose run --rm taba-cli health cluster -o table             # the three nodes, no conditions
docker compose run --rm taba-cli topic create orders --type queue
echo '{"order": 9001}' | docker compose run --rm -T taba-cli publish orders --stdin
docker compose run --rm taba-cli consume orders --wait 5 --ack      # receive and confirm
```

The client API is also at `http://localhost:8080` for any HTTP client (with `Authorization: Bearer <root token>`; the token is in the `secrets` volume: `docker compose run --rm --entrypoint cat taba-cli /secrets/root-token`; on Windows Git Bash prefix the command with `MSYS_NO_PATHCONV=1`). `docker compose down -v` deletes everything. HAProxy is just an example: any load balancer works. For a single node, with no cluster, use `docker run -p 8080:8080 -e TABA_MODE=development taba`.

> **Careful in production:** stop and update nodes **one at a time** (`docker compose stop` sends `SIGTERM`, which triggers a graceful *drain*). Stopping **all** nodes at once loses the messages and topics, and losing **two nodes in a row** locks a 3-node cluster. See the [operations guide](doc/pt-BR/23-operacao.md).

## The API in four calls

Everything is HTTP. The formal contract (OpenAPI) is not defined yet; the calls below are described in the [client API reference](doc/pt-BR/16-api-cliente.md).

| What you want | Call |
|---|---|
| Create a topic (Queue or Distributor) | `POST /topics` |
| Publish a message | `POST /topics/{topic}/messages` |
| Receive messages (long polling of up to 20 s) | `GET /topics/{topic}/messages` |
| Confirm consumption (ack) | `DELETE /topics/{topic}/messages/{id}` |

If the consumer does not confirm in time, the message becomes available again and is redelivered, as in SQS (the default invisibility is 120 s and can be adjusted).

## Connectors

The API is HTTP, so any language can already talk to Taba with the HTTP client it has. On top of that, the project provides **easy-to-use connectors** for the main languages, so you can publish and consume messages in a few lines. They have no external dependencies and run on old versions, up to 15 years old, as long as the platform speaks **TLS 1.2 or higher** (see each one's [base version](doc/pt-BR/21-conectores.md)):

| Language | Folder | Base version | State | Usage example |
|---|---|---|---|---|
| Java | `connectors/java/` | Java 11 | planned | TODO |
| C# | `connectors/csharp/` | .NET Framework 4.6.1 and .NET 5, 6, 8 and 10 | [ready](connectors/csharp/README.md) (nine async and sync operations, `subscribe`) | [console example](exemplos/csharp-console/) |
| Python | `connectors/python/` | 2.7.9 and 3.4.3 | planned | TODO |
| PHP | `connectors/php/` | 5.6 | planned | TODO |
| Ruby | `connectors/ruby/` | 2.0 | planned | TODO |
| Node.js | `connectors/node/` | 0.12 | planned | TODO |
| Go | `connectors/go/` | 1.7 | planned | TODO |
| Rust | `connectors/rust/client/` | 1.70 | synchronous ready ([README](connectors/rust/client/README.md)) | [example](connectors/rust/client/README.md) |
| C | `connectors/c/` | C89 | planned | TODO |
| C++ | `connectors/cpp/` | C++03 | planned | TODO |

The `taba-cli`, the client command line ([connectors/rust/cli](connectors/rust/cli/README.md)), is ready and is the shortest way to try a cluster.

Other languages use their own HTTP client or the C library. The design is in [the connectors guide](doc/pt-BR/21-conectores.md).

<!-- TODO: add usage examples (create topic, publish, consume with long polling and ack) for each language, once the connectors exist. -->

> The base versions are proposals and may change until confirmed (pending item D68, see [pending items](doc/pt-BR/31-pendencias.md)).

## Status and limits

- **Version 0.1 validated end to end:** the three-node cluster comes up with `docker compose up`, forms itself, serves through the `taba-cli`, detects a node going down, excludes it by vote and takes it back without losing confirmed messages (script in [docker/validate.sh](docker/validate.sh)). The complete documentation is in [doc/](doc/README.md). Still missing: the remaining connectors, the OpenAPI contract and publishing to package registries. See the [roadmap](doc/pt-BR/32-roadmap.md).
- **Not a protocol replacement:** Taba speaks HTTP. There is no AMQP, MQTT or Kafka protocol compatibility, so switching brokers means switching the client.
- **Not a streaming platform:** stream processing and replaying message history are out of scope. For queues and pub/sub, yes; for re-reading the past, no.
- **Out of scope for now:** graphical interface, advanced authentication and multi-region.

## How it works inside

**Taba** comes from **T**opics, **A**synchronous, **B**alanced and **A**utonomous (in Portuguese: **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos). In Tupi, *taba* is the **village**: each node is a hut, the cluster is the village, and messages move between them without you having to take care of it.

![C4 level 0: Taba context](doc/diagramas/img/c4-nivel0-contexto-en.svg)

| Letter | Promise | How Taba delivers |
|---|---|---|
| **T**opics | A lean API, with queue (single delivery) and prefix-based distribution in one place. | You send to a topic with HTTP verbs and choose the type: Queue Topic or Distributor Topic. |
| **A**synchronous | The producer sends and moves on. | A write is confirmed in memory by a configurable number of nodes; the disk only comes in on overflow. |
| **B**alanced | No node carries everything alone. | A consistent-hash ring splits topics into blocks and replicates them across several nodes. |
| **A**utonomous | No coordinator and no manual operation. | Nodes discover each other, monitor each other and vote out whoever fails. When the node returns, it is re-admitted. |

![Hash ring: blocks and roles](doc/diagramas/img/anel-hash-en.svg)

*The hash ring splits the work among the nodes.*

![Spread, redundant topic](doc/diagramas/img/topico-espalhado-en.svg)

*A topic spread across the cluster and replicated: if a node goes down, the village stays up.*

## Documentation

The documentation lives in [doc/](doc/README.md), one folder per language. **It is currently written in Brazilian Portuguese**: start at [doc/pt-BR/README.md](doc/pt-BR/README.md) (an AI-friendly index is in [doc/pt-BR/llms.txt](doc/pt-BR/llms.txt)). Other languages will be added as sibling folders. The code, identifiers, routes, configuration keys and error codes are in English.

## License

[GNU AGPLv3](LICENSE).
