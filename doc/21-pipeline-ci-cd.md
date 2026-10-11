# 21 — Pipeline de CI/CD (GitHub Actions)

Plano do pipeline do Taba. Status: **fase 1 implementada** (`.github/workflows/ci.yml`, `release.yml` e `.releaserc.json`): build, testes, semantic-release e imagem no Docker Hub. Versão única para todo o repositório (seção 7).

## 1. O que o repositório tem hoje

| Componente | Onde | Build/teste | Publicável como |
|---|---|---|---|
| Servidor (Rust, edition 2024) | `crates/taba-node` | `cargo test`, Dockerfile musl estático | imagem Docker (GHCR) e binário `taba` |
| Conector Rust (workspace próprio) | `connectors/rust` | `cargo test` | crates `taba-client` e `taba-cli` |
| Conector C# (net461 a net10) | `connectors/csharp` | `dotnet test`, e `scripts/integration.sh` com cluster Docker | NuGet `Taba.Client`, `.Abstractions` e `.Extensions.DependencyInjection` |
| Validação ponta a ponta | `docker/validate.sh` | sobe o compose com 3 nós e o HAProxy | não publica |

Não há `.github/`. O remoto é `github.com/rafaelpontani/taba` e os commits já seguem Conventional Commits (`feat(connectors/csharp): …`, `fix(cluster): …`), o que serve para gerar as notas da Release.

## 2. Limites do runner gratuito

Números do conhecimento do autor, não verificados no momento da escrita: confirmar em *Settings → Billing*.

- **Repositório público:** minutos ilimitados nos runners hospedados padrão. Se o Taba for público, o limite praticamente não existe.
- **Repositório privado no plano Free:** cerca de 2.000 min/mês no Linux. Windows conta 2x e macOS 10x, então usar só `ubuntu-latest`.
- **Por job:** máximo de 6 h. Máximo de 20 jobs concorrentes no Free.
- **`ubuntu-latest`:** 4 vCPU, 16 GB de RAM e cerca de 14 GB de disco no repo público; no privado, 2 vCPU e 7 GB de RAM. O `target/` do Rust cresce muito, então o disco importa.
- **Cache:** 10 GB por repositório, com expiração em 7 dias sem uso.
- **Artifacts e Packages:** cerca de 500 MB no Free privado. No GHCR, imagens de repo público são gratuitas.
- **Docker no runner:** Docker e plugin compose já vêm instalados. `integration.sh` e `validate.sh` rodam como estão, com a porta 8080 livre.

## 3. Equivalências GitLab para GitHub

| GitLab | GitHub |
|---|---|
| `spec: inputs` | `workflow_dispatch: inputs`, com tipos `choice`, `boolean`, `string` e `environment`. Não há `regex`: validar num primeiro step. Limite de 25 inputs. |
| `include` e components | Reusable workflows (`workflow_call`) e composite actions em `.github/actions/` |
| `rules:` / `only:` | `on:` com `paths:` e `if:` por job |
| `when: manual` | `environment` com *required reviewers* (grátis em repo público; em privado exige plano pago) |
| `needs`, `artifacts`, `cache` | `needs`, `actions/upload-artifact` e `actions/cache` (ou `Swatinem/rust-cache`) |
| Container Registry | GHCR com o `GITHUB_TOKEN`, sem segredo extra |
| Variáveis protegidas | Secrets e variables por environment |

## 4. Estrutura proposta

```
.github/
  workflows/
    ci.yml          # PR e push em main: roda por caminho alterado
    release.yml     # push na main: CI, semantic-release, imagem no Docker Hub e tags flutuantes (seção 7)
    manual.yml      # workflow_dispatch com inputs (opcional, fase 2)
  actions/
    setup-rust/     # composite: toolchain, cache, clippy
```

### 4.1 ci.yml (todo PR e push em main)

Uma detecção de mudança (`dorny/paths-filter`) decide quais jobs rodam, para economizar minutos:

1. **server**: `cargo fmt --check`, `cargo clippy -D warnings` e `cargo test --locked`, com `Swatinem/rust-cache`.
2. **connector-rust**: o mesmo, com `working-directory: connectors/rust`.
3. **connector-csharp**: `dotnet test` unitário, com SDK do .NET 8 e do 10 (o csproj condiciona o `net10.0` ao SDK).
4. **integration**: só em push na main, ou em PR que toque `docker/**`, `Dockerfile`, `crates/**` ou `connectors/**`. Roda `integration.sh` e depois `docker/validate.sh`. É o job mais caro (build Docker do servidor), por isso fica atrás de filtro de caminho.
5. **docker-build**: `docker build` sem push, só para garantir que o Dockerfile compila.

Os jobs usam `concurrency` com `cancel-in-progress` em PRs, para não gastar minuto com push antigo.

### 4.2 release.yml (só em main)

1. Roda depois do CI verde (`workflow_run`, ou reaproveitando o `ci.yml` por `workflow_call`).
2. **semantic-release** lê os commits desde a última tag e cria a tag e a GitHub Release (detalhes na seção 7).
3. Com a versão definida, jobs em paralelo:
   - **docker**: build multi-arch (amd64 e arm64) com buildx, push para `ghcr.io/rafaelpontani/taba:X.Y.Z` e `:latest`.
   - **binários**: `taba` e `taba-cli` anexados à Release, musl estático.
   - **nuget**: `dotnet pack` e `dotnet nuget push` para o nuget.org (secret `NUGET_API_KEY`).
   - **crates**: `cargo publish` do `taba-client` e do `taba-cli` (secret `CARGO_REGISTRY_TOKEN`). Pode ficar desligado até a publicação ser desejada.

### 4.3 Onde ficam os artefatos

| Artefato | Destino | Observação |
|---|---|---|
| Imagem Docker do servidor | GHCR: `ghcr.io/rafaelpontani/taba:X.Y.Z` e `:latest` | Grátis em repo público; em privado conta nos 500 MB de Packages. Usa `GITHUB_TOKEN`. |
| Binários `taba` e `taba-cli` | GitHub Releases, na tag `vX.Y.Z` | Grátis. Limite de 2 GB por arquivo. |
| Pacotes NuGet | nuget.org (recomendado) ou GitHub Packages NuGet | O GitHub Packages exige autenticação de quem consome, mesmo em pacote público. nuget.org exige `NUGET_API_KEY`. |
| Crates Rust | crates.io | Único destino possível (o GitHub Packages não tem Cargo). Exige `CARGO_REGISTRY_TOKEN` e nome livre. |
| Artifacts do CI | GitHub Actions artifacts | Só depuração, retenção de 1 a 7 dias. |

nuget.org e crates.io não permitem apagar versão publicada: a publicação neles é a última fase e fica atrás de um *environment* com aprovação manual.

## 5. Decisões tomadas

1. **Tag única** (`vX.Y.Z`) para todo o repositório: servidor, conector Rust e conector C# andam na mesma versão.
2. **Repositório privado por enquanto**, até os testes internos; depois fica público.
3. **Destinos finais:** GHCR, Docker Hub, nuget.org e crates.io, mais binários na Release.

### 5.1 Fases por visibilidade do repositório

| | Fase privada (agora) | Fase pública (depois) |
|---|---|---|
| CI | `ci.yml` completo; atenção aos 2.000 min/mês, só `ubuntu-latest`, `integration` filtrado por caminho | igual, sem limite de minutos |
| Release | tag, GitHub Release, imagem no GHCR | igual, mais Docker Hub, nuget.org e crates.io |
| nuget.org e crates.io | só `dotnet pack` e `cargo publish --dry-run` (versões publicadas não podem ser apagadas) | publicação de verdade |
| Gate de publicação | `workflow_dispatch` manual ou condição na tag (required reviewers em repo privado exige plano pago) | *environment* `release` com aprovação manual |

### 5.2 Credenciais dos registries

| Destino | Conta | Credencial | Secret no GitHub |
|---|---|---|---|
| Docker Hub | hub.docker.com | *Account settings → Personal access tokens*, permissão Read & Write | `DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN` |
| nuget.org | nuget.org, com conta Microsoft | *Perfil → API Keys*: *Push new packages and package versions*, glob `Taba.*`, expira em no máximo 365 dias | `NUGET_API_KEY` |
| crates.io | crates.io, com conta do GitHub e e-mail verificado | *Account Settings → API Tokens*, escopos `publish-new` e `publish-update` | `CARGO_REGISTRY_TOKEN` |

Antes de publicar:

- Docker Hub: o namespace é o usuário (`rafaelpontani/taba`); para um nome neutro, criar uma *organization*. O plano Free tem repositórios públicos ilimitados e um privado.
- nuget.org: conferir se os três IDs `Taba.*` estão livres e considerar reservar o prefixo `Taba.`.
- crates.io: conferir se `taba-client` e `taba-cli` estão livres; o primeiro `cargo publish` reserva o nome.
- crates.io e nuget.org têm *Trusted Publishing* via OIDC (sem token de longa duração). Confirmar os detalhes atuais na documentação de cada um quando chegar a publicação.

## 6. Ordem de implementação

1. `ci.yml` com os quatro jobs unitários e o `docker-build`. Já dá valor sozinho.
2. Job `integration` com o filtro de caminho.
3. `release.yml` com semantic-release, Release e tags flutuantes.
4. Imagem no GHCR e binários na Release.
5. `dotnet pack` e `cargo publish --dry-run` no release (validação, sem publicar).
6. Com o repo público e os secrets criados: Docker Hub, nuget.org e crates.io.
7. `manual.yml` com `workflow_dispatch`, se desejado. Hoje não há ação operacional equivalente, porque o Taba não gerencia VMs.

## 7. Versionamento e release (decidido)

O **semantic-release** gera as versões e as tags automaticamente, a cada push na `main`, a partir dos commits (Conventional Commits). Nenhuma tag é criada à mão.

Enquanto o projeto está no ciclo `0.1`, a `main` é uma branch de **pré-release** (`rc`): `.releaserc.json` tem `branches: ["release", {"name": "main", "prerelease": "rc"}]`. Cada commit relevante gera a próxima candidata: `v0.1.0-rc.1`, `rc.2`, `rc.3`...

### 7.1 Que commit gera versão

| Commit | Resultado |
|---|---|
| `feat:`, `fix:`, `perf:` | nova `rc.N` |
| `feat!:` ou `BREAKING CHANGE:` | também só uma nova `rc.N` (a regra `breaking -> minor` impede o salto para `1.0.0-rc.1`) |
| `docs:`, `chore:`, `ci:`, `refactor:`, `test:` | nenhuma versão |

O preset é o `conventionalcommits`, que entende o `!`. O preset padrão (`angular`) ignora `feat!:`. Testado em repositório descartável.

### 7.2 Preparação única (bootstrap)

O semantic-release começa em `1.0.0` quando não há tag anterior, e a branch `main` de pré-release exige que exista uma branch de release estável. Por isso o repositório precisa de:

1. uma **tag semente** `v0.1.0-rc.0`, com a anotação de canal `{"channels":["main"]}` (o canal de uma branch de pré-release tem o nome da branch). Sem a anotação o semantic-release a ignora;
2. uma branch `release`, ainda sem uso, reservada para as versões estáveis.

```bash
git tag v0.1.0-rc.0 <commit-base>
git notes --ref semantic-release-v0.1.0-rc.0 add -m '{"channels":["main"]}' v0.1.0-rc.0
git push origin v0.1.0-rc.0 refs/notes/semantic-release-v0.1.0-rc.0
git push origin <commit-base>:refs/heads/release
```

Só os commits depois da tag semente contam para a próxima versão.

### 7.3 Fluxo do release.yml

`push na main` → CI → semantic-release (tag e GitHub Release; só se houver versão nova) → plano de tags → imagem no Docker Hub → tags git flutuantes. O semantic-release cria a tag com o `GITHUB_TOKEN`, e tags assim não disparam outros workflows: por isso tudo roda no mesmo workflow.

### 7.4 Tags flutuantes

Cada tag flutuante só avança se a versão publicada for a maior do seu escopo e da sua família (rc ou final).

| Versão publicada | Tags Docker | Tags git flutuantes |
|---|---|---|
| `0.1.0-rc.2` | `0.1.0-rc.2`, `0.1-rc`, `0-rc`, `rc` | `v0.1-rc`, `v0-rc` |
| `0.1.0` | `0.1.0`, `0.1`, `0`, `latest` | `v0.1`, `v0` |

Em `0.x` o semver permite quebra a cada minor: `0.1` e `v0.1` são os ponteiros seguros; `0` e `v0` podem pular para um 0.2 incompatível.

### 7.5 Pacotes (NuGet e crates.io)

Versões publicadas são imutáveis e não há tag flutuante: quem consome usa faixas de versão.

| | Última 0.1.x final | Candidata |
|---|---|---|
| Cargo | `taba-client = "0.1"` (ou `"0"`) | `"0.1.0-rc.2"`, explícito: o Cargo não escolhe pre-release sozinho |
| NuGet | `Version="0.1.*"` (ou `"0.*"`) | `Version="0.1.0-rc.*"` |

Versão flutuante no NuGet torna o build não reproduzível, a menos que se use *lock file*.

### 7.6 Versão nos arquivos

O `.github/scripts/set-version.sh` grava a versão calculada (inclusive `-rc.N`) no `Cargo.toml` e `Cargo.lock` do servidor, nos dois `Cargo.toml` e no `Cargo.lock` dos conectores Rust e nos `<Version>` dos `.csproj`, só dentro do build do release, sem commitar. O `taba --version` da imagem reporta a versão da tag. No repositório os arquivos ficam em `0.1.0`; para sincronizar à mão, rode o script.

### 7.7 Sair do ciclo rc (lançar o 0.1.0 final)

Fazer a `main` passar a ser a branch estável: em `.releaserc.json`, trocar `branches` por `["main", {"name": "rc", "prerelease": true}]` e, a partir daí, as candidatas saem da branch `rc` e a versão final da `main`. A tag final precisa estar acima das candidatas (`0.1.0 > 0.1.0-rc.N`). Detalhar e testar quando chegar a hora.

### 7.8 Estado e limitações

Feito: `ci.yml` (servidor, conector Rust, conector C# unitário e `docker build`), `release.yml` e `.releaserc.json`.

- **Imagem só amd64**: arm64 sob QEMU custa muitos minutos no plano privado.
- **Fora desta fase:** job `integration` (`integration.sh` e `validate.sh`), binários na Release, NuGet e crates.io.
