# AGENTS.md

Repositório de documentação (em português) do Sistema de Mensagens. Não há código de aplicação ainda: o conteúdo está em `doc/`, começando por [doc/README.md](doc/README.md).

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

`anel-hash.svg` e `topico-espalhado.svg` não passam pelo Archify (ele não desenha anéis nem matrizes) e **não têm JSON**: a fonte é `doc/diagramas/scripts/gerar-hash.mjs`. Edite as constantes do script (nós, posições do anel, mensagens, matriz) e rode `node doc/diagramas/scripts/gerar-hash.mjs`; o `gerar.sh` não os gera. Faça commit do script **e** dos SVGs juntos. O script não valida o layout: abra o SVG (ou faça uma captura no Chrome) e confira textos sobrepostos.

O mesmo vale para o hero do README da raiz (`hero.svg` e `hero.gif`): a fonte é `doc/diagramas/scripts/gerar-hero.mjs`. Edite os textos, as cores e a linha do tempo (`T`, `LEGENDA`) e rode `node doc/diagramas/scripts/gerar-hero.mjs`. O `hero.svg` é o quadro estático (instante `ESTATICO`) e o `hero.gif` é o que o README embute; o GIF usa o Chrome headless e o `ffmpeg` (variável `FFMPEG` se não estiver no PATH). `--so-svg` gera só o SVG. Faça commit do script **e** dos dois arquivos juntos.

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
