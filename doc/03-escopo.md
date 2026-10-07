# Escopo

[← Índice](README.md)

## 4. Escopo inicial (dentro do escopo)
 
### 4.1 API
- Criar e configurar tópicos (tipo: Tópico Fila ou Tópico Distribuidor).
- **Filas voláteis** (excluídas sozinhas após `idleTimeout` sem consumidor), criáveis pelo próprio `GET` (`autoCreate`), e **Tópicos Distribuidores** que copiam a publicação para as filas cujo nome começa com um de seus prefixos, com um **nó gestor** por fila volátil — ver [15-topicos-distribuidor.md](15-topicos-distribuidor.md).
- **Filas FIFO** (`fifo: true`): ordem de entrega por grupo (`groupId`), uma mensagem por grupo em voo, e deduplicação das publicações por `deduplicationId` (ou pelo hash do corpo) numa janela; o grupo fica atrelado a um nó primário, que o ordena — ver [17-topicos-fifo.md](17-topicos-fifo.md).
- Enviar mensagem para um tópico (`POST`).
- Receber mensagens de um tópico (`GET`).
- Confirmar o consumo de uma mensagem (ack).
- **Autenticação por token** (opcional: liga sozinha quando há um token raiz): tokens de acesso criados e revogados por API, com as permissões `publish`, `consume`, `create` e `admin` e prefixos de tópico, replicados em todos os nós — ver [06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3.
- Controle de entrega por **invisibilidade temporária** (padrão 120 s, ajustável no `GET` e depois por `PUT .../visibility`), com contador de entregas e reentrega se não houver ack, e **long polling** de até 20 s, com aviso entre nós por canal permanente — ver [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).
### 4.2 Armazenamento e durabilidade
- Recebimento inicial da mensagem em memória, com **espaço configurável** e **overflow para disco** (`${topico}/${id}.body`) quando o limite é atingido — ver seção 3.2.
- Espaço de memória dividido **dinamicamente entre tópicos**, priorizando os de maior fluxo de leitura (seção 3.2).
- Replicação/confirmação nos nós responsáveis pelo bloco de mensagens (primário, secundário, terciário...).
- Retry configurável do registro e do commit; esgotado o retry sem o **quórum mínimo** fechado, erro ao cliente + rollback nos nós responsáveis; esgotado o teto do commit nas réplicas fora do quórum (`write.commit.timeout`), sem rollback — o commit já decidido continua sendo tentado em segundo plano (seção 3.1/7.2).
- Política configurável em duas camadas: número **total** de réplicas (`replication.factor`, padrão 3) e **quórum mínimo** de confirmação para responder ao cliente (`write.quorum`, padrão maioria — ex. 2 em 3), sempre incluindo o primário do bloco.
### 4.3 Cluster
- Descoberta automática de nós.
- Comunicação interna entre nós **via API HTTP**.
- Entrada e saída de nós usando o **mesmo protocolo de registro/commit** das mensagens (ver seção 6.2/6.3), por serem operações pesadas que afetam o roteamento de todo o anel.
- Falha no registro de um nó **exclui o nó** e dispara **rebalanceamento dos blocos** (ver seção 6.5).
- Detecção de falhas com **canal permanente entre nós como sinal de vida (reconexão antes de votar)**, votação de exclusão por **51% dos nós saudáveis**, e reintegração controlada de nós que voltam (ver seção 6.3/6.6).
- **Período de freezing configurável** para evitar nós instáveis entrando e saindo repetidamente do cluster (ver seção 6.7).
- Sincronização de blocos **escopada e distribuída** entre as réplicas existentes, sem bloquear o cluster inteiro (ver seção 7.3).
- Tamanho mínimo do cluster: **1 nó** (ambiente de teste/dev, no **modo desenvolvimento**, em que votação, quórum, canal entre nós e replicação ficam desabilitados; seção 6.4) ou **3 nós** (produção).
- Particionamento das mensagens por **hash circular**, com nós virtuais configuráveis (ver seção 7).
### 4.4 Distribuição e operação
- Imagem Docker única e leve.
- Integração nativa com Kubernetes (descoberta de nós, health checks, manifestos/Helm).
- **Conectores** simples, sem dependências e com suporte a versões de até 15 anos (desde que falem TLS 1.2 ou superior), em C#, Java, Rust, C, C++, Python, PHP, Ruby, Go e Node.js, na pasta `connectors/` — ver [19-conectores.md](19-conectores.md).
## 5. Fora do escopo (por enquanto)
 
- Interface gráfica de administração.
- Autenticação e autorização avançadas (OAuth/OIDC, políticas por tópico individual ou por operação); o escopo atual é o token simples da seção 4.1.
- Suporte a protocolos adicionais (AMQP, MQTT, Kafka protocol).
- Processamento de streams e reprocessamento histórico de mensagens.
- Multi-região.
> Itens fora do escopo podem ser reavaliados nas próximas versões.

