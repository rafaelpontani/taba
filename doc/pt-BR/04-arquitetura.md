---
título: Arquitetura
resumo: Como um nó do Taba é montado por dentro — camadas, direção das dependências, as três portas, as fronteiras de teste e o mapa de módulos.
tipo: explicação
status: estável
verificado-em: 2026-10-15 @ 56121c9
fontes: [crates/taba-node/src/lib.rs, crates/taba-node/src/node.rs, crates/taba-node/src/main.rs, crates/taba-node/src/http/mod.rs, crates/taba-node/src/cluster/transport/mod.rs]
---

# Arquitetura

> Leia antes: [03-conceitos.md](03-conceitos.md).

## Visão em uma tela

Um cluster Taba é feito de **nós idênticos**. Cada nó é **um processo** (o binário `taba`) que:

1. guarda mensagens **em memória**, com **overflow para o disco** (SQLite e arquivos);
2. abre **três portas HTTP** (cliente, operação, cluster);
3. fala com os outros nós por **chamadas HTTP** e por um **canal permanente WebSocket**, protegidos por TLS mútuo;
4. decide sozinho quem é dono de quê, a partir da **tabela de membros** e do **anel de hash**, sem coordenador.

Não existe um nó "mestre" nem um serviço externo. O que seria "o banco" são arquivos locais de cada nó; o que seria "o coordenador" é calculado por todos do mesmo jeito.

![Um nó por dentro: três portas, engine, cluster e storage](../diagrams/img/pt-BR/node-overview.svg)

## As camadas e a direção das dependências

O código é **um crate** (`taba-node`) organizado em módulos. A regra de dependência é de uma só direção:

```
core  ←  storage  ←  engine  ←  cluster  ←  http  ←  node
                       config fica ao lado: lê arquivo e ambiente e só depende de core
```

Cada módulo conhece apenas os da esquerda. Isso é verificável: `core` não importa mais ninguém.

| Camada | Responsabilidade | Não faz |
|---|---|---|
| `core` | Tipos de domínio e regras puras: IDs (GUID v7), tempo, erros (`ErrorCode`), validações, o **anel de hash**, tópico, token, paginação | Nenhum I/O |
| `storage` | O banco do nó (`_node.db`), as mensagens em disco, o banco de eventos de sincronização, tokens e perdas | Regras de entrega |
| `engine` | Tópicos, mensagens, listas de prontos e de invisíveis, entrega justa, FIFO, dead-letter, tokens, estatísticas | Rede |
| `cluster` | Membros, protocolo entre nós, canal, votação, freezing, sincronização, replicação, drain, atualização de versão, recuperação, tópicos voláteis, saúde | HTTP de cliente |
| `http` | As três APIs, autenticação, rate limit, IP do cliente, repasse, métricas | Regras de domínio (só lê a requisição e monta a resposta) |
| `telemetry` | Métricas, traces e logs; exportação OTLP e Prometheus | — |
| `config` | Arquivo `.yml`, variáveis `TABA_*`, a tabela de parâmetros e as validações de partida | — |
| `secrets` | A pasta de segredos: certificado do cluster e token raiz (`taba init`, `taba cert generate`, `taba token generate`) | — |
| `cli` | A linha de comando do binário `taba` | — |
| `node.rs` | **A subida do nó**: prepara o disco, cria o estado, liga as peças e abre os listeners | — |

O `main.rs` é só a **raiz de composição**: lê a configuração, chama o que está em `node.rs` e espera o sinal de parada. Tudo o mais mora na biblioteca `taba_node` para que os testes de integração subam **nós de verdade**.

![As camadas e a direção das dependências](../diagrams/img/pt-BR/layers.svg)

## As três portas

Cada uma é um *listener* próprio, para que a rede possa restringir o acesso a cada uma separadamente.

| Porta | Chave | Padrão | Roteador (`http/mod.rs`) | Camadas por onde passa a chamada |
|---|---|---|---|---|
| Cliente | `api.port` | 8080 | `client_router` | autenticar → rate limit → recusa por drain/sincronização/falha → autorizar |
| Operação | `operations.port` | 9090 | `operations_router` | filtro de IPs (`operations.allowed-ips`), rate limit por IP; **sem token** |
| Cluster | `cluster.port` | 8081 | `cluster_router` | TLS mútuo; só `/cluster/*` |

- As três portas estão **sempre abertas** e devem ser diferentes. As rotas de operação ficam **só** na porta de operação, com uma exceção: a porta de cliente também serve `/health/live` e `/health/ready` (sem token, para sondas que não conseguem enviá-lo) e `/metrics` (qualquer token válido). Assim, `/metrics` é aberto na porta de operação, para os coletores dentro da infraestrutura, e fica atrás de token na porta de cliente, para quem alcança o nó de fora. O `taba-cli` só usa a porta de cliente.
- Um nó sozinho (`replication.factor=1`) abre a porta do cluster do mesmo jeito; só nenhum outro nó a usa.
- Rotas `/tokens` só existem com a autenticação ligada; senão respondem `404`.
- Um nó em drain, em sincronização ou em falha **não atende** chamadas de cliente: num cluster ele as **repassa** a outro nó (pela porta do cluster do destino) e, sem destino, responde `503` com `Retry-After`. As leituras que dizem o que o cluster está fazendo (`GET /auth/me`, `/nodes`, `/cluster/status`, `/cluster/backup` e as chamadas `/nodes/{nodeId}/…`, que incluem devolver um nó) são a exceção: são repassadas enquanto outro nó puder atendê-las e, senão, respondidas pelo próprio nó, de modo que um cluster parado ainda pode ser inspecionado. Em drain o nó só valida o token root (os outros tokens são dados).

Detalhes das rotas em [15-api-visao-geral.md](15-api-visao-geral.md).

## Fronteiras que tornam o nó testável

O que toca o mundo fica atrás de um *trait*, para o teste trocar a implementação:

| Trait | Implementação real | Implementação de teste | Para que serve |
|---|---|---|---|
| `Transport` (`cluster/transport`) | HTTP + WebSocket, com mTLS (`transport/http`) | Rede em memória com partição, perda, atraso e nó mudo programáveis (`transport/memory`) | Chamadas e canal entre nós |
| `Clock` (`core/clock`) | Relógio do sistema | Relógio manual, avançado pelo teste | Invisibilidade, `ttl`, freezing, votação |
| `Handler` (`cluster/protocol`) | `ClusterHandler` sobre `ClusterService` | o mesmo | O que **recebe** uma chamada entre nós, igual para a rota HTTP e para a rede em memória |
| `ObjectStore` (`storage/backup/store`) | `S3Store` sobre o `aws-sdk-s3` (AWS e MinIO) | `MemoryStore`, com falhas programáveis (fora do ar, sem escrita condicional) | O depósito do backup opcional em S3 |
| `BackupSink` (`engine/messages`) | `Backup` (`cluster/backup`) | um registrador, nos testes do motor | O que o motor **avisa** (publicou, removeu, mudou o estado, mudou o dono), sem nunca esperar o S3 |

Por isso os testes de cluster (nível `C`, [25-testes.md](25-testes.md)) rodam vários nós **no mesmo processo** e controlam o tempo e a rede.

## Concorrência

- **Runtime:** `tokio`, vários threads.
- **Escritor único por banco:** toda escrita num banco SQLite (o de cada tópico e o `_node.db`) passa por **um único escritor**; leituras longas, como os streams de sincronização, usam conexões de leitura próprias (modo WAL).
- **Estado compartilhado:** tabela de membros, tabela de tópicos e tokens ficam em memória atrás de travas; mudanças de membros são **uma por vez** por nó.
- **Tarefas de fundo:** vencimento da invisibilidade, varredura de `ttl`, gestão das filas voláteis, reunião após partição, relatório de overflow.

## Mapa de módulos

```
crates/taba-node/src/
  main.rs            composição: linha de comando, configuração, sinais
  lib.rs             a lista de módulos (a biblioteca `taba_node`)
  node.rs            subida do nó
  cli/               argumentos do binário `taba`
  config/            arquivo, variáveis, tabela de parâmetros, validações
    params/ source/ otlp.rs error.rs
  core/              clock content error id pagination ring time token topic validation
  secrets/           pasta de segredos (certificado do cluster, token raiz)
  storage/           db (o `_node.db`) messages topics tokens state losses versions synclog
                     backup (store, keys, segment, journal, db: o `_backup.db`)
  engine/            topics tokens messages (blocks, index, rates, replicas, stats)
  cluster/           backup (upload, evict, definitions, heartbeat, restore, stop, topology)
                     channel drain health members protocol recovery replication restore
                     service state sync tls transport (http, memory) upgrade vote volatile
  http/              auth backup client_ip cluster_api common forward messages metrics observe
                     nodes operations ratelimit tokens topic_stats topics
  telemetry/         histogram metrics otlp process prometheus trace
```

Cada pasta tem um `mod.rs` que começa com um comentário `//!` dizendo o que faz. Os testes unitários ficam em `tests.rs` ao lado do módulo, ligados por `#[cfg(test)] mod tests;`.

## Como o nó sobe (`node.rs`)

1. Lê a configuração e valida (por exemplo, `storage.dir` precisa terminar em uma pasta chamada `taba`).
2. Prepara o disco: o `_node.db`, as pastas de tópicos; o disco da vida anterior é posto de lado e volta pela restauração ([07-cluster.md](07-cluster.md), seção 10).
3. Cria o estado compartilhado do nó, a telemetria, os registros de tópicos e tokens, e as mensagens.
4. Em produção: cria o `ClusterService`, a replicação e o transporte, e **entra no cluster** (bootstrap) ou, se for o primeiro nó (`cluster.first-node`), começa o cluster sozinho.
5. Abre os listeners das três portas e, a partir daí, atende. O nó só fica **pronto** (`/health/ready`) quando termina de sincronizar.

O detalhe de entrada e sincronização está em [07-cluster.md](07-cluster.md).

## Fontes e provas

- `crates/taba-node/src/lib.rs` (a regra de dependência), `node.rs`, `http/mod.rs` (`client_router`, `operations_router`, `cluster_router`, `drain_guard`), `cluster/transport/mod.rs` (`Transport`), `core/clock/mod.rs`.
- Casos de uso: UC-SEG (portas e TLS mútuo), UC-DEV (perfil de um nó), UC-FWD (repasse), UC-CFG (configuração); veja [26a](26a-casos-de-uso-implementados.md).
- Histórico: `02-arquitetura.md`, `18-estrutura-codigo.md`.
