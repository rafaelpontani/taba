# Escopo

[← Índice](README.md)

## 4. Escopo inicial (dentro do escopo)
 
### 4.1 API
- Criar e configurar tópicos (tipo: entrega única ou broadcast).
- Enviar mensagem para um tópico (`POST`).
- Receber mensagens de um tópico (`GET`).
- Confirmar o consumo de uma mensagem (ack).
### 4.2 Armazenamento e durabilidade
- Recebimento inicial da mensagem em memória, com **espaço configurável** e **overflow para disco** (`${topico}/${id}.json`) quando o limite é atingido — ver seção 3.2.
- Espaço de memória dividido **dinamicamente entre tópicos**, priorizando os de maior fluxo de leitura (seção 3.2).
- Replicação/confirmação nos nós responsáveis pelo bloco de mensagens (primário, secundário, terciário...).
- Retry configurável do registro e do commit e, esgotado, erro ao cliente + rollback nos nós responsáveis (seção 3.1).
- Política configurável: número de réplicas exigidas para a escrita ser considerada durável (padrão: 3).
### 4.3 Cluster
- Descoberta automática de nós.
- Comunicação interna entre nós **via API HTTP**.
- Entrada e saída de nós usando o **mesmo protocolo de registro/commit** das mensagens (ver seção 6.2/6.3), por serem operações pesadas que afetam o roteamento de todo o anel.
- Falha no registro de um nó **exclui o nó** e dispara **rebalanceamento dos blocos** (ver seção 6.5).
- Detecção de falhas com **retries de ping antes de votar**, quórum mínimo padrão de **3 votos**, e reintegração controlada de nós que voltam (ver seção 6.3/6.6).
- **Período de freezing configurável** para evitar nós instáveis entrando e saindo repetidamente do cluster (ver seção 6.7).
- Sincronização de blocos **escopada e distribuída** entre as réplicas existentes, sem bloquear o cluster inteiro (ver seção 7.3).
- Tamanho mínimo do cluster: **1 nó** (ambiente de teste/dev) ou **3 nós** (produção).
- Particionamento das mensagens por **hash circular**, com nós virtuais configuráveis (ver seção 7).
### 4.4 Distribuição e operação
- Imagem Docker única e leve.
- Integração nativa com Kubernetes (descoberta de nós, health checks, manifestos/Helm).
## 5. Fora do escopo (por enquanto)
 
- Interface gráfica de administração.
- Autenticação e autorização avançadas.
- Suporte a protocolos adicionais (AMQP, MQTT, Kafka protocol).
- Processamento de streams e reprocessamento histórico de mensagens.
- Multi-região.
> Itens fora do escopo podem ser reavaliados nas próximas versões.

