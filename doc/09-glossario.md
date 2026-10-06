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
| **Nó virtual** | Posição adicional que um nó físico ocupa no anel de hash, para melhorar a distribuição dos blocos. Quantidade configurável. |
| **Bootstrap** | Processo de um novo nó ingressar no cluster existente. |
| **Tabela de nós** | Lista de membros conhecidos do cluster (IP/porta de cada nó). |
| **Quórum** | Número mínimo de confirmações necessário para uma decisão. Dois usos distintos no sistema: **quórum de votação** (`cluster.vote.quorum`, padrão 3) para excluir um nó suspeito de falha (seção 6.3); **quórum de escrita** (`write.quorum`, padrão maioria de `replication.factor`) para decidir o commit de uma mensagem sem esperar todas as réplicas (seção 7.2) — não confundir os dois. |
| **Hash circular (consistent hashing)** | Anel de hash usado para mapear cada mensagem a um bloco e, a partir dele, aos nós responsáveis por persisti-la. |
| **Bloco de mensagens** | Partição do anel de hash pela qual um conjunto de nós (primário + réplicas) é responsável. |
| **Réplica primária / secundária / terciária** | Ordem de responsabilidade de um nó por um bloco de mensagens; o número de réplicas exigidas é configurável (padrão: 3). |
| **Protocolo registro + commit** | Padrão de duas fases (propagar/preparar → confirmar) usado tanto para publicar mensagens quanto para registrar/desregistrar nós e recuperar mensagens perdidas. |
| **Rollback** | Descarte de um registro (fase 1) ainda pendente, quando ele falha ou expira antes do **quórum de escrita** fechar. Só se aplica **antes** da decisão de commit; depois disso a operação nunca é desfeita (seção 7.2). |
| **Teto de commit** | Tempo máximo (`write.commit.timeout`) que o coordenador insiste no commit numa réplica **fora do quórum de escrita**, depois de já ter respondido ao cliente; esgotado, continua tentando em segundo plano, sem rollback (seção 7.2). |
| **Sincronização distribuída de bloco** | Processo pelo qual um nó recém-responsável por um bloco reconstrói seu conteúdo puxando dados de todas as réplicas existentes (balanceando a leitura), em vez de um único nó de origem. |
| **Log de deleções pendentes** | Registro das deleções (acks/consumo) recebidas por um bloco ainda em sincronização, aplicadas somente quando a sincronização termina. Junto com a verificação final (seção 7.3), dispensa um tombstone permanente por mensagem deletada. |
| **Rebalanceamento** | Redistribuição dos blocos do anel de hash entre os nós restantes, disparada quando um registro de nó falha (seção 6.5) ou quando um nó é desregistrado (seção 6.3). |
| **Evento de perda de mensagem** | Registro gerado quando uma mensagem esperada não é encontrada em nenhuma réplica durante a sincronização de bloco; usado depois para reconciliar mensagens recuperadas por um nó que retorna (seção 6.6). |
| **Período de freezing** | Intervalo de tempo configurável em que um nó desregistrado por falha fica impedido de se registrar novamente, para evitar flapping. |
| **Overflow para disco** | Persistência em disco (`${topico}/${id}.json`) das mensagens que excedem o espaço de memória configurado para o nó (seção 3.2). |
| **Invisibilidade** | Prazo (`invisibleUntil`) em que uma mensagem já entregue não é entregue a outro consumidor; se não houver ack até lá, ela volta a ser entregável (seções 12 e 13). Padrão: 120 s. |
| **Lista de invisíveis** | Lista, por tópico e só no dono da entrega, dos pares (ID, `invisibleUntil`) das mensagens invisíveis, ordenada por prazo e de tamanho limitado; a thread de vencimento a usa para devolver mensagens à lista de prontos (seções 13.4 e 13.6). |
| **Índice de expiração** | Arquivos em disco (`${topico}/_expiracao/${janela}.jsonl`) com os pares (ID, prazo) que não couberam na lista de invisíveis; derivado dos metadados, com remoção preguiçosa de entradas velhas (seção 13.5). |
| **Contador de entregas** | `deliveryCount`: quantas vezes a mensagem foi entregue; guardado nos metadados (`${topico}/${id}.meta.json` quando em disco). |
| **Dono da entrega** | Nó primário do bloco de uma mensagem; é o único que a coloca na lista de prontos, evitando disputa entre réplicas (seção 12.7). Sempre faz parte do **quórum de escrita** (seção 7.2) — sem isso, uma escrita poderia ser considerada durável antes de estar visível a qualquer consumidor. |
| **Long polling** | `GET` com `waitTime` (até 20 s) que aguarda novas mensagens em vez de responder vazio; o nó registra o pedido como ouvinte do tópico (seção 12.8). |
| **Canal entre nós** | Conexão WebSocket permanente, uma por par de nós e compartilhada por todos os tópicos, que substitui o ping como controle de vida entre nós (seção 6.3) e serve para informar tópicos com ouvintes, tópicos com mensagens prontas e avisar mensagens novas (seção 12.8). |
| **Lista de prontos** | Lista, por tópico e por nó, de IDs de mensagens prontas para entrega, mantida em tamanho fixo por uma thread (seção 12.6). |
| **Taxa de leitura de um tópico** | Métrica de fluxo de consumo (mensagens lidas por período) usada para decidir a prioridade de cada tópico na divisão do espaço de memória disponível. |

