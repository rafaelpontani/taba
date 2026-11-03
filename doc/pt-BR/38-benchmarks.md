---
título: Benchmarks
resumo: O benchmark comparativo local (bench/run.sh): o OpenMessaging Benchmark contra o Taba, Kafka, Redpanda, RabbitMQ e NATS JetStream em Docker numa máquina só, quem é meta (brokers da mesma classe) e quem é referência (logs de streaming), como a comparação é mantida justa, os níveis de durabilidade, os workloads, o relatório e como incluir um sistema. Os resultados da última rodada, com os gráficos.
tipo: guia
status: parcial
verificado-em: 2026-11-03 @ 3735b5f
fontes: [bench/cluster-trend.sh, bench/profile.sh, bench/profile.awk, bench/run.sh, bench/lib.sh, bench/report.mjs, bench/charts.mjs, doc/diagrams/scripts/generate-bench.mjs, bench/Dockerfile.runner, bench/run.test.mjs, bench/report.test.mjs, bench/test.sh, connectors/omb/OMB_VERSION]
---

# Benchmarks

> Leia antes: [21-conectores.md](21-conectores.md#driver-do-openmessaging-benchmark-connectorsomb) (o driver do Taba para o OMB) e [11-durabilidade.md](11-durabilidade.md).

As metas de velocidade e custo do Taba (RNF-02) são medidas por um script que qualquer pessoa roda: `bench/run.sh`. Ele precisa só de Docker (Compose v2) com pelo menos **4 núcleos e 7 GB**, sem conta em nuvem, e compara o Taba com Kafka, Redpanda, RabbitMQ e NATS JetStream usando o [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) (OMB) no commit fixado em `connectors/omb/OMB_VERSION`. **Nenhum resultado está publicado ainda**: a ferramenta está pronta, e os números virão de uma rodada `standard` feita pelo mantenedor, com a máquina descrita.

O resultado de uma máquina **nunca é número absoluto**. Ele compara os sistemas entre si, naquela máquina, naquela rodada. Todo `results.md` diz isso nas primeiras linhas.

## O que a comparação mede

O Taba é um broker de mensagens, não uma plataforma de streaming ([01-visao-geral.md](01-visao-geral.md#o-que-é)): ele confirma, reentrega, manda para dead-letter e expira cada mensagem no broker, e o objetivo é fazer isso com poucos recursos. Por isso os sistemas da rodada têm dois papéis (D195):

| Papel | Sistemas | O que responde |
|---|---|---|
| **Meta** (mesma classe) | RabbitMQ (fila de quórum), NATS JetStream | O Taba faz o mesmo trabalho por mensagem com mais vazão, ou com menos CPU e memória, sob os mesmos limites? A meta de desempenho é medida contra eles |
| **Referência** (log puro) | Kafka, Redpanda | Quão rápido a mesma máquina move bytes quando o broker não faz trabalho por mensagem. Um teto para saber onde está o custo, não uma meta |

Uma distância para o Kafka ou o Redpanda é lida como o preço do trabalho por mensagem (o estado da entrega, o ack de cada mensagem, a reentrega), não como uma falha por si só; uma distância para o RabbitMQ ou o NATS é um problema a resolver.

## Como rodar

```bash
bench/run.sh                                   # perfil quick (cerca de 1 hora), todos os sistemas
bench/run.sh --profile standard                # 3 rodadas de 5 minutos, seis workloads
bench/run.sh --systems taba,kafka --workloads fanout-1kb
bench/run.sh --durability both                 # default e fsync
bench/run.sh --nodes 1,3                       # um nó e um cluster de três (os sistemas com compose-cluster.yml)
bench/run.sh --profile showcase                # Taba, RabbitMQ e NATS, um e três nós, fsync, com a queda de um nó
bench/run.sh --taba-image rafaelpontani/taba:0.1.9   # padrão: constrói a imagem do commit atual
bench/run.sh --dry-run                         # o plano e a estimativa de tempo; não roda nada
bench/run.sh --report-only bench/results/<pasta>
```

`--cpus` e `--memory` sobrescrevem o limite de cada servidor. No Windows roda no Git Bash com o Docker Desktop. `Ctrl+C` remove os contêineres e os volumes.

## A tendência do caminho de publicação: um nó e três nós

`bench/cluster-trend.sh <núcleos por nó> <conexões por nó> <imagem>...` mede **só o Taba**, para comparar duas versões dele (antes e depois de uma mudança), com um nó (`replication.factor=1`) e com três nós (fator 3), os mesmos núcleos por nó, o `wrk` espalhado pelos nós dentro de uma rede do Docker e a configuração de produção (mTLS no cluster, o token raiz do `taba init`). Imprime, por rodada, o total de publicações por segundo, o p50 e o p99 de cada nó e as falhas (`non2xx`), e a CPU de cada nó. `BODY_BYTES`, `ROUNDS`, `SECONDS_PER_ROUND` e `NODES` mudam o tamanho da mensagem, as rodadas, a duração delas e quais tamanhos rodam. Como em todo este documento, o resultado de uma máquina só se compara com outro medido **na mesma execução**.

**Consumo, um nó e três nós.** `MODE=consume bench/cluster-trend.sh ...` mede o lado da leitura do mesmo jeito: por rodada enche uma fila nova durante `FILL_SECONDS` (padrão 20) e depois a drena durante `DRAIN_SECONDS` (padrão 10) com um consumidor por conexão, espalhado pelos nós. Um consumidor leva até 100 mensagens (`GET`) e confirma **cada uma** (`DELETE`), como o driver do Taba para o OMB. Imprime o backlog acumulado, as mensagens confirmadas e as confirmações por segundo; quando a drenagem esvaziou a fila, avisa, porque a taxa então fica diluída (aumente `FILL_SECONDS`). Essa comparação de um nó contra três é **o Taba contra ele mesmo**; a comparação com os outros sistemas é o perfil `consume` de um nó, abaixo, a que vale.

**Perfil de CPU de uma rodada.** `bench/profile.sh` roda ao lado do `bench/run.sh`: espera o executor do OMB, grava em `<saída>/monitor.log` (com o horário) as taxas que o OMB imprime, ao vivo, e a CPU e a memória do servidor, e quando o backlog chega a `--backlog` mil mensagens (ou depois de `--after` segundos) tira `--repeat` perfis de `--duration` segundos com `perf record -g` num contêiner privilegiado no espaço de processos do servidor. Cada perfil vai para `<saída>/perf-N/` (`children.txt`, `self.txt`, `threads.txt`, `tree.txt`) com, no Taba, a latência média e as chamadas por segundo de cada rota no mesmo intervalo (`routes.txt`, de `http.server.request.duration`), seguido de um perfil de espera de `--offcpu` segundos (`offcpu.txt`: o tempo que as threads do servidor passaram fora da CPU, por cadeia de chamadas; uma espera em futex da musl estática para no `syscall`); `--server-grep` copia para o mesmo log as linhas do log do servidor que casam (uma medição temporária, um pânico). Os resumos encurtam os nomes Rust com `bench/profile.awk`. As cadeias de chamadas só ficam legíveis com uma imagem compilada com frame pointers (`-C force-frame-pointers=yes`).

## Como a comparação é mantida justa

- **Um sistema por vez.** O script sobe o servidor, espera ficar saudável, roda os workloads, remove o servidor **e o volume dele**, e só então sobe o próximo.
- **Os mesmos limites.** Todo servidor recebe o mesmo `cpus` e `mem_limit` (metade dos núcleos do Docker, no máximo 4, e metade da memória, no máximo 8 GB). O OMB roda em outro contêiner com o resto, e o heap do JVM dele acompanha o limite.
- **Um nó ou três (`--nodes 1,3`, D197).** Com três nós cada nó recebe metade do que um nó sozinho recebe (2 núcleos e 1,5 GB numa máquina de 12 núcleos e 7 GB), igual para todos os sistemas, a partir do `compose-cluster.yml` (serviços `node-0` a `node-2`): o Taba com `replication.factor=3`, TLS mútuo e o token raiz, atrás de um HAProxy de 1 núcleo contado nos seus recursos, como no Compose de produção; o RabbitMQ com uma fila de quórum que tem um membro em cada nó; o NATS JetStream com fluxos de 3 réplicas. Os clientes do RabbitMQ e do NATS conectam direto nos nós. Os três nós dividem os núcleos e o disco de uma máquina só: os números de cluster comparam os sistemas entre si, não com um cluster de verdade. Um sistema sem `compose-cluster.yml` (Kafka, Redpanda) fica com um nó.
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
| `backlog-100b` | o mesmo com 100 bytes | o custo fixo de ler e confirmar uma mensagem |
| `backlog-1kb-4c` | o mesmo, 4 partições e 4 consumidores | consumo por vários consumidores da mesma fila |

Um workload com um arquivo `<workload>.faults` (linhas `<segundos> <kill|start> <serviço>`, contados do início da fase medida) só roda com três nós: o `failover-1kb` publica 5 000 mensagens de 1 KB por segundo, mata o `node-1` aos 60 s e o liga de novo aos 150 s. Os perfis são arquivos `bench/profile-*`: `quick` (1 rodada de 2 minutos, os quatro primeiros workloads), `standard` (3 rodadas de 5 minutos, os seis) e `smoke` (1 minuto de `simple-1kb`, usado só pelo `bench/test.sh`). `showcase` é a comparação com os brokers da mesma classe (Taba, RabbitMQ e NATS, um e três nós, `fsync`, `simple-1kb`, `max-rate-1kb`, `keyed-1kb`, `backlog-1kb` e `failover-1kb`). `consume` é o perfil do consumo: 3 rodadas de 3 minutos dos três workloads `backlog-*`, `bench/run.sh --profile consume`. O minuto de aquecimento não entra nos percentis.

Num workload `backlog-*` o relatório mostra a **vazão de drenagem do backlog** (a média do consumo depois do pico do backlog, quando o consumidor é liberado, enquanto ainda resta backlog) no lugar da latência ponta a ponta, que ali é só o tempo na fila. O OMB mantém o produtor na taxa configurada durante a drenagem.

## O relatório

`bench/results/<data>-<commit>/` (ignorada pelo git) guarda `raw/*.json` (o resultado do OMB de cada rodada, com o `.log`, a CPU e a memória dos contêineres do servidor amostradas a cada poucos segundos em `.stats` e o início da fase medida em `.t0`), `machine.json`, `startup.csv` (segundos até ficar saudável e memória parada), `results.md`, `summary.json`, `charts/*.svg` e, se uma célula falhou, `failed.txt`. Os gráficos (`bench/charts.mjs`) seguem a cor de cada sistema, com tema claro e escuro: barras de vazão e latência e, no tempo, as mensagens publicadas e consumidas por segundo, as mensagens processadas até ali, a CPU e a memória, com a queda e a volta de um nó marcadas. `node doc/diagrams/scripts/generate-bench.mjs --from <pasta>` copia o `summary.json` de uma rodada para `doc/diagrams/data/bench-results.json` e desenha os gráficos da documentação em todos os idiomas. O `results.md` mostra, por workload e durabilidade, uma tabela com a **mediana das rodadas e, entre parênteses, o mínimo e o máximo** da vazão publicada e consumida e da latência ponta a ponta p50, p99 e p99.9. Não aponta vencedor; termina com as diferenças conhecidas de cada sistema.

## Resultados

Rodada de 2026-11-03, commit `3735b5f`, perfil `showcase` com os workloads `simple-1kb` e `max-rate-1kb` (uma rodada de 3 minutos depois de 1 minuto de aquecimento), durabilidade fsync, num notebook (Intel i7-10750H, Docker Desktop com 12 núcleos e 7 GB): um nó com 4 núcleos e 3 GB, três nós com 2 núcleos e 1,5 GB cada. Os dados são `doc/diagrams/data/bench-results.json` (o `summary.json` da rodada) e os gráficos saem deles com `node doc/diagrams/scripts/generate-bench.mjs`. Uma rodada local compara os sistemas entre si; não é número absoluto.

| Célula | Taba | RabbitMQ (fila de quórum) |
|---|---|---|
| Taxa máxima, um nó | 26.280 msg/s, p50 89 ms, 11.120 msg/s por núcleo | 15.752 msg/s, p50 1,5 s, 6.601 msg/s por núcleo |
| Taxa máxima, três nós | 9.344 msg/s, p50 271 ms | 6.835 msg/s, p50 2,3 s |
| 10.000 msg/s, um nó | sustentada, p50 3 ms, p99 25 ms | sustentada, p50 515 ms, p99 1 s |
| 10.000 msg/s, três nós | sustentada, p50 79 ms, p99 999 ms | 6.612 msg/s (não acompanha), p50 1,1 s |
| Memória parada, um nó / três nós | 20 MB / 101 MB | 171 MB / 506 MB |

**O NATS JetStream não tem medida válida nesta rodada.** Com `sync_interval: always` ele publicou cerca de 1 mensagem por segundo na taxa máxima (o controlador de taxa do OMB baixou a taxa a zero depois do primeiro acúmulo, o efeito descrito no [PRB-087](31-pendencias.md)) e a célula `simple-1kb` de três nós falhou. Ele fica fora dos gráficos com `generate-bench.mjs --without nats`, que grava a escolha no arquivo de dados (`excluded`) e a diz na nota de cada gráfico. A meta da [D195](30-decisoes.md), o dobro da vazão dos brokers da mesma classe, ainda não foi atingida: 1,7 vez o RabbitMQ em um nó e 1,4 vez em três.

![Vazão máxima](../diagrams/img/pt-BR/bench-throughput.svg)

![Latência ponta a ponta a 10.000 msg/s](../diagrams/img/pt-BR/bench-latency.svg)

![Mensagens por núcleo de CPU](../diagrams/img/pt-BR/bench-efficiency.svg)

![Memória do servidor](../diagrams/img/pt-BR/bench-memory.svg)

![Mensagens por segundo ao longo do tempo, um nó](../diagrams/img/pt-BR/bench-timeline-1.svg)

![Mensagens por segundo ao longo do tempo, três nós](../diagrams/img/pt-BR/bench-timeline-3.svg)

![Mensagens processadas ao longo do tempo, um nó](../diagrams/img/pt-BR/bench-processed-1.svg)

![Mensagens processadas ao longo do tempo, três nós](../diagrams/img/pt-BR/bench-processed-3.svg)

## O que a comparação não esconde

- O corpo que o Taba guarda é **8 bytes maior** que o payload do OMB (a hora de publicação viaja nele).
- O Taba usa uma requisição HTTP/1.1 por chamada e lê até 100 mensagens por requisição; os outros usam protocolo binário com lote no cliente.
- O `fanout` do Taba copia a mensagem para uma fila por assinatura; nos outros as assinaturas são grupos de consumidores sobre um só log.
- **O perfil `consume` precisa de memória para o backlog.** O Taba guarda o backlog em memória, cerca de 2,4 KB por mensagem de 1 KB: o backlog de 1 GB do OMB (cerca de 1,07 milhão de mensagens) não cabe num servidor de 3 GB, e o consumo desaba ([PRB-086](31-pendencias.md)). Rode com `--memory` suficiente e trate uma célula que para no limite como um achado, não como um número.
- **A confirmação no consumo não é igual.** RabbitMQ e NATS confirmam cada mensagem; o Taba também confirma cada mensagem, mas o driver do OMB manda as confirmações de uma leitura numa **requisição só** (`POST …/messages/ack`, [D189](30-decisoes.md)) e publica em lotes de até 100 com 1 ms de `linger`, como o produtor do Kafka; Kafka e Redpanda confirmam um offset para um lote (`enable.auto.commit=false`, um commit depois de cada leitura do consumidor, como o driver do OMB faz). Isso é mais barato por mensagem, então a vazão de drenagem do Kafka e do Redpanda não é comparável ponto a ponto com a dos outros. Eles não são forçados a um commit por mensagem porque isso os deixaria lentos artificialmente; o relatório diz isso em todo workload `backlog-*`.

## Incluir um sistema

Um sistema é uma pasta, `bench/systems/<nome>/`, com `compose.yml` (um serviço chamado `server`, imagem fixada, healthcheck, `cpus: ${BENCH_CPUS}` e `mem_limit: ${BENCH_MEMORY}`), `env-default` e `env-fsync`, `driver-default.yaml` e `driver-fsync.yaml` (`driver-<nível>-<workload>.yaml` sobrescreve um workload) e um `README.md`. O `--systems <nome>` o encontra pela pasta, sem mudar o `run.sh`. Qualquer sistema com driver no OMB fixado (Pulsar, Redis, Artemis, RocketMQ e os outros) entra assim; o limite é a máquina que o executa.

## Testes

`node --test bench/run.test.mjs bench/report.test.mjs` não precisa de Docker: confere o plano e as recusas, a forma de cada pasta de sistema e de cada workload, e o relatório sobre os resultados de exemplo de `bench/testdata/`. O `bench/test.sh` roda o perfil `smoke` contra o Taba, de ponta a ponta. O job `bench` do CI roda o `--dry-run` e os testes em Node.js; benchmark não roda nos runners do GitHub, que não dão número útil.

## Fontes e provas

- `bench/run.sh`, `bench/lib.sh` (o plano, os limites, o rodízio, as células), `bench/Dockerfile.runner` (o OMB com todos os drivers e o driver do Taba), `bench/systems/`, `bench/workloads/`, `bench/profile-*`, `bench/report.mjs`.
- Casos de uso `UC-BNC-001` a `UC-BNC-005` em [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md); testes em `bench/run.test.mjs`, `bench/report.test.mjs` e `bench/test.sh`.
- Decisões [D185](30-decisoes.md) e [D195](30-decisoes.md) (meta e referência).
