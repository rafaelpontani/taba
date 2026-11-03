---
título: Casos de uso e rastreabilidade
resumo: O formato do código `UC-ÁREA-NNN`, os níveis de teste, como um teste cita um caso, a regra de cobertura que o `uc_coverage` confere, o índice das 38 áreas e como ler os dois catálogos (implementados e planejados).
tipo: guia
status: estável
verificado-em: 2026-10-27 @ ada9ff1
fontes: [crates/taba-node/tests/uc_coverage.rs, doc/pt-BR/26a-casos-de-uso-implementados.md, doc/pt-BR/26b-casos-de-uso-planejados.md]
---

# Casos de uso e rastreabilidade

> Este documento explica **o método**. Os casos em si estão em dois catálogos gerados: [26a — implementados](26a-casos-de-uso-implementados.md) (com teste) e [26b — planejados](26b-casos-de-uso-planejados.md) (sem teste ainda).

## Para que serve

Cada **comportamento observável** do Taba é um **caso de uso** com **código único**. Os testes **citam** o código, e assim dá para provar duas coisas: **todo caso está coberto por ao menos um teste** e **nenhum teste cobre algo que não está documentado**. Quando o texto técnico muda, o caso muda junto, no mesmo commit.

Os casos descrevem **comportamento**, não implementação. O "resultado" é o que um teste confere: um status, um corpo, um estado do disco, uma métrica, uma linha de log, ou o que o outro nó enxerga.

## O código

```
UC-<ÁREA>-<NNN>        exemplo: UC-MSG-003
```

- `<ÁREA>`: **três letras maiúsculas** (tabela abaixo). `<NNN>`: **três dígitos**, sequenciais **dentro da área**.
- **Os códigos nunca são reaproveitados.** Um caso que deixa de existir fica `(retirado…)` na tabela, **sem apagar a linha**. Um caso novo ganha o **próximo número livre da área**, mesmo fora da ordem do assunto. Um caso dividido em dois vira **dois códigos novos** e o original é retirado.

## Os níveis

| Sigla | Nível | Onde |
|---|---|---|
| **U** | Unitário | `src/**/tests.rs` |
| **I** | Um nó (perfil de um nó, HTTP e disco reais) | `tests/*.rs` com `TestNode` |
| **C** | Cluster simulado (rede em memória e relógio manual) | `tests/*.rs` com `TestCluster` |
| **K** | Conformidade de conectores | `connectors/<linguagem>/…` |

Regra prática: o que depende **só do cliente e de um nó** é `I`; o que depende de **mais de um nó, de falha ou de tempo** é `C`; uma regra pura é `U`. Um caso pode pedir dois níveis (`U, I`) e então precisa de **um teste em cada**. Detalhe em [25-testes.md](25-testes.md).

## Como um teste cita um caso

Uma linha **imediatamente acima** da função de teste, com um ou mais códigos separados por vírgula:

```rust
/// UC: UC-MSG-003, UC-ESC-004
#[test]
fn publishing_binary_stores_the_raw_bytes() { /* … */ }
```

- O prefixo é `/// UC:` (barras triplas, inclusive em C#) e os códigos são **completos** (`UC-MSG-003`, nunca `MSG-003`). Atributos (`#[…]`) e outras linhas de comentário entre a citação e a função são ignorados.
- Um teste pode cobrir vários casos, e um caso pode ter vários testes. Um caso só vale como **coberto** com um teste **no nível** da coluna "Nível".
- Não se cita um código que não existe.

## O que o `uc_coverage` confere

`crates/taba-node/tests/uc_coverage.rs` lê os códigos dos **cartões** (`### UC-XXX-NNN`) de [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md) e os citados em `crates/taba-node/{src,tests}` e em `connectors/*` (arquivos `.rs` e `.cs`). Falha quando:

1. um teste cita um código que **não existe** ou está `(retirado…)`;
2. um código aparece **duas vezes** na documentação.

A regra "todo caso documentado tem teste" **só falha com `TABA_UC_STRICT=1`**; sem a variável o teste imprime o placar (`cargo test --test uc_coverage -- --nocapture`). A verificação **não confere o nível**: citar o código num teste unitário não prova o caso de nível `I`; quem revisa olha o nível do cartão.

## Como manter os catálogos

| Catálogo | O que é |
|---|---|
| [26a](26a-casos-de-uso-implementados.md) | Casos com ao menos um teste, por área, com o nível e os testes que os provam |
| [26b](26b-casos-de-uso-planejados.md) | Casos **sem teste**: o que falta provar |

Os dois são **mantidos à mão**, nas duas línguas: um caso novo entra como cartão no formato dos demais (em 26a se tem teste, em 26b se não tem), e um caso que ganha o primeiro teste passa de 26b para 26a. Os códigos nunca são reaproveitados. Um caso em 26b **não quer dizer que o comportamento não existe**, só que **nenhum teste o cita**. Alguns têm "(pendente: D##)" no texto: dependem de uma dúvida em aberto ([31-pendencias.md](31-pendencias.md)).

## As áreas (38)

**Cliente** (a API que as aplicações usam):

| Área | Assunto |
|---|---|
| `TOP` | Tópicos: criar, listar, ler, alterar, remover |
| `MSG` | Publicação e o corpo da mensagem |
| `CNS` | Consumo, long polling e `autoCreate` |
| `ACK` | Confirmação do consumo |
| `VIS` | Alteração da invisibilidade |
| `RET` | Expiração (`ttl`), limite de entregas e dead-letter |
| `DST` | Tópico distribuidor |
| `VOL` | Fila volátil e nó gestor |
| `FIF` | Fila FIFO |
| `GRP` | Grupos de mensagens e entrega justa |
| `AUT` | Autenticação e tokens de acesso |
| `LIM` | Rate limit e IP do cliente |
| `PAG` | Paginação |
| `CNV` | Convenções comuns da API (formato, cabeçalhos, erros, status) |
| `CNT` | Conectores |
| `CLI` | O `taba-cli` |

**Nó e o caminho da mensagem:**

| Área | Assunto |
|---|---|
| `ESC` | Escrita: registro + commit, quórum, rollback, teto de commit |
| `ANE` | Anel de hash, blocos e papéis |
| `DEF` | Definição de tópicos e tokens replicada no cluster |
| `ENT` | Entrega no dono: lista de prontos, `take`, long polling |
| `CAN` | Canal permanente entre nós e filas de sinais |
| `INV` | Invisibilidade no dono e nas réplicas |
| `ARM` | Armazenamento: memória, overflow, SQLite, integridade |

**Nó e cluster:**

| Área | Assunto |
|---|---|
| `ING` | Entrada de nó (bootstrap) |
| `FAL` | Detecção de falhas, votação e saída por falha |
| `FRZ` | Período de freezing |
| `CNC` | Concorrência entre mudanças de membros |
| `SIN` | Sincronização distribuída de blocos |
| `STR` | Streams NDJSON entre nós |
| `RTN` | Retorno de nó excluído e recuperação de perdas |
| `FWD` | Repasse de chamadas de cliente |
| `DRN` | Parada e drain (inclui o drain como save) |
| `DEV` | Perfil de um nó (o antigo modo desenvolvimento) |
| `SEG` | Portas, TLS mútuo e certificados do cluster |
| `OPS` | Observabilidade, estatísticas, métricas e saúde |
| `CLR` | Contrato das rotas do cluster |
| `CFG` | Configuração e validações de partida |
| `BKP` | Backup opcional em S3: envio, restauração, batimento, evicção, flush e parada |
| `CHS` | Caos com histórico verificado: falhas sorteadas, o verificador do histórico e as falhas da rede |
| `BNC` | Benchmark comparativo local: o plano do `bench/run.sh`, as pastas dos sistemas e o relatório |

(O número de casos por área e a cobertura estão em [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md).)

## Premissas e limites do catálogo

- Os parâmetros sem valor fixo na doc histórica ("A definir") **não têm padrão fixado pelos testes**: os testes configuram o valor de que precisam.
- Os casos que dependem de pendência em aberto (D68, D70.1, D70.2: conectores) estão marcados **(pendente: D##)**: o resultado esperado pode mudar quando a dúvida for decidida.
- Os nomes de rotas, campos e códigos seguem [15](15-api-visao-geral.md) a [19](19-erros-e-limites.md) e os documentos OpenAPI (`openapi/`), que o `tests/openapi.rs` confere com respostas reais.
- Os casos **não cobrem desempenho** (sem benchmark) nem o texto das descrições nos documentos OpenAPI (o que se testa é que listam as rotas que o nó tem e que as respostas reais batem com seus schemas).

## Fontes e provas

- `crates/taba-node/tests/uc_coverage.rs` (as regras e o placar; **executado** em 2026-10-13).
- Casos de uso: este documento é o método; os casos estão em [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md).
- Histórico: `doc/20-casos-de-uso.md`, `doc/20a-casos-de-uso-cliente.md`, `20b`, `20c`.
