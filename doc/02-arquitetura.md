# Arquitetura em alto nível

[← Índice](README.md)

## 3. Arquitetura em alto nível
 
O sistema é composto por **nós** (nodes) idênticos entre si (sem nó "mestre" fixo), formando um cluster. Cada nó expõe:
 
| Camada | Responsabilidade |
|---|---|
| **API HTTP (cliente)** | Recebe publicações e entregas de mensagens dos clientes, na porta `api.port`. |
| **API HTTP (interna/cluster)** | Comunicação entre nós: registro, propagação de tabela de membros, replicação de mensagens; mais o canal permanente entre nós (sinal de vida e sinais de entrega). Fica numa **porta própria** (`cluster.port`), separada da API de cliente, para o acesso ser restrito na rede ([06-api.md](06-api.md), seção 8.0). |
| **Storage engine** | Armazenamento em memória com overflow para disco (seção 3.2). |
| **Cluster manager** | Descoberta, registro, detecção de falhas, votação de membros e particionamento (hash circular). |
 
![C4 nível 1: contêineres do Sistema de Mensagens](diagramas/img/c4-nivel1-conteineres.svg)

*Contêineres de um nó (C4 nível 1). São camadas lógicas do mesmo processo e da mesma imagem. Fonte: [c4-nivel1-conteineres.json](diagramas/fonte/c4-nivel1-conteineres.json).*

### 3.1 Fluxo de escrita de uma mensagem

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): o nó é um cluster de um só: `replication.factor` e `write.quorum` valem 1, e o registro e o commit acontecem só nele. Não há réplica, retry entre nós nem rollback por falta de quórum.

![Arquitetura da escrita de uma mensagem no cluster](diagramas/img/arquitetura-escrita.svg)

*Fonte: [arquitetura-escrita.json](diagramas/fonte/arquitetura-escrita.json). A sequência detalhada está na seção 7.2 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md).*
 
> **Fila FIFO** ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16): o bloco vem do grupo (`id` do tópico + `groupId`), e o nó que recebe a chamada a **repassa ao primário do grupo**, que passa a ser o coordenador, gera o ID e exige o primário e o secundário no quórum.

1. Cliente envia mensagem via API HTTP para um nó qualquer do cluster (o nó **coordenador** da requisição).
2. O nó coordenador calcula, pelo ID da mensagem, os **nós responsáveis pelo bloco** (primário + réplicas) e **encaminha internamente** a mensagem a eles — ver o procedimento detalhado na seção 7. O coordenador pode ser ele mesmo um dos nós de destino, mas provavelmente não é o único.
3. **Fase 1 (registro):** o coordenador envia a mensagem a **todos os nós responsáveis** (`replication.factor` réplicas); cada um a grava **em memória** (caminho crítico, baixa latência) como pendente e confirma o recebimento.
4. **Fase 2 (commit), decidida por quórum:** assim que um **quórum mínimo** de confirmações do registro é atingido — o **primário sempre incluso**, mais `write.quorum - 1` de quaisquer outras réplicas (nunca um quórum sem o primário: só ele decide o que é entregável, seção 12.7) —, o coordenador **decide commitar** e chama a rota de commit nos nós do quórum, sem esperar as demais réplicas.
5. **Falha antes do quórum ser fechado:** o coordenador **repete a chamada** ao nó que falhou, conforme a configuração de retry (`write.retry.*`). Esgotados os retries **sem quórum fechado**, a API retorna **erro ao cliente** e o coordenador envia a todos os nós que já haviam confirmado um **rollback**, para descartar o registro pendente — a operação ainda pode ser abortada sem risco, pois nenhum nó efetivou nada de forma que outro dependa dele. Mesmo sem o rollback, um registro sem commit seria descartado sozinho após `write.pending.ttl`; o rollback apenas **acelera a liberação de recursos**.
6. **Falha nas réplicas restantes (depois do quórum decidido):** a decisão de commitar **não volta atrás** — o quórum (com o primário) já pode ter efetivado a mensagem, e um rollback nessa altura o deixaria inconsistente com o resto do bloco. Se registro ou commit falharem numa réplica fora do quórum, o coordenador **insiste** (nunca rollback) dentro de um teto configurável (`write.commit.timeout`); esgotado sem confirmação nessa réplica, o bloco fica temporariamente abaixo do fator de replicação pleno (nunca abaixo do quórum) — evento de observabilidade, não falha de saúde do nó, retry continua em segundo plano — ver detalhamento e justificativa na seção 7.2.
7. **Sucesso:** a resposta ao cliente com o ID da mensagem sai assim que o **quórum confirma o commit** (primário incluso) — não é preciso esperar `replication.factor` réplicas inteiras. A durabilidade "plena" (todas as réplicas com commit confirmado) é alcançada em seguida, em segundo plano quando necessário.
### 3.2 Gerenciamento de memória e overflow para disco
 
O espaço em memória destinado às mensagens em cada nó é **configurável** (ex.: um limite de MB/GB total por nó).

**Overflow para disco:**
- Ao atingir o limite de memória configurado, as mensagens excedentes são **persistidas em disco**: o corpo em um arquivo por mensagem, no formato:
  ```
  ${topic}/${id}.body
  ```
  (uma pasta por tópico, um arquivo por mensagem, só com os **bytes do corpo, como vieram**, sem reserializar nem codificar; o formato do corpo está em `contentType`, abaixo, e o JSON ou base64 só existe na resposta e entre nós, [06a-api-cliente.md](06a-api-cliente.md), "O corpo da mensagem").
- Ao ler (`GET`) uma mensagem que não está mais em memória, ela é buscada no disco pelo mesmo caminho.
- Os metadados de entrega (contador de entregas, invisibilidade e `receiptToken`) **não ficam em arquivo ao lado**: ficam no **banco SQLite local do tópico** (abaixo) — ver [14-invisibilidade.md](14-invisibilidade.md).

**Banco SQLite local, por nó e por tópico:**
- Cada nó mantém, para cada tópico, um banco SQLite em `${topic}/_topic.db`. É **estritamente local**: serve só como gerenciador de disco daquele nó, **nunca é copiado, enviado nem sincronizado** com outros nós (o que se replica entre nós são as mensagens e os metadados pelo protocolo de registro + commit, não o arquivo).
- O **corpo** da mensagem continua fora do banco, em `${topic}/${id}.body`. O banco guarda o resto: a tabela `messages` (uma linha por mensagem em disco: `id`, posição no anel, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`; na fila FIFO, também `deduplicationId` e `deleteAt`, com índices por `(group_id, deduplicationId)` e por `deleteAt`, [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7) e a tabela `expiry` (excedente da lista de invisíveis, [14-invisibilidade.md](14-invisibilidade.md), seção 13.5), com índices por `id`, por `invisibleUntil`, pela posição no anel e por `(group_id, id)` (entrega justa, seção 12.11 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).
- **Escritor único:** toda **escrita** no banco de um tópico passa por **uma única thread** (na prática, uma fila de comandos por tópico, executada uma de cada vez, sem exigir uma thread de sistema por tópico). Leituras longas, como os streams de sincronização, usam conexões de leitura próprias (modo WAL) e não bloqueiam o escritor.
- **Ordem de gravação:** na escrita, primeiro o arquivo do corpo, depois a linha; no ack, primeiro a linha, depois o arquivo. Uma queda deixa no máximo arquivo sem linha ou linha sem arquivo, e o nó que reinicia volta como novo e refaz a partir do cluster (seção 6.6 de [04-cluster.md](04-cluster.md)).
- **Consultas que o banco responde:** a mensagem mais antiga do tópico (`MIN(id)`, para `oldestMessageAt`, somada à mais antiga em memória, que também é mantida ordenada por ID), os próximos a vencer (índice por `invisibleUntil`), as mensagens de um bloco (faixa pela posição no anel, para os streams da seção 7.3 e para a limpeza de blocos perdidos) e as contagens do tópico.
- **Banco do nó:** além dos bancos por tópico, o nó tem `${storage.dir}/_node.db`, também estritamente local, com a tabela `topics` (as definições de tópico, abaixo) e a tabela `losses` (`topic`, `id`, `block`, `detected_at`) das mensagens declaradas perdidas na sincronização, consultada pelo nó que volta de uma exclusão (seção 6.6 de [04-cluster.md](04-cluster.md)). A escrita também passa por uma thread única.
- **Banco corrompido:** o nó entra em **falha**, o mesmo estado da falha de disco da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md): ele sinaliza ao cluster, que fica liberado para votar o desregistro dele (seção 6.3 de [04-cluster.md](04-cluster.md)). Não há tentativa de reparo.
- **Modo drain:** o limite de memória efetivo cai a zero e todas as mensagens em memória vão para o disco (seção 6.10 de [04-cluster.md](04-cluster.md)).
- A **lista de prontos e as mensagens em memória não mudam**: a prioridade é velocidade e integridade, e o banco só cuida do que está em disco.
- Parâmetro: `storage.db.synchronous` (modo de durabilidade do SQLite; padrão `NORMAL` com WAL, já que a durabilidade vem da replicação entre nós).


**Divisão do espaço entre tópicos:**
- O espaço de memória **não é dividido de forma fixa e igual** entre os tópicos do nó.
- A divisão é **dinâmica**, priorizando os tópicos com **maior fluxo de saída/leitura** (consumo) — ou seja, tópicos mais lidos mantêm mais mensagens "quentes" em memória, enquanto tópicos com menor atividade de leitura têm suas mensagens movidas para disco primeiro quando o espaço fica escasso.
- Na prática, cada tópico tem uma métrica de **taxa de leitura** (mensagens consumidas por período), usada para decidir, no momento em que a memória está cheia, quais mensagens (de quais tópicos) devem ser movidas para disco para abrir espaço às novas.
**Algoritmo de priorização (decidido):**
- **Taxa de leitura:** média móvel das mensagens consumidas por segundo numa janela de **5 minutos** (`memory.rate.window`, configurável). A mesma janela calcula `publishRate` e `consumeRate` ([11-observabilidade.md](11-observabilidade.md)).
- **Reserva mínima por tópico:** cada tópico tem direito a uma reserva em memória (`memory.topic.reserve`, por padrão o tamanho da lista de prontos, 1000 mensagens), **independente da taxa**. Assim um tópico novo, sem histórico de leitura, não começa jogando as primeiras mensagens em disco. A divisão pela taxa de leitura vale só para o resto da memória, acima das reservas.
- **O que sai da memória:** quando a memória enche, as mensagens que ficam são as da **lista de prontos** (as próximas a serem entregues, seção 12.6 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)) e as **recém-chegadas**. O resto é candidato a ir para disco, começando pelo tópico de **menor taxa de leitura**. Não se usa LRU/LFU: cada mensagem é lida uma única vez e depois apagada pelo ack, então histórico de acesso não prediz nada, e a ordem de entrega já diz o que será lido em breve.

### 3.3 Definição de tópicos: propagação e persistência

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a definição é aplicada só no próprio nó, sem esperar outros nós. Não há bootstrap nem carga de tópicos de outro nó.

A **definição de um tópico** (nome, `id` GUID v7, tipo, tags; `prefixes` no Tópico Distribuidor; `volatile` e `idleTimeout` na fila — [15-topicos-distribuidor.md](15-topicos-distribuidor.md); `fifo` na fila, imutável — [17-topicos-fifo.md](17-topicos-fifo.md); `maxMessageBytes`, `ttl` e `maxDeliveries` opcionais, [12-limites-api.md](12-limites-api.md) e seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)) é um dado diferente da mensagem: pequeno, de baixa frequência de alteração, e do interesse do **cluster inteiro** — não é particionado pelo hash circular como as mensagens (seção 7). Por isso segue o mesmo tratamento da tabela de membros (seção 6): **todo nó conhece a definição de todos os tópicos.**

- **Propagação (dono da definição, D123):** cada tópico tem um **dono** no anel: o primeiro nó ativo na posição do hash de `topic:<nome>`. Criação (`POST /topics`), alteração (`PATCH /topics/{topic}`) e remoção (`DELETE /topics/{topic}`) são **propostas ao dono** pelo nó que recebe a chamada (`POST /cluster/definitions/propose`). O dono confere a mudança com o estado dele (nome que já existe, tópico desconhecido, `expectedId`), dá a ela a próxima **versão**, grava num **herdeiro** (o primeiro outro dono da posição que responde; se há outros donos e nenhum responde, a mudança é recusada com `503`) e a aplica em si. Em seguida manda a mudança decidida (`POST /cluster/definitions/apply`) a todos os outros nós e responde ao cliente depois dos que respondem dentro de `write.commit.timeout`; nos demais **insiste**, com a versão mais nova, até eles confirmarem ou saírem do cluster. Todo nó guarda a definição completa e a lê localmente; só a escrita passa pelo dono. Cada nó aplica só uma versão **maior ou igual** à que tem, então repetir ou chegar fora de ordem não muda o resultado, e a remoção deixa uma **lápide** com a versão para uma mudança atrasada não ressuscitar o tópico. Com o dono fora do ar e ainda na tabela, a mudança falha com `503` até a exclusão dele (D107). Substitui o registro + commit com confirmação de todos os nós (seção 7.2 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) para tópicos e tokens. O nó que entra lê também as lápides (`GET /cluster/definitions/tombstones`, D125) para conhecer as versões já usadas de definições removidas, e duas cópias de mesma versão e conteúdo diferente ficam com a de menor impressão (D124).
- **Falha:** mesma regra da escrita de mensagens (seção 3.1) — falha no registro é **cancelada** (rollback) e a API retorna erro ao cliente; falha no commit, já decidido, não é desfeita: o coordenador insiste dentro do teto `write.commit.timeout` e, esgotado, retorna erro ao cliente mas continua tentando até confirmar em todos. Nunca uma alteração parcial visível: um nó só passa a servir a nova definição do tópico no commit.
- **Persistência:** ao contrário das mensagens (que só vão a disco por overflow, acima), a definição do tópico é **sempre persistida**, por ser dado de configuração e não de fila — uma linha por tópico na tabela `topics` do `_node.db` (`nome`, chave; `definicao`, o JSON da definição), gravada no commit em cada nó, numa transação do SQLite (atômica: nunca uma linha pela metade). Não há arquivo por tópico. O SQLite compara o nome byte a byte, então `Pedidos` e `pedidos` são tópicos diferentes em qualquer disco; só a pasta `${topic}/` das mensagens, que é arquivo, precisa de cuidado com maiúsculas: cada letra maiúscula do nome vira `~` mais a minúscula (`Pedidos` é `~pedidos/`).
- **Fila de dead-letter:** não tem definição própria. É derivada da definição da fila de origem, criada e excluída na mesma operação, e não gera linha em `topics` (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)).
- **Volátil vs. persistente:** a tabela `topics` guarda só a **definição**. Dados de **consumo** — profundidade de fila, mensagens visíveis/invisíveis, taxas (`publishRate`, `consumeRate`), histogramas de latência etc. — nunca vão a essa tabela; ficam só em memória, específicos de cada nó (ver [11-observabilidade.md](11-observabilidade.md)).
- **Bootstrap:** um nó novo recebe a tabela de membros na resposta do registro (seção 6.2) e **puxa a tabela de tópicos** de um nó do cluster (a origem é o nó de **menor carga** segundo o indicador do sinal de vida, seção 12.8 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md); empatados ou sem dado de carga ainda, um nó **sorteado**) por `GET /cluster/topics`, um stream NDJSON ordenado por `id` ([06c-api-cluster.md](06c-api-cluster.md), seção 8.2.1). Como o nó já está na tabela de membros, ele recebe normalmente o registro e o commit de qualquer criação, alteração ou remoção que ocorra durante a carga, e para isso valem estas regras:
  - A carga é aplicada direto no armazenamento final, e tudo o que chega por registro e commit (criação, alteração ou remoção de tópico) vai para o **banco de eventos** (SQLite local), na ordem de chegada; o nó confirma o registro depois de gravar. Nada é aplicado na hora ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3).
  - Depois da carga, o banco de eventos é processado **por cima**, linha a linha, apagando as processadas: o evento vence a carga, e reaplicar um evento que a carga já refletia não muda nada. Por isso o `commit` não precisa carregar a definição completa nem há um registro de remoções pendentes: a remoção no banco apaga o que a carga trouxe. Uma mensagem de um tópico ainda desconhecido espera no mesmo banco.
  - **Verificação final**, como na seção 7.3: ao terminar, o nó repete a listagem e descarta o que não aparece mais na origem, cobrindo a remoção cujo commit não chegou a ele.
  - Se o nó de origem falhar no meio, a carga continua em outro nó, a partir do último `id` lido.
  - Até a sincronização inteira terminar (tópicos, blocos e logs) o nó **não atende rotas de cliente** (`/health/ready` falha, [11-observabilidade.md](11-observabilidade.md)): `GET /topics`, o casamento de prefixos do distribuidor e o cálculo do nó gestor dependem da tabela completa.
- **Remoção com mensagens pendentes:** o `DELETE /topics/{topic}` **sempre funciona**, mesmo com mensagens ainda não entregues ou não confirmadas (ack) no tópico. Cada nó, no commit da remoção, apaga **todas as mensagens do tópico** que tem localmente — em memória, em disco (`${topic}/`) e seus metadados — sem checar se estão pendentes.

### 3.4 Tokens de acesso: propagação e persistência

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a autenticação fica desligada e não há tokens.

Com a autenticação ligada (há um token raiz), o **token de acesso** da API de cliente ([06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3) é, como a definição de tópico, um dado pequeno, de baixa frequência de alteração e do interesse do cluster inteiro. Segue **o mesmo tratamento da seção 3.3**:

- **Todo nó conhece todos os tokens**, para validar uma chamada por consulta local em memória, sem rede.
- **Propagação:** o mesmo dono e a mesma versão da seção 3.3, com o `id` do token como chave (posição `token:<id>`). Criação, alteração e revogação passam pelo dono (`POST /cluster/definitions/propose`), que as espalha por `apply` levando só o **sal** e o **hash salgado**, nunca o segredo. A revogação vale na hora nos nós que respondem; o nó que estava fora do ar a recebe quando volta, porque o dono insiste nele (D123).
- **Persistência:** uma linha por token na tabela `tokens` do `_node.db` (seção 3.2), gravada no commit em cada nó, como a definição de tópico. Guarda `id`, `name`, permissões, prefixos, datas, o **sal** (aleatório, 128 bits, um por token) e o **hash salgado do segredo**, `SHA-256(sal ‖ segredo)`; o segredo nunca é guardado nem trafega entre nós (só o criador o recebe, na resposta da criação, do nó que coordenou).
- **Na partida:** fora do modo desenvolvimento o nó **não confia** nos tokens que acha no disco: eles podem estar dessincronizados do cluster (token revogado ou criado enquanto o nó esteve fora). A tabela `tokens` local só é relida em desenvolvimento com `development.resume-on-start`; em produção os tokens vêm do cluster (abaixo).
- **Bootstrap:** o nó novo puxa os tokens por `GET /cluster/tokens` (stream NDJSON ordenado por `id`, seção 8.2.1 de [06c-api-cluster.md](06c-api-cluster.md)) **junto com a tabela de tópicos**, da mesma origem e com as mesmas regras: carga aplicada direto, eventos que chegam no meio guardados no banco de eventos e processados por cima, verificação final ao terminar ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.3). Até a carga terminar o nó não atende rotas de cliente, então nunca valida um token com a tabela incompleta.
- **Expiração:** `expiresAt` é conferido localmente, e cada nó apaga os vencidos na varredura (`retention.sweep.interval`), sem propagar.

