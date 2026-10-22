---
título: Cluster
resumo: O ciclo de vida do cluster — como um nó entra, como a falha é detectada e votada, freezing, retorno, repasse de chamadas, sincronização, drain, perfil de um nó.
tipo: explicação
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src/cluster/members, cluster/service, cluster/channel, cluster/vote, cluster/health, cluster/sync, cluster/recovery, cluster/drain, cluster/state, http/forward]
---

# Cluster

> Leia antes: [03-conceitos.md](03-conceitos.md) e [06-anel-e-replicacao.md](06-anel-e-replicacao.md).

Este documento descreve **como os nós se organizam sozinhos**: entrar, ser monitorado, sair (por falha ou por vontade), voltar e se sincronizar. Não há coordenador: todas as decisões saem de regras que todos os nós aplicam do mesmo jeito.

## Ideia central: um protocolo, dois usos

O **registro + commit** em duas fases serve a dois fins com a mesma mecânica e regras de decisão diferentes:

| Uso | Quem decide | Por quê |
|---|---|---|
| Mensagens ([10-publicacao.md](10-publicacao.md)) | O **quórum** (`write.quorum`, com o primário) | É o caminho crítico de latência |
| Mudança de membros (entrada, saída) | **Todos** os nós | São operações raras e pesadas: mudam o anel de todos |

Regra comum às duas: **antes** da decisão, a operação pode ser abortada (*rollback*); **depois** de decidida, nunca se desfaz, e o coordenador **insiste** (dentro de `write.commit.timeout`) até confirmar. Cada fase é idempotente pelo `operationId`.

## Estados do nó

O nó tem um **estado** (`taba.node.state`) e, separado, um **estado de drain**:

| Estado | Significa |
|---|---|
| `joining` | Pediu para entrar; ainda não está na tabela |
| `syncing` | Está na tabela, carregando tópicos e blocos; **não atende clientes** |
| `active` | Pronto; atende |
| `returning` | Foi excluído (ou saiu por drain) e está recuperando mensagens antes de reentrar |
| `failed` | Disco cheio ou banco corrompido: só reiniciar o processo resolve |

`drainState`: `active` → `draining` → `drained`.

![Estados de um nó](../diagrams/img/pt-BR/node-states.svg)

> O **freezing** não é um estado do nó: é uma propriedade de um membro excluído, exposta como a condição `node-frozen` ([13-observabilidade.md](13-observabilidade.md)).

## 1. Comunicação entre nós

- **Porta própria** (`cluster.port`, padrão 8081), separada da de cliente, com **TLS mútuo**: o nó só fala com quem apresenta o certificado do cluster ([14-seguranca.md](14-seguranca.md)).
- **Chamadas HTTP** (`/cluster/*`): registro, commit, tabela de membros, replicação, streams de sincronização ([18-api-cluster.md](18-api-cluster.md)).
- **Canal permanente** (`GET /cluster/channel`, WebSocket): **um por par de nós**, em malha completa, compartilhado por todos os tópicos. Carrega só **sinais pequenos** (nunca mensagens): o sinal de vida e os avisos de entrega ([12-entrega.md](12-entrega.md)).
- Quem chama é reconhecido pelo **IP de origem** da conexão (todos usam o mesmo certificado, e a tabela guarda os IPs).

## 2. Como um nó entra

![Ingresso de um nó](../diagrams/img/pt-BR/node-join.svg)

![Falha no registro de um novo nó](../diagrams/img/pt-BR/node-register-failure.svg)

1. **Descoberta.** O nó chama a listagem de nós (`GET /cluster/nodes`) pelo endereço do balanceador (`cluster.entry-address`, host e **porta do cluster**). Se a chave falta, o nó não sobe (exceto o primeiro nó e o perfil de um nó).
2. **Primeiro nó.** Só com `cluster.first-node=true`, e só se **ninguém responde pela rede** (conexão recusada ou tempo esgotado), o nó inicia o cluster sozinho como único membro. Quem responde, mesmo com erro (`503`), foi encontrado: o nó tenta de novo a cada `cluster.bootstrap.retry.interval` (padrão 5 s), até `cluster.bootstrap.retry.max-attempts` (padrão 0 = sem limite).
3. **Pedido de ingresso.** O nó escolhe um membro ao acaso e envia, numa única chamada síncrona, `POST /cluster/nodes/join` com os próprios IPs e a porta do cluster; esse membro passa a **coordenar** o ingresso.
4. **Teste de conexão.** O coordenador tenta chamar o nó novo de volta em cada IP informado. Se falhar, o ingresso é abortado.
5. **Fase 1 (registro)** a todos os nós. Um `409` (há outra mudança pendente) é repetido até `write.commit.timeout`.
6. **Fase 2 (commit)** a todos. Cada nó que efetiva enfileira o quadro `membership` para os pares, para o nó atrasado ou fora do ar acertar a tabela ao voltar.
7. **Resposta** ao nó novo com a **tabela de membros**. A tabela de tópicos **não** vai na resposta (cresce com o número de tópicos): o nó a puxa depois.
8. O nó novo abre o canal permanente com cada nó da tabela. Quando um nó cai e reinicia, **é ele quem reabre** os canais.
9. **Sincronização** (seção 5) e só então o estado passa a `active` e `/health/ready` responde `200`.

**Falha antes do commit:** rollback nos que já confirmaram e o nó **não entra**; ele refaz o bootstrap do começo no intervalo de nova tentativa. **Falha no commit** (registro já confirmado por todos): não se desfaz; o coordenador insiste até confirmar.

### Uma mudança de membros por vez

Depois que um nó entra, nenhuma outra **entrada, saída ou rebalanceamento** é efetivada até ele ficar **pronto**. Também enquanto uma votação de exclusão está aberta, um pedido de entrada **espera**. A exceção é o próprio nó em sincronização: se ele entra em falha, a votação do desregistro dele é liberada.

Se duas mudanças são coordenadas ao mesmo tempo por membros diferentes, vence a de menor `operationId` (o GUID v7 mais antigo): o outro coordenador **cede**, desfaz o que registrou e tenta de novo depois de uma espera sorteada de 100 a 200 ms.

## 3. Como a falha é detectada

![Remoção de um nó por falha](../diagrams/img/pt-BR/node-removal.svg)

1. O canal permanente carrega um **sinal de vida** (*keepalive*) a cada `cluster.channel.keepalive.interval` (padrão **2 s**). **Não há ping separado.**
2. O canal com um par **cai** quando uma chamada falha, ou nada chega por `2 × keepalive.interval` (no mínimo 500 ms). As chamadas seguintes, a cada `cluster.channel.reconnect.backoff` (padrão 1 s), são as tentativas; esgotadas `cluster.channel.reconnect.retries` (padrão **3**), abre-se a **votação**.
3. Enquanto isso, os sinais para o nó se acumulam numa **fila de saída** em memória. Se ela passar de `cluster.channel.queue.max-size` (padrão 10 000) ou `max-age` (padrão 60 s), o nó do outro lado é tratado como **falho** e a votação abre sem esperar o fim das tentativas.
4. **Votação de exclusão.** Cada voto é `yes` se o votante também não alcança o candidato. Exige-se `ceil(S × cluster.vote.quorum-percent)` votos de **S nós saudáveis** (padrão 51%), com pelo menos `cluster.vote.min-voters` votantes (padrão 2). Nó **saudável** é o que o nó que abre a votação enxerga com o canal ativo e que responde ao pedido; os nós fora do ar **não votam e não entram na base**. Sem saudáveis suficientes, nenhuma exclusão se efetiva (condição `quorum-lost`).
5. **Quem coordena a exclusão:** o de **menor ID entre os que votaram**. O de ordem `k` espera `k × write.commit.timeout` antes de tentar, se o nó ainda consta da tabela.
6. Atingido o quórum, o **desregistro** segue o mesmo protocolo de duas fases. O coordenador insiste até confirmar em todos os nós que sobram, **menos os que ele vê caídos** (esperar um nó fora do ar impediria a exclusão de outro; quem fica com a tabela velha aparece como `membership-divergent`).
7. No commit, quem passa a ser primário das posições do nó removido assume as **filas voláteis** que ele gerenciava, e o anel é recalculado.

### Nó em falha

O nó em `failed` (**disco cheio** ou **banco corrompido**, tanto nos bancos de tópico quanto no `_node.db`) deixa de atender **chamadas de cliente**: num cluster as repassa a um nó são; sem outro nó, responde `503 node.failed`. As rotas de operação e as chamadas entre nós continuam até ele ser excluído. **Só reiniciar o processo** o tira de `failed`: não há reavaliação em execução, para o nó não oscilar entre `failed` e ativo.

### Perder dois nós seguidos (verificado)

Com 3 nós, `docker compose kill taba-2` leva em ~8 s à exclusão do nó (tabela na versão seguinte, condição `node-frozen`). Se **antes de ele voltar** um segundo nó cai, sobra **um** nó, menos que `cluster.vote.min-voters` (2): a condição `quorum-lost` aparece e a exclusão **não se efetiva**. Os dois nós reiniciados **não conseguem reentrar**: o ingresso exige o registro em **todos** os nós da tabela, e a tabela ainda lista o nó morto (`cluster.registration-failed` e `cluster.node-syncing` em laço no log). **Não há recuperação automática nem rota manual** (decisão adiada: "exclusão forçada"). O que funcionou foi **reiniciar o cluster inteiro**, ao custo de **perder mensagens e tópicos** ([11-durabilidade.md](11-durabilidade.md)). Por isso: reinicie e atualize **um nó por vez**, esperando `healthy` entre eles.

### Com o backup em S3: vivo não é alcançável

Com `backup.s3.bucket`, "inalcançável" deixa de bastar: cada nó grava um **batimento** no bucket, e a votação de exclusão **só abre depois de o batimento do candidato vencer** (`backup.dead-after`). Um nó inalcançável **com batimento fresco** está vivo em outra partição, e os outros **não assumem** os blocos dele. Quando sobra **um só nó** e todos os outros estão mortos para o bucket, ele exclui os mortos sozinho, mesmo abaixo de `cluster.vote.min-voters` (sem isso o cluster ficaria em `quorum-lost` sem saída). E quando os membros mortos saem da tabela, os sobreviventes buscam no S3 as mensagens que só existiam neles. A partida de um nó que não acha ninguém também consulta o bucket (o sobrevivente único restaura). Tudo em [33-backup-s3.md](33-backup-s3.md); sem o S3, ou com ele fora do ar, vale o que está aqui.

### Risco aceito: partições

A base de votantes é só quem o votante enxerga. Numa **partição de rede** em que os dois lados têm saudáveis suficientes, cada lado pode excluir o outro. O `cluster.vote.min-voters` impede só que um nó **isolado** exclua todos os demais; quem opera muitos nós pode aumentá-lo para uma maioria do cluster. Quando os dois lados se excluíram, cada nó guarda os endereços dos excluídos e, a cada `keepalive.interval × 10`, consulta a tabela deles: se for de **outro cluster** e este lado **perder** (menos membros; no empate, o do menor `nodeId`), o nó refaz o ingresso do zero.

## 4. Freezing e retorno do nó excluído

![Retorno de um nó excluído](../diagrams/img/pt-BR/excluded-node-return.svg)

![Reinício do processo](../diagrams/img/pt-BR/process-restart.svg)

**Freezing.** Um nó excluído por **falha** que **continua o mesmo processo** (mesmo ID) fica impedido de reentrar por `cluster.freezing.duration` (padrão **5 min**), para não entrar e sair em laço. Durante o freezing o pedido de ingresso é **rejeitado** com `Retry-After`; o nó segue tentando no laço normal do bootstrap. Quem executou o commit da exclusão guarda `(ID, instante)` e o repassa aos pares pelo canal (quadro `frozen`), então um membro que estava fora do ar também recusa o `join` do excluído. O **drain** e o **reinício** do processo **não** geram freezing (são voluntários, ou o ID é novo).

**Como o nó descobre que foi excluído.** Se três chamadas seguidas do canal a um par voltam `403 cluster.not-member` **e** a tabela do par é mais nova e não o tem, o nó se sabe excluído.

**Recuperação antes de reentrar** (estado `returning`):

1. Consulta **todos os nós que alcança** (`GET /cluster/message-loss`, stream NDJSON) e une as **listas de perdas**: os pares (tópico, ID) que alguma sincronização declarou perdidos.
2. Procura cada ID **no disco** (e na memória, se o processo for o mesmo).
3. Envia as encontradas ao nó responsável pelo bloco (`POST /cluster/messages/recover`). Quem aceita grava direto nos responsáveis, **idempotente pelo ID**, e dá a baixa da perda.
4. Tenta de novo a cada `cluster.bootstrap.retry.interval` até `cluster.recovery.timeout` (padrão 5 min); só então **limpa o estado local** e refaz o ingresso do zero, **como nó novo**.

**Reinício do processo.** Um processo que reinicia nasce como **nó novo (ID novo)**, mas o disco da vida anterior **não é apagado na partida**: é posto de lado numa **pasta com o ID antigo** (`${storage.dir}/_lives/<ID antigo>`, com o manifesto da vida; um disco sem manifesto, de versão anterior, vai para `_previous-life`), o nó entra com disco vazio e, **depois de ativo e em segundo plano**, devolve ao cluster o que esse disco tem das perdas. A recuperação não atrasa o ingresso e **não apaga** a pasta da vida (doc 37, seção 4.5).

## 5. Sincronização de um nó que entra

![Fases da sincronização](../diagrams/img/pt-BR/sync-phases.svg)

O nó novo já consta na tabela e **recebe** o protocolo normal, mas **não pode processar** os eventos até terminar. A sincronização é **escopada aos blocos que ele assumiu**; o resto do cluster segue normal.

1. **Carga de tópicos e tokens** de um membro (`GET /cluster/topics`, `GET /cluster/tokens`, streams NDJSON ordenados por `id`); a origem é o nó de menor carga, ou um sorteado.
2. **Carga de blocos.** Para cada bloco, pede aos nós responsáveis a **lista de IDs** (`GET /cluster/blocks/{id}/ids`), faz a **união** (não a interseção) por *merge* ordenado, divide em **faixas** e as busca (`GET /cluster/blocks/{id}/messages?from=&to=`) **balanceando entre as origens**. O que faltar é buscado em outra origem; a rota por ID único é o último recurso. Um stream cortado é retomado pelo último ID lido.
3. **Banco de eventos** (`${storage.dir}/_sync/sync.db`). Tudo o que chega por registro/commit nesse intervalo é guardado **na ordem de chegada**; o nó confirma o registro **depois de gravar**, então conta para o quórum.
4. **Replay.** Processa o banco **por cima da carga**: lê sempre a linha mais antiga, aplica (idempotente; o evento vence a carga) e apaga, até esvaziar.
5. **Verificação final.** Repete as listagens e descarta o que já não aparece em nenhuma origem (cobre um ack que nunca chegou).
6. **Troca para o modo normal.** Com o banco vazio, passa a processar em tempo real; o `sync.db` é descartado; o nó fica `active` e **pronto**.

**Uma mensagem que não vem.** Antes de declarar perda, o nó tenta: outra origem, a rota de ID único e uma relistagem. Se o ID **sumiu** das listas, foi um ack e o caso é descartado em silêncio. Só o que continua **listado e sem conteúdo** em todas as origens é uma **perda**: gera um log `error` com tópico, ID e bloco, e um registro na tabela `losses`; **não** compromete a saúde do nó.

**Falhas da sincronização.** Disco cheio, `cluster.sync.log.max-bytes` (padrão 1 GiB) estourado ou banco corrompido levam o nó a `failed`. Sem origem para uma carga, o nó espera e tenta de novo até `cluster.sync.timeout` (padrão 60 s); esgotado, cai em `failed`. Se, durante a espera, um membro da tabela que recebeu responder com uma tabela mais nova que já não tem o nó (o ingresso dele foi desfeito, por exemplo pelo reencontro depois de uma partição simétrica), o nó não espera: esquece a tabela e refaz o ingresso do zero.

Enquanto sincroniza, o nó **repassa** as chamadas de cliente a outro nó (seção 6), e a **limpeza** dos blocos que ele deixou de ter só ocorre com o cluster íntegro ([06-anel-e-replicacao.md](06-anel-e-replicacao.md)).

## 6. Repasse de chamadas de cliente

Um nó que ainda não está íntegro (sincronizando, em drain ou em falha) pode receber chamadas de cliente. Em vez de erro, ele **repassa** a chamada:

- **Destino:** na sincronização, um membro com canal ativo, ao acaso; no **drain**, o nó **mais livre** (menor carga pelo sinal de vida, empate sorteado), escolhido **a cada chamada**.
- **Transparente:** método, caminho, consulta, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu (inclusive `409`, `413`, `429`).
- **Pela porta do cluster** (`/cluster/forward/{caminho}`): assim a API de cliente não precisa aceitar nenhum marcador interno que um cliente pudesse forjar. Aceita só de membros da tabela e de nós que saíram por drain; quem recebe **nunca repassa de novo**.
- **Autenticação:** o `Authorization` vai como veio; **o destino confere** o token.
- **Falha do destino:** antes de enviar o corpo (conexão recusada), tenta outro, até um teto pequeno. **Depois de enviado, não repete**: um `POST` repetido duplicaria a mensagem. Responde `502`/`504`.
- **Rate limit:** contado uma vez, no nó que recebeu, **por IP**.
- **Sem destino:** `503` com `Retry-After`.
- **FIFO:** a publicação numa fila FIFO vai ao **primário do grupo** por uma chamada própria (`POST /cluster/topics/{topic}/fifo/publish`), não pelo repasse genérico.

## 7. Drain: sair sem perder o que está em memória

**Para que serve.** Manutenção, atualização ou realocação de contêiner. Sem drain, uma parada perde o que estava só em memória, o canal cai e o cluster só tira o nó depois das reconexões e da votação.

**Como entra:**

- **Sinal** `SIGTERM` ou `SIGINT` (o que Docker e Kubernetes enviam), com `drain.on-stop` ligado (padrão). Ao terminar, o processo encerra com código 0. Um **segundo sinal** encerra na hora.
- **API** `POST /node/drain`: faz o mesmo **sem encerrar**; o nó fica `drained` até alguém parar o processo ou chamar `DELETE /node/drain`. Vale só para o nó que recebe a chamada: use a **porta de operação** direto, não o balanceador.

**O que o nó faz, em ordem:**

1. **Sai da rotação:** `/health/ready` passa a `503`; continua a receber chamadas de cliente mas **repassa todas** ao nó mais livre; os long pollings em andamento respondem vazios.
2. **Avisa os pares** pelo canal ("em drain"): eles deixam de pedir `take` a ele e de escolhê-lo como origem de carga.
3. **Grava tudo em disco:** o limite de memória efetivo cai a **zero**; toda mensagem nova (publicada ou réplica) vai direto para o disco.
4. **Esvazia a memória:** move para o disco as mensagens em memória **com os metadados** (`deliveryCount`, `invisibleUntil`, `receiptToken`), primeiro os tópicos de menor taxa de leitura.
5. **Com o backup em S3, antes de sair:** manda ao S3 tudo o que está em disco, o `outbox`, o diário e as definições pendentes, dentro de `backup.drain.timeout` ([33-backup-s3.md](33-backup-s3.md)). Com o S3 fora do ar o nó não espera para sempre.
6. **Sai do cluster:** coordena o **desregistro de si mesmo** pelo protocolo de duas fases (`POST /cluster/nodes/unregister` e `commit`), **sem votação**. Rebalanceia, e os novos primários assumem as filas voláteis dele.
7. **Fecha o canal.** Estado `drained`. Se foi por sinal, espera as chamadas repassadas terminarem (dentro de `drain.timeout`) e encerra.

`drain.timeout` (padrão **25 s**) cobre tudo (menos o envio ao S3, que tem o prazo próprio `backup.drain.timeout`, 30 s) e precisa ser **menor** que o prazo de parada do orquestrador (`stop_grace_period` no Compose, `terminationGracePeriodSeconds` no Kubernetes, 30 s por padrão). Estourado, o nó registra o que sobrou em memória e a etapa em que parou e encerra mesmo assim (se por sinal): cai-se no comportamento sem drain. Erro de I/O no passo 4 **não interrompe** o drain.

**Voltar do drain** (`DELETE /node/drain`):

| Quando | Resultado |
|---|---|
| Antes da saída (`draining`) | Cancela: o nó volta a `active` e a gravar em memória. `200` |
| Durante o desregistro | `409`; tente de novo quando for `drained` |
| Depois da saída (`drained`) | O nó **entra como nó novo** (ID novo), refaz o bootstrap, faz antes a **recuperação de perdas**. `202` |

Sair por drain **não** gera freezing. Pendência **D75**: se o drain pedido pela API estoura `drain.timeout`, o nó fica `draining` até alguém chamar `DELETE` ([31-pendencias.md](31-pendencias.md)).

## 8. Tamanho do cluster e o perfil de um nó

| Ambiente | Nós mínimos | Por quê |
|---|---|---|
| Teste e desenvolvimento | **1** (o perfil de um nó: `replication.factor=1`) | Rodar localmente sem o peso do cluster |
| Produção | **3** | Permite votação com um nó fora do ar (2 votantes saudáveis) e evita decisão por um único nó |

No **perfil de um nó** (`replication.factor=1`, sem `cluster.entry-address`) o nó é um cluster de um só: parte sozinho, é o primário de todos os blocos e o quórum vale 1, mas o caminho é o de produção (cluster vazio, porta do cluster, registro e commit). O que o perfil relaxa (`cluster.tls.enabled=false`, `auth.dev-token`) só vale com fator 1 ([22-configuracao.md](22-configuracao.md)); o log lista o que está relaxado na partida e o `/health` mostra a condição `reduced-profile`. **Não é para produção:** perder o nó ou o disco é perder as mensagens.

**Reinício.** Todo reinício é de nó novo: o disco antigo é posto de lado em `_lives/<ID antigo>` e o nó o restaura para si, como mensagens novas, prontas, com o mesmo `deliveryCount` e um `receiptToken` novo ([37-retomada-do-cluster.md](37-retomada-do-cluster.md)). Não existe mais o modo `development` nem `development.resume-on-start`: para começar limpo, apague o volume ou o diretório.

## 9. As condições do cluster

`GET /health/cluster` devolve uma **lista** de condições (não um estado único), derivada dos sinais que cada nó colhe:

| Condição | Severidade | Significa |
|---|---|---|
| `write-quorum-at-risk` | crítica | Algum bloco não consegue formar o quórum de escrita |
| `quorum-lost` | crítica | Há menos nós saudáveis que `cluster.vote.min-voters`: nenhuma exclusão se efetiva |
| `membership-divergent` | crítica | Nós com versões diferentes da tabela de membros |
| `member-unreachable` | aviso | Um membro está inalcançável |
| `node-frozen` | aviso | Há nós em freezing |
| `syncing` | aviso | Há nó(s) sincronizando |
| `under-replicated` | aviso | Algum bloco está abaixo do fator de replicação pleno |
| `storage-pressure` | aviso | Espaço livre em disco abaixo de `stats.disk.free-min` (sem valor, nunca dispara) |
| `upgrade-stalled` | aviso | A retirada de versões anteriores parou num nó que não terminou o drain (D165) |
| `members-missing` | aviso | A sondagem dos IPs antigos terminou e faltam nós da tabela da vida anterior (doc 37, seção 4.14) |
| `reduced-profile` | aviso | O nó roda o perfil de um nó com algo relaxado: fator 1, TLS desligado ou `auth.dev-token`. Informativa (doc 37, seção 4.16) |
| `previous-life-pending` | aviso | A restauração das vidas anteriores não terminou: as filas FIFO recebem e não entregam (doc 37) |

O que fazer em cada uma está em [23-operacao.md](23-operacao.md).

## Limites conhecidos

- **D99:** o voto quando o candidato avisou que entrou em falha ainda é provisório.
- **D102:** `quorumAvailable` e `quorum-lost` não definem de quem é o ponto de vista.
- **D109, D120:** constantes do canal e do transporte (10 s de validade de aviso, prazos de conexão) estão no código, não são parâmetros.
- **PRB-005:** durante o drain o nó recusa **toda** chamada de cliente, inclusive ack: sem outro nó, o consumidor não confirma o que já recebeu.
- **PRB-051:** o cluster reconhece quem chama pelo IP de origem, e o `Transport` não fixa o IP de saída: num nó com várias interfaces pode falhar.

Lista completa: [31-pendencias.md](31-pendencias.md).

## Fontes e provas

- `cluster/members` (registro em duas fases e tabela), `cluster/service` (bootstrap, ingresso, repasse), `cluster/channel` (canal e fila de saída), `cluster/vote` (votação), `cluster/health` (condições), `cluster/sync` (fases), `cluster/recovery` (perdas), `cluster/drain`, `cluster/state`, `http/forward`.
- Casos de uso: UC-ING, UC-FAL, UC-FRZ, UC-CNC, UC-SIN, UC-STR, UC-RTN, UC-FWD, UC-DRN, UC-DEV; veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/cluster.rs`, `failures.rs`, `freezing.rs`, `sync.rs`, `recovery.rs`, `drain_cluster.rs`.
- Histórico: `04-cluster.md`, `05-particionamento-replicacao.md` (7.3).
