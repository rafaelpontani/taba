---
título: Problemas conhecidos
resumo: "Registro, com histórico, dos testes de cluster intermitentes e dos problemas conhecidos: cada um com título, status, data em que foi achado, data da solução e o histórico de ocorrências e tentativas."
tipo: referência
status: parcial
verificado-em: 2026-11-09 @ edcf155
fontes: [crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/recovery/restoration.rs, crates/taba-node/src/cluster/recovery/finish.rs, crates/taba-node/tests/restore_lives.rs, crates/taba-node/tests/regroup.rs, crates/taba-node/src/engine/messages/blocks.rs, connectors/gateway-contract/drivers/go/main.go, connectors/gateway-contract/scenarios/aws/sqs-standard.scn, connectors/gateway-contract/scenarios/taba/limits-sqs.scn, crates/taba-node/tests/partition.rs, crates/taba-node/tests/batch.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/tests/cold_start.rs, crates/taba-node/src/cluster/channel/mod.rs, crates/taba-node/tests/channel.rs, crates/taba-node/tests/failures.rs, crates/taba-node/tests/replication.rs, crates/taba-node/tests/notify.rs, crates/taba-node/tests/common/cluster.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/service/mod.rs]
---

# Problemas conhecidos

> Leia antes: [25-testes.md](25-testes.md).

Cada teste intermitente ou problema conhecido tem uma seção própria e um código `KI-NNN` (do inglês *known issue*) no início do título, dado na ordem em que os problemas foram achados; o código nunca é reutilizado e é a forma de citar o problema em commits, pendências e decisões. A seção **nunca é apagada**: quando o problema é resolvido ela passa a `Corrigido`; se voltar a aparecer, passa a `Reaberto` e o histórico continua na mesma seção. Os testes marcados com `#[ignore]` passam na máquina de desenvolvimento, mas falharam de forma intermitente nos runners do GitHub Actions, que são lentos e compartilhados. Para rodar um à mão: `cargo test -p taba-node --test <arquivo> <nome> -- --ignored`.

Status possíveis: `Aberto`, `Corrigido`, `Reaberto`. As regras para tratar testes intermitentes estão no [AGENTS.md](../../AGENTS.md).

| Código | Problema | Status | Achado em | Solução em |
|---|---|---|---|---|
| KI-029 | [Um aviso de falha que conta reconexões do canal sob carga](#ki-029-um-aviso-de-falha-que-conta-reconexões-do-canal-sob-carga) | Aberto | 2026-11-10 | — |
| KI-028 | [Um novo prazo que não chega a todas as cópias em 30 s no arm64](#ki-028-um-novo-prazo-que-não-chega-a-todas-as-cópias-em-30-s-no-arm64) | Aberto | 2026-11-09 | — |
| KI-027 | [Um teste de token que achou um pedaço do segredo no sal](#ki-027-um-teste-de-token-que-achou-um-pedaço-do-segredo-no-sal) | Corrigido | 2026-11-09 | 2026-11-09 |
| KI-026 | [Uma mensagem confirmada trazida de volta por uma vida mais antiga](#ki-026-uma-mensagem-confirmada-trazida-de-volta-por-uma-vida-mais-antiga) | Corrigido | 2026-11-09 | 2026-11-09 |
| KI-025 | [Uma votação da partida a frio que não fecha em 30 s](#ki-025-uma-votação-da-partida-a-frio-que-não-fecha-em-30-s) | Corrigido | 2026-11-09 | 2026-11-09 |
| KI-024 | [Uma sondagem que não termina com o nó faltando em 30 s](#ki-024-uma-sondagem-que-não-termina-com-o-nó-faltando-em-30-s) | Corrigido | 2026-11-09 | 2026-11-09 |
| KI-022 | [Um ReceiveMessage grande do SDK Go antigo que perde a conexão sob carga](#ki-022-um-receivemessage-grande-do-sdk-go-antigo-que-perde-a-conexão-sob-carga) | Corrigido | 2026-11-08 | 2026-11-09 |
| KI-021 | [Uma contagem de um emulador logo depois de um receive](#ki-021-uma-contagem-de-um-emulador-logo-depois-de-um-receive) | Corrigido | 2026-11-08 | 2026-11-09 |
| KI-020 | [Uma mensagem entregue duas vezes depois da reunião de uma partição](#ki-020-uma-mensagem-entregue-duas-vezes-depois-da-reunião-de-uma-partição) | Corrigido | 2026-11-07 | 2026-11-09 |
| KI-019 | [Um teste de lote que contou as chamadas antes do commit em segundo plano](#ki-019-um-teste-de-lote-que-contou-as-chamadas-antes-do-commit-em-segundo-plano) | Corrigido | 2026-11-07 | 2026-11-07 |
| KI-018 | [Um segundo canal que não é fechado primeiro](#ki-018-um-segundo-canal-que-não-é-fechado-primeiro) | Corrigido | 2026-11-05 | 2026-11-05 |
| KI-017 | [Um nó que para de responder e não é excluído em 30 s](#ki-017-um-nó-que-para-de-responder-e-não-é-excluído-em-30-s) | Aberto | 2026-11-04 | — |
| KI-016 | [Um teste de replicação cujas 14 publicações não caíram no nó isolado](#ki-016-um-teste-de-replicação-cujas-14-publicações-não-caíram-no-nó-isolado) | Corrigido | 2026-11-03 | 2026-11-03 |
| KI-015 | [Um teste de sincronização cuja segunda store achou o banco travado no arm64](#ki-015-um-teste-de-sincronização-cuja-segunda-store-achou-o-banco-travado-no-arm64) | Corrigido | 2026-11-03 | 2026-11-03 |
| KI-014 | [Um consume com ack que falha uma vez no docker/validate.sh depois de um drain](#ki-014-um-consume-com-ack-que-falha-uma-vez-no-dockervalidatesh-depois-de-um-drain) | Corrigido | 2026-11-03 | 2026-11-04 |
| KI-013 | [Uma votação de exclusão que falha uma vez na suíte completa](#ki-013-uma-votação-de-exclusão-que-falha-uma-vez-na-suíte-completa) | Corrigido | 2026-10-29 | 2026-11-10 |
| KI-010 | [Um register que verificou a mensagem antes de tomá-la](#ki-010-um-register-que-verificou-a-mensagem-antes-de-tomá-la) | Corrigido | 2026-10-27 | 2026-10-29 |
| KI-012 | [Uma rajada do canal que não chega ao outro nó na caça](#ki-012-uma-rajada-do-canal-que-não-chega-ao-outro-nó-na-caça) | Corrigido | 2026-10-29 | 2026-11-05 |
| KI-011 | [Um sinal de pronto perdido que a caça não corrige a tempo](#ki-011-um-sinal-de-pronto-perdido-que-a-caça-não-corrige-a-tempo) | Aberto | 2026-10-29 | — |
| KI-009 | [A rodada de caos: uma entrega além do limite e um ack que o dono nunca aplicou](#ki-009-a-rodada-de-caos-uma-entrega-além-do-limite-e-um-ack-que-o-dono-nunca-aplicou) | Corrigido | 2026-10-27 | 2026-10-27 |
| KI-008 | [Um nó de teste reiniciado que perde a porta para outro teste](#ki-008-um-nó-de-teste-reiniciado-que-perde-a-porta-para-outro-teste) | Corrigido | 2026-10-26 | 2026-11-09 |
| KI-023 | [Um nó excluído enquanto sincroniza que não consegue voltar](#ki-023-um-nó-excluído-enquanto-sincroniza-que-não-consegue-voltar) | Aberto | 2026-11-09 | — |
| KI-007 | [Uma definição oferecida a um nó que ainda estava entrando](#ki-007-uma-definição-oferecida-a-um-nó-que-ainda-estava-entrando) | Corrigido | 2026-10-26 | 2026-11-09 |
| KI-006 | [Um accept de canal que travou os peers duas vezes](#ki-006-um-accept-de-canal-que-travou-os-peers-duas-vezes) | Corrigido | 2026-10-26 | 2026-10-26 |
| KI-005 | [Teste de reinício da FIFO que espera 30 s numa execução combinada](#ki-005-teste-de-reinício-da-fifo-que-espera-30-s-numa-execução-combinada) | Aberto | 2026-10-22 | — |
| KI-004 | [Teste de aviso que espera 30 s uma vez na suíte completa](#ki-004-teste-de-aviso-que-espera-30-s-uma-vez-na-suíte-completa) | Resolvido | 2026-10-20 | 2026-11-08 |
| KI-003 | [Reingresso depois de uma partição simétrica](#ki-003-reingresso-depois-de-uma-partição-simétrica) | Corrigido | 2026-10-19 | 2026-10-20 |
| KI-002 | [Canal saudável que não volta](#ki-002-canal-saudável-que-não-volta) | Corrigido | 2026-10-18 | 2026-10-19 |
| KI-001 | [Inversão de ordem de travas ao escolher réplicas](#ki-001-inversão-de-ordem-de-travas-ao-escolher-réplicas) | Corrigido | 2026-10-18 | 2026-10-18 |

## KI-029: Um aviso de falha que conta reconexões do canal sob carga

**Status:** Aberto · **Achado em:** 2026-11-10 · **Solução em:** —

- **Teste:** `tests/failures.rs::a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up` (UC-FAL-005, UC-FAL-023).
- **Sintoma:** `failures.rs:274`, `left: 2.0, right: 0.0`: a soma de `taba_cluster_channel_reconnects_total` nos dois nós saudáveis, que o teste espera ser zero (a votação foi aberta pelo aviso do nó, não pela conexão). Na caça que validou o KI-013 (`ROUNDS=30 KEEP_GOING=1 docker/hang-hunt.sh failures`, 8 processos do binário inteiro com os logs detalhados ligados) falhou em 49 das 120 primeiras execuções; não foi visto nas execuções normais.
- **Causa:** não identificada. A leitura a conferir: sob essa carga o keepalive do canal (200 ms em `FAST`) expira e o canal reconecta uma vez em cada nó saudável, e o contador não fica em zero embora a exclusão esteja certa; se for isso, o defeito está na asserção e não no servidor.
- **Próximo passo:** ler num log guardado (`target/hang-hunt/run_*.log`) por que os canais reconectaram (o keepalive, ou a própria exclusão fechando-os antes de o contador ser lido).
- **Não fazer:** aumentar os 30 s do `eventually`.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-10 | Achado na caça que validou o KI-013: 49 falhas em 120 execuções sob essa carga. |

## KI-028: Um novo prazo que não chega a todas as cópias em 30 s no arm64

**Status:** Aberto · **Achado em:** 2026-11-09 · **Solução em:** —

- **Teste:** `tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation` (UC-INV-007, UC-INV-016).
- **Sintoma:** `waited 30 s for: the new deadline on all the copies` (`common/cluster.rs:89`), no job do servidor em arm64 do `release.yml` final da `v0.3.11` (execução 42725624609, commit `197d008`). O mesmo commit tinha passado nas duas arquiteturas no pull request e no candidato `v0.3.11-rc.3`; rodar de novo o job que falhou passou.
- **Causa:** não identificada. O log do job não foi estudado.
- **Próximo passo:** `docker/hang-hunt.sh replication the_visibility_can_be_changed` até guardar uma execução que falha, e ler qual cópia não recebeu o novo prazo e por quê (a mudança da visibilidade é aplicada pelo primário e chega às cópias pelo canal).
- **Não fazer:** aumentar os 30 s do `eventually`.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Uma falha no arm64 na release final da `v0.3.11`; o job rodou de novo e passou. |

## KI-027: Um teste de token que achou um pedaço do segredo no sal

**Status:** Corrigido · **Achado em:** 2026-11-09 · **Solução em:** 2026-11-09

- **Teste:** `tests/stored_definition.rs::each_token_is_a_row_with_the_salted_hash_and_never_the_secret`.
- **Sintoma:** `the secret is not in the row` (`stored_definition.rs:153`), no `cargo test` completo de 2026-11-09; 1 em 30 execuções do teste sozinho.
- **Causa (no teste):** o token é `tb_<id>.<secret>`, e o teste tomava como segredo o que vem depois do último `_` (`full.rsplit('_')`). O segredo é base64url e pode ter um `_`; aí o "segredo" era um pedaço de poucos caracteres, que aparecia por acaso dentro do sal ou do hash (também base64url) da linha.
- **Correção:** o segredo é o que vem depois do `.` (`split_once('.')`), 43 caracteres.
- **Verificação:** 40 execuções do teste sozinho, nenhuma falha; o `cargo test` completo passa.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Visto na suíte completa ao atualizar o `rusqlite` (sem relação com ele: a causa acima está no teste); corrigido no teste. |

## KI-026: Uma mensagem confirmada trazida de volta por uma vida mais antiga

**Status:** Corrigido · **Achado em:** 2026-11-09 · **Solução em:** 2026-11-09

- **Teste:** `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (UC-RTN-053).
- **Sintoma:** `round 1: <id> came back` em `cold_start.rs:696`: uma mensagem restaurada e confirmada na rodada 0 foi entregue de novo depois das quedas da rodada 1. Vista uma vez em 1800 execuções de processo de `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start`; não vista no CI.
- **Causa (confirmada, no servidor):** a ordem da restauração. Um nó que cai duas vezes guarda duas vidas: a mais antiga ainda tem a mensagem (uma cópia cuja remoção não chegou a ela antes da queda), e a mais nova registrou o ack no log de excluídos dela. A restauração pegava as vidas da mais antiga para a mais nova e **incorporava o log de cada vida logo antes de enviar as mensagens dela**, então a mensagem da vida antiga ia ao primário antes de o log da vida nova, que diz que ela morreu, estar no log do nó: a conferência dos mortos (o próprio log, os outros nós, o S3) não a achava, e a mensagem era ativada. A mesma brecha existia entre nós: a barreira da restauração contava as vidas que um nó anunciou, não se os logs delas já estavam no log dele, então um primário podia perguntar a um nó que ainda não os tinha incorporado. No log: a restauração da rodada 1 com `lives=3` enviou o `id` que a rodada 0 tinha restaurado, entregado e confirmado.
- **Correção:** o nó incorpora as linhas dos logs de **todas** as vidas quando a restauração dele começa, e só então diz aos outros `logsReady` em `GET /cluster/restore`; a barreira só conta as vidas de um nó depois disso (um nó de uma versão anterior não informa e conta como pronto). Dentro da passada, os logs de todas as vidas restauráveis (com o início da gravação) entram antes de a primeira mensagem de qualquer uma delas ser enviada ([07-cluster.md](07-cluster.md), seções 10.3 e 10.7).
- **Testes da correção:** `tests/restore_lives.rs::the_log_of_a_newer_life_is_in_before_the_messages_of_an_older_one_are_sent` (falha sem a correção: `node 1: an acknowledged message came back`) e `tests/restore_lives.rs::a_node_tells_its_lives_logs_are_in_only_when_its_answer_to_the_dead_has_them`.
- **Verificação:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` com todas as correções de 2026-11-09: 1800 execuções de processo e **nenhum** `came back` (as únicas 2 falhas foram a corrida de "Uma definição oferecida a um nó que ainda estava entrando", corrigida depois). O `tests/restore_lives.rs` (12 testes) passa.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Achada pela caça que conferiu o `PortHold` (rodada 17 de 180); o log mostrou o `id` restaurado de novo por uma vida da rodada 0. Corrigida na ordem da restauração, com os dois testes acima. |
| 2026-11-09 | Verificada: 1800 execuções de processo da mesma caça com a correção, nenhum `came back`. |

## KI-025: Uma votação da partida a frio que não fecha em 30 s

**Status:** Corrigido · **Achado em:** 2026-11-09 · **Solução em:** 2026-11-09

- **Teste:** `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (UC-RTN-053), em `come_back_all`.
- **Sintoma:** `waited 30 s for: the vote closed on every node` (`common/cluster.rs:89`). Visto duas vezes em 1800 execuções de processo de `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rodadas 3 e 16); não visto no CI.
- **Causa (no teste):** a mesma da seção abaixo. Os nós caíam um depois do outro, e sob carga os que ainda estavam de pé tinham tempo de votar a exclusão do primeiro que caiu e reescrever o manifesto da vida deles com uma tabela de dois. Na volta os discos discordavam da tabela: um nó começava sozinho com o grupo antigo de um, os outros formavam outro grupo, e só a sondagem dos IPs antigos (a cada 10 s) os juntava pelo reagrupamento (doc 07, seção 10.11), depois dos 30 s da espera. Os dois logs que falharam têm a linha `this side is the smaller and regroups` e um nó a mais que `starts alone` com `alive=1` do que os logs das outras falhas.
- **Correção:** o `fall_all` de `tests/cold_start.rs` corta a rede entre os nós (`isolate`) antes da primeira queda e a devolve (`restore`) depois da última: sem quórum ninguém é excluído, e todo disco fica com a tabela de três. Todos os testes do arquivo que derrubam o cluster inteiro o usam, e o `regroup.rs::two_groups` faz o mesmo.
- **Verificação:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` com o `fall_all`: 1800 execuções de processo, **nenhum** `the vote closed on every node` e nenhum `the probe ended with a node missing` (antes: 2 + 2 em 1800); `KEEP_GOING=1 ROUNDS=60 PAR=10 docker/hang-hunt.sh regroup`: 600 execuções, nenhuma falha.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Visto duas vezes na caça que conferiu o `PortHold`; registrado com a causa em aberto. |
| 2026-11-09 | Causa achada nos logs (um manifesto com tabela de dois) e corrigida no teste (`fall_all`); verificado em 1800 execuções do `cold_start` e 600 do `regroup` sem nenhuma falha. |

## KI-024: Uma sondagem que não termina com o nó faltando em 30 s

**Status:** Corrigido · **Achado em:** 2026-11-09 · **Solução em:** 2026-11-09

- **Teste:** `tests/cold_start.rs::the_node_that_was_missing_when_the_probe_ended_joins_later_by_the_normal_join_and_the_condition_goes_away` (UC-RTN-057).
- **Sintoma:** `waited 30 s for: the probe ended with a node missing` (`common/cluster.rs:89`). Visto duas vezes em 1800 execuções de processo de `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rodadas 28 e 161); não visto no CI.
- **Causa (no teste):** nos dois logs que falharam a sondagem **terminou** nos dois nós, com `expected=2`: a tabela da vida anterior tinha dois membros, não três, então dois membros ativos não "faltam" ninguém e o `members_missing` fica `false`. Os nós caíam um depois do outro (`while … fall`), e sob carga os dois que ainda estavam de pé votaram a exclusão do primeiro que caiu e gravaram o manifesto com a tabela de dois antes de caírem também.
- **Correção:** as quedas com a rede cortada, no `fall_all` (seção acima).
- **Verificação:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` com o `fall_all`: 1800 execuções de processo, **nenhum** `the vote closed on every node` e nenhum `the probe ended with a node missing` (antes: 2 + 2 em 1800); `KEEP_GOING=1 ROUNDS=60 PAR=10 docker/hang-hunt.sh regroup`: 600 execuções, nenhuma falha.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Visto duas vezes na caça que conferiu o `PortHold`; registrado com a causa em aberto. |
| 2026-11-09 | Causa achada nos logs (um manifesto com tabela de dois) e corrigida no teste (`fall_all`); verificado em 1800 execuções do `cold_start` e 600 do `regroup` sem nenhuma falha. |

## KI-022: Um ReceiveMessage grande do SDK Go antigo que perde a conexão sob carga

**Status:** Corrigido · **Achado em:** 2026-11-08 · **Corrigido em:** 2026-11-09

- **Teste:** os testes de contrato do gateway (`connectors/gateway-contract/run.sh`) num nó Taba, driver `go-old` (aws-sdk-go-v2 com `service/sqs` v1.26.0 e smithy-go v1.16.0): o `receive` de uma mensagem grande (200 KB).
- **Sintoma:** `operation error SQS: ReceiveMessage, https response error StatusCode: 200 … deserialization failed, failed to decode response body, read tcp …: use of closed network connection`, e a linha seguinte falha por não ter o receipt handle. Na máquina local, com a CPU carregada, cerca de 1 execução em 8; nunca nos emuladores.
- **Causa (confirmada, no cliente):** o net/http escreve o corpo da requisição direto no socket, o nó responde na hora e o `Do` retorna com os cabeçalhos. O smithy-go então fecha o corpo da requisição (`transport/http/client.go`). Depois de escrever o corpo, o net/http lê o corpo mais uma vez para conferir que não sobrou nada (`transferWriter.writeBody`, `doBodyCopy(io.Discard, body)`); essa leitura passa pelo `WriteTo` do `safeWriteToReadCloser` do smithy-go, que depois do fechamento responde `io.EOF` em vez de `(0, nil)`. Com a máquina carregada e essa leitura vindo depois do fechamento, o `persistConn.writeLoop` toma o `EOF` como erro de escrita e fecha a conexão no meio da resposta. O mesmo código está no smithy-go v1.28.8, mas o SDK atual não falhou nas nossas execuções. O nó responde certo: `Content-Length` correto, sem `Connection: close`, sem FIN nem RST do lado dele.
- **Correção:** só no teste. O driver Go entrega ao net/http o corpo da requisição sem o `WriteTo` (`plainBodyTransport` em `drivers/go/main.go`): um `Read` comum depois do fechamento é `(0, io.EOF)`, que o `io.Copy` toma como o fim do corpo.
- **Como foi achado:** um `tcpdump` no namespace de rede do nó (`-i eth0`, iniciado depois do `docker restart` do `gw_target_taba_up`; com `-i any` não capturou nada) mostrou o RST saindo do cliente no meio de uma resposta bem formada. Um invólucro de `net.Conn` no driver mostrou o `persistConn.writeLoop` fechando a conexão, e o `httptrace.WroteRequest` deu o erro: `EOF`. Desligar o keep-alive não resolveu (2 falhas em 19 execuções); esconder o `WriteTo` resolveu.
- **Verificação:** com a correção, 40 de 40 execuções do cenário de 200 KB passaram sob a mesma carga (12 containers usando a CPU); antes dela, entre 1 em 20 e 7 em 20 falhavam. O SDK atual e o antigo passam todos os cenários.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-08 | Falhou no CI da cadeia de release da v0.3.10 (testes de contrato do gateway num nó Taba); passou quando o job rodou de novo. |
| 2026-11-09 | Reproduzido localmente com uma mensagem de 200 KB e 10 a 12 containers usando a CPU (entre 1 em 20 e 7 em 20 execuções); causa achada com uma captura e os rastros do cliente, corrigida no driver. |

## KI-021: Uma contagem de um emulador logo depois de um receive

**Status:** Corrigido · **Achado em:** 2026-11-08 · **Corrigido em:** 2026-11-09

- **Teste:** os testes de contrato do gateway nos emuladores (`run.sh --target emulator`, o job "Scenarios on the emulators"), driver `rust-current`, linha 115 de `aws/sqs-standard.scn`.
- **Sintoma:** `getQueueAttributes :: attr_ApproximateNumberOfMessages=2 but was '0'` depois de um `sendBatch` de três mensagens e um `receive` de uma, no ElasticMQ. A mesma linha passou no nó Taba na mesma execução.
- **Causa:** a linha pedia uma contagem exata logo depois de um receive, mas o SQS chama essas contagens de aproximadas (podem demorar a se acertar), e o `sendBatch` antes dela não conferia que as três entradas foram aceitas. Qual das duas coisas o emulador fez não se sabe (uma ocorrência, não reproduzida). Não é o nó: o Taba responde as contagens a partir do índice da fila naquele instante (`topic_snapshot`).
- **Correção:** só no teste. `aws/sqs-standard.scn` confere `sendBatch … => ok=3 failed=0` e só que as duas contagens estão lá (`>=0`); as contagens exatas foram para `taba/limits-sqs.scn`, que só roda no Taba.
- **Verificação:** o contrato inteiro passou no nó Taba (as 12 variantes de SDK, 286 de 286 linhas cada, o consumidor nativo e os testes cruzados) e nos emuladores (173 de 173 linhas SQS e 50 de 50 SNS em cada variante).

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-08 | Falhou uma vez no CI do pull request do provider Pulumi, nos emuladores; passou quando o job rodou de novo. |
| 2026-11-09 | Não reproduzido localmente. O log do job que falhou mostra que o alvo era o ElasticMQ, não o nó; a linha foi dividida como acima. |

## KI-020: Uma mensagem entregue duas vezes depois da reunião de uma partição

**Status:** Corrigido · **Achado em:** 2026-11-07 · **Corrigido em:** 2026-11-09

- **Teste:** `tests/partition.rs::the_half_that_loses_the_reunion_keeps_what_it_accepted_while_apart` (UC-FAL-026, a medição de R1 em [37-garantias.md](37-garantias.md)).
- **Sintoma:** no job do servidor em arm64 do `release.yml` do merge do #63 (run 42409549368), o teste terminou com `delivered twice: [(18, 2)]` em `partition.rs:147`: nenhuma mensagem perdida, mas a de `seq` 18 (publicada no nó 3, da metade que fica com 3 e 4) foi entregue duas vezes. O mesmo commit passou no amd64, no CI do pull request e na `v0.3.8-rc.1`, que tem o mesmo código do servidor.
- **Causa (confirmada, no servidor):** uma conferência dos mortos separada da gravação. Os dois nós da metade perdedora guardam uma cópia de cada mensagem que aceitaram (o fator de replicação), então os dois restauram a sua vida e mandam a mesma mensagem, com o mesmo `id`, ao novo primário. O primário confere os mortos de cada lote (`accept_restore`: o próprio log, o log dos outros nós, o S3) e só depois ativa. Os dois lotes passaram pela conferência enquanto a mensagem ainda estava viva; a cópia do primeiro foi ativada, entregue e confirmada (o `id` foi para o log de excluídos); então o lote do segundo chegou à gravação, que só perguntava se o nó tinha a mensagem (`holds`), e a gravou de novo. No log de uma execução que falhou: duas linhas `lost message recovered` para o mesmo `id` com 19 ms de diferença, e as duas entregas com `deliveryCount=1` em dois nós diferentes, cada ack respondido com `204`. Não era o *pelo menos uma vez* de um ack que falhou, que teria voltado com `deliveryCount=2`.
- **Correção:** a gravação de uma mensagem recuperada ou restaurada lê de novo o log de excluídos **sob a mesma trava do tópico** (`Messages::apply_recovered`, usado por `Recovery::apply_local`; uma resposta nova, `Loaded::Dead`, que conta como resolvida). Um ack tira a mensagem e grava a exclusão sob essa mesma trava, então, quando a mensagem não está lá, o log já diz se ela morreu.
- **Teste da correção:** `src/engine/messages/tests/lives.rs::a_copy_of_a_message_acknowledged_after_the_check_of_the_dead_is_not_stored_again` (falha sem a correção: `left: Stored, right: Dead`).
- **Como foi achado:** o `docker/hang-hunt.sh partition` (8 processos, 40 rodadas) reproduziu em 4 de 320 processos; uma linha `HUNT` temporária no `drain` do teste (nó, `seq`, `id`, `deliveryCount`, status do ack) mostrou o mesmo `id` entregue por dois nós e os dois acks aceitos.
- **Verificação:** com a correção, a mesma caça rodou 120 rodadas de 8 processos (960 execuções) sem problema; antes dela, 4 de 320 falharam.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-07 | Uma falha em arm64 no `release.yml` do merge do #63, que só mudou conectores e documentação; os jobs que falharam rodaram de novo. |
| 2026-11-07 | O `docker/hang-hunt.sh partition` (8 processos ao mesmo tempo) reproduziu na rodada 12: `delivered twice: [(17, 2)]`. O log ficou em `target/hang-hunt/run_12_2.log`, ainda não lido. |
| 2026-11-09 | A caça (40 rodadas de 8 processos) falhou em 4 processos com a mesma assinatura; a causa foi achada no log com uma linha temporária no teste e corrigida na gravação da restauração. |

## KI-019: Um teste de lote que contou as chamadas antes do commit em segundo plano

**Status:** Corrigido · **Achado em:** 2026-11-07 · **Corrigido em:** 2026-11-07

- **Teste:** `tests/batch.rs::a_batch_of_publications_goes_through_each_phase_in_one_call_per_node_and_group` (e, com o mesmo padrão, `operations_waiting_for_the_same_node_travel_in_fewer_calls_than_operations` e `with_max_items_1_every_operation_goes_in_a_call_of_its_own`).
- **Sintoma:** no job do servidor em arm64 do pull request #62, `batch.rs:436` falhou com `362`: o teste esperava pelo menos 400 chamadas de registro e commit entre os nós para 100 publicações. Os outros 9 testes do arquivo passaram.
- **Causa:** o teste, não o lote. O coordenador responde quando o quórum confirma o commit (o primário e mais um nó); o terceiro nó recebe o commit em segundo plano ([doc 10](10-publicacao.md)). O teste contava as chamadas da rede em memória logo depois da resposta, então num runner lento parte dos commits ainda não tinha saído.
- **Correção:** os três testes esperam (`eventually`, até 30 s) que o número esperado de chamadas tenha passado antes de compará-lo com o número de lotes. O que eles provam continua igual. O código do servidor não mudou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-07 | Uma falha em arm64 no #62. Testes corrigidos; 0 falhas em 40 execuções do arquivo inteiro (10 rodadas de 4 processos) no Windows. |

## KI-018: Um segundo canal que não é fechado primeiro

**Status:** Corrigido · **Achado em:** 2026-11-05 · **Corrigido em:** 2026-11-05

- **Teste:** `tests/channel.rs::two_connections_at_once_end_with_a_single_channel_per_pair`.
- **Sintoma:** no job do servidor em arm64 do pull request "Release v0.3.6" (#52), `channel.rs:599` falhou com `assertion failed: closed.is_none()`: a primeira coisa que o teste leu da conexão extra foi uma mensagem, não o fechamento. Os outros 15 testes do arquivo passaram.
- **Causa:** o teste, não o canal. `Channels::install` só recusa a conexão extra quando o nó tem, naquele instante, um link aberto por ele mesmo; com um link aberto pelo outro lado, ou sem link, ele fica com a nova e conversa nela até o nó 1 reabrir a sua, e aí a conexão extra fecha. Esse é o comportamento projetado (o mesmo lado reabrindo substitui o link antigo), mas o teste exigia que a primeira coisa lida fosse o fechamento. O log da falha capturada mostra o link do par caindo 3 ms antes da asserção, o que não acontece no caminho que o teste supunha. Qual link o nó 2 tinha não vai para o log, então esse detalhe vem do código.
- **Correção:** o teste lê até a conexão fechar, com limite de 30 s. O que ele prova continua igual: a conexão do menor hash fecha e sobra um canal. O código do servidor não mudou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-05 | Uma falha em arm64 no #52. Não reproduzida na caça: 0 falhas em 480 execuções do teste sozinho (60 rodadas de 8 processos) e 0 em 320 execuções do arquivo inteiro (40 rodadas de 8 processos). |
| 2026-11-05 | Reproduzida uma vez em outras 320 execuções do arquivo inteiro, com o log guardado. Teste corrigido; a mesma caça depois: 0 falhas em 320 execuções. |

## KI-017: Um nó que para de responder e não é excluído em 30 s

**Status:** Aberto · **Achado em:** 2026-11-04 · **Corrigido em:** —

- **Teste:** `tests/failures.rs::a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded`.
- **Sintoma:** no job do servidor em amd64 do back-merge `main → release` depois da `v0.3.5` (#50), o teste terminou com `waited 30 s`. Rodar o job de novo passou.
- **Causa:** não identificada. O log do job que falhou não foi estudado.
- **Próximo passo:** `docker/hang-hunt.sh failures a_node_that_stops_answering` até guardar uma execução que falhe, e ler qual nó não participou da votação.
- **Não fazer:** aumentar os 30 s do `eventually`.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-04 | Uma falha em amd64 no #50; rodar o job de novo passou. |

## KI-016: Um teste de replicação cujas 14 publicações não caíram no nó isolado

**Status:** Corrigido · **Achado em:** 2026-11-03 · **Corrigido em:** 2026-11-03

- **Teste:** `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`.
- **Sintoma:** nos checks do merge de volta `main → release` depois da `v0.3.3` (#43), o job do servidor no amd64 falhou em `replication.rs:393` com `ok 14 failed 0`: nenhuma das 14 publicações foi recusada.
- **Causa:** o teste isola o nó 3 e espera que algumas publicações falhem, as que têm o nó 3 como primário; o primário vem do hash do `id` aleatório de cada mensagem, então com 14 publicações fixas nenhuma cai no nó 3 com probabilidade (2/3)^14, cerca de uma vez em 300 execuções. Não tem relação com uma mudança de código.
- **Correção:** o teste publica até ter visto os dois resultados (pelo menos uma aceita e uma recusada), no mínimo 14 e no máximo 60 vezes; a chance de não cair no nó 3 em 60 é de cerca de 10^-11.
- **Verificação:** o teste passou 5 de 5 e o arquivo de 21 testes passou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-03 | Falhou uma vez no amd64 no #43 (`ok 14 failed 0`); o teste publica até ver os dois resultados. |

## KI-015: Um teste de sincronização cuja segunda store achou o banco travado no arm64

**Status:** Corrigido · **Achado em:** 2026-11-03 · **Corrigido em:** 2026-11-03

- **Teste:** `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on` (UC-SIN-025 a 028).
- **Sintoma:** na rodada do `release.yml` em `main` para a `v0.3.3` (merge do #38), o job do servidor no arm64 falhou em `sync.rs:322`: `evict_body(...).unwrap()` sobre `SqliteFailure(DatabaseBusy, "database is locked")`; o release parou antes de publicar. O mesmo código passou no amd64, na rodada da candidata e nos checks do pull request.
- **Causa:** o teste abre uma segunda `MessageStore` sobre o banco de um nó que está rodando para apagar um corpo, e o nó pode estar escrevendo no mesmo banco (a escritora, a limpeza do log de mensagens excluídas): a escrita do teste recebe `SQLITE_BUSY`. Um nó tem uma store só por banco, então o produto não é afetado; o runner arm64, mais lento, aumentou a janela.
- **Correção:** o teste tenta apagar de novo enquanto o banco está travado (`eventually`), e falha em qualquer outro erro.
- **Verificação:** o teste passou 5 de 5 sozinho e o arquivo de 12 testes 3 de 3.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-03 | Falhou uma vez no arm64 no release de `main`; o teste tenta de novo enquanto o banco está travado. |

## KI-014: Um consume com ack que falha uma vez no docker/validate.sh depois de um drain

**Status:** Corrigido · **Achado em:** 2026-11-03 · **Corrigido em:** 2026-11-04

- **Teste:** `docker/validate.sh`, etapa "No acknowledged message was lost": `drain_topic validation-crash` chama `taba-cli consume --max 10 --wait 3 --ack` em laço logo depois de o `taba-1` sair por drain e voltar.
- **Sintoma:** um `taba-cli consume` saiu com código 1 e o script parou (`Failed (code 1)`) sem dizer por quê: o `taba_cli` jogava fora o stderr do `docker compose run`, e o erro do `taba-cli` junto. Foi a primeira rodada depois de a máquina reiniciar, com o Docker Desktop recém-iniciado: o cluster levou 402 s para ficar saudável (24 s na rodada seguinte). A rodada seguinte, no mesmo código, passou em todas as etapas (50 mensagens publicadas, todas recebidas e confirmadas).
- **Causa:** uma mensagem já confirmada voltava à vida a partir do backup em S3. Quando um membro sai do cluster, cada nó roda o `recover_missing` na rodada seguinte de heartbeat e busca no bucket o que deveria guardar e não tem. Para saber o que está morto ele lia só o journal no bucket, que chega atrasado (`backup.meta.interval`), e não o log de excluídas do próprio nó. Uma mensagem que já estava num segmento (o drain do nó que saiu envia tudo) e foi confirmada nos segundos antes dessa rodada voltava, só naquele nó. Ela era entregue de novo, e o ack dela não conseguia mais fechar o quórum: o primário aceitava e os outros dois nós, que não a tinham mais, respondiam `message.not-found`, e o cliente recebia `503 cluster.quorum-unavailable`. Nenhuma mensagem se perdeu; mensagens confirmadas foram entregues de novo. Só clusters com `backup.s3.bucket` são afetados. O defeito é do nó, não da máquina: a primeira falha pareceu ligada a uma partida lenta só porque o tempo da rodada de heartbeat decide se os acks vêm antes.
- **Como foi achado:** com o cluster mantido de pé depois de uma falha (`TABA_KEEP=1`), o log da rodada que falhou tem `messages that only existed on dead nodes came back from S3 recovered=5` menos de um segundo depois dos acks; o `memoryMessages` dos três nós era `1 6 1`, onde um tópico saudável tem o mesmo número em todos; e 5 das mensagens ainda entregues estavam na tabela `excluded` dos três nós.
- **Correção:** o `recover_missing` também pula o que o log de excluídas do nó tem (`src/cluster/backup/restore.rs`). O ack grava a exclusão antes de tirar a mensagem do índice, sob a mesma trava, então uma mensagem que o nó não guarda mais já está no log. Teste: `tests/backup.rs::a_message_acked_before_the_journal_reaches_the_bucket_does_not_come_back_from_s3`, que falha sem a correção (a recuperação devolve 1). UC-BKP-046.
- **Verificação:** o `docker/validate.sh` falhou 4 de 4 rodadas nesta máquina antes da correção (uma delas em `a3bdab9`, antes do gateway SQS e SNS) e passou 3 de 3 com ela.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-03 | Uma falha na primeira rodada depois de reiniciar a máquina (cluster saudável em 402 s); a rodada seguinte passou. O `taba_cli` agora mostra o erro. |
| 2026-11-04 | Quatro falhas seguidas com `cluster.quorum-unavailable` na mesma etapa, inclusive no commit anterior ao gateway; a `v0.3.5` foi promovida com a falha registrada no pull request dela. |
| 2026-11-04 | Causa achada e corrigida: o `recover_missing` ignorava o log de excluídas. Teste novo; o `docker/validate.sh` passou 3 de 3. |

## KI-013: Uma votação de exclusão que falha uma vez na suíte completa

**Status:** Corrigido · **Achado em:** 2026-10-29 · **Corrigido em:** 2026-11-10

- **Teste:** `tests/failures.rs::only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion` (UC-FAL da exclusão), a asserção sobre os coordenadores das chamadas `unregister` (linha 1087).
- **Sintoma:** falhou uma vez num `cargo test` completo num contêiner Linux (22 testes do arquivo em paralelo com a carga dos outros binários); sozinho passou 3 de 3, e uma segunda execução completa passou.
- **Causa:** não identificada. Não toca o register de uma mensagem (achado ao validar [um register que verificou a mensagem antes de tomá-la](#ki-010-um-register-que-verificou-a-mensagem-antes-de-tomá-la)). A leitura a conferir é um segundo nó saudável que também registra a saída antes de ver a primeira terminada.
- **Hipótese (2026-11-09, não confirmada):** o teste exige algo que o servidor não garante sob carga, então o defeito estaria no teste. No `exclude()` (`cluster/vote/mod.rs`) cada nó que fecha o quórum ordena os eleitores que responderam a ele e espera `rank × write.commit.timeout` antes de coordenar; nos testes esse prazo é de 400 ms (`FAST` em `common/cluster.rs`). No `unregister_node_traced` um nó só envia `unregister` aos outros se a fase 1 do de menor ID ainda não chegou a ele (senão o registro local falha com `cluster.conflict` antes de qualquer chamada). Então um segundo coordenador aparece quando o de menor ID começa a exclusão mais de 400 ms depois do segundo: sob carga cada nó vê o canal do candidato cair num instante diferente, e a votação espera a resposta de todos os eleitores, de modo que um eleitor lento atrasa o quórum de um nó e não o de outro. O segundo nó então recebe `cluster.conflict`, espera, encontra o nó fora da tabela e termina com `excluded`: a exclusão está certa, mas o teste vê dois remetentes de `unregister` e falha.
- **A outra asserção (2026-11-10):** no CI o teste também falhou em `all the healthy ones voted` (`network.count("vote") >= 4`), o caso oposto: o de menor ID excluiu o candidato tão rápido que os outros o encontraram já fora da tabela e não abriram a votação deles (o `open` retorna quando o candidato não é membro), então houve menos de quatro chamadas `vote`. As duas asserções dependem do tempo entre os nós; o teste precisa conferir o que o servidor garante (uma exclusão, nenhuma falha) e não quantos nós chegaram a votar ou a coordenar.
- **Como confirmar:** a caça abaixo, e no log os instantes em que cada nó abriu a votação e fechou o quórum, e quem foi o coordenador a mais. Se os ranks diferirem entre os nós (um eleitor que não respondeu a um deles), a causa pode estar no servidor.
- **Correção, se confirmada (no teste):** um `TABA_WRITE_COMMIT_TIMEOUT` maior para este teste (5 s, por exemplo), para o escalonamento absorver a variação sob carga; ou a asserção trocada por "o primeiro `unregister` veio do de menor ID e a exclusão não falhou".
- **Correção (2026-11-10, no teste):** o teste roda com `TABA_WRITE_COMMIT_TIMEOUT=5s`, de modo que a vez do segundo nó (5 s) é maior que a diferença entre os instantes em que os nós veem a queda sob carga; e a contagem de todos os `vote` foi trocada por "o coordenador pediu o voto a cada um dos outros três saudáveis", que vale sempre que ele fechou o quórum. O servidor não mudou: o que ele garante (uma exclusão, nenhuma falha) já era conferido.
- **Verificação:** `ROUNDS=30 KEEP_GOING=1 docker/hang-hunt.sh failures` (8 processos do binário inteiro ao mesmo tempo, logs detalhados ligados): nas 15 primeiras rodadas (120 execuções) este teste não falhou nenhuma vez. As outras falhas dessa caça são o [KI-029](#ki-029-um-aviso-de-falha-que-conta-reconexões-do-canal-sob-carga) e o [KI-017](#ki-017-um-nó-que-para-de-responder-e-não-é-excluído-em-30-s).

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-29 | Uma falha na suíte completa, 3 passagens sozinho, 1 passagem completa depois. |
| 2026-11-09 | Hipótese registrada a partir da leitura do código (o escalonamento de 400 ms dos testes curto demais sob carga); ainda não confirmada por uma caça. |
| 2026-11-10 | Falhou no job do servidor em amd64 do pull request "Release v0.3.12" (#82) na outra asserção, `all the healthy ones voted`; o job rodou de novo. |
| 2026-11-10 | Corrigido no teste (vez de 5 s, o voto conferido no coordenador); 120 execuções na caça sem falha deste teste. |

## KI-010: Um register que verificou a mensagem antes de tomá-la

**Status:** Corrigido · **Achado em:** 2026-10-27 · **Corrigido em:** 2026-10-29

- **Onde:** `src/cluster/replication/replica.rs` (`Replica::register`, `reserve`). Achado pelo `tests/chaos.rs` ([37-garantias.md](37-garantias.md)), sementes 2, 3 e 4, [PRB-077](30-decisoes.md#problemas-resolvidos-prb).
- **Sintoma:** em cerca de 8 % dos processos da caça (`docker/hang-hunt.sh chaos`, 8 de cada vez; 13 em 160), "I5: seq N was acked on its queue and also reached the dead-letter" e, em alguns, "I3: seq N was acked (204) and delivered again".
- **Como foi achado:** o relatório de falha da rodada de caos agora tem um dump da mensagem que quebrou o I3 ou o I5 (onde ela está, em cada nó e em cada fila: `diagnosis`), e a caça foi rodada com uma linha em cada register, commit, rollback e expiração da réplica e no começo e no fim de cada ack e ida para a dead-letter (tiradas depois). A primeira leitura (um register expirado) estava errada: **nenhum register expirou** em nenhuma das rodadas que falharam. No log de uma, a ida para a dead-letter registrou e deu commit nos três nós (+80 ms), e só depois o register do ack chegou a eles, 120 ms depois de enviado, e foi aceito.
- **Causa:** `ack_step`, `register_write` e `visibility_step` verificavam a mensagem (`check_visibility`, `check_publish`...) numa thread bloqueante e depois chamavam o `register`. A verificação e a reserva da mensagem eram dois passos sem trava entre eles, e sob carga (8 processos ao mesmo tempo) uma operação inteira cabia no intervalo: o ack verificava uma mensagem que estava na fila, a ida para a dead-letter registrava e dava commit, e o ack registrava, dava commit (aplica-se a uma mensagem que já saiu, sem erro) e era confirmado ao cliente com `204`.
- **Correção:** o `register` reserva a mensagem primeiro (`by_message`, `409 cluster.conflict` para quem chega nesse meio-tempo) e roda a verificação com a reserva mantida; uma verificação que falha, ou uma chamada abandonada no meio dela, solta a reserva (`Reservation`, no `Drop`). O register repetido depois da decisão e o replay de eventos não verificam nada e não são afetados.
- **Teste:** `replica::tests::a_register_holds_the_message_while_its_check_runs` (uma verificação lenta; a segunda operação deve receber o conflito) e `a_register_whose_check_fails_releases_the_message`. Verificação: a mesma caça, 20 rodadas de 8 processos: **0 falhas em 160** (antes da correção, 13 em 160); suíte completa, clippy e fmt.
- **Não fazer:** aumentar o `write.pending.ttl` ou as esperas da rodada: nada expirou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-27 | Visto com saltos de relógio de 35 s (3 rodadas em 400): achou-se que era um register expirado (a primeira forma do PRB-077). |
| 2026-10-29 | Visto sem saltos de relógio na caça (3/96, 5/96, 3/96 no código antes dos lotes e com eles): os lotes não mudaram a taxa. |
| 2026-10-29 | O dump e as linhas em cada register mostraram que nenhum register expirou: a verificação rodava antes da reserva. Corrigido reservando primeiro. 0 falhas em 160 processos. |

## KI-012: Uma rajada do canal que não chega ao outro nó na caça

**Status:** Corrigido · **Achado em:** 2026-10-29 · **Solução em:** 2026-11-05

- **Onde:** `tests/channel.rs::each_peer_has_an_out_queue_and_an_in_queue_that_drain` (e, uma vez cada, `every_node_has_one_channel_to_each_other_node_and_measures_it` e `two_connections_at_once_end_with_a_single_channel_per_pair`).
- **Sintoma:** em `docker/hang-hunt.sh channel` (8 processos ao mesmo tempo) termina com `waited 30 s for: the burst to reach node 2`, em cerca de 1 processo em 14.
- **Causa:** o teste, não o canal. A rajada sinaliza `ready` de tópicos inventados (`t0` a `t299`). O heartbeat seguinte (200 ms nos testes) lista os tópicos prontos de verdade, e o `Signals::apply_keepalive` tira corretamente os outros da tabela do nó 2, então o `t299` só fica lá até esse heartbeat. Num runner carregado, a checagem, a cada 10 ms, pode perder essa janela, e aí espera 30 s por um estado que não volta. Não era consequência dos lotes: a mesma caça falhava na mesma taxa no código anterior a eles.
- **Correção:** o teste parte com `QUIET` (heartbeat de 30 s). Em `every_node_has_one_channel_to_each_other_node_and_measures_it`, uma causa à parte: o `queue_out` era lido uma vez e podia pegar um sinal a caminho (`left: 1`); agora espera a fila esvaziar. O outro teste citado aqui é o de [Um segundo canal que não é fechado primeiro](#ki-018-um-segundo-canal-que-não-é-fechado-primeiro).
- **Verificação:** ver o histórico. O código do servidor não mudou.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-29 | Encontrado na caça que validou o `take` e o repasse da FIFO no lote; a mesma taxa no commit anterior. |
| 2026-11-05 | A caça do arquivo inteiro (40 rodadas de 8 processos, em `4892068`) teve problema em 15 rodadas: 18 falhas de `each_peer_has_an_out_queue_and_an_in_queue_that_drain` e 1 de `every_node_has_one_channel_to_each_other_node_and_measures_it`. |
| 2026-11-05 | `each_peer_has_an_out_queue_and_an_in_queue_that_drain` falhou em arm64 no #52, a primeira vez no CI. Com `QUIET`, a mesma caça teve 0 falhas dele em 320 execuções (eram 18) e 2 de `every_node_has_one_channel_to_each_other_node_and_measures_it`, corrigido em seguida. Com as duas correções, ver [Um segundo canal que não é fechado primeiro](#ki-018-um-segundo-canal-que-não-é-fechado-primeiro). |

## KI-011: Um sinal de pronto perdido que a caça não corrige a tempo

**Status:** Aberto · **Achado em:** 2026-10-29 · **Solução em:** —

- **Onde:** `tests/replication.rs::a_lost_ready_signal_is_corrected_by_the_periodic_repeat`.
- **Sintoma:** em `docker/hang-hunt.sh replication` (8 processos ao mesmo tempo, 15 threads cada) falha em cerca de 3 de 5 processos de cada rodada, de três jeitos: o nó 2 consome só parte das mensagens nos 10 s da espera (`left: 4, right: 9`), ou uma publicação responde `503 cluster.commit-timeout` ou `403 cluster.not-member` (`replication.rs:40`). Passa sozinho e na execução normal da suíte.
- **Causa:** não identificada. **Não é consequência dos lotes**: a mesma caça no código anterior a eles (`ab14fa5`) falhou em 19 de 32 processos, no primeiro lote (`078c11f`) em 13 de 16 e no código com o `take` e o repasse da FIFO no lote em 15 de 16; os outros 19 testes do arquivo quase nunca falharam (algumas falhas isoladas, de um teste cada, nas mesmas rodadas). O teste perde todos os quadros do `channel`, publica e espera 10 s (200 × 50 ms de tempo real) a repetição do estado; o `Clock` manual não anda, e o batimento que repete o estado roda em tempo real, então a leitura que se encaixa (não provada) é que sob essa carga é a margem que falta.
- **Próximo passo:** ler o log de uma execução que falhou (`target/hang-hunt-replication/run_*.log`) para ver se a repetição chega tarde ou não chega, e comparar o `503` com o quórum da publicação; até lá o teste não é sinal de regressão sob a caça.
- **Não fazer:** aumentar os 10 s da espera sem saber qual dos dois é.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-29 | Encontrado na caça que validou o `take` e o repasse da FIFO no lote. A mesma caça nos dois commits anteriores deu a mesma taxa, então já estava lá. |

## KI-009: A rodada de caos: uma entrega além do limite e um ack que o dono nunca aplicou

**Status:** Corrigido · **Achado em:** 2026-10-27 · **Corrigido em:** 2026-10-27

- **Onde:** `src/engine/messages/mod.rs` (`take`, `take_candidates`), `src/cluster/replication/coordinator.rs` (`commit_until_done`) e `src/cluster/protocol/mod.rs` (`Request::decided`). Achado pelo `tests/chaos.rs` ([37-garantias.md](37-garantias.md)).
- **Sintoma 1 (I5):** com `maxDeliveries: 2` uma mensagem foi entregue com `deliveryCount: 3`, na maioria das rodadas da semente 3. Causa: um commit de entrega que chegou ao dono depois de a própria invisibilidade acabar pôs a mensagem na lista de prontas sem a liberação que confere o limite. Correção: o `take_candidates` não entrega a mensagem que gastou as entregas: esconde-a por um instante e a próxima liberação a manda para a dead-letter (a primeira forma da correção, uma ida imediata, quebrou o `a_shorter_ttl_expires_what_is_already_stored_even_in_flight`, que espera que o `ttl` vença, e foi trocada). Teste: `a_commit_that_arrives_after_its_invisibility_does_not_deliver_past_the_limit` (falha sem a correção).
- **Sintoma 2 (I1):** em cerca de 1 rodada em 100 uma mensagem confirmada nunca foi entregue: ficava só no dono (as réplicas tinham aplicado o ack dela), a entrega dela precisa das réplicas, e então o grupo FIFO ficava travado para sempre. Causa: o commit do ack ao dono chegou depois de o register do dono expirar; o coordenador o registrou de novo e desistiu na primeira recusa, ou porque o recibo tinha expirado ou porque outra operação segurava a mensagem (`cluster.conflict`). Correção: o register enviado de novo depois da decisão não confere recibo (`Request::decided`) e, num conflito, continua tentando até a outra operação acabar. Testes: `the_register_repeated_after_the_decision_checks_no_receipt` e `an_ack_the_replicas_applied_reaches_the_owner_even_if_another_operation_took_the_message` (falha sem a correção).
- **Como foi achado:** a primeira suspeita foi o próprio verificador: a trilha de uma mensagem (`History::trail`) mostrou que o `maxDeliveries: 2` de uma fila tinha sido aplicado às outras (um falso positivo, corrigido no verificador). Depois o relatório de falha ganhou onde a mensagem está em cada nó, e mostrou o dono com uma mensagem que as réplicas já não tinham. A linha de base sem a correção da entrega também falhou (1 rodada em 120), então a segunda causa era independente.
- **Não corrigido:** um ack e uma ida para a dead-letter, os dois decididos, quando um register expira ([PRB-085](31-pendencias.md), não visto); as falhas que lhe foram atribuídas eram [um register que verificou a mensagem antes de tomá-la](#ki-010-um-register-que-verificou-a-mensagem-antes-de-tomá-la).
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

## KI-008: Um nó de teste reiniciado que perde a porta para outro teste

**Status:** Corrigido · **Achado em:** 2026-10-26 · **Solução em:** 2026-11-09

- **Onde:** `tests/common/cluster.rs` (`respawn_node`) e os testes que trazem um nó de volta no endereço do que caiu (`tests/cold_start.rs`: `fall`, `come_back`).
- **Sintoma:** `the process restarts: Bind { name: "do cluster", addr: 127.0.0.1:<porta>, source: … AddrInUse … }` depois dos 30 s do `respawn_node`. Visto 3 vezes em cerca de 1800 execuções de processo do `docker/hang-hunt.sh cold_start …` (10 processos ao mesmo tempo), em `when_several_nodes_connect_the_highest_version…` e em `a_node_that_joins_offers_the_definitions_of_its_disk…`; ainda não visto no CI.
- **Causa (conferida no log da execução que falhou):** o `fall()` desliga o nó e libera a porta de cluster dele; o teste inicia o nó de novo na mesma porta mais tarde. No intervalo, o sistema entrega a porta liberada (efêmera) a outro listener: no log, a porta era a de cluster do nó caído e, dois segundos depois, a porta de **cliente** de um nó de **outro teste do mesmo processo**. O `respawn_node` tenta por 30 s, mas esse nó guarda a porta até o fim do próprio teste, então a nova tentativa não consegue ganhar. Os testes de um binário rodam em threads paralelas, então o mesmo pode acontecer no CI.
- **Correção:** `PortHold` em `tests/common/cluster.rs`: um listener de reserva aberto logo depois do desligamento (em `fall` de `cold_start.rs` e `regroup.rs`) e solto um instante antes do novo bind (`come_back`).
- **Verificação:** `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (1800 execuções de processo) e `KEEP_GOING=1 ROUNDS=150 PAR=10 docker/hang-hunt.sh regroup` (1500): **nenhum** `AddrInUse`, contra 3 em cerca de 1800 antes. A caça recebe um arquivo de teste por vez (os argumentos seguintes filtram nomes de testes). As outras 7 falhas da caça do `cold_start` são outros problemas, nas seções deles.
- **Não fazer:** aumentar os 30 s do `respawn_node`; a porta não volta enquanto o outro teste vive.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-26 | Visto uma vez na caçada que validou a correção da oferta perdida (rodada 29, 10 processos ao mesmo tempo). O log mostra a porta reusada por outro nó do mesmo processo. Não corrigido: não tem relação com aquela mudança. |
| 2026-10-26 | Mais duas na validação de 1500 execuções (rodadas 19 e 141), uma delas em `a_node_that_joins_offers…`: o mesmo `AddrInUse`. |
| 2026-10-28 | **Primeira vez no CI**: a execução do `Release` no push para `main` (merge do PR #34, amd64) falhou em `with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls` (`cold_start.rs`, `AddrInUse` na porta do cluster). `PortHold` incluído; `cold_start` (13) e `regroup` (3) passam localmente. |
| 2026-11-09 | Conferido: 1800 execuções do `cold_start` e 1500 do `regroup` sob carga sem `AddrInUse`. `Corrigido`. |

## KI-023: Um nó excluído enquanto sincroniza que não consegue voltar

**Status:** Aberto · **Achado em:** 2026-11-09 · **Solução em:** —

- **Teste:** `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows` (UC-RTN-058), no `TestCluster::start_with(3, …)`, antes de o teste fazer qualquer coisa.
- **Sintoma:** `waited 30 s for: the node to join the cluster` (`common/cluster.rs:89`). Visto uma vez em 300 execuções de processo de `KEEP_GOING=1 ROUNDS=30 PAR=10 docker/hang-hunt.sh cold_start`; não visto no CI.
- **Causa (achada no log):** o terceiro nó se registrou (`new node registered` em 09.78) e estava sincronizando; sob carga o canal dele com os outros caiu e voltou três vezes (`reestablished` em 14.96, 15.07, 16.00), e em 16.57 os outros dois abriram a votação com `reason="channel-down"` e o excluíram (`yes=2 healthy=2`). O nó percebeu (`the sources excluded this node while it synchronized: redoing the join`), mas o ID dele estava congelado (D106: um nó excluído não volta por `left_secs=300`), e toda nova entrada foi recusada com `ingresso rejeitado (freezing)` (332 vezes) até o teste desistir. O keepalive dos testes (`200ms`, `FAST`) torna a queda mais provável, mas o caminho existe em produção: um nó excluído no meio da sincronização fica fora do cluster por 5 minutos ([PRB-111](31-pendencias.md#testes-intermitentes-novos-problemas-e-limites)).
- **Próximo passo:** decidir o PRB-111 (a votação de um nó que ainda sincroniza, ou o congelamento de um nó que nunca ficou ativo) e depois um teste que derruba o canal de um nó que sincroniza.

**Histórico**

| Data | Evento |
|---|---|
| 2026-11-09 | Visto uma vez na caça que conferiu a segunda solução da seção abaixo; causa lida no log e registrada com o PRB-111. |

## KI-007: Uma definição oferecida a um nó que ainda estava entrando

**Status:** Corrigido · **Achado em:** 2026-10-26 · **Solução em:** 2026-11-09

- **Onde:** `src/cluster/replication/definitions.rs` (`Definitions::offer`), testado em `tests/cold_start.rs`.
- **Sintoma:** no CI, `cold_start` › `when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator` falhou com `waited 30 s for: the highest version in every node` (na execução do PR de 2026-10-26), e `a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins` falhou com `waited 30 s for: the offered queue in the cluster` (arm64, o push na `release`, no mesmo dia). Os dois só sob carga. Localmente: 2 falhas em cerca de 290 execuções dos dois testes, com 10 processos ao mesmo tempo.
- **Causa:** um nó que entra oferece o que o disco tinha (`offer`). O vencedor era aplicado no nó e enviado só como um quadro do canal (`relay`), e um quadro só alcança os peers que esse nó conhece naquele instante. Na execução capturada, o terceiro nó ficou ativo e ofereceu `only-3` em 46.965; o segundo nó foi registrado na tabela em 46.972, sete milissegundos depois, e ainda não era peer, então o quadro não foi para ele. A sincronização dele tinha listado as definições antes de a origem aplicar `only-3` (em 46.986), e o quadro do canal nunca foi enviado a ele: ficou ativo com `shared` mas sem `only-3`, para sempre. Uma mudança decidida por um dono não tem a brecha: o `coordinate` também envia um `definition-apply` a todo membro da tabela (`spread`), e um nó que sincroniza grava esse pedido com os eventos e o aplica no fim. O `offer` era o único caminho que entregava uma definição nova só pelo canal.
- **Como foi achado:** o `docker/hang-hunt.sh cold_start …` reproduziu no container; o log não tinha nenhuma linha sobre a oferta, então linhas temporárias (`HUNT`, já removidas) foram postas no `offer`, `learn`, `relay` e `broadcast` e a execução que falha foi capturada de novo. Comparar, nó a nó, o que foi oferecido, a quem foi enviado e o que cada nó aprendeu mostrou o `broadcast` com um peer, sete milissegundos antes de o segundo nó aparecer.
- **Solução:** depois de aplicar o que vence, o `offer` solta o lock da chave e envia um `definition-apply` a todo membro da tabela (`spread`, todas as definições ao mesmo tempo), além do `relay`. O lock é solto antes: com ele preso, dois nós oferecendo a mesma definição esperariam um pelo outro até o `write.commit.timeout`.
- **Teste:** `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows` (UC-RTN-058) oferece uma definição a partir do nó 2 e exige um `definition-apply` para os nós 1 e 3; sem a correção ele falha.
- **Segunda causa (2026-11-09):** o `spread` lia a tabela **uma vez**, antes de enviar. Um nó cujo registro foi confirmado alguns milissegundos depois dessa leitura não estava na lista, e a sincronização dele puxava as definições de um membro que ainda aplicava a mudança (o `definition-apply` do spread não tinha terminado lá): o nó ficava ativo sem a definição, e o quadro do `relay` também não chegava a ele. Na execução capturada do `when_several_nodes_connect…` o terceiro nó espalhou `shared` (versão 6) e `only-3` só para o nó 1 em 46.077–46.089; o nó 2 foi registrado em 46.112 e carregou `shared` na versão 1 da listagem do nó 1, que só terminou de aplicar em 46.22; em 52.24 o nó 2 ofereceu a versão 4 dele, os outros a recusaram, e o nó 2 ficou com ela, sem `only-3`. A execução do `a_node_that_joins_offers…` (a falha do arm64 de 2026-10-26, não reproduzida na época) era a mesma: o spread de `offered` foi para um membro, e o terceiro nó se registrou 60 ms depois.
- **Segunda solução:** quando a espera do `spread` termina, a tabela é lida de novo e os membros que entraram nesse meio-tempo também recebem o `definition-apply`. Nesse ponto todo membro da primeira leitura respondeu (já aplicou), e um nó que ainda não está na tabela não terminou o registro, então só puxa as definições depois, de membros que já têm a mudança. O `coordinate` do dono usa o mesmo `spread` e tem a mesma correção.
- **Teste da segunda solução:** `tests/cold_start.rs::a_node_that_registers_while_an_offer_is_spreading_also_receives_it` (UC-RTN-058): o `definition-apply` para o nó 1 é segurado (a chamada desiste em 5 s), um nó novo que estava cortado da rede se registra enquanto o nó 2 espera e para antes de puxar as definições (`pause_sync` em `topics`), e o nó 2 tem de enviar a mudança também a ele; sem a correção ele falha (`node 2 tells the new node, which came in during the spread`). Sob carga só o registro pode levar mais que esses 5 s; então a tentativa não testa nada, o teste espera esse nó terminar a entrada (um nó sincroniza por vez) e tenta de novo com outro nó, até 5 vezes.
- **Verificação da segunda solução:** `KEEP_GOING=1 ROUNDS=120 PAR=10 docker/hang-hunt.sh cold_start`: 1200 execuções de processo **sem** nenhuma falha do `when_several_nodes_connect…` nem do `a_node_that_joins_offers…` (antes: 2 em 1800). A versão final do teste novo, em `KEEP_GOING=1 ROUNDS=30 PAR=10 docker/hang-hunt.sh cold_start`: 300 execuções, nenhuma falha dele.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-26 | Primeira falha no CI do PR (`when_several_nodes_connect…`); passou localmente em 3 + 40 + 9 execuções. |
| 2026-10-26 | Segunda falha, no push na `release` em arm64 (`a_node_that_joins_offers…`), outro teste do mesmo binário e da mesma área. |
| 2026-10-26 | `docker/hang-hunt.sh cold_start` (10 processos ao mesmo tempo): falha na rodada 13 e na rodada 16 de outra execução, as duas em `when_several_nodes_connect…`. Com o log temporário, a causa acima. |
| 2026-10-26 | Correção aplicada e teste adicionado (falha sem a correção). A mesma caçada com a correção, 1500 execuções de processo: veja a linha seguinte. |
| 2026-10-26 | Verificação: `KEEP_GOING=1 docker/hang-hunt.sh cold_start …`, 150 rodadas de 10 processos ao mesmo tempo (1500 execuções dos testes da oferta): **nenhum** `waited 30 s` por uma definição, contra 2 em cerca de 290 execuções antes da correção. Os únicos 2 problemas foram `AddrInUse` ao reiniciar um nó (veja "Um nó de teste reiniciado que perde a porta para outro teste"), sem relação. |
| 2026-11-09 | **Reaberto**: `waited 30 s for: the highest version in every node` em `when_several_nodes_connect…`, duas vezes em 1800 execuções de processo de `KEEP_GOING=1 ROUNDS=180 PAR=10 docker/hang-hunt.sh cold_start` (rodadas 45 e 54). O log no nível padrão não diz qual nó não tem qual definição; o próximo passo é a caça só deste teste com linhas temporárias em `offer`, `learn` e `spread`. |
| 2026-11-09 | A mesma caça com linhas temporárias `HUNT` em `offer`, `apply_with` e `spread` (já removidas): 2 falhas em 1800 execuções, uma de cada teste da área (rodadas 15 e 148), as duas a segunda causa acima. Corrigido no `spread` com o teste acima. |
| 2026-11-09 | Verificado: 1200 execuções sem nenhuma falha dos dois testes da área. A primeira versão do teste novo dependia de a entrada inteira caber nos 5 s da chamada e falhava no container sob carga; reescrito (a pausa em `topics` e a nova tentativa) e rodado 300 vezes sem falha. A única falha dessas 300 foi outra: [um nó excluído enquanto sincroniza que não consegue voltar](#ki-023-um-nó-excluído-enquanto-sincroniza-que-não-consegue-voltar). |

## KI-006: Um accept de canal que travou os peers duas vezes

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

## KI-005: Teste de reinício da FIFO que espera 30 s numa execução combinada

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

## KI-004: Teste de aviso que espera 30 s uma vez na suíte completa

**Status:** Resolvido · **Achado em:** 2026-10-20 · **Solução em:** 2026-11-08

- **Teste:** `notify.rs` › `a_notice_is_a_hint_that_ends_fetched_or_empty` (e, sob carga, os outros dois testes do arquivo).
- **Sintoma:** pânico `waited 30 s` do `eventually_async` (`tests/common/cluster.rs`) em uma execução de `cargo test -p taba-node`, logo depois da compilação. Sozinho passou 3 vezes seguidas, com e sem a mudança em teste, e uma segunda execução completa ficou verde.
- **Causa:** o teste fabrica o estado de um nó (um sinal `Ready` do nó 1) e espera o `GET` do nó 2 pedir um `take` ao nó 1. Mas o heartbeat de um par é o estado **completo** dele (`apply_heartbeat`): o heartbeat real do nó 1, sem mensagens prontas, apagou a entrada injetada, `ready_peers` voltou vazio, nenhum `take` foi feito e o aviso "vazio" nunca foi contado. Sob carga (a suíte completa, um runner lento) o heartbeat caiu entre a injeção e o `take`. Os outros dois testes do arquivo tinham pouca margem de tempo: no segundo o aviso passou ao próximo ouvinte (`sent` 2 em vez de 1) quando a espera de 700 ms foi estourada por um `sleep(250)` sem CPU, e no primeiro a janela de 1500 ms fechou antes de as 24 publicações terminarem. O servidor estava certo nos três.
- **Correção:** só nos testes. O primeiro injeta a dica velha de novo até o aviso ser contado (continua sendo a mesma dica velha); o segundo espera 2500 ms por um ouvinte; o terceiro tem uma janela de 4000 ms e espera um aviso com mais de uma mensagem em vez de ler só a última quantidade.
- **Como foi achado:** `docker/hang-hunt.sh notify` com os três testes juntos (12 processos ao mesmo tempo, `KEEP_GOING=1`) reproduziu: 20 de 30 rodadas falharam (o teste sozinho não falhou em 320 execuções). Linhas temporárias `HUNT` em `receive`, `ready_peers` e `take_remote` mostraram `ready-received … has_ready=true` e, 4 ms depois, `ready_peers raw n=0`.
- **Verificação:** depois da correção, 480 execuções (40 rodadas de 12 processos) sem problema.

**Histórico**

| Data | Evento |
|---|---|
| 2026-10-20 | Falhou uma vez numa execução completa local; passou sozinho (3 execuções) e na execução completa seguinte. |
| 2026-10-22 | Falhou de novo numa execução completa local (`waited 30 s for: the old notice counted as empty`); passou 6 de 6 sozinho logo depois. A mudança em teste não toca os avisos. Causa ainda não identificada. |
| 2026-11-08 | Falhou no CI do pull request do provider Pulumi (`Server (build and tests, amd64)`). Reproduzido com a caça (20 de 30 rodadas), causa achada no log e corrigida nos testes; 480 execuções sem problema. |

## KI-003: Reingresso depois de uma partição simétrica

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

## KI-002: Canal saudável que não volta

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

## KI-001: Inversão de ordem de travas ao escolher réplicas

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
- **Um teste que fabrica o estado de um nó compete com o nó real.** O heartbeat de um par é o estado completo dele e sobrescreve o que o teste injetou; injete de novo até ver o efeito, ou espere o estado real. Uma margem de tempo de poucas centenas de milissegundos não se sustenta num runner carregado: a caça com todos os testes do binário ao mesmo tempo acha (um teste sozinho não falhou em 320 execuções).
- **Veja qual alvo falhou antes de ler o servidor.** A contagem do gateway foi lida no código do nó por um tempo; o job que falhou era o dos emuladores.
- **Um erro do cliente se prova no fio.** Uma captura no namespace de rede do servidor diz quem fecha e se a resposta está bem formada; depois, um invólucro da conexão do cliente e o `httptrace` dizem por quê. A captura tem de começar depois do último restart do container, ou ela vê um namespace antigo.
- **Uma correção não está pronta sem um teste que falha sem ela** (aqui, um que falha por timeout e não travando) e sem repetir a execução que travava.
- **Olhe o que aconteceu antes de teorizar sobre o que poderia.** O item pendente culpava um register expirado; o dump da mensagem que falhou e uma linha de log em cada register mostraram que nada expirou e que uma verificação e uma reserva eram dois passos. Um relatório de falha deve dizer onde a coisa está agora, em cada nó (`diagnosis` no `tests/chaos.rs`).
- **Uma nova tentativa que continua falhando é um estado a inspecionar, não um tempo a esperar.** A primeira nota aqui propunha que o `drain_topic` tentasse de novo; a causa real teria ficado escondida. Manter o cluster de pé depois da falha e comparar a mesma mensagem em cada nó (`memoryMessages`, a tabela `excluded`) achou a causa em uma rodada.

## Fontes e provas

- Os testes citados e `tests/common/cluster.rs` (`NO_VOTE`, `FAST`, `WAIT`, `TABA_TEST_LOG`).
- `src/cluster/channel/mod.rs` (`accept`, `receive`, `sync_members`, `drop_all`, `allow_new_vote`), `src/cluster/backup/evict.rs`, `src/cluster/sync/mod.rs` e `tests/channel.rs` (UC-CAN-029).
- `src/cluster/replication/definitions.rs` (`offer`, `spread`, `relay`, `coordinate`), `src/cluster/service/mod.rs` (`recover_previous_life`, `finish_join`, `activate`) e `tests/cold_start.rs` (UC-RTN-058); a linha do tempo da oferta perdida está no log guardado da caçada de 2026-10-26.
- O travamento de 2026-10-26 foi reproduzido localmente e num container Linux; a pilha, os logs e as contagens de execuções são os da tabela de histórico.
- As falhas foram vistas nos logs do CI do repositório; as datas de "achado" vêm do histórico do git e dos logs.
