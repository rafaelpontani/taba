---
title: Dependencies
summary: What Taba uses to build, run and test — server libraries and why, system dependencies, tools and the "zero dependencies" rule of the connectors.
type: reference
status: stable
verified-on: 2026-10-14 @ efeb47a
sources: [Cargo.toml, crates/taba-node/Cargo.toml, connectors/rust/Cargo.toml, connectors/rust/client/Cargo.toml, connectors/csharp, Dockerfile, docker-compose.yml]
---

# Dependencies

There are four groups: the **server** (the `taba-node` crate), the **connectors**, the **runtime environment** (what the image and the cluster need) and the **development tools**.

## 1. The server (`taba-node`)

Language **Rust**, edition 2024, async runtime **tokio**. The workspace has a single crate. Exact versions are in `crates/taba-node/Cargo.toml` and `Cargo.lock`; the table below says **what each one is for**.

| Need | Library | Where it enters |
|---|---|---|
| Runtime and async tasks, channels, clock | `tokio`, `tokio-util` | Everywhere: ready lists, channel output queues, background tasks, `CancellationToken` |
| HTTP (the three listeners) | `axum` (with `ws`), `hyper`, `hyper-util`, `tower`, `tower-http`, `tower-service`, `http-body-util` | Routers in `http/`; `tower-http` with `catch-panic` turns a *panic* into a `500` |
| Mutual TLS | `rustls` (`ring` provider), `tokio-rustls`, `x509-parser`, `rcgen` | Cluster port, certificate reload, `taba init` and `taba cert generate`. The host name is **not** verified: trust is that of the certificate itself |
| HTTP client between nodes | `reqwest` (`rustls` only) | The real `Transport` calls the cluster port of the other nodes |
| Channel WebSocket | `axum` (ws), `tokio-tungstenite`, `futures-util` | Permanent channel between each pair of nodes |
| S3 backup | `aws-sdk-s3` and `aws-config` (the official SDK) | `storage/backup/store`: the `S3Store` talks to AWS and MinIO (`backup.s3.endpoint`). It builds on Windows (MSVC) with no extra tools; it brings `aws-lc-rs` (the SDK's own TLS provider, separate from the `ring` of the rest of the node). `aws-sdk-s3` goes **without its `rustls` feature** (`default-features = false`): that feature is the legacy client (hyper 0.14, rustls 0.21, h2 0.3), which carried known vulnerabilities; the SDK's current client (`default-https-client`) is the one used |
| Local database | `rusqlite` with **bundled** SQLite (`bundled`) | `_node.db`, one database per topic, synchronization event database |
| JSON | `serde`, `serde_json` (`raw_value`) | API and channel DTOs |
| YAML | `serde_norway` | `.yml` configuration file |
| Identifiers | `uuid` (feature `v7`), `getrandom` | GUID v7 for message, topic, node and token; random secrets |
| Hash and encoding | `sha2`, `base64` | Ring position (SHA-256), salted token hash, base64 body |
| Dates | `time` | RFC 3339 in UTC with milliseconds |
| Errors | `thiserror` | `TabaError` and the errors of each module |
| Networks and IPs | `ipnet` | `operations.allowed-ips`, `client-ip.trusted-proxies` (CIDR ranges) |
| Disk | `fs4` | Free disk space (`diskFreeBytes`) |
| Logs and traces | `tracing`, `tracing-subscriber` (`json`, `env-filter`), `tracing-opentelemetry` | `text`, `color` or `json` format on `stdout` |
| OpenTelemetry | `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` (`http-proto` and `grpc-tonic`), `opentelemetry-appender-tracing`, `opentelemetry-proto`, `prost`, `tonic` | Traces and logs through the SDK; **OTLP metrics are assembled by hand** with `opentelemetry-proto`, because the SDK does not accept aggregated data from outside |

**Tests only** (`[dev-dependencies]`): `reqwest` with `json` and `tempfile` (a temporary folder per test).

### Choices worth explaining

- **Bundled SQLite.** The node does not depend on an installed database, but the **build needs a C compiler** (MSVC, `gcc` or `clang`). On the Windows `-gnu` target, `cc` must not find the Cygwin `gcc`.
- **`ring` as the TLS provider** of `rustls`: it is installed as the process default when opening TLS or OTLP, because another dependency brings `aws-lc` and the two cannot coexist as the default.
- **No command-line library:** `taba` and `taba-cli` read the arguments with their own code.
- **No dependency injection framework:** the boundaries are *traits* (`Transport`, `Clock`), and `main` wires the pieces ([04-architecture.md](04-architecture.md)).

## 2. Runtime environment

| What | What for | Required |
|---|---|---|
| **Docker** | Run the `taba` image (Alpine, static binary linked to musl, user `taba` with uid 10001) | No: the binary runs by itself |
| **Docker Compose** | The sample three-node cluster | Only for the sample |
| **HAProxy** | The **sample** load balancer of the Compose (client port 8080; discovery through port 8081) | No: any load balancer will do |
| **Secrets folder** | Cluster certificate and root token, created by `taba init` | In production |
| **Data folder** (`storage.dir`) | The node's files; **the last folder of the path must be named `taba`** | Yes |
| **OTLP collector / Prometheus** | Receive the telemetry | Optional |

Taba **does not need** an external database, a broker, a coordinator (ZooKeeper, etcd) or a discovery service. Discovery uses the address of a load balancer (`cluster.entry-address`).

## 3. Connectors

**Rule:** a connector with **no external dependencies** and able to run on old versions (up to 15 years), as long as the platform speaks **TLS 1.2 or higher**.

| Connector | Real dependencies | Note |
|---|---|---|
| Rust (`taba-client`) | `rustls` (`ring`, `tls12`) and `rustls-native-certs`; `serde` and `serde_json` only with the optional `serde` feature | Own HTTP/1.1 and JSON reader. Edition 2021 |
| `taba-cli` | `taba-client` | No others |
| C# (`Taba.Client`) | None beyond the platform; targets `net461`, `net5.0`, `net6.0`, `net8.0` and `net10.0` | C# 7.3. The dependency-injection extension (`Taba.Client.Extensions.DependencyInjection`) is a **separate** package that uses `Microsoft.Extensions.*` |
| C# tests | NUnit 3.14, `Microsoft.NET.Test.Sdk`, `Microsoft.Extensions.*` | Only in the tests |

## 4. Development tools

| Tool | What for |
|---|---|
| **Rust** (stable; `rustup`) | Build the server and the Rust connector. On Windows with Git Bash, the coreutils `link.exe` hides the MSVC one: use `cargo +stable-x86_64-pc-windows-gnu` if linking fails |
| **.NET SDK 8 or 10** | Build and test the C# connector |
| **Node.js 22+** | The diagram scripts (`doc/diagrams/scripts`) |
| **Chrome or Edge** (variable `ARCHIFY_CHROME`) | Export the SVGs and the README hero GIF |
| **Archify** (`~/.claude/skills/archify`, variable `ARCHIFY_BIN`) | Generate the diagrams from JSON |
| **ffmpeg** (variable `FFMPEG`) | The hero GIF |
| **GitHub Actions** | CI/CD ([27-ci-cd.md](27-ci-cd.md)) |

Everyday quality commands are in [24-conventions.md](24-conventions.md).

## Sources and evidence

- `crates/taba-node/Cargo.toml`, `Cargo.toml` (workspace), `connectors/rust/client/Cargo.toml`, `connectors/csharp/*/*.csproj`, `Dockerfile`, `docker-compose.yml`, `AGENTS.md` (sections on SQLite and Windows).
- Use cases: UC-CNT (connectors), UC-CFG (configuration and startup validations); see [26a](26a-implemented-use-cases.md).
- History: `18-estrutura-codigo.md` (17.2), `19-conectores.md`.
