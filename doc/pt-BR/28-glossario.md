---
título: Glossário
resumo: Os termos do projeto em ordem alfabética, cada um com uma definição curta e o link do documento que o explica.
tipo: referência
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src, doc/pt-BR]
---

# Glossário

> Os termos são definidos **uma vez** em [03-conceitos.md](03-conceitos.md) (o modelo mental) e **em detalhe** no documento indicado. Identificadores em inglês (rotas, chaves, campos, códigos de erro) aparecem em `código`.

## A

| Termo | Significado | Detalhe |
|---|---|---|
| **Ack** | Confirmação do consumo: `DELETE …/messages/{id}?receiptToken=`. Remove a mensagem | [12](12-entrega.md) |
| **Anel de hash** | O círculo de posições de 64 bits que decide em quais nós mora cada mensagem (hash circular, *consistent hashing*) | [06](06-anel-e-replicacao.md) |
| **At-least-once** | Garantia de entrega: uma mensagem pode chegar mais de uma vez, mas não se perde por falta de ack | [11](11-durabilidade.md) |
| **`autoCreate`** | Parâmetro do `GET` que cria a fila (volátil por padrão) se ela não existir | [09](09-topicos.md) |

## B

| Termo | Significado | Detalhe |
|---|---|---|
| **Backup (S3)** | O backup opcional de falha grave: segmentos de mensagens, diários de estado, definições e batimentos num bucket S3 ou MinIO. Nunca entra no caminho de publicação | [33](33-backup-s3.md) |
| **Batimento** | O arquivo `nodes/<id>.json` que cada nó regrava no bucket, com um `seq` crescente; quem observa vê o ETag mudar e dá o nó por morto quando para por `backup.dead-after` | [33](33-backup-s3.md) |

| Termo | Significado | Detalhe |
|---|---|---|
| **Banco de eventos** | `_sync/sync.db`: onde um nó em sincronização guarda, na ordem de chegada, os eventos que não pode processar ainda | [07](07-cluster.md), [08](08-armazenamento.md) |
| **Banco do nó** | `_node.db`: SQLite local com as tabelas `topics`, `tokens`, `state`, `losses` e `definition_versions` | [08](08-armazenamento.md) |
| **Banco do tópico** | `<topico>/_topic.db`: SQLite local com a tabela `messages` (o que está em disco) | [08](08-armazenamento.md) |
| **Bloco** | O arco do anel que termina numa posição de nó virtual; tem um primário e réplicas | [06](06-anel-e-replicacao.md) |
| **Bootstrap** | O processo de um nó ingressar no cluster | [07](07-cluster.md) |

## C

| Termo | Significado | Detalhe |
|---|---|---|
| **Canal permanente** | Um WebSocket por par de nós, compartilhado por todos os tópicos; leva sinal de vida e avisos, nunca mensagens | [07](07-cluster.md), [12](12-entrega.md) |
| **Certificado do cluster** | Par `.crt`/`.key` igual em todos os nós, usado no TLS mútuo da porta do cluster | [14](14-seguranca.md) |
| **Cliente** | Aplicação que usa a API de cliente | [16](16-api-cliente.md) |
| **Cluster** | O conjunto de nós que se conhecem. Mínimo de 3 em produção | [07](07-cluster.md) |
| **Commit** | A fase 2 do protocolo: efetiva o que o registro deixou pendente | [10](10-publicacao.md) |
| **Condição** | Item da lista devolvida por `/health/cluster` ou pelas métricas de tópico (ex.: `under-replicated`, `backlog`) | [13](13-observabilidade.md) |
| **Conector** | Biblioteca de cliente do Taba numa linguagem (hoje Rust e C#) | [21](21-conectores.md) |
| **Contador de entregas** | `deliveryCount`: quantas vezes a mensagem foi entregue | [12](12-entrega.md) |
| **Coordenador** | O nó que recebeu a chamada do cliente (ou que conduz uma mudança de membros) | [10](10-publicacao.md), [07](07-cluster.md) |
| **`contentType`** | Metadado imutável da mensagem: o `Content-Type` da publicação | [16](16-api-cliente.md) |

## D

| Termo | Significado | Detalhe |
|---|---|---|
| **Dead-letter** | Fila `{fila}-dead-letter` para onde vai a mensagem que atinge `maxDeliveries` sem ack | [09](09-topicos.md), [12](12-entrega.md) |
| **`deduplicationId`** | Chave de deduplicação de uma publicação numa fila FIFO (sem ele, o SHA-256 do corpo) | [09](09-topicos.md) |
| **`deleteAt`** | Exclusão lógica na fila FIFO: o ack grava a data e apaga só o corpo, para a deduplicação valer pela janela | [09](09-topicos.md) |
| **Distribuidor (DT)** | Tópico que não guarda mensagens: copia cada publicação para as filas cujo nome começa com um dos seus prefixos | [09](09-topicos.md) |
| **Dono da definição** | O nó que decide e numera as mudanças de um tópico ou token (o primeiro nó ativo da posição `topic:<nome>`) | [08](08-armazenamento.md) |
| **Dono da entrega** | O primário do bloco de uma mensagem: o único que a põe na lista de prontos | [06](06-anel-e-replicacao.md), [12](12-entrega.md) |
| **Drain** | Sair do cluster sem perder o que está em memória: tudo vai para o disco e o nó se desregistra | [07](07-cluster.md), [17](17-api-operacao.md) |

## E

| Termo | Significado | Detalhe |
|---|---|---|
| **Entrega justa** | Preferência da entrega pelas mensagens de grupos menores, por faixas de potência de 2 | [12](12-entrega.md) |
| **Escritor único** | Toda escrita num banco SQLite passa por uma única fila de comandos | [04](04-arquitetura.md), [08](08-armazenamento.md) |
| **`ErrorCode`** | Um código do catálogo de erros, com status HTTP fixo | [19](19-erros-e-limites.md) |

## F

| Termo | Significado | Detalhe |
|---|---|---|
| **Fator de replicação** | Quantos nós guardam cada mensagem (`replication.factor`, padrão 3) | [10](10-publicacao.md) |
| **Fila (QT)** | Tópico que guarda mensagens e entrega cada uma a um consumidor | [09](09-topicos.md) |
| **Fila FIFO (QTF)** | Fila que entrega cada grupo em ordem, uma mensagem por vez, e deduplica | [09](09-topicos.md) |
| **Fila volátil (QTV)** | Fila excluída sozinha se ficar sem consumidor por `idleTimeout` | [09](09-topicos.md) |
| **Freezing** | Período em que um nó excluído por falha não pode reentrar | [07](07-cluster.md) |

## G

| Termo | Significado | Detalhe |
|---|---|---|
| **Gestor (nó gestor)** | O primário da posição do `id` de uma fila volátil: decide a exclusão por ociosidade | [09](09-topicos.md) |
| **GUID v7** | O formato de todos os identificadores (mensagem, tópico, token, nó): ordenável no tempo | [15](15-api-visao-geral.md) |
| **Grupo (`groupId`)** | Rótulo da mensagem: na FIFO define a ordem; nas outras filas, a entrega justa | [09](09-topicos.md), [12](12-entrega.md) |

## H

| Termo | Significado | Detalhe |
|---|---|---|
| **Herdeiro** | O primeiro outro dono ativo da posição de uma definição: recebe a cópia antes da resposta ao cliente | [08](08-armazenamento.md) |
| **Hash circular** | Ver *Anel de hash* | [06](06-anel-e-replicacao.md) |

## I

| Termo | Significado | Detalhe |
|---|---|---|
| **`id` do tópico** | GUID v7 gerado na criação: identifica a "encarnação" (recriar o nome gera outro) e posiciona o gestor | [09](09-topicos.md) |
| **Invisibilidade** | Prazo (`invisibleUntil`) em que uma mensagem já entregue não é entregue a outro consumidor (padrão 120 s) | [12](12-entrega.md) |
| **`invisibleUntil`** | Instante absoluto do fim da invisibilidade, fixado pelo relógio do dono | [12](12-entrega.md) |

## L

| Termo | Significado | Detalhe |
|---|---|---|
| **Lista de invisíveis** | Por tópico e no dono, as mensagens invisíveis ordenadas por prazo | [12](12-entrega.md) |
| **Lista de prontos** | Por tópico e no dono, as próximas mensagens a entregar | [12](12-entrega.md) |
| **Long polling** | O `GET` que espera (até 20 s) por mensagens (`waitTime`) | [12](12-entrega.md) |

## M

| Termo | Significado | Detalhe |
|---|---|---|
| **`maxDeliveries`** | Limite de entregas sem ack por fila (padrão 10); atingido, vai à dead-letter | [12](12-entrega.md) |
| **Mensagem** | Corpo (bytes) + metadados (`id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`…) | [03](03-conceitos.md) |
| **Modo desenvolvimento** | `mode=development`: um cluster de um nó, sem votação, quórum, canal nem réplicas | [07](07-cluster.md) |
| **Modo drain** | Ver *Drain* | [07](07-cluster.md) |

## N

| Termo | Significado | Detalhe |
|---|---|---|
| **NDJSON** | Um objeto JSON por linha: o formato dos streams de sincronização entre nós | [18](18-api-cluster.md) |
| **Nó** | Uma instância do programa `taba`; todos são iguais | [04](04-arquitetura.md) |
| **Nó virtual** | Posição adicional que um nó ocupa no anel (`ring.virtual-nodes`, padrão 128) | [06](06-anel-e-replicacao.md) |

## O

| Termo | Significado | Detalhe |
|---|---|---|
| **`operationId`** | GUID v7 de uma operação em duas fases; as três fases o levam e são idempotentes por ele | [18](18-api-cluster.md) |
| **Overflow** | Passagem das mensagens da memória para o disco quando `memory.limit` é atingido | [08](08-armazenamento.md) |

## P

| Termo | Significado | Detalhe |
|---|---|---|
| **Pasta de segredos** | Criada por `taba init`: certificados do cluster (`tls/`) e token raiz (`root-token`) | [14](14-seguranca.md) |
| **Porta de cliente / do cluster / de operação** | As três portas do nó | [04](04-arquitetura.md) |
| **Primário / secundário / terciário** | Os nós responsáveis por um bloco, em ordem ao longo do anel; o primário é o dono da entrega | [06](06-anel-e-replicacao.md) |
| **Prefixo** | Do distribuidor: nomes de filas de destino. Do token: tópicos que ele alcança | [09](09-topicos.md), [14](14-seguranca.md) |

## Q

| Termo | Significado | Detalhe |
|---|---|---|
| **Quórum de escrita** | Confirmações para decidir o commit (`write.quorum`, padrão maioria; o primário sempre conta) | [10](10-publicacao.md) |
| **Quórum de votação** | 51% dos nós saudáveis (`cluster.vote.quorum-percent`), com no mínimo `cluster.vote.min-voters` | [07](07-cluster.md) |

## R

| Termo | Significado | Detalhe |
|---|---|---|
| **Rate limit** | Limite de requisições por token ou IP (*token bucket*, por nó) | [14](14-seguranca.md), [19](19-erros-e-limites.md) |
| **`receiptToken`** | Token gerado a cada entrega; exigido no ack e na alteração da invisibilidade | [12](12-entrega.md) |
| **Registro** | A fase 1 do protocolo: guarda a operação como pendente | [10](10-publicacao.md) |
| **Rollback** | Descarte de um registro pendente; só **antes** da decisão do commit | [10](10-publicacao.md) |
| **Réplica** | Cópia de uma mensagem num nó que não é o primário | [06](06-anel-e-replicacao.md) |

## S

| Termo | Significado | Detalhe |
|---|---|---|
| **Save (drain como)** | No modo desenvolvimento, o drain grava tudo em disco e a próxima partida retoma | [07](07-cluster.md) |
| **Sinal de vida (*keepalive*)** | Mensagem periódica do canal (padrão a cada 2 s) com estado, carga e prontas por tópico | [12](12-entrega.md) |
| **Sincronização** | O que um nó novo faz ao entrar: carga de tópicos e blocos, replay do banco de eventos, verificação final | [07](07-cluster.md) |

## T

| Termo | Significado | Detalhe |
|---|---|---|
| **`take`** | Pedido de um nó a outro (o dono) para entregar mensagens prontas | [12](12-entrega.md) |
| **Tabela de membros** | A lista de nós do cluster, com uma `version` | [07](07-cluster.md) |
| **Tag** | Par chave/valor de um tópico, só para telemetria | [13](13-observabilidade.md) |
| **Token de acesso** | Credencial `tb_<id>.<segredo>` com permissões e prefixos | [14](14-seguranca.md) |
| **Token raiz** | Segredo da configuração com acesso total; o único que gerencia tokens | [14](14-seguranca.md) |
| **Tópico** | Canal nomeado: fila ou distribuidor | [09](09-topicos.md) |
| **`ttl`** | Tempo de vida da mensagem por fila (padrão 14 dias); vencida, é descartada (não vai à dead-letter) | [12](12-entrega.md) |
| **Transport** | O *trait* que leva as chamadas entre nós (HTTP real e rede em memória de teste) | [04](04-arquitetura.md) |

## U

| Termo | Significado | Detalhe |
|---|---|---|
| **UC (caso de uso)** | Código `UC-<ÁREA>-<NNN>` de um comportamento observável, citado nos testes | [26](26-casos-de-uso.md) |

## V

| Termo | Significado | Detalhe |
|---|---|---|
| **Versão-base** | A versão mais antiga de uma linguagem que um conector suporta | [21](21-conectores.md) |
| **Votação de exclusão** | Os nós saudáveis votam para retirar um nó cujo canal caiu | [07](07-cluster.md) |

## W

| Termo | Significado | Detalhe |
|---|---|---|
| **`waitTime`** | Segundos de long polling (0 a 20) | [16](16-api-cliente.md) |

## Siglas

| Sigla | Significa |
|---|---|
| **QT** / **DT** / **QTV** / **QTF** | Queue Topic / Distributor Topic / Queue Topic Volatile / Queue Topic FIFO |
| **mTLS** | TLS mútuo (os dois lados apresentam certificado) |
| **OTLP** | OpenTelemetry Protocol |
| **UCUM** | Unified Code for Units of Measure (as unidades das métricas) |
| **ADR** | *Architecture Decision Record* ([30-decisoes.md](30-decisoes.md)) |

## Fontes e provas

- `09-glossario.md` (a base), conferido contra os módulos de `crates/taba-node/src` e os demais documentos desta pasta. A definição **corrigida** em relação ao histórico: a definição de tópico vive na tabela `topics` do `_node.db` (não em `${topic}.json`), e o *freezing* não é um estado do nó.
- Casos de uso: UC-ANE, UC-ESC, UC-ENT, UC-INV; veja [26a](26a-casos-de-uso-implementados.md).
