---
título: Benchmarks
resumo: O benchmark comparativo local (bench/run.sh): o OpenMessaging Benchmark contra o Taba, Kafka, Redpanda, RabbitMQ e NATS JetStream em Docker numa máquina só, como a comparação é mantida justa, os níveis de durabilidade, os workloads, o relatório e como incluir um sistema. Nenhum número está publicado ainda.
tipo: guia
status: parcial
verificado-em: 2026-10-28 @ 2447d5e
fontes: [bench/cluster-trend.sh, bench/run.sh, bench/lib.sh, bench/report.mjs, bench/Dockerfile.runner, bench/run.test.mjs, bench/report.test.mjs, bench/test.sh, connectors/omb/OMB_VERSION]
---

# Benchmarks

> Leia antes: [21-conectores.md](21-conectores.md#driver-do-openmessaging-benchmark-connectorsomb) (o driver do Taba para o OMB) e [11-durabilidade.md](11-durabilidade.md).

As metas de velocidade e custo do Taba (RNF-02) são medidas por um script que qualquer pessoa roda: `bench/run.sh`. Ele precisa só de Docker (Compose v2) com pelo menos **4 núcleos e 8 GB**, sem conta em nuvem, e compara o Taba com Kafka, Redpanda, RabbitMQ e NATS JetStream usando o [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) no commit fixado em `connectors/omb/OMB_VERSION`. **Nenhum resultado está publicado ainda**: a ferramenta está pronta, e os números virão de uma rodada `standard` feita pelo mantenedor, com a máquina descrita.

O resultado de uma máquina **nunca é número absoluto**. Ele compara os sistemas entre si, naquela máquina, naquela rodada. Todo `results.md` diz isso nas primeiras linhas.

## Como rodar

```bash
bench/run.sh                                   # perfil quick (cerca de 1 hora), todos os sistemas
bench/run.sh --profile standard                # 3 rodadas de 5 minutos, seis workloads
bench/run.sh --systems taba,kafka --workloads fanout-1kb
bench/run.sh --durability both                 # default e fsync
bench/run.sh --taba-image rafaelpontani/taba:0.1.9   # padrão: constrói a imagem do commit atual
bench/run.sh --dry-run                         # o plano e a estimativa de tempo; não roda nada
bench/run.sh --report-only bench/results/<pasta>
```

`--cpus` e `--memory` sobrescrevem o limite de cada servidor. No Windows roda no Git Bash com o Docker Desktop. `Ctrl+C` remove os contêineres e os volumes.

## A tendência do caminho de publicação: um nó e três nós

`bench/cluster-trend.sh <núcleos por nó> <conexões por nó> <imagem>...` mede **só o Taba**, para comparar duas versões dele (antes e depois de uma mudança), com um nó (`replication.factor=1`) e com três nós (fator 3), os mesmos núcleos por nó, o `wrk` espalhado pelos nós dentro de uma rede do Docker e a configuração de produção (mTLS no cluster, o token raiz do `taba init`). Imprime, por rodada, o total de publicações por segundo, o p50 e o p99 de cada nó e as falhas (`non2xx`), e a CPU de cada nó. `BODY_BYTES`, `ROUNDS`, `SECONDS_PER_ROUND` e `NODES` mudam o tamanho da mensagem, as rodadas, a duração delas e quais tamanhos rodam. Como em todo este documento, o resultado de uma máquina só se compara com outro medido **na mesma execução**.

## Como a comparação é mantida justa

- **Um sistema por vez.** O script sobe o servidor, espera ficar saudável, roda os workloads, remove o servidor **e o volume dele**, e só então sobe o próximo.
- **Os mesmos limites.** Todo servidor recebe o mesmo `cpus` e `mem_limit` (metade dos núcleos do Docker, no máximo 4, e metade da memória, no máximo 8 GB). O OMB roda em outro contêiner com o resto, e o heap do JVM dele acompanha o limite.
- **Um nó por sistema.** Três nós numa máquina só competiriam pelos mesmos núcleos e discos e dariam números enganosos; o cluster de três nós é uma medição de nuvem, fora deste script.
- **A ordem gira** entre as rodadas (A B C, depois B C A, depois C A B), para que a deriva térmica e o cache do disco não favoreçam sempre o mesmo sistema.
- **O mesmo executor, o mesmo OMB, os mesmos arquivos de payload.** O Taba é medido pelo driver de `connectors/omb`; Kafka e Redpanda pelo driver Kafka do OMB; RabbitMQ e NATS pelos próprios. Toda imagem tem versão fixa.
- O relatório registra o commit do Taba, o commit do OMB, as imagens, os limites e a máquina (`machine.json`).

## Durabilidade

`--durability default|fsync|both` aplica um nível a todos os sistemas e o anota ao lado de cada resultado.

| Sistema | `default` | `fsync` |
|---|---|---|
| Taba | `storage.db.synchronous=NORMAL`; a durabilidade vem da réplica em memória | `synchronous=FULL` e `memory.limit=64MB`, para o excesso passar pelo disco |
| Kafka | `acks=all`, sem fsync por mensagem | `flush.messages=1`, `flush.ms=0` |
| Redpanda | `acks=all`, write caching do tópico ligado | write caching desligado (fsync antes do ack) |
| RabbitMQ | fila de quórum, mensagem persistente, confirmação do publicador | igual (a fila de quórum já faz fsync por confirmação) |
| NATS JetStream | fluxo em arquivo, `sync_interval` padrão | `sync_interval: always` |

Um sistema que não distingue os níveis mostra os mesmos números nos dois, com uma nota de rodapé dizendo por quê. Com um nó não há réplica: a coluna `default` do Taba **não tem cópia** além do próprio nó, como diz o `replication.factor=1` ([11-durabilidade.md](11-durabilidade.md)).

## Workloads e perfis

| Workload | Forma | Mede |
|---|---|---|
| `simple-1kb` | 1 tópico, 1 produtor, 1 consumidor, 1 KB, 10 000 msg/s | latência ponta a ponta de p50 a p99.9 em carga moderada |
| `max-rate-1kb` | igual, com a taxa descoberta pelo OMB | vazão sustentada |
| `fanout-1kb` | 5 assinaturas, 1 KB, 5 000 msg/s | o custo da cópia por assinatura |
| `keyed-1kb` | 16 chaves (no Taba, `ordering: per-key`), 1 KB, 10 000 msg/s | ordem por chave |
| `small-100b` | 100 bytes, 20 000 msg/s | custo fixo por mensagem (HTTP/1.1 contra protocolo binário) |
| `backlog-1kb` | 1 GB produzido com o consumidor parado e depois liberado | leitura de acúmulo |

Os perfis são arquivos `bench/profile-*`: `quick` (1 rodada de 2 minutos, os quatro primeiros workloads), `standard` (3 rodadas de 5 minutos, os seis) e `smoke` (1 minuto de `simple-1kb`, usado só pelo `bench/test.sh`). O minuto de aquecimento não entra nos percentis.

## O relatório

`bench/results/<data>-<commit>/` (ignorada pelo git) guarda `raw/*.json` (o resultado do OMB de cada rodada, com o `.log`), `machine.json`, `results.md`, `charts/*.svg` e, se uma célula falhou, `failed.txt`. O `results.md` mostra, por workload e durabilidade, uma tabela com a **mediana das rodadas e, entre parênteses, o mínimo e o máximo** da vazão publicada e consumida e da latência ponta a ponta p50, p99 e p99.9. Não aponta vencedor; termina com as diferenças conhecidas de cada sistema.

## O que a comparação não esconde

- O corpo que o Taba guarda é **8 bytes maior** que o payload do OMB (a hora de publicação viaja nele).
- O Taba usa uma requisição HTTP/1.1 por chamada e lê até 100 mensagens por requisição; os outros usam protocolo binário com lote no cliente.
- O `fanout` do Taba copia a mensagem para uma fila por assinatura; nos outros as assinaturas são grupos de consumidores sobre um só log.
- Um nó: o custo da replicação de um cluster não é medido aqui.

## Incluir um sistema

Um sistema é uma pasta, `bench/systems/<nome>/`, com `compose.yml` (um serviço chamado `server`, imagem fixada, healthcheck, `cpus: ${BENCH_CPUS}` e `mem_limit: ${BENCH_MEMORY}`), `env-default` e `env-fsync`, `driver-default.yaml` e `driver-fsync.yaml` (`driver-<nível>-<workload>.yaml` sobrescreve um workload) e um `README.md`. O `--systems <nome>` o encontra pela pasta, sem mudar o `run.sh`. Qualquer sistema com driver no OMB fixado (Pulsar, Redis, Artemis, RocketMQ e os outros) entra assim; o limite é a máquina que o executa.

## Testes

`node --test bench/run.test.mjs bench/report.test.mjs` não precisa de Docker: confere o plano e as recusas, a forma de cada pasta de sistema e de cada workload, e o relatório sobre os resultados de exemplo de `bench/testdata/`. O `bench/test.sh` roda o perfil `smoke` contra o Taba, de ponta a ponta. O job `bench` do CI roda o `--dry-run` e os testes em Node.js; benchmark não roda nos runners do GitHub, que não dão número útil.

## Fontes e provas

- `bench/run.sh`, `bench/lib.sh` (o plano, os limites, o rodízio, as células), `bench/Dockerfile.runner` (o OMB com todos os drivers e o driver do Taba), `bench/systems/`, `bench/workloads/`, `bench/profile-*`, `bench/report.mjs`.
- Casos de uso `UC-BNC-001` a `UC-BNC-004` em [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md); testes em `bench/run.test.mjs`, `bench/report.test.mjs` e `bench/test.sh`.
- Decisão [D185](30-decisoes.md).
