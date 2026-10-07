# Mapa mental

[← Índice](README.md)

## 15. Mapa mental do projeto

Visão do Taba em **cinco níveis**. O primeiro mapa mostra os dois primeiros níveis (o sistema e suas oito áreas); cada área se abre em seu próprio mapa, com os três níveis seguintes. Serve para se situar antes de entrar nos documentos técnicos.

**Cada caixa tem um número**, e a tabela abaixo de cada mapa liga esse número ao documento (e à seção) onde a definição está. A numeração é hierárquica: a área é `2`, um tema dela é `2.1`, uma decisão `2.1.1` e o detalhe `2.1.1.1`.

| Nível | O que é | Número | Cor nos mapas |
|---|---|---|---|
| 1 | O sistema (Taba) | — | verde, no mapa geral |
| 2 | Área (um ramo do sistema) | `2` | verde, no mapa da área |
| 3 | Tema dentro da área | `2.1` | âmbar |
| 4 | Decisão | `2.1.1` | rosa |
| 5 | Detalhe, valor ou parâmetro | `2.1.1.1` | violeta |

A legenda em inglês no rodapé das imagens (Backend, Cloud, Security, Database) é fixa do gerador e só indica a cor; vale a tabela acima.

![Mapa mental do Taba: níveis 1 e 2](diagramas/img/mapa-mental.svg)

*Fonte: [mapa-mental.json](diagramas/fonte/mapa-mental.json).*

Taba é uma mensageria distribuída de **nós idênticos, sem mestre**, inspirada no Amazon SQS e SNS.

| Nº | Área | Definição |
|---|---|---|
| 1 | [Escrita e quórum](#1-escrita-e-quórum) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| 2 | [Entrega](#2-entrega) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12 |
| 3 | [Invisibilidade](#3-invisibilidade) | [14-invisibilidade.md](14-invisibilidade.md), seção 13 |
| 4 | [Tópicos](#4-tópicos) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14 |
| 5 | [Cluster e membros](#5-cluster-e-membros) | [04-cluster.md](04-cluster.md), seção 6 |
| 6 | [Sincronização](#6-sincronização) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3; [04-cluster.md](04-cluster.md), seção 6.6 |
| 7 | [Armazenamento](#7-armazenamento) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| 8 | [API e observabilidade](#8-api-e-observabilidade) | [06-api.md](06-api.md); [11-observabilidade.md](11-observabilidade.md); [12-limites-api.md](12-limites-api.md) |

### 1. Escrita e quórum

O coordenador calcula o bloco pelo hash do ID (GUID v7) e faz registro + commit, com um quórum único.

![Mapa mental: Escrita e quórum](diagramas/img/mapa-mental-escrita.svg)

*Fonte: [mapa-mental-escrita.json](diagramas/fonte/mapa-mental-escrita.json).*

| Nº | Item | Definição |
|---|---|---|
| `1` | **Escrita e quórum** (área) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.1` | Roteamento (tema) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.1 |
| `1.1.1` | Bloco pelo hash do ID (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seções 7.1 e 7.2 |
| `1.1.1.1` | GUID v7 gerado no coordenador (detalhe) | [02-arquitetura.md](02-arquitetura.md), seção 3.1; [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.1.2` | Anel com nós virtuais (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.1 |
| `1.1.2.1` | ring.virtual-nodes (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `1.2` | Duas fases (tema) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.2.1` | Registro + commit (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.2.1.1` | pendente expira: write.pending.ttl (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `1.2.2` | Rollback só antes do quórum (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.2.2.1` | depois, insiste até commit.timeout (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2; [10-configuracao.md](10-configuracao.md) |
| `1.3` | Quórum (tema) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2; [10-configuracao.md](10-configuracao.md) |
| `1.3.1` | Valor único (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2; [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) |
| `1.3.1.1` | escrita, entrega e ack (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.2 e 12.4 |
| `1.3.2` | Maioria do fator (decisão) | [10-configuracao.md](10-configuracao.md) |
| `1.3.2.1` | 2 de 3, sempre com o primário (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |
| `1.3.3` | Sem quórum (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2; [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) |
| `1.3.3.1` | 503 com Retry-After (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2 |

### 2. Entrega

Só o primário entrega; a lista de prontos fica em memória e o coordenador completa com outros nós.

![Mapa mental: Entrega](diagramas/img/mapa-mental-entrega.svg)

*Fonte: [mapa-mental-entrega.json](diagramas/fonte/mapa-mental-entrega.json).*

| Nº | Item | Definição |
|---|---|---|
| `2` | **Entrega** (área) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12 |
| `2.1` | Quem entrega (tema) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.7 |
| `2.1.1` | Só o primário (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.7 |
| `2.1.1.1` | demais nós: cópias de segurança (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.7 |
| `2.1.2` | Coordenador do GET (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.2 e 12.8 |
| `2.1.2.1` | completa com outros nós (take) (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8; [06-api.md](06-api.md), seção 8.2 |
| `2.2` | Lista de prontos (tema) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.6 |
| `2.2.1` | Em memória, por tópico (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.6 |
| `2.2.1.1` | 1000 IDs (ready-list.size) (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `2.2.2` | Efeito sanfona (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.6 e 12.7 |
| `2.2.2.1` | pontas antiga e nova, intercaladas (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.6 |
| `2.2.3` | Thread com semáforo (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.6 |
| `2.2.3.1` | repõe quando abre espaço (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.6 |
| `2.3` | Consumo (tema) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.2 e 12.8 |
| `2.3.1` | Long polling (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |
| `2.3.1.1` | waitTime de 0 a 20 s (detalhe) | [12-limites-api.md](12-limites-api.md) |
| `2.3.2` | maxMessages (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.2 |
| `2.3.2.1` | padrão 10 (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `2.3.3` | Avisos de mensagem nova (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |
| `2.3.3.1` | janela de 100 ms, um nó por vez (detalhe) | [10-configuracao.md](10-configuracao.md); [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |

### 3. Invisibilidade

A mensagem entregue fica invisível por um prazo; o `ReceiptToken` evita confirmar uma entrega obsoleta.

![Mapa mental: Invisibilidade](diagramas/img/mapa-mental-invisibilidade.svg)

*Fonte: [mapa-mental-invisibilidade.json](diagramas/fonte/mapa-mental-invisibilidade.json).*

| Nº | Item | Definição |
|---|---|---|
| `3` | **Invisibilidade** (área) | [14-invisibilidade.md](14-invisibilidade.md), seção 13 |
| `3.1` | Prazo (tema) | [14-invisibilidade.md](14-invisibilidade.md), seções 13.2 e 13.3 |
| `3.1.1` | invisibleUntil absoluto (decisão) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.2 |
| `3.1.1.1` | calculado pelo dono (detalhe) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.3 |
| `3.1.2` | PATCH (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.3 |
| `3.1.2.1` | novo tempo a partir de agora; 0 libera (detalhe) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.3; [06-api.md](06-api.md), seção 8.1 |
| `3.2` | ReceiptToken (tema) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.2 |
| `3.2.1` | Gerado a cada entrega (decisão) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.2 |
| `3.2.1.1` | guardado com a invisibilidade (detalhe) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.2 |
| `3.2.2` | Exigido no ack e no PATCH (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.4 e 12.3 |
| `3.2.2.1` | token velho recebe 409 (detalhe) | [06-api.md](06-api.md), seção 8.1 |
| `3.3` | Expiração (tema) | [14-invisibilidade.md](14-invisibilidade.md), seções 13.4, 13.5 e 13.6 |
| `3.3.1` | Lista de invisíveis (decisão) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.4 |
| `3.3.1.1` | em memória (invisible-list.size) (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `3.3.2` | Excedente no SQLite (decisão) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.5 |
| `3.3.2.1` | tabela expiracao, por prazo (detalhe) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `3.3.3` | Vencimento não se propaga (decisão) | [14-invisibilidade.md](14-invisibilidade.md), seções 13.3 e 13.6 |
| `3.3.3.1` | dono recoloca na lista de prontos (detalhe) | [14-invisibilidade.md](14-invisibilidade.md), seção 13.6 |

### 4. Tópicos

Fila, Tópico Distribuidor (copia por prefixo) e fila volátil com nó gestor.

![Mapa mental: Tópicos](diagramas/img/mapa-mental-topicos.svg)

*Fonte: [mapa-mental-topicos.json](diagramas/fonte/mapa-mental-topicos.json).*

| Nº | Item | Definição |
|---|---|---|
| `4` | **Tópicos** (área) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14 |
| `4.1` | Tipos (tema) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.1 |
| `4.1.1` | Fila (QT) (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.1 |
| `4.1.1.1` | entrega única por mensagem (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12 |
| `4.1.2` | Distribuidor (DT) (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2 |
| `4.1.2.1` | copia por prefixo, até 20 (detalhe) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2; [12-limites-api.md](12-limites-api.md) |
| `4.2` | Fila volátil (QTV) (tema) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seções 14.3, 14.4 e 14.5 |
| `4.2.1` | Exclusão por ociosidade (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seções 14.3 e 14.5 |
| `4.2.1.1` | idleTimeout padrão 600 s (detalhe) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.3; [10-configuracao.md](10-configuracao.md) |
| `4.2.2` | autoCreate no GET (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.4 |
| `4.2.2.1` | só cria fila; teto configurável (detalhe) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.4; [10-configuracao.md](10-configuracao.md) |
| `4.2.3` | Nó gestor (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seções 14.5 e 14.6 |
| `4.2.3.1` | hash do id do tópico no anel (detalhe) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5 |
| `4.3` | Limites (tema) | [12-limites-api.md](12-limites-api.md) |
| `4.3.1` | Destinos por publicação (decisão) | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2; [10-configuracao.md](10-configuracao.md) |
| `4.3.1.1` | sem limite por padrão; 422 (detalhe) | [12-limites-api.md](12-limites-api.md) |
| `4.3.2` | Nome do tópico (decisão) | [06-api.md](06-api.md), seção 8.1; [12-limites-api.md](12-limites-api.md) |
| `4.3.2.1` | 1 a 80, A-Za-z0-9_- (detalhe) | [12-limites-api.md](12-limites-api.md) |

### 5. Cluster e membros

Entrada pelo balanceador, canal permanente entre os nós, votação de exclusão, freezing e uma mudança por vez.

![Mapa mental: Cluster e membros](diagramas/img/mapa-mental-cluster.svg)

*Fonte: [mapa-mental-cluster.json](diagramas/fonte/mapa-mental-cluster.json).*

| Nº | Item | Definição |
|---|---|---|
| `5` | **Cluster e membros** (área) | [04-cluster.md](04-cluster.md), seção 6 |
| `5.1` | Entrada (tema) | [04-cluster.md](04-cluster.md), seções 6.2, 6.8 e 6.9 |
| `5.1.1` | Endereço do balanceador (decisão) | [04-cluster.md](04-cluster.md), seção 6.2 |
| `5.1.1.1` | cluster.entry-address, obrigatório (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `5.1.2` | Uma mudança por vez (decisão) | [04-cluster.md](04-cluster.md), seção 6.8 |
| `5.1.2.1` | até o nó ficar pronto (detalhe) | [04-cluster.md](04-cluster.md), seção 6.8 |
| `5.1.3` | Repasse de chamadas (decisão) | [04-cluster.md](04-cluster.md), seção 6.9 |
| `5.1.3.1` | enquanto o nó sincroniza (detalhe) | [04-cluster.md](04-cluster.md), seção 6.9 |
| `5.2` | Canal (tema) | [04-cluster.md](04-cluster.md), seção 6.1; [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.8 e 12.9 |
| `5.2.1` | WebSocket permanente (decisão) | [04-cluster.md](04-cluster.md), seção 6.1; [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |
| `5.2.1.1` | malha completa entre os nós (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8 |
| `5.2.2` | Sinal de vida próprio (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8; [04-cluster.md](04-cluster.md), seção 6.3 |
| `5.2.2.1` | data, monitoramento, prontas (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `5.2.3` | Sinais com identificador (decisão) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9 |
| `5.2.3.1` | confirmado após processar (detalhe) | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9 |
| `5.3` | Falhas (tema) | [04-cluster.md](04-cluster.md), seções 6.3, 6.5 e 6.7 |
| `5.3.1` | Votação de exclusão (decisão) | [04-cluster.md](04-cluster.md), seção 6.3 |
| `5.3.1.1` | 51% dos saudáveis, mínimo 2 (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `5.3.2` | Freezing (decisão) | [04-cluster.md](04-cluster.md), seção 6.7 |
| `5.3.2.1` | ingresso rejeitado com Retry-After (detalhe) | [04-cluster.md](04-cluster.md), seção 6.7 |
| `5.3.3` | Retry do bootstrap (decisão) | [04-cluster.md](04-cluster.md), seção 6.5 |
| `5.3.3.1` | retry.interval configurável (detalhe) | [10-configuracao.md](10-configuracao.md) |

### 6. Sincronização

Carga direta no armazenamento, banco de eventos por cima, e recuperação de mensagens perdidas.

![Mapa mental: Sincronização](diagramas/img/mapa-mental-sync.svg)

*Fonte: [mapa-mental-sync.json](diagramas/fonte/mapa-mental-sync.json).*

| Nº | Item | Definição |
|---|---|---|
| `6` | **Sincronização** (área) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3; [04-cluster.md](04-cluster.md), seção 6.6 |
| `6.1` | Carga (tema) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.1.1` | Tópicos primeiro (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.3 |
| `6.1.1.1` | origem: nó de menor carga (detalhe) | [02-arquitetura.md](02-arquitetura.md), seção 3.3 |
| `6.1.2` | Blocos por faixas (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.1.2.1` | streams NDJSON retomáveis (detalhe) | [06-api.md](06-api.md), seção 8.2.1 |
| `6.2` | Eventos (tema) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.2.1` | Banco de eventos SQLite (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.2.1.1` | lê a mais antiga e apaga (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.2.2` | Troca para tempo real (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.2.2.1` | quando a tabela esvazia (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.3` | Perdas (tema) | [04-cluster.md](04-cluster.md), seção 6.6; [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `6.3.1` | Tabela perdas (decisão) | [04-cluster.md](04-cluster.md), seção 6.6; [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `6.3.1.1` | tópico e ID, com validade (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `6.3.2` | Nó que volta recupera (decisão) | [04-cluster.md](04-cluster.md), seção 6.6 |
| `6.3.2.1` | quarentena até terminar (detalhe) | [04-cluster.md](04-cluster.md), seção 6.6 |
| `6.3.3` | Recover (decisão) | [06-api.md](06-api.md), seção 8.2 |
| `6.3.3.1` | grava direto, idempotente por ID (detalhe) | [06-api.md](06-api.md), seção 8.2.1; [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) |

### 7. Armazenamento

Memória primeiro; em disco, um banco SQLite por tópico e o corpo em arquivo JSON.

![Mapa mental: Armazenamento](diagramas/img/mapa-mental-disco.svg)

*Fonte: [mapa-mental-disco.json](diagramas/fonte/mapa-mental-disco.json).*

| Nº | Item | Definição |
|---|---|---|
| `7` | **Armazenamento** (área) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.1` | Memória (tema) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.1.1` | Divisão por taxa de leitura (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.1.1.1` | janela de 5 minutos (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `7.1.2` | Reserva por tópico (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.1.2.1` | 1000 mensagens (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `7.2` | Disco (tema) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.2.1` | SQLite por nó e tópico (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.2.1.1` | escritor único por tópico (detalhe) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.2.2` | Corpo em arquivo JSON (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.2.2.1` | tópico/id.json (detalhe) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.2.3` | Raiz configurável (decisão) | [10-configuracao.md](10-configuracao.md) |
| `7.2.3.1` | /var/lib/taba por padrão (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `7.3` | Integridade (tema) | [02-arquitetura.md](02-arquitetura.md), seção 3.2; [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `7.3.1` | Banco corrompido (decisão) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| `7.3.1.1` | nó em falha; cluster vota (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3; [04-cluster.md](04-cluster.md), seção 6.3 |
| `7.3.2` | Bloco que o nó perdeu (decisão) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |
| `7.3.2.1` | só apaga com cluster íntegro (detalhe) | [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3 |

### 8. API e observabilidade

API HTTP com paginação por chave e rate limit por IP; métricas em memória e um único alvo para o Prometheus.

![Mapa mental: API e observabilidade](diagramas/img/mapa-mental-obs.svg)

*Fonte: [mapa-mental-obs.json](diagramas/fonte/mapa-mental-obs.json).*

| Nº | Item | Definição |
|---|---|---|
| `8` | **API e observabilidade** (área) | [06-api.md](06-api.md); [11-observabilidade.md](11-observabilidade.md); [12-limites-api.md](12-limites-api.md) |
| `8.1` | API de cliente (tema) | [06-api.md](06-api.md), seção 8.1; [12-limites-api.md](12-limites-api.md) |
| `8.1.1` | Paginação por chave (decisão) | [06-api.md](06-api.md), seção 8.1.2 |
| `8.1.1.1` | limit, cursor e sort (detalhe) | [06-api.md](06-api.md), seção 8.1.2 |
| `8.1.2` | Mensagem máxima (decisão) | [12-limites-api.md](12-limites-api.md); [10-configuracao.md](10-configuracao.md) |
| `8.1.2.1` | 5 MB; limite rígido (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `8.1.3` | Rate limit por IP (decisão) | [12-limites-api.md](12-limites-api.md) |
| `8.1.3.1` | cabeçalho do balanceador (detalhe) | [10-configuracao.md](10-configuracao.md); [12-limites-api.md](12-limites-api.md) |
| `8.2` | Métricas (tema) | [11-observabilidade.md](11-observabilidade.md) |
| `8.2.1` | Alvo único do Prometheus (decisão) | [11-observabilidade.md](11-observabilidade.md), seção "Endpoint Prometheus" |
| `8.2.1.1` | uma série por nó (detalhe) | [11-observabilidade.md](11-observabilidade.md), seção "Endpoint Prometheus" |
| `8.2.2` | Cópia em memória (decisão) | [11-observabilidade.md](11-observabilidade.md), seção "Endpoint Prometheus" |
| `8.2.2.1` | thread de atualização (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `8.2.3` | Streams de sincronização (decisão) | [11-observabilidade.md](11-observabilidade.md), seção "Anel, blocos e sincronização" |
| `8.2.3.1` | msg.sync.streams (detalhe) | [11-observabilidade.md](11-observabilidade.md) |
| `8.3` | Operação (tema) | [11-observabilidade.md](11-observabilidade.md); [12-limites-api.md](12-limites-api.md) |
| `8.3.1` | Rotas isentas de limite (decisão) | [12-limites-api.md](12-limites-api.md) |
| `8.3.1.1` | health, metrics, stats/node (detalhe) | [10-configuracao.md](10-configuracao.md) |
| `8.3.2` | IPs autorizados (decisão) | [11-observabilidade.md](11-observabilidade.md), seção "Acesso às rotas de operação" |
| `8.3.2.1` | proxies confiáveis; rede segura (detalhe) | [10-configuracao.md](10-configuracao.md); [12-limites-api.md](12-limites-api.md) |

### Para ir adiante

- O que já foi decidido, por tema: [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md).
- O que ainda está em aberto: [08b-pendencias.md](08b-pendencias.md).
- A ordem do trabalho: [08c-proximos-passos.md](08c-proximos-passos.md).
- Os parâmetros e seus padrões: [10-configuracao.md](10-configuracao.md).
