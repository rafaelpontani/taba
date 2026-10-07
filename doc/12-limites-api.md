# Limites da API

[← Índice](README.md)

> Valores dos parâmetros e padrões: [10-configuracao.md](10-configuracao.md#rate-limits) e [10-configuracao.md](10-configuracao.md#limites-fixos-da-api).

Dois tipos de limite na API de cliente ([06a-api-cliente.md](06a-api-cliente.md)): os **limites fixos** de validação (não configuráveis) e os **rate limits** (limites de taxa, configuráveis).

## Limites fixos (validação)

Regras de formato, aplicadas na criação/alteração de recursos, sem relação com taxa de requisições:

- **Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.body`) — 1 a 100 caracteres, regex `^[A-Za-z0-9_-]{1,100}$`, diferencia maiúsculas de minúsculas. Nome inválido é rejeitado na criação (`POST /topics`) com `400`.
- **Tags de tópico**: até 20 tags por tópico; chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL — ver [11-observabilidade.md](11-observabilidade.md)).
- **Prefixos de Tópico Distribuidor** (`prefixes`): até 20 por tópico, de 1 a 100 caracteres cada, mesmo conjunto do nome de tópico (`A-Za-z0-9_-`). Prefixo vazio é rejeitado com `400` ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2).
- **`idleTimeout` de fila volátil**: de 60 s a 7 dias (604800 s); fora da faixa, `400` (seção 14.3).
- **`ttl` de fila**: de 60 s a 365 dias (31536000 s); fora da faixa, `400` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10).
- **`maxDeliveries` de fila**: de 1 a 1000; fora da faixa, `400` (seção 12.10).
- **Nome de fila com dead-letter**: a fila de dead-letter acrescenta o sufixo ao nome (até 112 caracteres com `-dead-letter`), e o `{topic}` das rotas a aceita; nome criado terminado no sufixo é reservado, `400`.
- **`groupId` da mensagem**: de 1 a 128 caracteres, `A-Za-z0-9_-.:`; fora disso, `400` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11).
- **`groupId` na fila FIFO**: obrigatório; ausente, `400` ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16.2).
- **`deduplicationId`** (só em fila FIFO): de 1 a 128 caracteres, `A-Za-z0-9_-.:`; fora disso, ou numa fila que não é FIFO, `400` (seção 16.2).
- **Fila FIFO e fator de replicação**: criar uma fila FIFO com `replication.factor` abaixo de 2 é recusado com `422` (seção 16.2).
- **Token de acesso** ([06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3): `name` de 1 a 100 caracteres (`A-Za-z0-9_-.:`); `permissions` com ao menos uma de `publish`, `consume`, `create` e `admin`; até 20 `topicPrefixes` de 1 a 100 caracteres (`A-Za-z0-9_-`); `expiresAt` no futuro. Fora disso, `400`.
- **`contentType` da mensagem**: media type válido de 1 a 255 caracteres, padrão `application/json`; fora disso, `400` `validation.invalid-content-type` ([06a-api-cliente.md](06a-api-cliente.md), "O corpo da mensagem").
- **Long polling** (`waitTime` no `GET /topics/{topic}/messages`): de 0 a 20 segundos, padrão 0 (responde na hora). Valor fora da faixa é rejeitado com `400`. Ver [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).

## Limites configuráveis, sem teto por padrão

Definidos por variável de ambiente ([10-configuracao.md](10-configuracao.md)); enquanto não definidos, não há limite:

- **Destinos por publicação** num Tópico Distribuidor (`topics.distributor.max-targets`): acima do teto, a publicação é recusada inteira com `422`.
- **Filas voláteis** no cluster (`topics.volatile.max-count`): atingido o teto, o `autoCreate` é recusado com `422`.
- **Tamanho da mensagem**: `message.max-bytes`, padrão **5 MB** (5.242.880 bytes), para todo tópico. Cada tópico pode definir o seu `maxMessageBytes`, **inclusive maior que o geral**, e, sem ele, usa o geral. Há um **limite rígido** (`message.max-bytes.hard-limit`) que nenhum tópico ultrapassa: valor acima dele na criação ou no `PATCH` é rejeitado com `400`. Mensagem maior que o limite do tópico responde `413`. A alteração por `PATCH` só vale para publicações seguintes; mensagens já aceitas não são rejeitadas. Num Tópico Distribuidor vale o geral, e as cópias nas filas destino não são conferidas de novo. O tamanho é o dos **bytes enviados**, qualquer que seja o formato do corpo. O tamanho máximo de linha dos streams de sincronização (`cluster.sync.stream.max-line-bytes`) precisa caber o limite rígido em base64 (× 4/3) mais o envelope JSON.
- **Mensagens por `GET`** (`maxMessages`): padrão 10 (`delivery.max-messages.default`), com teto configurável (`delivery.max-messages.max`).

## Rate limits

> Ainda não fazem parte do escopo documentado ([03-escopo.md](03-escopo.md)) — esta seção propõe uma estrutura inicial, a ser validada.

Limite de requisições por cliente, aplicado no nó que recebe a chamada, separado por tipo de operação — publicação tende a ter um limite mais alto que operações administrativas de tópico:

- **Publicação** (`POST /topics/{topic}/messages`).
- **Consumo** (`GET /topics/{topic}/messages`, `PUT /topics/{topic}/messages/{id}/visibility`, a alteração de invisibilidade, e `DELETE /topics/{topic}/messages/{id}`, o ack).
- **Administrativo** (`POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`, e `/tokens`) — tende a ser o mais restritivo dos três.

Cada limite é do tipo taxa + estouro (`ratelimit.burst`, ex.: token bucket), para absorver picos curtos sem rejeitar requisições.

**Resposta ao exceder o limite:** `429 Too Many Requests` com cabeçalho `Retry-After`.

**Identificação do cliente:** a chave do contador depende de a chamada trazer um **token válido** ([06a-api-cliente.md](06a-api-cliente.md), seção 8.1.3). Os contadores são **locais a cada nó**, sem troca entre nós (um cliente que se espalha por N nós pode chegar a N vezes o limite, o que é aceito).

| Chamada | Chave do contador |
|---|---|
| Com **token válido** (inclusive o raiz, que tem o seu contador) | O **`id` do token**, em cada categoria. O IP não entra: o limite é da aplicação, mesmo atrás de um balanceador ou NAT e mesmo que ela mude de IP. Um `403` (token válido sem a permissão) também conta no token. |
| **Sem token válido** | O **IP do cliente**, na categoria da rota. Vale para: a autenticação desligada (sem token raiz, e sempre no modo desenvolvimento), em que todas as rotas contam por IP; a chamada recusada com `401` (token ausente, mal formado, desconhecido, revogado ou vencido), para quem tenta adivinhar tokens não ficar sem limite; e as rotas de operação, que não têm autenticação. |

- **Ordem:** a autenticação (consulta local em memória) vem antes do rate limit, que então escolhe a chave.
- **Todos os tokens têm o mesmo limite** (`ratelimit.*` e `ratelimit.burst`). Limite próprio por token não existe por ora; entra depois, se o uso pedir.
- **`Retry-After`** vem do contador que estourou, como antes.
- **Nó em sincronização ou em drain** (seção 6.9 de [04-cluster.md](04-cluster.md)): não tem a tabela de tokens completa e não valida o token, então conta a chamada **por IP** e a repassa; o destino não conta de novo. Um token só vira chave depois de validado, para ninguém esgotar o limite de outro token citando o `id` dele.
- **Observabilidade:** `msg.api.rate_limited` ganha o atributo `key` (`token` ou `ip`). O `id` do token vai nos logs e traces, nunca em métrica ([11-observabilidade.md](11-observabilidade.md)).

Para as chamadas contadas **por IP**, o IP do cliente pode vir de **cabeçalhos HTTP**, o que importa atrás de balanceador:

- `client-ip.headers.enabled` (bool, padrão `true`): liga ou desliga o uso dos cabeçalhos. Desligado, vale sempre o endereço da conexão.
- `client-ip.headers` (lista, padrão `X-Forwarded-For`): cabeçalhos consultados, em ordem; vale o primeiro presente. Em listas separadas por vírgula (`X-Forwarded-For: cliente, proxy1`), vale o **primeiro** endereço, o do cliente original. Cabeçalho ausente ou com endereço inválido: cai no endereço da conexão.
- **Cuidado:** com os cabeçalhos ligados e o nó exposto sem balanceador, o cliente pode forjá-los e escapar do limite; sem balanceador na frente, desligue (`false`). O IP resolvido vale para o rate limit e também para `operations.allowed-ips` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).
- `client-ip.trusted-proxies` (lista de IPs ou CIDR, padrão vazia): os cabeçalhos só valem se a conexão vier de um endereço da lista. **Vazia, os cabeçalhos são aceitos de qualquer origem**: o serviço deve ficar numa camada de rede segura, já que as rotas de operação não têm autenticação.
- Nas chamadas repassadas durante a sincronização e o drain (seção 6.9 de [04-cluster.md](04-cluster.md)), o IP resolvido segue num cabeçalho interno.

**Rotas de operação:** `/health/live`, `/health/ready`, `/metrics` e `/stats/node` são **isentas** de rate limit: são leves e chamadas por máquina em intervalo fixo, e um `429` no `/health/ready` faria o balanceador tirar o nó da rotação por engano. `/stats/cluster` e `/stats/topics`, que consultam outros nós, têm uma **categoria própria** (`ratelimit.ops.rps`, por IP, com o `ratelimit.burst` comum). Valem em qualquer configuração, com ou sem porta própria ou lista de IPs ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

**Não confundir com:** o retry de registro/commit entre nós do cluster ([04-cluster.md](04-cluster.md), [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) é comunicação interna e não tem relação com o rate limit da API de cliente — um protege a durabilidade, o outro protege a disponibilidade do nó contra uso excessivo de um cliente.

Pendências e decisões: [08b-pendencias.md](08b-pendencias.md) e [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md).
