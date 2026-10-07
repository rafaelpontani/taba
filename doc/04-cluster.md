# Protocolo de cluster

[← Índice](README.md)

## 6. Protocolo de cluster: descoberta, registro e falhas
 
Decisão de arquitetura definida para a comunicação e o gerenciamento de membros do cluster.
 
Registro e desregistro de nó são operações **pesadas** (afetam o roteamento do anel inteiro), por isso ambos usam o **mesmo protocolo em duas fases (registro → commit)** definido para mensagens na seção 7.2 — garantindo que a operação só é efetivada quando **todos** os nós confirmam. Diferente das mensagens, aqui **não há quórum reduzido**: são operações raras, sem o caminho crítico de latência que justifica decidir o commit antes de todas as confirmações.
 
### 6.1 Comunicação entre nós
- A comunicação entre nós é feita via **API HTTP**.
- Além das chamadas HTTP, cada nó mantém um **canal permanente WebSocket com cada um dos outros nós** (malha completa: um por par, para todos os tópicos): carrega o **sinal de vida** entre nós (seção 6.3) e os sinais de disponibilidade da entrega de mensagens (long polling, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8). O canal transporta **somente sinais pequenos** — nunca o conteúdo das mensagens nem listas longas —, para ficar livre e rápido.
### 6.2 Registro de um novo nó (bootstrap)

![Sequência do ingresso de um novo nó](diagramas/img/ingresso-no.svg)

*Fonte: [ingresso-no.json](diagramas/fonte/ingresso-no.json). A sincronização do bloco está na seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md).*

1. **Descoberta.** A configuração exige o **endereço do balanceador do cluster** (`cluster.entry-address`, obrigatório e sem padrão; se faltar, o nó não sobe). A primeira coisa que o nó faz ao iniciar é chamar, por esse endereço, a listagem de nós (`GET /cluster/nodes`), que devolve a tabela de membros com os **IPs internos** de cada um. Com a lista, o nó escolhe um membro ao acaso e envia a ele, **numa única requisição síncrona** (`POST /cluster/nodes/join`), o pedido de registro com **seus próprios IPs e porta**; esse nó passa a coordenar o registro dos passos 2 a 5. Se o membro não responde, tenta outro da lista. Vale também para o reinício de um nó, que volta como nó novo (seção 6.8). Se a listagem falha, o nó tenta de novo conforme `cluster.bootstrap.retry.interval`; só o **primeiro nó** de um cluster novo (`cluster.first-node=true`, padrão `false`), quando a listagem não encontra ninguém, inicia o cluster sozinho como único membro.
2. O nó que recebe a chamada **testa os IPs** informados, tentando estabelecer conexão de volta com o novo nó.
3. Se a conexão for bem-sucedida, o nó que recebeu envia a **todos os nós do cluster** um pedido de **registro (fase 1)** com os dados do novo nó.
4. Após todos confirmarem a fase de registro, o nó que recebeu envia a **todos os nós** uma chamada de **commit (fase 2)**, efetivando o novo nó na tabela de membros.
5. Só então **responde** à chamada original do novo nó com a **tabela de nós** (já com o commit confirmado em todo o cluster). A **tabela de tópicos** não vai nessa resposta, porque cresce com o número de tópicos: o novo nó a puxa de um nó do cluster pelo stream `GET /cluster/topics` ([02-arquitetura.md](02-arquitetura.md), seção 3.3).
6. Ao final do processo, o novo nó já conhece todos os demais membros do cluster e vice-versa, e o novo nó abre ativamente o canal permanente com cada nó da tabela (seção 6.1). Quando um nó cai e reinicia, é sempre ele quem se conecta de volta aos outros, pelos IPs que conhece da tabela de membros.
7. **Falha antes do commit** (teste de conexão do passo 2, ou fase de registro sem confirmação de todos): o processo é abortado (rollback do registro pendente) e o novo nó é **excluído** — ver seção 6.5. **Falha no commit** (fase de registro já confirmada por todos): a decisão já foi tomada e não é desfeita — o nó que coordena insiste no commit dentro do teto `write.commit.timeout` até confirmar em todos, em vez de abortar o novo nó, pois algum membro do cluster já pode tê-lo efetivado na tabela.
8. Ao assumir sua posição no anel (seção 7), o novo nó passa a ser responsável por blocos de mensagem como primário, secundário, terciário etc., conforme o fator de replicação configurado — seguindo o procedimento de sincronização distribuída da seção 7.3. Depois de pronto, passa a ser também **nó gestor** das filas voláteis cujo `id` cai na sua faixa do anel ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
9. **Prontidão.** O novo nó só fica **pronto** (`/health/ready`) quando termina toda a sincronização da seção 7.3: tópicos, blocos e o processamento do banco de eventos recebidos nesse intervalo. Até lá ele consta na tabela e confirma registros gravando-os no banco de eventos, mas não atende clientes, não coordena e não entrega mensagens.
### 6.3 Detecção de falhas e desregistro

![Sequência da remoção de um nó](diagramas/img/remocao-no.svg)

*Fonte: [remocao-no.json](diagramas/fonte/remocao-no.json).*
 
1. Cada par de nós mantém um **canal permanente** (seção 6.1), que também carrega um **sinal de vida** leve (keepalive), uma mensagem própria com data e hora, resumo de monitoramento e as mensagens prontas por tópico (seção 12.8 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). O controle de quem está online é feito por esse canal, em malha, sem coordenador — **não há ping separado**.
2. Quando o canal com um nó **cai ou fica sem sinal de vida**, o nó que detectou **tenta reconectar algumas vezes** (tentativas e intervalo configuráveis) antes de agir.
3. Se o canal continuar fora após as tentativas, isto é, o nó ficou inacessível por tempo demais, é aberta uma **votação de exclusão** entre os nós do cluster.
4. **Quórum de votação: 51% dos nós saudáveis** (`cluster.vote.quorum-percent`, configurável), com um mínimo de votantes (`cluster.vote.min-voters`, padrão 2). Nó **saudável** é o que o nó que abre a votação enxerga com o canal ativo e que responde ao pedido de voto. Os nós fora do ar, os suspeitos, **não votam e não entram na base**: se vários caem ao mesmo tempo, por exemplo uma zona com problema, a votação segue só entre os saudáveis, e são exigidos `ceil(S × 51%)` votos de S saudáveis (por exemplo, 2 de 3, 3 de 4 ou 5). Se há menos saudáveis que o mínimo de votantes, nenhuma exclusão se efetiva (condição `quorum-lost`). Com várias exclusões pendentes, elas são votadas uma de cada vez (seção 6.8), cada uma sobre os saudáveis do momento. **Risco aceito:** como a base é só quem o votante enxerga, uma partição de rede em que os dois lados têm saudáveis suficientes pode levar cada lado a excluir o outro; o mínimo de votantes impede só que um nó isolado exclua todos os demais. O lado excluído, ao voltar, refaz o bootstrap do zero (seção 6.6).
   - **Evolução futura (fora do escopo atual):** base na tabela inteira (`total`), como o MongoDB; rota manual de exclusão forçada para quando a maioria se perde; e época na tabela de membros, com desempate ao fim da partição (o grupo maior vence, e o perdedor refaz o bootstrap). Enquanto isso, quem opera muitos nós pode subir `cluster.vote.min-voters` para uma maioria do cluster e assim reduzir o risco de partição.
5. Atingido o quórum, o desregistro segue o **mesmo protocolo de duas fases**: um pedido de **desregistro (fase 1)** é enviado a todos os nós e, em seguida, um **commit (fase 2)** efetiva a remoção do nó da tabela de membros em todo o cluster. Mesma regra de falha da seção 7.2: antes do commit, rollback; depois de decidido, sem rollback — insiste dentro do teto `write.commit.timeout` até confirmar a remoção em todos.
> No commit do desregistro, quem passa a ser primário das posições do nó removido assume as **filas voláteis** que ele gerenciava, sem transferir estado ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
>
> Enquanto a votação não conclui, os sinais para o nó ficam acumulados em filas em memória; se o canal reconecta antes disso, nada muda e o envio é retomado. Se a fila estourar o limite, o nó é tratado como falho e a votação é aberta. Concluída a exclusão, todos os nós descartam as filas para aquele nó ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9).
### 6.4 Tamanho mínimo do cluster
| Ambiente | Nós mínimos | Racional |
|---|---|---|
| Teste / desenvolvimento | 1 | Permite rodar localmente sem overhead de cluster. |
| Produção | 3 | Permite votação com um nó fora do ar (2 votantes saudáveis) e evita decisão de falha por um único nó. |
 
### 6.5 Falha no registro de um novo nó

![Sequência da falha no registro de um novo nó](diagramas/img/falha-registro-no.svg)

*Fonte: [falha-registro-no.json](diagramas/fonte/falha-registro-no.json).*
 
Se o registro de um novo nó **falhar antes do commit** (teste de conexão do bootstrap malsucedido, ou a fase de registro sem confirmação de todos os nós, dentro de `write.commit.timeout` — seção 6.2, passo 7):
 
1. O nó é **excluído** do cluster — não chega a ser efetivado na tabela de membros.
2. Os **blocos** que seriam assumidos por ele são **rebalanceados** entre os nós restantes do cluster (o anel de hash é recalculado sem esse nó).
3. **Nova tentativa:** o nó repete o bootstrap do começo (seção 6.2) a cada `cluster.bootstrap.retry.interval`, até `cluster.bootstrap.retry.max-attempts` tentativas (padrão sem limite). O mesmo intervalo vale quando a listagem de nós, o pedido de ingresso ou a sincronização falham.
> Diferente da falha descrita na seção 7.3 (mensagem não encontrada durante a sincronização de um bloco), a falha de registro é sobre o **nó como um todo** não conseguir entrar no cluster — por isso o efeito é exclusão + rebalanceamento, e não apenas um alerta.
 
### 6.6 Retorno de um nó excluído

![Sequência do retorno de um nó excluído](diagramas/img/retorno-no-excluido.svg)

*Fonte: [retorno-no-excluido.json](diagramas/fonte/retorno-no-excluido.json).*
 
Quando um nó que foi **desregistrado por falha do canal** (seção 6.3) volta a ficar disponível, ele não simplesmente retoma seu lugar antigo no anel — os blocos que ele atendia já podem ter sido reatribuídos a outros nós. O procedimento recupera primeiro o que o nó tem e pode ajudar, e só depois refaz a entrada:

1. **Lista de perdas.** O nó consulta **todos os nós que consegue alcançar** (`GET /cluster/message-loss`, um stream NDJSON) e une as respostas. Cada nó responde com a sua tabela `perdas` (abaixo): os pares (tópico, ID) que alguma sincronização declarou perdidos (seção 7.3). Essas rotas aceitam chamadas de um nó que **ainda não é membro**, já que ele foi excluído.
2. **Confronto com o que o nó tem.** Para cada par, o nó procura o ID no que guarda localmente: no disco (`${topico}/${id}.json` e o banco do tópico) e, se o processo é o mesmo que ficou fora (sem reinício), também em memória. Num reinício, só o que está em disco conta.
3. **Envio.** Para cada mensagem encontrada, o nó a envia ao nó responsável pelo bloco do ID, pela rota de mensagens recuperadas (`POST /cluster/messages/recover`, um stream NDJSON). Quem recebe grava **direto nos nós responsáveis**, idempotente pelo ID e sem as duas fases (a mensagem não existe nos donos, ninguém concorre por ela), e confirma a linha quando `write.quorum` nós gravaram. Vai o corpo e o `deliveryCount`; a invisibilidade é zerada, e a mensagem volta a ficar pronta para entrega.
4. **Baixa do registro.** O nó que aceita a mensagem apaga o par da sua tabela `perdas` e avisa os demais para apagarem o deles (melhor esforço), para que um segundo nó que volte com a mesma mensagem não a reenvie depois de ela já ter sido entregue e confirmada.
5. **Estado local em quarentena.** Enquanto a recuperação não termina, o nó **não apaga** os dados antigos. Se algum nó não respondeu na consulta do passo 1, o nó tenta de novo a cada `cluster.bootstrap.retry.interval`, até `cluster.recovery.timeout`; só então segue.
6. **Limpeza e novo registro.** Concluída a recuperação (ou esgotado o prazo), o nó **limpa seu estado local** e **inicia um novo registro no cluster do zero** — o bootstrap completo da seção 6.2, como se fosse um nó novo, com uma nova posição no anel. O freezing (seção 6.7) só atrasa este passo; a recuperação dos passos 1 a 5 não depende dele.

**Tabela de perdas.** Cada nó guarda no banco do nó (`${storage.dir}/_node.db`, SQLite, seção 3.2 de [02-arquitetura.md](02-arquitetura.md)) a tabela `perdas` (`topico`, `id`, `bloco`, `detectado_em`), com chave (`topico`, `id`). O nó que detecta a perda na sincronização grava o registro e o envia também aos nós responsáveis pelo bloco (idempotente pela chave), para a informação sobreviver à queda do detector. Os registros expiram depois de `cluster.message-loss.ttl`.

### 6.7 Período de freezing (anti-flapping)
 
Para evitar que um nó instável/defeituoso entre e saia repetidamente do cluster (flapping), existe um **período de freezing**:
 
- Após um nó ser desregistrado por falha (seção 6.3), ele fica **impedido de se registrar novamente** no cluster até esse período expirar. A **duração é configurável** (`cluster.freezing.duration`).
- **Identidade do nó:** ao nascer (a cada início do processo), o nó se **auto-nomeia com um ID GUID v7**, guardado só em memória, e é por esse ID que se identifica perante os outros nós, inclusive no pedido de ingresso. O cluster guarda o ID e o instante do desregistro de cada nó em freezing.
- **A quem se aplica:** ao nó que foi excluído pela votação mas **continua o mesmo processo**, com a mesma memória e o mesmo ID, e fica caindo e voltando (por exemplo, por falha de rede), tentando se reconectar e se registrar sem reiniciar. Um **reinício** do processo é outra coisa: o nó nasce com um ID novo e entra como **nó novo**, sem freezing, e só aproveita o que está em disco para ajudar a recuperar mensagens perdidas (seção 6.6).

- **Durante o freezing, o pedido de ingresso é rejeitado** (não há lista de espera, e o cluster não guarda o pedido). A resposta traz `Retry-After` com o tempo que falta, e o nó volta a tentar depois desse prazo, dentro do laço de nova tentativa do bootstrap (`cluster.bootstrap.retry.interval`, seção 6.5). A rejeição é registrada no log ("rejeitado (freezing)").

### 6.8 Concorrência entre registro e desregistro

Registro (bootstrap) e desregistro (votação de exclusão) são as duas operações pesadas do cluster (seção 6). Regra de concorrência:

- **Uma mudança de membros por vez, até o nó ficar pronto:** depois que um nó entra (commit do registro, seção 6.2), nenhuma outra **entrada, saída (desregistro) ou rebalanceamento** é efetivada até ele ficar **pronto**, isto é, terminar toda a sincronização da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md). Pedidos de bootstrap e votações de exclusão que chegarem nesse intervalo **esperam**. Isso vale também enquanto uma votação de exclusão está em andamento (próximo item), então só há uma mudança de membros em curso por vez no cluster. A única exceção é o **próprio nó que está se sincronizando**: se ele entra em falha (seção 7.3, passo 6), a votação do desregistro dele é liberada.
- **Votação de exclusão em andamento:** enquanto uma votação de exclusão de algum nó está em andamento (da abertura até o commit do desregistro, seção 6.3), um **novo pedido de bootstrap fica em espera**; só é processado quando a votação terminar (com ou sem exclusão efetivada).
- **Por quê:** evita que um nó novo assuma posições no anel (seção 7) calculadas sobre uma tabela de membros que está prestes a mudar, ou que os blocos de um nó em sincronização mudem no meio da carga, o que exigiria recalcular o rebalanceamento em cima de outro em andamento. O custo é aceito: um nó que falha (cai) durante a sincronização de outro só é desregistrado depois que esta termina ou falha.

**Caso do nó que reinicia sem memória antes da votação concluir** (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), D29.6): o processo perdeu o estado e volta com o **mesmo IP e porta**, mas o nó ainda consta na tabela em votação de exclusão. O pedido de bootstrap dele entra na fila e espera o fim da votação, como qualquer outro. Encerrada a votação (o nó antigo é removido da tabela), o pedido é processado como um **registro comum de nó novo** (seção 6.2), sem tratamento especial por ter o mesmo endereço.


### 6.9 Repasse das chamadas de cliente durante a sincronização

Enquanto um nó que entrou ainda não está íntegro (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)), ele não tem a tabela de tópicos completa nem coordena nada, mas pode receber chamadas de cliente (antes de o `/health/ready` tirá-lo da rotação, ou de quem fala direto com ele). Em vez de devolver erro, ele **repassa a chamada a outro nó**. Vale só para as **rotas de cliente**; as rotas de sincronização e de comunicação entre nós não são repassadas. Como a API não tem estado preso a um nó, o resultado é o mesmo em qualquer um, e o repasse dura só o tempo da sincronização, que é de um nó por vez (seção 6.8).

- **Destino:** um membro da tabela com canal ativo, escolhido ao acaso. Como só há um nó em sincronização por vez, os demais estão prontos.
- **Repasse transparente:** método, caminho, query, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu (inclusive `409`, `413` e `429`). Vale para todas as rotas de cliente, também o `GET` de mensagens com long polling e `autoCreate`.
- **Sem laço:** o repasse leva um marcador interno (`X-Taba-Forwarded`), aceito só de membros da tabela. Quem o recebe nunca repassa de novo; se estiver sincronizando, responde `503`.
- **Falha do destino:** antes de o corpo ser enviado (conexão recusada), tenta outro nó, até um teto pequeno de tentativas. Depois de enviado, **não repete**: um `POST` de mensagem repetido duplicaria a publicação, já que o ID é gerado no coordenador. Responde `502` ou `504` e o cliente decide.
- **Rate limit e IP:** o limite é aplicado uma vez, no nó que recebeu a chamada, por IP do cliente ([12-limites-api.md](12-limites-api.md)); o destino não conta de novo a chamada marcada. O IP do cliente já resolvido segue em cabeçalho interno.
- **Sem destino:** se nenhum outro nó responde, `503` com `Retry-After`.
