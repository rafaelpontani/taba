# Limites da API

[← Índice](README.md)

> Valores dos parâmetros e padrões: [10-configuracao.md](10-configuracao.md#rate-limits) e [10-configuracao.md](10-configuracao.md#limites-fixos-da-api).

Dois tipos de limite na API de cliente ([06-api.md](06-api.md)): os **limites fixos** de validação (não configuráveis) e os **rate limits** (limites de taxa, configuráveis).

## Limites fixos (validação)

Regras de formato, aplicadas na criação/alteração de recursos, sem relação com taxa de requisições:

- **Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`) — 1 a 80 caracteres, regex `^[A-Za-z0-9_-]{1,80}$`, diferencia maiúsculas de minúsculas. Nome inválido é rejeitado na criação (`POST /topics`) com `400`.
- **Tags de tópico**: até 20 tags por tópico; chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL — ver [11-observabilidade.md](11-observabilidade.md)).
- **Prefixos de Tópico Distribuidor** (`prefixes`): até 20 por tópico, de 1 a 80 caracteres cada, mesmo conjunto do nome de tópico (`A-Za-z0-9_-`). Prefixo vazio é rejeitado com `400` ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2).
- **`idleTimeout` de fila volátil**: de 60 s a 7 dias (604800 s); fora da faixa, `400` (seção 14.3).
- **Long polling** (`waitTime` no `GET /topics/{topic}/messages`): de 0 a 20 segundos, padrão 0 (responde na hora). Valor fora da faixa é rejeitado com `400`. Ver [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md).

## Limites configuráveis, sem teto por padrão

Definidos por variável de ambiente ([10-configuracao.md](10-configuracao.md)); enquanto não definidos, não há limite:

- **Destinos por publicação** num Tópico Distribuidor (`topics.distributor.max-targets`): acima do teto, a publicação é recusada inteira com `422`.
- **Filas voláteis** no cluster (`topics.volatile.max-count`): atingido o teto, o `autoCreate` é recusado com `422`.
- **Mensagens por `GET`** (`maxMessages`): padrão 10 (`delivery.max-messages.default`), com teto configurável (`delivery.max-messages.max`).

## Rate limits

> Ainda não fazem parte do escopo documentado ([03-escopo.md](03-escopo.md)) — esta seção propõe uma estrutura inicial, a ser validada.

Limite de requisições por cliente, aplicado no nó que recebe a chamada, separado por tipo de operação — publicação tende a ter um limite mais alto que operações administrativas de tópico:

- **Publicação** (`POST /topics/{topic}/messages`).
- **Consumo** (`GET /topics/{topic}/messages`, `PATCH /topics/{topic}/messages/{id}`, a alteração de invisibilidade, e `DELETE /topics/{topic}/messages/{id}`, o ack).
- **Administrativo** (`POST/GET/PATCH/DELETE /topics` e `/topics/{topic}`) — tende a ser o mais restritivo dos três.

Cada limite é do tipo taxa + estouro (`ratelimit.burst`, ex.: token bucket), para absorver picos curtos sem rejeitar requisições.

**Resposta ao exceder o limite:** `429 Too Many Requests` com cabeçalho `Retry-After`.

**Identificação do cliente (pendência):** o escopo atual não define autenticação/API key ([03-escopo.md](03-escopo.md), seção 5 — fora do escopo). Sem isso, falta decidir se o rate limit é aplicado por IP, por um token de API futuro, ou globalmente por nó enquanto não houver identificação de cliente.

**Rotas de operação:** `/metrics`, `/stats/*` e `/health/*` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)) são tratadas como qualquer outra rota — inclusive para rate limit — quando não configuradas com porta própria (`operations.port`) ou lista de IPs (`operations.allowed-ips`). Se e como o limite se aplica a elas quando isoladas está em aberto.

**Não confundir com:** o retry de registro/commit entre nós do cluster ([04-cluster.md](04-cluster.md), [05-particionamento-replicacao.md](05-particionamento-replicacao.md)) é comunicação interna e não tem relação com o rate limit da API de cliente — um protege a durabilidade, o outro protege a disponibilidade do nó contra uso excessivo de um cliente.

Pendências: [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md) (passo 1).
