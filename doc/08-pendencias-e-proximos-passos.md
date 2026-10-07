# Pendências e próximos passos

[← Índice](README.md)

## 10. Pendências e próximos passos

O objetivo deste documento é **zerar as dúvidas e avançar**: cada próximo passo lista as decisões que precisam estar fechadas antes dele. Uma pendência aparece uma vez, no primeiro passo que depende dela. Ao ser decidida, é registrada no documento técnico correspondente (e o valor padrão em [10-configuracao.md](10-configuracao.md)) e removida daqui.

Legenda:

- **D1, D2…** — número da dúvida, para responder por número (ex.: "D4.1: o assinante é identificado por nome"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D4.1**, **D4.2**…), que podem ser respondidos separadamente. Nem os números nem os sub-códigos são reaproveitados: ao decidir, o item sai da lista e os demais mantêm o código.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto diz **o que já está definido**, e cada pergunta que falta tem seu sub-código.

**Já decididas** (registradas nos documentos técnicos): D22 (`/health/*`, `/metrics` e `/stats/node` isentos de rate limit, para o balanceador não tirar o nó da rotação por engano; `/stats/cluster` e `/stats/topics` em categoria própria, `ratelimit.ops.rps`), em [12-limites-api.md](12-limites-api.md) e [10-configuracao.md](10-configuracao.md); D20 (os nomes das métricas seguem a proposta do doc 11 e são conferidos com as convenções do OpenTelemetry na implementação, não é dúvida de design), D23 (o IP do cliente vem do cabeçalho só de `client-ip.trusted-proxies`; vazia, aceita de qualquer origem, e o serviço deve ficar em camada de rede segura; vale para o rate limit e para `operations.allowed-ips`) e D46 (métricas `msg.sync.streams`, `stream.resumes`, `stream.duration`, `stream.bytes` e `msg.recover.messages`), em [11-observabilidade.md](11-observabilidade.md), [12-limites-api.md](12-limites-api.md) e [10-configuracao.md](10-configuracao.md); D21 (o Prometheus tem um único alvo, o balanceador do cluster: o `/metrics` consulta todos os nós e devolve uma série por nó, com `scope=node` para métricas individuais, por exemplo via sidecar, como escolha de implementação), em [11-observabilidade.md](11-observabilidade.md) e [06-api.md](06-api.md); D45 (o `recover` grava direto nos nós responsáveis pelo bloco, idempotente pelo ID e sem registro + commit, confirmando a linha quando `write.quorum` nós gravaram), em [04-cluster.md](04-cluster.md) seção 6.6 e [06-api.md](06-api.md); D48 (o nó que volta de uma exclusão consulta a tabela `perdas` de todos os nós, confronta com o que tem em memória e disco e envia o que achar ao responsável pelo bloco; os dados antigos ficam em quarentena até a recuperação terminar), em [04-cluster.md](04-cluster.md) seção 6.6 e [02-arquitetura.md](02-arquitetura.md) seção 3.2; D18 (o pedido de ingresso de um nó em freezing é rejeitado com `Retry-After`, sem lista de espera; o nó se identifica por um ID GUID v7 que ele mesmo gera a cada início, guardado em memória; o freezing vale para o mesmo processo que fica caindo e voltando, e um reinício entra como nó novo), em [04-cluster.md](04-cluster.md) seção 6.7; D19 (o bootstrap é repetido a cada `cluster.bootstrap.retry.interval`, configurável, até `cluster.bootstrap.retry.max-attempts`, sem limite por padrão), em [04-cluster.md](04-cluster.md) seção 6.5 e [10-configuracao.md](10-configuracao.md); D16 (sem otimização da listagem de IDs: o stream NDJSON paginado e ordenado basta, e o custo restante é de tempo e banda; reavaliar se a medição mostrar problema), em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3; D15 (o evento de perda é registrado no log da aplicação e na tabela `perdas`, com tópico e ID da mensagem, vindo na linha do stream de IDs; antes de declarar perda, tenta outro nó, a rota de ID único e uma relistagem, e um ack concorrente é descartado em silêncio), em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3, [06-api.md](06-api.md) e [11-observabilidade.md](11-observabilidade.md); D33 (`write.quorum` é um valor único para escrita, entrega e ack, padrão a maioria de `replication.factor`, validado entre 1 e o fator; sem quórum disponível, `503`), em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.2 e [10-configuracao.md](10-configuracao.md); D17 (a votação de exclusão exige 51% dos nós saudáveis, configurável; os nós fora do ar não votam nem entram na base; há um mínimo de votantes, padrão 2), em [04-cluster.md](04-cluster.md) seção 6.3 e [10-configuracao.md](10-configuracao.md); D34 e D35 (a persistência local em disco usa um banco SQLite por nó e por tópico, com um escritor único por tópico: guarda os metadados e a tabela de expiração, e o corpo continua em `${topico}/${id}.json`; não há mais `.meta.json`; a mensagem mais antiga sai de `MIN(id)` somada à mais antiga em memória; banco corrompido põe o nó em falha), em [02-arquitetura.md](02-arquitetura.md) seção 3.2 e [14-invisibilidade.md](14-invisibilidade.md) seções 13.2 e 13.5; D12 (raiz de disco `storage.dir`, por nó, padrão `/var/lib/taba` e criada ao iniciar; o ack apaga o arquivo junto; um nó que perde um bloco no rebalanceamento só o apaga quando o cluster inteiro está íntegro, para poder voltar a ser o dono se o nó novo falhar), em [10-configuracao.md](10-configuracao.md) e [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3; D11 (a memória se divide entre tópicos pela taxa de leitura numa janela de 5 minutos configurável, com reserva mínima por tópico; ficam em memória a lista de prontos e as recém-chegadas, sem LRU/LFU), em [02-arquitetura.md](02-arquitetura.md) seção 3.2 e [10-configuracao.md](10-configuracao.md); D42 (a origem da carga de tópicos é o nó de menor carga pelo sinal de vida, ou um nó sorteado se empatar ou faltar dado), em [02-arquitetura.md](02-arquitetura.md) seção 3.3; D30 (os avisos de mensagem nova saem de um responsável por tópico no nó dono, agrupados numa janela de 100 ms configurável, a um nó com ouvinte por vez, em ciclos), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.8 e [10-configuracao.md](10-configuracao.md); D29.3 e D29.4 (o sinal de vida é uma mensagem própria com data e hora, monitoramento e mensagens prontas por tópico, que orienta o sorteio do `take`; todo outro sinal leva um identificador e é confirmado depois de processado, sem repetição), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seções 12.8 e 12.9 e [04-cluster.md](04-cluster.md) seção 6.3; D9 (fluxo de leitura: o coordenador reúne as mensagens da própria lista e completa com outros nós; só o primário entrega e marca a invisibilidade, os demais nós são cópias de segurança; a lista de prontos fica em memória, 1000 IDs por tópico por padrão, reposta por uma thread parada num semáforo), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seções 12.6 a 12.8 e [10-configuracao.md](10-configuracao.md); D8 (o nó novo recebe o endereço do balanceador do cluster por configuração obrigatória, lista os membros com os IPs internos e pede o ingresso a um deles), em [04-cluster.md](04-cluster.md) seção 6.2 e [10-configuracao.md](10-configuracao.md); D6 (rate limit por IP do cliente, contado localmente em cada nó; IP vindo de cabeçalhos do balanceador, configurável e com chave para desligar), em [12-limites-api.md](12-limites-api.md) e [10-configuracao.md](10-configuracao.md); D41 e D47 (um nó que enche ou perde o disco entra em falha e libera o cluster para votar o desregistro dele; um nó que entrou e ainda não está íntegro não entrega as mensagens de que é primário, e as chamadas de cliente que chegam a ele são repassadas a outro nó, seção 6.9 de [04-cluster.md](04-cluster.md)), uma mudança de membros por vez até o nó ficar pronto, em [04-cluster.md](04-cluster.md) seção 6.8, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.7 e [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3; D39 e D43 (o nó em sincronização aplica a carga direto no armazenamento e guarda os eventos que chegam em um banco SQLite, processados linha a linha por cima da carga até esvaziar, quando passa ao tempo real, e só fica pronto ao terminar; com isso o commit de tópico não precisa levar a definição completa e uma mensagem de tópico ainda desconhecido espera no log), em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3 e [02-arquitetura.md](02-arquitetura.md) seção 3.3; D40 (tamanho máximo da mensagem configurável, geral e por tópico; padrão geral de 5 MB, o tópico sem valor usa o geral, o tópico pode ser maior que o geral e há um limite rígido para todos), em [10-configuracao.md](10-configuracao.md) e [12-limites-api.md](12-limites-api.md); D38 (`GET /stats/topics` sem `sort`, sempre por nome; `GET /topics` ganha o filtro `name` por trecho do nome), em [06-api.md](06-api.md) seção 8.1.2 e [11-observabilidade.md](11-observabilidade.md); D27 (o `ReceiptToken`, gerado a cada entrega, guardado e replicado junto da invisibilidade e exigido no ack e no `PATCH`; token antigo recebe `409`) e D28 (sem disputa pelo `GET`, pois só o primário entrega os IDs prontos; falha do registro passa ao próximo ID e, esgotadas as tentativas, `503`), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) e [14-invisibilidade.md](14-invisibilidade.md); D2 (a lista de prontos é consumida pelas duas pontas do GUID v7, intercaladas — efeito sanfona —, sem garantia de ordem ao cliente), D25 (o `PATCH` define o novo tempo a partir de agora, e `0` libera na hora), D31 (`maxMessages`, padrão 10 e configurável; os streams de sincronização entre nós têm tetos próprios) e D36–D37 (tetos configuráveis por variável de ambiente, sem limite por padrão), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seções 12.2, 12.3, 12.6 e 12.7, [15-topicos-distribuidor.md](15-topicos-distribuidor.md) e [10-configuracao.md](10-configuracao.md); D7 (paginação por `limit`/`cursor`/`sort` com chave estável nas rotas de cliente, e streams NDJSON com linha final e retomada por ID nas rotas de sincronização entre nós; inclui a nova rota `GET /cluster/topics` para a carga da tabela de tópicos no bootstrap), em [06-api.md](06-api.md) seções 8.1.2 e 8.2.1; D4 e D24 (o modelo de assinantes deu lugar ao Tópico Distribuidor, que copia a publicação para filas por prefixo; não há assinante identificado; filas voláteis e nó gestor), em [15-topicos-distribuidor.md](15-topicos-distribuidor.md); D1 (semântica de entrega), D3 (modelo de consumo: polling e long polling de até 20 s) e D32 (limite e juntada das filas do canal), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md); o controle de vida entre nós pelo canal permanente, sem ping (seção 6.3); do canal entre nós (D29), D29.1, D29.2, D29.5 e D29.6 (concorrência entre registro e desregistro, seção 6.8); D5 (propagação e remoção de tópicos, incluindo mensagens pendentes) e D26 (ack por registro + commit, sem tombstone — a verificação final da sincronização de bloco cobre a janela residual), em [02-arquitetura.md](02-arquitetura.md) seção 3.3, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.4 e [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3; rollback do protocolo de registro/commit limitado a antes do quórum decidir, com teto configurável (`write.commit.timeout`) para as réplicas fora do quórum depois de decidido, em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.2; **estratégia de quórum mínimo para mensagens** (`write.quorum`, sempre incluindo o primário do bloco, em vez de esperar confirmação de todas as `replication.factor` réplicas) para escrita, entrega/invisibilidade e ack — registro/desregistro de nó e definição de tópico continuam exigindo confirmação de todos os nós, sem quórum reduzido — em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.2 e [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.2.

### Resumo das dúvidas por passo

| Passo | Dúvidas |
|---|---|
| 3 — Protótipo | D10, D13: linguagem, retenção/TTL/dead-letter |

Os passos 1, 2 e 4 a 10 não têm dúvidas próprias.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design, e por isso não aparecem aqui: podem ser fechados com um chute razoável e ajustados em teste.

### Passo 1 — Contrato da API (OpenAPI)

Cobre os endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8).


### Passo 2 — Arquitetura do cluster e fluxo em camadas

O fluxo de escrita está definido (seção 3.1); sem dúvidas abertas neste passo.


### Passo 3 — Protótipo de nó único (memória + disco)

Inclui o overflow por tópico (seção 3.2).

- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.
- **D13.** [~] **Retenção, expiração (TTL) e dead-letter.** Já definido: cada mensagem tem `deliveryCount` nos metadados, base para um limite de entregas. Reprocessamento histórico está fora do escopo (seção 5).
  - **D13.1** TTL da mensagem.
  - **D13.2** Limite de entregas antes do dead-letter.
  - **D13.3** Para onde a mensagem vai no dead-letter (tópico próprio?).

### Passo 4 — Protocolo genérico de registro/commit

Reutilizado em nós, mensagens e recuperação. Decidido nesta revisão: rollback só é seguro **antes** do quórum decidir (registro pendente); depois de decidido, o coordenador nunca desfaz — insiste na(s) réplica(s) restante(s) dentro de um teto configurável. Mensagens (escrita, entrega/invisibilidade, ack) decidem o commit por **quórum mínimo**, sempre incluindo o primário do bloco, em vez de esperar todas as réplicas; registro/desregistro de nó e definição de tópico continuam exigindo todos os nós, por serem operações raras sem caminho crítico de latência ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).


### Passo 5 — Anel de hash circular


### Passo 6 — Sincronização distribuída de blocos

Na entrada de novos nós, incluindo o evento de falha de perda de mensagem.



### Passo 7 — Rebalanceamento de blocos

Em caso de falha de registro ou desregistro de nó. Sem pendências.

### Passo 8 — Sinal de vida pelo canal, votação com quórum e freezing anti-flapping


### Passo 9 — Retorno de nó

Recuperação de mensagens perdidas + novo bootstrap.


### Passo 10 — Empacotamento em Docker e validação em Kubernetes

Observabilidade e rotas de operação, a fechar até aqui.


## Referências internas

- Mapa mental do projeto (`mapa-mental.html`, neste projeto) — visão gráfica do escopo e das decisões.
