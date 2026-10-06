# Particionamento e replicação

[← Índice](README.md)

## 7. Particionamento e replicação de mensagens
 
### 7.1 Hash circular (consistent hashing)
- O cluster particiona as mensagens em **blocos de mensagem**, distribuídos em um **anel de hash circular** (consistent hashing).
- Cada nó físico pode representar múltiplos **nós virtuais** no anel; o **número de nós virtuais é configurável**, melhorando a distribuição dos blocos entre os nós físicos.
- Os nós responsáveis por um bloco (primário, secundário, terciário etc.) são sempre **nós físicos distintos**: ao percorrer o anel em busca das réplicas, posições de nós virtuais de um nó físico já escolhido são puladas.
- O **primário** do bloco é o **dono da entrega** das mensagens dele ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.7). Qualquer nó calcula o primário de uma mensagem a partir do ID e da tabela de membros.
- Quando um nó sobe, ele assume blocos de mensagem como **primário**, **secundário**, **terciário** etc., conforme o **número de réplicas persistentes** exigido para uma mensagem. **Padrão: 3 réplicas.**
### 7.2 Procedimento de recebimento de uma mensagem (registro + commit, quórum mínimo)

![Sequência da recepção de uma mensagem](diagramas/img/recepcao.svg)

*Fonte: [recepcao.json](diagramas/fonte/recepcao.json).*

1. **Ler a configuração:** número total de réplicas (`replication.factor`, padrão 3) e o **quórum mínimo de confirmação** (`write.quorum`, padrão maioria simples de `replication.factor` — ex.: 2 em 3). O quórum **sempre conta o primário como confirmação obrigatória**, mais `write.quorum - 1` de quaisquer outras réplicas — nunca um quórum formado só por secundário/terciário. Motivo: só o primário decide o que é entregável (seção 12.7); se ele ficar de fora do quórum, a escrita pode ser considerada "durável" antes de estar visível a qualquer consumidor.
2. A partir do **GUID v7 (ID)** da mensagem, gerado pelo nó coordenador ao receber a requisição (antes da fase de registro) e ordenável por tempo, calcular no anel de hash circular **qual bloco de mensagens** deve persistir a mensagem, determinando os nós responsáveis: primário, secundário, terciário etc.
3. **Entregar a todos os nós responsáveis** (as `replication.factor` réplicas, não só o quórum) um pedido de **registro** da mensagem.
4. Assim que o **quórum de confirmações do registro** é atingido (primário incluso), o coordenador **decide commitar**: chama a rota de commit nos nós do quórum e, em paralelo — sem bloquear a resposta ao cliente — continua o registro + commit nas réplicas restantes.
5. Com o **commit confirmado no quórum** (primário incluso), **retornar o ID da mensagem** ao cliente. As réplicas fora do quórum ainda podem estar pendentes nesse instante.
6. **Falha antes do quórum ser fechado:** repetir a chamada ao nó que falhou conforme a configuração de retry (`write.retry.attempts`, `write.retry.backoff`). Esgotados os retries **sem que o quórum (com o primário) tenha sido atingido**, retornar **erro ao cliente** e enviar **rollback** a todos os nós que já haviam confirmado o registro, para descartar o registro pendente. Um registro sem commit também expira sozinho após `write.pending.ttl`; o rollback só acelera a liberação de recursos. **Antes do quórum decidir, a operação pode sempre ser abortada** — nenhum nó efetivou nada de forma que outro dependa dele.
7. **Falha depois do quórum decidido** (nas réplicas restantes, seja na fase de registro ou de commit delas): a decisão de commitar **não volta atrás**, mesmo que réplicas fora do quórum ainda estejam falhando ou pendentes — **nunca há rollback** nessa altura, pois o quórum (com o primário) já pode ter efetivado a mensagem. O coordenador **insiste** (registro + commit) nas réplicas que faltam, dentro de um **teto de tempo configurável** (`write.commit.timeout`):
   - **Confirmado dentro do teto:** a réplica passa a contar como durável normalmente, sem novo aviso ao cliente (a resposta já foi dada no passo 5).
   - **Teto esgotado sem confirmação em alguma réplica:** o bloco fica temporariamente **abaixo do fator de replicação pleno** (mas nunca abaixo do quórum, que já está garantido) — isso é tratado como o mesmo tipo de **evento isolado** das demais falhas pontuais deste protocolo (princípio 6, [07-principios-design.md](07-principios-design.md)), não uma falha de saúde do nó: gera um evento de observabilidade (`under-replicated`, não crítico), e o coordenador continua tentando em segundo plano — o commit é idempotente, repeti-lo não tem custo de correção.
> Este mesmo protocolo de duas fases (registro → commit) é reutilizado para registro e desregistro de nó (seção 6.2/6.3), para a definição de tópicos (seção 3.3) e para a recuperação de mensagens de um nó que retorna ao cluster (seção 6.6) — **porém esses três usos exigem confirmação de todos os nós, sem quórum reduzido**: são operações raras e pesadas (afetam o roteamento do anel inteiro ou a tabela de tópicos inteira), sem o caminho crítico de latência que justifica o quórum nas mensagens. A distinção entre rollback (só antes da decisão) e retry com teto configurável (depois da decisão) vale igualmente nos dois casos — só muda **quando** a decisão é tomada: no quórum (mensagens) ou em todos os nós (cluster/tópicos).
 
### 7.3 Entrada de um nó e sincronização distribuída de blocos
 
Quando um nó novo assume responsabilidade sobre um ou mais blocos do anel (como primário, secundário etc.), a sincronização é **escopada apenas aos blocos afetados** — o restante do cluster continua operando normalmente, sem bloqueio global.
 
Como a mesma mensagem existe em várias réplicas (uma por nó responsável pelo bloco), a sincronização **puxa os dados de forma distribuída entre todos os nós que já possuem o bloco**, em vez de sobrecarregar um único nó de origem. O procedimento:
 
1. O nó novo solicita a **todos os nós responsáveis pelo bloco** a lista de **IDs de mensagens** que cada um possui para aquele bloco. Uma mensagem já deletada (ack concluído) antes desta listagem **não é um caso especial**: ela simplesmente não aparece em nenhuma lista, porque cada nó a apaga ao confirmar o ack (seção 12.4 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)) — não há tombstone permanente a manter.
2. Consolida as respostas em uma lista única de IDs a sincronizar, pela **união** das listas recebidas (não a interseção — uma réplica pode ter recebido uma mensagem que outra ainda não recebeu).
3. Com a lista de IDs em mãos, **distribui (balanceia) as chamadas `GET`** de conteúdo entre os nós de origem, até obter todas as mensagens do bloco — evitando concentrar a carga de leitura em um único nó.
4. Enquanto isso ocorre:
   - O nó **já passa a receber novas mensagens** desse bloco normalmente (registro/commit conforme seção 7.2), e é também um dos nós responsáveis para **novos acks** desse bloco a partir do momento em que assume a posição no anel (seção 6.2, passo 8) — mesmo ainda sincronizando o conteúdo histórico.
   - **Deleções de mensagens** (consumo/ack) desse bloco, recebidas enquanto a sincronização está em andamento, são **registradas** em um log de deleções pendentes, não aplicadas de imediato — isso cobre o caso de um ack terminar de confirmar em outra réplica exatamente enquanto este nó ainda está buscando o conteúdo daquela mensagem.
   - O nó **não entrega mensagens** desse bloco enquanto a sincronização estiver em andamento — evitando entregar uma mensagem que já deveria constar como deletada, mas cujo estado ainda não chegou por completo.
5. **Verificação final:** antes de aplicar o log de deleções pendentes e marcar o bloco como íntegro, o nó **repete a listagem de IDs** (passo 1) só para as mensagens que pretende manter — as que buscou e ainda não estão no log de deleções. Se alguma já não aparece mais em nenhum nó de origem, é descartada. Isso cobre a janela residual: um ack cujo pedido nunca chegou a este nó, porque os nós responsáveis foram calculados um instante antes de ele assumir a posição no anel.
6. Ao final (todas as mensagens do bloco obtidas + log de deleções pendentes aplicado + verificação final), o bloco fica **íntegro** e o nó passa a operar normalmente para ele, incluindo a entrega de mensagens.

> Mesmo com os passos 4 e 5, uma janela residual — um ack que conclui exatamente entre a verificação final e o nó marcar o bloco como íntegro — não é impedida por construção. É tratada como o mesmo tipo de exceção rara da "falha ao recuperar uma mensagem específica" abaixo: um evento observável, não uma falha de saúde do nó, e não justifica manter uma estrutura de tombstones só para fechar esse instante.
**Falha ao recuperar uma mensagem específica:** se, durante a sincronização, uma mensagem constante na lista de IDs **não for encontrada em nenhum nó/log** (nenhuma réplica consegue fornecê-la), isso **não compromete a saúde do nó** nem interrompe a sincronização do bloco — apenas gera um **evento de falha de perda de mensagem** (mensagem inconsistente/perdida) para investigação, e a sincronização segue para as demais mensagens. Esse evento é o que permite, mais tarde, que um nó que retorne ao cluster (seção 6.6) reconheça e recupere mensagens que estavam registradas como perdidas.
 
> Para blocos muito grandes, relistar todos os IDs a cada sincronização pode ser custoso; uma otimização futura (ex.: merkle tree / hashing incremental por faixa) pode evitar esse custo — ver seção 10.

