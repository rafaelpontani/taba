# API

[← Índice](README.md)

## 8. API (rascunho inicial)
 
> Contrato formal (OpenAPI) ainda não definido — esta seção lista os verbos e recursos previstos até o momento.
 
### 8.1 API de cliente
 
| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/topics` | Cria um tópico (nome, tipo `queue` ou `distributor`, e opcionalmente `tags`; `prefixes` para `distributor`; `volatile` e `idleTimeout` para `queue`). Gera o `id` (GUID v7) do tópico. Propagado a todos os nós por registro + commit; persistido em disco ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `GET` | `/topics` | Lista os tópicos existentes. Respondida **localmente**, sem consultar outros nós: a definição do tópico é replicada em todo nó (seção 3.3), diferente de `/stats/topics`, que faz fan-out para métricas de consumo. |
| `GET` | `/topics/{topic}` | Retorna as informações e configurações do tópico (tipo, tags etc.). Também respondida localmente, pelo mesmo motivo. |
| `PATCH` | `/topics/{topic}` | Altera informações do tópico (tags; `prefixes` de um `distributor`; `idleTimeout` de uma fila volátil). Nome, `id`, tipo e `volatile` são imutáveis. Mesma propagação do `POST`. |
| `DELETE` | `/topics/{topic}` | Remove o tópico e **todas** as suas mensagens, mesmo as pendentes (ainda não entregues ou não confirmadas) — sempre funciona, sem checagem. Mesma propagação do `POST`; falha no registro **cancela** a remoção (rollback); falha no commit, já decidido, não cancela — o coordenador insiste dentro do teto `write.commit.timeout`. Nunca uma remoção parcial visível. |
| `POST` | `/topics/{topic}/messages` | Publica uma mensagem no tópico. Num `distributor`, copia a mensagem para as filas cujo nome começa com algum prefixo e responde com o resultado por fila; sem fila casando, responde sucesso e descarta ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.2). |
| `GET` | `/topics/{topic}/messages` | Consome mensagem(ns) do tópico. Rejeitado com `409` num `distributor`. Aceita `autoCreate=true` (cria a fila se não existir, volátil por padrão, com `idleTimeout`, `volatile` e `tag.<chave>` opcionais; seção 14.4), `visibilityTimeout` (segundos, padrão 120): a mensagem entregue fica invisível a outros consumidores por esse tempo; e `waitTime` (segundos, 0 a 20, padrão 0): long polling, aguarda novas mensagens antes de responder vazio ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)). |
| `PATCH` | `/topics/{topic}/messages/{id}` | Altera a invisibilidade de uma mensagem já entregue (`{"visibilityTimeout": 300}`), por exemplo para ganhar mais tempo de processamento. |
| `DELETE` | `/topics/{topic}/messages/{id}` | Confirma o consumo (ack) de uma mensagem, removendo-a do tópico. Decide o commit por **quórum** (sempre com o primário): falha antes do quórum **cancela** o ack (rollback); falha numa réplica fora do quórum, já decidido, não cancela — insiste dentro do teto `write.commit.timeout` ([13-entrega-de-mensagens.md](13-entrega-de-mensagens.md), seção 12.4). |

**Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`):
- de 1 a 80 caracteres;
- apenas letras ASCII (`a-z`, `A-Z`), dígitos (`0-9`), hífen (`-`) e sublinhado (`_`) — regex `^[A-Za-z0-9_-]{1,80}$`;
- diferencia maiúsculas de minúsculas (`Pedidos` e `pedidos` são tópicos distintos);
- nomes inválidos são rejeitados na criação (`POST /topics`) com erro `400`.

**Tags do tópico** (`tags`): pares chave/valor definidos na criação (`POST /topics`) e alterados via `PATCH /topics/{topic}`. Servem para monitoramento: são enviadas como **atributos** nos traces e logs exportados via **OpenTelemetry** para as operações daquele tópico, e nas métricas de tópico conforme a configuração de exportação ([11-observabilidade.md](11-observabilidade.md)) (ex.: `{"time": "financeiro", "ambiente": "prod"}`).
- chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL);
- máximo de 20 tags por tópico;
- as tags não afetam o comportamento das mensagens, apenas a telemetria.
 
### 8.1.1 API de operação (observabilidade)

Rotas para operadores, não para aplicações clientes. Detalhes, parâmetros e métricas em [11-observabilidade.md](11-observabilidade.md). Opcionalmente podem ser servidas em **porta própria** e/ou restritas a uma **lista de IPs/faixas** (`operations.port`, `operations.allowed-ips`); sem essa configuração, ficam na porta da API de cliente e são tratadas como qualquer outra rota ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

| Verbo | Recurso | Descrição |
|---|---|---|
| `GET` | `/metrics` | Métricas locais do nó no formato Prometheus (um alvo de scrape por nó, sem fan-out). |
| `GET` | `/stats/node` | Métricas do nó que atende a chamada (leitura local). |
| `GET` | `/stats/cluster` | Métricas de todos os nós e condição geral do cluster (consulta todos os nós, *best-effort*). |
| `GET` | `/stats/topics` | Métricas de tópicos, **paginada** (`limit`, `cursor`, `sort`, `tag.*`, `condition`, `fields`, `groupBy`, `scope`). Visão do cluster por padrão; `scope=node` devolve só os valores do nó. |
| `GET` | `/stats/topics/{topic}` | Métricas detalhadas e condição de um tópico. |
| `GET` | `/health/live` | O processo está de pé. |
| `GET` | `/health/ready` | O nó está apto a receber tráfego (registrado no cluster). |

### 8.2 API interna (cluster)

> Cada rota `rollback` só descarta um registro (fase 1) **pendente, sem commit** — falha ou timeout de retries antes de fechar o quórum. Uma falha na chamada de `commit` (fase 2) numa réplica fora do quórum, já decidido, nunca aciona `rollback`: o coordenador insiste no commit dentro do teto `write.commit.timeout` até confirmar em todas as réplicas ([05-particionamento-replicacao.md](05-particionamento-replicacao.md), seção 7.2).
>
> **Mensagens (rotas abaixo com `messages/{id}`, `visibility` e `ack`) decidem o commit por quórum** (`write.quorum`, sempre incluindo o primário do bloco) — a chamada de `register` ainda vai a todas as réplicas, mas o `commit` é decidido e respondido ao cliente assim que o quórum confirma, sem esperar as demais. **Nó e tópico (`nodes`, `topics`) continuam exigindo confirmação de todos os nós**, sem quórum reduzido (seção 7.2).

| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/cluster/nodes/register` | Fase 1 (registro) de um novo nó: propaga os dados do novo nó (IP/porta) a todos os nós. |
| `POST` | `/cluster/nodes/commit` | Fase 2 (commit) de registro **ou** desregistro de nó: efetiva a operação na tabela de membros de todos os nós. |
| `POST` | `/cluster/topics/register` | Fase 1 (registro) de uma criação, alteração ou remoção de tópico: propaga a definição a todos os nós ([02-arquitetura.md](02-arquitetura.md), seção 3.3). |
| `POST` | `/cluster/topics/commit` | Fase 2 (commit): efetiva a definição na tabela de tópicos de todos os nós, e cada um grava/atualiza/remove o `${topico}.json` local. |
| `POST` | `/cluster/topics/rollback` | Descarta a alteração de tópico pendente (sem commit) após esgotar os retries. |
| `GET` | `/cluster/topics/{id}/idle` | Usado pelo **nó gestor** de uma fila volátil: devolve há quanto tempo foi o último `GET` do tópico neste nó (duração decorrida; sem registro, o tempo desde que o nó subiu). Leitura local, sem estado pendente ([15-topicos-distribuidor.md](15-topicos-distribuidor.md), seção 14.5). |
| `POST` | `/cluster/vote-remove` | Usado para votar a remoção de um nó suspeito de falha (quórum mínimo: 3 — aberta quando o canal com o nó fica fora após as tentativas de reconexão, antes do commit de desregistro). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/register` | Fase 1 (registro) de uma mensagem, enviado ao(s) nó(s) responsável(is) pelo bloco (primário/réplicas). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/commit` | Fase 2 (commit) da entrega de uma mensagem já registrada nos nós responsáveis; decidido assim que o quórum confirma (primário incluso), enviado às demais réplicas em paralelo/segundo plano. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/rollback` | Descarta o registro pendente (sem commit) de uma mensagem cujo registro falhou após os retries. |
| `GET` | `/cluster/channel` | Abre (upgrade para **WebSocket**) o **canal permanente** entre dois nós, aberto pelo nó que entra, volta ou detecta primeiro a queda; em conexão dupla, o nó de menor hash do ID fecha a que abriu (um por par, para todos os tópicos): carrega o **sinal de vida** que substitui o ping (seção 6.3) e os sinais de disponibilidade da entrega — tópicos com ouvintes, tópicos com mensagens prontas e avisos de mensagem nova (seção 12.8). Só sinais, nunca conteúdo de mensagens nem listas longas. |
| `POST` | `/cluster/topics/{topic}/messages/take` | Pede ao nó dono que entregue mensagem(ns) prontas do tópico, com `visibilityTimeout`: ele faz o registro + commit de invisibilidade e devolve as mensagens ao nó que pediu (seção 12.8). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/register` | Fase 1 (registro) de uma entrega ou de uma alteração de invisibilidade: grava como pendente o novo `invisibleUntil` (e o `deliveryCount`, se for entrega) nos nós responsáveis. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/commit` | Fase 2 (commit): efetiva a invisibilidade; no caso de entrega, o nó também remove o ID da sua lista de prontos. Decidido por quórum (primário incluso) — o primário sempre recebe essa chamada antes da resposta ao cliente. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/visibility/rollback` | Descarta a alteração de invisibilidade pendente após esgotar os retries. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/register` | Fase 1 (registro) do ack: pedido de remoção da mensagem a todos os nós responsáveis pelo bloco. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/commit` | Fase 2 (commit): remove a mensagem (e seus metadados); decidido por quórum (primário incluso), replicado às demais réplicas em segundo plano. Sem tombstone: a ausência do ID já basta para a sincronização (seção 7.3). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/ack/rollback` | Descarta o registro de ack pendente após esgotar os retries; a mensagem permanece disponível. |
| `GET` | `/cluster/blocks/{id}/ids` | Retorna a lista de IDs de mensagens que o nó possui para o bloco — usado na sincronização distribuída, inclusive na verificação final (seção 7.3). |
| `GET` | `/cluster/blocks/{id}/messages/{messageId}` | Retorna o conteúdo de uma mensagem específica do bloco, para as chamadas balanceadas de sincronização. |
| `POST` | `/cluster/messages/recover` | Usado por um nó que retorna ao cluster (seção 6.6) para enviar aos novos donos mensagens que estavam registradas como perdidas. |
| `GET` | `/cluster/stats/node` | Métricas locais do nó, para a agregação de `/stats/cluster`. |
| `GET` | `/cluster/stats/topics` | Contadores locais por tópico (apenas mensagens em que o nó é primário: visíveis, invisíveis e data da mais antiga), agregados por quem atende `/stats/topics` (soma das quantidades, menor data). |

