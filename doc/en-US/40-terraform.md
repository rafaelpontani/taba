---
title: Terraform and OpenTofu provider
summary: "The Taba provider for Terraform and OpenTofu (`rafaelpontani/taba`), in `connectors/terraform`: what it manages (queues, distributors, access tokens and four data sources), the provider configuration, how plan, drift and import work, `force_destroy`, the optional `create_or_update` (when to use it and its risks), the token secret in the state and its rotation, the retries while a node is down, how it is tested and published, and its limits."
type: guide
status: partial
verified-on: 2026-11-07 @ 689495f
sources: [connectors/terraform/internal/provider/provider.go, connectors/terraform/internal/provider/queue_resource.go, connectors/terraform/internal/provider/distributor_resource.go, connectors/terraform/internal/provider/token_resource.go, connectors/terraform/internal/provider/topic_data_sources.go, connectors/terraform/internal/provider/token_data_sources.go, connectors/terraform/internal/provider/common.go, connectors/terraform/internal/provider/planmodifiers.go, connectors/terraform/internal/provider/acceptance_test.go, connectors/terraform/scripts/test.sh, connectors/terraform/scripts/cluster.sh, connectors/terraform/scripts/package.go, connectors/terraform/scripts/verify-release.sh, connectors/terraform/scripts/mirror.sh, connectors/go/tokens.go, connectors/go/transport.go, .github/workflows/release.yml]
---

# Terraform and OpenTofu provider

> Read first: [16-client-api.md](16-client-api.md) (topics and tokens) and [14-security.md](14-security.md) (tokens and permissions).

The provider manages as code what is otherwise created with the client API or with `taba-cli`: **queues, distributors and access tokens**, with `plan`, drift detection, `import` and a controlled removal. It is a Go program in `connectors/terraform` (terraform-plugin-framework) that calls the client API through the Go connector (`connectors/go`), so it needs nothing on the node besides the client API. The same binary serves **Terraform** and **OpenTofu**, and the [Pulumi provider](41-pulumi.md) is generated from it.

**Status.** The provider is done and tested (units, acceptance against a real node with Terraform and OpenTofu, a three-node run with a node down). It is published on the [Terraform Registry](https://registry.terraform.io/providers/rafaelpontani/taba) as `rafaelpontani/taba` since `v0.3.8-rc.3`; the submission to the OpenTofu Registry (the provider and its signing key) was validated and waits for the review of the OpenTofu team ([Publication](#publication)). The zips of each version are also in its GitHub Release.

```hcl
terraform {
  required_providers {
    taba = { source = "rafaelpontani/taba" }
  }
}

provider "taba" {
  endpoint = "https://taba.example.com:8080" # or TABA_URL
  token    = var.taba_token                  # or TABA_TOKEN
}

resource "taba_queue" "orders" {
  name           = "orders"
  ttl            = 86400
  max_deliveries = 5
}

resource "taba_distributor" "events" {
  name     = "events"
  prefixes = ["orders", "billing"]
}

resource "taba_token" "orders_app" {
  name           = "orders-app"
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders"]
  lifecycle { create_before_destroy = true }
}
```

The version of the provider is the version of the repository (`vX.Y.Z`), and it targets **the node of the same version**: there is no detection of what the node supports nor a path for older nodes (D221).

## Configuration

| Attribute | Environment variable | Default | |
|---|---|---|---|
| `endpoint` | `TABA_URL` | required | URL of the client API (the load balancer of the cluster) |
| `token` | `TABA_TOKEN` | none (calls without authentication) | Sensitive. Queues and distributors need `create` and `manage` within their prefixes; tokens need the root token (or an `admin` one for tokens that are not `admin`) |
| `ca_cert_pem` / `ca_cert_file` | `TABA_CA_CERT` (a PEM or a path) | the system roots | A private certificate authority of the API certificate |
| `request_timeout` | | `30s` | Timeout of each call (a Go duration) |
| `retry_max_wait` | | `3m` | Total wait between the attempts of a call that gets `429` or `503` ([below](#retries-and-a-node-that-is-down)); `0s` = no new attempt |
| `create_or_update` | `TABA_CREATE_OR_UPDATE` (`true`/`false`) | `false` | Default of every resource ([below](#taking-over-what-exists)); another text in the variable is a configuration error |

## Resources and data sources

| | Changes in place | Replaces (`-/+`) | Computed |
|---|---|---|---|
| `taba_queue` | `tags`, `ttl`, `max_deliveries`, `max_message_bytes`, `idle_timeout` (volatile only) | `name`, `fifo`, `volatile` | `id`, `created_at`, `dead_letter_name`, `effective_ttl`, `effective_max_deliveries`, `effective_max_message_bytes` |
| `taba_distributor` | `prefixes` (1 to 20, in the declared order), `tags` | `name` | `id`, `created_at` |
| `taba_token` | `name`, `expires_at` | `permissions`, `topic_prefixes` (sets) | `id`, `created_at`, `token` (the secret, sensitive) |

Besides those, `force_destroy` (queue) and `create_or_update` (the three) belong only to the provider and never reach the node.

- **What is not declared is the node default.** An attribute left out is `null` in the state, and the `effective_*` show the value in force, so a change of the node default is not a false difference. Taking an attribute out of the configuration brings it back to the default: an update sends the whole declared state with `PUT /topics/{topic}` (D215), because the `PATCH` cannot bring `idleTimeout` back. `idle_timeout` not declared is 600 on a volatile queue and `null` on the others.
- **Checked at `plan`**, before anything reaches the node: the name rules (1 to 100 characters `A-Za-z0-9_-`, not ending in `-dead-letter`), the ranges and the tags of [16-client-api.md](16-client-api.md#rules-for-name-tags-and-ranges), `idle_timeout` only with `volatile = true`, an `expires_at` in the past, and `admin` with `topic_prefixes`.
- **The dead-letter queue** (`<name>-dead-letter`) is born and dies with its queue and is read-only: it is not a resource, only `dead_letter_name`.
- **Data sources:** `taba_topic` (by name, a dead-letter queue too), `taba_topics` (by name fragment, `type`, `tags`, `include_dead_letters`), `taba_token` (by `id`) and `taba_tokens` (by name fragment, oldest first). They never return a secret, and they serve whoever only **uses** queues created by another team.
- **Errors** of the node keep their code, and the `details` become `- field: message` lines.

## Plan, drift and import

- **Read:** a `GET` per resource. A topic or token that is gone (`404`) leaves the state, and the plan proposes creating it again. A change made outside Terraform shows as a difference to undo.
- **Import:** a topic by name (`terraform import taba_queue.orders orders`, or an `import {}` block), a token by `id`. Importing a distributor as `taba_queue` (or the opposite), or a dead-letter queue, is an error, not a replacement. After an import, `force_destroy` is `false` and `create_or_update` is `null`.
- **The token date:** the node only accepts and returns `YYYY-MM-DDTHH:MM:SS.mmmZ`. The provider accepts any RFC 3339 in `expires_at`, sends the node format and keeps the declared text while it is the same instant, so `2027-01-01T00:00:00Z` is not a difference with `...00.000Z`, not even after an import.

## Protecting a queue with messages

`force_destroy` exists because `DELETE /topics/{topic}` always works and drops every message, even the pending ones. So the provider removes a queue with `DELETE ?ifEmpty=true` (D216) unless `force_destroy = true`, like the `aws_s3_bucket` of the AWS provider (D219):

| Situation | `force_destroy = false` (default) | `force_destroy = true` |
|---|---|---|
| A mutable field changed | `PUT` | `PUT` |
| An immutable field changed (`-/+`) | `DELETE ?ifEmpty=true` + `POST` | `DELETE` + `POST` |
| `terraform destroy` | `DELETE ?ifEmpty=true` | `DELETE` |
| Taken over by `create_or_update` with another immutable field | `PUT ?recreate=if-empty` | `PUT ?recreate=if-empty` |

A queue or dead-letter with messages is then the error `409 topic.not-empty`, which says how many there are and the two ways out: empty it, or **apply** `force_destroy = true` first. The value is read from the state, so setting it in the same `apply` that replaces the queue does not count (the same as `aws_s3_bucket`). The check is best-effort (PRB-103). A distributor keeps no messages and has no `force_destroy`.

## Taking over what exists

`create_or_update` is off by default: creating something that already exists is an error, as Terraform normally does, because taking over in silence what another configuration manages is dangerous (D218). The error of `409 topic.already-exists` names both ways out: `import`, or `create_or_update = true`.

The `import {}` block covers "I know it exists". `create_or_update` covers "it may or may not exist": the same configuration applied to environments where part of it was created by hand, by `taba-cli` or by an application at startup, with no manual step per environment. It is turned on in the provider (every resource) or per resource (`create_or_update = true|false` wins over the provider). It only counts at creation; afterwards the resource belongs to the state like any other, and `destroy` **removes what was taken over**.

**Queue and distributor:** one `PUT /topics/{name}?recreate=if-empty` with the declared state, decided atomically by the owner of the definition (D210):

- missing: created, with no warning;
- existing: brought to the declared state (what was not declared goes back to the node default), with a warning "Took over";
- another type, `fifo` or `volatile`: recreated only if the queue and its dead-letter are empty, with a warning "Recreated"; with messages, `409 topic.not-empty` becomes an error with the counts and the hint of `import` + `force_destroy`, and the topic is not touched. The adoption **never** uses `recreate=always`: the plan only showed `+ create`, and dropping messages there would be a destruction the plan did not show;
- `503`: "nothing was changed, try again"; `409 cluster.conflict` is repeated up to three times.

**Token** (the name is not unique and the secret never comes back): before the `POST`, the provider lists the tokens with the name (the same list the creation already makes) and keeps the exact matches. None: creates. One with the same permissions and prefixes: takes it over, patches `expires_at` only when it is another instant (`null` declared removes the deadline), leaves `token` **null** and warns that the secret cannot be recovered (`terraform apply -replace` makes a new one). Several, or one with other permissions or prefixes: error with the ids, nothing changed. This adoption is on the client side and not atomic (PRB-106).

## Tokens

- **The secret is in the state.** `token` is only known in the creation answer, so the state keeps it: treat the state as a secret (an encrypted remote backend, restricted access). A token taken over or imported has `token = null`.
- **Rotation:** `terraform apply -replace=taba_token.x` creates a new token and revokes the old one; with `lifecycle { create_before_destroy = true }` there is no moment without a token.
- **Expiry:** an expired token is hidden by the node (`404`, like a revoked one), so it leaves the state and the plan proposes creating it; since `expires_at` is in the past, the plan refuses with "the token expired: change `expires_at` (a new token) or remove the resource". Nothing is created again in silence.
- **Authentication off** (`404 route.not-found` on `/tokens`), a provider token that cannot administer tokens (`403 auth.forbidden`) and `401` are errors with a hint. A read with authentication off does not take the token out of the state.
- **A lost creation answer.** A `POST /tokens` answered with `503 cluster.quorum-unavailable` may have created the token on the owner of the definition ([16-client-api.md](16-client-api.md#tokens)). The provider lists the tokens with the name **before** the `POST`; after the `503`, the orphan is the equal token (name, permissions, prefixes) that was not on that list. One: revoked, and the `POST` tried again. None: tried again. Two or more: error with the ids, nothing revoked. The attempts stay within `retry_max_wait`; when it is spent, error after revoking. The residual limit is PRB-107.

## Retries and a node that is down

The calls repeat `429` and `503`, as the common contract of the connectors says ([21-connectors.md](21-connectors.md#the-common-contract)), waiting for the `Retry-After` or 1, 2, 4... up to 64 s, and no attempt is made whose wait would pass `retry_max_wait` (the `Config.RetryMaxWait` of the Go connector, [21-connectors.md](21-connectors.md#go-githubcomrafaelpontanitabaconnectorsgo-package-taba)). The default `3m` is sized by a measure: a node that crashed stays a member, and the owner of part of the definitions, until the others vote it out (about 65 s in the Compose cluster), and in that window a creation it owns gets `503 cluster.quorum-unavailable` with `Retry-After: 5`. A budget counted in attempts (8 attempts, 35 s) failed the `apply` 4 times in 5 (D220). `0s` turns the new attempts off.

A `POST /topics` answered `503 cluster.commit-timeout` "will complete": if the new attempt gets `409 topic.already-exists`, the provider reads the topic and takes it as its own creation only if it is what was declared.

## With the Helm chart

The node and its configuration are the Helm chart ([34-helm-chart.md](34-helm-chart.md)), which Terraform installs with the `helm` provider; the queues, distributors and tokens are this provider. The root token is in the `<release>-secrets` Secret, key `root-token`:

```hcl
resource "helm_release" "taba" {
  name             = "taba"
  repository       = "oci://ghcr.io/rafaelpontani/charts"
  chart            = "taba"
  version          = "0.2.0"
  namespace        = "taba"
  create_namespace = true
  wait             = true
}

data "kubernetes_secret" "taba" {
  metadata {
    name      = "taba-secrets"
    namespace = "taba"
  }
  depends_on = [helm_release.taba]
}

provider "taba" {
  endpoint = "https://taba.example.com:8080"
  token    = data.kubernetes_secret.taba.data["root-token"]
}
```

A provider configured from a resource of the same configuration only works when that resource already exists at plan time; a separate configuration (or `-target` on the first run) for the cluster is the usual answer.

## Out of scope

- Cluster operations (drain, backup, `cluster/stop`): actions, not a desired state; they stay with `taba-cli` and the operations API.
- Node configuration and deployment: the Helm chart.
- Consumer groups and the SQS/SNS gateway keys: they have no definition in the client API.
- Messages.

## Tests

| Level | Where | What |
|---|---|---|
| Units | `internal/provider/*_test.go` (except `acceptance_test.go`) | A fake HTTP server that records every request: bodies, routes, plan checks, `create_or_update`, the lost token answer, the retry budget |
| Acceptance | `internal/provider/acceptance_test.go` (`TF_ACC=1`) | 10 tests with terraform-plugin-testing against one node with authentication on, with **Terraform and OpenTofu**: create, import with verification, drift, `force_destroy`, replacement, tokens, `create_or_update`, data sources; after each step the framework plans again and fails on a non-empty plan |
| Three nodes | `scripts/cluster.sh` | The Compose cluster: apply 10 queues and 10 tokens, kill `taba-2`, apply 40 of each right away (each created once), empty plan, the node back, empty plan again, destroy. About 5 minutes; by hand before a release that changes the provider, not in CI |

`scripts/test.sh` runs everything in containers (`unit`, `lint` with golangci-lint, `docs` checks that `docs/` regenerated by `tfplugindocs` matches the committed one, `acc` with both CLIs); the CLIs are downloaded once, checked against their `SHA256SUMS`. The CI jobs are in [27-ci-cd.md](27-ci-cd.md). Use cases `UC-IAC-001` to `UC-IAC-023` ([26a](26a-implemented-use-cases.md)).

## Publication

The Terraform Registry and the OpenTofu Registry only read a public repository named `terraform-provider-<name>` with signed releases, so the provider is published through a **mirror repository**, `rafaelpontani/terraform-provider-taba`, fed by `release.yml`; the code stays here (D211).

1. `scripts/package.go` builds the 13 platforms (`-trimpath`, no cgo, zips with a fixed date: the same commit gives the same files), the `_manifest.json` (protocol 6.0) and the `_SHA256SUMS`. The zips go to the Taba GitHub Release of the version.
2. Only when the variable `TERRAFORM_PROVIDER_REPO` exists, the `terraform-registry` job signs the `SHA256SUMS` (a detached binary `.sig`, with an **RSA** key: the Terraform Registry refuses ECC keys), runs `scripts/verify-release.sh` against the `taba:<version>` image (sums, signature, installation of the exact version from the files with Terraform and OpenTofu, apply, empty plan, destroy) and only then `scripts/mirror.sh`: the source of `connectors/terraform` with the Go connector in `go/`, a commit, the tag `v<version>` and a Release with the files (a pre-release for `-rc.N`).
3. The registries read the mirror by themselves: the Terraform Registry through the namespace `rafaelpontani`, claimed in HCP Terraform (organization `taba`, Registry → Public namespaces), which holds the public key and a webhook on the mirror; the OpenTofu Registry through the entry of the provider and of the key in `opentofu/registry`. A new Release in the mirror is a new version in both, with no step by hand. `terraform init` checks the signature against that key (`self-signed, key ID 9CF9BC950E956E3F`).

What the maintainer creates is listed in [27-ci-cd.md](27-ci-cd.md#terraform-provider).

## Limits

- **PRB-106:** the adoption of a token is not atomic; two `apply` at the same time can create two tokens with the same name (the next one reports the ambiguity). Topics do not have it, thanks to the `PUT`.
- **PRB-107:** after a lost creation answer, an equal token created by someone else between the list and the `503` would be revoked.
- **PRB-109:** only the Go connector administers tokens, so the contract scenario of the connectors does not cover them.
- The emptiness check of `force_destroy` is best-effort (PRB-103), and during a rolling upgrade a `PUT` can fail (PRB-104).

## Sources and evidence

- `connectors/terraform/internal/provider/` (provider, resources, data sources), `connectors/terraform/scripts/` (tests, packaging, publication), `connectors/go/tokens.go` and `connectors/go/transport.go` (tokens and `RetryMaxWait`), the jobs `connector-terraform`, `terraform-provider` and `terraform-registry` of `.github/workflows/`.
- Decisions D210, D211, D215, D216, D218 to D221 in [30-decisions.md](30-decisions.md); problems PRB-103, PRB-104, PRB-106, PRB-107 and PRB-109 in [31-pending-items.md](31-pending-items.md).
- Use cases `UC-IAC-001` to `UC-IAC-023` in [26a](26a-implemented-use-cases.md), and `UC-CNT-044` (tokens in the Go connector).
