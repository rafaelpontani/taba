---
título: Problemas conhecidos
resumo: Registro, com histórico, dos testes de cluster intermitentes e dos problemas conhecidos: cada um com título, status, data em que foi achado, data da solução e o histórico de ocorrências e tentativas.
tipo: referência
status: parcial
verificado-em: 2026-10-19 @ 294fd03
fontes: [crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/common/cluster.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md).

Cada teste intermitente ou problema conhecido tem uma seção própria, **que nunca é apagada**: quando o problema é resolvido ela passa a `Corrigido`; se voltar a aparecer, passa a `Reaberto` e o histórico continua na mesma seção. Os testes marcados com `#[ignore]` passam na máquina de desenvolvimento, mas falharam de forma intermitente nos runners do GitHub Actions, que são lentos e compartilhados. Para rodar um à mão: `cargo test -p taba-node --test <arquivo> <nome> -- --ignored`.

Status possíveis: `Aberto`, `Corrigido`, `Reaberto`. As regras para tratar testes intermitentes estão no [AGENTS.md](../../AGENTS.md).

| Problema | Status | Achado em | Solução em |
|---|---|---|---|
| [Reingresso depois de uma partição simétrica](#reingresso-depois-de-uma-partição-simétrica) | Aberto | 2026-10-19 | — |
| [Canal saudável que não volta](#canal-saudável-que-não-volta) | Corrigido | 2026-10-18 | 2026-10-19 |
| [Inversão de ordem de travas ao escolher réplicas](#inversão-de-ordem-de-travas-ao-escolher-réplicas) | Corrigido | 2026-10-18 | 2026-10-18 |

## Reingresso depois de uma partição simétrica

**Status:** Aberto · **Achado em:** 2026-10-19 · **Solução em:** —

- **Teste:** `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again` (com `#[ignore]`).
- **Sintoma no CI:** "waited 30 s for: the four nodes in a single table".
- **Causa:** não identificada. O teste parte o cluster em 2 + 2, deixa cada lado excluir o outro, cura a rede e avança o relógio 301 s; o lado perdedor (o do menor `nodeId`) deveria refazer o ingresso e as quatro tabelas voltarem a ser uma em até 30 s.
- **Próximo passo:** capturar o log dos nós no runner amd64 quando falhar (tabelas e canais de cada nó no momento do timeout) para separar fragilidade do teste (corrida entre o `clear_calls` e o avanço do relógio) de um problema real no reingresso.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-19 | Falhou nas duas execuções do CI do PR #3 no runner amd64; passou no arm64 e no push da `release` com o mesmo código (o PR só mudava o `release.yml`). Teste marcado com `#[ignore]`. |
| 2026-10-19 | Investigação: não é o `NO_VOTE` (o teste não o usa) nem a inversão de travas. Não reproduziu localmente: passou em 12 e em 30 execuções simultâneas do mesmo binário. |

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

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`).
- As falhas foram vistas nos logs do CI do repositório; as datas de "achado" vêm do histórico do git e dos logs.
