---
título: Problemas conhecidos
resumo: Registro, com histórico, dos testes de cluster intermitentes e dos problemas conhecidos: cada um com título, status, data em que foi achado, data da solução e o histórico de ocorrências e tentativas.
tipo: referência
status: parcial
verificado-em: 2026-10-26 @ 75012bc
fontes: [crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/tests/cold_start.rs, crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md).

Cada teste intermitente ou problema conhecido tem uma seção própria, **que nunca é apagada**: quando o problema é resolvido ela passa a `Corrigido`; se voltar a aparecer, passa a `Reaberto` e o histórico continua na mesma seção. Os testes marcados com `#[ignore]` passam na máquina de desenvolvimento, mas falharam de forma intermitente nos runners do GitHub Actions, que são lentos e compartilhados. Para rodar um à mão: `cargo test -p taba-node --test <arquivo> <nome> -- --ignored`.

Status possíveis: `Aberto`, `Corrigido`, `Reaberto`. As regras para tratar testes intermitentes estão no [AGENTS.md](../../AGENTS.md).

| Problema | Status | Achado em | Solução em |
|---|---|---|---|
| [A rodada de caos: uma entrega além do limite e um ack que o dono nunca aplicou](#a-rodada-de-caos-uma-entrega-além-do-limite-e-um-ack-que-o-dono-nunca-aplicou) | Corrigido | 2026-10-27 | 2026-10-27 |
| [Um nó de teste reiniciado que perde a porta para outro teste](#um-nó-de-teste-reiniciado-que-perde-a-porta-para-outro-teste) | Aberto | 2026-10-26 | — |
| [Uma definição oferecida a um nó que ainda estava entrando](#uma-definição-oferecida-a-um-nó-que-ainda-estava-entrando) | Corrigido | 2026-10-26 | 2026-10-26 |
| [Um accept de canal que travou os peers duas vezes](#um-accept-de-canal-que-travou-os-peers-duas-vezes) | Corrigido | 2026-10-26 | 2026-10-26 |
| [Teste de reinício da FIFO que espera 30 s numa execução combinada](#teste-de-reinício-da-fifo-que-espera-30-s-numa-execução-combinada) | Aberto | 2026-10-22 | — |
| [Teste de aviso que espera 30 s uma vez na suíte completa](#teste-de-aviso-que-espera-30-s-uma-vez-na-suíte-completa) | Aberto | 2026-10-20 | — |
| [Reingresso depois de uma partição simétrica](#reingresso-depois-de-uma-partição-simétrica) | Corrigido | 2026-10-19 | 2026-10-20 |
| [Canal saudável que não volta](#canal-saudável-que-não-volta) | Corrigido | 2026-10-18 | 2026-10-19 |
| [Inversão de ordem de travas ao escolher réplicas](#inversão-de-ordem-de-travas-ao-escolher-réplicas) | Corrigido | 2026-10-18 | 2026-10-18 |

## A rodada de caos: uma entrega além do limite e um ack que o dono nunca aplicou

**Status:** Corrigido · **Achado em:** 2026-10-27 · **Corrigido em:** 2026-10-27

- **Onde:** `src/engine/messages/mod.rs` (`take`, `take_candidates`), `src/cluster/replication/coordinator.rs` (`commit_until_done`) e `src/cluster/protocol/mod.rs` (`Request::decided`). Achado pelo `tests/chaos.rs` ([37-garantias.md](37-garantias.md)).
- **Sintoma 1 (I5):** com `maxDeliveries: 2` uma mensagem foi entregue com `deliveryCount: 3`, na maioria das rodadas da semente 3. Causa: um commit de entrega que chegou ao dono depois de a própria invisibilidade acabar pôs a mensagem na lista de prontas sem a liberação que confere o limite. Correção: o `take_candidates` não entrega a mensagem que gastou as entregas: esconde-a por um instante e a próxima liberação a manda para a dead-letter (a primeira forma da correção, uma ida imediata, quebrou o `a_shorter_ttl_expires_what_is_already_stored_even_in_flight`, que espera que o `ttl` vença, e foi trocada). Teste: `a_commit_that_arrives_after_its_invisibility_does_not_deliver_past_the_limit` (falha sem a correção).
- **Sintoma 2 (I1):** em cerca de 1 rodada em 100 uma mensagem confirmada nunca foi entregue: ficava só no dono (as réplicas tinham aplicado o ack dela), a entrega dela precisa das réplicas, e então o grupo FIFO ficava travado para sempre. Causa: o commit do ack ao dono chegou depois de o register do dono expirar; o coordenador o registrou de novo e desistiu na primeira recusa, ou porque o recibo tinha expirado ou porque outra operação segurava a mensagem (`cluster.conflict`). Correção: o register enviado de novo depois da decisão não confere recibo (`Request::decided`) e, num conflito, continua tentando até a outra operação acabar. Testes: `the_register_repeated_after_the_decision_checks_no_receipt` e `an_ack_the_replicas_applied_reaches_the_owner_even_if_another_operation_took_the_message` (falha sem a correção).
- **Como foi achado:** a primeira suspeita foi o próprio verificador: a trilha de uma mensagem (`History::trail`) mostrou que o `maxDeliveries: 2` de uma fila tinha sido aplicado às outras (um falso positivo, corrigido no verificador). Depois o relatório de falha ganhou onde a mensagem está em cada nó, e mostrou o dono com uma mensagem que as réplicas já não tinham. A linha de base sem a correção da entrega também falhou (1 rodada em 120), então a segunda causa era independente.
- **Não corrigido:** um ack e uma ida para a dead-letter, os dois decididos, quando um register expira ([PRB-077](31-pendencias.md)); exige uma parada além do `write.pending.ttl` e está fora da rodada (os saltos de relógio dela são de 8 s).
- **Não fazer:** subir os 30 s do `eventually` ou deixar a rodada parar na primeira entrega: o grupo travado para sempre é exatamente o que a espera existe para mostrar.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-27 | A rodada (com o I5 acrescentado) falhou na semente 3 com `delivered 3 times, over maxDeliveries 2`; a primeira causa foi o verificador, que aplicava o limite às filas que não o têm. |
| 2026-10-27 | Com o verificador corrigido, 1 rodada em 5 ainda falhava (uma entrega real além do limite); o teste de cluster do commit tardio a reproduziu e a checagem no `take` a corrigiu. |
| 2026-10-27 | Uma segunda falha, I1 (mensagem nunca entregue), 1 rodada em 100; a mesma taxa sem a primeira correção, então independente. Um grupo travado por uma mensagem só no dono. |
| 2026-10-27 | Correção do register enviado de novo (sem conferir recibo): ainda 5 falhas em 300 rodadas das quatro sementes. O teste de cluster com um commit atrasado e outra operação sobre a mensagem reproduziu o resto (conflito no novo register); o coordenador agora continua tentando. |
| 2026-10-27 | Com as duas correções e saltos de relógio de 35 s: nenhum I1 em 400 rodadas, 3 falhas de outro formato (I3 e I5, [PRB-077](31-pendencias.md)). Com saltos de 8 s (abaixo do `write.pending.ttl`): **0 falhas em 400 rodadas das quatro sementes (1600 execuções)**; antes das correções, cerca de 1 em 100 falhava. |
| 2026-10-27 | A checagem no `take` fez o `a_shorter_ttl_expires_what_is_already_stored_even_in_flight` falhar (o `ttl` precisa vencer a dead-letter); a forma final esconde a mensagem e deixa a ida para a liberação. Código final: a suíte inteira passa (47 binários) e **0 falhas em 250 rodadas das quatro sementes (1000 execuções)**. |

## Um nó de teste reiniciado que perde a porta para outro teste

**Status:** Aberto · **Achado em:** 2026-10-26 · **Solução em:** —

- **Onde:** `tests/common/cluster.rs` (`respawn_node`) e os testes que trazem um nó de volta no endereço do que caiu (`tests/cold_start.rs`: `fall`, `come_back`).
- **Sintoma:** `the process restarts: Bind { name: "do cluster", addr: 127.0.0.1:<porta>, source: … AddrInUse … }` depois dos 30 s do `respawn_node`. Visto 3 vezes em cerca de 1800 execuções de processo do `docker/hang-hunt.sh cold_start …` (10 processos ao mesmo tempo), em `when_several_nodes_connect_the_highest_version…` e em `a_node_that_joins_offers_the_definitions_of_its_disk…`; ainda não visto no CI.
- **Causa (conferida no log da execução que falhou):** o `fall()` desliga o nó e libera a porta de cluster dele; o teste inicia o nó de novo na mesma porta mais tarde. No intervalo, o sistema entrega a porta liberada (efêmera) a outro listener: no log, a porta era a de cluster do nó caído e, dois segundos depois, a porta de **cliente** de um nó de **outro teste do mesmo processo**. O `respawn_node` tenta por 30 s, mas esse nó guarda a porta até o fim do próprio teste, então a nova tentativa não consegue ganhar. Os testes de um binário rodam em threads paralelas, então o mesmo pode acontecer no CI.
- **Correção (aplicada em 2026-10-28, verificação pendente):** `PortHold` em `tests/common/cluster.rs`: um listener de reserva aberto logo depois do desligamento (em `fall` de `cold_start.rs` e `regroup.rs`) e solto um instante antes do novo bind (`come_back`). Falta conferir com `docker/hang-hunt.sh cold_start regroup`; o estado passa a `Corrigido` depois disso.
- **Não fazer:** aumentar os 30 s do `respawn_node`; a porta não volta enquanto o outro teste vive.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-26 | Visto uma vez na caçada que validou a correção da oferta perdida (rodada 29, 10 processos ao mesmo tempo). O log mostra a porta reusada por outro nó do mesmo processo. Não corrigido: não tem relação com aquela mudança. |
| 2026-10-26 | Mais duas na validação de 1500 execuções (rodadas 19 e 141), uma delas em `a_node_that_joins_offers…`: o mesmo `AddrInUse`. |
| 2026-10-28 | **Primeira vez no CI**: a execução do `Release` no push para `main` (merge do PR #34, amd64) falhou em `with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (`cold_start.rs`, `AddrInUse` na porta do cluster). `PortHold` incluído; `cold_start` (13) e `regroup` (3) passam localmente. |

## Uma definição oferecida a um nó que ainda estava entrando

**Status:** Corrigido · **Achado em:** 2026-10-26 · **Solução em:** 2026-10-26

- **Onde:** `src/cluster/replication/definitions.rs` (`Definitions::offer`), testado em `tests/cold_start.rs`.
- **Sintoma:** no CI, `cold_start` › `when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator` falhou com `waited 30 s for: the highest version in every node` (na execução do PR de 2026-10-26), e `a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins` falhou com `waited 30 s for: the offered queue in the cluster` (arm64, o push na `release`, no mesmo dia). Os dois só sob carga. Localmente: 2 falhas em cerca de 290 execuções dos dois testes, com 10 processos ao mesmo tempo.
- **Causa:** um nó que entra oferece o que o disco tinha (`offer`). O vencedor era aplicado no nó e enviado só como um quadro do canal (`relay`), e um quadro só alcança os peers que esse nó conhece naquele instante. Na execução capturada, o terceiro nó ficou ativo e ofereceu `only-3` em 46.965; o segundo nó foi registrado na tabela em 46.972, sete milissegundos depois, e ainda não era peer, então o quadro não foi para ele. A sincronização dele tinha listado as definições antes de a origem aplicar `only-3` (em 46.986), e o quadro do canal nunca foi enviado a ele: ficou ativo com `shared` mas sem `only-3`, para sempre. Uma mudança decidida por um dono não tem a brecha: o `coordinate` também envia um `definition-apply` a todo membro da tabela (`spread`), e um nó que sincroniza grava esse pedido com os eventos e o aplica no fim. O `offer` era o único caminho que entregava uma definição nova só pelo canal.
- **Como foi achado:** o `docker/hang-hunt.sh cold_start …` reproduziu no container; o log não tinha nenhuma linha sobre a oferta, então linhas temporárias (`HUNT`, já removidas) foram postas no `offer`, `learn`, `relay` e `broadcast` e a execução que falha foi capturada de novo. Comparar, nó a nó, o que foi oferecido, a quem foi enviado e o que cada nó aprendeu mostrou o `broadcast` com um peer, sete milissegundos antes de o segundo nó aparecer.
- **Solução:** depois de aplicar o que vence, o `offer` solta o lock da chave e envia um `definition-apply` a todo membro da tabela (`spread`, todas as definições ao mesmo tempo), além do `relay`. O lock é solto antes: com ele preso, dois nós oferecendo a mesma definição esperariam um pelo outro até o `write.commit.timeout`.
- **Teste:** `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows` (UC-RTN-058) oferece uma definição a partir do nó 2 e exige um `definition-apply` para os nós 1 e 3; sem a correção ele falha.
- **Ponto em aberto:** a falha do arm64 do `a_node_that_joins_offers…` não foi reproduzida (esse teste só olha o nó 1, que é sempre peer). Pode ser a mesma corrida em outra ordem, ou não. Se voltar, reabra esta seção com o log do `docker/hang-hunt.sh`.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-26 | Primeira falha no CI do PR (`when_several_nodes_connect…`); passou localmente em 3 + 40 + 9 execuções. |
| 2026-10-26 | Segunda falha, no push na `release` em arm64 (`a_node_that_joins_offers…`), outro teste do mesmo binário e da mesma área. |
| 2026-10-26 | `docker/hang-hunt.sh cold_start` (10 processos ao mesmo tempo): falha na rodada 13 e na rodada 16 de outra execução, as duas em `when_several_nodes_connect…`. Com o log temporário, a causa acima. |
| 2026-10-26 | Correção aplicada e teste adicionado (falha sem a correção). A mesma caçada com a correção, 1500 execuções de processo: veja a linha seguinte. |
| 2026-10-26 | Verificação: `KEEP_GOING=1 docker/hang-hunt.sh cold_start …`, 150 rodadas de 10 processos ao mesmo tempo (1500 execuções dos testes da oferta): **nenhum** `waited 30 s` por uma definição, contra 2 em cerca de 290 execuções antes da correção. Os únicos 2 problemas foram `AddrInUse` ao reiniciar um nó (veja "Um nó de teste reiniciado que perde a porta para outro teste"), sem relação. |

## Um accept de canal que travou os peers duas vezes

**Status:** Corrigido · **Achado em:** 2026-10-26 · **Solução em:** 2026-10-26

- **Onde:** `src/cluster/channel/mod.rs` (`Channels::accept`).
- **Sintoma:** na execução de CI do servidor em amd64, o último teste do binário `nodes_api` ficou em "has been running for over 60 seconds", sem saída e sem mensagem de falha. Foi `a_node_that_does_not_answer_makes_the_consolidated_reads_partial_and_says_which` numa execução e `a_prune_that_fails_on_a_node_names_it_and_repeating_it_is_safe` em outra: os dois isolam o nó 3 e o trazem de volta. Não reproduz sozinho: 3 travamentos em 120 execuções do binário, todos sob carga (vários processos ao mesmo tempo).
- **Causa:** o `accept` procurava o peer com `match self.lock().get(from).cloned() { … None => { self.sync_members(); … } }`. O guard de `lock()` é um temporário no escrutínio, e um temporário no escrutínio de um `match` vive até o fim do `match` inteiro. `sync_members` (e `drop_all`, que ele chama) travam o mesmo `std::sync::Mutex` de novo, que não é reentrante: a thread bloqueia em si mesma para sempre. Só acontece quando um peer abre um canal que o nó ainda não conhece (corrida depois que a rede volta ou enquanto um nó faz drain). No runtime de produção, uma thread de trabalho se perde e a trava dos peers fica presa para sempre, então toda outra tarefa que mexe nos canais trava: o nó continua vivo e deixa de responder ao cluster. Nos testes (runtime `current_thread`) nada mais anda, **nem os timeouts**, e por isso o log ficou mudo.
- **Como foi achado:** com `TABA_TEST_LOG=taba_node=trace`, o log da execução travada terminava num `drain started` que nunca acabou, e nem o `drain.timeout` (25 s, que registra quando estoura) apareceu: o runtime inteiro estava parado, não esperando. A pilha de todas as threads, tirada com `gdb` num container Linux (receita em [25-testes.md](25-testes.md#investigando-um-teste-que-trava-ou-falha-só-às-vezes)), mostrou `accept → sync_members → drop_all → Channels::lock` bloqueado.
- **Solução:** a consulta virou uma instrução própria (`let known = self.lock().get(from).cloned();`) e o guard some antes de `sync_members` rodar; o `receive` já fazia assim. O mesmo formato foi tirado de três `if let` que seguravam um guard no escrutínio (`allow_new_vote`, `Backup::usage`, `Sync::abandon`) e o `clippy::significant_drop_in_scrutinee` agora está ligado no workspace (D179).
- **Teste:** `tests/channel.rs::a_channel_from_a_node_the_peers_do_not_know_yet_is_refused_without_blocking_the_node` (UC-CAN-029) chama o `accept` de um peer desconhecido numa thread própria e falha em 10 s se ele não voltar. Sem a correção ele falha.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-26 | Visto no CI como um teste "running for over 60 seconds" (a segunda ocorrência, noutro teste do mesmo binário). Não reproduziu sozinho nem em 40 execuções em sequência; reproduziu com 6 processos concorrentes (1 travamento em 48 execuções). |
| 2026-10-26 | Com o log ligado: 1 travamento em 16 execuções dos dois testes; a última linha era um drain que nunca terminou. Num container Linux com `gdb`: 1 travamento em 56 execuções, e a pilha deu a causa acima. |
| 2026-10-26 | Correção aplicada, teste adicionado (falha sem a correção). `cargo clippy --all-targets -- -D warnings` limpo com o novo lint. Os dois testes, repetidos no mesmo container com 8 processos ao mesmo tempo: veja a linha seguinte. |
| 2026-10-26 | Verificação: 100 rodadas de 8 processos ao mesmo tempo (800 execuções dos dois testes) no container que travava antes, nenhuma travou; antes da correção travava uma vez a cada 16 a 56 execuções. O binário `channel` passou (16 testes). |

## Teste de reinício da FIFO que espera 30 s numa execução combinada

**Status:** Aberto · **Achado em:** 2026-10-22 · **Solução em:** —

- **Teste:** `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate` e, com o mesmo sintoma, `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (os dois reiniciam um nó sobre o disco da vida anterior).
- **Sintoma:** `waited 30 s for: the folders of the previous lives to be restored` (o `TestNode` espera a restauração do disco que assume antes de devolver). Visto em 2 de 4 execuções combinadas (os binários `fifo`, `messages`, `volatile`, `operations`, `topics` e `auth` um depois do outro, e a suíte completa); sozinho passou 8 vezes seguidas, e 12 execuções do binário `fifo` inteiro e mais 5 combinadas com os logs ligados não falharam.
- **Causa:** não identificada. Duas causas reais do mesmo sintoma foram achadas e corrigidas no caminho (uma varredura que lia o disco antigo no estado de um tópico antes de ele ser posto de lado, e a mudança de pasta do tópico no Windows disputando com o varredor); esta não reproduziu depois.
- **Próximo passo:** na próxima ocorrência guardar o log `INFO` da segunda partida (`tracing_subscriber` no teste) para ver se a mudança da pasta falhou (`could not set aside the previous life's disk`) ou se a restauração não terminou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-22 | Achado ao mover os testes de integração para o perfil de um nó: falhou em duas execuções combinadas, passou sozinho e com os logs ligados nas seguintes. |
| 2026-10-22 | Falhou em duas execuções completas seguidas (`cargo test --no-fail-fast`, `waited 30 s`, em `common/cluster.rs`); o binário `fifo` sozinho passou 4 vezes e uma terceira execução completa não falhou. A causa continua sem identificação. |
| 2026-10-22 | Três execuções completas (`cargo test -p taba-node --no-fail-fast`) falharam com o mesmo `waited 30 s` e uma, sem o teste novo de `messages.rs`, passou. Falhou em `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (duas vezes) e em `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`; o binário `messages` sozinho passou 3 vezes seguidas e o teste sozinho, 3. A causa segue sem identificação (não é um teste só). |
| 2026-10-23 | O `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` falhou com o mesmo `waited 30 s for: the folders of the previous lives to be restored` em 2 de 6 execuções completas (`cargo test -p taba-node --no-fail-fast`, o binário `messages` levando 34 s em vez de 2 s), enquanto um cluster Docker com os dados de teste rodava na mesma máquina; o binário `messages` sozinho passou 5 vezes seguidas. Apareceu enquanto as mudanças de OpenAPI e de portas eram testadas, mas o sintoma, o teste e a espera são os das entradas de 2026-10-22, e nada da restauração do disco foi tocado. A causa continua não identificada. |

## Teste de aviso que espera 30 s uma vez na suíte completa

**Status:** Aberto · **Achado em:** 2026-10-20 · **Solução em:** —

- **Teste:** `notify.rs` › `a_notice_is_a_hint_that_ends_fetched_or_empty`.
- **Sintoma:** pânico `waited 30 s` do `eventually_async` (`tests/common/cluster.rs`) em uma execução de `cargo test -p taba-node`, logo depois da compilação. Sozinho passou 3 vezes seguidas, com e sem a mudança em teste, e uma segunda execução completa ficou verde.
- **Causa:** não identificada (a saída da execução que falhou não foi guardada).
- **Próximo passo:** na próxima ocorrência, guardar a saída inteira para ver qual condição estourou; tentar o teste sob carga paralela (muitos binários de teste rodando ao mesmo tempo) antes de mexer em qualquer tempo limite.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-20 | Falhou uma vez numa execução completa local; passou sozinho (3 execuções) e na execução completa seguinte. |
| 2026-10-22 | Falhou de novo numa execução completa local (`waited 30 s for: the old notice counted as empty`); passou 6 de 6 sozinho logo depois. A mudança em teste não toca os avisos. Causa ainda não identificada. |

## Reingresso depois de uma partição simétrica

**Status:** Corrigido · **Achado em:** 2026-10-19 · **Solução em:** 2026-10-20

- **Teste:** `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`.
- **Sintoma no CI:** "waited 30 s for: the four nodes in a single table".
- **Causa (confirmada):** quando as duas metades se reencontraram, os dois nós do lado perdedor refizeram o ingresso ao mesmo tempo. Um deles (o nó 3 na captura) foi aceito por intermédio do outro enquanto este ainda era reiniciado, recebeu uma tabela só com os dois e começou a sincronizar tendo-o como fonte; essa fonte então entrou no lado vencedor e deixou de ter o nó na tabela. As fontes respondiam `cluster.not-member` e o nó, em `syncing`, continuava pedindo por 60 s (`cluster.sync.timeout`) antes de entrar em `failed`: o `handle_exclusion` só age sobre um nó `active`, então um nó sincronizando nunca descobria que fora excluído. Em produção a mesma sequência terminaria com um nó em `failed` depois de um minuto.
- **Correção:** enquanto a sincronização espera uma fonte (`NoSource`), ela consulta os membros da tabela recebida; se algum tiver uma tabela mais nova sem o nó, a sincronização termina com `SyncError::Excluded` e o `finish_join` esquece a tabela, zera o estado local e o bootstrap refaz o ingresso (`ClusterService::excluded_while_syncing`).
- **Teste da correção:** `failures.rs` › `a_syncing_node_that_the_sources_excluded_redoes_the_join_instead_of_failing` (falha sem a correção, `waited 30 s for: the node to redo the join`).

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-19 | Falhou nas duas execuções do CI do PR #3 no runner amd64; passou no arm64 e no push da `release` com o mesmo código (o PR só mudava o `release.yml`). Teste marcado com `#[ignore]`. |
| 2026-10-19 | Investigação: não é o `NO_VOTE` (o teste não o usa) nem a inversão de travas. Não reproduziu localmente: passou em 12 e em 30 execuções simultâneas do mesmo binário. |
| 2026-10-20 | Reproduzido localmente com 40 a 60 execuções simultâneas do binário (1 falha em cada um de dois lotes). Um despejo temporário do estado e das tabelas de cada nó no estouro mostrou o nó preso em `syncing` com uma tabela antiga; a causa acima. |
| 2026-10-20 | Correção aplicada e `#[ignore]` removido. Verificação: 180 execuções simultâneas (3 lotes de 60) do teste, todas passaram; o teste novo falha sem a correção; a suíte inteira do `taba-node` passou (uma falha isolada e não relacionada, registrada acima). |

## Canal saudável que não volta

**Status:** Corrigido · **Achado em:** 2026-10-18 · **Solução em:** 2026-10-19

- **Testes:** `failures.rs` › `the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary` e `replication.rs` › `with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`.
- **Sintoma no CI:** o consumo pelo nó 1 para no meio (ex.: 16 de 28 mensagens), "waited 30 s for: the node to join the cluster", ou publicações de nós saudáveis falhando enquanto um nó está fora.
- **Causa (confirmada):** o `NO_VOTE` de `tests/common/cluster.rs` impedia a votação de exclusão subindo o backoff de reconexão de **todos** os canais para 600 s. Num runner lento, um canal saudável caía ao ficar 500 ms sem keepalive e só voltaria 10 minutos depois; o `take` e as signals entre dois nós saudáveis paravam. Uma queda simulada de 100 ms reproduzia a falha localmente.
- **Solução:** o `NO_VOTE` desliga a votação com `TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES=100000` e `TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE=600s`, sem mexer no backoff.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-18 | Falhas vistas no CI; os dois testes marcados com `#[ignore]`. Causa reproduzida localmente com uma queda simulada de 100 ms entre os nós 1 e 2. |
| 2026-10-19 | Correção aplicada em `NO_VOTE` e `#[ignore]` removido dos dois testes. Verificação: os seis arquivos de teste que usam `NO_VOTE` passaram 6 vezes seguidas com seis processos em paralelo (36 execuções). O travamento raro no `cluster.shutdown()` (≈ 1 em 25), visto numa tentativa anterior, não apareceu. |

## Inversão de ordem de travas ao escolher réplicas

**Status:** Corrigido · **Achado em:** 2026-10-18 · **Solução em:** 2026-10-18

- **Onde:** `src/cluster/channel/mod.rs` (`build_batch`) e `src/cluster/replication/signals.rs` (`ready_peers`).
- **Risco:** `build_batch` segura a trava do peer e chama as signals; `ready_peers` segurava a trava das tabelas e chamava `is_live`, que pega a do peer. No runtime de várias threads de produção isso podia travar.
- **Solução:** coletar os candidatos sob a trava das tabelas, soltá-la e só então filtrar com `is_live` (commit `a9ecf01`).

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-18 | Achado na análise do `NO_VOTE`. Corrigido no mesmo dia (`a9ecf01`). |

## Lições aprendidas

O que os problemas acima ensinaram, para o próximo ser achado mais rápido (a lista de passos para um agente está no [AGENTS.md](../../AGENTS.md)):

- **Um teste que fica mudo não é um teste lento.** "Has been running for over 60 seconds" **sem nenhum log depois de certo ponto**, nem de um timeout que deveria registrar, quer dizer que o runtime está parado, não esperando. Procure uma trava tomada duas vezes ou uma inversão de ordem de travas, não um timeout para aumentar.
- **Um `std::sync::Mutex` não é reentrante, e um guard vive mais do que parece.** Um temporário no escrutínio de um `match`, `if let`, `while let` ou `for` vive até o fim do bloco inteiro, `else` e braços incluídos (na edição 2024 um `if let` o solta antes do `else`, mas um `match` continua não soltando). Tire o valor numa instrução `let` própria e use-o depois.
- **Só aparece sob carga.** Essas corridas passam sozinhas e em sequência. Reproduza com vários processos do binário de teste ao mesmo tempo e guarde o log só das execuções que travam.
- **No runtime `current_thread` do `#[tokio::test]` uma trava bloqueada congela tudo**, inclusive os temporizadores, então o teste nunca falha sozinho: o código que chama a função suspeita num teste a roda numa thread própria e espera com `recv_timeout`, para o teste falhar em vez de travar.
- **Tire a pilha, não adivinhe.** O `docker/hang-hunt.sh <arquivo-de-teste> [filtro...]` faz a caçada inteira (carga, logs detalhados só ali, pilha de todas as threads de um processo travado). O `TABA_TEST_LOG` reduz o lugar; a pilha de todas as threads (`gdb -p <pid>`, `thread apply all bt`) dá a linha exata. É mais barato do que mais um dia lendo logs.
- **Procure a família inteira.** O mesmo erro estava no `accept`, e o mesmo formato (um guard no escrutínio) estava em mais três lugares; a inversão de ordem de travas de 2026-10-18 é uma prima. Depois de uma correção, rode `cargo clippy -- -W clippy::significant_drop_in_scrutinee` e as verificações listadas no [AGENTS.md](../../AGENTS.md).
- **Um `waited 30 s` por um estado que nunca chega é uma entrega perdida, não uma lenta.** Pergunte quem deveria enviar, a quem, e quem estava na lista de destinatários naquele instante. Um `broadcast` (ou um quadro do canal) alcança só os peers que existem **agora**: um nó que se registra milissegundos depois, e ainda sincroniza, nunca o recebe, a menos que outro caminho (um `definition-apply` com `spread`, o heartbeat, a própria sincronização) o cubra. Uma nova forma de entregar estado ao cluster precisa desse segundo caminho, ou de uma razão para não precisar.
- **Quando o log não tem nenhuma linha do caminho suspeito, ponha uma, rode de novo e tire.** A execução que falhou na oferta não tinha nenhuma linha sobre a oferta. Linhas temporárias em cada passo (o que foi oferecido, a quem foi enviado, o que cada nó aprendeu), marcadas com uma palavra que se possa buscar (`HUNT`), e a mesma caçada, deram a ordem dos eventos em milissegundos; as linhas foram removidas com `git checkout` antes da correção. Guarde o log capturado (`target/`).
- **Uma correção não está pronta sem um teste que falha sem ela** (aqui, um que falha por timeout e não travando) e sem repetir a execução que travava.

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` e `tests/channel.rs` (UC-CAN-029).
- `src/cluster/replication/definitions.rs` (`offer`, `spread`, `relay`, `coordinate`), `src/cluster/service/mod.rs` (`recover_previous_life`, `finish_join`, `activate`) e `tests/cold_start.rs` (UC-RTN-058); a linha do tempo da oferta perdida está no log guardado da caçada de 2026-10-26.
- O travamento de 2026-10-26 foi reproduzido localmente e num container Linux; a pilha, os logs e as contagens de execuções são os da tabela de histórico.
- As falhas foram vistas nos logs do CI do repositório; as datas de "achado" vêm do histórico do git e dos logs.
