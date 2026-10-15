---
título: Convenções
resumo: As regras de quem altera o código — idioma, estrutura de pastas, regras de código Rust, convenções da API e da configuração, testes, documentação, diagramas, commits — e o checklist ao terminar uma mudança.
tipo: guia
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [AGENTS.md, Cargo.toml, crates/taba-node/src, crates/taba-node/tests, .releaserc.json, doc/diagrams]
---

# Convenções

> Este é o guia humano das regras que o `AGENTS.md` da raiz dá a agentes de IA. Em caso de dúvida, o `AGENTS.md` e o código valem.

## Idioma

| O quê | Idioma |
|---|---|
| **Documentação** | Uma pasta por idioma em `doc/`, com o código do idioma: `doc/pt-BR/` (português do Brasil) e `doc/en-US/` (inglês dos EUA). **Toda mudança vale para todas as pastas** (ver [Documentação em vários idiomas](#documentação-em-vários-idiomas)) |
| **Nomes no código**: variáveis, funções, tipos, módulos, campos, **testes**, chaves de configuração, rotas, **códigos de erro**, eventos | **Inglês**. Nada de identificador em português, **nem em teste** |
| **Comentários** e **mensagens para o usuário** (erro da API, log, ajuda da linha de comando) | Português durante o desenvolvimento (serão traduzidos junto com a documentação) |
| **Nomes em disco**: tabelas e colunas do SQLite, `_topic.db`, `${topic}`, chaves do `state` | Inglês |
| **Raiz do repositório** (`README.md`, `AGENTS.md`) | **en-US** (`README-pt-BR.md` é a versão em português do README) |

## Documentação em vários idiomas

- Cada idioma fica numa pasta própria, `doc/<idioma>/` (hoje `pt-BR` e `en-US`), com os **mesmos arquivos**: o prefixo numérico (`NN-`) é a identidade do documento, e o resto do nome está no idioma da pasta (`22-configuracao.md` em `pt-BR`, `22-configuration.md` em `en-US`). `README.md`, `llms.txt` e os `scripts/` seguem a mesma regra.
- **Toda mudança na documentação deve ser aplicada a todas as pastas de idioma** na mesma alteração: o conteúdo, os metadados (`verificado-em` / `verified-on`), a seção "Fontes e provas" e os links. Uma pasta que fica para trás faz a documentação se contradizer. Quem não domina um idioma altera as outras pastas mesmo assim e registra a lacuna em [31-pendencias.md](31-pendencias.md) (um `PRB-NNN` com o nome do arquivo), em vez de deixá-la em silêncio.
- O que é **identificador ou saída do programa** não se traduz em idioma nenhum: rotas, chaves, códigos de erro, campos JSON, códigos `UC-…` e o que o servidor devolve hoje (o `message` de um erro e o texto dos logs seguem em português até o código ser traduzido).
- Os arquivos gerados (`26a`, `26b`, `30`, `31`) saem por idioma dos scripts de cada pasta; edite a fonte, não a saída.
- Para acrescentar um idioma: copie uma pasta completa para `doc/<idioma>/`, traduza, ponha o idioma na tabela de [`doc/README.md`](../README.md) e nas tabelas de tradução dos diagramas ([Índice dos diagramas](#índice-dos-diagramas)).

## Estrutura de pastas

```
taba/
  Cargo.toml                    workspace do servidor (um crate)
  crates/taba-node/             o servidor: biblioteca `taba_node` + binário `taba`
    src/                        módulos (mapa em 04-arquitetura.md)
    tests/                      testes de integração e de cluster
      common/                   TestNode, TestCluster, real.rs, otlp.rs
      uc_coverage.rs            confere os códigos de caso de uso
  connectors/                   conectores (uma pasta por linguagem; workspace próprio para o Rust)
    rust/{client,cli}/  csharp/  README.md
  docker/                       cluster.sh, validate.sh, haproxy.cfg, test-cpu.sh
  Dockerfile  Dockerfile.ci  Dockerfile.cli.ci  docker-compose.yml
  examples/csharp-console/      exemplo de uso do conector C#
  doc/                          README.md, diagrams/, uma pasta por idioma
  doc/pt-BR/                    esta documentação
  doc/en-US/                    a mesma documentação em inglês
  .github/                      workflows, actions e scripts
```

**Módulo** = pasta com `mod.rs` + `tests.rs` lado a lado; o `mod.rs` **começa com um comentário `//!`** que diz o que o módulo faz e em que documento está o desenho, e **termina** com `#[cfg(test)] mod tests;`. Os testes unitários **ficam no arquivo irmão**, não dentro do código.

## Regras de código (Rust)

- **Direção das dependências** `core ← storage ← engine ← cluster ← http ← node`: cada módulo conhece só os da esquerda. `core` não importa ninguém ([04-arquitetura.md](04-arquitetura.md)). Um **trait** fica na camada **mais baixa** de que o chamador precisa, junto das implementações real e de teste.
- **Passar mensagens, não compartilhar estado.** O escritor único por banco é uma fila de comandos executada uma de cada vez; prefira isso a `Arc<Mutex<…>>` espalhado.
- **Nunca segurar um lock durante um `.await`.**
- **SQLite fora do executor assíncrono** (`rusqlite` é síncrono): thread dedicada ou `spawn_blocking`; leituras longas usam conexões próprias em WAL.
- **Sem `unwrap()` fora de testes.** Erros usam um tipo próprio (`TabaError`) que mapeia direto para o catálogo de códigos.
- **Cancelamento limpo:** toda tarefa de fundo recebe um `CancellationToken` (é o que o drain usa).
- **Tudo que toca o mundo fica atrás de um trait** (`Transport`, `Clock`), para os testes injetarem rede e tempo.
- **Comente como o código vizinho** (densidade e estilo); não introduza padrões novos sem necessidade.

## Convenções da API

Detalhadas em [15-api-visao-geral.md](15-api-visao-geral.md). Em resumo:

- JSON `camelCase`, datas RFC 3339 UTC com milissegundos, durações em **segundos inteiros**, IDs **GUID v7** em minúsculas.
- **Todo erro é um `ErrorCode` do catálogo** (`core/error`) com **status fixo**. **Código novo no catálogo exige a linha em [19-erros-e-limites.md](19-erros-e-limites.md) e o caso na tabela de teste.**
- Campo desconhecido na requisição → `400 validation.unknown-field`; **na resposta, ignore o que não conhece**.
- O status vai só na linha HTTP; o `Retry-After` só no cabeçalho.
- Rota nova: define permissão, categoria de rate limit e status; atualize [16](16-api-cliente.md)/[17](17-api-operacao.md)/[18](18-api-cluster.md).

## Convenções de configuração

`config/params/mod.rs` é a **tabela de chaves e padrões** (fonte única). **Parâmetro novo entra na tabela _e_ no `Config`**; um teste confere que todo parâmetro da tabela é lido. Chave desconhecida no `.yml` é recusada. Variável: `TABA_` + chave em maiúsculas, `.` e `-` → `_`. Duração sempre com unidade. Documente em [22-configuracao.md](22-configuracao.md) (e o padrão efetivo, se o código aplica um que a tabela não tem).

## Testes

Resumo (detalhes em [25-testes.md](25-testes.md)):

- Cada teste **cita os casos de uso** que cobre numa linha `/// UC: UC-XXX-NNN` logo acima do `#[test]`. O `uc_coverage` falha se o código não existir.
- Teste unitário no `tests.rs` irmão; integração em `tests/`; cluster em `tests/` com `TestCluster`; conectores têm testes próprios que também citam `UC`.
- **Nomes de teste em inglês** e descritivos (`creating_a_queue_returns_201_with_location_and_the_defaults`).

## Comandos do dia a dia

```bash
cargo build
cargo test                                            # unitários e de integração
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --test uc_coverage -- --nocapture          # placar dos casos de uso
TABA_UC_WRITE=1 cargo test --test uc_coverage         # regera a coluna "Testes" de doc/20*.md
cargo run -- init --out ./secrets                     # certificado e token raiz

cd connectors/rust && cargo test --workspace --all-features && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
cd connectors/csharp && dotnet test Taba.Client.Tests

docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d   # cluster de 3 nós, imagem local
docker/validate.sh                                    # validação de ponta a ponta (≈ 5 min)
```

**Antes de fechar** mudança que afete **cluster, canal, Compose ou conectores**, rode `docker/validate.sh`.

### Ambiente (armadilhas conhecidas)

- **Disco:** o `target/` cresce rápido. O perfil `dev` guarda só as tabelas de linha (`debug = "line-tables-only"`), mas `cargo clean -p taba-node` apaga os binários de teste velhos (cada mudança deixa um novo). Com pouco espaço, use `CARGO_INCREMENTAL=0` ou apague `target/debug/incremental`. "no space on device" do `rustc` e `LNK1180` do `link.exe` são isso.
- **SQLite:** o `rusqlite` compila a SQLite embutida (`bundled`): o build precisa de um **compilador C** (MSVC, `gcc` ou `clang`). No alvo `-gnu` do Windows, o `cc` não pode achar o `gcc` do Cygwin.
- **Windows com Git Bash:** o `link.exe` do coreutils esconde o do MSVC. Se o link falhar, use `cargo +stable-x86_64-pc-windows-gnu …` (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`). Comandos Docker com caminhos precisam de `MSYS_NO_PATHCONV=1`.
- **Executável em uso:** no Windows, `cargo test` falha com "Acesso negado" ao remover `target/debug/taba.exe` se um nó compilado ainda estiver rodando.

## Ao terminar uma implementação

Na **mesma mudança** do código:

1. **Corrija o `.md` técnico** quando o código decidiu algo que o documento não dizia ou contraria. O desenho vive nos documentos, e o código é conferido contra eles.
2. **Decisão firme** → [30-decisoes.md](30-decisoes.md) (na doc histórica: `08a`); **dúvida** ou **problema** → [31-pendencias.md](31-pendencias.md) (`08b`).
3. **Casos de uso** novos ou mudados entram no catálogo (`doc/20a`, `20b` ou `20c`), cada teste os cita, e rode `TABA_UC_WRITE=1 cargo test --test uc_coverage` para regerar a coluna "Testes". Um caso sem teste fica com `—`, e a razão vai no próprio caso. Depois regenere os catálogos pt-BR (`node doc/pt-BR/scripts/gerar-casos-de-uso.mjs`).
4. **Módulo novo** entra no mapa de [04-arquitetura.md](04-arquitetura.md), e o `mod.rs` dele começa com `//!`.
5. **Parâmetro novo**: tabela, `Config`, [22-configuracao.md](22-configuracao.md). **Erro novo**: catálogo, [19-erros-e-limites.md](19-erros-e-limites.md), teste.

### Pendências e problemas descobertos

Toda implementação que descobre algo em aberto o registra **na mesma mudança**:

- **Pergunta de design** (o documento não define, ou há duas saídas): próxima dúvida `D` livre, com uma proposta. Decidida, vai para as decisões.
- **Problema de lógica, risco ou limite** (o código se comporta de um jeito furado, injusto ou incompleto): próximo `PRB-NNN`, dizendo o **efeito** e a **proposta**; se uma dúvida `D` o resolve, vai dentro dela ("Problema ligado"). Resolvido, vai para os "problemas resolvidos" com a data.
- Em [31-pendencias.md](31-pendencias.md): o que **não depende de nada** na seção 1; o que depende de outra dúvida, problema ou etapa, na seção 2 com "Depende de".
- **Os códigos (`D…`, `PRB-…`, `UC-…`) nunca são reaproveitados.**

## Casos de uso

Formato `UC-<ÁREA>-<NNN>` (três letras e três dígitos). Um caso que deixa de existir fica `(retirado)`; um novo ganha o próximo número livre da área. Ao mudar o comportamento descrito num `.md`, atualize os casos **no mesmo commit** (e o total do índice). Um caso só vale como coberto com ao menos um teste que o cite, **no nível da coluna "Nível"** ([26-casos-de-uso.md](26-casos-de-uso.md)).

## Diagramas

Gerados com o **Archify** a partir de JSON. **A fonte da verdade é o JSON**; SVG e HTML são saídas.

```
doc/diagrams/
  source/  *.json           fonte (em inglês, en-US), versionada: é o que se edita
  i18n/    <idioma>.json    uma tabela de tradução por idioma extra (pt-BR.json)
  img/<idioma>/ *.svg       exportado por idioma, versionado: é o que os .md embutem
  html/<idioma>/ *.html     interativo, NÃO versionado (só com --html)
  scripts/generate.sh       valida, renderiza e exporta o SVG de todos os idiomas
  scripts/respace.mjs       recalcula o y das mensagens de diagramas de sequência
```

- **Nunca edite SVG ou HTML.** Edite `source/<nome>.json` e rode `doc/diagrams/scripts/generate.sh <nome>` (sem nome, gera todos), a partir da **raiz**; faça commit do JSON **e** do SVG juntos.
- **Diagramas de sequência:** insira/remova/reordene no array `messages` (qualquer `y` na nova), rode `node doc/diagrams/scripts/respace.mjs doc/diagrams/source/<nome>.json`, depois `generate.sh`. Limites do perfil `showcase`: ≥ 28 px entre mensagens que dividem espaço; proporção do `viewBox` ≤ ~1,55 (se estourar, **divida o diagrama**); largura ≤ ~1085 px.
- **Diagramas de arquitetura (incl. C4):** sem reespaçamento automático; ajuste `pos` e `size` mantendo vizinhos conectados adjacentes (folga de `6,5 px × caracteres + 21 px` por rótulo de conexão), `boundaries` só `region`/`security-group` e `meta.viewBox` omitido nos novos.
- **Desenhados por script** (sem JSON): `ring-hash.svg`, `spread-topic.svg`, `fifo-groups.svg` (`generate-hash.mjs`) e o hero do README (`generate-hero.mjs`, com Chrome headless e `ffmpeg`).
- **Pré-requisitos:** Node ≥ 22, Archify (`~/.claude/skills/archify` ou `ARCHIFY_BIN`), Chrome/Edge (`ARCHIFY_CHROME`).
- Ao mudar o comportamento descrito num `.md`, **atualize o diagrama** correspondente e regere. Um ponto novo de diagrama, ainda por desenhar, é marcado com um bloco `🖼️ DIAGRAMA PLANEJADO` com `tipo:` e `objetivo:` (o `verificar.mjs` confere).

### Índice dos diagramas

Gerado pelo levantamento dos `![…](../diagrams/img/…)` desta pasta. Um diagrama com "—" na última coluna não é usado por esta documentação (ver a nota abaixo da tabela).

| Diagrama | Tipo | Fonte | Usado em |
|---|---|---|---|
| `ring-before-after` | architecture | `source/ring-before-after.json` | [06](06-anel-e-replicacao.md) |
| `ring-hash` | script | `doc/diagrams/scripts/generate-hash.mjs` | [06](06-anel-e-replicacao.md) |
| `disk-storage` | architecture | `source/disk-storage.json` | [08](08-armazenamento.md) |
| `write-architecture` | architecture | `source/write-architecture.json` | [10](10-publicacao.md) |
| `c4-level0-context` | architecture | `source/c4-level0-context.json` | [01](01-visao-geral.md) |
| `c4-level1-containers` | architecture | `source/c4-level1-containers.json` | — |
| `layers` | architecture | `source/layers.json` | [04](04-arquitetura.md) |
| `ci-cd` | workflow | `source/ci-cd.json` | [27](27-ci-cd.md) |
| `consumption` | sequence | `source/consumption.json` | [12](12-entrega.md) |
| `consumption-scenario1-local-list` | sequence | `source/consumption-scenario1-local-list.json` | [12](12-entrega.md) |
| `consumption-scenario2-other-node` | sequence | `source/consumption-scenario2-other-node.json` | [12](12-entrega.md) |
| `consumption-scenario3-waiting` | sequence | `source/consumption-scenario3-waiting.json` | [12](12-entrega.md) |
| `topic-decision` | workflow | `source/topic-decision.json` | [09](09-topicos.md) |
| `distributor-publishing` | sequence | `source/distributor-publishing.json` | [09](09-topicos.md) |
| `entities` | architecture | `source/entities.json` | [03](03-conceitos.md) |
| `message-states` | lifecycle | `source/message-states.json` | [12](12-entrega.md) |
| `node-states` | lifecycle | `source/node-states.json` | [07](07-cluster.md) |
| `node-register-failure` | sequence | `source/node-register-failure.json` | [07](07-cluster.md) |
| `fifo-groups` | script | `doc/diagrams/scripts/generate-hash.mjs` | [09](09-topicos.md) |
| `fifo-publishing` | sequence | `source/fifo-publishing.json` | [09](09-topicos.md) |
| `volatile-queue-creation` | sequence | `source/volatile-queue-creation.json` | [09](09-topicos.md) |
| `volatile-queue-deletion` | sequence | `source/volatile-queue-deletion.json` | [09](09-topicos.md) |
| `volatile-queue-manager-change` | sequence | `source/volatile-queue-manager-change.json` | [09](09-topicos.md) |
| `hero` | script | `doc/diagrams/scripts/generate-hero.mjs` | — |
| `compose-deployment` | architecture | `source/compose-deployment.json` | [02](02-inicio-rapido.md) |
| `node-join` | sequence | `source/node-join.json` | [07](07-cluster.md) |
| `invisibility` | sequence | `source/invisibility.json` | — |
| `mind-map` | architecture | `source/mind-map.json` | [29](29-mapa-mental.md) |
| `mind-map-cluster` | architecture | `source/mind-map-cluster.json` | [29](29-mapa-mental.md) |
| `mind-map-disk` | architecture | `source/mind-map-disk.json` | [29](29-mapa-mental.md) |
| `mind-map-delivery` | architecture | `source/mind-map-delivery.json` | [29](29-mapa-mental.md) |
| `mind-map-write` | architecture | `source/mind-map-write.json` | [29](29-mapa-mental.md) |
| `mind-map-invisibility` | architecture | `source/mind-map-invisibility.json` | [29](29-mapa-mental.md) |
| `mind-map-observability` | architecture | `source/mind-map-observability.json` | [29](29-mapa-mental.md) |
| `mind-map-sync` | architecture | `source/mind-map-sync.json` | [29](29-mapa-mental.md) |
| `mind-map-topics` | architecture | `source/mind-map-topics.json` | [29](29-mapa-mental.md) |
| `node-overview` | architecture | `source/node-overview.json` | [04](04-arquitetura.md) |
| `observability-flow` | dataflow | `source/observability-flow.json` | [13](13-observabilidade.md) |
| `test-pyramid` | architecture | `source/test-pyramid.json` | [25](25-testes.md) |
| `node-ports` | architecture | `source/node-ports.json` | [15](15-api-visao-geral.md) |
| `write-reception` | sequence | `source/write-reception.json` | [10](10-publicacao.md) |
| `process-restart` | sequence | `source/process-restart.json` | [07](07-cluster.md) |
| `node-removal` | sequence | `source/node-removal.json` | [07](07-cluster.md) |
| `excluded-node-return` | sequence | `source/excluded-node-return.json` | [07](07-cluster.md) |
| `sync-phases` | workflow | `source/sync-phases.json` | [07](07-cluster.md) |
| `spread-topic` | script | `doc/diagrams/scripts/generate-hash.mjs` | [06](06-anel-e-replicacao.md) |
| `security-zones` | architecture | `source/security-zones.json` | [14](14-seguranca.md) |

> `hero` é usado pelo `README.md` da raiz. `c4-nivel1-conteineres` e `invisibilidade` ficaram da documentação histórica e **estão desatualizados** (descrevem a API "interna" e a lista de invisíveis em disco, que o código não tem): foram substituídos por `no-visao-geral`, `estados-mensagem` e `consumo`.

## Commits e versões

- **Conventional Commits** em inglês curto ou português, com escopo: `feat(connectors/csharp): …`, `fix(cluster): …`. O tipo decide a versão ([27-ci-cd.md](27-ci-cd.md)): `feat`, `fix` e `perf` geram versão; `docs`, `chore`, `ci`, `refactor` e `test` **não**. `feat!:` ou `BREAKING CHANGE:` conta como `minor` enquanto o projeto está em `0.x`.
- A mensagem diz **o que mudou e por quê** (o histórico é o git; não há registro cronológico da implementação).
- Uma tag única `vX.Y.Z` para o repositório todo (servidor e conectores andam juntos). Ninguém cria tag à mão.

## Fontes e provas

- `AGENTS.md` (as regras de código, idioma, fim de implementação, pendências, casos de uso e diagramas), `Cargo.toml` (perfil `dev`), `.releaserc.json`, `18-estrutura-codigo.md` (17.3 a 17.7), `doc/diagrams/scripts/`.
- Casos de uso: UC-CFG (tabela de parâmetros lida), UC-CNV (erros e status); veja [26a](26a-casos-de-uso-implementados.md).
