# AGENTS.md

Repository of the Taba messaging system: the documentation in `doc/` (one folder per language, starting at [doc/README.md](doc/README.md)) and the Rust application in `crates/`. The application (the node, in a single crate) implements a node and the cluster, with level-`C` tests over an in-memory network; the OpenAPI contract, the remaining connectors and package publishing are still missing ([doc/pt-BR/32-roadmap.md](doc/pt-BR/32-roadmap.md)). The structure, the code rules and the module map are in [doc/pt-BR/04-arquitetura.md](doc/pt-BR/04-arquitetura.md) and [doc/pt-BR/24-convencoes.md](doc/pt-BR/24-convencoes.md).

## Where to read first

- **Current documentation (Brazilian Portuguese):** [doc/pt-BR/README.md](doc/pt-BR/README.md), written *after* the code and checked against it. A machine-readable index is [doc/pt-BR/llms.txt](doc/pt-BR/llms.txt). Every file there starts with metadata (`resumo`, `status`, `verificado-em`, `fontes`) and ends with "Fontes e provas" (the code paths and `UC-…` cases that back it up).
- **Registers still in `doc/` (no language folder):** the use-case catalogue that `uc_coverage` checks (`doc/20*.md`) and the decision/pending registers (`doc/08a`, `08b`, `08c`). The original pre-code design documents were removed (see the git history); where old comments in the code cite "doc 05, section 7" and similar, the matching text is in `doc/pt-BR/`. The known disagreements between the old design and the code are in [doc/pt-BR/_divergencias.md](doc/pt-BR/_divergencias.md).
- To look up code, prefer the module map in [doc/pt-BR/04-arquitetura.md](doc/pt-BR/04-arquitetura.md) and the `//!` comment at the top of each `mod.rs`.

## Code

A Cargo workspace with a single crate, `crates/taba-node` (the `taba_node` library plus the `taba` binary). The modules follow [doc/pt-BR/04-arquitetura.md](doc/pt-BR/04-arquitetura.md); `main.rs` is only the command line and the signals, and everything else lives in the library so integration tests can start real nodes.

```
cargo build
cargo test                                   # unit (in the modules) and integration (crates/taba-node/tests/)
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --test uc_coverage -- --nocapture # scoreboard of the use cases that have a test
TABA_UC_WRITE=1 cargo test --test uc_coverage  # regenerates the "Testes" column of doc/20*.md (the test fails if it is stale)
cargo run -- init --out ./secrets             # cluster certificate and root token (taba init)
```

- **Docker:** `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d` starts the three-node cluster building the local images (plain `docker compose up -d` pulls the public `rafaelpontani/taba` images instead; Dockerfile, docker-compose.yml and docker-compose.dev.yml at the root, `docker/haproxy.cfg`); `docker compose run --rm taba-cli ...` uses the CLI; `docker compose down -v` deletes everything. `docker compose stop` sends `SIGTERM`, which triggers a graceful drain; use `docker/cluster.sh kill taba-2` to simulate a failure (`taba-0` is the seed service; `taba-1`, `taba-2`... are replicas of the `taba` service, so `docker compose kill` cannot pick one; `cluster.sh` maps `taba-N` to replica N).
- **Rust connectors:** `taba-client` and `taba-cli` live in `connectors/`, in their own Cargo workspace (`cd connectors/rust && cargo test --workspace --all-features`, plus `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all`). They follow the same test rules (a `tests.rs` file next to the module, `/// UC:`), and the server's `uc_coverage` reads the citations from here too. The binary: `connectors/rust/cli/scripts/build.sh` (or `build.ps1`); the image: `connectors/rust/cli/scripts/docker.sh`.
- **C# connector:** `connectors/csharp/` (`Taba.Client` and `Taba.Client.Tests`, NUnit; `cd connectors/csharp && dotnet test Taba.Client.Tests`). C# 7.3, no dependencies, five targets (D137). The tests cite `/// UC: UC-CNT-NNN` (a plain line, no XML) and `uc_coverage` reads them. Integration tests only run with `TABA_URL`: `connectors/csharp/scripts/integration.sh` starts Compose, creates tokens and queues and runs everything.
- **Cluster for integrated tests:** `docker/cluster.sh` (a library to `source`, or a command) starts/stops Compose, provokes failures (`taba_kill`, `taba_stop`, `taba_start`), waits for the cluster to settle and creates tokens/topics; connector tests should reuse it.
- **End-to-end validation:** `docker/validate.sh` starts Compose from scratch and checks the cluster, the `taba-cli` (only it: each connector's test is individual, like `connectors/csharp/scripts/integration.sh`), nodes going down and coming back, and no message loss (takes about 5 minutes). Run it before closing changes that affect the cluster, the channel, Compose or the connectors.
- **Configuration:** `crates/taba-node/src/config/params/mod.rs` is the table of keys and defaults ([doc/pt-BR/22-configuracao.md](doc/pt-BR/22-configuracao.md)). A new parameter goes in the table **and** in `Config`; a test checks that every parameter in the table is read.
- **Tests:** unit tests live in a file next to the module (`foo/mod.rs` and `foo/tests.rs` side by side, linked by `#[cfg(test)] mod tests;`), not inside the code. Each test cites the use cases it covers on a `/// UC: UC-XXX-NNN` line right above the `#[test]`. `tests/uc_coverage.rs` fails if the code does not exist (rules in [doc/pt-BR/26-casos-de-uso.md](doc/pt-BR/26-casos-de-uso.md)). Integration tests use `tests/common/mod.rs` (`TestNode`: a node on free ports, disk in a temporary folder); cluster tests (level `C`) use `tests/common/cluster.rs` (`TestCluster`: several nodes in the same process, wired by the in-memory network `MemoryNetwork` and a manual `Clock`). The OTLP tests (`tests/otlp.rs`) start a fake collector (`tests/common/otlp.rs`, `http/protobuf` and `grpc`) and run one at a time, because the `tracing` subscriber is per process. The inter-node notice tests (`tests/notify.rs`) and the channel tests (`tests/channel.rs`) use the in-memory network. The cluster-security tests (`tests/mtls.rs`) start real nodes over TCP on `127.0.0.x`, with the HTTP `Transport` and mutual TLS (`tests/common/real.rs`). More in [doc/pt-BR/25-testes.md](doc/pt-BR/25-testes.md).
- **Errors:** every API error is an `ErrorCode` from the catalogue (`core/error.rs`), with the fixed status from [doc/pt-BR/19-erros-e-limites.md](doc/pt-BR/19-erros-e-limites.md). A new code in the catalogue needs its line in the doc and its case in the test table.
- **Disk:** `target/` grows fast. The `dev` profile already keeps only line tables (`debug = "line-tables-only"`), but `cargo clean -p taba-node` deletes old test binaries (every change leaves a new one). When space is low, run cargo with `CARGO_INCREMENTAL=0` or delete `target/debug/incremental`; "no space on device" from `rustc` and `LNK1180`/`LNK1201` from `link.exe` are this.
- **SQLite:** `rusqlite` compiles the bundled SQLite (`bundled`), so the build needs a C compiler (MSVC, `gcc` or `clang`). On the Windows `-gnu` target, `cc` must not find the Cygwin `gcc`.
- **Windows with Git Bash:** the coreutils `link.exe` hides the MSVC one, and the MSVC on this machine is incomplete. If `cargo build` fails at link time, use `cargo +stable-x86_64-pc-windows-gnu ...` (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`). Docker commands that take a container path need `MSYS_NO_PATHCONV=1`. On Windows, `cargo test` fails with "access denied" if a compiled `taba.exe` is still running.

## Decisions and pending items

- [doc/08a-decisoes-tomadas.md](doc/08a-decisoes-tomadas.md) keeps **everything already decided**; [doc/08b-pendencias.md](doc/08b-pendencias.md) keeps **only what is still open**. Never leave in `08b` a note about something already defined: it goes to `08a`, on its own line, with the code of the question and the link to where the definition is. (`doc/pt-BR/30-decisoes.md` and `31-pendencias.md` are **generated** from these two by `node doc/pt-BR/scripts/gerar-historico.mjs`.)
- A partially resolved question: the decided part goes to `08a` (with the same code, e.g. D13.1), and `08b` keeps only the questions that are missing, each with its sub-code.
- When closing a pending item: record the decision in the technical document, the default value in [doc/pt-BR/22-configuracao.md](doc/pt-BR/22-configuracao.md), the line in `08a`, and **delete** the entry from `08b` and the reference in [doc/08c-proximos-passos.md](doc/08c-proximos-passos.md). Codes are never reused.

## Language

- **Documentation:** in Brazilian Portuguese, in `doc/pt-BR/`. In the end it will be translated and live in sibling folders, one per language. **The repository root files (`README.md`, this `AGENTS.md`) are in English (en-US).**
- **Code:** names of variables, functions, types, modules, fields, tests, configuration keys, routes, error codes and events in **English**, to ease international collaboration. No identifier in Portuguese, not even in tests.
- **Comments and user-facing messages** (API errors, logs, command-line help): in **Portuguese** during development; they will be translated together with the documentation.
- On-disk names (SQLite tables and columns, `_topic.db`, `${topic}` in folders, `state` keys) are also in English, in the documents and in the code.

## When you finish an implementation

There is no chronological log of the implementation any more: the history is git, with commit messages that say what changed and why. In the same change as the code:

1. **Fix the technical `.md`** when the code decided something the document did not say or contradicts it. The design lives in the documents, and the code is checked against them. Update the matching file in `doc/pt-BR/` (the file whose "fontes" metadata names the code you changed), and run `node doc/pt-BR/scripts/verificar.mjs` (it checks links, metadata, configuration keys, error codes, routes, metrics and use-case codes against the code).
2. A **firm decision** goes to [doc/08a-decisoes-tomadas.md](doc/08a-decisoes-tomadas.md); a **question** or **problem**, to [doc/08b-pendencias.md](doc/08b-pendencias.md) (section below). Then run `node doc/pt-BR/scripts/gerar-historico.mjs`.
3. **New or changed use cases** go to `doc/20a`, `20b` or `20c`, and each test cites them with `/// UC: UC-XXX-NNN`. Run `TABA_UC_WRITE=1 cargo test --test uc_coverage` to regenerate the "Testes" column, then `node doc/pt-BR/scripts/gerar-casos-de-uso.mjs` to regenerate [doc/pt-BR/26a-casos-de-uso-implementados.md](doc/pt-BR/26a-casos-de-uso-implementados.md) and `26b`. A case without a test keeps "—", and the reason goes in the case itself.
4. **A new module** goes in the module map of [doc/pt-BR/04-arquitetura.md](doc/pt-BR/04-arquitetura.md), and its `mod.rs` starts with a `//!` comment saying what it does and which document holds the design.

## Pending items and problems found during implementation

Every implementation that finds something open records it **in the same change**:

- **Design question** (the document does not define it, or there are two ways out): goes to [doc/08b-pendencias.md](doc/08b-pendencias.md) with the next free `D` code and a proposal. When decided, it goes to `08a` (rules in the "Decisions and pending items" section).
- **Logic problem, risk or limit** (the code behaves in a flawed, unfair or incomplete way): goes to [doc/08b-pendencias.md](doc/08b-pendencias.md) with the next `PRB-NNN`, stating the effect and the proposal; if a `D` question resolves it, it goes inside it ("Problema ligado"). When resolved, it goes to the "Problemas resolvidos" section of [doc/08a-decisoes-tomadas.md](doc/08a-decisoes-tomadas.md) with the date; the code is not reused.
- In `08b`, what **does not depend on anything** stays in section 1 and what depends on another question, problem or step stays in section 2, with "Depende de".
- A deviation from the document is fixed in the technical `.md` itself; if it depends on a decision, it becomes a `D` question in `08b`.

## Use cases

The use cases that guide the tests are in [doc/20-casos-de-uso.md](doc/20-casos-de-uso.md) (and `20a`, `20b`, `20c`), each with a `UC-<AREA>-<NNN>` code that the tests cite on a `/// UC: UC-MSG-003` line. The method is explained in [doc/pt-BR/26-casos-de-uso.md](doc/pt-BR/26-casos-de-uso.md).

- Codes are **not reused**: a case that ceases to exist stays in the table as `(retirado)`; a new case gets the next free number in the area.
- When you change the behaviour described in a technical `.md`, update the matching use cases **in the same commit** (and the total in the index of `20-casos-de-uso.md`).
- A case only counts as covered with at least one test that cites it, at the level of the "Nível" column.

## Diagrams

Diagrams are generated with [Archify](https://github.com/tt-a1i/archify) from JSON. **The JSON is the source of truth**; the SVG and the HTML are generated outputs.

```
doc/diagramas/
  fonte/   *.json          source (English), versioned. This is what you edit.
  i18n/    <lang>.json     one translation table per extra language (pt-BR.json), versioned.
  img/<lang>/ *.svg        exported per language (en, pt-BR, ...), versioned. This is what the .md files embed.
  html/<lang>/ *.html      interactive, NOT versioned (only exists with --html).
  scripts/gerar.sh         validates, renders and exports the SVG of every language
  scripts/traduzir.mjs     applies a language table to a source JSON (used by gerar.sh)
  scripts/reespacar.mjs    recomputes the y of the messages of sequence diagrams
  scripts/exportar-svg.mjs used by gerar.sh (headless Chrome)
```

The documentation in `doc/pt-BR/` embeds the `pt-BR` SVGs (`![…](../diagramas/img/pt-BR/<name>.svg)`) and the English `README.md` embeds the `en` ones; the table of which diagram is used where is in [doc/pt-BR/24-convencoes.md](doc/pt-BR/24-convencoes.md#índice-dos-diagramas). A spot that still needs a diagram is marked `🖼️ DIAGRAMA PLANEJADO` with its type and goal.

### Rules

- Always edit `doc/diagramas/fonte/<name>.json`. Never edit the SVG or the HTML: they are overwritten at every generation.
- After editing, run `doc/diagramas/scripts/gerar.sh <name>` and commit the JSON **and** the SVGs of every language together. With no name, it generates all of them; `--lang <code>` limits it to one language.
- Run the commands from the repository root. The JSON `meta.output` is relative to it (`doc/diagramas/html/<name>.html`) and must not be changed.
- `--html` keeps the interactive HTML in `doc/diagramas/html/` to open in a browser. It is ignored by git (`.gitignore`).
- The `.archify/` folder is Archify's work area and is also ignored. New diagrams are born there, but only enter the repository after being copied to `doc/diagramas/fonte/` with a stable name (no date) and `meta.output` pointing to `doc/diagramas/html/`.
- English is the source language: `fonte/*.json` holds the English text. Every other language is a flat table in `doc/diagramas/i18n/<lang>.json` (`"English string": "translation"`) applied to the display keys (`title`, `label`, `sublabel`, `note`, `items`, `tag`) at generation time. A string missing from a table stays in English and `gerar.sh` warns about it, so when you add or reword a string in a source JSON, add the translation to every table. If a translation does not fit (the Archify gates complain), shorten it in the table.
- **To add a language:** create `doc/diagramas/i18n/<lang>.json` (copy `pt-BR.json` and translate the values), run `gerar.sh`, `node doc/diagramas/scripts/gerar-hash.mjs` and `node doc/diagramas/scripts/gerar-hero.mjs`, and embed `../diagramas/img/<lang>/<name>.svg` from that language's docs. The legend and the Archify viewer interface are fixed in English.
- When you change the behaviour described in a `.md`, update the matching diagram (table in [doc/pt-BR/24-convencoes.md](doc/pt-BR/24-convencoes.md#índice-dos-diagramas)) and regenerate.

### Script-drawn diagrams (exception)

`anel-hash.svg`, `topico-espalhado.svg` and `fifo-grupos.svg` do not go through Archify (it does not draw rings or matrices) and **have no JSON**: the source is `doc/diagramas/scripts/gerar-hash.mjs`. Edit the script constants (nodes, ring positions, messages, matrix) and run `node doc/diagramas/scripts/gerar-hash.mjs` (all languages; `--lang=<code>` for one; the strings go through the same `i18n/<lang>.json` tables); `gerar.sh` does not generate them. Commit the script **and** the SVGs together. The script does not validate the layout: open the SVG (or take a Chrome capture) and check for overlapping text.

The same applies to the root README hero (`hero.svg` and `hero.gif`): the source is `doc/diagramas/scripts/gerar-hero.mjs`. Edit the texts, colours and timeline (`PRODUTORES`, `CONSUMIDORES`, `MSGS`, `LEGENDA`) and run `node doc/diagramas/scripts/gerar-hero.mjs` (all languages; `--lang=<code>` for one; the texts come from the `i18n/<lang>.json` tables). `hero.svg` is the static frame (instant `ESTATICO`) and `hero.gif` is what the README embeds; the GIF uses headless Chrome and `ffmpeg` (variable `FFMPEG` if it is not on the PATH). `--so-svg` generates only the SVG. Commit the script **and** both files together.

### Prerequisites

- Node.js 22 or later.
- Archify installed as a skill (`~/.claude/skills/archify`). Another location: set `ARCHIFY_BIN` to the path of `archify.mjs`.
- Chrome or Edge. Another executable: set `ARCHIFY_CHROME`.

### Editing a sequence diagram

The `messages` array defines the order; `y` is recomputed by `scripts/reespacar.mjs`.

1. Insert, remove or reorder messages in the array. For a new message, use any `y` (if there are `segments`, use the `y` of a neighbouring message of the same phase).
2. `node doc/diagramas/scripts/reespacar.mjs doc/diagramas/fonte/<name>.json` reassigns the `y` values (28 px step, gap between phases), rebuilds the `segments` bands and adjusts `meta.viewBox`.
3. `doc/diagramas/scripts/gerar.sh <name>`.

Archify limits (the `showcase` profile) that `scripts/gerar.sh` enforces:

- Messages that share horizontal space need at least 28 px between them.
- The `viewBox` aspect ratio may not be taller than about 1.55 (width/height) at a width of 1080. If `scripts/reespacar.mjs` warns, split the diagram in two. That is how consumption became `consumo` (delivery and ack) plus three scenarios.
- With a width above about 1085, the secondary text gets too small on screen and the check fails.

### Editing an architecture diagram (including C4)

There is no automatic respacing. Adjust the components' `pos` and `size` in the JSON, keeping:

- connected neighbours adjacent, with a gap of `6.5 px × characters + 21 px` for each connection label on the main line;
- `boundaries` only of type `region` or `security-group`;
- `meta.viewBox` omitted in new architecture diagrams.

### Diagnostics

When Archify rejects a diagram, `scripts/gerar.sh` prints the codes and messages. The most common:

| Message | Fix |
|---|---|
| `less than 28px apart` | run `scripts/reespacar.mjs` |
| `leaving no room for the legend` | increase the `viewBox` height (`scripts/reespacar.mjs` already computes it) |
| `composition/viewport-height` | diagram too tall: split it |
| `composition/desktop-readability` | reduce the `viewBox` width |

Format details: `~/.claude/skills/archify/references/authoring-contract.md` and the schemas in `~/.claude/skills/archify/schemas/`.
