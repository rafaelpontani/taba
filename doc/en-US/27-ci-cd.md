---
title: CI/CD and releases
summary: How the repository is validated and published — the GitHub Actions workflows, the order of the release (nothing public before everything is green), automatic versioning (semantic-release, candidates and finals), floating tags by digest, publishing to Docker Hub, GHCR, crates.io and nuget.org, and how anyone verifies that an artifact came out of this pipeline.
type: reference
status: stable
verified-on: 2026-10-27 @ ada9ff1
sources: [.github/workflows, .github/actions/node-binary, .github/scripts/set-version.sh, .github/dependabot.yml, .releaserc.json, .github/actions/registry-login, .github/actions/provenance, Dockerfile, Dockerfile.ci, connectors/csharp/Directory.Build.props, connectors/rust/Cargo.toml, SECURITY.md]
---

# CI/CD and releases

> **Mind the name:** "pipeline" here is the **repository's CI/CD** (GitHub Actions), not the path of messages ([10-publishing.md](10-publishing.md), [12-delivery.md](12-delivery.md)). Read first: [24-conventions.md](24-conventions.md) (commits).

## Overview

| Flow | When | What it does |
|---|---|---|
| **CI** (`ci.yml`) | **Pull request** to `main`/`release` (and manual, for a branch without a PR). Not on `push`: with a PR open it would run everything twice | The three builds (server, Rust connector, C# connector) **without a version**, the static test of the Helm chart and the lint of the workflows. **Publishes nothing** |
| **Release** (`release.yml`) | **Push** to `main` (finals) or `release` (candidates) | The same builds; if semantic-release finds a version, publishes everything **in the order below**, with the tag created only after everything is green |
| **Scorecard** (`scorecard.yml`) | Push to `main`, weekly and on every protection change | The public OpenSSF Scorecard score (the repository's security practices) |
| **CodeQL** (`codeql.yml`) | Push to `main`/`release`, pull request to them and weekly | Static analysis (SAST) of the Rust, C# and workflow code; findings in the Security tab |

The three builds are **reusable workflows** (`workflow_call`), used by CI **and** by the release, with the same logic.

![From pull request to release](../diagrams/img/en-US/ci-cd.svg)

## Branch and tag flow

```
feature/*  ── PR ──▶ ci.yml: build + tests (nothing is published, no tag)
    │
    └──PR──▶ release ── release.yml ──▶ fixed tag  v0.1.0-rc.28        (immutable, never moves)
                │                         floating git: v0.1-rc, v0-rc   (move to the new commit)
                │                         docker:  0.1.0-rc.28 + 0.1-rc, 0-rc, rc  (same digest)
                │
                └──PR──▶ main ── release.yml ──▶ fixed tag  v0.1.0
                           │                       floating git: v0.1, v0
                           │                       docker:  0.1.0 + 0.1, 0, latest  (same digest)
                           └──back-merge──▶ release
```

1. Development enters `release` by PR: each `feat`/`fix`/`perf` commit produces the next candidate, with an image.
2. Once approved, `release` is promoted to `main` by a `release → main` PR, which produces the final.
3. `main` is merged back into `release` so they do not diverge; the next cycle starts there.

## The workflows (`.github/workflows/`)

| File | Does |
|---|---|
| `ci.yml` | Calls `build-server`, `build-connector-rust`, `build-connector-csharp` and the builds of the Java, Node.js and Python connectors and of the OMB driver (`build-connector-omb.yml`); `connector-contract` job (`contract-connectors.yml`, only on pull requests to `release`); `helm` job (`deploy/helm/tests/run.sh static`); `rust-audit` job (`cargo audit` on both `Cargo.lock` files: fails on a RustSec advisory); `workflows` job (`actionlint` and `zizmor`, fails from `medium` up); `plans` job (fails when `doc/plans/` has a file: plans never reach `release` or `main`). `concurrency` cancels old runs of the same ref |
| `build-server.yml` | **Server**, on **two native runners** (`amd64` on `ubuntu-latest`, `arm64` on `ubuntu-24.04-arm`): `cargo fmt --all --check` and `clippy -D warnings` **only on x86_64**, `cargo test --workspace --locked`, the **static musl binary** (`node-binary` action) uploaded as the `taba-node-<arch>` artifact |
| `build-connector-rust.yml` | **Rust connector** (`connectors/rust` workspace), on both runners: `fmt`, `clippy`, `test`, `cargo package --workspace` (what crates.io would receive), `cargo build --release -p taba-cli` (musl); `taba-cli-<arch>` artifact |
| `contract-connectors.yml` | **Contract tests of the five connectors** against a real node: builds the server image from the commit, starts one node (the one-node profile) and runs `connectors/contract/run.sh` (each connector in the container of its oldest runtime, then the five reading each other's messages; [21-connectors.md](21-connectors.md#the-contract-tests)). Called by `ci.yml` on pull requests to `release`, or by hand. Publishes nothing |
| `build-connector-csharp.yml` | **C# connector**: SDKs 6.0 and 8.0, `dotnet test` (without `TABA_URL`, the integration tests are **skipped**), `dotnet pack` of the **three packages** with the commit and the Release link; `nuget-packages` artifact (`.nupkg` and `.snupkg`) |
| `chaos.yml` | Every night (and by hand): a sweep of 200 seeds of the chaos run (`tests/chaos.rs`, [37-guarantees.md](37-guarantees.md)), a different range each day. Outside the required checks; a failing seed is printed in the log |
| `release.yml` | The release (below) |
| `scorecard.yml` | `ossf/scorecard-action`; the result goes to the Security tab and to the README badge |
| `codeql.yml` | CodeQL for `rust`, `csharp` and `actions`, all with `build-mode: none` (independent of the build toolchains) |

The builds' `version` input is **empty** when there is no release; with a version, `set-version.sh` writes it to the files **only inside the build, without committing**. Every build exports `TABA_GIT_SHA`: `taba --version` and `taba-cli --version` show the commit.

## The release (`release.yml`)

Principle: **nothing public before everything is green, and everything public points to the same commit** (`github.sha`). `concurrency: release-<ref>` without cancellation: a new push waits for the previous one to finish.

| # | Job | What it does |
|---|---|---|
| 1 | `plan` | semantic-release in **dry-run** computes the version. Without a version, only the builds run. Computes the Docker tags and the floating git tags (the new version enters the comparison, because its tag does not exist yet) |
| 2 | `server`, `connector-rust`, `connector-csharp` | Build and **test with the version**. Run on every push, even without a version |
| 3 | `chart` | `set-version.sh` writes the exact `version` and `appVersion` to `Chart.yaml`; `run.sh static`; `helm package` |
| 4 | `images` | Builds `taba` and `taba-cli` (the two targets of `Dockerfile.ci`) for `linux/amd64` and `linux/arm64`, **compiling nothing**, with SLSA provenance (`mode=max`), SBOM and OCI labels (`revision` = commit). Pushes **only by digest, without a tag**, to Docker Hub and GHCR; attests (`attest-build-provenance`) and signs (cosign keyless) each digest |
| 5 | `tag` | Only now semantic-release creates the **tag** on `github.sha` and the **GitHub Release as a draft**. Fails if the version is not the planned one or if the tag does not point to the commit |
| 6 | `release-assets` | Binaries (with the architecture in the name), `.nupkg`, `.snupkg`, the chart `.tgz` and `SHA256SUMS`, all attested, on the draft, plus `taba-<version>.intoto.jsonl` (the provenance bundle as a Release file, which the OpenSSF Scorecard recognizes) |
| 7 | `crates`, `nuget`, `chart-publish` | crates.io and nuget.org by **Trusted Publishing** (OIDC); the chart at `oci://ghcr.io/rafaelpontani/charts`, attested and signed |
| 8 | `finalize` | The Release leaves draft: `--prerelease` on candidates, `--latest` only on the highest final |
| 9 | `floating` | Creates the Docker tags (exact and floating) with `docker buildx imagetools create` **pointing to the digest** (no rebuild) and moves the floating git tags |
| 10 | `verify` | Checks from the outside: every git tag points to the commit; every Docker tag, on both registries, is the same digest, with the right `revision` label; `cosign verify` and `gh attestation verify` pass; the Release `SHA256SUMS` matches |

If the branch moves between `plan` and `tag`, semantic-release refuses ("local branch behind remote"), the `tag` job fails and the next run, already queued, releases the new commit. Nothing public is left behind: the images have no tag yet.

`workflow_dispatch` (on `main` or `release`) with `dry_run` (default `true`) runs up to the image build **without pushing** and stops before `tag`: it is the rehearsal of a release.

### Two images, one CI Dockerfile

| File | What for |
|---|---|
| `Dockerfile` (root) | **Builds everything inside the container** (`rust:1-alpine`, static musl build; both bases **pinned by digest**, as in `connectors/rust/cli/Dockerfile`) and produces the final image on Alpine. It is what **anyone uses** for `docker build -t taba .` or `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build` |
| `Dockerfile.ci` | **Does not compile**: one target per image (`taba` copies `dist/taba-${TARGETARCH}`, `taba-cli` copies `dist/taba-cli-${TARGETARCH}`), binaries already built with the runner's cache. Both start from the same `base` stage, with `alpine:3` **pinned by digest** in a single place |

**Keeping versions current without editing a SHA by hand:** GitHub does not accept a variable in `uses:`, so each action is pinned by SHA where it is used. The repeated ones (registry login, attestation and signature) live in the local actions `.github/actions/registry-login` and `.github/actions/provenance`, and Dependabot updates every occurrence in one grouped PR. The base digest only exists in `Dockerfile.ci`; its update comes as `fix` and produces a new version (`rc` on `release`), so the published image gets the base's security fixes without waiting for a feature. The `apk` packages are always the latest at build time. Dependabot waits 7 days (`cooldown`) before proposing a freshly published version.

The final image: **Alpine**, with only the binary and the CA certificates, **unprivileged user** (`taba`, uid 10001), `VOLUME /var/lib/taba`, `EXPOSE 8080 8081 9090`, `ENTRYPOINT ["taba"]`.

## Versioning (semantic-release)

Configured in `.releaserc.json`: `branches: ["main", {"name": "release", "prerelease": "rc"}]`, `tagFormat: "v${version}"`, preset `conventionalcommits`, rule **`breaking → minor`** (while the project is in `0.x`, a breaking change does **not** jump to `1.0.0`) and `draftRelease: true` (the Release is born as a draft and only `finalize` publishes it).

| Branch | Produces | Example |
|---|---|---|
| `release` | **Candidates** (`rc` pre-release) | `v0.1.0-rc.1`, `rc.2`… |
| `main` | **Final versions** | `v0.1.0`, `v0.1.1`… |

**Which commit produces a version:**

| Commit | Result |
|---|---|
| `feat:`, `fix:`, `perf:` | New version (`rc.N` on `release`, final on `main`) |
| `feat!:`, `BREAKING CHANGE:` | Counts as **minor** (rule `breaking → minor`) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | **No** version (only the builds run) |

**One version for the whole repository:** server, Rust and C# connectors and the chart move on the **same tag**. `set-version.sh X.Y.Z[-rc.N]` adjusts the server's `Cargo.toml` and `Cargo.lock`, those of the Rust connectors (and `taba-cli`'s `taba-client = "=X"` dependency), the `<Version>` of the `.csproj` files and the `version`/`appVersion` of `Chart.yaml`. In the repository the files stay at `0.1.0`.

**Bootstrap:** semantic-release starts at `1.0.0` without a previous tag. To start at `0.1.0`, the repository needs a **stable seed tag** `v0.0.0` on the **first commit of the repository** (`git rev-list --max-parents=0 HEAD`), which is on both branches; without it, the `plan` job stops the release. **Leaving `0.x`**: remove the `breaking → minor` rule.

### Docker tags and floating tags

Only the **fixed** tag (`vX.Y.Z`, `vX.Y.Z-rc.N`) is immutable. The floating ones are pointers and each one **only advances** if the published version is the highest in its scope and its family (candidate or final):

| Published version | Docker tags | Floating git tags |
|---|---|---|
| `0.1.0-rc.2` | `0.1.0-rc.2`, `0.1-rc`, `0-rc`, `rc` | `v0.1-rc`, `v0-rc` |
| `0.1.0` | `0.1.0`, `0.1`, `0`, `latest` | `v0.1`, `v0` |

- A `0.1.5` hotfix published after `0.2.0` moves `0.1`/`v0.1`, but **not** `0`, `v0` or `latest`.
- Candidates and finals do not mix: an `rc` never moves `0.1`/`latest`; a final never moves `0.1-rc`.
- In `0.x` semver allows breaking at every minor: **`0.1` and `v0.1` are the safe pointers**; `0` and `v0` may jump to an incompatible `0.2`.
- Every Docker tag of a version is the **same digest** (created by `imagetools create`, not by a new build): the digest's provenance and signature hold for all of them.

## Publishing and metadata

| Destination | What | How | Turned on when |
|---|---|---|---|
| Docker Hub | `rafaelpontani/taba`, `rafaelpontani/taba-cli` | Secrets `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` | Always |
| GHCR | `ghcr.io/rafaelpontani/taba`, `taba-cli` and `charts/taba` | `GITHUB_TOKEN` (`packages: write`) | Always |
| GitHub Release | Binaries, packages, chart, `SHA256SUMS` | `GITHUB_TOKEN` | Always |
| crates.io | `taba-client`, then `taba-cli` | Trusted Publishing (`rust-lang/crates-io-auth-action`); the first publish of each crate uses the `CARGO_REGISTRY_TOKEN` secret | Variable `PUBLISH_CRATES=true` |
| nuget.org | `Taba.Client`, `.Abstractions`, `.Extensions.DependencyInjection` (+ `.snupkg`) | Trusted Publishing (`NuGet/login`) | Variable `NUGET_USER` (the account that owns the policy) |

nuget.org and crates.io **do not allow deleting a published version**; both jobs are idempotent (they skip a version that already exists) and candidates go out as semver pre-releases, which `cargo add`/`dotnet add` do not pick unless asked. The publishing jobs run in the **`release`** environment (restrict it to the `main` and `release` branches; manual approval of finals is optional).

**Package metadata:**

- **NuGet** (`connectors/csharp/Directory.Build.props`): `RepositoryUrl`, `PackageProjectUrl`, `PackageReadmeFile`, deterministic build, SourceLink, `.snupkg`; the build passes `RepositoryCommit` (becomes `<repository commit>` in the `.nuspec`) and `PackageReleaseNotes` (the Release link).
- **Cargo** (`connectors/rust/*/Cargo.toml`): `repository`, `homepage`, `readme`, `keywords`, `categories`; `cargo publish` records the commit in `.cargo_vcs_info.json`. Because `set-version.sh` changes files, publishing uses `--allow-dirty` and the file says `"dirty": true`; the verifiable link is Trusted Publishing, which crates.io shows with the link to the run.
- **Images**: labels `org.opencontainers.image.{source,url,revision,version,created,licenses}`.

## Trust: how to verify an artifact

Anyone can check, without trusting the repository's word, that an artifact came out of this workflow and from which commit:

```bash
gh attestation verify oci://ghcr.io/rafaelpontani/taba:0.1.0 -R rafaelpontani/taba
cosign verify ghcr.io/rafaelpontani/taba:0.1.0 \
  --certificate-identity-regexp '^https://github.com/rafaelpontani/taba/.github/workflows/release.yml@refs/heads/(main|release)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
gh release download v0.1.0 -R rafaelpontani/taba && sha256sum -c SHA256SUMS
gh attestation verify taba-0.1.0-x86_64-unknown-linux-musl -R rafaelpontani/taba
```

> **Private repository:** GitHub attestations require a public repository (or GitHub Enterprise Cloud), and a signature from a private repository would point to a workflow nobody can see. While the repository is private, `plan` sets `signed=false`: attestations, cosign signatures and Scorecard are skipped (`verify` says so in the summary) and the rest of the pipeline (the order, the single digest, the tags and the package metadata) stays the same. Once the repository is made public, they turn on by themselves.

What backs this trust:

- **Actions pinned by SHA** (with the version comment) and updated by Dependabot (`.github/dependabot.yml`: actions, cargo in both workspaces, nuget and the Docker bases; the PRs go to `release` with a `ci`/`chore` prefix, which produces no version).
- **Minimal permissions:** `permissions: {}` at the top and each job asks only for what it uses; `checkout` with `persist-credentials: false`.
- **No long-lived secrets** for crates.io and nuget.org (OIDC); keyless signatures in Sigstore's public log (Rekor).
- **Workflow lint** (`actionlint`, `zizmor`), **dependency audit** (`cargo audit`) and **CodeQL** on every PR, and the public **OpenSSF Scorecard**.
- [SECURITY.md](../../SECURITY.md): how to report a vulnerability and which versions get fixes.

### Repository configuration (outside the code)

| Where | What |
|---|---|
| Branch rulesets (`main`, `release`) | PR required, the `ci.yml` checks, no force-push, no deletion |
| Tag ruleset (`v*.*.*`) | Only GitHub Actions creates; nobody updates or deletes. The floating ones (`v0`, `v0.1`, `v0.1-rc`) do not match the pattern and can move |
| Releases | **Immutable Releases** on: once published, neither the tag nor the assets change |
| `release` environment | Restricted to the `main` and `release` branches; holds the Docker Hub secrets |
| Variables | `PUBLISH_CRATES`, `NUGET_USER`, `DOCKERHUB_NAMESPACE` (default `rafaelpontani`) |
| crates.io / nuget.org | The trusted publisher: repository `rafaelpontani/taba`, workflow `release.yml`, environment `release` |

## What is not yet automated

- 🟡 **Integration job**: the connectors' contract tests (`contract-connectors.yml`) **do run in CI**, on pull requests to `release`. `connectors/csharp/scripts/integration.sh` and `docker/validate.sh` **do not run in CI**. They exist and work by hand; run `validate.sh` with the `rc` image before the `release → main` PR.
- ⬜ The chart's e2e cases (`deploy/helm/tests/run.sh 01 05 …`, with kind) run by hand.
- ⬜ Publishing of the other connectors (only Rust and C# exist).
- ⬜ Automatic verification of the **documentation** in CI: see `doc/pt-BR/scripts/verificar.mjs` below.

## Automatic verification of this documentation

`node doc/pt-BR/scripts/verificar.mjs` (from the root) checks, without network and without compiling:

| Check | Fails when |
|---|---|
| Markdown links between files of `doc/pt-BR` and to the repository | The file or the anchor does not exist |
| Metadata | `resumo`, `status`, `verificado-em` or `fontes` is missing at the top, or the "Fontes e provas" section is missing |
| Configuration parameters | A key of `config/params/mod.rs` does not appear in [22-configuration.md](22-configuration.md), or the document cites a nonexistent key |
| Error codes | An `ErrorCode` of the code does not appear in [19-errors-and-limits.md](19-errors-and-limits.md) (or vice versa) |
| Routes | A route registered in the code (`.route(…)` and the cluster map) is not cited in the API documents |
| Use cases | A `UC-…` cited in the documents does not exist in the cards of 26a/26b |
| Metrics | A metric of `telemetry/metrics/defs.rs` does not appear in [13-observability.md](13-observability.md) |
| Diagrams | Every `../diagrams/img/…` image exists, and every remaining `🖼️ DIAGRAMA PLANEJADO` block carries `tipo:` and `objetivo:` |

The script currently checks the `doc/pt-BR` folder (the Portuguese keys `resumo`, `fontes`…). A second script, `node doc/pt-BR/scripts/verificar-idiomas.mjs`, checks that the language folders say the same thing: every document exists in every folder (by its number prefix), the metadata has the keys of each language (`summary`, `sources`, `verified-on` in `en-US`), links and anchors resolve inside each folder, and the `UC-…`, `D…` and `PRB-…` codes, code blocks, images and tables are the same on both sides. Both can enter CI as a cheap PR job (Node only).

## Sources and evidence

- `.github/workflows/{ci,build-server,build-connector-rust,build-connector-csharp,contract-connectors,release,scorecard,codeql}.yml`, `.github/actions/node-binary/action.yml`, `.github/scripts/set-version.sh`, `.github/dependabot.yml`, `.releaserc.json`, `.github/actions/{registry-login,provenance}/action.yml`, `Dockerfile`, `Dockerfile.ci`, `connectors/csharp/Directory.Build.props`, `connectors/rust/{Cargo.toml,client/Cargo.toml,cli/Cargo.toml}`, `SECURITY.md`.
- Decisions: D159 to D163 ([30-decisions.md](30-decisions.md)).
- Use cases: there are no pipeline `UC-` cases; validation is `docker/validate.sh` ([25-testing.md](25-testing.md)).
