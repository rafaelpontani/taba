# Pulumi provider for Taba

Manages the queues, distributors and access tokens of a [Taba](https://github.com/rafaelpontani/taba) cluster as code, through its client API: `preview`, drift detection with `refresh`, `import`, a guard against removing a queue that still holds messages, and an optional `createOrUpdate` that takes over what already exists. Available for **TypeScript/JavaScript, Python, .NET and Go**. It is the [Terraform and OpenTofu provider](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/40-terraform.md) bridged to Pulumi, so both behave the same way.

> Other languages: [Português (Brasil)](https://github.com/rafaelpontani/taba/blob/main/doc/pt-BR/41-pulumi.md) · [all languages](https://github.com/rafaelpontani/taba/blob/main/doc/README.md)

The full guide (installation by language, configuration, every resource, `createOrUpdate` and its risks, tokens, replacement, tests, publication): [41-pulumi.md](https://github.com/rafaelpontani/taba/blob/main/doc/en-US/41-pulumi.md).

| Language | Package | Install |
|---|---|---|
| TypeScript / JavaScript | [`@rafaelpontani/pulumi-taba`](https://www.npmjs.com/package/@rafaelpontani/pulumi-taba) | `npm install @rafaelpontani/pulumi-taba` |
| Python | [`pulumi-taba`](https://pypi.org/project/pulumi-taba/) | `pip install pulumi-taba` |
| .NET | [`Taba.Pulumi`](https://www.nuget.org/packages/Taba.Pulumi) | `dotnet add package Taba.Pulumi` |
| Go | `github.com/rafaelpontani/pulumi-taba/sdk/go/taba` | `go get github.com/rafaelpontani/pulumi-taba/sdk` |

The Pulumi CLI also needs the **plugin** of the provider, which is not in those packages. It is installed from the GitHub Release of the mirror repository (use the version of the package, which is the version of Taba):

```bash
pulumi plugin install resource taba 0.3.9 --server github://api.github.com/rafaelpontani/pulumi-taba
```

```typescript
import * as taba from "@rafaelpontani/pulumi-taba";

// pulumi config set taba:endpoint https://taba.example.com:8080   (or TABA_URL)
// pulumi config set --secret taba:token ...                       (or TABA_TOKEN)

const orders = new taba.Queue("orders", { name: "orders", ttl: 86400, maxDeliveries: 5 });

const token = new taba.Token("orders-app", {
    name: "orders-app",
    permissions: ["publish", "consume"],
    topicPrefixes: [orders.name],
});

export const secret = token.secret; // a Pulumi secret
```

- **Resources:** `Queue`, `Distributor`, `Token`. **Functions:** `getTopic`, `getTopics`, `getToken`, `getTokens`.
- **`name` is required** in the three resources: nothing generates it (no autonaming), because the name of a queue is a contract with the applications and is how `createOrUpdate` finds what exists.
- **The provider version is the Taba version** and targets the node of the same version.
- **The token secret** is in the Pulumi state, as a secret: keep the state in a backend that encrypts it. The property is called `secret` in every language (a C# class cannot have a property with its own name).
- **A queue with messages is not removed** (nor replaced) unless `forceDestroy: true` was applied first.
- **`createOrUpdate` is off by default**: turn it on (provider or resource, or `TABA_CREATE_OR_UPDATE=true`) only for programs meant to take over what was created by hand, by `taba-cli` or by an application.

License: [Apache-2.0](https://github.com/rafaelpontani/taba/blob/main/connectors/LICENSE).
