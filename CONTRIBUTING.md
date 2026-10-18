# Contributing to Taba

Thank you for your interest. Issues, documentation fixes, tests and code are all welcome.

## Licenses

The repository has two licenses:

| Part | License |
|---|---|
| The server (`crates/`, the `taba` image, the Helm chart, the docs and the rest of the repository) | [AGPL-3.0-only](LICENSE) |
| The connectors (`connectors/`: `taba-client`, `taba-cli`, the `Taba.Client*` packages and the `taba-cli` image) | [Apache-2.0](connectors/LICENSE) |

A contribution enters under the license of the folder it changes.

## Contributor License Agreement (CLA)

Before your first pull request is merged, you sign the [Taba Individual CLA](CLA.md), once, for every future
contribution. You keep the copyright of what you write. The CLA gives the maintainer the right to distribute your
contribution under the project's licenses, and also under a commercial license offered alongside the AGPL.

The CLA Assistant bot checks every pull request. If you have not signed yet, it leaves a comment with a link: open
it, sign in with GitHub and accept. The check turns green and the pull request can be merged. If a commit's email
is not linked to your GitHub account, the bot cannot match it: add the email to your account, or amend the commit.

## Before opening a pull request

- Read [AGENTS.md](AGENTS.md): the build, the tests, the rules of the code and of the documentation (every change
  to the docs goes to both `doc/pt-BR/` and `doc/en-US/`).
- Run the checks of the part you changed:
  - server: `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all`;
  - Rust connectors: the same, in `connectors/rust` with `--workspace`;
  - C# connector: `dotnet test` in `connectors/csharp`;
  - docs: `node doc/pt-BR/scripts/verificar.mjs`.
- Write commit messages in the [Conventional Commits](https://www.conventionalcommits.org/) format (`feat:`, `fix:`,
  `docs:`, `test:`...): the version of each release is computed from them.

To report a vulnerability, do not open an issue: follow [SECURITY.md](SECURITY.md).
