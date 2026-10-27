---
título: Problemas conhecidos
resumo: Registro, com histórico, dos testes de cluster intermitentes e dos problemas conhecidos: cada um com título, status, data em que foi achado, data da solução e o histórico de ocorrências e tentativas.
tipo: referência
status: parcial
verificado-em: 2026-10-26 @ 75012bc
fontes: [crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md).

Cada teste intermitente ou problema conhecido tem uma seção própria, **que nunca é apagada**: quando o problema é resolvido ela passa a `Corrigido`; se voltar a aparecer, passa a `Reaberto` e o histórico continua na mesma seção. Os testes marcados com `#[ignore]` passam na máquina de desenvolvimento, mas falharam de forma intermitente nos runners do GitHub Actions, que são lentos e compartilhados. Para rodar um à mão: `cargo test -p taba-node --test <arquivo> <nome> -- --ignored`.

Status possíveis: `Aberto`, `Corrigido`, `Reaberto`. As regras para tratar testes intermitentes estão no [AGENTS.md](../../AGENTS.md).

| Problema | Status | Achado em | Solução em |
|---|---|---|---|
| [Um accept de canal que travou os peers duas vezes](#um-accept-de-canal-que-travou-os-peers-duas-vezes) | Corrigido | 2026-10-26 | 2026-10-26 |
| [Teste de reinício da FIFO que espera 30 s numa execução combinada](#teste-de-reinício-da-fifo-que-espera-30-s-numa-execução-combinada) | Aberto | 2026-10-22 | — |
| [Teste de aviso que espera 30 s uma vez na suíte completa](#teste-de-aviso-que-espera-30-s-uma-vez-na-suíte-completa) | Aberto | 2026-10-20 | — |
| [Reingresso depois de uma partição simétrica](#reingresso-depois-de-uma-partição-simétrica) | Corrigido | 2026-10-19 | 2026-10-20 |
| [Canal saudável que não volta](#canal-saudável-que-não-volta) | Corrigido | 2026-10-18 | 2026-10-19 |
| [Inversão de ordem de travas ao escolher réplicas](#inversão-de-ordem-de-travas-ao-escolher-réplicas) | Corrigido | 2026-10-18 | 2026-10-18 |

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
- **Uma correção não está pronta sem um teste que falha sem ela** (aqui, um que falha por timeout e não travando) e sem repetir a execução que travava.

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` e `tests/channel.rs` (UC-CAN-029).
- O travamento de 2026-10-26 foi reproduzido localmente e num container Linux; a pilha, os logs e as contagens de execuções são os da tabela de histórico.
- As falhas foram vistas nos logs do CI do repositório; as datas de "achado" vêm do histórico do git e dos logs.
