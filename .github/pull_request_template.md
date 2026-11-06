## What changes and why

<!-- A short description. Link the issue, if there is one: Closes #NNN -->

## Commit type (decides whether merging publishes a version)

- [ ] `fix:` / `perf:`: compatible change (patch version)
- [ ] `feat!:` / `BREAKING CHANGE:`: breaks an API, a configuration key, an on-disk format or the connectors' public API (upgrade documented in both languages)
- [ ] `docs:` / `test:` / `chore:` / `ci:` / `refactor:`: publishes nothing

## Checklist

- [ ] The pull request targets `release` (branch created from `release`)
- [ ] A behaviour change has a test, citing its use case on a `/// UC: UC-XXX-NNN` line
- [ ] Documentation updated in **both** `doc/pt-BR/` and `doc/en-US/`; `node doc/pt-BR/scripts/verificar.mjs` and `node doc/pt-BR/scripts/verificar-idiomas.mjs` pass
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all` pass (or the equivalent for the connector changed)
- [ ] `docker/validate.sh` run, if the change touches the cluster, the channel, Compose or the connectors
- [ ] Decisions and pending items recorded, if the change found any
- [ ] No file left in `doc/plans/`
- [ ] I have signed the [CLA](../CLA.md) (first contribution only)
