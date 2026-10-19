---
título: Problemas conhecidos
resumo: Os testes de cluster desligados com #[ignore] por falharem de forma intermitente nos runners do CI, com o sintoma, a causa conhecida e a correção prevista de cada um; serão corrigidos na próxima versão.
tipo: referência
status: parcial
verificado-em: 2026-10-19 @ 879f956
fontes: [crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/common/cluster.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md).

Na 0.1.0, três testes de cluster estão marcados com `#[ignore]`: passam na máquina de desenvolvimento, mas falham de forma intermitente nos runners do GitHub Actions, que são lentos e compartilhados. Todos serão corrigidos na próxima versão. Para rodá-los à mão: `cargo test -p taba-node --test <arquivo> <nome> -- --ignored`.

## Canal saudável que não volta (`NO_VOTE`)

| Teste | Sintoma no CI |
|---|---|
| `failures.rs` › `the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary` | o consumo pelo nó 1 para no meio (ex.: 16 de 28 mensagens), ou "waited 30 s for: the node to join the cluster" |
| `replication.rs` › `with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up` | publicações de nós saudáveis falham enquanto um nó está fora |

- **Causa (confirmada).** O `NO_VOTE` de `tests/common/cluster.rs` impede a votação de exclusão subindo o backoff de reconexão de **todos** os canais para 600 s, não só o do nó derrubado. Num runner lento, um canal **saudável** cai ao ficar 500 ms sem keepalive e só voltaria 10 minutos depois; o `take` e as signals entre dois nós saudáveis param. Uma queda simulada de 100 ms reproduz a falha localmente.
- **Correção prevista.** Desligar a votação sem tocar no backoff (`TABA_CLUSTER_CHANNEL_RECONNECT_RETRIES` muito alto e `TABA_CLUSTER_CHANNEL_QUEUE_MAX_AGE` alto). Ela resolve as duas falhas, mas expõe um travamento raro (≈ 1 em 25 execuções) no `cluster.shutdown()` de dois testes de `replication.rs` depois que o nó isolado volta: o processo para por completo, sugerindo um deadlock síncrono. Precisa ser entendido antes de a correção entrar.
- **Risco no produto, achado na análise.** Inversão de ordem de travas: `build_batch` (`src/cluster/channel/mod.rs`) segura a trava do peer e chama as signals; `ready_peers` (`src/cluster/replication/signals.rs`) segura a trava das tabelas e chama `is_live`, que pega a do peer. No runtime de várias threads de produção isso pode travar. Correção: coletar os candidatos sob a trava das tabelas, soltá-la e só então filtrar com `is_live`.

## Reingresso depois de uma partição simétrica

| Teste | Sintoma no CI |
|---|---|
| `failures.rs` › `after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again` | "waited 30 s for: the four nodes in a single table" |

- **Quando.** Falhou nas duas execuções do CI do PR #3 no runner amd64 (2026-10-19); passou no arm64 e no push da `release` com o mesmo código. Só o `release.yml` mudava no PR, então a falha não vem dele.
- **Causa.** Não investigada. O teste parte o cluster em 2 + 2, deixa cada lado excluir o outro, cura a rede e avança o relógio 301 s; o lado perdedor (o do menor `nodeId`) deveria refazer o ingresso e as quatro tabelas voltarem a ser uma em até 30 s.
- **Próximo passo.** Ver se é o mesmo tipo de fragilidade dos testes acima (canais que caem por falta de keepalive num runner lento) ou um problema real no reingresso depois de partição.

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`).
- As falhas foram vistas nos logs do CI do repositório; a causa do `NO_VOTE` foi reproduzida localmente com uma queda simulada de 100 ms no canal entre os nós 1 e 2.
