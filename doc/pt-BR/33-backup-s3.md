---
título: Backup opcional em S3
resumo: Como o Taba guarda um backup de falha grave num bucket S3 (ou MinIO) sem nunca esperar por ele — o que vai ao bucket, quem grava, como o nó restaura, como forçar o backup e parar e iniciar o cluster, a configuração, os limites e os cenários simulados.
tipo: explicação
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src/storage/backup, crates/taba-node/src/cluster/backup, crates/taba-node/src/engine/messages/backup.rs, crates/taba-node/src/http/backup.rs, crates/taba-node/src/cluster/protocol/backup.rs, crates/taba-node/src/cluster/drain/mod.rs, crates/taba-node/src/cluster/vote/mod.rs, crates/taba-node/src/cluster/service/mod.rs, crates/taba-node/src/config/mod.rs, crates/taba-node/tests/backup.rs, docker-compose.yml, examples/backup-s3]
---

# Backup opcional em S3

> Leia antes: [08-armazenamento.md](08-armazenamento.md), [11-durabilidade.md](11-durabilidade.md) e [07-cluster.md](07-cluster.md). A configuração está em [22-configuracao.md](22-configuracao.md); o passo a passo de quem opera, em [23-operacao.md](23-operacao.md).

O Taba guarda tudo em **memória e SQLite** e garante a mensagem pela **replicação** entre nós ([11-durabilidade.md](11-durabilidade.md)). Isso não cobre a **perda de todos os nós** (queda geral, parada total, um desastre no datacenter): em produção todo reinício é de nó novo, com disco vazio. O **backup em S3** cobre exatamente isso.

> **O S3 é só backup de falha total ou grave** (cluster abaixo do quórum). SQLite e memória continuam sendo o armazenamento primário. **O S3 nunca entra no caminho de publicação, consumo ou sincronização**: nada espera por ele.

## Princípios

| Princípio | Como é cumprido |
|---|---|
| **Opcional** | Sem `backup.s3.bucket` o recurso não existe: o nó é o de sempre e as rotas de backup respondem `422 backup.not-configured` |
| **Nunca atrasa nem recusa** | O commit de uma mensagem só **insere uma linha** (o `id`) no `_backup.db` local. Uma alteração de definição **nunca é recusada** por falha no S3 |
| **S3 fora do ar = recurso desligado** | O cluster funciona exatamente como sem ele; só se acumulam IDs no disco (o `outbox`). O nó **sobe com aviso** se o S3 está inacessível na partida |
| **A garantia continua sendo a replicação** | O backup aceita uma defasagem (RPO, abaixo) e isso está documentado; um recibo ou uma invisibilidade da última janela pode estar defasado, então a entrega é *at-least-once* e o consumidor deve ser idempotente |
| **Todas as filas** | Voláteis e dead-letter também entram |
| **Cliente oficial** | O `aws-sdk-s3` (fala com AWS e com MinIO). Credenciais opcionais: sem elas, vale a cadeia padrão da AWS (IAM, perfil, papel da tarefa) |

> 🖼️ **DIAGRAMA PLANEJADO** — tipo: sequência; objetivo: mostrar o commit da mensagem inserindo só uma linha no outbox, a thread de envio montando o segmento depois de `backup.delay`, o PUT confirmando e só então o outbox sendo esvaziado; reaproveita: `doc/diagramas/fonte/arquitetura-escrita.json`

## O que vai ao bucket, e quem grava

```text
<prefixo>/                                  backup.s3.prefix (padrão `taba`)
  topics/<tópico>.json                       definição (ou lápide), gravada pelo nó gestor
  tokens/<id>.json                           token, gravado pelo nó gestor
  nodes/<id>.json                            o batimento de cada nó
  messages/<tópico>/<topicId>/<primeiroId>_<escritor>_<seq>.seg   segmentos de mensagens
  journal/<tópico>/<topicId>/<instante>_<escritor>_<seq>.jrn      diários do estado mutável
  restore/lock.json                          a trava de quem restaura
  cluster/stop.json                          a lista dos nós vivos na parada programada
```

| Dado | Quem grava | Formato |
|---|---|---|
| Definição de tópico | O **nó gestor** do tópico (o dono da definição no anel, como em [09-topicos.md](09-topicos.md)) | 1 `.json` por tópico; a remoção grava uma **lápide** |
| Token | O gestor do token (o dono da definição do token) | 1 `.json` por token; a revogação apaga o objeto |
| Batimento | Cada nó, o seu | `nodes/<id>.json` |
| Mensagens | O **primário** do bloco, único escritor | Segmentos `.seg` |
| Estado mutável | O primário do bloco | Diário `.jrn` |

O `topicId` no caminho impede que as mensagens de um tópico **apagado** ressuscitem num tópico novo com o mesmo nome. O nome do segmento começa pelo `id` da primeira mensagem (um GUID v7 embute o instante): os nomes ordenam por tempo.

> ⚠️ O bucket guarda o **hash salgado** dos tokens (nunca o segredo), como o `_node.db`. Proteja o bucket como protege a pasta de dados.

## Mensagens: do commit ao segmento

1. **Outbox local.** O commit só insere o `id` em `outbox` no `_backup.db` (SQLite, ao lado do `_node.db`). Só o **dono** (primário do bloco) enfileira. Não há teto: se o disco enche, o nó entra em `failed`, como em qualquer falta de espaço (o outbox só guarda IDs, e cresce só com o backlog que já ocupa o disco).
2. **Atraso antes de enviar (`backup.delay`, 60 s).** A mensagem confirmada e consumida **antes da janela fechar nunca vai ao S3**, o que corta o volume de PUT: quase todo o tráfego de consumo rápido nunca chega ao bucket.
3. **Segmento.** Uma tarefa de fundo monta, por tópico, um segmento que **fecha pelo que vier primeiro**: `backup.segment.max-bytes` (8 MiB), `backup.segment.max-messages` (5000) ou `backup.segment.max-age` (60 s). O arquivo termina num **rodapé-índice** (id, deslocamento, tamanho): dá para ler o rodapé sem baixar o segmento (`Range` do fim) e uma mensagem por `Range`.
4. **Só depois do PUT confirmar** as linhas do lote saem do `outbox`, e o nó registra para cada mensagem onde ela está (`saved`: segmento, deslocamento, tamanho) e quantas vivas cada segmento tem (`segments`).
5. **Aviso às réplicas.** O primário avisa as réplicas, em lote (`POST /cluster/backup/saved`), de quais mensagens já foram salvas e **quando** (`saved_at`, o instante em que o PUT do segmento foi confirmado, no relógio do primário; só informação, não se compara com o relógio de outro nó). Com a referência, a réplica também pode fazer a evicção. Qualquer nó que guarda a mensagem consulta onde ela está e quando foi salva por uma função só (`Backup::saved_info`); a referência fica no `_backup.db` de cada nó, **não** no `_topic.db` (D142), e **não** é exposta pela API de cliente. Numa restauração `saved_at` é o instante da restauração.

**Exemplo.** O primário confirma 10 mensagens no tópico `pedidos`. Elas entram no outbox. Em 8 segundos o consumidor confirma 6, e essas 6 saem do outbox **sem gerar upload**. As outras 4 passam de `backup.delay`, entram num segmento, o segmento é enviado, e só então as 4 ganham referência e o `_backup.db` registra o segmento com 4 vivas.

Uma mensagem que **morre enquanto o segmento sobe** (ack ou `ttl`) não conta como viva: o commit do segmento a detecta (ela já não está no outbox) e grava a remoção no diário, para a restauração não ressuscitá-la.

### Estado mutável: o diário

`delivery_count`, `invisible_until`, `receipt_token`, dead-letter e a deduplicação **não estão nos segmentos**; vão num **diário** em lote (`.jrn`, linhas JSON).

- A cada `backup.meta.interval` (60 s) só o **último estado** de cada mensagem alterada é gravado: uma entregue 3 vezes na janela gera 1 registro.
- Um **ack entra como remoção**. **Nunca se apaga uma mensagem solta do bucket.**
- O primário apaga um **segmento do bucket somente quando todas as mensagens dele morreram** (confirmadas, expiradas ou em dead-letter). Segmento parcialmente morto fica até morrer por completo; **não há compactação**. Para o que o primário que mudou não enxerga, o gestor do tópico apaga segmentos cujas mensagens passaram todas do `ttl` (varredura em `backup.reconcile.interval`).
- Restauração = segmentos + diários, do mais antigo ao mais novo; o último estado vence e as remoções descartam a mensagem.

## Definições e tokens

Quem **recebe** a mudança (qualquer nó) a aplica no cluster normalmente; o **gestor** da definição, ao ver o commit, grava o objeto. Se a gravação falha, a alteração **vale mesmo assim** e entra numa fila de retentativa. A cada `backup.reconcile.interval` o gestor compara o que o bucket tem com a memória e regrava o que diverge.

**Exemplo.** O gestor do tópico `pedidos` recebe a criação, a alteração é aplicada no cluster e ele grava `topics/pedidos.json`. O S3 está fora do ar: a criação vale normalmente e `pedidos` fica na fila de retentativa até o S3 voltar.

**Remoção de tópico:** o gestor grava a lápide (`{"deleted": true, "id": …}`) e apaga em segundo plano os prefixos de segmentos e diários do `topicId`.

## Batimento e detecção de morte

Cada nó grava `nodes/<id>.json` a cada `backup.heartbeat.interval` (15 s) com **id**, **ips de acesso**, a porta do cluster, o estado (`member`, `joining`, `draining`) e um **número de sequência** (`seq`) que cresce a cada batimento. O `seq` é obrigatório: sem ele o conteúdo se repete, o ETag não muda e quem observa não percebe o batimento.

**Sem relógio sincronizado:** quem observa **não compara horários**. Guarda o ETag de cada arquivo e marca, no próprio relógio monotônico, quando ele mudou pela última vez. Se não mudou por `backup.dead-after` (60 s), o nó está **morto para quem observa**. Um arquivo visto pela primeira vez conta como fresco até completar `dead-after` sem mudar (sem histórico não há como saber). Se o S3 não pode ser lido, o resultado é **desconhecido** e vale o comportamento de sempre.

**Vivo não é o mesmo que alcançável.** O batimento no S3 mostra que o nó está vivo, seja qual for a rede entre os nós; a rede só responde "consigo falar com ele agora?":

| Rede até Y | Batimento de Y no S3 | Conclusão | Ação de X |
|---|---|---|---|
| alcançável | fresco | vivo | normal |
| inalcançável | fresco | **vivo em outra partição** | **não assume** os blocos de Y: a votação de exclusão espera |
| inalcançável | vencido (ou arquivo removido) | morto | pode excluir Y pela votação e assumir os blocos |

A votação de exclusão só **abre** depois de o batimento vencer (`backup.dead-after`); o canal de um nó que morre "pisca", então a espera só desiste se o canal ficar aberto em várias conferências seguidas. O voto de quem vota continua sendo "eu também não o alcanço".

### Ilhamento e sobrevivente

**A: partição {1,2} | {3,4,5}.** O 1 vê 3, 4 e 5 com batimento fresco: não assume nada; o 3 vê 1 e 2 frescos: não assume nada. Cada lado segue com o quórum de votos que já existe. O S3 é o desempate: quem está vivo aparece no bucket.

**B: 5 nós, 3 caem ao mesmo tempo.** O 1 e o 2 veem 3, 4 e 5 com batimento vencido: excluem os mortos pela votação de hoje e assumem os blocos. Ao ver membros saírem da tabela, cada sobrevivente **procura no bucket** as mensagens dos blocos que passou a guardar e não tem (só o rodapé de cada segmento e, do que falta, a entrada por `Range`; o diário diz o que já foi confirmado), e a sincronização também busca no S3 antes de declarar uma perda (`Syncer::settle_missing`). Só se o S3 também não tiver vira perda declarada.

**Sobrevivente único em execução.** Quando sobra **um** nó (abaixo de `cluster.vote.min-voters`), hoje o cluster ficaria em `quorum-lost` sem saída. Com o backup, se **todos os outros membros estão mortos para o S3**, o nó exclui os mortos sozinho.

**C: nó que sobe sozinho (partida).** Quando a listagem de nós em `cluster.entry-address` não acha ninguém, o nó consulta o bucket:

| O bucket mostra | Decisão |
|---|---|
| S3 inacessível | Vale o comportamento de sempre: começa sozinho só com `cluster.first-node` |
| Algum nó `member` batendo | **Não está sozinho**: segue tentando entrar pelo caminho normal (os ips ficam no log) |
| Nós que pararam de bater (`dead-after`) | **Sobrevivente único**: pega a trava e restaura |
| Nenhum arquivo de nó | Só começa sozinho com `first-node`, **ou** se existe `cluster/stop.json` (parada programada) |

Um nó que também está subindo (`joining`) **não prova** que o cluster existe; sem isso, uma subida simultânea de todos travaria.

### A trava de restauração

Dois restauradores ao mesmo tempo divergiriam. `restore/lock.json` é criado com escrita condicional (`If-None-Match: *`): **quem cria ganha**, quem perde espera e entra como nó normal. A trava tem validade (`backup.restore.lock-ttl`): o titular a renova; se o ETag fica parado por esse tempo, outro a toma (`If-Match`). Ao fim da restauração a trava é solta e `cluster/stop.json` é apagado. **Isto exige um S3 com escrita condicional** (AWS S3 recente e MinIO suportam); sem ela o nó avisa no log e segue, e **dois restauradores podem divergir** (risco aceito).

### Identidade e limpeza

Um nó morto que volta entra como **nó novo**, com id novo; não retoma o id antigo. O `nodes/<id>.json` de um nó morto fica até o cluster voltar a funcionar; então qualquer nó apaga os arquivos de ids que **não pertencem ao cluster e** cujo batimento está vencido. **Um arquivo fresco nunca é apagado** (pode ser um nó vivo em outra partição). Um nó que sai limpo (drain) remove o próprio arquivo.

## Disco cheio: evicção

- Acima de `backup.disk.evict-above` (85%) o nó apaga o **arquivo do corpo** (`.body`) das mensagens que já têm referência no bucket, até o uso cair a `backup.disk.evict-to` (70%). A **linha** em `_topic.db` fica, então a ordem "arquivo primeiro, linha depois" de [08-armazenamento.md](08-armazenamento.md) continua valendo.
- Só se libera o que está em **disco**: mensagem só em memória não libera nada.
- **Durante o flush e o drain**, ao chegar a `backup.disk.pause-at` (90%), a passagem da memória para o disco **pausa** até a evicção liberar espaço, no máximo por `backup.drain.timeout`; passado o prazo, o nó segue o comportamento de sempre (falha por disco cheio). Se o S3 está fora, nada se libera.
- Na leitura, o arquivo ausente vira uma busca no S3 por `Range` no segmento; sem S3, ou sem a mensagem lá, vale o comportamento atual (corpo sumido é perda). A leitura espera o S3 com a trava do tópico: é o caso raro de disco cheio, e só aquele tópico espera.

**Exemplo.** O disco do nó 2 passa de 85%. Ele apaga 500 `.body` de mensagens que já estão em segmentos. Um consumidor pede uma delas: o nó não acha o arquivo, lê `saved` (segmento, deslocamento, tamanho) e faz um `Range` no segmento.

## Encerramento programado (`SIGTERM`)

O sinal de parada dispara o **drain**; com o backup, ele ganha uma etapa **antes de o nó sair do cluster** (depois da saída o nó deixa de ser dono do que enviaria):

1. Para de aceitar publicações (como hoje) e passa a memória para o disco.
2. **Envia ao S3, com prioridade**, tudo o que está em disco e o `outbox`, e o diário e as definições pendentes, dentro de `backup.drain.timeout`.
3. Só depois do envio sai do cluster, grava `save_completed` e remove o próprio `nodes/<id>.json`.

O processo **não sai** antes do fim da etapa 2, **mas não espera para sempre**: passado `backup.drain.timeout` (30 s) com o S3 fora do ar o nó sai assim mesmo, e o que ficou no outbox depende das réplicas. **O `stop_grace_period` do Docker (e o `terminationGracePeriodSeconds`) precisa ficar acima de `drain.timeout` + `backup.drain.timeout`.** O `docker-compose.yml` do repositório usa 70 s.

## Operação: forçar o backup, parar e iniciar o cluster

Estas rotas ficam na API de operação (como `/node/drain`: sem token, filtradas por `operations.allowed-ips`, categoria *admin* do rate limit). Detalhes e respostas em [17-api-operacao.md](17-api-operacao.md).

| Rota | O que faz |
|---|---|
| `GET /node/backup` | O estado do backup neste nó: S3 no ar, o que falta enviar (`pending`), segmentos, corpos evictados |
| `POST /node/backup/flush` | Força o backup **neste nó**, em três etapas: **1.** memória → disco (pelo caminho do drain, sem parar o nó); **2.** disco → S3: segmentos e diário pendentes, com prioridade; **3.** `outbox` → S3. Responde ao terminar as três ou quando `backup.drain.timeout` estoura. **Não desregistra nem para o nó**: é seguro em produção |
| `POST /cluster/backup/flush` | O nó que recebe repassa a **todos os outros** e junta as respostas. Um nó lento aparece como pendente (`pending: true`): repita a chamada. Não muda a ordem de nenhum bloco |
| `POST /cluster/stop` | Faz o drain em **todos** os nós (os outros em paralelo; o que recebeu a chamada por último) pela mesma etapa de envio. O coordenador grava `cluster/stop.json` com os nós que pararam e a **ordem** em que pararam. Com o S3 indisponível ainda faz o drain em disco e responde com **aviso** de que o backup não foi enviado |

Sem S3 configurado os dois *flush* respondem `422 backup.not-configured` (ninguém acha que fez backup sem ter feito); o `POST /cluster/stop` ainda drena os nós e avisa que não há lista.

**Iniciar o cluster** não é uma rota: a API não inicia processos parados. "Iniciar" é subir os processos (`docker compose start`, escalar o Kubernetes). A restauração acontece **sozinha** na partida: o primeiro nó a pegar a trava vira o sobrevivente único, restaura definições, tokens e mensagens, e os demais entram como nós normais. Sem a lista da parada (queda, não parada programada), vale a regra de morte, com o atraso de `backup.dead-after`.

**Exemplo.** Antes de uma janela de manutenção, o operador roda `POST /cluster/stop`. Os cinco nós fazem drain, o coordenador grava a lista `[2,3,4,5,1]` na ordem em que pararam e o cluster fica parado. Na volta, `docker compose start` sobe os processos, o primeiro nó lê a lista, restaura e a restauração não precisa esperar o prazo de morte.

Um passo a passo com os comandos e uma configuração pronta estão em [`examples/backup-s3`](../../examples/backup-s3/README.md).

## Configuração

Todas as chaves aceitam variável de ambiente (`TABA_BACKUP_…`): veja [22-configuracao.md](22-configuracao.md). O mínimo:

```yaml
backup:
  s3:
    bucket: taba-backup          # ligar o recurso é só isto (a AWS usa a cadeia padrão de credenciais)
    region: sa-east-1
```

Com MinIO (ou outro S3 compatível), `backup.s3.endpoint`, `access-key` e `secret-key` (o `path-style` liga sozinho com `endpoint`).

## Cenários simulados e consequências

| # | Cenário | O que acontece | Consequência |
|:-:|---|---|---|
| 1 | S3 fora do ar para todos | Recurso desligado; o cluster segue como hoje | Nenhuma perda além da que já existia sem o backup |
| 2 | S3 fora só para um nó (vivo) | Esse nó para de renovar o batimento; os outros podem achá-lo morto | **Risco aceito:** só vira problema com partição ao mesmo tempo; mensagens aceitas só pelo lado perdedor podem se perder |
| 3 | Primário cai antes de enviar | O herdeiro assume o `outbox` na mudança de dono (`reown`) | As mensagens pendentes são enviadas pelo novo primário |
| 4 | Primário troca com segmentos parcialmente mortos | O herdeiro não tem o `segments` do antigo | O segmento só é apagado quando todas morrem **pelo diário e pelo `ttl`** (varredura do gestor). Mais lento, sem perda |
| 5 | Segmento parcialmente morto | Fica no bucket até morrer por completo | Gasta S3 com bytes mortos; aceito, sem compactação |
| 6 | Ack chega durante o upload de um segmento | O commit do segmento a detecta e grava a remoção no diário | Na restauração a mensagem é descartada |
| 7 | Mensagem confirmada e consumida antes de `backup.delay` | Nunca vai ao S3 | Se **todos** os nós caírem antes do fim da janela, ela se perde (aceito: a janela é curta) |
| 8 | Parada total com estado de até 60 s | Restaura as mensagens, mas o diário tem até `backup.meta.interval` de defasagem | Um recibo ou invisibilidade da última janela pode estar defasado; a mensagem pode voltar entregável antes do prazo. *At-least-once*: o consumidor precisa ser idempotente |
| 9 | `receipt_token` guardado no bucket | O consumidor em voo confirma o ack depois da restauração | O token não é segredo; só evita processamento duplicado |
| 10 | S3 **sem escrita condicional** | A trava de restauração não é segura | Dois restauradores podem divergir. Risco aceito, avisado no log |
| 11 | Vários nós caem, sobra algum | Os sobreviventes assumem os blocos e buscam no S3 o que nenhum nó vivo tem | As mensagens confirmadas ficam disponíveis; as que só existiam no S3 podem estar defasadas por `backup.meta.interval` |
| 12 | S3 fora por muito tempo com tráfego alto | O `outbox` acumula IDs | Se o backlog encher o disco, o nó entra em `failed`: o mesmo risco que já existia, porque o backlog já ocupa o disco |
| 13 | O `aws-sdk-s3` usa `aws-lc-rs` | Compilou no Windows (MSVC) sem ferramentas extras (CMake/NASM) | Nada a fazer; a compilação fica mais lenta |
| 14 | Sobrevivente único exclui os mortos e restaura | Se os mortos estavam só em partição, voltam como nós novos | Os dados deles só em memória podem divergir; o reencontro usa a reunião que já existe (`check_reunion`) |
| 15 | SIGTERM e o envio termina no prazo | Sai com `save_completed` e o batimento removido | Nada fica pendente no bucket |
| 16 | SIGTERM e o envio passa do prazo (`stop_grace_period`) | O Docker manda SIGKILL; o que sobrou no outbox continua pendente | O herdeiro reenvia os pendentes; o que só estava em disco e fora do S3 depende das réplicas |
| 17 | SIGKILL ou falha fatal | Mesmo que o 16 | Mesmo que o 16 |
| 18 | S3 fora do ar na parada | O envio não termina | **Não bloqueia para sempre:** sai após `backup.drain.timeout` |
| 19 | `POST /cluster/stop` com um nó que não responde | O drain dos outros termina; o silencioso não entra na lista | Ele é tratado como morto ao subir; sem perda confirmada se as réplicas existem |
| 20 | `POST /cluster/backup/flush` com um nó lento | As outras respostas voltam; o lento aparece como pendente | O operador repete a chamada |
| 21 | Rota chamada sem S3 configurado | `422 backup.not-configured` | Ninguém acha que fez backup sem ter feito |
| 22 | Parada interrompida antes de gravar a lista | A lista não existe | Vale a regra de morte, com o atraso de `backup.dead-after` |
| 23 | Flush ou drain com o disco chegando a 90% | A etapa 1 pausa; o envio e a evicção liberam espaço | Com o S3 fora nada é liberado: a espera vai até `backup.drain.timeout` e o nó segue o comportamento atual |

## Custo na publicação (medido)

O backup acrescenta ao commit **uma consulta e uma inserção** no `_backup.db` local, nunca uma chamada ao S3. Medido num cluster de 3 nós no mesmo processo (rede em memória), 300 publicações sequenciais depois de aquecer (`cargo test --test backup publish_latency -- --ignored --nocapture`):

| | p50 | p99 |
|---|---:|---:|
| sem backup | 0,92 ms | 1,49 ms |
| com backup, S3 no ar | 1,07 ms | 1,77 ms |
| com backup, S3 **fora do ar** | 1,07 ms | 1,82 ms |

O S3 fora do ar **não muda** a latência (a diferença para sem backup, ~0,15 ms, é o SQLite local). Os números são de uma máquina de desenvolvimento, só mostram que o S3 não está no caminho; não são uma meta de desempenho.

## Limites conhecidos

Os itens abaixo estão em [31-pendencias.md](31-pendencias.md): o diário não é compactado (**PRB-053**); um segmento de um primário que deixou de sê-lo só é reaproveitado pelo `ttl` (**PRB-054**); a restauração lê os segmentos inteiros (**PRB-055**); uma definição removida enquanto o gestor estava fora pode ressuscitar na restauração (**PRB-056**); o custo de procurar mensagens no bucket cresce com o bucket (**PRB-057**).

## Fontes e provas

- `storage/backup` (depósito `ObjectStore` com o S3 e o fake em memória, segmento, diário, nomes, `_backup.db`), `cluster/backup` (`upload`, `evict`, `definitions`, `heartbeat`, `restore`, `stop`), `engine/messages/backup.rs` (o `BackupSink`), `http/backup.rs`, `cluster/protocol/backup.rs`, e os ganchos em `cluster/drain`, `cluster/vote`, `cluster/service` (partida) e `cluster/sync`.
- Casos de uso: **UC-BKP** ([26a-casos-de-uso-implementados.md](26a-casos-de-uso-implementados.md#bkp-backup-opcional-em-s3)). Testes: `tests/backup.rs` (nível C, com o S3 em memória compartilhado e o `Clock` manual: envio, janela, diário, evicção, pausa por disco, partição, três de cinco, sobrevivente único, parada e restauração) e os unitários ao lado dos módulos.
