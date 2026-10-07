# Histórico da implementação

[← Índice](README.md)

Registro, **na ordem em que aconteceu**, do que já foi implementado em `crates/`. O objetivo é bater a aplicação com a documentação: o que está pronto, o que se desviou do texto e o que ainda falta. Quem implementa algo novo atualiza este arquivo **na mesma mudança** (procedimento no [AGENTS.md](../AGENTS.md)).

Último registro: etapa 5 (disco e drain). Casos de uso com ao menos um teste: **165 de 806** (`cargo test --test uc_coverage -- --nocapture`).

## Como ler e como atualizar

- **Linha do tempo (seção 1):** uma entrada por etapa, em ordem. Cada entrada diz o que foi feito, onde está no código, os desvios em relação à documentação (e se o texto foi corrigido) e o que ficou de fora.
- **Desvios e decisões (seção 2):** tudo o que a implementação decidiu e não estava (ou contraria) a documentação, para ser conferido e promovido a [08a-decisoes-tomadas.md](08a-decisoes-tomadas.md) quando valer.
- **Falta implementar (seção 3):** a documentação vista por área, com `Feito`, `Parcial` ou `Falta`. É a lista para conferir a documentação contra o código.
- **Próximas etapas (seção 4):** a ordem planejada.

Ao terminar uma etapa: acrescente a entrada na seção 1, mova os itens que passaram de `Falta` para `Parcial` ou `Feito` na seção 3, registre os desvios na seção 2, atualize a linha "Último registro" e o placar acima.

## 1. Linha do tempo

### Etapa 0: estrutura, configuração e testes

- **Projeto:** um Cargo workspace com um único crate, `crates/taba-node` (biblioteca `taba_node` e binário `taba`), edição 2024. Módulos `core`, `config`, `cli`, `cluster`, `storage`, `engine`, `http`, `node` e `telemetry`, na direção de dependência do [doc 18](18-estrutura-codigo.md) (`core ← storage ← engine ← cluster ← http ← node`).
- **Configuração** (`config/`): a tabela `params` é a fonte única de chaves e padrões do [doc 10](10-configuracao.md). Valores vêm do YAML (achatado com pontos), das variáveis `TABA_*` (vazia vale como ausente) e dos padrões. Validações juntam todos os problemas de uma vez; `Secret` não aparece no `Debug`. Em `mode=development`, replicação e quórum viram 1, com aviso. Um teste confere que todo parâmetro da tabela é lido.
- **Catálogo de erros** (`core/error`): 48 códigos com status fixo ([doc 06d](06d-api-convencoes.md)); `ErrorResponse` em camelCase, sem status no corpo e com `Retry-After` só no cabeçalho.
- **HTTP:** três listeners (cliente, operação opcional em porta própria, cluster). Middleware de `X-Request-Id`, `404 route.not-found` e `500 internal.error` para pânico. Rotas `/health/live` e `/health/ready`.
- **Nó** (`node`): sobe e para (`start`/`shutdown`), com o estado do nó e o relógio injetável (`Clock`, `SystemClock`, `ManualClock`). A linha de comando tem `--config`, `--help` e `--version`; configuração inválida sai com código 2. Log em JSON.
- **Testes:** unitários em `foo/tests.rs` ao lado de `foo/mod.rs`; integração em `crates/taba-node/tests/` com `TestNode` (portas livres, disco temporário, `ManualClock`). `uc_coverage` confere que cada `/// UC: UC-XXX-NNN` existe em `doc/20*.md`.
- **Desvios:** padrões `8080`, `8081` e `25 s`, `logs.level=info` e as regras de arquivo, duração e tamanho foram escritos nos docs [06](06-api.md) e [10](10-configuracao.md), que estavam em aberto. O trait de armazenamento mora em `storage`, e não no `engine` ([doc 18](18-estrutura-codigo.md) corrigido). Casos novos: UC-CFG-009 a 012.

### Etapa 1: tópicos (CRUD, persistência e paginação)

- **Rotas:** `POST` e `GET /topics`, `GET`, `PATCH` e `DELETE /topics/{topic}` ([doc 06a](06a-api-cliente.md)), com `415` para `Content-Type` errado, `validation.unknown-field`, `validation.required`, `PATCH` parcial (`null` volta ao padrão) e campos imutáveis recusados.
- **Dead-letter:** não tem definição própria. É montada na leitura a partir da fila de origem (`id` derivado do `id` da fila, trocando o último dígito hexadecimal), é somente leitura (`409`) e some com a fila.
- **Listagem:** `sort` por `name`, `id` e `createdAt`, filtro `name`, cursor opaco amarrado ao `sort` e ao filtro, ordem estável. Parâmetros novos `api.pagination.default-limit` (100) e `api.pagination.max-limit` (1000).
- **Camadas:** `engine/topics` (regras), `storage/topics` (trait `TopicStore` com versão em memória para teste) e `http/topics`. Uma alteração por vez (gravar antes de aplicar em memória).
- **Modo desenvolvimento:** `development.resume-on-start` retoma as definições do disco, e `false` apaga o disco na partida. Em produção as definições **não** são carregadas na partida (toda partida é de nó novo, [04 §6.10](04-cluster.md)).
- **Desvios:** os docs [02](02-arquitetura.md), [06](06-api.md), [10](10-configuracao.md) e [18](18-estrutura-codigo.md) foram ajustados; o total de casos de uso passou a 806 (UC-DEF-024 novo).

### Etapa 1b: SQLite do nó

- **`_node.db`** (`storage/db`): `${storage.dir}/_node.db` em modo WAL, com `storage.db.synchronous` aplicado. Uma conexão atrás de um `Mutex` faz o papel de escritor único. Esquema versionado em `PRAGMA user_version`; banco de versão mais nova ou arquivo que não é banco impede o nó de subir.
- **Definições de tópico:** passaram dos arquivos `_topics/${topico}.json` para a tabela `topicos` (`nome`, `definicao`), com `SqliteTopicStore`. O nome é chave byte a byte, então `Pedidos` e `pedidos` já são tópicos diferentes em qualquer disco (o truque do `~` no nome do arquivo saiu).
- **Dependência:** `rusqlite` com `bundled`, que exige um compilador C no build. No alvo `-gnu` do Windows o `cc` não pode achar o `gcc` do Cygwin ([AGENTS.md](../AGENTS.md)).
- **Docs ajustados:** [02](02-arquitetura.md), [04](04-cluster.md), [06c](06c-api-cluster.md), [10](10-configuracao.md), [13](13-entrega-de-mensagens.md), [15](15-topicos-distribuidor.md), [18](18-estrutura-codigo.md) e os casos UC-DEF-009, 011, 013, 024, UC-RET-012, UC-ARM-018, UC-CLR-006 e UC-DRN-022.
- **Ficou de fora:** as tabelas `perdas` e `estado` do `_node.db` e o banco por tópico (`${topico}/_topico.db`).

### Etapa 2: publicar e consumir (em memória)

- **Publicar** (`POST /topics/{topic}/messages`): corpo em bytes crus; `Content-Type` validado como media type (1 a 255 caracteres; padrão `application/json`); JSON ou texto UTF-8 inválidos dão `validation.invalid-body`; tamanho limitado por `maxMessageBytes` do tópico ou `message.max-bytes`, com a leitura cortada no limite (`413` sem ler o resto); `groupId` e `deduplicationId` validados; a fila FIFO exige `groupId`; `deduplicationId` fora da FIFO é `validation.not-applicable`. Os `id`s vêm de um gerador crescente (`IdGenerator`), mesmo dentro do mesmo milissegundo.
- **Consumir** (`GET`): `maxMessages` (padrão 10), `visibilityTimeout` (padrão 120 s), `waitTime` de 0 a 20 s (long polling que acorda com mensagem nova, com a volta de uma invisível ou com a remoção do tópico, e é cancelado se o cliente desconecta). O `body` sai como JSON nativo **exatamente como foi enviado** (`RawValue`), texto ou base64 (`bodyEncoding`). Cada entrega incrementa `deliveryCount` e troca o `receiptToken`; passado o prazo a mensagem volta (*at-least-once*). Mensagem vencida por `ttl` não é entregue.
- **Distribuidor:** copia a mensagem para cada fila cujo nome começa com um prefixo, cada cópia com `id` próprio; zero filas casadas responde `200`; `topics.distributor.max-targets` recusa a publicação inteira (`422`); uma fila FIFO sem `groupId` falha só naquela cópia.
- **Dead-letter:** pode receber e entregar mensagens como uma fila comum. Remover um tópico remove as mensagens dele e as da dead-letter.
- **Arquitetura:** `core/content` (tipo de conteúdo, validação e renderização do corpo), `engine/messages` (fila em memória ordenada por `id`, uma por tópico), `http/messages`. O `engine/topics` ganhou o gancho `on_removed` e `queues_with_prefix`.
- **Defeitos achados pelos testes e corrigidos:** overflow no cálculo de números negativos (agora `validation::in_range_signed`) e o `json!` que reserializava o corpo.
- **Desvios:** o distribuidor e o long polling local, previstos para etapas posteriores, entraram aqui. `delivery.max-messages.max` e `delivery.visibility-timeout.max` ainda são "A definir" no [doc 10](10-configuracao.md): sem valor, não há teto.

### Etapa 3: ack e invisibilidade

- **Ack** (`DELETE /topics/{topic}/messages/{id}?receiptToken=`): `204` e a mensagem some; segundo ack é `404`; sem `receiptToken` é `400`; distribuidor é `409`; token obsoleto (a mensagem foi reentregue) é `409 message.stale-receipt` sem mudar nada. Passado o prazo e antes de uma nova entrega, o token ainda vale.
- **`PUT .../visibility`:** o novo prazo é `agora + visibilityTimeout` (substitui, não soma), não mexe em `deliveryCount` nem em `receiptToken`, é idempotente; `0` libera a mensagem na hora e acorda um long polling. Os campos que faltam (`receiptToken` na query, `visibilityTimeout` no corpo) saem juntos no `details`.
- **`ttl`:** mensagem vencida é `404` no ack e no `PUT`, e é removida na hora.
- **Ficou de fora:** UC-VIS-011 (`PUT` com `0` na mensagem que já está no `maxDeliveries` vai à dead-letter) e UC-ACK-013 (remover a linha do banco e o `.body`), por dependerem da retenção e do disco.

### Etapa 4: retenção (`ttl`, `maxDeliveries` e dead-letter)

- **`ttl`:** a mensagem vencida é apagada em qualquer estado (pronta ou invisível) e **não** vai à dead-letter. O prazo usa o `ttl` atual do tópico, então um `PATCH` que o reduz vale para o que já está guardado. Na dead-letter o `ttl` conta de `deadLetteredAt`, e não do `id`.
- **`maxDeliveries`:** quando a invisibilidade da última entrega permitida vence sem ack, a mensagem vai para a dead-letter da fila e a entrega seguinte nunca acontece. `PUT .../visibility` com `0` numa mensagem que já gastou as entregas também a manda para lá (UC-VIS-011). Ack depois desse vencimento é `404`, porque a mensagem já foi movida.
- **Movimento:** mesmo `id`, `deliveryCount` e prazo zerados, `receiptToken` descartado, `deadLetteredAt` e `deadLetterReason: max-deliveries` (aparecem no `Message` da dead-letter). Quem espera na dead-letter (long polling) é acordado. A dead-letter não tem limite de entregas: não há dead-letter da dead-letter.
- **Varredura** (`Messages::sweep`, `run_sweeper`): uma tarefa de fundo do nó, parada junto com ele. A mesma passada (`settle`) roda também antes de cada `GET`, ack e `PUT`, então o resultado não depende do relógio da tarefa. Os eventos `taba.message.expired` e `taba.message.dead_lettered` saem como log (`tracing`); as métricas `taba.retention.expired` e `taba.delivery.dead_lettered` não existem ainda.
- **Desvio:** a tarefa roda a cada `min(retention.sweep.interval, 1 s)`, e não só a cada `retention.sweep.interval` (60 s): a mesma passada leva à dead-letter a mensagem cuja invisibilidade venceu, e quem a consome não deve esperar um minuto. O `ttl` não é afetado, porque a mensagem vencida nunca é entregue entre duas passadas.
- **Custo conhecido:** cada passada percorre todas as mensagens guardadas (não há lista de invisíveis nem tabela de expiração). Isso só pesa em filas grandes e some quando a lista de invisíveis do [doc 14](14-invisibilidade.md) existir.
- **Ficou de fora:** as métricas e os eventos estruturados (UC-RET-007 e 018), a fila FIFO (UC-RET-019), os casos de cluster (UC-RET-006, 010, 011 e 020) e o `autoCreate`/fila volátil (UC-RET-017).

### Etapa 5: mensagens em disco, banco por tópico e o drain como save

- **Overflow** (`storage/messages`, `engine/messages`): a mensagem fica em memória enquanto os bytes de corpo em memória couberem em `memory.limit`; a que passa do limite (ou chega durante o drain) vai para o disco. O corpo é `${topico}/${id}.body`, só os bytes como vieram; o resto é uma linha da tabela `mensagens` do banco `${topico}/_topico.db` (WAL, `storage.db.synchronous`, um escritor por banco). Escrita: primeiro o arquivo (por arquivo temporário e renomeação) e depois a linha; ack: primeiro a linha e depois o arquivo. Sem `memory.limit` nada vai para o disco por overflow.
- **Uma mensagem, um lugar:** cada mensagem está em memória **ou** em disco, e todas as operações valem nos dois: a entrega junta as duas origens em ordem de `id`, o ack, o `PUT .../visibility`, o `ttl` e o `maxDeliveries` acham a mensagem onde ela estiver. No disco o `ttl` e as esgotadas saem de consultas com índice (`age_base`, `invisible_until`), sem varrer. O movimento para a dead-letter de uma mensagem em disco só **renomeia** o arquivo para a pasta da dead-letter e troca a linha de banco.
- **Pasta do tópico:** `Pedidos` é a pasta `~pedidos` (cada maiúscula vira `~` mais a minúscula), para dois tópicos que só diferem em maiúsculas não colidirem em disco que não diferencia. Remover o tópico apaga a pasta (banco e corpos).
- **Drain como save** (`Messages::drain`, chamado por `RunningNode::shutdown` com `drain.on-stop`): o limite de memória efetivo cai a zero (toda mensagem nova vai direto ao disco) e as mensagens em memória são gravadas **com os metadados** (`deliveryCount`, `invisibleUntil`, `receiptToken`, `groupId`, `contentType`, dead-letter). Erro numa mensagem não interrompe o resto: ela fica em memória e entra no relatório. A parada por `SIGINT`/`SIGTERM` já passa por aí.
- **Retomada** (`development.resume-on-start`, padrão): a partida seguinte acha as mensagens pelos bancos, e o mesmo `receiptToken` ainda confirma; o prazo é um instante absoluto, então a mensagem cuja invisibilidade passou durante a parada volta pronta. Sem `resume`, e em produção, as pastas de mensagens são apagadas na partida (o nó começa vazio); pastas de tópicos que não existem mais também, para um tópico recriado não ressuscitar mensagens.
- **Fora do executor:** as operações de mensagem rodam em `spawn_blocking` (as rotas, o `GET` e a varredura), como pede o [doc 18](18-estrutura-codigo.md).
- **Desvios:** ver os itens 14 a 20 da seção 2.
- **Ficou de fora:** a divisão dinâmica da memória por taxa de leitura (UC-ARM-004 a 008), a tabela `expiracao`, as tabelas `perdas` e `estado` do `_node.db`, as métricas `taba.storage.*`, `POST/GET/DELETE /node/drain` com `drainState`, o `503` durante o drain e o `drain.timeout`.

## 2. Desvios e decisões a conferir com a documentação

| # | Assunto | O que o código faz | Documento | Situação |
|---|---|---|---|---|
| 1 | Definições de tópico | Tabela `topicos` do `_node.db`, e não `_topics/*.json`. | 02, 04, 06c, 10, 13, 15, 18, 20* | Docs corrigidos. |
| 2 | Tokens de acesso | O doc 02 (seção 3.4) ainda manda um arquivo por token, `_tokens/${id}.json`. Pelo mesmo critério das definições, deveria virar uma tabela do `_node.db`. | 02 | **Pendente**: decidir e corrigir o doc antes de implementar. |
| 3 | Trait do armazenamento | `TopicStore` mora em `storage`, não no `engine`. | 18 | Doc corrigido. |
| 4 | Tetos de entrega | Sem `delivery.max-messages.max` e `delivery.visibility-timeout.max`, não há teto. | 10 | Aguarda a definição dos valores. |
| 5 | `message.hard-limit` | Sem valor, não há teto para `maxMessageBytes`. | 10, 12 | Aguarda a definição do valor. |
| 6 | Paginação | Parâmetros `api.pagination.default-limit` (100) e `max-limit` (1000). | 06, 10 | Doc corrigido. |
| 7 | Definições na partida | Em produção não são carregadas do disco; em desenvolvimento só com `development.resume-on-start`. | 04 §6.10 | Conforme o doc. |
| 8 | Varredura de expiração | O `ttl` e o `maxDeliveries` são aplicados antes de cada `GET`, ack e `PUT` e por uma tarefa de fundo a cada `min(retention.sweep.interval, 1 s)`. | 13 §12.10 | Desvio consciente; vale a pena registrar em 13 quando o intervalo da dead-letter for decidido. |
| 9 | Fila FIFO | Aceita publicar e consumir, mas sem ordem por grupo, sem deduplicação e sem o segundo nó. | 17 | **Pendente** (etapa 6). Não confiar na semântica FIFO. |
| 10 | `autoCreate` | O `GET` ignora o parâmetro (tópico inexistente dá `404`). | 15 §14.4 | **Pendente** (etapa 6). |
| 11 | Lista de prontos | Cada `GET` varre a fila em ordem de `id`, pulando as invisíveis; não há lista de prontos nem efeito sanfona. | 13 §12.6 e 12.7 | **Pendente**; só pesa em filas grandes. |
| 12 | Vários `GET` esperando | A ordem em que os ouvintes são atendidos não é garantida. | 13 §12.8 (UC-CNS-029) | **Pendente**. |
| 13 | Compilador | O `rusqlite` embutido exige um compilador C. | 18 | Documentado no AGENTS.md. |
| 14 | Política de overflow | Passa para o disco a mensagem **nova** que não cabe em `memory.limit` (soma dos bytes de corpo, para o nó todo). Não há divisão por taxa de leitura, `memory.topic.reserve` nem `memory.rate.window`, e a mensagem que já está em memória não é rebaixada para abrir espaço. | 02 §3.2 | **Pendente**: UC-ARM-004 a 008. |
| 15 | Listas de prontos e de invisíveis | Não existem. A memória é varrida a cada operação; o disco usa os índices do banco. Por isso a tabela `expiracao` também não foi criada (o índice `invisible_until` de `mensagens` faz o papel). | 02 §3.2, 13 §12.6, 14 §13.4 e 13.5 | **Pendente**; decidir em 14 se `expiracao` continua necessária sem a lista em memória. |
| 16 | Colunas de `mensagens` | Não há a posição no anel (cluster) nem `deduplicationId`/`deleteAt` (FIFO). Há três colunas a mais: `age_base` (de onde o `ttl` conta), `dead_lettered_em` e `dead_letter_reason`. | 02 §3.2 | Doc 02 deve listar as colunas extras quando a FIFO e o cluster chegarem. |
| 17 | Partida de produção | As pastas de mensagens são apagadas na partida (nó novo). Em desenvolvimento só com `development.resume-on-start=false`; pasta de tópico que não existe mais é sempre apagada. | 04 §6.10 | Conforme o texto; o cluster vai recuperar do disco antes de limpar (04 §6.6). |
| 18 | Trava única | Um `Mutex` guarda a memória e o acesso ao disco de **todos** os tópicos; uma operação de disco bloqueia as outras. | 02 §3.2 (escritor por tópico) | **Pendente**: uma fila de comandos por tópico (doc 18 §17.4). |
| 19 | Drain | Só o drain local na parada (`drain.on-stop`). Faltam as rotas `/node/drain`, `drainState`, `/health/ready` em `503`, o `503` com `Retry-After` durante o drain, o `drain.timeout`, o segundo sinal e a marca `save_concluido`. | 04 §6.10, 06b | **Pendente**. |
| 20 | Disco com falha | Erro de disco vira `500 internal.error` com log; o nó **não** entra em `failed`. Pasta com nome reservado do Windows (`con`, `nul`...) para um tópico com esse nome não é tratada. | 02 §3.2, 05 §7.3 | **Pendente**: UC-ARM-019 e 020. |

## 3. Falta implementar, por área da documentação

`Feito` quer dizer com código **e** teste; `Parcial` lista o que falta.

| Área | Documento | Situação |
|---|---|---|
| Configuração | [10](10-configuracao.md) | **Parcial.** Toda chave é lida e validada, mas só parte é usada pelo código (as demais esperam as áreas abaixo). |
| Erros e convenções da API | [06d](06d-api-convencoes.md) | **Parcial.** Catálogo, `X-Request-Id`, `404` e `500` prontos; `Retry-After` só existe no tipo de erro (ninguém o gera ainda). |
| Tópicos | [06a](06a-api-cliente.md) | **Feito** para fila e distribuidor num nó. Faltam os casos de cluster. |
| Mensagens: publicar e consumir | [06a](06a-api-cliente.md), [13](13-entrega-de-mensagens.md) | **Parcial.** Memória e disco, num nó. Faltam replicação, `autoCreate`, a ordem de grupos justa (§12.11), a lista de prontos e o sanfona. |
| Ack e invisibilidade | [06a](06a-api-cliente.md), [14](14-invisibilidade.md) | **Parcial.** Feito num nó, em memória e em disco. Faltam as listas de invisíveis e a tabela de expiração (item 15) e o vencimento por quórum. |
| Retenção: `ttl`, `maxDeliveries` e dead-letter | [13 §12.10](13-entrega-de-mensagens.md) | **Parcial.** Feito num nó, em memória e em disco. Faltam as métricas, a FIFO e o cluster. |
| Armazenamento em disco | [02 §3.2](02-arquitetura.md) | **Parcial.** `_node.db` (`topicos`), overflow, banco por tópico e pasta segura para maiúsculas. Faltam a divisão da memória por taxa, `expiracao`, `perdas` e `estado`, as métricas `taba.storage.*` e o estado de falha (itens 14 a 20 da seção 2). |
| Distribuidor | [15](15-topicos-distribuidor.md) | **Feito** a publicação em memória. Faltam o evento `no_target`/`partial` e o `PATCH` de `prefixes` testado (UC-DST-013). |
| Fila volátil | [15 §14.3 a 14.6](15-topicos-distribuidor.md) | **Falta** (`idleTimeout`, nó gestor, exclusão por ociosidade). |
| Fila FIFO | [17](17-topicos-fifo.md) | **Falta** (grupos, ordem, deduplicação, primário e secundário, `deleteAt`). |
| Grupos e entrega justa | [13 §12.11](13-entrega-de-mensagens.md) | **Falta.** |
| Autenticação e tokens | [06a §8.1.3](06a-api-cliente.md) | **Falta** (rotas `/tokens`, token raiz, permissões, prefixos). |
| Limites e rate limit | [12](12-limites-api.md) | **Parcial.** Limites de validação prontos; o rate limit (`429`) não existe. |
| Rotas de operação | [11](11-observabilidade.md) | **Parcial.** `/health/live` e `/health/ready`; faltam `/metrics`, `/metrics/topics`, `/health/cluster`, `allowed-ips` e OpenTelemetry. |
| Drain e save | [04 §6.9 e 6.10](04-cluster.md) | **Parcial.** Save na parada e retomada em desenvolvimento. Faltam `/node/drain`, `drainState`, `drain.timeout` e o `503` durante o drain (item 19). |
| Cluster | [04](04-cluster.md), [05](05-particionamento-replicacao.md) | **Falta** tudo: entrada, votação, anel e blocos, registro e commit, quórum, sincronização (`sync.db`), freezing, streams, perdas. Os casos de nível `C` esperam o `Transport`. |
| TLS e mTLS | [04](04-cluster.md) | **Falta.** O nó avisa que atende em HTTP puro. |
| Conectores | [19](19-conectores.md) | **Falta.** Fora do crate do nó. |

## 4. Próximas etapas

1. **Nó único completo:** autenticação e tokens (decidir antes o item 2 da seção 2), rate limit, fila volátil e `autoCreate`, FIFO, grupos e entrega justa, e as rotas de operação (`/metrics`, `/health/cluster`, `/node/drain` com `drainState`).
2. **Cluster:** `Transport`, entrada, anel, registro e commit, quórum e sincronização.
