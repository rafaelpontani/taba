---
título: Operação (runbook)
resumo: Guia prático para quem roda um cluster Taba — subir, verificar, escalar, atualizar, retirar nós, diagnosticar cada condição de saúde, o que fazer em cada falha e o que não fazer.
tipo: guia
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [docker-compose.yml, docker/haproxy.cfg, docker/cluster.sh, docker/validate.sh, crates/taba-node/src/cluster/health, crates/taba-node/src/cluster/drain, crates/taba-node/src/http/operations.rs]
---

# Operação (runbook)

> Leia antes: [07-cluster.md](07-cluster.md) e [13-observabilidade.md](13-observabilidade.md). Os testes deste guia foram **executados** num cluster de três nós subido por `docker compose up` (Windows, Docker 29); onde algo é só orientação, está dito.

## Resumo para quem está de plantão

| Regra | Por quê |
|---|---|
| **Reinicie e atualize um nó por vez**, esperando `healthy` entre eles | Perder dois nós em sequência rápida **trava** o cluster (`quorum-lost`) e a saída é reiniciar tudo, **perdendo mensagens e tópicos** |
| **Pare nós com `SIGTERM`** (`docker stop`, `kubectl delete pod`), com prazo de parada **maior** que `drain.timeout` | Dispara o **drain**: sem ele, o que está só em memória se perde e o cluster só o tira depois da votação |
| **Nunca pare o cluster inteiro** em produção **sem o backup em S3** (ou sem `POST /cluster/stop`) | Todo reinício é de nó novo: com todos parados **as mensagens e os tópicos se perdem**. Com o backup, pare com `POST /cluster/stop` ([33-backup-s3.md](33-backup-s3.md)) |
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
taba-cli cluster status -o table        # 3 nós ativos, "conditions" vazia, worstCondition "ok"
taba-cli node list -o table                # todos os nós com estado "active"; a sonda é curl http://no:9090/health/ready
```

Esperado num cluster saudável: `summary.members.active = N`, `quorumAvailable: true`, `syncingBlocks: 0`, `conditions: []`.

> **Primeiro nó e reinício:** `cluster.first-node=true` só inicia o cluster sozinho se **nenhuma resposta de rede** vier do endereço de entrada. Se os outros nós estão de pé, ele **entra** neles. Por isso é seguro deixar a chave ligada nele para sempre (o Compose faz isso).
>
> **Partida a frio:** se todos os nós caem e voltam juntos, com os discos, **nenhum precisa de `cluster.first-node`**: cada um tenta o endereço de entrada e os IPs da tabela da vida anterior (no manifesto); passado `cluster.restore.cold-start.wait`, parte sozinho o de **menor ID antigo** entre os vivos, assume o disco (tópicos, tokens, mensagens em quarentena) e os outros entram nele. A restauração das mensagens espera os nós esperados (`cluster.restore.grace`); a FIFO só é liberada depois. Um nó que nunca teve disco ainda precisa de `cluster.first-node` para criar o cluster.

## Escalar

- **Adicionar um nó:** suba outro com a **mesma imagem, a mesma pasta de segredos** e o **mesmo** `cluster.entry-address`, **sem** `first-node`. Ele descobre o cluster, entra por registro + commit em **todos** os nós, sincroniza os blocos que assumiu e só então fica pronto. Entradas são **uma por vez**: se há outra mudança ou votação em curso, o pedido espera.
- **Efeito no anel:** o nó novo assume blocos; os antigos só apagam o que perderam quando o cluster está **íntegro** ([06-anel-e-replicacao.md](06-anel-e-replicacao.md)).
- **Reduzir:** use **drain** (abaixo), nunca derrube o nó à força de propósito. Depois de um nó sair, a redundância das mensagens dele fica em `replication.factor − 1` até entrar um novo nó (aceito, D116). **Não deixe o cluster com menos de 3 nós** em produção.
- Limites de capacidade: um nó guarda **uma cópia de cada mensagem dos blocos de que é responsável**; com `replication.factor=3` e 3 nós, **todo nó guarda tudo** (a memória de cada nó precisa comportar o tópico inteiro, ou use `memory.limit` para ir ao disco).

## Atualizar a versão (rolling)

**Suba a versão nova ao lado da antiga: os nós novos retiram os antigos.** Cada nó anuncia a versão do programa (o campo `version` do seu `NodeStatus`), e um nó que vê membros de versão **anterior** os drena, **um de cada vez**, com o mesmo drain de "Retirar um nó de propósito": o nó antigo grava a memória em disco e sai da tabela, e o orquestrador pode então pará-lo.

1. Adicione um nó da versão nova (Compose: aumente as réplicas com a imagem nova; Kubernetes: o rollout acrescenta o pod novo). Ele entra como **nó novo** e sincroniza.
2. Quando ele está `active`, o cluster drena o membro mais antigo. O drain seguinte só começa depois que o anterior saiu da tabela, e **só se pelo menos 3 nós continuarem ativos** depois dele: num cluster de 3, suba primeiro o nó novo (4 ativos) para que um antigo possa sair.
3. Repita até não restar nó de versão anterior e confira `taba-cli cluster status` (sem condições).

Só **um** nó age: o de menor `nodeId` entre os da versão mais nova, para que dois nós nunca peçam drains diferentes ao mesmo tempo. Um nó de versão **anterior** que entra não drena os mais novos; os novos o retiram. Um membro que não publica versão (uma versão anterior do programa) conta como o mais antigo.

Se um nó não termina o drain em `drain.timeout` mais 30 s, o processo **para nele** e aparece a condição `upgrade-stalled` (aviso): cancele o drain do nó (`DELETE /node/drain`) ou pare-o, e a atualização segue.

**Reiniciar no lugar, um nó por vez,** também funciona, esperando `healthy` (`/health/ready`) antes do próximo. Verificado: reiniciar `taba-2`, `taba-3` e `taba-1` em sequência (`docker/cluster.sh restart <nó>`) com 14 mensagens no tópico e consumir tudo devolveu **14 de 14**.

> As versões **não** são compatíveis entre si: a API do cluster é interna e não tem garantia de compatibilidade entre versões ([18-api-cluster.md](18-api-cluster.md)); o que uma versão nova acrescenta entre nós só é enviado aos nós que anunciaram essa versão. O `membersVersion` sobe a cada mudança de membros, o que é normal numa atualização.

> ⚠️ **A versão que introduziu o lote (`POST /cluster/batch`, [D186](30-decisoes.md)) não é alcançada por uma atualização em rolagem a partir de uma anterior.** As operações sobre mensagens entre nós mudaram de rota e de formato (protobuf), sem compatibilidade: um nó novo não entende as rotas de uma operação, e um antigo não tem a rota de lote, então enquanto houver nós das duas versões no cluster as escritas que precisam de uma réplica da outra versão falham. **Pare o cluster inteiro e suba a versão nova com os mesmos volumes** (o cluster retoma do disco, veja "Todos os nós caíram"). A partir da versão seguinte, vale de novo a regra acima.

O `stop_grace_period` (Compose) ou `terminationGracePeriodSeconds` (Kubernetes, 30 s por padrão) precisa ser **maior que `drain.timeout`** (25 s). O Compose do repositório usa **40 s**. Senão o processo é morto antes de registrar o resultado.

## Retirar um nó de propósito (drain)

```bash
# de fora, pela API de cliente (token admin): a CLI nomeia o nó, qualquer que seja o escolhido pelo balanceador
taba-cli node list                          # os ids
taba-cli drain start --node <id>            # POST /nodes/{id}/drain
taba-cli drain status --node <id>           # drainState: draining → drained; pendingMessages cai a 0
# de dentro da infraestrutura, na porta de operação do próprio nó (não pelo balanceador)
curl -s -X POST http://no:9090/node/drain
```

Verificado: `drained` em ~4 s; `/health/ready` passa a `503`; o cluster fica com os nós restantes ativos e sem condições. O nó **continua de pé** (repassando chamadas de cliente a outro nó); pare o processo quando quiser. Para **voltar**: `taba-cli drain cancel --node <id>` — antes da saída cancela; depois da saída **volta como nó novo** (em ~20 s o cluster voltou a 3 ativos, sem condições).

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
| `upgrade-stalled` | aviso | A atualização que retira versões anteriores parou num nó que não terminou o drain a tempo | Cancele o drain desse nó (`DELETE /node/drain` na porta de operações dele) ou pare-o; a atualização segue com o próximo. O nó é o que está em `draining` em `GET /nodes` |
| `members-missing` | aviso | A sondagem dos IPs antigos acabou com nós da vida anterior faltando | O cluster funciona; o nó que falta entra pelo join normal quando voltar. Se ele não vai voltar (o cluster foi reduzido de propósito), a condição some quando a tabela completar ou ao reiniciar os nós ([07-cluster.md](07-cluster.md), seção 10.11) |
| `reduced-profile` | aviso | O nó roda o perfil de um nó (desenvolvimento) | Esperada em desenvolvimento. Em produção, mostra que algo relaxado ficou ligado: confira `replication.factor`, `cluster.tls.enabled` e `auth.dev-token` ([07-cluster.md](07-cluster.md), seção 8) |
| `previous-life-pending` | aviso | A restauração das vidas anteriores não terminou (a votação não fechou, ou a restauração espera nós que não voltaram) | As filas FIFO recebem e **não entregam** enquanto isso. `GET /health/cluster` e o estado do nó (`GET /cluster/restore`) dizem quais nós antigos são esperados. Espere `cluster.restore.grace`; se um nó não vai voltar, force com `POST /cluster/restore/force` (token root, ou a porta de operações): mensagens já consumidas podem voltar ([07-cluster.md](07-cluster.md), seção 10.10) |

## Falhas e como reagir

### Um nó caiu (processo morto ou máquina)

Esperado, **sem ação**: em ~4 s aparecem `write-quorum-at-risk`, `member-unreachable`, `under-replicated`; em ~8 s o nó é excluído (versão da tabela sobe) e fica só `node-frozen`. Nada que foi confirmado se perde (as cópias estão em outros nós). **Suba o nó de novo**: ele entra como nó novo e sincroniza. Verificado com `kill` do `taba-2` seguido de `start` (feito, na época, pelo `docker compose`; hoje por `docker/cluster.sh kill taba-1`).

Durante a janela de ~4 s, publicações nos blocos do nó caído **podem** responder `503` com `Retry-After`: o cliente repete.

### Cluster travado (`quorum-lost`)

**Quando acontece:** dois nós de um cluster de três caem **em sequência rápida, antes do primeiro voltar**. Sobra um; a exclusão do segundo não se efetiva; os reiniciados **não reentram** (o registro exige **todos** os nós da tabela, e a tabela ainda lista o morto: logs `cluster.registration-failed` e `cluster.node-syncing` em laço).

**Sintomas:** `conditions = [write-quorum-at-risk, quorum-lost, member-unreachable, node-frozen]`; publicações e consumos em partes dos blocos falham ou voltam vazios.

**Não há recuperação automática nem comando de exclusão forçada.** O que funciona: **derrubar o cluster inteiro e subir de novo, com os mesmos volumes**. O cluster retoma do disco (veja "Todos os nós caíram"): tópicos, tokens e as mensagens que estavam em disco voltam, e se perde o que estava só em memória. Antes de fazê-lo, se o sobrevivente ainda atende, **consuma e salve o que puder**; sem os volumes o cluster nasce vazio. Prevenção: um nó por vez; considere aumentar o número de nós (5) e `cluster.vote.min-voters` (uma maioria) em clusters grandes.

### Todos os nós caíram

**Com o backup em S3** ([33-backup-s3.md](33-backup-s3.md)): suba os processos de novo (`docker compose start`). O primeiro nó que não achar ninguém batendo no bucket espera o batimento dos mortos vencer (`backup.dead-after`, 60 s), pega a trava de restauração, restaura definições, tokens e mensagens, e os demais entram como nós normais. Perde-se só o que ainda não tinha ido ao S3 (a janela de `backup.delay`) e o estado da última janela do diário.

**Sem o backup, com os volumes mantidos:** suba todos os nós de novo (`docker compose start`), sem `first-node`. Cada nó põe o disco da vida anterior numa pasta `_lives/<ID antigo>`; passado `cluster.restore.cold-start.wait` (15 s) parte sozinho o nó de menor ID antigo entre os vivos e assume o disco (tópicos e tokens), os outros entram nele, e as mensagens voltam pela restauração das vidas anteriores ([07-cluster.md](07-cluster.md), seção 10). Acompanhe `GET /health/cluster`: `previous-life-pending` some quando a restauração termina. Se um nó antigo não volta, a restauração segue com os presentes depois de `cluster.restore.grace` (5 min), ou antes, forçada (`POST /cluster/restore/force`); com `cluster.restore.strict` ela espera a força. Perde-se o que estava **só em memória** em todos os nós, e uma mensagem já consumida só reaparece nos casos da seção 10.10 (o consumidor precisa ser idempotente). **Sem os volumes** mensagens e tópicos se perdem: recrie os tópicos e tokens. O **token raiz e o certificado** estão na pasta de segredos e sobrevivem. Planeje para isto: produtores com repetição e tópicos recriáveis por automação.

### Nó em `failed` (disco cheio ou banco corrompido)

O nó deixa de atender **chamadas de cliente** (repassa a outro nó, ou `503 node.failed`), segue respondendo as chamadas entre nós e as rotas de operação, e **os demais ficam liberados para votar a exclusão dele**. **Só reiniciar o processo o tira de `failed`** (não há reavaliação em execução). Antes de reiniciar: libere disco. Banco corrompido: **não há reparo**; reinicie com disco novo se preciso (o nó entra como novo).

### Nó que não fica pronto

`/health/ready` devolve `503` com `reason`: `bootstrapping` (entrando), `syncing` (sincronizando), `returning`, `draining`/`drained`, `failed`. Se fica em `bootstrapping`: confira `cluster.entry-address` (porta **do cluster**, em TCP), o certificado (a mesma pasta de segredos), e os logs "falha no bootstrap" (`a listagem de nós em … falhou`, `cluster.registration-failed`, `cluster.node-frozen` com o tempo restante).

### Publicações respondendo `503`

| Código | Causa provável | Ação |
|---|---|---|
| `cluster.quorum-unavailable` | Sem nós para fechar o quórum, ou o primário do bloco/grupo fora | Veja `taba-cli cluster status`; repita após `Retry-After` |
| `cluster.commit-timeout` | Registro confirmado, commit lento | Em tópico/token a operação **vai se completar**; consulte antes de repetir |
| `cluster.node-syncing` / `cluster.node-draining` | O nó atendeu mas não achou outro para repassar | Tire o nó do balanceador; repita |
| `fifo.primary-unavailable` | O primário do grupo FIFO fora, sincronizando ou esgotou tentativas | Veja `taba-cli cluster status`; repita |

## Certificados: rotação sem reiniciar

Procedimento em [14-seguranca.md](14-seguranca.md). Resumo: gerar o par novo com `--starts-in` maior que o tempo de propagação, **acrescentar** à pasta (o antigo fica), e esperar. Monitore `taba.cluster.tls.expires_in` (alerta com < 30 dias) e os `warn` de expiração (`cluster.tls.expiry-warning`). **Antes** de o certificado vencer: um vencido deixa de ser aceito e o nó perde o cluster.

## Trocar o token raiz

Edite o arquivo do token raiz com **duas linhas** (a nova e a anterior); cada nó o relê em até `auth.reload-interval` (60 s) e aceita as duas; depois retire a anterior. Os tokens já criados seguem valendo.

## Backup e restauração

A durabilidade vem da **replicação**, não do disco de um nó: copiar `storage.dir` de um nó **não** é um backup restaurável. O backup **opcional em S3** ([33-backup-s3.md](33-backup-s3.md)) cobre a perda de todos os nós. Mesmo com ele, guarde a **pasta de segredos** (`taba init`): o certificado do cluster e o token raiz **não** vão ao bucket.

### Ligar o backup

```yaml
backup:
  s3:
    bucket: taba-backup
    region: sa-east-1          # credenciais: a cadeia padrão da AWS (IAM, perfil, papel da tarefa)
```

Com SeaweedFS, MinIO ou outro S3 compatível (que honre escrita condicional; o Garage não serve): `backup.s3.endpoint`, `access-key` e `secret-key`. Em produção o bucket **deve** ter versionamento/retenção do lado do S3 e permissão só de escrita para os nós (leitura é necessária para restaurar). Configuração pronta em [`examples/backup-s3`](../../examples/backup-s3/README.md). Confira com `GET /node/backup`: `s3Reachable: true` e `pending` indo a zero.

### Forçar o backup agora (flush)

Antes de uma manutenção, de uma troca de infraestrutura ou de qualquer risco, mande tudo ao bucket **sem parar nada**:

```bash
# neste nó (porta de operação do nó, não o balanceador):
curl -s -X POST http://nó:9090/node/backup/flush
# no cluster inteiro (o nó repassa aos outros e junta as respostas):
curl -s -X POST http://nó:9090/cluster/backup/flush
```

Cada nó faz: memória → disco, disco → S3 e `outbox` → S3. `"complete": true` quer dizer que **nada ficou pendente**. Um nó com `"pending": true` estava lento ou fora: repita a chamada e confira `GET /node/backup` (`pending: 0`). Sem S3 configurado a resposta é `422 backup.not-configured`. Durante o flush o nó grava as mensagens novas em disco; com o disco acima de 90% a passagem pausa até a evicção liberar espaço (ou `backup.drain.timeout`).

### Parar e iniciar o cluster inteiro (com backup)

```bash
curl -s -X POST http://nó:9090/cluster/stop     # drena todos os nós, envia ao S3 e grava a lista da parada
# confira: "stopList": true e "warnings": []
docker compose stop                             # (ou desligue os processos como a sua plataforma pede)
…
docker compose start                            # "iniciar" é subir os processos: a restauração é automática
```

Se `stopList` vier `false` o S3 estava indisponível: o drain em disco foi feito, mas **o backup não foi enviado**; não desligue a infraestrutura até repetir o `stop` com o S3 de volta (ou aceite perder a janela não enviada). O `stop_grace_period` (Docker) ou `terminationGracePeriodSeconds` (Kubernetes) precisa passar de `drain.timeout` + `backup.drain.timeout` (o `docker-compose.yml` do repositório usa 70 s).

### O que o backup não faz

Não é ponto de recuperação por tempo (não há versões de mensagens nem "voltar a ontem"); não restaura a pasta de segredos; e a defasagem (RPO) é a de `backup.delay` e `backup.meta.interval`. O consumidor precisa ser **idempotente**: uma restauração pode devolver entregável uma mensagem cujo recibo ou invisibilidade da última janela estava defasado.

## Monitorar

- **Mínimo:** sonda `liveness` em `/health/live`, `readiness` em `/health/ready`; alerta em `worstCondition != "ok"` por mais de alguns minutos; `up == 0`; `quorumAvailable == false`.
- **Prometheus:** raspe `GET /metrics` de **cada** nó (um alvo por nó). Alertas de exemplo em [13-observabilidade.md](13-observabilidade.md).
- **Tópicos:** `taba-cli stats topics --condition backlog` (limiares `stats.topics.*` precisam estar configurados). ⚠️ A agregação **entre nós** das métricas de tópico não está implementada: consulte cada nó ou some no backend.
- **Logs:** `telemetry.logs.format=json` para ingestão; `color` para terminal. Procure `error`: perda de mensagem (`taba.message.lost`), falha de bootstrap, drain que estourou o prazo.

## Kubernetes

O repositório tem um **chart Helm** (`deploy/helm/taba/`, [34-chart-helm.md](34-chart-helm.md)); seus modos de operação e os testes em bash, rodados num cluster kind, estão em [35-chart-helm-casos-de-uso.md](35-chart-helm-casos-de-uso.md). Ele não foi validado num cluster de produção. O que o nó oferece para encaixar (e o chart usa): sondas `/health/live` e `/health/ready` na porta de operação; parada por `SIGTERM` com drain (`terminationGracePeriodSeconds` > `drain.timeout`); `cluster.entry-address` apontando para um `Service` **interno** (ou *headless*) que repasse a porta do cluster **em TCP**, sem terminar o TLS; a pasta de segredos como `Secret` montado; `storage.dir` num volume por pod (a última pasta chamada `taba`). `StatefulSet` ou `Deployment` servem (o nó **não depende** de identidade estável: toda partida é de nó novo). Rolling update: `maxUnavailable: 1`.

## Docker Compose do repositório: comandos úteis

```bash
docker compose up -d                               # sobe tudo (imagens públicas; TABA_VERSION escolhe a tag)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d   # o mesmo, construindo do repositório
docker compose ps                                  # saúde dos nós
docker compose run --rm taba-cli cluster status -o table
docker compose up -d --scale taba=4                # mais nós: o serviço `taba` tem as réplicas (nós 2 em diante)
docker compose logs -f taba-0                      # logs coloridos (`taba` mostra as réplicas)
docker/cluster.sh stop taba-2  / start taba-2      # parada normal (drain) e volta; taba-N é a réplica N (taba-0 é o seed)
docker/cluster.sh kill taba-2                      # simula falha
docker/cluster.sh restart taba-1                   # reinício (um por vez!)
docker compose down                                # ⚠️ PARA TUDO: o cluster nasce vazio no próximo up
docker compose down -v                             # apaga também os volumes (inclui o token raiz)
```

`docker/cluster.sh` (para `source` ou comando) sobe/derruba o Compose, provoca falhas (`taba_kill`, `taba_stop`, `taba_start`), espera o cluster estabilizar e cria tokens e tópicos; `docker/validate.sh` roda o roteiro completo (≈ 5 minutos: cluster forma, `taba-cli`, queda e volta de nós, **ausência de perda de mensagens**).

> **No Windows com Git Bash**, defina `MSYS_NO_PATHCONV=1` para comandos que levam caminhos do contêiner.

## Fontes e provas

- `cluster/backup` (backup em S3: [33-backup-s3.md](33-backup-s3.md)), `cluster/health/mod.rs` (as 8 condições), `cluster/drain`, `cluster/state`, `cluster/service` (bootstrap, ingresso, freezing, votação), `http/operations.rs`, `docker-compose.yml`, `docker/`.
- **Execuções reais** deste guia (cluster de 3 nós, `docker compose`): kill de um nó (condições em 4 s e 8 s), parada normal com drain, drain pela API (`drained` em ~4 s e volta como nó novo), rolling restart dos três nós (14/14 mensagens), perda de dois nós seguidos (`quorum-lost`, reentrada impossível), `down`+`up` (cluster nasce vazio).
- Casos de uso: UC-FAL, UC-FRZ, UC-DRN, UC-RTN, UC-ING, UC-OPS; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/failures.rs`, `freezing.rs`, `drain_cluster.rs`, `recovery.rs`.
- Histórico: `04-cluster.md`, `11-observabilidade.md`.
