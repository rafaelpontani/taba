# API

[← Índice](README.md)

## 8. API (rascunho inicial)
 
> Contrato formal (OpenAPI) ainda não definido — esta seção lista os verbos e recursos previstos até o momento.
 
### 8.1 API de cliente
 
| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/topics` | Cria um tópico (nome, tipo `single` ou `broadcast`, e opcionalmente `tags`). |
| `GET` | `/topics` | Lista os tópicos existentes. |
| `GET` | `/topics/{topic}` | Retorna as informações e configurações do tópico (tipo, tags etc.). |
| `PATCH` | `/topics/{topic}` | Altera informações do tópico (ex.: tags). Nome e tipo são imutáveis. |
| `DELETE` | `/topics/{topic}` | Remove o tópico e suas mensagens. |
| `POST` | `/topics/{topic}/messages` | Publica uma mensagem no tópico. |
| `GET` | `/topics/{topic}/messages` | Consome mensagem(ns) do tópico. |
| `DELETE` | `/topics/{topic}/messages/{id}` | Confirma o consumo (ack) de uma mensagem, removendo-a do tópico. |

**Nome do tópico** (`{topic}`): restritivo, no estilo do SQS, para ser sempre seguro em URL e como nome de pasta em disco (`${topico}/${id}.json`):
- de 1 a 80 caracteres;
- apenas letras ASCII (`a-z`, `A-Z`), dígitos (`0-9`), hífen (`-`) e sublinhado (`_`) — regex `^[A-Za-z0-9_-]{1,80}$`;
- diferencia maiúsculas de minúsculas (`Pedidos` e `pedidos` são tópicos distintos);
- nomes inválidos são rejeitados na criação (`POST /topics`) com erro `400`.

**Tags do tópico** (`tags`): pares chave/valor definidos na criação (`POST /topics`) e alterados via `PATCH /topics/{topic}`. Servem para monitoramento: são enviadas como **atributos** nas métricas, traces e logs exportados via **OpenTelemetry** para as operações daquele tópico (ex.: `{"time": "financeiro", "ambiente": "prod"}`).
- chave de 1 a 64 caracteres e valor de 0 a 128, restritos a `A-Za-z0-9_-.:` (seguros como atributo OTel e em URL);
- máximo de 20 tags por tópico;
- as tags não afetam o comportamento das mensagens, apenas a telemetria.
 
### 8.2 API interna (cluster)
 
| Verbo | Recurso | Descrição |
|---|---|---|
| `POST` | `/cluster/nodes/register` | Fase 1 (registro) de um novo nó: propaga os dados do novo nó (IP/porta) a todos os nós. |
| `POST` | `/cluster/nodes/commit` | Fase 2 (commit) de registro **ou** desregistro de nó: efetiva a operação na tabela de membros de todos os nós. |
| `GET` | `/cluster/ping` | Usado pelos nós para checagem de vida mútua (com retries antes de abrir votação). |
| `POST` | `/cluster/vote-remove` | Usado para votar a remoção de um nó suspeito de falha (quórum mínimo: 3 — antes do commit de desregistro). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/register` | Fase 1 (registro) de uma mensagem, enviado ao(s) nó(s) responsável(is) pelo bloco (primário/réplicas). |
| `POST` | `/cluster/topics/{topic}/messages/{id}/commit` | Fase 2 (commit) da entrega de uma mensagem já registrada nos nós responsáveis. |
| `POST` | `/cluster/topics/{topic}/messages/{id}/rollback` | Descarta o registro pendente (sem commit) de uma mensagem cujo registro ou commit falhou após os retries. |
| `GET` | `/cluster/blocks/{id}/ids` | Retorna a lista de IDs de mensagens (e tombstones) que o nó possui para o bloco — usado na sincronização distribuída (seção 7.3). |
| `GET` | `/cluster/blocks/{id}/messages/{messageId}` | Retorna o conteúdo de uma mensagem específica do bloco, para as chamadas balanceadas de sincronização. |
| `POST` | `/cluster/messages/recover` | Usado por um nó que retorna ao cluster (seção 6.6) para enviar aos novos donos mensagens que estavam registradas como perdidas. |

