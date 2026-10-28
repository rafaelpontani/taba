---
título: CI/CD e releases
resumo: Como o repositório é validado e publicado — os workflows do GitHub Actions, a ordem do release (nada público antes de tudo verde), o versionamento automático (semantic-release, candidatas e finais), as tags flutuantes por digest, a publicação em Docker Hub, GHCR, crates.io e nuget.org, e como qualquer pessoa verifica que um artefato saiu deste pipeline.
tipo: referência
status: estável
verificado-em: 2026-10-27 @ ada9ff1
fontes: [.github/workflows, .github/actions/node-binary, .github/scripts/set-version.sh, .github/dependabot.yml, .releaserc.json, .github/actions/registry-login, .github/actions/provenance, Dockerfile, Dockerfile.ci, connectors/csharp/Directory.Build.props, connectors/rust/Cargo.toml, SECURITY.md]
---

# CI/CD e releases

> **Atenção ao nome:** "pipeline" aqui é o **CI/CD do repositório** (GitHub Actions), não o caminho das mensagens ([10-publicacao.md](10-publicacao.md), [12-entrega.md](12-entrega.md)). Leia antes: [24-convencoes.md](24-convencoes.md) (commits).

## Visão geral

| Fluxo | Quando | O que faz |
|---|---|---|
| **CI** (`ci.yml`) | **Pull request** para `main`/`release` (e manual, para uma branch sem PR). Não roda em `push`: com um PR aberto rodaria tudo duas vezes | Os três builds (servidor, conector Rust, conector C#) **sem versão**, o teste estático do chart Helm e o lint dos workflows. **Não publica nada** |
| **Release** (`release.yml`) | **Push** em `main` (finais) ou `release` (candidatas) | Os mesmos builds; se o semantic-release encontra versão, publica tudo **na ordem abaixo**, com a tag criada só depois de tudo verde |
| **Scorecard** (`scorecard.yml`) | Push na `main`, semanal e a cada mudança de proteção | A nota pública do OpenSSF Scorecard (práticas de segurança do repositório) |
| **CodeQL** (`codeql.yml`) | Push em `main`/`release`, pull request para elas e semanal | Análise estática (SAST) do código Rust, C# e dos workflows; achados na aba Security |

Os três builds são **workflows reutilizáveis** (`workflow_call`), usados pelo CI **e** pelo release, com a mesma lógica.

![Do pull request ao release](../diagrams/img/pt-BR/ci-cd.svg)

## Fluxo de branches e tags

```
feature/*  ── PR ──▶ ci.yml: build + testes (nada é publicado, nenhuma tag)
    │
    └──PR──▶ release ── release.yml ──▶ tag fixa  v0.1.0-rc.28        (imutável, nunca move)
                │                         git flutuante: v0.1-rc, v0-rc  (movem p/ o novo commit)
                │                         docker:  0.1.0-rc.28 + 0.1-rc, 0-rc, rc  (mesmo digest)
                │
                └──PR──▶ main ── release.yml ──▶ tag fixa  v0.1.0
                           │                       git flutuante: v0.1, v0
                           │                       docker:  0.1.0 + 0.1, 0, latest  (mesmo digest)
                           └──back-merge──▶ release
```

1. O desenvolvimento entra na `release` por PR: cada commit `feat`/`fix`/`perf` gera a próxima candidata, com imagem.
2. Aprovada, a `release` é promovida à `main` por PR `release → main`, que gera a final.
3. A `main` é mesclada de volta na `release` para não divergir; o ciclo seguinte recomeça lá.

## Os workflows (`.github/workflows/`)

| Arquivo | Faz |
|---|---|
| `ci.yml` | Chama `build-server`, `build-connector-rust`, `build-connector-csharp` e os builds dos conectores Java, Node.js e Python e do driver do OMB (`build-connector-omb.yml`); job `connector-contract` (`contract-connectors.yml`, só em pull requests para `release`); job `helm` (`deploy/helm/tests/run.sh static`); job `rust-audit` (`cargo audit` nos dois `Cargo.lock`: falha com um aviso do RustSec); job `workflows` (`actionlint` e `zizmor`, falha a partir de `medium`); job `plans` (falha quando `doc/plans/` tem um arquivo: planos nunca chegam a `release` ou `main`). `concurrency` cancela execuções antigas da mesma ref |
| `build-server.yml` | **Servidor**, em **dois runners nativos** (`amd64` em `ubuntu-latest`, `arm64` em `ubuntu-24.04-arm`): `cargo fmt --all --check` e `clippy -D warnings` **só em x86_64**, `cargo test --workspace --locked`, o **binário estático musl** (ação `node-binary`) enviado como artefato `taba-node-<arch>` |
| `build-connector-rust.yml` | **Conector Rust** (workspace `connectors/rust`), nos dois runners: `fmt`, `clippy`, `test`, `cargo package --workspace` (o que o crates.io receberia), `cargo build --release -p taba-cli` (musl); artefato `taba-cli-<arch>` |
| `contract-connectors.yml` | **Testes de contrato dos cinco conectores** contra um nó real: constrói a imagem do servidor do commit, sobe um nó (o perfil de um nó) e roda `connectors/contract/run.sh` (cada conector no contêiner da sua menor versão, depois os cinco lendo as mensagens uns dos outros; [21-conectores.md](21-conectores.md#os-testes-de-contrato)). Chamado pelo `ci.yml` em pull requests para `release`, ou à mão. Não publica nada |
| `build-connector-csharp.yml` | **Conector C#**: SDKs 6.0 e 8.0, `dotnet test` (sem `TABA_URL`, os de integração são **ignorados**), `dotnet pack` dos **três pacotes** com o commit e o link da Release; artefato `nuget-packages` (`.nupkg` e `.snupkg`) |
| `chaos.yml` | Toda noite (e à mão): uma varredura de 200 sementes da rodada de caos (`tests/chaos.rs`, [37-garantias.md](37-garantias.md)), uma faixa diferente por dia. Fora dos checks obrigatórios; a semente que falha é impressa no log |
| `release.yml` | O release (abaixo) |
| `scorecard.yml` | `ossf/scorecard-action`; o resultado vai para a aba Security e para o badge do README |
| `codeql.yml` | CodeQL para `rust`, `csharp` e `actions`, todos com `build-mode: none` (independente das toolchains dos builds) |

A entrada `version` dos builds é **vazia** quando não há release; com versão, o `set-version.sh` a grava nos arquivos **só dentro do build, sem commitar**. Todo build exporta `TABA_GIT_SHA`: `taba --version` e `taba-cli --version` mostram o commit.

## O release (`release.yml`)

Princípio: **nada público antes de tudo estar verde, e tudo o que é público aponta para o mesmo commit** (`github.sha`). `concurrency: release-<ref>` sem cancelamento: um push novo espera o anterior terminar.

| # | Job | O que faz |
|---|---|---|
| 1 | `plan` | semantic-release em **dry-run** calcula a versão. Sem versão, só os builds rodam. Calcula as tags Docker e as flutuantes do git (a versão nova entra na comparação, porque sua tag ainda não existe) |
| 2 | `server`, `connector-rust`, `connector-csharp` | Compilam e **testam com a versão**. Rodam a cada push, mesmo sem versão |
| 3 | `chart` | `set-version.sh` grava `version` e `appVersion` exatos no `Chart.yaml`; `run.sh static`; `helm package` |
| 4 | `images` | Monta `taba` e `taba-cli` (os dois *targets* do `Dockerfile.ci`) para `linux/amd64` e `linux/arm64`, **sem compilar nada**, com proveniência SLSA (`mode=max`), SBOM e labels OCI (`revision` = commit). Envia **só por digest, sem tag**, para Docker Hub e GHCR; atesta (`attest-build-provenance`) e assina (cosign keyless) cada digest |
| 5 | `tag` | Só agora o semantic-release cria a **tag** em `github.sha` e a **GitHub Release como rascunho**. Falha se a versão não é a do plano ou se a tag não aponta para o commit |
| 6 | `release-assets` | Binários (com a arquitetura no nome), `.nupkg`, `.snupkg`, o `.tgz` do chart e o `SHA256SUMS`, todos atestados, no rascunho, mais o `taba-<versão>.intoto.jsonl` (o bundle da proveniência como arquivo da Release, que o OpenSSF Scorecard reconhece) |
| 7 | `crates`, `nuget`, `chart-publish` | crates.io e nuget.org por **Trusted Publishing** (OIDC); o chart em `oci://ghcr.io/rafaelpontani/charts`, atestado e assinado |
| 8 | `finalize` | A Release sai do rascunho: `--prerelease` nas candidatas, `--latest` só na maior final |
| 9 | `floating` | Cria as tags Docker (exata e flutuantes) com `docker buildx imagetools create` **apontando para o digest** (sem reconstruir) e move as tags flutuantes do git |
| 10 | `verify` | Confere de fora: cada tag git aponta para o commit; cada tag Docker, nos dois registros, é o mesmo digest, com o label `revision` certo; `cosign verify` e `gh attestation verify` passam; o `SHA256SUMS` da Release confere |

Se a branch andar entre o `plan` e o `tag`, o semantic-release recusa ("local branch behind remote"), o job `tag` falha e a execução seguinte, já na fila, publica o commit novo. Nada público fica para trás: as imagens ainda não têm tag.

`workflow_dispatch` (na `main` ou na `release`) com `dry_run` (padrão `true`) roda até o build das imagens **sem enviar** e para antes do `tag`: é o ensaio de um release.

### Duas imagens, um Dockerfile de CI

| Arquivo | Para quê |
|---|---|
| `Dockerfile` (raiz) | **Compila tudo dentro do contêiner** (`rust:1-alpine`, build estático musl; as duas bases **fixadas por digest**, como no `connectors/rust/cli/Dockerfile`) e gera a imagem final em Alpine. É o que **qualquer pessoa usa** para `docker build -t taba .` ou `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build` |
| `Dockerfile.ci` | **Não compila**: um *target* por imagem (`taba` copia `dist/taba-${TARGETARCH}`, `taba-cli` copia `dist/taba-cli-${TARGETARCH}`), binários já compilados com o cache do runner. Os dois partem do mesmo estágio `base`, com `alpine:3` **fixado por digest** num lugar só |

**Mantendo as versões em dia sem editar SHA à mão:** o GitHub não aceita variável no `uses:`, então cada action fica fixada por SHA onde é usada. As repetidas (login nos registros, atestação e assinatura) vivem nas actions locais `.github/actions/registry-login` e `.github/actions/provenance`, e o Dependabot atualiza todas as ocorrências num PR agrupado. O digest da base só existe no `Dockerfile.ci`; a atualização dele chega como `fix` e gera uma versão nova (`rc` na `release`), então a imagem publicada recebe as correções de segurança da base sem esperar uma feature. Os pacotes do `apk` são sempre os mais recentes no momento do build. O Dependabot espera 7 dias (`cooldown`) antes de propor uma versão recém-publicada.

A imagem final: **Alpine**, só com o binário e os certificados de CA, **usuário sem privilégios** (`taba`, uid 10001), `VOLUME /var/lib/taba`, `EXPOSE 8080 8081 9090`, `ENTRYPOINT ["taba"]`.

## Versionamento (semantic-release)

Configurado em `.releaserc.json`: `branches: ["main", {"name": "release", "prerelease": "rc"}]`, `tagFormat: "v${version}"`, preset `conventionalcommits`, regra **`breaking → minor`** (enquanto o projeto está em `0.x`, um breaking change **não** salta para `1.0.0`) e `draftRelease: true` (a Release nasce rascunho e só o `finalize` a publica).

| Branch | Gera | Exemplo |
|---|---|---|
| `release` | **Candidatas** (pré-release `rc`) | `v0.1.0-rc.1`, `rc.2`… |
| `main` | **Versões finais** | `v0.1.0`, `v0.1.1`… |

**Que commit gera versão:**

| Commit | Resultado |
|---|---|
| `feat:`, `fix:`, `perf:` | Nova versão (`rc.N` na `release`, final na `main`) |
| `feat!:`, `BREAKING CHANGE:` | Conta como **minor** (regra `breaking → minor`) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | **Nenhuma** versão (só os builds rodam) |

**Uma versão para o repositório todo:** servidor, conectores Rust e C# e o chart andam na **mesma tag**. `set-version.sh X.Y.Z[-rc.N]` ajusta o `Cargo.toml` e o `Cargo.lock` do servidor, os dos conectores Rust (e a dependência `taba-client = "=X"` do `taba-cli`), o `<Version>` dos `.csproj` e o `version`/`appVersion` do `Chart.yaml`. No repositório os arquivos ficam em `0.1.0`.

**Bootstrap:** o semantic-release começa em `1.0.0` sem tag anterior. Para começar em `0.1.0`, o repositório precisa de uma **tag semente estável** `v0.0.0` no **primeiro commit do repositório** (`git rev-list --max-parents=0 HEAD`), que está nas duas branches; sem ela, o job `plan` para o release. **Sair do `0.x`**: remover a regra `breaking → minor`.

### Tags Docker e tags flutuantes

Só a tag **fixa** (`vX.Y.Z`, `vX.Y.Z-rc.N`) é imutável. As flutuantes são ponteiros e cada uma **só avança** se a versão publicada é a maior do seu escopo e da sua família (candidata ou final):

| Versão publicada | Tags Docker | Tags git flutuantes |
|---|---|---|
| `0.1.0-rc.2` | `0.1.0-rc.2`, `0.1-rc`, `0-rc`, `rc` | `v0.1-rc`, `v0-rc` |
| `0.1.0` | `0.1.0`, `0.1`, `0`, `latest` | `v0.1`, `v0` |

- Um hotfix `0.1.5` publicado depois de `0.2.0` move `0.1`/`v0.1`, mas **não** `0`, `v0` nem `latest`.
- Candidatas e finais não se misturam: uma `rc` nunca move `0.1`/`latest`; uma final nunca move `0.1-rc`.
- Em `0.x` o semver permite quebra a cada minor: **`0.1` e `v0.1` são os ponteiros seguros**; `0` e `v0` podem pular para um `0.2` incompatível.
- Toda tag Docker de uma versão é o **mesmo digest** (criada por `imagetools create`, não por um novo build): a proveniência e a assinatura do digest valem para todas.

## Publicação e metadados

| Destino | O que | Como | Liga quando |
|---|---|---|---|
| Docker Hub | `rafaelpontani/taba`, `rafaelpontani/taba-cli` | Secrets `DOCKERHUB_USERNAME` e `DOCKERHUB_TOKEN` | Sempre |
| GHCR | `ghcr.io/rafaelpontani/taba`, `taba-cli` e `charts/taba` | `GITHUB_TOKEN` (`packages: write`) | Sempre |
| GitHub Release | Binários, pacotes, chart, `SHA256SUMS` | `GITHUB_TOKEN` | Sempre |
| crates.io | `taba-client`, depois `taba-cli` | Trusted Publishing (`rust-lang/crates-io-auth-action`); o primeiro publish de cada crate usa o secret `CARGO_REGISTRY_TOKEN` | Variável `PUBLISH_CRATES=true` |
| nuget.org | `Taba.Client`, `.Abstractions`, `.Extensions.DependencyInjection` (+ `.snupkg`) | Trusted Publishing (`NuGet/login`) | Variável `NUGET_USER` (a conta dona da política) |

nuget.org e crates.io **não permitem apagar versão publicada**; os dois jobs são idempotentes (pulam a versão que já existe) e as candidatas saem como pré-release semver, que `cargo add`/`dotnet add` não escolhem sem pedir. Os jobs que publicam rodam no environment **`release`** (restrinja-o às branches `main` e `release`; a aprovação manual das finais é opcional).

**Metadados dos pacotes:**

- **NuGet** (`connectors/csharp/Directory.Build.props`): `RepositoryUrl`, `PackageProjectUrl`, `PackageReadmeFile`, build determinístico, SourceLink, `.snupkg`; o build passa `RepositoryCommit` (vira `<repository commit>` no `.nuspec`) e `PackageReleaseNotes` (o link da Release).
- **Cargo** (`connectors/rust/*/Cargo.toml`): `repository`, `homepage`, `readme`, `keywords`, `categories`; o `cargo publish` grava o commit em `.cargo_vcs_info.json`. Como o `set-version.sh` altera arquivos, o publish usa `--allow-dirty` e o arquivo marca `"dirty": true`; o vínculo verificável é o Trusted Publishing, que o crates.io mostra com o link da execução.
- **Imagens**: labels `org.opencontainers.image.{source,url,revision,version,created,licenses}`.

## Confiança: como verificar um artefato

Qualquer pessoa confere, sem confiar na palavra do repositório, que um artefato saiu deste workflow e de qual commit:

```bash
gh attestation verify oci://ghcr.io/rafaelpontani/taba:0.1.0 -R rafaelpontani/taba
cosign verify ghcr.io/rafaelpontani/taba:0.1.0 \
  --certificate-identity-regexp '^https://github.com/rafaelpontani/taba/.github/workflows/release.yml@refs/heads/(main|release)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
gh release download v0.1.0 -R rafaelpontani/taba && sha256sum -c SHA256SUMS
gh attestation verify taba-0.1.0-x86_64-unknown-linux-musl -R rafaelpontani/taba
```

> **Repositório privado:** as atestações do GitHub exigem repositório público (ou GitHub Enterprise Cloud), e uma assinatura de repositório privado apontaria para um workflow que ninguém vê. Enquanto o repositório é privado, o `plan` marca `signed=false`: atestações, assinaturas cosign e o Scorecard são pulados (o `verify` diz isso no resumo) e o resto do pipeline (a ordem, o digest único, as tags e os metadados dos pacotes) segue igual. Ao tornar o repositório público, eles ligam sozinhos.

O que sustenta essa confiança:

- **Actions fixadas por SHA** (com o comentário da versão) e atualizadas pelo Dependabot (`.github/dependabot.yml`: actions, cargo nos dois workspaces, nuget e as bases Docker; os PRs vão para a `release` com prefixo `ci`/`chore`, que não gera versão).
- **Permissões mínimas:** `permissions: {}` no topo e cada job pede só o que usa; `checkout` com `persist-credentials: false`.
- **Sem segredos de longa duração** para crates.io e nuget.org (OIDC); assinaturas keyless no log público do Sigstore (Rekor).
- **Lint dos workflows** (`actionlint`, `zizmor`), **auditoria das dependências** (`cargo audit`) e **CodeQL** em todo PR, e o **OpenSSF Scorecard** público.
- [SECURITY.md](../../SECURITY.md): como relatar uma vulnerabilidade e quais versões recebem correção.

### Configuração do repositório (fora do código)

| Onde | O quê |
|---|---|
| Rulesets de branch (`main`, `release`) | PR obrigatório, os checks do `ci.yml`, sem force-push, sem apagar |
| Ruleset de tag (`v*.*.*`) | Só o GitHub Actions cria; ninguém atualiza nem apaga. As flutuantes (`v0`, `v0.1`, `v0.1-rc`) não casam com o padrão e podem mover |
| Releases | **Immutable Releases** ligado: depois de publicada, nem a tag nem os assets mudam |
| Environment `release` | Restrito às branches `main` e `release`; guarda os secrets do Docker Hub |
| Variáveis | `PUBLISH_CRATES`, `NUGET_USER`, `DOCKERHUB_NAMESPACE` (padrão `rafaelpontani`) |
| crates.io / nuget.org | O trusted publisher: repositório `rafaelpontani/taba`, workflow `release.yml`, environment `release` |

## O que ainda não está automatizado

- 🟡 **Job de integração**: os testes de contrato dos conectores (`contract-connectors.yml`) **rodam no CI**, em pull requests para `release`. `connectors/csharp/scripts/integration.sh` e `docker/validate.sh` **não rodam no CI**. Existem e funcionam à mão; rode o `validate.sh` com a imagem `rc` antes do PR `release → main`.
- ⬜ Os casos e2e do chart (`deploy/helm/tests/run.sh 01 05 …`, com kind) rodam à mão.
- ⬜ Publicação dos outros conectores (só existem Rust e C#).
- ⬜ Verificação automática da **documentação** no CI: ver `doc/pt-BR/scripts/verificar.mjs` abaixo.

## Verificação automática desta documentação

`node doc/pt-BR/scripts/verificar.mjs` (da raiz) confere, sem rede e sem compilar:

| Verificação | Falha quando |
|---|---|
| Links Markdown entre arquivos de `doc/pt-BR` e para o repositório | O arquivo ou a âncora não existe |
| Metadados | Falta `resumo`, `status`, `verificado-em` ou `fontes` no topo, ou falta a seção "Fontes e provas" |
| Parâmetros de configuração | Uma chave de `config/params/mod.rs` não aparece em [22-configuracao.md](22-configuracao.md), ou o documento cita uma chave inexistente |
| Códigos de erro | Um `ErrorCode` do código não aparece em [19-erros-e-limites.md](19-erros-e-limites.md) (ou vice-versa) |
| Rotas | Uma rota registrada no código (`.route(…)` e o mapa do cluster) não é citada nos documentos de API |
| Casos de uso | Um `UC-…` citado nos documentos não existe nos cartões de 26a/26b |
| Métricas | Uma métrica de `telemetry/metrics/defs.rs` não aparece em [13-observabilidade.md](13-observabilidade.md) |
| Diagramas | Toda imagem `../diagrams/img/…` existe, e todo bloco `🖼️ DIAGRAMA PLANEJADO` que sobrar traz `tipo:` e `objetivo:` |

Um segundo script, `node doc/pt-BR/scripts/verificar-idiomas.mjs`, confere que as pastas de idioma dizem a mesma coisa: todo documento existe em todas as pastas (pelo prefixo numérico), os metadados têm as chaves de cada idioma (`summary`, `sources`, `verified-on` em `en-US`), links e âncoras resolvem dentro de cada pasta, e os códigos `UC-…`, `D…` e `PRB-…`, os blocos de código, as imagens e as tabelas são os mesmos dos dois lados. Os dois podem entrar no CI como um job de PR barato (só Node).

## Fontes e provas

- `.github/workflows/{ci,build-server,build-connector-rust,build-connector-csharp,contract-connectors,release,scorecard,codeql}.yml`, `.github/actions/node-binary/action.yml`, `.github/scripts/set-version.sh`, `.github/dependabot.yml`, `.releaserc.json`, `.github/actions/{registry-login,provenance}/action.yml`, `Dockerfile`, `Dockerfile.ci`, `connectors/csharp/Directory.Build.props`, `connectors/rust/{Cargo.toml,client/Cargo.toml,cli/Cargo.toml}`, `SECURITY.md`.
- Decisões: D159 a D163 ([30-decisoes.md](30-decisoes.md)).
- Casos de uso: não há casos `UC-` de pipeline; a validação é `docker/validate.sh` ([25-testes.md](25-testes.md)).
