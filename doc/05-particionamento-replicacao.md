# Particionamento e replicação

[← Índice](README.md)

## 7. Particionamento e replicação de mensagens
 
### 7.1 Hash circular (consistent hashing)
- O cluster particiona as mensagens em **blocos de mensagem**, distribuídos em um **anel de hash circular** (consistent hashing).
- Cada nó físico pode representar múltiplos **nós virtuais** no anel; o **número de nós virtuais é configurável**, melhorando a distribuição dos blocos entre os nós físicos.
- Quando um nó sobe, ele assume blocos de mensagem como **primário**, **secundário**, **terciário** etc., conforme o **número de réplicas persistentes** exigido para uma mensagem. **Padrão: 3 réplicas.**
### 7.2 Procedimento de recebimento de uma mensagem (registro + commit)
1. **Ler a configuração** do número de nós exigido para confirmar o recebimento (fator de replicação — padrão 3).
2. A partir do **GUID v7 (ID)** da mensagem, gerado pelo nó coordenador ao receber a requisição (antes da fase de registro) e ordenável por tempo, calcular no anel de hash circular **qual bloco de mensagens** deve persistir a mensagem, determinando os nós responsáveis: primário, secundário, terciário etc.
3. **Entregar a todos os nós responsáveis** um pedido de **registro** da mensagem.
4. Após os registros, enviar **mais uma chamada** com o **commit da entrega** para todos os nós envolvidos (protocolo em duas fases: registro → commit).
5. Com as **entregas confirmadas** (commit em todos os nós exigidos), **retornar o ID da mensagem** ao cliente.
6. Se o registro ou o commit falhar em algum nó, **repetir a chamada** a esse nó conforme a configuração de retry (vale para as duas fases; o registro é onde ocorre a maior parte das falhas). Esgotados os retries, retornar **erro ao cliente** e enviar **rollback** a todos os nós envolvidos para descartar o registro pendente. Registros sem commit também expiram sozinhos após um tempo; o rollback só acelera a liberação de recursos.
> Este mesmo protocolo de duas fases (registro → commit) é reutilizado para registro e desregistro de nó (seção 6.2/6.3) e para a recuperação de mensagens de um nó que retorna ao cluster (seção 6.6).
 
### 7.3 Entrada de um nó e sincronização distribuída de blocos
 
Quando um nó novo assume responsabilidade sobre um ou mais blocos do anel (como primário, secundário etc.), a sincronização é **escopada apenas aos blocos afetados** — o restante do cluster continua operando normalmente, sem bloqueio global.
 
Como a mesma mensagem existe em várias réplicas (uma por nó responsável pelo bloco), a sincronização **puxa os dados de forma distribuída entre todos os nós que já possuem o bloco**, em vez de sobrecarregar um único nó de origem. O procedimento:
 
1. O nó novo solicita a **todos os nós responsáveis pelo bloco** a lista de **IDs de mensagens** (e de IDs já deletados/tombstones) que cada um possui para aquele bloco.
2. Consolida as respostas em uma lista única de IDs a sincronizar, pela **união** das listas recebidas (não a interseção — uma réplica pode ter recebido uma mensagem que outra ainda não recebeu).
3. Com a lista de IDs em mãos, **distribui (balanceia) as chamadas `GET`** de conteúdo entre os nós de origem, até obter todas as mensagens do bloco — evitando concentrar a carga de leitura em um único nó.
4. Enquanto isso ocorre:
   - O nó **já passa a receber novas mensagens** desse bloco normalmente (registro/commit conforme seção 7.2).
   - **Deleções de mensagens** (consumo/ack) desse bloco são **registradas** em um log de deleções pendentes, não aplicadas de imediato.
   - O nó **não entrega mensagens** desse bloco enquanto a sincronização estiver em andamento — evitando entregar uma mensagem que já deveria constar como deletada, mas cujo estado ainda não chegou por completo.
5. Ao final (todas as mensagens do bloco obtidas + log de deleções pendentes aplicado), o bloco fica **íntegro** e o nó passa a operar normalmente para ele, incluindo a entrega de mensagens.
**Falha ao recuperar uma mensagem específica:** se, durante a sincronização, uma mensagem constante na lista de IDs **não for encontrada em nenhum nó/log** (nenhuma réplica consegue fornecê-la), isso **não compromete a saúde do nó** nem interrompe a sincronização do bloco — apenas gera um **evento de falha de perda de mensagem** (mensagem inconsistente/perdida) para investigação, e a sincronização segue para as demais mensagens. Esse evento é o que permite, mais tarde, que um nó que retorne ao cluster (seção 6.6) reconheça e recupere mensagens que estavam registradas como perdidas.
 
> Para blocos muito grandes, relistar todos os IDs a cada sincronização pode ser custoso; uma otimização futura (ex.: merkle tree / hashing incremental por faixa) pode evitar esse custo — ver seção 10.

