# Protocolo de cluster

[← Índice](README.md)

## 6. Protocolo de cluster: descoberta, registro e falhas
 
Decisão de arquitetura definida para a comunicação e o gerenciamento de membros do cluster.
 
Registro e desregistro de nó são operações **pesadas** (afetam o roteamento do anel inteiro), por isso ambos usam o **mesmo protocolo em duas fases (registro → commit)** definido para mensagens na seção 7.2 — garantindo que a operação só é efetivada quando **todos** os nós confirmam. Diferente das mensagens, aqui **não há quórum reduzido**: são operações raras, sem o caminho crítico de latência que justifica decidir o commit antes de todas as confirmações.
 
### 6.1 Comunicação entre nós

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não há comunicação entre nós: o canal permanente e a API interna (`/cluster/*`) ficam desabilitados.
- A comunicação entre nós é feita via **API HTTP**, numa **porta própria** (`cluster.port`), separada da API de cliente, para que o acesso a ela seja restrito na rede sem depender do caminho da requisição ([06-api.md](06-api.md), seção 8.0). O canal permanente abaixo usa a mesma porta. A porta é protegida por **TLS mútuo com um certificado único do cluster** (`cluster.tls.*`, [06-api.md](06-api.md), seção 8.0.1): um nó só fala com quem apresenta o mesmo certificado.
- Além das chamadas HTTP, cada nó mantém um **canal permanente WebSocket com cada um dos outros nós** (malha completa: um por par, para todos os tópicos): carrega o **sinal de vida** entre nós (seção 6.3) e os sinais de disponibilidade da entrega de mensagens (long polling, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.8). O canal transporta **somente sinais pequenos** — nunca o conteúdo das mensagens nem listas longas —, para ficar livre e rápido.
### 6.2 Registro de um novo nó (bootstrap)

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não há bootstrap. O nó sobe sozinho e `cluster.entry-address` não é exigido.

![Sequência do ingresso de um novo nó](diagramas/img/ingresso-no.svg)

*Fonte: [ingresso-no.json](diagramas/fonte/ingresso-no.json). A sincronização do bloco está na seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md).*

1. **Descoberta.** A configuração exige o **endereço do balanceador do cluster** (`cluster.entry-address`, obrigatório e sem padrão; se faltar, o nó não sobe; é o host e a **porta do cluster** do balanceador, não a da API de cliente). A primeira coisa que o nó faz ao iniciar é chamar, por esse endereço, a listagem de nós (`GET /cluster/nodes`), que devolve a tabela de membros com os **IPs internos** de cada um. Com a lista, o nó escolhe um membro ao acaso e envia a ele, **numa única requisição síncrona** (`POST /cluster/nodes/join`), o pedido de registro com **seus próprios IPs e a porta do cluster**; esse nó passa a coordenar o registro dos passos 2 a 5. Se o membro não responde, tenta outro da lista. Vale também para o reinício de um nó, que volta como nó novo (seção 6.8). Se a listagem falha, o nó tenta de novo conforme `cluster.bootstrap.retry.interval`; só o **primeiro nó** de um cluster novo (`cluster.first-node=true`, padrão `false`), quando a listagem não encontra ninguém, inicia o cluster sozinho como único membro. "Não encontra ninguém" quer dizer **nenhuma resposta de rede** (conexão recusada ou tempo esgotado): um membro que responde, mesmo com erro (como `503`), foi encontrado, e o nó tenta de novo.
2. O nó que recebe a chamada **testa os IPs** informados, tentando estabelecer conexão de volta com o novo nó (um `GET /cluster/nodes` ao nó novo, em cada IP e na porta informados; qualquer resposta vale, também a de um nó que ainda não tem a tabela).
3. Se a conexão for bem-sucedida, o nó que recebeu envia a **todos os nós do cluster** um pedido de **registro (fase 1)** com os dados do novo nó. Se algum nó responde `409` (há outra mudança de membros pendente nele), o coordenador repete a chamada por até `write.commit.timeout`, enquanto a sua mudança continua pendente (regra da seção 6.8 para duas mudanças que se cruzam, D94).
4. Após todos confirmarem a fase de registro, o nó que recebeu envia a **todos os nós** uma chamada de **commit (fase 2)**, efetivando o novo nó na tabela de membros.
5. Só então **responde** à chamada original do novo nó com a **tabela de nós** (já com o commit confirmado em todo o cluster). A **tabela de tópicos** não vai nessa resposta, porque cresce com o número de tópicos: o novo nó a puxa de um nó do cluster pelo stream `GET /cluster/topics` ([02-arquitetura.md](02-arquitetura.md), seção 3.3).
6. Ao final do processo, o novo nó já conhece todos os demais membros do cluster e vice-versa, e o novo nó abre ativamente o canal permanente com cada nó da tabela (seção 6.1). Quando um nó cai e reinicia, é sempre ele quem se conecta de volta aos outros, pelos IPs que conhece da tabela de membros.
7. **Falha antes do commit** (teste de conexão do passo 2, ou fase de registro sem confirmação de todos): o processo é abortado (rollback do registro pendente, `POST /cluster/nodes/rollback` nos nós que já tinham confirmado) e o novo nó é **excluído** — ver seção 6.5. **Falha no commit** (fase de registro já confirmada por todos): a decisão já foi tomada e não é desfeita — o nó que coordena insiste no commit dentro do teto `write.commit.timeout` até confirmar em todos, em vez de abortar o novo nó, pois algum membro do cluster já pode tê-lo efetivado na tabela.
8. Ao assumir sua posição no anel (seção 7), o novo nó passa a ser responsável por blocos de mensagem como primário, secundário, terciário etc., conforme o fator de replicação configurado — seguindo o procedimento de sincronização distribuída da seção 7.3. Depois de pronto, passa a ser também **nó gestor** das filas voláteis cujo `id` cai na sua faixa do anel ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
9. **Prontidão.** O novo nó só fica **pronto** (`/health/ready`) quando termina toda a sincronização da seção 7.3: tópicos, blocos e o processamento do banco de eventos recebidos nesse intervalo. Até lá ele consta na tabela e confirma registros gravando-os no banco de eventos, mas não atende clientes, não coordena e não entrega mensagens.
### 6.3 Detecção de falhas e desregistro

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): **desabilitados** o sinal de vida, a reconexão, a votação de exclusão e o quórum de votação (`cluster.vote.*` é ignorado). Não há outro nó para suspeitar, votar ou excluir.

![Sequência da remoção de um nó](diagramas/img/remocao-no.svg)

*Fonte: [remocao-no.json](diagramas/fonte/remocao-no.json).*
 
1. Cada par de nós mantém um **canal permanente** (seção 6.1), que também carrega um **sinal de vida** leve (keepalive), uma mensagem própria com data e hora, resumo de monitoramento e as mensagens prontas por tópico (seção 12.8 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). O controle de quem está online é feito por esse canal, em malha, sem coordenador — **não há ping separado**.
2. Quando o canal com um nó **cai ou fica sem sinal de vida**, o nó que detectou **tenta reconectar algumas vezes** (tentativas e intervalo configuráveis) antes de agir.
3. Se o canal continuar fora após as tentativas, isto é, o nó ficou inacessível por tempo demais, é aberta uma **votação de exclusão** entre os nós do cluster.
4. **Quórum de votação: 51% dos nós saudáveis** (`cluster.vote.quorum-percent`, configurável), com um mínimo de votantes (`cluster.vote.min-voters`, padrão 2). Nó **saudável** é o que o nó que abre a votação enxerga com o canal ativo e que responde ao pedido de voto. Os nós fora do ar, os suspeitos, **não votam e não entram na base**: se vários caem ao mesmo tempo, por exemplo uma zona com problema, a votação segue só entre os saudáveis, e são exigidos `ceil(S × 51%)` votos de S saudáveis (por exemplo, 2 de 3, 3 de 4 ou 5). Se há menos saudáveis que o mínimo de votantes, nenhuma exclusão se efetiva (condição `quorum-lost`). Com várias exclusões pendentes, elas são votadas uma de cada vez (seção 6.8), cada uma sobre os saudáveis do momento. **Risco aceito:** como a base é só quem o votante enxerga, uma partição de rede em que os dois lados têm saudáveis suficientes pode levar cada lado a excluir o outro; o mínimo de votantes impede só que um nó isolado exclua todos os demais. O lado excluído, ao voltar, refaz o bootstrap do zero (seção 6.6).
   - **Evolução futura (fora do escopo atual):** base na tabela inteira (`total`), como o MongoDB; rota manual de exclusão forçada para quando a maioria se perde; e época na tabela de membros, com desempate ao fim da partição (o grupo maior vence, e o perdedor refaz o bootstrap). Enquanto isso, quem opera muitos nós pode subir `cluster.vote.min-voters` para uma maioria do cluster e assim reduzir o risco de partição.
5. Atingido o quórum, o desregistro segue o **mesmo protocolo de duas fases**: um pedido de **desregistro (fase 1)** é enviado a todos os nós e, em seguida, um **commit (fase 2)** efetiva a remoção do nó da tabela de membros em todo o cluster. Mesma regra de falha da seção 7.2: antes do commit, rollback; depois de decidido, sem rollback — insiste dentro do teto `write.commit.timeout` até confirmar a remoção em todos **os nós que sobram, menos os que quem coordena enxerga caídos** (esperar a confirmação de um nó fora do ar impediria a exclusão de outro; quem fica com a tabela velha aparece como `membership-divergent`, [11-observabilidade.md](11-observabilidade.md); D108).
> No commit do desregistro, quem passa a ser primário das posições do nó removido assume as **filas voláteis** que ele gerenciava, sem transferir estado ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
>
> Um nó em **modo drain** não passa por votação: ele pede o próprio desregistro, pelo mesmo protocolo de duas fases (seção 6.10).
>
> Enquanto a votação não conclui, os sinais para o nó ficam acumulados em filas em memória; se o canal reconecta antes disso, nada muda e o envio é retomado. Se a fila estourar o limite, o nó é tratado como falho e a votação é aberta. Concluída a exclusão, todos os nós descartam as filas para aquele nó ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9).
> **Quem coordena a exclusão (provisório, D98).** Quando um nó cai, todos os saudáveis o percebem juntos e cada um abre a votação; se todos coordenassem o desregistro, cada um deixaria a sua mudança pendente e recusaria a dos outros (`409`, seção 6.8). Coordena o de **menor ID entre os saudáveis que votaram**; o de ordem `k` espera `k × write.commit.timeout` e só então tenta, se o nó ainda consta da tabela. No conflito, repete com uma espera sorteada de 100 a 200 ms. Uma votação que termina em falha por erro passageiro não se perde: a próxima falha do canal abre outra.
>
> **O voto (provisório, D99).** O votante responde `yes` se também não alcança o candidato (confere na hora, com uma chamada). Quando o motivo é `node-failed`, o candidato está vivo e o canal com ele segue de pé; o votante responde `yes` se o próprio canal dele recebeu o aviso `node-state failed` do candidato.
>
> **Tempo sem sinal (provisório, D109).** O canal com um par cai quando uma chamada falha ou passa de `2 × cluster.channel.keepalive.interval` (no mínimo 500 ms); as chamadas seguintes, a cada `cluster.channel.reconnect.backoff`, são as tentativas, e esgotadas `cluster.channel.reconnect.retries` (3) a votação é aberta.
>
> **O nó certo atrás do endereço (provisório, D100).** A chamada do canal leva o `nodeId` do **destino** (`to`). O processo novo que atende o mesmo IP e porta (um reinício rápido, antes de a queda ser declarada) tem outro ID e responde `421` `cluster.wrong-node`, antes de qualquer outra conferência; o emissor conta isso como canal caído, de modo que a votação abre, o nó antigo sai da tabela e o pedido de ingresso do nó novo (que esperava a votação, seção 6.8) é processado.
>
> **O nó em falha (provisório, D78.2).** O nó em `failed` (disco cheio, banco corrompido) deixa de atender chamadas de **cliente**: num cluster repassa a chamada a um nó são (como o nó em drain, seção 6.9) e, sem outro nó (ou num nó só), responde `503` `node.failed` com `Retry-After`. As rotas de operação continuam, e ele continua a responder as chamadas entre nós até ser excluído pelos demais (o voto da seção acima).
>
> **O reencontro depois de uma partição simétrica (provisório, D101).** Se os dois lados de uma partição excluíram um ao outro, as tabelas têm a mesma versão, nenhum canal liga um lado ao outro e nenhum nó recebe `not-member`. Por isso cada nó guarda os endereços dos nós que viu serem excluídos por votação e, a cada `cluster.channel.keepalive.interval` × 10, consulta a tabela deles (`GET /cluster/nodes`). Se ela é de **outro cluster** (sem este nó e sem nenhum membro em comum com a tabela deste) e este lado **perde** — tem menos membros; num empate, é o lado do menor `nodeId` —, o nó refaz o ingresso do zero, como o excluído que volta (procedimento da seção 6.6, mesmo ID, freezing). Os dois lados chegam à mesma conclusão. A marca de votação aberta de um nó que saiu da tabela é esquecida ao sair: se ele voltar com o mesmo ID, ela não bloqueia as entradas até `write.pending.ttl`.

> **Como o nó excluído descobre.** Se três chamadas seguidas do canal a um par voltam `403 cluster.not-member` **e** a tabela do par é mais nova (versão maior) e não o tem, o nó se sabe excluído: apaga o estado local e refaz o bootstrap com o mesmo ID, que o freezing (seção 6.7) atrasa. Antes de apagar, ele passa pelo procedimento da seção 6.6 (estado `returning`), que recupera as mensagens que só ele tinha.

### 6.4 Tamanho mínimo do cluster e modo desenvolvimento
| Ambiente | Nós mínimos | Racional |
|---|---|---|
| Teste / desenvolvimento | 1 | Permite rodar localmente sem overhead de cluster, no **modo desenvolvimento** (abaixo): votação, quórum e canal entre nós ficam desabilitados. |
| Produção | 3 | Permite votação com um nó fora do ar (2 votantes saudáveis) e evita decisão de falha por um único nó. |

#### Modo desenvolvimento

O modo é escolhido na partida, por `mode` (`TABA_MODE`): **`production`** (padrão) ou **`development`**. Em `development` o nó é um **cluster de um só** e tudo o que existe para coordenar vários nós fica **desabilitado**. Serve para rodar localmente, em testes e no desenvolvimento de clientes. **Não é para produção**: sem réplicas, perder o nó ou o disco é perder as mensagens.

| Mecanismo | Em modo desenvolvimento |
|---|---|
| Bootstrap e registro de nó (6.2, 6.5) | Não há. O nó sobe sozinho, e `cluster.entry-address` **não é exigido**. |
| Canal entre nós, sinal de vida e reconexão (6.3, 12.8, 12.9) | Não existem. |
| Votação de exclusão e quórum de votação (6.3) | Desabilitados; `cluster.vote.*` é ignorado. |
| Freezing, retorno de nó, tabela `losses`, concorrência de registro e desregistro, repasse durante a sincronização (6.6 a 6.9) | Não se aplicam. |
| Anel de hash (7.1) | Um nó só, primário de todos os blocos. Não há rebalanceamento. |
| Replicação e quórum de escrita (7.2) | `replication.factor` e `write.quorum` valem **1**; valores configurados são ignorados, com aviso no log. O registro e o commit acontecem só no próprio nó. |
| Sincronização de blocos e banco de eventos (7.3) | Não há. |
| Entrega (12) | O nó é dono de todas as mensagens. Não há `take` nem avisos entre nós; o long polling é só local. |
| Definição de tópicos e filas voláteis (3.3, 14) | Aplicada só no nó; o nó é o gestor de todas as filas voláteis. |
| Porta do cluster e rotas `/cluster/*` | A porta do cluster **não é aberta** (`cluster.port` é ignorado). Outro nó não consegue ingressar. |
| Entrega justa por grupos (12.11) | Vale igual, com contagem exata. |
| Fila FIFO ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16) | Funciona igual: o nó é o primário de todos os grupos, sem repasse, e o quórum vale 1 (não se exige secundário nem `replication.factor` 2). Ordem, entrega por grupo e deduplicação são as mesmas. |
| Modo drain (6.10) | Vale a parte local. Sem outro nó, as chamadas de cliente durante o drain respondem `503`. O drain serve de **save**: na partida seguinte o nó retoma do disco (ver "Modo desenvolvimento: o drain como save", seção 6.10). |
| Observabilidade | `GET /health/cluster` devolve o mesmo com `scope=cluster` e `scope=node`; as métricas `taba.cluster.*` não existem; o Prometheus lê o nó, com uma série só. |

**O que continua igual:** a API de cliente, o armazenamento com overflow para disco, a entrega e a invisibilidade, `ttl`, `maxDeliveries` e a dead-letter, os tópicos distribuidores e voláteis, os limites e o rate limit. Quem desenvolve contra o modo desenvolvimento vê o mesmo comportamento de cliente do cluster, **menos** o que depende de falha de nó, de réplica ou de quórum.

**Aviso:** ao subir, o nó registra um aviso (`warn`) informando o modo, e `GET /health/cluster` traz o campo `mode` de cada nó. O modo não muda em execução.
 
### 6.5 Falha no registro de um novo nó

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica; não há registro de nó nem rebalanceamento.

![Sequência da falha no registro de um novo nó](diagramas/img/falha-registro-no.svg)

*Fonte: [falha-registro-no.json](diagramas/fonte/falha-registro-no.json).*
 
Se o registro de um novo nó **falhar antes do commit** (teste de conexão do bootstrap malsucedido, ou a fase de registro sem confirmação de todos os nós, dentro de `write.commit.timeout` — seção 6.2, passo 7):
 
1. O nó é **excluído** do cluster — não chega a ser efetivado na tabela de membros.
2. Os **blocos** que seriam assumidos por ele são **rebalanceados** entre os nós restantes do cluster (o anel de hash é recalculado sem esse nó).
3. **Nova tentativa:** o nó repete o bootstrap do começo (seção 6.2) a cada `cluster.bootstrap.retry.interval`, até `cluster.bootstrap.retry.max-attempts` tentativas (padrão sem limite). O mesmo intervalo vale quando a listagem de nós, o pedido de ingresso ou a sincronização falham.
> Diferente da falha descrita na seção 7.3 (mensagem não encontrada durante a sincronização de um bloco), a falha de registro é sobre o **nó como um todo** não conseguir entrar no cluster — por isso o efeito é exclusão + rebalanceamento, e não apenas um alerta.
 
### 6.6 Retorno de um nó excluído

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica; a tabela `losses` não é usada. Um reinício retoma do disco (o drain serve de save; seção 6.10), e o que estava só em memória e não foi salvo se perde.

![Sequência do retorno de um nó excluído](diagramas/img/retorno-no-excluido.svg)

*Fonte: [retorno-no-excluido.json](diagramas/fonte/retorno-no-excluido.json).*
 
Quando um nó que foi **desregistrado por falha do canal** (seção 6.3) volta a ficar disponível, ele não simplesmente retoma seu lugar antigo no anel — os blocos que ele atendia já podem ter sido reatribuídos a outros nós. O procedimento recupera primeiro o que o nó tem e pode ajudar, e só depois refaz a entrada:

1. **Lista de perdas.** O nó consulta **todos os nós que consegue alcançar** (`GET /cluster/message-loss`, um stream NDJSON) e une as respostas. Cada nó responde com a sua tabela `losses` (abaixo): os pares (tópico, ID) que alguma sincronização declarou perdidos (seção 7.3). Essas rotas aceitam chamadas de um nó que **ainda não é membro**, já que ele foi excluído.
2. **Confronto com o que o nó tem.** Para cada par, o nó procura o ID no que guarda localmente: no disco (`${topic}/${id}.body` e o banco do tópico) e, se o processo é o mesmo que ficou fora (sem reinício), também em memória. Num reinício, só o que está em disco conta.
3. **Envio.** Para cada mensagem encontrada, o nó a envia ao nó responsável pelo bloco do ID, pela rota de mensagens recuperadas (`POST /cluster/messages/recover`, um stream NDJSON). Quem recebe grava **direto nos nós responsáveis**, idempotente pelo ID e sem as duas fases (a mensagem não existe nos donos, ninguém concorre por ela), e confirma a linha quando `write.quorum` nós gravaram. Vai o corpo e o `deliveryCount`; a invisibilidade é zerada, e a mensagem volta a ficar pronta para entrega.
4. **Baixa do registro.** O nó que aceita a mensagem apaga o par da sua tabela `losses` e avisa os demais para apagarem o deles (melhor esforço), para que um segundo nó que volte com a mesma mensagem não a reenvie depois de ela já ter sido entregue e confirmada.
5. **Estado local em quarentena.** Enquanto a recuperação não termina, o nó **não apaga** os dados antigos. Se algum nó não respondeu na consulta do passo 1, o nó tenta de novo a cada `cluster.bootstrap.retry.interval`, até `cluster.recovery.timeout`; só então segue.
6. **Limpeza e novo registro.** Concluída a recuperação (ou esgotado o prazo), o nó **limpa seu estado local** e **inicia um novo registro no cluster do zero** — o bootstrap completo da seção 6.2, como se fosse um nó novo, com uma nova posição no anel. O freezing (seção 6.7) só atrasa este passo; a recuperação dos passos 1 a 5 não depende dele.

**Tabela de perdas.** Cada nó guarda no banco do nó (`${storage.dir}/_node.db`, SQLite, seção 3.2 de [02-arquitetura.md](02-arquitetura.md)) a tabela `losses` (`topic`, `id`, `block`, `detected_at`), com chave (`topic`, `id`). O nó que detecta a perda na sincronização grava o registro e o envia também aos nós responsáveis pelo bloco (idempotente pela chave), para a informação sobreviver à queda do detector. Os registros expiram depois de `cluster.message-loss.ttl`.

**Como está implementado (etapa 15c).**

- **Quem volta:** o nó excluído (seção 6.3) e o que volta do drain (seção 6.10) entram em `returning` e consultam os nós da **última tabela que conheceram**; a tabela atual, de onde sai o responsável de cada bloco, vem do `GET /cluster/nodes` de quem respondeu (a de maior versão). O responsável é o primário do bloco no anel **sem o próprio nó**: a tabela de um par pode ainda listá-lo.
- **Quem recebe o `recover`** grava no próprio nó e, com `replicate` (padrão), manda a mensagem aos outros responsáveis com `?replicate=false`, que só gravam no disco deles. A linha vale como aplicada com `write.quorum` gravações (limitado ao número de responsáveis); uma mensagem que já existia, que venceu pelo `ttl` ou cujo tópico não existe mais conta como resolvida. Só então dá a baixa na perda e avisa os demais por `DELETE /cluster/message-loss/{topic}/{id}`.
- **Perda enviada aos responsáveis:** quem a detecta na sincronização a manda aos outros responsáveis do bloco por `POST /cluster/message-loss` (melhor esforço).
- **Prazo:** o nó que não respondeu à consulta é chamado de novo a cada `cluster.bootstrap.retry.interval` até `cluster.recovery.timeout` (padrão 5 min, D110); esgotado, o nó segue com as listas que tem. O lote que o responsável não aceita é reenviado inteiro no mesmo ritmo e até o mesmo prazo.
- **O que o nó guarda fora da tabela:** quem saiu da tabela (drain) não faz a limpeza dos blocos que perdeu (seção 7.3 de [05](05-particionamento-replicacao.md), passo 7); sem isso, as réplicas que ele guarda seriam apagadas antes de a recuperação procurá-las.
- **Reinício (provisório, D111):** um processo que reinicia nasce como nó novo (ID novo), mas o disco da vida anterior **não é apagado na partida**. Na primeira listagem do bootstrap, antes de pedir o ingresso, ele passa por este procedimento com os membros que a listagem trouxe: consulta a lista de perdas, devolve ao responsável o que o disco tem delas e só então apaga o disco e entra. O responsável é o primário do bloco no anel **sem o próprio nó**, também pelo endereço: a tabela ainda tem o ID antigo no mesmo IP e porta. O primeiro nó de um cluster novo não tem de quem recuperar, e o disco antigo segue o caminho de sempre (apagado pela varredura).

### 6.7 Período de freezing (anti-flapping)

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica (`cluster.freezing.*` é ignorado).
 
Para evitar que um nó instável/defeituoso entre e saia repetidamente do cluster (flapping), existe um **período de freezing**:
 
- Após um nó ser desregistrado por falha (seção 6.3), ele fica **impedido de se registrar novamente** no cluster até esse período expirar. A **duração é configurável** (`cluster.freezing.duration`).
- **Identidade do nó:** ao nascer (a cada início do processo), o nó se **auto-nomeia com um ID GUID v7**, guardado só em memória, e é por esse ID que se identifica perante os outros nós, inclusive no pedido de ingresso. O cluster guarda o ID e o instante do desregistro de cada nó em freezing.
- **A quem se aplica:** ao nó que foi excluído pela votação mas **continua o mesmo processo**, com a mesma memória e o mesmo ID, e fica caindo e voltando (por exemplo, por falha de rede), tentando se reconectar e se registrar sem reiniciar. A saída por **drain** (seção 6.10) também não gera freezing: é voluntária, e o retorno é com ID novo. Um **reinício** do processo é outra coisa: o nó nasce com um ID novo e entra como **nó novo**, sem freezing, e só aproveita o que está em disco para ajudar a recuperar mensagens perdidas (seção 6.6).

- **Durante o freezing, o pedido de ingresso é rejeitado** (não há lista de espera, e o cluster não guarda o pedido). A resposta traz `Retry-After` com o tempo que falta, e o nó volta a tentar dentro do laço de nova tentativa do bootstrap (`cluster.bootstrap.retry.interval`, seção 6.5), **a cada intervalo, sem esperar o `Retry-After`**: o `Retry-After` informa quem chama e não alonga a espera, e a tentativa feita dentro do freezing é recusada de novo. O cluster guarda o ID e o instante **na memória de quem fez o commit da exclusão**, e a resposta do `join` e a do `GET /cluster/nodes` levam a lista dos que ainda estão em freezing (`frozen`, [06c](06c-api-cluster.md)): o nó que entra depois da exclusão herda o freezing de quem o recebeu, sem subir a `version` da tabela (provisório, D106). Um membro que estava fora do ar no commit da exclusão e volta com a tabela velha não o conhece (a reconciliação ao reencontro do D123, etapa 18b, fecha esse caso). A rejeição é registrada no log ("rejeitado (freezing)").

### 6.8 Concorrência entre registro e desregistro

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica; só há um nó, sem mudança de membros.

Registro (bootstrap) e desregistro (votação de exclusão) são as duas operações pesadas do cluster (seção 6). Regra de concorrência:

- **Uma mudança de membros por vez, até o nó ficar pronto:** depois que um nó entra (commit do registro, seção 6.2), nenhuma outra **entrada, saída (desregistro) ou rebalanceamento** é efetivada até ele ficar **pronto**, isto é, terminar toda a sincronização da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md). Pedidos de bootstrap e votações de exclusão que chegarem nesse intervalo **esperam**. Isso vale também enquanto uma votação de exclusão está em andamento (próximo item), então só há uma mudança de membros em curso por vez no cluster. A única exceção é o **próprio nó que está se sincronizando**: se ele entra em falha (seção 7.3, passo 6), a votação do desregistro dele é liberada.
- **Desregistro voluntário (drain, seção 6.10):** conta como saída de membro e segue a mesma regra: espera outra mudança em curso e, enquanto corre, bloqueia novos pedidos de bootstrap.
- **Votação de exclusão em andamento:** enquanto uma votação de exclusão de algum nó está em andamento (da abertura até o commit do desregistro, seção 6.3), um **novo pedido de bootstrap fica em espera**; só é processado quando a votação terminar (com ou sem exclusão efetivada).
- **Duas mudanças coordenadas por membros diferentes ao mesmo tempo (provisório, D94):** a de menor `operationId` (o GUID v7 mais antigo) vence. Quem coordena uma mudança e recebe o registro de uma mais antiga **cede**: descarta a sua e aceita a outra; o registro que um participante mantém para outro coordenador nunca é deslocado. O coordenador deslocado desfaz o que já registrou nos outros nós e recomeça depois de uma espera sorteada de 100 a 200 ms (o ingresso, por até 60 s; a saída por drain e a exclusão já repetiam), e o vencedor repete o `409` dos pares por até `write.commit.timeout`. Assim há sempre um vencedor, sem um coordenador único.
- **Por quê:** evita que um nó novo assuma posições no anel (seção 7) calculadas sobre uma tabela de membros que está prestes a mudar, ou que os blocos de um nó em sincronização mudem no meio da carga, o que exigiria recalcular o rebalanceamento em cima de outro em andamento. O custo é aceito: um nó que falha (cai) durante a sincronização de outro só é desregistrado depois que esta termina ou falha.

**Caso do nó que reinicia sem memória antes da votação concluir** (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), D29.6): o processo perdeu o estado e volta com o **mesmo IP e porta**, mas o nó ainda consta na tabela em votação de exclusão. O pedido de bootstrap dele entra na fila e espera o fim da votação, como qualquer outro. Encerrada a votação (o nó antigo é removido da tabela), o pedido é processado como um **registro comum de nó novo** (seção 6.2), sem tratamento especial por ter o mesmo endereço.


### 6.9 Repasse das chamadas de cliente durante a sincronização e o drain

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não se aplica; nenhuma chamada é repassada.

Enquanto um nó que entrou ainda não está íntegro (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)), ele não tem a tabela de tópicos completa nem coordena nada, mas pode receber chamadas de cliente (antes de o `/health/ready` tirá-lo da rotação, ou de quem fala direto com ele). Em vez de devolver erro, ele **repassa a chamada a outro nó**. Vale só para as **rotas de cliente**; as rotas de sincronização, de comunicação entre nós e de operação (`/health/*`, `/metrics`, `/metrics/topics/*`, `/node/drain`) não são repassadas: cada nó responde as suas. Como a API não tem estado preso a um nó, o resultado é o mesmo em qualquer um, e o repasse dura só o tempo da sincronização, que é de um nó por vez (seção 6.8).

O **nó em drain** (seção 6.10) faz o mesmo repasse: segue recebendo chamadas de cliente, de quem ainda fala com ele, e repassa **todas** a outro nó, desde que o drain começa até o processo encerrar, mesmo depois de ter saído do cluster. O que muda é o destino (primeiro item).

- **Destino:** na sincronização, um membro da tabela com canal ativo, escolhido ao acaso (só há um nó em sincronização por vez, então os demais estão prontos). No **drain**, o **nó mais livre**: o de menor carga pelo indicador do sinal de vida (o mesmo da escolha da origem de carga de tópicos, seção 3.3 de [02-arquitetura.md](02-arquitetura.md)), com empate sorteado. Só entram nós prontos e fora de drain. A escolha é feita **a cada chamada**, e o nó soma ao indicador de cada destino as chamadas que ele mesmo já repassou e ainda estão em andamento, para não despejar tudo num só. Depois que o canal fecha, vale o último indicador recebido e a última tabela de membros conhecida, só em memória.
- **Repasse transparente:** método, caminho, query, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu (inclusive `409`, `413` e `429`). Vale para todas as rotas de cliente, também o `GET` de mensagens com long polling e `autoCreate`.
- **Pela porta do cluster:** o repasse vai à **porta do cluster** do destino, pela rota `/cluster/forward/{caminho}` ([06c-api-cluster.md](06c-api-cluster.md), seção 8.2), e não à porta da API de cliente. Assim a API de cliente não precisa aceitar nenhum marcador interno (que um cliente poderia forjar) e o repasse só trafega onde a rede já restringe o acesso a nós.
- **Autenticação:** com a autenticação ligada, o nó em sincronização ainda não tem a tabela de tokens (seção 3.4 de [02-arquitetura.md](02-arquitetura.md)) e não confere o token; repassa o `Authorization` como veio, e o **destino confere** ([06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3).
- **Sem laço:** a rota `/cluster/forward` é, por si, o marcador de repasse, aceita só de membros da tabela e de nós que **saíram por drain** (o commit do desregistro voluntário põe o nó numa lista em memória de cada nó, que só vale para aceitar o repasse e sai quando um nó novo se registra com o mesmo endereço). Quem a recebe nunca repassa de novo; se estiver sincronizando, responde `503`.
- **Falha do destino:** antes de o corpo ser enviado (conexão recusada), tenta outro nó, até um teto pequeno de tentativas. Depois de enviado, **não repete**: um `POST` de mensagem repetido duplicaria a publicação, já que o ID é gerado no coordenador. Responde `502` ou `504` e o cliente decide.
- **Rate limit e IP:** o limite é aplicado uma vez, no nó que recebeu a chamada, e **por IP do cliente**: esse nó não tem a tabela de tokens completa e não valida o token, então não o usa como chave ([12-limites-api.md](12-limites-api.md)). O destino não conta de novo a chamada repassada (a porta do cluster não tem rate limit). O IP do cliente já resolvido segue em cabeçalho interno.
- **Sem destino:** se nenhum outro nó responde, `503` com `Retry-After`.
- **Publicação em fila FIFO:** mesmo com o nó pronto, a publicação numa fila FIFO é **repassada ao primário do grupo** (o destino é o dono do grupo, não o nó mais livre). O primário confere se é o dono (`421` se não for), e na FIFO a repetição depois de enviado o corpo é segura, porque o primário deduplica ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.4). **Como está feito (etapa 15d):** o repasse não passa pelo `* /cluster/forward`: é uma chamada própria entre nós, `POST /cluster/topics/{topic}/fifo/publish` ([06c](06c-api-cluster.md)), usada também pelas cópias de um distribuidor para uma fila FIFO (D112).

### 6.10 Parada e manutenção: modo drain

O **modo drain** prepara o nó para **sair do cluster** com o mínimo de estrago, em manutenção, atualização ou realocação de container. Ele **força todas as mensagens do nó para o disco**, tira o nó do tráfego de cliente e o **desregistra do cluster por conta própria, sem votação**. Sem ele, uma parada perde tudo o que estava só em memória, o canal cai e o cluster só tira o nó depois das reconexões e da votação (seção 6.3).

**Estados** (`drainState`): `active` (normal) → `draining` (esvaziando a memória e saindo) → `drained` (tudo em disco e fora do cluster).

**Como entra:**

- **Sinal de parada.** Ao receber `SIGTERM` ou `SIGINT` (o que o Docker e o Kubernetes enviam), o nó entra em drain sozinho, se `drain.on-stop` está ligado (padrão). Terminado o drain, o processo **encerra** com código 0. Um segundo sinal durante o drain encerra na hora, sem terminar.
- **API.** `POST /node/drain` (seção 8.1.1 de [06b-api-operacao.md](06b-api-operacao.md)) faz o mesmo, **sem encerrar**: o nó fica em `drained`, fora do cluster e da rotação, até alguém parar o processo ou chamar `DELETE /node/drain` (abaixo). Vale **só para o nó que recebe a chamada**, então o operador precisa falar com o nó direto (por exemplo pela porta de operação, `operations.port`) e não pelo balanceador.

**O que o nó faz, em ordem:**

1. **Sai da rotação.** `/health/ready` passa a responder `503` na hora (seção "Health checks" de [11-observabilidade.md](11-observabilidade.md)); `/health/live` segue `200`. O nó **continua recebendo chamadas de cliente** (o balanceador demora a perceber, e há quem fale direto com ele), mas **não atende nenhuma**: repassa todas ao **nó mais livre**, como no repasse da seção 6.9, até o processo encerrar. Os long pollings que já estavam em andamento são respondidos vazios, para o cliente reconectar; os novos são repassados.
2. **Avisa os outros nós** pelo canal, com um sinal de estado "em drain" (idempotente, como os demais, seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Os pares **deixam de pedir `take` a ele** e de escolhê-lo como origem de carga de sincronização. Um `take` que ainda chegue é respondido vazio.
3. **Passa a gravar tudo em disco.** O limite de memória efetivo cai a **zero**: toda mensagem nova, publicada ou recebida como réplica, vai direto para disco (`${topic}/${id}.body` mais a linha no banco do tópico, nessa ordem, seção 3.2 de [02-arquitetura.md](02-arquitetura.md)). Os pares **continuam** mandando registros e commits a ele até a saída, como réplica.
4. **Esvazia a memória.** Uma thread move para o disco as mensagens que estão em memória, **com os metadados** (`deliveryCount`, `invisibleUntil` e `receiptToken`), pelas threads de escrita de cada tópico, em paralelo entre tópicos. A ordem segue o critério do overflow: primeiro os tópicos de menor taxa de leitura. A lista de prontos e a de invisíveis são só índices em memória, derivados dos metadados, e são refeitas quando o nó assumir blocos (seção 13.7 de [14-invisibilidade.md](14-invisibilidade.md)).
5. **Sai do cluster (desregistro voluntário).** Com a memória vazia, o nó coordena o **desregistro de si mesmo** pelo mesmo protocolo de duas fases do desregistro por votação (passo 5 da seção 6.3): fase 1 a todos os nós (`POST /cluster/nodes/unregister`), commit por `POST /cluster/nodes/commit`, com a mesma regra de falha (antes do commit, rollback; depois, o coordenador insiste dentro de `write.commit.timeout`). **Não há votação nem quórum de votação**: quem pede é o próprio nó, que está saindo por vontade. O commit tira o nó da tabela de membros, recalcula o anel e **rebalanceia** os blocos dele entre os que ficam, como no desregistro por falha (seção 6.3). Os novos primários assumem as filas voláteis que ele gerenciava (seção 14.6 de [15-topicos-distribuidor.md](15-topicos-distribuidor.md)).
6. **Fecha o canal** com os pares, já com o nó fora da tabela de membros: ninguém tenta reconectar nem abre votação. O estado vira `drained`. Se foi por sinal, o processo **espera as chamadas repassadas em andamento terminarem** (dentro de `drain.timeout`) e encerra; se foi pela API, o nó segue repassando as chamadas que receber.

O desregistro voluntário segue a regra de concorrência da seção 6.8 (uma mudança de membros por vez): se há outra entrada, saída ou votação em curso, o pedido **espera a sua vez**, dentro de `drain.timeout`. **Não há freezing** (seção 6.7): sair por drain não é falha.

**Falha ou prazo estourado.** `drain.timeout` cobre o drain inteiro, do passo 1 ao 6, e precisa ser **menor** que o prazo de parada do orquestrador (`terminationGracePeriodSeconds` no Kubernetes, 30 s por padrão), senão o processo é morto antes de registrar o resultado. Se o prazo estoura, o nó registra o que sobrou em memória e a etapa em que parou e **encerra mesmo assim** (se foi por sinal). Cai-se então no comportamento sem drain: o canal cai, valem a reconexão e a votação de exclusão (seção 6.3). Erro de I/O ou disco cheio no passo 4 não interrompe o drain: o nó segue com o resto, conta o erro e o inclui no resultado.

**Voltar do drain (`DELETE /node/drain`).**

- **Antes da saída** (estados `draining` até o início do passo 5): cancela o drain, o nó volta a `active` e a gravar em memória, e continua membro do cluster. Responde `200`.
- **Durante o desregistro** (passo 5 em andamento): responde `409`; tente de novo quando o nó estiver `drained`.
- **Depois da saída** (`drained`): o nó entra **como nó novo**. Gera um **ID novo** (GUID v7), refaz o bootstrap completo da seção 6.2, com nova posição no anel, e responde `202`; `drainState` volta a `active`, e o estado do nó no cluster passa por `joining` e `syncing` até ficar pronto (`taba.node.state`). Antes de limpar o estado local, o nó faz a **recuperação de mensagens perdidas** da seção 6.6 (passos 1 a 5), já que o disco tem **tudo** o que ele tinha. Não há freezing, pois o ID é outro e a saída foi voluntária.

#### Modo desenvolvimento: o drain como save

Sem outro nó para recuperar dados e sem cluster para limpar, o drain do modo desenvolvimento vale como um **save**: tudo o que está em memória vai para o disco, e a **partida seguinte retoma do disco**. Assim dá para parar e subir o nó (Ctrl+C, `docker stop`, troca de versão) sem perder o trabalho. Vale a parte local do drain (por sinal, com `drain.on-stop` ligado por padrão, ou por API); não há desregistro, canal nem rebalanceamento, e `DELETE /node/drain` volta a `active` direto. Sem outro nó para receber o repasse, as chamadas de cliente durante o drain respondem `503` com `Retry-After`.

**O que o save grava** (em `storage.dir`):

- **Mensagens:** corpo (`${topic}/${id}.body`) e metadados (`deliveryCount`, `invisibleUntil`, `receiptToken`, `groupId`, `contentType`) no banco do tópico, como no drain comum (seção 3.2 de [02-arquitetura.md](02-arquitetura.md)).
- **Definições de tópico:** já ficam sempre em disco (tabela `topics` do `_node.db`, seção 3.3). O save confere que estão em dia.
- **Estado do nó:** na tabela `state` do `_node.db`, uma marca `save_completed` com o instante do fim do drain. O instante do último `GET` das filas voláteis **não** é gravado: o relógio de ociosidade (seção 14.3 de [15-topicos-distribuidor.md](15-topicos-distribuidor.md)) recomeça na partida, então o dado não mudaria o resultado.
- **Não grava o que se refaz:** a lista de prontos, a de invisíveis, a contagem por grupo e as taxas são derivadas e reconstruídas na partida (seção 13.7 de [14-invisibilidade.md](14-invisibilidade.md)).

**Na partida**, em `mode=development` e com `development.resume-on-start` ligado (padrão), o nó **retoma o que encontra em `storage.dir`**, em vez de começar do zero:

1. Carrega as definições de tópico, mantendo o `id` e a data de criação de cada uma.
2. Lê o banco de cada tópico e reconstrói as listas e as contagens a partir dos metadados.
3. Refaz o relógio de ociosidade das filas voláteis: o prazo de cada uma conta a partir **da partida** (o prazo é `max(criação, partida, último GET) + idleTimeout`), para a fila não expirar por causa do tempo em que o nó esteve parado.
4. Fica pronto (`/health/ready`) só ao terminar. A marca `save_completed` é **apagada** nesse momento, para que uma queda depois da partida não passe por save limpo.

**Detalhes da retomada:**

- A invisibilidade é um instante absoluto: a mensagem cujo prazo passou durante a parada volta a ficar pronta. As que ainda estão invisíveis seguem assim, e o `receiptToken` continua valendo para o ack.
- O `ttl` conta do ID (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)), então o **tempo parado conta**: o que venceu enquanto o nó estava fora é descartado na primeira varredura.
- **Sem save concluído** (queda, `kill -9`, drain interrompido por `drain.timeout`): o nó retoma mesmo assim o que está em disco e registra um aviso (`warn`) de que o que estava só em memória se perdeu.
- Com `development.resume-on-start=false`, o nó **apaga** o conteúdo de `storage.dir` na partida e começa limpo (útil em teste automatizado), com um aviso no log.

O **cluster** (`mode=production`) não muda: toda partida é de nó novo, e o disco só serve ao retorno de um nó excluído (seção 6.6).
