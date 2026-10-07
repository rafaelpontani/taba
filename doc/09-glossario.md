# Glossário

[← Índice](README.md)

## 11. Glossário
 
| Termo | Significado |
|---|---|
| **Tópico** | Canal nomeado para o qual mensagens são publicadas. Pode ter tags, enviadas como atributos ao OpenTelemetry. A definição é replicada em todos os nós por registro + commit e persistida em `${topico}.json` (seção 3.3). |
| **Tópico Fila (QT, *Queue Topic*)** | Análogo do SQS. Tipo de tópico (`queue`) que guarda mensagens e as entrega a consumidores, uma mensagem para um consumidor (seção 14.1). |
| **Tópico Distribuidor (DT, *Distributor Topic*)** | Análogo do SNS. Tipo de tópico (`distributor`) que não guarda mensagens: copia cada publicação para os QTs cujo nome começa com um de seus `prefixes` (seção 14.2). |
| **Fila volátil (QTV, *Queue Topic Volatile*)** | QT com `volatile: true`: excluída, com suas mensagens, se ficar sem consumidor por mais de `idleTimeout` (padrão 10 min). Criável pelo próprio `GET` com `autoCreate` (seções 14.3 e 14.4). |
| **Nó gestor** | Nó primário do hash do `id` de uma fila volátil no anel; cuida só da vida do tópico (decide a exclusão por ociosidade), nunca das mensagens (seção 14.5). |
| **Verificação final (ociosidade)** | Consulta do gestor a todos os nós, antes de excluir uma fila volátil, sobre há quanto tempo foi o último `GET` do tópico em cada um (seção 14.5). |
| **`id` do tópico** | GUID v7 gerado na criação do tópico; identifica a encarnação (recriar o mesmo nome gera outro `id`) e define o nó gestor pelo hash no anel (seção 14.3). |
| **Nó (node)** | Instância do sistema que participa do cluster. |
| **Modo desenvolvimento** | `mode=development`: o nó é um cluster de um só, com votação, quórum, canal entre nós, replicação e sincronização desabilitados (seção 6.4). Para teste e desenvolvimento, nunca produção. |
| **Modo drain** | Estado do nó (`draining`, `drained`) em que todas as mensagens vão para o disco e o nó sai da rotação; entra sozinho ao receber `SIGTERM`/`SIGINT` ou por `POST /node/drain`, repassa as chamadas de cliente ao nó mais livre, e termina com o **desregistro voluntário** do nó, sem votação. Quem volta do drain pela API entra como nó novo (seção 6.10). No modo desenvolvimento serve de **save**: a partida seguinte retoma do disco. |
| **Nó virtual** | Posição adicional que um nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. Quantidade configurável. |
| **Bootstrap** | Processo de um novo nó ingressar no cluster existente. |
| **Tabela de nós** | Lista de membros conhecidos do cluster (IP/porta de cada nó). |
| **Quórum** | Número mínimo de confirmações necessário para uma decisão. Dois usos distintos no sistema: **quórum de votação** (`cluster.vote.quorum-percent`, padrão 51% dos nós saudáveis) para excluir um nó suspeito de falha (seção 6.3); **quórum de escrita** (`write.quorum`, padrão maioria de `replication.factor`) para decidir o commit de uma mensagem sem esperar todas as réplicas (seção 7.2) — não confundir os dois. |
| **Hash circular (consistent hashing)** | Anel de hash usado para mapear cada mensagem a um bloco e, a partir dele, aos nós responsáveis por persisti-la. |
| **Bloco de mensagens** | Partição do anel de hash pela qual um conjunto de nós (primário + réplicas) é responsável. |
| **Réplica primária / secundária / terciária** | Ordem de responsabilidade de um nó por um bloco de mensagens; o número de réplicas exigidas é configurável (padrão: 3). |
| **Protocolo registro + commit** | Padrão de duas fases (propagar/preparar → confirmar) usado tanto para publicar mensagens quanto para registrar/desregistrar nós e recuperar mensagens perdidas. |
| **Rollback** | Descarte de um registro (fase 1) ainda pendente, quando ele falha ou expira antes do **quórum de escrita** fechar. Só se aplica **antes** da decisão de commit; depois disso a operação nunca é desfeita (seção 7.2). |
| **Teto de commit** | Tempo máximo (`write.commit.timeout`) que o coordenador insiste no commit numa réplica **fora do quórum de escrita**, depois de já ter respondido ao cliente; esgotado, continua tentando em segundo plano, sem rollback (seção 7.2). |
| **Sincronização distribuída de bloco** | Processo pelo qual um nó recém-responsável por um bloco reconstrói seu conteúdo puxando dados de todas as réplicas existentes (balanceando a leitura), em vez de um único nó de origem. |
| **Banco de eventos (sincronização)** | Banco SQLite local (`_sync/sync.db`) onde um nó em sincronização guarda, na ordem de chegada, os eventos do protocolo (tópicos, mensagens, invisibilidade, acks) que não pode processar ainda. Depois da carga, é processado linha a linha, apagando as processadas; quando esvazia, o nó passa a processar em tempo real e o arquivo é descartado. Junto com a verificação final (seção 7.3), dispensa um tombstone permanente por mensagem deletada. |
| **Rebalanceamento** | Redistribuição dos blocos do anel de hash entre os nós restantes, disparada quando um registro de nó falha (seção 6.5) ou quando um nó é desregistrado (seção 6.3). |
| **Evento de perda de mensagem** | Registro, só no log da aplicação (nível `error`, com tópico e ID), gerado quando uma mensagem listada não é encontrada em nenhuma réplica durante a sincronização de bloco, depois de tentar outro nó, a rota de ID único e uma relistagem (seção 7.3). |
| **Período de freezing** | Intervalo de tempo configurável em que um nó desregistrado por falha fica impedido de se registrar novamente, para evitar flapping. |
| **Overflow para disco** | Persistência em disco (`${topico}/${id}.json`) das mensagens que excedem o espaço de memória configurado para o nó (seção 3.2). |
| **Invisibilidade** | Prazo (`invisibleUntil`) em que uma mensagem já entregue não é entregue a outro consumidor; se não houver ack até lá, ela volta a ser entregável (seções 12 e 13). Padrão: 120 s. |
| **Lista de invisíveis** | Lista, por tópico e só no dono da entrega, dos pares (ID, `invisibleUntil`) das mensagens invisíveis, ordenada por prazo e de tamanho limitado; a thread de vencimento a usa para devolver mensagens à lista de prontos (seções 13.4 e 13.6). |
| **Tabela de expiração** | Tabela `expiracao` do banco SQLite local do tópico, com os pares (ID, prazo) que não couberam na lista de invisíveis; indexada por prazo e derivada dos metadados (seção 13.5). |
| **Contador de entregas** | `deliveryCount`: quantas vezes a mensagem foi entregue; guardado nos metadados (no banco SQLite local do tópico quando a mensagem está em disco). |
| **Dead-letter** | Fila `{fila}-dead-letter` (sufixo configurável) que toda fila tem, criada e excluída com ela. Recebe a mensagem que atinge `maxDeliveries` sem ack. Sem definição própria: as configurações são as da fila de origem, somente leitura; no mais, é um tópico comum (seção 12.10). |
| **TTL (`ttl`)** | Tempo de vida da mensagem, por fila (padrão 14 dias, `retention.ttl.default`). Vencida, a mensagem é descartada, sem ir ao dead-letter (seção 12.10). |
| **`maxDeliveries`** | Limite de entregas de uma mensagem sem ack, por fila (padrão 10, `retention.max-deliveries.default`); atingido, a mensagem vai ao dead-letter (seção 12.10). |
| **Grupo (`groupId`)** | Texto opcional da mensagem (até 128 caracteres) que identifica o remetente ou a carga; cada dono conta as mensagens prontas e em processamento por grupo e a entrega prefere os grupos menores, para um produtor barulhento não atrasar os demais (seção 12.11). |
| **Entrega justa** | Preferência da entrega pelas mensagens de grupos menores (tamanho = em processamento + `delivery.fair.ready-weight` × prontas), por faixas de potência de 2, com uma entrega em cada `delivery.fair.normal-every` pela ordem normal (seção 12.11). |
| **Dono da entrega** | Nó primário do bloco de uma mensagem; é o único que a coloca na lista de prontos, evitando disputa entre réplicas (seção 12.7). Sempre faz parte do **quórum de escrita** (seção 7.2) — sem isso, uma escrita poderia ser considerada durável antes de estar visível a qualquer consumidor. |
| **Long polling** | `GET` com `waitTime` (até 20 s) que aguarda novas mensagens em vez de responder vazio; o nó registra o pedido como ouvinte do tópico (seção 12.8). |
| **Canal entre nós** | Conexão WebSocket permanente, uma por par de nós e compartilhada por todos os tópicos, que substitui o ping como controle de vida entre nós (seção 6.3) e serve para informar tópicos com ouvintes, tópicos com mensagens prontas e avisar mensagens novas (seção 12.8). |
| **Lista de prontos** | Lista, por tópico e por nó, de IDs de mensagens prontas para entrega, mantida em tamanho fixo por uma thread (seção 12.6). |
| **Taxa de leitura de um tópico** | Métrica de fluxo de consumo (mensagens lidas por período) usada para decidir a prioridade de cada tópico na divisão do espaço de memória disponível. |

