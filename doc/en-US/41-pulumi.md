---
title: Pulumi provider
summary: "The Taba provider for Pulumi (`taba`), in `connectors/pulumi`: the Terraform provider bridged to TypeScript/JavaScript, Python, .NET and Go; the packages and how the plugin is installed, the provider configuration, the resources and their names in the SDKs, why nothing is named automatically, `preview`, drift and `import`, `forceDestroy`, the optional `createOrUpdate`, how a replacement and the rotation of a token work in Pulumi, `protect` and `retainOnDelete`, the token secret in the state, how the SDKs are generated and tested, and how it is published."
type: guide
status: partial
verified-on: 2026-11-08 @ f8a9a0a
sources: [connectors/pulumi/provider/resources.go, connectors/pulumi/provider/docs_edits.go, connectors/pulumi/provider/resources_test.go, connectors/pulumi/provider/cmd/pulumi-resource-taba/main.go, connectors/pulumi/provider/cmd/pulumi-tfgen-taba/main.go, connectors/pulumi/provider/cmd/pulumi-resource-taba/schema.json, connectors/pulumi/scripts/generate.sh, connectors/pulumi/scripts/test.sh, connectors/pulumi/scripts/package.sh, connectors/pulumi/scripts/verify-release.sh, connectors/pulumi/scripts/mirror.sh, connectors/pulumi/examples/scenarios.sh, connectors/terraform/shim/shim.go, .github/workflows/release.yml, .github/workflows/build-connector-pulumi.yml]
---

# Pulumi provider

> Read first: [40-terraform.md](40-terraform.md) (the behaviour of every resource is defined there), [16-client-api.md](16-client-api.md) (topics and tokens) and [14-security.md](14-security.md) (tokens and permissions).

The provider manages as code, in a Pulumi program, what the [Terraform and OpenTofu provider](40-terraform.md) manages: **queues, distributors and access tokens**, with `preview`, drift detection, `import` and a controlled removal. It is **the same provider**, bridged to Pulumi with the [pulumi-terraform-bridge](https://github.com/pulumi/pulumi-terraform-bridge): `connectors/pulumi` embeds the Terraform provider (`connectors/terraform/shim`), maps its resources to Pulumi tokens and generates the SDKs of **TypeScript/JavaScript, Python, .NET and Go**. There is one body of code for the behaviour and one set of tests for it; a behaviour that is wrong in Pulumi is wrong in Terraform, and is fixed and tested there (D212).

**Status.** The provider is done and tested locally: units, the generated SDKs up to date, acceptance scenarios in TypeScript and Python against a real node, and the release files installed as a user gets them. The pipeline packages it with every version; **nothing is published yet**: the mirror repository, the npm, PyPI and NuGet packages wait for the maintainer's setup ([Publication](#publication)). Java is not generated yet ([31-pending-items.md](31-pending-items.md)).

```typescript
import * as taba from "@rafaelpontani/pulumi-taba";

const orders = new taba.Queue("orders", { name: "orders", ttl: 86400, maxDeliveries: 5 });

const events = new taba.Distributor("events", { name: "events", prefixes: ["orders", "billing"] });

const token = new taba.Token("orders-app", {
    name: "orders-app",
    permissions: ["publish", "consume"],
    topicPrefixes: [orders.name],
});

export const secret = token.secret; // a Pulumi secret
```

The version of the provider is the version of the repository (`vX.Y.Z`), and it targets **the node of the same version**, like the Terraform one (D221).

## Installation

A program needs two things: the **package** of its language and the **plugin** (the provider binary) that the Pulumi CLI runs.

| Language | Package | Install |
|---|---|---|
| TypeScript / JavaScript | `@rafaelpontani/pulumi-taba` (npm) | `npm install @rafaelpontani/pulumi-taba` |
| Python | `pulumi-taba` (PyPI; import `pulumi_taba`) | `pip install pulumi-taba` |
| .NET | `Taba.Pulumi` (nuget.org) | `dotnet add package Taba.Pulumi` |
| Go | `github.com/rafaelpontani/pulumi-taba/sdk/go/taba` | `go get github.com/rafaelpontani/pulumi-taba/sdk` |

The plugin is not inside those packages. The packages carry `pluginDownloadURL`, so the CLI downloads it by itself on the first `pulumi up`; to install it before (a build agent without internet at run time, a pinned version):

```bash
pulumi plugin install resource taba 0.3.9 --server github://api.github.com/rafaelpontani/pulumi-taba
```

It reads the GitHub Release `v0.3.9` of the mirror repository, which has `pulumi-resource-taba-v0.3.9-<os>-<arch>.tar.gz` for linux, darwin and windows on amd64 and arm64 (D213). The same archives are in the Taba Release, and `pulumi plugin install resource taba <version> --file <archive>` installs one without the network. Use the version of the package: the provider and the SDK of one version go together.

Python follows PEP 440, so a candidate `0.3.9-rc.1` is `0.3.9rc1` on PyPI; npm keeps `0.3.9-rc.1` and puts candidates on the `next` tag, so `npm install` and `pip install` never pick a candidate by themselves.

## Configuration

The same settings as the Terraform provider, in `pulumi config` (a stack setting) or in `new taba.Provider(...)`, and the same environment variables:

| Setting | Environment variable | Default | |
|---|---|---|---|
| `taba:endpoint` | `TABA_URL` | required | URL of the client API (the load balancer of the cluster) |
| `taba:token` | `TABA_TOKEN` | none (calls without authentication) | A secret: `pulumi config set --secret taba:token ...`. Queues and distributors need `create` and `manage` within their prefixes; tokens need the root token (or an `admin` one for tokens that are not `admin`) |
| `taba:caCertPem` / `taba:caCertFile` | `TABA_CA_CERT` (a PEM or a path) | the system roots | A private certificate authority of the API certificate. The certificate is public, so `caCertPem` is not a secret |
| `taba:requestTimeout` | | `30s` | Timeout of each call (a Go duration) |
| `taba:retryMaxWait` | | `3m` | Total wait between the attempts of a call that gets `429` or `503` ([40-terraform.md](40-terraform.md#retries-and-a-node-that-is-down), D220); `0s` = no new attempt |
| `taba:createOrUpdate` | `TABA_CREATE_OR_UPDATE` (`true`/`false`) | `false` | Default of every resource ([below](#taking-over-what-exists)) |

```bash
pulumi config set taba:endpoint https://taba.example.com:8080
pulumi config set --secret taba:token "$ROOT_TOKEN"
```

## Resources and functions

| Terraform | Pulumi (token) | |
|---|---|---|
| `taba_queue` | `taba:index/queue:Queue` | `Queue` |
| `taba_distributor` | `taba:index/distributor:Distributor` | `Distributor` |
| `taba_token` | `taba:index/token:Token` | `Token` |
| data sources `taba_topic`, `taba_topics`, `taba_token`, `taba_tokens` | `taba:index/getTopic:getTopic` and its three siblings | `getTopic`, `getTopics`, `getToken`, `getTokens` (and the `...Output` forms) |

The attributes keep their meaning, validation and what is computed (the tables of [40-terraform.md](40-terraform.md#resources-and-data-sources)); only the spelling follows each language: `maxDeliveries` in TypeScript, `max_deliveries` in Python, `MaxDeliveries` in .NET and Go. The `id` is the one of Terraform: the name of the topic, and the `id` the server gives a token. Two names differ from Terraform:

- **The secret of a token is `secret`, not `token`** (D212). The generated C# class `Token` cannot have a property called `Token` (CS0542), so the field has the same name in all the languages. It is a Pulumi secret: it shows as `[secret]` in the CLI and is encrypted in the state by the backend.
- **`createOrUpdate`** is also a property of each resource (`new taba.Queue("orders", { name: "orders", createOrUpdate: true })`) and `forceDestroy` of the queue.

A function (`taba.getTopic({ name: "orders" })`) reads a queue or token created elsewhere, and never returns a secret. A function whose input does not depend on a resource of the program **runs during `preview`, before the resource exists**: pass the output of the resource (`taba.getTopicOutput({ name: orders.name })`) when it has to wait for it.

## No automatic names

Pulumi gives a resource a physical name with a random suffix (`orders-1a2b3c4`) when the provider allows it ("autonaming"). Taba does not allow it, and **`name` is required** in `Queue`, `Distributor` and `Token` (D222): the name of a queue is a contract with the applications that publish and consume, and it is how `createOrUpdate` finds a queue that already exists, which a suffix would make impossible. The Pulumi name of the resource (`"orders"` in `new taba.Queue("orders", ...)`) is only the identity inside the program, and can differ from `name`.

## Preview, drift and import

- **Preview:** the plan checks the same rules as Terraform before anything reaches the node (names, ranges, tags, `idleTimeout` only with `volatile`, an `expiresAt` in the past, `admin` with `topicPrefixes`) and shows `~` for an in-place change and `+-` for a replacement.
- **Drift:** `pulumi refresh` reads each resource again; a topic or token deleted outside Pulumi leaves the state, and a field changed outside shows as a difference that the next `pulumi up` undoes.
- **Import:** `pulumi import taba:index/queue:Queue orders orders` (the topic name), `pulumi import taba:index/token:Token app <token id>`, or the `import` option of the resource. Importing a distributor as a `Queue` (or the opposite), or a dead-letter queue, is an error. The program that `pulumi import` writes has neither `forceDestroy` nor `createOrUpdate` (they belong to the provider, `false`/`null` after an import), and the imported resource is `protect`ed: remove the protection to destroy it.

## Protecting a queue with messages

`forceDestroy` works as in Terraform ([40-terraform.md](40-terraform.md#protecting-a-queue-with-messages), D219): without it, a removal or a replacement uses `DELETE ?ifEmpty=true` and a queue with messages is the error `409 topic.not-empty`, which says how many there are and the two ways out. Its value is read from the **state**, so it must be applied by a `pulumi up` **before** the one that replaces or removes the queue:

```bash
# 1. forceDestroy: true in the program, pulumi up   (nothing else changes)
# 2. the change that replaces the queue, or pulumi destroy
```

## Taking over what exists

`createOrUpdate` is off by default (D218) and has the behaviour, the warnings and the limits of [40-terraform.md](40-terraform.md#taking-over-what-exists): a queue or distributor is created, or brought to the declared state, by one `PUT ?recreate=if-empty` decided by the node; a token is taken over only when exactly one has the name, the permissions and the prefixes (its secret does not come back: `pulumi up --replace <urn>` makes a new one). The warnings "Took over" and "Recreated" appear as diagnostics of `pulumi up`.

```typescript
const provider = new taba.Provider("taba", { createOrUpdate: true });
const orders = new taba.Queue("orders", { name: "orders" }, { provider });
// or, for one resource: new taba.Queue("orders", { name: "orders", createOrUpdate: true })
// or, for the whole stack: pulumi config set taba:createOrUpdate true
```

`pulumi import` covers "I know it exists"; `createOrUpdate` covers "it may or may not exist". After `up`, a resource taken over belongs to the state like any other, and `pulumi destroy` **removes it**.

## Replacement and rotation

Pulumi creates the new resource **before** deleting the old one by default. For a queue or distributor that would fail with `409 topic.already-exists` (the name is the id), so both are declared `deleteBeforeReplace`: a change of `fifo` or `volatile` deletes first (with `forceDestroy` and the emptiness check above) and then creates. A `Token` keeps create-before-delete, because its id is the server's: the **rotation** is

```bash
pulumi up --replace 'urn:pulumi:dev::project::taba:index/token:Token::orders-app'
```

which creates the new token, updates whoever uses its `secret` and revokes the old one, with no moment without a valid token. `permissions` and `topicPrefixes` also replace a token; `name` and `expiresAt` change it in place.

## `protect` and `retainOnDelete`

Options of Pulumi that complement `forceDestroy`:

| Option | Effect |
|---|---|
| `protect: true` | `pulumi destroy` and a replacement refuse the resource until it is removed. Use it on a queue that is production data |
| `retainOnDelete: true` | The resource leaves the state and **stays on the node**. Use it to hand a queue to another program or tool |
| `forceDestroy: true` (queue) | Allows deleting a queue that still holds messages, on the next removal |

## Tokens

- **The secret is in the state**, as a Pulumi secret. Keep the state in a backend that encrypts it (Pulumi Cloud, or a self-managed one with a `--secrets-provider`); `pulumi stack output --show-secrets` prints it.
- A token **taken over** (`createOrUpdate`) or **imported** has an empty `secret`, and the warning of the provider says so.
- Expiry, a lost creation answer and the permissions needed are the same as in [40-terraform.md](40-terraform.md#tokens).

## How it is built

- `provider/resources.go` maps the Terraform provider (`shim.New(version)`): the Pulumi tokens, the `secret` field, `deleteBeforeReplace` and no autonaming. The bridge uses the same `terraform-plugin-framework` (v1.19) and `terraform-plugin-go` (v0.31) as the Terraform provider, with no fork; it needs Go 1.26, so `connectors/pulumi/provider` is a Go module of its own, with two local `replace` (`connectors/terraform` and `connectors/go`).
- `provider/docs_edits.go` adapts the descriptions that come from `connectors/terraform/docs/`: the `lifecycle` block of the token example (it only exists in Terraform and breaks the converter) and the sentence of the rotation.
- `scripts/generate.sh` runs `pulumi-tfgen-taba` in a container (the Pulumi CLI is downloaded once into a Docker volume and checked against its `SHA256SUMS`) and writes `provider/cmd/pulumi-resource-taba/schema.json` and the four SDKs in `sdk/`, which are **versioned** with `linguist-generated`. A change to a schema or to an example of the Terraform provider needs `generate.sh` again; the CI fails if the SDKs are not up to date.
- The SDKs are generated with a placeholder version (`0.0.0`); `scripts/package.sh` writes the real one into a copy, so the committed tree does not change with a release.

## Tests

| Level | Command | What |
|---|---|---|
| Units | `scripts/test.sh unit` | `gofmt`, `go vet` and `provider/resources_test.go`: every resource and data source of the Terraform provider has a Pulumi token and is in the schema (a resource added to Terraform without mapping fails here), `name` is required and nothing has autonaming, everything sensitive in Terraform is a `secret`, the replacement order |
| SDKs | `scripts/test.sh sdks` | Runs `generate.sh` again and compares: the committed schema and SDKs are the generated ones |
| Acceptance | `scripts/test.sh acc` | Builds the plugin and the SDKs (.NET and Go are compiled) and runs `examples/scenarios.sh` with TypeScript and Python against one node, with the file backend (no Pulumi account). After each step, `pulumi preview --expect-no-changes` |
| Release files | `scripts/verify-release.sh <folder> <version>` | Installs the plugin, the npm package and the wheel **from the files** that `package.sh` made and runs `up` (three resources), an empty `preview` and `destroy` against the image of the version |

The scenarios of `acc`: the first `up` (queue, distributor, token and functions); an in-place update; a change of an immutable field on a queue with messages, refused and then made after `forceDestroy`; drift made through the API, seen by `refresh` and corrected; `createOrUpdate` over a queue that exists; the rotation of the token; `import` of a queue and a token; `destroy` with nothing left on the node. Everything runs in containers (`TABA_IMAGE` chooses the node, `taba:local` by default); CI job `connector-pulumi` runs the units and the SDK check, and the acceptance runs in `connector-contract` against the image built from the commit. A program in .NET or Go with `preview` is not run yet ([31-pending-items.md](31-pending-items.md)).

Use cases `UC-IAC-024` to `UC-IAC-034` ([26a](26a-implemented-use-cases.md), [26b](26b-planned-use-cases.md)).

## Publication

`release.yml` packages the provider with every version (`scripts/package.sh`: six plugin archives, the npm package, the wheel and the sdist, the NuGet package, with the README of the provider as the one nuget.org shows, the Go SDK module and a `SHA256SUMS`, reproducible) and the files go to the Taba Release. Then the job `pulumi-verify` runs `verify-release.sh` against the image just published (it also reads the `.nupkg`: id, version and README, which cannot be changed after the push), and only if it passes does each destination run, each with its own variable (empty = not published):

| Destination | Variable | What |
|---|---|---|
| Mirror `rafaelpontani/pulumi-taba` (D213) | `PULUMI_PROVIDER_REPO` and the secret `PULUMI_MIRROR_TOKEN` | `scripts/mirror.sh`: the Go SDK (`sdk/`), the schema, the examples and the README on `main`, the tags `v<version>` and `sdk/v<version>` (the Go module is in `sdk/`, so Go reads its versions from `sdk/` tags), and a Release with the files, which is what `pulumi plugin install --server github://...` downloads |
| npm | `PULUMI_NPM=true` | Trusted Publishing; `--tag next` on a candidate; then installs the package from npm and checks it |
| PyPI | `PULUMI_PYPI=true` | Trusted Publishing; `skip-existing` |
| nuget.org | `PULUMI_NUGET_USER` | Trusted Publishing (`NuGet/login`); `--skip-duplicate` |

The mirror exists because the Go module path and the Pulumi plugin download need a repository with its own tags and Releases, and the tag ruleset of this repository does not allow a tag like `sdk/v0.3.9` (D213). Every job is idempotent, so a rerun is harmless, and a candidate (`-rc.N`) goes out as a pre-release. What the maintainer creates once is in [27-ci-cd.md](27-ci-cd.md#pulumi-provider).

## Limits

- **Java** (Maven Central) is not generated: it needs a Central Portal account, a verified namespace and a GPG key of its own ([31-pending-items.md](31-pending-items.md)).
- **The Pulumi Registry** does not list the provider yet: the listing is a pull request to `pulumi/registry`, reviewed by Pulumi, and waits for the first final version ([31-pending-items.md](31-pending-items.md)).
- `forceDestroy` is read from the state (apply it before), and an imported resource is `protect`ed.
- The limits of the Terraform provider apply: PRB-103, PRB-104, PRB-106, PRB-107 ([40-terraform.md](40-terraform.md#limits)).

## Sources and evidence

- `connectors/pulumi/provider/` (mapping, tests, plugin and `tfgen`), `connectors/pulumi/scripts/` (generation, tests, packaging, publication), `connectors/pulumi/examples/` (the scenarios), `connectors/terraform/shim/` (the package the bridge embeds), the jobs `connector-pulumi`, `pulumi-provider`, `pulumi-verify`, `pulumi-mirror`, `pulumi-npm`, `pulumi-pypi` and `pulumi-nuget` of `.github/workflows/`.
- Decisions D212, D213 and D222 in [30-decisions.md](30-decisions.md), with D211 and D218 to D221 of the Terraform provider.
- Use cases `UC-IAC-024` to `UC-IAC-034` in [26a](26a-implemented-use-cases.md) and [26b](26b-planned-use-cases.md).
