# Visão geral e objetivos

[← Índice](README.md)

## 1. Descrição
 
Sistema de mensageria **altamente escalável, rápido e simples**, inspirado no Amazon SQS e SNS.
 
A aplicação expõe uma API enxuta em que o cliente envia mensagens para um **tópico**, que pode ser de dois tipos:
 
- **Tópico Fila (QT, *Queue Topic*):** guarda as mensagens, e cada uma é consumida por apenas um consumidor. Pode ser **volátil**: some sozinho se ficar sem consumidor por um tempo configurável. Pode ser **FIFO**: entrega cada grupo de mensagens (`groupId`) em ordem, uma por vez, e deduplica as publicações ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16).
- **Tópico Distribuidor (DT, *Distributor Topic*):** não guarda mensagens; copia cada publicação para todas as filas cujo nome começa com um dos prefixos que ele distribui.

O QT é o análogo do Amazon SQS e o DT é o análogo do Amazon SNS (análogos, não equivalentes). Nomenclatura e siglas em [15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.
![C4 nível 0: contexto do Sistema de Mensagens](diagramas/img/c4-nivel0-contexto.svg)

*Contexto do sistema (C4 nível 0). Fonte: [c4-nivel0-contexto.json](diagramas/fonte/c4-nivel0-contexto.json).*

## 2. Objetivos
 
| # | Objetivo | Descrição |
|---|----------|-----------|
| 1 | API simples | Uso dos verbos HTTP para enviar e receber mensagens, sem complexidade desnecessária. |
| 2 | Rapidez | Escrita em duas fases (registro → commit) nos nós responsáveis, com as mensagens mantidas em memória; disco apenas por overflow. |
| 3 | Escalabilidade simples e automática | Novos nós entram no cluster sem configuração manual complexa. |
| 4 | Configuração simples | Definição de cache em memória e de confirmação de persistência por número de nós que confirmaram a mensagem. |
| 5 | Comunicação automática entre nós | Descoberta e troca de informações entre nós sem intervenção manual. |
| 6 | Suporte nativo a Kubernetes | Funcionamento em clusters k8s como cidadão de primeira classe. |
| 7 | Docker simples e leve | Tudo em uma única imagem. |

