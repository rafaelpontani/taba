---
título: Entrega de mensagens
resumo: Como o Taba entrega, confirma e reentrega mensagens — invisibilidade, lista de prontos, dono da entrega, long polling, canal entre nós, ack, limite de entregas, dead-letter e entrega justa entre grupos.
tipo: explicação
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/engine/messages, crates/taba-node/src/engine/messages/index, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/cluster/replication/signals.rs, crates/taba-node/src/cluster/channel, crates/taba-node/src/http/messages]
---

# Entrega de mensagens

> Leia antes: [06-anel-e-replicacao.md](06-anel-e-replicacao.md) e [10-publicacao.md](10-publicacao.md).

## O modelo: invisibilidade temporária

Como o *visibility timeout* do Amazon SQS: ao **entregar** uma mensagem, o Taba a torna **invisível** por um tempo. Se o consumidor a **confirma** (ack) nesse prazo, ela é removida; se não, o prazo vence e ela **volta a ficar disponível**. A semântica é **pelo menos uma vez** ([11-durabilidade.md](11-durabilidade.md)).

Do ponto de vista do **dono da entrega**, uma mensagem está sempre em um destes estados:

| Estado | Como se reconhece | O que a controla |
|---|---|---|
| **Pronta** | `invisibleUntil` ausente ou no passado | Lista de prontos |
| **Invisível** | `invisibleUntil` no futuro | Lista de invisíveis (ordenada por prazo) |
| **Removida** | Ack confirmado | Nada: mensagem e metadados apagados |

![Estados de uma mensagem no dono da entrega](../diagramas/img/pt-BR/estados-mensagem.svg)

## Metadados da mensagem entregue

| Campo | Significado |
|---|---|
| `deliveryCount` | Quantas vezes foi entregue (sobe a cada **entrega**; não muda na alteração da invisibilidade) |
| `invisibleUntil` | **Instante absoluto**, calculado pelo relógio do **dono** (`agora + visibilityTimeout`). As réplicas guardam o valor pronto e nunca recalculam |
| `receiptToken` | Token opaco gerado **a cada entrega**; exigido no ack e na alteração. Como o `ReceiptHandle` do SQS |

**Uso do `receiptToken`:** se o prazo venceu e a mensagem foi **reentregue a outro**, o token do primeiro consumidor ficou **obsoleto**: o ack dele responde `409 message.stale-receipt` e nada muda. Se o prazo venceu mas ela **ainda não foi reentregue**, o token continua válido e o ack vale. A alteração da invisibilidade mantém o mesmo token.

Os metadados ficam junto da mensagem, em memória ou no banco do tópico ([08-armazenamento.md](08-armazenamento.md)).

## Consumir (`GET`)

```bash
curl -s "localhost:8080/topics/pedidos/messages?visibilityTimeout=120&maxMessages=10&waitTime=20"
```

| Parâmetro | Padrão | Faixa | Efeito |
|---|---|---|---|
| `visibilityTimeout` | **120** s (`delivery.visibility-timeout.default`) | até `delivery.visibility-timeout.max` | Quanto a mensagem fica invisível |
| `maxMessages` | **10** (`delivery.max-messages.default`) | até `delivery.max-messages.max` | Máximo de mensagens na resposta |
| `waitTime` | **0** | 0 a **20** s | Long polling: espera mensagens em vez de responder vazio |
| `autoCreate`, `volatile`, `fifo`, `idleTimeout`, `tag.*` | — | — | Criação da fila pelo `GET` ([09-topicos.md](09-topicos.md)) |

A resposta traz **de 1 a `maxMessages`** mensagens: devolve o que está pronto, sem esperar completar a quantidade (com `waitTime > 0`, espera só a **primeira**). Se não há nada, `{"messages":[]}`.

### O que acontece por dentro

![Consumo: entrega e ack](../diagramas/img/pt-BR/consumo.svg)

1. O nó que atende escolhe um ID da **lista de prontos** do tópico, que só contém mensagens de que ele é o **dono da entrega**.
2. Calcula os nós responsáveis pelo bloco do ID.
3. **Fase 1:** registra em todos que a mensagem fica invisível até `agora + visibilityTimeout`, com `deliveryCount + 1` e um novo `receiptToken`.
4. **Fase 2:** quando o **quórum** confirma (**primário sempre incluso**), decide o commit.
5. **Só então** a mensagem é devolvida ao cliente. No commit, cada nó do quórum remove o ID da sua lista de prontos.
6. **Falha antes do quórum:** repete e, esgotado, **rollback** do pendente; o `GET` **segue imediatamente para o próximo ID** (o rollback termina em outra tarefa), até `delivery.get.max-attempts` falhas (padrão 3). Ao fim responde com o que capturou; se **nada** foi capturado e houve falhas, responde **`503` com `Retry-After`**, nunca vazio (o cliente leria como fila sem mensagens).

A mensagem só chega ao cliente depois de a invisibilidade estar marcada no quórum com o primário; uma queda do coordenador logo após a entrega **não** faz outro nó entregá-la em paralelo a partir do mesmo bloco.

## Dono da entrega

Cada mensagem tem **um único dono**: o **primário do bloco** dela ([06-anel-e-replicacao.md](06-anel-e-replicacao.md)). Se todas as réplicas entregassem, disputariam o commit de invisibilidade da mesma mensagem.

- O tópico **não pertence a nenhum nó**: as mensagens estão espalhadas por todo o anel, e o tópico completo é a **união das listas de prontos** dos nós.
- **A partição por dono não é o que garante a correção.** O registro de invisibilidade continua **exclusivo**: um nó recusa o pedido se já há outro pendente ou se a mensagem já está invisível, e o `GET` tenta outro ID. Mesmo com tabelas de membros divergentes por instantes, **nenhuma mensagem é entregue em duplicidade**.
- **Novo primário ainda sincronizando:** as mensagens dos blocos dele **não são entregues por ninguém** até ele ficar pronto (o primário antigo as tira da lista no commit do registro). Dura só o tempo da sincronização.
- **Dono fora do ar** e ainda na tabela: as mensagens dele ficam sem entrega até a exclusão. Os demais nós do bloco são cópias de segurança e **não** assumem a entrega.
- **Ordem:** **nenhuma garantia** de ordem (nem por tópico, nem por `groupId` em filas comuns); a reentrega por invisibilidade também embaralha. Quem precisa de ordem usa uma **fila FIFO** ou ordena no consumo pelo `id` (GUID v7).

## Lista de prontos

Por tópico e por nó, a **lista de IDs prontos** de que o nó é dono.

- **Ordem: efeito sanfona.** Ordenada pelo GUID v7, a entrega consome **as duas pontas intercaladas** (mais antiga, mais nova, segunda mais antiga…), para as duas pontas estarem sempre disponíveis.
- **Na entrega justa** (grupos), a ordem é por **faixa** do grupo e, dentro da faixa, a sanfona (ver abaixo). **Na FIFO**, a lista tem **uma entrada por grupo livre** ([09-topicos.md](09-topicos.md)).
- O `GET` só **consulta o índice**: não varre memória e disco a cada chamada.
- **Quando falta lugar na memória**, quem sai primeiro é o que está em processamento e, depois, o **meio** da lista de prontos (o último que a sanfona alcançaria); as pontas ficam, por serem as próximas a entregar.
- O índice é **derivado** dos metadados: refeito quando o tópico é aberto (reinício ou novo dono).

> **Como o código está hoje (diverge da doc histórica).** O índice (`engine/messages/index`) mantém **todas** as mensagens prontas e invisíveis do dono em memória (conjuntos ordenados), e não há uma tarefa que reponha uma lista de tamanho fixo. Os parâmetros `delivery.ready-list.size`, `delivery.invisible-list.size` e `delivery.invisible.reload-threshold` são **aceitos mas sem efeito** (ver [22-configuracao.md](22-configuracao.md) e [31-pendencias.md](31-pendencias.md)). A tabela `expiry` também não existe.

## Alterar a invisibilidade

```bash
curl -s -X PUT "localhost:8080/topics/pedidos/messages/<id>/visibility?receiptToken=<token>" \
  -H 'Content-Type: application/json' -d '{"visibilityTimeout": 300}'
# 200 {"id":"…","invisibleUntil":"2026-10-13T17:10:13.601Z"}
```

- Usa o **mesmo registro + commit por quórum** da entrega, **sem** incrementar `deliveryCount` e sem mexer na lista de prontos.
- O valor é o **novo tempo a partir de agora** (`agora + visibilityTimeout`), **não** um acréscimo; respeita `delivery.visibility-timeout.max`.
- **`0` libera a mensagem na hora**: o dono a recoloca na lista de prontos como num vencimento. Se `deliveryCount ≥ maxDeliveries`, ela vai para a dead-letter.
- `409 message.stale-receipt` se o token está obsoleto.

## Confirmar (ack)

```bash
curl -s -i -X DELETE "localhost:8080/topics/pedidos/messages/<id>?receiptToken=<token>"
# HTTP/1.1 204 No Content
```

- Exige o `receiptToken` da última entrega. Token que não confere → `409`, nada muda.
- Mesmo protocolo de registro + commit: o quórum (primário incluso) confirma e o commit **remove a mensagem e seus metadados**, **sem tombstone** (a ausência do ID basta; a verificação final da sincronização cobre a janela residual). As réplicas fora do quórum removem em segundo plano.
- **Fila FIFO:** o ack grava `deleteAt` e apaga só o corpo, para a deduplicação ([09-topicos.md](09-topicos.md)). É a única exceção ao "sem tombstone".
- **Falha antes do quórum:** o ack é **cancelado** e a mensagem continua disponível (o cliente repete). Nunca há ack parcial visível.

## Vencimento da invisibilidade

Uma tarefa do dono olha a **cabeça** da lista de invisíveis (a próxima a vencer) e dorme até o prazo (com um teto curto, para reagir a mudanças). Ao vencer:

1. Remove a entrada e **confere** nos metadados: a mensagem ainda existe, o nó ainda é o dono e o prazo ainda é o da entrada; senão descarta.
2. **Limite de entregas:** se `deliveryCount ≥ maxDeliveries`, **não** volta à lista de prontos: o dono a manda à dead-letter. Mensagem já vencida por `ttl` também é descartada.
3. Senão, **insere o ID na lista de prontos** pela posição do GUID v7 (uma mensagem que volta é antiga e tende a ficar perto da ponta antiga).
4. Se a lista **estava vazia**, avisa os outros nós; se há ouvinte local, a mensagem vai **direto a ele**.
5. **Nada é escrito nos metadados nem enviado às réplicas:** o vencimento **não se propaga**. `invisibleUntil` já é um instante absoluto: "no passado" **significa** "visível", em qualquer nó. `deliveryCount` só sobe na **próxima entrega**.

**Relógio.** O prazo é decidido e conferido pelo relógio do **dono**. Uma diferença de relógio entre nós, na troca de dono, encurta ou alonga o prazo pelo tamanho da diferença: aceito no *at-least-once*, e razão para manter os relógios sincronizados (NTP).

**Mudança de dono e reinício.** O novo dono, ao terminar a sincronização, percorre os metadados das mensagens de que passou a ser primário: as com `invisibleUntil` no futuro entram como invisíveis; as demais, como candidatas a prontas. Quem **deixa** de ser dono não limpa nada: o que restar falha na conferência de dono e é descartado.

## Long polling e canal entre nós

Com `waitTime > 0` e nada pronto, a chamada **espera** novas mensagens (até 20 s). Como o tópico está espalhado, o nó precisa saber **onde há mensagens** sem varrer o cluster.

### Os três cenários

Cluster de 3 nós (A, B, C), tópico `pedidos` com 6 mensagens, cada nó primário de duas:

| Cenário | O que acontece |
|---|---|
| **1. Lista local com mensagem** | O `GET` chega a A; A é o dono de m1: registra, commita no quórum e devolve. Nenhum outro nó é consultado. É o caso comum com fila cheia |
| **2. Lista local vazia, outro nó tem** | A esvaziou e avisou "sem prontas em `pedidos`"; B e C têm a tabela `{A: não, B: sim, C: sim}` em memória. A sorteia um (com peso pela quantidade informada), chama `take` em B (`POST /cluster/topics/{topic}/messages/take`); B, o dono, faz o registro + commit e devolve a mensagem a A, que a repassa. Se B já esvaziou, A tenta C, até `delivery.take.max-attempts` (padrão 3) |
| **3. Nenhum nó tem** | A vira **ouvinte local** e avisa os demais, **uma vez**, "`pedidos` tem ouvintes em A". Quando chega mensagem pronta em B, B vê o ouvinte em A e manda "há mensagens novas aqui"; A chama `take` em B e entrega ao ouvinte. Sem mensagem até `waitTime`, responde vazio |

![Cenário 1: lista local](../diagramas/img/pt-BR/consumo-cenario1-lista-local.svg)

![Cenário 2: completa com outro nó](../diagramas/img/pt-BR/consumo-cenario2-outro-no.svg)

![Cenário 3: espera](../diagramas/img/pt-BR/consumo-cenario3-espera.svg)

O custo de saber onde há mensagens é pago só nas **transições** (de "tem" para "não tem"), com sinais pequenos:

| Alternativa | Custo por `GET` com lista local vazia |
|---|---|
| Ler o monitoramento de todos os nós | Consulta a N nós, estado velho ao chegar |
| `GET` em todos, um por um | Até N chamadas por `GET`; C consumidores geram C × N |
| **Sinais nas transições (adotada)** | Consulta local em memória + até `delivery.take.max-attempts` chamadas |

### O canal permanente

Um **WebSocket por par de nós**, compartilhado por **todos** os tópicos (nunca um por chamada de long polling). Só **sinais**, **nunca** o conteúdo das mensagens (esse vai pelas rotas internas, com registro e commit). Enviado só **quando o estado muda**:

| Sinal | Quando |
|---|---|
| **Tópicos com ouvintes** | O tópico ganha o primeiro ouvinte neste nó, ou perde o último |
| **Tópicos com mensagens prontas** | O nó passa a ter prontas para o tópico, ou esvazia |
| **Aviso de mensagem nova** | Chegou mensagem nova pronta no dono, **sem ouvinte local**, e outro nó informou ter ouvinte |

- O aviso é uma **dica**, não garantia: quem recebe vai buscar e pode não achar nada. Um aviso perdido só custa espera até o fim do `waitTime`.
- **Quem envia os avisos:** um responsável por tópico, ativo só enquanto há ouvintes e mensagens novas. **Agrupa** as que ficam prontas numa janela curta (`cluster.notify.window`, padrão 100 ms) em **um aviso** com a quantidade; avisa **um nó por vez**, revezando, e espera `cluster.notify.wait` (padrão 50 ms) para ver se ele vem buscar; se não vem, passa ao próximo.
- **Sem fan-out por chamada:** N ouvintes esperando o mesmo tópico num nó geram **uma** informação de interesse.
- **Sinal de vida:** mensagem própria (não o ping do WebSocket, para detectar nó travado com a conexão aberta), a cada `cluster.channel.keepalive.interval` (padrão 2 s), com data e hora, um resumo de monitoramento (carga do nó), as **quantidades de mensagens prontas por tópico** (até `cluster.channel.keepalive.max-topics`, padrão 1000) e o **estado** do nó. **Não é confirmado nem reenviado**; o seguinte o substitui. Por levar o estado, corrige um `node-state` perdido.
- **Confirmação:** todo sinal **exceto** o de vida leva um número crescente por canal e sentido; o receptor devolve a confirmação depois de **processar**; o emissor só tira da fila de saída ao receber; na reconexão, **reenvia o que ficou sem confirmação**. Um reenvio já processado é reconhecido pelo número e ignorado.
- **Quem abre a conexão:** o nó que **entra ou volta** abre ativamente com todos; um nó só aceita quem consta na sua tabela. Se dois abrirem ao mesmo tempo, **fica a conexão do nó de maior hash do ID**.
- **Estado inicial:** ao abrir o canal, cada lado envia primeiro um **instantâneo** (tópicos com ouvintes, com prontas e os nós em freezing) e só depois as mudanças. Um nó sincronizando só o consome ao ficar pronto.
- **Fila de saída limitada:** sinais de **estado** do mesmo tópico se **substituem** (só o último importa); **avisos** velhos (mais de 10 s) são **descartados**; o `frozen` **não expira**. Passando de `cluster.channel.queue.max-size`/`max-age`, o nó do outro lado é tratado como **falho** e a votação abre ([07-cluster.md](07-cluster.md)).

Em **modo desenvolvimento** não há canal nem `take`: o long polling só espera mensagens chegarem no próprio nó. **Em drain**, o nó responde `take` vazio e os pares deixam de pedir a ele.

## Expiração (`ttl`), limite de entregas e dead-letter

Três regras de retenção valem para toda fila; cada uma tem **padrão do nó** e pode ser definida **por fila** (`POST /topics`, `PATCH`).

| Regra | Campo | Padrão | Faixa | Efeito |
|---|---|---|---|---|
| Expiração | `ttl` (s) | 14 dias | 60 s a 365 d | A mensagem mais velha que `ttl` desde a publicação é **descartada**, entregue ou não |
| Limite de entregas | `maxDeliveries` | 10 | 1 a 1000 | Ao atingir `maxDeliveries` entregas sem ack, vai à **dead-letter** |
| Dead-letter | derivada da fila | `-dead-letter` | — | Recebe as que estouraram `maxDeliveries` |

O padrão vale **na hora da conferência**: quem não definiu o campo acompanha mudanças do padrão; quem definiu mantém o seu.

- **`ttl`:** a idade vem do próprio ID; o prazo usa o `ttl` **atual** da fila (um `PATCH` vale para mensagens já guardadas). Vale **mesmo para mensagem invisível**. A conferência é **local e determinística**, sem propagação: o dono não entrega vencida, e uma tarefa de cada nó apaga as próprias cópias a cada `retention.sweep.interval` (padrão 60 s; a tarefa roda no menor entre ele e 1 s), e também antes de cada `GET`, ack e `PUT`. Vencida, é **excluída, não vai à dead-letter** (métrica `taba.retention.expired`).
- **`maxDeliveries`:** `maxDeliveries: 10` significa **no máximo 10 entregas**; a décima primeira nunca acontece.
- **Dead-letter:** nasce com a fila (na mesma operação), é excluída com ela, **sem definição própria** (somente leitura, `deadLetter: true`, `source`). `PATCH` e `DELETE` nela dão `409`. **Não há dead-letter da dead-letter.** O **movimento** é **uma operação de registro + commit**: cria a mensagem na dead-letter e a remove da origem, com o **mesmo ID** (logo, mesmo bloco e mesmos nós). `deliveryCount` volta a 0, `invisibleUntil` é zerado, e ganha `deadLetteredAt` e `deadLetterReason` (`max-deliveries`). Falha antes do quórum: rollback, e o dono tenta de novo na próxima volta do vencimento.
- **Nome reservado:** não se cria tópico com nome terminado no sufixo (`400`). O nome da dead-letter pode passar de 100 (até 112 com o sufixo padrão).

Exemplo real (fila com `maxDeliveries: 2`, a mensagem entregue duas vezes sem ack e devolvida com `visibilityTimeout: 0`):

```text
GET dl/messages  →  deliveryCount 1
GET dl/messages  →  deliveryCount 2     (depois de o prazo vencer)
PUT .../visibility {"visibilityTimeout":0}   →  200
GET dl/messages  →  {"messages":[]}
GET dl-dead-letter/messages → {"messages":[{… "deadLetteredAt":"…","deadLetterReason":"max-deliveries"}]}
```

## Grupos e entrega justa

O **vizinho barulhento**: um produtor que despeja milhares de mensagens faz as de quem manda poucas esperarem atrás. A mensagem pode levar um **`groupId`** (1 a 128 caracteres, `A-Za-z0-9_-.:`, imutável) e a entrega dá **preferência aos grupos que têm menos mensagens**. Sem `groupId`, a mensagem pertence ao grupo "vazio" e um tópico sem grupos se comporta como a lista de prontos simples. Numa fila **comum** o grupo **não ordena** nada: serve só de chave para a justiça.

**Contagem.** Cada **dono** mantém, por tópico, dois contadores por grupo: **prontas** e **em processamento** (invisíveis). O **tamanho** do grupo é:

```
tamanho = em processamento + peso × prontas        (peso = delivery.fair.ready-weight, padrão 0,1)
```

O peso fica entre o SQS (`0`: só o que está em processamento, que reage tarde a uma rajada) e o backlog inteiro (`1`: penaliza um grupo grande e inofensivo de consumidor lento). A contagem é **local** (sem troca entre nós): como o ID decide o bloco, as mensagens de um grupo se espalham pelos donos de forma proporcional, então cada nó vê uma amostra fiel e a justiça é **aproximada no cluster** e **exata em modo desenvolvimento**. O número de grupos acompanhados tem teto (`delivery.fair.max-groups`); acima dele, os grupos novos **compartilham um contador**.

**Preferência.** O tamanho vira uma **faixa** `⌊log2(tamanho)⌋` (tamanho < 1 conta como 1): 1 → faixa 0, 2–3 → 1, 4–7 → 2… Faixas por potência de 2 evitam reordenar a lista a cada mensagem.

1. A **reposição** percorre os grupos **do menor para o maior** e toma de cada um as prontas mais antigas, **uma por grupo por rodada**.
2. A lista fica ordenada pela **faixa** e, dentro dela, pela sanfona. Uma mensagem nova entra na posição da faixa do seu grupo.
3. A **entrega** tira da menor faixa com prontas; se a faixa do grupo mudou desde que o ID entrou, o ID é reposicionado.
4. **Progresso garantido:** a cada `delivery.fair.normal-every` entregas (padrão **10**), uma pega a **mais antiga de qualquer grupo**, ignorando a faixa. Grupos grandes nunca esperam sem limite.

O `take` e o sorteio com peso entre nós **não mudam**: a justiça é aplicada por cada dono, na hora de escolher o ID. **Na fila FIFO** nada disso vale (a cabeça mais antiga vence). Métricas por grupo: [13-observabilidade.md](13-observabilidade.md).

## Parâmetros

| Chave | Padrão |
|---|---|
| `delivery.visibility-timeout.default` / `.max` | 120 s / sem teto |
| `delivery.max-messages.default` / `.max` | 10 / sem teto |
| `delivery.get.max-attempts` | 3 |
| `delivery.take.max-attempts` | 3 |
| `delivery.fair.ready-weight` / `max-groups` / `normal-every` | 0,1 / sem teto / 10 |
| `retention.ttl.default` / `retention.max-deliveries.default` | 14 d / 10 |
| `retention.sweep.interval` | 60 s |
| `topics.dead-letter.suffix` | `-dead-letter` |
| `cluster.notify.window` / `cluster.notify.wait` | 100 ms / 50 ms |

## Limites conhecidos

- **PRB-019:** achar a mensagem do meio para rebaixar e conferir as prontas quando `maxDeliveries` muda varrem a memória do tópico.
- **D96:** quem fixa o prazo do `PUT visibility` e do ack, e o prazo do repasse, ainda tem pontos em aberto.
- **D109:** as constantes do canal (10 s de validade do aviso, 3 `not-member` seguidos) estão no código.
- **D113:** o recibo da FIFO carrega a posição do grupo (decidido na implementação).
- Parâmetros sem efeito hoje: `delivery.ready-list.size`, `delivery.invisible-list.size`, `delivery.invisible.reload-threshold`.

## Fontes e provas

- `engine/messages/mod.rs` (publicar, consumir, ack, vencimento), `engine/messages/index/mod.rs` (listas, faixas, sanfona), `cluster/replication/messages_api.rs` e `signals.rs` (`take`, sinais), `cluster/channel` (canal e fila de saída), `http/messages`.
- Respostas dos exemplos copiadas de um nó real.
- Casos de uso: UC-CNS (consumo), UC-ACK, UC-VIS, UC-RET (`ttl`, entregas e dead-letter), UC-GRP (grupos), UC-ENT (entrega no dono), UC-CAN (canal), UC-INV; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/messages.rs`, `listeners.rs`, `channel.rs`, `notify.rs`.
- Histórico: `13-entrega-de-mensagens.md`, `14-invisibilidade.md`.
