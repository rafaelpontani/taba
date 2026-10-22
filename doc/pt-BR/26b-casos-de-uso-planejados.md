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

**Placar (2026-10-13, commit 71183b9):** **183** casos sem teste, de **885**. Os casos marcados "(pendente: D##)" dependem de uma dúvida ainda aberta ([31-pendencias.md](31-pendencias.md)).

Nível: **U** unitário, **I** um nó real, **C** cluster simulado, **K** conector. Para fechar um caso, cite-o num teste (`/// UC: UC-XXX-NNN`) do nível indicado e regenere ([26-casos-de-uso.md](26-casos-de-uso.md)).

## Onde estão as lacunas

| Área | Assunto | Sem teste | Com teste |
|---|---|---:|---:|
| `TOP` | Tópicos: criar, listar, ler, alterar e remover | 4 | 45 |
| `MSG` | Publicação e o corpo da mensagem | 1 | 19 |
| `CNS` | Consumo (`GET`), long polling e `autoCreate` | 1 | 39 |
| `VIS` | Alteração da invisibilidade | 2 | 10 |
| `RET` | Expiração (`ttl`), limite de entregas e dead-letter | 6 | 14 |
| `DST` | Tópico Distribuidor | 12 | 6 |
| `GRP` | Grupos de mensagens e entrega justa | 1 | 16 |
| `AUT` | Autenticação e tokens de acesso | 6 | 33 |
| `LIM` | Rate limit e IP do cliente | 2 | 13 |
| `PAG` | Paginação | 2 | 7 |
| `CNV` | Convenções comuns da API (formato, cabeçalhos, erros, status) | 7 | 10 |
| `ESC` | Escrita: registro + commit, quórum, rollback, teto de commit | 14 | 22 |
| `ANE` | Anel de hash, blocos e papéis | 4 | 8 |
| `DEF` | Definição de tópicos e de tokens replicada no cluster | 11 | 19 |
| `ENT` | Entrega no dono: lista de prontos, `take`, long polling | 21 | 10 |
| `INV` | Invisibilidade no dono e nas réplicas | 19 | 13 |
| `ARM` | Armazenamento: memória, overflow, SQLite, integridade | 8 | 17 |
| `ING` | Entrada de nó (bootstrap) | 5 | 30 |
| `FAL` | Detecção de falhas, votação de exclusão e saída por falha | 1 | 32 |
| `FRZ` | Período de freezing | 1 | 13 |
| `CNC` | Concorrência entre mudanças de membros | 3 | 12 |
| `SIN` | Sincronização distribuída de blocos | 1 | 29 |
| `STR` | Streams NDJSON entre nós | 12 | 3 |
| `FWD` | Repasse de chamadas de cliente | 8 | 4 |
| `DRN` | Parada e modo drain (inclui o drain como save) | 3 | 25 |
| `DEV` | Modo desenvolvimento | 10 | 3 |
| `OPS` | Observabilidade (OpenTelemetry), estatísticas, métricas e saúde | 9 | 39 |
| `CLR` | Contrato das rotas do cluster (DTOs e regras comuns) | 7 | 3 |
| `CFG` | Configuração e validações de partida | 2 | 10 |

## TOP: Tópicos: criar, listar, ler, alterar e remover

### UC-TOP-007

**Caso:** Criar com tags válidas (até 20; chave de 1 a 64 e valor de 0 a 128 caracteres de `A-Za-z0-9_-.:`)  
**Então:** `201` e as tags voltam no `Topic`.  
**Nível:** I  
**Fonte histórica:** 06a

---

### UC-TOP-019

**Caso:** Criar fila FIFO com `replication.factor` abaixo de 2  
**Então:** `422` `topic.fifo-needs-replication`.  
**Nível:** C  
**Fonte histórica:** 06a, 17 §16.2

---

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

**Caso:** O primeiro `GET` com `autoCreate` paga o registro + commit em todos os nós; os seguintes não.  
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

### UC-RET-003

**Caso:** O padrão do `ttl` (14 dias) vale na hora da conferência  
**Então:** mudar `retention.ttl.default` afeta as filas sem `ttl` próprio, e não as que o definiram.  
**Nível:** U, I  
**Fonte histórica:** 13 §12.10

---

### UC-RET-010

**Caso:** A mensagem na dead-letter cai no mesmo bloco e nos mesmos nós da mensagem original (mesmo `id`).  
**Nível:** U, C  
**Fonte histórica:** 13 §12.10

---

### UC-RET-012

**Caso:** A dead-letter não tem definição própria  
**Então:** não há linha na tabela `topics` e as configurações são as da fila de origem.  
**Nível:** I  
**Fonte histórica:** 13 §12.10, 02 §3.3

---

### UC-RET-015

**Caso:** O sufixo da dead-letter é configurável (`topics.dead-letter.suffix`) e passa a ser o nome reservado.  
**Nível:** U, I  
**Fonte histórica:** 10

---

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

### UC-DST-004

**Caso:** Uma fila que casa com **dois ou mais prefixos do mesmo** distribuidor recebe **uma** cópia.  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-005

**Caso:** Uma fila que casa com prefixos de **distribuidores diferentes** recebe uma cópia de cada publicação em cada um.  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-006

**Caso:** Só filas são destino  
**Então:** um distribuidor nunca recebe cópia (sem laços).  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-007

**Caso:** O casamento é feito na hora da publicação  
**Então:** uma fila criada depois só recebe o que for publicado depois.  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-008

**Caso:** Falha parcial  
**Então:** uma cópia falha (por exemplo, sem quórum) e as outras seguem; a resposta é `200`, com `error` (`code`, `message`) no item que falhou e `id` nos demais, e o evento `taba.distributor.partial`. Nada é desfeito.  
**Nível:** C  
**Fonte histórica:** 06a, 15 §14.2

---

### UC-DST-010

**Caso:** O `groupId` e o `deduplicationId` informados vão para todas as cópias; sem `deduplicationId`, cada cópia usa o hash do corpo.  
**Nível:** I  
**Fonte histórica:** 15 §14.2, 17 §16.8

---

### UC-DST-012

**Caso:** Num distribuidor vale o `message.max-bytes` geral, e as cópias não são conferidas de novo contra o `maxMessageBytes` de cada fila.  
**Nível:** I  
**Fonte histórica:** 12

---

### UC-DST-013

**Caso:** O `PATCH` de `prefixes` vale a partir do commit, só para publicações posteriores.  
**Nível:** I  
**Fonte histórica:** 06a, 15 §14.2

---

### UC-DST-014

**Caso:** O distribuidor não guarda mensagens  
**Então:** não há mensagem em memória nem em disco para ele, e nenhuma lista de prontos.  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-015

**Caso:** Não há ordem entre as filas destino. O teste não pode exigir ordem.  
**Nível:** I  
**Fonte histórica:** 15 §14.2

---

### UC-DST-016

**Caso:** `GET` e ack num distribuidor  
**Então:** `409` `topic.wrong-type`.  
**Nível:** I  
**Fonte histórica:** 15 §14.1

---

### UC-DST-017

**Caso:** A permissão `publish` é conferida só no distribuidor; as filas destino não são conferidas de novo (nem os prefixos do token).  
**Nível:** I  
**Fonte histórica:** 06a §8.1.3

---


## GRP: Grupos de mensagens e entrega justa

### UC-GRP-014

**Caso:** Num distribuidor, o `groupId` é repassado a todas as cópias.  
**Nível:** I  
**Fonte histórica:** 13 §12.11, 15 §14.2

---


## AUT: Autenticação e tokens de acesso

### UC-AUT-004

**Caso:** `auth.enabled=true` sem token raiz impede o nó de subir; um `auth.root-token-file` definido que não existe também.  
**Nível:** I  
**Fonte histórica:** 06a §8.1.3, 10

---

### UC-AUT-019

**Caso:** A validação é uma consulta local em memória, sem chamada de rede  
**Então:** com os outros nós fora do ar, um token válido continua aceito.  
**Nível:** C  
**Fonte histórica:** 06a §8.1.3, 02 §3.4

---

### UC-AUT-030

**Caso:** Criar, alterar e revogar usam o registro + commit com confirmação de **todos** os nós; com algum nó fora do ar  
**Então:** `503` `cluster.registration-failed` (e `cluster.commit-timeout` conforme o caso).  
**Nível:** C  
**Fonte histórica:** 06a, 02 §3.4

---

### UC-AUT-031

**Caso:** O segredo nunca trafega entre nós  
**Então:** o `register` leva só o `salt` e o `secretHash`.  
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

### UC-PAG-009

**Caso:** `GET /metrics/topics` não aceita `sort` (`400`) e pagina sempre por nome.  
**Nível:** I  
**Fonte histórica:** 06b, 11

---


## CNV: Convenções comuns da API (formato, cabeçalhos, erros, status)

### UC-CNV-005

**Caso:** `traceparent` na requisição  
**Então:** as operações do nó entram no mesmo trace.  
**Nível:** I  
**Fonte histórica:** 06d §8.3.2, 11

---

### UC-CNV-010

**Caso:** `POST`, `PATCH` e `DELETE` que não esperam corpo ignoram o que vier; `204` nunca tem corpo.  
**Nível:** I  
**Fonte histórica:** 06d §8.3.1

---

### UC-CNV-013

**Caso:** As rotas `/health/*` devolvem `HealthResponse` (nunca `ErrorResponse`), e uma falha **depois do `200`** de um stream NDJSON usa a linha `{"error":"..."}`.  
**Nível:** I, C  
**Fonte histórica:** 06d §8.3.3

---

### UC-CNV-014

**Caso:** Erro dentro de um resultado parcial (`200` do distribuidor) leva `error` com `code` e `message`, **sem status**  
**Então:** o status do item é o do `code` no catálogo.  
**Nível:** I  
**Fonte histórica:** 06d §8.3.3

---

### UC-CNV-015

**Caso:** Um `503` ou `504` de operação que cria ou apaga (tópico, token) pode já ter sido aplicado em parte do cluster  
**Então:** ao repetir, `409` `topic.already-exists` (criação) ou `404` (remoção) significam que a operação já tinha sido aplicada.  
**Nível:** C  
**Fonte histórica:** 06d §8.3.5

---

### UC-CNV-016

**Caso:** `Accept: application/openmetrics-text` no `/metrics` pede OpenMetrics; o padrão é o texto do Prometheus.  
**Nível:** I  
**Fonte histórica:** 06d §8.3.2, 06b

---

### UC-CNV-017

**Caso:** Os DTOs reutilizados `Ack`, `StreamEnd` e `ConditionEntry` têm os campos e tipos definidos em 06d §8.3.7 (`severity` de `ConditionEntry` só vale `info`, `warning` ou `critical`).  
**Nível:** U  
**Fonte histórica:** 06d §8.3.7

---


## ESC: Escrita: registro + commit, quórum, rollback, teto de commit

### UC-ESC-001

**Caso:** O coordenador gera o GUID v7 **antes** da fase de registro e calcula, pelo ID, o bloco e os nós responsáveis (primário, secundário, terciário).  
**Nível:** U, C  
**Fonte histórica:** 05 §7.2

---

### UC-ESC-008

**Caso:** O padrão de `write.quorum` é `floor(rf / 2) + 1`  
**Então:** 2 com rf 3, 3 com rf 5, 1 com rf 1.  
**Nível:** U  
**Fonte histórica:** 05 §7.2, 10

---

### UC-ESC-009

**Caso:** `write.quorum` é validado entre 1 e `replication.factor`; fora disso, a configuração é recusada. Abaixo da maioria é aceito.  
**Nível:** U  
**Fonte histórica:** 05 §7.2, 10

---

### UC-ESC-012

**Caso:** `write.quorum` é um valor **único** para escrita, entrega (invisibilidade) e ack.  
**Nível:** U, C  
**Fonte histórica:** 05 §7.2

---

### UC-ESC-018

**Caso:** O rollback só se aplica **antes** da decisão  
**Então:** nunca é enviado depois de o quórum decidir.  
**Nível:** C  
**Fonte histórica:** 05 §7.2

---

### UC-ESC-026

**Caso:** Todas as fases são **idempotentes pelo `operationId`**  
**Então:** repetir `register`, `commit` ou `rollback` devolve o mesmo `Ack`, sem efeito novo.  
**Nível:** U, C  
**Fonte histórica:** 06c

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

### UC-ANE-011

**Caso:** Com um nó só (modo desenvolvimento) o nó é primário de todos os blocos, sem rebalanceamento.  
**Nível:** I  
**Fonte histórica:** 05 §7.1

---

### UC-ANE-012

**Caso:** As métricas do anel  
**Então:** `taba.ring.virtual_nodes`, e `taba.ring.blocks` por papel (`primary`, `secondary`, `tertiary`) e estado (`healthy`, `syncing`).  
**Nível:** C  
**Fonte histórica:** 11

---


## DEF: Definição de tópicos e de tokens replicada no cluster

### UC-DEF-010

**Caso:** O arquivo guarda **só a definição**  
**Então:** dados de consumo (profundidade, taxas, histogramas) nunca vão para ele.  
**Nível:** I  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-011

**Caso:** A dead-letter não tem linha de definição na tabela `topics`  
**Então:** é derivada da fila de origem, criada e excluída na mesma operação.  
**Nível:** I  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-012

**Caso:** Cada token é uma linha da tabela `tokens` do `_node.db` (com `id`, `name`, permissões, prefixos, datas, o sal e o hash salgado do segredo, nunca o segredo).  
**Nível:** I  
**Fonte histórica:** 02 §3.4

---

### UC-DEF-013

**Caso:** Na partida, o nó (em desenvolvimento, retomando do disco) carrega as definições da tabela `topics` do `_node.db`, preservando o `id` e a data de criação de cada uma.  
**Nível:** I  
**Fonte histórica:** 04 §6.10

---

### UC-DEF-014

**Caso:** O `DELETE` sempre funciona, mesmo com mensagens não entregues ou não confirmadas. No commit, cada nó apaga todas as mensagens do tópico que tem localmente (memória, `${topic}/` em disco e metadados), sem checar se estão pendentes.  
**Nível:** C  
**Fonte histórica:** 02 §3.3

---

### UC-DEF-017

**Caso:** A carga é aplicada direto no armazenamento final; tudo o que chega por registro e commit durante a carga (criação, alteração, remoção) vai para o banco de eventos, na ordem de chegada, e o nó confirma o registro **depois de gravar**.  
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

### UC-DEF-023

**Caso:** Em modo desenvolvimento a definição é aplicada só no próprio nó, sem esperar outros, sem bootstrap nem carga.  
**Nível:** I  
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

### UC-ENT-007

**Caso:** Em modo desenvolvimento o nó é o dono de todas as mensagens.  
**Nível:** I  
**Fonte histórica:** 13 §12.7

---

### UC-ENT-008

**Caso:** A lista de prontos é mantida por tópico em cada nó, em memória, com `delivery.ready-list.size` IDs (padrão 1000).  
**Nível:** U  
**Fonte histórica:** 13 §12.6

---

### UC-ENT-009

**Caso:** Uma thread repõe a lista quando o semáforo abre espaço (entrega commitada, ID que sai), repondo a quantidade que abriu, sem consultar à toa.  
**Nível:** U  
**Fonte histórica:** 13 §12.6

---

### UC-ENT-010

**Caso:** A reposição segue a sanfona  
**Então:** dois cursores sobre as mensagens do nó (da mais antiga para a frente e da mais nova para trás, por GUID v7, em memória e em disco), alternando as duas pontas.  
**Nível:** U  
**Fonte histórica:** 13 §12.6

---

### UC-ENT-011

**Caso:** A entrega consome as duas pontas de forma **intercalada**  
**Então:** a mais antiga, a mais nova, a segunda mais antiga, a segunda mais nova.  
**Nível:** U  
**Fonte histórica:** 13 §12.6

---

### UC-ENT-012

**Caso:** Uma mensagem nova que chega ao dono entra direto na lista se há lugar e o cursor da ponta nova está no fim; senão é encontrada depois pela thread.  
**Nível:** U  
**Fonte histórica:** 13 §12.6

---

### UC-ENT-014

**Caso:** A mensagem cuja invisibilidade vence volta à lista pela posição do GUID v7, **não** pela varredura; se a lista está cheia, o ID do **meio** sai para dar lugar e é reposto depois (nenhuma mensagem se perde).  
**Nível:** U  
**Fonte histórica:** 13 §12.6, 14 §13.6

---

### UC-ENT-015

**Caso:** O `GET` só consulta a lista, sem varrer memória e disco a cada chamada.  
**Nível:** U  
**Fonte histórica:** 13 §12.6

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

### UC-ENT-019

**Caso:** O peer é sorteado com **peso** proporcional às mensagens prontas informadas no sinal de vida; sem informação de quantidade, o sorteio é uniforme.  
**Nível:** U  
**Fonte histórica:** 13 §12.8

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

### UC-INV-001

**Caso:** Os metadados são `deliveryCount`, `invisibleUntil` (data absoluta) e `receiptToken`, guardados e replicados juntos  
**Então:** no mesmo registro + commit, em memória ou no banco local do tópico.  
**Nível:** U, C  
**Fonte histórica:** 14 §13.2

---

### UC-INV-002

**Caso:** `invisibleUntil` é calculado pelo **dono** (`agora + visibilityTimeout`, pelo relógio dele); as réplicas guardam o valor pronto e nunca recalculam a partir de duração.  
**Nível:** C  
**Fonte histórica:** 14 §13.2 e 13.3

---

### UC-INV-005

**Caso:** Os estados da mensagem para o dono  
**Então:** **pronta** (`invisibleUntil` ausente ou no passado), **invisível** (no futuro) e **removida** (ack).  
**Nível:** U  
**Fonte histórica:** 14 §13.1

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

### UC-INV-017

**Caso:** A thread do dono olha a cabeça da lista de invisíveis, dorme até o prazo dela (com um teto curto para reagir a alterações e entradas mais cedo) e, no vencimento, remove a entrada e **confere** nos metadados  
**Então:** a mensagem ainda existe, o nó ainda é dono e o prazo ainda é o da entrada; senão descarta e segue.  
**Nível:** U  
**Fonte histórica:** 14 §13.6

---

### UC-INV-018

**Caso:** Passada a conferência, a mensagem entra na lista de prontos pela posição do GUID v7 (perto da ponta antiga); com a lista cheia, o ID do meio sai.  
**Nível:** U  
**Fonte histórica:** 14 §13.6

---

### UC-INV-020

**Caso:** Na fila FIFO, a mensagem que volta é a cabeça do grupo; o grupo deixa de estar ocupado e entra na lista de prontos pela posição do ID, sem a regra do meio da lista.  
**Nível:** U  
**Fonte histórica:** 14 §13.6

---

### UC-INV-021

**Caso:** Se a lista de invisíveis esvazia e ainda há linhas na tabela de expiração, a thread faz a recarga antes de dormir.  
**Nível:** U  
**Fonte histórica:** 14 §13.6

---

### UC-INV-022

**Caso:** A lista de invisíveis é, por tópico e só no dono, uma lista de pares (ID, `invisibleUntil`) ordenada por prazo, com no máximo `delivery.invisible-list.size` entradas; busca por ID para alteração e ack.  
**Nível:** U  
**Fonte histórica:** 14 §13.4

---

### UC-INV-023

**Caso:** Regra de inclusão com a lista cheia  
**Então:** se o prazo novo é **anterior** ao último da lista, o par novo entra e o último vai para a tabela; senão o par novo vai direto para a tabela.  
**Nível:** U  
**Fonte histórica:** 14 §13.5

---

### UC-INV-024

**Caso:** Invariante  
**Então:** tudo na lista vence antes de tudo na tabela; a thread nunca consulta a tabela enquanto a lista não esvazia.  
**Nível:** U  
**Fonte histórica:** 14 §13.5

---

### UC-INV-025

**Caso:** Recarga  
**Então:** abaixo de `delivery.invisible.reload-threshold` (ou vazia), a thread lê da tabela os pares de menor `invisibleUntil` (consulta ordenada com limite pelo índice), move-os para a lista e apaga as linhas lidas.  
**Nível:** U, I  
**Fonte histórica:** 14 §13.5

---

### UC-INV-026

**Caso:** Sem entradas velhas  
**Então:** ack, alteração e nova entrega apagam ou atualizam a linha da tabela no mesmo commit que atualiza a lista. Não há remoção preguiçosa.  
**Nível:** U, I  
**Fonte histórica:** 14 §13.5

---

### UC-INV-027

**Caso:** A gravação na tabela faz parte do commit no dono  
**Então:** se falhar, o commit não se completa e a entrega é desfeita antes da resposta ao cliente.  
**Nível:** I  
**Fonte histórica:** 14 §13.5

---

### UC-INV-028

**Caso:** Com a lista com espaço, nenhuma escrita além dos metadados.  
**Nível:** U  
**Fonte histórica:** 14 §13.5

---

### UC-INV-031

**Caso:** Reinício  
**Então:** a tabela de expiração é descartada junto com o estado antigo; as listas são reconstruídas quando o nó assumir blocos.  
**Nível:** C  
**Fonte histórica:** 14 §13.7

---

### UC-INV-032

**Caso:** Em modo desenvolvimento o registro e o commit acontecem só no próprio nó (quórum 1), sem propagação; só vale o caso do reinício.  
**Nível:** I  
**Fonte histórica:** 14 §13.3 e 13.7

---


## ARM: Armazenamento: memória, overflow, SQLite, integridade

### UC-ARM-008

**Caso:** A lista de prontos e as mensagens em memória não mudam por causa do banco  
**Então:** ele só cuida do que está em disco.  
**Nível:** U  
**Fonte histórica:** 02 §3.2

---

### UC-ARM-012

**Caso:** A tabela `expiry` guarda os pares que não couberam na lista de invisíveis, com índice por prazo.  
**Nível:** I  
**Fonte histórica:** 02 §3.2, 14 §13.5

---

### UC-ARM-016

**Caso:** O banco responde  
**Então:** a mensagem mais antiga do tópico (`MIN(id)`, somada à mais antiga em memória, mantida ordenada por ID), os próximos a vencer (índice por `invisibleUntil`), as mensagens de um bloco (faixa pela posição no anel) e as contagens do tópico.  
**Nível:** I  
**Fonte histórica:** 02 §3.2

---

### UC-ARM-018

**Caso:** O banco do nó, `${storage.dir}/_node.db`, é estritamente local, com as tabelas `topics` (as definições de tópico) e `losses` (`topic`, `id`, `block`, `detected_at`), escrita por uma thread única.  
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

### UC-ARM-021

**Caso:** O nó cria `storage.dir` (padrão `/var/lib/taba`, a última pasta se chama `taba`) ao iniciar; se não conseguir criar ou gravar, não sobe.  
**Nível:** I  
**Fonte histórica:** 10

---

### UC-ARM-025

**Caso:** Dados de **consumo** (profundidade, visíveis, invisíveis, taxas, latências) ficam só em memória e específicos de cada nó.  
**Nível:** I  
**Fonte histórica:** 02 §3.3

---


## ING: Entrada de nó (bootstrap)

### UC-ING-007

**Caso:** O nó se auto-nomeia com um **GUID v7** a cada início do processo, só em memória, e se identifica por ele, inclusive no pedido de ingresso.  
**Nível:** U, C  
**Fonte histórica:** 04 §6.7

---

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

### UC-ING-025

**Caso:** Em modo desenvolvimento não há bootstrap  
**Então:** o nó sobe sozinho, e outro nó não consegue ingressar (a porta do cluster não é aberta). Um nó em produção que tenta ingressar num nó em desenvolvimento não consegue.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 06c

---

### UC-ING-026

**Caso:** A tabela de membros **não** é persistida  
**Então:** o reinício do processo perde a tabela e o nó volta como nó novo.  
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


## FRZ: Período de freezing

### UC-FRZ-008

**Caso:** Em modo desenvolvimento o freezing não se aplica (`cluster.freezing.*` é ignorado).  
**Nível:** I  
**Fonte histórica:** 04 §6.4

---


## CNC: Concorrência entre mudanças de membros

### UC-CNC-008

**Caso:** Em modo desenvolvimento não há mudança de membros a coordenar.  
**Nível:** I  
**Fonte histórica:** 04 §6.4

---

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


## SIN: Sincronização distribuída de blocos

### UC-SIN-024

**Caso:** Em modo desenvolvimento não há entrada de nó, sincronização, banco de eventos nem recuperação de perdas.  
**Nível:** I  
**Fonte histórica:** 05 §7.3

---


## STR: Streams NDJSON entre nós

### UC-STR-002

**Caso:** A ordem é por **ID ascendente** (GUID v7 comparado como texto na forma canônica em minúsculas).  
**Nível:** U  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-003

**Caso:** `from` e `to` (ambos inclusivos e opcionais) delimitam a faixa; `limit` opcional reduz o teto de itens.  
**Nível:** U, C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-005

**Caso:** Teto por stream  
**Então:** o emissor fecha ao atingir `cluster.sync.stream.max-items` ou `max-bytes`, com `hasMore: true`; o receptor abre outro com `from=<last>`.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-006

**Caso:** Retomada depois de um corte ou de `hasMore`  
**Então:** o receptor repete com `from=<último ID lido>`; o emissor devolve de novo essa linha e o receptor a **ignora** (aplicação idempotente por ID). Sem token nem estado no servidor.  
**Nível:** U, C  
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

### UC-STR-009

**Caso:** Limite do tamanho da linha (`cluster.sync.stream.max-line-bytes`)  
**Então:** precisa caber a maior mensagem possível (hard-limit em base64 mais o envelope).  
**Nível:** U  
**Fonte histórica:** 06c §8.2.1, 10

---

### UC-STR-010

**Caso:** Contrapressão  
**Então:** o receptor lê no próprio ritmo e o emissor não mantém transação nem lista congelada aberta; lê o armazenamento em blocos pequenos.  
**Nível:** C  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-011

**Caso:** Linhas de cada rota  
**Então:** `TopicDefinition`, `TokenRecord`, `{id, topic}` (e `deleteAt` na FIFO), `{id, topic}` das perdas e `MessageRecord` (sem `body` e com `deleteAt` na FIFO com exclusão lógica).  
**Nível:** U  
**Fonte histórica:** 06c §8.2.1

---

### UC-STR-013

**Caso:** `POST /cluster/messages/recover`  
**Então:** corpo NDJSON de `MessageRecord` terminado por `StreamEnd`; a resposta é `RecoverResult` (`end: true`, `count` aplicadas); se a conexão cair, quem envia reenvia tudo e as já aplicadas são ignoradas.  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-STR-014

**Caso:** O contrato não cria tombstone nem log novo  
**Então:** a remoção continua por ausência do ID.  
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

### UC-FWD-012

**Caso:** Em modo desenvolvimento nenhuma chamada é repassada.  
**Nível:** I  
**Fonte histórica:** 04 §6.4 e 6.9

---


## DRN: Parada e modo drain (inclui o drain como save)

### UC-DRN-015

**Caso:** `drain.timeout` precisa ser menor que o prazo de parada do orquestrador (`terminationGracePeriodSeconds`, 30 s por padrão no Kubernetes).  
**Nível:** I  
**Fonte histórica:** 04 §6.10, 10

---

### UC-DRN-025

**Caso:** O `ttl` conta do ID, então o tempo parado conta  
**Então:** o que venceu enquanto o nó estava fora é descartado na primeira varredura.  
**Nível:** I  
**Fonte histórica:** 04 §6.10

---

### UC-DRN-028

**Caso:** Em modo desenvolvimento, as chamadas de cliente durante o drain respondem `503` com `Retry-After` (não há outro nó para repassar).  
**Nível:** I  
**Fonte histórica:** 04 §6.4 e 6.10

---


## DEV: Modo desenvolvimento

### UC-DEV-001

**Caso:** `mode` vale `production` por padrão; `development` (`TABA_MODE`) faz o nó ser um cluster de um só. O modo não muda em execução.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 10

---

### UC-DEV-003

**Caso:** `replication.factor` e `write.quorum` valem **1**; os valores configurados são ignorados, com aviso no log. O registro e o commit acontecem só no próprio nó, sem réplica, retry entre nós nem rollback por falta de quórum.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 05 §7.2

---

### UC-DEV-004

**Caso:** Os parâmetros `cluster.*`, `ring.*`, `replication.*` e `write.quorum` são ignorados, com aviso no log.  
**Nível:** I  
**Fonte histórica:** 10

---

### UC-DEV-005

**Caso:** Ao subir o nó registra `warn` informando o modo, e `GET /health/cluster` traz o campo `mode` de cada nó.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 06b

---

### UC-DEV-006

**Caso:** O comportamento de cliente é o mesmo do cluster  
**Então:** API, overflow, entrega e invisibilidade, `ttl`, `maxDeliveries`, dead-letter, distribuidores e filas voláteis, limites e rate limit (a mesma bateria de casos `I` roda aqui).  
**Nível:** I  
**Fonte histórica:** 04 §6.4

---

### UC-DEV-007

**Caso:** O nó é dono de todas as mensagens  
**Então:** não há `take` nem avisos entre nós, e o long polling é só local.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 13 §12.7

---

### UC-DEV-008

**Caso:** O nó é o gestor de todas as filas voláteis, e a verificação final consulta só ele.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 15 §14.5

---

### UC-DEV-009

**Caso:** A entrega justa por grupos vale igual, com contagem exata.  
**Nível:** I  
**Fonte histórica:** 04 §6.4, 13 §12.11

---

### UC-DEV-010

**Caso:** A autenticação nunca é exigida e as rotas `/tokens` respondem `404` `route.not-found` (UC-AUT-007 e UC-AUT-025).  
**Nível:** I  
**Fonte histórica:** 04 §6.4

---

### UC-DEV-013

**Caso:** Sem réplicas, perder o nó ou o disco é perder as mensagens (exceto pelo drain como save, UC-DRN-021 a UC-DRN-027).  
**Nível:** I  
**Fonte histórica:** 04 §6.4

---


## OPS: Observabilidade (OpenTelemetry), estatísticas, métricas e saúde

### UC-OPS-015

**Caso:** As condições aparecem como gauges `0/1` com a unidade `{flag}` (`taba_cluster_condition{taba_condition=...}`).  
**Nível:** I  
**Fonte histórica:** 11

---

### UC-OPS-020

**Caso:** O resultado de `/metrics/topics` fica em cache por `stats.topics.cache-ttl` (com `collectedAt`); o filtro e o `limit` são aplicados **depois** do merge entre nós; sem `sort` (`400`).  
**Nível:** C  
**Fonte histórica:** 06b, 11

---

### UC-OPS-023

**Caso:** Visão do nó × visão do cluster  
**Então:** **só o primário conta** (senão a soma seria multiplicada pelo fator de replicação); o cluster soma `visibleMessages`, `invisibleMessages` e `depth`, e `oldestMessageAt` é a **menor** data entre os nós; `oldestAge = agora − oldestMessageAt`.  
**Nível:** U, C  
**Fonte histórica:** 11

---

### UC-OPS-025

**Caso:** Nó que não responde fica de fora e o resultado é marcado `partial`  
**Então:** as somas viram piso.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-OPS-027

**Caso:** `GET /cluster/metrics/topics` devolve `TopicCounters` (`collectedAt` e `topics` com `visibleMessages`, `invisibleMessages`, `oldestMessageAt`), só das mensagens em que o nó é primário; o parâmetro `topic` pede só um tópico.  
**Nível:** C  
**Fonte histórica:** 06c, 11

---

### UC-OPS-030

**Caso:** Degradação de replicação (`under-replicated`, `syncing`) é propriedade do **cluster**, não do tópico  
**Então:** aparece na condição do cluster, não na do tópico.  
**Nível:** C  
**Fonte histórica:** 11

---

### UC-OPS-036

**Caso:** O nó não sobe se `operations.port` estiver ocupada ou se `operations.allowed-ips` tiver entrada inválida.  
**Nível:** I  
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

### UC-CLR-002

**Caso:** Todo `register`, `commit` e `rollback` leva o `operationId` (GUID v7 do coordenador) e é idempotente por ele. Um registro sem commit nem rollback expira após `write.pending.ttl`.  
**Nível:** U, C  
**Fonte histórica:** 06c

---

### UC-CLR-003

**Caso:** A resposta de `register`, `commit` e `rollback` é `Ack` (`operationId`, `accepted`).  
**Nível:** U  
**Fonte histórica:** 06c, 06d §8.3.7

---

### UC-CLR-004

**Caso:** Corpo inválido nas rotas do cluster  
**Então:** `400` `validation.failed`, `validation.required` ou `validation.unknown-field`.  
**Nível:** U, C  
**Fonte histórica:** 06c

---

### UC-CLR-005

**Caso:** Os DTOs do cluster (`Member`, `MemberTable`, `JoinRequest`, `NodeRegisterRequest`, `NodeUnregisterRequest`, `OperationRef`, `VoteRequest`, `VoteResponse`, `TopicRegisterRequest`, `TopicDefinition`, `TokenRegisterRequest`, `TokenRecord`, `MessageRegisterRequest`, `VisibilityRegisterRequest`, `AckRegisterRequest`, `TakeRequest`, `TakeResponse`, `MessageRecord`, `IdleResponse`, `RecoverResult`, `TopicCounters`) serializam e desserializam com os campos e tipos documentados, e os valores `null` de `ttl`, `maxDeliveries` e `maxMessageBytes` da `TopicDefinition` são enviados como `null`.  
**Nível:** U  
**Fonte histórica:** 06c

---

### UC-CLR-006

**Caso:** O DTO `TopicDefinition` é igual ao JSON guardado na tabela `topics`  
**Então:** os campos de `Topic` sem `effective`, `deadLetter` e `source`.  
**Nível:** U  
**Fonte histórica:** 06c, 02 §3.3

---

### UC-CLR-008

**Caso:** Nó em sincronização ou em drain devolve `503` `cluster.node-syncing` ou `cluster.node-draining` nas rotas que não pode atender. Um nó em sincronização confirma `register` gravando no banco de eventos (UC-ESC-031).  
**Nível:** C  
**Fonte histórica:** 06c

---

### UC-CLR-010

**Caso:** Códigos só do cluster  
**Então:** `cluster.not-member`, `cluster.node-frozen`, `cluster.join-unreachable`, `cluster.conflict`, `cluster.wrong-node`, `fifo.misdirected` e `cluster.operation-unknown`.  
**Nível:** U, C  
**Fonte histórica:** 06d §8.3.6

---


## CFG: Configuração e validações de partida

### UC-CFG-006

**Caso:** Todos os nós rodam a **mesma imagem**; o papel de cada um vem só da configuração (`cluster.first-node`, `mode`).  
**Nível:** I  
**Fonte histórica:** 01, 06 §8.0.1

---

### UC-CFG-007

**Caso:** `mode=development` ignora `cluster.*`, `ring.*`, `replication.*` e `write.quorum` com aviso (UC-DEV-004).  
**Nível:** I  
**Fonte histórica:** 10

---


## Casos retirados

Os códigos nunca são reaproveitados: UC-DEF-001, UC-DEF-002, UC-DEF-003, UC-DEF-004, UC-DEF-005, UC-DEF-006, UC-DEF-007, UC-DEF-008, UC-OPS-008, UC-OPS-009, UC-OPS-010.

## Fontes e provas

- Catálogo: `doc/20a`, `20b`, `20c`. Testes: as linhas `/// UC:` do código (regra de `uc_coverage.rs`).
- Mantido à mão: um caso que ganha o primeiro teste passa para [26a](26a-casos-de-uso-implementados.md).
