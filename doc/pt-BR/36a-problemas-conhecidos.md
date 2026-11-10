---
título: "Problemas conhecidos: abertos"
resumo: "Os testes intermitentes e problemas conhecidos ainda abertos (ou reabertos): cada um com status, datas, a causa achada até agora, o próximo passo e o histórico de ocorrências e tentativas. Os corrigidos estão no 36f."
tipo: referência
status: parcial
verificado-em: 2026-11-10 @ d6e1d66
fontes: [crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/recovery/restoration.rs, crates/taba-node/src/cluster/recovery/finish.rs, crates/taba-node/tests/restore_lives.rs, crates/taba-node/tests/regroup.rs, crates/taba-node/src/engine/messages/blocks.rs, connectors/gateway-contract/drivers/go/main.go, connectors/gateway-contract/scenarios/aws/sqs-standard.scn, connectors/gateway-contract/scenarios/taba/limits-sqs.scn, crates/taba-node/tests/partition.rs, crates/taba-node/tests/batch.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/tests/cold_start.rs, crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Problemas conhecidos: abertos

> Leia antes: [25-testes.md](25-testes.md).

Este arquivo guarda as seções dos problemas **abertos** (`Aberto` ou `Reaberto`). O índice deles, onde fica cada arquivo e o padrão de uma seção estão no [36-problemas-conhecidos.md](36-problemas-conhecidos.md); os corrigidos estão no [36f-problemas-conhecidos.md](36f-problemas-conhecidos.md).

## KI-034: Um nó reiniciado na tabela junto com o antigo numa caça sobrecarregada

**Status:** Aberto · **Achado em:** 2026-11-10 · **Corrigido em:** —

- **Teste:** `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node` (UC-CNC-002, UC-CNC-007, UC-FRZ-005, UC-CAN-014).
- **Sintoma:** `the new node's request waited for the end of the vote`: uma tabela teve o nó antigo e o novo juntos por um instante. Uma vez em 80 execuções de `ROUNDS=10 KEEP_GOING=1 docker/hang-hunt.sh failures`, numa rodada em que a suíte completa (`cargo test`) também rodava na mesma máquina (a mesma rodada teve outras falhas e travamentos de sobrecarga). A mesma caça repetida com a máquina livre (80 execuções) não falhou.
- **Causa:** não identificada. A leitura a conferir: desde a correção do KI-030 o teste retém o `unregister` até o `join` do nó novo chegar a um membro; sob aquela sobrecarga a chamada retida pode esgotar o prazo (5 s, três tentativas), a exclusão falhar (`failed`), a votação terminar sem excluir, e o `join` então passar com o nó antigo ainda na tabela. Se for isso, o servidor deixou um nó no mesmo endereço entrar enquanto o morto ainda era membro, depois de uma exclusão que falhou: a pergunta é se ele deve continuar recusando até uma nova votação excluir o antigo.
- **Próximo passo:** no log guardado (`target/hunt-failures-loaded/run_7_1.log`), os eventos do cluster deste teste: se a exclusão falhou antes do `join`; depois decidir a regra acima.
- **Não fazer:** aumentar o prazo da chamada retida.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-10 | Uma falha numa caça sobrecarregada (a suíte completa rodando ao mesmo tempo); nenhuma na repetição limpa de 80 execuções. |

## KI-005: Teste de reinício da FIFO que espera 30 s numa execução combinada

**Status:** Aberto · **Achado em:** 2026-10-22 · **Corrigido em:** —

- **Teste:** `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate` e, com o mesmo sintoma, `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (os dois reiniciam um nó sobre o disco da vida anterior).
- **Sintoma:** `waited 30 s for: the folders of the previous lives to be restored` (o `TestNode` espera a restauração do disco que assume antes de devolver). Visto em 2 de 4 execuções combinadas (os binários `fifo`, `messages`, `volatile`, `operations`, `topics` e `auth` um depois do outro, e a suíte completa); sozinho passou 8 vezes seguidas, e 12 execuções do binário `fifo` inteiro e mais 5 combinadas com os logs ligados não falharam.
- **Causa:** não confirmada. A leitura que encaixa (achada em 2026-11-10, sem uma execução que falhasse): o `Recovery::drop_lives` tentava **uma vez** apagar a pasta de cada vida restaurada. No Windows um arquivo que outro handle segura sem permitir a exclusão não pode ser apagado: o SQLite abre seus arquivos assim, e também pode um antivírus ou o indexador lendo um arquivo novo. Um handle ainda aberto por um instante (uma conexão ao banco da vida sendo fechada, uma varredura) fazia o `remove_dir_all` falhar, a pasta ficava, e a espera do `TestNode` pelas pastas sumirem se esgotava. Bate com o que foi visto (só no Windows, só em execuções combinadas, que gravam e varrem mais arquivos, e nunca sozinho), mas nenhuma execução que falhou foi guardada para mostrar o aviso `could not delete the folder of a restored life`.
- **Correção (2026-11-10, no servidor, ainda não confirmada como a deste problema):** o `drop_lives` tenta de novo (mais 5 vezes, uns 3 s ao todo) enquanto a pasta ainda está lá, e diz quantas tentativas levou. Teste de Windows `tests/restore_lives.rs::the_folder_of_a_restored_life_is_deleted_even_if_a_file_in_it_is_held_for_a_moment` (UC-RTN-049): um arquivo seguro como o SQLite o segura faz a primeira remoção falhar, e a pasta some quando o handle fecha; com uma tentativa só, falha.
- **Próximo passo:** nas próximas execuções combinadas no Windows, procurar `could not delete the folder of a restored life` ou `deleted after a retry` no log; se a falha voltar sem nenhum dos dois, a causa é outra (a restauração não terminou: `the folders of the previous lives stay`). Fica aberto até lá.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-22 | Achado ao mover os testes de integração para o perfil de um nó: falhou em duas execuções combinadas, passou sozinho e com os logs ligados nas seguintes. |
| 2026-10-22 | Falhou em duas execuções completas seguidas (`cargo test --no-fail-fast`, `waited 30 s`, em `common/cluster.rs`); o binário `fifo` sozinho passou 4 vezes e uma terceira execução completa não falhou. A causa continua sem identificação. |
| 2026-10-22 | Três execuções completas (`cargo test -p taba-node --no-fail-fast`) falharam com o mesmo `waited 30 s` e uma, sem o teste novo de `messages.rs`, passou. Falhou em `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` (duas vezes) e em `fifo.rs` › `a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`; o binário `messages` sozinho passou 3 vezes seguidas e o teste sozinho, 3. A causa segue sem identificação (não é um teste só). |
| 2026-10-23 | O `messages.rs` › `stopping_saves_memory_to_disk_and_the_next_start_resumes_it` falhou com o mesmo `waited 30 s for: the folders of the previous lives to be restored` em 2 de 6 execuções completas (`cargo test -p taba-node --no-fail-fast`, o binário `messages` levando 34 s em vez de 2 s), enquanto um cluster Docker com os dados de teste rodava na mesma máquina; o binário `messages` sozinho passou 5 vezes seguidas. Apareceu enquanto as mudanças de OpenAPI e de portas eram testadas, mas o sintoma, o teste e a espera são os das entradas de 2026-10-22, e nada da restauração do disco foi tocado. A causa continua não identificada. |
| 2026-11-10 | Leitura do código: uma tentativa só de apagar a pasta de uma vida restaurada, que um handle sem permitir a exclusão faz falhar no Windows; a remoção agora tenta de novo (teste de Windows). Não confirmada como a causa: nenhuma execução que falhou foi guardada. |

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
- **Um teste que fabrica o estado de um nó compete com o nó real.** O heartbeat de um par é o estado completo dele e sobrescreve o que o teste injetou; injete de novo até ver o efeito, ou espere o estado real. Uma margem de tempo de poucas centenas de milissegundos não se sustenta num runner carregado: a caça com todos os testes do binário ao mesmo tempo acha (um teste sozinho não falhou em 320 execuções).
- **Veja qual alvo falhou antes de ler o servidor.** A contagem do gateway foi lida no código do nó por um tempo; o job que falhou era o dos emuladores.
- **Um erro do cliente se prova no fio.** Uma captura no namespace de rede do servidor diz quem fecha e se a resposta está bem formada; depois, um invólucro da conexão do cliente e o `httptrace` dizem por quê. A captura tem de começar depois do último restart do container, ou ela vê um namespace antigo.
- **Uma janela mantida aberta por um atraso fixo é um palpite sobre quanto o outro lado demora.** Sob carga o outro lado demorou mais (KI-030), ou um quadro ultrapassou uma chamada (KI-031). Retenha a chamada até o evento que o teste espera (`MemoryNetwork::hold_kind`), e a ordem deixa de depender da máquina.
- **Uma correção não está pronta sem um teste que falha sem ela** (aqui, um que falha por timeout e não travando) e sem repetir a execução que travava.
- **Olhe o que aconteceu antes de teorizar sobre o que poderia.** O item pendente culpava um register expirado; o dump da mensagem que falhou e uma linha de log em cada register mostraram que nada expirou e que uma verificação e uma reserva eram dois passos. Um relatório de falha deve dizer onde a coisa está agora, em cada nó (`diagnosis` no `tests/chaos.rs`).
- **Uma nova tentativa que continua falhando é um estado a inspecionar, não um tempo a esperar.** A primeira nota aqui propunha que o `drain_topic` tentasse de novo; a causa real teria ficado escondida. Manter o cluster de pé depois da falha e comparar a mesma mensagem em cada nó (`memoryMessages`, a tabela `excluded`) achou a causa em uma rodada.

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` e `tests/channel.rs` (UC-CAN-029).
- `src/cluster/replication/definitions.rs` (`offer`, `spread`, `relay`, `coordinate`), `src/cluster/service/mod.rs` (`recover_previous_life`, `finish_join`, `activate`) e `tests/cold_start.rs` (UC-RTN-058); a linha do tempo da oferta perdida está no log guardado da caçada de 2026-10-26.
- O travamento de 2026-10-26 foi reproduzido localmente e num container Linux; a pilha, os logs e as contagens de execuções são os da tabela de histórico.
- As falhas foram vistas nos logs do CI do repositório; as datas de "achado" vêm do histórico do git e dos logs.
