# API de cliente

[← Índice](README.md) · [Visão geral das APIs](06-api.md)

## 8.1 API de cliente

> Porta: `api.port` ([06-api.md](06-api.md), seção 8.0). A numeração das seções é a do conjunto das APIs: a 8.1.1, a API de operação, está em [06b-api-operacao.md](06b-api-operacao.md).
 
| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/topics` | Cria um tópico (nome, tipo `queue` ou `distributor`, e opcionalmente `tags`; `prefixes` para `distributor`; `volatile` e `idleTimeout` para `queue`; `fifo` opcional para `queue` (imutável; fila FIFO com ordem por grupo e deduplicação, exige `replication.factor` de pelo menos 2, `422` se não houver; [17-topicos-fifo.md](17-topicos-fifo.md), seção 16); `maxMessageBytes` opcional, tamanho máximo da mensagem do tópico, padrão `message.max-bytes` = 5 MB, podendo ser maior, até `message.max-bytes.hard-limit`; `ttl` e `maxDeliveries` opcionais para `queue`, padrões `retention.ttl.default` = 14 dias e `retention.max-deliveries.default` = 10). Toda fila ganha junto uma fila `{nome}-dead-letter` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.10). Gera o `id` (GUID v7) do tópico. Propagado a todos os nós por registro + commit; persistido em disco ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `GET` | `/topics` | Lista os tópicos existentes, **paginada** (seção 8.1.2), com `sort` (`name`, `id`, `createdAt`) e o filtro `name`, que devolve os tópicos cujo nome **contém** o texto informado (ex.: `?name=pedidos`), para ajudar na busca; diferencia maiúsculas de minúsculas, como o nome do tópico. Respondida **localmente**, sem consultar outros nós: a definição do tópico é replicada em todo nó (seção 3.3), diferente de `/stats/topics`, que faz fan-out para métricas de consumo. |
| `GET` | `/topics/{topic}` | Retorna as informações e configurações do tópico (tipo, tags, `ttl`, `maxDeliveries` etc.; na dead-letter, os valores da fila de origem, só leitura, com `deadLetter: true` e `source`). Também respondida localmente, pelo mesmo motivo. |
| `PATCH` | `/topics/{topic}` | Altera informações do tópico (tags; `prefixes` de um `distributor`; `idleTimeout` de uma fila volátil; `maxMessageBytes`; `ttl` e `maxDeliveries` de uma fila). Nome, `id`, tipo, `volatile` e `fifo` são imutáveis. Na fila `{nome}-dead-letter`, `PATCH` e `DELETE` respondem `409`: ela é somente leitura e segue a fila de origem. Mesma propagação do `POST`. |
| `DELETE` | `/topics/{topic}` | Remove o tópico e **todas** as suas mensagens, mesmo as pendentes (ainda não entregues ou não confirmadas) — sempre funciona, sem checagem. Mesma propagação do `POST`; falha no registro **cancela** a remoção (rollback); falha no commit, já decidido, não cancela — o coordenador insiste dentro do teto `write.commit.timeout`. Nunca uma remoção parcial visível. |
| `POST` | `/topics/{topic}/messages` | Publica uma mensagem no tópico. Corpo acima do tamanho máximo do tópico (`maxMessageBytes` ou, sem ele, `message.max-bytes`) retorna `413`. Aceita `groupId` (query, opcional, 1 a 128 caracteres): o grupo da mensagem, que faz a entrega dar preferência aos grupos com menos mensagens ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.11); valor inválido retorna `400`. Numa **fila FIFO**, `groupId` é **obrigatório** (`400` sem ele) e o grupo ordena a entrega; aceita também `deduplicationId` (opcional, 1 a 128 caracteres; sem ele vale o hash do corpo; `400` numa fila que não é FIFO), e a publicação repetida na janela responde com o `id` da original e `duplicate: true` ([17-topicos-fifo.md](17-topicos-fifo.md), seção 16). Num `distributor`, copia a mensagem para as filas cujo nome começa com algum prefixo e responde com o resultado por fila; sem fila casando, responde sucesso e descarta ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). |
| `GET` | `/topics/{topic}/messages` | Consome mensagem(ns) do tópico. Rejeitado com `409` num `distributor`. Aceita `autoCreate=true` (cria a fila se não existir, volátil por padrão, com `idleTimeout`, `volatile` e `tag.<chave>` opcionais; seção 14.4), `visibilityTimeout` (segundos, padrão 120): a mensagem entregue fica invisível a outros consumidores por esse tempo; e `waitTime` (segundos, 0 a 20, padrão 0): long polling, aguarda novas mensagens antes de responder vazio; e `maxMessages` (padrão **10**, configurável, com teto): a resposta traz de 1 a `maxMessages` mensagens, as que estiverem prontas ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Sem garantia de ordem entre as mensagens (seção 12.7), exceto numa fila FIFO, que entrega cada grupo em ordem e uma mensagem por vez, com cada mensagem do lote de um grupo diferente (seção 16.6 de [17-topicos-fifo.md](17-topicos-fifo.md)). Cada mensagem volta com o seu `groupId`, se tiver, e, na FIFO, com o `deduplicationId`. `autoCreate` aceita `fifo=true`. |
| `PATCH` | `/topics/{topic}/messages/{id}` | Altera a invisibilidade de uma mensagem já entregue (`{"visibilityTimeout": 300, "receiptToken": "..."}`), por exemplo para ganhar mais tempo de processamento. O `receiptToken` (devolvido na entrega) é obrigatório; token obsoleto retorna `409`. O valor é o novo tempo **a partir de agora**, não um acréscimo; `0` libera a mensagem na hora. |
| `DELETE` | `/topics/{topic}/messages/{id}` | Confirma o consumo (ack) de uma mensagem, removendo-a do tópico. Exige `receiptToken` (query string), o da última entrega; token obsoleto retorna `409`. Decide o commit por **quórum** (sempre com o primário; na FIFO, com o primário e o secundário, e o ack grava `deleteAt` em vez de apagar a linha, [17-topicos-fifo.md](17-topicos-fifo.md), seção 16.7): falha antes do quórum **cancela** o ack (rollback); falha numa réplica fora do quórum, já decidido, não cancela — insiste dentro do teto `write.commit.timeout` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.4). |
| `POST` | `/tokens` | Cria um token de acesso (`name`, `permissions`, `topicPrefixes` opcional, `expiresAt` opcional) e devolve o **segredo uma única vez** (seção 8.1.3). Só com o token raiz. Propagado a todos os nós por registro + commit ([02-arquitetura.md](02-arquitetura.md), seção 3.4). |
| `GET` | `/tokens` | Lista os tokens, **paginada** (seção 8.1.2), com `sort` (`id`, `createdAt`) e o filtro `name` (contém). Nunca devolve o segredo. Respondida localmente. Só com o token raiz. |
| `GET` | `/tokens/{id}` | Retorna um token (sem o segredo). Só com o token raiz. |
| `PATCH` | `/tokens/{id}` | Altera `name` e `expiresAt`. Permissões e prefixos são imutáveis: para mudar o alcance, crie outro token e revogue este. Só com o token raiz. |
| `DELETE` | `/tokens/{id}` | **Revoga** o token: vale em todos os nós no commit. Só com o token raiz. |

**Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`):
- de 1 a 100 caracteres na criação; nome terminado no sufixo de dead-letter (`-dead-letter`) é reservado e rejeitado com `400`. A fila de dead-letter, de nome `{fila}` + sufixo, pode passar de 100 (até 112 com o sufixo padrão), e o `{topic}` das rotas a aceita;
- apenas letras ASCII (`a-z`, `A-Z`), dígitos (`0-9`), hífen (`-`) e sublinhado (`_`) — regex `^[A-Za-z0-9_-]{1,100}$`;
- diferencia maiúsculas de minúsculas (`Pedidos` e `pedidos` são tópicos distintos);
- nomes inválidos são rejeitados na criação (`POST /topics`) com erro `400`.

**Tags do tópico** (`tags`): pares chave/valor definidos na criação (`POST /topics`) e alterados via `PATCH /topics/{topic}`. Servem para monitoramento: são enviadas como **atributos** nos traces e logs exportados via **OpenTelemetry** para as operações daquele tópico, e nas métricas de tópico conforme a configuração de exportação ([11-observabilidade.md](11-observabilidade.md)) (ex.: `{"time": "financeiro", "ambiente": "prod"}`).
- chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL);
- máximo de 20 tags por tópico;
- as tags não afetam o comportamento das mensagens, apenas a telemetria.

### 8.1.2 Paginação

Toda rota de **cliente ou de operação** que pode devolver um número ilimitado de itens é paginada, sempre com a mesma convenção:

| Parâmetro | Descrição |
|---|---|
| `limit` | Itens por página. Tem padrão e máximo (a calibrar, [10-configuracao.md](10-configuracao.md)). |
| `cursor` | Token **opaco** devolvido em `nextCursor` pela página anterior. O cliente nunca o monta. |
| `sort` | Campo de ordenação, prefixo `-` para decrescente. Só nas rotas que o aceitam (hoje, `GET /topics` e `GET /tokens`), e só com campos imutáveis. Sem `sort`, a ordem é a do nome. |

A resposta traz os itens e `nextCursor`; sem `nextCursor`, acabou. Não há `page`/`offset` nem `total` (contar custa caro e o valor muda durante a leitura).

**Para não perder nem repetir itens entre páginas**, a paginação é por **chave** (*keyset*), não por deslocamento:

- A ordem é sempre **total e estável**: toda ordenação termina num campo único e imutável como desempate (o nome ou o `id`). `sort=-createdAt` vale, na prática, `(createdAt desc, id asc)`.
- O cursor guarda a **última chave lida**, mais o `sort` e os filtros em uso. A página seguinte pede "depois dessa chave", então inserir ou remover itens antes dela não desloca nada. Cursor com `sort` ou filtros diferentes dos atuais é rejeitado com `400`.
- Repetir um cursor devolve a mesma página enquanto os dados não mudarem, e o cursor **não expira** (não há estado no servidor).

A garantia é a de quem percorre tudo: recebe todo item que existia durante o percurso inteiro, sem repetição. Itens criados ou removidos durante o percurso podem ou não aparecer. É consistência fraca, suficiente para consulta; as rotas entre nós usam outro mecanismo (seção 8.2.1, [06c-api-cluster.md](06c-api-cluster.md)).

Por isso nenhuma rota ordena por valor que muda: `GET /stats/topics` não tem `sort` e pagina sempre por nome.

### 8.1.3 Autenticação por token

A autenticação da API de cliente é **opcional e liga sozinha quando existe um token raiz**: o arquivo `root-token` na pasta de segredos, `auth.root-token-file` ou `auth.root-token` (veja "Token raiz" abaixo). Não há variável de ligar. Para manter a pasta montada (por causa do TLS do cluster) e ainda assim **não** exigir token, `auth.enabled=false` força a autenticação desligada ([10-configuracao.md](10-configuracao.md)). Sem token raiz, nenhuma rota de cliente exige nada. No **modo desenvolvimento** ela nunca é exigida, e as rotas `/tokens` respondem `404`. Ligada, toda rota da seção 8.1 exige `Authorization: Bearer <token>`. As rotas de operação (seção 8.1.1, [06b-api-operacao.md](06b-api-operacao.md)) e as `/cluster/*` ([06c-api-cluster.md](06c-api-cluster.md)) não usam token: têm proteção própria ([06-api.md](06-api.md), seção 8.0). O token trafega em claro se o balanceador não terminar o TLS, então use HTTPS na frente da API de cliente.

**Token de acesso.** Credencial opaca no formato `tb_<id>.<segredo>`, em que `id` é um GUID v7 e o segredo tem 256 bits aleatórios. O cluster guarda apenas o **SHA-256 do segredo**; o segredo completo só aparece na resposta da criação e não pode ser recuperado depois. O `id` do prefixo permite achar o token direto e identificá-lo em logs e traces **sem expor o segredo** (o token inteiro nunca é registrado). Como cada nó conhece todos os tokens, validar é uma consulta local em memória, sem chamada de rede.

**Permissões** (`permissions`, ao menos uma, combináveis):

| Permissão | Permite |
|---|---|
| `publish` | `POST /topics/{topic}/messages`. |
| `consume` | `GET`, `PATCH` (invisibilidade) e `DELETE` (ack) em `/topics/{topic}/messages`. |
| `create` | `POST /topics`, e o `autoCreate` do `GET` (que exige também `consume`). **Só cria**: alterar e remover tópicos é `admin`. |
| `admin` | `PATCH` e `DELETE` em `/topics/{topic}`. **Inclui `create`**, então um token só com `admin` também cria tópicos. |

Todo token válido consulta `GET /topics` e `GET /topics/{topic}` dentro dos seus prefixos. Exemplos de combinação: `publish` + `create` é um produtor que cria as próprias filas, e `consume` + `create`, um consumidor que usa `autoCreate`.

**Prefixos de tópico** (`topicPrefixes`, opcional): o token só alcança tópicos cujo nome **começa com** algum dos prefixos, o mesmo critério do Tópico Distribuidor ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). Sem prefixos, alcança todos. Até 20 prefixos de 1 a 100 caracteres (`A-Za-z0-9_-`).

- `GET /topics` devolve só os tópicos dentro dos prefixos do token, e um tópico fora deles responde `403`.
- `POST /topics` com `create` só aceita um nome dentro dos prefixos, e, num Tópico Distribuidor, também cada um dos seus `prefixes` de distribuição: do contrário, um token restrito poderia injetar mensagens em filas fora do seu alcance.
- A **fila de dead-letter** é alcançada pelo mesmo prefixo da fila de origem, pois o nome dela começa com o da fila.
- Na publicação num **Tópico Distribuidor**, a permissão é conferida **só no distribuidor**, e as cópias nas filas destino não são conferidas de novo.

**Token raiz.** `auth.root-token` (ou `auth.root-token-file`) define um segredo, igual em todos os nós, que tem **acesso total**: todas as permissões, sem limite de prefixos, e é o **único** que gerencia os tokens (`/tokens`). Como vem da configuração e não da tabela de tokens, existe desde o primeiro instante, inclusive num nó que acabou de entrar. Por isso **não há um primeiro admin para criar**: o operador começa a trabalhar com o raiz e cria tokens de alcance menor para as aplicações, guardando o raiz num cofre. Nos logs e traces ele aparece como `token.id=root`.

- **Geração:** o `taba init` já cria o arquivo `root-token` na pasta de segredos ([06-api.md](06-api.md), seção 8.0.1); `taba token generate` imprime um segredo aleatório de 256 bits avulso. O token raiz precisa ter **ao menos 32 caracteres**, senão o nó não sobe. O padrão de `auth.root-token-file` é `<secrets.dir>/root-token`.
- **Troca:** o `auth.root-token-file` é relido a cada `auth.reload-interval` e aceita **duas linhas** (a atual e a anterior), para trocar o segredo sem reiniciar nenhum nó. Os tokens já criados seguem valendo, pois o raiz não os assina.
- `auth.enabled=true` sem token raiz impede o nó de subir. Um `auth.root-token-file` definido que não existe também.

**Primeiros passos:**

```
taba init --out ./secrets        # certificados do cluster + root-token
# em todos os nós: montar ./secrets em /etc/taba/secrets (a autenticação liga sozinha)
curl -H "Authorization: Bearer $(cat secrets/root-token)" -X POST https://taba.exemplo/topics -d '{"name":"pedidos","type":"queue"}'
```

**Criar um token:**

```json
POST /tokens
{
  "name": "servico-pedidos",
  "permissions": ["publish", "create"],
  "topicPrefixes": ["pedidos"],
  "expiresAt": "2027-06-01T00:00:00Z"
}
```

```json
201 Created
{
  "id": "01927f3a-...",
  "token": "tb_01927f3a-....<segredo>",
  "name": "servico-pedidos",
  "permissions": ["publish", "create"],
  "topicPrefixes": ["pedidos"],
  "createdAt": "2026-10-06T12:00:00Z",
  "expiresAt": "2027-06-01T00:00:00Z"
}
```

**Validade.** `expiresAt` é opcional e absoluto; sem ele, o token vale até ser revogado. Cada nó ignora o token vencido e o apaga na varredura local (`retention.sweep.interval`), sem propagar nada, como no `ttl` das mensagens (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). Depende do relógio dos nós (NTP).

**Respostas.**

| Situação | Resposta |
|---|---|
| Sem token, mal formado, desconhecido, revogado ou vencido | `401` com `WWW-Authenticate: Bearer`. A resposta não diz qual dos casos. |
| Token válido sem a permissão da rota, ou tópico fora dos prefixos | `403`. |

**Criação e revogação** usam o mesmo registro + commit de **todos** os nós da definição de tópico (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)): com algum nó fora do ar, a operação falha e o cliente tenta de novo. A revogação vale em todos os nós no commit. A conferência acontece no nó que recebe a chamada e, numa chamada repassada (seção 6.9 de [04-cluster.md](04-cluster.md)), de novo no destino, pois o cabeçalho `Authorization` segue como veio.
