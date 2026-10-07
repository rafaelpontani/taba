# Limites da API

[← Índice](README.md)

> Valores dos parâmetros e padrões: [10-configuracao.md](10-configuracao.md#rate-limits) e [10-configuracao.md](10-configuracao.md#limites-fixos-da-api).

Dois tipos de limite na API de cliente ([06-api.md](06-api.md)): os **limites fixos** de validação (não configuráveis) e os **rate limits** (limites de taxa, configuráveis).

## Limites fixos (validação)

Regras de formato, aplicadas na criação/alteração de recursos, sem relação com taxa de requisições:

- **Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`) — 1 a 100 caracteres, regex `^[A-Za-z0-9_-]{1,100}$`, diferencia maiúsculas de minúsculas. Nome inválido é rejeitado na criação (`POST /topics`) com `400`.
- **Tags de tópico**: até 20 tags por tópico; chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL — ver [11-observabilidade.md](11-observabilidade.md)).
- **Prefixos de Tópico Distribuidor** (`prefixes`): até 20 por tópico, de 1 a 100 caracteres cada, mesmo conjunto do nome de tópico (`A-Za-z0-9_-`). Prefixo vazio é rejeitado com `400` ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2).
- **`idleTimeout` de fila volátil**: de 60 s a 7 dias (604800 s); fora da faixa, `400` (seção 14.3).
- **`ttl` de fila**: de 60 s a 365 dias (31536000 s); fora da faixa, `400` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10).
- **`maxDeliveries` de fila**: de 1 a 1000; fora da faixa, `400` (seção 12.10).
- **Nome de fila com dead-letter**: a fila de dead-letter acrescenta o sufixo ao nome (até 112 caracteres com `-dead-letter`), e o `{topic}` das rotas a aceita; nome criado terminado no sufixo é reservado, `400`.
- **`groupId` da mensagem**: de 1 a 128 caracteres, `A-Za-z0-9_-.:`; fora disso, `400` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11).
- **Long polling** (`waitTime` no `GET /topics/{topic}/messages`): de 0 a 20 segundos, padrão 0 (responde na hora). Valor fora da faixa é rejeitado com `400`. Ver [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).

## Limites configuráveis, sem teto por padrão

Definidos por variável de ambiente ([10-configuracao.md](10-configuracao.md)); enquanto não definidos, não há limite:

- **Destinos por publicação** num Tópico Distribuidor (`topics.distributor.max-targets`): acima do teto, a publicação é recusada inteira com `422`.
- **Filas voláteis** no cluster (`topics.volatile.max-count`): atingido o teto, o `autoCreate` é recusado com `422`.
- **Tamanho da mensagem**: `message.max-bytes`, padrão **5 MB** (5.242.880 bytes), para todo tópico. Cada tópico pode definir o seu `maxMessageBytes`, **inclusive maior que o geral**, e, sem ele, usa o geral. Há um **limite rígido** (`message.max-bytes.hard-limit`) que nenhum tópico ultrapassa: valor acima dele na criação ou no `PATCH` é rejeitado com `400`. Mensagem maior que o limite do tópico responde `413`. A alteração por `PATCH` só vale para publicações seguintes; mensagens já aceitas não são rejeitadas. Num Tópico Distribuidor vale o geral, e as cópias nas filas destino não são conferidas de novo. O tamanho máximo de linha dos streams de sincronização (`cluster.sync.stream.max-line-bytes`) precisa caber o limite rígido mais o envelope JSON.
- **Mensagens por `GET`** (`maxMessages`): padrão 10 (`delivery.max-messages.default`), com teto configurável (`delivery.max-messages.max`).

## Rate limits

> Ainda não fazem parte do escopo documentado ([03-escopo.md](03-escopo.md)) — esta seção propõe uma estrutura inicial, a ser validada.

Limite de requisições por cliente, aplicado no nó que recebe a chamada, separado por tipo de operação — publicação tende a ter um limite mais alto que operações administrativas de tópico:

- **Publicação** (`POST /topics/{topic}/messages`).
- **Consumo** (`GET /topics/{topic}/messages`, `PATCH /topics/{topic}/messages/{id}`, a alteração de invisibilidade, e `DELETE /topics/{topic}/messages/{id}`, o ack).
- **Administrativo** (`POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`) — tende a ser o mais restritivo dos três.

Cada limite é do tipo taxa + estouro (`ratelimit.burst`, ex.: token bucket), para absorver picos curtos sem rejeitar requisições.

**Resposta ao exceder o limite:** `429 Too Many Requests` com cabeçalho `Retry-After`.

**Identificação do cliente:** como o escopo não tem autenticação nem API key ([03-escopo.md](03-escopo.md), seção 5), o rate limit é **por IP do cliente**, contado **localmente em cada nó**, sem compartilhar contadores entre nós (um cliente que se espalha por N nós pode chegar a N vezes o limite, o que é aceito). Para nós atrás de balanceador, o IP do cliente pode vir de **cabeçalhos HTTP**:

- `client-ip.headers.enabled` (bool, padrão `true`): liga ou desliga o uso dos cabeçalhos. Desligado, vale sempre o endereço da conexão.
- `client-ip.headers` (lista, padrão `X-Forwarded-For`): cabeçalhos consultados, em ordem; vale o primeiro presente. Em listas separadas por vírgula (`X-Forwarded-For: cliente, proxy1`), vale o **primeiro** endereço, o do cliente original. Cabeçalho ausente ou com endereço inválido: cai no endereço da conexão.
- **Cuidado:** com os cabeçalhos ligados e o nó exposto sem balanceador, o cliente pode forjá-los e escapar do limite; sem balanceador na frente, desligue (`false`). O IP resolvido vale para o rate limit e também para `operations.allowed-ips` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).
- `client-ip.trusted-proxies` (lista de IPs ou CIDR, padrão vazia): os cabeçalhos só valem se a conexão vier de um endereço da lista. **Vazia, os cabeçalhos são aceitos de qualquer origem**: o serviço deve ficar numa camada de rede segura, já que não há autenticação.
- Nas chamadas repassadas durante a sincronização e o drain (seção 6.9 de [04-cluster.md](04-cluster.md)), o IP resolvido segue num cabeçalho interno.

**Rotas de operação:** `/health/live`, `/health/ready`, `/metrics` e `/stats/node` são **isentas** de rate limit: são leves e chamadas por máquina em intervalo fixo, e um `429` no `/health/ready` faria o balanceador tirar o nó da rotação por engano. `/stats/cluster` e `/stats/topics`, que consultam outros nós, têm uma **categoria própria** (`ratelimit.ops.rps`, por IP, com o `ratelimit.burst` comum). Valem em qualquer configuração, com ou sem porta própria ou lista de IPs ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

**Não confundir com:** o retry de registro/commit entre nós do cluster ([04-cluster.md](04-cluster.md), [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) é comunicação interna e não tem relação com o rate limit da API de cliente — um protege a durabilidade, o outro protege a disponibilidade do nó contra uso excessivo de um cliente.

Pendências e decisões: [08b-pendencias.md](08b-pendencias.md) e [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md).
