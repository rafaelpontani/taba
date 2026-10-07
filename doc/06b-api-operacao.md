# API de operação

[← Índice](README.md) · [Visão geral das APIs](06-api.md)

## 8.1.1 API de operação (observabilidade)

> **Porta:** `operations.port` ou, se não for definida, `api.port` ([06-api.md](06-api.md), seção 8.0). **Autenticação:** nenhuma; a proteção é de rede (porta própria, grupo de segurança, `operations.allowed-ips`). Detalhes em [06-api.md](06-api.md), seção 8.0.1.

Rotas para operadores, não para aplicações clientes. Detalhes, parâmetros e métricas em [11-observabilidade.md](11-observabilidade.md). As rotas `/node/drain` valem só para o nó que recebe a chamada, por isso o operador as chama no nó, e não pelo balanceador. Opcionalmente podem ser servidas em **porta própria** e/ou restritas a uma **lista de IPs/faixas** (`operations.port`, `operations.allowed-ips`); sem essa configuração, ficam na porta da API de cliente e são tratadas como qualquer outra rota ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)). As rotas que devolvem listas seguem a paginação da seção 8.1.2 ([06a-api-cliente.md](06a-api-cliente.md)).

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
