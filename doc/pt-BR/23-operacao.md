---
título: Operação (runbook)
resumo: Guia prático para quem roda um cluster Taba — subir, verificar, escalar, atualizar, retirar nós, diagnosticar cada condição de saúde, o que fazer em cada falha e o que não fazer.
tipo: guia
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [docker-compose.yml, docker/haproxy.cfg, docker/cluster.sh, docker/validate.sh, crates/taba-node/src/cluster/health, crates/taba-node/src/cluster/drain, crates/taba-node/src/http/operations.rs]
---

# Operação (runbook)

> Leia antes: [07-cluster.md](07-cluster.md) e [13-observabilidade.md](13-observabilidade.md). Os testes deste guia foram **executados** num cluster de três nós subido por `docker compose up` (Windows, Docker 29); onde algo é só orientação, está dito.

## Resumo para quem está de plantão

| Regra | Por quê |
|---|---|
| **Reinicie e atualize um nó por vez**, esperando `healthy` entre eles | Perder dois nós em sequência rápida **trava** o cluster (`quorum-lost`) e a saída é reiniciar tudo, **perdendo mensagens e tópicos** |
| **Pare nós com `SIGTERM`** (`docker stop`, `kubectl delete pod`), com prazo de parada **maior** que `drain.timeout` | Dispara o **drain**: sem ele, o que está só em memória se perde e o cluster só o tira depois da votação |
| **Nunca pare o cluster inteiro** em produção | Todo reinício é de nó novo: com todos parados **as mensagens e os tópicos se perdem** |
| **Proteja as portas de operação e do cluster** na rede | Não têm autenticação (operação) / confiam no certificado (cluster); `POST /node/drain` tira um nó do ar |
| **Tenha ≥ 3 nós** | Com 2, a falha de um deixa um sobrevivente que não alcança `cluster.vote.min-voters` |

## Subir um cluster

Receita completa e executável em [02-inicio-rapido.md](02-inicio-rapido.md). Em resumo:

1. **Crie a pasta de segredos uma vez** (`taba init --out <pasta>`) e entregue a **todos** os nós como segredo da plataforma ([14-seguranca.md](14-seguranca.md)).
2. **Defina** `cluster.entry-address` (o endereço do balanceador na **porta do cluster**, em TCP/L4, sem terminar o TLS) em todos os nós, e `cluster.first-node=true` **só no primeiro nó de um cluster novo**.
3. **Suba o primeiro nó** e espere ficar `healthy`; **depois os outros**. Cada um fica pronto (`/health/ready`) só ao **terminar de sincronizar**.
4. **Configure o balanceador de clientes** para só enviar tráfego a nós com `/health/ready` = `200` (porta de operação).

**Verifique:**

```bash
taba-cli health cluster -o table        # 3 nós ativos, "conditions" vazia, worstCondition "ok"
taba-cli health ready                   # {"health":"ready"} em cada nó
```

Esperado num cluster saudável: `summary.members.active = N`, `quorumAvailable: true`, `syncingBlocks: 0`, `conditions: []`.

> **Primeiro nó e reinício:** `cluster.first-node=true` só inicia o cluster sozinho se **nenhuma resposta de rede** vier do endereço de entrada. Se os outros nós estão de pé, ele **entra** neles. Por isso é seguro deixar a chave ligada nele para sempre (o Compose faz isso).

## Escalar

- **Adicionar um nó:** suba outro com a **mesma imagem, a mesma pasta de segredos** e o **mesmo** `cluster.entry-address`, **sem** `first-node`. Ele descobre o cluster, entra por registro + commit em **todos** os nós, sincroniza os blocos que assumiu e só então fica pronto. Entradas são **uma por vez**: se há outra mudança ou votação em curso, o pedido espera.
- **Efeito no anel:** o nó novo assume blocos; os antigos só apagam o que perderam quando o cluster está **íntegro** ([06-anel-e-replicacao.md](06-anel-e-replicacao.md)).
- **Reduzir:** use **drain** (abaixo), nunca derrube o nó à força de propósito. Depois de um nó sair, a redundância das mensagens dele fica em `replication.factor − 1` até entrar um novo nó (aceito, D116). **Não deixe o cluster com menos de 3 nós** em produção.
- Limites de capacidade: um nó guarda **uma cópia de cada mensagem dos blocos de que é responsável**; com `replication.factor=3` e 3 nós, **todo nó guarda tudo** (a memória de cada nó precisa comportar o tópico inteiro, ou use `memory.limit` para ir ao disco).

## Atualizar a versão (rolling)

**Um nó por vez**, esperando `healthy` (`/health/ready`) antes do próximo. Verificado: reiniciar `taba-2`, `taba-3` e `taba-1` em sequência (`docker compose restart <nó>`) com 14 mensagens no tópico e consumir tudo devolveu **14 de 14**.

1. Pare o nó com `SIGTERM` (ele faz drain, grava a memória em disco, se desregistra e encerra com código 0).
2. Suba a versão nova (ela entra como **nó novo**, sincroniza, fica pronta).
3. Confira `health cluster` (sem condições) e **só então** vá ao próximo.

> **Todos os nós de um cluster devem rodar a mesma versão:** a API do cluster é interna e não tem garantia de compatibilidade entre versões ([18-api-cluster.md](18-api-cluster.md)). O `membersVersion` sobe a cada mudança de membros, o que é normal num rolling.

O `stop_grace_period` (Compose) ou `terminationGracePeriodSeconds` (Kubernetes, 30 s por padrão) precisa ser **maior que `drain.timeout`** (25 s). O Compose do repositório usa **40 s**. Senão o processo é morto antes de registrar o resultado.

## Retirar um nó de propósito (drain)

```bash
# no nó a retirar, pela porta de operação DELE (não pelo balanceador)
taba-cli drain start       # POST /node/drain
taba-cli drain status      # drainState: draining → drained; pendingMessages cai a 0
```

Verificado: `drained` em ~4 s; `/health/ready` passa a `503`; o cluster fica com os nós restantes ativos e sem condições. O nó **continua de pé** (repassando chamadas de cliente a outro nó); pare o processo quando quiser. Para **voltar**: `taba-cli drain cancel` — antes da saída cancela; depois da saída **volta como nó novo** (em ~20 s o cluster voltou a 3 ativos, sem condições).

Sair por drain **não** gera freezing. Se o drain estourar `drain.timeout` pedido pela API, o nó fica `draining` até alguém chamar `drain cancel` (D75).

## O que significa cada condição, e o que fazer

`GET /health/cluster` devolve a **lista** de condições. Combine-as: a mesma falha costuma ligar várias.

| Condição | Severidade | O que significa | O que fazer |
|---|---|---|---|
| `member-unreachable` | aviso | O canal com um nó caiu e está em reconexão, ainda antes da votação | **Espere ~10 s**: se o nó volta, some. Se não, o cluster vai excluí-lo. Verifique a máquina/contêiner do nó (`docker compose ps`, logs) |
| `write-quorum-at-risk` | **crítica** | Algum bloco tem réplicas íntegras abaixo do quórum, ou o **primário** está fora e ainda na tabela | **Publicações no bloco falham** (`503 cluster.quorum-unavailable`) até o nó ser excluído ou voltar. Comum nos primeiros segundos de uma queda. **Persistente = problema real**: ache o nó e reinicie-o |
| `under-replicated` | aviso | Blocos com menos cópias que `replication.factor`, mas ainda no quórum | **Normal e transitório** depois de uma queda ou de uma entrada. Some quando entra o nó novo e sincroniza. Persistente com todos os nós vivos: veja `blocksBelowReplication` e os logs de sincronização |
| `node-frozen` | aviso | Há nó(s) excluído(s) por votação em freezing (padrão 5 min) | Esperado depois de uma exclusão. Um nó **reiniciado** (ID novo) **não** fica em freezing; só o mesmo processo que volta |
| `syncing` | aviso | Algum nó está sincronizando; a entrega está suspensa nos blocos dele | Normal ao entrar um nó. Se **não termina**, veja os logs `sync`/`falha no bootstrap` e `cluster.sync.timeout` (60 s sem origem) |
| `quorum-lost` | **crítica** | Menos nós saudáveis que `cluster.vote.min-voters` (2): **nenhuma exclusão se efetiva** | Há **um só** nó vivo. Veja "Cluster travado" abaixo |
| `membership-divergent` | **crítica** | Tabelas de membros com versões diferentes | Propagação incompleta de uma mudança; normalmente some sozinha em segundos. Persistente: veja os logs `membership` e reinicie o nó divergente (**um de cada vez**) |
| `storage-pressure` | aviso | Espaço livre em `storage.dir` abaixo de `stats.disk.free-min` (só dispara se configurado) | Libere disco ou aumente o volume **antes** de encher: **disco cheio leva o nó a `failed`** |

## Falhas e como reagir

### Um nó caiu (processo morto ou máquina)

Esperado, **sem ação**: em ~4 s aparecem `write-quorum-at-risk`, `member-unreachable`, `under-replicated`; em ~8 s o nó é excluído (versão da tabela sobe) e fica só `node-frozen`. Nada que foi confirmado se perde (as cópias estão em outros nós). **Suba o nó de novo**: ele entra como nó novo e sincroniza. Verificado com `docker compose kill taba-2` seguido de `start`.

Durante a janela de ~4 s, publicações nos blocos do nó caído **podem** responder `503` com `Retry-After`: o cliente repete.

### Cluster travado (`quorum-lost`)

**Quando acontece:** dois nós de um cluster de três caem **em sequência rápida, antes do primeiro voltar**. Sobra um; a exclusão do segundo não se efetiva; os reiniciados **não reentram** (o registro exige **todos** os nós da tabela, e a tabela ainda lista o morto: logs `cluster.registration-failed` e `cluster.node-syncing` em laço).

**Sintomas:** `conditions = [write-quorum-at-risk, quorum-lost, member-unreachable, node-frozen]`; publicações e consumos em partes dos blocos falham ou voltam vazios.

**Não há recuperação automática nem comando de exclusão forçada.** O que funcionou (verificado): **derrubar o cluster inteiro e subir de novo**. ⚠️ **Isso perde as mensagens e os tópicos** (o cluster nasce vazio). Antes de fazê-lo, se o sobrevivente ainda atende, **consuma e salve o que puder**. Prevenção: um nó por vez; considere aumentar o número de nós (5) e `cluster.vote.min-voters` (uma maioria) em clusters grandes.

### Todos os nós caíram

Em produção, **mensagens e tópicos se perdem** (todo reinício é de nó novo; o disco só devolve "perdas" listadas por nós vivos). Suba o primeiro nó (`first-node`), depois os demais; **recrie os tópicos e tokens**. O **token raiz e o certificado** estão na pasta de segredos e sobrevivem. Planeje para isto: produtores com repetição e tópicos recriáveis por automação.

### Nó em `failed` (disco cheio ou banco corrompido)

O nó deixa de atender **chamadas de cliente** (repassa a outro nó, ou `503 node.failed`), segue respondendo as chamadas entre nós e as rotas de operação, e **os demais ficam liberados para votar a exclusão dele**. **Só reiniciar o processo o tira de `failed`** (não há reavaliação em execução). Antes de reiniciar: libere disco. Banco corrompido: **não há reparo**; reinicie com disco novo se preciso (o nó entra como novo).

### Nó que não fica pronto

`/health/ready` devolve `503` com `reason`: `bootstrapping` (entrando), `syncing` (sincronizando), `returning`, `draining`/`drained`, `failed`. Se fica em `bootstrapping`: confira `cluster.entry-address` (porta **do cluster**, em TCP), o certificado (a mesma pasta de segredos), e os logs "falha no bootstrap" (`a listagem de nós em … falhou`, `cluster.registration-failed`, `cluster.node-frozen` com o tempo restante).

### Publicações respondendo `503`

| Código | Causa provável | Ação |
|---|---|---|
| `cluster.quorum-unavailable` | Sem nós para fechar o quórum, ou o primário do bloco/grupo fora | Veja `health cluster`; repita após `Retry-After` |
| `cluster.commit-timeout` | Registro confirmado, commit lento | Em tópico/token a operação **vai se completar**; consulte antes de repetir |
| `cluster.node-syncing` / `cluster.node-draining` | O nó atendeu mas não achou outro para repassar | Tire o nó do balanceador; repita |
| `fifo.primary-unavailable` | O primário do grupo FIFO fora, sincronizando ou esgotou tentativas | Veja `health cluster`; repita |

## Certificados: rotação sem reiniciar

Procedimento em [14-seguranca.md](14-seguranca.md). Resumo: gerar o par novo com `--starts-in` maior que o tempo de propagação, **acrescentar** à pasta (o antigo fica), e esperar. Monitore `taba.cluster.tls.expires_in` (alerta com < 30 dias) e os `warn` de expiração (`cluster.tls.expiry-warning`). **Antes** de o certificado vencer: um vencido deixa de ser aceito e o nó perde o cluster.

## Trocar o token raiz

Edite o arquivo do token raiz com **duas linhas** (a nova e a anterior); cada nó o relê em até `auth.reload-interval` (60 s) e aceita as duas; depois retire a anterior. Os tokens já criados seguem valendo.

## Backup e restauração

⬜ **Não há backup nem restauração.** A durabilidade vem da **replicação**, não do disco de um nó: copiar `storage.dir` de um nó **não** é um backup restaurável (um nó que reinicia entra como nó novo e o disco antigo só serve ao retorno de um nó excluído). O que se deve guardar: a **pasta de segredos** (`taba init`) e **a definição dos tópicos e tokens** em automação (IaC). Veja o roadmap ([32-roadmap.md](32-roadmap.md)).

## Monitorar

- **Mínimo:** sonda `liveness` em `/health/live`, `readiness` em `/health/ready`; alerta em `worstCondition != "ok"` por mais de alguns minutos; `up == 0`; `quorumAvailable == false`.
- **Prometheus:** raspe `GET /metrics` de **cada** nó (um alvo por nó). Alertas de exemplo em [13-observabilidade.md](13-observabilidade.md).
- **Tópicos:** `taba-cli stats topics --condition backlog` (limiares `stats.topics.*` precisam estar configurados). ⚠️ A agregação **entre nós** das métricas de tópico não está implementada: consulte cada nó ou some no backend.
- **Logs:** `telemetry.logs.format=json` para ingestão; `color` para terminal. Procure `error`: perda de mensagem (`taba.message.lost`), falha de bootstrap, drain que estourou o prazo.

## Kubernetes (orientação, **não validada**)

O repositório **não tem** manifestos nem Helm. O que o nó oferece para encaixar: sondas `/health/live` e `/health/ready` na porta de operação; parada por `SIGTERM` com drain (`terminationGracePeriodSeconds` > `drain.timeout`); `cluster.entry-address` apontando para um `Service` **interno** (ou *headless*) que repasse a porta do cluster **em TCP**, sem terminar o TLS; a pasta de segredos como `Secret` montado; `storage.dir` num volume por pod (a última pasta chamada `taba`). `StatefulSet` ou `Deployment` servem (o nó **não depende** de identidade estável: toda partida é de nó novo). Rolling update: `maxUnavailable: 1`.

## Docker Compose do repositório: comandos úteis

```bash
docker compose up --build -d                       # sobe tudo
docker compose ps                                  # saúde dos nós
docker compose run --rm taba-cli health cluster -o table
docker compose logs -f taba-1                      # logs coloridos
docker compose stop taba-3     / start taba-3      # parada normal (drain) e volta
docker compose kill taba-3                         # simula falha
docker compose restart taba-2                      # reinício (um por vez!)
docker compose down                                # ⚠️ PARA TUDO: o cluster nasce vazio no próximo up
docker compose down -v                             # apaga também os volumes (inclui o token raiz)
```

`docker/cluster.sh` (para `source` ou comando) sobe/derruba o Compose, provoca falhas (`taba_kill`, `taba_stop`, `taba_start`), espera o cluster estabilizar e cria tokens e tópicos; `docker/validate.sh` roda o roteiro completo (≈ 5 minutos: cluster forma, `taba-cli`, queda e volta de nós, **ausência de perda de mensagens**).

> **No Windows com Git Bash**, defina `MSYS_NO_PATHCONV=1` para comandos que levam caminhos do contêiner.

## Fontes e provas

- `cluster/health/mod.rs` (as 8 condições), `cluster/drain`, `cluster/state`, `cluster/service` (bootstrap, ingresso, freezing, votação), `http/operations.rs`, `docker-compose.yml`, `docker/`.
- **Execuções reais** deste guia (cluster de 3 nós, `docker compose`): kill de um nó (condições em 4 s e 8 s), parada normal com drain, drain pela API (`drained` em ~4 s e volta como nó novo), rolling restart dos três nós (14/14 mensagens), perda de dois nós seguidos (`quorum-lost`, reentrada impossível), `down`+`up` (cluster nasce vazio).
- Casos de uso: UC-FAL, UC-FRZ, UC-DRN, UC-RTN, UC-ING, UC-OPS; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/failures.rs`, `freezing.rs`, `drain_cluster.rs`, `recovery.rs`.
- Histórico: `04-cluster.md`, `11-observabilidade.md`.
