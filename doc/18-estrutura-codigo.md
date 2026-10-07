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
| TLS mútuo com recarga | `rustls` (verifier e resolver próprios) | [06-api.md](06-api.md), seção 8.0.1 |
| WebSocket do canal | `axum` (ws) / `tokio-tungstenite` | [04-cluster.md](04-cluster.md), seção 6.1 |
| SQLite por tópico | `rusqlite` (síncrono, em thread dedicada) | [02-arquitetura.md](02-arquitetura.md), seção 3.2 |
| JSON e NDJSON | `serde`, `serde_json` | [06d-api-convencoes.md](06d-api-convencoes.md), [06c-api-cluster.md](06c-api-cluster.md), seção 8.2.1 |
| GUID v7 | `uuid` (feature `v7`) | IDs de mensagem, tópico, nó e token |
| Logs e traces | `tracing` | [11-observabilidade.md](11-observabilidade.md) |
| OTLP e Prometheus | `opentelemetry` + exportador de texto Prometheus | Uma instrumentação, dois exportadores |
| Configuração | `serde` + variáveis `TABA_*` | [10-configuracao.md](10-configuracao.md) |

### 17.3 Estrutura: um workspace, começando por um crate

O repositório é um **Cargo workspace**. Cada crate é o equivalente de um projeto de uma solução .NET. Em Rust não há container de injeção de dependência: os traits definem as fronteiras e o `main` cria as implementações e as entrega aos construtores.

**Decisão: começar com um único crate** (o binário `taba`), com a organização em **módulos**, e extrair um módulo para crate só quando houver motivo (abaixo). Os módulos já impõem a separação, e passar de módulo a crate é uma mudança mecânica. Separar cedo custa um `Cargo.toml` a mais por parte, `pub` entre crates e recompilação, e a fronteira entre entrega e cluster ainda não está estável (o coordenador de escrita precisa do storage, e a entrega precisa do registro + commit).

**Ponto de partida:**

```
taba/
  Cargo.toml                workspace
  doc/                      documentação
  crates/
    taba-node/              binário `taba`
      Cargo.toml
      src/
        main.rs             composition root: config, criação das peças, sinais
        core/               tipos de domínio, anel de hash, GUID v7, erros   (sem I/O)
        storage/            memória, overflow em disco, SQLite, sync.db, _node.db
        engine/             tópicos, listas de prontos e invisíveis, entrega justa, FIFO, dead-letter
        cluster/            membros, votação, freezing, canal, registro + commit, sincronização, drain
        http/               as três APIs, autenticação, rate limit, mTLS
        telemetry/          métricas, traces, logs
  tests/                    testes de integração (cluster simulado, seção 17.5)
```

**Crates-alvo.** Quando o motivo existir, o módulo vira crate com este nome:

| Crate | Vem do módulo | Motivo para extrair |
|---|---|---|
| `taba-core` | `core/` | Já é lógica pura. Pode virar crate logo, sem custo, e ser testado sozinho. |
| `taba-protocol` | DTOs de `http/` e frames do canal | Só se algo além do servidor precisar dos DTOs. O conector Rust (`connectors/rust/`) **não** o usa: não tem dependências e compila em Rust antigo ([19-conectores.md](19-conectores.md), seção 18.2). |
| `taba-storage` | `storage/` | Quando houver mais de uma implementação (memória, SQLite) ou o tempo de compilação pesar. |
| `taba-cluster` | `cluster/` + `engine/` | Só se a fronteira entre os dois se firmar. Até lá ficam juntos. |
| `taba-http` | `http/` | Se um segundo binário (por exemplo, ferramentas de operação) precisar das rotas. |

**Direção das dependências** (o Cargo recusa ciclos entre crates, e nos módulos vale a mesma regra por convenção):

```
core  ←  storage  ←  engine  ←  cluster  ←  http  ←  node (main)
```

Cada camada conhece só as da esquerda. O `core` não conhece nada.

**Traits ficam no lado de quem os usa**, não num crate de abstrações central: o `engine` define o trait de que precisa do storage, e o `storage` o implementa.

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

Com eles, `tests/` cobre quórum, rollback antes e depois da decisão, freezing, votação, sincronização com o banco de eventos e o retorno de nó excluído, sem rede real e de forma determinística.

### 17.6 Impacto em outros pontos da documentação

- **`process.thread.count`** ([11-observabilidade.md](11-observabilidade.md)): mede as **threads do sistema operacional** do processo (as do runtime, do pool de bloqueio e as dos escritores SQLite). As tasks do tokio não entram.
- **Imagem Docker:** binário único, em imagem mínima ([01-visao-geral.md](01-visao-geral.md), objetivo 7).
- **Conectores:** ficam em `connectors/`, fora do workspace do servidor, uma pasta por linguagem. O `taba-client` em Rust é o conector `connectors/rust/`, independente do `taba-protocol` ([19-conectores.md](19-conectores.md), seção 18).
