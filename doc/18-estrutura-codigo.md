# Linguagem e estrutura do código

[← Índice](README.md)

## 17. Linguagem, runtime e estrutura do código

> Os nomes de crates e as bibliotecas abaixo são **propostas**, como no restante da documentação ([10-configuracao.md](10-configuracao.md)). A decisão firme é a linguagem (seção 17.1) e a regra de começar simples (seção 17.3).

### 17.1 Linguagem: Rust

O Taba é escrito em **Rust**, com o runtime assíncrono **tokio**. Fecha a dúvida D10 ([08a-decisoes-tomadas.md](08a-decisoes-tomadas.md)).

**Por quê:**

- **Latência previsível.** As mensagens ficam em memória ([02-arquitetura.md](02-arquitetura.md), seção 3.2). Sem coletor de lixo, não há pausa que apareça na latência de publicação e de entrega.
- **Baixo overhead e imagem leve.** Um binário único e estático atende ao objetivo 7, "Docker simples e leve" ([01-visao-geral.md](01-visao-geral.md)).
- **Concorrência explícita e checada na compilação.** O desenho depende de escritor único por tópico, threads paradas em semáforo, canais entre nós e streams com contrapressão. Rust expressa isso sem corrida de dados.
- **Encaixe com o que a documentação já exige:** mTLS com certificados fixados e recarga a quente, WebSocket em malha, NDJSON, SQLite e OpenTelemetry têm bibliotecas maduras.

**Custo aceito:** a curva de aprendizado do *ownership* com estado compartilhado em código assíncrono. A mitigação está na seção 17.4 (passar mensagens em vez de compartilhar estado).

### 17.2 Bibliotecas de partida

Escolhas iniciais, trocáveis sem mexer no desenho:

| Necessidade | Biblioteca | Onde o desenho a pede |
|---|---|---|
| Runtime, tasks, canais, semáforo | `tokio` | Lista de prontos, fila de comandos por tópico, avisos ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.6 e 12.8) |
| HTTP (três listeners) | `axum` + `hyper` | Portas de cliente, cluster e operação ([06-api.md](06-api.md), seção 8.0) |
| TLS mútuo com recarga | `rustls` (verifier e resolver próprios; provedor `ring`, instalado como padrão do processo ao abrir o OTLP e o TLS, porque o `aws-lc` das dependências do `reqwest` também está ligado e o `rustls` não escolhe sozinho) | [06-api.md](06-api.md), seção 8.0.1 |
| WebSocket do canal | `axum` (ws) / `tokio-tungstenite` | [04-cluster.md](04-cluster.md), seção 6.1 |
| SQLite (`_node.db` e um banco por tópico) | `rusqlite` com a SQLite embutida (`bundled`: o build precisa de um compilador C) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| JSON e NDJSON | `serde`, `serde_json` | [06d-api-convencoes.md](06d-api-convencoes.md), [06c-api-cluster.md](06c-api-cluster.md), seção 8.2.1 |
| GUID v7 | `uuid` (feature `v7`) | IDs de mensagem, tópico, nó e token |
| Logs e traces | `tracing` | [11-observabilidade.md](11-observabilidade.md) |
| OTLP e Prometheus | `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` e `tracing-opentelemetry` para traces e logs; as métricas OTLP são montadas à mão com `opentelemetry-proto` (o SDK não aceita dados agregados de fora) a partir do registro próprio, que também escreve o texto do Prometheus | Uma instrumentação, dois exportadores (o exportador Prometheus do SDK está descontinuado) |
| Configuração | `serde` + `serde_norway` (YAML) + variáveis `TABA_*`; `ipnet` para IPs e faixas CIDR | [10-configuracao.md](10-configuracao.md) |

### 17.3 Estrutura: um workspace, começando por um crate

O repositório é um **Cargo workspace**. Cada crate é o equivalente de um projeto de uma solução .NET. Em Rust não há container de injeção de dependência: os traits definem as fronteiras e o `main` cria as implementações e as entrega aos construtores.

**Decisão: começar com um único crate** (o binário `taba`), com a organização em **módulos**, e extrair um módulo para crate só quando houver motivo (abaixo). Os módulos já impõem a separação, e passar de módulo a crate é uma mudança mecânica. Separar cedo custa um `Cargo.toml` a mais por parte, `pub` entre crates e recompilação, e a fronteira entre entrega e cluster ainda não está estável (o coordenador de escrita precisa do storage, e a entrega precisa do registro + commit).

**Ponto de partida:**

```
taba/
  Cargo.toml                workspace
  doc/                      documentação
  crates/
    taba-node/              biblioteca `taba_node` e binário `taba`
      Cargo.toml
      src/
        main.rs             composition root: linha de comando, configuração, sinais
        lib.rs              os módulos abaixo; os testes de integração usam a biblioteca
        cli.rs              argumentos do binário
        node.rs             subida do nó: disco, estado, listeners
        config/             arquivo .yml, variáveis TABA_*, tabela de parâmetros, validações
        core/               tipos de domínio, relógio, GUID v7, erros, validações   (sem I/O)
        storage/            memória, overflow em disco, SQLite, sync.db, _node.db
        engine/             tópicos, listas de prontos e invisíveis, entrega justa, FIFO, dead-letter
        cluster/            membros, votação, freezing, canal, registro + commit, sincronização, drain
        http/               as três APIs, autenticação, rate limit, mTLS
        telemetry/          métricas, traces, logs
      tests/                testes de integração (um nó real, seção 17.5; o cluster simulado entra com o `Transport`)
        common/mod.rs       `TestNode`: nó em portas livres, disco numa pasta temporária
        uc_coverage.rs      confere os códigos de caso de uso citados nos testes (doc 20, seção 19.5)
```

Os testes unitários ficam num arquivo irmão do módulo (o módulo `foo` é a pasta `foo/`, com `mod.rs` e `tests.rs` lado a lado; o `mod.rs` termina com `#[cfg(test)] mod tests;`). Assim o código fica limpo e os testes continuam enxergando os itens privados do módulo. Os de integração ficam em `crates/taba-node/tests/`, pois o Cargo só os acha dentro do crate; o `main.rs` é fino justamente para a biblioteca poder ser subida pelos testes.

**Crates-alvo.** Quando o motivo existir, o módulo vira crate com este nome:

| Crate | Vem do módulo | Motivo para extrair |
|---|---|---|
| `taba-core` | `core/` | Já é lógica pura. Pode virar crate logo, sem custo, e ser testado sozinho. |
| `taba-protocol` | DTOs de `http/` e frames do canal | Só se algo além do servidor precisar dos DTOs. O conector Rust (`connectors/rust/client/`) **não** o usa: não tem dependências e compila em Rust antigo ([19-conectores.md](19-conectores.md), seção 18.2). |
| `taba-storage` | `storage/` | Quando houver mais de uma implementação (memória, SQLite) ou o tempo de compilação pesar. |
| `taba-cluster` | `cluster/` + `engine/` | Só se a fronteira entre os dois se firmar. Até lá ficam juntos. |
| `taba-http` | `http/` | Se um segundo binário (por exemplo, ferramentas de operação) precisar das rotas. |

**Direção das dependências** (o Cargo recusa ciclos entre crates, e nos módulos vale a mesma regra por convenção):

```
core  ←  storage  ←  engine  ←  cluster  ←  http  ←  node (main)
```

Cada camada conhece só as da esquerda. O `core` não conhece nada. O `config` fica ao lado da cadeia: só depende do `core`, e o `main` e o `node` o entregam às demais camadas, que usam os tipos dele (`Mode`, por exemplo) mas nunca o leem do arquivo nem do ambiente.

**Traits ficam na camada mais baixa de que o chamador precisa**, não num crate de abstrações central. Como as dependências só andam para a direita, o trait de armazenamento que o `engine` usa mora no `storage` (por exemplo, `TopicStore`), junto das implementações real (`FileTopicStore`) e de teste (`MemoryTopicStore`); o `main` escolhe qual entregar.

### 17.4 Regras de código

- **Passar mensagens, não compartilhar estado.** O escritor único por tópico ([02-arquitetura.md](02-arquitetura.md), seção 3.2) é um *actor*: uma task com fila de comandos (`mpsc`), executada uma de cada vez. Prefira isso a `Arc<Mutex<…>>` espalhado.
- **Nunca segurar um lock durante um `.await`.**
- **SQLite fora do executor assíncrono.** O `rusqlite` é síncrono: roda em thread dedicada (ou `spawn_blocking`), nunca numa task do tokio. As leituras longas usam conexões próprias em WAL.
- **Sem `unwrap()` fora de testes.** Erros usam um tipo próprio que mapeia direto para o catálogo de códigos ([06d-api-convencoes.md](06d-api-convencoes.md), seção 8.3.6).
- **Cancelamento limpo.** Toda task de fundo recebe um sinal de parada (`CancellationToken`), que é o que o drain usa ([04-cluster.md](04-cluster.md), seção 6.10).

### 17.5 Testabilidade: o que fica atrás de um trait

Num sistema distribuído, o teste mais valioso sobe vários nós no mesmo processo e injeta falhas. Para isso, o que toca o mundo externo fica atrás de um trait, definido já nos primeiros módulos:

| Trait | Implementação real | Implementação de teste |
|---|---|---|
| `Transport` (chamadas entre nós) | HTTP/WebSocket com mTLS | Canais em memória, com perda, atraso e partição programáveis |
| `Clock` | Relógio do sistema | Relógio manual (invisibilidade, `ttl`, freezing, votação) |
| `Storage` | SQLite + arquivos | Memória |

Estado atual: o `Clock` existe (`core/clock`, com `SystemClock` e `ManualClock`). O `Transport` existe em `cluster/transport` e fala em tipos do protocolo (`cluster/protocol`), não em bytes; quem atende a chamada é um `Handler`, o mesmo das rotas `/cluster/*`. Há a versão em memória (`MemoryNetwork`, com partição, isolamento, perda das próximas chamadas ou de um tipo de chamada, atraso, nó mudo, nó que cai e balanceador) e, desde a etapa 16, a de HTTP (`cluster/transport/http`: `reqwest`, com o TLS mútuo de `cluster/tls` quando há pasta de certificados; `Deps::production` monta tudo para o `taba`). A porta do cluster com TLS é servida por `http::observe::serve_tls`. O canal entre dois nós é uma ponta com um sentido de envio e um de recepção (`ChannelLink`): um WebSocket na rede real (`cluster/transport/http`, `bridge`) e um par de filas na rede em memória, que um corte de rede derruba. O harness dos testes de cluster em memória está em `tests/common/cluster.rs`; o dos testes com rede de verdade (nós em `127.0.0.x`, TLS mútuo e um cliente TLS cru), em `tests/common/real.rs`. As mensagens usam o `MessageStore` concreto (SQLite e arquivos) sem trait: o trait nasce quando houver uma segunda implementação.

Com eles, os testes de integração (`crates/taba-node/tests/`) cobrem quórum, rollback antes e depois da decisão, freezing, votação, sincronização com o banco de eventos e o retorno de nó excluído, sem rede real e de forma determinística.

### 17.6 Impacto em outros pontos da documentação

- **`process.thread.count`** ([11-observabilidade.md](11-observabilidade.md)): mede as **threads do sistema operacional** do processo (as do runtime, do pool de bloqueio e as dos escritores SQLite). As tasks do tokio não entram.
- **Imagem Docker:** binário único, em imagem mínima ([01-visao-geral.md](01-visao-geral.md), objetivo 7).
- **Conectores:** ficam em `connectors/`, fora do workspace do servidor, uma pasta por linguagem. Os de Rust (`taba-client` e `taba-cli`) formam um workspace do Cargo próprio, `connectors/rust/Cargo.toml`: `cd connectors && cargo test`. O `taba-client` em Rust é o conector `connectors/rust/client/`, independente do `taba-protocol` ([19-conectores.md](19-conectores.md), seção 18).

### 17.7 Onde está cada coisa no código

Cada `mod.rs` começa com um comentário `//!` que diz o que o módulo faz e em que documento está o desenho. Este mapa é o caminho de volta, do documento ao código (`crates/taba-node/src/`):

| Assunto | Documento | Módulo |
|---|---|---|
| Configuração e tabela de parâmetros | [10](10-configuracao.md) | `config/` (`params` é a fonte única de chaves e padrões) |
| Erros, IDs, datas, cursor, limites, anel de hash, definição de tópico e de token | [06d](06d-api-convencoes.md), [05](05-particionamento-replicacao.md), [02](02-arquitetura.md) | `core/` (`error`, `id`, `time`, `pagination`, `validation`, `ring`, `topic`, `token`, `content`, `clock`) |
| Armazenamento: mensagens em disco, `_node.db` (`topics`, `tokens`, `state`, `losses`) e o banco de eventos | [02 §3.2](02-arquitetura.md) | `storage/` (`messages`, `db`, `topics`, `tokens`, `state`, `losses`, `synclog`) |
| Tópicos, mensagens, listas de prontos e de invisíveis, entrega justa, FIFO, dead-letter, tokens | [13](13-entrega-de-mensagens.md), [14](14-invisibilidade.md), [15](15-topicos-distribuidor.md), [17](17-topicos-fifo.md) | `engine/` (`topics`, `messages` com `index`, `rates`, `replicas` e `stats`, `tokens`) |
| API de cliente, de operação e do cluster; autenticação; rate limit; repasse | [06](06-api.md) a [06d](06d-api-convencoes.md), [12](12-limites-api.md) | `http/` (`topics`, `messages`, `tokens`, `auth`, `ratelimit`, `client_ip`, `forward`, `cluster_api`, `metrics`, `topic_stats`, `operations.rs`, `observe`) |
| Tabela de membros e registro em duas fases | [04 §6.2](04-cluster.md) | `cluster/members/` (a mudança efetivada também viaja pelo quadro `membership` do canal) |
| Chamadas entre nós como tipos | [06c](06c-api-cluster.md) | `cluster/protocol/`; o `Transport` em `cluster/transport/` (`http` real, `memory` de teste) |
| Canal permanente, sinal de vida, fila de saída | [04 §6.1 e 6.3](04-cluster.md), [13 §12.8 e 12.9](13-entrega-de-mensagens.md) | `cluster/channel/` (com `queue/`) |
| Votação de exclusão e freezing | [04 §6.3 e 6.7](04-cluster.md) | `cluster/vote/`, `cluster/members/` |
| Replicação: escrita, invisibilidade, ack, avisos, definições de tópico e de token | [05 §7.2](05-particionamento-replicacao.md), [02 §3.3 e 3.4](02-arquitetura.md) | `cluster/replication/` (`coordinator`, `replica`, `forward`, `messages_api`, `signals`, `definitions`) |
| Sincronização de blocos de um nó que entra | [05 §7.3](05-particionamento-replicacao.md) | `cluster/sync/` |
| Retorno de nó e recuperação de perdas | [04 §6.6](04-cluster.md) | `cluster/recovery/`, `storage/losses/` |
| Drain e save | [04 §6.10](04-cluster.md) | `cluster/drain/` |
| Fila volátil (nó gestor) | [15 §14.5 e 14.6](15-topicos-distribuidor.md) | `cluster/volatile/` |
| Condições do cluster | [11](11-observabilidade.md) | `cluster/health/` |
| TLS mútuo | [06 §8.0.1](06-api.md) | `cluster/tls/`, `secrets/` |
| Métricas, traces, logs, OTLP, Prometheus | [11](11-observabilidade.md) | `telemetry/` (`metrics`, `prometheus`, `otlp`, `trace`, `process`, `histogram`) |

Detalhes que não aparecem no desenho:

- Os **traces e os logs do OTLP** entram no `tracing` em `init_logging`: o `Hub` aponta para o pipeline do nó (amostrador, processador e logger trocáveis). Sem pipeline, o amostrador descarta e os logs não saem. Num processo com vários nós (os testes) vale o último a subir.
- O **estado de falha** do nó (`failed`) é o mesmo para um erro de disco nas mensagens e para um erro fatal do `_node.db` (disco cheio ou banco corrompido): o nó deixa de atender o cliente e só um reinício o tira de lá (D78).
