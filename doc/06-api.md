# API

[← Índice](README.md)

## 8. API (rascunho inicial)
 
> Contrato formal (OpenAPI) ainda não definido — esta seção lista os verbos e recursos previstos até o momento.

### 8.0 Portas: cliente, cluster e operação

O nó atende em **portas separadas, uma por público**. Assim cada porta é liberada ou bloqueada na rede (firewall, grupo de segurança, `NetworkPolicy`) sem olhar o caminho da requisição, e restringir a API interna não depende de filtro na aplicação.

| Porta | Parâmetro | Padrão | Quem acessa | O que serve |
|---|---|---|---|---|
| **API de cliente** | `api.port` | A definir (sugestão: 8080) | Aplicações clientes, em geral pelo balanceador | As rotas da seção 8.1. Sem `operations.port`, também as rotas de operação (seção 8.1.1). |
| **Cluster** | `cluster.port` | A definir (sugestão: 8081) | **Só os outros nós** (mais o balanceador interno do bootstrap, abaixo) | As rotas `/cluster/*` da seção 8.2 e o canal WebSocket `/cluster/channel`. |
| **Operação** (opcional) | `operations.port` | Vazia (usa `api.port`) | Prometheus, orquestrador, operadores | `/metrics`, `/stats/*`, `/health/*` e `/node/drain` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)). |

- **Separação por porta, não só por caminho.** São listeners diferentes do mesmo processo. A porta de cliente responde `404` a `/cluster/*`; a porta do cluster responde `404` a tudo que não seja `/cluster/*`. As três portas precisam ser diferentes entre si; valores repetidos, ou uma porta ocupada, impedem o nó de subir (a mesma falha explícita de `operations.port`).
- **A tabela de membros guarda o IP e a `cluster.port` de cada nó** (seção 6.2 de [04-cluster.md](04-cluster.md)). Toda chamada entre nós vai **direto** ao IP e à porta do cluster do par, sem balanceador no caminho (seção 8.2.1). A única chamada que passa por um balanceador é a primeira do bootstrap, `GET /cluster/nodes`, e por isso `cluster.entry-address` é o endereço da **porta do cluster** do balanceador.
- **Dois listeners no balanceador.** Um para `api.port`, o do tráfego de clientes. Outro, **interno**, para `cluster.port`, usado só no bootstrap; em Kubernetes pode ser um `Service` interno (ou *headless*) em vez de um balanceador. A porta do cluster **nunca** é publicada no balanceador de clientes, no `-p` do Docker nem fora da rede dos nós.
- **A porta do cluster não tem rate limit nem leitura de IP do cliente.** O tráfego é entre nós, e os cabeçalhos de `client-ip.headers` são ignorados nela ([12-limites-api.md](12-limites-api.md)). Quem repassa uma chamada de cliente a outro nó leva o IP já resolvido em cabeçalho interno (seção 6.9 de [04-cluster.md](04-cluster.md)).
- **Autenticação:** a porta do cluster usa TLS mútuo com um certificado único do cluster (seção 8.0.1). A API de cliente tem **autenticação opcional por token** (seção 8.1.3). As rotas de operação **não têm autenticação**; a proteção delas é de rede.
- **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a porta do cluster **não é aberta** e `cluster.port` é ignorado.

#### 8.0.1 Segurança da porta do cluster

A comunicação entre nós usa **TLS mútuo com certificados do cluster**, gerados pelo operador e lidos de **uma pasta**. Não há CA nem certificado por nó, e a confiança é a dos próprios certificados (*pinning*). A pasta pode ter **vários certificados ao mesmo tempo**, cada um com a sua data de início e de expiração, e o nó a **recarrega sozinho**: trocar o certificado não exige reiniciar nenhum nó.

- **Geração.** `taba init --out <pasta>`, da própria imagem, cria de uma vez a **pasta de segredos**: o primeiro certificado (em `tls/`) e o token raiz (`root-token`, seção 8.1.3). Para gerar só um certificado, como numa rotação, use o comando `taba cert generate --days <n> [--starts-in <duração>] --out <pasta>`, da própria imagem, cria um par `cluster-<data de início>.crt` e `.key` (o nome traz a data, para gerações sucessivas não se sobrescreverem), autoassinado e válido para servidor e cliente. `--starts-in` define o `notBefore` no futuro (padrão: agora, com uma folga de alguns minutos para diferença de relógio). O operador o roda quando precisa de um certificado novo, **não a cada início de container**, e entrega a pasta a todos os nós como segredo da plataforma (Secret do Kubernetes, Docker secret ou volume).
- **Configuração.** A pasta é `cluster.tls.dir`, cujo padrão é `<secrets.dir>/tls` (`secrets.dir` vale `/etc/taba/secrets`); `cluster.tls.reload-interval` define de quanto em quanto tempo ela é conferida ([10-configuracao.md](10-configuracao.md)). Basta **montar a pasta de segredos** no nó: o TLS liga **sozinho** quando a pasta `tls` existe, sem variável de ligar, e a porta do cluster passa a exigir TLS. Cada par é um `<nome>.crt` mais um `<nome>.key` de mesmo nome; um `.crt` sem `.key` serve só para confiar.
- **Quem o nó aceita.** Quem apresentar um certificado igual a um da pasta, ou emitido por um deles, desde que ele **ainda não tenha expirado** (`notAfter`). A data de início não conta para a confiança, para um nó aceitar o certificado novo antes de qualquer um passar a apresentá-lo.
- **Qual o nó apresenta.** O par de **início mais recente que já começou a valer** (`notBefore` ≤ agora) e ainda não expirou. Isso vale ao **receber** e ao **abrir** uma conexão (chamadas HTTP e o WebSocket `/cluster/channel`). Cada lado exige que o outro apresente um dos certificados aceitos; sem isso o handshake falha e nenhuma rota é alcançada. Os nós se conectam por **IP** (o da tabela de membros), então o nome do host **não é verificado**.
- **Sem a pasta** (o padrão não existe e `cluster.tls.dir` não foi definido), a porta do cluster atende em HTTP puro e o nó registra um aviso (`warn`) no log. Com a pasta existente, o nó **não sobe** se não houver nela ao menos um par válido; e um `cluster.tls.dir` definido que não existe também impede a subida. Em `mode=development` a porta não é aberta e o TLS é ignorado.
- **Balanceador.** O listener da porta do cluster no balanceador precisa **repassar o TLS sem abri-lo** (TCP/L4, como um NLB), ou o `cluster.entry-address` aponta para um nome DNS que resolve direto para os nós (por exemplo, um `Service` *headless*). Um balanceador que **termina** o TLS (como um ALB) perderia o certificado do cliente e derrubaria a autenticação. Só o bootstrap passa por ele (seção 8.0). O TLS da API de cliente (ex.: certificado do ACM no ALB) é do balanceador e independe disto.
- **Rotação, sem reiniciar nenhum nó:**
  1. Gerar o certificado novo com início no futuro: `taba cert generate --days 1825 --starts-in 14d --out <pasta>`.
  2. Acrescentar o par novo à pasta (por exemplo, nova entrada no Secret). Cada nó o percebe em até `cluster.tls.reload-interval` e passa a **confiar** nele, mas segue apresentando o antigo.
  3. Quando a data de início chega, **todos os nós passam a apresentar o novo sozinhos**, sem nenhuma ação.
  4. O antigo deixa de ser aceito quando expira; o operador pode retirá-lo da pasta antes.

  O prazo de `--starts-in` precisa ser maior que o tempo de o par novo chegar a **todos** os nós (a propagação do Secret e o `reload-interval`): um nó que ainda não tem o novo, quando a data chega, passa a rejeitar quem o apresenta. Os relógios dos nós devem estar em NTP (seção 13.3 de [14-invisibilidade.md](14-invisibilidade.md)). Conexões já abertas, como o canal WebSocket, seguem com o certificado antigo até caírem, o que é inofensivo porque ele continua aceito até expirar.
- **Recarga.** Se uma recarga encontra arquivo ilegível, par incompleto ou certificado inválido, o nó **mantém o conjunto anterior** e registra `error` no log. O conjunto só muda quando a pasta inteira é válida.
- **Expiração.** Um certificado expirado deixa de ser aceito e de ser apresentado. Para não chegar lá, o nó registra `warn` no log a partir de `cluster.tls.expiry-warning` (padrão 30 dias) antes do `notAfter` do certificado em uso, e expõe a métrica `msg.cluster.tls.expires_in` ([11-observabilidade.md](11-observabilidade.md)), para alertar no Prometheus.
- **Limite aceito.** Quem obtiver a chave de **um** nó pode se passar por qualquer nó, o mesmo nível de risco de um segredo compartilhado, mas com tráfego cifrado. Quem quiser um certificado por nó, emitido por uma CA (cert-manager, malha de serviço), pode colocar na pasta o `.crt` da CA para confiar nela e o par do próprio nó para apresentar.

**Rotas de operação:** `/metrics`, `/stats/*`, `/health/*` e `/node/drain` ficam **sem autenticação**, por decisão. Quem instala protege com a porta própria (`operations.port`) e com a rede (grupo de segurança, `NetworkPolicy`) e, em complemento, com `operations.allowed-ips` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

**Exemplo com Docker.** Uma vez, o operador cria a pasta de segredos e a entrega a todos os nós:

```bash
docker run --rm -v "$PWD/secrets:/out" taba init --out /out
```

Cada nó monta a pasta e informa o endereço do cluster (`--stop-timeout` deve ser maior que `drain.timeout`; a porta do cluster e a de operação ficam presas à interface privada do host):

```bash
docker run -d --name taba-1 --stop-timeout 30 \
  -p 8080:8080 -p 10.0.0.5:8081:8081 -p 10.0.0.5:9090:9090 \
  -v taba-data:/var/lib/taba \
  -v "$PWD/secrets:/etc/taba/secrets:ro" \
  -e TABA_CLUSTER_ENTRY_ADDRESS=taba-interno.exemplo:8081 \
  taba
```

No primeiro nó de um cluster novo, acrescente `-e TABA_CLUSTER_FIRST_NODE=true`. No modo desenvolvimento basta `-e TABA_MODE=development` e a porta `8080`, sem pasta de segredos.

### 8.1 API de cliente

> Porta: `api.port` (seção 8.0).
 
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
 
### 8.1.1 API de operação (observabilidade)

Rotas para operadores, não para aplicações clientes. Detalhes, parâmetros e métricas em [11-observabilidade.md](11-observabilidade.md). As rotas `/node/drain` valem só para o nó que recebe a chamada, por isso o operador as chama no nó, e não pelo balanceador. Opcionalmente podem ser servidas em **porta própria** e/ou restritas a uma **lista de IPs/faixas** (`operations.port`, `operations.allowed-ips`); sem essa configuração, ficam na porta da API de cliente e são tratadas como qualquer outra rota ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics` | Métricas do cluster inteiro (padrão, uma série por nó) ou só deste nó (`scope=node`), no formato Prometheus. Alvo único de scrape: o balanceador. Lê só uma cópia em memória, atualizada por uma thread (seção "Endpoint Prometheus" de [11-observabilidade.md](11-observabilidade.md)). |
| `GET` | `/stats/node` | Métricas do nó que atende a chamada (leitura local), incluindo `mode` e `drainState`. |
| `POST` | `/node/drain` | Põe **este nó** em modo drain: sai da rotação (`/health/ready` `503`), grava todas as mensagens em disco, repassa as chamadas de cliente ao nó mais livre e **sai do cluster por desregistro voluntário**, sem votação, sem encerrar o processo ([04-cluster.md](04-cluster.md), seção 6.10). Responde `202`, e o andamento fica em `GET /node/drain`. Já em drain, responde `200` sem efeito. |
| `GET` | `/node/drain` | Estado do drain deste nó: `drainState` (`active`, `draining`, `drained`), início, quem o disparou (`signal` ou `api`) e mensagens ainda em memória. |
| `DELETE` | `/node/drain` | Antes da saída, cancela o drain (`200`, o nó segue no cluster). Depois da saída (`drained`), o nó **volta como nó novo**, com ID novo e bootstrap completo (`202`). `409` durante o desregistro ou se o processo já está parando por sinal. |
| `GET` | `/stats/cluster` | Métricas de todos os nós e condição geral do cluster (consulta todos os nós, *best-effort*). |
| `GET` | `/stats/topics` | Métricas de tópicos, **paginada** (`limit`, `cursor`, `tag.*`, `condition`, `fields`, `groupBy`, `scope`), **sem `sort`**: a ordem é sempre por nome do tópico. Visão do cluster por padrão; `scope=node` devolve só os valores do nó. |
| `GET` | `/stats/topics/{topic}/groups` | Métricas por `groupId` de um tópico com grupos: mensagens (prontas e invisíveis), parcela, mais antiga e taxas, com `groupId`, `top` e `scope` ([11-observabilidade.md](11-observabilidade.md), "Métricas por grupo"). |
| `GET` | `/stats/topics/{topic}` | Métricas detalhadas e condição de um tópico. |
| `GET` | `/health/live` | O processo está de pé. |
| `GET` | `/health/ready` | O nó está apto a receber tráfego (registrado no cluster e com toda a sincronização concluída: tópicos, blocos e banco de eventos; seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)). |

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

A garantia é a de quem percorre tudo: recebe todo item que existia durante o percurso inteiro, sem repetição. Itens criados ou removidos durante o percurso podem ou não aparecer. É consistência fraca, suficiente para consulta; as rotas entre nós usam outro mecanismo (seção 8.2.1).

Por isso nenhuma rota ordena por valor que muda: `GET /stats/topics` não tem `sort` e pagina sempre por nome.

### 8.1.3 Autenticação por token

A autenticação da API de cliente é **opcional e liga sozinha quando existe um token raiz**: o arquivo `root-token` na pasta de segredos, `auth.root-token-file` ou `auth.root-token` (veja "Token raiz" abaixo). Não há variável de ligar. Para manter a pasta montada (por causa do TLS do cluster) e ainda assim **não** exigir token, `auth.enabled=false` força a autenticação desligada ([10-configuracao.md](10-configuracao.md)). Sem token raiz, nenhuma rota de cliente exige nada. No **modo desenvolvimento** ela nunca é exigida, e as rotas `/tokens` respondem `404`. Ligada, toda rota da seção 8.1 exige `Authorization: Bearer <token>`. As rotas de operação (seção 8.1.1) e as `/cluster/*` não usam token: têm proteção própria (seção 8.0). O token trafega em claro se o balanceador não terminar o TLS, então use HTTPS na frente da API de cliente.

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

- **Geração:** o `taba init` já cria o arquivo `root-token` na pasta de segredos (seção 8.0.1); `taba token generate` imprime um segredo aleatório de 256 bits avulso. O token raiz precisa ter **ao menos 32 caracteres**, senão o nó não sobe. O padrão de `auth.root-token-file` é `<secrets.dir>/root-token`.
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

### 8.2 API interna (cluster)

> **Porta:** todas as rotas desta seção ficam na **porta do cluster** (`cluster.port`, seção 8.0) e nunca na da API de cliente, que responde `404` a `/cluster/*`. Só os outros nós devem alcançá-la.

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a porta do cluster **não é aberta**, então nenhuma rota `/cluster/*` responde. Um nó em modo produção não consegue ingressar num nó em desenvolvimento.

> Cada rota `rollback` só descarta um registro (fase 1) **pendente, sem commit** — falha ou timeout de retries antes de fechar o quórum. Uma falha na chamada de `commit` (fase 2) numa réplica fora do quórum, já decidido, nunca aciona `rollback`: o coordenador insiste no commit dentro do teto `write.commit.timeout` até confirmar em todas as réplicas ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).
>
> **Mensagens (rotas abaixo com `messages/{id}`, `visibility` e `ack`) decidem o commit por quórum** (`write.quorum`, sempre incluindo o primário do bloco) — a chamada de `register` ainda vai a todas as réplicas, mas o `commit` é decidido e respondido ao cliente assim que o quórum confirma, sem esperar as demais. **Nó e tópico (`nodes`, `topics`) continuam exigindo confirmação de todos os nós**, sem quórum reduzido (seção 7.2).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/cluster/nodes` | Lista a tabela de membros (ID, IPs internos, porta do cluster, estado). É a primeira chamada de um nó ao iniciar, feita pelo endereço do balanceador (`cluster.entry-address`, que aponta para a porta do cluster); qualquer nó a responde. É a **única** rota desta seção que passa por balanceador. |
| `POST` | `/cluster/nodes/join` | Pedido de ingresso do nó novo, com seus IPs e a porta do cluster, feito a um membro da lista. O membro testa os IPs (conectando à porta do cluster informada) e coordena o registro + commit (seção 6.2 de [04-cluster.md](04-cluster.md)); responde com a tabela de nós. |
| `POST` | `/cluster/nodes/register` | Fase 1 (registro) de um novo nó: propaga os dados do novo nó (IP e porta do cluster) a todos os nós. |
| `POST` | `/cluster/nodes/unregister` | Fase 1 (registro) do **desregistro voluntário** de um nó em drain, enviada por ele a todos os nós; sem votação. O commit usa `/cluster/nodes/commit` ([04-cluster.md](04-cluster.md), seção 6.10). |
| `POST` | `/cluster/nodes/commit` | Fase 2 (commit) de registro **ou** desregistro de nó: efetiva a operação na tabela de membros de todos os nós. |
| `POST` | `/cluster/topics/register` | Fase 1 (registro) de uma criação, alteração ou remoção de tópico: propaga a definição a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `POST` | `/cluster/topics/commit` | Fase 2 (commit): efetiva a definição na tabela de tópicos de todos os nós, e cada um grava/atualiza/remove o `${topico}.json` local. |
| `POST` | `/cluster/topics/rollback` | Descarta a alteração de tópico pendente (sem commit) após esgotar os retries. |
| `POST` | `/cluster/tokens/register` | Fase 1 (registro) da criação, alteração ou revogação de um token de acesso: propaga o registro (com o hash do segredo, nunca o segredo) a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.4). |
| `POST` | `/cluster/tokens/commit` | Fase 2 (commit): efetiva o token na tabela de todos os nós, e cada um grava, atualiza ou apaga o `_tokens/${id}.json` local. |
| `POST` | `/cluster/tokens/rollback` | Descarta a alteração de token pendente (sem commit) após esgotar os retries. |
| `GET` | `/cluster/tokens` | **Stream** (NDJSON, seção 8.2.1) com os tokens que o nó possui, ordenados por `id`; `from`/`to` opcionais. Usado pelo nó novo, junto com `/cluster/topics`, no bootstrap. |
| `GET` | `/cluster/topics/{id}/idle` | Usado pelo **nó gestor** de uma fila volátil: devolve há quanto tempo foi o último `GET` do tópico neste nó (duração decorrida; sem registro, o tempo desde que o nó subiu). Leitura local, sem estado pendente ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5). |
| `POST` | `/cluster/vote-remove` | Usado para votar a remoção de um nó suspeito de falha (51% dos nós saudáveis, com mínimo de votantes — aberta quando o canal com o nó fica fora após as tentativas de reconexão, antes do commit de desregistro). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/register` | Fase 1 (registro) de uma mensagem, enviado ao(s) nó(s) responsável(is) pelo bloco (primário/réplicas). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/commit` | Fase 2 (commit) da entrega de uma mensagem já registrada nos nós responsáveis; decidido assim que o quórum confirma (primário incluso), enviado às demais réplicas em paralelo/segundo plano. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/rollback` | Descarta o registro pendente (sem commit) de uma mensagem cujo registro falhou após os retries. |
| `GET` | `/cluster/channel` | Abre (upgrade para **WebSocket**) o **canal permanente** entre dois nós, aberto pelo nó que entra, volta ou detecta primeiro a queda; em conexão dupla, o nó de menor hash do ID fecha a que abriu (um por par, para todos os tópicos): carrega o **sinal de vida** que substitui o ping (seção 6.3) e os sinais de disponibilidade da entrega — tópicos com ouvintes, tópicos com mensagens prontas e avisos de mensagem nova (seção 12.8). Só sinais, nunca conteúdo de mensagens nem listas longas. |
| `POST` | `/cluster/topics/{topic}/messages/take` | Pede ao nó dono que entregue até `maxMessages` mensagens prontas do tópico (a quantidade que ainda falta ao `GET` do cliente), com `visibilityTimeout`: ele faz o registro + commit de invisibilidade e devolve as mensagens ao nó que pediu (seção 12.8). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/register` | Fase 1 (registro) de uma entrega ou de uma alteração de invisibilidade: grava como pendente o novo `invisibleUntil` (e o `deliveryCount`, se for entrega) nos nós responsáveis. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/commit` | Fase 2 (commit): efetiva a invisibilidade; no caso de entrega, o nó também remove o ID da sua lista de prontos. Decidido por quórum (primário incluso) — o primário sempre recebe essa chamada antes da resposta ao cliente. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/rollback` | Descarta a alteração de invisibilidade pendente após esgotar os retries. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/register` | Fase 1 (registro) do ack: pedido de remoção da mensagem a todos os nós responsáveis pelo bloco. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/commit` | Fase 2 (commit): remove a mensagem (e seus metadados); decidido por quórum (primário incluso), replicado às demais réplicas em segundo plano. Sem tombstone: a ausência do ID já basta para a sincronização (seção 7.3). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/rollback` | Descarta o registro de ack pendente após esgotar os retries; a mensagem permanece disponível. |
| `GET` | `/cluster/topics` | **Stream** (NDJSON, seção 8.2.1) com as definições de tópico que o nó possui, ordenadas por `id`; `from`/`to` opcionais. Usado pelo nó novo para montar a tabela de tópicos depois do registro, inclusive na verificação final (seção 3.3 de [02-arquitetura.md](02-arquitetura.md)). |
| `GET` | `/cluster/metrics` | Métricas **locais** do nó, no formato Prometheus. É o que a thread de atualização das métricas do cluster consulta em cada nó (seção "Endpoint Prometheus" de [11-observabilidade.md](11-observabilidade.md)). |
| `GET` | `/cluster/blocks/{id}/ids` | **Stream** (NDJSON) com os IDs de mensagens que o nó possui para o bloco, cada um com o nome do tópico, ordenados por ID; `from`/`to` opcionais. Usado na sincronização distribuída, inclusive na verificação final (seção 7.3). |
| `GET` | `/cluster/blocks/{id}/messages` | **Stream** (NDJSON) com as mensagens do bloco que o nó possui na faixa `from`–`to` (IDs, inclusivos). É a chamada balanceada da sincronização: o nó novo divide a lista unida de IDs em faixas e pede cada faixa a um nó de origem. |
| `GET` | `/cluster/blocks/{id}/messages/{messageId}` | Retorna o conteúdo de **uma** mensagem do bloco. Mantida por consistência REST e para análise individual quando há problema com uma mensagem; também serve de reforço para uma mensagem que faltou numa faixa. |
| `GET` | `/cluster/message-loss` | **Stream** (NDJSON) com a tabela `perdas` do nó: os pares (`topic`, `id`) declarados perdidos na sincronização, ordenados por ID; `from`/`to` opcionais. Consultada em todos os nós por um nó que retorna (seção 6.6). Aceita chamadas de nó que ainda não é membro. |
| `POST` | `/cluster/messages/recover` | Usado por um nó que retorna ao cluster (seção 6.6) para enviar ao responsável pelo bloco as mensagens que encontrou na lista de perdas (corpo e `deliveryCount`). O **corpo da requisição** é um stream NDJSON (seção 8.2.1). O nó que recebe grava **direto nos nós responsáveis pelo bloco**, idempotente pelo ID, sem fase de registro pendente nem rollback; a linha conta como aplicada quando `write.quorum` nós confirmam. Quem aceita apaga o registro de perda e avisa os demais nós. Aceita chamadas de nó que ainda não é membro. |
| `*` | `/cluster/forward/{caminho}` | **Repasse** de uma chamada de cliente a outro nó: de quem está sincronizando ou em drain (seção 6.9 de [04-cluster.md](04-cluster.md)) e, na fila FIFO, da publicação ao primário do grupo (seção 16.4 de [17-topicos-fifo.md](17-topicos-fifo.md)). `{caminho}` é o caminho original da API de cliente (ex.: `/cluster/forward/topics/pedidos/messages`); método, query, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu. Com a autenticação ligada, o destino **confere o token de novo** (o `Authorization` segue como veio; um nó em sincronização ainda não tem a tabela de tokens). Aceita só de membros da tabela e de nós que saíram por drain; a própria rota é o marcador de repasse, e quem a recebe nunca repassa de novo. |
| `GET` | `/cluster/stats/node` | Métricas locais do nó, para a agregação de `/stats/cluster`. |
| `GET` | `/cluster/stats/topics` | Contadores locais por tópico (apenas mensagens em que o nó é primário: visíveis, invisíveis e data da mais antiga), agregados por quem atende `/stats/topics` (soma das quantidades, menor data). |

### 8.2.1 Rotas de sincronização: streams NDJSON

As rotas entre nós que podem mover volume grande (`/cluster/topics`, `/cluster/tokens`, `/cluster/blocks/{id}/ids`, `/cluster/blocks/{id}/messages` e o corpo do `/cluster/messages/recover`) **não usam a paginação do cliente** (seção 8.1.2). Quem sincroniza precisa saber que recebeu **tudo**, e a conexão é sempre direta entre dois nós, sem balanceador no caminho. O contrato é:

- **Formato:** `Content-Type: application/x-ndjson`, **um objeto JSON por linha**. Emissor e receptor trabalham com um buffer por linha, sem montar a resposta inteira em memória. O tamanho máximo de uma linha é limitado (`cluster.sync.stream.max-line-bytes`).
- **Ordem:** por **ID ascendente** (GUID v7, comparado como texto na forma canônica em minúsculas), que acompanha a ordem de criação. Mensagem e definição de tópico não mudam o `id`.
- **Faixa:** `from` e `to` (ambos **inclusivos**, ambos opcionais) delimitam os IDs pedidos. `limit` opcional reduz o teto de itens do stream.
- **Linha final obrigatória:** o stream termina com `{"end":true,"count":N,"last":"<id>","hasMore":false}`. **Sem essa linha o trecho é considerado falho**, mesmo que a conexão tenha fechado limpa: um corte entre duas linhas não se distingue de um fim normal sem ela.
- **Teto por stream:** o emissor fecha o stream ao atingir `cluster.sync.stream.max-items` ou `cluster.sync.stream.max-bytes`, com `hasMore: true`. O receptor abre outro com `from=<last>`.
- **Retomada:** depois de um corte ou de `hasMore`, o receptor repete a chamada com `from=<último ID lido>`. O emissor devolve de novo essa linha, e o receptor a **ignora** (aplicação idempotente por ID). Não há token nem estado no servidor, e a repetição devolve as mesmas linhas porque a mensagem não muda depois de gravada.
- **Erro depois do `200`:** o status já foi enviado, então o emissor escreve `{"error":"..."}` e fecha, ou apenas fecha sem a linha final. Nos dois casos o receptor retoma pela regra acima.
- **Timeout:** vale um **timeout de leitura por linha** (`cluster.sync.stream.idle-timeout`), não um teto para o stream inteiro. Não há *heartbeat*: sem balanceador, só um nó parado ou lento fica sem escrever.
- **Contrapressão:** o receptor lê no próprio ritmo e o TCP segura o emissor. O emissor não mantém transação nem lista congelada aberta; ele lê o armazenamento em blocos pequenos.
- **Corpo de `POST`** (`/cluster/messages/recover`): o mesmo formato, com a mesma linha final no corpo. A resposta também termina com `{"end":true,"count":N}`, com a quantidade aplicada. Se a conexão cair, quem envia reenvia tudo; mensagens já aplicadas são ignoradas.

Cada rota usa o mesmo formato com linhas diferentes:

| Rota | Linha de dados |
|---|---|
| `/cluster/topics` | A definição do tópico (a mesma de `_topics/${topico}.json`). |
| `/cluster/tokens` | O token (o mesmo de `_tokens/${id}.json`, com o hash do segredo). |
| `/cluster/blocks/{id}/ids` | `{"id":"<id>","topic":"<topico>"}`; na fila FIFO, a linha de uma mensagem com exclusão lógica traz também `"deleteAt"` (seção 16.7 de [17-topicos-fifo.md](17-topicos-fifo.md)). |
| `/cluster/message-loss` | `{"id":"<id>","topic":"<topico>"}` |
| `/cluster/blocks/{id}/messages` | A mensagem completa (`id`, conteúdo e metadados). Na fila FIFO, a mensagem com exclusão lógica vem **sem conteúdo** e com `"deleteAt"`. |

Quando faltar mensagem numa faixa (o `count` é menor que os IDs pedidos), o receptor busca só as que faltaram em outro nó de origem, e a rota por ID único serve de último recurso. Se nenhum nó a tem, vale o evento de perda da seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md), registrado no log com o tópico e o ID.

Esse contrato **não cria tombstone nem log novo**: a remoção continua por ausência do ID, e a janela residual da seção 7.3 continua tratada como lá.

