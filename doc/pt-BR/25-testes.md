---
título: Testes
resumo: Como o Taba é testado — os níveis (unitário, um nó, cluster simulado, conectores, ponta a ponta), as ferramentas de teste (TestNode, TestCluster, rede em memória, relógio manual, coletor OTLP falso) e como escrever e rodar um teste.
tipo: guia
status: estável
verificado-em: 2026-10-24 @ 34e9246
fontes: [crates/taba-node/tests, crates/taba-node/tests/common, crates/taba-node/src/cluster/transport/memory, crates/taba-node/src/core/clock, connectors/rust, connectors/csharp/Taba.Client.Tests, docker/validate.sh, docker/seed-demo.sh]
---

# Testes

> Leia antes: [04-arquitetura.md](04-arquitetura.md) (por que o nó é testável) e [24-convencoes.md](24-convencoes.md).

## A ideia

Num sistema distribuído, o teste mais valioso **sobe vários nós no mesmo processo e injeta falhas**. O Taba foi desenhado para isso: o que toca o mundo fica atrás de um *trait* (`Transport` para a rede entre nós, `Clock` para o tempo), e a biblioteca `taba_node` expõe tudo o que o binário usa. Assim os testes sobem **nós de verdade** (HTTP real em portas livres, SQLite real numa pasta temporária) sem mock do que importa, e de forma **determinística**: o relógio e a rede são do teste.

## Os níveis

| Nível | Sigla | O que roda | Onde fica |
|---|---|---|---|
| **Unitário** | `U` | Uma peça isolada, sem rede nem disco reais: domínio, validação, anel, listas, quórum, leitor de JSON | `src/**/tests.rs`, **irmão do módulo** |
| **Um nó** | `I` | Um nó real no **perfil de um nó**, HTTP real, disco real (pasta temporária). O que o cliente enxerga | `crates/taba-node/tests/*.rs` com `TestNode` |
| **Cluster** | `C` | **Vários nós no mesmo processo**, rede em memória (perda, atraso, partição programáveis) e `Clock` manual. Quórum, rollback, falha, votação, sincronização, drain | `crates/taba-node/tests/*.rs` com `TestCluster` |
| **Conector** | `K` | O conector contra um transporte de mentira (unitário) e contra um Taba real (integração) | `connectors/rust/**/tests.rs`, `connectors/csharp/Taba.Client.Tests` |
| **Ponta a ponta** | — | O Compose de 3 nós, de verdade | `docker/validate.sh`, `connectors/csharp/scripts/integration.sh` |

Há mais **testes por TCP e TLS reais** (`tests/mtls.rs`, `tests/common/real.rs`: nós em `127.0.0.x`, `Transport` HTTP e TLS mútuo) e **testes de OTLP** (`tests/otlp.rs`, com um coletor falso).

![Os níveis de teste](../diagrams/img/pt-BR/test-pyramid.svg)

## Os arquivos de integração (`crates/taba-node/tests/`)

| Arquivo | Cobre |
|---|---|
| `topics.rs`, `messages.rs` | API de cliente: tópicos, publicação, consumo, ack, invisibilidade (`I`) |
| `auth.rs`, `ratelimit.rs`, `secrets.rs` | Tokens, rate limit, `taba init` |
| `fifo.rs`, `volatile.rs` | Fila FIFO e fila volátil num nó |
| `operations.rs`, `metrics.rs`, `topic_stats.rs`, `conditions.rs`, `otlp.rs` | Saúde, métricas, condições, exportação OTLP |
| `startup.rs`, `listeners.rs` | Subida do nó, ouvintes (long polling) |
| `cluster.rs`, `cluster_api.rs`, `channel.rs`, `notify.rs`, `replication.rs`, `definitions.rs` | Cluster simulado: ingresso, rotas, canal, quórum, definições (`C`) |
| `failures.rs`, `freezing.rs`, `sync.rs`, `recovery.rs`, `drain_cluster.rs` | Falha, votação, freezing, sincronização, retorno, drain |
| `fifo_cluster.rs`, `volatile_cluster.rs` | FIFO e voláteis no cluster |
| `mtls.rs` | Segurança do cluster por TCP real e TLS mútuo |
| `uc_coverage.rs` | **Confere** os casos de uso (ver [26-casos-de-uso.md](26-casos-de-uso.md)) |

Em números: ~330 testes nesses arquivos mais os unitários dos módulos; o cluster tem **702 de 885** casos de uso com teste ([26a](26a-casos-de-uso-implementados.md)).

## As ferramentas (`tests/common/`)

| Ferramenta | O que dá |
|---|---|
| **`TestNode`** (`mod.rs`) | Sobe **um nó real** em portas livres, disco numa pasta temporária. `TestNode::single()` (um nó no perfil de um nó, já ativo e com a restauração concluída) e `::production()`; `builder()` (o perfil de um nó por padrão) com `.env(nome, valor)`, `.in_network(&rede, host)`, `.clock(relógio)`, `.real(ip, entry)`, `.single_node()`, `.production()`. Métodos: `client_url()`, `operations_url()`, `cluster_url()`, `client_get()`, `metrics()`, `stats()`, `storage_dir()`, `shutdown()`. Auxiliares: `series()` (lê uma série de `/metrics`), `error_code()` (confere status e devolve o `code`) |
| **`TestCluster`** (`cluster.rs`) | **Vários nós no mesmo processo** (nó `n` = `10.0.0.n`), ligados pela **`MemoryNetwork`** e por **um `Clock` manual compartilhado**; o balanceador (`balancer:8081`) reparte as chamadas. `start(n)`, `start_with(n, env)`, `add_node()`, `spawn_node()`, `respawn_node()`, `node(n)`, `table(n)`, `member_ids(n)`. Auxiliares: `eventually(…)` (espera uma condição), `settled()` (cluster estável), `create_queue()`, `name_owned_by()` (acha um nome de tópico cujo dono é o nó n), `pause_sync()`/`wait_phase()` (para no meio de uma fase da sincronização), `replication_of()`, `channels_of()`, `signals_of()` |
| **`MemoryNetwork`** (`src/cluster/transport/memory`) | O `Transport` de teste: **partição** entre dois nós ou isolamento de um, **perda** das próximas chamadas a um nó (ou de um tipo), **atraso** em todas as chamadas, nó **mudo** (aceita e não responde), nó que **cai** (some da rede) e **balanceador** |
| **`ManualClock`** (`src/core/clock`) | O relógio do teste: o teste o avança (invisibilidade, `ttl`, freezing, votação, `idleTimeout`) |
| **`RealCluster`** (`real.rs`) | Nós por **TCP real em `127.0.0.x`**, com o `Transport` HTTP e **TLS mútuo**; geração e rotação de certificados (`cert_dir`, `new_pair`, `add_named_pair`), conexão com ou sem certificado (`connect`, `anonymous_client`) |
| **Coletor OTLP falso** (`otlp.rs`) | `Collector::http()` e `::grpc(tls)`, com `metrics()`, `spans()`, `log_records()`. Os testes de OTLP **andam um de cada vez**, porque o assinante do `tracing` é do processo |

## Como escrever um teste

**Regra de ouro:** teste **comportamento observável** (status, corpo, estado do disco, métrica, log, o que o outro nó enxerga), no **menor nível que o prova**.

```rust
/// UC: UC-TOP-001
#[tokio::test]
async fn creating_a_queue_returns_201_with_location_and_the_defaults() {
    let node = TestNode::single().await;
    // … POST /topics, confere status, Location e corpo …
    node.shutdown().await;
}
```

1. **Cite o caso de uso** numa linha `/// UC: UC-XXX-NNN[, UC-YYY-NNN]` **logo acima** do `#[test]` (ou `#[tokio::test]`); pode haver atributos entre a linha e a função. O código precisa existir no catálogo ([26-casos-de-uso.md](26-casos-de-uso.md)).
2. **Nome em inglês e descritivo** (a frase diz o resultado esperado).
3. **Unitário:** em `tests.rs` ao lado do módulo. **Integração:** em `tests/`, usando `TestNode` ou `TestCluster`. Não crie teste de integração novo se um arquivo existente da área serve.
4. **Cluster:** use o `Clock` manual (**avance o tempo**, não durma) e a `MemoryNetwork` (programe a falha). Use `eventually(…)` para esperar o estado, não `sleep`.
5. **Idioma:** identificadores e nomes de teste em inglês; comentários em português.
6. **Rode e atualize o catálogo:** `cargo test --test uc_coverage` confere os códigos citados; acrescente o teste ao cartão do caso em [26a](26a-casos-de-uso-implementados.md) (um caso que ganha o primeiro teste passa de [26b](26b-casos-de-uso-planejados.md) para 26a), nas duas línguas.

## Como rodar

```bash
cargo test                                          # tudo (servidor)
cargo test --test messages                          # um arquivo de integração
cargo test --test uc_coverage -- --nocapture        # placar: "casos de uso com ao menos um teste: 702 de 885"
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check

cd connectors/rust && cargo test --workspace --all-features
cd connectors/csharp && dotnet test Taba.Client.Tests          # unitários; os de integração exigem TABA_URL
connectors/csharp/scripts/integration.sh                       # sobe o Compose, cria tokens e filas, roda tudo
docker/validate.sh                                             # ponta a ponta (≈ 5 min)
```

**O `uc_coverage` em detalhe.** Falha quando: (1) um teste cita um código que **não existe** ou está `(retirado)`; (2) um código aparece **duas vezes** na documentação. Uma terceira regra, "todo caso documentado tem teste", **só falha com `TABA_UC_STRICT=1`**: com 183 casos sem teste, ela imprime o placar em vez de falhar. O `uc_coverage` lê os cartões `### UC-…` de [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md).

## `docker/validate.sh` e `docker/cluster.sh`

- **`cluster.sh`** (biblioteca para `source`, ou comando): `taba_up`, `taba_down`, `taba_kill` (queda brusca), `taba_stop` (saída ordenada, com drain), `taba_start`, `taba_restart`, `taba_wait_healthy`, `taba_wait_members N`, `taba_wait_stable`, `taba_create_token`, `taba_ensure_topic`, `taba_cli …`, `taba_status`. Os testes dos conectores devem **reaproveitá-lo**.
- **`validate.sh`** (≈ 5 minutos): sobe o Compose do zero e confere (1) o cluster formou (`taba-cli cluster status` sem condições); (2) `taba-cli`: criar tópico, publicar, consumir; (3) queda brusca do `taba-2` (`kill`; o `docker/cluster.sh` mapeia `taba-N`, N ≥ 1, para a réplica N do serviço `taba`); (4) saída ordenada do `taba-1` (`stop`, com drain); (5) **nenhuma mensagem confirmada se perdeu**; (6) estado final. **Rode antes de fechar mudanças que afetem cluster, canal, Compose ou conectores.**

## Dados de demonstração: `docker/seed-demo.sh`

Para testes manuais, testes de carga e demonstrações (a TUI, os gráficos, a condição `noisy-group`), o `docker/seed-demo.sh` sobe um cluster já populado em poucos minutos:

```bash
docker/seed-demo.sh                    # do zero; deixa o cluster no ar no fim
docker/seed-demo.sh --no-build         # reaproveita as imagens já construídas
docker/seed-demo.sh --messages 10000   # outra quantidade por distribuidor (padrão 100000; mínimo 1000)
docker/seed-demo.sh --no-profile         # não mexe no perfil do taba-cli
PARALLEL=16 docker/seed-demo.sh        # mais contêineres publicando ao mesmo tempo (padrão 8)
```

O que ele faz, nesta ordem:

1. **Apaga tudo:** `docker compose down -v` (volumes, segredos, dados dos nós e o bucket do backup) e sobe o cluster de três nós construindo as imagens deste repositório (o mesmo `taba_up` do `validate.sh`). O que estava nesse Compose se perde. Precisa da porta 8080 livre.
2. **Cria um token admin local.** O **token raiz** (acesso total, do volume `secrets` do Compose) só serve para começar: é o único que pode criar um token `admin`, e o script o usa para isso (pelo `taba-cli` do Compose) e em nenhum outro lugar da sua máquina. O script cria o token **`local-admin`** com a permissão `admin` (gerencia tokens e opera o cluster) e grava o segredo, que só aparece na resposta da criação, em **`docker/.local/admin-token`**, sem quebra de linha no fim e com modo `0600` onde o sistema permite. Depois chama `GET /auth/me` para conferir que a API o aceita como `admin` (`"root": false`). Esse é o jeito certo de usar o Taba: um token com nome e revogável, não o raiz ([14-seguranca.md](14-seguranca.md)). O arquivo fica só na sua máquina: `docker/.local/` e todo arquivo chamado `admin-token` estão no `.gitignore`. Cada execução **recria** o volume, então o token é **novo** e o arquivo é sobrescrito (`ADMIN_NAME=` muda o nome do token). Ele aparece na aba Tokens do `taba-cli tui` e em `GET /tokens`; o raiz não, porque não vem da tabela. Use assim:

   ```bash
   curl -H "Authorization: Bearer $(cat docker/.local/admin-token)" http://localhost:8080/topics
   ```

   Para outro caminho: `TOKEN_FILE=/caminho/token docker/seed-demo.sh`. O `taba-cli` do Compose não precisa do arquivo (ele lê o token do próprio volume).
3. **Configura o perfil do `taba-cli`.** Se há um `taba-cli` instalado na máquina (`TABA_CLI_BIN`, o `PATH`, ou o build deste repositório em `connectors/rust/target/release`), o script aponta o perfil **`local_admin`** (`PROFILE=` muda o nome) para `http://localhost:8080` e para o novo token: se o perfil existe, só o `url` e o `token` mudam (o token vira `token-file`, apontando para `docker/.local/admin-token`); se não existe, ele é criado e vira o **padrão**. Depois, `taba-cli topic list` funciona sem opções. `--no-profile` pula o passo; sem `taba-cli` ele é pulado, com o comando para rodar à mão. O arquivo de perfis é o de [20-cli.md](20-cli.md#perfis) (`TABA_CLI_CONFIG` muda o caminho).
4. **Cria os tópicos** (as filas primeiro, porque o distribuidor só copia para as filas que existem na hora em que publica) e **publica** as mensagens com `taba-cli publish --count`, em contêineres paralelos.

| Tópico | Tipo | Conteúdo |
|---|---|---|
| `demo-orders-billing` | fila | 100.000 mensagens (cópias de `demo-orders`) |
| `demo-orders-stock` | fila | 100.000 mensagens (cópias de `demo-orders`) |
| `demo-events-audit` | fila | 100.000 mensagens (cópias de `demo-events`): **a fila barulhenta** |
| `demo-orders` | distribuidor, prefixo `demo-orders-` | recebe 100.000 publicações; não guarda nada |
| `demo-events` | distribuidor, prefixo `demo-events-` | recebe 100.000 publicações; não guarda nada |
| `demo-payments-fifo` | fila FIFO | 10.000 mensagens (10% da quantidade), publicadas direto: 20 contas (`account-01` … `account-20`) × 500, numeradas com `seq` 1…500 |

Assim as três filas terminam com 100.000 mensagens cada (mais as filas `-dead-letter`, vazias; a fila FIFO tem as 10.000 acima), e os dois distribuidores é que recebem as 200.000 publicações. O que muda são os grupos ([12-entrega.md](12-entrega.md)):

- **`demo-payments-fifo` (FIFO):** cada conta é um grupo (`groupId`) e a fila mantém a ordem *dentro* do grupo: consumir devolve `seq` 1, 2, 3… de cada conta (`taba-cli consume demo-payments-fifo --max 10`). Toda mensagem tem `deduplicationId` (`account-NN-seq`), porque a FIFO descarta corpo repetido no mesmo grupo.
- **`demo-orders-*` (equilibradas):** 100 grupos (`customer-001` … `customer-100`) com 1.000 mensagens cada.
- **`demo-events-audit` (barulhenta):** 95% das mensagens (95.000) são de um único grupo, `noisy-integration`, e os outros 5% se espalham em 50 grupos (`service-01` … `service-50`, 100 cada). É o exemplo do vizinho barulhento: a entrega prefere os grupos com menos mensagens, então quem consome esta fila recebe primeiro as mensagens `service-NN`. Para ver: `taba-cli stats groups demo-events-audit` (ou `/stats/topics/demo-events-audit/groups`) mostra `noisy-integration` com `share` 0,95, e `/stats/topics/demo-events-audit?scope=node` mostra `noisyShare` ≈ 0,95 e a condição `noisy-group` ([13-observabilidade.md](13-observabilidade.md)). A visão do cluster inteiro (sem `scope=node`) dá um **limite inferior** do `noisyShare` (≈ 0,33 com três nós, porque cada nó guarda um terço das mensagens) e **não** levanta a condição: medido com `--messages 2000`.

No fim o script imprime a profundidade de cada fila e deixa o cluster no ar: explore (`docker compose run --rm taba-cli tui`, `stats topics`, `consume`) e apague tudo com `docker/cluster.sh down`. Com `--messages N`, as quantidades escalam (N por distribuidor, 95% barulhentas); os 100.000 acima são o padrão.

> O tempo de publicação depende da máquina (cada publicação espera o quórum, passando pelo HAProxy); o script imprime quanto levou. A falha de qualquer publicador interrompe o script com erro.

## Estratégia: o que cada nível protege

| Risco | Onde se pega |
|---|---|
| Regra pura errada (quórum, anel, faixa de ID) | `U` |
| Contrato HTTP quebrado (status, corpo, erro) | `I` |
| Perda de mensagem, quórum, split, votação, freezing, sincronização | `C` (determinístico, com falha injetada) |
| Segurança de TLS e rotação de certificados | `RealCluster` (TCP real) |
| Integração real (imagem, Compose, HAProxy, `SIGTERM`) | `validate.sh` |
| Conector divergente do contrato | `K` |

⬜ **Faltam:** testes de **desempenho e carga** (as metas de velocidade e custo não têm benchmark), de **Kubernetes** e a **suíte de conformidade compartilhada** entre conectores ([32-roadmap.md](32-roadmap.md)). `docker/test-cpu.sh` é um roteiro de medição de CPU.

## Problemas conhecidos

O teste desligado com `#[ignore]` por falhar de forma intermitente no CI, com o que se sabe da causa e o próximo passo, está em [36-problemas-conhecidos.md](36-problemas-conhecidos.md).

## Fontes e provas

- `crates/taba-node/tests/*.rs` e `tests/common/{mod,cluster,real,otlp}.rs`, `src/cluster/transport/memory`, `src/core/clock`, `tests/uc_coverage.rs`, `docker/{cluster,validate,seed-demo}.sh`, `connectors/rust/**/tests.rs`, `connectors/csharp/Taba.Client.Tests`.
- O placar "702 de 885" e a falha da coluna "Testes" foram **executados** em 2026-10-13 (`cargo test --test uc_coverage -- --nocapture`). Também **executados e verdes** nessa data: `cargo test --lib` (**509 testes unitários**, 2,25 s) e os arquivos `topics` (21), `messages` (41), `failures` (21), `freezing` (5) e `drain_cluster` (7). **O restante da suíte não foi executado** nesta verificação: a máquina ficou sem espaço em disco no *link* (`LNK1201`), o problema do `target/` descrito em [24-convencoes.md](24-convencoes.md).
- Casos de uso: todos (cada teste cita os seus); veja [26a](26a-casos-de-uso-implementados.md).
- Histórico: `doc/20-casos-de-uso.md` (19.4, 19.5), `18-estrutura-codigo.md` (17.5).
