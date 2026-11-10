---
título: Problemas conhecidos
resumo: "Índice dos problemas conhecidos: os abertos numa tabela, onde estão os abertos (36a) e os corrigidos (36f), e o padrão que toda seção segue."
tipo: referência
status: parcial
verificado-em: 2026-11-10 @ d6e1d66
fontes: [AGENTS.md, docker/hang-hunt.sh, crates/taba-node/tests/common/cluster.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md). As regras para tratar testes intermitentes estão no [AGENTS.md](../../AGENTS.md).

Um problema conhecido é um teste intermitente ou um problema achado no sistema que não é resolvido na hora. Cada um tem um código `KI-NNN` (do inglês *known issue*), dado na ordem em que foram achados: o código nunca é reusado, um problema reaberto o mantém, e é o jeito de citar o problema em commits, pendências e decisões. O próximo código livre é o seguinte ao maior do [36a-problemas-conhecidos.md](36a-problemas-conhecidos.md) e do [36f-problemas-conhecidos.md](36f-problemas-conhecidos.md).

## Onde estão

| Arquivo | O que guarda |
|---|---|
| Este | O índice dos problemas **abertos** (a tabela abaixo), onde estão e como uma seção é escrita. |
| [36a-problemas-conhecidos.md](36a-problemas-conhecidos.md) | As seções dos problemas **abertos** (`Aberto` ou `Reaberto`), e as lições aprendidas com todos eles. |
| [36f-problemas-conhecidos.md](36f-problemas-conhecidos.md) | As seções dos problemas **corrigidos** (`Corrigido`), com a tabela própria: histórico, e o lugar onde procurar quando um voltar. |

Uma seção **nunca é apagada**; ela muda de arquivo, e a linha dela vai junto (a tabela abaixo para os abertos, a tabela do 36f-problemas-conhecidos.md para os corrigidos):

- Um problema novo vai para o 36a-problemas-conhecidos.md e ganha uma linha aqui. Um achado e corrigido na mesma mudança vai direto para o 36f-problemas-conhecidos.md.
- Ao corrigir: status `Corrigido`, a data da solução e uma linha do histórico (o que mudou e como foi verificado); a seção vai para o 36f-problemas-conhecidos.md e a linha sai desta tabela para a do 36f-problemas-conhecidos.md.
- Quando **volta**: nada de seção nova. Ela volta para o 36a-problemas-conhecidos.md com o status `Reaberto`, a data da solução limpa e uma linha do histórico para a nova ocorrência; corrigida de novo, outra linha, `Corrigido` com a nova data, e de volta ao 36f-problemas-conhecidos.md.
- Um link para uma seção aponta para o arquivo onde ela está naquele momento (`36a-problemas-conhecidos.md#ki-...` ou `36f-problemas-conhecidos.md#ki-...`): mover uma seção atualiza esses links.

## Problemas abertos

| Código | Problema | Status | Achado em |
|---|---|---|---|
| KI-034 | [Um nó reiniciado na tabela junto com o antigo numa caça sobrecarregada](36a-problemas-conhecidos.md#ki-034-um-nó-reiniciado-na-tabela-junto-com-o-antigo-numa-caça-sobrecarregada) | Aberto | 2026-11-10 |
| KI-005 | [Teste de reinício da FIFO que espera 30 s numa execução combinada](36a-problemas-conhecidos.md#ki-005-teste-de-reinício-da-fifo-que-espera-30-s-numa-execução-combinada) | Aberto | 2026-10-22 |

## Como uma seção é escrita

O título começa com o código; a linha logo abaixo tem o status e as datas; depois os campos, nesta ordem, e o histórico. Os campos de um problema corrigido se somam aos que ele tinha enquanto aberto (a causa, quando achada, substitui o "não identificada"):

```markdown
## KI-NNN: <o que se vê, em poucas palavras: o teste ou o comportamento>

**Status:** Aberto · **Achado em:** AAAA-MM-DD · **Corrigido em:** —

- **Teste:** `tests/<arquivo>.rs::<teste>` (UC-…), ou **Onde:** o job, o comando ou o módulo.
- **Sintoma:** a mensagem ou o comportamento errado, onde foi visto (job, commit, execução) e com que frequência.
- **Causa:** o que foi achado e como (log, pilha, código); `não identificada` enquanto não se sabe.
- **Próximo passo:** (enquanto aberto) o que rodar ou ler para achar a causa.
- **Não fazer:** (opcional) a correção tentadora que esconderia o problema, como aumentar os 30 s do `eventually`.
- **Correção (AAAA-MM-DD, no servidor | no teste):** o que mudou, o teste que falha sem ela.
- **Verificação:** o comando, quantas execuções sob carga, quantas falhas.

**Histórico**

| Data | Evento |
|---|---|
| AAAA-MM-DD | Achado: onde, quantas falhas em quantas execuções. |
| AAAA-MM-DD | Causa achada …; corrigido …; N execuções sem falha. |
```

- **Status:** `Aberto`, `Corrigido` ou `Reaberto`; as datas são `AAAA-MM-DD` e `—` enquanto não há.
- Os textos dizem o que foi visto e medido (execuções, falhas, commit, job), não o que se supõe; uma causa não achada é escrita como tal, com o próximo passo.
- Um campo a mais vale quando o caso pede (**Como foi achado**, **Risco**, **Teste da correção**, **Segunda correção**); os de cima vêm sempre nesta ordem.

## Fontes e provas

- [AGENTS.md](../../AGENTS.md), seção "Intermittent tests and known issues" (as regras e o método da caça).
- `docker/hang-hunt.sh` e `tests/common/cluster.rs` (`eventually`, `TABA_TEST_LOG`), as ferramentas que as seções citam.
