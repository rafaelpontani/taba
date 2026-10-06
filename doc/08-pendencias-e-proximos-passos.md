# Pendências e próximos passos

[← Índice](README.md)

## 10. Pendências e próximos passos

O objetivo deste documento é **zerar as dúvidas e avançar**: cada próximo passo lista as decisões que precisam estar fechadas antes dele. Uma pendência aparece uma vez, no primeiro passo que depende dela. Ao ser decidida, é registrada no documento técnico correspondente (e o valor padrão em [10-configuracao.md](10-configuracao.md)) e removida daqui.

Legenda:

- **D1, D2…** — número da dúvida, para responder por número (ex.: "D4.1: o assinante é identificado por nome"). Uma dúvida com mais de uma pergunta em aberto é dividida em **sub-códigos** (**D4.1**, **D4.2**…), que podem ser respondidos separadamente. Nem os números nem os sub-códigos são reaproveitados: ao decidir, o item sai da lista e os demais mantêm o código.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto diz **o que já está definido**, e cada pergunta que falta tem seu sub-código.

**Já decididas** (registradas nos documentos técnicos): D1 (semântica de entrega), D3 (modelo de consumo: polling e long polling de até 20 s) e D32 (limite e juntada das filas do canal), em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md); o controle de vida entre nós pelo canal permanente, sem ping (seção 6.3); do canal entre nós (D29), D29.1, D29.2, D29.5 e D29.6 (concorrência entre registro e desregistro, seção 6.8); D5 (propagação e remoção de tópicos, incluindo mensagens pendentes) e D26 (ack por registro + commit, sem tombstone — a verificação final da sincronização de bloco cobre a janela residual), em [02-arquitetura.md](02-arquitetura.md) seção 3.3, [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) seção 12.4 e [05-particionamento-replicacao.md](05-particionamento-replicacao.md) seção 7.3.

### Resumo das dúvidas por passo

| Passo | Dúvidas |
|---|---|
| 1 — Contrato da API | D2, D4, D6–D7, D25, D27–D28, D31: ordenação, assinantes do `broadcast`, rate limits, paginação, alteração de invisibilidade, ack de entrega obsoleta, concorrência no `GET`, mensagens por `GET` |
| 2 — Arquitetura | D8, D9, D29, D30: como o nó novo acha o primeiro nó, fluxo de leitura, detalhes do canal WebSocket entre nós, avisos de mensagem nova |
| 3 — Protótipo | D10–D13: linguagem, priorização de memória, disco, retenção/TTL/dead-letter |
| 6 — Sincronização | D15–D16: evento de perda, otimização de listagem |
| 8 — Sinal de vida, quórum e freezing | D17–D19: quórum, freezing, retry do bootstrap |
| 10 — Docker e Kubernetes | D20–D24: métricas, Prometheus, rate limit das rotas de operação, `X-Forwarded-For`, métricas por assinante |

Os passos 4, 5, 7 e 9 não têm dúvidas próprias.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares, tetos) são **calibragem**, não dúvida de design: cada passo os cita numa linha "Valores iniciais" e eles podem ser fechados com um chute razoável e ajustados em teste.

### Passo 1 — Contrato da API (OpenAPI)

Cobre os endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8).

- **D2.** [~] **Ordenação.** Já definido: o ID é um GUID v7 ordenável por tempo, e a lista de prontos de cada nó é reposta nessa ordem, só com as mensagens de que ele é o dono da entrega ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seções 12.6 e 12.7). Falta: declarar a garantia ao cliente — a ordem vale por nó dono, não por tópico (as mensagens se espalham por todos os blocos, seção 7.2), então a garantia seria de melhor esforço, sem ordem global; por chave não há conceito de chave na API.
- **D4.** [ ] **Assinantes do tipo `broadcast`.** O tipo existe (`single` ou `broadcast`), mas não há conceito de assinante. A invisibilidade e o contador de entregas ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)) hoje são por mensagem; no `broadcast` teriam de ser por assinante.
  - **D4.1** Quem é o assinante e como é identificado (não há autenticação).
  - **D4.2** Como o ack funciona quando cada assinante confirma separadamente e quando a mensagem sai do tópico.
- **D6.** [~] **Rate limits da API de cliente** ([12-limites-api.md](12-limites-api.md)). Já definido: três categorias (publicação, consumo, administrativo), taxa + burst, resposta `429` com `Retry-After`. Falta: **como identificar o cliente** (não há autenticação — está fora do escopo) e se o limite é por IP ou global por nó.
- **D7.** [~] **Paginação.** Já definido: `/stats/topics` usa `limit`/`cursor`. Falta: se `GET /topics` também pagina (o `limit` padrão e máximo entram na calibragem).
- **D25.** [~] **Alteração da invisibilidade** (`PATCH`, seção 12.3). Já definido: rota que altera o prazo, propagada por registro + commit.
  - **D25.1** O valor é o **novo tempo a partir de agora** ou um **acréscimo** ao prazo atual.
  - **D25.2** Se `0` libera a mensagem na hora.
- **D27.** [ ] **Ack ou alteração de uma entrega obsoleta.** Ack e `PATCH` identificam a mensagem só pelo ID: um consumidor lento, cujo prazo já expirou, pode confirmar ou alterar uma mensagem que já foi reentregue a outro. Falta decidir se a entrega devolve um recibo (ex.: o `deliveryCount` da entrega, exigido no ack e no `PATCH`) ou se isso é aceito como característica do at-least-once.
- **D28.** [~] **Concorrência no `GET`.** Já definido: o dono da entrega evita a disputa em condições normais (seção 12.7).
  - **D28.1** Proposta a confirmar: o registro de invisibilidade é exclusivo — um nó rejeita o pedido se já há um pendente ou se a mensagem já está invisível, e o `GET` tenta outro ID da lista.
  - **D28.2** O que o `GET` devolve quando o registro falha após os retries (erro ao cliente ou próximo ID).
- **D31.** [ ] **Mensagens por `GET`.** A API fala em "mensagem(ns)", mas não há parâmetro nem teto para a quantidade (ex.: `maxMessages`). Vale também para o pedido `take` entre nós.
- Valores iniciais: `delivery.visibility-timeout.max` (vale para o `GET` e o `PATCH`), `limit` padrão e máximo da paginação, valores de `ratelimit.*`.

### Passo 2 — Arquitetura do cluster e fluxo em camadas

O fluxo de escrita está definido (seção 3.1); os itens abaixo são lacunas do desenho.

- **D8.** [ ] **Como um nó novo encontra o primeiro nó do cluster** (seção 6.2 diz "se conecta a um nó existente", mas não como obtém o endereço): lista de sementes, DNS, API do Kubernetes. Liga-se à "descoberta automática" do escopo e à integração com Kubernetes (seção 4.4). Vale também para o reinício de um nó, que volta como nó novo (D29.2).
- **D9.** [~] **Fluxo de leitura/consumo.** Já definido ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)): o nó que recebe o `GET` pega um ID da **lista de prontos** do tópico, marca a invisibilidade em todos os nós responsáveis (registro + commit) e só então devolve a mensagem. A lista de cada nó tem só as mensagens de que ele é **primário** (dono da entrega, seção 12.7); com a lista vazia, o nó pede ao dono de outro nó e, com long polling, espera avisos pelo canal entre nós (seção 12.8). Depende de D2 e D28.
  - **D9.1** Com o dono **fora do ar** e ainda não desregistrado, o secundário pode assumir a entrega das mensagens dele?
  - **D9.2** Como a thread da lista encontra candidatos (varredura vs. índice, incluindo as de invisibilidade expirada).
- **D29.** [~] **Canal entre nós** (seções 12.8 e 12.9, e 6.3). Já definido: canal **WebSocket** permanente entre cada par de nós (malha completa), que também é o controle de vida (substitui o ping); só sinais pequenos; filas por nó com retomada na reconexão.
  - *Decidido:* **D29.1** o nó que entra ou volta abre a conexão, pelos IPs da tabela de membros, e numa queda com os dois nós vivos quem detectar primeiro reconecta, com desempate pelo menor hash do ID do nó, que fecha a conexão dele; **D29.2** a tabela de membros não é persistida: um reinício faz o nó voltar como novo e passar pelo registro (seção 6.6), enviando as mensagens em disco a quem as perdeu; **D29.5** não há ressincronização do estado do canal: se a fila estoura, o nó é excluído do cluster e, ao voltar, é forçado a sincronizar por um novo registro.
  - **D29.3** O sinal de vida: usar o ping/pong do WebSocket ou um sinal próprio.
  - **D29.4** Se os sinais levam **número de sequência**, para reenviar os que podem ter se perdido na queda, ou se basta reenviar o que estiver na fila (o problema está explicado em [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.9).
- **D30.** [~] **Avisos de mensagem nova.** Já definido: o nó dono avisa um nó com ouvinte, que vai buscar.
  - **D30.1** **Quantos nós avisar** por mensagem nova (proposta: um por mensagem pronta, revezando entre os nós com ouvintes, para não acordar todos e provocar disputa).
  - **D30.2** Se avisos em rajada são agrupados numa janela curta.
- Valores iniciais: `cluster.channel.queue.max-size`, `cluster.channel.queue.max-age`.

### Passo 3 — Protótipo de nó único (memória + disco)

Inclui o overflow por tópico (seção 3.2).

- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.
- **D11.** [~] **Priorização de memória entre tópicos** (seção 3.2). Já definido: divisão dinâmica por **taxa de leitura** de cada tópico, exposta como `consumeRate`.
  - **D11.1** Janela de tempo da taxa (a mesma serve a `publishRate` e `consumeRate`).
  - **D11.2** Política de despejo (LRU/LFU por tópico).
  - **D11.3** Tratamento de tópicos novos sem histórico.
- **D12.** [~] **Disco.** Já definido: um arquivo por mensagem em `${topico}/${id}.json` (mais `${id}.meta.json`), e o nome do tópico é restrito para ser seguro como pasta.
  - **D12.1** Onde fica a raiz (`storage.dir`) e se há subpasta por bloco.
  - **D12.2** Rotina de limpeza dos arquivos.
- **D13.** [~] **Retenção, expiração (TTL) e dead-letter.** Já definido: cada mensagem tem `deliveryCount` nos metadados, base para um limite de entregas. Reprocessamento histórico está fora do escopo (seção 5).
  - **D13.1** TTL da mensagem.
  - **D13.2** Limite de entregas antes do dead-letter.
  - **D13.3** Para onde a mensagem vai no dead-letter (tópico próprio?).
- Valores iniciais: `memory.limit`, `delivery.ready-list.size`.

### Passo 4 — Protocolo genérico de registro/commit

Reutilizado em nós, mensagens e recuperação. Sem dúvidas de design.

- Valores iniciais: `write.retry.attempts`, `write.retry.backoff`, `write.pending.ttl`.

### Passo 5 — Anel de hash circular

- Valores iniciais: `ring.virtual-nodes`.

### Passo 6 — Sincronização distribuída de blocos

Na entrada de novos nós, incluindo o evento de falha de perda de mensagem.

- **D15.** [~] **Evento de perda de mensagem** (seção 7.3). Já definido: proposta em [11-observabilidade.md](11-observabilidade.md) — log `error`, métrica `msg.cluster.message_loss` e lista consultável; o alerta fica no backend, o sistema não notifica.
  - **D15.1** Aprovar a proposta.
  - **D15.2** **Como o evento identifica o tópico** (o sincronizador só enxerga IDs de mensagem por bloco); sem isso só existe a condição `message-loss` do cluster, não a do tópico.
- **D16.** [ ] *(não bloqueia)* **Otimização da listagem de IDs por bloco** para blocos muito grandes — ex.: merkle tree ou hashing incremental por faixa, em vez de listar todos os IDs a cada vez. Vale tanto para a listagem inicial quanto para a verificação final (seção 7.3), que hoje relista.

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

Recuperação de mensagens perdidas + novo bootstrap. Depende da lista de eventos de perda (D15). Sem pendências próprias.

### Passo 10 — Empacotamento em Docker e validação em Kubernetes

Observabilidade e rotas de operação, a fechar até aqui.

- **D20.** [~] **Nomes de métricas e atributos.** Já definido: proposta completa em [11-observabilidade.md](11-observabilidade.md), com mapeamento OTel → Prometheus. Falta: revisar contra as convenções semânticas do OpenTelemetry ao implementar.
- **D21.** [~] **Como o Prometheus descobre os nós.** Já definido: cada nó é um alvo de scrape e `/metrics` é local, sem fan-out. Falta: escolher entre lista estática, DNS ou tabela de membros — no Kubernetes, o caminho natural é a descoberta do próprio cluster (ligado à D8).
- **D22.** [~] **Rate limit das rotas de operação.** Já definido: sem porta própria nem lista de IPs, valem as regras de qualquer rota. Falta só o caso em que estão isoladas: em qual categoria entram e se ficam isentas.
- **D23.** [~] **`X-Forwarded-For` em `operations.allowed-ips`.** Já definido: por padrão não é confiado. Falta: se pode ser habilitado, com lista de proxies confiáveis, para nós atrás de balanceador.
- **D24.** [ ] **Métricas por assinante** (`broadcast`): dependem do modelo de assinantes (D4).
- Valores iniciais: `telemetry.*`, `stats.topics.*` (inclui `telemetry.topics.max-topics`), `stats.disk.free-min`.

## Referências internas

- Mapa mental do projeto (`mapa-mental.html`, neste projeto) — visão gráfica do escopo e das decisões.
