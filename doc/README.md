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
| [06-api.md](06-api.md) | 8 | API de cliente (tópicos, mensagens, tags) e API interna do cluster. |
| [07-principios-design.md](07-principios-design.md) | 9 | Princípios de design. |
| [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) | 10.1 | Decisões já tomadas, por tema, com o código da dúvida e onde estão registradas. |
| [08b-pendencias.md](08b-pendencias.md) | 10.2 | Pendências em aberto. |
| [08c-proximos-passos.md](08c-proximos-passos.md) | 10.3 | Os passos do trabalho e as dúvidas de cada um. |
| [09-glossario.md](09-glossario.md) | 11 | Termos do projeto. |
| [10-configuracao.md](10-configuracao.md) | — | Parâmetros de configuração e valores padrão (inclui observabilidade e rate limits). |
| [11-observabilidade.md](11-observabilidade.md) | — | Observabilidade: métricas por nó, métricas e condição de tópicos (rotas próprias), endpoint Prometheus (`/metrics`), traces, logs, health checks. |
| [12-limites-api.md](12-limites-api.md) | — | Limites fixos da API e definição dos rate limits. |
| [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) | 12 | Controle de entrega: contador de entregas, lista de prontos, dono da entrega, long polling e canal entre nós; ack por registro + commit; expiração (TTL), limite de entregas e dead-letter (12.10); grupos de mensagens e entrega justa (12.11). |
| [15-topicos-distribuidor.md](15-topicos-distribuidor.md) | 14 | Nomenclatura (QT, DT, QTV; análogos ao SQS e ao SNS), Tópico Distribuidor por prefixos, fila volátil, criação automática pelo `GET`, nó gestor e sua relação com entrada, saída e rebalanceamento de nós. |
| [14-invisibilidade.md](14-invisibilidade.md) | 13 | Invisibilidade: metadados, propagação entre nós, lista de invisíveis em memória, excedente em disco (índice de expiração), vencimento e reconstrução. |
| [16-mapa-mental.md](16-mapa-mental.md) | 15 | Mapa mental do projeto em cinco níveis: o mapa geral e um mapa por área, com decisões e parâmetros. |

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
| Métricas do cluster em memória e coleta do Prometheus | [11-observabilidade.md](11-observabilidade.md), seção "Endpoint Prometheus" |
| Mapa mental do Taba (geral e oito áreas) | [16-mapa-mental.md](16-mapa-mental.md), seção 15 |

> A numeração das seções é contínua entre os arquivos: uma referência como "seção 7.2" está sempre no arquivo cujo índice lista o número 7.
