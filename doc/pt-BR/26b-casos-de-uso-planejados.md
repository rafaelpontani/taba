---
título: Casos de uso planejados
resumo: Os casos de uso documentados que ainda não têm teste que os cite — o que falta provar — por área, com o motivo quando o próprio caso o diz.
tipo: referência
status: parcial
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/tests/uc_coverage.rs]
---

# Casos de uso planejados (sem teste)

> **Mantido à mão.** Um caso aqui **não quer dizer que o comportamento não existe**: quer dizer que **nenhum teste o cita** hoje. Alguns recursos já funcionam (e aparecem em outros documentos como ✅) sem prova por caso; outros são ⬜ planejados. Os casos com teste estão em [26a-casos-de-uso-implementados.md](26a-casos-de-uso-implementados.md).

**Placar (2026-10-24):** **92** casos sem teste, de **862**. Os casos marcados "(pendente: D##)" dependem de uma dúvida ainda aberta ([31-pendencias.md](31-pendencias.md)).

Nível: **U** unitário, **I** um nó real, **C** cluster simulado, **K** conector. Para fechar um caso, cite-o num teste (`/// UC: UC-XXX-NNN`) do nível indicado e regenere ([26-casos-de-uso.md](26-casos-de-uso.md)).

## Onde estão as lacunas

| Área | Assunto | Sem teste | Com teste |
|---|---|---:|---:|
| `TOP` | Tópicos: criar, listar, ler, alterar e remover | 2 | 46 |
| `MSG` | Publicação e o corpo da mensagem | 1 | 19 |
| `CNS` | Consumo (`GET`), long polling e `autoCreate` | 1 | 39 |
| `VIS` | Alteração da invisibilidade | 2 | 10 |
| `RET` | Expiração (`ttl`), limite de entregas e dead-letter | 2 | 18 |
| `DST` | Tópico Distribuidor | 1 | 17 |
| `GRP` | Grupos de mensagens e entrega justa | 0 | 17 |
| `AUT` | Autenticação e tokens de acesso | 5 | 34 |
| `LIM` | Rate limit e IP do cliente | 2 | 13 |
| `PAG` | Paginação | 1 | 8 |
| `CNV` | Convenções comuns da API (formato, cabeçalhos, erros, status) | 3 | 14 |
| `ESC` | Escrita: registro + commit, quórum, rollback, teto de commit | 9 | 29 |
| `ANE` | Anel de hash, blocos e papéis | 3 | 8 |
| `DEF` | Definição de tópicos e de tokens replicada no cluster | 7 | 24 |
| `ENT` | Entrega no dono: lista de prontos, `take`, long polling | 12 | 18 |
| `INV` | Invisibilidade no dono e nas réplicas | 5 | 26 |
| `ARM` | Armazenamento: memória, overflow, SQLite, integridade | 5 | 24 |
| `ING` | Entrada de nó (bootstrap) | 3 | 31 |
| `FAL` | Detecção de falhas, votação de exclusão e saída por falha | 1 | 32 |
| `FRZ` | Período de freezing | 0 | 13 |
| `CNC` | Concorrência entre mudanças de membros | 2 | 12 |
| `SIN` | Sincronização distribuída de blocos | 0 | 29 |
| `STR` | Streams NDJSON entre nós | 7 | 8 |
| `FWD` | Repasse de chamadas de cliente | 7 | 4 |
| `DRN` | Parada e modo drain | 2 | 19 |
| `DEV` | Perfil de um nó (o antigo modo desenvolvimento) | 0 | 3 |
| `OPS` | Observabilidade (OpenTelemetry), estatísticas, métricas e saúde | 6 | 41 |
| `CLR` | Contrato das rotas do cluster (DTOs e regras comuns) | 2 | 8 |
| `CFG` | Configuração e validações de partida | 1 | 10 |

## TOP: Tópicos: criar, listar, ler, alterar e remover


### UC-TOP-025

**Caso:** A listagem e a leitura são respondidas localmente, sem chamar outros nós (a definição está em todo nó).  
**Nível:** C  
**Fonte histórica:** 06a, 02 §3.3

---

### UC-TOP-033

**Caso:** `PATCH` de `idleTimeout` numa fila volátil vale a partir da próxima verificação do gestor.  
**Nível:** I, C  
**Fonte histórica:** 06a, 15 §14.3

---


## MSG: Publicação e o corpo da mensagem

### UC-MSG-017

**Caso:** `502` ou `504` num repasse depois de enviado o corpo, fora da FIFO  
**Então:** a chamada não é repetida e o erro sobe ao cliente.  
**Nível:** C  
**Fonte histórica:** 06a, 04 §6.9

---


## CNS: Consumo (`GET`), long polling e `autoCreate`

### UC-CNS-039

**Caso:** O primeiro `GET` com `autoCreate` paga a criação da definição (proposta ao dono e aplicada em todos os nós); os seguintes não.  
**Nível:** C  
**Fonte histórica:** 15 §14.4

---


## VIS: Alteração da invisibilidade

### UC-VIS-008

**Caso:** `PUT` sem quórum  
**Então:** `503` `cluster.quorum-unavailable`, e o prazo anterior continua valendo.  
**Nível:** C  
**Fonte histórica:** 06a, 14 §13.3

---

### UC-VIS-012

**Caso:** O `PUT` é decidido pelo mesmo registro + commit por quórum da entrega, sem tocar na lista de prontos (exceto com `0`).  
**Nível:** C  
**Fonte histórica:** 13 §12.3, 14 §13.3

---


## RET: Expiração (`ttl`), limite de entregas e dead-letter

### UC-RET-019

**Caso:** Na fila FIFO, a mensagem que atinge `maxDeliveries` vai para a dead-letter e o grupo segue com a próxima; a vencida por `ttl` na cabeça também libera o grupo (casos `FIF`).  
**Nível:** I  
**Fonte histórica:** 13 §12.10, 17 §16.6

---

### UC-RET-020

**Caso:** A sincronização de bloco descarta o que já venceu por `ttl` em vez de copiar.  
**Nível:** C  
**Fonte histórica:** 13 §12.10, 05 §7.3

---


## DST: Tópico Distribuidor

### UC-DST-008

**Caso:** Falha parcial  
**Então:** uma cópia falha (por exemplo, sem quórum) e as outras seguem; a resposta é `200`, com `error` (`code`, `message`) no item que falhou e `id` nos demais, e o evento `taba.distributor.partial`. Nada é desfeito.  
**Nível:** C  
**Fonte histórica:** 06a, 15 §14.2

---


## GRP: Grupos de mensagens e entrega justa


## AUT: Autenticação e tokens de acesso

### UC-AUT-019

**Caso:** A validação é uma consulta local em memória, sem chamada de rede  
**Então:** com os outros nós fora do ar, um token válido continua aceito.  
**Nível:** C  
**Fonte histórica:** 06a §8.1.3, 02 §3.4

---

### UC-AUT-030

**Caso:** Criar, alterar e revogar um token são propostos ao **dono** do token no anel, que dá a próxima versão, grava no herdeiro e aplica em todos os nós (`definitions/propose` e `apply`); não esperam todos os nós  
**Então:** o dono responde depois dos nós que respondem em `write.commit.timeout`, e os demais recebem pelo canal; sem o dono (fora do ar e ainda na tabela) ou sem nenhum outro dono para confirmar, `503` `cluster.quorum-unavailable` e nada é aplicado.  
**Nível:** C  
**Fonte histórica:** 06a, 02 §3.4

---

### UC-AUT-031

**Caso:** O segredo nunca trafega entre nós  
**Então:** o `TokenRecord` do `propose` e do `apply` leva só o `salt` e o `secretHash`.  
**Nível:** C  
**Fonte histórica:** 06c, 02 §3.4

---

### UC-AUT-033

**Caso:** Um nó que entra puxa os tokens (`GET /cluster/tokens`) junto com a tabela de tópicos; até a carga terminar o nó não atende rotas de cliente, então nunca valida com a tabela incompleta.  
**Nível:** C  
**Fonte histórica:** 02 §3.4

---

### UC-AUT-034

**Caso:** Uma chamada repassada (nó em sincronização ou drain) leva o `Authorization` como veio, e o **destino** confere o token de novo.  
**Nível:** C  
**Fonte histórica:** 06a, 04 §6.9

---


## LIM: Rate limit e IP do cliente

### UC-LIM-008

**Caso:** Os contadores são locais a cada nó, sem troca entre nós.  
**Nível:** C  
**Fonte histórica:** 12

---

### UC-LIM-014

**Caso:** Nó em sincronização ou drain  
**Então:** conta a chamada por IP e a repassa; o destino não conta de novo. A porta do cluster não tem rate limit.  
**Nível:** C  
**Fonte histórica:** 12, 04 §6.9

---


## PAG: Paginação

### UC-PAG-004

**Caso:** Itens inseridos ou removidos antes da chave do cursor durante o percurso não deslocam a página seguinte (paginação por chave, não por deslocamento).  
**Nível:** I  
**Fonte histórica:** 06a §8.1.2

---


## CNV: Convenções comuns da API (formato, cabeçalhos, erros, status)

### UC-CNV-005

**Caso:** `traceparent` na requisição  
**Então:** as operações do nó entram no mesmo trace.  
**Nível:** I  
**Fonte histórica:** 06d §8.3.2, 11

---

### UC-CNV-013

**Caso:** As rotas `/health/*` devolvem `HealthResponse` (nunca `ErrorResponse`), e uma falha **depois do `200`** de um stream NDJSON usa a linha `{"error":"..."}`.  
**Nível:** I, C  
**Fonte histórica:** 06d §8.3.3

---

### UC-CNV-015

**Caso:** Uma operação que cria ou apaga (tópico, token) e fica sem resposta definitiva (`504` ou conexão cortada) pode já ter sido aplicada pelo dono  
**Então:** ao repetir, `409` `topic.already-exists` (criação) ou `404` (remoção) significam que a operação já tinha sido aplicada. Um `503` `cluster.quorum-unavailable` não aplicou nada.  
**Nível:** C  
**Fonte histórica:** 06d §8.3.5

---


## ESC: Escrita: registro + commit, quórum, rollback, teto de commit

### UC-ESC-018

**Caso:** O rollback só se aplica **antes** da decisão  
**Então:** nunca é enviado depois de o quórum decidir.  
**Nível:** C  
**Fonte histórica:** 05 §7.2

---

### UC-ESC-027

**Caso:** `commit` ou `rollback` de uma operação que o nó não conhece (já expirou ou nunca chegou)  
**Então:** `404` `cluster.operation-unknown`.  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-ESC-028

**Caso:** `register` de um `id` que já tem registro pendente, ou de mensagem que já existe com conteúdo diferente  
**Então:** `409` `cluster.conflict`.  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-ESC-031

**Caso:** Um nó **em sincronização** confirma o `register` gravando-o no banco de eventos (e o aplica depois); para o coordenador a confirmação conta normalmente para o quórum.  
**Nível:** C  
**Fonte histórica:** 06c, 05 §7.3

---

### UC-ESC-032

**Caso:** Um nó que não é membro recebe `403` `cluster.not-member` em qualquer rota de escrita.  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-ESC-033

**Caso:** Coordenador  
**Então:** `taba.write.coordinated` (`committed` e `failed`), `taba.write.duration` por fase, `taba.write.retries`, `taba.write.rollbacks` (só por falha de quórum), `taba.write.replicas_confirmed` (por `stage`: `response` e `eventual`) e `taba.write.quorum_latency`.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-ESC-034

**Caso:** Réplica  
**Então:** `taba.write.received` por fase, `taba.write.pending`, `taba.write.pending.oldest_age` e `taba.write.pending.expired`.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-ESC-035

**Caso:** O trace da publicação tem o span raiz, o registro e o commit por nó de destino e, se for o caso, o rollback; o span raiz termina quando o quórum confirma; o `traceparent` é propagado entre os nós. Falhas e eventos de durabilidade são sempre mantidos na amostragem.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-ESC-036

**Caso:** Os logs  
**Então:** falha de registro ou commit por tentativa (`warn`), escrita abortada com rollback (`error`), registro pendente expirado (`warn`), teto de commit esgotado numa réplica (`warn`).  
**Nível:** C  
**Fonte histórica:** 11

---


## ANE: Anel de hash, blocos e papéis

### UC-ANE-006

**Caso:** O tópico fica espalhado  
**Então:** as mensagens de um mesmo tópico caem em todo o anel; nenhum nó guarda o tópico inteiro nem é dono dele.  
**Nível:** C  
**Fonte histórica:** 05 §7.1

---

### UC-ANE-007

**Caso:** A perda de um nó deixa cada mensagem ainda em pelo menos `replication.factor − 1` nós.  
**Nível:** C  
**Fonte histórica:** 05 §7.1

---


### UC-ANE-012

**Caso:** As métricas do anel  
**Então:** `taba.ring.virtual_nodes`, e `taba.ring.blocks` por papel (`primary`, `secondary`, `tertiary`) e estado (`healthy`, `syncing`).  
**Nível:** C  
**Fonte histórica:** 11

---


## DEF: Definição de tópicos e de tokens replicada no cluster

### UC-DEF-013

**Caso:** Na partida em que o nó parte **sozinho** (o primeiro nó, ou o de menor ID antigo na retomada), ele carrega as definições do `_node.db` (tabelas `topics` e `tokens`), com as versões e as lápides de `definition_versions`, preservando o `id` e a data de criação de cada uma. Um nó que entra num cluster que já está de pé não as usa: recebe as do cluster e só as **oferece** depois.  
**Nível:** I, C  
**Fonte histórica:** 04 §6.10

---

### UC-DEF-014

**Caso:** O `DELETE` sempre funciona, mesmo com mensagens não entregues ou não confirmadas. Ao aplicar a remoção (`apply` com `deleted`), cada nó apaga todas as mensagens do tópico que tem localmente (memória, `${topic}/` em disco e metadados), sem checar se estão pendentes, e fica a lápide da definição.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-017

**Caso:** A carga é aplicada direto no armazenamento final; tudo o que chega durante a carga (as mudanças de definição por `apply` e as fases de registro + commit das mensagens) vai para o banco de eventos, na ordem de chegada, e o nó confirma o registro **depois de gravar**.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-018

**Caso:** Depois da carga, o banco de eventos é processado por cima, linha a linha, apagando as processadas  
**Então:** o evento vence a carga (uma remoção durante a carga apaga o que a carga trouxe). Uma mensagem de um tópico ainda desconhecido espera no mesmo banco.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-019

**Caso:** Verificação final  
**Então:** ao terminar, o nó repete a listagem e descarta o que não aparece mais na origem (cobre a remoção cujo commit não chegou a ele).  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-020

**Caso:** Se o nó de origem falhar no meio, a carga continua em outro nó, a partir do último `id` lido.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-021

**Caso:** Até a sincronização inteira terminar, o nó não atende rotas de cliente (`/health/ready` falha)  
**Então:** `GET /topics`, o casamento de prefixos e o cálculo do nó gestor dependem da tabela completa.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---


## ENT: Entrega no dono: lista de prontos, `take`, long polling

### UC-ENT-003

**Caso:** Nenhuma mensagem é entregue em duplicidade, mesmo com tabelas de membros divergentes por instantes  
**Então:** o registro de invisibilidade é exclusivo (rejeitado se já há outro pendente ou se a mensagem já está invisível) e o `GET` tenta outro ID.  
**Nível:** C  
**Fonte histórica:** 13 §12.7, 14 §13.3

---

### UC-ENT-005

**Caso:** Um nó que entrou e ainda não está íntegro **não** entrega as mensagens de que é primário; o primário antigo as tira da lista de prontos no commit do registro; ninguém as entrega até a sincronização terminar. As chamadas de cliente que chegam a esse nó são repassadas, e ele não envia avisos nem sinais de prontas.  
**Nível:** C  
**Fonte histórica:** 13 §12.7

---

### UC-ENT-006

**Caso:** Dono fora do ar e ainda não desregistrado  
**Então:** as mensagens dele ficam sem entrega até o desregistro; os demais nós do bloco são cópias de segurança e **não** assumem a entrega.  
**Nível:** C  
**Fonte histórica:** 13 §12.7

---


### UC-ENT-016

**Caso:** Cenário 1  
**Então:** lista local com mensagem: o nó entrega direto, sem consultar nenhum outro nó para achar a mensagem.  
**Nível:** C  
**Fonte histórica:** 13 §12.8

---

### UC-ENT-018

**Caso:** O `take` leva a **quantidade que ainda falta** para completar o `maxMessages` do cliente e o `visibilityTimeout` pedido.  
**Nível:** C  
**Fonte histórica:** 13 §12.2 e 12.8, 06c

---

### UC-ENT-020

**Caso:** O `take` responde lista vazia quando o estado "tem mensagens prontas" era uma dica desatualizada  
**Então:** o nó tenta o próximo, até `delivery.take.max-attempts`; esgotado, segue para o long polling. O custo é limitado, nunca um pedido a todos os nós.  
**Nível:** C  
**Fonte histórica:** 13 §12.8

---

### UC-ENT-021

**Caso:** Quem entrega no `take` é sempre o dono da mensagem  
**Então:** a mensagem tem um único coordenador, o relógio que fixa o `invisibleUntil` é o do dono.  
**Nível:** C  
**Fonte histórica:** 13 §12.8, 14 §13.3

---

### UC-ENT-026

**Caso:** N ouvintes esperando o mesmo tópico em um nó geram **uma** informação de interesse; ao receber um aviso, o nó busca de uma vez tantas mensagens quantos são os ouvintes.  
**Nível:** C  
**Fonte histórica:** 13 §12.8

---

### UC-ENT-027

**Caso:** Um aviso perdido só custa espera até o fim do `waitTime` (no máximo 20 s).  
**Nível:** C  
**Fonte histórica:** 13 §12.8

---

### UC-ENT-028

**Caso:** Esgotado o `waitTime` a chamada responde vazio e o ouvinte é removido. Se o cliente desconecta antes, o ouvinte também é removido.  
**Nível:** C  
**Fonte histórica:** 13 §12.8

---

### UC-ENT-029

**Caso:** A mensagem cuja invisibilidade vence, com a lista de prontos vazia, envia o sinal "tem mensagens prontas"; havendo ouvinte local, a mensagem vai direto a ele.  
**Nível:** C  
**Fonte histórica:** 14 §13.6

---

### UC-ENT-030

**Caso:** Trade-off aceito  
**Então:** o primário cai entre o commit local da invisibilidade e a replicação às réplicas fora do quórum; o novo primário herda o `invisibleUntil` de quem sobreviveu e a mensagem pode ser entregue antes do prazo prometido, sem quebrar o at-least-once.  
**Nível:** C  
**Fonte histórica:** 13 §12.2

---


## INV: Invisibilidade no dono e nas réplicas

### UC-INV-002

**Caso:** `invisibleUntil` é calculado pelo **dono** (`agora + visibilityTimeout`, pelo relógio dele); as réplicas guardam o valor pronto e nunca recalculam a partir de duração.  
**Nível:** C  
**Fonte histórica:** 14 §13.2 e 13.3

---

### UC-INV-009

**Caso:** O **dono** é sempre o coordenador  
**Então:** no `GET` local ele mesmo; num `take`, o nó que atende o cliente pede ao dono, que coordena.  
**Nível:** C  
**Fonte histórica:** 14 §13.3

---

### UC-INV-011

**Caso:** O commit é decidido por quórum (primário incluso); só depois a chamada responde ao cliente. As réplicas fora do quórum recebem o commit em segundo plano, dentro de `write.commit.timeout`.  
**Nível:** C  
**Fonte histórica:** 14 §13.3

---

### UC-INV-015

**Caso:** A diferença de relógio entre nós, na troca de dono, encurta ou alonga o prazo pelo tamanho da diferença  
**Então:** efeito aceito no at-least-once.  
**Nível:** C  
**Fonte histórica:** 14 §13.3

---

### UC-INV-031

**Caso:** Reinício  
**Então:** a tabela de expiração é descartada junto com o estado antigo; as mensagens da vida anterior voltam pela restauração (`_lives`), prontas, com a invisibilidade zerada e um `receiptToken` novo, e as listas são reconstruídas quando o nó assumir blocos.  
**Nível:** C  
**Fonte histórica:** 14 §13.7

---


## ARM: Armazenamento: memória, overflow, SQLite, integridade

### UC-ARM-016

**Caso:** O banco responde  
**Então:** a mensagem mais antiga do tópico (`MIN(id)`, somada à mais antiga em memória, mantida ordenada por ID), os próximos a vencer (índice por `invisibleUntil`), as mensagens de um bloco (faixa pela posição no anel) e as contagens do tópico.  
**Nível:** I  
**Fonte histórica:** 02 §3.2

---

### UC-ARM-018

**Caso:** O banco do nó, `${storage.dir}/_node.db`, é estritamente local, com as tabelas `topics` (as definições de tópico), `tokens`, `state`, `losses` (`topic`, `id`, `block`, `detected_at`) e `definition_versions` (as versões e as lápides), escrita por uma conexão única, o único escritor.  
**Nível:** I  
**Fonte histórica:** 02 §3.2

---

### UC-ARM-019

**Caso:** Banco corrompido  
**Então:** o nó entra em **falha**, sinaliza ao cluster (que fica liberado para votar o desregistro dele) e não tenta reparar. `/health/ready` passa a `503` com `reason: failed`.  
**Nível:** C  
**Fonte histórica:** 02 §3.2, 05 §7.3

---

### UC-ARM-020

**Caso:** Disco cheio ou falha de I/O  
**Então:** o nó entra em falha, como no banco corrompido; o erro conta em `taba.storage.disk.errors` e gera log `error`.  
**Nível:** C  
**Fonte histórica:** 05 §7.3, 11

---

### UC-ARM-025

**Caso:** Dados de **consumo** (profundidade, visíveis, invisíveis, taxas, latências) ficam só em memória e específicos de cada nó.  
**Nível:** I  
**Fonte histórica:** 02 §3.3

---


## ING: Entrada de nó (bootstrap)

### UC-ING-012

**Caso:** Ao final, o nó novo conhece todos os demais membros e vice-versa, e abre ativamente o canal permanente com cada nó da tabela.  
**Nível:** C  
**Fonte histórica:** 04 §6.2

---

### UC-ING-019

**Caso:** A falha de registro do nó (o nó como um todo não entra) tem efeito de exclusão + rebalanceamento; já a perda de uma mensagem na sincronização gera só alerta.  
**Nível:** C  
**Fonte histórica:** 04 §6.5, 07

---


### UC-ING-026

**Caso:** A tabela de membros **não** é persistida como estado vivo  
**Então:** o reinício do processo perde a tabela e o nó volta como nó novo, com ID novo; o `manifest.json` da vida anterior guarda uma cópia dela só para a restauração e a sondagem dos IPs antigos, e não faz o nó voltar como o antigo.  
**Nível:** C  
**Fonte histórica:** 13 §12.9

---


## FAL: Detecção de falhas, votação de exclusão e saída por falha

### UC-FAL-030

**Caso:** **Sem teste ainda.** A exclusão que não consegue se efetivar dentro da vez do nó mais o prazo de ingresso (`turn_at` + `JOIN_TIMEOUT`, 60 s) termina a votação como `failed`  
**Então:** o nó segue na tabela e a próxima falha do canal abre outra votação (D98). Para testar: manter o desregistro em conflito permanente e reduzir o prazo por configuração de teste. Confere o resultado `failed` na métrica de votação, o nó ainda na tabela e a nova votação na falha seguinte.  
**Nível:** C  
**Fonte histórica:** 04 §6.3

---


## CNC: Concorrência entre mudanças de membros


### UC-CNC-011

**Caso:** **Sem teste ainda.** Um ingresso que só encontra conflito (outra mudança de membros sempre mais antiga) repete depois de 100 a 200 ms sorteados, mas desiste ao fim de 60 s (`JOIN_TIMEOUT`)  
**Então:** o coordenador devolve `cluster.registration-failed` e o nó novo repete o bootstrap pelo `cluster.bootstrap.retry.interval`. Para testar: manter uma mudança pendente que nunca termina (por exemplo, um `register` que o nó mais antigo nunca confirma) e reduzir o prazo de 60 s por configuração de teste, para a suíte não esperar o tempo real. Confere o erro ao fim do prazo, o rollback do que o coordenador registrou e a nova tentativa do nó.  
**Nível:** C  
**Fonte histórica:** 04 §6.2 e 6.8

---

### UC-CNC-012

**Caso:** **Sem teste ainda.** Uma exclusão e um ingresso que chegam a coordenadores diferentes no mesmo instante terminam com os dois efetivados, um depois do outro  
**Então:** vence a mudança de menor `operationId`, e a outra cede, desfaz e recomeça (D94). Hoje só a regra está testada, no `Membership` (UC-CNC-010), e a espera do ingresso pela votação (UC-CNC-002); falta provocar o cruzamento verdadeiro, com o `register` e o `unregister` atrasados na rede de teste para as duas fases 1 se sobreporem. Confere que a tabela final tem o nó novo e não tem o excluído, que nenhum dos dois ficou pendente e que a mudança deslocada recomeçou.  
**Nível:** C  
**Fonte histórica:** 04 §6.8

---


## STR: Streams NDJSON entre nós

### UC-STR-005

**Caso:** Teto por stream  
**Então:** o emissor fecha ao atingir `cluster.sync.stream.max-items` ou `max-bytes`, com `hasMore: true`; o receptor abre outro com `from=<last>`.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-007

**Caso:** Erro depois do `200`  
**Então:** o emissor escreve `{"error":"..."}` (que não é `ErrorResponse`) e fecha, ou apenas fecha sem a linha final; nos dois casos o receptor retoma.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-008

**Caso:** Timeout de leitura **por linha** (`cluster.sync.stream.idle-timeout`), não um teto para o stream inteiro; sem *heartbeat*.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-010

**Caso:** Contrapressão  
**Então:** o receptor lê no próprio ritmo e o emissor não mantém transação nem lista congelada aberta; lê o armazenamento em blocos pequenos.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-013

**Caso:** `POST /cluster/messages/recover`  
**Então:** corpo NDJSON de `MessageRecord` terminado por `StreamEnd`; a resposta é `RecoverResult` (`end: true`, `count` aplicadas); se a conexão cair, quem envia reenvia tudo e as já aplicadas são ignoradas.  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-STR-014

**Caso:** Os streams de mensagens (`ids` e `messages`) não carregam lápide  
**Então:** a remoção de uma mensagem continua por ausência do ID; o que impede uma cópia morta de voltar é o log de excluídos (`/cluster/blocks/{id}/excluded`) e, nas definições, o stream de lápides (`/cluster/definitions/tombstones`), cada um em rota própria.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-015

**Caso:** Métricas  
**Então:** `taba.sync.streams` por `kind`, `direction`, `outcome` (`complete`, `truncated`, `error`), `taba.sync.stream.resumes`, `duration` e `bytes`.  
**Nível:** C  
**Fonte histórica:** 11

---


## FWD: Repasse de chamadas de cliente

### UC-FWD-002

**Caso:** Destino na sincronização  
**Então:** um membro da tabela com canal ativo, escolhido ao acaso (só há um nó em sincronização por vez).  
**Nível:** C  
**Fonte histórica:** 04 §6.9

---

### UC-FWD-003

**Caso:** Destino no drain  
**Então:** o **nó mais livre** (menor carga pelo sinal de vida, mais as chamadas já repassadas e em andamento por aquele destino), com empate sorteado; só entram nós prontos e fora de drain; a escolha é feita **a cada chamada**. Depois que o canal fecha, vale o último indicador e a última tabela conhecida.  
**Nível:** U, C  
**Bloqueado:** O código diverge do cartão: `Forwarder::pick` usa só as chamadas em andamento do próprio nó e sorteia o empate; a carga do sinal de vida não entra. Decidir se o cartão ou o código muda antes de testar.  
**Fonte histórica:** 04 §6.9

---

### UC-FWD-005

**Caso:** O repasse vai à **porta do cluster** do destino, pela rota `/cluster/forward/{caminho}`; a API de cliente não aceita marcador interno de repasse.  
**Nível:** C  
**Fonte histórica:** 04 §6.9, 06c

---

### UC-FWD-006

**Caso:** Com autenticação ligada, o `Authorization` segue como veio e o **destino confere** o token (o nó em sincronização ainda não tem a tabela de tokens).  
**Nível:** C  
**Fonte histórica:** 04 §6.9, 06c

---

### UC-FWD-007

**Caso:** Sem laço  
**Então:** a rota `/cluster/forward` é o marcador; quem a recebe nunca repassa de novo, e se estiver sincronizando responde `503` `cluster.node-syncing`. Aceita só de membros e de nós que saíram por drain (lista em memória preenchida no commit do desregistro voluntário e esvaziada quando um nó novo se registra com o mesmo endereço).  
**Nível:** C  
**Fonte histórica:** 04 §6.9, 06c

---

### UC-FWD-008

**Caso:** Falha do destino **antes** de o corpo ser enviado (conexão recusada)  
**Então:** tenta outro nó, até um teto pequeno. **Depois** de enviado: **não repete** (um `POST` de mensagem repetido duplicaria) e responde `502` `forward.bad-gateway` ou `504` `forward.timeout`.  
**Nível:** C  
**Fonte histórica:** 04 §6.9

---

### UC-FWD-010

**Caso:** Sem nenhum destino respondendo  
**Então:** `503` com `Retry-After` (`cluster.node-syncing` ou `cluster.node-draining`).  
**Nível:** C  
**Fonte histórica:** 04 §6.9, 06c

---


## DRN: Parada e modo drain (inclui o drain como save)

### UC-DRN-015

**Caso:** `drain.timeout` precisa ser menor que o prazo de parada do orquestrador (`terminationGracePeriodSeconds`, 30 s por padrão no Kubernetes).  
**Nível:** I  
**Fonte histórica:** 04 §6.10, 10

---

### UC-DRN-028

**Caso:** Num nó só (perfil de um nó), as chamadas de cliente durante o drain respondem `503` com `Retry-After` (não há outro nó para repassar).  
**Nível:** I  
**Fonte histórica:** 04 §6.4 e 6.10

---


## OPS: Observabilidade (OpenTelemetry), estatísticas, métricas e saúde

### UC-OPS-015

**Caso:** As condições aparecem como gauges `0/1` com a unidade `{flag}` (`taba_cluster_condition{taba_condition=...}`).  
**Nível:** I  
**Fonte histórica:** 11

---

### UC-OPS-020

**Caso:** No `GET /stats/topics` (API de cliente) o filtro e o `limit` são aplicados **depois** do merge entre nós, e `sort` não existe (`400`). O cache do resultado por `stats.topics.cache-ttl` (com `collectedAt`) está planejado: hoje o parâmetro não tem efeito.  
**Nível:** C  
**Fonte histórica:** 06b, 11

---

### UC-OPS-025

**Caso:** No `/stats/topics`, um nó que não responde fica de fora e o resultado é marcado `partial`, com `missingNodes`  
**Então:** as somas viram piso.  
**Nível:** C  
**Fonte histórica:** 11

---


### UC-OPS-030

**Caso:** Degradação de replicação (`under-replicated`, `syncing`) é propriedade do **cluster**, não do tópico  
**Então:** aparece na condição do cluster, não na do tópico.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-OPS-037

**Caso:** As rotas `/cluster/*` não são rotas de operação  
**Então:** ficam na porta do cluster e não são afetadas por `operations.*`.  
**Nível:** I  
**Fonte histórica:** 11, 06 §8.0

---

### UC-OPS-042

**Caso:** Cada nó expõe só as próprias séries, sem rótulo `node`  
**Então:** o rótulo `instance` é o `service.instance.id`. As métricas `taba.cluster.*` são a visão que o nó tem do cluster, e as demais `taba.*` são o trabalho dele; o total do cluster vem de agregação no consumidor.  
**Nível:** C  
**Fonte histórica:** 11

---


## CLR: Contrato das rotas do cluster (DTOs e regras comuns)

### UC-CLR-003

**Caso:** A resposta de `register`, `commit` e `rollback` é `Ack` (`operationId`, `accepted`).  
**Nível:** U  
**Fonte histórica:** 06c, 06d §8.3.7

---

### UC-CLR-008

**Caso:** Nó em sincronização ou em drain devolve `503` `cluster.node-syncing` ou `cluster.node-draining` nas rotas que não pode atender. Um nó em sincronização confirma `register` gravando no banco de eventos (UC-ESC-031).  
**Nível:** C  
**Fonte histórica:** 06c

---


## CFG: Configuração e validações de partida

### UC-CFG-006

**Caso:** Todos os nós rodam a **mesma imagem**; o papel de cada um vem só da configuração (`cluster.first-node`, `cluster.entry-address`, `replication.factor`).  
**Nível:** I  
**Fonte histórica:** 01, 06 §8.0.1

---


## Casos retirados

Os códigos nunca são reaproveitados: UC-DEF-001, UC-DEF-002, UC-DEF-003, UC-DEF-004, UC-DEF-005, UC-DEF-006, UC-DEF-007, UC-DEF-008, UC-OPS-008, UC-OPS-009, UC-OPS-010, UC-DRN-021, UC-DRN-022, UC-DRN-023, UC-DRN-024, UC-DRN-025, UC-DRN-026, UC-DRN-027, UC-DEV-001, UC-DEV-003, UC-DEV-004, UC-DEV-005, UC-DEV-010, UC-DEV-013, UC-RTN-011, UC-SEG-005, UC-ANE-011, UC-DEF-023, UC-ENT-007, UC-INV-032, UC-ING-025, UC-FRZ-008, UC-CNC-008, UC-SIN-024, UC-FWD-012, UC-DEV-006, UC-DEV-007, UC-DEV-008, UC-DEV-009, UC-CFG-007, UC-TOP-019, UC-OPS-027 (o modo desenvolvimento e o drain como save saíram, [07-cluster.md](07-cluster.md), seção 8; UC-TOP-019, porque a fila FIFO aceita qualquer `replication.factor`, [09-topicos.md](09-topicos.md); UC-OPS-027, porque a rota `GET /cluster/metrics/topics` não existe, [18-api-cluster.md](18-api-cluster.md)).

## Fontes e provas

- Catálogo: `doc/20a`, `20b`, `20c`. Testes: as linhas `/// UC:` do código (regra de `uc_coverage.rs`).
- Mantido à mão: um caso que ganha o primeiro teste passa para [26a](26a-casos-de-uso-implementados.md).
