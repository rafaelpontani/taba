# Casos de uso: o nó e o caminho da mensagem

[← Índice](README.md) · [Convenções e níveis de teste](20-casos-de-uso.md)

## 19.B Casos de uso do nó: o caminho da mensagem

O que o nó faz com uma mensagem, do recebimento ao disco: escrita por quórum, anel de hash, definição de tópicos replicada, entrega pelo dono, canal entre nós, invisibilidade e armazenamento. Quase tudo aqui precisa de **mais de um nó**, então o nível padrão é `C` (cluster simulado, com `Transport` e `Clock` de teste); as regras puras são `U`. Convenções: [20-casos-de-uso.md](20-casos-de-uso.md). A entrada e a saída de nós estão em [20c-casos-de-uso-no-cluster.md](20c-casos-de-uso-no-cluster.md).

Nos exemplos, "rf" é `replication.factor` e "quórum" é `write.quorum`.

### ESC: escrita, registro + commit e quórum

Regras: [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2, e [02-arquitetura.md](02-arquitetura.md), seção 3.1.

#### Caminho feliz

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-001 | O coordenador gera o GUID v7 **antes** da fase de registro e calcula, pelo ID, o bloco e os nós responsáveis (primário, secundário, terciário). | 05 §7.2 | U, C | <!--t-->— |
| UC-ESC-002 | A fase 1 (registro) vai a **todos** os `replication.factor` nós responsáveis, não só ao quórum; cada um grava a mensagem como **pendente** em memória e confirma. | 05 §7.2, 02 §3.1 | C | <!--t-->`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it` |
| UC-ESC-003 | Atingido o quórum de confirmações do registro (primário incluso), o coordenador decide o commit, chama o commit nos nós do quórum e **continua** o registro + commit nas demais réplicas em segundo plano, sem bloquear a resposta. | 05 §7.2 | C | <!--t-->`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it` |
| UC-ESC-004 | A resposta ao cliente com o ID sai assim que o **commit está confirmado no quórum** (primário incluso); as réplicas fora do quórum ainda podem estar pendentes nesse instante. | 05 §7.2, 02 §3.1 | C | <!--t-->`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`<br>`tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call` |
| UC-ESC-005 | A durabilidade plena (commit confirmado em todas as `replication.factor` réplicas) é alcançada em seguida, em segundo plano. | 02 §3.1 | C | <!--t-->`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it` |
| UC-ESC-006 | O coordenador pode ser um dos nós de destino (inclusive o primário), sem chamada de rede para si mesmo. | 02 §3.1 | C | <!--t-->`tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call` |
| UC-ESC-007 | O nó que recebe a requisição é o coordenador, qualquer que seja o nó. | 02 §3.1 | C | <!--t-->`tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call` |

#### Quórum

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-008 | O padrão de `write.quorum` é `floor(rf / 2) + 1`: 2 com rf 3, 3 com rf 5, 1 com rf 1. | 05 §7.2, 10 | U | <!--t-->— |
| UC-ESC-009 | `write.quorum` é validado entre 1 e `replication.factor`; fora disso, a configuração é recusada. Abaixo da maioria é aceito. | 05 §7.2, 10 | U | <!--t-->— |
| UC-ESC-010 | O quórum **sempre inclui o primário**: com primário sem responder e secundário e terciário confirmando, o commit **não** é decidido, ainda que o número de confirmações seja o do quórum. | 05 §7.2 | U, C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |
| UC-ESC-011 | O quórum é primário mais `write.quorum − 1` **quaisquer** outras réplicas. | 05 §7.2 | U, C | <!--t-->`tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call` |
| UC-ESC-012 | `write.quorum` é um valor **único** para escrita, entrega (invisibilidade) e ack. | 05 §7.2 | U, C | <!--t-->— |
| UC-ESC-013 | Sem nós suficientes para o quórum, a operação falha **antes** de decidir (rollback do registro) e a API responde `503` `cluster.quorum-unavailable` com `Retry-After`. | 05 §7.2 | C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |
| UC-ESC-014 | Com rf 3 e quórum 2, a escrita continua funcionando com **um** nó responsável fora do ar. Com dois fora (o primário e um), a escrita falha com `503`. | 05 §7.2 | C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |

#### Falha antes do quórum (rollback)

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-015 | Falha de registro num nó antes do quórum: o coordenador repete a chamada a esse nó conforme `write.retry.attempts` e `write.retry.backoff`. | 05 §7.2 | C | <!--t-->`tests/replication.rs::a_lost_call_is_retried_before_the_quorum_gives_up` |
| UC-ESC-016 | Esgotados os retries **sem quórum**: erro ao cliente e **rollback** a todos os nós que já haviam confirmado, para descartar o registro pendente. Nenhum nó efetivou nada. | 05 §7.2, 02 §3.1 | C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |
| UC-ESC-017 | Um registro sem commit nem rollback é descartado sozinho depois de `write.pending.ttl`; o rollback só acelera a liberação. | 05 §7.2 | C | <!--t-->`tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself` |
| UC-ESC-018 | O rollback só se aplica **antes** da decisão: nunca é enviado depois de o quórum decidir. | 05 §7.2 | C | <!--t-->— |
| UC-ESC-019 | A mensagem descartada por rollback nunca aparece para um consumidor, e o ID gerado não é reaproveitado. | 05 §7.2 | C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |

#### Falha depois do quórum decidido

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-020 | Falha de registro ou commit numa réplica **fora do quórum**, depois da decisão: **nunca há rollback**; o coordenador insiste, dentro de `write.commit.timeout`. | 05 §7.2, 02 §3.1 | C | <!--t-->`tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`<br>`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |
| UC-ESC-021 | A réplica confirma dentro do teto: passa a contar como durável, sem novo aviso ao cliente (a resposta já foi dada). | 05 §7.2 | C | <!--t-->`tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down` |
| UC-ESC-022 | O teto esgota sem confirmação: o bloco fica abaixo do fator pleno (nunca abaixo do quórum), o evento `under-replicated` (não crítico) é gerado, `taba.write.commit_timeouts` sobe, o coordenador **continua tentando** em segundo plano, e a saúde do nó não muda. | 05 §7.2, 11 | C | <!--t-->`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |
| UC-ESC-023 | O commit é idempotente: repeti-lo numa réplica que já o aplicou não tem efeito. | 05 §7.2 | U, C | <!--t-->`src/engine/messages/tests/cluster.rs::applying_a_commit_twice_changes_nothing` |
| UC-ESC-024 | Quando a réplica atrasada volta, o commit em segundo plano a alcança e o bloco volta ao fator pleno (a condição `under-replicated` some). | 05 §7.2, 11 | C | <!--t-->`tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`<br>`tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` |

#### Rotas e protocolo entre os nós

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-025 | `POST /cluster/topics/{topic}/messages/{id}/register`, `commit` e `rollback` (fases 1, 2 e abortar) respondem `200` `Ack` com o `operationId` e `accepted: true`. | 06c | C | <!--t-->`src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back` |
| UC-ESC-026 | Todas as fases são **idempotentes pelo `operationId`**: repetir `register`, `commit` ou `rollback` devolve o mesmo `Ack`, sem efeito novo. | 06c | U, C | <!--t-->— |
| UC-ESC-027 | `commit` ou `rollback` de uma operação que o nó não conhece (já expirou ou nunca chegou): `404` `cluster.operation-unknown`. | 06c | C | <!--t-->— |
| UC-ESC-028 | `register` de um `id` que já tem registro pendente, ou de mensagem que já existe com conteúdo diferente: `409` `cluster.conflict`. | 06c | C | <!--t-->— |
| UC-ESC-029 | O `register` com `kind: "dead-letter"` cria a mensagem na dead-letter (com `deliveryCount: 0`, `invisibleUntil` nulo, `deadLetteredAt`, `deadLetterReason`) e remove a de `sourceTopic` no commit, numa operação só. | 06c, 13 §12.10 | C | <!--t-->`tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node` |
| UC-ESC-030 | Nó, tópico e token exigem a confirmação de **todos** os nós, sem quórum reduzido; mensagem decide por quórum. | 05 §7.2, 06c | C | <!--t-->`tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them` |
| UC-ESC-031 | Um nó **em sincronização** confirma o `register` gravando-o no banco de eventos (e o aplica depois); para o coordenador a confirmação conta normalmente para o quórum. | 06c, 05 §7.3 | C | <!--t-->— |
| UC-ESC-032 | Um nó que não é membro recebe `403` `cluster.not-member` em qualquer rota de escrita. | 06c | C | <!--t-->— |

#### Observabilidade da escrita

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ESC-033 | Coordenador: `taba.write.coordinated` (`committed` e `failed`), `taba.write.duration` por fase, `taba.write.retries`, `taba.write.rollbacks` (só por falha de quórum), `taba.write.replicas_confirmed` (por `stage`: `response` e `eventual`) e `taba.write.quorum_latency`. | 11 | C | <!--t-->— |
| UC-ESC-034 | Réplica: `taba.write.received` por fase, `taba.write.pending`, `taba.write.pending.oldest_age` e `taba.write.pending.expired`. | 11 | C | <!--t-->— |
| UC-ESC-035 | O trace da publicação tem o span raiz, o registro e o commit por nó de destino e, se for o caso, o rollback; o span raiz termina quando o quórum confirma; o `traceparent` é propagado entre os nós. Falhas e eventos de durabilidade são sempre mantidos na amostragem. | 11 | C | <!--t-->— |
| UC-ESC-036 | Os logs: falha de registro ou commit por tentativa (`warn`), escrita abortada com rollback (`error`), registro pendente expirado (`warn`), teto de commit esgotado numa réplica (`warn`). | 11 | C | <!--t-->— |

### ANE: anel de hash, blocos e papéis

Regras: [05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.1.

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ANE-001 | O ID da mensagem vira um hash, que cai numa posição do anel, que pertence a um bloco com os nós responsáveis em ordem (primário, secundário, terciário). | 05 §7.1 | U | <!--t-->`src/core/ring/tests.rs::a_message_id_becomes_a_position_that_belongs_to_a_block_with_ordered_owners` |
| UC-ANE-002 | Qualquer nó chega ao mesmo primário para o mesmo ID, a partir só do ID e da tabela de membros. | 05 §7.1, 13 §12.7 | U | <!--t-->`src/core/ring/tests.rs::any_node_reaches_the_same_owners_from_the_ids_alone` |
| UC-ANE-003 | Os nós responsáveis por um bloco são sempre **nós físicos distintos**: ao percorrer o anel, posições de nós virtuais de um nó físico já escolhido são puladas. | 05 §7.1 | U | <!--t-->`src/core/ring/tests.rs::owners_are_distinct_physical_nodes` |
| UC-ANE-004 | Cada nó físico ocupa `ring.virtual-nodes` posições; mais nós virtuais distribuem melhor os blocos (a variação da quantidade de blocos por nó diminui). | 05 §7.1 | U | <!--t-->`src/core/ring/tests.rs::more_virtual_nodes_spread_the_blocks_more_evenly` |
| UC-ANE-005 | O hash do GUID v7 distribui as mensagens de forma uniforme (o timestamp nos bits altos não concentra), tanto de mensagem quanto do `id` do tópico (nó gestor). | 05 §7.1, 15 §14.5 | U | <!--t-->`src/core/ring/tests.rs::the_hash_spreads_time_ordered_ids_over_the_whole_ring` |
| UC-ANE-006 | O tópico fica espalhado: as mensagens de um mesmo tópico caem em todo o anel; nenhum nó guarda o tópico inteiro nem é dono dele. | 05 §7.1 | C | <!--t-->— |
| UC-ANE-007 | A perda de um nó deixa cada mensagem ainda em pelo menos `replication.factor − 1` nós. | 05 §7.1 | C | <!--t-->— |
| UC-ANE-008 | O primário do bloco é o dono da entrega das mensagens dele. | 05 §7.1 | C | <!--t-->`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it` |
| UC-ANE-009 | Numa fila FIFO a posição é o hash de `id do tópico + ":" + groupId`, não o do ID da mensagem (UC-FIF-023). | 05 §7.1, 17 §16.3 | U | <!--t-->`src/core/ring/tests.rs::a_fifo_group_has_one_position_for_all_its_messages` |
| UC-ANE-010 | Quando a tabela de membros muda (entrada, saída, falha de registro), o anel é recalculado e os papéis de cada bloco mudam de acordo; sem o nó que falhou, o anel volta a ser o anterior. | 04 §6.5, 05 §7.1 | U, C | <!--t-->`src/cluster/members/tests.rs::the_ring_follows_the_table`<br>`src/core/ring/tests.rs::the_ring_is_recalculated_from_the_members_and_goes_back_without_the_failed_node`<br>`tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`<br>`tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table` |
| UC-ANE-011 | Com um nó só (modo desenvolvimento) o nó é primário de todos os blocos, sem rebalanceamento. | 05 §7.1 | I | <!--t-->— |
| UC-ANE-012 | As métricas do anel: `taba.ring.virtual_nodes`, e `taba.ring.blocks` por papel (`primary`, `secondary`, `tertiary`) e estado (`healthy`, `syncing`). | 11 | C | <!--t-->— |

### DEF: definição de tópicos e de tokens no cluster

Regras: [02-arquitetura.md](02-arquitetura.md), seções 3.3 e 3.4. A API de cliente correspondente está nos casos `TOP` e `AUT`.

#### Propagação

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-DEF-001 | (retirado: o registro + commit com confirmação de todos os nós foi trocado pelo dono da definição; ver UC-DEF-025 a UC-DEF-031.) | 02 §3.3 | — | <!--t-->— |
| UC-DEF-002 | (retirado: não há mais rollback do registro em todos os nós; a mudança só vale depois de decidida pelo dono e gravada num herdeiro, UC-DEF-030.) | 02 §3.3 | — | <!--t-->— |
| UC-DEF-003 | (retirado: a insistência depois da decisão passou a ser do dono, UC-DEF-031.) | 02 §3.3 | — | <!--t-->— |
| UC-DEF-004 | (retirado: a idempotência é a da versão, UC-DEF-026.) | 02 §3.3, 05 §7.3 | — | <!--t-->— |
| UC-DEF-005 | (retirado: `POST /cluster/topics/register` deixou de existir; ver UC-DEF-033.) | 06c | — | <!--t-->— |
| UC-DEF-006 | (retirado: as validações passaram a ser feitas pelo dono, UC-DEF-029.) | 06c | — | <!--t-->— |
| UC-DEF-007 | (retirado: o `expectedId` é conferido pelo dono, UC-DEF-029.) | 06c, 15 §14.5 | — | <!--t-->— |
| UC-DEF-008 | (retirado: `POST /cluster/tokens/register` deixou de existir; ver UC-DEF-033 e UC-DEF-034.) | 06c, 02 §3.4 | — | <!--t-->— |
| UC-DEF-025 | Cada definição tem um **dono** no anel: o primeiro nó ativo na posição do hash de `topic:<nome>` (ou `token:<id>`). O nó que recebe a chamada do cliente **propõe** a mudança ao dono (`definition-propose`), que decide; a API só responde quando a mudança já vale também no nó que recebeu a chamada. | 02 §3.3 e 3.4 | C | <!--t-->`tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner`<br>`tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers` |
| UC-DEF-026 | O dono dá a cada mudança a próxima **versão** (1, 2, 3...). Cada nó só aplica uma mudança de versão **maior ou igual** à que tem: repetir é inofensivo e uma mudança atrasada (versão menor) é ignorada. Todos os nós terminam na última versão. | 02 §3.3 | C | <!--t-->`tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back`<br>`tests/definitions.rs::each_change_gets_the_next_version_and_every_node_ends_on_the_last` |
| UC-DEF-027 | A remoção deixa uma **lápide** com a versão: uma criação ou alteração atrasada, de versão menor, não ressuscita o tópico (nem o token revogado). | 02 §3.3 e 3.4 | C | <!--t-->`tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back` |
| UC-DEF-028 | Duas criações do mesmo nome ao mesmo tempo, em nós diferentes, chegam ao mesmo dono, que as serializa: uma responde `201` e a outra `409` `topic.already-exists`. | 02 §3.3 | C | <!--t-->`tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict` |
| UC-DEF-029 | O dono confere a mudança com o próprio estado antes de decidir: `create` de nome existente é `409` `topic.already-exists`; `update` ou `delete` de tópico desconhecido é `404` `topic.not-found` (de token, `token.not-found`); `delete` com `expectedId` que não confere é `409` `cluster.conflict`. Nada é aplicado em nenhum nó. | 06c, 15 §14.5 | C | <!--t-->`tests/definitions.rs::the_owner_refuses_a_change_that_does_not_fit_the_current_state`<br>`tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict`<br>`tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers` |
| UC-DEF-030 | Antes de responder, o dono grava a mudança num **herdeiro**: o primeiro outro dono da posição que responde. Se há outros donos e nenhum responde, a mudança é recusada com `503` `cluster.quorum-unavailable` e não é aplicada em nó algum. | 02 §3.3 | C | <!--t-->`tests/definitions.rs::a_change_with_no_other_owner_reachable_fails_with_503` |
| UC-DEF-031 | Um nó que não responde não impede a mudança: o dono responde depois dos nós que respondem dentro de `write.commit.timeout` e **insiste** nos demais, com a versão mais nova, até eles confirmarem ou saírem do cluster. O nó que volta passa a ter a definição (ou a revogação). | 02 §3.3 e 3.4 | C | <!--t-->`tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`<br>`tests/definitions.rs::a_topic_created_while_a_node_is_unreachable_reaches_it_when_it_returns` |
| UC-DEF-032 | Dono fora do ar e ainda na tabela (até a exclusão dele): a mudança falha com `503` `cluster.quorum-unavailable` e `Retry-After`, sem aplicar nada, e o cliente repete a chamada. Se o dono está em falha, o código é `node.failed`. A leitura segue local. | 04 §6.3, 05 §7.2 (D107) | C | <!--t-->`tests/definitions.rs::a_change_whose_owner_is_unreachable_fails_with_503` |
| UC-DEF-033 | `POST /cluster/definitions/propose` (`DefinitionProposal`: tipo, operação, chave, corpo e `expectedId`) e `POST /cluster/definitions/apply` (`DefinitionApply`: tipo, chave, versão, `deleted` e corpo) respondem com o `DefinitionApply` decidido. Os streams `GET /cluster/topics` e `/cluster/tokens` levam a `version` de cada linha. O corpo do token leva só o `salt` e o `secretHash`, nunca o segredo. | 06c | U, C | <!--t-->`src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`<br>`src/cluster/protocol/tests.rs::the_routes_are_the_ones_of_the_document`<br>`tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner` |
| UC-DEF-034 | Tokens seguem a mesma regra, com o `id` do token como chave: criação, alteração e revogação passam pelo dono. A revogação vale na hora nos nós que respondem e chega ao nó que estava fora do ar quando ele volta; `token.not-found` na revogação de um token desconhecido. | 06c, 02 §3.4 | C | <!--t-->`tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`<br>`tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them` |
| UC-DEF-035 | Duas cópias de uma definição com a **mesma versão e conteúdo diferente** (a nova tentativa que reaproveita a versão, ou dois nós que se achavam donos) terminam iguais em todo nó, qualquer que seja a ordem de chegada: vale a de **menor impressão** (SHA-256 do corpo normalizado; a remoção tem uma só). O empate é avisado no registro e contado em `taba.cluster.definitions.conflicts`; repetir a cópia igual não é empate. Vale no `apply`, na carga do nó novo e na reconciliação. | 02 §3.3, 06c (D124) | C | <!--t-->`tests/definitions.rs::two_copies_with_the_same_version_end_the_same_in_any_arrival_order` |

#### Persistência

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-DEF-009 | A definição do tópico é sempre persistida na tabela `topics` do `${storage.dir}/_node.db` (uma linha por tópico, sem arquivo por tópico), gravada no commit em cada nó, e atualizada ou removida com a definição. | 02 §3.3 | I, C | <!--t-->`src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`<br>`src/storage/topics/tests.rs::saving_again_replaces_the_row`<br>`tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart` |
| UC-DEF-010 | O arquivo guarda **só a definição**: dados de consumo (profundidade, taxas, histogramas) nunca vão para ele. | 02 §3.3 | I | <!--t-->— |
| UC-DEF-011 | A dead-letter não tem linha de definição na tabela `topics`: é derivada da fila de origem, criada e excluída na mesma operação. | 02 §3.3 | I | <!--t-->— |
| UC-DEF-012 | Cada token é uma linha da tabela `tokens` do `_node.db` (com `id`, `name`, permissões, prefixos, datas, o sal e o hash salgado do segredo, nunca o segredo). | 02 §3.4 | I | <!--t-->— |
| UC-DEF-013 | Na partida, o nó (em desenvolvimento, retomando do disco) carrega as definições da tabela `topics` do `_node.db`, preservando o `id` e a data de criação de cada uma. | 04 §6.10 | I | <!--t-->— |

#### Remoção com mensagens pendentes

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-DEF-014 | O `DELETE` sempre funciona, mesmo com mensagens não entregues ou não confirmadas. No commit, cada nó apaga todas as mensagens do tópico que tem localmente (memória, `${topic}/` em disco e metadados), sem checar se estão pendentes. | 02 §3.3 | C | <!--t-->— |

#### Bootstrap das tabelas (nó novo)

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-DEF-015 | O nó novo puxa a tabela de tópicos por `GET /cluster/topics` (stream NDJSON ordenado por `id`) de um nó de **menor carga** pelo sinal de vida; empatados ou sem dado de carga, um nó **sorteado**. | 02 §3.3 | C | <!--t-->`tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins` |
| UC-DEF-016 | A tabela de tópicos **não** vai na resposta do ingresso (só a tabela de nós). | 04 §6.2, 06c | C | <!--t-->`tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins` |
| UC-DEF-017 | A carga é aplicada direto no armazenamento final; tudo o que chega por registro e commit durante a carga (criação, alteração, remoção) vai para o banco de eventos, na ordem de chegada, e o nó confirma o registro **depois de gravar**. | 02 §3.3 | C | <!--t-->— |
| UC-DEF-018 | Depois da carga, o banco de eventos é processado por cima, linha a linha, apagando as processadas: o evento vence a carga (uma remoção durante a carga apaga o que a carga trouxe). Uma mensagem de um tópico ainda desconhecido espera no mesmo banco. | 02 §3.3 | C | <!--t-->— |
| UC-DEF-019 | Verificação final: ao terminar, o nó repete a listagem e descarta o que não aparece mais na origem (cobre a remoção cujo commit não chegou a ele). | 02 §3.3 | C | <!--t-->— |
| UC-DEF-020 | Se o nó de origem falhar no meio, a carga continua em outro nó, a partir do último `id` lido. | 02 §3.3 | C | <!--t-->— |
| UC-DEF-021 | Até a sincronização inteira terminar, o nó não atende rotas de cliente (`/health/ready` falha): `GET /topics`, o casamento de prefixos e o cálculo do nó gestor dependem da tabela completa. | 02 §3.3 | C | <!--t-->— |
| UC-DEF-022 | Os tokens são puxados por `GET /cluster/tokens` junto com a tabela de tópicos, da mesma origem e com as mesmas regras (carga direta, eventos no banco, verificação final). | 02 §3.4 | C | <!--t-->`tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them` |
| UC-DEF-023 | Em modo desenvolvimento a definição é aplicada só no próprio nó, sem esperar outros, sem bootstrap nem carga. | 02 §3.3 | I | <!--t-->— |
| UC-DEF-024 | A definição de cada tópico é gravada na tabela `topics` do `_node.db` numa transação (nunca uma linha pela metade), e nomes que só diferem em maiúsculas (`Pedidos` e `pedidos`) são tópicos diferentes, em qualquer disco. | 02 §3.3 | U, I | <!--t-->`src/engine/messages/tests.rs::topics_that_differ_only_in_case_keep_separate_folders`<br>`src/storage/messages/tests.rs::topic_folders_are_safe_for_case_insensitive_disks`<br>`src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`<br>`tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart` |

### ENT: entrega no dono, lista de prontos, `take` e long polling

Regras: [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.2 e 12.6 a 12.8. O que o cliente enxerga está em `CNS`.

#### Dono da entrega

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ENT-001 | Cada mensagem tem um único dono da entrega: o primário do bloco. Só ele a coloca na sua lista de prontos; réplica não entra na lista. | 13 §12.7 | U, C | <!--t-->`src/engine/messages/tests/cluster.rs::only_the_owner_puts_a_message_in_the_delivery_lists`<br>`tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it` |
| UC-ENT-002 | Com três nós A, B e C, cada um é primário de parte das mensagens: a lista de prontos de cada um tem só as suas, e a união das listas é o tópico completo. | 13 §12.7 e 12.8 | C | <!--t-->`tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own` |
| UC-ENT-003 | Nenhuma mensagem é entregue em duplicidade, mesmo com tabelas de membros divergentes por instantes: o registro de invisibilidade é exclusivo (rejeitado se já há outro pendente ou se a mensagem já está invisível) e o `GET` tenta outro ID. | 13 §12.7, 14 §13.3 | C | <!--t-->— |
| UC-ENT-004 | Quando o primário de um bloco muda (entrada, saída, rebalanceamento), a thread refaz o filtro dos blocos afetados. Um bloco em sincronização não entrega. | 13 §12.7 | C | <!--t-->`src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata` |
| UC-ENT-005 | Um nó que entrou e ainda não está íntegro **não** entrega as mensagens de que é primário; o primário antigo as tira da lista de prontos no commit do registro; ninguém as entrega até a sincronização terminar. As chamadas de cliente que chegam a esse nó são repassadas, e ele não envia avisos nem sinais de prontas. | 13 §12.7 | C | <!--t-->— |
| UC-ENT-006 | Dono fora do ar e ainda não desregistrado: as mensagens dele ficam sem entrega até o desregistro; os demais nós do bloco são cópias de segurança e **não** assumem a entrega. | 13 §12.7 | C | <!--t-->— |
| UC-ENT-007 | Em modo desenvolvimento o nó é o dono de todas as mensagens. | 13 §12.7 | I | <!--t-->— |

#### Lista de prontos

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ENT-008 | A lista de prontos é mantida por tópico em cada nó, em memória, com `delivery.ready-list.size` IDs (padrão 1000). | 13 §12.6 | U | <!--t-->— |
| UC-ENT-009 | Uma thread repõe a lista quando o semáforo abre espaço (entrega commitada, ID que sai), repondo a quantidade que abriu, sem consultar à toa. | 13 §12.6 | U | <!--t-->— |
| UC-ENT-010 | A reposição segue a sanfona: dois cursores sobre as mensagens do nó (da mais antiga para a frente e da mais nova para trás, por GUID v7, em memória e em disco), alternando as duas pontas. | 13 §12.6 | U | <!--t-->— |
| UC-ENT-011 | A entrega consome as duas pontas de forma **intercalada**: a mais antiga, a mais nova, a segunda mais antiga, a segunda mais nova. | 13 §12.6 | U | <!--t-->— |
| UC-ENT-012 | Uma mensagem nova que chega ao dono entra direto na lista se há lugar e o cursor da ponta nova está no fim; senão é encontrada depois pela thread. | 13 §12.6 | U | <!--t-->— |
| UC-ENT-013 | Ao receber o commit de uma entrega, o nó remove o ID da lista. O primário está sempre no quórum, então a mensagem deixa de ser candidata a nova entrega no próprio nó que a entregou no mesmo instante em que o cliente a recebe. | 13 §12.2 | C | <!--t-->`src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts` |
| UC-ENT-014 | A mensagem cuja invisibilidade vence volta à lista pela posição do GUID v7, **não** pela varredura; se a lista está cheia, o ID do **meio** sai para dar lugar e é reposto depois (nenhuma mensagem se perde). | 13 §12.6, 14 §13.6 | U | <!--t-->— |
| UC-ENT-015 | O `GET` só consulta a lista, sem varrer memória e disco a cada chamada. | 13 §12.6 | U | <!--t-->— |

#### `take` (completar com outros nós)

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ENT-016 | Cenário 1: lista local com mensagem: o nó entrega direto, sem consultar nenhum outro nó para achar a mensagem. | 13 §12.8 | C | <!--t-->— |
| UC-ENT-017 | Cenário 2: lista local vazia e outro nó tem: o nó consulta a tabela em memória, escolhe um nó que tem mensagens prontas, chama `POST /cluster/topics/{topic}/messages/take`; o dono faz registro + commit e devolve a mensagem, que o nó repassa ao cliente. | 13 §12.8 | C | <!--t-->`tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own` |
| UC-ENT-018 | O `take` leva a **quantidade que ainda falta** para completar o `maxMessages` do cliente e o `visibilityTimeout` pedido. | 13 §12.2 e 12.8, 06c | C | <!--t-->— |
| UC-ENT-019 | O peer é sorteado com **peso** proporcional às mensagens prontas informadas no sinal de vida; sem informação de quantidade, o sorteio é uniforme. | 13 §12.8 | U | <!--t-->— |
| UC-ENT-020 | O `take` responde lista vazia quando o estado "tem mensagens prontas" era uma dica desatualizada: o nó tenta o próximo, até `delivery.take.max-attempts`; esgotado, segue para o long polling. O custo é limitado, nunca um pedido a todos os nós. | 13 §12.8 | C | <!--t-->— |
| UC-ENT-021 | Quem entrega no `take` é sempre o dono da mensagem: a mensagem tem um único coordenador, o relógio que fixa o `invisibleUntil` é o do dono. | 13 §12.8, 14 §13.3 | C | <!--t-->— |
| UC-ENT-022 | `take` num nó em drain responde lista vazia; num nó em sincronização, `503` `cluster.node-syncing`. | 06c | C | <!--t-->`tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking` |
| UC-ENT-023 | Cenário 3: nenhum nó tem mensagem: o `GET` com `waitTime > 0` vira ouvinte local, e o nó informa aos demais, uma única vez, "o tópico tem ouvintes aqui". | 13 §12.8 | C | <!--t-->`tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another` |

#### Long polling entre nós

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ENT-024 | Mensagem nova pronta no **próprio nó** vai direto ao primeiro ouvinte local, sem tráfego entre nós. | 13 §12.8 | C | <!--t-->`tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another` |
| UC-ENT-025 | Mensagem nova pronta em **outro nó** com ouvinte conhecido: o dono envia o aviso, o nó do ouvinte faz o `take` só com aquele nó e entrega ao ouvinte. Se a mensagem já foi levada por outro, o ouvinte volta a esperar. | 13 §12.8 | C | <!--t-->`tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another` |
| UC-ENT-026 | N ouvintes esperando o mesmo tópico em um nó geram **uma** informação de interesse; ao receber um aviso, o nó busca de uma vez tantas mensagens quantos são os ouvintes. | 13 §12.8 | C | <!--t-->— |
| UC-ENT-027 | Um aviso perdido só custa espera até o fim do `waitTime` (no máximo 20 s). | 13 §12.8 | C | <!--t-->— |
| UC-ENT-028 | Esgotado o `waitTime` a chamada responde vazio e o ouvinte é removido. Se o cliente desconecta antes, o ouvinte também é removido. | 13 §12.8 | C | <!--t-->— |
| UC-ENT-029 | A mensagem cuja invisibilidade vence, com a lista de prontos vazia, envia o sinal "tem mensagens prontas"; havendo ouvinte local, a mensagem vai direto a ele. | 14 §13.6 | C | <!--t-->— |

#### Efeitos aceitos e métricas

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ENT-030 | Trade-off aceito: o primário cai entre o commit local da invisibilidade e a replicação às réplicas fora do quórum; o novo primário herda o `invisibleUntil` de quem sobreviveu e a mensagem pode ser entregue antes do prazo prometido, sem quebrar o at-least-once. | 13 §12.2 | C | <!--t-->— |
| UC-ENT-031 | Métricas da entrega: `taba.delivery.ready`, `taba.delivery.redeliveries`, `taba.delivery.longpoll.waiting`, `taba.delivery.longpoll.outcome` (`delivered`, `empty`, `disconnected`) e `taba.delivery.take` (`in` e `out`; `delivered`, `empty`, `error`). | 11 | C | <!--t-->`tests/metrics.rs::long_polling_is_counted_while_waiting_and_by_outcome` |

### CAN: canal permanente entre nós

Regras: [04-cluster.md](04-cluster.md), seção 6.1, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.8 e 12.9, e [06c-api-cluster.md](06c-api-cluster.md), `GET /cluster/channel`.

#### O canal e os quadros

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-CAN-001 | Há um canal WebSocket por par de nós (malha completa), compartilhado por todos os tópicos: N nós têm N−1 canais cada (`taba.cluster.channel.connections`). | 04 §6.1, 13 §12.8 | C | <!--t-->`tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it` |
| UC-CAN-002 | O canal carrega só sinais pequenos, nunca o conteúdo das mensagens nem listas longas. | 04 §6.1, 13 §12.8 | C | <!--t-->`src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content` |
| UC-CAN-003 | `GET /cluster/channel` faz o upgrade para WebSocket (`101`) só para quem consta na tabela de membros; quem não consta recebe `403` `cluster.not-member` **antes** do upgrade. | 06c | C | <!--t-->`tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered` |
| UC-CAN-004 | Os quadros são JSON com o campo `type`: `keepalive`, `listeners`, `ready`, `notify`, `node-state` e `confirm`, cada um com os campos definidos. | 06c | U, C | <!--t-->`src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content` |
| UC-CAN-005 | O `keepalive` é uma mensagem própria (não ping/pong do WebSocket), enviada a cada `cluster.channel.keepalive.interval`, com a data e hora do nó, a carga, o resumo de monitoramento e as mensagens prontas por tópico (só tópicos com prontas, até `cluster.channel.keepalive.max-topics`). | 13 §12.8, 06c | C | <!--t-->`tests/channel.rs::the_keepalive_carries_the_time_the_state_the_version_and_the_busiest_topics_up_to_the_limit` |
| UC-CAN-006 | O `keepalive` **não tem `seq`**, não é confirmado nem reenviado: o seguinte o substitui. | 13 §12.8 e 12.9 | U, C | <!--t-->`src/cluster/protocol/tests.rs::channel_messages_are_json_text_and_the_confirm_is_told_by_its_type`<br>`tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored` |
| UC-CAN-007 | Os sinais de estado são enviados **só nas mudanças** (ouvintes 0→1 e →0; prontas vazio→não vazio e o contrário), não a cada chamada. | 13 §12.8 | C | <!--t-->`src/engine/messages/tests/cluster.rs::the_ready_signal_fires_only_when_the_state_changes` |
| UC-CAN-008 | Ao abrir o canal, cada lado envia primeiro um instantâneo (um `listeners` e um `ready` para cada tópico em que o estado é verdadeiro) e só depois as mudanças. Um nó em sincronização só o consome quando fica pronto. | 13 §12.9, 06c | C | <!--t-->`tests/channel.rs::a_channel_opens_with_a_snapshot_of_the_state_the_listeners_and_the_ready_topics` |

#### Confirmação e reenvio

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-CAN-009 | Todo quadro, exceto `keepalive` e `confirm`, leva `seq` crescente por canal e por sentido. O receptor devolve o `confirm` (possivelmente agrupando vários `seqs`) **depois de processar**. | 13 §12.9, 06c | U, C | <!--t-->`src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`<br>`tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored` |
| UC-CAN-010 | O emissor só tira o sinal da fila de saída ao receber o `confirm`; se o canal cai, reenvia na reconexão o que ficou sem confirmação, e o que foi confirmado nunca se repete. | 13 §12.9 | C | <!--t-->`src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`<br>`tests/channel.rs::a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns` |
| UC-CAN-011 | Um `seq` já processado e reenviado (confirmação perdida) é ignorado pelo receptor (basta guardar o maior `seq` processado). | 13 §12.9 | U, C | <!--t-->`tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored` |

#### Quem abre a conexão

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-CAN-012 | O nó que entra ou volta se conecta ativamente a cada nó da tabela que recebeu como resposta; os demais só aceitam. Um nó só aceita conexão de quem consta na sua tabela. | 13 §12.9, 04 §6.2 | C | <!--t-->`tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered` |
| UC-CAN-013 | Queda com os dois nós vivos: o nó que detectar primeiro reconecta. Se os dois abrem uma conexão ao mesmo tempo, o nó de **menor hash do ID** fecha a que abriu e fica com a aberta pelo outro: sempre um canal por par. | 13 §12.9, 06c | U, C | <!--t-->`src/cluster/channel/tests.rs::both_nodes_keep_the_same_connection_in_any_order_of_arrival`<br>`src/cluster/channel/tests.rs::of_two_connections_the_one_opened_by_the_node_with_the_larger_hash_stays`<br>`src/cluster/channel/tests.rs::the_same_side_opening_again_replaces_the_dead_connection`<br>`tests/channel.rs::two_connections_at_once_end_with_a_single_channel_per_pair` |
| UC-CAN-014 | Reinício do processo: o nó perde a memória (inclusive a tabela de membros, que não é persistida) e volta como nó novo, pelo registro; é ele quem abre os canais. | 13 §12.9, 04 §6.2 | C | <!--t-->`tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`<br>`tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node` |

#### Avisos de mensagem nova

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-CAN-015 | Em cada nó dono há um responsável por tópico (ativo só enquanto há ouvintes em algum nó e mensagens novas a avisar). As mensagens que ficam prontas são acumuladas por `cluster.notify.window` (padrão 100 ms) e viram **um** aviso, com a quantidade. | 13 §12.8 | U, C | <!--t-->`tests/notify.rs::the_ready_messages_of_a_window_become_one_notice_with_the_quantity` |
| UC-CAN-016 | O aviso vai a **um** nó com ouvinte por vez, revezando entre os que têm; o responsável espera `cluster.notify.wait` que o nó venha buscar (`take`). Se não vem, passa ao próximo nó com ouvinte e repete o ciclo, até acabarem as mensagens avisadas ou os nós com ouvinte. | 13 §12.8 | C | <!--t-->`tests/notify.rs::the_notice_goes_to_one_listener_at_a_time_and_passes_on_when_nobody_comes` |
| UC-CAN-017 | O aviso é uma dica: quem o recebe vai buscar e pode não encontrar nada (`taba.cluster.channel.notifications` com `outcome` `fetched` ou `empty`). | 13 §12.8 | C | <!--t-->`tests/notify.rs::a_notice_is_a_hint_that_ends_fetched_or_empty` |

#### Filas de saída e de entrada

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-CAN-018 | Há uma fila de saída por nó de destino (uma thread de escrita a esvazia sem bloquear quem produz) e uma fila de entrada por nó de origem (processamento separado da leitura do socket). | 13 §12.9 | U, C | <!--t-->`tests/channel.rs::each_peer_has_an_out_queue_and_an_in_queue_that_drain` |
| UC-CAN-019 | Com o canal fora, sinais de **estado** do mesmo tópico se substituem na fila (só o último importa); avisos antigos são descartados (`taba.cluster.channel.queue.dropped` com `coalesced` e `expired`). | 13 §12.9 | U | <!--t-->`src/cluster/channel/queue/tests.rs::old_notices_are_dropped_and_old_states_are_kept`<br>`src/cluster/channel/queue/tests.rs::state_signals_of_the_same_topic_replace_each_other_and_notices_do_not` |
| UC-CAN-020 | A fila de saída estoura `cluster.channel.queue.max-size` ou `cluster.channel.queue.max-age`: o nó com quem o canal está fora é tratado como **falho** e a votação de exclusão é aberta, sem esperar o fim das reconexões (`dropped` com `overflow`). | 13 §12.9, 04 §6.3 | C | <!--t-->`src/cluster/channel/queue/tests.rs::the_queue_overflows_by_size_or_by_age`<br>`tests/channel.rs::a_queue_that_waits_too_long_opens_the_vote_without_waiting_for_the_retries` |
| UC-CAN-021 | Reconexão **antes** de a votação concluir: nada muda, os nós retomam o envio dos sinais pendentes de onde pararam. | 13 §12.9, 04 §6.3 | C | <!--t-->`tests/channel.rs::a_channel_that_returns_before_the_retries_end_changes_nothing` |
| UC-CAN-022 | Votação concluída: todos os nós descartam as filas (de saída e de entrada) para o nó excluído e param de acumular. Se o nó voltar, refaz o bootstrap com filas novas e vazias. | 13 §12.9 | C | <!--t-->`tests/failures.rs::once_the_exclusion_is_decided_the_nodes_drop_the_channel_and_the_return_starts_with_empty_queues` |
| UC-CAN-023 | O sinal `node-state` com `state: draining`: os pares deixam de pedir `take` a esse nó e de escolhê-lo como origem de carga. | 06c, 04 §6.10 | C | <!--t-->`tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking` |
| UC-CAN-024 | Métricas do canal: `taba.cluster.channel.state` por peer, `rtt`, `reconnects`, `queue.out` e `queue.in` (a fila de entrada por nó de origem). | 11 | C | <!--t-->`tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it` |
| UC-CAN-025 | A chamada do canal leva o `nodeId` do destino (`to`): o processo novo que atende o mesmo IP e porta (outro ID, por exemplo um reinício rápido) responde `421` `cluster.wrong-node` antes de qualquer outra conferência, e o emissor conta o canal como caído. A votação abre, o nó antigo sai da tabela e o novo entra (D100). | 06c, 04 §6.3 | C | <!--t-->`tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced` |
| UC-CAN-026 | A chamada do canal **sem** `to` (vazio) não confere o destino: quem chama sem saber o ID do nó que procura (a rota de teste, uma versão que não o envia) é tratado como antes, sem `421` (D100). | 06c, 04 §6.3 | C | <!--t-->`tests/channel.rs::a_channel_call_without_the_destination_id_is_not_checked_against_it` |

### INV: invisibilidade no dono e nas réplicas

Regras: [14-invisibilidade.md](14-invisibilidade.md). O que o cliente enxerga: `CNS`, `ACK` e `VIS`.

#### Metadados

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-INV-001 | Os metadados são `deliveryCount`, `invisibleUntil` (data absoluta) e `receiptToken`, guardados e replicados juntos: no mesmo registro + commit, em memória ou no banco local do tópico. | 14 §13.2 | U, C | <!--t-->— |
| UC-INV-002 | `invisibleUntil` é calculado pelo **dono** (`agora + visibilityTimeout`, pelo relógio dele); as réplicas guardam o valor pronto e nunca recalculam a partir de duração. | 14 §13.2 e 13.3 | C | <!--t-->— |
| UC-INV-003 | O `receiptToken` é gerado pelo dono a cada entrega; o nó compara com o dos metadados no ack e no `PUT`, e responde `409` se for diferente, sem alterar nada. A alteração mantém o mesmo token. | 14 §13.2 | U, C | <!--t-->`src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message` |
| UC-INV-004 | Mensagem em memória: metadados em memória. Mensagem em disco: linha na tabela `messages` do banco do tópico (`id`, posição, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`), atualizada no commit e removida com a mensagem no ack. Alterar a invisibilidade de uma mensagem em disco atualiza só a linha, sem tocar no corpo. | 14 §13.2 | I | <!--t-->`src/engine/messages/index/tests.rs::an_inserted_message_with_a_future_deadline_starts_in_processing` |
| UC-INV-005 | Os estados da mensagem para o dono: **pronta** (`invisibleUntil` ausente ou no passado), **invisível** (no futuro) e **removida** (ack). | 14 §13.1 | U | <!--t-->— |

#### Propagação (entrega, alteração, ack)

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-INV-006 | Entrega: propaga novo `invisibleUntil` e novo `receiptToken`, `deliveryCount` +1, remove o ID da lista de prontos e insere ID e prazo na lista de invisíveis do dono. | 14 §13.3 | C | <!--t-->`src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts`<br>`tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own` |
| UC-INV-007 | Alteração: propaga o novo `invisibleUntil`, `deliveryCount` não muda, lista de prontos não muda (com `0`, o dono insere o ID, como num vencimento) e a lista de invisíveis substitui o prazo (com `0`, remove). | 14 §13.3 | C | <!--t-->`tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation` |
| UC-INV-008 | Ack: remove a mensagem e os metadados e remove a entrada da lista de invisíveis. | 14 §13.3 | C | <!--t-->`src/engine/messages/tests/cluster.rs::the_ack_removes_the_copy_and_only_the_owner_counts_it`<br>`tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused` |
| UC-INV-009 | O **dono** é sempre o coordenador: no `GET` local ele mesmo; num `take`, o nó que atende o cliente pede ao dono, que coordena. | 14 §13.3 | C | <!--t-->— |
| UC-INV-010 | O registro é exclusivo: o nó rejeita se já há outro pendente para a mensagem (`409` `cluster.conflict`) ou, numa entrega, se ela já está invisível. Na alteração e no ack, também se o `receiptToken` não confere (`409` `message.stale-receipt`). | 14 §13.3, 06c | C | <!--t-->`src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message`<br>`tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused` |
| UC-INV-011 | O commit é decidido por quórum (primário incluso); só depois a chamada responde ao cliente. As réplicas fora do quórum recebem o commit em segundo plano, dentro de `write.commit.timeout`. | 14 §13.3 | C | <!--t-->— |
| UC-INV-012 | Falha antes do quórum: retry e, esgotado, rollback do pendente; as listas do dono **não** são tocadas e a mensagem segue como estava. Falha nas réplicas fora do quórum: sem rollback, o dono insiste até o teto. | 14 §13.3 | C | <!--t-->`tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`<br>`tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays` |
| UC-INV-013 | `POST .../visibility/register` com `kind` `delivery` (levando `deliveryCount`, token novo) ou `change` (o `receiptToken` do cliente deve conferir com o atual); `commit` e `rollback` com `OperationRef`; erros `404` `message.not-found`, `409` `cluster.conflict` e `409` `message.stale-receipt`. | 06c | C | <!--t-->`src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back` |
| UC-INV-014 | O `commit` da entrega remove o ID da lista de prontos do nó, e o primário **sempre recebe** essa chamada antes da resposta ao cliente. | 06c | C | <!--t-->`tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own` |
| UC-INV-015 | A diferença de relógio entre nós, na troca de dono, encurta ou alonga o prazo pelo tamanho da diferença: efeito aceito no at-least-once. | 14 §13.3 | C | <!--t-->— |

#### Vencimento e volta à lista de prontos

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-INV-016 | O vencimento **não se propaga**: nada é escrito nos metadados nem enviado às réplicas; `deliveryCount` só sobe na próxima entrega. | 14 §13.3 e 13.6 | C | <!--t-->`tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation` |
| UC-INV-017 | A thread do dono olha a cabeça da lista de invisíveis, dorme até o prazo dela (com um teto curto para reagir a alterações e entradas mais cedo) e, no vencimento, remove a entrada e **confere** nos metadados: a mensagem ainda existe, o nó ainda é dono e o prazo ainda é o da entrada; senão descarta e segue. | 14 §13.6 | U | <!--t-->— |
| UC-INV-018 | Passada a conferência, a mensagem entra na lista de prontos pela posição do GUID v7 (perto da ponta antiga); com a lista cheia, o ID do meio sai. | 14 §13.6 | U | <!--t-->— |
| UC-INV-019 | Passada a conferência com `deliveryCount ≥ maxDeliveries`, a mensagem **não** volta à lista: vai para a dead-letter. Mensagem vencida por `ttl` também é descartada, sem voltar. | 14 §13.6, 13 §12.10 | U, C | <!--t-->`src/engine/messages/tests/cluster.rs::the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure`<br>`tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node` |
| UC-INV-020 | Na fila FIFO, a mensagem que volta é a cabeça do grupo; o grupo deixa de estar ocupado e entra na lista de prontos pela posição do ID, sem a regra do meio da lista. | 14 §13.6 | U | <!--t-->— |
| UC-INV-021 | Se a lista de invisíveis esvazia e ainda há linhas na tabela de expiração, a thread faz a recarga antes de dormir. | 14 §13.6 | U | <!--t-->— |

#### Lista de invisíveis e tabela de expiração

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-INV-022 | A lista de invisíveis é, por tópico e só no dono, uma lista de pares (ID, `invisibleUntil`) ordenada por prazo, com no máximo `delivery.invisible-list.size` entradas; busca por ID para alteração e ack. | 14 §13.4 | U | <!--t-->— |
| UC-INV-023 | Regra de inclusão com a lista cheia: se o prazo novo é **anterior** ao último da lista, o par novo entra e o último vai para a tabela; senão o par novo vai direto para a tabela. | 14 §13.5 | U | <!--t-->— |
| UC-INV-024 | Invariante: tudo na lista vence antes de tudo na tabela; a thread nunca consulta a tabela enquanto a lista não esvazia. | 14 §13.5 | U | <!--t-->— |
| UC-INV-025 | Recarga: abaixo de `delivery.invisible.reload-threshold` (ou vazia), a thread lê da tabela os pares de menor `invisibleUntil` (consulta ordenada com limite pelo índice), move-os para a lista e apaga as linhas lidas. | 14 §13.5 | U, I | <!--t-->— |
| UC-INV-026 | Sem entradas velhas: ack, alteração e nova entrega apagam ou atualizam a linha da tabela no mesmo commit que atualiza a lista. Não há remoção preguiçosa. | 14 §13.5 | U, I | <!--t-->— |
| UC-INV-027 | A gravação na tabela faz parte do commit no dono: se falhar, o commit não se completa e a entrega é desfeita antes da resposta ao cliente. | 14 §13.5 | I | <!--t-->— |
| UC-INV-028 | Com a lista com espaço, nenhuma escrita além dos metadados. | 14 §13.5 | U | <!--t-->— |

#### Mudança de dono e reinício

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-INV-029 | Novo dono de um bloco: ao terminar a sincronização, percorre os metadados das mensagens de que agora é primário: as com `invisibleUntil` no futuro entram na lista de invisíveis (ou na tabela de expiração, pela regra de inclusão) e as demais viram candidatas da lista de prontos. | 14 §13.7 | C | <!--t-->`src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata` |
| UC-INV-030 | Quem deixa de ser dono não limpa nada na hora: as entradas que restarem falham na conferência de dono e são descartadas quando lidas. | 14 §13.7 | C | <!--t-->`src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata` |
| UC-INV-031 | Reinício: a tabela de expiração é descartada junto com o estado antigo; as listas são reconstruídas quando o nó assumir blocos. | 14 §13.7 | C | <!--t-->— |
| UC-INV-032 | Em modo desenvolvimento o registro e o commit acontecem só no próprio nó (quórum 1), sem propagação; só vale o caso do reinício. | 14 §13.3 e 13.7 | I | <!--t-->— |

### ARM: armazenamento, memória e disco

Regras: [02-arquitetura.md](02-arquitetura.md), seção 3.2.

#### Memória e overflow

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ARM-001 | A mensagem é gravada primeiro em memória; ao atingir `memory.limit`, as excedentes são persistidas em disco. | 02 §3.2 | U, I | <!--t-->`src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`<br>`tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`<br>`tests/messages.rs::without_a_memory_limit_nothing_goes_to_disk_by_overflow` |
| UC-ARM-002 | O corpo vai para `${topic}/${id}.body` (uma pasta por tópico, um arquivo por mensagem), com os **bytes crus**, sem reserializar nem codificar; o `contentType` fica no metadado. | 02 §3.2 | I | <!--t-->`src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`<br>`src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`<br>`tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact` |
| UC-ARM-003 | Ao ler uma mensagem que não está em memória, ela é buscada no disco pelo mesmo caminho. | 02 §3.2 | I | <!--t-->`src/engine/messages/tests.rs::delivery_merges_memory_and_disk_in_id_order_with_the_exact_bytes`<br>`tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact` |
| UC-ARM-004 | A memória não é dividida de forma fixa entre os tópicos: tópicos de maior taxa de leitura ficam com mais mensagens em memória; os de menor taxa vão para disco primeiro. | 02 §3.2 | U | <!--t-->`src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk`<br>`src/engine/messages/tests.rs::a_topic_that_is_read_more_is_never_demoted_by_one_that_is_read_less`<br>`src/engine/messages/tests.rs::above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less` |
| UC-ARM-005 | A taxa de leitura é a média móvel das mensagens consumidas por segundo na janela `memory.rate.window` (5 minutos), a mesma de `publishRate` e `consumeRate`. | 02 §3.2 | U | <!--t-->`src/engine/messages/rates/tests.rs::publish_and_consume_are_counted_apart`<br>`src/engine/messages/rates/tests.rs::several_events_in_one_second_share_a_bucket`<br>`src/engine/messages/rates/tests.rs::the_rate_is_the_moving_average_over_the_window`<br>`src/engine/messages/rates/tests.rs::what_left_the_window_stops_counting`<br>`src/engine/messages/tests.rs::above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less`<br>`src/engine/messages/tests.rs::publish_and_consume_rates_are_tracked_per_topic`<br>`src/storage/messages/tests.rs::the_rows_of_a_block_come_by_position_in_id_order_with_range_and_limit` |
| UC-ARM-006 | Cada tópico tem uma reserva em memória (`memory.topic.reserve`, 1000 mensagens), **independente da taxa**: um tópico novo, sem histórico de leitura, não começa jogando as primeiras mensagens em disco. A divisão pela taxa vale só acima das reservas. | 02 §3.2 | U | <!--t-->`src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk` |
| UC-ARM-007 | O que permanece em memória são as mensagens da lista de prontos e as recém-chegadas; o resto é candidato a disco, começando pelo tópico de menor taxa. Não se usa LRU/LFU. | 02 §3.2 | U | <!--t-->`src/engine/messages/tests.rs::demotion_takes_the_middle_of_the_ready_list_and_keeps_the_ends`<br>`src/engine/messages/tests.rs::messages_in_processing_are_demoted_before_the_ready_ones` |
| UC-ARM-008 | A lista de prontos e as mensagens em memória não mudam por causa do banco: ele só cuida do que está em disco. | 02 §3.2 | U | <!--t-->— |
| UC-ARM-009 | `taba.storage.*`: memória usada e limite, mensagens em memória e em disco, disco usado e livre, overflow, leituras de disco e de memória, duração e erros de disco. | 11 | I | <!--t-->`tests/metrics.rs::free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure`<br>`tests/metrics.rs::storage_metrics_count_overflow_and_reads_by_tier` |

#### Banco SQLite local

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ARM-010 | Cada nó mantém um banco por tópico, `${topic}/_topic.db`, estritamente local: nunca é copiado, enviado nem sincronizado com outros nós. | 02 §3.2 | I | <!--t-->`src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`<br>`tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact` |
| UC-ARM-011 | A tabela `messages` tem uma linha por mensagem em disco (`id`, posição no anel, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`; na FIFO, `deduplicationId` e `deleteAt`) com índices por `id`, `invisibleUntil`, posição, `(group_id, id)`, `(group_id, deduplicationId)` e `deleteAt`. | 02 §3.2, 17 §16.7 | I | <!--t-->`src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`<br>`src/storage/messages/tests.rs::a_topic_database_from_before_the_fifo_columns_is_upgraded_in_place` |
| UC-ARM-012 | A tabela `expiry` guarda os pares que não couberam na lista de invisíveis, com índice por prazo. | 02 §3.2, 14 §13.5 | I | <!--t-->— |
| UC-ARM-013 | Toda **escrita** no banco de um tópico passa por um **escritor único** (uma fila de comandos por tópico, executada uma de cada vez); leituras longas usam conexões próprias em modo WAL e não bloqueiam o escritor. | 02 §3.2, 18 §17.4 | U, I | <!--t-->`src/engine/messages/tests.rs::a_slow_topic_never_blocks_the_other_topics` |
| UC-ARM-014 | Ordem de gravação: na escrita, primeiro o arquivo do corpo, depois a linha; no ack, primeiro a linha, depois o arquivo. Uma queda deixa no máximo arquivo sem linha ou linha sem arquivo. | 02 §3.2 | I | <!--t-->`src/engine/messages/tests.rs::a_message_on_disk_is_delivered_again_and_acked_like_any_other`<br>`src/storage/messages/tests.rs::removing_deletes_the_row_and_then_the_file`<br>`tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact` |
| UC-ARM-015 | O nó que reinicia depois de uma queda volta como novo e refaz a partir do cluster. | 02 §3.2, 04 §6.6 | C | <!--t-->`tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready` |
| UC-ARM-016 | O banco responde: a mensagem mais antiga do tópico (`MIN(id)`, somada à mais antiga em memória, mantida ordenada por ID), os próximos a vencer (índice por `invisibleUntil`), as mensagens de um bloco (faixa pela posição no anel) e as contagens do tópico. | 02 §3.2 | I | <!--t-->— |
| UC-ARM-017 | `storage.db.synchronous` (padrão `NORMAL` com WAL) é aplicado ao abrir o banco. | 02 §3.2, 10 | I | <!--t-->`src/storage/db/tests.rs::opens_in_wal_mode_with_the_configured_synchronous`<br>`tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart` |
| UC-ARM-018 | O banco do nó, `${storage.dir}/_node.db`, é estritamente local, com as tabelas `topics` (as definições de tópico) e `losses` (`topic`, `id`, `block`, `detected_at`), escrita por uma thread única. | 02 §3.2 | I | <!--t-->— |

#### Integridade e falha

| Código | Caso de uso | Fonte | Nível | Testes |
|---|---|---|---|---|
| UC-ARM-019 | Banco corrompido: o nó entra em **falha**, sinaliza ao cluster (que fica liberado para votar o desregistro dele) e não tenta reparar. `/health/ready` passa a `503` com `reason: failed`. | 02 §3.2, 05 §7.3 | C | <!--t-->— |
| UC-ARM-020 | Disco cheio ou falha de I/O: o nó entra em falha, como no banco corrompido; o erro conta em `taba.storage.disk.errors` e gera log `error`. | 05 §7.3, 11 | C | <!--t-->— |
| UC-ARM-021 | O nó cria `storage.dir` (padrão `/var/lib/taba`, a última pasta se chama `taba`) ao iniciar; se não conseguir criar ou gravar, não sobe. | 10 | I | <!--t-->— |
| UC-ARM-022 | Um nó que perde um bloco (mudança do anel) **não apaga nada na hora**: continua guardando as mensagens do bloco e pode servir de origem de sincronização. Só apaga (memória, `.body` e linhas do banco, por faixa da posição no anel) quando **todo o cluster está íntegro**. | 05 §7.3 | C | <!--t-->`src/engine/messages/tests/blocks.rs::copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay`<br>`tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready` |
| UC-ARM-023 | Se o nó novo não chega a ficar saudável, o anel volta ao anterior e o nó antigo volta a ser o responsável, com as mensagens intactas, sem nada a refazer. Se o anel mudar de novo antes da limpeza, o nó recalcula quais blocos ainda não são dele. | 05 §7.3 | C | <!--t-->`tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready` |
| UC-ARM-024 | Mensagem recebida como réplica ou publicada em modo drain vai direto para disco (limite de memória efetivo zero). | 02 §3.2, 04 §6.10 | I | <!--t-->`src/engine/messages/tests.rs::drain_saves_everything_in_memory_with_its_metadata_and_a_restart_resumes_it` |
| UC-ARM-025 | Dados de **consumo** (profundidade, visíveis, invisíveis, taxas, latências) ficam só em memória e específicos de cada nó. | 02 §3.3 | I | <!--t-->— |
