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
 
### 3.1 Fluxo de escrita de uma mensagem
 
1. Cliente envia mensagem via API HTTP para um nó qualquer do cluster (o nó **coordenador** da requisição).
2. O nó coordenador calcula, pelo ID da mensagem, os **nós responsáveis pelo bloco** (primário + réplicas) e **encaminha internamente** a mensagem a eles — ver o procedimento detalhado na seção 7. O coordenador pode ser ele mesmo um dos nós de destino, mas provavelmente não é o único.
3. **Fase 1 (registro):** o coordenador envia a mensagem a **todos os nós responsáveis**; cada um a grava **em memória** (caminho crítico, baixa latência) como pendente e confirma o recebimento.
4. **Fase 2 (commit):** com todos os registros confirmados, o coordenador chama a **rota de commit** em todos os nós responsáveis, efetivando a mensagem.
5. **Falha no registro ou no commit:** a fase **não é efetivada** e o coordenador **repete a chamada** ao nó que falhou, conforme a configuração de retry. O retry vale para **as duas fases**; o envio (registro) é onde ocorre a maior parte das falhas.
6. **Esgotados os retries:** a API retorna um **erro ao cliente** informando que não foi possível receber a mensagem, e o coordenador envia a todos os nós um **rollback** para descartar o registro pendente. Mesmo sem o rollback, um registro sem commit seria descartado após um tempo; o rollback apenas **acelera a liberação de recursos**.
7. **Sucesso:** a durabilidade "efetiva" da escrita é definida pelo **número de réplicas com commit confirmado** (padrão: 3), e só então a API responde ao cliente com o ID da mensagem.
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

