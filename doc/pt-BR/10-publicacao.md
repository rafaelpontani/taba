---
título: Publicação de mensagens
resumo: O caminho de uma mensagem publicada — do nó que recebe a chamada até a resposta ao cliente — com registro, quórum, commit, rollback e as falhas possíveis em cada ponto.
tipo: explicação
status: estável
verificado-em: 2026-10-28 @ 2447d5e
fontes: [crates/taba-node/src/cluster/replication/coordinator.rs, crates/taba-node/src/cluster/replication/replica.rs, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/http/messages/mod.rs, crates/taba-node/src/engine/messages/mod.rs]
---

# Publicação de mensagens

> Leia antes: [06-anel-e-replicacao.md](06-anel-e-replicacao.md). Leia depois: [11-durabilidade.md](11-durabilidade.md) (o que isso garante) e [12-entrega.md](12-entrega.md).

## Resumo

Um cliente publica em **qualquer nó**. Esse nó vira o **coordenador** da requisição, escolhe pelo `id` da mensagem os nós **responsáveis** (primário e réplicas) e executa um protocolo de **duas fases**:

1. **Registro** em **todos** os responsáveis: cada um guarda a mensagem como **pendente**, em memória.
2. **Commit**, decidido assim que um **quórum** confirmou o registro (sempre com o **primário**).
3. **Resposta** ao cliente, com o `id`, quando o **commit está confirmado no quórum**. As réplicas fora do quórum terminam em segundo plano.

![Quem participa de uma escrita](../diagrams/img/pt-BR/write-architecture.svg)

![Registro e commit passo a passo](../diagrams/img/pt-BR/write-reception.svg)

## Passo a passo

1. **Receber.** O nó confere token e permissão (`publish`), rate limit, tamanho do corpo (`message.max-bytes`, padrão 5 MB, ou o `maxMessageBytes` da fila) e, na FIFO, `groupId`.
2. **Gerar o `id`.** O coordenador gera o GUID v7 **antes** do registro; ele decide o bloco. Na FIFO quem gera é o **primário do grupo** (ver [09-topicos.md](09-topicos.md)).
3. **Achar os responsáveis.** Pelo hash do `id` no anel: primário, secundário, terciário… (`replication.factor` nós, padrão 3). Qualquer nó chega ao mesmo resultado.
4. **Fase 1 — registro.** O coordenador manda a **todos** os responsáveis, ao mesmo tempo, cada um com repetições (`write.retry.attempts`, padrão 3, com espera `write.retry.backoff`, padrão 200 ms). Se o próprio coordenador é um responsável, é chamado **sem rede**. Cada nó grava a mensagem em memória como **pendente** e confirma.
5. **Decisão do commit.** Quando o **quórum** confirmou — o **primário mais `write.quorum − 1` quaisquer outros** (padrão `floor(replication.factor / 2) + 1`: 2 de 3) —, o coordenador **decide commitar** e chama o commit nos nós do quórum. As demais réplicas seguem em paralelo, sem bloquear.
6. **Resposta.** Com o commit **confirmado no quórum**, o cliente recebe `201` com `{"id": "…"}`.
7. **Efeito do commit no primário:** a mensagem passa a ser **entregável**: entra na lista de prontos do **dono da entrega** (ou direto no ouvinte de um long polling) e dispara os avisos para outros nós ([12-entrega.md](12-entrega.md)).

### Por que o primário é sempre parte do quórum

Só o primário decide o que é entregável. Um quórum formado por secundário e terciário poderia dar a escrita por durável **antes** de ela estar visível a qualquer consumidor, e a mesma verdade precisa valer para escrita, entrega (invisibilidade) e ack. Por isso `write.quorum` é **um valor único** para as três operações, validado entre 1 e `replication.factor`.

## Falhas

| Quando | O que acontece | O cliente vê |
|---|---|---|
| **Antes do quórum** (um responsável não responde) | Repete a chamada ao nó que falhou (`write.retry.*`) | Nada, se o quórum fechar |
| **Esgotadas as repetições sem quórum** | **Rollback**: o coordenador manda descartar o registro pendente em todos os que já tinham confirmado. Um registro sem commit também **expira sozinho** em `write.pending.ttl` (padrão 30 s); o rollback só acelera a liberação | `503 cluster.quorum-unavailable` com `Retry-After` |
| **Primário fora do ar**, ainda na tabela | A escrita do bloco falha: o secundário **não** assume a coordenação | `503 cluster.quorum-unavailable` (`fifo.primary-unavailable` na FIFO). Repita a chamada; depois da exclusão do primário o herdeiro assume |
| **Depois do quórum decidido** (réplica fora do quórum falha) | **Nunca há rollback**: o quórum pode já ter efetivado a mensagem. O coordenador **insiste** (registro + commit, idempotentes) em segundo plano | Nada: a resposta já saiu |
| **Teto esgotado** (`write.commit.timeout`, padrão 10 s) numa réplica fora do quórum | O bloco fica **abaixo do fator de replicação pleno** (nunca abaixo do quórum); gera a condição `under-replicated` e uma métrica; o coordenador continua tentando | Nada |
| **Commit decidido mas não confirmado no quórum no prazo** | A decisão não se desfaz; o cliente é avisado | `cluster.commit-timeout` com `Retry-After` |

> **Repetir uma publicação que deu erro pode duplicar a mensagem**, porque o `id` é gerado de novo e a primeira tentativa pode ter chegado a efetivar. A fila **FIFO** é a exceção: o primário deduplica ([09-topicos.md](09-topicos.md)).

### Rollback e insistência: a regra

| Fase da decisão | Pode abortar? |
|---|---|
| **Antes** de o quórum fechar | **Sim**: ninguém depende ainda do que foi registrado |
| **Depois** de o commit ser decidido | **Não**: só insistir até confirmar, dentro do teto |

É a mesma regra do protocolo de cluster ([07-cluster.md](07-cluster.md)). O que difere é **quem decide**: o quórum para mensagens, todos os nós para mudanças de membros.

## Tópico distribuidor e FIFO

- **Distribuidor:** o coordenador casa os prefixos e executa **este mesmo procedimento uma vez por fila destino**, com cópias de `id` próprio. As cópias são independentes.
- **FIFO:** o nó que recebeu **repassa ao primário do grupo**, que gera o `id` monotônico, deduplica e coordena; o quórum inclui **primário e secundário**.

## Num nó só

No perfil de um nó (fator 1) o nó é o primário de tudo e o quórum vale 1: registro e commit acontecem só nele, sem réplicas, sem repetição entre nós e sem rollback por falta de quórum.

Com um único nó responsável (fator 1, em qualquer tamanho de cluster) o registro dele já é a decisão: o coordenador registra e faz o commit em sequência na própria tarefa da requisição, sem uma tarefa por nó e sem esperar o quórum; só um commit que falha na primeira tentativa vai para a insistência que o coordenador mantém em segundo plano depois da decisão (veja Falhas). Uma publicação que só mexe em memória (o tópico ainda não tem banco no disco, o limite de memória tem espaço, o nó não está em drenagem e o backup em S3 está desligado) é conferida e aplicada pela réplica na mesma thread; senão as duas vão para uma thread bloqueante, porque leem ou escrevem o disco.

## Exemplo

```bash
curl -s -i -X POST localhost:8080/topics/pedidos/messages \
  -H 'Content-Type: application/json' -d '{"pedido":9001}'
# HTTP/1.1 201 Created
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}
```

O `id` é um GUID v7: carrega o instante da publicação (de onde vem a idade da mensagem para o `ttl`) e é ordenável no tempo.

## Parâmetros

| Chave | Padrão | Efeito |
|---|---|---|
| `replication.factor` | 3 | Quantos nós guardam cada mensagem |
| `write.quorum` | `floor(fator/2)+1` | Confirmações para decidir o commit (primário incluso) |
| `write.retry.attempts` / `write.retry.backoff` | 3 / 200 ms | Repetições de cada chamada antes do quórum |
| `write.pending.ttl` | 30 s | Vida de um registro pendente sem commit |
| `write.commit.timeout` | 10 s | Teto da insistência nas réplicas fora do quórum |
| `message.max-bytes` | 5 MB | Corpo máximo (cada fila pode ter o seu `maxMessageBytes`) |

Todos em [22-configuracao.md](22-configuracao.md).

## Fontes e provas

- `cluster/replication/coordinator.rs` (`run`, `Plan`, `Decision`), `replica.rs` (o lado que recebe registro, commit e rollback), `messages_api.rs` (a API de mensagens no cluster), `http/messages`, `engine/messages`.
- Casos de uso: UC-ESC (escrita: registro + commit, quórum, rollback, teto de commit), UC-MSG (publicação), UC-DEV; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/replication.rs`, `failures.rs`, `messages.rs`.
- Histórico: `02-arquitetura.md` (3.1), `05-particionamento-replicacao.md` (7.2).
