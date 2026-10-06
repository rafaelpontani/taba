# Pendências e próximos passos

[← Índice](README.md)

## 10. Pendências e próximos passos

O objetivo deste documento é **zerar as dúvidas e avançar**: cada próximo passo lista as decisões que precisam estar fechadas antes dele. Uma pendência aparece uma vez, no primeiro passo que depende dela. Ao ser decidida, é registrada no documento técnico correspondente (e o valor padrão em [10-configuracao.md](10-configuracao.md)) e removida daqui.

Legenda:

- **D1, D2…** — número da dúvida, para responder por número (ex.: "D4.1: o assinante é identificado por nome"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D4.1**, **D4.2**…), que podem ser respondidos separadamente. Nem os números nem os sub-códigos são reaproveitados: ao decidir, o item sai da lista e os demais mantêm o código.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto diz **o que já está definido**, e cada pergunta que falta tem seu sub-código.

**Já decididas** (registradas nos documentos técnicos): D27 (o `ReceiptToken`, gerado a cada entrega, guardado e replicado junto da invisibilidade e exigido no ack e no `PATCH`; token antigo recebe `409`) e D28 (sem disputa pelo `GET`, pois só o primário entrega os IDs prontos; falha do registro passa ao próximo ID e, esgotadas as tentativas, `503`), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) e [14-invisibilidade.md](14-invisibilidade.md); D2 (a lista de prontos é consumida pelas duas pontas do GUID v7, intercaladas — efeito sanfona —, sem garantia de ordem ao cliente), D25 (o `PATCH` define o novo tempo a partir de agora, e `0` libera na hora), D31 (`maxMessages`, padrão 10 e configurável; os streams de sincronização entre nós têm tetos próprios) e D36–D37 (tetos configuráveis por variável de ambiente, sem limite por padrão), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seções 12.2, 12.3, 12.6 e 12.7, [15-topicos-distribuidor.md](15-topicos-distribuidor.md) e [10-configuracao.md](10-configuracao.md); D7 (paginação por `limit`/`cursor`/`sort` com chave estável nas rotas de cliente, e streams NDJSON com linha final e retomada por ID nas rotas de sincronização entre nós; inclui a nova rota `GET /cluster/topics` para a carga da tabela de tópicos no bootstrap), em [06-api.md](06-api.md) seções 8.1.2 e 8.2.1; D4 e D24 (o modelo de assinantes deu lugar ao Tópico Distribuidor, que copia a publicação para filas por prefixo; não há assinante identificado; filas voláteis e nó gestor), em [15-topicos-distribuidor.md](15-topicos-distribuidor.md); D1 (semântica de entrega), D3 (modelo de consumo: polling e long polling de até 20 s) e D32 (limite e juntada das filas do canal), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md); o controle de vida entre nós pelo canal permanente, sem ping (seção 6.3); do canal entre nós (D29), D29.1, D29.2, D29.5 e D29.6 (concorrência entre registro e desregistro, seção 6.8); D5 (propagação e remoção de tópicos, incluindo mensagens pendentes) e D26 (ack por registro + commit, sem tombstone — a verificação final da sincronização de bloco cobre a janela residual), em [02-arquitetura.md](02-arquitetura.md) seção 3.3, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.4 e [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3; rollback do protocolo de registro/commit limitado a antes do quórum decidir, com teto configurável (`write.commit.timeout`) para as réplicas fora do quórum depois de decidido, em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.2; **estratégia de quórum mínimo para mensagens** (`write.quorum`, sempre incluindo o primário do bloco, em vez de esperar confirmação de todas as `replication.factor` réplicas) para escrita, entrega/invisibilidade e ack — registro/desregistro de nó e definição de tópico continuam exigindo confirmação de todos os nós, sem quórum reduzido — em [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.2 e [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.2.

### Resumo das dúvidas por passo

| Passo | Dúvidas |
|---|---|
| 1 — Contrato da API | D6, D38–D40: rate limits, `sort` por valor mutável, commit de tópico com a definição completa, tamanho máximo da mensagem |
| 2 — Arquitetura | D8, D9, D29, D30, D41–D42: como o nó novo acha o primeiro nó, fluxo de leitura, detalhes do canal WebSocket entre nós, avisos de mensagem nova, falha da carga de tópicos e escolha do nó de origem |
| 3 — Protótipo | D10–D13, D34–D35: linguagem, priorização de memória, disco, retenção/TTL/dead-letter |
| 4 — Protocolo de registro/commit | D33: quórum de escrita fixo ou proporcional |
| 6 — Sincronização | D15–D16, D43–D44: evento de perda, otimização de listagem, mensagem de tópico ainda desconhecido, divisão em faixas |
| 8 — Sinal de vida, quórum e freezing | D17–D19: quórum, freezing, retry do bootstrap |
| 9 — Retorno de nó | D45: o `recover` em lote contra o registro/commit |
| 10 — Docker e Kubernetes | D20–D23, D46: métricas, Prometheus, rate limit das rotas de operação, `X-Forwarded-For`, métricas dos streams de sincronização |

Os passos 5 e 7 não têm dúvidas próprias.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design: cada passo os cita numa linha "Valores iniciais" e eles podem ser fechados com um chute razoável e ajustados em teste.

### Passo 1 — Contrato da API (OpenAPI)

Cobre os endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8).

- **D6.** [~] **Rate limits da API de cliente** ([12-limites-api.md](12-limites-api.md)). Já definido: três categorias (publicação, consumo, administrativo), taxa + burst, resposta `429` com `Retry-After`. Falta: **como identificar o cliente** (não há autenticação — está fora do escopo) e se o limite é por IP ou global por nó.
- **D38.** [~] **`sort` por valor que muda** ([06-api.md](06-api.md), seção 8.1.2). Já definido: a paginação por chave não perde nem repete itens quando o `sort` é por campo imutável (nome, `id`, `createdAt`); em `sort` por valor mutável (ex.: `-depth` de `/stats/topics`) a ordem pode mudar entre páginas, e isso está registrado como ressalva. Falta decidir se a ressalva basta ou se `sort` fica restrito a campos estáveis (a ordenação por métrica seguiria só como top-N, sem paginar além da primeira página).
- **D39.** [~] **O commit de tópico carrega a definição completa?** A carga da tabela de tópicos no bootstrap ([02-arquitetura.md](02-arquitetura.md), seção 3.3) assume que o `commit` de criação ou alteração leva a definição inteira, para que o que chegou por commit prevaleça sobre o que veio do stream. Falta escrever isso no contrato de `/cluster/topics/commit` e decidir o que o `commit` da remoção leva (basta o `id`).
- **D40.** [ ] **Tamanho máximo da mensagem.** Nenhum documento fixa um limite para o corpo de uma mensagem. Ele define `cluster.sync.stream.max-line-bytes` (o buffer por linha dos streams de sincronização) e o corpo aceito em `POST /topics/{topic}/messages`, e uma mensagem maior que a linha quebraria a sincronização. Falta o valor e se entra nos limites fixos da API ([10-configuracao.md](10-configuracao.md)).
- Valores iniciais: `delivery.visibility-timeout.max` (vale para o `GET` e o `PATCH`), `delivery.max-messages.max`, `delivery.get.max-attempts`, `limit` padrão e máximo da paginação, valores de `ratelimit.*`.

### Passo 2 — Arquitetura do cluster e fluxo em camadas

O fluxo de escrita está definido (seção 3.1); os itens abaixo são lacunas do desenho.

- **D8.** [ ] **Como um nó novo encontra o primeiro nó do cluster** (seção 6.2 diz "se conecta a um nó existente", mas não como obtém o endereço): lista de sementes, DNS, API do Kubernetes. Liga-se à "descoberta automática" do escopo e à integração com Kubernetes (seção 4.4). Vale também para o reinício de um nó, que volta como nó novo (D29.2).
- **D9.** [~] **Fluxo de leitura/consumo.** Já definido ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)): o nó que recebe o `GET` pega um ID da **lista de prontos** do tópico, marca a invisibilidade por registro + commit (decidido por **quórum**, sempre incluindo o primário) e só então devolve a mensagem. A lista de cada nó tem só as mensagens de que ele é **primário** (dono da entrega, seção 12.7); com a lista vazia, o nó pede ao dono de outro nó e, com long polling, espera avisos pelo canal entre nós (seção 12.8).
  - **D9.1** Com o dono **fora do ar** e ainda não desregistrado, o secundário pode assumir a entrega das mensagens dele?
  - **D9.2** Como a thread da lista encontra candidatos visíveis que não estão na lista. As de invisibilidade vencida já voltam pela lista de invisíveis ([14-invisibilidade.md](14-invisibilidade.md), seção 13.6); falta o caso geral (varredura vs. índice para as demais).
- **D29.** [~] **Canal entre nós** (seções 12.8 e 12.9, e 6.3). Já definido: canal **WebSocket** permanente entre cada par de nós (malha completa), que também é o controle de vida (substitui o ping); só sinais pequenos; filas por nó com retomada na reconexão.
  - *Decidido:* **D29.1** o nó que entra ou volta abre a conexão, pelos IPs da tabela de membros, e numa queda com os dois nós vivos quem detectar primeiro reconecta, com desempate pelo menor hash do ID do nó, que fecha a conexão dele; **D29.2** a tabela de membros não é persistida: um reinício faz o nó voltar como novo e passar pelo registro (seção 6.6), enviando as mensagens em disco a quem as perdeu; **D29.5** não há ressincronização do estado do canal: se a fila estoura, o nó é excluído do cluster e, ao voltar, é forçado a sincronizar por um novo registro.
  - **D29.3** O sinal de vida: usar o ping/pong do WebSocket ou um sinal próprio.
  - **D29.4** Se os sinais levam **número de sequência**, para reenviar os que podem ter se perdido na queda, ou se basta reenviar o que estiver na fila (o problema está explicado em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9).
- **D30.** [~] **Avisos de mensagem nova.** Já definido: o nó dono avisa um nó com ouvinte, que vai buscar.
  - **D30.1** **Quantos nós avisar** por mensagem nova (proposta: um por mensagem pronta, revezando entre os nós com ouvintes, para não acordar todos e provocar disputa).
  - **D30.2** Se avisos em rajada são agrupados numa janela curta.
- **D41.** [ ] **Falha total da carga da tabela de tópicos** (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)). Se nenhum nó consegue servir `GET /cluster/topics`, o nó novo nunca fica pronto. Falta decidir se ele espera e tenta de novo, ou se a falha conta como falha do registro (exclusão e novo bootstrap, ligado ao D19).
- **D42.** [ ] **De qual nó puxar os tópicos.** Qualquer nó serve, e a carga continua em outro se o primeiro falhar. Falta escolher a regra: o nó de contato, um aleatório ou o mais próximo. Não afeta a corretude.
- Valores iniciais: `cluster.channel.queue.max-size`, `cluster.channel.queue.max-age`.

### Passo 3 — Protótipo de nó único (memória + disco)

Inclui o overflow por tópico (seção 3.2).

- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.
- **D11.** [~] **Priorização de memória entre tópicos** (seção 3.2). Já definido: divisão dinâmica por **taxa de leitura** de cada tópico, exposta como `consumeRate`.
  - **D11.1** Janela de tempo da taxa (a mesma serve a `publishRate` e `consumeRate`).
  - **D11.2** Política de despejo (LRU/LFU por tópico).
  - **D11.3** Tratamento de tópicos novos sem histórico.
- **D12.** [~] **Disco.** Já definido: um arquivo por mensagem em `${topico}/${id}.json` (mais `${id}.meta.json`) e um por tópico em `_topics/${topico}.json` (seção 3.3) — sempre organizado por **tópico**, nunca por bloco (a mensagem se move de bloco se o anel mudar, mas não muda de pasta).
  - **D12.1** Onde fica a raiz (`storage.dir`): path padrão e se é por nó.
  - **D12.2** Rotina de limpeza dos arquivos — a remoção pelo `DELETE /topics/{topic}` já está decidida (seção 3.3); falta o caso geral (arquivos órfãos, expiração por TTL quando D13.1 for decidido).
- **D13.** [~] **Retenção, expiração (TTL) e dead-letter.** Já definido: cada mensagem tem `deliveryCount` nos metadados, base para um limite de entregas. Reprocessamento histórico está fora do escopo (seção 5).
  - **D13.1** TTL da mensagem.
  - **D13.2** Limite de entregas antes do dead-letter.
  - **D13.3** Para onde a mensagem vai no dead-letter (tópico próprio?).
- **D34.** [~] **Excedente da lista de invisíveis em disco** ([14-invisibilidade.md](14-invisibilidade.md), seção 13.5). Já definido: índice de expiração por janela de tempo em `${topico}/_expiracao/`, com a invariante "a lista em memória vence antes do disco", remoção preguiçosa e reconstrução a partir dos metadados.
  - **D34.1** Tamanho da janela e limiar de recarga da lista.
  - **D34.2** Se a gravação do índice precisa de `fsync` (proposta: não, por ser derivado dos metadados).
  - **D34.3** Custo da reconstrução ao assumir um bloco grande (varredura dos `.meta.json`) e se vale um índice persistente.
- **D35.** [ ] **Como o nó descobre a mensagem mais antiga do tópico sem varredura** (`oldestMessageAt`, [11-observabilidade.md](11-observabilidade.md)). A mais antiga visível é a cabeça da lista de prontos; falta a das invisíveis (a lista é ordenada por prazo, não por ID, e o excedente está em disco) e o que fazer quando a mais antiga recebe ack. Propostas: manter também um índice ordenado por ID, ou aceitar valor aproximado, recalculado por varredura periódica.
- Valores iniciais: `memory.limit`, `delivery.ready-list.size`, `delivery.invisible-list.size`, `delivery.invisible.spill.window`.

### Passo 4 — Protocolo genérico de registro/commit

Reutilizado em nós, mensagens e recuperação. Decidido nesta revisão: rollback só é seguro **antes** do quórum decidir (registro pendente); depois de decidido, o coordenador nunca desfaz — insiste na(s) réplica(s) restante(s) dentro de um teto configurável. Mensagens (escrita, entrega/invisibilidade, ack) decidem o commit por **quórum mínimo**, sempre incluindo o primário do bloco, em vez de esperar todas as réplicas; registro/desregistro de nó e definição de tópico continuam exigindo todos os nós, por serem operações raras sem caminho crítico de latência ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).

- **D33.** [ ] **Quórum de escrita fixo ou proporcional?** Hoje proposto como maioria simples de `replication.factor` (ex.: 2 em 3). Falta decidir se é sempre maioria, um valor fixo configurável independente do fator de replicação, ou se pode variar por tipo de operação (escrita vs. entrega/invisibilidade vs. ack) em vez de um único `write.quorum` global.
- Valores iniciais: `write.retry.attempts`, `write.retry.backoff`, `write.pending.ttl`, `write.commit.timeout`, `write.quorum`.

### Passo 5 — Anel de hash circular

- Valores iniciais: `ring.virtual-nodes`.

### Passo 6 — Sincronização distribuída de blocos

Na entrada de novos nós, incluindo o evento de falha de perda de mensagem.

- **D15.** [~] **Evento de perda de mensagem** (seção 7.3). Já definido: proposta em [11-observabilidade.md](11-observabilidade.md) — log `error`, métrica `msg.cluster.message_loss` e lista consultável; o alerta fica no backend, o sistema não notifica.
  - **D15.1** Aprovar a proposta.
  - **D15.2** **Como o evento identifica o tópico** (o sincronizador só enxerga IDs de mensagem por bloco); sem isso só existe a condição `message-loss` do cluster, não a do tópico.
  - **D15.4** Com a busca por faixa (`GET /cluster/blocks/{id}/messages?from=&to=`, seção 8.2.1 de [06-api.md](06-api.md)), o que faltou na faixa é buscado em outro nó e depois pela rota por ID único. Revisar o D15.3 com isso: a mensagem só é declarada perdida depois dessas tentativas e da verificação final.
  - **D15.3** *(achado nesta revisão)* **Falso positivo com a verificação final (seção 7.3):** hoje, se a busca de conteúdo (passo 3) falha para um ID porque ele foi deletado por um ack que nunca passou por este nó (a mesma janela residual que a verificação final do passo 5 cobre), o texto atual gera um evento de perda **antes** de chegar ao passo 5 — a verificação final só re-checa mensagens já obtidas, não as que falharam ao buscar. Ou o passo 3 dá à mensagem não encontrada a mesma chance da verificação final antes de declarar perda, ou se aceita esse falso positivo raro como está.
- **D16.** [ ] *(não bloqueia)* **Otimização da listagem de IDs por bloco** para blocos muito grandes — ex.: merkle tree ou hashing incremental por faixa, em vez de listar todos os IDs a cada vez. Vale tanto para a listagem inicial quanto para a verificação final (seção 7.3), que hoje relista.

- **D43.** [ ] **Mensagem para um tópico que o nó novo ainda não conhece.** O nó novo entra na tabela de membros antes de terminar de carregar os tópicos, então pode receber o registro de uma mensagem (como réplica) ou uma publicação de um tópico ainda desconhecido. Falta decidir se ele guarda a mensagem à espera da definição, a recusa (o coordenador tenta de novo) ou a aceita sem validar o tópico.
- **D44.** [ ] *(calibragem)* **Divisão em faixas** da sincronização de conteúdo: tamanho de cada faixa pedida a uma origem e relação com `cluster.sync.stream.max-items`. Entra nos valores iniciais abaixo.
- Valores iniciais: `cluster.sync.stream.*` (teto de itens e de bytes, tamanho máximo de linha, timeout de leitura) e o tamanho das faixas.

### Passo 7 — Rebalanceamento de blocos

Em caso de falha de registro ou desregistro de nó. Sem pendências.

### Passo 8 — Sinal de vida pelo canal, votação com quórum e freezing anti-flapping

- **D17.** [~] **Quórum de votação.** Já definido: padrão de 3 votos e cluster de produção com 3 nós. Problema: com 3 nós e um fora do ar sobram 2 votantes, então o quórum de 3 nunca é atingido e o nó não pode ser excluído (condição `quorum-lost`).
  - **D17.1** O quórum é fixo em 3 ou proporcional ao tamanho do cluster (maioria)?
  - **D17.2** O nó suspeito e o que detectou a falha contam como votantes?
- **D18.** [~] **Freezing** (seção 6.7). Já definido: duração configurável, e a observabilidade já prevê o log "rejeitado (freezing)", o que aponta para **rejeição no bootstrap**.
  - **D18.1** Confirmar rejeição vs. lista de espera.
  - **D18.2** Se a resposta informa quando tentar de novo.
- **D19.** [ ] **Retry do bootstrap após exclusão por falha no registro** (seção 6.5): tentativa imediata, backoff ou intervenção manual.
- Valores iniciais: `cluster.channel.keepalive.interval`, `cluster.channel.reconnect.retries` e `.backoff` (quanto tempo o canal pode ficar fora antes da votação), `cluster.freezing.duration`.

### Passo 9 — Retorno de nó

Recuperação de mensagens perdidas + novo bootstrap. Depende da lista de eventos de perda (D15).

- **D45.** [ ] **`recover` em lote contra o registro/commit** (seção 6.6 de [04-cluster.md](04-cluster.md)). O texto diz que o nó que volta envia as mensagens "via o mesmo procedimento de registro/commit da seção 7.2", mas `POST /cluster/messages/recover` agora é um stream NDJSON em lote, e duas fases por mensagem não combinam com isso. Falta decidir se o `recover` grava direto nos donos do bloco (idempotente por ID, como a sincronização) ou se mantém registro + commit por linha, e como isso conversa com o quórum de escrita.

### Passo 10 — Empacotamento em Docker e validação em Kubernetes

Observabilidade e rotas de operação, a fechar até aqui.

- **D20.** [~] **Nomes de métricas e atributos.** Já definido: proposta completa em [11-observabilidade.md](11-observabilidade.md), com mapeamento OTel → Prometheus. Falta: revisar contra as convenções semânticas do OpenTelemetry ao implementar.
- **D21.** [~] **Como o Prometheus descobre os nós.** Já definido: cada nó é um alvo de scrape e `/metrics` é local, sem fan-out. Falta: escolher entre lista estática, DNS ou tabela de membros — no Kubernetes, o caminho natural é a descoberta do próprio cluster (ligado à D8).
- **D22.** [~] **Rate limit das rotas de operação.** Já definido: sem porta própria nem lista de IPs, valem as regras de qualquer rota. Falta só o caso em que estão isoladas: em qual categoria entram e se ficam isentas.
- **D23.** [~] **`X-Forwarded-For` em `operations.allowed-ips`.** Já definido: por padrão não é confiado. Falta: se pode ser habilitado, com lista de proxies confiáveis, para nós atrás de balanceador.
- **D46.** [ ] **Métricas dos streams de sincronização** ([11-observabilidade.md](11-observabilidade.md)). As métricas `msg.sync.*` cobrem só blocos. Faltam, por exemplo, streams sem linha final, retomadas por corte, duração e volume da carga de tópicos, e o espelho disso no `recover`. Falta propor nomes e atributos.
- Valores iniciais: `telemetry.*`, `stats.topics.*` (inclui `telemetry.topics.max-topics`), `stats.disk.free-min`.

## Referências internas

- Mapa mental do projeto (`mapa-mental.html`, neste projeto) — visão gráfica do escopo e das decisões.
