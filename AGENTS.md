# AGENTS.md

Repository of the Taba messaging system: the server (a single Rust crate in `crates/`, the node and the cluster), the connectors (`connectors/`: the Rust client and `taba-cli`, and the C# client), the Helm chart (`deploy/helm/taba/`) and the documentation (`doc/`, one folder per language, starting at [doc/README.md](doc/README.md)). The structure, the code rules and the module map are in [doc/en-US/04-architecture.md](doc/en-US/04-architecture.md) and [doc/en-US/24-conventions.md](doc/en-US/24-conventions.md); what is still planned is in [doc/en-US/32-roadmap.md](doc/en-US/32-roadmap.md).

## Taba is in production

Taba is released and people run it. Every version is published to Docker Hub and GHCR (images and the Helm chart), crates.io (`taba-client`, `taba-cli`), nuget.org (`Taba.Client*`) and as a GitHub Release. **Published versions are permanent**: crates.io and nuget.org do not delete versions, GitHub Releases are immutable and version tags are protected. So:

- Treat every change as a change to a running system. The client and operations APIs, the configuration keys, the on-disk formats (SQLite databases, `storage.dir` layout), the S3 backup layout and the connectors' public API are contracts. Keep them compatible; when a change cannot be, mark it as breaking (`feat!:` or a `BREAKING CHANGE:` footer) and describe the upgrade in the documentation of both languages.
- A behaviour change without a test is not done. A change that touches the cluster, the channel between nodes, Compose or the connectors is also checked with `docker/validate.sh` before it reaches `main`.
- Known limits are documented, not hidden: a limit you find goes to the pending items ([below](#pending-items-and-problems-found-during-implementation)) in the same change.

## Workflow: branches, pull requests and releases

```
feature branch ──PR──▶ release ── release.yml ──▶ vX.Y.Z-rc.N   (candidate: images, packages, Release)
                          │
                          └──PR "Release vX.Y.Z"──▶ main ── release.yml ──▶ vX.Y.Z   (final)
                                                     │
                          release ◀──PR (back-merge)─┘
```

1. **Work on a branch created from `release`** (`feat/...`, `fix/...`, `docs/...`, `ci/...`) and open a pull request **to `release`**. `ci.yml` builds and tests the server, both connectors and the chart and lints the workflows; the pull request can only be merged with every required check green. Merge with a **merge commit** (no squash, no rebase): the commit types inside are what compute the version.
2. **Merging into `release` publishes a candidate** when the commits include `feat`, `fix` or `perf`: `release.yml` builds, tests, publishes `vX.Y.Z-rc.N` everywhere (as a pre-release) and runs the `verify` job. `docs`, `test`, `chore`, `ci` and `refactor` only run the builds.
3. **Test the candidate** before promoting it: `docker/validate.sh` on the `release` commit (three-node cluster, crash and drain of nodes, no message loss, S3 backup and restore; about 5 minutes); the chart tests (`deploy/helm/tests/run.sh`) when the chart changed; `connectors/csharp/scripts/integration.sh` when the C# connector changed.
4. **Promote with a pull request `release → main`** titled "Release vX.Y.Z". Merging it publishes the final `vX.Y.Z` (and moves `latest`, `vX.Y`, `vX`).
5. **Merge `main` back into `release`** with a pull request, so the two do not diverge. The next cycle starts on `release`.

A correction to a published final follows the same path (`fix:` → candidate → validation → `main`); there is no direct fix on `main`. Dependabot opens its pull requests against `release` with `ci`/`chore` prefixes, which do not release by themselves.

`main` and `release` are protected by rulesets: pull request required, the `ci.yml` checks required, no force push, no deletion. Version tags (`v*.*.*`) cannot be moved or deleted. Pipeline details: [doc/en-US/27-ci-cd.md](doc/en-US/27-ci-cd.md).

**Commit type and version:** a change that does not break the signature of the APIs (client and operations APIs, connectors' public API, configuration keys, on-disk formats) is a `fix:` or a `perf:`, which raises the **patch** number (`0.1.1` → `0.1.2`). Only a change that breaks a signature is marked `feat!:` (or has a `BREAKING CHANGE:` footer), which raises the **minor** number while the project is in `0.x` (rule `breaking → minor` in [doc/en-US/27-ci-cd.md](doc/en-US/27-ci-cd.md)). Do not use a plain `feat:` to describe additive, compatible work; use `fix:` or `perf:`. Changes that must not publish a version are `docs:`, `test:`, `chore:`, `ci:` or `refactor:`.

**An agent never creates commits.** It leaves every change in the working tree (staged or not) for the developer to review; the developer commits. Not even when a task seems finished, and not even a "small" or documentation-only change. When it needs to propose a message, it writes it in the reply (with the commit type that decides whether a merge publishes a version) and does not run `git commit`.

**What an agent does not do on its own** (only when the maintainer asks, in that conversation): push to `main` or `release`, merge a pull request, create, move or push tags (a pushed tag that points to an old commit runs that commit's old `release.yml`), create Releases, run `release.yml` by hand with `dry_run=false`, or change repository settings, rulesets, environments, secrets, variables or registry settings. Pick the commit type with care: it decides whether a merge publishes a permanent version.

External contributions need the CLA ([CONTRIBUTING.md](CONTRIBUTING.md)); vulnerabilities are reported privately ([SECURITY.md](SECURITY.md)).

## Where to read first

- **Current documentation (Brazilian Portuguese and US English, with the same content):** [doc/pt-BR/README.md](doc/pt-BR/README.md) and [doc/en-US/README.md](doc/en-US/README.md), written *after* the code and checked against it. Machine-readable indexes are [doc/pt-BR/llms.txt](doc/pt-BR/llms.txt) and [doc/en-US/llms.txt](doc/en-US/llms.txt). Every file there starts with metadata (`resumo`, `status`, `verificado-em`, `fontes` in `pt-BR`; `summary`, `status`, `verified-on`, `sources` in `en-US`) and ends with "Fontes e provas" / "Sources and evidence" (the code paths and `UC-…` cases that back it up).
- The original pre-code design documents were removed (they are in the git history); where old comments in the code cite "doc 05, section 7" and similar, the matching text is in the language folders.
- To look up code, prefer the module map in [doc/en-US/04-architecture.md](doc/en-US/04-architecture.md) and the `//!` comment at the top of each `mod.rs`.

## Code

A Cargo workspace with a single crate, `crates/taba-node` (the `taba_node` library plus the `taba` binary). The modules follow [doc/en-US/04-architecture.md](doc/en-US/04-architecture.md); `main.rs` is only the command line and the signals, and everything else lives in the library so integration tests can start real nodes.

```
cargo build
cargo test                                   # unit (in the modules) and integration (crates/taba-node/tests/)
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --test uc_coverage -- --nocapture # scoreboard of the use cases that have a test
cargo run -- init --out ./secrets             # cluster certificate and root token (taba init)
```

- **Docker:** `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d` starts the three-node cluster building the local images (plain `docker compose up -d` pulls the public `rafaelpontani/taba` images instead; Dockerfile, docker-compose.yml and docker-compose.dev.yml at the root, `docker/haproxy.cfg`); `docker compose run --rm taba-cli ...` uses the CLI; `docker compose down -v` deletes everything. `docker compose stop` sends `SIGTERM`, which triggers a graceful drain; use `docker/cluster.sh kill taba-2` to simulate a failure (`taba-0` is the seed service; `taba-1`, `taba-2`... are replicas of the `taba` service, so `docker compose kill` cannot pick one; `cluster.sh` maps `taba-N` to replica N).
- **Kubernetes (Helm):** the chart is `deploy/helm/taba/` ([doc/en-US/34-helm-chart.md](doc/en-US/34-helm-chart.md); operating modes and tests in [doc/en-US/35-helm-use-cases.md](doc/en-US/35-helm-use-cases.md)). `deploy/helm/tests/run.sh static` needs no cluster (helm or Docker); `run.sh 01 05` creates a kind cluster and applies the chart, one script per use case (`uc-NN-*.sh`, values in `deploy/helm/taba/ci/`). A chart change updates `values.yaml`, `values.schema.json`, both languages of docs 34/35 and the tests.
- **Rust connectors:** `taba-client` and `taba-cli` live in `connectors/`, in their own Cargo workspace (`cd connectors/rust && cargo test --workspace --all-features`, plus `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all`). They follow the same test rules (a `tests.rs` file next to the module, `/// UC:`), and the server's `uc_coverage` reads the citations from here too. The binary: `connectors/rust/cli/scripts/build.sh` (or `build.ps1`); the image: `connectors/rust/cli/scripts/docker.sh`.
- **C# connector:** `connectors/csharp/` (`Taba.Client` and `Taba.Client.Tests`, NUnit; `cd connectors/csharp && dotnet test Taba.Client.Tests`). C# 7.3, no dependencies, five targets (D137). The tests cite `/// UC: UC-CNT-NNN` (a plain line, no XML) and `uc_coverage` reads them. Integration tests only run with `TABA_URL`: `connectors/csharp/scripts/integration.sh` starts Compose, creates tokens and queues and runs everything.
- **Cluster for integrated tests:** `docker/cluster.sh` (a library to `source`, or a command) starts/stops Compose, provokes failures (`taba_kill`, `taba_stop`, `taba_start`), waits for the cluster to settle and creates tokens/topics; connector tests should reuse it.
- **End-to-end validation:** `docker/validate.sh` starts Compose from scratch and checks the cluster, the `taba-cli` (only it: each connector's test is individual, like `connectors/csharp/scripts/integration.sh`), nodes going down and coming back, no message loss and the S3 backup (takes about 5 minutes). Run it before closing changes that affect the cluster, the channel, Compose or the connectors, and on every candidate before the pull request to `main`.
- **OpenAPI:** the client API (`api.port`) and the monitoring API (`operations.port`) are described by documents generated from the routes ([doc/en-US/15-api-overview.md](doc/en-US/15-api-overview.md)). A route is registered with `routes!(handler)` of `utoipa-axum` and the handler carries its `#[utoipa::path(...)]` (path, parameters, request body, every status it can answer, with the permission in the description); a DTO the handler serializes from a `struct` derives `ToSchema`, and one built with `json!` has its twin in `crates/taba-node/src/http/openapi/schemas.rs`. After changing a route, a parameter or a response, regenerate the published copies and commit them with the change: `cargo run -q -- openapi client > openapi/client.json` and `cargo run -q -- openapi monitoring > openapi/monitoring.json` (`tests/openapi.rs` fails when they are out of date, and checks real responses against the schemas). The cluster API is internal and is never documented there.
- **Configuration:** `crates/taba-node/src/config/params/mod.rs` is the table of keys and defaults ([doc/en-US/22-configuration.md](doc/en-US/22-configuration.md)). A new parameter goes in the table **and** in `Config`; a test checks that every parameter in the table is read.
- **Tests:** unit tests live in a file next to the module (`foo/mod.rs` and `foo/tests.rs` side by side, linked by `#[cfg(test)] mod tests;`), not inside the code. Each test cites the use cases it covers on a `/// UC: UC-XXX-NNN` line right above the `#[test]`. `tests/uc_coverage.rs` fails if a cited code does not exist (rules in [doc/en-US/26-use-cases.md](doc/en-US/26-use-cases.md)). Integration tests use `tests/common/mod.rs` (`TestNode`: a node on free ports, disk in a temporary folder); cluster tests (level `C`) use `tests/common/cluster.rs` (`TestCluster`: several nodes in the same process, wired by the in-memory network `MemoryNetwork` and a manual `Clock`). The OTLP tests (`tests/otlp.rs`) start a fake collector (`tests/common/otlp.rs`, `http/protobuf` and `grpc`) and run one at a time, because the `tracing` subscriber is per process. The inter-node notice tests (`tests/notify.rs`) and the channel tests (`tests/channel.rs`) use the in-memory network. The cluster-security tests (`tests/mtls.rs`) start real nodes over TCP on `127.0.0.x`, with the HTTP `Transport` and mutual TLS (`tests/common/real.rs`). More in [doc/en-US/25-testing.md](doc/en-US/25-testing.md).
- **Errors:** every API error is an `ErrorCode` from the catalogue (`core/error.rs`), with the fixed status from [doc/en-US/19-errors-and-limits.md](doc/en-US/19-errors-and-limits.md). A new code in the catalogue needs its line in the doc and its case in the test table.
- **Disk:** `target/` grows fast. The `dev` profile already keeps only line tables (`debug = "line-tables-only"`), but `cargo clean -p taba-node` deletes old test binaries (every change leaves a new one). When space is low, run cargo with `CARGO_INCREMENTAL=0` or delete `target/debug/incremental`; "no space on device" from `rustc` and `LNK1180`/`LNK1201` from `link.exe` are this.
- **SQLite:** `rusqlite` compiles the bundled SQLite (`bundled`), so the build needs a C compiler (MSVC, `gcc` or `clang`). On the Windows `-gnu` target, `cc` must not find the Cygwin `gcc`.
- **Windows with Git Bash:** the coreutils `link.exe` can hide the MSVC one. If `cargo build` fails at link time, use `cargo +stable-x86_64-pc-windows-gnu ...` (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`). Docker commands that take a container path need `MSYS_NO_PATHCONV=1`. On Windows, `cargo test` fails with "access denied" if a compiled `taba.exe` is still running.

## Decisions and pending items

Both registers live in the language folders and are **maintained by hand, in both languages, in the same commit** as the change: [doc/en-US/30-decisions.md](doc/en-US/30-decisions.md) / [doc/pt-BR/30-decisoes.md](doc/pt-BR/30-decisoes.md) keep **everything already decided**, and [doc/en-US/31-pending-items.md](doc/en-US/31-pending-items.md) / [doc/pt-BR/31-pendencias.md](doc/pt-BR/31-pendencias.md) keep **only what is still open**.

- Never leave in the pending items a note about something already defined: it goes to the decisions, on its own row, with the code of the question and the link to where the definition is.
- A partially resolved question: the decided part goes to the decisions (with the same code, e.g. D13.1), and the pending items keep only the questions that are missing, each with its sub-code.
- When closing a pending item: record the decision in the technical document, the default value in the configuration doc (22), the row in the decisions, **delete** the entry from the pending items and its reference in the roadmap (32). A resolved `PRB` goes to the "Resolved problems" table of the decisions. Codes are never reused; the next free ones follow the highest already used.

## When you finish an implementation

There is no chronological log of the implementation: the history is git, with commit messages that say what changed and why. In the same change as the code:

1. **Fix the technical `.md`** when the code decided something the document did not say or contradicts it. The design lives in the documents, and the code is checked against them. Update the matching file **in every language folder** (the file whose "fontes"/"sources" metadata names the code you changed), and run `node doc/pt-BR/scripts/verificar.mjs` (links, metadata, configuration keys, error codes, routes, metrics and use-case codes against the code) and `node doc/pt-BR/scripts/verificar-idiomas.mjs` (parity between the language folders).
2. A **firm decision** goes to the decisions; a **question** or **problem**, to the pending items (sections above and below).
3. **New or changed use cases** go in as a card (`### UC-XXX-NNN`) in the catalogue of both languages: [26a](doc/en-US/26a-implemented-use-cases.md) if a test covers it, [26b](doc/en-US/26b-planned-use-cases.md) if not (with the reason in the card). Each test cites them with `/// UC: UC-XXX-NNN`, and the test is added to the card; a case that gets its first test moves from 26b to 26a.
4. **A new module** goes in the module map of [doc/en-US/04-architecture.md](doc/en-US/04-architecture.md), and its `mod.rs` starts with a `//!` comment saying what it does and which document holds the design.

## Pending items and problems found during implementation

Every implementation that finds something open records it **in the same change**:

- **Design question** (the document does not define it, or there are two ways out): goes to the pending items with the next free `D` code and a proposal. When decided, it goes to the decisions (rules in the "Decisions and pending items" section).
- **Logic problem, risk or limit** (a behaviour that is incomplete, risky or has a known limit): goes to the pending items with the next `PRB-NNN`, stating the effect and the proposal; if a `D` question resolves it, it goes inside it ("Linked problem"). When resolved, it goes to the "Resolved problems" table of the decisions; the code is not reused.
- In the pending items, what **does not depend on anything** stays in section 1 and what depends on another question, problem or step stays in section 2, with "Depends on".
- A deviation from the document is fixed in the technical `.md` itself; if it depends on a decision, it becomes a `D` question.

## Intermittent tests and known issues

A test that fails only sometimes (usually on the slow CI runners) is recorded in [doc/en-US/36-known-issues.md](doc/en-US/36-known-issues.md) / [doc/pt-BR/36-problemas-conhecidos.md](doc/pt-BR/36-problemas-conhecidos.md), in both languages, in the same change.

- **One section per test or issue, with a title.** Right after the title: the **status** (`Open`, `Fixed` or `Reopened`), the date **found** and the date **fixed**; then the test, symptom, cause and fix, and a **History** table (date, event) with every occurrence and attempt. The summary table at the top is kept in sync.
- **Sections are never deleted.** When fixed, set the status to `Fixed`, fill the fixed date and add the history row (what changed and how it was verified). If the problem **comes back**, do not open a new section: set the status to `Reopened`, clear the fixed date, add the history row for the new occurrence and, when it is solved again, add another row and set `Fixed` again with the new date.
- **Ignoring a test (`#[ignore = "..."]`)** is a last resort: it needs a section in the known issues first, and the reason points to it. Removing the `#[ignore]` goes in the same change as the fix and its verification (repeated runs under parallel load, noting how many).
- Do not hide an intermittent failure by raising a timeout or retrying without understanding the cause; if the cause is not identified, say so in the section ("Cause: not identified") and record the next step.

## Use cases

The use cases that guide the tests are the cards of [26a](doc/en-US/26a-implemented-use-cases.md) and [26b](doc/en-US/26b-planned-use-cases.md), each with a `UC-<AREA>-<NNN>` code that the tests cite on a `/// UC: UC-MSG-003` line. The method is explained in [doc/en-US/26-use-cases.md](doc/en-US/26-use-cases.md).

- Codes are **not reused**: a case that ceases to exist stays as `(retirado)`; a new case gets the next free number in the area.
- When you change the behaviour described in a technical `.md`, update the matching use cases **in the same commit**.
- A case only counts as covered with at least one test that cites it, at the level written on the card.

## Language

- **Documentation:** one folder per language under `doc/`, named by the language code: `doc/pt-BR/` (Brazilian Portuguese) and `doc/en-US/` (US English). A folder holds the same files as the others: the number prefix (`NN-`) is the identity of a document and the rest of the name is in the language of the folder (`22-configuracao.md` in `pt-BR`, `22-configuration.md` in `en-US`). **Every change to the documentation must be applied to ALL the language folders in the same change** (content, metadata date, "Sources and evidence" and links); if you cannot translate one, say so in the commit message and record a `PRB-NNN` in the pending items naming the file, never leave a folder silently behind. What is an identifier or output of the program (routes, keys, error codes, JSON fields, `UC-…` codes, the text the server returns today) stays as it is in every language. **The repository root files (`README.md`, this `AGENTS.md`) are in English (en-US)**; `README-pt-BR.md` is the Portuguese version of the README (a language in the file name).
- **Package READMEs** (the ones crates.io and nuget.org show: `connectors/rust/client/README.md`, `connectors/rust/cli/README.md`, `connectors/csharp/README.md`) are in English and link the **en-US** documentation, with the other languages in a note right below: `> Other languages: [Português (Brasil)](…/doc/pt-BR/…) · [all languages](…/doc/README.md)`. Their links are **absolute** (`https://github.com/rafaelpontani/taba/blob/main/…`): relative ones break outside the repository. They cite sections by anchor, never by number ("section 18.10.3"). A change to them only reaches the registries with the next published version.
- **Code:** names of variables, functions, types, modules, fields, tests, configuration keys, routes, error codes and events in **English**, to ease international collaboration. No identifier in Portuguese, not even in tests.
- **Comments and user-facing messages** (API errors, logs, command-line help): in **Portuguese** during development; they will be translated together with the documentation.
- On-disk names (SQLite tables and columns, `_topic.db`, `${topic}` in folders, `state` keys) are also in English, in the documents and in the code.

## Diagrams

Diagrams are generated with [Archify](https://github.com/tt-a1i/archify) from JSON. **The JSON is the source of truth**; the SVG and the HTML are generated outputs.

```
doc/diagrams/
  source/  *.json          source (English, en-US), versioned. This is what you edit.
  i18n/    <lang>.json     one translation table per extra language (pt-BR.json), versioned.
  img/<lang>/ *.svg        exported per language (en-US, pt-BR, ...), versioned. This is what the .md files embed.
  html/<lang>/ *.html      interactive, NOT versioned (only exists with --html).
  scripts/generate.sh      validates, renders and exports the SVG of every language
  scripts/translate.mjs    applies a language table to a source JSON (used by generate.sh)
  scripts/respace.mjs      recomputes the y of the messages of sequence diagrams
  scripts/export-svg.mjs   used by generate.sh (headless Chrome)
```

The documentation in `doc/pt-BR/` embeds the `pt-BR` SVGs (`![…](../diagrams/img/pt-BR/<name>.svg)`), the documentation in `doc/en-US/` and the English `README.md` embed the `en-US` ones; the table of which diagram is used where is in [doc/en-US/24-conventions.md](doc/en-US/24-conventions.md). A spot that still needs a diagram is marked `🖼️ DIAGRAMA PLANEJADO` with its type and goal.

### Rules

- Always edit `doc/diagrams/source/<name>.json`. Never edit the SVG or the HTML: they are overwritten at every generation.
- After editing, run `doc/diagrams/scripts/generate.sh <name>` and commit the JSON **and** the SVGs of every language together. With no name, it generates all of them; `--lang <code>` limits it to one language.
- Run the commands from the repository root. The JSON `meta.output` is relative to it (`doc/diagrams/html/<name>.html`) and must not be changed.
- `--html` keeps the interactive HTML in `doc/diagrams/html/` to open in a browser. It is ignored by git (`.gitignore`).
- The `.archify/` folder is Archify's work area and is also ignored. New diagrams are born there, but only enter the repository after being copied to `doc/diagrams/source/` with a stable name (no date) and `meta.output` pointing to `doc/diagrams/html/`.
- English is the source language: `source/*.json` holds the English text. Every other language is a flat table in `doc/diagrams/i18n/<lang>.json` (`"English string": "translation"`) applied to the display keys (`title`, `label`, `sublabel`, `note`, `items`, `tag`) at generation time. A string missing from a table stays in English and `generate.sh` warns about it, so when you add or reword a string in a source JSON, add the translation to every table. If a translation does not fit (the Archify gates complain), shorten it in the table.
- **To add a language:** create `doc/diagrams/i18n/<lang>.json` (copy `pt-BR.json` and translate the values), run `generate.sh`, `node doc/diagrams/scripts/generate-hash.mjs` and `node doc/diagrams/scripts/generate-hero.mjs`, and embed `../diagrams/img/<lang>/<name>.svg` from that language's docs. Adding a documentation language also means copying a complete `doc/<lang>/` folder (see the Documentation rule in the Language section above). The legend and the Archify viewer interface are fixed in English.
- When you change the behaviour described in a `.md`, update the matching diagram (table in [doc/en-US/24-conventions.md](doc/en-US/24-conventions.md)) and regenerate.

### Script-drawn diagrams (exception)

`ring-hash.svg`, `spread-topic.svg` and `fifo-groups.svg` do not go through Archify (it does not draw rings or matrices) and **have no JSON**: the source is `doc/diagrams/scripts/generate-hash.mjs`. Edit the script constants (nodes, ring positions, messages, matrix) and run `node doc/diagrams/scripts/generate-hash.mjs` (all languages; `--lang=<code>` for one; the strings go through the same `i18n/<lang>.json` tables); `generate.sh` does not generate them. Commit the script **and** the SVGs together. The script does not validate the layout: open the SVG (or take a Chrome capture) and check for overlapping text.

The same applies to the root README hero (`hero.svg` and `hero.gif`): the source is `doc/diagrams/scripts/generate-hero.mjs`. Edit the texts, colours and timeline (`PRODUCERS`, `CONSUMERS`, `MSGS`, `CAPTIONS`) and run `node doc/diagrams/scripts/generate-hero.mjs` (all languages; `--lang=<code>` for one; the texts come from the `i18n/<lang>.json` tables). `hero.svg` is the static frame (instant `STATIC_AT`) and `hero.gif` is what the README embeds; the GIF uses headless Chrome and `ffmpeg` (variable `FFMPEG` if it is not on the PATH). `--svg-only` generates only the SVG. Commit the script **and** both files together.

### TUI screenshots (exception)

`doc/diagrams/img/<lang>/tui-*.png` are real captures of the `taba-cli` interface, used by the READMEs and doc 20b. The source is `doc/diagrams/scripts/capture-tui.sh` (it starts Compose if needed, generates load with `tui-load.mjs` and records each screen with VHS in a container on the Compose network; Docker, Node 22 and `gh`; about 7 minutes, because the charts and the rates need history). Run it from the root (`--lang <code>` for one language) when the interface changes, open the PNGs to check them, and commit them together with the script.

### Prerequisites

- Node.js 22 or later.
- Archify installed as a Claude Code skill (`~/.claude/skills/archify`). Another location: set `ARCHIFY_BIN` to the path of `archify.mjs`.
- Chrome or Edge. Another executable: set `ARCHIFY_CHROME`.

### Editing a sequence diagram

The `messages` array defines the order; `y` is recomputed by `scripts/respace.mjs`.

1. Insert, remove or reorder messages in the array. For a new message, use any `y` (if there are `segments`, use the `y` of a neighbouring message of the same phase).
2. `node doc/diagrams/scripts/respace.mjs doc/diagrams/source/<name>.json` reassigns the `y` values (28 px step, gap between phases), rebuilds the `segments` bands and adjusts `meta.viewBox`.
3. `doc/diagrams/scripts/generate.sh <name>`.

Archify limits (the `showcase` profile) that `scripts/generate.sh` enforces:

- Messages that share horizontal space need at least 28 px between them.
- The `viewBox` aspect ratio may not be taller than about 1.55 (width/height) at a width of 1080. If `scripts/respace.mjs` warns, split the diagram in two. That is how consumption became `consumo` (delivery and ack) plus three scenarios.
- With a width above about 1085, the secondary text gets too small on screen and the check fails.

### Editing an architecture diagram (including C4)

There is no automatic respacing. Adjust the components' `pos` and `size` in the JSON, keeping:

- connected neighbours adjacent, with a gap of `6.5 px × characters + 21 px` for each connection label on the main line;
- `boundaries` only of type `region` or `security-group`;
- `meta.viewBox` omitted in new architecture diagrams.

### Diagnostics

When Archify rejects a diagram, `scripts/generate.sh` prints the codes and messages. The most common:

| Message | Fix |
|---|---|
| `less than 28px apart` | run `scripts/respace.mjs` |
| `leaving no room for the legend` | increase the `viewBox` height (`scripts/respace.mjs` already computes it) |
| `composition/viewport-height` | diagram too tall: split it |
| `composition/desktop-readability` | reduce the `viewBox` width |

Format details: `~/.claude/skills/archify/references/authoring-contract.md` and the schemas in `~/.claude/skills/archify/schemas/`.
