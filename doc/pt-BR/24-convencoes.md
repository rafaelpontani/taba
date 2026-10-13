---
título: Convenções
resumo: As regras de quem altera o código — idioma, estrutura de pastas, regras de código Rust, convenções da API e da configuração, testes, documentação, diagramas, commits — e o checklist ao terminar uma mudança.
tipo: guia
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [AGENTS.md, Cargo.toml, crates/taba-node/src, crates/taba-node/tests, .releaserc.json, doc/diagramas]
---

# Convenções

> Este é o guia humano das regras que o `AGENTS.md` da raiz dá a agentes de IA. Em caso de dúvida, o `AGENTS.md` e o código valem.

## Idioma

| O quê | Idioma |
|---|---|
| **Documentação** | Português do Brasil (será traduzida para outros idiomas, em pastas irmãs: `doc/pt-BR/` hoje) |
| **Nomes no código**: variáveis, funções, tipos, módulos, campos, **testes**, chaves de configuração, rotas, **códigos de erro**, eventos | **Inglês**. Nada de identificador em português, **nem em teste** |
| **Comentários** e **mensagens para o usuário** (erro da API, log, ajuda da linha de comando) | Português durante o desenvolvimento (serão traduzidos junto com a documentação) |
| **Nomes em disco**: tabelas e colunas do SQLite, `_topic.db`, `${topic}`, chaves do `state` | Inglês |
| **Raiz do repositório** (`README.md`, `AGENTS.md`) | Deve ser **en-US** (hoje ainda em português; ver [32-roadmap.md](32-roadmap.md)) |

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
  exemplos/csharp-console/      exemplo de uso do conector C#
  doc/                          registros 08a-c, catálogo de casos 20*, diagramas/
  doc/pt-BR/                    esta documentação
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

docker compose up --build -d                          # cluster de 3 nós
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
doc/diagramas/
  fonte/   *.json           fonte, versionada: é o que se edita
  img/     *.svg            exportado, versionado: é o que os .md embutem
  html/    *.html           interativo, NÃO versionado (só com --html)
  scripts/gerar.sh          valida, renderiza e exporta o SVG
  scripts/reespacar.mjs     recalcula o y das mensagens de diagramas de sequência
```

- **Nunca edite SVG ou HTML.** Edite `fonte/<nome>.json` e rode `doc/diagramas/scripts/gerar.sh <nome>` (sem nome, gera todos), a partir da **raiz**; faça commit do JSON **e** do SVG juntos.
- **Diagramas de sequência:** insira/remova/reordene no array `messages` (qualquer `y` na nova), rode `node doc/diagramas/scripts/reespacar.mjs doc/diagramas/fonte/<nome>.json`, depois `gerar.sh`. Limites do perfil `showcase`: ≥ 28 px entre mensagens que dividem espaço; proporção do `viewBox` ≤ ~1,55 (se estourar, **divida o diagrama**); largura ≤ ~1085 px.
- **Diagramas de arquitetura (incl. C4):** sem reespaçamento automático; ajuste `pos` e `size` mantendo vizinhos conectados adjacentes (folga de `6,5 px × caracteres + 21 px` por rótulo de conexão), `boundaries` só `region`/`security-group` e `meta.viewBox` omitido nos novos.
- **Desenhados por script** (sem JSON): `anel-hash.svg`, `topico-espalhado.svg`, `fifo-grupos.svg` (`gerar-hash.mjs`) e o hero do README (`gerar-hero.mjs`, com Chrome headless e `ffmpeg`).
- **Pré-requisitos:** Node ≥ 22, Archify (`~/.claude/skills/archify` ou `ARCHIFY_BIN`), Chrome/Edge (`ARCHIFY_CHROME`).
- Ao mudar o comportamento descrito num `.md`, **atualize o diagrama** correspondente e regere. Um ponto novo de diagrama, ainda por desenhar, é marcado com um bloco `🖼️ DIAGRAMA PLANEJADO` com `tipo:` e `objetivo:` (o `verificar.mjs` confere).

### Índice dos diagramas

Gerado pelo levantamento dos `![…](../diagramas/img/…)` desta pasta. Um diagrama com "—" na última coluna não é usado por esta documentação (ver a nota abaixo da tabela).

| Diagrama | Tipo | Fonte | Usado em |
|---|---|---|---|
| `anel-antes-depois` | architecture | `fonte/anel-antes-depois.json` | [06](06-anel-e-replicacao.md) |
| `anel-hash` | script | `doc/diagramas/scripts/gerar-hash.mjs` | [06](06-anel-e-replicacao.md) |
| `armazenamento-disco` | architecture | `fonte/armazenamento-disco.json` | [08](08-armazenamento.md) |
| `arquitetura-escrita` | architecture | `fonte/arquitetura-escrita.json` | [10](10-publicacao.md) |
| `c4-nivel0-contexto` | architecture | `fonte/c4-nivel0-contexto.json` | [01](01-visao-geral.md) |
| `c4-nivel1-conteineres` | architecture | `fonte/c4-nivel1-conteineres.json` | — |
| `camadas` | architecture | `fonte/camadas.json` | [04](04-arquitetura.md) |
| `ci-cd` | workflow | `fonte/ci-cd.json` | [27](27-ci-cd.md) |
| `consumo` | sequence | `fonte/consumo.json` | [12](12-entrega.md) |
| `consumo-cenario1-lista-local` | sequence | `fonte/consumo-cenario1-lista-local.json` | [12](12-entrega.md) |
| `consumo-cenario2-outro-no` | sequence | `fonte/consumo-cenario2-outro-no.json` | [12](12-entrega.md) |
| `consumo-cenario3-espera` | sequence | `fonte/consumo-cenario3-espera.json` | [12](12-entrega.md) |
| `decisao-topico` | workflow | `fonte/decisao-topico.json` | [09](09-topicos.md) |
| `distribuidor-publicacao` | sequence | `fonte/distribuidor-publicacao.json` | [09](09-topicos.md) |
| `entidades` | architecture | `fonte/entidades.json` | [03](03-conceitos.md) |
| `estados-mensagem` | lifecycle | `fonte/estados-mensagem.json` | [12](12-entrega.md) |
| `estados-no` | lifecycle | `fonte/estados-no.json` | [07](07-cluster.md) |
| `falha-registro-no` | sequence | `fonte/falha-registro-no.json` | [07](07-cluster.md) |
| `fifo-grupos` | script | `doc/diagramas/scripts/gerar-hash.mjs` | [09](09-topicos.md) |
| `fifo-publicacao` | sequence | `fonte/fifo-publicacao.json` | [09](09-topicos.md) |
| `fila-volatil-criacao` | sequence | `fonte/fila-volatil-criacao.json` | [09](09-topicos.md) |
| `fila-volatil-exclusao` | sequence | `fonte/fila-volatil-exclusao.json` | [09](09-topicos.md) |
| `fila-volatil-troca-gestor` | sequence | `fonte/fila-volatil-troca-gestor.json` | [09](09-topicos.md) |
| `hero` | script | `doc/diagramas/scripts/gerar-hero.mjs` | — |
| `implantacao-compose` | architecture | `fonte/implantacao-compose.json` | [02](02-inicio-rapido.md) |
| `ingresso-no` | sequence | `fonte/ingresso-no.json` | [07](07-cluster.md) |
| `invisibilidade` | sequence | `fonte/invisibilidade.json` | — |
| `mapa-mental` | architecture | `fonte/mapa-mental.json` | [29](29-mapa-mental.md) |
| `mapa-mental-cluster` | architecture | `fonte/mapa-mental-cluster.json` | [29](29-mapa-mental.md) |
| `mapa-mental-disco` | architecture | `fonte/mapa-mental-disco.json` | [29](29-mapa-mental.md) |
| `mapa-mental-entrega` | architecture | `fonte/mapa-mental-entrega.json` | [29](29-mapa-mental.md) |
| `mapa-mental-escrita` | architecture | `fonte/mapa-mental-escrita.json` | [29](29-mapa-mental.md) |
| `mapa-mental-invisibilidade` | architecture | `fonte/mapa-mental-invisibilidade.json` | [29](29-mapa-mental.md) |
| `mapa-mental-obs` | architecture | `fonte/mapa-mental-obs.json` | [29](29-mapa-mental.md) |
| `mapa-mental-sync` | architecture | `fonte/mapa-mental-sync.json` | [29](29-mapa-mental.md) |
| `mapa-mental-topicos` | architecture | `fonte/mapa-mental-topicos.json` | [29](29-mapa-mental.md) |
| `no-visao-geral` | architecture | `fonte/no-visao-geral.json` | [04](04-arquitetura.md) |
| `observabilidade-fluxo` | dataflow | `fonte/observabilidade-fluxo.json` | [13](13-observabilidade.md) |
| `piramide-testes` | architecture | `fonte/piramide-testes.json` | [25](25-testes.md) |
| `portas-do-no` | architecture | `fonte/portas-do-no.json` | [15](15-api-visao-geral.md) |
| `recepcao` | sequence | `fonte/recepcao.json` | [10](10-publicacao.md) |
| `reinicio-processo` | sequence | `fonte/reinicio-processo.json` | [07](07-cluster.md) |
| `remocao-no` | sequence | `fonte/remocao-no.json` | [07](07-cluster.md) |
| `retorno-no-excluido` | sequence | `fonte/retorno-no-excluido.json` | [07](07-cluster.md) |
| `sincronizacao-fases` | workflow | `fonte/sincronizacao-fases.json` | [07](07-cluster.md) |
| `topico-espalhado` | script | `doc/diagramas/scripts/gerar-hash.mjs` | [06](06-anel-e-replicacao.md) |
| `zonas-seguranca` | architecture | `fonte/zonas-seguranca.json` | [14](14-seguranca.md) |

> `hero` é usado pelo `README.md` da raiz. `c4-nivel1-conteineres` e `invisibilidade` ficaram da documentação histórica e **estão desatualizados** (descrevem a API "interna" e a lista de invisíveis em disco, que o código não tem): foram substituídos por `no-visao-geral`, `estados-mensagem` e `consumo`.

## Commits e versões

- **Conventional Commits** em inglês curto ou português, com escopo: `feat(connectors/csharp): …`, `fix(cluster): …`. O tipo decide a versão ([27-ci-cd.md](27-ci-cd.md)): `feat`, `fix` e `perf` geram versão; `docs`, `chore`, `ci`, `refactor` e `test` **não**. `feat!:` ou `BREAKING CHANGE:` conta como `minor` enquanto o projeto está em `0.x`.
- A mensagem diz **o que mudou e por quê** (o histórico é o git; não há registro cronológico da implementação).
- Uma tag única `vX.Y.Z` para o repositório todo (servidor e conectores andam juntos). Ninguém cria tag à mão.

## Fontes e provas

- `AGENTS.md` (as regras de código, idioma, fim de implementação, pendências, casos de uso e diagramas), `Cargo.toml` (perfil `dev`), `.releaserc.json`, `18-estrutura-codigo.md` (17.3 a 17.7), `doc/diagramas/scripts/`.
- Casos de uso: UC-CFG (tabela de parâmetros lida), UC-CNV (erros e status); veja [26a](26a-casos-de-uso-implementados.md).
