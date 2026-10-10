# Taba — Documentação

> Versão 0.9 — documento vivo (escopo + documentação técnica), atualizado conforme o projeto evolui.

Taba é uma mensageria altamente escalável, rápida e simples, inspirada no Amazon SQS e SNS. Para a apresentação do projeto, veja o [README da raiz](../README.md).

> Quer entender como tudo isso funciona? Comece pela [visão geral](01-visao-geral.md) e siga o índice abaixo.

## Índice

| Arquivo | Seções | Conteúdo |
|---|---|---|
| [01-visao-geral.md](01-visao-geral.md) | 1, 2 | Descrição e objetivos. |
| [02-arquitetura.md](02-arquitetura.md) | 3 | Camadas do nó, fluxo de escrita de mensagem, memória e overflow para disco. |
| [03-escopo.md](03-escopo.md) | 4, 5 | O que está dentro e fora do escopo inicial. |
| [04-cluster.md](04-cluster.md) | 6 | Descoberta, registro de nós, detecção de falhas, votação, retorno de nó, freezing; tamanho mínimo e modo desenvolvimento (1 nó, sem votação nem quórum); parada e modo drain. |
| [05-particionamento-replicacao.md](05-particionamento-replicacao.md) | 7 | Hash circular, registro + commit de mensagens, sincronização distribuída de blocos. |
| [06-api.md](06-api.md) | 8, 8.0 | Visão geral das três APIs: as portas (cliente, cluster e operação) e a segurança da porta do cluster (certificados, rotação, exemplo com Docker). |
| [06a-api-cliente.md](06a-api-cliente.md) | 8.1, 8.1.2, 8.1.3 | API de cliente: tópicos, mensagens, tags, paginação e autenticação por token. |
| [06b-api-operacao.md](06b-api-operacao.md) | 8.1.1 | API de operação: `/metrics` (nó e tópicos), `/health` (nó e cluster) e drain. |
| [06c-api-cluster.md](06c-api-cluster.md) | 8.2, 8.2.1 | API do cluster: rotas entre nós, DTOs e streams NDJSON de sincronização. |
| [06d-api-convencoes.md](06d-api-convencoes.md) | 8.3 | Convenções comuns às três APIs: formato, cabeçalhos, DTO de erro, envelope de paginação, códigos de status e catálogo de códigos de erro. |
| [07-principios-design.md](07-principios-design.md) | 9 | Princípios de design. |
| [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) | 10.1 | Decisões já tomadas, por tema, com o código da dúvida e onde estão registradas. |
| [08b-pendencias.md](08b-pendencias.md) | 10.2 | Pendências em aberto. |
| [08c-proximos-passos.md](08c-proximos-passos.md) | 10.3 | Os passos do trabalho e as dúvidas de cada um. |
| [09-glossario.md](09-glossario.md) | 11 | Termos do projeto. |
| [10-configuracao.md](10-configuracao.md) | — | Parâmetros de configuração e valores padrão (inclui observabilidade e rate limits). |
| [11-observabilidade.md](11-observabilidade.md) | — | Observabilidade em OpenTelemetry: como ligar um coletor (OTLP e variáveis `OTEL_*`), métricas `taba.*` e `http.*`/`process.*` por nó, `/metrics` de cada nó, estado e condições (`/health/cluster`), métricas e condição de tópicos (rotas próprias), traces, logs, eventos, health checks. |
| [12-limites-api.md](12-limites-api.md) | — | Limites fixos da API e definição dos rate limits. |
| [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) | 12 | Controle de entrega: contador de entregas, lista de prontos, dono da entrega, long polling e canal entre nós; ack por registro + commit; expiração (TTL), limite de entregas e dead-letter (12.10); grupos de mensagens e entrega justa (12.11). |
| [15-topicos-distribuidor.md](15-topicos-distribuidor.md) | 14 | Nomenclatura (QT, DT, QTV; análogos ao SQS e ao SNS), Tópico Distribuidor por prefixos, fila volátil, criação automática pelo `GET`, nó gestor e sua relação com entrada, saída e rebalanceamento de nós. |
| [17-topicos-fifo.md](17-topicos-fifo.md) | 16 | Fila FIFO (QTF): ordem por grupo, colocação do grupo no anel pelo `id` do tópico e `groupId`, publicação coordenada pelo primário do grupo, quórum com primário e secundário, entrega de uma mensagem por grupo, deduplicação (`deduplicationId`, exclusão lógica `deleteAt`), limites e observabilidade. |
| [14-invisibilidade.md](14-invisibilidade.md) | 13 | Invisibilidade: metadados, propagação entre nós, lista de invisíveis em memória, excedente em disco (índice de expiração), vencimento e reconstrução. |
| [16-mapa-mental.md](16-mapa-mental.md) | 15 | Mapa mental do projeto em cinco níveis: o mapa geral e um mapa por área, com decisões e parâmetros. |
| [18-estrutura-codigo.md](18-estrutura-codigo.md) | 17 | Linguagem (Rust, tokio), bibliotecas de partida, estrutura do workspace (um crate com módulos, crates-alvo), regras de código e o que fica atrás de traits para testes. |
| [19-conectores.md](19-conectores.md) | 18 | Conectores (bibliotecas de cliente) em dez linguagens: pastas em `connectors/`, superfície mínima, contrato comum (long polling, repetição, erros), sem dependências, regras para versões de até 15 anos, suíte de conformidade e plano de entrega. |
| [19a-conectores-linguagens.md](19a-conectores-linguagens.md) | 18.10 | Versão-base, tecnologia (HTTP, JSON, base64), particularidades e imagens Docker de compilação e teste de cada linguagem. |
| [19b-cli.md](19b-cli.md) | 18.11 | O `taba-cli`, a linha de comando do cliente e primeiro conector: todos os comandos da API de cliente e de operação, configuração, saída, imagem Docker em Alpine e scripts de build para Windows, Linux e macOS (x64 e arm64). |
| [20-casos-de-uso.md](20-casos-de-uso.md) | 19 | Casos de uso que orientam os testes: código `UC-<ÁREA>-<NNN>` de cada caso, níveis de teste (unitário, nó único, cluster, conectores), como citar o código nos testes para provar a cobertura, e o índice das áreas. |
| [20a-casos-de-uso-cliente.md](20a-casos-de-uso-cliente.md) | 19.A | Casos de uso do cliente: tópicos, publicação, consumo, ack, invisibilidade, retenção e dead-letter, distribuidor, fila volátil, FIFO, grupos, tokens, limites, paginação, convenções da API e conectores. |
| [20b-casos-de-uso-no-dados.md](20b-casos-de-uso-no-dados.md) | 19.B | Casos de uso do nó e do caminho da mensagem: escrita e quórum, anel de hash, definição de tópicos no cluster, entrega entre nós, canal, invisibilidade e armazenamento. |
| [20c-casos-de-uso-no-cluster.md](20c-casos-de-uso-no-cluster.md) | 19.C | Casos de uso do nó e do cluster: entrada, falhas e votação, freezing, concorrência de membros, sincronização, streams, retorno de nó, repasse, drain, modo desenvolvimento, segurança, observabilidade, rotas do cluster e configuração. |

## Diagramas

Os diagramas ficam em [diagramas/](diagramas/): a fonte é o JSON em `diagramas/fonte/` e o SVG embutido nos documentos fica em `diagramas/img/`. Para editar e regerar, veja a seção "Diagramas" do [AGENTS.md](../AGENTS.md).

| Diagrama | Onde aparece |
|---|---|
| C4 nível 0, contexto | [01-visao-geral.md](01-visao-geral.md) |
| C4 nível 1, contêineres | [02-arquitetura.md](02-arquitetura.md), seção 3 |
| Arquitetura da escrita | [02-arquitetura.md](02-arquitetura.md), seção 3.1 |
| Anel de hash: blocos e papéis | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.1 |
| Tópico espalhado e redundante | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.1 |
| Recepção de mensagem | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| Ingresso de nó | [04-cluster.md](04-cluster.md), seção 6.2 |
| Remoção de nó | [04-cluster.md](04-cluster.md), seção 6.3 |
| Falha no registro de nó | [04-cluster.md](04-cluster.md), seção 6.5 |
| Retorno de nó excluído | [04-cluster.md](04-cluster.md), seção 6.6 |
| Ciclo da invisibilidade | [14-invisibilidade.md](14-invisibilidade.md), seção 13.1 |
| Entrega e ack | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.2 |
| Três cenários do `GET` | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |
| Publicação em Tópico Distribuidor | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2 |
| Criação automática de fila volátil | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.4 |
| Exclusão de fila volátil pelo gestor | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5 |
| Troca de gestor nas mudanças do anel | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6 |
| Grupos de uma fila FIFO no anel | [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.3 |
| Publicação numa fila FIFO | [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.4 |
| Mapa mental do Taba (geral e oito áreas) | [16-mapa-mental.md](16-mapa-mental.md), seção 15 |

> A numeração das seções é contínua entre os arquivos: uma referência como "seção 7.2" está sempre no arquivo cujo índice lista o número 7.
