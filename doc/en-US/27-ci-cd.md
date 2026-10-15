---
title: CI/CD and releases
summary: How the repository is validated and published — the GitHub Actions workflows, the Docker images, automatic versioning (semantic-release, candidates and finals), floating tags and what is not yet automated.
type: reference
status: partial
verified-on: 2026-10-13 @ 71183b9
sources: [.github/workflows, .github/actions/node-binary, .github/scripts/set-version.sh, .releaserc.json, Dockerfile, Dockerfile.ci, Dockerfile.cli.ci, docker/validate.sh]
---

# CI/CD and releases

> **Mind the name:** "pipeline" here is the **repository's CI/CD** (GitHub Actions), not the path of messages ([10-publishing.md](10-publishing.md), [12-delivery.md](12-delivery.md)). Read first: [24-conventions.md](24-conventions.md) (commits).

## Overview

| Flow | When | What it does |
|---|---|---|
| **CI** (`ci.yml`) | On every **pull request** | Calls the three builds (server, Rust connector, C# connector), **without a version** |
| **Semantic** (`semantic.yml`) | On every **push** to `release` or `main` | Runs semantic-release: computes the version from the commits and creates the tag and the GitHub Release (nothing, if the commit does not generate a version) |
| **Release** (`release.yml`) | When a **full tag** (`vX.Y.Z` or `vX.Y.Z-rc.N`) is created | Builds and tests with the tag's version, publishes the Docker images and attaches binaries and packages to the Release. **Floating tags do not trigger it** |

The three builds are **reusable workflows** (`workflow_call`), used by CI **and** by the release, with the same logic.

![From pull request to release](../diagrams/img/en-US/ci-cd.svg)

## The workflows (`.github/workflows/`)

| File | Does |
|---|---|
| `ci.yml` | `pull_request`: calls `build-server`, `build-connector-rust` and `build-connector-csharp`. `concurrency` cancels old runs of the same PR |
| `build-server.yml` | **Server**, on **two native runners** (`amd64` on `ubuntu-latest`, `arm64` on `ubuntu-24.04-arm`): `cargo fmt --all --check` and `clippy -D warnings` **only on x86_64** (the code is the same), `cargo test --workspace --locked`, the **static musl binary** (`node-binary` action) uploaded as artifact `taba-node-<arch>` |
| `build-connector-rust.yml` | **Rust connector** (workspace `connectors/rust`), on both runners: `fmt`, `clippy`, `test`, `cargo build --release --locked -p taba-cli` (musl, `musl-tools`); artifact `taba-cli-<arch>` |
| `build-connector-csharp.yml` | **C# connector**: SDKs 6.0 and 8.0, `dotnet test` (without `TABA_URL`, the integration ones are **skipped**), `dotnet pack` of the **three packages** (`Taba.Client`, `.Abstractions`, `.Extensions.DependencyInjection`) as the `nuget-packages` artifact |
| `semantic.yml` | `push` on `main`/`release`: version preview (dry-run), creates the tag (semantic-release) and triggers `release.yml` on it. Only uses the `GITHUB_TOKEN` |
| `release.yml` | The release flow (below), triggered by the tag |

The `version` input parameter of the builds is **empty on a pull request**; on the release it receives the tag's version and `set-version.sh` writes it to the files **only inside the build, without committing**.

## The release (`semantic.yml` and `release.yml`)

0. **`semantic.yml`**: on every push, the `preview` job runs semantic-release in **dry-run** and shows in the run summary the version that would come out (creating nothing); the `semantic` job runs next, **automatically**, if there is a version (the approval is that of the MR that takes the commit to the branch). **semantic-release** reads the commits since the last tag and creates the **tag** (`vX.Y.Z`) and the **GitHub Release**. Without a relevant commit, nothing is created and `release.yml` does not run. Everything uses the `GITHUB_TOKEN` (no PAT): since a tag created with it does not generate a push event, `semantic.yml` itself triggers `release.yml` through `workflow_dispatch` with the ref on the tag (`gh workflow run release.yml --ref vX.Y.Z`; a tag push also triggers it). The tag filters (`v[0-9]+.[0-9]+.[0-9]+` and `…-rc.[0-9]+`) leave the floating tags out, and the `version` job checks the format again.
1. **`version`**: extracts the version from the tag name.
2. **`server`, `connector-rust`, `connector-csharp`** (in parallel): build and **test with the tag's version**.
3. **`plan`**: computes the Docker tags and the floating git tags (below).
4. **`docker`** (Docker Hub): downloads the `taba-node-*` and `taba-cli-*` binaries and assembles **two multi-architecture images** (`linux/amd64`, `linux/arm64`), **compiling nothing**: `rafaelpontani/taba` (`Dockerfile.ci`) and `rafaelpontani/taba-cli` (`Dockerfile.cli.ci`, `ENTRYPOINT taba-cli`).
5. **`release-assets`**: attaches to the GitHub Release the binaries (with the architecture in the name: `x86_64` and `aarch64`) and the NuGet packages.
6. **Floating tags**: moves the tags `v0.1`, `v0` etc.

### Two images, two Dockerfiles

| File | What for |
|---|---|
| `Dockerfile` (root) | **Compiles everything inside the container** (`rust:1-alpine`, static musl build) and produces the final image on Alpine. It is what **anyone uses** for `docker build -t taba .` or `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build` |
| `Dockerfile.ci` | **Does not compile**: copies the already compiled `dist/taba-${TARGETARCH}` binary (with the runner's cache). A build of seconds. Same final image |
| `Dockerfile.cli.ci` | The same, for `taba-cli` |

The final image: **Alpine**, with only the binary and the CA certificates, **unprivileged user** (`taba`, uid 10001), `VOLUME /var/lib/taba`, `EXPOSE 8080 8081 9090`, `ENTRYPOINT ["taba"]`.

## Versioning (semantic-release)

Configured in `.releaserc.json`: `branches: ["main", {"name": "release", "prerelease": "rc"}]`, `tagFormat: "v${version}"`, preset `conventionalcommits`, rule **`breaking → minor`** (while the project is in `0.x`, a breaking change does **not** jump to `1.0.0`).

| Branch | Generates | Example |
|---|---|---|
| `release` | **Candidates** (`rc` pre-release) | `v0.1.0-rc.1`, `rc.2`… |
| `main` | **Final versions** | `v0.1.0`, `v0.1.1`… |

**Which commit generates a version:**

| Commit | Result |
|---|---|
| `feat:`, `fix:`, `perf:` | New version (`rc.N` on `release`, final on `main`) |
| `feat!:`, `BREAKING CHANGE:` | Counts as **minor** (rule `breaking → minor`) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | **No** version |

**Flow:** 1) development goes into `release` (each relevant commit generates the next candidate, with an image); 2) once approved, `release` is promoted to `main` (preferably by a `release → main` PR), which generates the final; 3) the next cycle goes back to `release`: the first `fix` gives `0.1.1-rc.1`, a `feat` gives `0.2.0-rc.1`; **`main` must be merged back into `release`** so they do not diverge; 4) a *hotfix* can go straight on `main`, and then be merged into `release`.

**One version for the whole repository:** server, Rust connector and C# connector move on the **same tag**. `set-version.sh X.Y.Z[-rc.N]` adjusts the server's `Cargo.toml` and `Cargo.lock`, those of the Rust connectors and the `<Version>` of the `.csproj`s. In the repository the files stay at `0.1.0`; run the script by hand to synchronize. The image's `taba --version` reports the tag's version.

**Bootstrap:** semantic-release starts at `1.0.0` with no previous tag. To start at `0.1.0`, the repository needs a stable **seed tag** `v0.0.0` on a commit that is in both branches. **Leaving `0.x`**: remove the `breaking → minor` rule.

### Docker tags and floating tags

Each floating tag **only advances** if the published version is the highest of its scope and its family (candidate or final):

| Published version | Docker tags | Floating git tags |
|---|---|---|
| `0.1.0-rc.2` | `0.1.0-rc.2`, `0.1-rc`, `0-rc`, `rc` | `v0.1-rc`, `v0-rc` |
| `0.1.0` | `0.1.0`, `0.1`, `0`, `latest` | `v0.1`, `v0` |

In `0.x` semver allows a break at every minor: **`0.1` and `v0.1` are the safe pointers**; `0` and `v0` may jump to an incompatible `0.2`.

## Credentials (GitHub secrets)

| Destination | Secret | State |
|---|---|---|
| Docker Hub | `DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN` | ✅ used by `release.yml` |
| nuget.org | `NUGET_API_KEY` | ⬜ planned (today only `dotnet pack`) |
| crates.io | `CARGO_REGISTRY_TOKEN` | ⬜ planned |

nuget.org and crates.io **do not allow deleting a published version**: publishing to them is left for the end, behind a manual approval.

## What is not yet automated

- ⬜ **Integration job** (`connectors/csharp/scripts/integration.sh` and `docker/validate.sh`): **does not run in CI**. They exist and work by hand; the plan was a path-filtered job (it is the most expensive: it builds the image on the runner).
- ⬜ **Publishing** of NuGet and crates (and of the other connectors).
- ⬜ **Image registry on GHCR** (the plan foresaw it; today only Docker Hub).
- ⬜ Automatic verification of the **documentation** (links, `UC-`, parameters, routes): see `doc/pt-BR/scripts/verificar.mjs` below.
- ⬜ The planned `manual.yml` workflow (`workflow_dispatch`).

## Automatic verification of this documentation

`node doc/pt-BR/scripts/verificar.mjs` (from the root) checks, without network and without compiling:

| Check | Fails when |
|---|---|
| Markdown links between files of `doc/pt-BR` and to the repository | The file or the anchor does not exist |
| Metadata | `resumo`, `status`, `verificado-em` or `fontes` is missing at the top, or the "Fontes e provas" section is missing |
| Configuration parameters | A key of `config/params/mod.rs` does not appear in [22-configuration.md](22-configuration.md), or the document cites a nonexistent key |
| Error codes | An `ErrorCode` of the code does not appear in [19-errors-and-limits.md](19-errors-and-limits.md) (or vice versa) |
| Routes | A route registered in the code (`.route(…)` and the cluster map) is not cited in the API documents |
| Use cases | A `UC-…` cited in the documents does not exist in `doc/20a-c` |
| Metrics | A metric of `telemetry/metrics/defs.rs` does not appear in [13-observability.md](13-observability.md) |
| Diagrams | Every `../diagrams/img/…` image exists, and every remaining `🖼️ DIAGRAMA PLANEJADO` block carries `tipo:` and `objetivo:` |

The script currently checks the `doc/pt-BR` folder (the Portuguese keys `resumo`, `fontes`…). A second script, `node doc/pt-BR/scripts/verificar-idiomas.mjs`, checks that the language folders say the same thing: every document exists in every folder (by its number prefix), the metadata has the keys of each language (`summary`, `sources`, `verified-on` in `en-US`), links and anchors resolve inside each folder, and the `UC-…`, `D…` and `PRB-…` codes, code blocks, images and tables are the same on both sides. Both can enter CI as a cheap PR job (Node only).

## Sources and evidence

- `.github/workflows/{ci,build-server,build-connector-rust,build-connector-csharp,release}.yml`, `.github/actions/node-binary/action.yml`, `.github/scripts/set-version.sh`, `.releaserc.json`, `Dockerfile`, `Dockerfile.ci`, `Dockerfile.cli.ci`.
- Use cases: there are no pipeline `UC-` cases; validation is `docker/validate.sh` ([25-testing.md](25-testing.md)).
- History: `21-pipeline-ci-cd.md` (**is partly out of date**: it says "There is no `.github/`" and plans the `integration` job and GHCR, which do not exist).
