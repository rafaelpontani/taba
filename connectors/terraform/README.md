# Terraform and OpenTofu provider for Taba

Manages the queues, distributors and access tokens of a [Taba](https://github.com/rafaelpontani/taba) cluster as code, through its client API: `plan`, drift detection, `import`, a guard against removing a queue that still holds messages, and an optional `create_or_update` that takes over what already exists. One binary for **Terraform** and **OpenTofu**.

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/40-terraform.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

The full guide (configuration, every resource, `create_or_update` and its risks, tokens, retries, tests, publication): [40-terraform.md](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/40-terraform.md). The reference of each attribute is in [`docs/`](docs/index.md), generated from the schemas.

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

resource "taba_token" "orders_app" {
  name           = "orders-app"
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders"]
  lifecycle { create_before_destroy = true }
}
```

- **Resources:** `taba_queue`, `taba_distributor`, `taba_token`. **Data sources:** `taba_topic`, `taba_topics`, `taba_token`, `taba_tokens`.
- **The provider version is the Taba version** and targets the node of the same version.
- **The state holds the token secrets** (`taba_token.*.token`): keep it in an encrypted backend with restricted access.
- **A queue with messages is not removed** (nor replaced) unless `force_destroy = true` was applied first.
- **`create_or_update` is off by default**: turn it on (provider or resource, or `TABA_CREATE_OR_UPDATE=true`) only for configurations meant to take over what was created by hand, by `taba-cli` or by an application. A resource taken over is removed by `destroy` like any other.

## Installing

From the [Terraform Registry](https://registry.terraform.io/providers/rafaelpontani/taba), and from the [OpenTofu Registry](https://search.opentofu.org/provider/rafaelpontani/taba), as `rafaelpontani/taba` in both, signed with the same key. The documentation of every resource and data source is on the [Terraform Registry](https://registry.terraform.io/providers/rafaelpontani/taba/latest/docs). Without a registry, each [Taba Release](https://github.com/rafaelpontani/taba/releases) carries the zips (`terraform-provider-taba_<version>_<os>_<arch>.zip`) with their `SHA256SUMS`: put them in a [filesystem mirror](https://developer.hashicorp.com/terraform/cli/config/config-file#filesystem_mirror) under `registry.terraform.io/rafaelpontani/taba/` (or `registry.opentofu.org/...` for OpenTofu).

## Developing

The provider is a Go module (Go 1.25) on top of the [Go connector](https://github.com/rafaelpontani/taba/tree/main/connectors/go), linked by a local `replace`. Everything runs in Docker:

```
scripts/test.sh            # units, golangci-lint, docs/ up to date, acceptance with Terraform and OpenTofu against one node
scripts/test.sh unit       # only the units (no node)
scripts/cluster.sh         # against the three-node Compose, killing a node during an apply (about 5 minutes)
scripts/package.sh 0.0.0-dev dist   # the files the registries read
```

`docs/` is generated (`go generate ./...` with `terraform` on the `PATH`) from the schemas and from `examples/`; a pull request with an outdated `docs/` fails.

This folder is copied to [rafaelpontani/terraform-provider-taba](https://github.com/rafaelpontani/terraform-provider-taba) on each release, which is the repository the registries read. Changes go to [rafaelpontani/taba](https://github.com/rafaelpontani/taba) ([CONTRIBUTING](https://github.com/rafaelpontani/taba/blob/main/CONTRIBUTING.md)).
