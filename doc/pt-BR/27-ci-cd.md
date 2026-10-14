---
título: CI/CD e releases
resumo: Como o repositório é validado e publicado — os workflows do GitHub Actions, as imagens Docker, o versionamento automático (semantic-release, candidatas e finais), as tags flutuantes e o que ainda não está automatizado.
tipo: referência
status: parcial
verificado-em: 2026-10-13 @ 71183b9
fontes: [.github/workflows, .github/actions/node-binary, .github/scripts/set-version.sh, .releaserc.json, Dockerfile, Dockerfile.ci, Dockerfile.cli.ci, docker/validate.sh]
---

# CI/CD e releases

> **Atenção ao nome:** "pipeline" aqui é o **CI/CD do repositório** (GitHub Actions), não o caminho das mensagens ([10-publicacao.md](10-publicacao.md), [12-entrega.md](12-entrega.md)). Leia antes: [24-convencoes.md](24-convencoes.md) (commits).

## Visão geral

| Fluxo | Quando | O que faz |
|---|---|---|
| **CI** (`ci.yml`) | A cada **pull request** | Chama os três builds (servidor, conector Rust, conector C#), **sem versão** |
| **Semantic** (`semantic.yml`) | A cada **push** em `release` ou `main` | Roda o semantic-release: calcula a versão pelos commits e cria a tag e a GitHub Release (nada, se o commit não gera versão) |
| **Release** (`release.yml`) | Quando uma **tag completa** (`vX.Y.Z` ou `vX.Y.Z-rc.N`) é criada | Compila e testa com a versão da tag, publica as imagens Docker e anexa binários e pacotes à Release. **Tags flutuantes não disparam** |

Os três builds são **workflows reutilizáveis** (`workflow_call`), usados pelo CI **e** pelo release, com a mesma lógica.

![Do pull request ao release](../diagramas/img/ci-cd.svg)

## Os workflows (`.github/workflows/`)

| Arquivo | Faz |
|---|---|
| `ci.yml` | `pull_request`: chama `build-server`, `build-connector-rust` e `build-connector-csharp`. `concurrency` cancela execuções antigas do mesmo PR |
| `build-server.yml` | **Servidor**, em **dois runners nativos** (`amd64` em `ubuntu-latest`, `arm64` em `ubuntu-24.04-arm`): `cargo fmt --all --check` e `clippy -D warnings` **só em x86_64** (o código é o mesmo), `cargo test --workspace --locked`, o **binário estático musl** (ação `node-binary`) enviado como artefato `taba-node-<arch>` |
| `build-connector-rust.yml` | **Conector Rust** (workspace `connectors/rust`), nos dois runners: `fmt`, `clippy`, `test`, `cargo build --release --locked -p taba-cli` (musl, `musl-tools`); artefato `taba-cli-<arch>` |
| `build-connector-csharp.yml` | **Conector C#**: SDKs 6.0 e 8.0, `dotnet test` (sem `TABA_URL`, os de integração são **ignorados**), `dotnet pack` dos **três pacotes** (`Taba.Client`, `.Abstractions`, `.Extensions.DependencyInjection`) como o artefato `nuget-packages` |
| `semantic.yml` | `push` em `main`/`release`: só cria a tag (semantic-release). Usa o secret `RELEASE_TOKEN` (PAT ou GitHub App), **não** o `GITHUB_TOKEN`: tags criadas com o `GITHUB_TOKEN` não disparam workflows |
| `release.yml` | O fluxo de release (abaixo), disparado pela tag |

O parâmetro de entrada `version` dos builds é **vazio em pull request**; no release recebe a versão da tag e o `set-version.sh` a grava nos arquivos **só dentro do build, sem commitar**.

## O release (`semantic.yml` e `release.yml`)

0. **`semantic.yml`**: a cada push, o job `preview` roda o semantic-release em **dry-run** e mostra no resumo da execução a versão que sairia (sem criar nada); o job `semantic` só roda se houver versão e **depois de aprovação manual** (environment `release` com *Required reviewers*). Aprovado, o **semantic-release** lê os commits desde a última tag e cria a **tag** (`vX.Y.Z`) e a **GitHub Release**. Sem commit relevante, nada é criado e o `release.yml` não roda. O push da tag dispara o `release.yml`. Os filtros de tag (`v[0-9]+.[0-9]+.[0-9]+` e `…-rc.[0-9]+`) deixam de fora as tags flutuantes, e o job `version` confere o formato de novo.
1. **`version`**: extrai a versão do nome da tag.
2. **`server`, `connector-rust`, `connector-csharp`** (em paralelo): compilam e **testam com a versão da tag**.
3. **`plan`**: calcula as tags Docker e as tags flutuantes do git (abaixo).
4. **`docker`** (Docker Hub): baixa os binários `taba-node-*` e `taba-cli-*` e monta **duas imagens multi-arquitetura** (`linux/amd64`, `linux/arm64`), **sem compilar nada**: `rafaelpontani/taba` (`Dockerfile.ci`) e `rafaelpontani/taba-cli` (`Dockerfile.cli.ci`, `ENTRYPOINT taba-cli`).
5. **`release-assets`**: anexa à GitHub Release os binários (com a arquitetura no nome: `x86_64` e `aarch64`) e os pacotes NuGet.
6. **Tags flutuantes**: move as tags `v0.1`, `v0` etc.

### Duas imagens, dois Dockerfiles

| Arquivo | Para quê |
|---|---|
| `Dockerfile` (raiz) | **Compila tudo dentro do contêiner** (`rust:1-alpine`, build estático musl) e gera a imagem final em Alpine. É o que **qualquer pessoa usa** para `docker build -t taba .` ou `docker compose up --build` |
| `Dockerfile.ci` | **Não compila**: copia o binário `dist/taba-${TARGETARCH}` já compilado (com o cache do runner). Build de segundos. Mesma imagem final |
| `Dockerfile.cli.ci` | O mesmo, para o `taba-cli` |

A imagem final: **Alpine**, só com o binário e os certificados de CA, **usuário sem privilégios** (`taba`, uid 10001), `VOLUME /var/lib/taba`, `EXPOSE 8080 8081 9090`, `ENTRYPOINT ["taba"]`.

## Versionamento (semantic-release)

Configurado em `.releaserc.json`: `branches: ["main", {"name": "release", "prerelease": "rc"}]`, `tagFormat: "v${version}"`, preset `conventionalcommits`, regra **`breaking → minor`** (enquanto o projeto está em `0.x`, um breaking change **não** salta para `1.0.0`).

| Branch | Gera | Exemplo |
|---|---|---|
| `release` | **Candidatas** (pré-release `rc`) | `v0.1.0-rc.1`, `rc.2`… |
| `main` | **Versões finais** | `v0.1.0`, `v0.1.1`… |

**Que commit gera versão:**

| Commit | Resultado |
|---|---|
| `feat:`, `fix:`, `perf:` | Nova versão (`rc.N` na `release`, final na `main`) |
| `feat!:`, `BREAKING CHANGE:` | Conta como **minor** (regra `breaking → minor`) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | **Nenhuma** versão |

**Fluxo:** 1) o desenvolvimento entra na `release` (cada commit relevante gera a próxima candidata, com imagem); 2) aprovada, a `release` é promovida à `main` (de preferência por PR `release → main`), que gera a final; 3) o ciclo seguinte volta à `release`: o primeiro `fix` dá `0.1.1-rc.1`, um `feat` dá `0.2.0-rc.1`; **a `main` deve ser mesclada de volta na `release`** para não divergir; 4) um *hotfix* pode ir direto na `main`, e depois se mescla na `release`.

**Uma versão para o repositório todo:** servidor, conector Rust e conector C# andam na **mesma tag**. `set-version.sh X.Y.Z[-rc.N]` ajusta o `Cargo.toml` e o `Cargo.lock` do servidor, os dos conectores Rust e o `<Version>` dos `.csproj`. No repositório os arquivos ficam em `0.1.0`; rode o script à mão para sincronizar. O `taba --version` da imagem reporta a versão da tag.

**Bootstrap:** o semantic-release começa em `1.0.0` sem tag anterior. Para começar em `0.1.0`, o repositório precisa de uma **tag semente estável** `v0.0.0` num commit que esteja nas duas branches. **Sair do `0.x`**: remover a regra `breaking → minor`.

### Tags Docker e tags flutuantes

Cada tag flutuante **só avança** se a versão publicada é a maior do seu escopo e da sua família (candidata ou final):

| Versão publicada | Tags Docker | Tags git flutuantes |
|---|---|---|
| `0.1.0-rc.2` | `0.1.0-rc.2`, `0.1-rc`, `0-rc`, `rc` | `v0.1-rc`, `v0-rc` |
| `0.1.0` | `0.1.0`, `0.1`, `0`, `latest` | `v0.1`, `v0` |

Em `0.x` o semver permite quebra a cada minor: **`0.1` e `v0.1` são os ponteiros seguros**; `0` e `v0` podem pular para um `0.2` incompatível.

## Credenciais (secrets do GitHub)

| Destino | Secret | Estado |
|---|---|---|
| Docker Hub | `DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN` | ✅ usado pelo `release.yml` |
| nuget.org | `NUGET_API_KEY` | ⬜ planejado (hoje só `dotnet pack`) |
| crates.io | `CARGO_REGISTRY_TOKEN` | ⬜ planejado |

nuget.org e crates.io **não permitem apagar versão publicada**: a publicação neles fica para o fim, atrás de uma aprovação manual.

## O que ainda não está automatizado

- ⬜ **Job de integração** (`connectors/csharp/scripts/integration.sh` e `docker/validate.sh`): **não roda no CI**. Existem e funcionam à mão; o plano era um job filtrado por caminho (é o mais caro: constrói a imagem no runner).
- ⬜ **Publicação** de NuGet e crates (e dos outros conectores).
- ⬜ **Registro de imagem no GHCR** (o plano previa; hoje só Docker Hub).
- ⬜ Verificação automática da **documentação** (links, `UC-`, parâmetros, rotas): ver `doc/pt-BR/scripts/verificar.mjs` abaixo.
- ⬜ O workflow `manual.yml` (`workflow_dispatch`) planejado.

## Verificação automática desta documentação

`node doc/pt-BR/scripts/verificar.mjs` (da raiz) confere, sem rede e sem compilar:

| Verificação | Falha quando |
|---|---|
| Links Markdown entre arquivos de `doc/pt-BR` e para o repositório | O arquivo ou a âncora não existe |
| Metadados | Falta `resumo`, `status`, `verificado-em` ou `fontes` no topo, ou falta a seção "Fontes e provas" |
| Parâmetros de configuração | Uma chave de `config/params/mod.rs` não aparece em [22-configuracao.md](22-configuracao.md), ou o documento cita uma chave inexistente |
| Códigos de erro | Um `ErrorCode` do código não aparece em [19-erros-e-limites.md](19-erros-e-limites.md) (ou vice-versa) |
| Rotas | Uma rota registrada no código (`.route(…)` e o mapa do cluster) não é citada nos documentos de API |
| Casos de uso | Um `UC-…` citado nos documentos não existe em `doc/20a-c` |
| Métricas | Uma métrica de `telemetry/metrics/defs.rs` não aparece em [13-observabilidade.md](13-observabilidade.md) |
| Diagramas | Toda imagem `../diagramas/img/…` existe, e todo bloco `🖼️ DIAGRAMA PLANEJADO` que sobrar traz `tipo:` e `objetivo:` |

Pode entrar no CI como um job de PR barato (só Node).

## Fontes e provas

- `.github/workflows/{ci,build-server,build-connector-rust,build-connector-csharp,release}.yml`, `.github/actions/node-binary/action.yml`, `.github/scripts/set-version.sh`, `.releaserc.json`, `Dockerfile`, `Dockerfile.ci`, `Dockerfile.cli.ci`.
- Casos de uso: não há casos `UC-` de pipeline; a validação é `docker/validate.sh` ([25-testes.md](25-testes.md)).
- Histórico: `21-pipeline-ci-cd.md` (**está parcialmente velho**: diz "Não há `.github/`" e planeja o job `integration` e o GHCR, que não existem).
