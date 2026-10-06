# Sistema de Mensagens — Documentação

> Versão 0.9 — documento vivo (escopo + documentação técnica), atualizado conforme o projeto evolui.

Sistema de mensageria **altamente escalável, rápido e simples**, inspirado no Amazon SQS e SNS.

## Índice

| Arquivo | Seções | Conteúdo |
|---|---|---|
| [01-visao-geral.md](01-visao-geral.md) | 1, 2 | Descrição e objetivos. |
| [02-arquitetura.md](02-arquitetura.md) | 3 | Camadas do nó, fluxo de escrita de mensagem, memória e overflow para disco. |
| [03-escopo.md](03-escopo.md) | 4, 5 | O que está dentro e fora do escopo inicial. |
| [04-cluster.md](04-cluster.md) | 6 | Descoberta, registro de nós, detecção de falhas, votação, retorno de nó, freezing. |
| [05-particionamento-replicacao.md](05-particionamento-replicacao.md) | 7 | Hash circular, registro + commit de mensagens, sincronização distribuída de blocos. |
| [06-api.md](06-api.md) | 8 | API de cliente (tópicos, mensagens, tags) e API interna do cluster. |
| [07-principios-design.md](07-principios-design.md) | 9 | Princípios de design. |
| [08-pendencias-e-proximos-passos.md](08-pendencias-e-proximos-passos.md) | 10 | Decisões em aberto, organizadas pelos próximos passos que dependem delas. |
| [09-glossario.md](09-glossario.md) | 11 | Termos do projeto. |
| [10-configuracao.md](10-configuracao.md) | — | Parâmetros de configuração e valores padrão (inclui observabilidade e rate limits). |
| [11-observabilidade.md](11-observabilidade.md) | — | Observabilidade: métricas por nó, métricas e condição de tópicos (rotas próprias), endpoint Prometheus (`/metrics`), traces, logs, health checks. |
| [12-limites-api.md](12-limites-api.md) | — | Limites fixos da API e definição dos rate limits. |
| [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md) | 12 | Controle de entrega: invisibilidade, contador de entregas, metadados em disco, lista de prontos, dono da entrega, long polling e canal entre nós. |

> A numeração das seções é contínua entre os arquivos: uma referência como "seção 7.2" está sempre no arquivo cujo índice lista o número 7.
