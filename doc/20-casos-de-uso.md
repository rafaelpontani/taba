# Casos de uso

[← Índice](README.md)

## 19. Casos de uso e rastreabilidade dos testes

Este conjunto de documentos lista **todos os casos de uso** do Taba, cada um com um **código único**. Os testes unitários e de integração citam esse código, e assim dá para provar que **todo caso de uso está coberto por ao menos um teste** (e que nenhum teste testa algo que não está documentado).

Os casos foram derivados da documentação técnica (seções 1 a 18). Quando o texto de origem muda, o caso de uso muda junto, no mesmo commit.

> Os casos descrevem **comportamento observável**, não a implementação. "Resultado" é o que um teste deve conferir: um status, um corpo, um estado do disco, uma métrica, uma mensagem de log ou o que o outro nó enxerga.

### 19.1 Como os arquivos se dividem

A ordem segue o caminho de quem usa o Taba: primeiro o cliente, depois o que acontece dentro do nó, e por fim a vida do cluster (entrada, saída, falhas).

| Arquivo | Seção | Conteúdo |
|---|---|---|---:|
| [20-casos-de-uso.md](20-casos-de-uso.md) | 19 | Este arquivo: convenções, níveis de teste, como citar o código nos testes e o índice de áreas. |
| [20a-casos-de-uso-cliente.md](20a-casos-de-uso-cliente.md) | 19.A | O que o **cliente** faz: tópicos, publicação, consumo, ack, invisibilidade, retenção e dead-letter, distribuidor, fila volátil, FIFO, grupos, tokens, limites, paginação, convenções da API e conectores. |
| [20b-casos-de-uso-no-dados.md](20b-casos-de-uso-no-dados.md) | 19.B | O que o **nó** faz com a mensagem: escrita e quórum, anel de hash, definição de tópicos no cluster, entrega entre nós, canal, invisibilidade, armazenamento. |
| [20c-casos-de-uso-no-cluster.md](20c-casos-de-uso-no-cluster.md) | 19.C | O que acontece com o **nó e o cluster**: entrada, falhas e votação, freezing, concorrência de membros, sincronização, streams, retorno de nó excluído, repasse, drain, modo desenvolvimento, segurança das portas, observabilidade, rotas do cluster e configuração. |

### 19.2 Código do caso de uso

```
UC-<ÁREA>-<NNN>
```

- `<ÁREA>`: três letras, a área do caso (tabela da seção 19.5). Ex.: `MSG` (publicação), `ESC` (escrita e quórum).
- `<NNN>`: número sequencial dentro da área, com três dígitos. Ex.: `UC-MSG-003`.
- **Os códigos nunca são reaproveitados**, como os códigos de dúvida (D13, D29) de [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md). Um caso que deixa de existir fica na tabela com o texto `(retirado)`, sem apagar a linha, e o número não volta a ser usado. Um caso novo ganha o próximo número livre da área, mesmo que fique fora da ordem do assunto.
- Um caso que se divide em dois vira dois códigos novos, e o original é retirado.

### 19.3 Colunas

Todas as tabelas têm as mesmas colunas:

| Coluna | Significado |
|---|---|
| **Código** | `UC-<ÁREA>-<NNN>`. |
| **Caso de uso** | A situação (**quando**) e o resultado esperado (**então**), numa frase. É o que o teste precisa conferir. |
| **Fonte** | O documento e a seção onde o comportamento está definido (por exemplo, `06a` é [06a-api-cliente.md](06a-api-cliente.md) e `13 §12.2` é a seção 12.2 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). |
| **Nível** | O nível de teste que deve cobrir o caso (seção 19.4). Pode haver mais de um. |

### 19.4 Níveis de teste

| Sigla | Nível | O que roda | Onde fica |
|---|---|---|---|
| **U** | Unitário | Uma peça isolada, sem rede nem disco reais: lógica de domínio, validação, anel de hash, listas, cálculo de quórum, leitor de JSON. `Clock`, `Transport` e `Storage` são simulados ([18-estrutura-codigo.md](18-estrutura-codigo.md), seção 17.5). | Junto do código, nos módulos. |
| **I** | Integração de um nó | Um nó de verdade em **modo desenvolvimento** ([04-cluster.md](04-cluster.md), seção 6.4), falando HTTP real, com disco real (pasta temporária). Cobre o que o cliente enxerga. | `crates/taba-node/tests/` |
| **C** | Integração de cluster | **Vários nós no mesmo processo**, com `Transport` em memória (perda, atraso e partição programáveis) e `Clock` manual. Cobre quórum, rollback, falha de nó, votação, sincronização, drain. | `crates/taba-node/tests/` (cluster simulado) |
| **K** | Conformidade de conectores | As fixtures e os cenários de `connectors/contrato/`, rodados por todos os conectores ([19-conectores.md](19-conectores.md), seção 18.7). | `connectors/<linguagem>/test/` |

Regra prática: o que depende **só do cliente e de um nó** é `I`; o que depende de **mais de um nó, de falha ou de tempo** é `C`; o que é uma regra pura é `U`. Um caso pode pedir os dois (`U, I`) quando a regra tem teste próprio e também precisa ser visto na API.

### 19.5 Como citar o código no teste

Todo teste declara **quais casos de uso ele cobre**, numa linha de comentário no formato abaixo, logo acima da função de teste. É uma linha só, com um ou mais códigos separados por vírgula:

```rust
/// UC: UC-MSG-003, UC-ESC-004
#[test]
fn publicar_binario_guarda_bytes_crus() { /* ... */ }
```

Nos conectores, a mesma linha no comentário de cada linguagem (`// UC: ...`, `# UC: ...`) ou no nome do arquivo de fixture/cenário (`UC-MSG-003.json`).

Regras:

- O prefixo `UC:` e os códigos completos (`UC-MSG-003`, nunca só `MSG-003`) permitem achar tudo com uma busca de texto.
- Um teste pode cobrir vários casos, e um caso pode ter vários testes. Um caso só está **coberto** quando existe ao menos um teste no nível indicado na coluna "Nível". Quando o nível lista mais de um (`U, I`), deve haver um teste em cada.
- Não se cita um código que não existe nestes arquivos.

**Verificação de cobertura.** O teste `crates/taba-node/tests/uc_coverage.rs` lê os códigos destes arquivos (as linhas de tabela que começam por `| UC-`) e os códigos citados em `crates/taba-node/src/` e `crates/taba-node/tests/`, e falha quando:

1. algum teste cita um código que não existe, ou que está `(retirado)`;
2. algum código aparece duas vezes na documentação;
3. algum código documentado (que não esteja `(retirado)`) não aparece em nenhum teste. Esta regra só falha com `TABA_UC_STRICT=1`: enquanto a implementação está no começo, o teste só imprime o placar (`cargo test --test uc_coverage -- --nocapture`). Ela passa a valer sempre, no CI, quando o protótipo de nó único (passo 3 de [08c-proximos-passos.md](08c-proximos-passos.md)) cobrir os casos de nível `I`.

A verificação não confere o **nível** (`U`, `I`, `C`, `K`): citar o código num teste unitário não prova o caso de nível `I`, e quem revisa precisa olhar a coluna "Nível".

### 19.6 Índice de áreas

#### Cliente ([20a-casos-de-uso-cliente.md](20a-casos-de-uso-cliente.md))

| Área | Assunto | Documentos de origem | Casos |
|---|---|---|---:|
| `TOP` | Tópicos: criar, listar, ler, alterar e remover | [06a](06a-api-cliente.md), [02](02-arquitetura.md) | 43 |
| `MSG` | Publicação e o corpo da mensagem | [06a](06a-api-cliente.md), [12](12-limites-api.md) | 20 |
| `CNS` | Consumo (`GET`), long polling e `autoCreate` | [06a](06a-api-cliente.md), [13](13-entrega-de-mensagens.md) | 40 |
| `ACK` | Confirmação do consumo | [06a](06a-api-cliente.md), [13](13-entrega-de-mensagens.md) | 13 |
| `VIS` | Alteração da invisibilidade | [06a](06a-api-cliente.md), [14](14-invisibilidade.md) | 12 |
| `RET` | Expiração (`ttl`), limite de entregas e dead-letter | [13 §12.10](13-entrega-de-mensagens.md) | 20 |
| `DST` | Tópico Distribuidor | [15 §14.2](15-topicos-distribuidor.md) | 18 |
| `VOL` | Fila volátil e nó gestor | [15 §14.3 a 14.6](15-topicos-distribuidor.md) | 23 |
| `FIF` | Fila FIFO | [17](17-topicos-fifo.md) | 47 |
| `GRP` | Grupos de mensagens e entrega justa | [13 §12.11](13-entrega-de-mensagens.md) | 17 |
| `AUT` | Autenticação e tokens de acesso | [06a §8.1.3](06a-api-cliente.md), [02 §3.4](02-arquitetura.md) | 36 |
| `LIM` | Rate limit e IP do cliente | [12](12-limites-api.md) | 15 |
| `PAG` | Paginação | [06a §8.1.2](06a-api-cliente.md), [06d §8.3.4](06d-api-convencoes.md) | 9 |
| `CNV` | Convenções comuns da API (formato, cabeçalhos, erros, status) | [06d](06d-api-convencoes.md) | 17 |
| `CNT` | Conectores | [19](19-conectores.md), [19a](19a-conectores-linguagens.md) | 32 |

#### Nó: o caminho da mensagem ([20b-casos-de-uso-no-dados.md](20b-casos-de-uso-no-dados.md))

| Área | Assunto | Documentos de origem | Casos |
|---|---|---|---:|
| `ESC` | Escrita: registro + commit, quórum, rollback, teto de commit | [05 §7.2](05-particionamento-replicacao.md), [02 §3.1](02-arquitetura.md) | 36 |
| `ANE` | Anel de hash, blocos e papéis | [05 §7.1](05-particionamento-replicacao.md) | 12 |
| `DEF` | Definição de tópicos e de tokens replicada no cluster | [02 §3.3 e 3.4](02-arquitetura.md) | 24 |
| `ENT` | Entrega no dono: lista de prontos, `take`, long polling | [13 §12.6 a 12.8](13-entrega-de-mensagens.md) | 31 |
| `CAN` | Canal permanente entre nós e filas de sinais | [13 §12.8 e 12.9](13-entrega-de-mensagens.md), [06c](06c-api-cluster.md) | 25 |
| `INV` | Invisibilidade no dono e nas réplicas | [14](14-invisibilidade.md) | 32 |
| `ARM` | Armazenamento: memória, overflow, SQLite, integridade | [02 §3.2](02-arquitetura.md) | 25 |

#### Nó e cluster: entrada, saída, falhas ([20c-casos-de-uso-no-cluster.md](20c-casos-de-uso-no-cluster.md))

| Área | Assunto | Documentos de origem | Casos |
|---|---|---|---:|
| `ING` | Entrada de nó (bootstrap) | [04 §6.2 e 6.5](04-cluster.md) | 27 |
| `FAL` | Detecção de falhas, votação de exclusão e saída por falha | [04 §6.3](04-cluster.md) | 27 |
| `FRZ` | Período de freezing | [04 §6.7](04-cluster.md) | 9 |
| `CNC` | Concorrência entre mudanças de membros | [04 §6.8](04-cluster.md) | 9 |
| `SIN` | Sincronização distribuída de blocos | [05 §7.3](05-particionamento-replicacao.md) | 29 |
| `STR` | Streams NDJSON entre nós | [06c §8.2.1](06c-api-cluster.md) | 15 |
| `RTN` | Retorno de nó excluído e recuperação de perdas | [04 §6.6](04-cluster.md) | 13 |
| `FWD` | Repasse de chamadas de cliente | [04 §6.9](04-cluster.md) | 12 |
| `DRN` | Parada e modo drain (inclui o drain como save) | [04 §6.10](04-cluster.md) | 28 |
| `DEV` | Modo desenvolvimento | [04 §6.4](04-cluster.md) | 13 |
| `SEG` | Portas, TLS mútuo e certificados do cluster | [06 §8.0](06-api.md) | 21 |
| `OPS` | Observabilidade (OpenTelemetry), estatísticas, métricas e saúde | [11](11-observabilidade.md), [06b](06b-api-operacao.md) | 42 |
| `CLR` | Contrato das rotas do cluster (DTOs e regras comuns) | [06c](06c-api-cluster.md) | 10 |
| `CFG` | Configuração e validações de partida | [10](10-configuracao.md) | 12 |

**Total: 814 casos de uso** (os casos `(retirado)` não entram na contagem).

### 19.7 Premissas e limites

- Os parâmetros marcados **"A definir"** em [10-configuracao.md](10-configuracao.md) (retries, intervalos, tetos) não têm valor fixo: os testes configuram o valor de que precisam, em vez de depender de um padrão.
- Os casos que dependem de pendência em aberto ([08b-pendencias.md](08b-pendencias.md), D68, D70.1, D70.2 e D71.1, conectores) estão marcados com **(pendente: D##)**. O caso existe, mas o resultado esperado pode mudar quando a dúvida for decidida.
- Os nomes de rotas, campos e códigos de erro são os do rascunho da API ([06-api.md](06-api.md)) e acompanham o contrato OpenAPI quando ele existir.
- Estes casos não cobrem desempenho (as metas de velocidade e custo ainda não têm benchmark, [README](../README.md#status-e-limites)) nem o conteúdo do contrato OpenAPI.
