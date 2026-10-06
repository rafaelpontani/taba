# Visão geral e objetivos

[← Índice](README.md)

## 1. Descrição
 
Sistema de mensageria **altamente escalável, rápido e simples**, inspirado no Amazon SQS e SNS.
 
A aplicação expõe uma API enxuta em que o cliente envia mensagens para um **tópico**, que pode ser de dois tipos:
 
- **Entrega única (fila):** cada mensagem é consumida por apenas um consumidor (modelo SQS).
- **Broadcast (pub/sub):** cada mensagem é entregue a todos os assinantes do tópico (modelo SNS).
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

