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

1. O novo nó se conecta a um nó existente do cluster, informando **seus próprios IPs e porta** na mesma chamada síncrona (uma única requisição de registro).
2. O nó que recebe a chamada **testa os IPs** informados, tentando estabelecer conexão de volta com o novo nó.
3. Se a conexão for bem-sucedida, o nó que recebeu envia a **todos os nós do cluster** um pedido de **registro (fase 1)** com os dados do novo nó.
4. Após todos confirmarem a fase de registro, o nó que recebeu envia a **todos os nós** uma chamada de **commit (fase 2)**, efetivando o novo nó na tabela de membros.
5. Só então **responde** à chamada original do novo nó com a **tabela de nós** (já com o commit confirmado em todo o cluster) e a **tabela de tópicos** ([02-arquitetura.md](02-arquitetura.md), seção 3.3).
6. Ao final do processo, o novo nó já conhece todos os demais membros do cluster e vice-versa, e o novo nó abre ativamente o canal permanente com cada nó da tabela (seção 6.1). Quando um nó cai e reinicia, é sempre ele quem se conecta de volta aos outros, pelos IPs que conhece da tabela de membros.
7. **Falha antes do commit** (teste de conexão do passo 2, ou fase de registro sem confirmação de todos): o processo é abortado (rollback do registro pendente) e o novo nó é **excluído** — ver seção 6.5. **Falha no commit** (fase de registro já confirmada por todos): a decisão já foi tomada e não é desfeita — o nó que coordena insiste no commit dentro do teto `write.commit.timeout` até confirmar em todos, em vez de abortar o novo nó, pois algum membro do cluster já pode tê-lo efetivado na tabela.
8. Ao assumir sua posição no anel (seção 7), o novo nó passa a ser responsável por blocos de mensagem como primário, secundário, terciário etc., conforme o fator de replicação configurado — seguindo o procedimento de sincronização distribuída da seção 7.3. Também passa a ser **nó gestor** das filas voláteis cujo `id` cai na sua faixa do anel ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
### 6.3 Detecção de falhas e desregistro

![Sequência da remoção de um nó](diagramas/img/remocao-no.svg)

*Fonte: [remocao-no.json](diagramas/fonte/remocao-no.json).*
 
1. Cada par de nós mantém um **canal permanente** (seção 6.1), que também carrega um **sinal de vida** leve (keepalive). O controle de quem está online é feito por esse canal, em malha, sem coordenador — **não há ping separado**.
2. Quando o canal com um nó **cai ou fica sem sinal de vida**, o nó que detectou **tenta reconectar algumas vezes** (tentativas e intervalo configuráveis) antes de agir.
3. Se o canal continuar fora após as tentativas, isto é, o nó ficou inacessível por tempo demais, é aberta uma **votação de exclusão** entre os nós do cluster.
4. **Quórum mínimo padrão: 3 votos** concordando com a exclusão para efetivar a remoção — valor inicial a ser confirmado/ajustado conforme testes em produção.
5. Atingido o quórum, o desregistro segue o **mesmo protocolo de duas fases**: um pedido de **desregistro (fase 1)** é enviado a todos os nós e, em seguida, um **commit (fase 2)** efetiva a remoção do nó da tabela de membros em todo o cluster. Mesma regra de falha da seção 7.2: antes do commit, rollback; depois de decidido, sem rollback — insiste dentro do teto `write.commit.timeout` até confirmar a remoção em todos.
> No commit do desregistro, quem passa a ser primário das posições do nó removido assume as **filas voláteis** que ele gerenciava, sem transferir estado ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.6).
>
> Enquanto a votação não conclui, os sinais para o nó ficam acumulados em filas em memória; se o canal reconecta antes disso, nada muda e o envio é retomado. Se a fila estourar o limite, o nó é tratado como falho e a votação é aberta. Concluída a exclusão, todos os nós descartam as filas para aquele nó ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9).
### 6.4 Tamanho mínimo do cluster
| Ambiente | Nós mínimos | Racional |
|---|---|---|
| Teste / desenvolvimento | 1 | Permite rodar localmente sem overhead de cluster. |
| Produção | 3 | Permite quórum de votação e evita decisão de falha por um único nó. |
 
### 6.5 Falha no registro de um novo nó

![Sequência da falha no registro de um novo nó](diagramas/img/falha-registro-no.svg)

*Fonte: [falha-registro-no.json](diagramas/fonte/falha-registro-no.json).*
 
Se o registro de um novo nó **falhar antes do commit** (teste de conexão do bootstrap malsucedido, ou a fase de registro sem confirmação de todos os nós, dentro de `write.commit.timeout` — seção 6.2, passo 7):
 
1. O nó é **excluído** do cluster — não chega a ser efetivado na tabela de membros.
2. Os **blocos** que seriam assumidos por ele são **rebalanceados** entre os nós restantes do cluster (o anel de hash é recalculado sem esse nó).
3. Comportamento de retry (o cliente/nó tenta novamente o bootstrap, com que backoff, etc.): a definir (ver seção 10).
> Diferente da falha descrita na seção 7.3 (mensagem não encontrada durante a sincronização de um bloco), a falha de registro é sobre o **nó como um todo** não conseguir entrar no cluster — por isso o efeito é exclusão + rebalanceamento, e não apenas um alerta.
 
### 6.6 Retorno de um nó excluído

![Sequência do retorno de um nó excluído](diagramas/img/retorno-no-excluido.svg)

*Fonte: [retorno-no-excluido.json](diagramas/fonte/retorno-no-excluido.json).*
 
Quando um nó que foi **desregistrado por falha do canal** (seção 6.3) volta a ficar disponível, ele não simplesmente retoma seu lugar antigo no anel — os blocos que ele atendia já podem ter sido reatribuídos a outros nós. O procedimento:
 
1. Ao voltar, o nó **confere localmente** se possui mensagens cujo ID conste como registrado em algum **evento de perda de mensagem** (seção 7.3) — ou seja, mensagens que ele detinha e que, na sua ausência, podem não ter sido encontradas por outras réplicas.
2. Se encontrar mensagens nessa condição, ele **envia essas mensagens aos nós que são atualmente os novos donos** do bloco correspondente (via o mesmo procedimento de registro/commit da seção 7.2), recuperando dados que estariam perdidos.
3. Concluído esse envio, o nó **limpa seu estado local** (dados dos blocos antigos) e **inicia um novo registro no cluster do zero** — ou seja, refaz o bootstrap completo da seção 6.2, como se fosse um nó novo, e recebe uma nova posição no anel.
### 6.7 Período de freezing (anti-flapping)
 
Para evitar que um nó instável/defeituoso entre e saia repetidamente do cluster (flapping), pode ser configurado um **período de freezing**:
 
- Após um nó ser desregistrado por falha (seção 6.3), ele fica **impedido de se registrar novamente** no cluster até esse período expirar.
- A **duração do período de freezing é configurável**.
> O comportamento exato durante o freezing (o nó é rejeitado no bootstrap? fica em uma "lista de espera"?) e o valor padrão sugerido ainda não foram definidos (ver seção 10).

### 6.8 Concorrência entre registro e desregistro

Registro (bootstrap) e desregistro (votação de exclusão) são as duas operações pesadas do cluster (seção 6). Regra de concorrência:

- **Entradas simultâneas não têm restrição:** vários nós novos podem fazer bootstrap ao mesmo tempo, cada um seguindo a seção 6.2 em paralelo — não há necessidade de serializar registros entre si.
- **Entrada e saída, uma por vez:** enquanto uma **votação de exclusão** de algum nó está em andamento (da abertura da votação até o commit do desregistro, seção 6.3), um **novo pedido de bootstrap fica em espera**; só é processado quando a votação terminar (com ou sem exclusão efetivada).
- **Por quê:** evita que um nó novo assuma posições no anel (seção 7) calculadas sobre uma tabela de membros que está prestes a mudar por uma exclusão, o que exigiria recalcular o rebalanceamento em cima de outro em andamento.

**Caso do nó que reinicia sem memória antes da votação concluir** (seção 12.9 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), D29.6): o processo perdeu o estado e volta com o **mesmo IP e porta**, mas o nó ainda consta na tabela em votação de exclusão. O pedido de bootstrap dele entra na fila e espera o fim da votação, como qualquer outro. Encerrada a votação (o nó antigo é removido da tabela), o pedido é processado como um **registro comum de nó novo** (seção 6.2), sem tratamento especial por ter o mesmo endereço.

