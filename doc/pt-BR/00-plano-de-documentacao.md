# Plano da nova documentação (pt-BR)

> **Status:** **executado** (2026-10-13, commit 71183b9). Este arquivo é o plano aprovado; o resultado da execução está na seção 9. As decisões da seção 8 foram respondidas.
> **Pasta:** `doc/pt-BR/`. A pasta antiga `doc/` **não é alterada nem apagada**; ela passa a ser *fonte secundária* (histórico de desenho). A fonte primária é o **código**.

## 1. Objetivo

A documentação antiga (`doc/`, 7,3 mil linhas, 34 arquivos) foi escrita **antes** do código, para guiar a implementação: mistura decisões, propostas, "A definir" e numeração contínua entre arquivos (a "seção 7.2" está num arquivo que só o índice revela). O código já existe, então a nova documentação tem outro papel:

> **Explicar como o Taba é hoje, para quem nunca viu o projeto, de modo que a pessoa (ou uma IA) consiga entender, operar, usar e contribuir sem ler o código nem o histórico.**

Isso muda três coisas em relação ao `doc/`:

1. **Descreve o que existe.** O que é proposta, parcial ou planejado é marcado (ver seção 5.2), nunca misturado ao texto como se fosse fato.
2. **Cada documento se sustenta sozinho.** Sem "seção 7.2 de outro arquivo": links por nome de arquivo e âncora, e um bloco "Pré-requisitos de leitura" no topo.
3. **Cada afirmação técnica aponta para o código** (módulo/arquivo e, quando existir, o teste/caso de uso que a prova).

## 2. Validação do escopo contra o código

Conferi a lista inicial com a estrutura real do repositório (`crates/taba-node`, `connectors/`, `docker/`, `.github/`, `exemplos/`, os 28 diagramas e as 3 portas HTTP).

### 2.1 O que da lista original se confirma

| Item pedido | Evidência no código | Observação |
|---|---|---|
| Arquitetura | módulos `core`, `storage`, `engine`, `cluster`, `http`, `telemetry`, `config`, `secrets`, `cli`, `node.rs` | Um crate só (`taba-node`), lib + binário `taba` |
| Dependências | `crates/taba-node/Cargo.toml`, `connectors/*`, Docker, HAProxy, SQLite embutida | Há três grupos: do servidor, dos conectores (zero dependências por regra) e de ferramentas (Node, Chrome, ffmpeg para diagramas) |
| Anéis | `core/ring`, `cluster/replication`, `cluster/sync` | "Anel" é um só (hash circular com blocos e papéis P/S/T); ver 2.3 |
| API (as 3) | `http/cluster_api` (+ canal WebSocket `/cluster/channel`), `http/operations.rs` (+ `/metrics`), `http/topics`, `messages`, `tokens` | Cliente, operação e cluster. O cluster tem ainda o **canal WebSocket** entre nós |
| Tipos de tópico | `engine/topics`, `cluster/volatile`, testes `fifo*`, `volatile*` | Fila (QT), distribuidor (DT), fila volátil (QTV), FIFO (QTF) |
| Envio, durabilidade, entrega | `cluster/replication/{coordinator,replica,forward}`, `engine/messages`, `storage/*` | Quórum, registro + commit, overflow em disco |
| Monitoramento | `telemetry/*`, `http/metrics`, `http/topic_stats`, `/health/*` | OTLP (http/protobuf e grpc), Prometheus, traces, logs, condições |
| Casos de uso | 3 arquivos `20a/b/c` + `tests/uc_coverage.rs` | Os casos são **testáveis**: cada `UC-…` é citado em testes |
| CLI | `connectors/rust/cli` (inclui TUI) **e** `crates/taba-node/src/cli` | São **duas CLIs diferentes**: `taba` (servidor: `init`, subir o nó) e `taba-cli` (cliente). A nova doc precisa separar as duas |
| Conectores | `connectors/rust/client`, `connectors/csharp/*` | Só **2 de 10** implementados (Rust e C#); os outros 8 são plano |
| Pipeline | `.github/workflows/{ci,build-*,release}.yml`, `Dockerfile*`, `docker/validate.sh` | |
| Glossário, mapa mental, convenções, pendências, próximos passos | `doc/09`, `16`, `18`, `06d`, `08b`, `08c` | Reaproveitáveis como fonte |

### 2.2 O que **falta** na lista e o código mostra ser essencial

| Lacuna | Evidência | Por que entra |
|---|---|---|
| **Ciclo de vida do cluster** (entrada, detecção de falha, votação, *freezing*, retorno, drain, modo 1 nó) | `cluster/{members,vote,health,recovery,drain,state}`, testes `failures`, `drain_cluster`, `recovery`, `freezing` | É onde está a maior parte da complexidade do projeto e não cabe em "anéis" |
| **Armazenamento e memória** (memória primeiro, overflow em disco, SQLite por tópico, `_node.db`, sync log) | `storage/{db,messages,topics,state,synclog,losses,tokens}` | Responde "onde minha mensagem está?" |
| **Invisibilidade, ack, TTL, dead-letter, grupos** | `engine/messages`, testes `messages`, `conditions` | É o coração da *entrega*; merece documento próprio, não um item de lista |
| **Segurança** (tokens, mTLS com certificado fixado e recarga, `taba init`, rate limit, IP do cliente/proxies, filtro de IPs de operação) | `secrets/`, `cluster/tls`, `http/{auth,ratelimit,client_ip}`, `engine/tokens`, testes `mtls`, `auth`, `ratelimit` | Transversal às 3 APIs; hoje espalhada |
| **Configuração** (tabela de parâmetros, `TABA_*`, YAML, padrões) | `config/params`, há teste que confere que todo parâmetro é lido | Primeira coisa que quem opera procura |
| **Início rápido** | `docker-compose.yml`, `docker/haproxy.cfg`, `taba init` | Quem chega precisa ver funcionando em 5 minutos |
| **Operação** (runbook: subir, escalar, derrubar nó, drain, backup, o que fazer em cada condição de `/health/cluster`) | `docker/cluster.sh`, `docker/validate.sh`, `operations.rs` | Hoje não existe como documento |
| **Testes e qualidade** (níveis U/N/C/conectores, rede em memória, relógio manual, `uc_coverage`) | `tests/common/{mod,cluster,real,otlp}.rs`, `tests/*.rs` | É o método que garante que a doc e o código batem |
| **Erros e limites** (catálogo `ErrorCode` com status fixo, limites fixos) | `core/error`, `doc/06d`, `doc/12` | Referência de consulta, não narrativa |
| **Decisões (ADR)** | `doc/08a` (D1…D120) | O "porquê" vive aqui e não pode se perder; entra como histórico, fora do fluxo principal |
| **Para contribuidores** (como compilar no Windows/Git Bash, SQLite `bundled`, disco do `target/`, ao terminar uma implementação) | `AGENTS.md` | Já existe, mas é instrução para IA; precisa de versão humana |

### 2.3 Pontos que a lista original pede que eu **reformule**

- **"Funcionamento dos anéis" (plural).** O código tem um único anel de hash (posição → bloco → papéis primário/secundário/terciário). O que parece "outro anel" é: a posição de um **grupo FIFO** no anel (`id` do tópico + `groupId`) e o **gestor** de tópicos voláteis, que muda quando o anel muda. Proposta: um documento "Anel e replicação" com o anel e, em seção própria, essas duas aplicações dele.
- **"Monitoramento"** dentro de "especificação funcional" vira documento próprio de **Observabilidade**, porque mistura métricas de nó, de tópico, traces, logs, condições de saúde e OTLP, e atende a quem opera.
- **"Pipeline"** é CI/CD do **repositório** (GitHub Actions, imagens, release). Não confundir com "pipeline de mensagens". O título será **"CI/CD e releases"**.
- **"Casos de uso"** deixa de ser narrativa e vira **catálogo gerado/verificado**: a nova doc só explica o formato e os 3 grupos com exemplos; a tabela completa continua sendo conferida por `uc_coverage` (ver 6.3 sobre não duplicar).
- **"Conectores"** precisa de uma tabela de **estado real** (implementado / planejado) e um guia de "como escrever um conector" (o contrato comum), porque 8 de 10 ainda não existem.
- **"Pendências" e "Próximos passos"** da doc antiga misturam o que está feito com o que falta (ex.: os passos 1–5 da 0.1 estão "feito"). A nova versão lista **só o que está aberto**, no formato da seção 5.4.

### 2.4 Divergências entre a doc antiga e o código (a registrar)

A doc antiga se descreve como "propostas" em vários pontos (bibliotecas, nomes de crates, parâmetros "A definir") e o `README.md` da raiz ainda diz que o projeto "está em fase de documentação". Durante a escrita, cada divergência encontrada vai para `doc/pt-BR/_divergencias.md` (arquivo de trabalho, não publicado no índice), com: o que a doc antiga diz, o que o código faz, qual dos dois está certo. Isso alimenta o `08b` novo e evita corrigir a doc antiga.

## 3. Estrutura proposta (ordem revisada)

**Princípio da ordem:** seguir a jornada de quem chega: *o que é → ver funcionando → modelo mental → como funciona por dentro → como usar → como operar → como contribuir → referência → história/futuro.* A lista original começava por arquitetura; isso exige que a pessoa já conheça os conceitos. Aqui os conceitos vêm antes.

Numeração por **parte**, links por nome de arquivo. Cada arquivo traz a coluna "Fontes" (onde se confere no código) e a "Doc antiga" (de onde reaproveitar).

### Parte A — Entender

| # | Arquivo | Conteúdo | Fontes no código | Doc antiga |
|---|---|---|---|---|
| 01 | `01-visao-geral.md` | O que é o Taba, para quem, o que **não** é; **objetivos e requisitos** numerados (`RF-`, `RNF-`) cada um com status; comparação com SQS/SNS/Kafka/RabbitMQ; estado do projeto | `README.md`, `Cargo.toml`, testes | 01, 03, 07, README raiz |
| 02 | `02-inicio-rapido.md` | Subir o Compose de 3 nós, criar token e tópico, publicar e consumir com `taba-cli`, derrubar um nó e ver o cluster reagir | `docker-compose.yml`, `docker/`, `taba init` | README raiz, 19b |
| 03 | `03-conceitos.md` | Modelo mental em 1 página: nó, cluster, tópico, mensagem, bloco, anel, quórum, invisibilidade, ack. Define cada termo **uma vez** e aponta para o detalhe | `core/*` | 09, 16 |

### Parte B — Como funciona por dentro

| # | Arquivo | Conteúdo | Fontes | Doc antiga |
|---|---|---|---|---|
| 04 | `04-arquitetura.md` | Contêineres e camadas do nó, fronteiras (traits `Transport`, `Clock`), três portas, composição no `main`/`node.rs`, mapa de módulos | `lib.rs`, `node.rs`, `main.rs` | 02, 18 |
| 05 | `05-dependencias.md` | Bibliotecas do servidor e **por que cada uma**, dependências de sistema (compilador C, Docker), ferramentas (diagramas), regra "conectores sem dependências" | `Cargo.toml`, `connectors/*` | 18 (17.2), 19, AGENTS |
| 06 | `06-anel-e-replicacao.md` | Hash circular, blocos, papéis P/S/T, fator de replicação, colocação de grupos FIFO e do gestor de voláteis no anel, o que acontece quando o anel muda | `core/ring`, `cluster/replication` | 05 |
| 07 | `07-cluster.md` | Entrada de nó, tabela de membros, detecção de falha, votação, *freezing*, retorno de nó excluído, tamanho mínimo e modo 1 nó, drain e parada, sincronização por streams | `cluster/{members,vote,health,recovery,sync,drain,state,channel}` | 04 |
| 08 | `08-armazenamento.md` | Memória primeiro, overflow para disco, SQLite por tópico, `_node.db`, log de sincronização, reconstrução após reinício, perdas | `storage/*` | 02 (3.2), 14 |

### Parte C — Especificação funcional

| # | Arquivo | Conteúdo | Fontes | Doc antiga |
|---|---|---|---|---|
| 09 | `09-topicos.md` | Tipos: fila, distribuidor, fila volátil (criação automática, gestor), FIFO (grupos, deduplicação). Tabela "qual escolher" e o que cada tipo permite | `engine/topics`, `cluster/volatile` | 15, 17 |
| 10 | `10-publicacao.md` | Caminho da escrita: qualquer nó recebe → encaminha ao primário → registro + commit → quórum → resposta; falhas no meio; publicação FIFO e em distribuidor | `cluster/replication`, `http/messages` | 02 (3.1), 05 |
| 11 | `11-durabilidade.md` | Garantias e **limites** (o que é confirmado, quando a mensagem pode ser perdida, o que acontece com 1 nó, com quórum perdido), retenção/TTL | `cluster/replication`, `storage`, testes `failures`, `recovery` | 05, 13 |
| 12 | `12-entrega.md` | Long polling, lista de prontos, dono da entrega, invisibilidade, ack, limite de entregas, dead-letter, grupos e entrega justa, entrega entre nós | `engine/messages`, `cluster/channel` | 13, 14 |
| 13 | `13-observabilidade.md` | Métricas de nó e de tópico, `/health/*` e condições, OTLP/Prometheus, traces, logs (formato text/color/json), amostragem | `telemetry/*`, `http/metrics`, `http/topic_stats` | 11 |
| 14 | `14-seguranca.md` | Tokens (criação, escopos, revogação), mTLS do cluster (`taba init`, rotação, recarga), rate limit, IP do cliente e proxies confiáveis, filtro de IPs de operação, riscos conhecidos | `secrets/`, `cluster/tls`, `http/{auth,ratelimit,client_ip}`, `engine/tokens` | 06 (8.0), 12 |

### Parte D — Interfaces

| # | Arquivo | Conteúdo | Fontes | Doc antiga |
|---|---|---|---|---|
| 15 | `15-api-visao-geral.md` | As 3 portas, quem usa cada uma, autenticação de cada, convenções (formato, cabeçalhos, paginação, DTO de erro) | `http/mod.rs`, `http/common` | 06, 06d |
| 16 | `16-api-cliente.md` | Tópicos, mensagens, tokens: rota a rota, com exemplo `curl` e resposta real | `http/{topics,messages,tokens}` | 06a |
| 17 | `17-api-operacao.md` | `/health/*`, `/metrics`, `/metrics/topics*`, drain | `http/operations.rs`, `metrics`, `topic_stats` | 06b |
| 18 | `18-api-cluster.md` | Rotas entre nós, canal WebSocket, streams NDJSON, DTOs. Aviso: **interna, sem garantia de estabilidade** | `http/cluster_api`, `cluster/protocol` | 06c |
| 19 | `19-erros-e-limites.md` | Catálogo de `ErrorCode` com status HTTP e causa; limites fixos e rate limits | `core/error`, `http/ratelimit` | 06d, 12 |
| 20 | `20-cli.md` | **Duas** CLIs: `taba` (servidor) e `taba-cli` (cliente, perfis, TUI, saída) | `crates/taba-node/src/cli`, `connectors/rust/cli` | 19b |
| 21 | `21-conectores.md` | Contrato comum, estado de cada linguagem, Rust e C# em detalhe, como escrever um novo | `connectors/*`, `exemplos/` | 19, 19a |

### Parte E — Operar

| # | Arquivo | Conteúdo | Fontes | Doc antiga |
|---|---|---|---|---|
| 22 | `22-configuracao.md` | Todas as chaves, padrões, variável `TABA_*` equivalente, quais são "calibragem pendente" | `config/params` | 10 |
| 23 | `23-operacao.md` | Runbook: subir, escalar, retirar nó (drain), nó caiu, quórum perdido, disco cheio, rotação de certificado, o que significa cada condição de saúde | `docker/`, `cluster/*`, `operations.rs` | 04, 11 (novo) |

### Parte F — Contribuir

| # | Arquivo | Conteúdo | Fontes | Doc antiga |
|---|---|---|---|---|
| 24 | `24-convencoes.md` | **Código** (idioma, módulos, `mod.rs` com `//!`, testes irmãos), **API** (nomes, erros), **pastas**, commits | `AGENTS.md`, `18` | 18, 06d, AGENTS |
| 25 | `25-testes.md` | Níveis de teste, `TestNode`/`TestCluster`, rede em memória, relógio manual, como escrever um teste que cita `UC-`, `uc_coverage` | `tests/`, `tests/common/` | 20 (19.1–19.5) |
| 26 | `26-casos-de-uso.md` | Formato do código `UC-<ÁREA>-<NNN>`, áreas, como um caso vira teste, como ler os dois catálogos abaixo | `tests/uc_coverage.rs` | 20 (19.1–19.5) |
| 26a | `26a-casos-de-uso-implementados.md` | Catálogo dos casos **com ao menos um teste** que os cita, agrupados por área, com o teste que os prova | `tests/*.rs`, `src/**/tests.rs`, `connectors/*` | 20a, 20b, 20c |
| 26b | `26b-casos-de-uso-planejados.md` | Catálogo dos casos **sem teste ainda** (coluna "Testes" = "—" na doc antiga) ou de recursos ⬜ planejados, cada um com o motivo e a pendência/marco que o fecha | `doc/20*`, `uc_coverage` | 20a, 20b, 20c |
| 27 | `27-ci-cd.md` | Workflows, imagens, versionamento e release, `validate.sh` | `.github/`, `Dockerfile*`, `docker/` | 21 |

### Parte G — Referência e projeto

| # | Arquivo | Conteúdo | Doc antiga |
|---|---|---|---|
| 28 | `28-glossario.md` | Termos em ordem alfabética, cada um com link ao documento que o explica | 09 |
| 29 | `29-mapa-mental.md` | Mapa geral e por área (só texto agora; ver diagramas) | 16 |
| 30 | `30-decisoes.md` | Histórico de decisões (ADR curto: contexto, decisão, consequência), por tema | 08a |
| 31 | `31-pendencias.md` | Só o que está **aberto**: dúvidas `D` e problemas `PRB`, no formato 5.4 | 08b |
| 32 | `32-roadmap.md` | O que falta e a ordem (OpenAPI, 8 conectores, publicação de pacotes, Kubernetes), por marco | 08c |

Arquivos de apoio:

- **`doc/README.md`** (novo, **en-US**): porta de entrada da pasta `doc/`, só aponta para a documentação de cada idioma (hoje `pt-BR/README.md`; a pasta antiga, sem idioma, é listada como "histórico de desenho, pt-BR"). Idiomas futuros entram como uma linha nova. **Atenção:** já existe um `doc/README.md` (o índice da doc antiga, em português). Como nada se apaga e mover o conteúdo quebraria os links internos da doc antiga, a proposta é: o `doc/README.md` atual **fica**, e ganha no topo um bloco en-US com a lista de idiomas e a nota "o índice abaixo é o histórico de desenho, em pt-BR". Ver decisão 3 na seção 8.
- **`doc/pt-BR/README.md`**: índice em português + "por onde começar" por perfil (usuário, operador, contribuidor, IA).
- **`doc/pt-BR/llms.txt`**: ver seção 4.
- **`doc/pt-BR/_divergencias.md`**: arquivo de trabalho.

**Raiz do repositório:** o `README.md` e o `AGENTS.md` devem ser **en-US**. Hoje estão em português. Isso é uma tarefa **separada** (tradução + links para `doc/README.md`), listada como marco no roadmap (fase 8); não será feita junto com a doc pt-BR.

## 4. Técnicas de documentação para projetos assistidos por IA

Aplicadas desde o primeiro arquivo:

| Técnica | Como entra aqui |
|---|---|
| **`llms.txt`** (índice curto em Markdown para modelos) | Um `doc/pt-BR/llms.txt` com uma linha por documento: título, link, "responde a quais perguntas" |
| **Metadados no topo (front matter)** | Cada arquivo começa com `título`, `resumo` (1 frase), `status`, `última verificação` (data + hash do commit), `fontes` (caminhos no código), `leia antes` |
| **Blocos autocontidos** | Seções curtas com título que já diz a resposta; nada de "como visto acima"; bom para recuperação por trechos (RAG) |
| **Rastreabilidade documento → código → teste** | Rodapé "Fontes e provas" em cada arquivo, com caminhos e códigos `UC-…`; o `uc_coverage` já faz o elo teste → caso de uso |
| **Estrutura Diátaxis** | Cada arquivo é de **um** tipo: *tutorial* (02), *explicação* (03–14), *guia prático* (23, 24), *referência* (15–22, 28). Evita misturar "por quê" com "como" |
| **ADRs** (registro de decisões) | `30-decisoes.md` guarda o "porquê"; a doc principal descreve só o resultado e linka a decisão |
| **Exemplos executáveis** | Todo `curl` e todo comando da doc vêm de execução real (ou de um teste); nenhum exemplo "de cabeça" |
| **Documentação como código** | Verificação automática: links quebrados, `UC-` citados que existem, parâmetros da tabela de config que existem, rotas documentadas que existem. Proposta de script em `doc/pt-BR/scripts/` (fase 4) |
| **Contexto para agentes** | O `AGENTS.md` da raiz passa a apontar para `doc/pt-BR/README.md` e `llms.txt` e a descrever **como manter** a doc nova |
| **Termos canônicos** | Um nome por conceito, em português; o identificador em inglês aparece em `código`, na primeira menção |

## 5. Convenções de escrita

### 5.1 Modelo de cada arquivo

```
---
título: …
resumo: uma frase
tipo: tutorial | explicação | guia | referência
status: estável | parcial | planejado
verificado-em: AAAA-MM-DD @ <commit>
fontes: [caminhos no código]
---
# Título
> Leia antes: [03-conceitos](03-conceitos.md)
## O que é / Para que serve
## Como funciona
## Exemplo
## Limites e comportamentos de borda
## Fontes e provas
```

### 5.2 Marcação de status (em cada recurso)

`✅ implementado` · `🟡 parcial` (diz o que falta) · `⬜ planejado` · `⚠️ limite conhecido` (aponta `PRB-…`). Um recurso só é `✅` se existe código **e** teste.

### 5.3 Idioma

Texto em português do Brasil; identificadores, rotas, chaves e códigos de erro em inglês, como no `AGENTS.md`. Uma tradução futura para outro idioma ganha pasta irmã (`doc/en/`).

### 5.4 Pendências e roadmap

Uma pendência é uma tabela com: **código** (`D…`/`PRB-…`), **tipo**, **onde no código**, **efeito**, **proposta**, **depende de**, **status**. O roadmap agrupa por marco e cita as pendências que cada marco fecha. As regras de numeração do `AGENTS.md` (códigos nunca reaproveitados) continuam valendo e os códigos são **os mesmos** da doc antiga.

## 6. Diagramas previstos (nenhum será desenhado agora)

Em cada arquivo, o ponto fica marcado com um bloco:

```
> 🖼️ **DIAGRAMA PLANEJADO** — tipo: …; objetivo: …; reaproveita: `doc/diagrams/source/….json`
```

| Arquivo | Diagrama | Tipo | Objetivo (o que o leitor deve entender) | Reaproveitar |
|---|---|---|---|---|
| 01 | Contexto do sistema | C4 nível 0 | Quem usa o Taba (aplicações, operador) e o que ele substitui | `c4-nivel0-contexto` |
| 02 | Topologia do Compose | Arquitetura (implantação) | O que sobe com `docker compose up`: init, 3 nós, HAProxy, cli | novo |
| 03 | Mapa de conceitos | Relações entre entidades | Como nó, cluster, tópico, mensagem, bloco e anel se ligam | novo |
| 04 | Contêineres e portas | C4 nível 1 | As 3 portas do nó e quem fala com quem | `c4-nivel1-conteineres` |
| 04 | Camadas do nó | Arquitetura em camadas | Dependência entre módulos (`core` → `storage` → `engine` → `cluster` → `http`) | novo |
| 06 | Anel e papéis | Desenho por script (anel) | Como posições mapeiam para blocos e P/S/T | `ring-hash.svg` |
| 06 | Tópico espalhado | Matriz mensagem × nó | Redundância: quem guarda o quê | `spread-topic.svg` |
| 06 | Anel muda (nó entra/sai) | Antes/depois do anel | Quais blocos migram e por quê | novo |
| 07 | Entrada de nó | Sequência | Passos do `join` até o nó estar ativo | `ingresso-no` |
| 07 | Remoção de nó | Sequência | Detecção, votação e exclusão | `remocao-no` |
| 07 | Falha no registro | Sequência | O que acontece quando o registro falha | `falha-registro-no` |
| 07 | Retorno de nó excluído | Sequência | Como um nó excluído volta como novo | `retorno-no-excluido` |
| 07 | Estados do nó | Máquina de estados | `joining`, `syncing`, `active`, `returning`, `failed` e o `drainState` (o freezing **não** é um estado) | novo |
| 08 | Memória e disco | Fluxo de dados | Quando a mensagem sai da memória para o disco e volta | novo |
| 09 | Escolha do tipo de tópico | Árvore de decisão | Qual tipo usar para cada necessidade | novo |
| 09 | Distribuidor | Sequência | Publicação em DT e fan-out | `distribuidor-publicacao` |
| 09 | Fila volátil | Sequência (3) | Criação automática, exclusão, troca de gestor | `fila-volatil-*` |
| 09 | Grupos FIFO no anel | Matriz grupo × nó | Onde cada grupo mora | `fifo-groups.svg` |
| 10 | Caminho da escrita | Arquitetura (fluxo) | Receber, encaminhar, registrar, commitar | `arquitetura-escrita`, `recepcao` |
| 10 | Publicação FIFO | Sequência | Como o primário do grupo coordena | `fifo-publicacao` |
| 11 | Garantias por cenário | Tabela visual (cenários × resultado) | O que sobrevive a cada falha | novo |
| 12 | Entrega e ack | Sequência | Do `GET` ao `DELETE` | `consumo` |
| 12 | Três cenários do `GET` | Sequência (3) | Lista local, outro nó, espera | `consumo-cenario1/2/3` |
| 12 | Ciclo da invisibilidade | Máquina de estados | Pronta → invisível → pronta/removida/DLQ | `invisibilidade` |
| 13 | Caminho da telemetria | Fluxo de dados | Nó → OTLP/Prometheus → coletor | novo |
| 14 | Fronteiras de confiança | Segurança (zonas) | Quem confia em quem: cliente/token, operação/IP, cluster/mTLS | novo |
| 15 | As três APIs | Arquitetura | Portas, públicos e autenticação | novo |
| 21 | Anatomia de um conector | Camadas | `client` → `transport` → `http`/`json` | novo |
| 23 | Árvore de diagnóstico | Fluxograma | Do sintoma (`/health/cluster`) à ação | novo |
| 25 | Pirâmide de testes do projeto | Camadas | U, N, C e conectores, e o que cada um simula | novo |
| 27 | Pipeline de CI/CD | Fluxo (workflows) | Do PR ao release | novo |
| 29 | Mapa mental geral e por área | Mapa mental | Visão de tudo e de cada área | `mapa-mental*` |

As regras de geração (JSON fonte → SVG, Archify) do `AGENTS.md` continuam valendo quando chegar a vez dos diagramas; a nova doc usará a mesma pasta `doc/diagrams/` para não duplicar.

## 7. Como escrever (método e fases)

Para cada arquivo, nesta ordem:

1. **Ler o código** do módulo da coluna "Fontes" (não a doc antiga).
2. **Ler a doc antiga** só para recuperar intenção e o "porquê".
3. Escrever; marcar status (5.2); onde código e doc antiga divergem, registrar em `_divergencias.md`.
4. Conferir cada exemplo executando-o.
5. Preencher "Fontes e provas" e `verificado-em`.

| Fase | Entrega | Motivo da ordem |
|---|---|---|
| 0 | Este plano, aprovado | — |
| 1 | `README`, `llms.txt`, 01, 02, 03 | Dá a porta de entrada e fixa o vocabulário que os demais usam |
| 2 | 04–08 (por dentro) | Base para tudo o que é funcional |
| 3 | 09–14 (funcional), 22 | Depende do vocabulário e do modelo de armazenamento |
| 4 | 15–21 (interfaces) | Rota a rota, com exemplos reais |
| 5 | 23–27 (operar e contribuir) | Usa o que já foi explicado |
| 6 | 28–32 (referência e projeto) | Glossário e mapa só fecham no fim, quando os termos estão estáveis |
| 7 | Verificação automática (links, `UC-`, parâmetros, rotas) | Impede a doc de envelhecer de novo |
| 8 | Raiz: `README.md` e `AGENTS.md` em en-US apontando para `doc/README.md` | Tarefa separada, depois da doc pt-BR |

Estimativa de tamanho: doc nova mais enxuta que a antiga (alvo ≈ 4 a 5 mil linhas, contra 7,3 mil), porque os casos de uso (`20a-c`, ~1,5 mil linhas) **não são copiados**.

## 8. Decisões

| # | Decisão | Resposta |
|---|---|---|
| 1 | Pasta | **`doc/pt-BR/`** (confirmado) |
| 2 | Casos de uso | **Dois arquivos novos**: `26a` (implementados, com teste) e `26b` (planejados, sem teste). Consequência: o `uc_coverage` hoje lê `doc/20a-c`; enquanto a doc antiga existir ele continua lendo lá. Migrar o teste para ler `26a`/`26b` é uma mudança de código, **fora deste plano**; a nova doc cita os mesmos códigos `UC-`, que não mudam |
| 3 | Índice por idioma | **`doc/README.md`** referencia o README de cada idioma; raiz e `AGENTS.md` em en-US (ver seção 3, "Arquivos de apoio"). Pendente: confirmar a proposta de manter o `doc/README.md` antigo com o bloco en-US no topo, já que "não apagar nada" impede substituí-lo |
| 4 | Partes E (Operar) e F (Contribuir) | Esclarecimento abaixo. **Aprovado por padrão**, a menos que você diga o contrário |
| 5 | Front matter e `llms.txt` | **Sim** |

**O que são as Partes E e F** (não estavam na sua lista; vieram do código):

- **E — Operar** (22 configuração, 23 operação): para quem **roda** o Taba em produção. Todas as chaves de configuração e seus padrões, e um runbook: o que fazer quando um nó cai, o quórum se perde, o disco enche, o certificado vence, e o que cada condição de `/health/cluster` significa. Sem isso, a doc explicaria como o Taba funciona, mas não como mantê-lo de pé.
- **F — Contribuir** (24 convenções, 25 testes, 26/26a/26b casos de uso, 27 CI/CD): para quem **altera o código**. Inclui o que você já pediu (convenções de API, código e pastas; casos de uso; pipeline) e acrescenta o guia de testes (`TestNode`, `TestCluster`, rede em memória), que é o que prende a doc ao código.

Ou seja, E e F não são extras soltos: a sua lista já tinha quase tudo de F; E é o único grupo realmente novo.

## 9. Resultado da execução (2026-10-13)

**Entregue:** 33 documentos numerados (01 a 32, mais 26a e 26b), `README.md` e `llms.txt`, `_divergencias.md`, três scripts em `scripts/` (`gerar-casos-de-uso.mjs`, `gerar-historico.mjs`, `verificar.mjs`) e, fora desta pasta, o `doc/README.md` com o índice de idiomas e o `README.md` e o `AGENTS.md` da raiz em **en-US**. A doc histórica (`doc/*.md`) **não foi alterada** (só o `doc/README.md` ganhou um bloco no topo).

| Fase | Entrega | Estado |
|---|---|---|
| 1 | `README`, `llms.txt`, 01, 02, 03 | ✅ |
| 2 | 04 a 08 | ✅ |
| 3 | 09 a 14 e 22 | ✅ |
| 4 | 15 a 21 | ✅ |
| 5 | 23 a 27 | ✅ |
| 6 | 28 a 32 | ✅ (30 e 31 e os catálogos 26a/26b são gerados por script) |
| 7 | Verificação automática | ✅ `node doc/pt-BR/scripts/verificar.mjs`: links, metadados, 105 chaves de configuração, 50 códigos de erro, 43 rotas, 84 métricas, 896 códigos `UC-`, 31 diagramas planejados |
| 8 | Raiz em en-US | ✅ `README.md` e `AGENTS.md` traduzidos; `doc/README.md` lista os idiomas |
| 9 | Diagramas | ✅ Os 31 pontos resolvidos: 15 diagramas novos em `doc/diagrams/source/`, os demais reaproveitados (os de escrita e de mapa mental tiveram textos corrigidos contra o código), a tela da TUI em texto e a tabela de durabilidade em Markdown. Índice em [24-convencoes.md](24-convencoes.md#índice-dos-diagramas) |

**Como foi escrito:** cada documento partiu do **código** (módulos, `//!`, catálogos, tabelas) e usou a doc histórica só para o "porquê". Os exemplos de API, CLI, cluster, segurança e operação foram **executados** contra um nó em modo desenvolvimento e contra um cluster de três nós subido por `docker compose`. As divergências entre a doc histórica e o código estão em [_divergencias.md](_divergencias.md) (25) e as descobertas, em [31-pendencias.md](31-pendencias.md).

**O que foi achado** (resumo; detalhe em [31-pendencias.md](31-pendencias.md)):

1. As métricas de tópico **não agregam entre nós** (`scope=cluster` devolve só o nó que atende).
2. **Sete parâmetros** de configuração são aceitos mas não têm efeito.
3. **Perder dois nós seguidos trava** um cluster de três (`quorum-lost`), sem exclusão forçada.
4. **Parar o cluster inteiro** em produção perde mensagens **e tópicos**.
5. O teste `uc_coverage` **falha** hoje (coluna "Testes" velha).
6. `docker compose stop` é um **drain**, não uma queda; para simular falha use `kill`.
7. Várias pendências do `08b` estão velhas (D79/PRB-013) e comentários `//!` do código também.

**Limites desta execução:** a suíte completa do `cargo test` **não** foi executada (a máquina ficou sem disco no *link*); foram executados `uc_coverage`, `--lib` (509 testes) e cinco arquivos de integração, todos verdes exceto a coluna "Testes" do `uc_coverage`. Os **diagramas não foram desenhados** (31 pontos marcados). O Kubernetes **não** foi validado.
