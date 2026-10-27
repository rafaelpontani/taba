---
título: Garantias e como são verificadas
resumo: As garantias que o Taba declara, escritas de forma que uma máquina consiga checar (I1 a I6 e o risco assumido R1), a rodada de caos que as verifica contra um histórico gravado, quais falhas ela aplica, o que ainda não cobre e como rodar ou varrer sementes.
tipo: referência
status: parcial
verificado-em: 2026-10-27 @ ada9ff1
fontes: [crates/taba-node/tests/chaos.rs, crates/taba-node/tests/common/history.rs, crates/taba-node/src/cluster/transport/memory/mod.rs, crates/taba-node/tests/common/cluster.rs, .github/workflows/chaos.yml]
---

# Garantias e como são verificadas

> Leia antes: [11-durabilidade.md](11-durabilidade.md) e [12-entrega.md](12-entrega.md).

O [11-durabilidade.md](11-durabilidade.md) diz em prosa o que sobrevive a cada falha. Um teste só prova algo quando compara o resultado com uma lista de promessas que uma máquina consegue checar. Este documento é essa lista, e diz quais delas uma **rodada de caos** já verifica, com quais falhas, e o que ainda fica de fora.

## As garantias

Valem para o cluster padrão (3 nós, `replication.factor=3`, `write.quorum=2`) com **no máximo `write.quorum − 1` nós prejudicados ao mesmo tempo**.

| Código | Garantia | Verificada pela rodada de caos |
|---|---|:-:|
| **I1** Sem perda | Toda publicação que recebeu `201` é entregue pelo menos uma vez, salvo se expirar (`ttl`), for confirmada ou for para a dead-letter | ✅ |
| **I2** Sem fantasma | Nada é entregue sem que uma tentativa de publicação o explique, e o ID entregue é o que o `201` devolveu | ✅ |
| **I3** Ack definitivo | Uma mensagem cujo ack respondeu `204` não é entregue a uma requisição que começou depois do ack | ✅ |
| **I4** FIFO | Na fila FIFO as primeiras entregas de um grupo seguem a ordem de publicação, e uma publicação é guardada uma só vez | ✅ |
| **I5** Dead-letter | Uma mensagem só vai para a dead-letter depois de `maxDeliveries` entregas, nunca está nos dois lugares e não é entregue mais que `maxDeliveries` vezes | ✅ |
| **I6** Perda declarada | Toda perda aparece em `/cluster/message-loss`, nunca em silêncio: uma mensagem confirmada que não foi entregue precisa ter sido declarada | ✅ |
| **R1** Risco assumido | Uma partição com nós suficientes nos dois lados deixa cada lado excluir o outro e seguir ([07-cluster.md](07-cluster.md), "Risco aceito"): pode haver escritas nos dois lados, duplicadas ou perdidas na junção | ✅ medido por `tests/partition.rs`: partição 2 + 2 de 4 nós, escritas nas duas metades, o reencontro; toda escrita respondida `201` é entregue uma vez (D181). O padrão de `cluster.vote.min-voters` segue em aberto (D182) |

Duas coisas que as garantias não dizem: a entrega é **ao-menos-uma-vez**, então uma mensagem entregue sem ack volta e um ack perdido repete a entrega (isso não viola o I3: o ack não respondeu `204`); e, depois da parada de todo o cluster, a **reentrega residual** do [07-cluster.md](07-cluster.md) (seção 10.10) é a exceção documentada ao I3.

## Verificando um histórico

Cada cliente grava o que fez, com um instante lógico para o começo e para o fim de cada operação. Uma operação termina de um de três jeitos:

| Resultado | Quando | O que o verificador exige |
|---|---|---|
| `Ok` | `2xx` | o I1 exige a entrega de uma mensagem publicada; o I3 proíbe nova entrega depois de um ack |
| `Fail` | um `4xx` definitivo | nada |
| `Indeterminate` | timeout, conexão fechada, `5xx`, `408`, `429` | nada é exigido (pode ter acontecido), mas tudo o que for entregue é explicado por ela (I2) |

Um cliente nunca sabe mais do que a API respondeu, então o verificador só exige o que foi confirmado e só proíbe o que nenhuma tentativa explica. O histórico fica em `tests/common/history.rs`; `History::check` devolve uma linha por violação, começando pelo código da garantia. O verificador tem testes próprios que montam um histórico ruim para cada garantia e exigem a violação: um verificador que não consegue falhar não prova nada.

## A rodada de caos

`tests/chaos.rs` sobe três nós (`NO_VOTE`: sem votação de exclusão, para a tabela ficar estável e o I1 poder ser exigido), cria uma fila comum e uma fila FIFO com três grupos, e roda ao mesmo tempo:

- 2 publicadores na fila comum e 3 na FIFO (um por grupo, uma mensagem de cada vez), cada um passando pelos nós em rodízio;
- uma terceira fila com `maxDeliveries: 2` e a sua dead-letter: um publicador, e um consumidor que nunca dá ack nas mensagens veneno (`seq % 3 == 0`), que por isso precisam terminar na dead-letter;
- 6 consumidores (2 na fila comum, 2 na FIFO, 1 na fila com dead-letter e 1 na própria dead-letter) que pegam até 5 mensagens com 5 s de invisibilidade, gravam a entrega e dão o ack;
- o **nemesis**, que a cada 100 a 300 ms sorteia uma falha a partir da semente. Cada falha é aplicada sozinha (a anterior é curada antes):

| Falha | Na `MemoryNetwork` |
|---|---|
| Isolar um nó | `isolate` |
| Cortar um par, nos dois sentidos | `partition` |
| Cortar só um sentido | `cut_one_way` |
| A chamada roda, a resposta nunca volta | `lose_replies` |
| Perder 40% de um sentido | `set_link_loss` (sorteio a partir da semente) |
| 15 ms em toda chamada | `set_delay` |
| Um nó que aceita e nunca responde | `set_silent` |
| O relógio salta 8 s (a invisibilidade de 5 s acaba, os prazos andam) | `ManualClock::advance` |

No fim os publicadores param, toda falha é curada (`heal_all`), o relógio avança até acabar a invisibilidade do que foi entregue durante uma falha, e os consumidores esvaziam as filas. Aí o histórico é conferido com o que cada nó declarou perdido.

**A mesma semente sorteia as mesmas falhas na mesma ordem.** O entrelaçamento com os clientes anda em tempo real e não se repete bit a bit, então uma semente que falhou pode precisar de algumas rodadas para falhar de novo; a mensagem de falha imprime a semente, as contagens e a lista de falhas.

### O que ainda não cobre

| Fora da rodada | Por quê |
|---|---|
| Morte abrupta e volta de um nó, drain durante a rodada | ainda não estão no nemesis |
| Disco lento ou com erro | o armazenamento ainda não tem gancho para isso |
| TCP real, TLS mútuo, timeouts do kernel, disco real | a rede em memória não os reproduz (previsto: rodada em Docker) |
| Uma parada maior que o `write.pending.ttl` (30 s) no meio de uma operação, ou um salto de relógio além dele | os saltos da rodada (8 s) ficam abaixo dele; com saltos de 35 s a rodada acha uma mensagem confirmada e depois entregue de novo pela dead-letter ([PRB-077](31-pendencias.md)) |
| Dois nós prejudicados ao mesmo tempo, ou partição com maioria nos dois lados (R1) | fora do que o cluster padrão promete, e o `NO_VOTE` esconde a votação de exclusão; o R1 tem teste próprio (`tests/partition.rs`, veja a linha dele) |

## Como rodar

```bash
cargo test --test chaos                      # as sementes fixas (1 a 4) e os testes do verificador
TABA_CHAOS_SEEDS=1..200 cargo test --test chaos a_range_of_seeds -- --nocapture
```

A segunda forma é a que o `.github/workflows/chaos.yml` roda toda noite (fora dos checks obrigatórios). Uma semente que falha vira um teste próprio em `tests/chaos.rs`, e o bug que ela achou segue o caminho de sempre: um `fix:` com o teste, uma entrada em [36-problemas-conhecidos.md](36-problemas-conhecidos.md) e o candidato validado com `docker/validate.sh`.

## O que a rodada achou

Três bugs do servidor, corrigidos na mesma mudança da rodada ([D180](30-decisoes.md), [36-problemas-conhecidos.md](36-problemas-conhecidos.md#a-rodada-de-caos-uma-entrega-além-do-limite-e-um-ack-que-o-dono-nunca-aplicou)):

- uma mensagem entregue **uma vez além do `maxDeliveries`** (I5) quando o commit da entrega anterior chegava ao dono depois de a invisibilidade acabar;
- uma mensagem que ficava **só no dono**, travando o grupo FIFO para sempre (I1), quando um ack que as réplicas tinham aplicado era recusado pelo dono depois de o coordenador registrá-lo de novo.
- as escritas da metade de uma partição 2 + 2 que perde o reencontro, **apagadas** quando ela entra de novo na outra metade (R1, achado pela medição de `tests/partition.rs`, D181).

## Fontes e provas

- `crates/taba-node/tests/chaos.rs` (a rodada, o nemesis, os testes do verificador), `tests/common/history.rs` (o histórico, `History::check`, o gerador com semente) e `src/cluster/transport/memory/mod.rs` (`cut_one_way`, `lose_replies`, `set_link_loss`, `seed_links`, `heal_all`).
- Casos de uso: `UC-CHS-001` a `UC-CHS-005` em [26a](26a-casos-de-uso-implementados.md).
- Medido ao escrever: as sementes 1 a 24 rodaram sem violação, com 48 a 578 publicações confirmadas e até 13 reentregas por rodada.
