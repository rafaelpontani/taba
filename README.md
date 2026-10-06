# Taba — a aldeia das suas mensagens

**Taba** é uma mensageria **altamente escalável, rápida e simples**, inspirada no Amazon SQS e SNS.

**T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos
*(inglês: **T**opics, **A**synchronous, **B**alanced and **A**utonomous)*

Em tupi, *taba* é a **aldeia**: um lugar onde todos se conhecem, cada um tem seu papel e ninguém precisa de um chefe para dar as ordens. É assim que o Taba funciona. Cada nó é uma oca, o cluster é a aldeia, e as mensagens circulam entre elas sem que você precise cuidar disso. *(English: "taba" means "village" in Tupi, the language of Brazil's coast.)*

![C4 nível 0: contexto do Taba](doc/diagramas/img/c4-nivel0-contexto.svg)

## Por que Taba?

| Letra | Promessa | Como o Taba cumpre |
|---|---|---|
| **T**ópicos | Uma API enxuta, com fila (entrega única) e pub/sub (broadcast) no mesmo lugar. | Você envia para um tópico com verbos HTTP e escolhe o modelo: SQS ou SNS. |
| **A**ssíncronos | O produtor envia e segue em frente. | A escrita é confirmada em memória por um número configurável de nós; o disco só entra por overflow. |
| **B**alanceados | Nenhum nó carrega tudo sozinho. | Um hash circular reparte os tópicos em blocos e os replica em vários nós. |
| **A**utogeridos | Sem coordenador e sem operação manual. | Os nós se descobrem, se monitoram e votam a saída de quem falha. Quando o nó volta, é reintegrado. |

![Anel de hash: blocos e papéis](doc/diagramas/img/anel-hash.svg)

*O anel de hash reparte o trabalho entre os nós.*

## O que você ganha

- **Simples de usar:** poucas rotas HTTP.
- **Rápido:** as mensagens ficam em memória e o disco é só para o excedente.
- **Cresce sozinho:** um nó novo entra no cluster e assume sua parte, sem configuração complexa.
- **Resistente a falhas:** cada tópico é espalhado e redundante entre os nós.
- **Nativo em Kubernetes:** vem em uma única imagem Docker, simples e leve.

![Tópico espalhado e redundante](doc/diagramas/img/topico-espalhado.svg)

*Um tópico espalhado pelo cluster e replicado: se um nó cai, a aldeia continua de pé.*

## Documentação

O projeto está em fase de documentação e ainda não tem código de aplicação. Quer entender como tudo funciona? Comece pela [visão geral](doc/01-visao-geral.md) e siga o [índice da documentação](doc/README.md).
