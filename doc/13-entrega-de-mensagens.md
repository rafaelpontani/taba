# Entrega de mensagens

[← Índice](README.md)

## 12. Controle de entrega (invisibilidade)

A entrega segue o modelo de **invisibilidade temporária**, como o *visibility timeout* do SQS: ao entregar uma mensagem, o sistema a torna **invisível** por um tempo. Se o consumidor a confirma (ack) nesse prazo, ela é removida; se não confirma, o prazo expira e a mensagem volta a ficar disponível para entrega. A semântica resultante é **at-least-once**: uma mensagem pode ser entregue mais de uma vez, mas não se perde por falta de confirmação.

> Os nomes de parâmetros, rotas e arquivos abaixo são **propostas**; ainda não há contrato definido (mesma ressalva de [10-configuracao.md](10-configuracao.md)). Pendências deste tema: [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md).

### 12.1 Peças do controle

| Peça | Papel |
|---|---|
| **Invisibilidade** | Prazo (`invisibleUntil`) até o qual a mensagem não é entregue a outro consumidor. Propagada a todos os nós responsáveis pela mensagem antes de ela ser entregue. |
| **Contador de entregas** | `deliveryCount`, incrementado a cada entrega. Base para reentrega e, futuramente, dead-letter. |
| **Metadados da mensagem** | `deliveryCount` e `invisibleUntil`, guardados junto da mensagem em memória ou em disco (seção 12.5). |
| **Dono da entrega** | O nó primário do bloco da mensagem: só ele a coloca na sua lista de prontos (seção 12.7). |
| **Lista de prontos** | Por tópico, em cada nó: IDs prontos para serem entregues, mantida por uma thread (seção 12.6). |

### 12.2 Entrega (`GET`)

O cliente informa a invisibilidade desejada na query string: `GET /topics/{topic}/messages?visibilityTimeout=120`. O valor é em segundos; **o padrão é 120 s** (`delivery.visibility-timeout.default`).

O nó que atende o `GET` usa o mesmo protocolo em duas fases da escrita (seção 7.2), agora para a invisibilidade:

1. Escolhe um ID da **lista de prontos** do tópico (seção 12.6), que contém só mensagens de que ele é o dono da entrega (seção 12.7).
2. Calcula, pelo ID, os **nós responsáveis** pelo bloco (primário + réplicas), como na seção 7.2.
3. **Fase 1 (registro):** envia a todos eles o pedido de marcar a mensagem como invisível até `agora + visibilityTimeout`, com `deliveryCount + 1`. Cada nó grava a alteração como **pendente** e confirma.
4. **Fase 2 (commit):** com todos os registros confirmados, o coordenador do `GET` chama o commit em todos os nós. Cada nó efetiva a nova invisibilidade e o novo contador.
5. **Só depois** de o commit ser confirmado a mensagem é **devolvida ao cliente**.
6. Ao receber o commit, cada nó **remove o ID da sua lista de prontos** do tópico — a mensagem deixa de ser candidata a nova entrega em qualquer réplica.
7. Falha no registro ou no commit: retry, e esgotado, **rollback** para descartar a alteração pendente. Mesmos parâmetros de retry e de expiração de pendente da escrita (`write.retry.*`, `write.pending.ttl`).

A mensagem só chega ao cliente depois de a invisibilidade estar marcada em **todos** os nós responsáveis; assim, uma queda do coordenador logo após a entrega não faz outro nó entregá-la em paralelo.

### 12.3 Alteração da invisibilidade

O cliente pode **alterar e acrescentar** tempo de invisibilidade de uma mensagem já entregue — por exemplo, um consumidor que precisa de mais tempo para processá-la:

| Verbo | Recurso | Descrição |
|---|---|---|
| `PATCH` | `/topics/{topic}/messages/{id}` | Altera a invisibilidade da mensagem: `{"visibilityTimeout": 300}`. |

A alteração é propagada com o **mesmo processo**: pedido de alteração (registro) a todos os nós responsáveis e, com todos confirmando, commit. Só então a chamada responde ao cliente. Esta operação **não** incrementa `deliveryCount` (não é uma nova entrega) e **não** mexe na lista de prontos.

### 12.4 Confirmação (ack) e reentrega

- **Ack** (`DELETE /topics/{topic}/messages/{id}`): segue o **mesmo protocolo de registro + commit** das demais operações sobre a mensagem (entrega, seção 12.2; escrita, seção 7.2). O coordenador do `DELETE` pede o registro da remoção a todos os nós responsáveis pelo bloco; só com todos confirmando ele chama o commit, que remove a mensagem e seus metadados em cada nó — **sem deixar tombstone**: a ausência do ID já é suficiente, e a sincronização de bloco trata a janela residual de uma remoção concorrente por conta própria (verificação final, seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)).
- **Falha:** se o registro ou o commit não forem confirmados por todos os nós, a operação é **cancelada** (rollback, mesma regra da seção 3.1) e a API retorna **erro ao cliente** — a mensagem continua disponível, nunca um ack parcial.
- **Reentrega:** se o prazo `invisibleUntil` passa sem ack, a mensagem volta a ser elegível. *(Proposta: a thread da lista de prontos, seção 12.6, também re-inclui as mensagens cuja invisibilidade expirou.)* Ao ser entregue de novo, `deliveryCount` sobe.

### 12.5 Metadados em memória e em disco

Os metadados são `deliveryCount` (incremental) e `invisibleUntil` (data e hora).

- **Mensagem em memória:** os metadados ficam em memória junto dela.
- **Mensagem em disco** (overflow, seção 3.2): os metadados ficam em **outro arquivo, com quase o mesmo nome e sufixo de metadado**, ao lado do arquivo da mensagem:

  ```
  ${topico}/${id}.json          # a mensagem
  ${topico}/${id}.meta.json     # metadados de entrega
  ```

- O arquivo de metadados é atualizado no **commit** de cada entrega ou alteração e removido junto com a mensagem no ack.
- Assim, alterar a invisibilidade de uma mensagem em disco reescreve só o arquivo pequeno de metadados, sem tocar no conteúdo da mensagem.

### 12.6 Lista de prontos

- Cada nó mantém, **por tópico**, uma lista de **IDs de mensagens prontas para entrega** — somente as de que ele é o **dono da entrega** (seção 12.7).
- Uma **thread** dedicada é responsável por **manter sempre um número fixo de IDs prontos** (`delivery.ready-list.size`), repondo a lista conforme ela é consumida.
- A lista é reposta **na ordem do GUID v7** (ordem de publicação) das mensagens de que o nó é dono.
- Ao receber o **commit** de uma entrega (seção 12.2, passo 4), o nó remove o ID da lista automaticamente.
- O `GET` só consulta a lista, sem varrer memória e disco a cada chamada; o custo da busca fica na thread de reposição, fora do caminho crítico da entrega.

### 12.7 Dono da entrega

As mensagens de um tópico se espalham por vários blocos e nós (seção 7.2), e vários nós têm cópia de cada mensagem. Se todas as réplicas entregassem, elas disputariam o commit de invisibilidade da mesma mensagem, com falhas e retries que aumentariam a latência do `GET` quando há muitos consumidores. Por isso cada mensagem tem **um único dono da entrega: o nó primário do bloco dela**.

**Como cada nó sabe de quais mensagens é dono, sem conhecer o tópico inteiro:**

1. O ID da mensagem vira um hash, que cai numa posição do anel (seção 7.1), que pertence a um bloco, que tem seus nós responsáveis (primário, secundário, terciário...).
2. O cálculo só depende do ID e da tabela de membros, que todos os nós têm (seção 6). Qualquer nó chega ao mesmo primário para o mesmo ID.
3. Cada nó guarda apenas mensagens dos blocos em que é responsável. A thread da lista de prontos calcula o papel do nó em cada mensagem local (uma busca no anel) e inclui na lista só as que ele tem como **primário**.
4. O tópico completo é a **união das listas** dos nós; nenhum nó precisa montá-la.

**Efeitos:**

- Em condições normais, o commit de entrega de uma mensagem só é tentado pelo dono, sem disputa entre nós. Os nós virtuais equilibram a quantidade de mensagens de que cada nó é primário.
- A ordem de entrega é a do GUID v7 **dentro da lista de cada nó**; não há ordem global no tópico.
- **A partição não é o que garante a correção.** O registro de invisibilidade continua sendo exclusivo: um nó rejeita o pedido se já há outro pendente ou se a mensagem já está invisível, e o `GET` tenta outro ID. A divisão por dono só evita a disputa; se ela acontecer (por exemplo, com tabelas de membros divergentes por instantes), nenhuma mensagem é entregue em duplicidade.
- **Mudança no anel:** quando o primário de um bloco muda (entrada, saída ou rebalanceamento), a thread refaz o filtro dos blocos afetados. Um bloco em sincronização não entrega (seção 7.3).
- **Lista vazia:** se a lista do nó que recebeu o `GET` está vazia, ele pede mensagens ao dono de outro nó e, com long polling, aguarda novas mensagens — ver seção 12.8.
- **Dono indisponível:** se o dono de parte das mensagens está fora do ar e ainda não foi desregistrado, essas mensagens ficam sem entrega até o desregistro. *(Se outro nó pode assumir nesse intervalo está em aberto — ver [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md).)*

**Alternativa descartada — nó controlador por tópico:** um nó eleito com a lista de entrega do tópico daria ordem total, mas todo `GET` do tópico passaria por ele (gargalo em tópico quente), exigiria eleição de líder (o cluster só tem votação de exclusão) e contraria o desenho de nós idênticos, sem mestre fixo (seção 3).

### 12.8 Long polling e canal entre nós

**Long polling.** O `GET` aceita `waitTime`, em segundos, de **0 a 20** (padrão 0: responde na hora). Com `waitTime > 0`, se não houver mensagem para entregar, a chamada **aguarda novas mensagens** até esse tempo e responde vazia se nenhuma chegar. `GET /topics/{topic}/messages?visibilityTimeout=120&waitTime=20`.

**Fluxo do `GET` quando o nó não tem mensagem pronta:**

1. O nó consulta o que sabe em memória (ver "Canal entre nós" abaixo) sobre **quais outros nós têm mensagens prontas** do tópico e pede a eles, **uma única vez**, que entreguem (rota interna `POST /cluster/topics/{topic}/messages/take`). Quem entrega é sempre o **dono da mensagem**: ele escolhe o ID da sua lista de prontos, faz o registro + commit de invisibilidade (seção 12.2) e devolve a mensagem ao nó que pediu, que a repassa ao cliente. Assim a mensagem continua tendo um único coordenador.
2. Se nenhum nó tem mensagem, o `GET` entra como **ouvinte local** do tópico (fila de espera em memória, na ordem de chegada) até `waitTime`.
3. Quando chega uma **mensagem nova pronta no próprio nó**, ela vai direto ao primeiro ouvinte local, sem tráfego entre nós.
4. Quando o aviso vem de **outro nó** (abaixo), o ouvinte repete o passo 1 apenas com aquele nó. Se a mensagem já foi levada por outro, ele volta a esperar.
5. Esgotado `waitTime`, responde vazio. Se o cliente desconectar antes, o ouvinte é removido, liberando o recurso.

**Canal entre nós.** Entre cada par de nós existe **um canal de comunicação permanente** (uma conexão **WebSocket**, em malha completa: cada nó conectado a todos os outros), único e compartilhado por todos os tópicos (nunca uma conexão por chamada de long polling nem por tópico). Por ele cada nó informa aos outros, **somente quando o estado muda** (não a cada chamada):

| Informação | Quando é enviada |
|---|---|
| **Tópicos com ouvintes** | O tópico passa a ter ouvintes neste nó (0 → 1) ou deixa de ter (→ 0). |
| **Tópicos com mensagens prontas** | O nó passa a ter mensagens prontas para o tópico (vazio → não vazio) ou esvazia. |
| **Aviso de mensagem nova** | Chegou uma mensagem nova pronta no nó dono e **não há ouvinte local**, mas outro nó informou ter ouvinte no tópico: envia a ele "há mensagens novas do tópico X aqui, venha buscar". |

Cada nó mantém em memória, por tópico, quais nós têm ouvintes e quais têm mensagens prontas; com isso, a decisão de a quem pedir ou avisar é local, sem consultar ninguém.

- O aviso é uma **dica**, não uma garantia: quem recebe vai buscar, e pode não encontrar nada. Um aviso perdido só custa espera até o fim do `waitTime` (no máximo 20 s), e o cliente repete o `GET`.
- **Sem fan-out por chamada:** N ouvintes esperando o mesmo tópico em um nó geram uma única informação de interesse; ao receber um aviso, o nó busca de uma vez tantas mensagens quantos são os ouvintes que estão esperando.
- **Custo de conexões:** uma conexão por par de nós, a mesma malha usada para o sinal de vida (seção 6.3), independente do número de tópicos e de consumidores.
- O canal é usado só para **sinais**: sinal de vida, estado de ouvintes e de mensagens prontas, e avisos. **Nunca** trafega o conteúdo das mensagens nem listas longas, para ficar livre e rápido; o conteúdo continua indo pelas rotas internas, com registro e commit (seções 12.2 e 7.2). O estado é enviado em **mudanças pontuais** (tópico X ganhou ou perdeu ouvinte), não como listas completas.

### 12.9 Filas do canal, reconexão e exclusão de nó

Cada nó mantém **em memória**, para cada outro nó, duas filas:

| Fila | Conteúdo |
|---|---|
| **Fila de saída** (por nó de destino) | Sinais a enviar ao outro nó: sinal de vida, mudanças de estado (ouvintes, mensagens prontas) e avisos. Uma thread de escrita esvazia a fila pelo WebSocket, sem bloquear quem produz os sinais. |
| **Fila de entrada** (por nó de origem) | Sinais recebidos, ainda a processar. O processamento é separado da leitura do socket, para o canal nunca esperar por trabalho lento. |

**Queda e reconexão:**

- Quando o WebSocket cai, os sinais **continuam acumulando na fila de saída** daquele nó e o nó tenta reconectar (seção 6.3).
- Se o canal **reconecta antes** de a votação de exclusão ser concluída, **nada muda**: os nós retomam o envio dos sinais pendentes, de onde pararam.
- Se a votação de exclusão for **concluída**, todos os nós **descartam as filas** de comunicação com aquele nó e param de acumular sinais para ele. Se o nó voltar, refaz o bootstrap do zero (seção 6.6), com filas novas e vazias.

**Retomada após a queda — número de sequência (proposta, em aberto):** uma conexão TCP/WebSocket que cai não informa ao emissor quais sinais o receptor chegou a processar: um sinal pode ter sido escrito no socket e perdido no caminho. Sem controle, o emissor precisa escolher entre reenviar tudo (duplicatas, inofensivas porque os sinais são idempotentes) ou dar por entregue o que só foi escrito (risco de perder, por exemplo, "o tópico X passou a ter ouvintes"). Com um **número de sequência por canal** e uma confirmação periódica do receptor, o emissor reenvia exatamente do primeiro sinal não confirmado e o receptor ignora números já processados. É um contador e uma confirmação, sem custo relevante de tráfego.

**Quem abre a conexão:**

- O nó que **entra ou volta** é sempre o que **se conecta ativamente** aos outros. No bootstrap de um nó novo, ele abre o canal com cada nó da tabela que recebeu como resposta (seção 6.2); os demais apenas aceitam. Um nó só aceita conexão de quem consta na sua tabela de membros.
- **Queda com os dois nós vivos** (falha de rede, sem reinício): o nó que **detectar primeiro** tenta reconectar. Se os dois acabarem abrindo uma conexão ao mesmo tempo, **o nó de menor hash do ID fecha a conexão que ele abriu** e fica com a aberta pelo outro: regra de desempate, para haver sempre um só canal por par.
- **Reinício do processo:** o nó perde a memória, inclusive a tabela de membros (ela **não é persistida**), e volta como **nó novo**, passando pelo registro (seção 6.2). Os dados que estavam em disco seguem o procedimento de retorno da seção 6.6: as mensagens são enviadas a quem as perdeu.

**Limite e juntada dos sinais na fila de saída:** enquanto o canal está fora e a votação não conclui, a fila de saída é mantida pequena assim:

- Sinais de **estado** são idempotentes (ex.: "o tópico X tem ouvintes"): só o **último de cada tópico** importa, então um novo sinal **substitui** o anterior ainda na fila em vez de se acumular.
- **Avisos** de mensagem nova antigos são **descartados**: perderam o sentido e o ouvinte já foi atendido ou expirou.
- A fila tem **tamanho e idade máximos** (`cluster.channel.queue.max-size`, `cluster.channel.queue.max-age`). Se mesmo assim passar do limite, o nó com quem o canal está fora é tratado como **falho**: abre-se a votação de exclusão (seção 6.3), sem esperar o fim das tentativas de reconexão. Não há ressincronização do estado do canal — o nó excluído, ao voltar, faz um **novo registro** e sincroniza do zero.
- A **fila de entrada** de um nó excluído é descartada junto com a de saída.
