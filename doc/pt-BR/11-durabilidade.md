---
título: Durabilidade
resumo: O que o Taba garante sobre mensagens confirmadas, em quais falhas elas sobrevivem, em quais se perdem, e como ajustar o equilíbrio entre latência e garantia.
tipo: explicação
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src/cluster/replication, crates/taba-node/src/cluster/recovery, crates/taba-node/src/cluster/sync, crates/taba-node/src/cluster/drain, crates/taba-node/src/storage, crates/taba-node/src/engine/messages]
---

# Durabilidade

> Leia antes: [10-publicacao.md](10-publicacao.md) e [08-armazenamento.md](08-armazenamento.md).

Este documento responde: **"se eu recebi o `201` (ou o `204` do ack), em que situações a mensagem não se perde?"** A resposta curta: ela sobrevive à falha de **qualquer nó** (e a mais de um, conforme o fator de replicação), mas **não** a uma parada simultânea de todos os nós **em produção**, porque a durabilidade vem da **replicação em memória**, não do disco. **Com o backup opcional em S3** ([33-backup-s3.md](33-backup-s3.md)) o cluster restaura o que estava no bucket, com a defasagem (RPO) de `backup.delay` para as mensagens e de `backup.meta.interval` para o estado.

## O modelo

| Pergunta | Resposta |
|---|---|
| Onde fica a mensagem confirmada? | Em memória (e no disco, se houve overflow) de pelo menos **`write.quorum`** nós responsáveis pelo bloco, o **primário incluso** |
| Quando o cliente recebe a confirmação? | Quando o **commit está confirmado no quórum** |
| O disco garante a mensagem? | **Não por si.** O disco só entra por overflow (memória cheia) e por drain. A durabilidade vem das **cópias** |
| Quantas cópias existem no fim? | `replication.factor` (padrão 3); as fora do quórum completam em segundo plano |
| O que o usuário ajusta? | `replication.factor` e `write.quorum`: mais cópias confirmadas, mais latência |

Isso é o princípio 3 de [01-visao-geral.md](01-visao-geral.md): **durabilidade configurável**.

## Cenários

Considere o padrão: 3 nós, `replication.factor=3`, `write.quorum=2`.

| Cenário | A mensagem confirmada sobrevive? | Por quê |
|---|---|---|
| Um nó cai (processo ou máquina) | **Sim** | Há ≥ 2 cópias confirmadas; sobra pelo menos 1 depois da queda. O cluster exclui o nó por votação e as cópias restantes seguem servindo |
| Um nó cai **logo depois** de confirmar, antes de a 3ª réplica completar | **Sim** | O quórum (2) já tinha a mensagem; a 2ª cópia sobrevive |
| Dois nós caem **ao mesmo tempo** | **Pode se perder** | Se os dois eram os do quórum, só a 3ª cópia (se completou) resta. Mensagens cuja 3ª réplica ainda estava pendente podem se perder |
| Um nó é **reiniciado** (processo reinicia) | **Sim**, as cópias dos **outros** nós | O nó volta como **nó novo** e sincroniza. O disco da vida anterior ainda devolve ao cluster o que só ele tinha |
| Um nó sai por **drain** | **Sim** | O nó grava tudo em disco e se desregistra; as cópias dele já existiam nos outros nós |
| Os três nós são reiniciados **um por vez** (`docker compose restart`) | **Sim** | Verificado: com 14 mensagens no tópico, reiniciar `taba-2`, `taba-3` e `taba-1` em sequência (cada um até ficar `healthy`) e consumir tudo devolveu **14 de 14** |
| **Todos os nós** param ao mesmo tempo (produção), **com o backup em S3** | **Sim, com defasagem:** definições e tokens voltam; as mensagens voltam **menos** as confirmadas e expiradas (diário) e **mais** as que estavam no bucket. Perde o que ainda esperava no `outbox` (até `backup.delay`) e o estado da última janela. *At-least-once*: o consumidor precisa ser idempotente | Parada **programada** (`POST /cluster/stop`): sem defasagem, o drain manda tudo ao S3. Queda: o último envio. Detalhes e cenários: [33-backup-s3.md](33-backup-s3.md) |
| **Todos os nós** param ao mesmo tempo (produção), **sem backup** | **Não: perde as mensagens e também as definições de tópico** | Em produção todo reinício é de nó novo e o disco antigo só serve para devolver "perdas" listadas pelos outros nós. Com o cluster inteiro parado não há quem liste. Verificado: `docker compose down` + `up` (volumes mantidos) subiu um cluster **sem nenhum tópico**. ⚠️ Limite de projeto |
| Partição de rede | **Depende do lado** | Cada lado pode excluir o outro e seguir ([07-cluster.md](07-cluster.md), "Risco aceito"). Mensagens com cópia em ambos os lados sobrevivem; as escritas no lado minoritário falham por falta de quórum |
| Perfil de um nó (fator 1) | **Só com drain** | Sem réplicas: o drain grava tudo e a próxima partida restaura o disco antigo como nó novo. Queda sem drain perde o que só estava em memória |
| **Primário** de um bloco fora do ar | A mensagem **não é perdida**, mas as escritas do bloco **falham** | O secundário não coordena; quando o primário é excluído, o herdeiro assume |

> **Dois nós ao mesmo tempo** depende também de `replication.factor`: com 5 cópias e quórum 3, sobrevive a duas falhas simultâneas. Como regra: o cluster tolera `write.quorum − 1` perdas simultâneas de nós **sem perder o que foi confirmado**.

**Em resumo** (3 nós, `replication.factor=3`, `write.quorum=2`):

| Nós perdidos ao mesmo tempo | A mensagem confirmada | Por quê |
|:-:|---|---|
| 0 | ✅ sobrevive | Há três cópias, ou ao menos duas confirmadas |
| 1 | ✅ sobrevive | Sobra ao menos uma das cópias do quórum |
| 2 | ⚠️ pode se perder | Se os dois eram os do quórum, só resta a terceira cópia, e só se ela já completou |
| 3 (parada total) | ❌ perde tudo | Em produção todo reinício é de nó novo; ninguém lista as perdas |

## O que *nunca* se perde sem aviso: perdas na sincronização

Quando um nó novo sincroniza um bloco e uma mensagem **listada** não vem de nenhuma origem, o nó **não finge que ela não existia**:

1. Tenta outra origem, a rota por ID único e relista; se o ID sumiu das listas, foi um **ack** (descartado em silêncio).
2. Se continua **listada e sem conteúdo**, vira uma **perda declarada**: log `error` (tópico, ID, bloco, nós consultados) e registro na tabela `losses` dos nós responsáveis.
3. Quando um nó excluído **volta**, consulta essas listas e **devolve** as mensagens que ainda tem, idempotentes pelo ID ([07-cluster.md](07-cluster.md)).

Uma perda **não** compromete a saúde do nó nem para a sincronização.

## Garantias de entrega

- **Pelo menos uma vez** (*at-least-once*): uma mensagem pode ser entregue mais de uma vez (ack que não chega, prazo vencido), mas **não se perde por falta de ack**. Quem consome deve ser **idempotente**.
- **Sem ordem global:** não há ordem por tópico. A **fila FIFO** dá ordem **por grupo** ([09-topicos.md](09-topicos.md)).
- **Sem duplicata de publicação:** só na fila FIFO, dentro da janela de deduplicação. Nas demais, repetir uma publicação que falhou pode duplicar.
- **Uma mensagem fica, no máximo, `ttl`:** 14 dias por padrão; vencida, é descartada (e **não** vai para a dead-letter).

### Trade-off aceito na invisibilidade

Se o primário cair **exatamente** entre o commit local e a replicação às réplicas fora do quórum, e for excluído antes de terminar, o novo primário herda o `invisibleUntil` de quem sobreviveu, que pode estar desatualizado: a mensagem pode voltar a ser entregável **antes** do prazo prometido ao primeiro consumidor. Não quebra o *at-least-once*; só encurta, nesse caso raro, o tempo mínimo de processamento.

## Retenção

| Regra | Padrão | Efeito |
|---|---|---|
| `ttl` | 14 dias (`retention.ttl.default`; faixa 60 s a 365 d por fila) | A mensagem mais velha que o `ttl` é descartada onde estiver, **também** se estiver invisível |
| `maxDeliveries` | 10 (`retention.max-deliveries.default`; faixa 1 a 1000) | Ao atingi-lo sem ack, vai para a dead-letter |
| Dead-letter | `{fila}-dead-letter` | Guarda o que falhou no consumo, também sujeita a `ttl` |

A idade vem **do próprio `id`** (GUID v7 carrega o instante da publicação; na dead-letter, o instante de entrada nela). A conferência é **local e determinística**, sem rodada de registro + commit: cada nó apaga as próprias cópias a cada `retention.sweep.interval` (padrão 60 s).

## Como ajustar

| Quero… | Faça |
|---|---|
| Tolerar mais falhas simultâneas | Aumente `replication.factor` e `write.quorum` (a latência de publicação sobe) |
| Menos latência de publicação | Reduza `write.quorum` (a garantia cai); ele precisa ser ≥ 1 e ≤ `replication.factor` |
| Não perder nada numa parada **programada** de um nó | Deixe `drain.on-stop` ligado (padrão) e `stop_grace_period` maior que `drain.timeout` |
| Sobreviver a uma queda do cluster inteiro | **Não há como hoje:** use mais de um cluster, ou um produtor que repita publicações. ⚠️ ver [32-roadmap.md](32-roadmap.md) |
| Menos memória | Defina `memory.limit`: o excedente vai para o disco |

## Limites conhecidos

- **Parada total** em produção perde o que estava só em memória (acima), **a menos que** o backup em S3 esteja ligado ([33-backup-s3.md](33-backup-s3.md)): então perde só a janela ainda não enviada.
- **PRB-005:** durante o drain o nó recusa o ack; sem outro nó para repassar, o consumidor não confirma.
- **D116 (decidido):** a redundância das mensagens de um nó que sai fica em `replication.factor − 1` até entrar um nó novo.
- **D114:** o que `delivery.get.max-attempts` conta ainda não está decidido.

## Fontes e provas

- `cluster/replication/coordinator.rs` (quórum, rollback, insistência), `cluster/recovery` (perdas e devolução), `cluster/sync` (perda declarada), `cluster/drain`, `storage/*`, `engine/messages` (`ttl`, dead-letter).
- Casos de uso: UC-ESC, UC-FAL, UC-RTN (retorno e recuperação), UC-SIN, UC-DRN, UC-RET (`ttl`, entregas, dead-letter); veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/failures.rs`, `recovery.rs`, `drain_cluster.rs`, `sync.rs`, `replication.rs`; `docker/validate.sh` confere a ausência de perda na queda e na volta de um nó.
- Histórico: `05-particionamento-replicacao.md` (7.2, 7.3), `13-entrega-de-mensagens.md` (12.10).
