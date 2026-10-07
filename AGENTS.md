# AGENTS.md

Repositório (em português) do Sistema de Mensagens: a documentação em `doc/`, começando por [doc/README.md](doc/README.md), e a aplicação em Rust em `crates/`. A aplicação está no começo (esqueleto, configuração, listeners, saúde do nó, tópicos e publicação e consumo em memória); a estrutura e as regras de código estão em [doc/18-estrutura-codigo.md](doc/18-estrutura-codigo.md).

## Código

Um Cargo workspace com um único crate, `crates/taba-node` (biblioteca `taba_node` mais o binário `taba`). Os módulos seguem [doc/18-estrutura-codigo.md](doc/18-estrutura-codigo.md); o `main.rs` é só a linha de comando e os sinais, e o resto fica na biblioteca para os testes de integração subirem nós de verdade.

```
cargo build
cargo test                                   # unitários (nos módulos) e de integração (crates/taba-node/tests/)
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test --test uc_coverage -- --nocapture # placar dos casos de uso com teste
```

- **Configuração:** `crates/taba-node/src/config/params.rs` é a tabela de chaves e padrões de [doc/10-configuracao.md](doc/10-configuracao.md). Parâmetro novo entra na tabela **e** no `Config`; um teste confere que todo parâmetro da tabela é lido.
- **Testes:** os unitários ficam em arquivo irmão do módulo (`foo/mod.rs` e `foo/tests.rs` lado a lado, ligados por `#[cfg(test)] mod tests;`), não dentro do código. Cada teste cita os casos de uso que cobre numa linha `/// UC: UC-XXX-NNN` logo acima do `#[test]`. O `tests/uc_coverage.rs` falha se o código não existir (regras em [doc/20-casos-de-uso.md](doc/20-casos-de-uso.md)). Testes de integração usam `tests/common/mod.rs` (`TestNode`: nó em portas livres, disco numa pasta temporária).
- **Erros:** todo erro de API é um `ErrorCode` do catálogo (`core/error.rs`), com o status fixo de [doc/06d-api-convencoes.md](doc/06d-api-convencoes.md). Código novo no catálogo exige a linha no doc e o caso na tabela de teste.
- **SQLite:** o `rusqlite` compila a SQLite embutida (`bundled`), então o build precisa de um compilador C (MSVC, `gcc` ou `clang`). No alvo `-gnu` do Windows o `cc` não pode achar o `gcc` do Cygwin.
- **Windows com Git Bash:** o `link.exe` do coreutils esconde o do MSVC, e o MSVC desta máquina está incompleto. Se o `cargo build` falhar no link, use `cargo +stable-x86_64-pc-windows-gnu ...` (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`).

## Decisões e pendências

- [doc/08a-decisoes-tomadas.md](doc/08a-decisoes-tomadas.md) guarda **tudo o que já foi decidido**; [doc/08b-pendencias.md](doc/08b-pendencias.md) guarda **só o que continua em aberto**. Nunca deixe em `08b` uma anotação do que já está definido: ela vai para `08a`, em linha própria, com o código da dúvida e o link para onde a definição está.
- Dúvida parcialmente resolvida: a parte decidida entra em `08a` (com o mesmo código, ex.: D13.1), e `08b` mantém só as perguntas que faltam, cada uma com seu sub-código.
- Ao fechar uma pendência: registre a decisão no documento técnico, o valor padrão em [doc/10-configuracao.md](doc/10-configuracao.md), a linha em `08a`, e **apague** a entrada de `08b` e a referência em [doc/08c-proximos-passos.md](doc/08c-proximos-passos.md). Os códigos não são reaproveitados.

## Histórico da implementação

[doc/21-historico-implementacao.md](doc/21-historico-implementacao.md) registra, **na ordem em que aconteceu**, tudo o que já foi implementado, os desvios em relação à documentação e o que ainda falta. Serve para conferir o código contra os documentos. **Toda nova implementação o atualiza na mesma mudança**:

1. Acrescente uma entrada na seção 1 ("Linha do tempo"), ao final: o que foi feito, onde está no código, os desvios e o que ficou de fora.
2. Registre na seção 2 cada decisão que a implementação tomou e que o documento não dizia ou contraria, e corrija o `.md` técnico. Quando virar decisão firme, ela vai para [doc/08a-decisoes-tomadas.md](doc/08a-decisoes-tomadas.md).
3. Mova na seção 3 as áreas que mudaram de `Falta` para `Parcial` ou `Feito`, e ajuste a seção 4 ("Próximas etapas").
4. Atualize a linha "Último registro" e o placar de casos de uso com teste (`cargo test --test uc_coverage -- --nocapture`).

Nunca reescreva as entradas antigas para parecerem melhores: se algo mudou, acrescente uma entrada nova que diz o que mudou.

## Casos de uso

Os casos de uso que orientam os testes estão em [doc/20-casos-de-uso.md](doc/20-casos-de-uso.md) (e `20a`, `20b`, `20c`), cada um com um código `UC-<ÁREA>-<NNN>` que os testes citam numa linha `/// UC: UC-MSG-003`.

- Os códigos **não são reaproveitados**: um caso que deixa de existir fica na tabela como `(retirado)`; um caso novo ganha o próximo número livre da área.
- Ao mudar o comportamento descrito em um `.md` técnico, atualize os casos de uso correspondentes **no mesmo commit** (e o total do índice em `20-casos-de-uso.md`).
- Um caso só vale como coberto com ao menos um teste que o cite, no nível da coluna "Nível".

## Diagramas

Os diagramas são gerados com o [Archify](https://github.com/tt-a1i/archify) a partir de JSON. **A fonte da verdade é o JSON**; o SVG e o HTML são saídas geradas.

```
doc/diagramas/
  fonte/   *.json          fonte, versionada. É o que se edita.
  img/     *.svg           exportado, versionado. É o que os .md embutem.
  html/    *.html          interativo, NÃO versionado (só existe com --html).
  scripts/gerar.sh         valida, renderiza e exporta o SVG
  scripts/reespacar.mjs    recalcula o y das mensagens de diagramas de sequência
  scripts/exportar-svg.mjs usado pelo gerar.sh (Chrome headless)
```

### Regras

- Edite sempre `doc/diagramas/fonte/<nome>.json`. Nunca edite o SVG ou o HTML: são sobrescritos a cada geração.
- Depois de editar, rode `doc/diagramas/scripts/gerar.sh <nome>` e faça commit do JSON **e** do SVG juntos. Sem nome, gera todos.
- Rode os comandos a partir da raiz do repositório. O `meta.output` dos JSON é relativo a ela (`doc/diagramas/html/<nome>.html`) e não deve ser alterado.
- `--html` mantém o HTML interativo em `doc/diagramas/html/` para abrir no navegador. Ele é ignorado pelo git (`.gitignore`).
- A pasta `.archify/` é área de trabalho do Archify e também é ignorada. Diagramas novos nascem lá, mas só entram no repositório depois de copiados para `doc/diagramas/fonte/` com nome estável (sem data) e `meta.output` apontando para `doc/diagramas/html/`.
- O texto dos diagramas é em português; a legenda e a interface do viewer do Archify são fixas em inglês.
- Ao mudar o comportamento descrito em um `.md`, atualize o diagrama correspondente (tabela em [doc/README.md](doc/README.md#diagramas)) e regere.

### Diagramas desenhados por script (exceção)

`anel-hash.svg`, `topico-espalhado.svg` e `fifo-grupos.svg` não passam pelo Archify (ele não desenha anéis nem matrizes) e **não têm JSON**: a fonte é `doc/diagramas/scripts/gerar-hash.mjs`. Edite as constantes do script (nós, posições do anel, mensagens, matriz) e rode `node doc/diagramas/scripts/gerar-hash.mjs`; o `gerar.sh` não os gera. Faça commit do script **e** dos SVGs juntos. O script não valida o layout: abra o SVG (ou faça uma captura no Chrome) e confira textos sobrepostos.

O mesmo vale para o hero do README da raiz (`hero.svg` e `hero.gif`): a fonte é `doc/diagramas/scripts/gerar-hero.mjs`. Edite os textos, as cores e a linha do tempo (`PRODUTORES`, `CONSUMIDORES`, `MSGS`, `LEGENDA`) e rode `node doc/diagramas/scripts/gerar-hero.mjs`. O `hero.svg` é o quadro estático (instante `ESTATICO`) e o `hero.gif` é o que o README embute; o GIF usa o Chrome headless e o `ffmpeg` (variável `FFMPEG` se não estiver no PATH). `--so-svg` gera só o SVG. Faça commit do script **e** dos dois arquivos juntos.

### Pré-requisitos

- Node.js 22 ou superior.
- Archify instalado como skill (`~/.claude/skills/archify`). Outro local: defina `ARCHIFY_BIN` com o caminho do `archify.mjs`.
- Chrome ou Edge. Outro executável: defina `ARCHIFY_CHROME`.

### Editar um diagrama de sequência

O array `messages` define a ordem; o `y` é recalculado por `scripts/reespacar.mjs`.

1. Insira, remova ou reordene mensagens no array. Para uma mensagem nova, use qualquer `y` (se houver `segments`, use o `y` de uma mensagem vizinha da mesma fase).
2. `node doc/diagramas/scripts/reespacar.mjs doc/diagramas/fonte/<nome>.json` reatribui os `y` (passo de 28 px, folga entre fases), refaz as faixas de `segments` e ajusta `meta.viewBox`.
3. `doc/diagramas/scripts/gerar.sh <nome>`.

Limites do Archify (perfil `showcase`) que o `scripts/gerar.sh` faz valer:

- Mensagens que dividem espaço horizontal precisam de pelo menos 28 px entre si.
- A proporção do `viewBox` não pode ser mais alta que cerca de 1,55 (largura/altura) com largura de 1080. Se o `scripts/reespacar.mjs` avisar, divida o diagrama em dois. Foi assim que o consumo virou `consumo` (entrega e ack) mais três cenários.
- Com largura maior que cerca de 1085, o texto secundário fica pequeno demais na tela e a checagem falha.

### Editar um diagrama de arquitetura (incluindo C4)

Não há reespaçamento automático. Ajuste `pos` e `size` dos componentes no JSON, mantendo:

- vizinhos conectados adjacentes, com folga de `6,5 px × caracteres + 21 px` para cada rótulo de conexão na linha principal;
- `boundaries` só do tipo `region` ou `security-group`;
- `meta.viewBox` omitido em diagramas de arquitetura novos.

### Diagnóstico

Quando o Archify recusa um diagrama, o `scripts/gerar.sh` imprime os códigos e mensagens. As mais comuns:

| Mensagem | Correção |
|---|---|
| `less than 28px apart` | rodar `scripts/reespacar.mjs` |
| `leaving no room for the legend` | aumentar a altura do `viewBox` (o `scripts/reespacar.mjs` já calcula) |
| `composition/viewport-height` | diagrama alto demais: dividir |
| `composition/desktop-readability` | reduzir a largura do `viewBox` |

Detalhes do formato: `~/.claude/skills/archify/references/authoring-contract.md` e os schemas em `~/.claude/skills/archify/schemas/`.
