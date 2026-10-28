# Plano: testes de caos e benchmarks reproduzíveis

> **Plano em desenvolvimento.** Este arquivo descreve o que **deve ser feito**, não como o código está. Ele vive
> só na branch de trabalho, é apagado quando o plano for executado e **nunca** entra em `release` ou `main`
> (regra em [AGENTS.md](../../AGENTS.md#development-plans-docplans)). O que for decidido e implementado vai para a
> documentação permanente (`doc/pt-BR/`, `doc/en-US/`), nos dois idiomas, na mesma mudança do código.

Criado em 2026-10-27, a partir de uma crítica externa com dois pontos:

1. **Prova de fogo em partições de rede (Jepsen / chaos engineering).** O Taba replica entre nós, faz failover e
   promete semânticas de entrega (FIFO, DLQ). Esses sistemas costumam falhar de forma sutil em partições
   assimétricas, disco lento e morte abrupta do primário (perda, mensagens fantasmas, split-brain). Falta um
   conjunto automatizado de testes de caos que prove que o cluster mantém as garantias durante essas falhas.
2. **Benchmarks padronizados e reproduzíveis.** Faltam números concretos: latência p99 e p99.9 sob saturação,
   vazão de 1 nó e de 3 nós, efeito das conexões HTTP concorrentes comparado com protocolos binários (NATS,
   Kafka). Falta um diretório de benchmarks com scripts reproduzíveis e gráficos comparativos honestos.

## Ponto de partida (o que já existe)

- **Rede em memória:** `MemoryNetwork` (`crates/taba-node/src/cluster/transport/memory/mod.rs`) já faz partição,
  isolamento, perda (`lose_next`, `lose_kind`), atraso (`set_delay*`) e nó mudo (`set_silent`), com o `Clock`
  manual de `tests/common/cluster.rs`. Toda partição é **simétrica**: `partition(a, b)` corta os dois sentidos.
- **Validação ponta a ponta:** `docker/validate.sh` faz crash, drain e backup S3 e confere que nenhuma mensagem
  confirmada se perdeu. É um roteiro fixo e não grava um histórico verificável.
- **Split-brain assumido:** [07-cluster.md](../en-US/07-cluster.md#accepted-risk-partitions) registra que, numa
  partição com nós suficientes dos dois lados, cada lado pode excluir o outro e continuar.
- **Sem fsync por escrita:** a durabilidade vem da réplica em memória com quorum
  ([11-durability.md](../en-US/11-durability.md)); `storage.db.synchronous` é `NORMAL` por padrão. O benchmark
  precisa deixar isso explícito, senão a comparação com Kafka ou NATS fica desonesta.
- **Sem benchmark:** o roadmap (linha "Benchmarks", RNF-02), 01-overview e os READMEs já dizem "no published
  benchmark". `docker/test-cpu.sh` mede só CPU.

---

## Parte 1 — Testes de caos com verificação de histórico

### Fases 1.0 e 1.1 — feitas em 2026-10-27, falta o que está abaixo

Já no código e na documentação permanente (não repetir aqui): o documento 37 com as garantias I1 a I6 e R1
(`doc/*/37-*.md`), as falhas novas da `MemoryNetwork` (`cut_one_way`, `lose_replies`, `set_link_loss`, `seed_links`,
`heal_all`; a ponte é uma `partition` entre os dois lados), o gravador e o verificador de histórico
(`tests/common/history.rs`), a rodada `tests/chaos.rs` (4 sementes fixas no PR e a varredura por
`TABA_CHAOS_SEEDS`), o workflow noturno `chaos.yml` e os casos `UC-CHS-001` a `003`. Resultado das primeiras 24
sementes: nenhuma violação.

Falta da 1.1 (o I5 está feito; a rodada achou e corrigiu dois bugs, D180):

- **Gancho de disco lento no storage**, só para testes: atraso ou erro antes do commit SQLite e no overflow. Depois,
  `disco lento` entra no nemesis.
- **Morte abrupta e volta de um nó, e drain**, dentro do nemesis (hoje só há isolamento, que não perde a memória do
  nó). Exige reiniciar o nó com `respawn_node` no meio da rodada.
- **Dois nós prejudicados ao mesmo tempo** (além do que o cluster padrão promete), para ver como a perda é
  *declarada* (I6) quando passa de `write.quorum − 1`.
- **Rodada noturna com abertura de issue** quando falhar (hoje só o log do job mostra a semente).

### Fase 1.2 — Caos com processos reais em Docker (rodada noturna)

Cobre o que a rede em memória não reproduz: TCP de verdade, mTLS, timeouts do kernel e disco real.

- **`docker/chaos.sh`**, construído sobre `docker/cluster.sh`, com:
  - `tc netem` e `iptables` nos contêineres (`NET_ADMIN`): perda, atraso, partição assimétrica e ponte;
  - `docker pause` para simular processo congelado (SIGSTOP);
  - `kill -9` no **primário** de um bloco escolhido;
  - limite de I/O por cgroup (`--device-write-bps`) para simular disco lento;
  - `libfaketime` para relógio desajustado.
- **Gerador de carga e checker**: um binário Rust (`chaos/taba-chaos`) que usa o `taba-client`, grava o
  histórico em JSONL e roda o mesmo checker da 1.1. Os dois compartilham o crate do checker.
- **Workflow `chaos.yml`** (já existe, com a varredura em memória): ganha um job com o Docker. Ao falhar, abre
  uma issue com o histórico e os logs como artefato.

### Fase 1.3 — Jepsen de verdade (opcional, depois)

Jepsen é escrito em Clojure e tem o checker Elle e o workload de fila. Vale fazer **depois** que as fases 1.1 e
1.2 estabilizarem, como uma prova independente: um diretório `jepsen/` com um cliente HTTP simples. O selo forte
seria uma análise independente da própria Jepsen, mas isso é contratação, não código.

### Resultados e regras do repositório

- Um bug achado vira `fix:` com teste, entra em 36-known-issues e segue o fluxo candidato → `validate.sh`.
- O resultado do risco R1 vira uma pergunta **D182** (o R1 já foi medido e corrigido, D181): tornar `cluster.vote.min-voters` maioria por padrão? Isso é
  mudança de comportamento e precisa de decisão do mantenedor.
- Tipos de commit: `test:` e `ci:` para a infraestrutura, que não publicam versão.

---

## Parte 2 — Benchmarks reproduzíveis

### Fase 2.1 — Ferramenta de carga própria (`bench/`, em um workspace separado)

- **Gerador Rust em carga aberta**, com taxa constante e correção de *coordinated omission*. Usa HdrHistogram.
  Sem isso, o p99.9 sai otimista.
- **Mede** a latência de publicação, a latência ponta a ponta (publicação → entrega via long poll), a vazão
  máxima e a curva latência × carga até saturar.
- **Grava** um JSON por rodada com o commit, a configuração, a máquina e os percentis p50, p90, p99, p99.9 e máximo.

### Fase 2.2 — Matriz de cenários (fixa e versionada em `bench/scenarios/*.toml`)

| Eixo | Valores |
|---|---|
| Topologia | 1 nó (fator 1) · 3 nós (fator 3, quorum 2) · 3 nós com quorum 3 |
| Payload | 256 B · 1 KB · 16 KB · 64 KB |
| Conexões | 10 · 100 · 1 000 · 10 000 (o custo do HTTP/1.1) |
| Tipo | tópico padrão · fila FIFO · fan-out para N assinantes |
| Durabilidade | padrão (memória) · overflow forçado para disco · `storage.db.synchronous=FULL` |
| Sob falha | vazão e latência **durante** a queda de um nó e o drain (reaproveita `chaos.sh`) |

### Fase 2.3 — Comparação justa com outros sistemas

- **Recomendação:** escrever um **driver Taba para o OpenMessaging Benchmark (OMB)**. É o harness neutro que já
  tem drivers para Kafka, NATS JetStream, RabbitMQ, Pulsar e Redpanda, o que dá mais credibilidade do que um
  script nosso.
- **Durabilidade equivalente, documentada lado a lado:**
  - Kafka com `acks=all`, RF 3, sem fsync por mensagem;
  - NATS JetStream R3;
  - RabbitMQ com quorum queues, que fazem fsync. A diferença entra como ressalva explícita.
- **Máquina:** 3 servidores mais 1 cliente, num tipo de instância fixo (por exemplo, c7i.2xlarge), descritos em
  `bench/infra/` com Terraform ou script. A versão em Docker Compose fica para quem quiser reproduzir, marcada
  como "não representativa".

### Fase 2.4 — Benchmark de regressão no CI

Runners do GitHub são ruidosos demais para números absolutos. Ali só cabem:

- micro-benchmarks `criterion` nos caminhos quentes (codec, anel, storage);
- uma carga curta comparada com a do `main`, que falha acima de, por exemplo, 15% de regressão.

### Fase 2.5 — Publicação

- **Resultados:** `bench/results/<data>-<commit>/` com o JSON bruto, mais gráficos SVG gerados por script.
- **Documento novo 38-benchmarks** nos dois idiomas, com **onde o Taba ganha e onde perde**. Por exemplo: o
  HTTP/1.1 com muitas conexões provavelmente perde para protocolo binário em vazão por núcleo, e o fato de não
  haver fsync explica parte da latência.
- **Atualizações:** RNF-02 em 01-overview, a linha "Benchmarks" do roadmap e o "Still to come" dos dois READMEs.
- **Gargalos achados com flamegraph** viram `perf:`, cada um com um antes e depois medido.

---

## Ordem sugerida

1. **1.0 Garantias:** curto, e destrava todo o resto.
2. **1.1 Caos em memória:** maior retorno, entra no CI.
3. **2.1 e 2.2 Ferramenta e cenários:** em paralelo com a 1.1, porque o gerador e o gravador de histórico
   compartilham código.
4. **1.2 Caos em Docker**, depois **2.3 OMB e comparação**.
5. **2.4 e 2.5 Regressão e publicação.**
6. **1.3 Jepsen**, opcional.

## Decisões em aberto

1. **Infra de benchmark:** há orçamento para instâncias dedicadas? Sem elas, os números comparativos ficam só
   como "Docker local, não representativo".
2. **Jepsen em Clojure** agora, ou checker próprio primeiro? Recomendação: checker próprio primeiro e Jepsen depois.
3. **Split-brain:** se o teste confirmar o efeito do risco R1, aceitar como está, documentado, ou tratar (D182 trata só o padrão de min-voters; o R1 foi corrigido em D181)?

Quando cada decisão for tomada, ela vai para [30-decisions.md](../en-US/30-decisions.md) /
[30-decisoes.md](../pt-BR/30-decisoes.md) e sai daqui.
