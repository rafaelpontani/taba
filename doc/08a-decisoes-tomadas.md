# Decisões tomadas

[← Índice](README.md)

## 10.1 Decisões já tomadas

Parte da seção 10, junto de [08b-pendencias.md](08b-pendencias.md) e [08c-proximos-passos.md](08c-proximos-passos.md). Cada linha traz o código da dúvida (para referência), a decisão e onde ela está registrada. Os códigos não são reaproveitados. Quando uma pendência é decidida, ela é registrada no documento técnico correspondente (e o valor padrão em [10-configuracao.md](10-configuracao.md)), sai de [08b-pendencias.md](08b-pendencias.md) e entra aqui.


### Cluster, membros e canal

| Código | Decisão | Onde |
|---|---|---|
| D8 | O nó novo recebe por configuração obrigatória o endereço do balanceador do cluster, lista os membros com os IPs internos e pede o ingresso a um deles. | [04-cluster.md](04-cluster.md) 6.2, [10-configuracao.md](10-configuracao.md) |
| D29, D29.1, D29.2, D29.5, D29.6 | Canal WebSocket permanente entre cada par de nós, que também controla a vida (sem ping). Quem entra ou volta abre a conexão; a tabela de membros não é persistida; fila do canal que estoura exclui o nó; registro e desregistro de nó não correm juntos. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.8 e 12.9, [04-cluster.md](04-cluster.md) 6.3 e 6.8 |
| D29.3, D29.4 | O sinal de vida é uma mensagem própria, com data e hora, monitoramento e mensagens prontas por tópico (orienta o sorteio do `take`). Todo outro sinal leva um identificador e é confirmado depois de processado, sem repetição. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.8 e 12.9, [04-cluster.md](04-cluster.md) 6.3 |
| D30 | Os avisos de mensagem nova saem de um responsável por tópico no nó dono, agrupados numa janela de 100 ms configurável, a um nó com ouvinte por vez, em ciclos. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.8 |
| D32 | Limite e juntada dos sinais na fila de saída do canal. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.9 |
| D17 | A votação de exclusão exige 51% dos nós saudáveis, configurável. Os nós fora do ar não votam nem entram na base. Há um mínimo de votantes (padrão 2). | [04-cluster.md](04-cluster.md) 6.3, [10-configuracao.md](10-configuracao.md) |
| D49 | `mode=development` (padrão `production`) faz o nó rodar sozinho: sem bootstrap, canal entre nós, votação, quórum, freezing, replicação e sincronização; `replication.factor` e `write.quorum` valem 1, `cluster.entry-address` não é exigido e `/cluster/*` fica desabilitado. O comportamento de cliente é o mesmo. | [04-cluster.md](04-cluster.md) 6.4, [10-configuracao.md](10-configuracao.md) |
| D53 | A mensagem pode levar um `groupId`; cada dono conta, por grupo e sem troca entre nós, as mensagens prontas e as em processamento; o tamanho do grupo é `em processamento + 0,1 × prontas` (peso configurável, `0` como no SQS e `1` o backlog inteiro), e a lista de prontos é reposta e ordenada por faixas de potência de 2, os menores primeiro. Uma entrega em cada 10 segue a ordem normal. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.11, [10-configuracao.md](10-configuracao.md) |
| D51 | No modo desenvolvimento o drain é um save: grava mensagens, metadados e o estado das filas voláteis, e a partida seguinte retoma do disco (`development.resume-on-start`, padrão `true`). Sem save concluído, retoma o que há em disco e avisa. No cluster nada muda: toda partida é de nó novo. | [04-cluster.md](04-cluster.md) 6.10, [10-configuracao.md](10-configuracao.md) |
| D50, D52 | O modo drain (`active`, `draining`, `drained`) grava todas as mensagens em disco, tira o nó da rotação (`/health/ready` `503`), avisa os pares e repassa todas as chamadas de cliente ao nó mais livre (menor carga pelo sinal de vida), como no repasse da sincronização. Entra sozinho ao receber `SIGTERM`/`SIGINT` (`drain.on-stop`), e então encerra, ou por `POST /node/drain`, sem encerrar. Termina com o **desregistro voluntário** do nó, sem votação nem freezing. `DELETE /node/drain` cancela antes da saída; depois dela, o nó volta como nó novo. Limitado por `drain.timeout`, menor que o prazo do orquestrador. | [04-cluster.md](04-cluster.md) 6.10, [06-api.md](06-api.md) 8.1.1, [10-configuracao.md](10-configuracao.md) |
| D18 | O pedido de ingresso de um nó em freezing é rejeitado com `Retry-After`, sem lista de espera. O nó se identifica por um ID GUID v7 que ele gera a cada início, em memória: o freezing vale para o mesmo processo que fica caindo e voltando, e um reinício entra como nó novo. | [04-cluster.md](04-cluster.md) 6.7 |
| D19 | O bootstrap é repetido a cada `cluster.bootstrap.retry.interval` até `cluster.bootstrap.retry.max-attempts` (sem limite por padrão). | [04-cluster.md](04-cluster.md) 6.5, [10-configuracao.md](10-configuracao.md) |
| D41, D47 | Um nó que enche ou perde o disco entra em falha e libera o cluster para votar o desregistro dele. Um nó que entrou e ainda não está íntegro não entrega as mensagens de que é primário, e as chamadas de cliente que chegam a ele são repassadas a outro nó. Só há uma mudança de membros por vez, até o nó ficar pronto. | [04-cluster.md](04-cluster.md) 6.8 e 6.9, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.7, [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.3 |
| D42 | A origem da carga de tópicos é o nó de menor carga pelo sinal de vida; empatados ou sem dado, um nó sorteado. | [02-arquitetura.md](02-arquitetura.md) 3.3 |

### Escrita, entrega e invisibilidade

| Código | Decisão | Onde |
|---|---|---|
| — | O rollback do registro/commit só vale antes de o quórum decidir; depois de decidido, o coordenador insiste nas réplicas restantes até `write.commit.timeout`. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.2 |
| — | Mensagens (escrita, entrega e ack) decidem o commit por quórum mínimo, sempre com o primário. Registro e desregistro de nó e definição de tópico exigem todos os nós. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.2, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.2 |
| D33 | `write.quorum` é um valor único para escrita, entrega e ack, padrão a maioria de `replication.factor`, validado entre 1 e o fator. Sem quórum disponível, `503`. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.2, [10-configuracao.md](10-configuracao.md) |
| D1, D3 | Semântica at-least-once e modelo de consumo por polling e long polling de até 20 s. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) |
| D9 | Fluxo de leitura: o coordenador reúne as mensagens da própria lista e completa com outros nós. Só o primário entrega e marca a invisibilidade; os demais nós são cópias de segurança. A lista de prontos fica em memória, 1000 IDs por tópico por padrão, reposta por uma thread parada num semáforo. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.6 a 12.8 |
| D2 | A lista de prontos é consumida pelas duas pontas do GUID v7, intercaladas (efeito sanfona), sem garantia de ordem ao cliente. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.6 e 12.7 |
| D25 | O `PATCH` define o novo tempo a partir de agora, e `0` libera a mensagem na hora. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.3 |
| D27 | O `ReceiptToken`, gerado a cada entrega e guardado junto da invisibilidade, é exigido no ack e no `PATCH`; token antigo recebe `409`. | [14-invisibilidade.md](14-invisibilidade.md) 13.2 |
| D28 | Não há disputa pelo `GET`, pois só o primário entrega os IDs prontos. Falha do registro passa ao próximo ID e, esgotadas as tentativas, `503`. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.2 |
| D31 | `maxMessages` com padrão 10, configurável; os streams de sincronização entre nós têm tetos próprios. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.2 |
| D5, D26 | Propagação e remoção de tópicos (inclusive mensagens pendentes); ack por registro + commit, sem tombstone, e a verificação final da sincronização cobre a janela residual. | [02-arquitetura.md](02-arquitetura.md) 3.3, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.4 |

### Armazenamento e memória

| Código | Decisão | Onde |
|---|---|---|
| D11 | A memória se divide entre tópicos pela taxa de leitura numa janela de 5 minutos (configurável), com reserva mínima por tópico. Ficam em memória a lista de prontos e as recém-chegadas, sem LRU/LFU. | [02-arquitetura.md](02-arquitetura.md) 3.2 |
| D12 | Raiz de disco `storage.dir`, por nó, padrão `/var/lib/taba`, criada ao iniciar. O ack apaga o arquivo junto. Um nó que perde um bloco no rebalanceamento só o apaga quando o cluster inteiro está íntegro. | [10-configuracao.md](10-configuracao.md), [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.3 |
| D34, D35 | Um banco SQLite por nó e por tópico, com escritor único por tópico: guarda os metadados e a tabela de expiração; o corpo continua em `${topico}/${id}.json`. A mensagem mais antiga sai de `MIN(id)` somada à mais antiga em memória. Banco corrompido põe o nó em falha. | [02-arquitetura.md](02-arquitetura.md) 3.2, [14-invisibilidade.md](14-invisibilidade.md) 13.2 e 13.5 |
| D40 | Tamanho máximo da mensagem configurável: padrão geral de 5 MB, por tópico podendo ser maior, e um limite rígido para todos. | [10-configuracao.md](10-configuracao.md), [12-limites-api.md](12-limites-api.md) |

### Sincronização e recuperação

| Código | Decisão | Onde |
|---|---|---|
| D39, D43 | O nó em sincronização aplica a carga direto no armazenamento e guarda os eventos que chegam num banco SQLite, processados linha a linha por cima da carga até esvaziar; então passa ao tempo real. Só fica pronto ao terminar. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.3, [02-arquitetura.md](02-arquitetura.md) 3.3 |
| D15 | O evento de perda vai para o log e para a tabela `perdas`, com tópico e ID. Antes de declarar perda, tenta outro nó, a rota de ID único e uma relistagem; um ack concorrente é descartado em silêncio. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.3, [06-api.md](06-api.md), [11-observabilidade.md](11-observabilidade.md) |
| D16 | Sem otimização da listagem de IDs: o stream NDJSON ordenado basta. Reavaliar se a medição mostrar problema. | [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.3 |
| D45 | O `recover` grava direto nos nós responsáveis, idempotente pelo ID, sem registro + commit, confirmando a linha quando `write.quorum` nós gravaram. | [04-cluster.md](04-cluster.md) 6.6, [06-api.md](06-api.md) |
| D48 | O nó que volta consulta a tabela `perdas` de todos os nós, confronta com o que tem e envia o que achar ao responsável. Os dados antigos ficam em quarentena até a recuperação terminar. | [04-cluster.md](04-cluster.md) 6.6, [02-arquitetura.md](02-arquitetura.md) 3.2 |

### Filas FIFO

| Código | Decisão | Onde |
|---|---|---|
| D54 | A fila FIFO é o atributo `fifo: true` de uma fila, imutável, combinável com volátil e com dead-letter. Alternativas descartadas: a fila inteira num bloco só e o sequenciador por grupo com dados espalhados por ID. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.2 e 16.13 |
| D55 | A posição no anel é o hash do `id` do tópico mais o `groupId`: o grupo fica atrelado a um primário, que ordena. O nó que recebe a publicação a repassa ao primário, que gera o ID monotônico (`max(agora, último + 1)`). | [17-topicos-fifo.md](17-topicos-fifo.md) 16.3 e 16.4, [05-particionamento-replicacao.md](05-particionamento-replicacao.md) 7.1 |
| D56 | O quórum da FIFO sempre inclui o primário e o secundário (herdeiro do bloco), e `replication.factor` mínimo é 2. Evita a reentrega fora de ordem depois do failover. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.5 |
| D57 | `groupId` é obrigatório na FIFO (`400` sem ele); a cópia de um distribuidor sem `groupId` para uma fila FIFO falha só naquela fila. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.2 e 16.8, [15-topicos-distribuidor.md](15-topicos-distribuidor.md) 14.2 |
| D58 | Um grupo, uma mensagem em voo: a lista de prontos tem uma entrada por grupo livre, a cabeça mais antiga vence (sem entrega justa por faixas) e o lote do `GET` traz grupos diferentes. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.6, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.6 e 12.11 |
| D59 | Na FIFO, a mensagem que atinge `maxDeliveries` vai ao dead-letter e o grupo segue; a vencida por `ttl` na cabeça também libera o grupo. A dead-letter de uma FIFO não é FIFO e usa o `id` da fila de origem para a posição. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.6 e 16.8, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.10 |
| D60 | Deduplicação por grupo: `deduplicationId` é metadado da mensagem (sem ele, SHA-256 do corpo), janela de 5 minutos. O ack não apaga a linha: grava `deleteAt` e apaga o corpo, e a linha, replicada em todos os nós, sai por varredura. O novo primário herda a deduplicação, e a sincronização leva as linhas sem rota nova. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.7, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.4, [10-configuracao.md](10-configuracao.md) |
| D61 | Publicação FIFO durante a sincronização do novo primário ou com o primário fora do ar: `503`. O primário recalcula o dono do grupo e responde `421` se não for ele, e o repasse tenta de novo até `fifo.forward.max-attempts`. A repetição de uma publicação FIFO é segura. | [17-topicos-fifo.md](17-topicos-fifo.md) 16.4, [04-cluster.md](04-cluster.md) 6.9 |

### API, tópicos e limites

| Código | Decisão | Onde |
|---|---|---|
| D4, D24 | O modelo de assinantes deu lugar ao Tópico Distribuidor, que copia a publicação para filas por prefixo; há filas voláteis e um nó gestor. | [15-topicos-distribuidor.md](15-topicos-distribuidor.md) |
| D36, D37 | Tetos de destinos por publicação num distribuidor e de filas voláteis, configuráveis por variável de ambiente, sem limite por padrão. | [15-topicos-distribuidor.md](15-topicos-distribuidor.md), [10-configuracao.md](10-configuracao.md) |
| D13, D13.1, D13.2, D13.3 | Toda fila tem `ttl` (padrão 14 dias) e `maxDeliveries` (padrão 10), configuráveis por tópico e, nos padrões, no cluster. A mensagem vencida é descartada; a que atinge o limite de entregas sem ack vai para a fila `{fila}-dead-letter` (sufixo configurável), que nasce e morre com a fila, não tem definição própria (configuração só leitura, derivada da fila) e é, no mais, um tópico comum. | [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) 12.10, [10-configuracao.md](10-configuracao.md) |
| D7 | Paginação por `limit`, `cursor` e `sort` com chave estável nas rotas de cliente; streams NDJSON com linha final e retomada por ID nas rotas entre nós, incluindo `GET /cluster/topics`. | [06-api.md](06-api.md) 8.1.2 e 8.2.1 |
| D38 | `GET /stats/topics` sem `sort`, sempre por nome; `GET /topics` ganha o filtro `name`. | [06-api.md](06-api.md) 8.1.2, [11-observabilidade.md](11-observabilidade.md) |
| D6 | Rate limit por IP do cliente, contado localmente em cada nó; o IP vem de cabeçalhos do balanceador, configurável e com chave para desligar. | [12-limites-api.md](12-limites-api.md), [10-configuracao.md](10-configuracao.md) |
| D22 | `/health/*`, `/metrics` e `/stats/node` isentos de rate limit; `/stats/cluster` e `/stats/topics` em categoria própria (`ratelimit.ops.rps`). | [12-limites-api.md](12-limites-api.md), [10-configuracao.md](10-configuracao.md) |
| D23 | O IP do cliente vem do cabeçalho só de `client-ip.trusted-proxies`; vazia, aceita de qualquer origem, e o serviço deve ficar em camada de rede segura. Vale para o rate limit e para `operations.allowed-ips`. | [11-observabilidade.md](11-observabilidade.md), [12-limites-api.md](12-limites-api.md) |

### Observabilidade

| Código | Decisão | Onde |
|---|---|---|
| D20 | Os nomes das métricas seguem a proposta do doc 11 e são conferidos com as convenções do OpenTelemetry na implementação. | [11-observabilidade.md](11-observabilidade.md) |
| D21 | O Prometheus tem um único alvo, o balanceador do cluster. Cada nó mantém em memória as métricas de todos, atualizadas por uma thread, e o `/metrics` só as lê, com uma série por nó. `scope=node` para métricas individuais. | [11-observabilidade.md](11-observabilidade.md), [06-api.md](06-api.md) |
| D46 | Métricas dos streams de sincronização: `msg.sync.streams`, `stream.resumes`, `stream.duration`, `stream.bytes` e `msg.recover.messages`. | [11-observabilidade.md](11-observabilidade.md) |

