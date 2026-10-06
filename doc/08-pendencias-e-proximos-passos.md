# Pendências e próximos passos

[← Índice](README.md)

## 10. Pendências e próximos passos

O objetivo deste documento é **zerar as dúvidas e avançar**: cada próximo passo lista as decisões que precisam estar fechadas antes dele. Uma pendência aparece uma vez, no primeiro passo que depende dela. Ao ser decidida, é registrada no documento técnico correspondente (e o valor padrão em [10-configuracao.md](10-configuracao.md)) e removida daqui.

Legenda:

- **D1, D2…** — número da dúvida, para responder por número (ex.: "D4: cada assinante recebe uma cópia"). Os números **não são reaproveitados**: ao decidir uma dúvida ela sai da lista, e as demais mantêm o número.
- `[ ]` — em aberto, sem proposta.
- `[~]` — parcialmente resolvida: o texto diz **o que já está definido** e **o que falta**.

### Resumo das dúvidas por passo

| Passo | Dúvidas |
|---|---|
| 1 — Contrato da API | D1–D7: semântica de entrega, ordenação, modelo de consumo, assinantes do `broadcast`, propagação de tópicos, rate limits, paginação |
| 2 — Arquitetura | D8–D9: como o nó novo acha o primeiro nó, fluxo de leitura |
| 3 — Protótipo | D10–D13: linguagem, priorização de memória, disco, retenção/TTL/dead-letter |
| 4 — Registro/commit | D14: retry e expiração |
| 6 — Sincronização | D15–D16: evento de perda, otimização de listagem |
| 8 — Ping, quórum e freezing | D17–D19: quórum, freezing, retry do bootstrap |
| 10 — Docker e Kubernetes | D20–D24: métricas, Prometheus, rate limit das rotas de operação, `X-Forwarded-For`, métricas por assinante |

Os passos 5, 7 e 9 não têm dúvidas próprias.

Os valores numéricos ainda "A definir" em [10-configuracao.md](10-configuracao.md) (retries, intervalos, TTLs, limiares) são **calibragem**, não dúvida de design: cada passo os cita numa linha "Valores iniciais" e eles podem ser fechados com um chute razoável e ajustados em teste.

### Passo 1 — Contrato da API (OpenAPI)

Cobre os endpoints de registro/descoberta de nós, mensagens, sincronização e recuperação (seções 6, 7 e 8).

- **D1.** [~] **Semântica de entrega.** Já definido: `GET` entrega e `DELETE /messages/{id}` confirma (ack) e remove a mensagem — o desenho aponta para *at-least-once*. Falta: confirmar isso e definir o que acontece com uma mensagem entregue e **não confirmada** (reentrega após quanto tempo, quantas vezes) — sem isso `inFlight` e dead-letter não podem ser definidos.
- **D2.** [~] **Ordenação.** Já definido: o ID é um GUID v7 ordenável por tempo. Falta: declarar a garantia — as mensagens de um tópico se espalham por todos os blocos do anel (seção 7.2), então ordem por tópico exigiria ordenar entre nós; por chave não há conceito de chave na API.
- **D3.** [~] **Modelo de consumo.** Já definido: `GET /topics/{topic}/messages` consome uma ou mais mensagens (polling). Falta: decidir se long polling ou push entram no escopo inicial.
- **D4.** [ ] **Assinantes do tipo `broadcast`.** O tipo existe (`single` ou `broadcast`), mas não há conceito de assinante: quem recebe cópia, como é identificado e como o ack funciona quando cada assinante confirma separadamente.
- **D5.** [ ] **Propagação de tópicos entre os nós.** Criação, alteração e remoção de tópico não têm rota interna em 8.2 (provavelmente o mesmo protocolo registro → commit). Falta também o comportamento do `DELETE /topics/{topic}` com mensagens ainda pendentes.
- **D6.** [~] **Rate limits da API de cliente** ([12-limites-api.md](12-limites-api.md)). Já definido: três categorias (publicação, consumo, administrativo), taxa + burst, resposta `429` com `Retry-After`. Falta: **como identificar o cliente** (não há autenticação — está fora do escopo) e se o limite é por IP ou global por nó; valores padrão.
- **D7.** [~] **Paginação.** Já definido: `/stats/topics` usa `limit`/`cursor`. Falta: `limit` padrão e máximo, e se `GET /topics` também pagina.

### Passo 2 — Arquitetura do cluster e fluxo em camadas

O fluxo de escrita está definido (seção 3.1); os itens abaixo são lacunas do desenho.

- **D8.** [ ] **Como um nó novo encontra o primeiro nó do cluster** (seção 6.2 diz "se conecta a um nó existente", mas não como obtém o endereço): lista de sementes, DNS, API do Kubernetes. Liga-se à "descoberta automática" do escopo e à integração com Kubernetes (seção 4.4).
- **D9.** [ ] **Fluxo de leitura/consumo.** Só a escrita está desenhada. Como as mensagens de um tópico se espalham por todos os blocos, falta definir como o nó que recebe o `GET` escolhe a mensagem a entregar, e como evita entregá-la duas vezes a consumidores diferentes. Depende de D1 (semântica de entrega) e D2 (ordenação).

### Passo 3 — Protótipo de nó único (memória + disco)

Inclui o overflow por tópico (seção 3.2).

- **D10.** [ ] **Linguagem e runtime.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Precisa estar decidida antes de escrever código; também define o que `msg.node.threads` mede.
- **D11.** [~] **Priorização de memória entre tópicos** (seção 3.2). Já definido: divisão dinâmica por **taxa de leitura** de cada tópico, exposta como `consumeRate`. Falta: janela de tempo da taxa (a mesma serve a `publishRate` e `consumeRate`), política de despejo (LRU/LFU por tópico) e tratamento de tópicos novos sem histórico.
- **D12.** [~] **Disco.** Já definido: um arquivo por mensagem em `${topico}/${id}.json`, e o nome do tópico é restrito para ser seguro como pasta. Falta: onde fica a raiz (`storage.dir`) e se há subpasta por bloco, e a rotina de limpeza dos arquivos.
- **D13.** [ ] **Retenção, expiração (TTL) e dead-letter.** Reprocessamento histórico está fora do escopo (seção 5), mas TTL e dead-letter não estão decididos; dead-letter depende da reentrega definida em D1.
- Valores iniciais: `memory.limit`.

### Passo 4 — Protocolo genérico de registro/commit

Reutilizado em nós, mensagens e recuperação.

- **D14.** [~] **Retry e expiração.** Já definido: retry nas duas fases, rollback ao esgotar, e registro sem commit expira sozinho. Falta só os valores (`write.retry.attempts`, `write.retry.backoff`, `write.pending.ttl`).

### Passo 5 — Anel de hash circular

- Valores iniciais: `ring.virtual-nodes`.

### Passo 6 — Sincronização distribuída de blocos

Na entrada de novos nós, incluindo o evento de falha de perda de mensagem.

- **D15.** [~] **Evento de perda de mensagem** (seção 7.3). Já definido: proposta em [11-observabilidade.md](11-observabilidade.md) — log `error`, métrica `msg.cluster.message_loss` e lista consultável; o alerta fica no backend, o sistema não notifica. Falta: aprovar a proposta e **como o evento identifica o tópico** (o sincronizador só enxerga IDs de mensagem por bloco); sem isso só existe a condição `message-loss` do cluster, não a do tópico.
- **D16.** [ ] *(não bloqueia)* **Otimização da listagem de IDs por bloco** para blocos muito grandes — ex.: merkle tree ou hashing incremental por faixa, em vez de listar todos os IDs a cada vez.

### Passo 7 — Rebalanceamento de blocos

Em caso de falha de registro ou desregistro de nó. Sem pendências.

### Passo 8 — Retries de ping, votação com quórum e freezing anti-flapping

- **D17.** [~] **Quórum de votação.** Já definido: padrão de 3 votos e cluster de produção com 3 nós. Falta validar: com 3 nós e um fora do ar sobram 2 votantes, então o quórum de 3 nunca é atingido e o nó não pode ser excluído (condição `quorum-lost`). Decidir se o quórum é fixo em 3, proporcional ao tamanho do cluster (maioria) ou se o nó suspeito e o que detectou contam.
- **D18.** [~] **Freezing** (seção 6.7). Já definido: duração configurável, e a observabilidade já prevê o log "rejeitado (freezing)", o que aponta para **rejeição no bootstrap**. Falta: confirmar rejeição vs. lista de espera e se a resposta informa quando tentar de novo.
- **D19.** [ ] **Retry do bootstrap após exclusão por falha no registro** (seção 6.5): tentativa imediata, backoff ou intervenção manual.
- Valores iniciais: `cluster.ping.interval`, `cluster.ping.retries`, `cluster.freezing.duration`.

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
