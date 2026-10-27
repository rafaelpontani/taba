---
title: Conventions
summary: The rules for whoever changes the code — language, folder structure, Rust code rules, API and configuration conventions, tests, documentation in several languages, diagrams, commits — and the checklist when finishing a change.
type: guide
status: stable
verified-on: 2026-10-26 @ 75012bc
sources: [AGENTS.md, Cargo.toml, crates/taba-node/src, crates/taba-node/tests, .releaserc.json, doc/diagrams]
---

# Conventions

> This is the human guide to the rules that the root `AGENTS.md` gives to AI agents. When in doubt, `AGENTS.md` and the code win.

## Language

| What | Language |
|---|---|
| **Documentation** | One folder per language under `doc/`, named by the language code: `doc/pt-BR/` (Brazilian Portuguese) and `doc/en-US/` (US English). **Every change goes to all the folders** (see [Documentation in several languages](#documentation-in-several-languages)) |
| **Names in the code**: variables, functions, types, modules, fields, **tests**, configuration keys, routes, **error codes**, events | **English**. No identifier in Portuguese, **not even in tests** |
| **Comments** and **user-facing messages** (API error, log, command-line help) | Portuguese during development (they will be translated together with the documentation) |
| **On-disk names**: SQLite tables and columns, `_topic.db`, `${topic}`, `state` keys | English |
| **Repository root** (`README.md`, `AGENTS.md`) | **en-US** |

## Documentation in several languages

- Each language lives in its own folder, `doc/<language>/` (today `pt-BR` and `en-US`), with the **same files**: the number prefix (`NN-`) is the identity of the document, and the rest of the name is in the language of the folder (`22-configuracao.md` in `pt-BR`, `22-configuration.md` in `en-US`). `README.md`, `llms.txt` and the `scripts/` follow the same rule.
- **Every change to the documentation must be applied to all the language folders** in the same change: the content, the metadata (`verified-on` / `verificado-em`), the "Sources and evidence" section and the links. A folder that falls behind makes the documentation contradict itself. If you do not speak a language, change the others anyway and mark the gap in [31-pending-items.md](31-pending-items.md) (a `PRB-NNN` naming the file) instead of leaving it silent.
- What is **identifier or example output** stays as it is in all languages: routes, keys, error codes, JSON fields, `UC-…` codes and what the server returns today (the `message` of an error and the log text are still in Portuguese until the code is translated).
- The generated files (`26a`, `26b`, `30`, `31`) are produced per language by the scripts of each folder; edit the source, not the output.
- To add a language: copy a complete folder to `doc/<language>/`, translate it, add it to the table of [`doc/README.md`](../README.md) and to the diagram translation tables ([Diagram index](#diagram-index)).

## Folder structure

```
taba/
  Cargo.toml                    server workspace (one crate)
  crates/taba-node/             the server: `taba_node` library + `taba` binary
    src/                        modules (map in 04-architecture.md)
    tests/                      integration and cluster tests
      common/                   TestNode, TestCluster, real.rs, otlp.rs
      uc_coverage.rs            checks the use-case codes
  connectors/                   connectors (one folder per language; own workspace for Rust)
    rust/{client,cli}/  csharp/  README.md
  docker/                       cluster.sh, validate.sh, seed-demo.sh, haproxy.cfg, test-cpu.sh
  Dockerfile  Dockerfile.ci  docker-compose.yml
  examples/csharp-console/      example of using the C# connector
  doc/                          README.md, diagrams/, one folder per language
  doc/pt-BR/                    the documentation in Brazilian Portuguese
  doc/en-US/                    this documentation
  .github/                      workflows, actions and scripts
```

**Module** = folder with `mod.rs` + `tests.rs` side by side; the `mod.rs` **starts with a `//!` comment** saying what the module does and which document holds the design, and **ends** with `#[cfg(test)] mod tests;`. Unit tests **live in the sibling file**, not inside the code.

## Code rules (Rust)

- **Direction of dependencies** `core ← storage ← engine ← cluster ← http ← node`: each module knows only those on its left. `core` imports no one ([04-architecture.md](04-architecture.md)). A **trait** goes in the **lowest** layer the caller needs, together with the real and test implementations.
- **Pass messages, do not share state.** The single writer per database is a command queue executed one at a time; prefer that to a scattered `Arc<Mutex<…>>`.
- **Never hold a lock across an `.await`.**
- **Never call, while holding a guard, anything that locks the same `Mutex` again.** `std::sync::Mutex` is not reentrant: the thread blocks on itself, with no error and no log. A guard created in the scrutinee of a `match`, `if let`, `while let` or `for` lives until the end of that block, so take the value out first (`let known = self.lock().get(k).cloned();`) and use it afterwards. When two locks are needed, take them in the same order everywhere (peers map, then the peer's state). `clippy::significant_drop_in_scrutinee` is on for the workspace and CI fails on it ([36-known-issues.md](36-known-issues.md#a-channel-accept-that-locked-the-peers-twice), D179).
- **SQLite outside the async executor** (`rusqlite` is synchronous): dedicated thread or `spawn_blocking`; long reads use their own connections in WAL.
- **No `unwrap()` outside tests.** Errors use their own type (`TabaError`) that maps directly to the code catalogue.
- **Clean cancellation:** every background task receives a `CancellationToken` (it is what drain uses).
- **Everything that touches the outside world sits behind a trait** (`Transport`, `Clock`), so tests inject network and time.
- **Comment like the neighboring code** (density and style); do not introduce new patterns without need.

## API conventions

Detailed in [15-api-overview.md](15-api-overview.md). In short:

- `camelCase` JSON, RFC 3339 UTC dates with milliseconds, durations in **integer seconds**, lowercase **GUID v7** IDs.
- **Every error is an `ErrorCode` of the catalogue** (`core/error`) with a **fixed status**. **A new code in the catalogue requires its line in [19-errors-and-limits.md](19-errors-and-limits.md) and its case in the test table.**
- Unknown field in the request → `400 validation.unknown-field`; **in the response, ignore what you do not know**.
- The status goes only in the HTTP line; `Retry-After` only in the header.
- New route: define permission, rate limit category and status; update [16](16-client-api.md)/[17](17-operations-api.md)/[18](18-cluster-api.md).

## Configuration conventions

`config/params/mod.rs` is the **table of keys and defaults** (single source). **A new parameter goes in the table _and_ in `Config`**; a test checks that every parameter in the table is read. An unknown key in the `.yml` is refused. Variable: `TABA_` + key in uppercase, `.` and `-` → `_`. A duration always with a unit. Document it in [22-configuration.md](22-configuration.md) (and the effective default, if the code applies one the table does not have).

## Tests

Summary (details in [25-testing.md](25-testing.md)):

- Each test **cites the use cases** it covers on a `/// UC: UC-XXX-NNN` line right above the `#[test]`. `uc_coverage` fails if the code does not exist.
- Unit test in the sibling `tests.rs`; integration in `tests/`; cluster in `tests/` with `TestCluster`; connectors have their own tests that also cite `UC`.
- **Test names in English** and descriptive (`creating_a_queue_returns_201_with_location_and_the_defaults`).

## Everyday commands

```bash
cargo build
cargo test                                            # unit and integration
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --test uc_coverage -- --nocapture          # use-case scoreboard
cargo run -- init --out ./secrets                     # certificate and root token

cd connectors/rust && cargo test --workspace --all-features && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
cd connectors/csharp && dotnet test Taba.Client.Tests

docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d   # 3-node cluster, local image
docker/validate.sh                                    # end-to-end validation (≈ 5 min)
```

**Before closing** a change that affects **cluster, channel, Compose or connectors**, run `docker/validate.sh`.

### Environment (known pitfalls)

- **Disk:** `target/` grows fast. The `dev` profile keeps only line tables (`debug = "line-tables-only"`), but `cargo clean -p taba-node` deletes old test binaries (every change leaves a new one). With little space, use `CARGO_INCREMENTAL=0` or delete `target/debug/incremental`. "no space on device" from `rustc` and `LNK1180` from `link.exe` are this.
- **SQLite:** `rusqlite` compiles the bundled SQLite (`bundled`): the build needs a **C compiler** (MSVC, `gcc` or `clang`). On the Windows `-gnu` target, `cc` must not find the Cygwin `gcc`.
- **Windows with Git Bash:** the coreutils `link.exe` hides the MSVC one. If linking fails, use `cargo +stable-x86_64-pc-windows-gnu …` (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`). Docker commands with paths need `MSYS_NO_PATHCONV=1`.
- **Executable in use:** on Windows, `cargo test` fails with "Access denied" when removing `target/debug/taba.exe` if a compiled node is still running.

## When you finish an implementation

In the **same change** as the code:

1. **Fix the technical `.md`** when the code decided something the document did not say or contradicts it. The design lives in the documents, and the code is checked against them. **In all the language folders.**
2. **Firm decision** → [30-decisions.md](30-decisions.md) (in the original design documents: `08a`); **question** or **problem** → [31-pending-items.md](31-pending-items.md) (`08b`).
3. New or changed **use cases** go in as a card (`### UC-XXX-NNN`) in [26a](26a-implemented-use-cases.md) (with a test) or [26b](26b-planned-use-cases.md) (without one), in both languages, and each test cites them with `/// UC:`. A case without a test stays in 26b, with the reason in the card itself. `cargo test --test uc_coverage` checks the cited codes.
4. A **new module** goes in the map of [04-architecture.md](04-architecture.md), and its `mod.rs` starts with `//!`.
5. **New parameter**: table, `Config`, [22-configuration.md](22-configuration.md). **New error**: catalogue, [19-errors-and-limits.md](19-errors-and-limits.md), test.

### Pending items and problems found

Every implementation that finds something open records it **in the same change**:

- **Design question** (the document does not define it, or there are two ways out): next free `D` question, with a proposal. When decided, it goes to the decisions.
- **Logic problem, risk or limit** (a behavior that is incomplete, risky or has a known limit): next `PRB-NNN`, stating the **effect** and the **proposal**; if a `D` question resolves it, it goes inside it ("Linked problem"). When resolved, it goes to the "resolved problems" with the date.
- In [31-pending-items.md](31-pending-items.md): what **does not depend on anything** in section 1; what depends on another question, problem or step, in section 2 with "Depends on".
- **The codes (`D…`, `PRB-…`, `UC-…`) are never reused.**

## Use cases

Format `UC-<AREA>-<NNN>` (three letters and three digits). A case that ceases to exist stays `(retirado)`; a new one gets the next free number in the area. When you change the behavior described in a `.md`, update the cases **in the same commit** (and the index total). A case only counts as covered with at least one test that cites it, **at the level of the "Level" column** ([26-use-cases.md](26-use-cases.md)).

## Diagrams

Generated with **Archify** from JSON. **The source of truth is the JSON**; SVG and HTML are outputs.

```
doc/diagrams/
  source/  *.json           source (English, en-US), versioned: this is what you edit
  i18n/    <lang>.json      one translation table per extra language (pt-BR.json)
  img/<lang>/ *.svg         exported per language, versioned: what the .md files embed
  html/<lang>/ *.html       interactive, NOT versioned (only with --html)
  scripts/generate.sh       validates, renders and exports the SVG of every language
  scripts/respace.mjs       recomputes the y of the messages of sequence diagrams
```

- The `en-US` documentation embeds the **`en-US`** diagrams (`../diagrams/img/en-US/<name>.svg`), whose text is the English one of `source/*.json`; the `pt-BR` documentation embeds `img/pt-BR/`. A new language gets its `i18n/<language>.json` and its `img/<language>/` ([AGENTS.md](../../AGENTS.md)).
- **Never edit SVG or HTML.** Edit `source/<name>.json` and run `doc/diagrams/scripts/generate.sh <name>` (without a name, generates all), from the **root**; commit the JSON **and** the SVGs of every language together.
- **Sequence diagrams:** insert/remove/reorder in the `messages` array (any `y` on the new one), run `node doc/diagrams/scripts/respace.mjs doc/diagrams/source/<name>.json`, then `generate.sh`. Limits of the `showcase` profile: ≥ 28 px between messages that share space; `viewBox` aspect ratio ≤ ~1.55 (if exceeded, **split the diagram**); width ≤ ~1085 px.
- **Architecture diagrams (incl. C4):** no automatic respacing; adjust `pos` and `size` keeping connected neighbors adjacent (room of `6.5 px × characters + 21 px` per connection label), `boundaries` only `region`/`security-group` and `meta.viewBox` omitted on new ones.
- **Script-drawn** (no JSON): `ring-hash.svg`, `spread-topic.svg`, `fifo-groups.svg` (`generate-hash.mjs`) and the README hero (`generate-hero.mjs`, with headless Chrome and `ffmpeg`).
- **Screenshots** (PNG, no JSON): `tui-cluster`, `tui-topics`, `tui-queue-chart` and `tui-node` are real captures of the `taba-cli` interface, one per language. `doc/diagrams/scripts/capture-tui.sh` starts Compose, generates load with `tui-load.mjs` and records with VHS in a container (Docker and Node ≥ 22; about 7 minutes, because the charts need history). Regenerate them when the interface changes.
- **Prerequisites:** Node ≥ 22, Archify (`~/.claude/skills/archify` or `ARCHIFY_BIN`), Chrome/Edge (`ARCHIFY_CHROME`).
- When you change the behavior described in a `.md`, **update the matching diagram** and regenerate. A new diagram spot, still to be drawn, is marked with a `🖼️ DIAGRAMA PLANEJADO` block with `tipo:` and `objetivo:` (`verificar.mjs` checks it).

### Diagram index

Generated by surveying the `![…](../diagrams/img/…)` of this folder. A diagram with "—" in the last column is not used by this documentation (see the note below the table).

| Diagram | Type | Source | Used in |
|---|---|---|---|
| `ring-before-after` | architecture | `source/ring-before-after.json` | [06](06-ring-and-replication.md) |
| `ring-hash` | script | `doc/diagrams/scripts/generate-hash.mjs` | [06](06-ring-and-replication.md) |
| `disk-storage` | architecture | `source/disk-storage.json` | [08](08-storage.md) |
| `write-architecture` | architecture | `source/write-architecture.json` | [10](10-publishing.md) |
| `c4-level0-context` | architecture | `source/c4-level0-context.json` | [01](01-overview.md) |
| `c4-level1-containers` | architecture | `source/c4-level1-containers.json` | — |
| `layers` | architecture | `source/layers.json` | [04](04-architecture.md) |
| `ci-cd` | workflow | `source/ci-cd.json` | [27](27-ci-cd.md) |
| `consumption` | sequence | `source/consumption.json` | [12](12-delivery.md) |
| `consumption-scenario1-local-list` | sequence | `source/consumption-scenario1-local-list.json` | [12](12-delivery.md) |
| `consumption-scenario2-other-node` | sequence | `source/consumption-scenario2-other-node.json` | [12](12-delivery.md) |
| `consumption-scenario3-waiting` | sequence | `source/consumption-scenario3-waiting.json` | [12](12-delivery.md) |
| `topic-decision` | workflow | `source/topic-decision.json` | [09](09-topics.md) |
| `distributor-publishing` | sequence | `source/distributor-publishing.json` | [09](09-topics.md) |
| `entities` | architecture | `source/entities.json` | [03](03-concepts.md) |
| `message-states` | lifecycle | `source/message-states.json` | [12](12-delivery.md) |
| `node-states` | lifecycle | `source/node-states.json` | [07](07-cluster.md) |
| `node-register-failure` | sequence | `source/node-register-failure.json` | [07](07-cluster.md) |
| `fifo-groups` | script | `doc/diagrams/scripts/generate-hash.mjs` | [09](09-topics.md) |
| `fifo-publishing` | sequence | `source/fifo-publishing.json` | [09](09-topics.md) |
| `volatile-queue-creation` | sequence | `source/volatile-queue-creation.json` | [09](09-topics.md) |
| `volatile-queue-deletion` | sequence | `source/volatile-queue-deletion.json` | [09](09-topics.md) |
| `volatile-queue-manager-change` | sequence | `source/volatile-queue-manager-change.json` | [09](09-topics.md) |
| `hero` | script | `doc/diagrams/scripts/generate-hero.mjs` | — |
| `compose-deployment` | architecture | `source/compose-deployment.json` | [02](02-quick-start.md) |
| `node-join` | sequence | `source/node-join.json` | [07](07-cluster.md) |
| `invisibility` | sequence | `source/invisibility.json` | — |
| `mind-map` | architecture | `source/mind-map.json` | [29](29-mind-map.md) |
| `mind-map-cluster` | architecture | `source/mind-map-cluster.json` | [29](29-mind-map.md) |
| `mind-map-disk` | architecture | `source/mind-map-disk.json` | [29](29-mind-map.md) |
| `mind-map-delivery` | architecture | `source/mind-map-delivery.json` | [29](29-mind-map.md) |
| `mind-map-write` | architecture | `source/mind-map-write.json` | [29](29-mind-map.md) |
| `mind-map-invisibility` | architecture | `source/mind-map-invisibility.json` | [29](29-mind-map.md) |
| `mind-map-observability` | architecture | `source/mind-map-observability.json` | [29](29-mind-map.md) |
| `mind-map-sync` | architecture | `source/mind-map-sync.json` | [29](29-mind-map.md) |
| `mind-map-topics` | architecture | `source/mind-map-topics.json` | [29](29-mind-map.md) |
| `node-overview` | architecture | `source/node-overview.json` | [04](04-architecture.md) |
| `observability-flow` | dataflow | `source/observability-flow.json` | [13](13-observability.md) |
| `test-pyramid` | architecture | `source/test-pyramid.json` | [25](25-testing.md) |
| `node-ports` | architecture | `source/node-ports.json` | [15](15-api-overview.md) |
| `write-reception` | sequence | `source/write-reception.json` | [10](10-publishing.md) |
| `process-restart` | sequence | `source/process-restart.json` | [07](07-cluster.md) |
| `node-removal` | sequence | `source/node-removal.json` | [07](07-cluster.md) |
| `excluded-node-return` | sequence | `source/excluded-node-return.json` | [07](07-cluster.md) |
| `sync-phases` | workflow | `source/sync-phases.json` | [07](07-cluster.md) |
| `spread-topic` | script | `doc/diagrams/scripts/generate-hash.mjs` | [06](06-ring-and-replication.md) |
| `security-zones` | architecture | `source/security-zones.json` | [14](14-security.md) |
| `tui-cluster` | screenshot | `doc/diagrams/scripts/capture-tui.sh` | [20b](20b-cli-ui.md) |
| `tui-topics` | screenshot | `doc/diagrams/scripts/capture-tui.sh` | [20b](20b-cli-ui.md) |
| `tui-queue-chart` | screenshot | `doc/diagrams/scripts/capture-tui.sh` | [20b](20b-cli-ui.md) |
| `tui-node` | screenshot | `doc/diagrams/scripts/capture-tui.sh` | [20b](20b-cli-ui.md) |

> `hero` is used by the root `README.md`, and the `tui-*` screenshots too. `c4-nivel1-conteineres` and `invisibilidade` are left over from the original design documents and **are out of date** (they describe the "internal" API and the invisible list on disk, which the code does not have): they were replaced by `no-visao-geral`, `estados-mensagem` and `consumo`.

## Commits and versions

- **Conventional Commits** in short English or Portuguese, with scope: `feat(connectors/csharp): …`, `fix(cluster): …`. The type decides the version ([27-ci-cd.md](27-ci-cd.md)): `feat`, `fix` and `perf` generate a version; `docs`, `chore`, `ci`, `refactor` and `test` **do not**. `feat!:` or `BREAKING CHANGE:` counts as `minor` while the project is in `0.x`.
- The message says **what changed and why** (history is git; there is no chronological log of the implementation).
- A single `vX.Y.Z` tag for the whole repository (server and connectors move together). Nobody creates a tag by hand.
- **Taba is in production** and every published version is permanent (crates.io and nuget.org do not delete versions; Releases are immutable). The APIs, the configuration keys, the on-disk formats, the S3 backup layout and the connectors' public API are contracts: an incompatible change is marked as breaking and describes the upgrade in the documentation.
- **Flow:** a branch from `release` → pull request to `release` (CI checks required, merge commit) → candidate `rc` published → validation (`docker/validate.sh`; the chart and C# connector tests when they changed) → pull request `release → main` ("Release vX.Y.Z") → final → pull request `main → release`. Nobody pushes directly to `main` or `release`: both are protected by rulesets ([27-ci-cd.md](27-ci-cd.md#branch-and-tag-flow)). A correction to a final follows the same path.

## Sources and evidence

- `AGENTS.md` (the rules for code, language, end of implementation, pending items, use cases and diagrams), `Cargo.toml` (`dev` profile), `.releaserc.json`, `18-estrutura-codigo.md` (17.3 to 17.7), `doc/diagrams/scripts/`.
- Use cases: UC-CFG (parameter table read), UC-CNV (errors and statuses); see [26a](26a-implemented-use-cases.md).
