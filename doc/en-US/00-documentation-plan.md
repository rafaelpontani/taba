# Plan for the new documentation (pt-BR; mirrored in en-US)

> **Status:** **executed** (2026-10-13, commit 71183b9). This file is the approved plan; the result of the execution is in section 9. The decisions of section 8 were answered.
> **Folder:** `doc/pt-BR/`. The old `doc/` folder is **neither changed nor deleted**; it becomes a *secondary source* (design history). The primary source is the **code**. (Later, the old documents were removed from the tree and live in the git history; and `doc/en-US/` was created as the English mirror of `doc/pt-BR/`.)
> **Note on this translation:** this is a historical plan. The file names cited here are those of the Portuguese folder (`01-visao-geral.md`…); the English folder has the same number prefixes with English names (`01-overview.md`…, see the table in [README.md](README.md)).

## 1. Goal

The old documentation (`doc/`, 7.3 thousand lines, 34 files) was written **before** the code, to guide the implementation: it mixes decisions, proposals, "To be defined" and continuous numbering across files ("section 7.2" is in a file that only the index reveals). The code now exists, so the new documentation has another role:

> **Explain how Taba is today, for someone who has never seen the project, so that the person (or an AI) can understand, operate, use and contribute without reading the code or the history.**

That changes three things relative to `doc/`:

1. **It describes what exists.** What is a proposal, partial or planned is marked (see section 5.2), never mixed into the text as if it were fact.
2. **Each document stands on its own.** No "section 7.2 of another file": links by file name and anchor, and a "Reading prerequisites" block at the top.
3. **Each technical statement points to the code** (module/file and, when it exists, the test/use case that proves it).

## 2. Validation of the scope against the code

I checked the initial list against the real structure of the repository (`crates/taba-node`, `connectors/`, `docker/`, `.github/`, `exemplos/`, the 28 diagrams and the 3 HTTP ports).

### 2.1 What of the original list is confirmed

| Requested item | Evidence in the code | Note |
|---|---|---|
| Architecture | modules `core`, `storage`, `engine`, `cluster`, `http`, `telemetry`, `config`, `secrets`, `cli`, `node.rs` | A single crate (`taba-node`), lib + `taba` binary |
| Dependencies | `crates/taba-node/Cargo.toml`, `connectors/*`, Docker, HAProxy, bundled SQLite | There are three groups: of the server, of the connectors (zero dependencies by rule) and of tools (Node, Chrome, ffmpeg for diagrams) |
| Rings | `core/ring`, `cluster/replication`, `cluster/sync` | "Ring" is just one (circular hash with blocks and P/S/T roles); see 2.3 |
| API (the 3) | `http/cluster_api` (+ WebSocket channel `/cluster/channel`), `http/operations.rs` (+ `/metrics`), `http/topics`, `messages`, `tokens` | Client, operations and cluster. The cluster also has the **WebSocket channel** between nodes |
| Topic types | `engine/topics`, `cluster/volatile`, tests `fifo*`, `volatile*` | Queue (QT), distributor (DT), volatile queue (QTV), FIFO (QTF) |
| Sending, durability, delivery | `cluster/replication/{coordinator,replica,forward}`, `engine/messages`, `storage/*` | Quorum, register + commit, disk overflow |
| Monitoring | `telemetry/*`, `http/metrics`, `http/topic_stats`, `/health/*` | OTLP (http/protobuf and grpc), Prometheus, traces, logs, conditions |
| Use cases | 3 files `20a/b/c` + `tests/uc_coverage.rs` | The cases are **testable**: each `UC-…` is cited in tests |
| CLI | `connectors/rust/cli` (includes TUI) **and** `crates/taba-node/src/cli` | They are **two different CLIs**: `taba` (server: `init`, start the node) and `taba-cli` (client). The new doc must separate the two |
| Connectors | `connectors/rust/client`, `connectors/csharp/*` | Only **2 of 10** implemented (Rust and C#); the other 8 are a plan |
| Pipeline | `.github/workflows/{ci,build-*,release}.yml`, `Dockerfile*`, `docker/validate.sh` | |
| Glossary, mind map, conventions, pending items, next steps | `doc/09`, `16`, `18`, `06d`, `08b`, `08c` | Reusable as a source |

### 2.2 What is **missing** from the list and the code shows to be essential

| Gap | Evidence | Why it enters |
|---|---|---|
| **Cluster life cycle** (join, failure detection, voting, *freezing*, return, drain, 1-node mode) | `cluster/{members,vote,health,recovery,drain,state}`, tests `failures`, `drain_cluster`, `recovery`, `freezing` | It is where most of the project's complexity is and does not fit in "rings" |
| **Storage and memory** (memory first, disk overflow, SQLite per topic, `_node.db`, sync log) | `storage/{db,messages,topics,state,synclog,losses,tokens}` | Answers "where is my message?" |
| **Invisibility, ack, TTL, dead-letter, groups** | `engine/messages`, tests `messages`, `conditions` | It is the heart of *delivery*; deserves its own document, not a list item |
| **Security** (tokens, mTLS with pinned certificate and reload, `taba init`, rate limit, client IP/proxies, operations IP filter) | `secrets/`, `cluster/tls`, `http/{auth,ratelimit,client_ip}`, `engine/tokens`, tests `mtls`, `auth`, `ratelimit` | Cross-cutting to the 3 APIs; today scattered |
| **Configuration** (parameter table, `TABA_*`, YAML, defaults) | `config/params`, there is a test that checks every parameter is read | The first thing whoever operates looks for |
| **Quick start** | `docker-compose.yml`, `docker/haproxy.cfg`, `taba init` | Whoever arrives needs to see it working in 5 minutes |
| **Operation** (runbook: start, scale, bring a node down, drain, backup, what to do on each `/health/cluster` condition) | `docker/cluster.sh`, `docker/validate.sh`, `operations.rs` | Today it does not exist as a document |
| **Tests and quality** (levels U/N/C/connectors, in-memory network, manual clock, `uc_coverage`) | `tests/common/{mod,cluster,real,otlp}.rs`, `tests/*.rs` | It is the method that guarantees the doc and the code match |
| **Errors and limits** (`ErrorCode` catalogue with fixed status, fixed limits) | `core/error`, `doc/06d`, `doc/12` | Lookup reference, not narrative |
| **Decisions (ADR)** | `doc/08a` (D1…D120) | The "why" lives here and must not be lost; enters as history, outside the main flow |
| **For contributors** (how to build on Windows/Git Bash, bundled SQLite, `target/` disk, when finishing an implementation) | `AGENTS.md` | Already exists, but is instruction for AI; needs a human version |

### 2.3 Points of the original list that I ask to **reformulate**

- **"How the rings work" (plural).** The code has a single hash ring (position → block → primary/secondary/tertiary roles). What looks like "another ring" is: the position of a **FIFO group** on the ring (topic `id` + `groupId`) and the **manager** of volatile topics, which changes when the ring changes. Proposal: a "Ring and replication" document with the ring and, in its own section, those two applications of it.
- **"Monitoring"** inside "functional specification" becomes its own **Observability** document, because it mixes node and topic metrics, traces, logs, health conditions and OTLP, and serves whoever operates.
- **"Pipeline"** is the **repository's** CI/CD (GitHub Actions, images, release). Not to be confused with the "message pipeline". The title will be **"CI/CD and releases"**.
- **"Use cases"** stops being narrative and becomes a **generated/verified catalogue**: the new doc only explains the format and the 3 groups with examples; the complete table continues to be checked by `uc_coverage` (see 6.3 on not duplicating).
- **"Connectors"** needs a **real-state** table (implemented / planned) and a "how to write a connector" guide (the common contract), because 8 of 10 do not exist yet.
- **"Pending items" and "Next steps"** of the old doc mix what is done with what is missing (e.g. steps 1–5 of 0.1 are "done"). The new version lists **only what is open**, in the format of section 5.4.

### 2.4 Divergences between the old doc and the code (to record)

The old doc describes itself as "proposals" in several places (libraries, crate names, "To be defined" parameters) and the root `README.md` still says the project "is in the documentation phase". During writing, each divergence found goes to `doc/pt-BR/_divergencias.md` (working file, not published in the index), with: what the old doc says, what the code does, which of the two is right. That feeds the new `08b` and avoids correcting the old doc.

## 3. Proposed structure (revised order)

**Principle of the order:** follow the journey of whoever arrives: *what it is → see it working → mental model → how it works inside → how to use → how to operate → how to contribute → reference → history/future.* The original list started with architecture; that requires the person to already know the concepts. Here the concepts come first.

Numbering by **part**, links by file name. Each file carries the "Sources" column (where it is checked in the code) and the "Old doc" column (where to reuse from).

### Part A — Understand

| # | File | Content | Sources in the code | Old doc |
|---|---|---|---|---|
| 01 | `01-visao-geral.md` | What Taba is, for whom, what it is **not**; numbered **goals and requirements** (`RF-`, `RNF-`) each with status; comparison with SQS/SNS/Kafka/RabbitMQ; state of the project | `README.md`, `Cargo.toml`, tests | 01, 03, 07, root README |
| 02 | `02-inicio-rapido.md` | Start the 3-node Compose, create token and topic, publish and consume with `taba-cli`, bring a node down and watch the cluster react | `docker-compose.yml`, `docker/`, `taba init` | root README, 19b |
| 03 | `03-conceitos.md` | Mental model on 1 page: node, cluster, topic, message, block, ring, quorum, invisibility, ack. Defines each term **once** and points to the detail | `core/*` | 09, 16 |

### Part B — How it works inside

| # | File | Content | Sources | Old doc |
|---|---|---|---|---|
| 04 | `04-arquitetura.md` | Containers and layers of the node, boundaries (traits `Transport`, `Clock`), three ports, composition in `main`/`node.rs`, module map | `lib.rs`, `node.rs`, `main.rs` | 02, 18 |
| 05 | `05-dependencias.md` | Server libraries and **why each one**, system dependencies (C compiler, Docker), tools (diagrams), the "connectors without dependencies" rule | `Cargo.toml`, `connectors/*` | 18 (17.2), 19, AGENTS |
| 06 | `06-anel-e-replicacao.md` | Circular hash, blocks, P/S/T roles, replication factor, placement of FIFO groups and of the volatile manager on the ring, what happens when the ring changes | `core/ring`, `cluster/replication` | 05 |
| 07 | `07-cluster.md` | Node join, member table, failure detection, voting, *freezing*, return of an excluded node, minimum size and 1-node mode, drain and stop, synchronization by streams | `cluster/{members,vote,health,recovery,sync,drain,state,channel}` | 04 |
| 08 | `08-armazenamento.md` | Memory first, disk overflow, SQLite per topic, `_node.db`, synchronization log, rebuild after restart, losses | `storage/*` | 02 (3.2), 14 |

### Part C — Functional specification

| # | File | Content | Sources | Old doc |
|---|---|---|---|---|
| 09 | `09-topicos.md` | Types: queue, distributor, volatile queue (automatic creation, manager), FIFO (groups, deduplication). "Which one" table and what each type allows | `engine/topics`, `cluster/volatile` | 15, 17 |
| 10 | `10-publicacao.md` | The write path: any node receives → forwards to the primary → register + commit → quorum → response; failures in the middle; FIFO and distributor publishing | `cluster/replication`, `http/messages` | 02 (3.1), 05 |
| 11 | `11-durabilidade.md` | Guarantees and **limits** (what is confirmed, when the message may be lost, what happens with 1 node, with the quorum lost), retention/TTL | `cluster/replication`, `storage`, tests `failures`, `recovery` | 05, 13 |
| 12 | `12-entrega.md` | Long polling, ready list, delivery owner, invisibility, ack, delivery limit, dead-letter, groups and fair delivery, delivery between nodes | `engine/messages`, `cluster/channel` | 13, 14 |
| 13 | `13-observabilidade.md` | Node and topic metrics, `/health/*` and conditions, OTLP/Prometheus, traces, logs (text/color/json format), sampling | `telemetry/*`, `http/metrics`, `http/topic_stats` | 11 |
| 14 | `14-seguranca.md` | Tokens (creation, scopes, revocation), cluster mTLS (`taba init`, rotation, reload), rate limit, client IP and trusted proxies, operations IP filter, known risks | `secrets/`, `cluster/tls`, `http/{auth,ratelimit,client_ip}`, `engine/tokens` | 06 (8.0), 12 |

### Part D — Interfaces

| # | File | Content | Sources | Old doc |
|---|---|---|---|---|
| 15 | `15-api-visao-geral.md` | The 3 ports, who uses each, authentication of each, conventions (format, headers, pagination, error DTO) | `http/mod.rs`, `http/common` | 06, 06d |
| 16 | `16-api-cliente.md` | Topics, messages, tokens: route by route, with a `curl` example and a real response | `http/{topics,messages,tokens}` | 06a |
| 17 | `17-api-operacao.md` | `/health/*`, `/metrics`, `/metrics/topics*`, drain | `http/operations.rs`, `metrics`, `topic_stats` | 06b |
| 18 | `18-api-cluster.md` | Node-to-node routes, WebSocket channel, NDJSON streams, DTOs. Warning: **internal, no stability guarantee** | `http/cluster_api`, `cluster/protocol` | 06c |
| 19 | `19-erros-e-limites.md` | `ErrorCode` catalogue with HTTP status and cause; fixed limits and rate limits | `core/error`, `http/ratelimit` | 06d, 12 |
| 20 | `20-cli.md` | **Two** CLIs: `taba` (server) and `taba-cli` (client, profiles, TUI, output) | `crates/taba-node/src/cli`, `connectors/rust/cli` | 19b |
| 21 | `21-conectores.md` | Common contract, state of each language, Rust and C# in detail, how to write a new one | `connectors/*`, `exemplos/` | 19, 19a |

### Part E — Operate

| # | File | Content | Sources | Old doc |
|---|---|---|---|---|
| 22 | `22-configuracao.md` | All keys, defaults, equivalent `TABA_*` variable, which are "calibration pending" | `config/params` | 10 |
| 23 | `23-operacao.md` | Runbook: start, scale, retire a node (drain), node went down, quorum lost, full disk, certificate rotation, what each health condition means | `docker/`, `cluster/*`, `operations.rs` | 04, 11 (new) |

### Part F — Contribute

| # | File | Content | Sources | Old doc |
|---|---|---|---|---|
| 24 | `24-convencoes.md` | **Code** (language, modules, `mod.rs` with `//!`, sibling tests), **API** (names, errors), **folders**, commits | `AGENTS.md`, `18` | 18, 06d, AGENTS |
| 25 | `25-testes.md` | Test levels, `TestNode`/`TestCluster`, in-memory network, manual clock, how to write a test that cites `UC-`, `uc_coverage` | `tests/`, `tests/common/` | 20 (19.1–19.5) |
| 26 | `26-casos-de-uso.md` | Format of the `UC-<AREA>-<NNN>` code, areas, how a case becomes a test, how to read the two catalogues below | `tests/uc_coverage.rs` | 20 (19.1–19.5) |
| 26a | `26a-casos-de-uso-implementados.md` | Catalogue of the cases **with at least one test** that cites them, grouped by area, with the test that proves them | `tests/*.rs`, `src/**/tests.rs`, `connectors/*` | 20a, 20b, 20c |
| 26b | `26b-casos-de-uso-planejados.md` | Catalogue of the cases **without a test yet** ("Tests" column = "—" in the old doc) or of ⬜ planned features, each with the reason and the pending item/milestone that closes it | `doc/20*`, `uc_coverage` | 20a, 20b, 20c |
| 27 | `27-ci-cd.md` | Workflows, images, versioning and release, `validate.sh` | `.github/`, `Dockerfile*`, `docker/` | 21 |

### Part G — Reference and project

| # | File | Content | Old doc |
|---|---|---|---|
| 28 | `28-glossario.md` | Terms in alphabetical order, each with a link to the document that explains it | 09 |
| 29 | `29-mapa-mental.md` | General map and per area (text only for now; see diagrams) | 16 |
| 30 | `30-decisoes.md` | History of decisions (short ADR: context, decision, consequence), by theme | 08a |
| 31 | `31-pendencias.md` | Only what is **open**: `D` questions and `PRB` problems, in the 5.4 format | 08b |
| 32 | `32-roadmap.md` | What is missing and the order (OpenAPI, 8 connectors, package publishing, Kubernetes), by milestone | 08c |

Support files:

- **`doc/README.md`** (new, **en-US**): entry door of the `doc/` folder, only points to the documentation of each language (today `pt-BR/README.md`; the old folder, without a language, is listed as "design history, pt-BR"). Future languages enter as a new row. **Attention:** a `doc/README.md` already exists (the index of the old doc, in Portuguese). Since nothing is deleted and moving the content would break the old doc's internal links, the proposal is: the current `doc/README.md` **stays**, and gains at the top an en-US block with the list of languages and the note "the index below is the design history, in pt-BR". See decision 3 in section 8.
- **`doc/pt-BR/README.md`**: index in Portuguese + "where to start" by profile (user, operator, contributor, AI).
- **`doc/pt-BR/llms.txt`**: see section 4.
- **`doc/pt-BR/_divergencias.md`**: working file.

**Repository root:** `README.md` and `AGENTS.md` must be **en-US**. Today they are in Portuguese. This is a **separate** task (translation + links to `doc/README.md`), listed as a milestone in the roadmap (phase 8); it will not be done together with the pt-BR doc.

## 4. Documentation techniques for AI-assisted projects

Applied from the first file:

| Technique | How it enters here |
|---|---|
| **`llms.txt`** (short Markdown index for models) | A `doc/pt-BR/llms.txt` with one line per document: title, link, "answers which questions" |
| **Metadata at the top (front matter)** | Each file starts with `título`, `resumo` (1 sentence), `status`, `última verificação` (date + commit hash), `fontes` (paths in the code), `leia antes` |
| **Self-contained blocks** | Short sections with a title that already says the answer; no "as seen above"; good for retrieval by chunks (RAG) |
| **Traceability document → code → test** | A "Sources and evidence" footer in each file, with paths and `UC-…` codes; `uc_coverage` already makes the test → use case link |
| **Diátaxis structure** | Each file is of **one** type: *tutorial* (02), *explanation* (03–14), *practical guide* (23, 24), *reference* (15–22, 28). Avoids mixing "why" with "how" |
| **ADRs** (decision record) | `30-decisoes.md` keeps the "why"; the main doc describes only the result and links the decision |
| **Executable examples** | Every `curl` and every command in the doc comes from a real run (or a test); no example "from memory" |
| **Documentation as code** | Automatic verification: broken links, cited `UC-` that exist, config table parameters that exist, documented routes that exist. Script proposal in `doc/pt-BR/scripts/` (phase 4) |
| **Context for agents** | The root `AGENTS.md` starts to point to `doc/pt-BR/README.md` and `llms.txt` and to describe **how to maintain** the new doc |
| **Canonical terms** | One name per concept, in Portuguese; the English identifier appears in `code`, at the first mention |

## 5. Writing conventions

### 5.1 Model of each file

```
---
título: …
resumo: one sentence
tipo: tutorial | explicação | guia | referência
status: estável | parcial | planejado
verificado-em: YYYY-MM-DD @ <commit>
fontes: [paths in the code]
---
# Title
> Read first: [03-conceitos](03-conceitos.md)
## What it is / What it is for
## How it works
## Example
## Limits and edge behaviors
## Sources and evidence
```

(In the English folder the keys are `title`, `summary`, `type`, `status` (`stable | partial | planned`), `verified-on` and `sources`.)

### 5.2 Status marking (on each feature)

`✅ implemented` · `🟡 partial` (says what is missing) · `⬜ planned` · `⚠️ known limit` (points to a `PRB-…`). A feature is only `✅` if code **and** test exist.

### 5.3 Language

Text in Brazilian Portuguese; identifiers, routes, keys and error codes in English, as in `AGENTS.md`. A future translation to another language gets a sibling folder (`doc/en/`, which became `doc/en-US/`).

### 5.4 Pending items and roadmap

A pending item is a table with: **code** (`D…`/`PRB-…`), **type**, **where in the code**, **effect**, **proposal**, **depends on**, **status**. The roadmap groups by milestone and cites the pending items each milestone closes. The numbering rules of `AGENTS.md` (codes never reused) continue to apply and the codes are **the same** as in the old doc.

## 6. Planned diagrams (none will be drawn now)

In each file, the spot is marked with a block:

```
> 🖼️ **DIAGRAMA PLANEJADO** — tipo: …; objetivo: …; reaproveita: `doc/diagrams/source/….json`
```

| File | Diagram | Type | Goal (what the reader must understand) | Reuse |
|---|---|---|---|---|
| 01 | System context | C4 level 0 | Who uses Taba (applications, operator) and what it replaces | `c4-nivel0-contexto` |
| 02 | Compose topology | Architecture (deployment) | What starts with `docker compose up`: init, 3 nodes, HAProxy, cli | new |
| 03 | Concept map | Relations between entities | How node, cluster, topic, message, block and ring connect | new |
| 04 | Containers and ports | C4 level 1 | The node's 3 ports and who talks to whom | `c4-nivel1-conteineres` |
| 04 | Node layers | Layered architecture | Dependency between modules (`core` → `storage` → `engine` → `cluster` → `http`) | new |
| 06 | Ring and roles | Script drawing (ring) | How positions map to blocks and P/S/T | `ring-hash.svg` |
| 06 | Spread topic | Message × node matrix | Redundancy: who stores what | `spread-topic.svg` |
| 06 | Ring changes (node joins/leaves) | Ring before/after | Which blocks migrate and why | new |
| 07 | Node join | Sequence | Steps from the `join` until the node is active | `ingresso-no` |
| 07 | Node removal | Sequence | Detection, voting and exclusion | `remocao-no` |
| 07 | Register failure | Sequence | What happens when the register fails | `falha-registro-no` |
| 07 | Return of an excluded node | Sequence | How an excluded node returns as new | `retorno-no-excluido` |
| 07 | Node states | State machine | `joining`, `syncing`, `active`, `returning`, `failed` and the `drainState` (freezing is **not** a state) | new |
| 08 | Memory and disk | Data flow | When the message leaves memory for disk and comes back | new |
| 09 | Topic type choice | Decision tree | Which type to use for each need | new |
| 09 | Distributor | Sequence | Publishing to a DT and fan-out | `distribuidor-publicacao` |
| 09 | Volatile queue | Sequence (3) | Automatic creation, deletion, manager change | `fila-volatil-*` |
| 09 | FIFO groups on the ring | Group × node matrix | Where each group lives | `fifo-groups.svg` |
| 10 | Write path | Architecture (flow) | Receive, forward, register, commit | `arquitetura-escrita`, `recepcao` |
| 10 | FIFO publishing | Sequence | How the group's primary coordinates | `fifo-publicacao` |
| 11 | Guarantees per scenario | Visual table (scenarios × result) | What survives each failure | new |
| 12 | Delivery and ack | Sequence | From `GET` to `DELETE` | `consumo` |
| 12 | Three `GET` scenarios | Sequence (3) | Local list, other node, waiting | `consumo-cenario1/2/3` |
| 12 | Invisibility cycle | State machine | Ready → invisible → ready/removed/DLQ | `invisibilidade` |
| 13 | Telemetry path | Data flow | Node → OTLP/Prometheus → collector | new |
| 14 | Trust boundaries | Security (zones) | Who trusts whom: client/token, operations/IP, cluster/mTLS | new |
| 15 | The three APIs | Architecture | Ports, audiences and authentication | new |
| 21 | Anatomy of a connector | Layers | `client` → `transport` → `http`/`json` | new |
| 23 | Diagnosis tree | Flowchart | From the symptom (`/health/cluster`) to the action | new |
| 25 | The project's test pyramid | Layers | U, N, C and connectors, and what each simulates | new |
| 27 | CI/CD pipeline | Flow (workflows) | From PR to release | new |
| 29 | General and per-area mind map | Mind map | View of everything and of each area | `mapa-mental*` |

The generation rules (source JSON → SVG, Archify) of `AGENTS.md` continue to apply when the diagrams' turn comes; the new doc will use the same `doc/diagrams/` folder so as not to duplicate.

## 7. How to write (method and phases)

For each file, in this order:

1. **Read the code** of the module in the "Sources" column (not the old doc).
2. **Read the old doc** only to recover intent and the "why".
3. Write; mark status (5.2); where code and old doc diverge, record in `_divergencias.md`.
4. Check each example by running it.
5. Fill in "Sources and evidence" and `verificado-em`.

| Phase | Deliverable | Reason for the order |
|---|---|---|
| 0 | This plan, approved | — |
| 1 | `README`, `llms.txt`, 01, 02, 03 | Gives the entry door and fixes the vocabulary the others use |
| 2 | 04–08 (inside) | Base for everything functional |
| 3 | 09–14 (functional), 22 | Depends on the vocabulary and on the storage model |
| 4 | 15–21 (interfaces) | Route by route, with real examples |
| 5 | 23–27 (operate and contribute) | Uses what has been explained |
| 6 | 28–32 (reference and project) | Glossary and map only close at the end, when the terms are stable |
| 7 | Automatic verification (links, `UC-`, parameters, routes) | Stops the doc from aging again |
| 8 | Root: `README.md` and `AGENTS.md` in en-US pointing to `doc/README.md` | Separate task, after the pt-BR doc |

Size estimate: new doc leaner than the old (target ≈ 4 to 5 thousand lines, against 7.3 thousand), because the use cases (`20a-c`, ~1.5 thousand lines) are **not copied**.

## 8. Decisions

| # | Decision | Answer |
|---|---|---|
| 1 | Folder | **`doc/pt-BR/`** (confirmed) |
| 2 | Use cases | **Two new files**: `26a` (implemented, with a test) and `26b` (planned, without a test). Consequence: `uc_coverage` today reads `doc/20a-c`; while the old doc exists it keeps reading there. Migrating the test to read `26a`/`26b` is a code change, **outside this plan**; the new doc cites the same `UC-` codes, which do not change |
| 3 | Index per language | **`doc/README.md`** references each language's README; root and `AGENTS.md` in en-US (see section 3, "Support files"). Pending: confirm the proposal of keeping the old `doc/README.md` with the en-US block at the top, since "do not delete anything" prevents replacing it |
| 4 | Parts E (Operate) and F (Contribute) | Clarification below. **Approved by default**, unless you say otherwise |
| 5 | Front matter and `llms.txt` | **Yes** |

**What Parts E and F are** (they were not in your list; they came from the code):

- **E — Operate** (22 configuration, 23 operation): for whoever **runs** Taba in production. All the configuration keys and their defaults, and a runbook: what to do when a node goes down, the quorum is lost, the disk fills, the certificate expires, and what each `/health/cluster` condition means. Without it, the doc would explain how Taba works, but not how to keep it up.
- **F — Contribute** (24 conventions, 25 tests, 26/26a/26b use cases, 27 CI/CD): for whoever **changes the code**. It includes what you already asked for (API, code and folder conventions; use cases; pipeline) and adds the test guide (`TestNode`, `TestCluster`, in-memory network), which is what ties the doc to the code.

That is, E and F are not loose extras: your list already had almost everything in F; E is the only really new group.

## 9. Result of the execution (2026-10-13)

**Delivered:** 33 numbered documents (01 to 32, plus 26a and 26b), `README.md` and `llms.txt`, `_divergencias.md`, three scripts in `scripts/` (`gerar-casos-de-uso.mjs`, `gerar-historico.mjs`, `verificar.mjs`) and, outside this folder, `doc/README.md` with the language index and the root `README.md` and `AGENTS.md` in **en-US**. The historical doc (`doc/*.md`) **was not changed** (only `doc/README.md` gained a block at the top).

| Phase | Deliverable | State |
|---|---|---|
| 1 | `README`, `llms.txt`, 01, 02, 03 | ✅ |
| 2 | 04 to 08 | ✅ |
| 3 | 09 to 14 and 22 | ✅ |
| 4 | 15 to 21 | ✅ |
| 5 | 23 to 27 | ✅ |
| 6 | 28 to 32 | ✅ (30 and 31 and the catalogues 26a/26b are generated by script) |
| 7 | Automatic verification | ✅ `node doc/pt-BR/scripts/verificar.mjs`: links, metadata, 105 configuration keys, 50 error codes, 43 routes, 84 metrics, 896 `UC-` codes, 31 planned diagrams |
| 8 | Root in en-US | ✅ `README.md` and `AGENTS.md` translated; `doc/README.md` lists the languages |
| 9 | Diagrams | ✅ The 31 spots resolved: 15 new diagrams in `doc/diagrams/source/`, the others reused (the write and mind map ones had their texts corrected against the code), the TUI screen in text and the durability table in Markdown. Index in [24-conventions.md](24-conventions.md#diagram-index) |

**How it was written:** each document started from the **code** (modules, `//!`, catalogues, tables) and used the historical doc only for the "why". The examples of API, CLI, cluster, security and operation were **run** against a node in development mode and against a three-node cluster started by `docker compose`. The divergences between the historical doc and the code are in [_divergences.md](_divergences.md) (25) and the findings, in [31-pending-items.md](31-pending-items.md).

**What was found** (summary; detail in [31-pending-items.md](31-pending-items.md)):

1. Topic metrics **do not aggregate across nodes** (`scope=cluster` returns only the node that serves).
2. **Seven** configuration parameters are accepted but have no effect.
3. **Losing two nodes in a row locks** a three-node cluster (`quorum-lost`), with no forced exclusion.
4. **Stopping the whole cluster** in production loses messages **and topics**.
5. The `uc_coverage` test **fails** today (old "Tests" column).
6. `docker compose stop` is a **drain**, not a failure; to simulate a failure use `kill`.
7. Several `08b` pending items are old (D79/PRB-013) and so are `//!` comments in the code.

**Limits of this run:** the complete `cargo test` suite was **not** run (the machine ran out of disk at *link*); `uc_coverage`, `--lib` (509 tests) and five integration files were run, all green except the "Tests" column of `uc_coverage`. The **diagrams were not drawn** (31 spots marked). Kubernetes was **not** validated.
