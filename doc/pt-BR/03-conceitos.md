---
título: Conceitos
resumo: O modelo mental do Taba em uma página — nó, cluster, tópico, mensagem, bloco, anel, quórum, invisibilidade, ack — cada termo definido uma vez, com o link do detalhe.
tipo: explicação
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/core, crates/taba-node/src/engine, crates/taba-node/src/cluster]
---

# Conceitos

Este é o vocabulário mínimo para ler o resto da documentação. Cada termo é definido aqui **uma vez**; os outros arquivos só o usam. A lista completa e em ordem alfabética está em [28-glossario.md](28-glossario.md).

## Em uma frase

Aplicações **publicam mensagens** em **tópicos**; o Taba as guarda em vários **nós** de um **cluster** (espalhando-as por um **anel de hash**), entrega cada uma a **um consumidor** e a apaga quando o consumidor a **confirma**; se ele não confirmar a tempo, a mensagem volta.

## Quem é quem

| Termo | O que é |
|---|---|
| **Nó** | Uma instância do programa `taba`. Todos são iguais; não há mestre. Cada nó tem um **ID** (GUID v7) gerado a cada partida. |
| **Cluster** | O conjunto de nós que se conhecem. Mínimo em produção: **3**. Com **1** nó, só no **perfil de um nó**. |
| **Tabela de membros** | A lista de nós do cluster (IDs e endereços), com uma **versão** que sobe a cada mudança. Todo nó tem uma cópia. |
| **Cliente** | Qualquer aplicação que fala com a API de cliente (produtor ou consumidor). |
| **Operador** | Quem roda o cluster. Fala com a API de operação e com a linha de comando. |

## O que se guarda

| Termo | O que é |
|---|---|
| **Tópico** | Canal nomeado. Tem **tipo**: **fila** (`queue`) ou **distribuidor** (`distributor`). Tem um `id` (GUID v7) que identifica a "encarnação": recriar o mesmo nome gera outro `id`. |
| **Fila** | Tópico que guarda mensagens e as entrega, uma para cada consumidor. Pode ser **volátil** (some sozinha se ficar sem consumidor) e/ou **FIFO** (ordem por grupo). |
| **Distribuidor** | Tópico que **não guarda** mensagens: copia cada publicação para as filas cujo nome começa com um dos seus **prefixos**. |
| **Mensagem** | Um corpo (bytes, normalmente JSON) mais metadados: `id` (GUID v7, ordenável no tempo), `contentType`, `publishedAt` e, quando entregue, `deliveryCount`, `invisibleUntil` e `receiptToken`. |
| **Dead-letter** | Fila `{fila}-dead-letter`, criada com cada fila, para onde vai a mensagem que atinge o limite de entregas sem ack. |
| **Token** | Credencial da API de cliente, com permissões e prefixos de tópico. O **token raiz** tem acesso total. |

## Como a mensagem é espalhada

| Termo | O que é |
|---|---|
| **Anel de hash** | Um círculo de posições de 64 bits. Cada nó ocupa várias posições (**nós virtuais**, padrão 128). A posição de uma mensagem vem do hash do seu `id`. |
| **Bloco** | O arco do anel que termina numa posição de nó virtual. Toda mensagem cuja posição cai no arco pertence ao bloco. |
| **Primário / secundário / terciário** | Os nós responsáveis por um bloco, em ordem ao longo do anel, sempre **nós físicos diferentes**. O **primário** é o **dono da entrega**: só ele coloca a mensagem na lista de prontos. |
| **Fator de replicação** | Quantos nós guardam cada mensagem (`replication.factor`, padrão **3**). |
| **Quórum de escrita** | Quantas confirmações são necessárias para dar a publicação por feita (`write.quorum`, padrão maioria: 2 de 3). O primário sempre conta. |
| **Registro + commit** | O protocolo de duas fases da escrita: o coordenador manda a mensagem a todos os responsáveis (**registro**, guardada como pendente) e, fechado o quórum, manda **commit**. |
| **Coordenador** | O nó que recebeu a chamada do cliente. Qualquer nó pode ser coordenador. |

## Como a mensagem é entregue

| Termo | O que é |
|---|---|
| **Long polling** | O `GET` que espera (até 20 s) por mensagens em vez de responder vazio. |
| **Lista de prontos** | Por tópico, no dono: os IDs das próximas mensagens a entregar. |
| **Invisibilidade** | Depois de entregue, a mensagem fica **invisível** até `invisibleUntil` (padrão 120 s depois da entrega). Se não houver ack até lá, volta à lista de prontos com `deliveryCount` +1. |
| **Ack** | `DELETE` da mensagem com o `receiptToken` recebido: confirma o consumo e apaga a mensagem. |
| **Grupo (`groupId`)** | Rótulo da mensagem. Na **FIFO** define a ordem (uma mensagem por grupo em voo). Nas outras filas serve para a **entrega justa**. |

## Como o cluster se mantém

| Termo | O que é |
|---|---|
| **Canal permanente** | Uma conexão WebSocket entre cada par de nós, que carrega o **sinal de vida** e avisos de entrega. Não carrega mensagens. |
| **Votação de exclusão** | Quando o canal com um nó cai e não volta, os nós saudáveis votam (51%) para retirá-lo. |
| **Freezing** | Período em que um nó excluído por falha não pode voltar, para evitar instabilidade. |
| **Sincronização** | O que um nó novo faz ao entrar: puxa tópicos e blocos dos nós que já os têm, guardando num **banco de eventos** o que chega no meio. Só então fica **pronto**. |
| **Drain** | Sair do cluster sem perder o que está só em memória: tudo vai para o disco e o nó se desregistra. |
| **Perfil de um nó** | `replication.factor=1`: um cluster de um nó, que parte sozinho e é o primário de tudo; o caminho é o de produção, com quórum 1. |

## As três portas

| Porta | Padrão | Quem usa | O que serve |
|---|---|---|---|
| **Cliente** (`api.port`) | 8080 | Aplicações | Tópicos, mensagens, tokens |
| **Cluster** (`cluster.port`) | 8081 | Outros nós | Rotas `/cluster/*` e o canal permanente; TLS mútuo |
| **Operação** (`operations.port`) | 9090 | Coletores e orquestrador dentro da infraestrutura (Prometheus, Telegraf, sondas); sem token | `/health/*`, `/metrics*`, drain |

Detalhes em [15-api-visao-geral.md](15-api-visao-geral.md).

## Um mapa de dependência dos conceitos

![Como cluster, nó, anel, bloco, tópico e mensagem se ligam](../diagrams/img/pt-BR/entities.svg)

## Onde seguir

| Quero entender… | Leia |
|---|---|
| Como o nó é montado por dentro | [04-arquitetura.md](04-arquitetura.md) |
| O anel, os blocos e os papéis | [06-anel-e-replicacao.md](06-anel-e-replicacao.md) |
| Como o cluster entra, falha e volta | [07-cluster.md](07-cluster.md) |
| Os tipos de tópico | [09-topicos.md](09-topicos.md) |
| Publicar e entregar | [10-publicacao.md](10-publicacao.md), [12-entrega.md](12-entrega.md) |

## Fontes e provas

- `crates/taba-node/src/core/ring` (posição e blocos), `core/topic`, `core/id` (GUID v7), `cluster/members`, `cluster/state`, `engine/messages`.
- Casos de uso: UC-ANE (anel), UC-ESC (escrita), UC-ENT (entrega), UC-INV (invisibilidade); veja [26a](26a-casos-de-uso-implementados.md).
- Histórico: `09-glossario.md`, `16-mapa-mental.md`.
