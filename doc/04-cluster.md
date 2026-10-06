# Protocolo de cluster

[← Índice](README.md)

## 6. Protocolo de cluster: descoberta, registro e falhas
 
Decisão de arquitetura definida para a comunicação e o gerenciamento de membros do cluster.
 
Registro e desregistro de nó são operações **pesadas** (afetam o roteamento do anel inteiro), por isso ambos usam o **mesmo protocolo em duas fases (registro → commit)** definido para mensagens na seção 7.2 — garantindo que a operação só é efetivada quando todos os nós confirmam.
 
### 6.1 Comunicação entre nós
- A comunicação entre nós é feita via **API HTTP**.
### 6.2 Registro de um novo nó (bootstrap)
1. O novo nó se conecta a um nó existente do cluster, informando **seus próprios IPs e porta** na mesma chamada síncrona (uma única requisição de registro).
2. O nó que recebe a chamada **testa os IPs** informados, tentando estabelecer conexão de volta com o novo nó.
3. Se a conexão for bem-sucedida, o nó que recebeu envia a **todos os nós do cluster** um pedido de **registro (fase 1)** com os dados do novo nó.
4. Após todos confirmarem a fase de registro, o nó que recebeu envia a **todos os nós** uma chamada de **commit (fase 2)**, efetivando o novo nó na tabela de membros.
5. Só então **responde** à chamada original do novo nó com a **tabela de nós** (já com o commit confirmado em todo o cluster).
6. Ao final do processo, o novo nó já conhece todos os demais membros do cluster e vice-versa.
7. Se o teste de conexão (passo 2) falhar, ou se a fase de registro/commit não for confirmada por todos os nós, o processo é abortado e o novo nó é **excluído** — ver seção 6.5.
8. Ao assumir sua posição no anel (seção 7), o novo nó passa a ser responsável por blocos de mensagem como primário, secundário, terciário etc., conforme o fator de replicação configurado — seguindo o procedimento de sincronização distribuída da seção 7.3.
### 6.3 Detecção de falhas e desregistro
 
1. Todos os nós fazem **ping** periodicamente uns com os outros (checagem em malha/mesh, não centralizada).
2. Quando um nó **não responde** ao ping, o nó que detectou a falha **tenta novamente algumas vezes** (número de tentativas configurável) antes de agir.
3. Se o nó continuar sem responder após as tentativas, é aberta uma **votação de exclusão** entre os nós do cluster.
4. **Quórum mínimo padrão: 3 votos** concordando com a exclusão para efetivar a remoção — valor inicial a ser confirmado/ajustado conforme testes em produção.
5. Atingido o quórum, o desregistro segue o **mesmo protocolo de duas fases**: um pedido de **desregistro (fase 1)** é enviado a todos os nós e, em seguida, um **commit (fase 2)** efetiva a remoção do nó da tabela de membros em todo o cluster.
### 6.4 Tamanho mínimo do cluster
| Ambiente | Nós mínimos | Racional |
|---|---|---|
| Teste / desenvolvimento | 1 | Permite rodar localmente sem overhead de cluster. |
| Produção | 3 | Permite quórum de votação e evita decisão de falha por um único nó. |
 
### 6.5 Falha no registro de um novo nó
 
Se o registro de um novo nó **falhar** (teste de conexão do bootstrap malsucedido, ou a fase de registro/commit não confirmada por todos os nós):
 
1. O nó é **excluído** do cluster — não chega a ser efetivado na tabela de membros.
2. Os **blocos** que seriam assumidos por ele são **rebalanceados** entre os nós restantes do cluster (o anel de hash é recalculado sem esse nó).
3. Comportamento de retry (o cliente/nó tenta novamente o bootstrap, com que backoff, etc.): a definir (ver seção 10).
> Diferente da falha descrita na seção 7.3 (mensagem não encontrada durante a sincronização de um bloco), a falha de registro é sobre o **nó como um todo** não conseguir entrar no cluster — por isso o efeito é exclusão + rebalanceamento, e não apenas um alerta.
 
### 6.6 Retorno de um nó excluído
 
Quando um nó que foi **desregistrado por falha de ping** (seção 6.3) volta a ficar disponível, ele não simplesmente retoma seu lugar antigo no anel — os blocos que ele atendia já podem ter sido reatribuídos a outros nós. O procedimento:
 
1. Ao voltar, o nó **confere localmente** se possui mensagens cujo ID conste como registrado em algum **evento de perda de mensagem** (seção 7.3) — ou seja, mensagens que ele detinha e que, na sua ausência, podem não ter sido encontradas por outras réplicas.
2. Se encontrar mensagens nessa condição, ele **envia essas mensagens aos nós que são atualmente os novos donos** do bloco correspondente (via o mesmo procedimento de registro/commit da seção 7.2), recuperando dados que estariam perdidos.
3. Concluído esse envio, o nó **limpa seu estado local** (dados dos blocos antigos) e **inicia um novo registro no cluster do zero** — ou seja, refaz o bootstrap completo da seção 6.2, como se fosse um nó novo, e recebe uma nova posição no anel.
### 6.7 Período de freezing (anti-flapping)
 
Para evitar que um nó instável/defeituoso entre e saia repetidamente do cluster (flapping), pode ser configurado um **período de freezing**:
 
- Após um nó ser desregistrado por falha (seção 6.3), ele fica **impedido de se registrar novamente** no cluster até esse período expirar.
- A **duração do período de freezing é configurável**.
> O comportamento exato durante o freezing (o nó é rejeitado no bootstrap? fica em uma "lista de espera"?) e o valor padrão sugerido ainda não foram definidos (ver seção 10).

