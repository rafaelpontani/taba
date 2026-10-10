# Entrega de mensagens

[← Índice](README.md)

## 12. Controle de entrega (invisibilidade)

A entrega segue o modelo de **invisibilidade temporária**, como o *visibility timeout* do SQS: ao entregar uma mensagem, o sistema a torna **invisível** por um tempo. Se o consumidor a confirma (ack) nesse prazo, ela é removida; se não confirma, o prazo expira e a mensagem volta a ficar disponível para entrega. A semântica resultante é **at-least-once**: uma mensagem pode ser entregue mais de uma vez, mas não se perde por falta de confirmação.

> A invisibilidade em si (metadados, propagação, lista de invisíveis, excedente em disco e vencimento) está reunida em [14-invisibilidade.md](14-invisibilidade.md). Os nomes de parâmetros, rotas e arquivos abaixo são **propostas**; ainda não há contrato definido (mesma ressalva de [10-configuracao.md](10-configuracao.md)). Pendências deste tema: [08b-pendencias.md](08b-pendencias.md).

### 12.1 Peças do controle

| Peça | Papel |
|---|---|
| **Invisibilidade** | Prazo (`invisibleUntil`) até o qual a mensagem não é entregue a outro consumidor ([14-invisibilidade.md](14-invisibilidade.md)). Propagada ao **quórum** de nós responsáveis (`write.quorum`, sempre incluindo o primário) antes de ela ser entregue; réplicas fora do quórum recebem em segundo plano (`write.commit.timeout`). |
| **Contador de entregas** | `deliveryCount`, incrementado a cada entrega. Base para reentrega e para o limite de entregas, que manda a mensagem ao dead-letter (seção 12.10). |
| **Grupo** | `groupId` opcional da mensagem; chave da entrega justa entre remetentes (seção 12.11). Na fila FIFO é obrigatório e é a chave da ordem ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16). |
| **Metadados da mensagem** | `deliveryCount`, `invisibleUntil` e `receiptToken` (devolvido ao cliente na entrega e exigido no ack e na alteração da invisibilidade), guardados junto da mensagem em memória ou em disco (seção 12.5). |
| **Dono da entrega** | O nó primário do bloco da mensagem: só ele a coloca na sua lista de prontos (seção 12.7). |
| **Lista de prontos** | Por tópico, em cada nó: IDs prontos para serem entregues, mantida por uma thread (seção 12.6). |

### 12.2 Entrega (`GET`)

![Sequência da entrega e do ack](diagramas/img/consumo.svg)

*Entrega (`GET`) e confirmação (ack). Fonte: [consumo.json](diagramas/fonte/consumo.json). Como o `GET` acha a mensagem quando a lista local está vazia: seção 12.8.*

O cliente informa a invisibilidade desejada na query string: `GET /topics/{topic}/messages?visibilityTimeout=120`. O valor é em segundos; **o padrão é 120 s** (`delivery.visibility-timeout.default`).

**Quantidade de mensagens.** O parâmetro `maxMessages` define quantas mensagens o `GET` pode devolver de uma vez; **o padrão é 10** (`delivery.max-messages.default`), com teto configurável (`delivery.max-messages.max`). A resposta traz de **1 a `maxMessages`** mensagens — o `GET` devolve o que está pronto, sem esperar completar a quantidade (com `waitTime > 0`, espera só a primeira). O coordenador captura **mensagem a mensagem** até `maxMessages`; cada uma passa pelo registro + commit de invisibilidade descrito abaixo, com o seu próprio `invisibleUntil`, e uma que falha não atrasa as demais (passo 7). Esse limite é da **API de cliente**; a comunicação entre nós para sincronização de blocos usa streams com tetos próprios (`cluster.sync.stream.*`, seção 8.2.1 de [06c-api-cluster.md](06c-api-cluster.md)), pensados para grandes volumes.

O nó que atende o `GET` usa o mesmo protocolo em duas fases da escrita (seção 7.2), agora para a invisibilidade:

1. Escolhe um ID da **lista de prontos** do tópico (seção 12.6), que contém só mensagens de que ele é o dono da entrega (seção 12.7).
2. Calcula, pelo ID, os **nós responsáveis** pelo bloco (primário + réplicas), como na seção 7.2.
3. **Fase 1 (registro):** envia a todos eles o pedido de marcar a mensagem como invisível até `agora + visibilityTimeout`, com `deliveryCount + 1` e um novo `receiptToken`. Cada nó grava a alteração como **pendente** e confirma.
4. **Fase 2 (commit), decidida por quórum:** assim que o **quórum de confirmações** é atingido — o **primário sempre incluso** (é ele quem remove o ID da própria lista de prontos, passo 6) —, o coordenador do `GET` decide e chama o commit nos nós do quórum.
5. **Só depois** de o commit ser confirmado **no quórum** a mensagem é **devolvida ao cliente**; as réplicas fora do quórum recebem o commit em segundo plano.
6. Ao receber o commit, cada nó do quórum **remove o ID da sua lista de prontos** do tópico. Como o primário está sempre no quórum, a mensagem deixa de ser candidata a nova entrega **no próprio nó que a entregou** no mesmo instante em que o cliente a recebe — igual ao comportamento de hoje, sem depender das réplicas fora do quórum.
7. **Falha antes do quórum fechar:** retry e, esgotado, **rollback** para descartar a alteração pendente (mesmos `write.retry.*` e `write.pending.ttl` da escrita, seção 7.2). O nó coordenador do `GET` captura as mensagens **uma a uma** até atingir `maxMessages`. Quando o registro de uma delas falha, ele **entrega o rollback a outra thread** e **segue imediatamente para o próximo ID** da lista (que pode ser de um bloco com nós saudáveis), sem esperar o rollback terminar: a prioridade é **velocidade e integridade**. A integridade vem de três pontos: a mensagem que falhou **nunca** entra na resposta; o rollback é concluído pela outra thread com os mesmos `write.retry.*`; e, se ele não chegar a uma réplica, o pendente expira sozinho por `write.pending.ttl`, e a mensagem segue pronta como estava. O número de falhas toleradas é limitado por `delivery.get.max-attempts` (3 por padrão, D114; conta as mensagens cujo registro não fechou, depois das repetições de `write.retry.*`). Ao fim, o `GET` responde com as mensagens capturadas, ainda que menos que `maxMessages`; se **nenhuma** foi capturada e houve falhas, responde `503` com `Retry-After`, nunca vazio, que o cliente leria como fila sem mensagens. **Falha nas réplicas fora do quórum:** sem rollback — o coordenador insiste dentro do teto `write.commit.timeout`.

A mensagem só chega ao cliente depois de a invisibilidade estar marcada no **quórum** (sempre incluindo o primário); assim, uma queda do coordenador logo após a entrega não faz outro nó entregá-la em paralelo **a partir do mesmo bloco**.

> **Trade-off aceito:** se o primário cair exatamente entre commitar localmente (passo 4) e replicar isso às réplicas fora do quórum, e for excluído do cluster antes de terminar essa replicação, o novo primário herda — via sincronização de bloco (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) — o `invisibleUntil` de **quem sobreviveu**, que pode estar desatualizado. Na prática, a mensagem pode voltar a ficar entregável **antes** do prazo original prometido ao primeiro consumidor. Isso não quebra a semântica *at-least-once* (entrega duplicada já é esperada) — só encurta, nesse caso raro, a garantia de tempo mínimo de processamento do `visibilityTimeout`.

### 12.3 Alteração da invisibilidade

O cliente pode alterar o tempo de invisibilidade de uma mensagem já entregue (`PUT /topics/{topic}/messages/{id}/visibility?receiptToken=...`, com o corpo `{"visibilityTimeout": 300}`, e `409` se o token estiver obsoleto), com o mesmo registro + commit por quórum da entrega, sem incrementar `deliveryCount` e sem mexer na lista de prontos. O valor é o **novo tempo a partir de agora** (`invisibleUntil = agora + visibilityTimeout`), não um acréscimo ao prazo atual, e respeita `delivery.visibility-timeout.max`. **`0` libera a mensagem na hora**: o prazo passa a ser o instante atual e o dono a recoloca na lista de prontos como num vencimento (seção 13.6 de [14-invisibilidade.md](14-invisibilidade.md)). Detalhes em [14-invisibilidade.md](14-invisibilidade.md), seção 13.3.

### 12.4 Confirmação (ack) e reentrega

- **Ack** (`DELETE /topics/{topic}/messages/{id}?receiptToken=...`): exige o `receiptToken` recebido na entrega; token que não confere com o dos metadados (a mensagem foi reentregue) responde `409` e nada muda. Segue o **mesmo protocolo de registro + commit** das demais operações sobre a mensagem (entrega, seção 12.2; escrita, seção 7.2). O coordenador do `DELETE` pede o registro da remoção a todos os nós responsáveis pelo bloco; com o **quórum** confirmando (primário incluso) ele chama o commit, que remove a mensagem e seus metadados nesses nós — as réplicas fora do quórum removem em segundo plano — **sem deixar tombstone**: a ausência do ID já é suficiente, e a sincronização de bloco trata a janela residual de uma remoção concorrente por conta própria (verificação final, seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)).
- **Fila FIFO:** o ack **não apaga a linha**: grava `deleteAt` e apaga só o corpo e os metadados de entrega, para a deduplicação valer pela janela ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7). É a única exceção ao "sem tombstone"; o quórum inclui o primário e o secundário.
- **Falha:** mesma regra da seção 3.1/7.2 — falha antes do quórum fechar **cancela** o ack (rollback) e a API retorna erro ao cliente, a mensagem continua disponível; falha nas réplicas fora do quórum, já decidido, não é desfeita: o coordenador insiste dentro do teto `write.commit.timeout` e, esgotado, o bloco fica temporariamente abaixo do fator de replicação pleno (evento de observabilidade, não falha de saúde). Nunca um ack parcial visível a outro consumidor — o quórum inclui sempre o primário, que é quem decide se a mensagem ainda existe para efeito de entrega.
- **Reentrega:** se o prazo `invisibleUntil` passa sem ack, a mensagem volta a ser elegível: o dono a recoloca na lista de prontos a partir da sua lista de invisíveis (com excedente em disco), sem propagar nada às réplicas — ver [14-invisibilidade.md](14-invisibilidade.md), seções 13.4 a 13.6. Ao ser entregue de novo, `deliveryCount` sobe.

### 12.5 Metadados em memória e em disco

`deliveryCount`, `invisibleUntil` e `receiptToken` ficam junto da mensagem em memória ou, se ela está em disco, no banco SQLite local do tópico (seção 3.2 de [02-arquitetura.md](02-arquitetura.md)); não há arquivo de metadados. Detalhes em [14-invisibilidade.md](14-invisibilidade.md), seção 13.2.

### 12.6 Lista de prontos

- Cada nó mantém, **por tópico**, uma lista de **IDs de mensagens prontas para entrega** — somente as de que ele é o **dono da entrega** (seção 12.7).
- Uma **thread** dedicada mantém a lista **cheia** (`delivery.ready-list.size`, **padrão 1000 IDs por tópico**, em memória). Ela fica parada num **semáforo** (o "farol"): cada vez que a lista libera lugar — uma entrega commitada, ou um ID que sai —, o semáforo é liberado e a thread repõe a quantidade de IDs que abriu, sem ficar consultando à toa. A reposição segue o efeito sanfona: a thread guarda dois cursores sobre as mensagens de que o nó é dono (da mais antiga para a frente e da mais nova para trás, por GUID v7, em memória e em disco) e repõe alternando as duas pontas. Uma mensagem nova que chega ao dono entra direto na lista se há lugar e o cursor da ponta nova está no fim; senão é encontrada pela thread depois. As mensagens de invisibilidade vencida voltam pela lista de invisíveis (seção 13.6 de [14-invisibilidade.md](14-invisibilidade.md)), não pela varredura.
- **Ordem de entrega: efeito sanfona.** A lista é mantida ordenada pelo GUID v7 (ordem de publicação) e a entrega consome **as duas pontas de forma intercalada**: a mais antiga, depois a mais nova, depois a segunda mais antiga, a segunda mais nova, e assim por diante. A thread de reposição também repõe a lista pelas duas pontas — as mais antigas e as mais novas ainda não listadas das mensagens de que o nó é dono —, para que as duas pontas estejam sempre disponíveis. Não há garantia de ordem ao cliente (seção 12.7).
- **Tópico com grupos:** a ordem e a reposição seguem a entrega justa da seção 12.11; sem `groupId`, vale o que está aqui.
- **Fila FIFO:** a lista tem **uma entrada por grupo livre** (a cabeça do grupo), ordenada pela cabeça mais antiga, em vez da sanfona e da entrega justa; um grupo com mensagem em voo não está na lista ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.6).
- Ao receber o **commit** de uma entrega (seção 12.2, passo 4), o nó remove o ID da lista automaticamente.
- Mensagens cuja invisibilidade vence voltam à lista pela thread de vencimento ([14-invisibilidade.md](14-invisibilidade.md), seção 13.6), pela posição do GUID v7; se a lista está cheia, o ID do **meio** da lista (o último que a sanfona alcançaria) sai para dar lugar e é reposto depois.
- O `GET` só consulta a lista, sem varrer memória e disco a cada chamada; o custo da busca fica na thread de reposição, fora do caminho crítico da entrega.

### 12.7 Dono da entrega

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): o nó é o dono de todas as mensagens.

As mensagens de um tópico se espalham por vários blocos e nós (seção 7.2), e vários nós têm cópia de cada mensagem. Se todas as réplicas entregassem, elas disputariam o commit de invisibilidade da mesma mensagem, com falhas e retries que aumentariam a latência do `GET` quando há muitos consumidores. Por isso cada mensagem tem **um único dono da entrega: o nó primário do bloco dela**.

**Como o `GET` acha uma mensagem (resumo).** O particionamento é **por ID da mensagem, não por tópico** (seção 7.1): as mensagens de um mesmo tópico ficam espalhadas por todos os nós, e o tópico **não pertence a nenhum nó**. Por isso o `GET` nunca precisa localizar "o nó do tópico". Basta:

1. Cada nó entrega apenas as mensagens de que é o **primário**, guardadas na sua lista de prontos do tópico.
2. Se a lista de prontos local tem um ID, o nó entrega direto, sem consultar outros nós.
3. Se a lista local está vazia, o nó usa a tabela **em memória** de "quais nós têm mensagens prontas neste tópico", mantida pelos sinais do canal entre nós (seção 12.8), e pede a **um** desses nós que entregue. Não há varredura nem consulta a todos os nós.

Um exemplo com os três cenários está na seção 12.8.

**Como cada nó sabe de quais mensagens é dono, sem conhecer o tópico inteiro:**

1. O ID da mensagem vira um hash, que cai numa posição do anel (seção 7.1), que pertence a um bloco, que tem seus nós responsáveis (primário, secundário, terciário...).
2. O cálculo só depende do ID e da tabela de membros, que todos os nós têm (seção 6). Qualquer nó chega ao mesmo primário para o mesmo ID.
3. Cada nó guarda apenas mensagens dos blocos em que é responsável. A thread da lista de prontos calcula o papel do nó em cada mensagem local (uma busca no anel) e inclui na lista só as que ele tem como **primário**.
4. O tópico completo é a **união das listas** dos nós; nenhum nó precisa montá-la.

**Efeitos:**

- Em condições normais, o commit de entrega de uma mensagem só é tentado pelo dono, sem disputa entre nós. Os nós virtuais equilibram a quantidade de mensagens de que cada nó é primário.
- A ordem de entrega é a sanfona do GUID v7 (mais antiga e mais nova, intercaladas) **dentro da lista de cada nó**; não há ordem global no tópico. **Garantia ao cliente: nenhuma ordem** — nem por tópico nem por chave (o `groupId` da seção 12.11 não ordena); a reentrega por invisibilidade vencida também a embaralha. Quem precisa de ordem deve ordenar no consumo, pelo `id` (GUID v7), ou usar uma **fila FIFO**, que garante a ordem por grupo ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16).
- **A partição não é o que garante a correção.** O registro de invisibilidade continua sendo exclusivo: um nó rejeita o pedido se já há outro pendente ou se a mensagem já está invisível, e o `GET` tenta outro ID. A divisão por dono só evita a disputa; se ela acontecer (por exemplo, com tabelas de membros divergentes por instantes), nenhuma mensagem é entregue em duplicidade.
- **Mudança no anel:** quando o primário de um bloco muda (entrada, saída ou rebalanceamento), a thread refaz o filtro dos blocos afetados. Um bloco em sincronização não entrega (seção 7.3).
- **Primário ainda não íntegro:** um nó que acabou de entrar e é primário de blocos só entrega quando termina a sincronização (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)). **Até lá as mensagens desses blocos não são entregues por ninguém**, nem pelo primário antigo, que as tira da sua lista de prontos no commit do registro. As chamadas de cliente que chegam a esse nó são repassadas a outro (seção 6.9 de [04-cluster.md](04-cluster.md)), e ele não envia avisos nem sinais de mensagens prontas. É o preço de manter um único dono por mensagem, e dura só o tempo da sincronização, já que só um nó se sincroniza por vez.
- **Lista vazia:** se a lista do nó que recebeu o `GET` está vazia, ele consulta a tabela em memória de nós com mensagens prontas, pede a **um** deles que entregue e, com long polling, aguarda novas mensagens se nenhum tiver — ver seção 12.8.
- **Dono indisponível:** se o dono de parte das mensagens está fora do ar e ainda não foi desregistrado, essas mensagens ficam sem entrega até o desregistro. Só o primário entrega e marca a invisibilidade; os demais nós do bloco são **cópias de segurança** (para o caso de o primário se perder) e participam do rebalanceamento de dados, mas não assumem a entrega.

**Alternativa descartada — nó controlador por tópico:** um nó eleito com a lista de entrega do tópico daria ordem total, mas todo `GET` do tópico passaria por ele (gargalo em tópico quente), exigiria eleição de líder (o cluster só tem votação de exclusão) e contraria o desenho de nós idênticos, sem mestre fixo (seção 3).

### 12.8 Long polling e canal entre nós

> **Modo drain** (seção 6.10 de [04-cluster.md](04-cluster.md)): o nó em drain responde `take` vazio e responde vazios os long pollings em andamento. Os pares, avisados por um sinal de estado, deixam de pedir `take` a ele.

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não há canal entre nós, `take` nem avisos de mensagem nova entre nós: o long polling só espera mensagens chegarem no próprio nó.

**Long polling.** O `GET` aceita `waitTime`, em segundos, de **0 a 20** (padrão 0: responde na hora). Com `waitTime > 0`, se não houver mensagem para entregar, a chamada **aguarda novas mensagens** até esse tempo e responde vazia se nenhuma chegar. `GET /topics/{topic}/messages?visibilityTimeout=120&waitTime=20`.

**Fluxo do `GET` quando a lista local não completa o `maxMessages`:** o nó coordenador começa pelas mensagens da própria lista de prontos (as de que ele é primário) e, se não chegou a `maxMessages`, completa com os passos abaixo; com a lista local vazia, parte direto deles. Só devolve ao cliente quando a lista está formada e todas as mensagens estão com a invisibilidade commitada. Não há dono do tópico: o tópico está distribuído entre os nós, e cada nó entrega o que é seu.

1. O nó consulta o que sabe em memória (ver "Canal entre nós" abaixo) sobre **quais outros nós têm mensagens prontas** do tópico e pede a **um deles por vez** que entregue (rota interna `POST /cluster/topics/{topic}/messages/take`). O peer é escolhido por **sorteio com peso** entre os que a tabela marca como "tem mensagens prontas": quanto mais mensagens prontas do tópico um nó informou no seu sinal de vida, maior a chance (sem informação de quantidade, o sorteio é uniforme). Se ele responder vazio (o estado era uma dica desatualizada) ou falhar, o nó tenta o próximo, até um teto de tentativas (`delivery.take.max-attempts`, a definir); esgotado o teto, segue para o passo 2. Assim o custo de um `GET` com lista local vazia é limitado a esse teto, nunca a um pedido a todos os nós. O pedido leva a **quantidade que ainda falta** para completar o `maxMessages` do cliente (seção 12.2). Quem entrega é sempre o **dono da mensagem**: ele escolhe o ID da sua lista de prontos, faz o registro + commit de invisibilidade (seção 12.2) e devolve a mensagem ao nó que pediu, que a repassa ao cliente. Assim a mensagem continua tendo um único coordenador.
2. Se, ao fim, **nenhuma** mensagem foi reunida (nem local nem dos outros nós), o `GET` entra como **ouvinte local** do tópico (fila de espera em memória, na ordem de chegada) até `waitTime`.
3. Quando chega uma **mensagem nova pronta no próprio nó**, ela vai direto ao primeiro ouvinte local, sem tráfego entre nós.
4. Quando o aviso vem de **outro nó** (abaixo), o ouvinte repete o passo 1 apenas com aquele nó. Se a mensagem já foi levada por outro, ele volta a esperar.
5. Esgotado `waitTime`, responde vazio. Se o cliente desconectar antes, o ouvinte é removido, liberando o recurso.

**Exemplo: três cenários.** Cluster com 3 nós (A, B, C) e o tópico `pedidos` com 6 mensagens. O hash do ID as distribui, e cada nó é primário de duas delas:

| Nó | É primário de | Lista de prontos de `pedidos` |
|---|---|---|
| A | m1, m4 | [m1, m4] |
| B | m2, m5 | [m2, m5] |
| C | m3, m6 | [m3, m6] |

Cada nó também guarda réplicas de mensagens de outros nós, mas réplica não entra na lista de prontos: só o primário entrega.

- **Cenário 1: lista local com mensagem.** O consumidor faz `GET` em A. A lista de A tem m1, então A é o dono: faz registro + commit de invisibilidade no quórum e devolve m1. Nenhum outro nó é consultado para achar a mensagem. Em fila cheia, é o caso comum.
- **Cenário 2: lista local vazia, outro nó tem.** A já entregou m1 e m4 e sua lista esvaziou. Nesse momento A enviou o sinal "sem mensagens prontas em `pedidos`", e B e C mantêm na memória a tabela `pedidos → {A: não, B: sim, C: sim}`. Um novo `GET` chega a A, que consulta a própria tabela, escolhe B ao acaso e chama `take`. B, o dono, faz registro + commit de invisibilidade e devolve a mensagem a A, que a repassa ao cliente. Se B já tivesse esvaziado, A tentaria C.
- **Cenário 3: nenhum nó tem.** A tabela marca A, B e C como "não". O `GET` de A, com `waitTime > 0`, vira ouvinte local, e A informa aos demais, uma única vez, "`pedidos` tem ouvintes em A". Quando uma mensagem nova fica pronta em B, B consulta a própria tabela, vê o ouvinte em A e manda o aviso "há mensagens novas de `pedidos` aqui". A chama `take` em B, recebe a mensagem e a entrega ao ouvinte. Sem chegada de mensagem até `waitTime`, o `GET` responde vazio.

![Cenário 1: a lista local tem mensagem](diagramas/img/consumo-cenario1-lista-local.svg)

![Cenário 2: lista local vazia, outro nó tem mensagem](diagramas/img/consumo-cenario2-outro-no.svg)

![Cenário 3: nenhum nó tem mensagem, o GET espera](diagramas/img/consumo-cenario3-espera.svg)

*Fontes: [cenário 1](diagramas/fonte/consumo-cenario1-lista-local.json), [cenário 2](diagramas/fonte/consumo-cenario2-outro-no.json) e [cenário 3](diagramas/fonte/consumo-cenario3-espera.json).*

Ninguém varre os nós: o custo de saber onde há mensagens é pago só nas **transições** (um nó passa de "tem" para "não tem", ou o contrário), com sinais pequenos. Comparação com as alternativas descartadas:

| Alternativa | Custo por `GET` com lista local vazia |
|---|---|
| Ler o monitoramento de todos os nós | Consulta a N nós, estado já velho ao chegar |
| `GET` em todos, um por um | Até N chamadas por `GET`; C consumidores esperando geram C × N chamadas |
| Sinais nas transições (adotada) | Consulta local em memória + até `delivery.take.max-attempts` chamadas |

**Canal entre nós.** Entre cada par de nós existe **um canal de comunicação permanente** (uma conexão **WebSocket**, em malha completa: cada nó conectado a todos os outros), único e compartilhado por todos os tópicos (nunca uma conexão por chamada de long polling nem por tópico). Por ele cada nó informa aos outros, **somente quando o estado muda** (não a cada chamada):

| Informação | Quando é enviada |
|---|---|
| **Tópicos com ouvintes** | O tópico passa a ter ouvintes neste nó (0 → 1) ou deixa de ter (→ 0). |
| **Tópicos com mensagens prontas** | O nó passa a ter mensagens prontas para o tópico (vazio → não vazio) ou esvazia. |
| **Aviso de mensagem nova** | Chegou uma mensagem nova pronta no nó dono e **não há ouvinte local**, mas outro nó informou ter ouvinte no tópico: envia a ele "há mensagens novas do tópico X aqui, venha buscar". |

Cada nó mantém em memória, por tópico, quais nós têm ouvintes e quais têm mensagens prontas; com isso, a decisão de a quem pedir ou avisar é local, sem consultar ninguém.

- O aviso é uma **dica**, não uma garantia: quem recebe vai buscar, e pode não encontrar nada. Um aviso perdido só custa espera até o fim do `waitTime` (no máximo 20 s), e o cliente repete o `GET`.
- **Quem envia os avisos e como (decidido):** em cada nó dono há **um responsável por tópico** (thread ou tarefa, ativo só enquanto o tópico tem ouvintes em algum nó e mensagens novas a avisar) que cuida de todos os avisos daquele tópico:
  - **Agrupamento:** as mensagens que ficam prontas são **acumuladas por uma janela curta** (`cluster.notify.window`, **padrão 100 ms**, configurável) e viram **um aviso só**, com a quantidade de mensagens prontas, em vez de um aviso por mensagem.
  - **Um nó por vez, em ciclos:** o aviso vai a **um** nó com ouvinte, escolhido revezando entre os que têm. O responsável **espera alguns milissegundos** (`cluster.notify.wait`, padrão 50 ms) para ver se esse nó vem buscar as mensagens (`take`). Se o nó consome, o ciclo termina; se ele **não vem** (já não quer mensagens do tópico, ou foi atendido por outro), o responsável passa ao **próximo nó** com ouvinte e **repete o ciclo**, até acabarem as mensagens avisadas ou os nós com ouvinte.
  - Nenhum aviso é um compromisso: o nó que recebe vai buscar o que existir (seção acima), e pode não encontrar nada.
- **Sem fan-out por chamada:** N ouvintes esperando o mesmo tópico em um nó geram uma única informação de interesse; ao receber um aviso, o nó busca de uma vez tantas mensagens quantos são os ouvintes que estão esperando.
- **Custo de conexões:** uma conexão por par de nós, a mesma malha usada para o sinal de vida (seção 6.3), independente do número de tópicos e de consumidores.
- O canal é usado só para **sinais**: sinal de vida, estado de ouvintes e de mensagens prontas, e avisos. **Nunca** trafega o conteúdo das mensagens nem listas longas, para ficar livre e rápido; o conteúdo continua indo pelas rotas internas, com registro e commit (seções 12.2 e 7.2). O estado é enviado em **mudanças pontuais** (tópico X ganhou ou perdeu ouvinte), não como listas completas.

**Sinal de vida (decidido):** é uma **mensagem própria**, enviada periodicamente (`cluster.channel.keepalive.interval`), não o ping/pong do WebSocket, para também detectar um nó travado com a conexão ainda aberta. Cada sinal leva a **data e hora** do nó e um **resumo de monitoramento** no estilo do OpenTelemetry (inclui um indicador de carga do nó, como uso de CPU e requisições em andamento), e dentro dele a **quantidade de mensagens prontas por tópico** naquele nó. Quem recebe usa essas quantidades para **orientar o `GET`**: ao pedir mensagens a outros nós (`take`), em vez de sortear com a mesma chance, **sorteia com peso proporcional** às mensagens prontas de cada nó, indo mais vezes aos que têm mais. Para o sinal ficar pequeno, só entram os tópicos com mensagens prontas, até um teto de tópicos por sinal (`cluster.channel.keepalive.max-topics`), e o que não coube segue pelas mudanças de estado já descritas. A quantidade é uma dica e pode estar velha; o `take` continua tentando o próximo nó se o escolhido responder vazio. O sinal de vida **não é confirmado nem reenviado**: o seguinte o substitui. Por isso ele também **repete o estado do nó** (`state`: `active`, `syncing`, `draining` ou `failed`) e a versão da tabela de membros: se um `node-state` se perdeu, o sinal de vida seguinte o corrige.

### 12.9 Filas do canal, reconexão e exclusão de nó

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica; não há canal, fila de saída nem exclusão de nó.

Cada nó mantém **em memória**, para cada outro nó, duas filas:

| Fila | Conteúdo |
|---|---|
| **Fila de saída** (por nó de destino) | Sinais a enviar ao outro nó: sinal de vida, mudanças de estado (ouvintes, mensagens prontas) e avisos. Uma thread de escrita esvazia a fila pelo WebSocket, sem bloquear quem produz os sinais. |
| **Fila de entrada** (por nó de origem) | O que chega do par é lido do socket e posto nesta fila, e uma tarefa à parte a processa na ordem e devolve o `confirm`: um processamento lento não esconde o sinal de vida do par, só atrasa o `confirm` (métrica `taba.cluster.channel.queue.in`). |

**Queda e reconexão:**

- Quando o WebSocket cai, os sinais **continuam acumulando na fila de saída** daquele nó e o nó tenta reconectar (seção 6.3).
- Se o canal **reconecta antes** de a votação de exclusão ser concluída, **nada muda**: os nós retomam o envio dos sinais pendentes, de onde pararam.
- Se a votação de exclusão for **concluída**, todos os nós **descartam as filas** de comunicação com aquele nó e param de acumular sinais para ele. Se o nó voltar, refaz o bootstrap do zero (seção 6.6), com filas novas e vazias.

**Confirmação de cada sinal (decidido):** todo sinal enviado pelo canal, **exceto o sinal de vida**, leva um **identificador** (um número crescente por canal e por sentido). O receptor, depois de **processar** o sinal, devolve a **confirmação** daquele identificador (as confirmações podem ir agrupadas). O emissor só tira o sinal da fila de saída ao receber a confirmação; se o canal cai, **reenvia na reconexão o que ficou sem confirmação**, e o que já foi confirmado **nunca se repete**. Se uma confirmação se perde e o sinal é reenviado, o receptor reconhece o identificador já processado e o ignora (basta guardar o maior identificador processado). O sinal de vida não tem identificador nem reenvio: o próximo substitui o anterior.

**Quem abre a conexão:**

- O nó que **entra ou volta** é sempre o que **se conecta ativamente** aos outros. No bootstrap de um nó novo, ele abre o canal com cada nó da tabela que recebeu como resposta (seção 6.2); os demais apenas aceitam. Um nó só aceita conexão de quem consta na sua tabela de membros.
- **Queda com os dois nós vivos** (falha de rede, sem reinício): o nó que **detectar primeiro** tenta reconectar. Se os dois acabarem abrindo uma conexão ao mesmo tempo, **o nó de menor hash do ID fecha a conexão que ele abriu** e fica com a aberta pelo outro: regra de desempate, para haver sempre um só canal por par.
- **Reinício do processo:** o nó perde a memória, inclusive a tabela de membros (ela **não é persistida**), e volta como **nó novo**, passando pelo registro (seção 6.2). Os dados que estavam em disco seguem o procedimento de retorno da seção 6.6: as mensagens são enviadas a quem as perdeu.

> **Como está no código.** O canal é um WebSocket (`GET /cluster/channel`, doc 06c) com lotes de quadros e `confirm`, na rede real, e um par de filas na rede em memória dos testes. Quem entra abre a conexão com todos; os antigos esperam um intervalo de sinal de vida e só então abrem; em conexão dupla, fica a do nó de maior hash. A conexão cai por fechamento ou por silêncio do par (duas vezes o intervalo do sinal de vida).

**Estado inicial ao abrir o canal:** os sinais acima só são enviados nas **mudanças**, então um nó que acaba de ficar pronto (ou que reconectou depois de perder a fila) não saberia o estado atual dos outros. Por isso, ao abrir um canal, cada lado envia primeiro um **instantâneo** dos seus sinais — tópicos com ouvintes, tópicos com mensagens prontas e os nós ainda em freezing (`frozen`, seção 6.7 de [04-cluster.md](04-cluster.md)) —, em sinais pequenos, e só depois passa às mudanças. Um nó em sincronização ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3) só consome o instantâneo quando fica pronto, e antes disso não entrega nem avisa.

**Limite e juntada dos sinais na fila de saída:** enquanto o canal está fora e a votação não conclui, a fila de saída é mantida pequena assim:

- Sinais de **estado** são idempotentes (ex.: "o tópico X tem ouvintes"): só o **último de cada tópico** importa, então um novo sinal **substitui** o anterior ainda na fila em vez de se acumular.
- **Avisos** de mensagem nova antigos são **descartados** (depois de 10 s, fixo no código, D109): perderam o sentido e o ouvinte já foi atendido ou expirou.
- O sinal `frozen` (um nó excluído por votação entrou em freezing) **não expira** na fila e só é substituído por outro `frozen` do **mesmo nó**: ele existe para o par que estava fora do ar saber, quando voltar, que aquele ID está em quarentena. Quem já o conhecia não o repassa (eco), e um `frozen` já vencido é ignorado (17c-3).
- A fila tem **tamanho e idade máximos** (`cluster.channel.queue.max-size`, `cluster.channel.queue.max-age`). Se mesmo assim passar do limite, o nó com quem o canal está fora é tratado como **falho**: abre-se a votação de exclusão (seção 6.3), sem esperar o fim das tentativas de reconexão. Não há ressincronização do estado do canal — o nó excluído, ao voltar, faz um **novo registro** e sincroniza do zero.
- A fila de saída de um nó excluído é descartada (a de entrada não existe).

### 12.10 Expiração (TTL), limite de entregas e dead-letter

Três regras de retenção valem para toda fila (QT). Cada uma tem um **padrão do cluster**, configurável, e pode ser **definida por tópico**, na criação (`POST /topics`) e no `PATCH`, nos campos `ttl` e `maxDeliveries` da definição do tópico (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)). Campo ausente significa "usa o padrão". Um Tópico Distribuidor não guarda mensagens e não tem esses campos.

| Regra | Campo do tópico | Padrão (chave) | Efeito |
|---|---|---|---|
| **Expiração** | `ttl` (segundos) | **14 dias** (`retention.ttl.default`) | A mensagem com mais de `ttl` desde a publicação é **descartada**, entregue ou não. |
| **Limite de entregas** | `maxDeliveries` | **10** (`retention.max-deliveries.default`) | A mensagem que chega a `maxDeliveries` entregas sem ack vai para o dead-letter. |
| **Dead-letter** | — (derivado da fila) | Sufixo **`-dead-letter`** (`topics.dead-letter.suffix`) | Fila que recebe as mensagens que estouraram `maxDeliveries` (abaixo). |

O padrão vale **na hora da conferência**, não na da criação: quem não definiu o campo acompanha uma mudança futura do padrão do cluster; quem definiu mantém o seu valor. Faixas aceitas em [12-limites-api.md](12-limites-api.md).

**Expiração (`ttl`).**

- A idade vem do próprio ID: o GUID v7 já carrega o instante da publicação (na fila de dead-letter, do instante em que a mensagem entrou nela, gravado em `deadLetteredAt`). O prazo de uma mensagem é `instante + ttl`, calculado com o `ttl` **atual** do tópico: um `PATCH` em `ttl` vale também para as mensagens já guardadas.
- A conferência é **determinística e local**, sem propagação: o dono não entrega mensagem vencida, e uma thread de cada nó varre as suas próprias cópias (memória e disco) e as apaga, a cada `retention.sweep.interval`. Como todos aplicam a mesma regra ao mesmo ID, não há rodada de registro + commit por mensagem, como no vencimento da invisibilidade (seção 13.3 de [14-invisibilidade.md](14-invisibilidade.md)). A sincronização de bloco (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) também descarta o que já venceu em vez de copiar.
- A expiração vale mesmo para mensagem **invisível** (em processamento): ela some, e o ack que chegar depois recebe a resposta de mensagem inexistente.
- Mensagem expirada é **excluída, não vai para o dead-letter**: o dead-letter guarda o que falhou no consumo, e a expiração é a regra de que "acabou o prazo de interesse". Cada exclusão gera a métrica `taba.retention.expired` e o evento `taba.message.expired`.

**Limite de entregas (`maxDeliveries`).** `deliveryCount` sobe a cada entrega (seção 12.1). Quando o prazo de invisibilidade de uma mensagem vence sem ack e `deliveryCount ≥ maxDeliveries`, ela **não** volta à lista de prontos: o dono a manda ao dead-letter (seção 13.6 de [14-invisibilidade.md](14-invisibilidade.md)). O mesmo vale para a alteração com `visibilityTimeout: 0`. Assim, `maxDeliveries: 10` significa **no máximo 10 entregas**; a décima primeira nunca acontece. Isso para um consumidor que baixa a mesma mensagem sem nunca apagá-la (ack que falha, erro no processamento, consumidor que morre): em vez de ela circular para sempre, ela sai da fila.

**Dead-letter.**

- Toda **fila** tem uma fila de dead-letter, de nome `{fila}` + sufixo, por padrão `{fila}-dead-letter`. Ela nasce **com a fila**, na mesma operação de registro + commit da criação, e é **excluída com ela**, com as mensagens que tiver. Também sai junto uma fila volátil excluída por ociosidade.
- **Sem definição própria.** Todas as configurações (`ttl`, `maxMessageBytes`, `tags`) ficam na **fila de origem**; a dead-letter as deriva e **não tem linha de definição** na tabela `topics`. Perguntada diretamente (`GET /topics/{fila}-dead-letter`), traz essas informações como **somente leitura**, com `deadLetter: true` e `source` (o nome da fila). `PATCH` e `DELETE` nela respondem `409`; muda-se a fila de origem. O `maxDeliveries` não se aplica a ela: **não há dead-letter da dead-letter**, e a mensagem só sai por ack ou por `ttl`.
- **No mais, é um tópico comum.** Entra em `GET /topics`, nas métricas e nas rotas `/metrics/topics`, na lista de prontos, no overflow para disco, na invisibilidade e no ack. Dá para consumir (`GET`), confirmar, e também publicar (por exemplo, devolver à fila de origem uma mensagem corrigida). O prefixo do nome não a distingue de uma fila comum para nenhum desses mecanismos; as exceções estão na lista abaixo.
- **O movimento** é **uma única operação de registro + commit** sobre os nós responsáveis, que cria a mensagem na dead-letter e a remove da fila de origem. A mensagem **mantém o mesmo ID**, e por isso cai no **mesmo bloco e nos mesmos nós**, o que dispensa coordenar blocos diferentes e torna a repetição idempotente. `deliveryCount` volta a `0`, `invisibleUntil` é zerado, e os metadados ganham `deadLetteredAt` e `deadLetterReason` (`max-deliveries`). A falha antes do quórum faz rollback e a mensagem segue na fila, e o dono tenta de novo na próxima volta da thread de vencimento.
- **Fila FIFO:** a mensagem que atinge `maxDeliveries` vai para o dead-letter e **o grupo segue** com a próxima ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.6). A dead-letter de uma fila FIFO **não é FIFO**, e usa o `id` da fila de origem (e o `groupId` da mensagem) para a posição no anel, para o movimento continuar sendo uma operação só. A mensagem que sai por `ttl` na cabeça também libera o grupo.
- **Exceções ao tópico comum:**
  - A dead-letter **nunca é destino de um Tópico Distribuidor**: o casamento por prefixo (seção 14.2 de [15-topicos-distribuidor.md](15-topicos-distribuidor.md)) a ignora, para a publicação no distribuidor não cair nela.
  - O `GET` nela **não conta como consumidor** da fila volátil de origem (seção 14.3), e `autoCreate` com nome terminado no sufixo é recusado com `400`.
  - **Nome reservado:** um tópico não pode ser criado com nome terminado no sufixo (`400`). O nome da dead-letter passa do limite de 100 do nome criado (até **112** com o sufixo padrão); só ela pode ser assim, e as rotas a aceitam.
- **Eventos:** `taba.message.dead_lettered` e a métrica `taba.delivery.dead_lettered`, por fila de origem ([11-observabilidade.md](11-observabilidade.md)). A dead-letter crescendo é o sinal de que há mensagem que ninguém consegue processar.

### 12.11 Grupos de mensagens e entrega justa (filas barulhentas)

Quando um produtor despeja muitas mensagens de uma vez, ele **atrasa as dos outros**: as mensagens de quem manda poucas ficam atrás de um monte do mesmo remetente. Para evitar isso (o "vizinho barulhento"), a mensagem pode levar um **grupo** (`groupId`, texto), e a entrega dá **preferência aos grupos que têm menos mensagens**. Os grupos pequenos andam rápido, e o grupo grande é atendido com o que sobra de capacidade, sem ficar parado para sempre.

**Grupo da mensagem.**

- Opcional, informado na publicação: `POST /topics/{topic}/messages?groupId=...` ([06a-api-cliente.md](06a-api-cliente.md)). De 1 a 128 caracteres, `A-Za-z0-9_-.:` (limite fixo, [12-limites-api.md](12-limites-api.md)). Imutável, guardado com a mensagem (em memória ou na coluna `group_id` do banco do tópico, seção 3.2 de [02-arquitetura.md](02-arquitetura.md)) e devolvido no `GET`.
- A mensagem **sem grupo** pertence a um grupo vazio, que conta como qualquer outro. Um tópico em que ninguém usa `groupId` tem **um grupo só** e se comporta exatamente como na seção 12.6.
- Num Tópico Distribuidor, o `groupId` é repassado a todas as cópias (seção 14.2 de [15-topicos-distribuidor.md](15-topicos-distribuidor.md)). Na dead-letter (seção 12.10), a mensagem mantém o grupo.
- Numa fila comum, o grupo **não** ordena nem serializa a entrega, como acontece nas filas FIFO do SQS: serve só de chave para a justiça entre remetentes. Quem precisa de ordem usa uma **fila FIFO** ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16), em que o `groupId` é obrigatório, ordena e serializa, e a preferência por grupos menores desta seção **não vale** (a cabeça mais antiga vence).

**Contagem, por nó.** Cada **dono da entrega** (seção 12.7) mantém, por tópico, **dois contadores por grupo**, das mensagens que ele possui e que ainda existem: as **prontas** e as **em processamento** (invisíveis, já entregues e sem ack). O **tamanho do grupo** mistura os dois: `tamanho = em processamento + peso × prontas`, com o `peso` em `delivery.fair.ready-weight` (padrão **0,1**).

- A publicação soma uma pronta. A entrega move uma de pronta para em processamento. O vencimento da invisibilidade (e a alteração com `0`) a devolve a pronta. O ack, a expiração por `ttl` (seção 12.10) e o movimento para o dead-letter a descontam.
- **Por que o peso.** Contar só o que está em processamento, como o SQS faz, reage tarde a uma rajada: o grupo que acabou de publicar milhares de mensagens parece pequeno até os consumidores o puxarem. Contar todo o backlog penaliza um grupo grande e inofensivo, de consumidor lento e sem pressa. O peso fica no meio: `0` equivale ao SQS (só o que está em processamento) e `1` ao backlog inteiro. Com `0,1`, 100 mil prontas valem 10 mil, e cada mensagem puxada pelo consumidor **sobe** o tamanho do grupo em `1 − peso`.
- É **local**, sem troca entre nós. Como o ID decide o bloco, as mensagens de um grupo se espalham pelos donos de forma aproximadamente proporcional, então cada nó enxerga uma amostra fiel do tamanho de cada grupo. A justiça é, por isso, aproximada no cluster, e exata no modo desenvolvimento (seção 6.4 de [04-cluster.md](04-cluster.md)).
- Fica em memória e é **derivada**: refeita dos metadados quando o nó assume blocos ou reinicia (seção 13.7 de [14-invisibilidade.md](14-invisibilidade.md)), contando a coluna `group_id` do banco e as mensagens em memória.
- O número de grupos acompanhados é limitado (`delivery.fair.max-groups`, por tópico e por nó). Passando do limite, os grupos novos **compartilham um contador único**, tratados como um só grupo.

**Preferência na entrega.** O tamanho do grupo (acima) é medido por uma **faixa**, `⌊log2(tamanho)⌋`, com tamanho menor que 1 contado como 1: tamanho 1 é a faixa 0, de 2 a 3 a faixa 1, de 4 a 7 a faixa 2, e assim por diante. Faixas por potência de 2 evitam reordenar a lista a cada mensagem que entra ou sai.

1. **Reposição.** A thread da lista de prontos (seção 12.6), em vez de repor só pelas pontas por ID, percorre os grupos **do menor para o maior** e toma de cada um as mensagens prontas mais antigas, **uma por grupo a cada rodada**, até encher a lista. Os grupos são achados por um **índice por grupo**: em memória para as mensagens em memória, e no banco, por `(group_id, id)`, para as em disco.
2. **Ordem da lista.** A lista de prontos fica ordenada pela **faixa** do grupo e, dentro da faixa, pela ordem normal (efeito sanfona, seção 12.6). Uma mensagem que chega ao dono entra na lista **na posição da faixa do seu grupo**, à frente das de grupos maiores.
3. **Entrega.** O `GET` tira do início da lista, isto é, da menor faixa com mensagens. Ao tirar, confere a faixa atual do grupo: se mudou desde que o ID entrou, o ID é reposicionado e o próximo é considerado.
4. **Progresso garantido.** A cada `delivery.fair.normal-every` entregas (padrão **10**), uma delas pega **a mensagem mais antiga da lista, de qualquer grupo**, ignorando a faixa. Os grupos grandes nunca esperam sem limite, mesmo que grupos pequenos cheguem sem parar.

O `take` (seção 12.8) e o sorteio com peso entre nós não mudam: a justiça é aplicada por cada dono, na hora de escolher o ID. A invisibilidade, o ack, a reentrega e o dead-letter também não mudam.

**Observabilidade.** As métricas consideram o grupo: `GET /metrics/topics/{topic}/groups` traz mensagens, parcela, idade e taxas **por `groupId`**, com `top=N` para achar o barulhento; `GET /metrics/topics/{topic}` traz `groups`, `topGroups` e `noisyShare`, e a condição `noisy-group`; e a métrica `taba.delivery.fair.wait` mede a espera por faixa. Não há série por grupo no OTLP nem no Prometheus, pela cardinalidade ([11-observabilidade.md](11-observabilidade.md)).
