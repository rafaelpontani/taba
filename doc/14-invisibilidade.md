# Invisibilidade

[← Índice](README.md)

## 13. Invisibilidade das mensagens

Uma mensagem **entregue** fica **invisível** por um prazo (`invisibleUntil`): nesse tempo nenhum outro consumidor a recebe. Se o consumidor a confirma (ack) dentro do prazo, ela é removida; se não confirma, o prazo vence e a mensagem **volta a ser entregável**. Esta seção reúne tudo sobre o tema: o que é guardado e onde, como a informação se propaga entre os nós, como o dono da mensagem controla a volta à visibilidade e o que acontece quando a memória não comporta todas as mensagens invisíveis. A entrega em si (o `GET`, o long polling e o canal entre nós) continua em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).

> Nomes de parâmetros, rotas e arquivos são **propostas**, como em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).

### 13.1 Visão geral

![Sequência do ciclo da invisibilidade](diagramas/img/invisibilidade.svg)

*Fonte: [invisibilidade.json](diagramas/fonte/invisibilidade.json).*

Uma mensagem está sempre em um destes estados, do ponto de vista do **dono da entrega** (o primário do bloco, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.7):

| Estado | Como se reconhece | Onde o dono a controla |
|---|---|---|
| **Pronta** | `invisibleUntil` ausente ou no passado | Lista de prontos (seção 12.6), até o limite `delivery.ready-list.size`; o resto é reposto por uma thread |
| **Invisível** | `invisibleUntil` no futuro | Lista de invisíveis em memória (seção 13.4) ou, se não coube, tabela de expiração no banco local (seção 13.5) |
| **Removida** | Ack confirmado (seção 12.4) | Em lugar nenhum: mensagem e metadados apagados |

Quem guarda o quê:

| Informação | Onde vive | Quem guarda |
|---|---|---|
| `invisibleUntil` e `deliveryCount` (**fonte de verdade**) | Metadados da mensagem, em memória ou no banco SQLite local do tópico (seção 13.2) | **Todos** os nós responsáveis pelo bloco |
| Lista de prontos | Memória | Só o dono |
| Lista de invisíveis (ID e prazo, ordenada por prazo) | Memória | Só o dono |
| Tabela de expiração (excedente da lista de invisíveis) | Banco local, em disco | Só o dono |

Só os **metadados** são replicados. As três estruturas do dono são um **índice derivado** dos metadados: servem para achar rápido o que expira e o que está pronto, e podem ser reconstruídas a qualquer momento (seção 13.7). Por isso a perda de uma delas nunca perde uma mensagem.

### 13.2 Metadados

Os metadados são `deliveryCount` (incremental), `invisibleUntil` (data e hora absoluta) e `receiptToken` (token opaco gerado pelo dono a cada entrega, por exemplo um GUID). O `receiptToken` é guardado e replicado **exatamente como a invisibilidade**: no mesmo registro + commit da entrega, nos mesmos metadados, em memória ou no banco local do tópico.

**Uso do `receiptToken`** (como o `ReceiptHandle` do SQS): a entrega devolve o token ao cliente junto da mensagem, e o ack (`DELETE`) e a alteração da invisibilidade (`PUT .../visibility`) o enviam (`receiptToken` na query string, nas duas rotas). O nó compara com o token dos metadados e, se for diferente, responde `409`, sem alterar nada. Assim, um consumidor lento cujo prazo venceu e cuja mensagem foi reentregue a outro não consegue confirmá-la nem alterá-la: o token dele ficou obsoleto. Se o prazo venceu mas a mensagem **ainda não foi reentregue**, o token continua válido e o ack vale. A alteração mantém o mesmo token; só uma nova entrega gera outro.

- **Fila FIFO:** a mensagem ganha também `deduplicationId` e, depois do ack, `deleteAt` (exclusão lógica, sem corpo nem metadados de entrega); detalhes em [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7.
- **Mensagem em memória:** os metadados ficam em memória junto dela.
- **Mensagem em disco** (overflow, [02-arquitetura.md](02-arquitetura.md), seção 3.2): o corpo fica em `${topico}/${id}.json` e os metadados ficam no **banco SQLite local do tópico** naquele nó, na tabela `mensagens` (`id`, posição no anel, `deliveryCount`, `invisibleUntil`, `receiptToken`). Não há mais arquivo de metadados.
- A linha é atualizada no **commit** de cada entrega ou alteração e removida junto com a mensagem no ack. Alterar a invisibilidade de uma mensagem em disco atualiza só a linha, sem tocar no corpo.
- `invisibleUntil` é sempre **data absoluta**, calculada pelo **dono** (`agora + visibilityTimeout`, pelo relógio dele): as réplicas guardam o valor pronto e nunca recalculam a partir de duração.

### 13.3 Propagação

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): o registro e o commit da invisibilidade acontecem só no próprio nó (quórum de 1); nada é propagado.

A invisibilidade se propaga por **registro + commit decidido por quórum**, o mesmo protocolo da escrita ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2). Há duas operações que a alteram; a terceira ocorrência, o vencimento do prazo, **não** se propaga.

| Operação | Quando | O que se propaga | `deliveryCount` | Lista de prontos | Lista de invisíveis do dono |
|---|---|---|---|---|---|
| **Entrega** (`GET` ou `take`) | Uma mensagem pronta é entregue | Novo `invisibleUntil` = `agora + visibilityTimeout` e novo `receiptToken` | +1 | Remove o ID | Insere ID e prazo |
| **Alteração** (`PUT .../visibility`) | O consumidor pede mais (ou menos) tempo | Novo `invisibleUntil` = `agora + visibilityTimeout` (não é acréscimo); com `0`, a mensagem é liberada na hora | Não muda | Não muda (com `0`, o dono insere o ID, como num vencimento) | Substitui o prazo da entrada (com `0`, remove) |
| **Ack** (`DELETE`) | O consumidor confirma | Remoção da mensagem e dos metadados | — | — | Remove a entrada |
| **Vencimento do prazo** | `invisibleUntil` chega ao passado sem ack | **Nada** | Não muda | Insere o ID | Remove a entrada |

**Passo a passo (entrega e alteração).** Detalhe do `GET` em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.2.

1. O **dono** é sempre o coordenador: no `GET` local ele mesmo; num `take` (seção 12.8), o nó que atende o cliente pede ao dono, que coordena. Assim o relógio que fixa `invisibleUntil` é o do dono.
2. O dono calcula os nós responsáveis pelo ID (primário + réplicas) e envia a todos o **registro** do novo `invisibleUntil` (e de `deliveryCount + 1`, na entrega). Cada nó grava como **pendente** e confirma.
3. Não há disputa em condições normais: só o primário entrega e só IDs prontos (seção 12.7 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Por segurança, o registro continua **exclusivo**: um nó rejeita o pedido se já há outro pendente para a mensagem ou, numa entrega, se ela já está invisível, e o `GET` tenta outro ID. Na alteração e no ack, o registro também é rejeitado se o `receiptToken` não confere.
4. Com o **quórum** confirmando (`write.quorum`, sempre incluindo o primário), o dono decide o **commit**. Cada nó do quórum efetiva os metadados; no **dono**, o commit também atualiza as listas da tabela acima (remove da de prontos e insere na de invisíveis, ou substitui a entrada, na alteração).
5. Só então a chamada **responde ao cliente**. As réplicas fora do quórum recebem o commit em segundo plano, dentro de `write.commit.timeout`.
6. **Falha antes do quórum:** retry e, esgotado, rollback do pendente (`write.retry.*`, `write.pending.ttl`); as listas do dono **não** são tocadas, e a mensagem segue como estava. **Falha nas réplicas fora do quórum:** sem rollback, o dono insiste até o teto.

**Por que o vencimento não se propaga.** `invisibleUntil` já é um instante absoluto: "no passado" **significa** "visível", em qualquer nó, sem nenhuma escrita. Só o dono precisa *agir* no vencimento (recolocar a mensagem na lista de prontos), e ele o faz sozinho, olhando a própria lista de invisíveis (seção 13.6). Propagar o vencimento custaria uma rodada de registro + commit por mensagem só para gravar um fato que o relógio já expressa. O próximo evento sobre a mensagem, uma nova entrega, grava o novo prazo e o novo `deliveryCount` como qualquer outra.

**Relógio.** O prazo é decidido e conferido pelo relógio do dono; as réplicas não comparam com o próprio relógio, exceto para a checagem de exclusividade do passo 3 e depois de assumir um bloco (seção 13.7). Uma diferença de relógio entre nós, na troca de dono, encurta ou alonga o prazo pelo tamanho da diferença: efeito aceito no at-least-once, e uma razão a mais para manter os relógios sincronizados (NTP).

**Trade-off aceito** (descrito em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.2): se o dono cair entre o commit local e a replicação às réplicas fora do quórum, o novo dono herda o `invisibleUntil` de quem sobreviveu, e a mensagem pode voltar a ser entregável antes do prazo prometido.

### 13.4 Lista de invisíveis (memória)

O dono mantém, **por tópico**, uma **lista de invisíveis**: as mensagens de que é dono e que estão invisíveis, cada uma como par **(ID, `invisibleUntil`)**, **ordenada por prazo** (a primeira é sempre a próxima a vencer). Serve para responder, sem varrer nada, "qual é a próxima mensagem a voltar e quando".

- **Tamanho limitado:** no máximo `delivery.invisible-list.size` entradas por tópico (memória, como a lista de prontos, é finita).
- **Só o dono a tem.** Réplicas guardam apenas os metadados.
- **Operações**, todas dentro do commit do dono (seção 13.3): a entrega **insere**, a alteração **substitui** o prazo, o ack **remove** e o vencimento **remove** (seção 13.6).
- **Estrutura:** ordenada por prazo com busca por ID, para que a alteração e o ack achem a entrada sem percorrer a lista. Um par de estruturas (ordenada por prazo e por ID) ou uma árvore com índice servem; a escolha é de implementação.

Enquanto todas as mensagens invisíveis do dono cabem na lista, ela é a única estrutura necessária. O problema é quando não cabem.

### 13.5 Excedente da lista de invisíveis: a tabela de expiração

**Por que não basta anotar o tempo.** Se a mensagem que não coube na lista ficasse só com o `invisibleUntil` nos metadados, ninguém saberia que ela precisa voltar: o dono teria de **varrer os metadados de todas as mensagens** para descobrir quais venceram. Por isso o excedente precisa de uma estrutura própria, ordenada por tempo.

**Tabela de expiração.** No banco SQLite local do tópico (seção 3.2 de [02-arquitetura.md](02-arquitetura.md)), a tabela `expiracao` guarda os pares (`id`, `invisibleUntil`) que não couberam na lista em memória, com **índice por `invisibleUntil`**. Ler o que vence primeiro é uma consulta ordenada com limite, sem janelas de tempo nem arquivos de apoio.

**Regra de inclusão** (quando o dono insere um par (ID, prazo) e a lista está cheia):

1. Se o prazo novo é **anterior** ao último da lista, o par novo entra na lista e o **último** (o de prazo mais distante) é gravado na tabela.
2. Senão, o par novo é gravado direto na tabela.

Isso mantém uma **invariante**: **tudo o que está na lista vence antes de tudo o que está na tabela.** Logo a cabeça da lista é sempre o próximo a vencer de fato, e a thread nunca precisa consultar a tabela enquanto a lista não esvazia.

**Recarga.** Quando a lista cai abaixo de um limiar (`delivery.invisible.reload-threshold`) ou esvazia, a thread lê da tabela os pares de menor `invisibleUntil`, move-os para a lista (apagando as linhas lidas) e segue.

**Sem entradas velhas.** Como a tabela é transacional e indexada por `id`, o ack, a alteração e a nova entrega **apagam ou atualizam a linha** na hora, no mesmo commit que atualiza a lista em memória. Não há remoção preguiçosa, nem conferência de linhas velhas na leitura (a conferência de dono da seção 13.6 continua valendo).

**Durabilidade.** A gravação na tabela faz parte do **commit no dono** (seção 13.3, passo 4): se falhar, o commit não se completa e a entrega é desfeita antes da resposta ao cliente. Como a tabela é derivada dos metadados, perdê-la é coberto pela reconstrução (seção 13.7).

**Custo.** Com a lista com espaço, nenhuma escrita além dos metadados. Só o excedente vai à tabela: uma escrita indexada na entrega e uma consulta ordenada na recarga.

### 13.6 Vencimento e volta à lista de prontos

Uma thread do dono (pode ser a mesma da lista de prontos, seção 12.6) cuida do vencimento:

1. Olha a **cabeça** da lista de invisíveis do tópico e dorme até o prazo dela (com um teto curto, para reagir a alterações, entregas e chegadas de entradas mais cedo).
2. Quando `invisibleUntil ≤ agora`, **remove** a entrada e **confere** nos metadados: a mensagem ainda existe, o nó ainda é dono, e o prazo ainda é o da entrada. Se não, descarta e segue.
   **Limite de entregas:** se a conferência passa e `deliveryCount ≥ maxDeliveries` do tópico (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)), a mensagem **não** segue para o passo 3: o dono a manda ao dead-letter da fila e segue. Uma mensagem já vencida pelo `ttl` também é descartada aqui, sem voltar à lista.
   **Fila FIFO:** a mensagem que volta é a cabeça do seu grupo; o grupo deixa de estar ocupado e entra na lista de prontos pela posição do ID, sem a regra do meio da lista abaixo ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.6). Atingido `maxDeliveries`, a mensagem vai ao dead-letter e o grupo segue com a próxima.
3. Insere o ID na **lista de prontos**, pela posição do GUID v7: uma mensagem que volta é antiga, e por isso tende a ficar perto da ponta antiga, a primeira da sanfona. Se a lista de prontos está cheia, o ID do **meio** da lista (o último que a entrega intercalada pelas duas pontas alcançaria, seção 12.6) sai para dar lugar (ele continua visível e é reposto depois pela thread); a mensagem que venceu não se perde.
4. Se a lista de prontos **estava vazia**, envia o sinal "tem mensagens prontas" aos outros nós (seção 12.8); se há **ouvinte local** (long polling), a mensagem vai direto a ele, sem tráfego entre nós, como qualquer mensagem pronta no próprio dono.
5. Nada é escrito nos metadados nem enviado às réplicas (seção 13.3): `deliveryCount` só sobe na **próxima entrega**.

Se a lista de invisíveis esvazia e ainda há linhas na tabela de expiração, a thread faz a **recarga** (seção 13.5) antes de dormir.

### 13.7 Mudança de dono, reinício e reconstrução

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não há mudança de dono. Só vale o caso do reinício, sem blocos a assumir de outro nó.

As estruturas do dono são derivadas; três situações as refazem a partir dos metadados:

- **Novo dono de um bloco** (entrada, saída ou rebalanceamento do anel, [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3): ao terminar a sincronização do bloco, a thread percorre os metadados das mensagens em que agora é primário, uma vez por mudança de anel: as com `invisibleUntil` no futuro entram na lista de invisíveis (ou na tabela de expiração, pela regra da seção 13.5) e as demais viram candidatas da lista de prontos. Como as réplicas guardam o `invisibleUntil` absoluto, o novo dono só precisa do relógio para compará-lo com `agora`.
- **Fila FIFO:** o novo primário também refaz, por grupo, a cabeça e o estado livre ou ocupado (um grupo com mensagem invisível no futuro entra como ocupado), e a base do ID monotônico do grupo ([17-topicos-fifo.md](17-topicos-fifo.md), seções 16.4 e 16.6). As linhas com `deleteAt` não entram em nenhuma lista.
- **Quem deixa de ser dono:** não precisa limpar nada de imediato. As entradas que restarem na lista ou na tabela falham na conferência de dono (passo 2 da seção 13.6) e são descartadas quando lidas.
- **Reinício do processo:** o nó perde a memória e volta como **nó novo** ([04-cluster.md](04-cluster.md), seções 6.2 e 6.6): a tabela de expiração é descartada junto com o estado antigo, e as listas são reconstruídas quando ele assumir blocos, como no primeiro caso.

### 13.8 Parâmetros

| Parâmetro | Chave sugerida | Padrão |
|---|---|---|
| Invisibilidade padrão | `delivery.visibility-timeout.default` | 120 s |
| Invisibilidade máxima | `delivery.visibility-timeout.max` | A definir |
| Tamanho da lista de prontos | `delivery.ready-list.size` | 1000 |
| Tamanho da lista de invisíveis | `delivery.invisible-list.size` | A definir |
| Limiar de recarga da lista de invisíveis | `delivery.invisible.reload-threshold` | A definir |

Detalhes em [10-configuracao.md](10-configuracao.md).
