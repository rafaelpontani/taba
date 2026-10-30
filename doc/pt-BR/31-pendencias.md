---
título: Pendências
resumo: Tudo o que continua em aberto no projeto — dúvidas de design (D…), problemas de lógica, risco ou limite (PRB-…) e os limites conhecidos, cada um com a evidência.
tipo: referência
status: parcial
verificado-em: 2026-10-29 @ 6e8cdf6
fontes: [crates/taba-node/src, connectors, doc/pt-BR/30-decisoes.md]
---

# Pendências

> **Dois tipos de item, dois códigos.** **`D…`** é uma **dúvida de design** (o documento não define, ou há duas saídas), sempre com uma proposta; **`PRB-…`** é um **problema** que a implementação descobriu (um comportamento incompleto, com risco ou com limite conhecido), com o **efeito** e a **proposta**. Os códigos **nunca são reaproveitados**; o que já foi decidido ou resolvido está em [30-decisoes.md](30-decisoes.md).
>
> **Legenda:** `[ ]` em aberto, sem decisão; `[~]` parcialmente resolvida (o texto traz **só as perguntas que faltam**, cada uma com seu sub-código); **Depende de** = a dúvida só se responde depois de outra.
>
> **Mantido à mão**, nas duas pastas de idioma, no commit da mudança que abre, altera ou fecha um item. Os próximos códigos livres seguem os mais altos já usados (hoje **D182** e **PRB-081**).

## Limites conhecidos

Achados conferindo o código e executando o cluster; cada um tem a evidência. Os que dizem respeito à operação também estão em [23-operacao.md](23-operacao.md).

| # | Tipo | O que se viu | Efeito | Proposta | Evidência |
|---|---|---|---|---|---|
| A | incompleto | **A agregação entre nós das métricas de tópico não existe.** `GET /metrics/topics*` com `scope=cluster` devolve a visão do nó que atende; `partial` é sempre `false`; a rota `GET /cluster/metrics/topics` não existe | `depth`/`published` do "cluster" são só os de um nó; num cluster de 3 nós com 12 mensagens, cada nó e o escopo `cluster` responderam 4 | Implementar a rota interna e o fan-out (com `stats.topics.cache-ttl`) ou documentar que só vale por nó | [13-observabilidade.md](13-observabilidade.md); `http/topic_stats/mod.rs`; execução num cluster de 3 nós |
| B | definição | **Quatro parâmetros são aceitos mas não têm efeito:** `cluster.sync.stream.max-line-bytes`, `cluster.sync.stream.idle-timeout`, `fifo.dedup.index.max-entries`, `stats.topics.cache-ttl` | Quem os ajusta acredita que mudou algo; um deles se liga a PRB-022 | Implementar, ou recusar a chave, ou tirá-los da tabela e de [22-configuracao.md](22-configuracao.md) | [22-configuracao.md](22-configuracao.md); script de uso dos campos de `Config` |
| C | risco | **Perder dois nós seguidos trava o cluster** (`quorum-lost`): sobra um nó, a exclusão do segundo não se efetiva, e os reiniciados não reentram (o registro exige todos da tabela). Não há exclusão forçada | Só se recupera parando tudo e subindo de novo; a retomada restaura o que estava em disco ([07-cluster.md](07-cluster.md), seção 10) | A "exclusão forçada" já prevista no doc 04 (evolução futura): rota manual de exclusão, ou tabela por época | [07-cluster.md](07-cluster.md), [23-operacao.md](23-operacao.md); execução com `docker compose kill` |
| E | processo | **Os catálogos de casos de uso ([26a](26a-casos-de-uso-implementados.md)/[26b](26b-casos-de-uso-planejados.md)), as decisões ([30-decisoes.md](30-decisoes.md)) e estas pendências são mantidos à mão.** O registro de onde eram gerados saiu do repositório; o `uc_coverage` lê os cartões `### UC-…` de 26a e 26b | O placar de 26a/26b só muda quando alguém o atualiza | Um gerador que leia os cartões e as linhas `/// UC:` e regenere os dois catálogos nas duas línguas | `crates/taba-node/tests/uc_coverage.rs`; [26-casos-de-uso.md](26-casos-de-uso.md) |
| F | documentação | **O `verificar.mjs` só conhece `doc/pt-BR/`** (chaves de metadados e nomes de arquivo em português); o `verificar-idiomas.mjs` confere a paridade de arquivos, metadados e links entre as pastas | Rotas, chaves e códigos citados só no texto en-US não são conferidos contra o código | Rodar o `verificar.mjs` por idioma e pôr os dois verificadores num job de PR | `doc/pt-BR/scripts/`; [27-ci-cd.md](27-ci-cd.md) |

## Conectores (problemas e limites novos)

Achados ao escrever os conectores Java, Python e Node.js ([21-conectores.md](21-conectores.md)).

| Código | Efeito | Proposta |
|---|---|---|
| **PRB-079** | **Os conectores Java, Python e Node.js não têm job de publicação.** Nenhum pacote está publicado em registro (os identificadores são a D70.1). (A parte de integração deste problema está resolvida: os testes de contrato, D183) | Os jobs de publicação depois que a D70.1 for decidida |
| **PRB-080** | **O cliente `asyncio` do Python roda o transporte síncrono em threads de trabalho** (`asyncio.to_thread`), porque a biblioteca-padrão não tem cliente HTTP assíncrono e o conector não tem dependências. Uma tarefa cancelada fecha a conexão em andamento, mas cada chamada concorrente ainda ocupa uma thread | Medir com muitos long pollings concorrentes; se importar, um HTTP/1.1 próprio sobre os *streams* do `asyncio`, como o conector Rust tem sobre o `TcpStream` |
| **PRB-081** | **Node.js e Python repetem uma chamada que falhou numa conexão do pool que o servidor tinha fechado (`ECONNRESET` antes de qualquer byte da resposta), como uma conexão recusada.** Se o nó já tivesse aplicado a chamada e fechado a conexão antes de responder, uma publicação não-FIFO poderia ser entregue duas vezes. Os clientes HTTP do .NET e do Java fazem o mesmo dentro da biblioteca | Medir a frequência; se importar, enviar `Connection: close` nas publicações sem `deduplicationId`, ou repetir só verbos idempotentes |
| **PRB-082** | **Na primeira rodada smoke do driver do OMB (`bench/run.sh`, um nó com 4 núcleos, `simple-1kb` a 10 000 msg/s por 1 minuto) o Taba publicou cerca de 2 700 msg/s e consumiu cerca de 110 msg/s, e o acúmulo cresceu.** Causa, medida no nó com um cliente que não usa o driver: **o nó, não o driver.** (1) Parte era custo removível: a réplica varria a memória de operações decididas (4096 entradas) e todas as operações pendentes a cada escrita; corrigido, junto com um alocador por thread (a imagem estática musl gastava cerca de um quinto da CPU em locks do `malloc`): o mesmo nó passou de cerca de 3 200 para 5 400 publicações/s com 4 núcleos, e a rodada smoke publicou 9 200 msg/s. (2) O que sobra é de projeto: cada mensagem custa três requisições HTTP/1.1 (publicação, lote de `consume`, **um `ack` por mensagem**) de cerca de 0,7 ms de CPU, e o nó enfileira o trabalho de 1 000 requisições simultâneas por ordem de chegada, então um `consume` ou um `ack` espera atrás das publicações (cerca de 350 ms) e o consumo cai para cerca de 100 msg/s enquanto o acúmulo cresce. A 10 000 msg/s o nó precisaria de cerca de 20 000 requisições/s. (3) O caminho da publicação ficou mais barato (2026-10-28, sem mudar contrato): com um único nó responsável, registro e commit na própria tarefa e, só em memória, sem thread bloqueante; tópico sem banco lembrado (sem `stat` por publicação); ids sem lock e sem `getrandom`; métricas com atômicos; spans só com OTLP; estado da aplicação num único `Arc`; definição do tópico compartilhada. Medido com `wrk` na rede do Docker (4 núcleos, 1 KB, 128 conexões, não representativo): de cerca de 9 400 para 26 600 publicações/s, p50 de 13 para 4,6 ms, e de cerca de 32 para 2 chamadas ao sistema por publicação. O cliente em Node.js da medição anterior não satura mais o nó. (4) Com três nós e fator 3 (3 núcleos por nó, mTLS, `wrk` espalhado pelos nós) o cluster publica cerca de 7 500 msg/s (antes da mudança, 4 500), contra 26 000 de um nó com fator 1 nos mesmos 3 núcleos (antes, 8 300), com os três nós saturados: cerca de 1,1 ms de CPU somada por mensagem, contra 0,1 ms. Cada publicação faz quatro chamadas HTTP entre nós (registro e commit em cada uma das duas réplicas), com o corpo em base64 dentro de JSON (5) As chamadas entre nós viraram lotes em protobuf (D186, 2026-10-28; mesma configuração e mesma execução que a (4), `bench/cluster-trend.sh 3 64`): o cluster com fator 3 foi de cerca de 8 300 para 19 500 msg/s na primeira rodada (p50 de 22 para 9,6 ms) e de 7 600 para 15 000 na média de três rodadas; as rodadas seguintes caem (13 600, 11 800) conforme o acúmulo cresce, porque ninguém consome, e a memória de cada nó dobra (0,5 para 1 GB) com o dobro de mensagens guardadas. Um nó com fator 1 não mudou além do ruído (23 100 para 26 200) | Decidir, com uma medição por opção: um `ack` de várias mensagens numa só requisição (aditivo, compatível), publicação em lote, e dar precedência ao `consume`/`ack` sobre as publicações quando o nó satura. Reproduzir com `bench/run.sh --systems taba --workloads simple-1kb` |
| **PRB-083** | **As chamadas entre nós que não são operações sobre mensagens ainda são JSON.** A D186 passou o `register`, o `commit` e o `rollback` das escritas, invisibilidades e acks, o consumo entre nós (`take`) e o repasse da FIFO para lotes em protobuf. Ainda são JSON: o repasse de chamadas do cliente, as listas de sincronização, recuperação e restauração (NDJSON com os corpos em base64), as rotas de controle (entrada, membros, votos, definições, status, backup) e os quadros do canal permanente (WebSocket). As respostas de erro da porta do cluster também são o `ErrorResponse` JSON da camada HTTP. Efeito: essas chamadas mantêm o custo do texto, e as listas de sincronização levam corpos em base64 | Passá-las ao mesmo esquema (decidido: entre nós, só protobuf), primeiro as que levam corpo (as listas), as rotas de controle e o canal por último; o plano é `doc/plans/cluster-batch-protobuf.md` (fases 2 e 4) |
| **PRB-085** | **Um ack e uma ida para a dead-letter da mesma mensagem poderiam ser decididos os dois se o register de um deles expirasse no dono (`write.pending.ttl`, 30 s) antes de o commit chegar.** Não visto: as falhas que lhe foram atribuídas eram o [PRB-077](30-decisoes.md#problemas-resolvidos-prb), outra corrida. A leitura é que o register expirado libera a mensagem, a outra operação a toma e dá commit, e o commit da primeira registra de novo com `Request::decided` (sem verificação) e é confirmado. Exige uma parada de mais que o `write.pending.ttl` no meio de uma operação, por exemplo uma partição longa, que a rodada de caos não faz (os saltos de relógio dela são de 8 s) | Provar com um teste que expira o register entre as duas operações (como o `tests/replication.rs` faz só para o ack, PRB-076); depois decidir como distinguir um register expirado de um decidido (manter a operação no dono até o commit ou o rollback, ou checar no commit que a mensagem ainda é a que foi registrada) |
| **PRB-086** | **Um backlog de cerca de um milhão de mensagens de 1 KB leva o nó à memória do contêiner, e o consumo cai para dezenas ou centenas de mensagens por segundo.** Achado pelo perfil `consume` do benchmark (`backlog-1kb`, servidor de 3 GB, `replication.factor=1`, 2026-10-29): quando o OMB libera o consumidor (1,07 milhão de mensagens, que ele conta como 1 GB) o processo guarda cerca de **3,1 GB de memória anônima** (cerca de 2,4 KB por mensagem de 1 KB), o cgroup bate no limite centenas de vezes (`memory.events: max 306`), cada ack leva cerca de 156 ms e cada leitura de 100 mensagens cerca de 520 ms em média, e o consumo fica em 30 a 150 msg/s enquanto o backlog continua crescendo. O mesmo nó drena 1 100 a 3 700 msg/s com o `wrk` num backlog de até 1,4 milhão, com uma conexão e sem publicação ao mesmo tempo, então o custo não é só o tamanho do backlog, e sim a pressão de memória junto com a publicação que continua durante a drenagem. Ligar `TABA_MEMORY_LIMIT=1536MB` não baixou os 3,1 GB, então o que o limite conta parece não incluir tudo o que uma mensagem custa. Sem o limite, o nó foi morto pelo limite de memória do contêiner quando o backlog passou de 2 milhões de mensagens. Medido depois sem Docker ([25-testes.md](25-testes.md#medir-o-custo-de-uma-mensagem-sem-docker), `tests/memory_cost.rs`, um alocador contador no nó inteiro com 5 000 mensagens de 1 KB pela API): **1 938 bytes pedidos e 2 088 reservados por mensagem para um corpo de 1 024**, ou seja, de 900 a 1 060 bytes além do corpo. Os blocos vivos são o corpo (1), o id como uma `String` de 36 caracteres (5 cópias: a memória por id, a memória por idade, o `MessageMeta`, e as duas do índice, que só guarda `delivery.ready-list.size`, 1 000 por padrão, então um backlog fundo tem 3), o tipo de conteúdo (2 cópias de uma `String`) e os nós dos dois mapas ordenados, o maior sendo a folha da memória por id (2 744 bytes para cerca de 7 mensagens, 384 bytes por mensagem, porque cada `Stored` tem 250 bytes e as folhas ficam 65 % cheias). A primeira estimativa de "770 bytes fora do motor" estava errada: veio de comparar bytes vivos com a memória do cgroup, e a diferença são as páginas do alocador, não blocos que o nó guarda. **O limite conta só `body.len()`** (`put_in_memory`, `reserve_memory`), então nunca limita o resto. A lentidão do consumo tem outra causa, achada pelo mesmo método e corrigida na [D188](30-decisoes.md): o ack custava 105 µs no motor por causa de uma escrita no log de mensagens excluídas a cada ack; com isso corrigido o ack custa 16 µs. Ainda não medido de novo: o perfil `consume` do benchmark com essa correção | Cortar a sobra do motor: uma só cópia do id compartilhada pelos mapas (um `Arc<str>` ou os 16 bytes do GUID), o tipo de conteúdo internado, os campos pouco usados do `MessageMeta` fora do registro quente, um mapa que não deixe as folhas 35 % vazias; fazer o `memory.limit` contar o custo inteiro de uma mensagem, e não só o corpo; se o transbordo para o disco é a saída para um backlog fundo, conferir que ele mantém a taxa de consumo; rodar o perfil `consume` de novo (com `--memory` suficiente para o backlog) para ver o que a correção do ack mudou. |
| **PRB-087** | **O caminho do cluster custa cerca de 11 µs por mensagem além do motor, e um tópico não usa mais núcleos.** Medido no processo com um nó (`tests/memory_cost.rs::cost_of_the_cluster_path`, build release): publicar custa 14 µs pelo `ClusterMessages` contra 2,6 µs no motor, e o ack 12 µs contra 2,9 µs; com 128 ao mesmo tempo o tempo por mensagem é o mesmo, porque os itens de um lote rodam na task da requisição e o motor pega o bloqueio do tópico para cada um. Um perfil (perf, num contêiner Linux) mostra o tempo espalhado em chamadas ao heap (cópias do `Request`, dos ids e do nome do tópico em cada fase), nas métricas com rótulos (`Registry::with_series`) e nos spans de cada fase. Efeito: um tópico chega a cerca de 85 mil mensagens por segundo publicadas ou confirmadas em lote nesta máquina, abaixo do que o Kafka drena no perfil `consume` do `bench/run.sh`. No contêiner Docker do benchmark (4 CPUs, 3 GB) é pior: o nó gasta de 135 a 225 µs de CPU por mensagem publicada em lote no distribuidor do OMB (20 mil por segundo com 2,8 núcleos ocupados, 15 mil durante um backlog que cresce), e o backlog do `backlog-1kb` chega aos 3 GB do contêiner em cerca de 140 s (PRB-086); o Kafka publica os 50 mil por segundo do workload e drena 154 mil na mesma rodada. | Contar as chamadas ao heap por operação do caminho do cluster (`heap_cost_of_a_message`) e cortar as cópias (compartilhar o `Request` do registro e do commit, `Arc<str>` para o tópico e o id); métricas sem rótulos no caminho quente; espalhar os itens de um lote pelos workers (`tokio::spawn` por grupo de itens); uma operação em lote no motor que pegue o bloqueio do tópico uma vez para várias mensagens; depois medir de novo com o perfil `consume` contra o Kafka. |

## Backup em S3 (problemas e limites novos)

Achados ao implementar o plano do backup ([33-backup-s3.md](33-backup-s3.md)). Cada um tem o **efeito** e a **proposta**.

| Código | Efeito | Proposta |
|---|---|---|
| **PRB-053** | **O diário não é compactado.** Cada `backup.meta.interval` grava um diário por tópico e o bucket só os apaga junto com o tópico: a restauração lê todos, e a leitura cresce com o tempo | Um *snapshot* periódico por tópico (o estado de todas as mensagens vivas) que substitui os diários anteriores; a restauração lê o último snapshot e os diários depois dele |
| **PRB-054** | **Um segmento de um primário que deixou de sê-lo só é reaproveitado pelo `ttl`.** O herdeiro não tem a contagem de vivas do antigo, e as referências avisadas às réplicas não a trazem. O gestor apaga um segmento quando **todas** as mensagens passaram do `ttl` (padrão 14 dias); até lá o bucket guarda bytes mortos | Fazer o gestor calcular a vida do segmento pelos rodapés e pelos diários (nunca pela memória), com um cache dos rodapés |
| **PRB-055** | **A restauração lê cada segmento inteiro** (e todos os diários do tópico), em série: num bucket grande a partida do sobrevivente demora. A busca de mensagens perdidas (`recover_missing`) lê só rodapés e `Range`, mas também lista todo o bucket do tópico | Restaurar por tópico em paralelo e filtrar pelo anel já na leitura; restaurar só as mensagens de que o nó é responsável |
| **PRB-056** | **Uma definição removida enquanto o gestor estava fora do ar pode ressuscitar na restauração:** a lápide e a revogação saem da fila em memória do gestor se ele cai antes de gravá-las. A reconciliação periódica **só grava**, nunca apaga do bucket o que a memória não tem (apagar daria perda geral num nó que subiu vazio) | Gravar a lápide e a revogação num `outbox` durável no `_backup.db` |
| **PRB-057** | **A busca no bucket depois de uma exclusão por morte custa o tamanho do bucket do tópico** (listagem, diários inteiros, um rodapé por segmento) e roda em cada sobrevivente | Um índice leve de ids por segmento mantido pelo gestor |

Também ficam registrados como **limites aceitos** (cenários 2, 7, 8 e 10 de [33-backup-s3.md](33-backup-s3.md)): S3 fora só para um nó vivo, mensagem confirmada e consumida antes de `backup.delay` quando **todos** os nós caem, estado defasado pela janela do diário (consumidor idempotente), e S3 sem escrita condicional (a trava de restauração não é segura). A votação de exclusão **rejeitada** não é repetida sozinha (herdado do protocolo de votação; no backup o atraso até o batimento vencer deixa isso raro).

## Trabalho de servidor da TUI (problemas e limites novos)

Achados ao implementar D152 a D155 ([30-decisoes.md](30-decisoes.md)). Cada um tem o **efeito** e a **proposta**.

| Código | Efeito | Proposta |
|---|---|---|
| **PRB-058** | **Um prune não cobre um nó que está sincronizando, nem uma mensagem publicada enquanto ele corre.** Um nó sincronizando esvazia o que tem, mas os blocos que copia depois, ou os eventos que gravou, podem trazer mensagens de volta; uma mensagem publicada durante o prune pode sobreviver. Não foi exercitado em teste se o backup em S3 restaura uma mensagem esvaziada depois de um reinício | Fazer o nó sincronizando responder o prune com `503 cluster.node-syncing` (a pessoa repete quando ele estiver pronto), ou gravar o prune como evento do log de sincronização; acrescentar um caso de prune com o backup em S3 |
| **PRB-059** | **As estatísticas consolidadas são aproximações em dois campos, e o `/metrics/topics*` da API de operação continua a visão de um nó só** (achado A, em parte resolvido pela D156: o `/stats/topics…` da API de cliente junta o cluster). Os percentis de latência são o **máximo** dos nós (o percentil real do cluster exige os histogramas), e `groups` e o maior grupo são o máximo (um limite inferior; a condição de grupo ruidoso pode não ver um grupo espalhado entre nós); quem lê o percentil do cluster recebe um limite superior | Mandar os histogramas de latência e as listas de grupos entre os nós; ou fazer o `/metrics/topics*` da API de operação consolidar também |
| **PRB-061** | **Um nó que saiu por drain só é listado e endereçável enquanto o nó que recebe se lembra dele** (até 64, em memória): depois que o receptor reinicia, `GET /nodes` já não o mostra e `/nodes/{nodeId}/drain` responde `404`, então ele não pode ser devolvido pela API | Guardar os nós que saíram no banco do nó, ou derivá-los da lista de parada e dos batimentos do S3 |
| **PRB-063** | **A deduplicação da FIFO na liberação tem os limites da janela e do primário** ([07-cluster.md](07-cluster.md), seção 10.9): (a) usa as chaves de deduplicação que o primário ainda lembra, que vivem `fifo.dedup.window` (300 s) a partir do `id`; com `cluster.restore.grace` igual à janela, uma mensagem restaurada depois de `G` pode não achar mais a chave da publicação que ela repete; (b) a lista provisória está na memória do primário do grupo: se o primário muda durante a restauração, as publicações aceitas pelo antigo deixam de constar na lista e não são deduplicadas; (c) o gerador fica acima das marcas d'água que o nó viu, e uma publicação aceita antes dos manifestos serem anunciados pode receber um `id` abaixo da marca d'água de uma vida ainda não anunciada | (a) ampliar `fifo.dedup.window` enquanto o cluster restaura, ou restaurar as vidas antes do fim da janela; (b) reconstruir a lista a partir das mensagens com `id` acima da marca d'água quando um novo primário assume o grupo; (c) reter as publicações FIFO até a primeira consulta responder |
| **PRB-064** | **A partida a frio tem três limites** ([07-cluster.md](07-cluster.md), seção 10.5): (a) a eleição do que parte sozinho usa quem responde no momento; se dois nós se veem de modo assimétrico (um alcança o outro e não o contrário) ou o menor ID antigo responde tarde, dois partem sozinhos e formam clusters separados, que só o reencontro de grupos junta; (b) o nó que parte sozinho recua a data de início do log de excluídos à da vida anterior sem provar que nenhum nó da tabela antiga ficou de pé gravando no buraco (um nó que continuou de pé em outra partição pode ter exclusões que este cluster não vê, e a reentrega residual da seção 10.10 as cobre); (c) a oferta das definições do disco de um nó que **entra** vale pela versão, e a lápide só protege enquanto não expira (`TOMBSTONE_TTL`): uma definição que o cluster apagou há mais tempo que isso e que ainda está viva num disco volta ao cluster | (a) o desempate do reencontro de grupos; (b) conferir o `last_seen` das vidas dos nós presentes (se todos estavam parados no buraco, a data pode recuar); (c) aceitar o risco ou guardar as lápides por mais tempo que a ausência esperada de um nó |
| **PRB-065** | **A varredura do S3 tem dois limites** ([33-backup-s3.md](33-backup-s3.md), seção "Varredura na partida"): (a) o diário só registra a remoção depois do atraso do backup; uma mensagem confirmada nesse intervalo, cujo log se perdeu ou foi aparado (nó novo, log mais novo que a mensagem), volta da varredura: é *at-least-once*, e o consumidor precisa ser idempotente; (b) a varredura lê o rodapé de **todos** os segmentos de cada fila a cada partida com backup, e o custo cresce com o balde (proposta: indexar por faixa de `id` dos blocos do nó, ou limitar pelo `ttl`). |
| **PRB-066** | **A sondagem dos IPs antigos tem limites** ([07-cluster.md](07-cluster.md), seção 10.11): (a) passado o tempo (`cluster.restore.probe.timeout`), dois grupos que se encontrem depois **não** se juntam sozinhos: o operador reinicia o menor, que entra pelo join normal (proposta: continuar sondando num intervalo maior depois do tempo); (b) a completude é por **contagem** de membros ativos, então um cluster que o operador reduziu de propósito mostra `members-missing` até o tempo acabar; (c) o nó perdedor fica fora de serviço (entrando) durante o reencontro, e as chamadas de clientes a ele falham até ele voltar; (d) só o que o grupo perdedor já tinha em disco é restaurado: o que estava só em memória se perde, como em toda partida ([07-cluster.md](07-cluster.md), seção 10). |
| **PRB-067** | **Um reinício completo não leva as linhas de deduplicação das mensagens FIFO confirmadas** (achado quando os testes foram para o perfil de um nó): a restauração de uma vida só envia as mensagens que estavam vivas, então a linha curta que o ack deixa para a janela de deduplicação se perde, e o mesmo `deduplicationId` publicado depois do reinício é aceito de novo (`201` em vez de `200`). A chave de uma mensagem que estava viva continua deduplicando (proposta: enviar junto com a vida as linhas curtas que não expiraram, como faz a sincronização). |
| **PRB-068** | **No caminho do cluster, uma mensagem além do `ttl` ainda pode ser confirmada ou ter a invisibilidade alterada até o varredor removê-la** (ele roda a cada segundo, no máximo): `ack` e `visibility` conferem a mensagem no dono sem acertar a fila antes, ao contrário do caminho de um nó que havia antes. Achado quando os testes foram para o perfil de um nó; o teste agora espera o varredor (proposta: acertar a fila antes do passo de registro de `ack` e `visibility`). |
| **PRB-069** | **Enquanto um nó está em drain, um token comum não lê `GET /metrics` na porta de cliente.** Um nó em drain só valida o token raiz (os outros tokens são dados, e o nó conta por IP e repassa), e `/metrics` pede um token válido mas fica fora da recusa do drain, então só o token raiz lê ali as métricas de um nó em drain. A porta de operação (sem token) e o histórico que os coletores guardam não são afetados | Manter, já que a porta de operação é a porta dos coletores (D177), e documentar; alternativa: deixar `/metrics` aceitar qualquer token que o nó ainda lembre enquanto drena |
| **PRB-071** | **A permissão que uma rota exige só está na descrição.** Os documentos OpenAPI dizem `Permissão: admin` no texto, mas não num campo que um gerador ou um gateway leia (não há `x-required-permission`, e o esquema de segurança não tem escopos) | Acrescentar a extensão em cada operação, com um teste que a confira com o `need()` de `http/auth`, para os dois não diferirem |
| **PRB-072** | **Algumas respostas estão documentadas como objeto livre.** Os relatórios do backup em S3 (`/nodes/{nodeId}/backup`, `/cluster/backup`, as rotas de flush) e os `signals` do status de um nó são `object` sem campos, porque o servidor os monta com `json!`; um cliente gerado recebe um mapa sem tipo ali | Dar a cada um uma `struct` (ou um gêmeo de documentação conferido pelo `tests/openapi.rs`), como foi feito com os outros DTOs |
| **PRB-073** | **O `cargo audit` ignora o `RUSTSEC-2026-0387` (`crypto-bigint` 0.5.5, variação de tempo).** O crate vem do `aws-sigv4` 1.6.3 (o mais recente), que o backup em S3 usa, e nenhuma versão publicada evita o `crypto-bigint` antigo. Só o assinador SigV4a (ECDSA) alcança as operações afetadas, e o backup assina com SigV4 (HMAC), então o nó não está exposto; a exceção está em `.cargo/audit.toml` | Quando o `aws-sigv4` depender de `crypto-bigint` >= 0.7.6 (`cargo update -p aws-sigv4`), apagar a linha de `.cargo/audit.toml` e este item |

## Rodada de caos (problemas e limites novos)

Achados pela rodada de caos ([37-garantias.md](37-garantias.md)).

| Código | Efeito | Proposta |
|---|---|---|

**Padrão de `cluster.vote.min-voters` em clusters de 4 nós ou mais (D182).** O padrão (2) deixa cada metade de um cluster de 4 nós excluir a outra numa partição 2 + 2; as duas metades aceitam escritas e o perdedor se reagrupa quando elas se encontram (D181, nenhuma mensagem se perde). Uma maioria da tabela como padrão evitaria a divisão, ao custo de nenhuma escrita numa partição 2 + 2 (nenhuma metade tem maioria). Proposta: manter 2 (disponibilidade) e documentar a maioria para quem prefere consistência; a alternativa é uma maioria por padrão.

## 1. Sem bloqueio

Podem ser respondidas **agora**, em qualquer ordem: não dependem de outra pergunta nem de etapa.

### D68

- **Estado:** `[~]` parcial · **Tipo:** dúvida de design

**Versões-base dos conectores (as demais linguagens): confirmar a tabela.** As do **C#** (.NET Framework 4.6.1 e .NET 5) e do **Java 11, Python 3.9 e Node.js 18** estão decididas (D68.1, D68.2). A "versão-base" é a versão mais antiga de cada linguagem que o conector aceita rodar. Em vez de escolher cada uma no olho, a proposta é uma regra só, derivada do que já foi decidido: a base é a **mais antiga** que (1) fala TLS 1.2 ou superior (D69), (2) tem cliente HTTP com `PATCH` (D72) e, onde a linguagem tem assíncrono, o usa (D71). A tabela que sai da regra está em [21-conectores.md](21-conectores.md), seção 18.10.1: **PHP 5.6, Ruby 2.0, C89 com `libcurl` 7.34, C++03, Go 1.7 e Rust 1.70** para as linguagens ainda por fazer (a regra dava Python 2.7.9/3.4.3 e Node 0.12, e o Java 11 deixa o **Java 8** de fora só por causa do `PATCH`; a D68.2 subiu o Python e o Node.js). A pergunta é: **a regra e a tabela estão certas para as linguagens ainda por fazer?** A mesma dúvida do Python e do Node.js vale para o PHP 5.6 e o Ruby 2.0: uma base antiga proíbe os idiomas que as pessoas daquela linguagem esperam. O Rust 1.70 é uma estimativa a fechar no protótipo.

### D70

- **Estado:** `[~]` parcial · **Tipo:** dúvida de design

**Publicação dos conectores.** Já decidido: o lugar de cada um (a regra "mais popular e gratuito"). Falta:
  - **D70.1.** O **nome da organização** e os identificadores de pacote em cada registro: Maven *groupId* (que exige um domínio ou uma organização verificada), NuGet, PyPI, Packagist, RubyGems, npm, crates.io e o caminho do módulo Go (que é o do repositório).
  - **D70.2.** O **PHP** (Packagist exige o `composer.json` na raiz) sai de um **espelho somente leitura** por `git subtree split`? E as versões dos conectores são **independentes** da do servidor, como proposto (SemVer próprio, declarando a versão do contrato)?

### D75

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Drain pedido pela API que estoura `drain.timeout`.** Hoje o nó fica em `draining` (log `error`, sem a marca do save) até alguém chamar `DELETE /node/drain`; o texto de [07-cluster.md](07-cluster.md) (seção 6.10) só diz o que acontece com o drain por sinal (encerra mesmo assim). A proposta é manter assim e dizer isso em 6.10; a alternativa é o nó voltar sozinho a `active`. Origem: o drain pedido pela API não estava no texto de 6.10. Problema ligado, **PRB-004** (lógica, `cluster/drain`): O drain pedido pela API que estoura o prazo (ou tem erro de disco) fica em `draining` sem terminar nem voltar. Efeito: O nó fica fora da rotação sem a marca do save, esperando um `DELETE` do operador.

### D76

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**`operations.allowed-ips` e os health checks.** O [22-configuracao.md](22-configuracao.md) manda filtrar `/health/*` junto com o resto, mas o `kubelet` e o balanceador chamam `/health/live` e `/health/ready` de IPs que o operador raramente lista. Perguntas: **D76.1** `/health/live` e `/health/ready` ficam **fora** do filtro (proposta, pois só dizem "vivo" e "pronto")? **D76.2** Se ficarem dentro, o `403` é `ErrorResponse` (como hoje) ou `HealthResponse`, já que [06b](17-api-operacao.md) diz que essas duas rotas nunca devolvem `ErrorResponse`? Problema ligado, **PRB-006** (definição, `http/operations`): O `allowed-ips` filtra também `/health/live` e `/health/ready`, e o `403` sai como `ErrorResponse`, contra o que o 06b diz dessas rotas. Efeito: O orquestrador pode ler `403` como nó doente.

### D77

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**`Retry-After` do `503` do drain.** Hoje fixo em 5 s. Proposta: manter fixo (o cliente só precisa esperar o balanceador tirar o nó); a alternativa é um parâmetro.

### D80

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Nome de tópico e nomes reservados do Windows.** O nome do tópico vira nome de pasta ([06a](16-api-cliente.md)), e `con`, `nul`, `aux`, `com1`... não podem ser pasta no Windows. Proposta: a pasta ganha o prefixo `_` quando o nome (sem maiúsculas) é reservado; a alternativa é proibir esses nomes de tópico. Problema ligado, **PRB-008** (limite, `storage/messages`): Nome de tópico igual a um nome reservado do Windows (`con`, `nul`...) não vira pasta. Efeito: Publicar nesse tópico no Windows dá erro de disco (`500`).

### D84

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Padrões do rate limit.** O [22-configuracao.md](22-configuracao.md) deixa `ratelimit.publish.rps`, `consume.rps`, `admin.rps` e `burst` como "A definir" (só `ops.rps` tem sugestão, 2). Sem valor, o nó hoje **não limita** a categoria. Proposta: manter sem limite por padrão (quem instala escolhe), e só sugerir valores na documentação; a alternativa é um padrão generoso (por exemplo 1000 rps de publicação e consumo, 20 de administração).

### D87

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O teto `topics.volatile.max-count` vale para o `POST /topics`?** Hoje só o `autoCreate` é barrado (é o que o [22-configuracao.md](22-configuracao.md) diz), mas uma fila volátil criada por `POST /topics` já entra na conta, então o `autoCreate` pode ser recusado por causa delas. Proposta: manter só o `autoCreate` barrado (quem usa `POST` é o operador, que sabe o que faz).

### D89

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Teto do mapa de deduplicação da FIFO.** `fifo.dedup.index.max-entries` está "A definir" em [22-configuracao.md](22-configuracao.md). Pergunta: qual o padrão e o que fazer ao atingir? A proposta é um teto por tópico (por exemplo 1.000.000 de chaves) e, atingido, **recusar a publicação nova** com `503` e `Retry-After` (em vez de esquecer chaves da janela e duplicar em silêncio); a alternativa é descartar as mais antigas e registrar `warn`. Problema ligado, **PRB-022** (limite, `engine/messages`): O mapa de deduplicação da FIFO guarda uma entrada por publicação da janela e não tem teto (`fifo.dedup.index.max-entries` está "A definir"). Efeito: Com chaves únicas por mensagem, a memória cresce com a vazão × 300 s; uma rajada de milhões enche o mapa.

### D90

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Padrões dos limiares das condições.** [22-configuracao.md](22-configuracao.md) deixa "A definir" `stats.topics.backlog.depth`, `backlog.age`, `stalled-after`, `idle-after`, `group-stalled-deliveries`, `stats.disk.free-min` e `stats.topics.cache-ttl`. Hoje, sem valor, a condição **não dispara** (e o cache não existe). Pergunta: quais os padrões, e a condição deve vir ligada? A proposta: `backlog.depth` 10000, `backlog.age` 15 min, `stalled-after` 5 min, `idle-after` 1 h, `group-stalled-deliveries` 5 (a sugestão do doc), `stats.disk.free-min` 1 GiB e `cache-ttl` 5 s. Junto: `memory-pressure` hoje é `diskMessages > memoryMessages` (o doc 11 diz ">>"); aceitar essa definição ou usar uma razão (por exemplo, o dobro)? Problema ligado, **PRB-023** (risco, `engine/messages`): Um grupo FIFO preso numa mensagem que ninguém confirma (consumidor que cai a cada tentativa) só sai pelo `maxDeliveries` ou pelo `ttl`; sem eles, o grupo para para sempre. A condição `group-stalled` existe desde a etapa 12, mas só dispara com `stats.topics.group-stalled-deliveries`, que está sem valor. Efeito: O backlog do grupo cresce sem aviso enquanto o limite não for definido.

### D91

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Métricas de tópico em `/metrics`.** (a) `telemetry.topics.max-topics` está "A definir"; hoje vale 1000 no modo `per-topic`. (b) No `per-topic` as séries levam só as tags de `telemetry.topics.group-by-tags` (o doc diz "mais as tags", sem dizer quais): conferir. (c) `taba.topic.condition` só sai no `per-topic`; no `by-tags` a proposta é não emitir. Pergunta: confirma esses três?

### D93

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Posição, bloco e primeiro nó.** O [doc 05](06-anel-e-replicacao.md) (seção 7.1) não dizia como se calcula a posição no anel nem o que é o `{id}` de um bloco. A implementação usou: posição = os 8 primeiros bytes do SHA-256 da chave; nó virtual `<nodeId>#<i>`; bloco = o arco que termina num nó virtual, com `{id}` = a posição dele em 16 dígitos hexadecimais; réplicas = os nós físicos seguintes no anel. E o nó com `cluster.first-node` começa sozinho só quando a listagem não tem **nenhuma resposta de rede**. Pergunta: confirma? A alternativa para o bloco é um número fixo de faixas iguais do espaço de hash (por exemplo 4096), independente dos nós, o que daria blocos estáveis quando o cluster muda (a sincronização trabalharia por faixa fixa, não por arco que se parte).

### D96

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Quem fixa o prazo do `PUT visibility` e do ack, e o prazo do repasse.** **D96.1** O [doc 06c](18-api-cluster.md) diz que o coordenador da invisibilidade é sempre o dono da entrega, para o relógio que fixa `invisibleUntil` ser o dele. No `GET` e no `take` isso vale; no `PUT` e no ack quem coordena é o nó que recebeu a chamada, com o relógio dele (o dono não sabe quem recebeu). Proposta: aceitar (a diferença de relógio entre nós já é aceita no at-least-once, doc 14, seção 13.3); a alternativa é repassar o `PUT` ao primário. **D96.2** O repasse tem prazo fixo de 60 s no código. Proposta: um parâmetro `cluster.forward.timeout` com esse padrão. Problema ligado, **PRB-034** (limite, `cluster/replication/forward`): O prazo do repasse é fixo (60 s) e o `502`/`504` depois de enviado o corpo (UC-MSG-017) não tem teste. Efeito: Sem como ajustar o prazo nem provar o comportamento de um destino mudo.

### D99

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O voto quando o candidato avisou que entrou em falha.** O `vote-remove` responde `yes` a quem não alcança o candidato; mas um nó em `failed` está vivo e o canal com ele segue de pé, então todo votante respondia `no` e o [doc 05](07-cluster.md) (seção 7.3) não se cumpria ("os demais ficam liberados para votar o desregistro dele"). Implementado: com `reason: node-failed`, o votante responde `yes` se o canal dele recebeu o `node-state failed` do candidato; senão confere se o alcança. Pergunta: o aviso deve ser conferido assim, ou o `VoteRequest` deve levar a prova (o estado que o abridor viu)?

### D102

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**`quorumAvailable` e `quorum-lost`.** O [doc 11](13-observabilidade.md) diz "menos nós saudáveis que o mínimo de votantes" e não diz de quem é o ponto de vista nem o que acontece com um cluster de um nó. Implementado (provisório): cada nó conta a si mesmo e os pares com o canal conectado, e o mínimo vale `min(cluster.vote.min-voters, membros)`, para o primeiro nó de um cluster novo não sair em `quorum-lost`. Em `scope=cluster` o `quorumAvailable` do resumo é o E dos nós que responderam. Pergunta: aceita? Alternativa: contar também os nós que não responderam à consulta do `/health/cluster`.

### D109

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Constantes do canal que não são parâmetros.** Fixas no código: o aviso expira na fila de saída em 10 s (`NOTIFY_TTL`); 3 respostas `not-member` seguidas bastam para conferir se o nó foi excluído (`EXCLUDED_AFTER`); a chamada do canal tem prazo de `2 × cluster.channel.keepalive.interval` (mínimo de 500 ms), que é também o "tempo sem sinal" que derruba o canal. Pergunta: viram parâmetros (`cluster.channel.notify-ttl` etc.) ou ficam fixos e documentados?

### D113

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O recibo da FIFO carrega a posição do grupo.** O ack e a alteração da invisibilidade chegam a qualquer nó, que só tem o `id` e o `receiptToken`, mas o bloco de uma mensagem FIFO vem do grupo (que o doc 17 não diz como achar). Implementado (provisório): `receiptToken = {posição}-{guid}` só na FIFO. Pergunta: aceita? Alternativa: pôr o `groupId` como parâmetro do ack e da alteração (muda a API de cliente).

### D114

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O que `delivery.get.max-attempts` conta e o padrão.** O [doc 13](12-entrega.md) (seção 12.2) diz que o número de falhas toleradas por `GET` é limitado por ele, sem valor. Implementado (provisório): conta cada mensagem cujo registro de invisibilidade não fechou (já depois das repetições de `write.retry.*`); o `GET` segue para o próximo ID da lista, sem esperar, até **3** falhas; vale em cada nó que entrega, então o `GET` que também pede mensagens a outros nós (`take`) pode somar até 3 falhas por nó. Esgotado, vale UC-CNS-020 (devolve o que capturou) ou UC-CNS-021 (`503`). Pergunta: o padrão é 3? O teto deveria ser do `GET` inteiro, e não por nó?

### D115

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Padrões do gestor das voláteis e a consulta de ociosidade.** O [doc 10](22-configuracao.md) deixava `topics.volatile.idle-check.retry-interval` (sugestão 30 s) e `topics.volatile.takeover.jitter` sem valor. Implementado (provisório): 30 s e 2 s. O espalhamento é um atraso estável por fila (o hash do `id` módulo a janela), não sorteado, e a primeira consulta de uma fila que o nó passa a gerenciar acontece nesse atraso depois de ela aparecer no conjunto dele. `idleSeconds` é um inteiro (segundos cheios, arredondado para baixo, o que só pode adiar a exclusão), e o gestor também limita a ociosidade pela idade da fila (`agora − createdAt`), porque um nó sem registro de `GET` responde o tempo desde que subiu. Pergunta: aceita?

### PRB-005

- **Estado:** problema · **Tipo:** definição

(definição, `http (drain)`): No drain o nó recusa **toda** chamada de cliente, inclusive o ack e o `PUT visibility`. Sem outro nó para repassar, o consumidor não confirma o que já recebeu. Efeito: A mensagem entregue volta a ficar pronta quando a invisibilidade vence (duplicata) se o nó seguir em drain por mais que o `visibilityTimeout`. O `receiptToken` só volta a valer depois do `DELETE` ou do reinício. Proposta: durante o drain, no modo desenvolvimento, o ack e o `PUT visibility` passam (só a entrega e a publicação são recusadas). Alternativa: manter recusando tudo.

### PRB-015

- **Estado:** problema · **Tipo:** risco

(risco, `http/ratelimit, http/client_ip`): Com `client-ip.trusted-proxies` vazia (o padrão), qualquer cliente forja `X-Forwarded-For` e escolhe o próprio IP. Efeito: Escapa do rate limit (cada pedido com um IP novo) e faz o nó guardar um contador por IP inventado, até a poda dos contadores já cheios (acima de 4096). O doc 12 aceita o risco só para o nó atrás de balanceador. Proposta: o padrão sem balanceador é `client-ip.headers.enabled=false`. Alternativa: manter e documentar o risco.

### PRB-017

- **Estado:** problema · **Tipo:** risco

(risco, `engine/tokens`): A validação de um token com `id` desconhecido responde mais rápido que a de um `id` existente com segredo errado (não calcula o hash). Efeito: Em tese permite descobrir, por tempo de resposta, quais `id` existem. O `id` não é segredo (aparece em logs) e o segredo de 256 bits não se adivinha, então o dano é baixo. Proposta: aceitar (dano baixo). Alternativa: calcular um hash de enchimento para o `id` desconhecido.

### PRB-019

- **Estado:** problema · **Tipo:** limite

(limite, `engine/messages`): Achar a mensagem do meio para rebaixar (`nth(len/2)`) e conferir as prontas já entregues quando o `maxDeliveries` muda varrem a memória do tópico (a conferência do `maxDeliveries` e as consultas sob demanda leem as mensagens em páginas; a mensagem mais antiga sai da ordem por idade). Efeito: O rebaixamento custa O(n) por mensagem num tópico grande, com a trava do tópico presa. Proposta: trocar por uma estrutura ordenada com posição quando medir. Sem pergunta de design.

### PRB-025

- **Estado:** problema · **Tipo:** limite

(limite, `engine/messages, http/metrics`): `diskBytes` (e `bytes`) soma o tamanho dos arquivos `.body` do tópico, uma chamada por mensagem em disco, a cada leitura de `/metrics/topics*` e, nos modos `by-tags` e `per-topic`, a cada raspagem de `/metrics`. `taba.storage.disk.usage` e `.duration` ficaram de fora pelo mesmo motivo. Cada leitura também mede todos os tópicos listados. Efeito: Com muitas mensagens em disco ou muitos tópicos, a leitura fica lenta e custa disco. Proposta: guardar o tamanho do corpo na linha do banco (coluna nova) e no índice, e medir só o que mudou.

### PRB-026

- **Estado:** problema · **Tipo:** incompleto

(incompleto, `telemetry/process`): `process.memory.usage`, `process.thread.count`, `process.open_file_descriptor.count` e `process.cpu.utilization` só existem no Linux (leem `/proc`); no Windows e no macOS só há `process.uptime`. Efeito: O painel do nó fica sem memória, threads, arquivos e CPU fora do Linux. Proposta: chamadas do sistema por plataforma (precisam de `unsafe` ou de uma dependência), ou aceitar que só o Linux tem essas métricas.

### PRB-039

- **Estado:** problema · **Tipo:** limite

(limite, `engine/messages/blocks`): A listagem de IDs de um bloco lê as linhas do banco por faixa de posição (índice) mas varre o mapa em memória do tópico inteiro a cada trecho, e ordena as linhas de todos os tópicos juntas. Efeito: O custo cresce com as mensagens em memória × blocos × origens; aceitável enquanto a memória é limitada por `memory.limit`. Proposta: medir; um índice por posição em memória resolveria.

### D117

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Fração padrão da amostragem de traces.** O [doc 10](22-configuracao.md) deixava a fração de `parentbased_traceidratio` "a definir". Implementado (provisório, etapa 12b): **0,1** (10 % dos traces sem pai); `OTEL_TRACES_SAMPLER_ARG` troca. Pergunta: aceita 0,1? Alternativa: 1,0 (tudo), como o padrão do SDK, deixando a redução para o Collector.

### D118

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Intervalo do resumo do overflow.** Implementado (provisório, etapa 12b): um log `info` (`taba.storage.overflow`) a cada 60 s, só se houve overflow, com a quantidade de mensagens da janela (UC-OPS-034). O intervalo é constante. Pergunta: aceita 60 s? Vira parâmetro?

### PRB-050

- **Estado:** problema · **Tipo:** limite

(limite, `telemetry/otlp`): a exportação OTLP lê só o essencial das variáveis `OTEL_*`: o prazo de cada envio é fixo (10 s), as variáveis por sinal (`OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` etc.) não valem para as métricas (nos traces e nos logs o SDK as lê), não há compressão, `OTEL_BSP_*` e `OTEL_BLRP_*` são as que o SDK lê do ambiente do processo, e o log em JSON no `stdout` não leva `trace_id` nem `span_id`. Efeito: um coletor que exige outro prazo, compressão ou endpoint por sinal só serve com o SDK. Proposta: ler `OTEL_EXPORTER_OTLP_TIMEOUT` e `OTEL_EXPORTER_OTLP_COMPRESSION` no `config/otlp` e pôr os ids do trace no JSON quando o span estiver amostrado.

### D95

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O IP do próprio nó.** O `join` leva os `addresses` do nó, e o nó precisa saber os seus. Implementado (provisório, etapa 16): `cluster.advertise-addresses` (lista de IPs) e, sem ele, o endereço local de um soquete UDP "conectado" ao `cluster.entry-address` (nada é enviado); se a descoberta falha, o nó não sobe e manda definir o parâmetro. Pergunta: aceita? Alternativa: exigir sempre o parâmetro.

### D119

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**O contexto do trace entre nós.** Implementado (provisório, etapa 16): o `Transport` HTTP leva o `traceparent` do span corrente e a porta do cluster abre um span filho quando a chamada o traz (só então: o sinal de vida e as tarefas de fundo não geram trace). A rede em memória dos testes não leva o cabeçalho. Pergunta: aceita? Falta decidir se o `tracestate` também viaja e se o span recebido deve existir sempre, mesmo sem `traceparent` (mais ruído, amostrado).

### D120

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Prazos e constantes do transporte HTTP.** Fixos no código (provisório, etapa 16): abrir a conexão 5 s, handshake TLS recebido 10 s, parada graciosa da porta do cluster 10 s, conexão ociosa guardada 60 s, aviso de expiração do certificado repetido de hora em hora, certificados intermediários seguidos até 4. A chamada inteira segue o prazo de quem chama (`cluster.channel.keepalive.interval × 2`, `write.commit.timeout`...). Pergunta: ficam fixos e documentados, ou viram parâmetros?

### PRB-051

- **Estado:** problema · **Tipo:** risco

(risco, `cluster/transport/http`, `cluster/service`): o cluster reconhece quem chama pelo IP de origem da conexão, e o `Transport` HTTP não fixa o IP de saída. Num nó com várias interfaces (ou NAT), o sistema pode escolher um IP diferente do informado em `cluster.advertise-addresses`, e o par responde `403` `cluster.not-member` a um membro. Efeito: nó que não se comunica com os pares, sem erro claro. Proposta: com `cluster.advertise-addresses` explícito e local, fixar o IP de saída (`HttpTransport::bound_to` já existe, só os testes o usam); ou identificar o membro pelo certificado/`nodeId` em vez do IP.

### D121

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**A espera pelo `take` do aviso (`cluster.notify.wait`).** Implementado (provisório, etapa 17a): o responsável pelos avisos do tópico acumula as prontas por `cluster.notify.window` (100 ms), manda **um** aviso com a quantidade a **um** nó com ouvinte (revezando), espera `cluster.notify.wait` que ele venha buscar e, se não veio (ou veio e ainda sobrou), passa ao próximo nó com ouvinte, até acabarem as mensagens ou os nós; o padrão do tempo de espera ficou em 50 ms. O nó avisado conta `fetched` ou `empty` conforme o `take` que o aviso provocou. Pergunta: 50 ms serve (a espera é a de um `take` na mesma rede)? Alternativa: um valor maior, ou proporcional ao RTT do canal com o nó.

### D103

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**Blocos íntegros e `under-replicated`.** O doc 11 fala em "réplicas íntegras" sem dizer o que é íntegra nem o esperado. Implementado (provisório): íntegra é o dono do bloco `active` na tabela com o canal de pé (o próprio nó conta); o esperado é o número de donos que o anel dá (`min(replication.factor, membros)`, então dois nós com fator 3 não ficam `under-replicated` para sempre); `blocksBelowReplication` inclui os blocos abaixo do quórum, e `under-replicated` aparece quando há mais blocos abaixo da replicação que abaixo do quórum; `syncingBlocks` conta os blocos com algum dono `syncing`. Pergunta: o esperado deve ser sempre `replication.factor`? Um nó em `draining` deve contar como íntegro?

### D112

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D107 (a saída (b), o secundário coordena).

**O repasse da publicação FIFO e `fifo.forward.max-attempts`.** O [doc 17](09-topicos.md) (seção 16.4) manda repassar pelo mecanismo da seção 6.9 e, no `421`, reler a tabela. Implementado (provisório): uma chamada própria entre nós (`POST /cluster/topics/{topic}/fifo/publish`), porque o distribuidor também publica em FIFO e não tem requisição de cliente a repassar; o token do cliente não é conferido de novo no primário (quem recebeu a chamada já o conferiu); no `421` o nó espera `write.retry.backoff` e relê **a própria** tabela, sem buscar a do primário; `fifo.forward.max-attempts` vale 3. Pergunta: aceita? Alternativa: o primário devolver a própria tabela no `421`. **Depende de:** D107 (a saída (b), o secundário coordena).

### D128

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design

**A rodada por grupo dentro da faixa.** A entrega justa escolhe a faixa do grupo pelo tamanho (potência de 2); no empate de faixa o código escolhe a mensagem mais antiga entre os grupos da faixa (e a mais nova na vez seguinte), então dois grupos pequenos de tamanho próximo se alternam por idade, e não por rodada (UC-GRP-007). O texto de [12-entrega.md](12-entrega.md) (seção 12.11) fala em rodada. Pergunta: a rodada por grupo deve valer dentro da faixa? A proposta é manter por idade, que é justa entre grupos de tamanho parecido, e dizer isso em 12.11.

## 2. Bloqueadas por outra decisão ou etapa

Respondem-se **depois** do que está em **Depende de** (ou junto).

### D97

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D104 (a mesma versão da tabela).

**O estado do membro na tabela e a época.** O `Member.state` (`syncing`, `active`...) é um campo da tabela de membros, mas a mudança de `syncing` para `active` não é uma mudança de membros em duas fases: o nó que ficou pronto anuncia o estado por sinal (`node-state` `active`, a todos, com tentativas, e repetido a cada `cluster.channel.keepalive.interval`), e cada nó troca o campo na própria tabela **sem subir a `version`**. Pergunta: a época deve subir, ou o estado deve sair da tabela e viver só no sinal de vida? Proposta: manter assim (a época continua contando só entradas e saídas, e `membership-divergent` não dispara por causa do estado); se o canal chegar, o sinal de vida passa a carregar o estado. **Depende de:** D104 (a mesma versão da tabela).

### D104

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D97 (se a época passar a contar o estado, a condição muda).

**`membership-divergent` só entre nós.** O sinal de vida leva a versão da tabela, mas chega com até `cluster.channel.keepalive.interval` de atraso, e derivar a condição dele a faria piscar a cada mudança de membros. Implementado: a condição só existe em `scope=cluster` (versões diferentes entre os nós que responderam); `scope=node` e `taba.cluster.condition` não a mostram. Pergunta: aceita, ou o nó deve compará-la com a versão que os pares dizem ter, aceitando o pisca? **Depende de:** D97 (se a época passar a contar o estado, a condição muda).

### D88

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D92 (o teto de grupos e a janela por grupo).

**Tetos de memória do índice.** Na fila justa a memória do índice já tem teto: a lista de prontas (`delivery.ready-list.size`), a de invisíveis (`delivery.invisible-list.size`, com a tabela `expiry`) e os contadores por grupo; as demais mensagens só são contadas e o motor lê os metadados quando precisa delas. Falta: (1) `delivery.fair.max-groups` precisa de um padrão (hoje sem teto; a sugestão do doc 10 é 100000); (2) a fila FIFO guarda uma entrada por mensagem, porque a lista de cada grupo é completa por desenho; (3) a tabela de réplicas do cluster guarda o ID e a idade de cada réplica. Pergunta: fixar o padrão de `max-groups` e aceitar os itens 2 e 3 como estão, ou limitá-los também (a fila FIFO com a lista de cada grupo no banco, as réplicas sem tabela em memória)? Problema ligado, **PRB-020** (definição, `engine/messages/index`): `delivery.fair.max-groups` não tem padrão (o doc 10 sugere 100000). Sem ele, um produtor que inventa um `groupId` por mensagem cria um grupo por mensagem. Efeito: Memória e tempo da escolha crescem com o número de grupos na faixa mais baixa. **Depende de:** D92 (o teto de grupos e a janela por grupo).

### D92

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D88 (`delivery.

**Taxas por grupo.** `GroupStats` promete `publishRate`, `consumeRate` e `ackRate` por grupo, e os grupos não têm teto. Pergunta: como medir sem uma janela por grupo? A proposta: manter a janela só para os `stats.topics.top-groups` maiores grupos de cada tópico (recalculados na leitura de `/groups?top=N`), e deixar as taxas ausentes nos demais; a alternativa é uma janela compacta por grupo (contagem por 10 s, 30 baldes) com o teto de `delivery.fair.max-groups`. Problema ligado, **PRB-024** (incompleto, `http/topic_stats`): `GroupStats` não traz `publishRate`, `consumeRate` e `ackRate`: uma janela de taxa por grupo custaria memória proporcional ao número de grupos, que não tem teto. Efeito: O produtor barulhento só se acha pelo tamanho do grupo, não pela vazão dele. **Depende de:** D88 (`delivery.fair.max-groups`). A etapa 12b não a implementou: espera esta decisão.

### D85

- **Estado:** `[ ]` aberta · **Tipo:** dúvida de design · **Depende de:** D84 (o estouro só existe com `rps`).

**Estouro (`ratelimit.burst`) sem valor.** Hoje vale a taxa arredondada para cima, no mínimo 1 (um segundo de folga). Proposta: manter e escrever isso em [19-erros-e-limites.md](19-erros-e-limites.md); a alternativa é um `ratelimit.burst` obrigatório quando houver `rps`. **Depende de:** D84 (o estouro só existe com `rps`).

## Fontes e provas

- Cada item cita a evidência: o módulo do código, o teste ou a execução descrita em [23-operacao.md](23-operacao.md) e [13-observabilidade.md](13-observabilidade.md).
- O roadmap que fecha estas pendências está em [32-roadmap.md](32-roadmap.md).
