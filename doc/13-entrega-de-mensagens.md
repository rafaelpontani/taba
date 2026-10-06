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

- **Ack** (`DELETE /topics/{topic}/messages/{id}`): remove a mensagem, e com ela seus metadados.
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
- **Lista vazia ou dono indisponível:** se a lista do nó que recebeu o `GET` está vazia, ou se o dono de parte das mensagens está fora do ar e ainda não foi desregistrado, o nó pode **buscar mensagens de outro dono**, aceitando a rara disputa, que o registro exclusivo resolve. *(Os detalhes desse fluxo estão em aberto — ver [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md).)*

**Alternativa descartada — nó controlador por tópico:** um nó eleito com a lista de entrega do tópico daria ordem total, mas todo `GET` do tópico passaria por ele (gargalo em tópico quente), exigiria eleição de líder (o cluster só tem votação de exclusão) e contraria o desenho de nós idênticos, sem mestre fixo (seção 3).
