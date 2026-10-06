# Arquitetura em alto nível

[← Índice](README.md)

## 3. Arquitetura em alto nível
 
O sistema é composto por **nós** (nodes) idênticos entre si (sem nó "mestre" fixo), formando um cluster. Cada nó expõe:
 
| Camada | Responsabilidade |
|---|---|
| **API HTTP (cliente)** | Recebe publicações e entregas de mensagens dos clientes. |
| **API HTTP (interna/cluster)** | Comunicação entre nós: registro, propagação de tabela de membros, replicação de mensagens; mais o canal permanente entre nós (sinal de vida e sinais de entrega). |
| **Storage engine** | Armazenamento em memória com overflow para disco (seção 3.2). |
| **Cluster manager** | Descoberta, registro, detecção de falhas, votação de membros e particionamento (hash circular). |
 
![C4 nível 1: contêineres do Sistema de Mensagens](diagramas/img/c4-nivel1-conteineres.svg)

*Contêineres de um nó (C4 nível 1). São camadas lógicas do mesmo processo e da mesma imagem. Fonte: [c4-nivel1-conteineres.json](diagramas/fonte/c4-nivel1-conteineres.json).*

### 3.1 Fluxo de escrita de uma mensagem

![Arquitetura da escrita de uma mensagem no cluster](diagramas/img/arquitetura-escrita.svg)

*Fonte: [arquitetura-escrita.json](diagramas/fonte/arquitetura-escrita.json). A sequência detalhada está na seção 7.2 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md).*
 
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
- Ao atingir o limite de memória configurado, as mensagens excedentes são **persistidas em disco**, um arquivo por mensagem, no formato:
  ```
  ${topico}/${id}.json
  ```
  (uma pasta por tópico, um arquivo JSON por mensagem).
- Ao ler (`GET`) uma mensagem que não está mais em memória, ela é buscada no disco pelo mesmo caminho.
- Os metadados de entrega (contador de entregas e invisibilidade) ficam em um arquivo ao lado, `${topico}/${id}.meta.json` — ver [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).
**Divisão do espaço entre tópicos:**
- O espaço de memória **não é dividido de forma fixa e igual** entre os tópicos do nó.
- A divisão é **dinâmica**, priorizando os tópicos com **maior fluxo de saída/leitura** (consumo) — ou seja, tópicos mais lidos mantêm mais mensagens "quentes" em memória, enquanto tópicos com menor atividade de leitura têm suas mensagens movidas para disco primeiro quando o espaço fica escasso.
- Na prática, cada tópico tem uma métrica de **taxa de leitura** (mensagens consumidas por período), usada para decidir, no momento em que a memória está cheia, quais mensagens (de quais tópicos) devem ser movidas para disco para abrir espaço às novas.
> O algoritmo exato de priorização (ex.: janela de tempo para calcular a taxa de leitura, política de despejo tipo LRU/LFU por tópico, como tratar tópicos novos sem histórico) ainda não foi definido (ver seção 10).

### 3.3 Definição de tópicos: propagação e persistência

A **definição de um tópico** (nome, tipo, tags) é um dado diferente da mensagem: pequeno, de baixa frequência de alteração, e do interesse do **cluster inteiro** — não é particionado pelo hash circular como as mensagens (seção 7). Por isso segue o mesmo tratamento da tabela de membros (seção 6): **todo nó conhece a definição de todos os tópicos.**

- **Propagação:** criação (`POST /topics`), alteração (`PATCH /topics/{topic}`) e remoção (`DELETE /topics/{topic}`) usam o **mesmo protocolo de duas fases** (registro → commit) das mensagens e do registro de nó (seções 7.2 e 6.2): o nó que recebe a chamada propaga o pedido a **todos os nós do cluster** e, confirmado por **todos** (sem quórum reduzido — é operação rara, de baixo volume, sem o caminho crítico de latência que justifica o quórum das mensagens), envia o commit, efetivando a alteração em toda a tabela de tópicos. Só então a API responde ao cliente.
- **Falha:** mesma regra da escrita de mensagens (seção 3.1) — falha no registro é **cancelada** (rollback) e a API retorna erro ao cliente; falha no commit, já decidido, não é desfeita: o coordenador insiste dentro do teto `write.commit.timeout` e, esgotado, retorna erro ao cliente mas continua tentando até confirmar em todos. Nunca uma alteração parcial visível: um nó só passa a servir a nova definição do tópico no commit.
- **Persistência:** ao contrário das mensagens (que só vão a disco por overflow, acima), a definição do tópico é **sempre persistida**, por ser dado de configuração e não de fila — um arquivo por tópico, `${storage.dir}/_topics/${topico}.json`, gravado no commit em cada nó.
- **Volátil vs. persistente:** o arquivo do tópico guarda só a **definição**. Dados de **consumo** — profundidade de fila, mensagens visíveis/invisíveis, taxas (`publishRate`, `consumeRate`), histogramas de latência etc. — nunca vão a esse arquivo; ficam só em memória, específicos de cada nó (ver [11-observabilidade.md](11-observabilidade.md)).
- **Bootstrap:** um nó novo recebe a tabela de tópicos da mesma forma que a tabela de membros (seção 6.2).
- **Remoção com mensagens pendentes:** o `DELETE /topics/{topic}` **sempre funciona**, mesmo com mensagens ainda não entregues ou não confirmadas (ack) no tópico. Cada nó, no commit da remoção, apaga **todas as mensagens do tópico** que tem localmente — em memória, em disco (`${topico}/`) e seus metadados — sem checar se estão pendentes.

