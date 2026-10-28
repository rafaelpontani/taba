---
título: Casos de uso implementados
resumo: O catálogo dos casos de uso do Taba que têm ao menos um teste que os cita, por área, com o caso, o nível e os testes que o provam.
tipo: referência
status: estável
verificado-em: 2026-10-27 @ ada9ff1
fontes: [crates/taba-node/tests, crates/taba-node/src, connectors/rust, connectors/csharp]
---

# Casos de uso implementados

> **Mantido à mão.** Cada cartão traz o caso, o nível e os testes que o citam (as linhas `/// UC: UC-XXX-NNN` do código). Formato dos códigos e regra de cobertura: [26-casos-de-uso.md](26-casos-de-uso.md). Os casos **sem teste** estão em [26b-casos-de-uso-planejados.md](26b-casos-de-uso-planejados.md).

**Placar (2026-10-15, commit 56121c9, mais 15 casos do trabalho de servidor para a TUI):** **818** de **909** casos têm ao menos um teste (90.0%); **91** não têm; **42** estão retirados (o código não é reaproveitado).

Nível: **U** unitário, **I** um nó real, **C** cluster simulado, **K** conector. Cada caso é um bloco: o **código** no cabeçalho; abaixo, **Caso** (a situação), **Então** (o resultado esperado), **Nível** e **Testes** (`arquivo::função`, relativo a `crates/taba-node`, ou à raiz do repositório para os conectores).

## Cobertura por área

| Área | Assunto | Com teste | Sem teste |
|---|---|---:|---:|
| `TOP` | Tópicos: criar, listar, ler, alterar e remover | 46 | 3 |
| `MSG` | Publicação e o corpo da mensagem | 19 | 1 |
| `CNS` | Consumo (`GET`), long polling e `autoCreate` | 39 | 1 |
| `ACK` | Confirmação do consumo | 13 | 0 |
| `VIS` | Alteração da invisibilidade | 10 | 2 |
| `RET` | Expiração (`ttl`), limite de entregas e dead-letter | 18 | 2 |
| `DST` | Tópico Distribuidor | 17 | 1 |
| `VOL` | Fila volátil e nó gestor | 23 | 0 |
| `FIF` | Fila FIFO | 53 | 0 |
| `GRP` | Grupos de mensagens e entrega justa | 17 | 0 |
| `AUT` | Autenticação e tokens de acesso | 34 | 5 |
| `LIM` | Rate limit e IP do cliente | 13 | 2 |
| `PAG` | Paginação | 8 | 1 |
| `CNV` | Convenções comuns da API (formato, cabeçalhos, erros, status) | 14 | 3 |
| `CNT` | Conectores | 35 | 0 |
| `CLI` | O `taba-cli`, a linha de comando do cliente | 36 | 0 |
| `ESC` | Escrita: registro + commit, quórum, rollback, teto de commit | 29 | 9 |
| `ANE` | Anel de hash, blocos e papéis | 8 | 4 |
| `DEF` | Definição de tópicos e de tokens replicada no cluster | 26 | 8 |
| `ENT` | Entrega no dono: lista de prontos, `take`, long polling | 18 | 13 |
| `CAN` | Canal permanente entre nós e filas de sinais | 28 | 0 |
| `INV` | Invisibilidade no dono e nas réplicas | 26 | 6 |
| `ARM` | Armazenamento: memória, overflow, SQLite, integridade | 24 | 5 |
| `ING` | Entrada de nó (bootstrap) | 33 | 4 |
| `FAL` | Detecção de falhas, votação de exclusão e saída por falha | 32 | 1 |
| `FRZ` | Período de freezing | 13 | 1 |
| `CNC` | Concorrência entre mudanças de membros | 12 | 3 |
| `SIN` | Sincronização distribuída de blocos | 31 | 1 |
| `STR` | Streams NDJSON entre nós | 8 | 7 |
| `RTN` | Retorno de nó excluído e recuperação de perdas | 45 | 0 |
| `FWD` | Repasse de chamadas de cliente | 5 | 7 |
| `DRN` | Parada e modo drain | 19 | 2 |
| `DEV` | Perfil de um nó (o antigo modo desenvolvimento) | 3 | 4 |
| `SEG` | Portas, TLS mútuo e certificados do cluster | 20 | 0 |
| `OPS` | Observabilidade (OpenTelemetry), estatísticas, métricas e saúde | 44 | 7 |
| `CLR` | Contrato das rotas do cluster (DTOs e regras comuns) | 8 | 2 |
| `CFG` | Configuração e validações de partida | 11 | 2 |
| `BKP` | Backup opcional em S3 | 45 | 0 |
| `CHS` | Caos com histórico verificado | 5 | 0 |
| `BNC` | Benchmark comparativo local (`bench/run.sh`) | 3 | 1 |

## TOP: Tópicos: criar, listar, ler, alterar e remover

### UC-TOP-001

**Caso:** Criar uma fila (`type: queue`) com nome válido  
**Então:** `201`, `Location: /topics/{name}`, corpo `Topic` com `id` GUID v7, `createdAt`, `tags: {}`, `volatile: false`, `fifo: false` e `effective` com os padrões do cluster.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::creating_a_queue_returns_201_with_location_and_the_defaults`

---

### UC-TOP-002

**Caso:** Criar um distribuidor (`type: distributor`) com `prefixes`  
**Então:** `201`, e o `Topic` traz `prefixes` e não traz os campos de fila (`volatile`, `fifo`, `ttl`, `maxDeliveries`, `maxMessageBytes`, `effective`).  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::a_distributor_shows_prefixes_and_none_of_the_queue_fields`

---

### UC-TOP-003

**Caso:** Criar fila volátil (`volatile: true`) sem `idleTimeout`  
**Então:** vale o padrão de 600 s (`topics.volatile.idle-timeout.default`); com `idleTimeout` informado, vale o informado.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-004

**Caso:** Criar fila FIFO (`fifo: true`) com `replication.factor` de pelo menos 2  
**Então:** `201` e `fifo: true` no `Topic`.  
**Nível:** I, C  
**Testes:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-005

**Caso:** Criar fila com `ttl`, `maxDeliveries` e `maxMessageBytes` próprios  
**Então:** o `Topic` traz os valores definidos e `effective` os mesmos; sem eles, os três são `null` e `effective` traz os padrões (14 dias, 10, 5 MB).  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-006

**Caso:** Toda fila criada ganha junto a fila `{nome}-dead-letter`, que aparece em `GET /topics` com `deadLetter: true` e `source`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-007

**Caso:** Criar com tags válidas (até 20; chave de 1 a 64 e valor de 0 a 128 caracteres de `A-Za-z0-9_-.:`)  
**Então:** `201` e as tags voltam no `Topic`.  
**Nível:** I  
**Testes:**
  - `src/engine/topics/tests.rs::tags_at_the_limits_are_accepted_and_come_back_in_the_topic`

---

### UC-TOP-008

**Caso:** Criar com tags inválidas (mais de 20; chave vazia ou com mais de 64; valor com mais de 128; caractere fora do conjunto)  
**Então:** `400` `validation.failed`, com o campo em `details`.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::invalid_tags_fail_with_the_field_in_details`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-009

**Caso:** Nome fora de `^[A-Za-z0-9_-]{1,100}$` (vazio, 101 caracteres, espaço, acento, `/`)  
**Então:** `400` `validation.invalid-name`.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::topic_name_accepts_the_documented_charset_and_length`
  - `src/core/validation/tests.rs::topic_name_rejects_empty_long_and_foreign_characters`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-010

**Caso:** Nome terminado em `-dead-letter`  
**Então:** `400` `validation.reserved-name`.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::topic_name_ending_in_the_dead_letter_suffix_is_reserved`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-011

**Caso:** O nome diferencia maiúsculas de minúsculas  
**Então:** `Pedidos` e `pedidos` coexistem como tópicos distintos.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-012

**Caso:** Criar com nome que já existe  
**Então:** `409` `topic.already-exists`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-013

**Caso:** Campo que não vale para o tipo (`prefixes` numa fila, `fifo`, `volatile`, `ttl`, `maxDeliveries` ou `maxMessageBytes` num distribuidor)  
**Então:** `400` `validation.not-applicable`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::fields_that_do_not_apply_to_the_type_are_refused`
  - `tests/topics.rs::fields_out_of_place_or_out_of_range_are_400`

---

### UC-TOP-014

**Caso:** `idleTimeout` fora de 60 a 604800, `ttl` fora de 60 a 31536000 ou `maxDeliveries` fora de 1 a 1000  
**Então:** `400` `validation.out-of-range`. Os limites exatos (59 e 60, 604800 e 604801 etc.) são aceitos e recusados como definido.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::ranges_are_inclusive_at_both_ends`
  - `src/engine/topics/tests.rs::idle_timeout_ttl_and_max_deliveries_have_exact_limits`
  - `tests/topics.rs::fields_out_of_place_or_out_of_range_are_400`

---

### UC-TOP-015

**Caso:** `maxMessageBytes` acima de `message.hard-limit`  
**Então:** `400` `validation.max-bytes-above-hard-limit`. Acima do geral (5 MB) e dentro do limite rígido: aceito.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::max_message_bytes_may_exceed_the_default_but_not_the_hard_limit`
  - `tests/topics.rs::max_message_bytes_can_pass_the_default_but_not_the_hard_limit`

---

### UC-TOP-016

**Caso:** Distribuidor com mais de 20 prefixos, prefixo vazio, de mais de 100 caracteres ou com caractere fora de `A-Za-z0-9_-`  
**Então:** `400`.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::distributor_prefixes_follow_the_documented_limits`
  - `src/engine/topics/tests.rs::distributor_prefixes_are_validated`
  - `tests/topics.rs::distributor_prefixes_follow_the_limits`

---

### UC-TOP-017

**Caso:** Distribuidor com `prefixes` vazio é aceito  
**Então:** não copia a publicação para ninguém.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::a_distributor_shows_prefixes_and_none_of_the_queue_fields`

---

### UC-TOP-018

**Caso:** Corpo com campo desconhecido  
**Então:** `400` `validation.unknown-field`. Sem `name` ou sem `type`: `400` `validation.required`, com o campo em `details`.  
**Nível:** U, I  
**Testes:**
  - `tests/topics.rs::unknown_and_missing_fields_are_400_naming_the_field`

---

### UC-TOP-020

**Caso:** Recriar um tópico com o mesmo nome (depois de removê-lo) gera um `id` diferente.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-021

**Caso:** Listar sem tópicos  
**Então:** `200` com `items: []` e sem `nextCursor`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::listing_without_topics_is_an_empty_page`

---

### UC-TOP-022

**Caso:** A listagem vem ordenada por nome por padrão e inclui as filas de dead-letter.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-023

**Caso:** `sort` por `name`, `id`, `createdAt` e cada um com `-` (decrescente); `sort` desconhecido  
**Então:** `400` `validation.failed`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::listing_sorts_and_filters`

---

### UC-TOP-024

**Caso:** O filtro `name` devolve os tópicos cujo nome **contém** o texto, diferenciando maiúsculas de minúsculas.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::listing_sorts_and_filters`

---

### UC-TOP-026

**Caso:** `GET /topics/{topic}` devolve a definição; tópico inexistente  
**Então:** `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::reading_a_topic_returns_it_and_an_unknown_one_is_404`

---

### UC-TOP-027

**Caso:** `GET` da dead-letter devolve `deadLetter: true`, `source` e os valores da fila de origem, somente leitura.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-028

**Caso:** `PATCH` parcial  
**Então:** só os campos enviados mudam. `tags`, quando enviado, **substitui o mapa inteiro**.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::patch_is_partial_tags_replace_and_null_goes_back_to_the_default`

---

### UC-TOP-029

**Caso:** `null` em `ttl`, `maxDeliveries` ou `maxMessageBytes` volta ao padrão do cluster (`effective` acompanha o padrão).  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::patch_is_partial_tags_replace_and_null_goes_back_to_the_default`

---

### UC-TOP-030

**Caso:** Novo `ttl` vale também para as mensagens já guardadas, a partir da próxima varredura.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_new_ttl_applies_to_messages_already_stored`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-TOP-031

**Caso:** Novo `maxMessageBytes` só vale para as publicações seguintes; mensagens já aceitas não são rejeitadas.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-TOP-032

**Caso:** `PATCH` de `prefixes` num distribuidor vale a partir do commit e só para publicações posteriores.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::patching_distributor_prefixes_replaces_them`

---

### UC-TOP-034

**Caso:** `PATCH` com campo imutável (`name`, `id`, `type`, `volatile`, `fifo`), **mesmo com o valor atual**  
**Então:** `400` `validation.immutable-field`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::patch_refuses_immutable_fields_even_with_the_current_value`
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-035

**Caso:** `PATCH` com campo que não vale para o tipo (`ttl` num distribuidor)  
**Então:** `400` `validation.not-applicable`. Fora da faixa ou acima do limite rígido: `400`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::patch_validates_applicability_and_ranges`
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-036

**Caso:** `PATCH` na dead-letter  
**Então:** `409` `topic.read-only`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-037

**Caso:** `PATCH` em tópico inexistente  
**Então:** `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::reading_a_topic_returns_it_and_an_unknown_one_is_404`

---

### UC-TOP-038

**Caso:** Remover um tópico vazio  
**Então:** `204`; um `GET` posterior responde `404`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-039

**Caso:** Remover uma fila com mensagens prontas, invisíveis (em processamento) e sem ack  
**Então:** `204`, sempre, sem checagem. Todas as mensagens somem (memória, disco e metadados).  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests.rs::deleting_a_queue_drops_its_messages_and_those_of_its_dead_letter`
  - `src/engine/messages/tests.rs::removing_a_topic_removes_its_folder_with_database_and_bodies`
  - `tests/messages.rs::deleting_a_queue_drops_its_ready_and_in_flight_messages`

---

### UC-TOP-040

**Caso:** Remover uma fila remove também a sua dead-letter, com as mensagens dela.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-041

**Caso:** `DELETE` na dead-letter  
**Então:** `409` `topic.read-only`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-042

**Caso:** `DELETE` de tópico inexistente  
**Então:** `404` `topic.not-found` (inclusive na repetição depois de um `503` `cluster.commit-timeout`, que o cliente trata como sucesso).  
**Nível:** I, C  
**Testes:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-043

**Caso:** Remover um distribuidor  
**Então:** `204`; as filas que ele copiava não são afetadas.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-044

**Caso:** Prune em um nó: `Messages::prune` esvazia a fila (mensagens visíveis e invisíveis, próprias ou réplicas, em memória ou em disco)  
**Então:** Devolve quantas mensagens o nó **era dono**; o tópico fica; uma mensagem publicada depois é entregue normalmente.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests/prune.rs::pruning_removes_the_visible_and_the_invisible_messages_and_counts_them`

---

### UC-TOP-045

**Caso:** O mesmo `pruneId` chega de novo a um nó  
**Então:** Responde a primeira contagem e não remove nada novo (uma mensagem publicada depois do primeiro prune fica); um `pruneId` novo esvazia de novo.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests/prune.rs::a_repeated_prune_id_answers_the_same_and_removes_nothing_new`

---

### UC-TOP-046

**Caso:** Prune de um tópico que não existe, ou de um distribuidor  
**Então:** `404 topic.not-found`; `409 topic.wrong-type`.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests/prune.rs::pruning_an_unknown_topic_is_404_and_a_distributor_is_a_wrong_type`

---

### UC-TOP-047

**Caso:** `DELETE /topics/{topic}/messages` num cluster com mensagens visíveis e invisíveis, recebido por qualquer nó  
**Então:** `200` `{pruneId, removed, nodes}`: todos os nós foram esvaziados (nenhum entrega nada), `removed` conta cada mensagem uma vez e não uma por cópia, o nó que recebe esvazia a si mesmo sem rede, e a fila continua publicando e entregando.  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::pruning_empties_the_queue_on_every_node_and_the_queue_keeps_working`

---

### UC-TOP-048

**Caso:** Prune com um token sem `manage`, ou de um tópico que não existe  
**Então:** `403 auth.forbidden` (com `required: "manage"`); `404 topic.not-found`; com `manage` sobre o tópico, `200`.  
**Nível:** I  
**Testes:**
  - `tests/nodes_api.rs::pruning_an_unknown_topic_is_404_and_needs_the_manage_permission`

---

### UC-TOP-049

**Caso:** Um prune enquanto um nó não responde  
**Então:** `503 cluster.prune-incomplete` nomeando esse nó (na mensagem e em `details`); os outros nós foram esvaziados; repetir a chamada depois que o nó volta o esvazia e responde `200`.  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::a_prune_that_fails_on_a_node_names_it_and_repeating_it_is_safe`

---


## MSG: Publicação e o corpo da mensagem

### UC-MSG-001

**Caso:** Publicar JSON válido, sem `Content-Type`  
**Então:** `201`, `PublishResponse` com `id` GUID v7, e a mensagem fica com `contentType: application/json`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::publishing_returns_increasing_guid_v7_ids`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`
  - `tests/messages.rs::the_json_body_comes_back_exactly_as_it_was_sent`

---

### UC-MSG-002

**Caso:** Publicar texto (`text/plain; charset=utf-8`)  
**Então:** `201`, e o `contentType` guarda o parâmetro `charset`.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-003

**Caso:** Publicar binário (`image/png`)  
**Então:** `201`, e os bytes ficam guardados exatamente como vieram, sem reserializar nem codificar.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-004

**Caso:** `Content-Type` que não é um media type válido, ou com mais de 255 caracteres  
**Então:** `400` `validation.invalid-content-type`.  
**Nível:** U, I  
**Testes:**
  - `src/core/content/tests.rs::rejects_what_is_not_a_media_type`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-005

**Caso:** Corpo `application/json` (ou `*/*+json`) que não é JSON válido  
**Então:** `400` `validation.invalid-body`.  
**Nível:** U, I  
**Testes:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-006

**Caso:** Corpo `text/*` que não é UTF-8 válido  
**Então:** `400` `validation.invalid-body`.  
**Nível:** U, I  
**Testes:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-007

**Caso:** Corpo de qualquer outro tipo não é validado (bytes arbitrários são aceitos).  
**Nível:** I  
**Testes:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-008

**Caso:** O tamanho conta os **bytes enviados** (não o base64). Corpo com exatamente `maxMessageBytes` é aceito; um byte a mais  
**Então:** `413` `message.too-large`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-MSG-009

**Caso:** Sem `maxMessageBytes` no tópico vale `message.max-bytes` (5 MB); um tópico com `maxMessageBytes` maior que o geral aceita mensagens maiores, até o limite rígido.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-MSG-010

**Caso:** Publicar em tópico inexistente  
**Então:** `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::unknown_topics_are_404`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-MSG-011

**Caso:** `groupId` válido (1 a 128 caracteres de `A-Za-z0-9_-.:`) numa fila comum  
**Então:** aceito e devolvido no `GET`. Inválido (vazio, mais de 128, caractere fora do conjunto): `400`.  
**Nível:** U, I  
**Testes:**
  - `src/core/validation/tests.rs::message_keys_accept_1_to_128_chars_of_the_tag_set`
  - `src/engine/messages/tests.rs::group_id_is_validated_and_deduplication_is_fifo_only`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-MSG-012

**Caso:** `deduplicationId` numa fila que não é FIFO  
**Então:** `400` `validation.not-applicable`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::group_id_is_validated_and_deduplication_is_fifo_only`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-MSG-013

**Caso:** O `id` é gerado pelo nó coordenador antes do registro, é um GUID v7 em minúsculas e ordenável por tempo  
**Então:** duas publicações sequenciais têm `id` crescente.  
**Nível:** U, I  
**Testes:**
  - `src/core/id/tests.rs::generator_is_strictly_increasing_even_within_the_same_millisecond`
  - `src/engine/messages/tests.rs::publishing_returns_increasing_guid_v7_ids`

---

### UC-MSG-014

**Caso:** A resposta só sai depois de o commit estar confirmado no quórum (a mensagem já é entregável no instante em que o cliente recebe o `id`).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-MSG-015

**Caso:** Publicar por qualquer nó do cluster funciona  
**Então:** o nó que recebe é o coordenador, e a mensagem acaba nos nós responsáveis pelo bloco.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-MSG-016

**Caso:** Sem quórum  
**Então:** `503` `cluster.quorum-unavailable` com `Retry-After`, e a mensagem **não** foi aceita (nenhum `GET` a vê).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-MSG-018

**Caso:** `PublishResponse` numa fila comum não traz `duplicate`; o campo só existe na fila FIFO.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-MSG-019

**Caso:** Publicar com `X-Request-Id`  
**Então:** a resposta devolve o mesmo valor.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-MSG-020

**Caso:** A mensagem publicada preserva o `groupId` e o `contentType` por todo o ciclo (publicação, entrega, reentrega e dead-letter).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---


## CNS: Consumo (`GET`), long polling e `autoCreate`

### UC-CNS-001

**Caso:** `GET` numa fila com mensagens  
**Então:** `200`, `messages` com `id`, `body`, `contentType`, `receiptToken`, `deliveryCount: 1`, `publishedAt` e `invisibleUntil`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::consuming_delivers_the_message_with_its_metadata`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-CNS-002

**Caso:** `GET` sem mensagens prontas  
**Então:** `200` com `messages: []`, **nunca** `204`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::consuming_delivers_the_message_with_its_metadata`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-003

**Caso:** `maxMessages` sem informar vale 10 (`delivery.max-messages.default`); informado, devolve até esse número; fora de 1 a `delivery.max-messages.max`  
**Então:** `400` `validation.out-of-range`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::max_messages_defaults_to_10_and_must_be_in_range`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-004

**Caso:** O `GET` devolve o que estiver pronto, sem esperar completar `maxMessages` (com `waitTime` 0).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::max_messages_defaults_to_10_and_must_be_in_range`
  - `src/engine/messages/tests.rs::consuming_returns_what_is_ready_without_waiting_to_fill_the_batch`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-005

**Caso:** `visibilityTimeout` sem informar vale 120 s; informado vale o informado; fora de 0 a `delivery.visibility-timeout.max`  
**Então:** `400`. O `invisibleUntil` devolvido é `agora + visibilityTimeout`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `src/engine/messages/tests.rs::visibility_timeout_zero_makes_the_message_deliverable_again_at_once`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-006

**Caso:** Uma mensagem entregue fica invisível  
**Então:** um segundo `GET` dentro do prazo não a devolve.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-007

**Caso:** Sem ack até o fim do prazo, a mensagem volta a ser entregue, com `deliveryCount` incrementado e um `receiptToken` **novo** (o anterior não é mais aceito).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-008

**Caso:** Corpo JSON sai como **JSON nativo** (objeto, lista, texto, número, booleano, `null`), exatamente como foi enviado, sem `bodyEncoding`.  
**Nível:** I  
**Testes:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`
  - `tests/messages.rs::the_json_body_comes_back_exactly_as_it_was_sent`

---

### UC-CNS-009

**Caso:** Corpo `text/*` em UTF-8 (ou sem `charset`) sai como **string**, sem `bodyEncoding`.  
**Nível:** I  
**Testes:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-010

**Caso:** Corpo de qualquer outro tipo, e `text/*` com outro `charset`, sai em **base64**, com `bodyEncoding: "base64"`.  
**Nível:** I  
**Testes:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-011

**Caso:** `*/*+json` (por exemplo, `application/vnd.api+json`) é tratado como JSON.  
**Nível:** U, I  
**Testes:**
  - `src/core/content/tests.rs::json_kinds_include_plus_json_suffixes`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-012

**Caso:** Num lote com JSON, texto e binário, cada mensagem sai na forma do seu `contentType`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-013

**Caso:** Não há garantia de ordem entre mensagens de uma fila comum; o teste não pode exigir ordem de entrega. As duas pontas do GUID v7 são consumidas de forma intercalada (efeito sanfona).  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::inside_a_band_the_oldest_and_the_newest_alternate_like_an_accordion`

---

### UC-CNS-014

**Caso:** A mensagem só chega ao cliente depois de a invisibilidade estar commitada no quórum.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-CNS-015

**Caso:** Mensagem vencida por `ttl` (mesmo antes da varredura) não é entregue.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_delivered`
  - `tests/messages.rs::a_message_past_its_ttl_is_not_delivered`

---

### UC-CNS-016

**Caso:** Consumir da dead-letter  
**Então:** `GET` normal, com `deadLetteredAt` e `deadLetterReason` nas mensagens.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_dead_letter_is_a_common_queue_for_publish_and_consume`
  - `tests/messages.rs::the_dead_letter_can_be_published_to_and_consumed`

---

### UC-CNS-017

**Caso:** `GET` num distribuidor  
**Então:** `409` `topic.wrong-type`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_cannot_be_consumed`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-CNS-018

**Caso:** `GET` num tópico que não existe, sem `autoCreate`  
**Então:** `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::unknown_topics_are_404`
  - `src/engine/messages/tests.rs::deleting_the_topic_ends_a_long_poll_with_not_found`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-CNS-019

**Caso:** O registro de invisibilidade de uma mensagem falha e há outras prontas  
**Então:** o rollback segue em outra thread, o `GET` passa ao próximo ID sem esperar, e a mensagem que falhou **nunca** entra na resposta.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-CNS-020

**Caso:** Ao menos uma mensagem capturada e outras falharam  
**Então:** `200` com as que conseguiu (menos que `maxMessages`).  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit`

---

### UC-CNS-021

**Caso:** **Nenhuma** mensagem capturada e houve falha no registro  
**Então:** `503` `delivery.unavailable` com `Retry-After`, nunca `200` vazio.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-CNS-022

**Caso:** O número de falhas toleradas por `GET` é limitado por `delivery.get.max-attempts`; esgotado, aplica-se UC-CNS-020 ou UC-CNS-021.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit`

---

### UC-CNS-023

**Caso:** Se o rollback não chega a uma réplica, o pendente expira por `write.pending.ttl` e a mensagem segue pronta como estava.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`
  - `src/cluster/replication/replica/tests.rs::only_the_pendings_past_the_ttl_expire_and_the_newer_ones_stay`

---

### UC-CNS-024

**Caso:** `waitTime` de 0 a 20 é aceito (padrão 0, responde na hora); fora da faixa  
**Então:** `400` `validation.out-of-range`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::wait_time_is_limited_to_0_20`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-025

**Caso:** Com `waitTime > 0` e fila vazia, o `GET` espera; uma mensagem publicada durante a espera é devolvida **sem** esperar o fim do tempo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::long_polling_returns_as_soon_as_a_message_arrives`
  - `src/engine/messages/tests.rs::long_polling_notices_a_message_whose_invisibility_ends`
  - `tests/messages.rs::long_polling_returns_as_soon_as_a_message_is_published`

---

### UC-CNS-026

**Caso:** Com `waitTime > 0` e nenhuma mensagem chegando, responde `200` com `messages: []` ao fim do tempo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::long_polling_ends_with_an_empty_list`
  - `tests/messages.rs::long_polling_ends_with_an_empty_list_when_nothing_arrives`

---

### UC-CNS-027

**Caso:** Com long polling, o `GET` devolve a primeira mensagem que chegar, sem esperar completar `maxMessages`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::long_polling_returns_as_soon_as_a_message_arrives`
  - `tests/messages.rs::long_polling_returns_as_soon_as_a_message_is_published`

---

### UC-CNS-028

**Caso:** O cliente desconecta durante a espera  
**Então:** o ouvinte é removido (nenhuma mensagem é entregue a uma conexão morta, e o recurso é liberado).  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_client_that_gives_up_does_not_swallow_the_next_message`

---

### UC-CNS-029

**Caso:** Vários `GET` em espera no mesmo tópico  
**Então:** cada mensagem nova vai ao primeiro da fila de ouvintes, na ordem de chegada.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::waiting_gets_are_served_in_the_order_they_arrived`
  - `src/engine/messages/tests.rs::one_publish_with_several_messages_wakes_the_next_waiting_get_too`

---

### UC-CNS-030

**Caso:** Long polling em espera num nó e mensagem publicada em outro nó do cluster  
**Então:** o consumidor recebe a mensagem (aviso entre nós + `take`).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-CNS-031

**Caso:** `autoCreate=true` num nome que não existe cria uma fila **volátil** por padrão (`volatile` padrão `true`), com `idleTimeout` 600 s, e já conta como consumidor.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_makes_a_volatile_queue_with_the_default_idle_timeout`
  - `src/engine/messages/tests.rs::without_auto_create_a_missing_topic_is_404_and_a_bad_flag_is_400`
  - `tests/volatile.rs::auto_create_on_get_makes_a_volatile_queue_and_the_get_counts`

---

### UC-CNS-032

**Caso:** `autoCreate` com `volatile=false`, `idleTimeout`, `fifo=true` e `tag.<chave>=<valor>` cria a fila com esses atributos.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_takes_the_creation_attributes_from_the_query`
  - `tests/volatile.rs::auto_create_takes_idle_timeout_fifo_volatile_and_tags_from_the_query`

---

### UC-CNS-033

**Caso:** `autoCreate` num tópico que já existe e é fila  
**Então:** os parâmetros de criação são **ignorados** e o `GET` é um consumo normal.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_on_an_existing_queue_ignores_the_creation_parameters`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-034

**Caso:** `autoCreate` num nome que pertence a um distribuidor  
**Então:** `409` `topic.wrong-type`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_never_makes_a_distributor_and_refuses_bad_names`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-035

**Caso:** `autoCreate` com nome terminado em `-dead-letter`  
**Então:** `400` `validation.reserved-name`. Nome inválido: `400` `validation.invalid-name`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_never_makes_a_distributor_and_refuses_bad_names`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-036

**Caso:** `autoCreate` que esbarra em `topics.volatile.max-count`  
**Então:** `422` `topic.volatile-limit`. Sem o teto definido, não há limite.  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests.rs::auto_create_stops_at_the_volatile_ceiling_but_post_does_not`
  - `tests/volatile.rs::the_volatile_ceiling_refuses_auto_create_with_422`

---

### UC-CNS-037

**Caso:** `autoCreate` de fila FIFO com `replication.factor` 1 (o perfil de um nó)  
**Então:** a fila é criada e aceita mensagens: o quórum da FIFO é `min(2, fator)` (o `topic.fifo-needs-replication` não é mais devolvido).  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-CNS-038

**Caso:** Vários consumidores com `autoCreate` ao mesmo tempo no mesmo nome  
**Então:** um cria, os demais tratam como "já existe" e seguem como consumo normal, sem erro `409`.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::consumers_racing_to_auto_create_the_same_name_all_succeed`

---

### UC-CNS-040

**Caso:** `autoCreate` exige as permissões `consume` **e** `create` (com autenticação ligada).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::creating_by_auto_create_needs_the_create_permission_but_consuming_does_not`
  - `tests/volatile.rs::auto_create_needs_consume_and_create_but_a_plain_consume_only_needs_consume`
  - `tests/volatile.rs::auto_create_stays_inside_the_token_prefixes`

---


## ACK: Confirmação do consumo

### UC-ACK-001

**Caso:** Ack com o `receiptToken` da última entrega  
**Então:** `204`, e a mensagem some do tópico (nenhum `GET` a devolve, nem depois do prazo).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::ack_removes_the_message_and_a_second_ack_is_not_found`
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-002

**Caso:** Ack sem `receiptToken` na query  
**Então:** `400` `validation.required`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::missing_fields_are_reported_together`
  - `tests/messages.rs::ack_validates_the_request_and_the_topic`

---

### UC-ACK-003

**Caso:** Um segundo ack da mesma mensagem  
**Então:** `404` `message.not-found`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::ack_removes_the_message_and_a_second_ack_is_not_found`
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-004

**Caso:** Ack com token obsoleto (a mensagem foi reentregue a outro consumidor)  
**Então:** `409` `message.stale-receipt`, e **nada muda** (a mensagem continua do segundo consumidor).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`

---

### UC-ACK-005

**Caso:** Ack depois de vencido o prazo mas **antes** da reentrega  
**Então:** o token continua válido e o ack vale.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery`
  - `src/engine/messages/tests.rs::ack_after_the_deadline_but_before_redelivery_still_works`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`
  - `tests/messages.rs::ack_after_the_deadline_but_before_redelivery_works`

---

### UC-ACK-006

**Caso:** Ack de mensagem vencida por `ttl`  
**Então:** `404` `message.not-found`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_found_for_ack_and_visibility`
  - `tests/messages.rs::a_message_past_its_ttl_cannot_be_acked_or_extended`

---

### UC-ACK-007

**Caso:** Ack em distribuidor  
**Então:** `409` `topic.wrong-type`. Em tópico inexistente: `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::ack_checks_the_topic_first`
  - `tests/messages.rs::ack_validates_the_request_and_the_topic`

---

### UC-ACK-008

**Caso:** Sem quórum, o ack é cancelado (rollback)  
**Então:** `503` `cluster.quorum-unavailable` e a mensagem **continua disponível** (volta ao fim do prazo).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`

---

### UC-ACK-009

**Caso:** O ack é decidido por quórum **com o primário**  
**Então:** com o primário fora do quórum o ack não se efetiva.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`

---

### UC-ACK-010

**Caso:** Falha numa réplica fora do quórum, já decidido o ack  
**Então:** não cancela; o coordenador insiste até `write.commit.timeout` e, esgotado, o bloco fica abaixo do fator pleno (evento, não falha de saúde).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ACK-011

**Caso:** Na fila comum o ack remove a mensagem e os metadados sem deixar tombstone  
**Então:** a ausência do ID basta.  
**Nível:** I, C  
**Testes:**
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-012

**Caso:** Ack depois de a fila volátil ter sido excluída  
**Então:** `404` `topic.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::ack_and_visibility_after_the_topic_is_removed_are_404`

---

### UC-ACK-013

**Caso:** Ack de uma mensagem em disco (overflow) remove a linha do banco e o arquivo `.body`, nessa ordem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_message_on_disk_is_delivered_again_and_acked_like_any_other`
  - `tests/messages.rs::the_dead_letter_receives_the_body_file_of_a_message_on_disk`

---


## VIS: Alteração da invisibilidade

### UC-VIS-001

**Caso:** `PUT` com `receiptToken` válido e `visibilityTimeout` de 0 a `delivery.visibility-timeout.max`  
**Então:** `200` `VisibilityResponse` com `id` e o novo `invisibleUntil`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `src/engine/messages/tests.rs::visibility_works_on_a_message_on_disk`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-002

**Caso:** O novo prazo é `agora + visibilityTimeout`, **não** um acréscimo ao prazo atual (encurtar também funciona).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-003

**Caso:** `visibilityTimeout: 0` libera a mensagem na hora  
**Então:** o `invisibleUntil` devolvido é o instante atual e o próximo `GET` a entrega.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_zero_releases_the_message_at_once_and_wakes_a_long_poll`
  - `tests/messages.rs::visibility_zero_releases_the_message_for_the_next_get`

---

### UC-VIS-004

**Caso:** O `PUT` **não** incrementa `deliveryCount` e mantém o mesmo `receiptToken` (só uma nova entrega gera outro).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-005

**Caso:** Sem `receiptToken` (query) ou sem `visibilityTimeout` (corpo)  
**Então:** `400` `validation.required`. Tempo fora da faixa: `400` `validation.out-of-range`. Campo desconhecido no corpo: `400` `validation.unknown-field`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::missing_fields_are_reported_together`
  - `src/engine/messages/tests.rs::visibility_rejects_out_of_range_and_stale_tokens`
  - `tests/messages.rs::visibility_validates_query_and_body`

---

### UC-VIS-006

**Caso:** `PUT` com token obsoleto (mensagem reentregue)  
**Então:** `409` `message.stale-receipt`, nada muda.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_rejects_out_of_range_and_stale_tokens`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`

---

### UC-VIS-007

**Caso:** `PUT` de mensagem já confirmada, vencida por `ttl` ou de tópico removido  
**Então:** `404` (`message.not-found` ou `topic.not-found`).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_found_for_ack_and_visibility`
  - `tests/messages.rs::a_message_past_its_ttl_cannot_be_acked_or_extended`
  - `tests/messages.rs::ack_and_visibility_after_the_topic_is_removed_are_404`

---

### UC-VIS-009

**Caso:** Repetir o mesmo `PUT` dá o mesmo resultado (idempotente, por substituir o valor).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-010

**Caso:** Com o prazo estendido, a mensagem **não** é reentregue enquanto o novo prazo não vence.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-011

**Caso:** `PUT` com `0` numa mensagem que já está no `maxDeliveries`  
**Então:** vai para a dead-letter em vez de voltar à lista de prontos.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::visibility_zero_on_an_exhausted_message_sends_it_to_the_dead_letter`
  - `src/engine/messages/tests.rs::visibility_zero_below_the_limit_just_releases_the_message`
  - `tests/messages.rs::visibility_zero_on_the_last_delivery_sends_the_message_to_the_dead_letter`

---


## RET: Expiração (`ttl`), limite de entregas e dead-letter

### UC-RET-001

**Caso:** Mensagem com mais de `ttl` desde a publicação é descartada, entregue ou não, e **não** vai para a dead-letter.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_index_only_says_when_a_ttl_check_is_worth_a_read`
  - `src/engine/messages/tests.rs::sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter`
  - `src/engine/messages/tests.rs::ttl_and_the_dead_letter_work_on_messages_on_disk`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`
  - `src/engine/messages/tests/bounded.rs::the_ttl_removes_the_ready_messages_that_wait_outside_the_window`

---

### UC-RET-002

**Caso:** O prazo é `instante do ID + ttl` calculado com o `ttl` **atual** do tópico  
**Então:** reduzir o `ttl` por `PATCH` expira mensagens já guardadas.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_new_ttl_applies_to_messages_already_stored`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-RET-003

**Caso:** O padrão do `ttl` (14 dias) vale na hora da conferência  
**Então:** mudar `retention.ttl.default` afeta as filas sem `ttl` próprio, e não as que o definiram.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::changing_the_default_ttl_affects_the_queues_without_their_own_ttl_only`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, I.

---

### UC-RET-004

**Caso:** A expiração vale também para mensagem **invisível** (em processamento)  
**Então:** ela some, e o ack seguinte recebe `404`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-RET-005

**Caso:** O dono não entrega mensagem vencida entre duas varreduras.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_background_sweeper_moves_messages_without_anyone_consuming`
  - `tests/messages.rs::the_background_sweeper_moves_it_even_if_nobody_touches_the_source_queue`

---

### UC-RET-006

**Caso:** A varredura de cada nó (`retention.sweep.interval`) apaga as cópias vencidas em memória e em disco, sem propagar nada.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::lowering_max_deliveries_sends_the_ready_messages_over_the_new_limit_to_the_dead_letter`
  - `src/engine/messages/tests/cluster.rs::each_node_expires_its_own_replicas_by_ttl`
  - `tests/replication.rs::every_node_expires_its_own_copies_by_ttl_without_propagating`

---

### UC-RET-007

**Caso:** Cada mensagem expirada gera o evento `taba.message.expired` e conta em `taba.retention.expired`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::an_expired_message_raises_the_event_and_the_counters`
  - `tests/metrics.rs::redeliveries_dead_letters_and_expirations_are_counted`

---

### UC-RET-008

**Caso:** `maxDeliveries: N`  
**Então:** ao vencer a invisibilidade da N-ésima entrega sem ack, a mensagem vai para a dead-letter e a (N+1)-ésima entrega **nunca acontece**.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::after_max_deliveries_the_message_goes_to_the_dead_letter_with_the_same_id`
  - `src/engine/messages/tests.rs::an_acked_message_never_reaches_the_dead_letter`
  - `src/engine/messages/tests.rs::the_limit_counts_only_when_the_invisibility_ends_without_ack`
  - `src/engine/messages/tests.rs::ack_after_the_last_delivery_expired_finds_the_message_already_moved`
  - `src/engine/messages/tests.rs::the_background_sweeper_moves_messages_without_anyone_consuming`
  - `src/engine/messages/tests.rs::ttl_and_the_dead_letter_work_on_messages_on_disk`
  - `tests/messages.rs::the_message_goes_to_the_dead_letter_after_max_deliveries_without_ack`
  - `tests/messages.rs::an_acked_message_does_not_reach_the_dead_letter`
  - `tests/messages.rs::the_background_sweeper_moves_it_even_if_nobody_touches_the_source_queue`
  - `tests/messages.rs::the_dead_letter_receives_the_body_file_of_a_message_on_disk`

---

### UC-RET-009

**Caso:** Na dead-letter a mensagem mantém o **mesmo `id`**, com `deliveryCount: 0`, `invisibleUntil` zerado, `deadLetteredAt` e `deadLetterReason: max-deliveries`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::after_max_deliveries_the_message_goes_to_the_dead_letter_with_the_same_id`
  - `tests/messages.rs::the_message_goes_to_the_dead_letter_after_max_deliveries_without_ack`

---

### UC-RET-010

**Caso:** A mensagem na dead-letter cai no mesmo bloco e nos mesmos nós da mensagem original (mesmo `id`).  
**Nível:** U, C  
**Testes:**
  - `src/core/ring/tests.rs::the_dead_letter_copy_falls_in_the_same_block_and_nodes_as_the_original`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-RET-011

**Caso:** O movimento para a dead-letter é **uma operação** de registro + commit (cria na dead-letter e remove da fila de origem). Falha antes do quórum  
**Então:** rollback, a mensagem segue na fila e o dono tenta de novo na próxima volta.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure`

---

### UC-RET-012

**Caso:** A dead-letter não tem definição própria  
**Então:** não há linha na tabela `topics` e as configurações são as da fila de origem.  
**Nível:** I  
**Testes:**
  - `src/engine/topics/tests.rs::the_dead_letter_is_derived_from_its_queue_and_never_stored`

---

### UC-RET-013

**Caso:** A dead-letter não tem dead-letter  
**Então:** a mensagem lá só sai por ack ou por `ttl` (contado de `deadLetteredAt`).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_dead_letter_has_no_dead_letter_and_its_ttl_counts_from_deadlettering`
  - `tests/messages.rs::the_dead_letter_never_has_its_own_dead_letter`

---

### UC-RET-014

**Caso:** A dead-letter é um tópico comum para consumo, ack e publicação  
**Então:** dá para devolver uma mensagem corrigida à fila de origem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_dead_letter_message_can_be_published_back_to_the_source`
  - `tests/messages.rs::a_corrected_message_can_be_published_back_and_the_dead_one_acked`

---

### UC-RET-015

**Caso:** O sufixo da dead-letter é configurável (`topics.dead-letter.suffix`) e passa a ser o nome reservado.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::the_dead_letter_suffix_is_configurable_and_becomes_the_reserved_name`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, I.

---

### UC-RET-016

**Caso:** A dead-letter nunca é destino de um distribuidor, mesmo que o nome comece com o prefixo.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_distributor_never_copies_into_a_dead_letter`

---

### UC-RET-017

**Caso:** O `GET` na dead-letter não conta como consumidor da fila volátil de origem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_get_on_the_dead_letter_does_not_count_as_a_consumer`

---

### UC-RET-018

**Caso:** Cada mensagem enviada à dead-letter gera o evento `taba.message.dead_lettered` (fila, ID, `deliveryCount`) e conta em `taba.delivery.dead_lettered`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::a_dead_lettered_message_raises_the_event_and_the_counters`
  - `tests/metrics.rs::redeliveries_dead_letters_and_expirations_are_counted`

---


## DST: Tópico Distribuidor

### UC-DST-001

**Caso:** Publicar num distribuidor copia a mensagem para **cada** fila cujo nome começa com um dos prefixos  
**Então:** `200` `DistributorPublishResponse` com `matched` e uma entrada em `deliveries` por fila, cada cópia com `id` próprio.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-002

**Caso:** Cada cópia é independente  
**Então:** o mesmo conteúdo e `contentType`, `id` diferente, e consumo e ack de uma não afetam as outras.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-003

**Caso:** Nenhuma fila casa  
**Então:** `200` com `matched: 0` e `deliveries: []`, a mensagem é descartada, e o evento `taba.distributor.no_target` é registrado.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-004

**Caso:** Uma fila que casa com **dois ou mais prefixos do mesmo** distribuidor recebe **uma** cópia.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::a_queue_matching_two_prefixes_of_one_distributor_gets_one_copy`

---

### UC-DST-005

**Caso:** Uma fila que casa com prefixos de **distribuidores diferentes** recebe uma cópia de cada publicação em cada um.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-006

**Caso:** Só filas são destino  
**Então:** um distribuidor nunca recebe cópia (sem laços).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-007

**Caso:** O casamento é feito na hora da publicação  
**Então:** uma fila criada depois só recebe o que for publicado depois.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::the_match_is_made_when_publishing_and_a_new_prefix_counts_from_then_on`

---

### UC-DST-009

**Caso:** Cópia sem `groupId` para uma fila FIFO falha só naquela fila, com `validation.group-id-required` no item; as outras cópias seguem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_copy_to_a_fifo_queue_without_group_fails_only_that_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-010

**Caso:** O `groupId` e o `deduplicationId` informados vão para todas as cópias; sem `deduplicationId`, cada cópia usa o hash do corpo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::the_group_and_the_deduplication_key_go_to_every_copy`

---

### UC-DST-011

**Caso:** Com `topics.distributor.max-targets` definido e a publicação casando mais filas  
**Então:** `422` `distributor.too-many-targets`, e **nenhuma** cópia é feita. Sem o teto, não há limite.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_over_max_targets_refuses_the_whole_publication`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`
  - `tests/messages.rs::max_targets_refuses_the_whole_publication`

---

### UC-DST-012

**Caso:** Num distribuidor vale o `message.max-bytes` geral, e as cópias não são conferidas de novo contra o `maxMessageBytes` de cada fila.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_checks_the_general_size_and_not_that_of_each_queue`

---

### UC-DST-013

**Caso:** O `PATCH` de `prefixes` vale a partir do commit, só para publicações posteriores.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::the_match_is_made_when_publishing_and_a_new_prefix_counts_from_then_on`

---

### UC-DST-014

**Caso:** O distribuidor não guarda mensagens  
**Então:** não há mensagem em memória nem em disco para ele, e nenhuma lista de prontos.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_keeps_no_message_and_has_no_ack`

---

### UC-DST-015

**Caso:** Não há ordem entre as filas destino. O teste não pode exigir ordem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::a_queue_matching_two_prefixes_of_one_distributor_gets_one_copy`
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-016

**Caso:** `GET` e ack num distribuidor  
**Então:** `409` `topic.wrong-type`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_keeps_no_message_and_has_no_ack`

---

### UC-DST-017

**Caso:** A permissão `publish` é conferida só no distribuidor; as filas destino não são conferidas de novo (nem os prefixos do token).  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::publishing_to_a_distributor_checks_only_the_distributor`

---

### UC-DST-018

**Caso:** As métricas do distribuidor são só `received`, `copies` e `copyErrors` (as de fila se aplicam às filas destino).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::a_distributor_raises_no_target_and_partial_and_counts_its_copies`
  - `tests/topic_stats.rs::a_distributor_only_shows_what_it_received_copied_and_failed_to_copy`

---


## VOL: Fila volátil e nó gestor

### UC-VOL-001

**Caso:** Fila volátil sem nenhum `GET` por `idleTimeout` segundos é excluída, com as mensagens e a dead-letter, e o evento `taba.volatile.deleted` é registrado.  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests.rs::an_idle_volatile_queue_is_deleted_with_its_messages_and_dead_letter`
  - `src/engine/messages/tests.rs::persistent_queues_and_distributors_are_never_deleted_for_idleness`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`
  - `tests/volatile.rs::an_ordinary_queue_never_expires`
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-002

**Caso:** O relógio começa na **criação**  
**Então:** o prazo é `max(createdAt, último GET em qualquer nó) + idleTimeout`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::an_idle_volatile_queue_is_deleted_with_its_messages_and_dead_letter`
  - `src/engine/messages/tests.rs::the_clock_runs_from_the_creation_and_is_reset_by_each_get`
  - `src/engine/messages/tests.rs::a_recreated_queue_does_not_inherit_the_last_get_of_the_old_one`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`

---

### UC-VOL-003

**Caso:** **Qualquer** `GET` conta como consumidor (com ou sem mensagem, com ou sem long polling), e o instante é registrado no **início** da chamada.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::any_get_counts_with_or_without_messages_and_a_long_poll_counts_from_its_start`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`

---

### UC-VOL-004

**Caso:** Publicar **não** conta como consumidor  
**Então:** uma fila volátil só com produtores é excluída, e as mensagens se perdem.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::publishing_does_not_count_as_a_consumer`
  - `tests/volatile.rs::publishing_and_reading_the_dead_letter_do_not_keep_a_volatile_queue_alive`

---

### UC-VOL-005

**Caso:** O `GET` na dead-letter da fila volátil não conta como consumidor.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_get_on_the_dead_letter_does_not_count_as_a_consumer`
  - `tests/volatile.rs::publishing_and_reading_the_dead_letter_do_not_keep_a_volatile_queue_alive`

---

### UC-VOL-006

**Caso:** O gestor é o nó primário do hash do `id` do tópico no anel; qualquer nó chega ao mesmo gestor a partir das tabelas de tópicos e de membros.  
**Nível:** U, C  
**Testes:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`
  - `tests/volatile_cluster.rs::many_queues_are_managed_by_different_nodes_and_all_expire`

---

### UC-VOL-007

**Caso:** A lista de tópicos gerenciados é derivada, não guardada  
**Então:** cada nó a calcula localmente e ela muda sozinha com a tabela de membros ou de tópicos.  
**Nível:** U, C  
**Testes:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-VOL-008

**Caso:** No vencimento o gestor pergunta a **todos os nós** (`GET /cluster/topics/{id}/idle`) há quanto tempo foi o último `GET`; cada nó responde uma duração decorrida, não um instante.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-009

**Caso:** Se algum nó teve `GET` mais recente que o `idleTimeout`, a fila **não** é excluída  
**Então:** o gestor reagenda para `idleTimeout` menos a menor duração recebida.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-010

**Caso:** Se algum nó não responde, o gestor **não exclui** e tenta de novo depois de `topics.volatile.idle-check.retry-interval`; o evento `taba.volatile.check_blocked` é registrado.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive`

---

### UC-VOL-011

**Caso:** Um nó sem registro de `GET` para o tópico (subiu agora ou reiniciou) responde o tempo desde que subiu, com `sinceStart: true`  
**Então:** um reinício só pode **adiar** a exclusão.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start`

---

### UC-VOL-012

**Caso:** A exclusão usa o registro + commit de todos os nós com o `id` do tópico (`expectedId`)  
**Então:** uma fila excluída e **recriada** no meio do caminho não é apagada por engano.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::a_recreated_queue_is_not_deleted_by_the_old_deadline`

---

### UC-VOL-013

**Caso:** Corrida residual  
**Então:** um `GET` depois da verificação final e antes do commit recebe `404`, ou recria a fila (com `autoCreate`) com `id` novo e sem as mensagens antigas.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::a_get_after_the_final_deletion_is_not_found_or_recreates_an_empty_queue_with_a_new_id`

---

### UC-VOL-014

**Caso:** Um consumidor com mensagem invisível recebe `404` ao tentar o ack depois da exclusão.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::an_ack_after_the_queue_was_deleted_is_not_found`

---

### UC-VOL-015

**Caso:** Gestor fora do ar e ainda não desregistrado  
**Então:** nenhuma fila dele é excluída; a fila continua recebendo mensagens e consumidores. Desregistrado o nó, o novo gestor assume.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive`

---

### UC-VOL-016

**Caso:** Entrada de nó  
**Então:** o nó novo só gerencia depois de ficar pronto; até lá nenhuma fila desse trecho do anel é excluída. Os nós existentes abandonam as filas cuja posição passou ao novo.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-VOL-017

**Caso:** Saída de nó (commit do desregistro)  
**Então:** o novo primário das posições do nó removido assume as filas voláteis dele, sem transferir estado.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::when_the_manager_leaves_the_new_primary_of_its_positions_takes_over`

---

### UC-VOL-018

**Caso:** Falha no registro de nó e retorno de nó excluído  
**Então:** o conjunto de filas gerenciadas de cada nó volta ao anterior. Tópico criado ou excluído atualiza o conjunto ao aplicar o commit.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_managed_set_of_every_node_goes_back_when_the_excluded_node_returns`

---

### UC-VOL-019

**Caso:** Ao assumir uma fila, o novo gestor faz **imediatamente** a consulta de ociosidade e calcula o prazo real; muitas filas trocando de gestor espalham as consultas no tempo (`topics.volatile.takeover.jitter`).  
**Nível:** C  
**Testes:**
  - `src/cluster/volatile/tests.rs::the_takeover_delay_is_stable_per_queue_and_spreads_the_queries_inside_the_window`

---

### UC-VOL-020

**Caso:** Exclusão em andamento durante a troca de gestor  
**Então:** as duas tentativas são inofensivas (a segunda é rejeitada sem efeito pelo `id`).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::a_get_after_the_final_deletion_is_not_found_or_recreates_an_empty_queue_with_a_new_id`

---

### UC-VOL-021

**Caso:** `PATCH` de `idleTimeout` vale na próxima verificação; `volatile` e o tipo são imutáveis.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::patching_the_idle_timeout_applies_to_the_next_check`
  - `tests/volatile.rs::volatile_and_the_type_are_immutable_but_the_idle_timeout_can_change`

---

### UC-VOL-022

**Caso:** Uma fila FIFO pode ser volátil, com as mesmas regras.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_fifo_queue_can_be_volatile`

---

### UC-VOL-023

**Caso:** Exclusões de filas voláteis e mudanças de membros podem correr em paralelo, sem espera entre elas.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::many_queues_are_managed_by_different_nodes_and_all_expire`

---


## FIF: Fila FIFO

### UC-FIF-001

**Caso:** `fifo` é imutável  
**Então:** `PATCH` com `fifo` responde `400` `validation.immutable-field`. Um distribuidor não pode ser FIFO (`400` `validation.not-applicable`).  
**Nível:** I  
**Testes:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`

---

### UC-FIF-002

**Caso:** Publicar numa FIFO sem `groupId`  
**Então:** `400` `validation.group-id-required`. `groupId` inválido: `400`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::a_fifo_queue_requires_the_group_id`
  - `src/engine/messages/tests.rs::a_fifo_publish_needs_a_group_and_says_whether_it_was_a_duplicate`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-FIF-003

**Caso:** Publicar com `groupId` válido  
**Então:** `201` `PublishResponse` com `id` e `duplicate: false`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_fifo_publish_needs_a_group_and_says_whether_it_was_a_duplicate`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-004

**Caso:** Publicação duplicada (mesmo grupo, mesmo `deduplicationId`, dentro da janela)  
**Então:** `200` (não `201`) com o `id` da **original** e `duplicate: true`, e nenhuma mensagem nova é criada.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_repeated_publish_in_the_window_returns_the_original_and_creates_nothing`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-005

**Caso:** Sem `deduplicationId`, vale o SHA-256 dos **bytes** do corpo, em hexadecimal; o `contentType` não entra no hash. Duas mensagens idênticas no mesmo grupo viram uma só.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::without_a_deduplication_id_the_sha256_of_the_body_is_the_key`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-006

**Caso:** O `deduplicationId` é devolvido no `GET` (o informado ou o hash).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_repeated_publish_in_the_window_returns_the_original_and_creates_nothing`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-007

**Caso:** Depois da janela (`fifo.dedup.window`, padrão 300 s, contada do instante do ID da original), a mesma chave cria uma mensagem nova.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_window_ends_and_the_scope_is_the_group`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-008

**Caso:** O escopo é o grupo  
**Então:** o mesmo `deduplicationId` em grupos diferentes **não** é duplicata.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_window_ends_and_the_scope_is_the_group`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-009

**Caso:** Duas publicações simultâneas com a mesma chave são serializadas pelo primário  
**Então:** a segunda espera. Se a primeira commitou, devolve o mesmo `id`; se sofreu rollback, segue como nova.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::simultaneous_publications_with_the_same_key_create_one_message`

---

### UC-FIF-010

**Caso:** Repetir uma publicação que deu `502` ou `504` é seguro  
**Então:** o primário deduplica e a duplicata devolve o `id` da original.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-011

**Caso:** A deduplicação continua valendo **depois do ack** da original, enquanto a janela não passa (a linha com `deleteAt` ainda existe).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`
  - `tests/fifo.rs::deduplication_still_holds_after_the_ack`

---

### UC-FIF-012

**Caso:** A mensagem seguinte de um grupo só é entregue depois do **ack** da anterior (ou da saída dela por `ttl` ou dead-letter).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_next_message_of_a_group_waits_for_the_ack_of_the_previous_one`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-013

**Caso:** Grupos diferentes andam em paralelo  
**Então:** com dois grupos livres, o `GET` entrega uma mensagem de cada.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::different_groups_go_in_parallel_and_a_batch_never_has_two_of_the_same_group`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-014

**Caso:** Cada mensagem do lote do `GET` vem de um grupo **diferente**; nunca duas do mesmo grupo no mesmo lote.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::with_a_zero_visibility_the_group_is_free_again_after_the_batch`
  - `src/engine/messages/tests.rs::different_groups_go_in_parallel_and_a_batch_never_has_two_of_the_same_group`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-015

**Caso:** Dentro do grupo a ordem de entrega é a ordem do `id`. Um produtor que publica em sequência (espera a resposta) tem a ordem preservada; produtores concorrentes ficam na ordem em que chegam ao primário.  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-016

**Caso:** Reentrega  
**Então:** vencida a invisibilidade sem ack, a **mesma** mensagem volta como cabeça do grupo (`deliveryCount` sobe) e o grupo volta a ser livre; a ordem se mantém.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::an_expired_or_released_head_comes_back_first_and_the_order_holds`

---

### UC-FIF-017

**Caso:** `PUT .../visibility` com `0` na cabeça libera o grupo, como um vencimento.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::an_expired_or_released_head_comes_back_first_and_the_order_holds`

---

### UC-FIF-018

**Caso:** O ack da cabeça libera o grupo, e a mensagem seguinte entra na lista de prontos.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::the_next_message_of_a_group_waits_for_the_ack_of_the_previous_one`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-019

**Caso:** Entre grupos livres vale a cabeça mais antiga; a entrega justa por faixas **não** vale na FIFO.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_fifo_list_has_one_entry_per_free_group_the_oldest_head_first`
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`

---

### UC-FIF-020

**Caso:** A lista de prontos tem uma entrada por grupo livre (a cabeça), ordenada pelo ID; um grupo com mensagem em voo não está na lista.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_fifo_list_has_one_entry_per_free_group_the_oldest_head_first`
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`

---

### UC-FIF-021

**Caso:** `maxDeliveries` atingido pela cabeça  
**Então:** vai para a dead-letter e o grupo segue com a próxima; a ordem das restantes é mantida. A dead-letter de uma FIFO **não é FIFO**.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_head_over_max_deliveries_goes_to_the_dead_letter_and_the_group_goes_on`
  - `src/engine/messages/tests.rs::a_message_that_went_to_the_dead_letter_still_deduplicates_in_the_window`
  - `tests/fifo.rs::the_dead_letter_of_a_fifo_is_a_common_queue`

---

### UC-FIF-022

**Caso:** A cabeça vencida por `ttl` é descartada e o grupo é liberado para a próxima.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::a_head_past_its_ttl_is_dropped_and_the_group_is_freed`

---

### UC-FIF-023

**Caso:** A posição no anel é `hash(id do tópico + ":" + groupId)`  
**Então:** todas as mensagens de um grupo caem no mesmo bloco, com o mesmo primário e as mesmas réplicas; grupos diferentes têm posições diferentes.  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::a_fifo_group_has_one_position_for_all_its_messages`

---

### UC-FIF-024

**Caso:** Recriar o tópico (outro `id`) muda a posição dos grupos.  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::recreating_a_fifo_topic_moves_its_groups_in_the_ring`

---

### UC-FIF-025

**Caso:** A dead-letter de uma FIFO usa o `id` da fila de origem (e o `groupId`) para a posição, para o movimento continuar sendo uma operação só.  
**Nível:** U, C  
**Testes:**
  - `tests/fifo_cluster.rs::the_dead_letter_of_a_fifo_stays_in_the_block_of_the_group`

---

### UC-FIF-026

**Caso:** O nó que recebe a publicação a repassa ao **primário do grupo**, que coordena (gera o ID, confere a deduplicação, faz o registro + commit). Se o próprio nó é o primário, não há repasse.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-027

**Caso:** ID monotônico por grupo  
**Então:** `max(agora, último ID do grupo + 1)`, usando os bits de contador do GUID v7 no mesmo milissegundo; com o relógio do primário atrasado, os IDs ainda crescem.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_id_of_a_group_keeps_growing_even_if_the_clock_of_the_new_primary_is_behind`

---

### UC-FIF-028

**Caso:** O primário recalcula o dono do grupo ao receber a publicação repassada  
**Então:** se não for ele, responde `421` `fifo.misdirected`; o nó que repassa tenta de novo (relê a própria tabela), até `fifo.forward.max-attempts`, e esgotado o cliente recebe `503` `fifo.primary-unavailable` com `Retry-After`.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_node_that_is_not_the_primary_of_the_group_answers_421`

---

### UC-FIF-029

**Caso:** O primário do grupo ainda em sincronização (nó novo)  
**Então:** as publicações do grupo respondem `503` com `Retry-After`; o primário antigo **não** coordena.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-030

**Caso:** Primário fora do ar e ainda não desregistrado  
**Então:** as publicações do grupo respondem `503` até o desregistro.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-031

**Caso:** O quórum de escrita, de entrega e de ack **sempre inclui o primário e o secundário**, e nunca fica abaixo de 2; `write.quorum` maior que 2 também os inclui.  
**Nível:** U, C  
**Testes:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-032

**Caso:** Secundário fora do ar e ainda não desregistrado  
**Então:** escrita, entrega e ack do grupo respondem `503`, mesmo com o terciário saudável; some quando ele é desregistrado.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-033

**Caso:** Failover sem reordenação  
**Então:** o primário confirma `m1`, libera `m2`, e cai; o novo primário (o secundário) já tem o ack de `m1`, e o consumidor **não** vê `m1, m2, m1`.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-034

**Caso:** O novo primário herda a deduplicação sem perda (os registros existem em todos os nós).  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-035

**Caso:** Ao assumir um bloco, o novo primário refaz por grupo a cabeça, o estado livre ou ocupado (grupo com invisível no futuro entra ocupado) e a base do ID monotônico.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-036

**Caso:** O ack na FIFO **não apaga a linha**  
**Então:** grava `deleteAt` (= instante do ID + `fifo.dedup.window`) e apaga o corpo (`.body` e a cópia em memória) e os metadados de entrega; a linha curta mantém `id`, posição, `group_id`, `deduplicationId` e `deleteAt`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`
  - `src/storage/messages/tests.rs::a_tombstone_keeps_a_short_row_and_drops_the_body`
  - `src/storage/messages/tests.rs::a_tombstone_works_for_a_message_that_had_no_row`

---

### UC-FIF-037

**Caso:** Se `deleteAt` já passou no ack, a linha é apagada na hora, como na fila comum. O mesmo vale para a mensagem que vai à dead-letter.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::an_ack_after_the_window_deletes_the_row_at_once`

---

### UC-FIF-038

**Caso:** A linha com `deleteAt` existe em todos os nós, replicada pelo registro + commit do ack, decidido por quórum.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-039

**Caso:** A varredura de cada nó apaga as linhas vencidas pelo índice de `deleteAt` a cada `retention.sweep.interval`, sem propagar nada.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::the_sweep_deletes_the_rows_whose_delete_at_passed`
  - `src/storage/messages/tests.rs::sweeping_deletes_only_the_rows_whose_delete_at_passed`

---

### UC-FIF-040

**Caso:** A linha com `deleteAt` é ignorada pela lista de prontos, pelos contadores de fila e de grupo, por `depth`, pelo `ttl`, pelo `MIN(id)` de `oldestMessageAt`, pelo `GET`, pelo `PUT .../visibility` e por um segundo ack (que respondem como mensagem inexistente). Só a deduplicação e a varredura a enxergam.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`

---

### UC-FIF-041

**Caso:** A listagem de IDs e o stream de mensagens do bloco incluem a linha com `deleteAt`, **sem corpo**, e o banco de eventos grava o `deleteAt` de forma idempotente.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::the_short_line_of_a_confirmed_fifo_message_travels_without_a_body`
  - `src/storage/messages/tests.rs::the_short_fifo_row_is_listed_with_its_delete_at`
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-042

**Caso:** `autoCreate` aceita `fifo=true`, com qualquer `replication.factor` (UC-CNS-037).  
**Nível:** I, C  
**Testes:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-FIF-043

**Caso:** Um distribuidor com FIFO entre os destinos mantém a ordem por grupo em cada fila (cada cópia é coordenada pelo primário do grupo da fila destino).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests.rs::a_distributor_copy_without_a_group_fails_only_that_fifo_queue`
  - `tests/fifo_cluster.rs::a_distributor_keeps_the_order_by_group_in_a_fifo_target`

---

### UC-FIF-044

**Caso:** Métricas e condições da FIFO  
**Então:** `taba.fifo.deduplicated`, `taba.fifo.forward` (`ok`, `misdirected`, `error`), `taba.fifo.groups.busy`, evento `taba.message.deduplicated`, e a condição `group-stalled` quando a cabeça passa de `stats.topics.group-stalled-deliveries` entregas sem ack.  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::a_deduplicated_publication_raises_the_event_and_the_counters`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`
  - `tests/topic_stats.rs::group_stalled_fires_when_a_fifo_head_is_delivered_too_often_without_ack`

---

### UC-FIF-045

**Caso:** A condição `noisy-group` indica o grupo quente de uma FIFO.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::noisy_group_points_at_the_hot_group_of_a_fifo_queue`

---

### UC-FIF-048

**Caso:** A fila FIFO **durante a restauração**
**Então:** recebe mensagens e não as entrega até o status ser **restaurado do disco**; as filas comuns não ficam retidas.
**Nível:** C
**Testes:**
  - `src/engine/messages/tests/hold.rs::a_held_fifo_queue_receives_and_does_not_deliver_until_it_is_released`
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`

---

### UC-FIF-049

**Caso:** O **gerador de IDs** da vida nova
**Então:** começa acima da **marca d'água** (o maior `id` das vidas antigas, no manifesto de cada uma e anunciada entre os nós): os `id` novos são maiores que os antigos e nenhum `id` já devolvido a um produtor muda.
**Nível:** C
**Testes:**
  - `src/core/id/tests.rs::a_generator_with_a_floor_never_goes_below_it_and_reports_its_watermark`
  - `src/engine/messages/tests/hold.rs::the_ids_of_the_new_life_start_above_the_watermark_of_the_old_ones`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-050

**Caso:** Mensagens novas durante a restauração
**Então:** vão para a **lista provisória** do primário do grupo, com um `id` normal, e **nada é deduplicado**: toda publicação é aceita, inclusive a que repete uma chave.
**Nível:** C
**Testes:**
  - `src/engine/messages/tests/hold.rs::while_held_nothing_is_deduplicated_and_the_duplicate_answer_never_comes`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-051

**Caso:** A **liberação**
**Então:** o primário deduplica a lista provisória contra as chaves restauradas e dentro da própria lista, em ordem de `id`; descarta as duplicatas (um ack nos donos, sem recibo) e só então libera a entrega.
**Nível:** C
**Testes:**
  - `src/engine/messages/tests/hold.rs::the_release_lists_as_duplicates_what_repeats_a_restored_key_or_an_earlier_publication`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-FIF-052

**Caso:** O produtor de uma mensagem que é descartada como duplicata na liberação
**Então:** havia recebido `duplicate=false` (exceção aceita; PRB-063 para o limite da janela).
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-053

**Caso:** As mensagens FIFO restauradas
**Então:** chegam ao primário do grupo pelo caminho da restauração e ficam sem entrega até a liberação; mantêm a chave antes de qualquer provisória.
**Nível:** C
**Testes:**
  - `src/engine/messages/tests/hold.rs::the_release_lists_as_duplicates_what_repeats_a_restored_key_or_an_earlier_publication`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-047

**Caso:** O `receiptToken` de uma mensagem FIFO leva a posição do grupo (`{posição}-{guid}`)  
**Então:** o ack e a alteração da invisibilidade, de qualquer nó, acham o bloco sem saber o grupo; recibo malformado é `409` `message.stale-receipt`.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-046

**Caso:** Num nó só (perfil de um nó, fator 1) a FIFO funciona igual  
**Então:** o nó é o primário de todos os grupos (sem repasse), o quórum vale 1 e `replication.factor` 1 é aceito.  
**Nível:** I  
**Testes:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`
  - `tests/fifo.rs::a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`

---


## GRP: Grupos de mensagens e entrega justa

### UC-GRP-001

**Caso:** O `groupId` é opcional; a mensagem sem grupo pertence a um grupo vazio. Um tópico em que ninguém usa `groupId` tem um grupo só e se comporta como a lista de prontos simples.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/index/tests.rs::messages_without_a_group_are_one_group_of_the_empty_name`

---

### UC-GRP-002

**Caso:** O `groupId` é imutável, guardado com a mensagem (memória ou coluna `group_id`) e devolvido no `GET`; na dead-letter a mensagem mantém o grupo.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-003

**Caso:** Numa fila comum o grupo **não** ordena nem serializa a entrega  
**Então:** duas mensagens do mesmo grupo podem estar em voo ao mesmo tempo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_small_group_is_delivered_before_the_big_one_is_exhausted`
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-004

**Caso:** Cada dono mantém por grupo os contadores de **prontas** e **em processamento**. Publicação  
**Então:** +1 pronta. Entrega: pronta vira em processamento. Vencimento da invisibilidade (e `PUT` com `0`): volta a pronta. Ack, expiração por `ttl` e dead-letter descontam.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_counters_follow_publish_delivery_expiry_ack_and_dead_letter`

---

### UC-GRP-005

**Caso:** `tamanho = em processamento + peso × prontas` com o peso de `delivery.fair.ready-weight` (padrão 0,1). Com peso `0` só conta o que está em processamento; com `1` conta o backlog inteiro.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_group_size_weighs_ready_messages_by_the_configured_weight`

---

### UC-GRP-006

**Caso:** A faixa é `⌊log2(tamanho)⌋`, e tamanho menor que 1 conta como 1  
**Então:** tamanho 1 é a faixa 0, 2 e 3 a faixa 1, 4 a 7 a faixa 2.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_band_is_the_floor_of_log2_with_a_minimum_size_of_one`

---

### UC-GRP-007

**Caso:** A reposição da lista de prontos percorre os grupos do menor para o maior e toma **uma** mensagem de cada grupo por rodada, até encher a lista.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`

---

### UC-GRP-008

**Caso:** A lista fica ordenada pela faixa e, dentro dela, pela sanfona. Uma mensagem nova entra na posição da faixa do seu grupo, à frente das de grupos maiores.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`

---

### UC-GRP-009

**Caso:** Ao tirar um ID, o `GET` confere a faixa atual do grupo  
**Então:** se mudou desde que o ID entrou na lista, o ID é reposicionado e o próximo é considerado.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_band_is_checked_again_at_every_pick`

---

### UC-GRP-010

**Caso:** Efeito observável  
**Então:** um grupo com 10 mil mensagens e outro com 5 mensagens publicados juntos: as 5 do grupo pequeno são entregues antes de o grande ser esgotado.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`
  - `src/engine/messages/tests.rs::a_small_group_is_delivered_before_the_big_one_is_exhausted`
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-011

**Caso:** Progresso garantido  
**Então:** a cada `delivery.fair.normal-every` entregas (padrão 10), uma pega a mensagem mais antiga da lista, de qualquer grupo; o grupo grande nunca espera sem limite. Com `0`, a garantia é desligada.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/index/tests.rs::every_nth_delivery_takes_the_oldest_of_any_group`
  - `src/engine/messages/index/tests.rs::with_normal_every_zero_the_guarantee_is_off`
  - `src/engine/messages/tests.rs::every_nth_delivery_takes_the_oldest_message_so_the_big_group_never_starves`

---

### UC-GRP-012

**Caso:** Acima de `delivery.fair.max-groups`, os grupos novos compartilham um contador único (tratados como um grupo só), que aparece em `GroupStats` com `overflow: true` e sem `groupId`.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::groups_above_the_ceiling_share_one_counter`
  - `tests/topic_stats.rs::the_shared_counter_shows_as_overflow_and_an_empty_group_disappears`
  - `tests/fifo.rs::a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`

---

### UC-GRP-013

**Caso:** As contagens por grupo são derivadas  
**Então:** refeitas dos metadados e da coluna `group_id` quando o nó assume blocos ou reinicia.  
**Nível:** C  
**Testes:**

---

### UC-GRP-014

**Caso:** Num distribuidor, o `groupId` é repassado a todas as cópias.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/distributor.rs::the_group_and_the_deduplication_key_go_to_every_copy`

---

### UC-GRP-015

**Caso:** `GET /metrics/topics/{topic}/groups` devolve mensagens, parcela, idade e taxas por `groupId`; `top=N` devolve os `N` maiores sem paginação; `groupId=` devolve só esse; um grupo sem mensagens e sem atividade some.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::group_snapshots_show_counts_bands_and_the_oldest_of_each_group`
  - `tests/topic_stats.rs::the_groups_route_lists_pages_filters_and_tops_the_groups`
  - `tests/topic_stats.rs::the_shared_counter_shows_as_overflow_and_an_empty_group_disappears`

---

### UC-GRP-016

**Caso:** A condição `noisy-group` dispara quando há dois ou mais grupos e o maior ocupa mais de `stats.topics.noisy-group-share` do `depth`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/stats/tests.rs::noisy_group_needs_two_groups_and_more_than_the_share`
  - `tests/topic_stats.rs::noisy_group_fires_with_two_groups_and_the_largest_over_the_share`

---

### UC-GRP-017

**Caso:** A métrica `taba.delivery.fair.wait` mede a espera por faixa  
**Então:** as faixas baixas esperam menos que as altas.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/stats/tests.rs::the_wait_is_kept_per_band_and_the_last_band_is_15_or_more`
  - `tests/metrics.rs::the_wait_is_measured_per_band_and_small_groups_wait_less`

---


## AUT: Autenticação e tokens de acesso

### UC-AUT-001

**Caso:** Sem token raiz (sem `root-token` na pasta de segredos, sem `auth.root-token-file` e sem `auth.root-token`), nenhuma rota de cliente exige nada.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::without_a_root_token_nothing_matches`
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-002

**Caso:** Com um token raiz (pela pasta de segredos, pelo arquivo ou por `auth.root-token`), a autenticação **liga sozinha** e as rotas de cliente exigem `Authorization: Bearer`.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::an_inline_root_token_matches_only_itself`
  - `tests/auth.rs::with_a_root_token_every_client_route_asks_for_a_valid_one_and_the_401_never_says_why`
  - `tests/secrets.rs::a_node_mounting_the_folder_from_init_turns_authentication_on_by_itself`

---

### UC-AUT-003

**Caso:** `auth.enabled=false` força a autenticação desligada, mesmo com a pasta montada.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::auth_disabled_overrides_the_root_token`

---

### UC-AUT-004

**Caso:** `auth.enabled=true` sem token raiz impede o nó de subir; um `auth.root-token-file` definido que não existe também.  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::forcing_auth_without_a_root_token_stops_startup`
  - `src/config/tests.rs::a_named_root_token_file_that_does_not_exist_stops_startup`

---

### UC-AUT-005

**Caso:** Token raiz com menos de 32 caracteres  
**Então:** o nó não sobe.  
**Nível:** I  
**Testes:**
  - `src/secrets/tests.rs::init_creates_the_certificate_in_tls_and_a_root_token_of_at_least_32_characters`
  - `src/secrets/tests.rs::generated_root_tokens_are_random_and_long_enough`

---

### UC-AUT-006

**Caso:** O `auth.root-token-file` aceita duas linhas (a atual e a anterior) e é relido a cada `auth.reload-interval`  
**Então:** trocar o segredo não reinicia o nó, os dois valem durante a troca, e os tokens já criados seguem valendo.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::the_root_token_file_takes_two_lines_and_is_reloaded`
  - `src/http/auth/tests.rs::a_failed_reload_keeps_the_previous_secret`
  - `tests/auth.rs::the_root_token_file_is_reloaded_without_restarting`

---

### UC-AUT-007

**Caso:** Sem token raiz e sem `auth.dev-token` a autenticação não é exigida.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-008

**Caso:** Sem token, mal formado, desconhecido, revogado ou vencido  
**Então:** `401` `auth.unauthorized` com `WWW-Authenticate: Bearer`; a resposta é igual nos cinco casos (não diz qual).  
**Nível:** U, I  
**Testes:**
  - `src/core/token/tests.rs::malformed_tokens_do_not_parse`
  - `src/engine/tokens/tests.rs::every_invalid_token_is_just_none`
  - `src/engine/tokens/tests.rs::an_expired_token_is_refused_and_the_sweep_erases_it`
  - `tests/auth.rs::with_a_root_token_every_client_route_asks_for_a_valid_one_and_the_401_never_says_why`

---

### UC-AUT-009

**Caso:** `publish` permite só `POST /topics/{topic}/messages`; `consume` permite `GET`, `PUT .../visibility` e `DELETE` de mensagens; `create` permite `POST /topics` e o `autoCreate` (junto de `consume`); `manage` permite `PATCH` e `DELETE` em `/topics/{topic}` **e inclui `create`**; `admin` (os tokens e o cluster) inclui `manage` e `create`. Cada rota fora da permissão  
**Então:** `403` `auth.forbidden`.  
**Nível:** U, I  
**Testes:**
  - `src/core/token/tests.rs::manage_includes_create_and_admin_includes_manage_and_nothing_else_does`
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-010

**Caso:** `create` só **cria**  
**Então:** alterar e remover tópicos exige `manage` (ou `admin`, que o inclui).  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-011

**Caso:** Todo token válido consulta `GET /topics` e `GET /topics/{topic}` dentro dos seus prefixos.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-012

**Caso:** Prefixos de tópico  
**Então:** o token só alcança tópicos cujo nome **começa com** algum prefixo; um tópico fora: `403` `auth.out-of-scope`. Sem prefixos, alcança todos.  
**Nível:** U, I  
**Testes:**
  - `src/core/token/tests.rs::prefixes_limit_the_topics_a_token_reaches`
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`
  - `tests/volatile.rs::auto_create_stays_inside_the_token_prefixes`

---

### UC-AUT-013

**Caso:** `GET /topics` com token de prefixos devolve só os tópicos dentro deles.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`

---

### UC-AUT-014

**Caso:** `POST /topics` com `create` só aceita nome dentro dos prefixos; num distribuidor, também cada um dos seus `prefixes` de distribuição (senão o token injetaria mensagens em filas fora do alcance). O `PATCH` de `prefixes` segue a mesma regra.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::a_distribution_prefix_must_sit_inside_the_token_prefixes`
  - `tests/auth.rs::a_token_only_creates_inside_its_prefixes_including_the_distribution_prefixes`

---

### UC-AUT-015

**Caso:** A dead-letter é alcançada pelo mesmo prefixo da fila de origem.  
**Nível:** I  
**Testes:**
  - `src/core/token/tests.rs::prefixes_limit_the_topics_a_token_reaches`
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`

---

### UC-AUT-016

**Caso:** Num distribuidor, a permissão é conferida só nele; as cópias nas filas destino não são conferidas de novo.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::publishing_to_a_distributor_checks_only_the_distributor`

---

### UC-AUT-017

**Caso:** O token raiz tem acesso total (todas as permissões, sem prefixos) e é o único que usa `/tokens`; nos logs e traces aparece como `token.id=root`.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::root_and_open_reach_everything_and_a_token_only_what_it_has`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::only_the_root_and_admin_tokens_manage_tokens`

---

### UC-AUT-018

**Caso:** Token com `expiresAt` vencido é recusado com `401`; cada nó o ignora e o apaga na varredura local (`retention.sweep.interval`), sem propagar.  
**Nível:** C  
**Testes:**
  - `src/core/token/tests.rs::a_token_expires_at_the_given_instant`
  - `src/engine/tokens/tests.rs::an_expired_token_is_refused_and_the_sweep_erases_it`
  - `tests/auth.rs::an_expired_token_is_a_401`

---

### UC-AUT-020

**Caso:** O formato do token é `tb_<id>.<segredo>`, com `id` GUID v7 e segredo de 256 bits aleatórios; o cluster guarda só o hash salgado do segredo (`SHA-256(sal ‖ segredo)`, sal aleatório por token).  
**Nível:** U  
**Testes:**
  - `src/core/token/tests.rs::the_token_is_tb_id_dot_a_256_bit_secret`
  - `src/core/token/tests.rs::only_a_salted_hash_is_kept_and_it_verifies_the_secret`
  - `src/core/token/tests.rs::every_token_has_its_own_salt_so_equal_secrets_hash_differently`
  - `src/engine/tokens/tests.rs::creating_returns_the_full_token_once_and_it_authenticates`

---

### UC-AUT-021

**Caso:** O token completo e o segredo nunca vão para logs nem traces; só o `id` (`token.id`). O `id` do token nunca é atributo de métrica.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/metrics.rs::auth_requests_are_counted_by_outcome_and_the_token_id_is_never_a_label`

---

### UC-AUT-022

**Caso:** `POST /tokens` com o token raiz  
**Então:** `201` `TokenCreated`, com `Location: /tokens/{id}` e o campo `token` (o segredo completo), que aparece **só aqui**.  
**Nível:** I  
**Testes:**
  - `src/engine/tokens/tests.rs::creating_returns_the_full_token_once_and_it_authenticates`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`

---

### UC-AUT-023

**Caso:** `POST /tokens` inválido (`permissions` vazia ou desconhecida, `name` fora do padrão, prefixo inválido ou acima de 20, `expiresAt` no passado)  
**Então:** `400`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/tokens/tests.rs::invalid_requests_are_400_with_the_field`
  - `tests/auth.rs::an_invalid_token_request_is_400`

---

### UC-AUT-024

**Caso:** `/tokens` com um token que não é o raiz  
**Então:** `403` `auth.forbidden`. Sem token: `401`.  
**Nível:** I  
**Testes:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::only_the_root_and_admin_tokens_manage_tokens`

---

### UC-AUT-025

**Caso:** `/tokens` com a autenticação desligada  
**Então:** `404` `route.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-026

**Caso:** `GET /tokens` é paginada, nunca devolve o segredo, aceita `sort` por `id` e `createdAt` (não `name`, que é alterável  
**Então:** `400`) e o filtro `name` por trecho.  
**Nível:** I  
**Testes:**
  - `src/engine/tokens/tests.rs::listing_is_paginated_by_id_or_creation_and_filters_by_name`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::listing_tokens_is_paginated_filtered_and_refuses_sorting_by_name`

---

### UC-AUT-027

**Caso:** `GET /tokens/{id}`  
**Então:** `200` `Token`, ou `404` `token.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-028

**Caso:** `PATCH /tokens/{id}` altera só `name` e `expiresAt` (`null` remove o prazo); permissões e prefixos são imutáveis (`400` `validation.immutable-field`).  
**Nível:** I  
**Testes:**
  - `src/engine/tokens/tests.rs::patch_changes_only_the_name_and_the_expiry`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-029

**Caso:** `DELETE /tokens/{id}`  
**Então:** `204`; as chamadas seguintes com o token respondem `401`, em **todos** os nós, a partir do commit; um segundo `DELETE`: `404`.  
**Nível:** I, C  
**Testes:**
  - `src/engine/tokens/tests.rs::revoking_makes_the_token_unknown_and_a_second_revoke_is_404`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-032

**Caso:** Cada token é uma linha da tabela `tokens` do `_node.db`, gravada no commit em cada nó, com o sal e o hash do segredo (nunca o segredo).  
**Nível:** I, C  
**Testes:**
  - `src/engine/tokens/tests.rs::a_failed_write_leaves_no_visible_token`
  - `src/engine/tokens/tests.rs::the_store_keeps_the_tokens_that_were_created`
  - `src/storage/tokens/tests.rs::tokens_are_rows_of_the_node_db_with_the_hash_and_the_salt`
  - `src/storage/tokens/tests.rs::the_secret_is_never_in_the_database_file`

---

### UC-AUT-035

**Caso:** As rotas de operação (na porta de operação) e as do cluster não usam o token de acesso.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::the_operations_routes_do_not_use_the_access_token`

---

### UC-AUT-036

**Caso:** Eventos  
**Então:** token criado, alterado e revogado (`info`, com o `id`); chamada recusada por `401`/`403` (`warn`, amostrada em volume). `taba.auth.requests` conta por `outcome` (`ok`, `unauthorized`, `forbidden`) e `taba.auth.tokens` traz quantos o nó conhece.  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::auth_requests_are_counted_by_outcome_and_the_token_id_is_never_a_label`
  - `tests/metrics.rs::without_authentication_there_are_no_auth_series`

---

### UC-AUT-037

**Caso:** Um token válido chama uma rota para a qual não tem permissão  
**Então:** O `403 auth.forbidden` traz `required`: `root` nas rotas só do raiz, o nome da permissão (`manage`, `admin`...) nas demais; os outros erros não têm esse campo.  
**Nível:** U, I  
**Testes:**
  - `src/core/error/tests.rs::a_forbidden_error_says_what_was_missing_and_other_errors_leave_the_field_out`
  - `tests/nodes_api.rs::a_forbidden_answer_says_what_the_token_lacked`

---

### UC-AUT-038

**Caso:** `GET /auth/me`  
**Então:** Qualquer token válido recebe seu `id`, `name`, `permissions`, `topicPrefixes`, `expiresAt` e `root: false`; o token raiz recebe `root: true` e todas as permissões; sem token, `401`.  
**Nível:** I  
**Testes:**
  - `tests/nodes_api.rs::auth_me_says_who_the_token_is_and_what_it_can_do`

---

### UC-AUT-039

**Caso:** O que um token que não é o raiz pode ver: `GET /nodes`, `GET /stats/topics`, `/stats/topics/{topic}` e `/groups`  
**Então:** `GET /nodes` lista os nós sem endereços; a lista de estatísticas só traz os tópicos dentro dos prefixos do token e `/stats/topics/{topic}` fora deles é `403 auth.out-of-scope`; as rotas de cluster e de nó pedem o token raiz.  
**Nível:** U, I  
**Testes:**
  - `src/http/auth/tests.rs::the_monitoring_routes_split_between_any_token_and_the_root_token`
  - `tests/nodes_api.rs::any_valid_token_lists_the_nodes_and_sees_the_statistics_of_its_prefixes`

---


### UC-AUT-040

**Caso:** Na porta do cliente as sondas são abertas e `/metrics` aceita qualquer token válido  
**Então:** `GET /health/live` e `GET /health/ready` respondem sem token; `GET /metrics` responde `401` sem token ou com token inválido e `200` com qualquer token válido (sem permissão específica); o resto das rotas de operação não existe ali (`404`). Na porta de operação `/metrics` não pede token.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::on_the_client_port_the_probes_are_open_and_the_metrics_take_any_valid_token`

---

### UC-AUT-041

**Caso:** Toda rota do documento de cliente tem sua exigência decidida na tabela de autorização
**Então:** uma rota é casada pelo gabarito com que foi registrada, e os documentos a registram com o gabarito que mostram; toda rota documentada, exceto as sondas (abertas de propósito), tem uma entrada em `need()`, então renomear um parâmetro de caminho não pode fazer uma rota escapar da sua permissão.
**Nível:** U
**Testes:**
  - `src/http/auth/tests.rs::every_route_of_the_client_document_has_its_requirement_decided`

---

## LIM: Rate limit e IP do cliente

### UC-LIM-001

**Caso:** Excedido o limite de publicação, consumo ou administrativo  
**Então:** `429` `rate-limit.exceeded` com `Retry-After`.  
**Nível:** I  
**Testes:**
  - `src/http/ratelimit/tests.rs::the_retry_after_is_the_wait_for_a_whole_token`
  - `src/http/ratelimit/tests.rs::a_category_without_a_rate_is_unlimited`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`
  - `tests/ratelimit.rs::admin_and_consume_have_their_own_counters`
  - `tests/ratelimit.rs::without_configured_rates_nothing_is_limited`

---

### UC-LIM-002

**Caso:** As categorias são independentes  
**Então:** publicação (`POST .../messages`), consumo (`GET`, `PUT .../visibility`, `DELETE` de mensagem) e administrativo (`/topics`, `/tokens`). Estourar uma não afeta as outras.  
**Nível:** I  
**Testes:**
  - `src/http/ratelimit/tests.rs::categories_and_clients_count_separately`
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`
  - `tests/ratelimit.rs::admin_and_consume_have_their_own_counters`

---

### UC-LIM-003

**Caso:** O limite é taxa mais estouro (`ratelimit.burst`)  
**Então:** picos curtos dentro do burst não são rejeitados.  
**Nível:** U, I  
**Testes:**
  - `src/http/ratelimit/tests.rs::a_burst_is_absorbed_and_then_the_rate_applies`
  - `src/http/ratelimit/tests.rs::without_a_burst_the_capacity_is_one_second_of_the_rate`
  - `src/http/ratelimit/tests.rs::tokens_never_pile_up_past_the_burst`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`

---

### UC-LIM-004

**Caso:** Com token válido (inclusive o raiz), a chave do contador é o `id` do token em cada categoria, mesmo mudando de IP; um `403` também conta no token.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-005

**Caso:** Sem token válido (autenticação desligada, `401`, rotas de operação), a chave é o IP do cliente. Tentar adivinhar tokens não fica sem limite.  
**Nível:** I  
**Testes:**
  - `src/http/ratelimit/tests.rs::idle_counters_are_dropped_when_the_map_grows`
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`
  - `tests/ratelimit.rs::without_a_token_the_counter_is_per_client_ip_from_the_headers`

---

### UC-LIM-006

**Caso:** A autenticação vem antes do rate limit, e um token só vira chave **depois de validado** (ninguém esgota o limite de outro citando o `id` dele).  
**Nível:** U, I  
**Testes:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-007

**Caso:** Todos os tokens têm o mesmo limite; não há limite próprio por token.  
**Nível:** I  
**Testes:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-009

**Caso:** `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` nunca respondem `429`.  
**Nível:** I  
**Testes:**
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::probes_are_exempt_and_the_cluster_query_has_its_own_category`

---

### UC-LIM-010

**Caso:** `/health/cluster?scope=cluster` e `/metrics/topics` têm categoria própria (`ratelimit.ops.rps`, por IP, com o mesmo burst).  
**Nível:** I  
**Testes:**
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::probes_are_exempt_and_the_cluster_query_has_its_own_category`

---

### UC-LIM-011

**Caso:** O IP do cliente vem do primeiro endereço do primeiro cabeçalho presente de `client-ip.headers` (padrão `X-Forwarded-For`), só se a conexão vier de `client-ip.trusted-proxies` (vazia  
**Então:** aceita de qualquer origem). Cabeçalho ausente ou inválido: vale o endereço da conexão.  
**Nível:** U, I  
**Testes:**
  - `src/http/client_ip/tests.rs::the_first_address_of_the_first_present_header_wins`
  - `src/http/client_ip/tests.rs::a_missing_or_invalid_header_falls_back_to_the_connection`
  - `src/http/client_ip/tests.rs::addresses_with_a_port_and_ipv6_are_understood`
  - `src/http/client_ip/tests.rs::headers_are_only_trusted_from_the_trusted_proxies`
  - `tests/ratelimit.rs::without_a_token_the_counter_is_per_client_ip_from_the_headers`
  - `tests/ratelimit.rs::the_headers_only_count_from_the_trusted_proxies`

---

### UC-LIM-012

**Caso:** Com `client-ip.headers.enabled=false` vale sempre o endereço da conexão.  
**Nível:** U, I  
**Testes:**
  - `src/http/client_ip/tests.rs::disabled_headers_always_use_the_connection`
  - `tests/ratelimit.rs::with_the_headers_disabled_a_forged_one_does_not_escape_the_limit`

---

### UC-LIM-013

**Caso:** O IP resolvido vale também para `operations.allowed-ips`.  
**Nível:** I  
**Testes:**
  - `tests/ratelimit.rs::the_resolved_ip_is_the_one_checked_against_allowed_ips`

---

### UC-LIM-015

**Caso:** `taba.ratelimit.rejected` conta as rejeições por `category` (`publish`, `consume`, `admin`, `ops`) e `key` (`token` ou `ip`).  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::rate_limit_rejections_are_counted_by_category_and_key_and_metrics_is_exempt`

---


## PAG: Paginação

### UC-PAG-001

**Caso:** A resposta é `Page<T>`  
**Então:** `items` e, havendo mais, `nextCursor`; **na última página `nextCursor` não existe**.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-002

**Caso:** `limit` fora da faixa  
**Então:** `400` `validation.out-of-range`. `cursor` malformado: `400` `validation.invalid-cursor`.  
**Nível:** U, I  
**Testes:**
  - `src/core/pagination/tests.rs::malformed_cursor_is_invalid_cursor`
  - `src/core/pagination/tests.rs::limit_must_be_between_one_and_the_maximum`
  - `src/engine/topics/tests.rs::a_cursor_only_works_with_the_sort_and_filter_that_made_it`
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-003

**Caso:** Percorrer todas as páginas devolve cada item que existia durante o percurso inteiro, **sem repetir**.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-005

**Caso:** Cursor usado com `sort` ou filtros diferentes dos que o geraram  
**Então:** `400` `validation.invalid-cursor`.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::a_cursor_only_works_with_the_sort_and_filter_that_made_it`
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-006

**Caso:** Repetir um cursor devolve a mesma página enquanto os dados não mudarem, e o cursor não expira.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-007

**Caso:** A ordem é total e estável  
**Então:** toda ordenação termina num campo único e imutável (nome ou `id`); `sort=-createdAt` vale `(createdAt desc, id asc)`; itens com o mesmo `createdAt` não se perdem entre páginas.  
**Nível:** U, I  
**Testes:**
  - `src/engine/topics/tests.rs::walking_every_page_returns_each_item_once_even_with_equal_creation_times`

---

### UC-PAG-008

**Caso:** Não há `page`, `offset` nem `total`.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-009

**Caso:** `GET /metrics/topics` não aceita `sort` (`400`) e pagina sempre por nome.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::pages_by_name_and_validates_the_parameters`

---

## CNV: Convenções comuns da API (formato, cabeçalhos, erros, status)

### UC-CNV-001

**Caso:** JSON em UTF-8, campos em `camelCase`, enumerações em minúsculas com hífen, datas RFC 3339 em UTC com milissegundos (`2026-10-06T12:00:00.000Z`), durações em segundos inteiros, tamanhos em bytes, identificadores GUID v7 em minúsculas.  
**Nível:** U, I  
**Testes:**
  - `src/core/id/tests.rs::generated_ids_are_lowercase_canonical_v7`
  - `src/core/time/tests.rs::formats_as_rfc3339_utc_with_milliseconds`

---

### UC-CNV-002

**Caso:** Campo ausente = não se aplica ao recurso; campo `null` = aplica-se e está sem valor próprio (`ttl: null` usa o padrão).  
**Nível:** I  
**Testes:**
  - `src/core/topic/tests.rs::queue_json_has_null_for_defaults_and_distributor_json_omits_queue_fields`

---

### UC-CNV-003

**Caso:** Requisição com corpo JSON e `Content-Type` diferente de `application/json`  
**Então:** `415` `media.unsupported`. A exceção é a publicação de mensagem, que aceita qualquer media type.  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::a_json_route_with_another_content_type_is_415`

---

### UC-CNV-004

**Caso:** `X-Request-Id` de 1 a 128 caracteres de `A-Za-z0-9_-.:`  
**Então:** devolvido igual na resposta. Sem ele, o nó gera um GUID v7. Está **sempre** na resposta e igual ao `requestId` do corpo de erro.  
**Nível:** I  
**Testes:**
  - `src/http/common/tests.rs::client_request_id_is_echoed_in_header_and_error_body`
  - `src/http/common/tests.rs::missing_or_invalid_request_id_is_replaced_by_a_guid_v7`
  - `src/http/common/tests.rs::unknown_route_is_a_404_error_response_with_matching_request_id`
  - `tests/listeners.rs::every_response_carries_the_request_id`

---

### UC-CNV-006

**Caso:** Toda resposta de erro (`4xx` e `5xx`) de rota JSON é `ErrorResponse`  
**Então:** `code` (formato `grupo.motivo`), `message`, `requestId` e, quando há mais de um problema, `details` (`field`, `code`, `message`). O status vai só na linha de status.  
**Nível:** U, I  
**Testes:**
  - `src/core/error/tests.rs::codes_follow_group_dot_reason`
  - `src/core/error/tests.rs::error_response_has_no_status_nor_retry_after`
  - `src/core/error/tests.rs::details_carry_field_code_and_message`
  - `src/http/common/tests.rs::unknown_route_is_a_404_error_response_with_matching_request_id`

---

### UC-CNV-007

**Caso:** `Retry-After` só no cabeçalho (nunca no corpo), em `429`, `503` e `403` por freezing; `WWW-Authenticate: Bearer` no `401`; `Location` no `201`.  
**Nível:** I  
**Testes:**
  - `src/http/common/tests.rs::retry_after_goes_only_in_the_header_and_401_asks_for_a_bearer_token`

---

### UC-CNV-008

**Caso:** Cada `code` do catálogo tem um status HTTP fixo; o catálogo inteiro é conferido por teste de tabela (código → status).  
**Nível:** U  
**Testes:**
  - `src/core/error/tests.rs::every_code_has_the_documented_status`

---

### UC-CNV-009

**Caso:** Campo desconhecido na requisição  
**Então:** `400` `validation.unknown-field`. Campo obrigatório ausente: `400` `validation.required`, com o campo em `details`.  
**Nível:** U, I  
**Testes:**
  - `tests/topics.rs::unknown_and_missing_fields_are_400_naming_the_field`

---

### UC-CNV-010

**Caso:** `POST`, `PATCH` e `DELETE` que não esperam corpo ignoram o que vier; `204` nunca tem corpo.  
**Nível:** I  
**Testes:**
  - `tests/operations.rs::a_post_that_expects_no_body_ignores_the_one_it_gets`
  - `tests/topics.rs::a_delete_ignores_the_body_it_gets_and_the_204_has_none`

---

### UC-CNV-011

**Caso:** A porta de cliente responde `404` `route.not-found` a `/cluster/*`; a porta do cluster responde `404` a tudo que não seja `/cluster/*`.  
**Nível:** I, C  
**Testes:**
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`

---

### UC-CNV-012

**Caso:** Falha inesperada do nó  
**Então:** `500` `internal.error` com `ErrorResponse`.  
**Nível:** U, I  
**Testes:**
  - `src/http/common/tests.rs::a_panic_in_a_handler_becomes_500_internal_error`

---


### UC-CNV-014

**Caso:** Erro dentro de um resultado parcial (`200` do distribuidor) leva `error` com `code` e `message`, **sem status**  
**Então:** o status do item é o do `code` no catálogo.  
**Nível:** I  
**Testes:**
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-CNV-016

**Caso:** `Accept: application/openmetrics-text` no `/metrics` pede OpenMetrics; o padrão é o texto do Prometheus.  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-CNV-017

**Caso:** Os DTOs reutilizados `Ack`, `StreamEnd` e `ConditionEntry` têm os campos e tipos definidos em 06d §8.3.7 (`severity` de `ConditionEntry` só vale `info`, `warning` ou `critical`).  
**Nível:** U  
**Testes:**
  - `src/cluster/health/tests.rs::a_condition_entry_has_a_name_and_a_severity_of_the_document`
  - `src/cluster/protocol/dto_tests.rs::the_ack_and_the_stream_end_have_the_fields_and_types_of_the_document`
**Observação:** `Severity` hoje só tem `warning` e `critical`: nenhuma condição produz `info`.

---

### UC-CNV-018

**Caso:** Cada porta serve o documento OpenAPI da sua API e uma interface, sem token
**Então:** `GET /openapi/client.json` e `/docs` na `api.port`, `GET /openapi/monitoring.json` e `/docs` na `operations.port`, abertos mesmo com a autenticação ligada; cada porta tem só o seu documento (`404` para o outro); a interface é embutida (nada é carregado da internet); o documento lista só as rotas que o nó tem (sem `/tokens` sem autenticação, sem `/metrics` com o Prometheus desligado) e é o que o `taba openapi` imprime. Toda operação tem `operationId`, uma tag e respostas, toda resposta de erro é um `ErrorResponse` (as sondas, um `HealthResponse`), e as sondas não pedem token onde o resto pede.
**Nível:** I, U
**Testes:**
  - `tests/openapi.rs::each_port_serves_its_own_document_and_an_interface_without_a_token`
  - `tests/openapi.rs::the_document_only_lists_the_routes_the_node_has`
  - `tests/openapi.rs::every_operation_is_identified_described_and_has_its_security_decided`
  - `tests/openapi.rs::the_documentation_of_the_monitoring_port_follows_its_ip_filter`

---

### UC-CNV-019

**Caso:** As respostas que um nó realmente dá são as que os documentos descrevem
**Então:** para cada rota exercitada (tópicos, mensagens com corpos JSON, texto e binário e um distribuidor, tokens, quem é o chamador, os nós, o cluster, as estatísticas, as sondas e as rotas de monitoramento) o status está listado no documento e o corpo JSON bate com o schema documentado para aquele status; a própria verificação recusa um corpo que não bate e um status que não está listado.
**Nível:** I
**Testes:**
  - `tests/openapi.rs::the_answers_of_the_client_api_match_the_document`
  - `tests/openapi.rs::the_answers_of_the_tokens_routes_match_the_document`
  - `tests/openapi.rs::the_answers_of_the_monitoring_port_match_the_document`
  - `tests/openapi.rs::the_check_refuses_a_body_that_does_not_match_its_schema`
  - `tests/openapi.rs::the_check_refuses_a_status_the_document_does_not_list`

---

### UC-CNV-020

**Caso:** Os documentos em `openapi/` são os que o servidor gera
**Então:** `openapi/client.json` e `openapi/monitoring.json` são iguais ao que o `taba openapi` imprime (o `info.version` à parte); senão o teste falha e diz como gerá-los de novo.
**Nível:** I
**Testes:**
  - `tests/openapi.rs::the_published_documents_are_the_ones_the_server_generates`

---

## CNT: Conectores

### UC-CNT-001

**Caso:** O conector oferece as nove operações (`createTopic`, `getTopic`, `listTopics`, `updateTopic`, `deleteTopic`, `publish`, `consume`, `ack`, `changeVisibility`), cada uma montando o verbo, o caminho, a query, os cabeçalhos e o corpo da rota correspondente.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_nine_operations_build_the_expected_routes`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::create_topic_sends_only_what_was_filled`
  - `connectors/rust/client/src/client/tests.rs::topic_operations_build_the_expected_routes`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theNineOperationsBuildTheExpectedRoutes`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::createTopicSendsOnlyWhatWasFilled`
  - `connectors/node/test/client.test.js::the nine operations build the expected routes`
  - `connectors/node/test/client.test.js::create topic sends only what was filled`
  - `connectors/python/tests/test_client.py::test_the_nine_operations_build_the_expected_routes`
  - `connectors/python/tests/test_client.py::test_create_topic_sends_only_what_was_filled`

---

### UC-CNT-002

**Caso:** `baseUrl` é obrigatório. Os padrões são `connectTimeout` 5 s, `requestTimeout` 30 s, `maxAttempts` 3 e `userAgent` `taba-<linguagem>/<versão>`.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::config_defaults_and_required_base_url`
  - `connectors/rust/client/src/client/tests.rs::config_defaults_and_required_base_url`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::configDefaultsAndRequiredBaseUrl`
  - `connectors/node/test/client.test.js::config defaults and required base url`
  - `connectors/python/tests/test_client.py::test_config_defaults_and_required_base_url`

---

### UC-CNT-003

**Caso:** Com `token`, todas as chamadas levam `Authorization: Bearer`; sem ele, nenhuma leva o cabeçalho.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_token_goes_in_every_call_and_without_it_no_call_carries_authorization`
  - `connectors/rust/client/src/client/tests.rs::headers_carry_the_token_the_agent_and_the_optional_ids`
  - `connectors/rust/client/src/client/tests.rs::without_a_token_no_call_carries_authorization`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aTokenGoesInEveryCallAndWithoutItNoCallCarriesAuthorization`
  - `connectors/node/test/client.test.js::a token goes in every call and without it no call carries authorization`
  - `connectors/python/tests/test_client.py::test_a_token_goes_in_every_call_and_without_it_no_call_carries_authorization`

---

### UC-CNT-004

**Caso:** `publish` recebe os bytes e o `Content-Type`; `publishRawJson` e `publishText` só escolhem o `Content-Type`; as opções `groupId`, `deduplicationId`, `contentType` e `requestId` viram query e cabeçalhos corretos. O conector não valida o corpo.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_distributor_answer_lists_each_copy_and_the_partial_failures`
  - `connectors/rust/client/src/client/tests.rs::publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::publishSendsRawBytesWithContentTypeAndQuery`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aDistributorAnswerListsEachCopyAndThePartialFailures`
  - `connectors/node/test/client.test.js::publish sends raw bytes with content type and query`
  - `connectors/node/test/client.test.js::a distributor answer lists each copy and the partial failures`
  - `connectors/python/tests/test_client.py::test_publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/python/tests/test_client.py::test_a_distributor_answer_lists_each_copy_and_the_partial_failures`

---

### UC-CNT-005

**Caso:** `consume` envia só os valores que a pessoa informou (`maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate`, `volatile`, `idleTimeout`, `fifo`, `tags` como `tag.<chave>`) e **não aplica padrões próprios**.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_sends_only_what_was_given`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::auto_create_parameters_are_left_out_without_auto_create`
  - `connectors/rust/client/src/client/tests.rs::consume_sends_only_what_was_given_and_reads_for_wait_time_plus_ten`
  - `connectors/rust/client/src/client/tests.rs::auto_create_parameters_are_left_out_without_auto_create`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::consumeSendsOnlyWhatWasGiven`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::autoCreateParametersAreLeftOutWithoutAutoCreate`
  - `connectors/node/test/client.test.js::consume sends only what was given`
  - `connectors/node/test/client.test.js::auto create parameters are left out without auto create`
  - `connectors/python/tests/test_client.py::test_consume_sends_only_what_was_given`
  - `connectors/python/tests/test_client.py::test_auto_create_parameters_are_left_out_without_auto_create`

---

### UC-CNT-006

**Caso:** `ack` e `changeVisibility` mandam o `receiptToken` na query.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::ack_and_visibility_put_the_receipt_token_in_the_query`
  - `connectors/rust/client/src/client/tests.rs::ack_and_visibility_put_the_receipt_token_in_the_query`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::ackAndVisibilityPutTheReceiptTokenInTheQuery`
  - `connectors/node/test/client.test.js::ack and visibility put the receipt token in the query`
  - `connectors/python/tests/test_client.py::test_ack_and_visibility_put_the_receipt_token_in_the_query`

---

### UC-CNT-007

**Caso:** `listTopics` aceita `limit`, `cursor` e `name`, e oferece um iterador sobre todas as páginas onde a linguagem permite.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::list_all_topics_follows_next_cursor_until_the_last_page`
  - `connectors/rust/client/src/client/tests.rs::list_all_topics_follows_next_cursor_until_the_last_page`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::listAllTopicsFollowsNextCursorUntilTheLastPage`
  - `connectors/node/test/client.test.js::list all topics follows next cursor until the last page`
  - `connectors/python/tests/test_client.py::test_list_all_topics_follows_next_cursor_until_the_last_page`

---

### UC-CNT-008

**Caso:** O tempo limite de leitura do `consume` é `waitTime + 10 s`, nunca o `requestTimeout`  
**Então:** um `waitTime` de 20 s com `requestTimeout` de 30 s não derruba a chamada.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::long_polling_waits_on_an_empty_queue_and_returns_as_soon_as_a_message_arrives`
  - `connectors/rust/client/src/client/tests.rs::consume_sends_only_what_was_given_and_reads_for_wait_time_plus_ten`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theConsumeReadTimeoutIsWaitTimePlusTenSecondsAndNotTheRequestTimeout`
  - `connectors/node/test/client.test.js::the consume read timeout is wait time plus ten seconds and not the request timeout`
  - `connectors/python/tests/test_client.py::test_the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout`

---

### UC-CNT-009

**Caso:** Estouro de tempo no `consume` **não** é repetido  
**Então:** o conector devolve o erro de tempo (`client.timeout`).  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_timeout_is_not_repeated_and_is_a_client_side_error`
  - `connectors/rust/client/src/client/tests.rs::a_timeout_after_sending_is_not_repeated`
  - `connectors/rust/client/src/transport/tests.rs::a_silent_server_times_out_after_the_request_was_sent`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aTimeoutIsNotRepeatedAndIsAClientSideError`
  - `connectors/node/test/client.test.js::a timeout is not repeated and is a client side error`
  - `connectors/python/tests/test_client.py::test_a_timeout_is_not_repeated_and_is_a_client_side_error`

---

### UC-CNT-010

**Caso:** `429` e `503` são repetidos até `maxAttempts` esperando o `Retry-After`; sem ele, esperam 1 s, 2 s, 4 s.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::retries_429_and_503_waiting_for_retry_after_then_backoff`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts`
  - `connectors/rust/client/src/client/tests.rs::retries_429_and_503_waiting_for_retry_after_then_backoff`
  - `connectors/rust/client/src/client/tests.rs::backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::retries429And503WaitingForRetryAfterThenBackoff`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::backoffWithoutRetryAfterIsOneTwoFourSecondsUpToMaxAttempts`
  - `connectors/node/test/client.test.js::retries 429 and 503 waiting for retry after then backoff`
  - `connectors/node/test/client.test.js::backoff without retry after is one two four seconds up to max attempts`
  - `connectors/python/tests/test_client.py::test_retries_429_and_503_waiting_for_retry_after_then_backoff`
  - `connectors/python/tests/test_client.py::test_backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts`

---

### UC-CNT-011

**Caso:** Num `503` de operação que cria ou apaga tópico, o `409` `topic.already-exists` e o `404` da repetição contam como sucesso.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/rust/client/src/client/tests.rs::after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success`
  - `connectors/rust/client/src/client/tests.rs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::afterA503A409OnCreateAndA404OnDeleteCountAsSuccess`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::withoutAPrevious503A409OrA404IsAPlainError`
  - `connectors/node/test/client.test.js::after a 503 a 409 on create and a 404 on delete count as success`
  - `connectors/node/test/client.test.js::without a previous 503 a 409 or a 404 is a plain error`
  - `connectors/python/tests/test_client.py::test_after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success`
  - `connectors/python/tests/test_client.py::test_without_a_previous_503_a_409_or_a_404_is_a_plain_error`

---

### UC-CNT-012

**Caso:** `502` e `504` só são repetidos na publicação em fila FIFO; fora dela o erro sobe sem repetir.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::gateway_errors_are_only_repeated_when_publishing_to_a_fifo`
  - `connectors/rust/client/src/client/tests.rs::gateway_errors_are_only_repeated_when_publishing_to_a_fifo`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::gatewayErrorsAreOnlyRepeatedWhenPublishingToAFifo`
  - `connectors/node/test/client.test.js::gateway errors are only repeated when publishing to a fifo`
  - `connectors/python/tests/test_client.py::test_gateway_errors_are_only_repeated_when_publishing_to_a_fifo`

---

### UC-CNT-013

**Caso:** Falha de rede **antes** de enviar o corpo (recusa de conexão, DNS) é repetida como um `503`; **depois** de enviar o corpo não é repetida.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::network_failure_before_sending_is_repeated_and_after_is_not`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_real_refused_connection_is_a_network_error_after_the_attempts`
  - `connectors/rust/client/src/client/tests.rs::network_failure_before_sending_is_repeated_and_after_is_not`
  - `connectors/rust/client/src/http/tests.rs::tells_apart_a_connection_closed_before_any_byte_and_after_some`
  - `connectors/rust/client/src/transport/tests.rs::a_refused_connection_is_a_failure_before_sending`
  - `connectors/rust/client/src/transport/tests.rs::a_silent_server_times_out_after_the_request_was_sent`
  - `connectors/rust/client/src/transport/tests.rs::a_connection_dropped_mid_response_is_an_io_failure`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::networkFailureAfterSendingIsNotRepeated`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aRefusedConnectionIsRepeatedAndThenANetworkError`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aRefusedConnectionSucceedsWhenTheNodeComesBack`
  - `connectors/node/test/client.test.js::network failure before sending is repeated and after is not`
  - `connectors/node/test/client.test.js::a refused connection is repeated and then succeeds when the node comes back`
  - `connectors/python/tests/test_client.py::test_network_failure_after_sending_is_not_repeated`
  - `connectors/python/tests/test_client.py::test_a_refused_connection_is_repeated_and_then_a_network_error`
  - `connectors/python/tests/test_client.py::test_a_refused_connection_succeeds_when_the_node_comes_back`

---

### UC-CNT-014

**Caso:** Qualquer `4xx` (menos `429`) nunca é repetido.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_4xx_other_than_429_is_never_repeated`
  - `connectors/rust/client/src/client/tests.rs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/rust/client/src/client/tests.rs::a_4xx_other_than_429_is_never_repeated`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::withoutAPrevious503A409OrA404IsAPlainError`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::a4xxOtherThan429IsNeverRepeated`
  - `connectors/node/test/client.test.js::without a previous 503 a 409 or a 404 is a plain error`
  - `connectors/node/test/client.test.js::a 4xx other than 429 is never repeated`
  - `connectors/python/tests/test_client.py::test_without_a_previous_503_a_409_or_a_404_is_a_plain_error`
  - `connectors/python/tests/test_client.py::test_a_4xx_other_than_429_is_never_repeated`

---

### UC-CNT-015

**Caso:** O `body` da mensagem sai como **bytes**  
**Então:** JSON vira o texto JSON tal qual veio; `text/*` em UTF-8 vira a string em UTF-8; `bodyEncoding: base64` vira os bytes decodificados.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::text_json_and_binary_bodies_come_back_as_the_same_bytes`
  - `connectors/rust/client/src/client/tests.rs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::messagesComeBackAsBytesForJsonTextAndBase64`
  - `connectors/node/test/client.test.js::messages come back as bytes for json text and base64`
  - `connectors/python/tests/test_client.py::test_messages_come_back_as_bytes_for_json_text_and_base64`

---

### UC-CNT-016

**Caso:** A `Message` traz `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt`, `invisibleUntil` e, quando vierem, `groupId`, `deduplicationId`, `deadLetteredAt` e `deadLetterReason`; campos desconhecidos na resposta são ignorados.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/rust/client/src/client/tests.rs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::messagesComeBackAsBytesForJsonTextAndBase64`
  - `connectors/node/test/client.test.js::messages come back as bytes for json text and base64`
  - `connectors/python/tests/test_client.py::test_messages_come_back_as_bytes_for_json_text_and_base64`

---

### UC-CNT-017

**Caso:** Uma só forma de erro por conector, com `status`, `code`, `message`, `requestId`, `retryAfter` e `details`; erro de rede e de tempo têm `status` zero e `code` `client.network` ou `client.timeout`.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_timeout_is_not_repeated_and_is_a_client_side_error`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_error_carries_the_error_response_fields_and_the_headers`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_non_json_error_still_reports_a_status_code`
  - `connectors/rust/client/src/client/tests.rs::a_timeout_after_sending_is_not_repeated`
  - `connectors/rust/client/src/client/tests.rs::the_error_carries_the_error_response_fields`
  - `connectors/rust/client/src/client/tests.rs::retry_after_and_request_id_come_from_the_headers_and_a_non_json_error_still_reports`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aTimeoutIsNotRepeatedAndIsAClientSideError`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theErrorCarriesTheErrorResponseFieldsAndTheHeaders`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aNonJsonErrorStillReportsAStatusCode`
  - `connectors/node/test/client.test.js::a timeout is not repeated and is a client side error`
  - `connectors/node/test/client.test.js::the error carries the error response fields and the headers`
  - `connectors/node/test/client.test.js::a non json error still reports a status code`
  - `connectors/python/tests/test_client.py::test_a_timeout_is_not_repeated_and_is_a_client_side_error`
  - `connectors/python/tests/test_client.py::test_the_error_carries_the_error_response_fields_and_the_headers`
  - `connectors/python/tests/test_client.py::test_a_non_json_error_still_reports_a_status_code`

---

### UC-CNT-018

**Caso:** O conector envia `Accept: application/json`, `User-Agent` e, se informados, `X-Request-Id` e `traceparent`, e lê `Retry-After` e `X-Request-Id` da resposta.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::headers_carry_the_agent_and_the_optional_ids`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_error_carries_the_error_response_fields_and_the_headers`
  - `connectors/rust/client/src/client/tests.rs::headers_carry_the_token_the_agent_and_the_optional_ids`
  - `connectors/rust/client/src/client/tests.rs::retry_after_and_request_id_come_from_the_headers_and_a_non_json_error_still_reports`
  - `connectors/rust/client/src/client/tests.rs::the_reply_exposes_the_request_id_header`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::headersCarryTheAgentAndTheOptionalIds`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theErrorCarriesTheErrorResponseFieldsAndTheHeaders`
  - `connectors/node/test/client.test.js::headers carry the agent and the optional ids`
  - `connectors/node/test/client.test.js::the error carries the error response fields and the headers`
  - `connectors/python/tests/test_client.py::test_headers_carry_the_agent_and_the_optional_ids`
  - `connectors/python/tests/test_client.py::test_the_error_carries_the_error_response_fields_and_the_headers`

---

### UC-CNT-019

**Caso:** HTTP/1.1 com conexão persistente quando a biblioteca HTTP oferece sem custo; sem HTTP/2.  
**Nível:** K  
**Testes:**
  - `connectors/rust/client/src/transport/tests.rs::sends_http_1_1_over_a_plain_socket_and_reads_the_reply`

---

### UC-CNT-020

**Caso:** Segurança de thread  
**Então:** o cliente é seguro para uso concorrente em Java, C#, Go, Rust e Node; em C, C++, Python, PHP e Ruby vale um cliente por thread, e o README de cada pasta diz como.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::many_threads_can_use_one_client_at_the_same_time`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::cancelling_the_token_cancels_the_call_without_a_client_error`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::many_parallel_publishers_share_one_client`
  - `connectors/rust/client/src/client/tests.rs::the_client_is_safe_to_share_between_threads`
  - `connectors/rust/client/src/client/tests.rs::many_threads_can_use_one_client_at_the_same_time`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::manyThreadsCanUseOneClientAtTheSameTime`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::interruptingTheThreadCancelsTheCallWithoutAClientError`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::cancellingTheFutureInterruptsTheAsyncCall`
  - `connectors/node/test/client.test.js::many concurrent calls can use one client at the same time`
  - `connectors/node/test/client.test.js::aborting the signal cancels the call without a client error`
  - `connectors/python/tests/test_client.py::test_many_threads_can_use_one_client_at_the_same_time`
  - `connectors/python/tests/test_client.py::test_cancelling_the_token_cancels_the_call_without_a_client_error`
  - `connectors/python/tests/test_client.py::test_cancelling_the_task_cancels_the_call_in_flight`

---

### UC-CNT-021

**Caso:** O leitor de JSON mínimo passa o conjunto comum de documentos de `contrato/fixtures/` (escapes, Unicode e pares substitutos, números grandes, aninhamento profundo, espaços e quebras de linha, valores vazios) e entrega o `body` como trecho de texto sem reserializar.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::skips_every_kind_of_value`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::rejects_malformed_documents`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::deep_nesting_is_bounded_not_a_stack_overflow`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::object_fields_give_the_raw_text_of_each_value`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::decodes_escapes_unicode_and_surrogate_pairs`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::quote_round_trips_through_decode`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::array_items_split_values_with_nested_commas`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::fields_look_up_typed_values_and_ignore_unknown_ones`
  - `connectors/rust/client/src/json/tests.rs::skips_every_kind_of_value`
  - `connectors/rust/client/src/json/tests.rs::rejects_malformed_documents`
  - `connectors/rust/client/src/json/tests.rs::deep_nesting_is_bounded_not_a_stack_overflow`
  - `connectors/rust/client/src/json/tests.rs::object_fields_give_the_raw_text_of_each_value`
  - `connectors/rust/client/src/json/tests.rs::decodes_escapes_unicode_and_surrogate_pairs`
  - `connectors/rust/client/src/json/tests.rs::quote_round_trips_through_decode`
  - `connectors/rust/client/src/json/tests.rs::array_items_split_values_with_nested_commas`

---

### UC-CNT-022

**Caso:** O base64 próprio (C, C++, Rust) codifica e decodifica como o padrão, inclusive com preenchimento (`=`) e bytes arbitrários.  
**Nível:** K  
**Testes:**
  - `connectors/rust/client/src/base64/tests.rs::matches_the_rfc_4648_vectors`
  - `connectors/rust/client/src/base64/tests.rs::round_trips_arbitrary_bytes_and_the_png_header`
  - `connectors/rust/client/src/base64/tests.rs::accepts_missing_padding_and_rejects_garbage`

---

### UC-CNT-023

**Caso:** Sem dependências externas além da biblioteca-padrão da linguagem (no C, só a `libcurl`); no C++ o cabeçalho único embrulha a `libtaba`.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::the_only_package_is_the_reference_assemblies_for_the_classic_target`
  - `connectors/rust/client/src/client/tests.rs::the_only_dependencies_are_rustls_and_its_root_certificates`

---

### UC-CNT-024

**Caso:** O código compila e passa os testes **na versão-base e na mais recente** da linguagem, sem usar recurso posterior à versão-base.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::the_library_targets_classic_and_modern_dotnet_in_csharp_7_3`

---

### UC-CNT-025

**Caso:** O texto vai e volta sempre em UTF-8, de forma explícita, qualquer que seja o *charset* local.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::quote_round_trips_through_decode`
  - `connectors/rust/client/src/json/tests.rs::quote_round_trips_through_decode`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::publishSendsRawBytesWithContentTypeAndQuery`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::messagesComeBackAsBytesForJsonTextAndBase64`
  - `connectors/node/test/client.test.js::publish sends raw bytes with content type and query`
  - `connectors/node/test/client.test.js::messages come back as bytes for json text and base64`
  - `connectors/python/tests/test_client.py::test_publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/python/tests/test_client.py::test_messages_come_back_as_bytes_for_json_text_and_base64`

---

### UC-CNT-026

**Caso:** As fixtures de cada rota conferem a requisição montada e a leitura de uma resposta pronta  
**Então:** `ErrorResponse`, `Page<T>`, corpo JSON, texto e base64, e o `Retry-After`.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::an_invalid_answer_is_an_invalid_response_error_and_not_a_crash`
  - `connectors/rust/client/src/client/tests.rs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/rust/client/src/http/tests.rs::encodes_a_request_with_host_length_and_close`
  - `connectors/rust/client/src/http/tests.rs::a_get_without_body_has_no_content_length_and_post_without_body_has_zero`
  - `connectors/rust/client/src/http/tests.rs::reads_a_response_with_content_length`
  - `connectors/rust/client/src/http/tests.rs::reads_a_chunked_response_with_extensions_and_trailers`
  - `connectors/rust/client/src/http/tests.rs::reads_until_close_when_there_is_no_length_and_skips_100_continue`
  - `connectors/rust/client/src/http/tests.rs::no_content_has_no_body`
  - `connectors/rust/client/src/http/tests.rs::rejects_malformed_responses`
  - `connectors/rust/client/src/http/tests.rs::builds_a_target_with_encoded_query`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::messagesComeBackAsBytesForJsonTextAndBase64`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::anInvalidAnswerIsAnInvalidResponseErrorAndNotACrash`
  - `connectors/node/test/client.test.js::messages come back as bytes for json text and base64`
  - `connectors/node/test/client.test.js::an invalid answer is an invalid response error and not a crash`
  - `connectors/python/tests/test_client.py::test_messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/python/tests/test_client.py::test_an_invalid_answer_is_an_invalid_response_error_and_not_a_crash`

---

### UC-CNT-027

**Caso:** Os cenários de integração passam contra um Taba em modo desenvolvimento  
**Então:** criar tópico, publicar, consumir com long polling, ack, invisibilidade, fila FIFO com duplicata, distribuidor com falha parcial e token sem permissão.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::a_topic_can_be_created_read_listed_updated_and_deleted`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::a_message_is_published_consumed_and_acked_once`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::text_json_and_binary_bodies_come_back_as_the_same_bytes`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::the_server_refuses_a_body_that_is_not_the_declared_json`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::long_polling_waits_on_an_empty_queue_and_returns_as_soon_as_a_message_arrives`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::an_unacked_message_is_invisible_until_released_and_then_comes_back`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::a_fifo_queue_reports_a_duplicate_and_keeps_the_order_inside_a_group`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::a_distributor_copies_to_each_matching_queue_and_a_partial_failure_is_not_an_error`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::the_shared_queue_created_by_the_script_works_with_the_application_token`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::a_token_without_the_permission_or_outside_its_prefixes_is_refused`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::subscribe_handles_and_acks_every_message_until_cancelled`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::many_parallel_publishers_share_one_client`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::auto_create_makes_the_queue_on_consume`

---

### UC-CNT-028

**Caso:** Cada conector declara a versão do contrato da API que implementa, e ignora campos novos que o servidor acrescente.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::fields_look_up_typed_values_and_ignore_unknown_ones`
  - `connectors/rust/client/src/client/tests.rs::unknown_fields_in_responses_are_ignored`
  - `connectors/rust/client/src/json/tests.rs::fields_look_up_typed_values_and_ignore_unknown_ones`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::unknownFieldsInResponsesAreIgnoredAndTheContractVersionIsDeclared`
  - `connectors/node/test/client.test.js::unknown fields in responses are ignored and the contract version is declared`
  - `connectors/python/tests/test_client.py::test_unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared`

---

### UC-CNT-029

**Caso:** `updateTopic` sai como `PATCH` de verdade, sem `X-HTTP-Method-Override`, em cada conector, e o conector não faz conexão HTTP própria para esse verbo.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_nine_operations_build_the_expected_routes`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::update_topic_is_a_real_patch_without_a_method_override_header`
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::patch_uses_the_http_method_and_never_the_override_header`
  - `connectors/rust/client/src/client/tests.rs::topic_operations_build_the_expected_routes`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theNineOperationsBuildTheExpectedRoutes`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::updateTopicIsARealPatchWithoutAMethodOverrideHeader`
  - `connectors/node/test/client.test.js::the nine operations build the expected routes`
  - `connectors/node/test/client.test.js::update topic is a real PATCH without a method override header`
  - `connectors/python/tests/test_client.py::test_the_nine_operations_build_the_expected_routes`
  - `connectors/python/tests/test_client.py::test_update_topic_is_a_real_patch_without_a_method_override_header`

---

### UC-CNT-030

**Caso:** O conector usa a TLS da plataforma, sem pilha de TLS própria (no Rust, o `rustls`), exige TLS 1.2 como mínimo (onde a biblioteca permite configurar) e **nunca** desliga a verificação do certificado.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::tls_is_the_platforms_and_certificate_checks_are_never_turned_off`
  - `connectors/rust/client/src/transport/tests.rs::tls_config_accepts_a_pem_ca_and_rejects_files_without_certificates`

---

### UC-CNT-031

**Caso:** O `subscribe(handler)` (long polling, chamada do handler e ack automático).  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_sync_subscribe_blocks_until_cancelled`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::subscribe_handles_and_acks_every_message_until_cancelled`
  - `connectors/rust/client/src/client/tests.rs::subscribe_acks_only_what_the_handler_accepted_and_stops_on_demand`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::subscribeAcksOnlyWhatTheHandlerAcceptedAndStopsOnClose`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::subscribeEndsWithoutAnErrorWhenClosedDuringTheLongPolling`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aServerErrorWhileSubscribingEndsTheLoopAndSurfacesInTheCompletion`
  - `connectors/node/test/client.test.js::subscribe acks only what the handler accepted and stops on abort`
  - `connectors/node/test/client.test.js::subscribe ends without an error when aborted during the long polling`
  - `connectors/python/tests/test_client.py::test_subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel`
  - `connectors/python/tests/test_client.py::test_subscribe_returns_without_an_error_when_cancelled_during_the_long_polling`
  - `connectors/python/tests/test_client.py::test_the_async_subscribe_acks_only_what_the_handler_accepted_and_stops_when_the_task_is_cancelled`

---

### UC-CNT-032

**Caso:** Nas linguagens com modelo assíncrono (C#, Java, Python 3.5+, Node.js, Go e Rust), as nove operações têm a forma assíncrona, com a mesma montagem de requisição, a mesma repetição de chamadas, o mesmo tempo limite do `consume` e os mesmos erros da síncrona. C, C++, PHP e Ruby só têm a síncrona.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_async_forms_do_the_same_as_the_sync_ones`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::every_operation_has_a_sync_and_an_async_form`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::theAsyncFormsDoTheSameAsTheSyncOnes`
  - `connectors/node/test/client.test.js::every operation is asynchronous and returns a promise`
  - `connectors/python/tests/test_client.py::test_the_async_forms_do_the_same_as_the_sync_ones`
  - `connectors/python/tests/test_client.py::test_errors_and_retries_are_the_same_in_the_async_client`

---

### UC-CNT-033

**Caso:** Nas linguagens com serialização na plataforma (C#, Java, Go, Node.js, Python...), `publishJson(topic, objeto)` serializa o objeto (serializador padrão ou o da configuração) e publica como `application/json`; o JSON pronto vai em `publishRawJson`.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_json_serializes_the_object_and_sends_it_as_json`
  - `connectors/rust/client/src/client/handlers/tests.rs::publish_object_serializes_and_sends_json`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::publishJsonSerializesTheObjectAndSendsItAsJson`
  - `connectors/node/test/client.test.js::publish json serializes the object and sends it as json`
  - `connectors/python/tests/test_client.py::test_publish_json_serializes_the_object_and_sends_it_as_json`

---

### UC-CNT-034

**Caso:** `consume(topic, opções, handler)` (e `consume<T>`, com o corpo já deserializado; corpo que não é um `T` dá `client.invalid-response` sem apagar) entrega cada mensagem, com todas as propriedades, a um contexto com `delete` (padrão `true`  
**Então:** apaga se o `handler` termina sem erro; `false` deixa na fila; erro não apaga e sobe) e `changeVisibility`; enquanto o `handler` roda, uma tarefa renova a invisibilidade a cada um terço dela, e o valor mudado pelo `handler` vale para as renovações seguintes.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_of_t_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_sync_consume_with_a_handler_works_like_the_async_one`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_with_deletes_by_default_keeps_on_request_and_never_after_an_error`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_with_renews_the_visibility_while_the_handler_runs_and_lets_it_change`
  - `connectors/rust/client/src/client/handlers/tests.rs::change_visibility_in_the_handler_becomes_the_renewal_value`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_typed_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::consumeWithAHandlerDeletesByDefaultAndKeepsTheMessageWhenAskedOrWhenItFails`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::consumeWithAHandlerRenewsTheVisibilityWhileItRunsAndLetsItChangeIt`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::aRefusedRenewalMarksTheLeaseAsLostAndTheDeleteIsTolerated`
  - `connectors/java/src/test/java/io/github/rafaelpontani/taba/TabaClientTest.java::consumeOfTHandsOverTheDeserializedBodyAndRefusesABodyThatIsNotAT`
  - `connectors/node/test/client.test.js::consume with a handler deletes by default and keeps the message when asked or when it fails`
  - `connectors/node/test/client.test.js::consume with a handler renews the visibility while it runs and lets it change it`
  - `connectors/node/test/client.test.js::a refused renewal marks the lease as lost and the delete is tolerated`
  - `connectors/node/test/client.test.js::consume json hands over the parsed body and refuses a body that is not json`
  - `connectors/python/tests/test_client.py::test_consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails`
  - `connectors/python/tests/test_client.py::test_consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it`
  - `connectors/python/tests/test_client.py::test_a_refused_renewal_marks_the_lease_as_lost_and_the_delete_is_tolerated`
  - `connectors/python/tests/test_client.py::test_consume_json_hands_over_the_parsed_body_and_refuses_a_body_that_is_not_json`
  - `connectors/python/tests/test_client.py::test_the_async_consume_with_renews_the_visibility_and_deletes_when_the_handler_returns`

---

### UC-CNT-035

**Caso:** Na linguagem com contêiner de injeção de dependência e opções (C#  
**Então:** `AddTabaClient`), o cliente é registrado como *singleton* da interface, com a configuração vinda do mecanismo de opções (código, `appsettings.json` ou `Configure`), o serializador do contêiner vale quando a configuração não traz um, e o endereço inválido falha com `client.invalid-config` ao resolver o cliente.  
**Nível:** K  
**Testes:**
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_registers_one_singleton_with_the_config_from_code`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_binds_a_configuration_section_and_uses_a_registered_serializer`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_with_a_section_path_reads_the_configuration_only_when_the_client_is_resolved`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::an_invalid_base_url_fails_when_the_client_is_resolved_with_client_invalid_config`

---

### UC-CNT-036

**Caso:** O driver do OpenMessaging Benchmark (`connectors/omb`) cria um tópico do OMB como um **distribuidor** do Taba com o nome do tópico e o prefixo `<tópico>-`, e cada assinatura como uma **fila** `<tópico>-<assinatura>` criada antes da primeira publicação; o prefixo de nome de tópico do driver é `omb`.  
**Então:** toda assinatura recebe toda mensagem publicada no tópico, cada cópia com o próprio `id`; o driver apaga as filas e o distribuidor ao fechar.  
**Nível:** K  
**Testes:**
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::theTopicPrefixIsOmb`
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverIntegrationTest.java::everySubscriptionReceivesEveryMessageAndTheTimestampComesBack`

---

### UC-CNT-037

**Caso:** Vários consumidores do OMB são criados para a mesma assinatura do driver.  
**Então:** eles leem a mesma fila e disputam as mensagens dela: cada mensagem vai a um deles e nenhuma se perde.  
**Nível:** K  
**Testes:**
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverIntegrationTest.java::consumersOfTheSameSubscriptionShareTheMessages`

---

### UC-CNT-038

**Caso:** O driver do OMB publica com chave de mensagem (ou sem ela) nos modos `ordering: none` e `ordering: per-key`.  
**Então:** a chave é o `groupId`; com `none` a fila é comum e a mensagem sem chave não tem grupo; com `per-key` a fila é FIFO, cada chave é entregue na ordem de publicação e a mensagem sem chave recebe `p0`..`p<partições-1>` em rodízio. O número de partições do OMB não cria nada no Taba.  
**Nível:** K  
**Testes:**
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::theKeyIsTheGroupWhenThereIsOne`
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::withoutKeyThereIsNoGroupInAnOrdinaryQueue`
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::withoutKeyPerKeyRotatesOverThePartitionsAsGroups`
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverIntegrationTest.java::perKeyKeepsTheOrderOfEachKey`

---

### UC-CNT-039

**Caso:** O OMB pede ao driver a hora de publicação de cada mensagem e o Taba não guarda atributo além do corpo.  
**Então:** o produtor grava o relógio dele (milissegundos, 8 bytes, big-endian) no começo do corpo e o consumidor o retira antes de chamar o callback do OMB, que recebe o payload original e essa hora; o corpo no servidor tem 8 bytes a mais que o payload do OMB.  
**Nível:** K  
**Testes:**
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::theStampComesOutOfTheBodyWithThePayloadIntact`
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverIntegrationTest.java::everySubscriptionReceivesEveryMessageAndTheTimestampComesBack`

---

### UC-CNT-040

**Caso:** O YAML do driver do OMB tem um `ordering` diferente de `none` e `per-key`, um `consumer.waitTime` fora de 0 a 20 ou um `producer.inFlight` abaixo de 1.  
**Então:** o driver se recusa a iniciar com um erro que cita a chave.  
**Nível:** K  
**Testes:**
  - `connectors/omb/src/test/java/io/github/rafaelpontani/taba/omb/DriverUnitTest.java::theConfigurationIsValidated`

---


## CLI: O `taba-cli`, a linha de comando do cliente

### UC-CLI-001

**Caso:** A conexão vem do perfil; os comandos de API não têm opção de conexão (`--url`, `--token` etc.), só `--profile`, `-o`, `-q` e `--request-id`. A precedência de cada campo é variável `TABA_CLI_*`, perfil e valor embutido. Não existe `ops-url`: a CLI só usa a API do cliente.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/args/tests.rs::reads_values_flags_and_positionals_in_any_order`
  - `connectors/rust/cli/src/args/tests.rs::accepts_equals_attached_short_values_and_a_dash_value`
  - `connectors/rust/cli/src/args/tests.rs::repeated_options_keep_every_value_and_the_last_wins_for_single_reads`
  - `connectors/rust/cli/src/args/tests.rs::double_dash_ends_options_and_a_lone_dash_is_positional`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`
  - `connectors/rust/cli/tests/commands.rs::the_default_profile_gives_the_url_and_the_token_from_its_file`

---

### UC-CLI-002

**Caso:** O token vai em `Authorization: Bearer`, vindo do campo `token-file` (primeira linha) ou `token` do perfil, ou de `TABA_CLI_TOKEN`; sem token, a chamada vai sem o cabeçalho. `config show` nunca imprime o token.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/settings/tests.rs::the_token_comes_from_the_highest_level_and_the_file_wins_a_tie`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`
  - `connectors/rust/cli/tests/commands.rs::the_default_profile_gives_the_url_and_the_token_from_its_file`
  - `connectors/rust/cli/tests/commands.rs::the_token_variable_works_without_any_profile`

---

### UC-CLI-003

**Caso:** Não existe opção para desligar a verificação do certificado; o campo `ca-file` do perfil acrescenta uma CA a confiar.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::there_is_no_option_to_turn_off_certificate_checks_and_a_missing_ca_file_exits_5`

---

### UC-CLI-004

**Caso:** A saída `json` é a resposta do servidor sem campos acrescentados; `table` e `raw` só mudam a forma. Os `204` não imprimem nada, e `--quiet` não imprime nada em sucesso.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/output/tests.rs::json_is_the_servers_answer_indented_and_raw_is_left_alone`
  - `connectors/rust/cli/src/output/tests.rs::table_lists_the_items_of_a_page_with_a_column_per_field`
  - `connectors/rust/cli/src/output/tests.rs::table_of_one_object_is_field_and_value_and_an_empty_list_says_so`
  - `connectors/rust/cli/src/output/tests.rs::long_cells_are_cut`
  - `connectors/rust/cli/tests/commands.rs::json_is_the_server_answer_indented_table_and_raw_change_only_the_form`
  - `connectors/rust/cli/tests/commands.rs::the_output_format_comes_from_the_profile_and_the_flag_overrides_it`
  - `connectors/rust/cli/tests/commands.rs::a_204_prints_nothing_and_quiet_silences_success`
  - `connectors/rust/cli/tests/commands.rs::a_closed_output_pipe_ends_quietly_like_piping_into_head`
  - `connectors/rust/client/src/json/tests.rs::pretty_reindents_without_touching_scalars`

---

### UC-CLI-005

**Caso:** O erro da API vai para a saída de erro com `código`, mensagem e `requestId` (e o `Retry-After` do `429` e do `503`), e o código de saída é `1`. Uso errado sai com `2`, falha de conexão com `3`, tempo limite com `4` e arquivo ilegível com `5`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/args/tests.rs::rejects_unknown_options_missing_values_and_values_on_flags`
  - `connectors/rust/cli/src/args/tests.rs::numbers_are_checked_without_calling_the_server`
  - `connectors/rust/cli/src/output/tests.rs::api_errors_print_code_message_request_id_details_and_retry_after`
  - `connectors/rust/cli/src/output/tests.rs::with_json_the_error_response_is_printed_as_it_came`
  - `connectors/rust/cli/tests/commands.rs::there_is_no_option_to_turn_off_certificate_checks_and_a_missing_ca_file_exits_5`
  - `connectors/rust/cli/tests/commands.rs::an_unreadable_token_file_exits_5`
  - `connectors/rust/cli/tests/commands.rs::an_api_error_goes_to_stderr_with_code_message_and_request_id_and_exits_1`
  - `connectors/rust/cli/tests/commands.rs::with_json_the_error_response_is_printed_as_it_came_and_retry_after_gets_its_own_line`
  - `connectors/rust/cli/tests/commands.rs::exit_codes_cover_usage_connection_and_timeout`
  - `connectors/rust/cli/tests/commands.rs::the_request_id_flag_goes_in_the_header`

---

### UC-CLI-006

**Caso:** `topic create` envia só as opções dadas, com `--tag`, `--prefix` repetíveis, e o servidor recusa o que não vale para o tipo.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::topic_create_sends_only_the_options_given`
  - `connectors/rust/cli/tests/commands.rs::topic_create_needs_a_valid_type_and_well_formed_numbers_and_tags`
  - `connectors/rust/cli/tests/commands.rs::the_server_refusal_for_a_field_that_does_not_fit_the_type_is_shown`

---

### UC-CLI-007

**Caso:** `topic list` repassa `name`, `sort`, `limit` e `cursor`; com `--all` percorre todas as páginas pelo `nextCursor` e imprime todos os itens.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::topic_list_passes_name_sort_limit_and_cursor`
  - `connectors/rust/cli/tests/commands.rs::topic_list_all_follows_every_page_and_prints_all_the_items`

---

### UC-CLI-008

**Caso:** `topic get` e `topic update`  
**Então:** o `update` envia só o que foi dado, `default` vira `null` e `--clear-tags` envia o mapa vazio; os campos imutáveis não existem como opção.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::topic_get_and_update_send_only_what_was_given`
  - `connectors/rust/cli/tests/commands.rs::immutable_fields_are_not_options_and_an_empty_update_is_a_usage_error`

---

### UC-CLI-009

**Caso:** `topic delete` e `token revoke` pedem confirmação num terminal e, fora dele, só seguem com `--yes` (sem ele saem com `2`).  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::delete_without_yes_refuses_outside_a_terminal`
  - `connectors/rust/cli/tests/commands.rs::in_a_terminal_delete_asks_and_only_yes_goes_on`
  - `connectors/rust/cli/tests/commands.rs::yes_skips_the_question`

---

### UC-CLI-010

**Caso:** `publish` aceita o corpo pelo argumento, por `--file` (bytes crus) ou por `--stdin`, com `--content-type` (padrão `application/json`), `--json`, `--text`, `--group-id` e `--deduplication-id`. Não valida o corpo, e `--id-only` imprime só o `id`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::publish_takes_the_body_from_the_argument_the_file_or_stdin`
  - `connectors/rust/cli/tests/commands.rs::publish_wants_exactly_one_body_source_and_one_content_type_choice`
  - `connectors/rust/cli/tests/commands.rs::publish_id_only_and_count`
  - `connectors/rust/cli/tests/commands.rs::publish_does_not_validate_the_body_the_server_does`
  - `connectors/rust/cli/tests/commands.rs::a_distributor_answer_is_printed_whole_even_with_id_only`

---

### UC-CLI-011

**Caso:** `publish` não repete sozinho a chamada depois de `502` ou `504` fora da FIFO; na FIFO a repetição é segura.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::after_a_502_outside_a_fifo_publish_is_not_repeated`
  - `connectors/rust/cli/tests/commands.rs::after_a_502_on_a_fifo_publish_is_repeated_because_the_primary_deduplicates`

---

### UC-CLI-012

**Caso:** `consume` manda só os parâmetros informados (`maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate` e seus parâmetros) e imprime a mensagem inteira, com o `receiptToken`. `--exit-empty` sai com `6` quando a lista vem vazia, e `--ack` confirma cada mensagem entregue.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::consume_sends_only_the_given_parameters_and_prints_the_whole_message_with_the_receipt`
  - `connectors/rust/cli/tests/commands.rs::the_auto_create_options_need_auto_create`
  - `connectors/rust/cli/tests/commands.rs::consume_raw_prints_only_the_body_one_per_line_and_ack_confirms_each_message`
  - `connectors/rust/cli/tests/commands.rs::exit_empty_returns_6_only_when_nothing_came`
  - `connectors/rust/cli/tests/commands.rs::consume_binary_bodies_come_out_decoded_in_raw_mode`

---

### UC-CLI-013

**Caso:** `subscribe` consome em laço com long polling; com `--exec`, confirma só se o comando sai com `0`, e sem ele imprime e confirma. `--limit` termina após `n` mensagens, e `Ctrl+C` termina sem confirmar a mensagem em andamento.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::subscribe_prints_each_message_confirms_it_and_stops_at_the_limit`
  - `connectors/rust/cli/tests/commands.rs::subscribe_exec_runs_the_command_with_the_body_and_confirms_only_on_exit_zero`
  - `connectors/rust/cli/tests/commands.rs::subscribe_keeps_going_when_the_receipt_went_stale_during_processing`

---

### UC-CLI-014

**Caso:** `ack` e `visibility` exigem `--receipt` (e `--seconds` no segundo); um `receiptToken` obsoleto devolve `message.stale-receipt` (`409`) e o código de saída `1`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::ack_and_visibility_need_the_receipt_and_visibility_needs_seconds`
  - `connectors/rust/cli/tests/commands.rs::a_stale_receipt_is_a_409_error_with_exit_1`

---

### UC-CLI-015

**Caso:** `token create` envia nome, permissões, prefixos e `expiresAt`, e mostra o segredo uma única vez (`--token-only` imprime só ele); `token list` e `token get` nunca mostram o segredo; `token update` troca `name` e `expiresAt` (`none` remove o prazo). Sem autenticação, o `404 route.not-found` é mostrado como erro.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::token_create_sends_name_permissions_prefixes_and_expiry`
  - `connectors/rust/cli/tests/commands.rs::token_create_validates_the_permissions_without_the_server`
  - `connectors/rust/cli/tests/commands.rs::token_list_get_and_update_use_the_token_routes`
  - `connectors/rust/cli/tests/commands.rs::without_authentication_the_tokens_routes_answer_404_and_the_cli_says_so`

---

### UC-CLI-016

**Caso:** `cluster status` e `node list` usam a API do cliente com o token do perfil; `cluster status` aceita `--scope`, e os comandos `health` não existem porque a CLI nunca chama a API de monitoramento.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/output/tests.rs::the_cluster_status_table_has_one_row_per_node_and_per_condition`
  - `connectors/rust/cli/tests/commands.rs::cluster_status_goes_through_the_client_api_with_the_token`
  - `connectors/rust/cli/tests/commands.rs::cluster_status_validates_the_scope`
  - `connectors/rust/cli/tests/commands.rs::the_cli_has_no_health_commands_because_it_never_calls_the_monitoring_api`

---

### UC-CLI-017

**Caso:** `metrics` (com `--openmetrics` e `--node`), `stats topics`, `stats topic` e `stats groups` vão pela API do cliente (`/metrics`, `/nodes/{id}/metrics`, `/stats/topics*`) e repassam `scope`, `tag`, `condition`, `fields`, `groupBy` e a paginação.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::metrics_prints_the_text_as_it_came_and_openmetrics_changes_the_accept_header`
  - `connectors/rust/cli/tests/commands.rs::stats_pass_scope_tags_condition_fields_group_by_and_pagination`

---

### UC-CLI-018

**Caso:** `drain start`, `drain status` e `drain cancel` agem sobre o nó indicado pela opção obrigatória `--node`, por `/nodes/{id}/drain` da API do cliente, e mostram o `DrainStatus`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::drain_acts_on_the_node_named_by_the_option_through_the_client_api`
  - `connectors/rust/cli/tests/commands.rs::drain_without_a_node_is_a_usage_error_that_calls_nothing`

---

### UC-CLI-019

**Caso:** `version` mostra a versão do programa e a do contrato; `help <comando>` mostra a ajuda de cada comando.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::version_shows_the_program_and_the_contract_version`
  - `connectors/rust/cli/tests/commands.rs::help_lists_the_commands_and_shows_each_one`

---

### UC-CLI-020

**Caso:** A imagem Docker (Alpine) roda o `taba-cli` como usuário sem privilégios, com o `ENTRYPOINT` no programa, e funciona contra o Compose.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/packaging.rs::the_dockerfile_builds_a_static_binary_and_runs_it_as_an_unprivileged_user_on_alpine`
  - `connectors/rust/cli/tests/packaging.rs::the_docker_script_offers_the_two_linux_platforms`
  - `connectors/rust/cli/tests/packaging.rs::the_compose_file_has_the_init_container_three_nodes_haproxy_and_the_cli_tool`
  - `connectors/rust/cli/tests/packaging.rs::the_haproxy_example_balances_the_client_api_and_passes_the_cluster_port_through_in_tcp`
  - `connectors/rust/cli/tests/packaging.rs::the_node_image_runs_the_taba_binary_without_privileges`

---

### UC-CLI-021

**Caso:** Os scripts `build.sh` e `build.ps1` geram o binário da plataforma, e com `--target` os alvos da tabela de 19b §18.11.8, com `SHA256SUMS`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/packaging.rs::the_unix_script_covers_the_four_unix_targets_with_checksums_and_a_locked_build`
  - `connectors/rust/cli/tests/packaging.rs::the_windows_script_covers_the_two_windows_targets_with_checksums_and_a_locked_build`
  - `connectors/rust/cli/tests/packaging.rs::the_scripts_do_not_try_to_cross_to_macos_from_another_system`

---

### UC-CLI-022

**Caso:** Sem arquivo de configuração e sem variável nem perfil, o `taba-cli` usa `http://127.0.0.1:8080` e nenhum token.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/settings/tests.rs::without_file_variables_or_profile_the_built_in_defaults_apply`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`

---

### UC-CLI-023

**Caso:** O arquivo de perfis fica na pasta de configuração do usuário (`%APPDATA%\taba\config.yml`, `~/.config/taba/config.yml` ou `~/Library/Application Support/taba/config.yml`); `TABA_CLI_CONFIG` troca o caminho, e em Unix o arquivo nasce com permissão `0600`. `profile path` o imprime.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/profile/tests.rs::the_file_lives_in_the_user_config_folder_of_each_system`
  - `connectors/rust/cli/src/profile/tests.rs::taba_cli_config_overrides_the_path`
  - `connectors/rust/cli/tests/commands.rs::profile_path_prints_the_config_file_location`
  - `connectors/rust/cli/tests/commands.rs::the_default_location_follows_the_system_when_the_override_is_absent`

---

### UC-CLI-024

**Caso:** `profile add` cria um perfil (o primeiro vira o padrão; `--default` torna outro o padrão) e recusa, com `2`, um nome que já existe ou inválido; `profile set` altera e `--unset` apaga campos; `profile remove` pede confirmação ou `--yes`, e remover o padrão deixa o arquivo sem padrão.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/profile/tests.rs::the_first_profile_becomes_the_default_and_names_are_validated`
  - `connectors/rust/cli/src/profile/tests.rs::fields_are_validated_when_set`
  - `connectors/rust/cli/src/profile/tests.rs::a_file_with_the_old_ops_url_field_still_loads_and_drops_it_when_written_again`
  - `connectors/rust/cli/src/profile/tests.rs::removing_the_default_leaves_the_file_without_one`
  - `connectors/rust/cli/tests/commands.rs::profile_add_creates_the_file_the_first_profile_is_the_default_and_a_duplicate_exits_2`
  - `connectors/rust/cli/tests/commands.rs::profile_set_changes_fields_and_unset_clears_them`
  - `connectors/rust/cli/tests/commands.rs::profile_remove_asks_outside_yes_and_removing_the_default_leaves_none`

---

### UC-CLI-025

**Caso:** `profile default` mostra, define ou (com `--none`) remove o padrão; `profile list` marca o padrão e o perfil em uso; `profile show` nunca imprime o token.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/profile/tests.rs::removing_the_default_leaves_the_file_without_one`
  - `connectors/rust/cli/src/profile/tests.rs::set_default_needs_an_existing_profile`
  - `connectors/rust/cli/tests/commands.rs::profile_default_shows_sets_and_clears_and_list_marks_default_and_session`
  - `connectors/rust/cli/tests/commands.rs::profile_show_never_prints_the_token`

---

### UC-CLI-026

**Caso:** O perfil usado é o de `--profile`, senão o de `TABA_CLI_PROFILE`, senão o `default` do arquivo; uma variável `TABA_CLI_*` sobrepõe só o campo que cobre, e perfil inexistente sai com `2`. `config show` diz a origem de cada valor (variável, perfil, padrão).  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/settings/tests.rs::the_profile_is_the_flag_then_the_variable_then_the_file_default`
  - `connectors/rust/cli/src/settings/tests.rs::an_unknown_profile_is_an_error_that_lists_the_known_ones`
  - `connectors/rust/cli/src/settings/tests.rs::a_variable_overrides_only_the_field_it_covers`
  - `connectors/rust/cli/src/settings/tests.rs::an_invalid_variable_value_is_reported_with_its_name`
  - `connectors/rust/cli/tests/commands.rs::the_default_profile_gives_the_url_and_the_token_from_its_file`
  - `connectors/rust/cli/tests/commands.rs::the_profile_flag_beats_the_variable_and_the_variable_overrides_only_its_field`
  - `connectors/rust/cli/tests/commands.rs::an_unknown_profile_is_a_usage_error`
  - `connectors/rust/cli/tests/commands.rs::config_show_tells_where_each_value_came_from`

---

### UC-CLI-027

**Caso:** `profile session` não grava nada em disco  
**Então:** imprime as linhas para o terminal aplicar (`TABA_CLI_PROFILE` com um nome; as variáveis `TABA_CLI_*` com campos, que formam o perfil temporário; o desfazer com `--clear`), na sintaxe de `--shell sh`, `powershell` ou `cmd`, com os valores escapados.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/commands/profile/tests.rs::session_lines_for_a_saved_profile_and_for_loose_fields`
  - `connectors/rust/cli/src/commands/profile/tests.rs::clearing_unsets_the_profile_and_every_field_variable`
  - `connectors/rust/cli/src/commands/profile/tests.rs::values_are_escaped_for_the_chosen_shell`
  - `connectors/rust/cli/src/commands/profile/tests.rs::shell_names_are_checked`
  - `connectors/rust/cli/tests/commands.rs::profile_session_prints_lines_and_writes_nothing`
  - `connectors/rust/cli/tests/commands.rs::profile_session_defaults_to_powershell_on_windows_and_checks_its_arguments`

---

### UC-CLI-028

**Caso:** O arquivo de perfis é lido e reescrito por inteiro de forma atômica (arquivo temporário e renomeação); YAML inválido ou arquivo ilegível sai com `5` e a linha do erro, e uma gravação interrompida não deixa o arquivo pela metade.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/profile/tests.rs::the_file_round_trips_in_order`
  - `connectors/rust/cli/src/profile/tests.rs::invalid_content_is_refused_with_a_reason`
  - `connectors/rust/cli/src/profile/tests.rs::a_missing_file_is_empty_and_saving_replaces_it_atomically`
  - `connectors/rust/cli/src/profile/tests.rs::the_saved_file_is_private_on_unix`
  - `connectors/rust/cli/src/yaml/tests.rs::reads_the_documented_example`
  - `connectors/rust/cli/src/yaml/tests.rs::handles_comments_blank_lines_bom_and_empty_documents`
  - `connectors/rust/cli/src/yaml/tests.rs::reads_quoted_scalars_and_keeps_hash_inside_quotes`
  - `connectors/rust/cli/src/yaml/tests.rs::reports_the_line_of_invalid_yaml`
  - `connectors/rust/cli/src/yaml/tests.rs::rendering_then_parsing_gives_the_same_values_for_awkward_text`
  - `connectors/rust/cli/src/yaml/tests.rs::renders_nested_maps_and_empty_maps`
  - `connectors/rust/cli/tests/commands.rs::an_invalid_config_file_exits_5_with_the_line`
  - `connectors/rust/cli/tests/commands.rs::a_hand_written_config_with_comments_and_quotes_is_read`

---

### UC-CLI-029

**Caso:** `taba-cli profile` sem subcomando, num terminal, abre a tela interativa  
**Então:** a lista de perfis navegada com `↑`/`↓`, com o padrão marcado (`●`) e o perfil em uso na sessão; `Enter` abre o formulário, `n` cria, `d` define o padrão, `q`/`Esc` saem.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/form/tests.rs::a_short_terminal_scrolls_to_keep_the_focused_line_visible`
  - `connectors/rust/cli/src/tui/screen/tests.rs::the_list_shows_profiles_with_the_default_and_the_session_profile_and_never_the_token`
  - `connectors/rust/cli/src/tui/screen/tests.rs::up_and_down_move_the_selection_and_stay_inside_the_list`
  - `connectors/rust/cli/src/tui/screen/tests.rs::an_empty_list_says_so_and_offers_n`
  - `connectors/rust/cli/src/tui/screen/tests.rs::q_escape_and_ctrl_c_leave_the_screen`
  - `connectors/rust/cli/src/tui/screen/tests.rs::d_makes_the_selected_profile_the_default_and_saves`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_screen_creates_a_profile_that_the_commands_then_use`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_screen_removes_after_confirming_and_marks_the_default`

---

### UC-CLI-030

**Caso:** O formulário de perfil é navegado com `↑`/`↓`/`Tab`; campos de texto se editam direto, `output` é escolha (`←`/`→`) e `Perfil padrão` é caixa (`Espaço`); campo vazio mostra o valor que valerá e apagar o texto equivale a `--unset`; `Ctrl+S` ou o botão Salvar grava o mesmo que `profile add`/`set`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/form/tests.rs::arrows_and_tab_move_between_fields_and_buttons_wrapping_around`
  - `connectors/rust/cli/src/tui/form/tests.rs::text_fields_are_edited_in_place_with_the_cursor_keys`
  - `connectors/rust/cli/src/tui/form/tests.rs::editing_handles_multibyte_characters`
  - `connectors/rust/cli/src/tui/form/tests.rs::output_is_a_choice_and_the_default_is_a_box`
  - `connectors/rust/cli/src/tui/form/tests.rs::empty_fields_show_the_value_that_will_apply`
  - `connectors/rust/cli/src/tui/form/tests.rs::an_empty_field_is_left_out_of_the_profile_like_unset`
  - `connectors/rust/cli/src/tui/form/tests.rs::ctrl_s_and_the_buttons_ask_to_save_test_and_cancel`
  - `connectors/rust/cli/src/tui/form/tests.rs::the_cursor_sits_after_the_typed_text_of_the_focused_field`
  - `connectors/rust/cli/src/tui/keys/tests.rs::decodes_arrows_in_csi_and_application_mode`
  - `connectors/rust/cli/src/tui/keys/tests.rs::decodes_editing_keys`
  - `connectors/rust/cli/src/tui/keys/tests.rs::a_lone_escape_is_the_escape_key_and_control_letters_are_named`
  - `connectors/rust/cli/src/tui/keys/tests.rs::decodes_typed_text_including_multibyte_and_pasted_runs`
  - `connectors/rust/cli/src/tui/keys/tests.rs::unknown_sequences_are_skipped_whole`
  - `connectors/rust/cli/src/tui/screen/tests.rs::a_new_profile_is_created_through_the_form_and_the_first_one_becomes_the_default`
  - `connectors/rust/cli/src/tui/screen/tests.rs::a_second_profile_does_not_take_the_default_unless_the_box_is_ticked`
  - `connectors/rust/cli/src/tui/screen/tests.rs::editing_an_existing_profile_changes_it_in_place_and_a_rename_keeps_the_default`
  - `connectors/rust/cli/src/tui/screen/tests.rs::unticking_the_box_of_the_default_profile_removes_the_default`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_screen_creates_a_profile_that_the_commands_then_use`

---

### UC-CLI-031

**Caso:** O formulário valida na hora (URL, números, nome repetido ou com caracteres inválidos)  
**Então:** o erro aparece em vermelho sob o campo e Salvar não grava enquanto houver erro.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/form/tests.rs::invalid_values_get_a_message_under_the_field`
  - `connectors/rust/cli/src/tui/form/tests.rs::the_name_is_required_and_checked_but_an_edited_profile_may_keep_its_own_name`
  - `connectors/rust/cli/src/tui/screen/tests.rs::save_is_refused_while_a_field_is_invalid`

---

### UC-CLI-032

**Caso:** O botão Testar conexão faz `GET /topics?limit=1` com os valores ainda não salvos do formulário e mostra o resultado numa linha; só informa e não impede de salvar.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/screen/tests.rs::test_connection_uses_the_unsaved_values_and_only_reports`
  - `connectors/rust/cli/src/tui/screen/tests.rs::testing_is_refused_while_the_form_has_errors`
  - `connectors/rust/cli/src/tui/tests.rs::the_connection_config_uses_builtin_values_where_the_profile_is_silent`
  - `connectors/rust/cli/src/tui/tests.rs::an_unreadable_token_file_or_ca_file_is_reported`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_test_button_calls_the_client_api_with_the_unsaved_values`

---

### UC-CLI-033

**Caso:** Remover (`Del` ou `r`) pede a confirmação `[s/N]` antes de apagar; `Esc` num formulário alterado pede confirmação; `Ctrl+C` em qualquer tela sai e restaura o terminal; o que foi confirmado já está gravado e o formulário não salvo é descartado.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/form/tests.rs::escape_cancels_at_once_when_nothing_changed_and_asks_when_it_did`
  - `connectors/rust/cli/src/tui/form/tests.rs::ctrl_c_quits_from_anywhere`
  - `connectors/rust/cli/src/tui/keys/tests.rs::a_lone_escape_is_the_escape_key_and_control_letters_are_named`
  - `connectors/rust/cli/src/tui/screen/tests.rs::removing_asks_first_and_only_s_removes`
  - `connectors/rust/cli/src/tui/screen/tests.rs::removing_the_default_leaves_the_file_without_one`
  - `connectors/rust/cli/src/tui/screen/tests.rs::an_unsaved_form_is_discarded_on_a_second_escape`
  - `connectors/rust/cli/src/tui/screen/tests.rs::ctrl_c_quits_from_the_form_too`
  - `connectors/rust/cli/src/tui/screen/tests.rs::a_failed_write_keeps_the_previous_state_and_shows_the_error`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_screen_removes_after_confirming_and_marks_the_default`

---

### UC-CLI-034

**Caso:** `token` e `token-file` são mostrados e editados à vista no formulário (o token no `config.yml` não é criptografado); `profile show` e `config show` continuam sem mostrar o token.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/form/tests.rs::the_token_is_shown_in_plain_text`

---

### UC-CLI-035

**Caso:** A tela usa tela alternada, ANSI e entrada em modo cru (`stty` no Linux e macOS, `SetConsoleMode` no Windows) e restaura o terminal ao sair; `NO_COLOR` tira as cores; um terminal menor que 60x16 mostra um aviso, e a tela se redesenha ao mudar de tamanho.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/src/tui/screen/tests.rs::a_terminal_smaller_than_60_by_16_shows_a_warning_instead_of_a_broken_screen`
  - `connectors/rust/cli/src/tui/screen/tests.rs::colors_are_only_emitted_when_asked_for`

---

### UC-CLI-036

**Caso:** Fora de um terminal (entrada ou saída redirecionada), `taba-cli profile` sem subcomando não abre a tela  
**Então:** imprime a ajuda dos comandos de perfil e sai com `2`.  
**Nível:** K  
**Testes:**
  - `connectors/rust/cli/tests/commands.rs::outside_a_terminal_the_profile_command_prints_help_and_exits_2`

---


## ESC: Escrita: registro + commit, quórum, rollback, teto de commit

### UC-ESC-001

**Caso:** O coordenador gera o GUID v7 **antes** da fase de registro e calcula, pelo ID, o bloco e os nós responsáveis (primário, secundário, terciário).  
**Nível:** U, C  
**Testes:**
  - `src/core/ring/tests.rs::the_id_alone_gives_the_block_and_the_primary_secondary_and_tertiary`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-ESC-002

**Caso:** A fase 1 (registro) vai a **todos** os `replication.factor` nós responsáveis, não só ao quórum; cada um grava a mensagem como **pendente** em memória e confirma.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-003

**Caso:** Atingido o quórum de confirmações do registro (primário incluso), o coordenador decide o commit, chama o commit nos nós do quórum e **continua** o registro + commit nas demais réplicas em segundo plano, sem bloquear a resposta.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-004

**Caso:** A resposta ao cliente com o ID sai assim que o **commit está confirmado no quórum** (primário incluso); as réplicas fora do quórum ainda podem estar pendentes nesse instante.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-005

**Caso:** A durabilidade plena (commit confirmado em todas as `replication.factor` réplicas) é alcançada em seguida, em segundo plano.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-006

**Caso:** O coordenador pode ser um dos nós de destino (inclusive o primário), sem chamada de rede para si mesmo.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-007

**Caso:** O nó que recebe a requisição é o coordenador, qualquer que seja o nó.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-008

**Caso:** O padrão de `write.quorum` é `floor(rf / 2) + 1`  
**Então:** 2 com rf 3, 3 com rf 5, 1 com rf 1.  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::write_quorum_defaults_to_floor_of_half_the_factor_plus_one`

---

### UC-ESC-009

**Caso:** `write.quorum` é validado entre 1 e `replication.factor`; fora disso, a configuração é recusada. Abaixo da maioria é aceito.  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::write_quorum_must_be_between_one_and_the_factor_and_below_the_majority_is_accepted`

---

### UC-ESC-010

**Caso:** O quórum **sempre inclui o primário**  
**Então:** com primário sem responder e secundário e terciário confirmando, o commit **não** é decidido, ainda que o número de confirmações seja o do quórum.  
**Nível:** U, C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-011

**Caso:** O quórum é primário mais `write.quorum − 1` **quaisquer** outras réplicas.  
**Nível:** U, C  
**Testes:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-012

**Caso:** `write.quorum` é um valor **único** para escrita, entrega (invisibilidade) e ack.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/service/tests.rs::write_quorum_is_a_single_value_for_write_delivery_and_ack`
  - `src/config/tests.rs::there_is_only_one_write_quorum_parameter`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-ESC-013

**Caso:** Sem nós suficientes para o quórum, a operação falha **antes** de decidir (rollback do registro) e a API responde `503` `cluster.quorum-unavailable` com `Retry-After`.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-014

**Caso:** Com rf 3 e quórum 2, a escrita continua funcionando com **um** nó responsável fora do ar. Com dois fora (o primário e um), a escrita falha com `503`.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-015

**Caso:** Falha de registro num nó antes do quórum  
**Então:** o coordenador repete a chamada a esse nó conforme `write.retry.attempts` e `write.retry.backoff`.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_lost_call_is_retried_before_the_quorum_gives_up`

---

### UC-ESC-016

**Caso:** Esgotados os retries **sem quórum**  
**Então:** erro ao cliente e **rollback** a todos os nós que já haviam confirmado, para descartar o registro pendente. Nenhum nó efetivou nada.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-017

**Caso:** Um registro sem commit nem rollback é descartado sozinho depois de `write.pending.ttl`; o rollback só acelera a liberação.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`

---

### UC-ESC-019

**Caso:** A mensagem descartada por rollback nunca aparece para um consumidor, e o ID gerado não é reaproveitado.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-020

**Caso:** Falha de registro ou commit numa réplica **fora do quórum**, depois da decisão  
**Então:** **nunca há rollback**; o coordenador insiste, dentro de `write.commit.timeout`.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-021

**Caso:** A réplica confirma dentro do teto  
**Então:** passa a contar como durável, sem novo aviso ao cliente (a resposta já foi dada).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-022

**Caso:** O teto esgota sem confirmação  
**Então:** o bloco fica abaixo do fator pleno (nunca abaixo do quórum), o evento `under-replicated` (não crítico) é gerado, `taba.write.commit_timeouts` sobe, o coordenador **continua tentando** em segundo plano, e a saúde do nó não muda.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-023

**Caso:** O commit é idempotente  
**Então:** repeti-lo numa réplica que já o aplicou não tem efeito.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::applying_a_commit_twice_changes_nothing`

---

### UC-ESC-024

**Caso:** Quando a réplica atrasada volta, o commit em segundo plano a alcança e o bloco volta ao fator pleno (a condição `under-replicated` some).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-025

**Caso:** `POST /cluster/topics/{topic}/messages/{id}/register`, `commit` e `rollback` (fases 1, 2 e abortar) respondem `200` `Ack` com o `operationId` e `accepted: true`.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-ESC-026

**Caso:** Todas as fases são **idempotentes pelo `operationId`**  
**Então:** repetir `register`, `commit` ou `rollback` devolve o mesmo `Ack`, sem efeito novo.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/replication/replica/tests.rs::repeating_register_or_commit_returns_the_same_ack_without_a_new_effect`
  - `src/cluster/replication/replica/tests.rs::repeating_rollback_returns_the_same_ack_and_the_message_never_appears`
  - `src/cluster/replication/replica/tests.rs::the_memory_of_decided_operations_forgets_the_oldest_first_and_stays_bounded`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-ESC-029

**Caso:** O `register` com `kind: "dead-letter"` cria a mensagem na dead-letter (com `deliveryCount: 0`, `invisibleUntil` nulo, `deadLetteredAt`, `deadLetterReason`) e remove a de `sourceTopic` no commit, numa operação só.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node`

---

### UC-ESC-030

**Caso:** Nó, tópico e token exigem a confirmação de **todos** os nós, sem quórum reduzido; mensagem decide por quórum.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-ESC-037

**Caso:** Ao confirmar (ack) uma mensagem, **todo nó que guarda uma cópia** (o primário e as réplicas, em memória ou em disco) grava o registro de exclusão (`id`, posição no anel e instante) no **log de excluídos** do tópico, na mesma transação que apaga a mensagem; na fila FIFO o registro acompanha a linha curta da deduplicação  
**Então:** o id fica registrado como morto em todos os nós que tinham a cópia; uma mensagem que não foi confirmada não é registrada. A mensagem que estava só em memória é registrada pelo `id` e pela posição.  
**Nível:** U e C  
**Testes:**
  - `tests/excluded_log.rs::an_ack_records_the_exclusion_on_every_node_that_keeps_a_copy`
  - `src/storage/messages/excluded/tests.rs::removing_with_exclusion_deletes_the_message_and_records_it_in_the_same_transaction`
  - `src/storage/messages/excluded/tests.rs::a_message_that_was_only_in_memory_is_recorded_by_its_id_and_position`
  - `src/storage/messages/excluded/tests.rs::the_fifo_ack_records_the_exclusion_together_with_the_short_row`
  - `src/storage/messages/excluded/tests.rs::rows_are_listed_by_the_ranges_of_a_block_in_id_order`

---

### UC-ESC-038

**Caso:** Ao enviar uma mensagem à dead-letter (`maxDeliveries` esgotado), todo nó que guarda uma cópia grava a exclusão no log da **fila de origem**  
**Então:** o id é morto na fila de origem e continua vivo na dead-letter (o log é por tópico).  
**Nível:** U e C  
**Testes:**
  - `tests/excluded_log.rs::the_dead_letter_records_the_exclusion_in_the_source_queue_on_every_node`
  - `src/storage/messages/excluded/tests.rs::moving_to_the_dead_letter_records_the_exclusion_in_the_source_only`

---


## ANE: Anel de hash, blocos e papéis

### UC-ANE-001

**Caso:** O ID da mensagem vira um hash, que cai numa posição do anel, que pertence a um bloco com os nós responsáveis em ordem (primário, secundário, terciário).  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::a_message_id_becomes_a_position_that_belongs_to_a_block_with_ordered_owners`

---

### UC-ANE-002

**Caso:** Qualquer nó chega ao mesmo primário para o mesmo ID, a partir só do ID e da tabela de membros.  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::any_node_reaches_the_same_owners_from_the_ids_alone`

---

### UC-ANE-003

**Caso:** Os nós responsáveis por um bloco são sempre **nós físicos distintos**  
**Então:** ao percorrer o anel, posições de nós virtuais de um nó físico já escolhido são puladas.  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::owners_are_distinct_physical_nodes`

---

### UC-ANE-004

**Caso:** Cada nó físico ocupa `ring.virtual-nodes` posições; mais nós virtuais distribuem melhor os blocos (a variação da quantidade de blocos por nó diminui).  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::more_virtual_nodes_spread_the_blocks_more_evenly`

---

### UC-ANE-005

**Caso:** O hash do GUID v7 distribui as mensagens de forma uniforme (o timestamp nos bits altos não concentra), tanto de mensagem quanto do `id` do tópico (nó gestor).  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::the_hash_spreads_time_ordered_ids_over_the_whole_ring`

---

### UC-ANE-008

**Caso:** O primário do bloco é o dono da entrega das mensagens dele.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ANE-009

**Caso:** Numa fila FIFO a posição é o hash de `id do tópico + ":" + groupId`, não o do ID da mensagem (UC-FIF-023).  
**Nível:** U  
**Testes:**
  - `src/core/ring/tests.rs::a_fifo_group_has_one_position_for_all_its_messages`

---

### UC-ANE-010

**Caso:** Quando a tabela de membros muda (entrada, saída, falha de registro), o anel é recalculado e os papéis de cada bloco mudam de acordo; sem o nó que falhou, o anel volta a ser o anterior.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/members/tests.rs::the_ring_follows_the_table`
  - `src/core/ring/tests.rs::the_ring_is_recalculated_from_the_members_and_goes_back_without_the_failed_node`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---


## DEF: Definição de tópicos e de tokens replicada no cluster

### UC-DEF-010

**Caso:** O arquivo guarda **só a definição**  
**Então:** dados de consumo (profundidade, taxas, histogramas) nunca vão para ele.  
**Nível:** I  
**Testes:**
  - `tests/stored_definition.rs::the_file_keeps_only_the_definition_and_never_consumption_data`

---

### UC-DEF-011

**Caso:** A dead-letter não tem linha de definição na tabela `topics`  
**Então:** é derivada da fila de origem, criada e excluída na mesma operação.  
**Nível:** I  
**Testes:**
  - `src/engine/topics/tests.rs::the_dead_letter_is_derived_from_its_queue_and_never_stored`

---

### UC-DEF-012

**Caso:** Cada token é uma linha da tabela `tokens` do `_node.db` (com `id`, `name`, permissões, prefixos, datas, o sal e o hash salgado do segredo, nunca o segredo).  
**Nível:** I  
**Testes:**
  - `tests/stored_definition.rs::each_token_is_a_row_with_the_salted_hash_and_never_the_secret`

---

### UC-DEF-025

**Caso:** Cada definição tem um **dono** no anel  
**Então:** o primeiro nó ativo na posição do hash de `topic:<nome>` (ou `token:<id>`). O nó que recebe a chamada do cliente **propõe** a mudança ao dono (`definition-propose`), que decide; a API só responde quando a mudança já vale também no nó que recebeu a chamada.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner`
  - `tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers`

---

### UC-DEF-026

**Caso:** O dono dá a cada mudança a próxima **versão** (1, 2, 3...). Cada nó só aplica uma mudança de versão **maior ou igual** à que tem  
**Então:** repetir é inofensivo e uma mudança atrasada (versão menor) é ignorada. Todos os nós terminam na última versão.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back`
  - `tests/definitions.rs::each_change_gets_the_next_version_and_every_node_ends_on_the_last`

---

### UC-DEF-027

**Caso:** A remoção deixa uma **lápide** com a versão  
**Então:** uma criação ou alteração atrasada, de versão menor, não ressuscita o tópico (nem o token revogado).  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back`

---

### UC-DEF-028

**Caso:** Duas criações do mesmo nome ao mesmo tempo, em nós diferentes, chegam ao mesmo dono, que as serializa  
**Então:** uma responde `201` e a outra `409` `topic.already-exists`.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict`

---

### UC-DEF-029

**Caso:** O dono confere a mudança com o próprio estado antes de decidir  
**Então:** `create` de nome existente é `409` `topic.already-exists`; `update` ou `delete` de tópico desconhecido é `404` `topic.not-found` (de token, `token.not-found`); `delete` com `expectedId` que não confere é `409` `cluster.conflict`. Nada é aplicado em nenhum nó.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict`
  - `tests/definitions.rs::the_owner_refuses_a_change_that_does_not_fit_the_current_state`
  - `tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers`

---

### UC-DEF-030

**Caso:** Antes de responder, o dono grava a mudança num **herdeiro**  
**Então:** o primeiro outro dono da posição que responde. Se há outros donos e nenhum responde, a mudança é recusada com `503` `cluster.quorum-unavailable` e não é aplicada em nó algum.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_change_with_no_other_owner_reachable_fails_with_503`

---

### UC-DEF-031

**Caso:** Um nó que não responde não impede a mudança  
**Então:** o dono responde depois dos nós que respondem dentro de `write.commit.timeout` e o dono e o herdeiro enfileiram o estado da definição para todos os pares no quadro `definition` do canal (D127); o nó que não respondeu a recebe quando o canal volta, ou é excluído se a fila estoura. O nó que volta passa a ter a definição (ou a revogação).  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_topic_created_while_a_node_is_unreachable_reaches_it_when_it_returns`
  - `tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`

---

### UC-DEF-032

**Caso:** Dono fora do ar e ainda na tabela (até a exclusão dele)  
**Então:** a mudança falha com `503` `cluster.quorum-unavailable` e `Retry-After`, sem aplicar nada, e o cliente repete a chamada. Se o dono está em falha, o código é `node.failed`. A leitura segue local.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_change_whose_owner_is_unreachable_fails_with_503`

---

### UC-DEF-033

**Caso:** `POST /cluster/definitions/propose` (`DefinitionProposal`  
**Então:** tipo, operação, chave, corpo e `expectedId`) e `POST /cluster/definitions/apply` (`DefinitionApply`: tipo, chave, versão, `deleted` e corpo) respondem com o `DefinitionApply` decidido. Os streams `GET /cluster/topics` e `/cluster/tokens` levam a `version` de cada linha. O corpo do token leva só o `salt` e o `secretHash`, nunca o segredo.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_routes_are_the_ones_of_the_document`
  - `tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner`

---

### UC-DEF-034

**Caso:** Tokens seguem a mesma regra, com o `id` do token como chave  
**Então:** criação, alteração e revogação passam pelo dono. A revogação vale na hora nos nós que respondem e chega ao nó que estava fora do ar quando ele volta; `token.not-found` na revogação de um token desconhecido.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-DEF-035

**Caso:** Duas cópias de uma definição com a **mesma versão e conteúdo diferente** (a nova tentativa que reaproveita a versão, ou dois nós que se achavam donos) terminam iguais em todo nó, qualquer que seja a ordem de chegada  
**Então:** vale a de **menor impressão** (SHA-256 do corpo normalizado; a remoção tem uma só). O empate é avisado no registro e contado em `taba.cluster.definitions.conflicts`; repetir a cópia igual não é empate. Vale no `apply`, na carga do nó novo e na reconciliação.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::two_copies_with_the_same_version_end_the_same_in_any_arrival_order`

---

### UC-DEF-036

**Caso:** O nó que sincroniza lê as **lápides** da origem (`GET /cluster/definitions/tombstones`  
**Então:** tipo, chave e a versão da remoção, que a origem ainda guarda) logo depois dos streams de tópicos e de tokens. Assim ele sabe a versão já usada de uma definição removida: uma criação atrasada de versão menor não a ressuscita, e a recriação do mesmo nome por ele, se virar dono, sai com a versão seguinte e não com a 1. Uma lápide menor que a versão viva que o nó tem é ignorada; maior, remove a definição.  
**Nível:** C, U  
**Testes:**
  - `tests/definitions.rs::a_new_node_learns_the_tombstones_and_a_late_creation_does_not_bring_the_topic_back`

---

### UC-DEF-037

**Caso:** O **herdeiro** também leva a definição aos pares  
**Então:** o dono grava nele com `relay` e o herdeiro enfileira o quadro `definition` para todos. Se o dono não alcança um nó (ou cai), o nó recebe a definição, e a remoção (lápide), pelo herdeiro.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::the_heir_carries_the_definition_to_a_node_the_owner_cannot_reach`

---

### UC-DEF-038

**Caso:** O nó que ficou fora do ar durante várias mudanças da mesma definição recebe só o **estado mais recente** dela quando o canal volta  
**Então:** o quadro novo substitui o antigo na fila do par. Não há lista de pendentes no dono.  
**Nível:** C  
**Testes:**
  - `tests/definitions.rs::a_node_that_was_down_gets_the_latest_state_of_each_definition_from_the_queue`

---

### UC-DEF-039

**Caso:** Cada nó grava na tabela `definition_versions` do `_node.db` a **versão** de cada tópico e de cada token que aplicou e a **lápide** de cada remoção, no mesmo ponto em que grava a definição  
**Então:** as linhas sobrevivem ao reinício do processo e são lidas na partida **junto com as definições** que o nó retoma do disco: uma linha fica só se a definição dela está lá (um tópico com o mesmo `topicId`), e a lápide fica até expirar (1 hora). A remoção da definição, o `wipe` do nó novo e a conferência final do join apagam as linhas.  
**Nível:** U e C  
**Testes:**
  - `storage/versions/tests.rs::versions_and_tombstones_with_the_topic_id_survive_reopening_the_database`
  - `storage/versions/tests.rs::saving_again_replaces_the_row_and_delete_tolerates_a_missing_one`
  - `storage/versions/tests.rs::old_tombstones_are_dropped_and_clear_empties_the_table`
  - `cluster/replication/definitions/tests.rs::the_stored_versions_of_the_definitions_that_were_loaded_come_back_with_their_topic_id`
  - `cluster/replication/definitions/tests.rs::a_tombstone_is_kept_until_it_expires`
  - `cluster/replication/definitions/tests.rs::a_live_row_without_its_definition_is_dropped`
  - `tests/definition_versions.rs::every_node_writes_the_version_and_the_tombstone_with_the_topic_id_to_its_database`
  - `tests/definition_versions.rs::a_new_node_stores_the_tombstones_it_learns_with_the_topic_id`

---

### UC-DEF-040

**Caso:** A lápide e a versão de um tópico guardam o **`topicId`** do tópico (um tópico recriado com o mesmo nome tem outro `topicId`)  
**Então:** a lápide do tópico antigo não apaga nem descreve o tópico recriado: na partida, uma linha com outro `topicId` é descartada e o tópico fica. A lápide chega ao nó novo com o `topicId` no fluxo `GET /cluster/definitions/tombstones` (campo `topicId`, opcional: quem não o traz continua entendido).  
**Nível:** U e C  
**Testes:**
  - `cluster/replication/definitions/tests.rs::a_row_of_another_life_of_the_topic_does_not_describe_the_recreated_one`
  - `tests/definition_versions.rs::every_node_writes_the_version_and_the_tombstone_with_the_topic_id_to_its_database`
  - `tests/definition_versions.rs::a_new_node_stores_the_tombstones_it_learns_with_the_topic_id`

---

### UC-DEF-041

**Caso:** Um nó **assume o disco** na partida a frio
**Então:** ele lê tópicos, tokens, versões e lápides do `_node.db`, atende com eles sem esperar o cluster, e os nós que entram os recebem dele. O log de excluídos do nó continua o da vida anterior (a data de início volta à dela), senão nenhuma vida estaria dentro da janela.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-DEF-042

**Caso:** Um nó que **entra num cluster que já está de pé**
**Então:** entra limpo: as definições do disco dele são apagadas antes da sincronização e não sobrepõem as do cluster (um tópico excluído enquanto ele estava fora não volta).
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_node_that_joins_a_running_cluster_does_not_impose_the_definitions_of_its_disk`

---

### UC-DEF-009

**Caso:** A definição do tópico é sempre persistida na tabela `topics` do `${storage.dir}/_node.db` (uma linha por tópico, sem arquivo por tópico), gravada no commit em cada nó, e atualizada ou removida com a definição.  
**Nível:** I, C  
**Testes:**
  - `src/storage/topics/tests.rs::saving_again_replaces_the_row`
  - `src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---

### UC-DEF-015

**Caso:** O nó novo puxa a tabela de tópicos por `GET /cluster/topics` (stream NDJSON ordenado por `id`) de um nó de **menor carga** pelo sinal de vida; empatados ou sem dado de carga, um nó **sorteado**.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins`

---

### UC-DEF-016

**Caso:** A tabela de tópicos **não** vai na resposta do ingresso (só a tabela de nós).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins`

---

### UC-DEF-022

**Caso:** Os tokens são puxados por `GET /cluster/tokens` junto com a tabela de tópicos, da mesma origem e com as mesmas regras (carga direta, eventos no banco, verificação final).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-DEF-024

**Caso:** A definição de cada tópico é gravada na tabela `topics` do `_node.db` numa transação (nunca uma linha pela metade), e nomes que só diferem em maiúsculas (`Pedidos` e `pedidos`) são tópicos diferentes, em qualquer disco.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::topics_that_differ_only_in_case_keep_separate_folders`
  - `src/storage/messages/tests.rs::topic_folders_are_safe_for_case_insensitive_disks`
  - `src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---


## ENT: Entrega no dono: lista de prontos, `take`, long polling

### UC-ENT-001

**Caso:** Cada mensagem tem um único dono da entrega  
**Então:** o primário do bloco. Só ele a coloca na sua lista de prontos; réplica não entra na lista.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::only_the_owner_puts_a_message_in_the_delivery_lists`
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ENT-002

**Caso:** Com três nós A, B e C, cada um é primário de parte das mensagens  
**Então:** a lista de prontos de cada um tem só as suas, e a união das listas é o tópico completo.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-ENT-004

**Caso:** Quando o primário de um bloco muda (entrada, saída, rebalanceamento), a thread refaz o filtro dos blocos afetados. Um bloco em sincronização não entrega.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---

### UC-ENT-008

**Caso:** A lista de prontos é mantida por tópico em cada nó, em memória, com `delivery.ready-list.size` IDs (padrão 1000).  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_ready_list_holds_at_most_the_configured_size_and_the_counters_stay_exact`
  - `src/engine/messages/tests/expiry.rs::a_small_ready_list_still_delivers_every_message_exactly_once`
  - `src/engine/messages/tests/bounded.rs::the_index_keeps_no_more_entries_than_the_window_and_the_list_hold`
  - `src/engine/messages/tests/bounded.rs::reopening_a_topic_rebuilds_the_counts_and_the_window_from_the_disk`
  - `src/engine/messages/tests/cluster.rs::the_owner_reserves_and_aborts_with_small_lists_as_with_large_ones`
  - `src/engine/messages/tests/bounded.rs::the_oldest_message_comes_from_the_age_order_of_memory_and_disk`

---

### UC-ENT-009

**Caso:** Uma thread repõe a lista quando o semáforo abre espaço (entrega commitada, ID que sai), repondo a quantidade que abriu, sem consultar à toa.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::only_the_room_that_opened_is_refilled_and_a_full_list_costs_nothing`
  - `src/engine/messages/tests/bounded.rs::ack_and_visibility_work_on_a_message_the_index_only_counts`

---

### UC-ENT-010

**Caso:** A reposição segue a sanfona  
**Então:** dois cursores sobre as mensagens do nó (da mais antiga para a frente e da mais nova para trás, por GUID v7, em memória e em disco), alternando as duas pontas.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_refill_walks_two_cursors_alternating_the_oldest_and_the_newest_end`

---

### UC-ENT-011

**Caso:** A entrega consome as duas pontas de forma **intercalada**  
**Então:** a mais antiga, a mais nova, a segunda mais antiga, a segunda mais nova.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::delivery_interleaves_the_two_ends_oldest_newest_second_oldest_second_newest`

---

### UC-ENT-012

**Caso:** Uma mensagem nova que chega ao dono entra direto na lista se há lugar e o cursor da ponta nova está no fim; senão é encontrada depois pela thread.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::a_new_message_enters_the_list_if_there_is_room_and_nothing_waits_else_the_cursor_finds_it`

---

### UC-ENT-013

**Caso:** Ao receber o commit de uma entrega, o nó remove o ID da lista. O primário está sempre no quórum, então a mensagem deixa de ser candidata a nova entrega no próprio nó que a entregou no mesmo instante em que o cliente a recebe.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts`
  - `src/engine/messages/tests/cluster.rs::the_owner_reserves_and_aborts_with_small_lists_as_with_large_ones`

---

### UC-ENT-014

**Caso:** A mensagem cuja invisibilidade vence volta à lista pela posição do GUID v7, **não** pela varredura; se a lista está cheia, o ID do **meio** sai para dar lugar e é reposto depois (nenhuma mensagem se perde).  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::a_returning_message_enters_by_its_position_and_the_middle_id_leaves_when_full`

---

### UC-ENT-015

**Caso:** O `GET` só consulta a lista, sem varrer memória e disco a cada chamada.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::choosing_the_next_message_only_reads_the_list_and_scans_nothing`
  - `src/engine/messages/tests/expiry.rs::a_small_ready_list_still_delivers_every_message_exactly_once`
  - `src/engine/messages/tests/bounded.rs::small_lists_choose_the_same_messages_as_large_ones`
  - `src/storage/messages/tests.rs::the_end_of_a_group_is_read_by_an_index_in_id_order`

---

### UC-ENT-017

**Caso:** Cenário 2  
**Então:** lista local vazia e outro nó tem: o nó consulta a tabela em memória, escolhe um nó que tem mensagens prontas, chama `POST /cluster/topics/{topic}/messages/take`; o dono faz registro + commit e devolve a mensagem, que o nó repassa ao cliente.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-ENT-019

**Caso:** O peer é sorteado com **peso** proporcional às mensagens prontas informadas no sinal de vida; sem informação de quantidade, o sorteio é uniforme.  
**Nível:** U  
**Testes:**
  - `src/cluster/replication/signals/tests.rs::the_peer_is_drawn_with_a_weight_proportional_to_the_ready_ones_it_said`
  - `src/cluster/replication/signals/tests.rs::without_the_quantity_of_some_peer_the_draw_is_uniform`
  - `src/cluster/replication/signals/tests.rs::a_peer_that_said_zero_still_has_a_chance_and_every_peer_comes_once`

---

### UC-ENT-022

**Caso:** `take` num nó em drain responde lista vazia; num nó em sincronização, `503` `cluster.node-syncing`.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-ENT-023

**Caso:** Cenário 3  
**Então:** nenhum nó tem mensagem: o `GET` com `waitTime > 0` vira ouvinte local, e o nó informa aos demais, uma única vez, "o tópico tem ouvintes aqui".  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-024

**Caso:** Mensagem nova pronta no **próprio nó** vai direto ao primeiro ouvinte local, sem tráfego entre nós.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-025

**Caso:** Mensagem nova pronta em **outro nó** com ouvinte conhecido  
**Então:** o dono envia o aviso, o nó do ouvinte faz o `take` só com aquele nó e entrega ao ouvinte. Se a mensagem já foi levada por outro, o ouvinte volta a esperar.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-031

**Caso:** Métricas da entrega  
**Então:** `taba.delivery.ready`, `taba.delivery.redeliveries`, `taba.delivery.longpoll.waiting`, `taba.delivery.longpoll.outcome` (`delivered`, `empty`, `disconnected`) e `taba.delivery.take` (`in` e `out`; `delivered`, `empty`, `error`).  
**Nível:** C  
**Testes:**
  - `tests/metrics.rs::long_polling_is_counted_while_waiting_and_by_outcome`

---


## CAN: Canal permanente entre nós e filas de sinais

### UC-CAN-001

**Caso:** Há um canal WebSocket por par de nós (malha completa), compartilhado por todos os tópicos  
**Então:** N nós têm N−1 canais cada (`taba.cluster.channel.connections`).  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it`

---

### UC-CAN-002

**Caso:** O canal carrega só sinais pequenos, nunca o conteúdo das mensagens nem listas longas.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content`

---

### UC-CAN-003

**Caso:** `GET /cluster/channel` faz o upgrade para WebSocket (`101`) só para quem consta na tabela de membros; quem não consta recebe `403` `cluster.not-member` **antes** do upgrade.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered`

---

### UC-CAN-004

**Caso:** Os quadros são JSON com o campo `type`  
**Então:** `keepalive`, `listeners`, `ready`, `notify`, `node-state` e `confirm`, cada um com os campos definidos.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content`

---

### UC-CAN-005

**Caso:** O `keepalive` é uma mensagem própria (não ping/pong do WebSocket), enviada a cada `cluster.channel.keepalive.interval`, com a data e hora do nó, a carga, o resumo de monitoramento e as mensagens prontas por tópico (só tópicos com prontas, até `cluster.channel.keepalive.max-topics`).  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::the_keepalive_carries_the_time_the_state_the_version_and_the_busiest_topics_up_to_the_limit`

---

### UC-CAN-006

**Caso:** O `keepalive` **não tem `seq`**, não é confirmado nem reenviado  
**Então:** o seguinte o substitui.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/tests.rs::channel_messages_are_json_text_and_the_confirm_is_told_by_its_type`
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-007

**Caso:** Os sinais de estado são enviados **só nas mudanças** (ouvintes 0→1 e →0; prontas vazio→não vazio e o contrário), não a cada chamada.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_ready_signal_fires_only_when_the_state_changes`

---

### UC-CAN-008

**Caso:** Ao abrir o canal, cada lado envia primeiro um instantâneo (um `listeners` e um `ready` para cada tópico em que o estado é verdadeiro) e só depois as mudanças. Um nó em sincronização só o consome quando fica pronto.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_channel_opens_with_a_snapshot_of_the_state_the_listeners_and_the_ready_topics`

---

### UC-CAN-009

**Caso:** Todo quadro, exceto `keepalive` e `confirm`, leva `seq` crescente por canal e por sentido. O receptor devolve o `confirm` (possivelmente agrupando vários `seqs`) **depois de processar**.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-010

**Caso:** O emissor só tira o sinal da fila de saída ao receber o `confirm`; se o canal cai, reenvia na reconexão o que ficou sem confirmação, e o que foi confirmado nunca se repete.  
**Nível:** C  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`
  - `tests/channel.rs::a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns`

---

### UC-CAN-011

**Caso:** Um `seq` já processado e reenviado (confirmação perdida) é ignorado pelo receptor (basta guardar o maior `seq` processado).  
**Nível:** U, C  
**Testes:**
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-012

**Caso:** O nó que entra ou volta se conecta ativamente a cada nó da tabela que recebeu como resposta; os demais só aceitam. Um nó só aceita conexão de quem consta na sua tabela.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered`

---

### UC-CAN-013

**Caso:** Queda com os dois nós vivos  
**Então:** o nó que detectar primeiro reconecta. Se os dois abrem uma conexão ao mesmo tempo, o nó de **menor hash do ID** fecha a que abriu e fica com a aberta pelo outro: sempre um canal por par.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/channel/tests.rs::of_two_connections_the_one_opened_by_the_node_with_the_larger_hash_stays`
  - `src/cluster/channel/tests.rs::the_same_side_opening_again_replaces_the_dead_connection`
  - `src/cluster/channel/tests.rs::both_nodes_keep_the_same_connection_in_any_order_of_arrival`
  - `tests/channel.rs::two_connections_at_once_end_with_a_single_channel_per_pair`

---

### UC-CAN-014

**Caso:** Reinício do processo  
**Então:** o nó perde a memória (inclusive a tabela de membros, que não é persistida) e volta como nó novo, pelo registro; é ele quem abre os canais.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CAN-015

**Caso:** Em cada nó dono há um responsável por tópico (ativo só enquanto há ouvintes em algum nó e mensagens novas a avisar). As mensagens que ficam prontas são acumuladas por `cluster.notify.window` (padrão 100 ms) e viram **um** aviso, com a quantidade.  
**Nível:** U, C  
**Testes:**
  - `tests/notify.rs::the_ready_messages_of_a_window_become_one_notice_with_the_quantity`

---

### UC-CAN-016

**Caso:** O aviso vai a **um** nó com ouvinte por vez, revezando entre os que têm; o responsável espera `cluster.notify.wait` que o nó venha buscar (`take`). Se não vem, passa ao próximo nó com ouvinte e repete o ciclo, até acabarem as mensagens avisadas ou os nós com ouvinte.  
**Nível:** C  
**Testes:**
  - `tests/notify.rs::the_notice_goes_to_one_listener_at_a_time_and_passes_on_when_nobody_comes`

---

### UC-CAN-017

**Caso:** O aviso é uma dica  
**Então:** quem o recebe vai buscar e pode não encontrar nada (`taba.cluster.channel.notifications` com `outcome` `fetched` ou `empty`).  
**Nível:** C  
**Testes:**
  - `tests/notify.rs::a_notice_is_a_hint_that_ends_fetched_or_empty`

---

### UC-CAN-018

**Caso:** Há uma fila de saída por nó de destino (uma thread de escrita a esvazia sem bloquear quem produz) e uma fila de entrada por nó de origem (processamento separado da leitura do socket).  
**Nível:** U, C  
**Testes:**
  - `tests/channel.rs::each_peer_has_an_out_queue_and_an_in_queue_that_drain`

---

### UC-CAN-019

**Caso:** Com o canal fora, sinais de **estado** do mesmo tópico se substituem na fila (só o último importa); avisos antigos são descartados (`taba.cluster.channel.queue.dropped` com `coalesced` e `expired`).  
**Nível:** U  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::state_signals_of_the_same_topic_replace_each_other_and_notices_do_not`
  - `src/cluster/channel/queue/tests.rs::old_notices_are_dropped_and_old_states_are_kept`

---

### UC-CAN-020

**Caso:** A fila de saída estoura `cluster.channel.queue.max-size` ou `cluster.channel.queue.max-age`  
**Então:** o nó com quem o canal está fora é tratado como **falho** e a votação de exclusão é aberta, sem esperar o fim das reconexões (`dropped` com `overflow`).  
**Nível:** C  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::the_queue_overflows_by_size_or_by_age`
  - `tests/channel.rs::a_queue_that_waits_too_long_opens_the_vote_without_waiting_for_the_retries`

---

### UC-CAN-021

**Caso:** Reconexão **antes** de a votação concluir  
**Então:** nada muda, os nós retomam o envio dos sinais pendentes de onde pararam.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_channel_that_returns_before_the_retries_end_changes_nothing`

---

### UC-CAN-022

**Caso:** Votação concluída  
**Então:** todos os nós descartam as filas (de saída e de entrada) para o nó excluído e param de acumular. Se o nó voltar, refaz o bootstrap com filas novas e vazias.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::once_the_exclusion_is_decided_the_nodes_drop_the_channel_and_the_return_starts_with_empty_queues`

---

### UC-CAN-023

**Caso:** O sinal `node-state` com `state: draining`  
**Então:** os pares deixam de pedir `take` a esse nó e de escolhê-lo como origem de carga.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-CAN-024

**Caso:** Métricas do canal  
**Então:** `taba.cluster.channel.state` por peer, `rtt`, `reconnects`, `queue.out` e `queue.in` (a fila de entrada por nó de origem).  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it`

---

### UC-CAN-025

**Caso:** A chamada do canal leva o `nodeId` do destino (`to`)  
**Então:** o processo novo que atende o mesmo IP e porta (outro ID, por exemplo um reinício rápido) responde `421` `cluster.wrong-node` antes de qualquer outra conferência, e o emissor conta o canal como caído. A votação abre, o nó antigo sai da tabela e o novo entra (D100).  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CAN-026

**Caso:** A chamada do canal **sem** `to` (vazio) não confere o destino  
**Então:** quem chama sem saber o ID do nó que procura (a rota de teste, uma versão que não o envia) é tratado como antes, sem `421` (D100).  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_channel_call_without_the_destination_id_is_not_checked_against_it`

---

### UC-CAN-027

**Caso:** O quadro `membership` do canal **não é substituído nem expira**  
**Então:** ao contrário do `ready` (que o último substitui) e do `notify` (que expira), uma mudança de membros fica na fila de saída até o `confirm` do par. A fila que cresce demais já faz do par um nó falho.  
**Nível:** U  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::a_membership_change_is_never_replaced_and_never_expires`

---

### UC-CAN-028

**Caso:** O quadro `definition` do canal é **substituído** pelo mais novo da mesma definição (mesmo tipo e mesma chave) na fila de saída; outro tipo ou outro nome não o substitui, e ele não expira.  
**Nível:** U  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::a_definition_frame_is_replaced_by_the_newer_of_the_same_definition_and_never_expires`

---

### UC-CAN-029

**Caso:** Um canal aberto por um nó que os peers **ainda não conhecem** é recusado **sem bloquear o nó**  
**Então:** o `accept` procura o peer, pede ao `sync_members` que ajuste os canais à tabela e procura de novo; quem continua desconhecido é descartado. Ele nunca trava os peers uma segunda vez enquanto os segura (esse deadlock parava o nó para sempre: doc 36, "Um accept de canal que travou os peers duas vezes").  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_channel_from_a_node_the_peers_do_not_know_yet_is_refused_without_blocking_the_node`

---


## INV: Invisibilidade no dono e nas réplicas

### UC-INV-001

**Caso:** Os metadados são `deliveryCount`, `invisibleUntil` (data absoluta) e `receiptToken`, guardados e replicados juntos  
**Então:** no mesmo registro + commit, em memória ou no banco local do tópico.  
**Nível:** U, C  
**Testes:**
  - `src/storage/messages/tests.rs::the_delivery_metadata_lives_in_the_message_row_and_survives_a_reopening`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-INV-003

**Caso:** O `receiptToken` é gerado pelo dono a cada entrega; o nó compara com o dos metadados no ack e no `PUT`, e responde `409` se for diferente, sem alterar nada. A alteração mantém o mesmo token.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message`

---

### UC-INV-004

**Caso:** Mensagem em memória  
**Então:** metadados em memória. Mensagem em disco: linha na tabela `messages` do banco do tópico (`id`, posição, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`), atualizada no commit e removida com a mensagem no ack. Alterar a invisibilidade de uma mensagem em disco atualiza só a linha, sem tocar no corpo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/index/tests.rs::an_inserted_message_with_a_future_deadline_starts_in_processing`

---

### UC-INV-005

**Caso:** Os estados da mensagem para o dono  
**Então:** **pronta** (`invisibleUntil` ausente ou no passado), **invisível** (no futuro) e **removida** (ack).  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::a_message_is_ready_invisible_or_removed_by_its_deadline`

---

### UC-INV-006

**Caso:** Entrega  
**Então:** propaga novo `invisibleUntil` e novo `receiptToken`, `deliveryCount` +1, remove o ID da lista de prontos e insere ID e prazo na lista de invisíveis do dono.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts`
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-INV-007

**Caso:** Alteração  
**Então:** propaga o novo `invisibleUntil`, `deliveryCount` não muda, lista de prontos não muda (com `0`, o dono insere o ID, como num vencimento) e a lista de invisíveis substitui o prazo (com `0`, remove).  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation`

---

### UC-INV-008

**Caso:** Ack  
**Então:** remove a mensagem e os metadados e remove a entrada da lista de invisíveis.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_ack_removes_the_copy_and_only_the_owner_counts_it`
  - `tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused`
  - `tests/replication.rs::an_ack_of_a_gone_message_answers_404_even_when_the_primary_answers_last`

---

### UC-INV-010

**Caso:** O registro é exclusivo  
**Então:** o nó rejeita se já há outro pendente para a mensagem (`409` `cluster.conflict`) ou, numa entrega, se ela já está invisível. Na alteração e no ack, também se o `receiptToken` não confere (`409` `message.stale-receipt`).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message`
  - `tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused`
  - `tests/replication.rs::an_ack_of_a_gone_message_answers_404_even_when_the_primary_answers_last`

---

### UC-INV-012

**Caso:** Falha antes do quórum  
**Então:** retry e, esgotado, rollback do pendente; as listas do dono **não** são tocadas e a mensagem segue como estava. Falha nas réplicas fora do quórum: sem rollback, o dono insiste até o teto.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-INV-013

**Caso:** `POST .../visibility/register` com `kind` `delivery` (levando `deliveryCount`, token novo) ou `change` (o `receiptToken` do cliente deve conferir com o atual); `commit` e `rollback` com `OperationRef`; erros `404` `message.not-found`, `409` `cluster.conflict` e `409` `message.stale-receipt`.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-INV-014

**Caso:** O `commit` da entrega remove o ID da lista de prontos do nó, e o primário **sempre recebe** essa chamada antes da resposta ao cliente.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-INV-016

**Caso:** O vencimento **não se propaga**  
**Então:** nada é escrito nos metadados nem enviado às réplicas; `deliveryCount` só sobe na próxima entrega.  
**Nível:** C  
**Testes:**
  - `tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation`

---

### UC-INV-017

**Caso:** A thread do dono olha a cabeça da lista de invisíveis, dorme até o prazo dela (com um teto curto para reagir a alterações e entradas mais cedo) e, no vencimento, remove a entrada e **confere** nos metadados  
**Então:** a mensagem ainda existe, o nó ainda é dono e o prazo ainda é o da entrada; senão descarta e segue.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_head_is_taken_out_and_checked_and_a_pair_that_is_not_the_messages_is_dropped`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/engine/messages/tests/bounded.rs::reopening_a_topic_rebuilds_the_counts_and_the_window_from_the_disk`

---

### UC-INV-018

**Caso:** Passada a conferência, a mensagem entra na lista de prontos pela posição do GUID v7 (perto da ponta antiga); com a lista cheia, o ID do meio sai.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::after_the_check_the_message_joins_the_ready_list_by_its_position_and_the_middle_leaves`

---

### UC-INV-019

**Caso:** Passada a conferência com `deliveryCount ≥ maxDeliveries`, a mensagem **não** volta à lista  
**Então:** vai para a dead-letter. Mensagem vencida por `ttl` também é descartada, sem voltar.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure`
  - `tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node`

---

### UC-INV-020

**Caso:** Na fila FIFO, a mensagem que volta é a cabeça do grupo; o grupo deixa de estar ocupado e entra na lista de prontos pela posição do ID, sem a regra do meio da lista.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/tests.rs::in_a_fifo_queue_the_returning_message_is_the_group_head_and_frees_the_group`
  - `src/engine/messages/index/bounded_tests.rs::on_a_fifo_queue_the_ready_list_has_no_ceiling_and_no_middle_rule`
  - `src/engine/messages/tests/bounded.rs::small_lists_choose_the_same_messages_as_large_ones`

---

### UC-INV-021

**Caso:** Se a lista de invisíveis esvazia e ainda há linhas na tabela de expiração, a thread faz a recarga antes de dormir.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::when_the_list_empties_with_pairs_in_the_table_the_reload_is_asked_for_before_sleeping`

---

### UC-INV-022

**Caso:** A lista de invisíveis é, por tópico e só no dono, uma lista de pares (ID, `invisibleUntil`) ordenada por prazo, com no máximo `delivery.invisible-list.size` entradas; busca por ID para alteração e ack.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_invisible_list_is_ordered_by_deadline_with_a_ceiling_and_found_by_id`
  - `src/engine/messages/tests/bounded.rs::the_index_keeps_no_more_entries_than_the_window_and_the_list_hold`

---

### UC-INV-023

**Caso:** Regra de inclusão com a lista cheia  
**Então:** se o prazo novo é **anterior** ao último da lista, o par novo entra e o último vai para a tabela; senão o par novo vai direto para a tabela.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::with_a_full_list_an_earlier_deadline_enters_and_the_last_goes_to_the_table`

---

### UC-INV-024

**Caso:** Invariante  
**Então:** tudo na lista vence antes de tudo na tabela; a thread nunca consulta a tabela enquanto a lista não esvazia.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`

---

### UC-INV-025

**Caso:** Recarga  
**Então:** abaixo de `delivery.invisible.reload-threshold` (ou vazia), a thread lê da tabela os pares de menor `invisibleUntil` (consulta ordenada com limite pelo índice), move-os para a lista e apaga as linhas lidas.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`
  - `src/engine/messages/index/bounded_tests.rs::the_reload_starts_below_the_threshold_reads_the_earliest_and_deletes_the_rows_read`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/engine/messages/tests/bounded.rs::ack_and_visibility_work_on_a_message_the_index_only_counts`

---

### UC-INV-026

**Caso:** Sem entradas velhas  
**Então:** ack, alteração e nova entrega apagam ou atualizam a linha da tabela no mesmo commit que atualiza a lista. Não há remoção preguiçosa.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
**Observação:** A mudança de prazo, o ack e a nova entrega apagam ou atualizam a linha no mesmo passo em que atualizam a lista; se a gravação falhar, o par fica na fila e é repetido na próxima mudança (só a entrega desfaz a operação, ver UC-INV-027).

---

### UC-INV-027

**Caso:** A gravação na tabela faz parte do commit no dono  
**Então:** se falhar, o commit não se completa e a entrega é desfeita antes da resposta ao cliente.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/expiry.rs::a_failed_table_write_undoes_the_delivery_before_the_answer`
  - `src/engine/messages/tests/cluster.rs::a_failed_table_write_undoes_a_reservation_of_the_owner`
**Observação:** Provado no caminho de um nó (`take`); o caminho do cluster (`take_candidates`) aplica a mesma regra, mas ainda não tem teste.

---

### UC-INV-028

**Caso:** Com a lista com espaço, nenhuma escrita além dos metadados.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/index/bounded_tests.rs::with_room_in_the_list_nothing_is_written_beyond_the_metadata`

---

### UC-INV-029

**Caso:** Novo dono de um bloco  
**Então:** ao terminar a sincronização, percorre os metadados das mensagens de que agora é primário: as com `invisibleUntil` no futuro entram na lista de invisíveis (ou na tabela de expiração, pela regra de inclusão) e as demais viram candidatas da lista de prontos.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---

### UC-INV-030

**Caso:** Quem deixa de ser dono não limpa nada na hora  
**Então:** as entradas que restarem falham na conferência de dono e são descartadas quando lidas.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---


## ARM: Armazenamento: memória, overflow, SQLite, integridade

### UC-ARM-001

**Caso:** A mensagem é gravada primeiro em memória; ao atingir `memory.limit`, as excedentes são persistidas em disco.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`
  - `tests/messages.rs::without_a_memory_limit_nothing_goes_to_disk_by_overflow`

---

### UC-ARM-002

**Caso:** O corpo vai para `${topic}/${id}.body` (uma pasta por tópico, um arquivo por mensagem), com os **bytes crus**, sem reserializar nem codificar; o `contentType` fica no metadado.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-003

**Caso:** Ao ler uma mensagem que não está em memória, ela é buscada no disco pelo mesmo caminho.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::delivery_merges_memory_and_disk_in_id_order_with_the_exact_bytes`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-004

**Caso:** A memória não é dividida de forma fixa entre os tópicos  
**Então:** tópicos de maior taxa de leitura ficam com mais mensagens em memória; os de menor taxa vão para disco primeiro.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk`
  - `src/engine/messages/tests.rs::above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less`
  - `src/engine/messages/tests.rs::a_topic_that_is_read_more_is_never_demoted_by_one_that_is_read_less`

---

### UC-ARM-005

**Caso:** A taxa de leitura é a média móvel das mensagens consumidas por segundo na janela `memory.rate.window` (5 minutos), a mesma de `publishRate` e `consumeRate`.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/rates/tests.rs::the_rate_is_the_moving_average_over_the_window`
  - `src/engine/messages/rates/tests.rs::what_left_the_window_stops_counting`
  - `src/engine/messages/rates/tests.rs::several_events_in_one_second_share_a_bucket`
  - `src/engine/messages/rates/tests.rs::publish_and_consume_are_counted_apart`
  - `src/engine/messages/tests.rs::above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less`
  - `src/engine/messages/tests.rs::publish_and_consume_rates_are_tracked_per_topic`
  - `src/storage/messages/tests.rs::the_rows_of_a_block_come_by_position_in_id_order_with_range_and_limit`
  - `src/storage/messages/tests.rs::the_end_of_a_group_is_read_by_an_index_in_id_order`

---

### UC-ARM-006

**Caso:** Cada tópico tem uma reserva em memória (`memory.topic.reserve`, 1000 mensagens), **independente da taxa**  
**Então:** um tópico novo, sem histórico de leitura, não começa jogando as primeiras mensagens em disco. A divisão pela taxa vale só acima das reservas.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk`

---

### UC-ARM-007

**Caso:** O que permanece em memória são as mensagens da lista de prontos e as recém-chegadas; o resto é candidato a disco, começando pelo tópico de menor taxa. Não se usa LRU/LFU.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests.rs::demotion_takes_the_middle_of_the_ready_list_and_keeps_the_ends`
  - `src/engine/messages/tests.rs::messages_in_processing_are_demoted_before_the_ready_ones`

---

### UC-ARM-008

**Caso:** A lista de prontos e as mensagens em memória não mudam por causa do banco  
**Então:** ele só cuida do que está em disco.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests.rs::the_database_only_holds_what_is_on_disk_and_memory_messages_never_touch_it`

---

### UC-ARM-009

**Caso:** `taba.storage.*`  
**Então:** memória usada e limite, mensagens em memória e em disco, disco usado e livre, overflow, leituras de disco e de memória, duração e erros de disco.  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::storage_metrics_count_overflow_and_reads_by_tier`
  - `tests/metrics.rs::free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure`

---

### UC-ARM-010

**Caso:** Cada nó mantém um banco por tópico, `${topic}/_topic.db`, estritamente local  
**Então:** nunca é copiado, enviado nem sincronizado com outros nós.  
**Nível:** I  
**Testes:**
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-011

**Caso:** A tabela `messages` tem uma linha por mensagem em disco (`id`, posição no anel, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`; na FIFO, `deduplicationId` e `deleteAt`) com índices por `id`, `invisibleUntil`, posição, `(group_id, id)`, `(group_id, deduplicationId)` e `deleteAt`.  
**Nível:** I  
**Testes:**
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `src/storage/messages/tests.rs::a_topic_database_from_before_the_fifo_columns_is_upgraded_in_place`

---

### UC-ARM-012

**Caso:** A tabela `expiry` guarda os pares que não couberam na lista de invisíveis, com índice por prazo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/storage/messages/tests.rs::the_expiry_table_keeps_the_pairs_that_did_not_fit_ordered_by_deadline`

---

### UC-ARM-013

**Caso:** Toda **escrita** no banco de um tópico passa por um **escritor único** (uma fila de comandos por tópico, executada uma de cada vez); leituras longas usam conexões próprias em modo WAL e não bloqueiam o escritor.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/tests.rs::a_slow_topic_never_blocks_the_other_topics`

---

### UC-ARM-014

**Caso:** Ordem de gravação  
**Então:** na escrita, primeiro o arquivo do corpo, depois a linha; no ack, primeiro a linha, depois o arquivo. Uma queda deixa no máximo arquivo sem linha ou linha sem arquivo.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::a_message_on_disk_is_delivered_again_and_acked_like_any_other`
  - `src/storage/messages/tests.rs::removing_deletes_the_row_and_then_the_file`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-015

**Caso:** O nó que reinicia depois de uma queda volta como novo e refaz a partir do cluster.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ARM-017

**Caso:** `storage.db.synchronous` (padrão `NORMAL` com WAL) é aplicado ao abrir o banco.  
**Nível:** I  
**Testes:**
  - `src/storage/db/tests.rs::opens_in_wal_mode_with_the_configured_synchronous`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---

### UC-ARM-021

**Caso:** O nó cria `storage.dir` (padrão `/var/lib/taba`, a última pasta se chama `taba`) ao iniciar; se não conseguir criar ou gravar, não sobe.  
**Nível:** I  
**Testes:**
  - `tests/startup.rs::a_storage_dir_that_cannot_be_created_stops_startup`
  - `tests/startup.rs::startup_creates_storage_dir_when_missing`

---

### UC-ARM-022

**Caso:** Um nó que perde um bloco (mudança do anel) **não apaga nada na hora**  
**Então:** continua guardando as mensagens do bloco e pode servir de origem de sincronização. Só apaga (memória, `.body` e linhas do banco, por faixa da posição no anel) quando **todo o cluster está íntegro**.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay`
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-ARM-023

**Caso:** Se o nó novo não chega a ficar saudável, o anel volta ao anterior e o nó antigo volta a ser o responsável, com as mensagens intactas, sem nada a refazer. Se o anel mudar de novo antes da limpeza, o nó recalcula quais blocos ainda não são dele.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-ARM-024

**Caso:** Mensagem recebida como réplica ou publicada em modo drain vai direto para disco (limite de memória efetivo zero).  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests.rs::drain_saves_everything_in_memory_to_disk`

---

### UC-ARM-026

**Caso:** O log de excluídos tem **limite de idade** (`storage.excluded.max-age`, 24 h) e **de tamanho** (`storage.excluded.max-size`, 100 MB, somando todos os tópicos)  
**Então:** os registros mais antigos saem primeiro, de qualquer tópico, até o log caber; a poda roda a cada 30 s.  
**Nível:** U  
**Testes:**
  - `src/storage/messages/excluded/tests.rs::the_limits_drop_the_oldest_rows_first_and_move_the_start_forward`
  - `src/storage/messages/excluded/tests.rs::the_size_limit_counts_every_topic_and_drops_the_oldest_of_all`

---

### UC-ARM-027

**Caso:** O log guarda a **data de início da gravação contínua**: começa na criação do log (no `_node.db`, sobrevive ao reinício) e só avança quando um registro é descartado por um limite  
**Então:** ela passa a ser o instante do registro mais novo descartado; sem descarte, um cluster sem exclusões por dias continua cobrindo esses dias.  
**Nível:** U  
**Testes:**
  - `src/storage/messages/excluded/tests.rs::the_limits_drop_the_oldest_rows_first_and_move_the_start_forward`
  - `src/storage/messages/excluded/tests.rs::the_start_of_the_recording_does_not_move_without_a_discarded_row`
  - `src/storage/messages/excluded/tests.rs::the_start_of_the_recording_is_kept_in_the_node_database`

---

### UC-ARM-028

**Caso:** O log de uma **vida passada** é incorporado ao atual (`import_excluded`) e a data de início só recua se o intervalo antigo e o atual se encontram, sem buraco  
**Então:** os registros somam (o instante mais antigo de um `id` fica); com buraco, a data de início continua onde está.  
**Nível:** U  
**Testes:**
  - `src/storage/messages/excluded/tests.rs::a_past_life_adds_its_rows_and_moves_the_start_back_only_if_there_is_no_hole`

---

### UC-ARM-029

**Caso:** O registro é gravado na **mesma transação** que apaga a mensagem, com `storage.db.synchronous` como configurado (`NORMAL` por padrão): sem fsync extra no ack. Um banco de tópico de versão anterior ganha a tabela `excluded` ao abrir  
**Então:** a mensagem some e o registro existe, ou nenhum dos dois; nenhuma sincronização além da configurada.  
**Nível:** U  
**Testes:**
  - `src/storage/messages/excluded/tests.rs::removing_with_exclusion_deletes_the_message_and_records_it_in_the_same_transaction`
  - `src/storage/messages/excluded/tests.rs::recording_adds_no_synchronization_beyond_the_configured_one`
  - `src/storage/messages/excluded/tests.rs::a_topic_database_from_before_the_log_gets_the_table`

---


## ING: Entrada de nó (bootstrap)

### UC-ING-001

**Caso:** Com `replication.factor` acima de 1, sem `cluster.entry-address` o nó **não sobe** (com fator 1 o nó parte sozinho).  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::the_entry_address_must_be_host_and_port`
  - `tests/startup.rs::a_cluster_without_the_entry_address_does_not_start`

---

### UC-ING-002

**Caso:** Ao iniciar, o nó chama `GET /cluster/nodes` pelo endereço do balanceador (porta do cluster) e recebe a `MemberTable` (`version` e `members` com `nodeId`, `addresses`, `port`, `state`). Qualquer nó responde; um chamador que ainda não é membro é aceito.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::the_first_node_starts_the_cluster_alone`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster.rs::only_members_call_the_phase_routes_but_anyone_lists_and_asks_to_join`
  - `tests/cluster_api.rs::the_cluster_port_serves_only_cluster_routes_and_the_client_port_none`

---

### UC-ING-003

**Caso:** O nó escolhe um membro **ao acaso** da lista e envia `POST /cluster/nodes/join` (`JoinRequest`  
**Então:** `nodeId`, `addresses`, `port`, `version`), numa única requisição síncrona; esse membro coordena o registro. Se o membro não responde, tenta outro da lista.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::the_join_request_has_the_documented_fields`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster_api.rs::the_join_route_registers_the_node_and_answers_with_the_table`

---

### UC-ING-004

**Caso:** Se a listagem falha, o nó tenta de novo a cada `cluster.bootstrap.retry.interval`.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::without_the_first_node_flag_the_node_keeps_trying_and_stays_out_of_rotation`

---

### UC-ING-005

**Caso:** Só o **primeiro nó** de um cluster novo (`cluster.first-node=true`), quando a listagem não encontra ninguém, inicia o cluster sozinho como único membro. Sem a flag, a listagem sem resposta é erro e o nó tenta de novo.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::the_first_node_starts_the_cluster_alone`
  - `tests/cluster.rs::without_the_first_node_flag_the_node_keeps_trying_and_stays_out_of_rotation`

---

### UC-ING-006

**Caso:** `GET /cluster/nodes` num nó que ainda não tem a tabela (sincronizando)  
**Então:** `503` `cluster.node-syncing`.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_node_that_has_no_table_yet_answers_503`
  - `tests/cluster_api.rs::a_node_without_the_table_answers_503_with_retry_after`

---

### UC-ING-007

**Caso:** O nó se auto-nomeia com um **GUID v7** a cada início do processo, só em memória, e se identifica por ele, inclusive no pedido de ingresso.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/dto_tests.rs::the_node_names_itself_with_a_new_guid_v7_at_each_start_and_the_join_request_carries_it`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-ING-008

**Caso:** A tabela de membros guarda o IP e a **porta do cluster** de cada nó, e toda chamada entre nós vai direto a eles, sem balanceador (só o `GET /cluster/nodes` inicial passa por ele).  
**Nível:** C  
**Testes:**
  - `src/cluster/members/tests.rs::callers_are_recognized_by_the_addresses_in_the_table`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-009

**Caso:** O membro que recebe o `join` **testa os IPs** informados, conectando de volta à porta do cluster. Falha  
**Então:** `422` `cluster.join-unreachable`, o nó é excluído e repete o bootstrap.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_new_node_the_member_cannot_connect_back_to_is_refused`

---

### UC-ING-010

**Caso:** Conexão bem-sucedida  
**Então:** o membro envia a **todos os nós** o pedido de registro (`POST /cluster/nodes/register`, fase 1); com todos confirmando, envia o commit (`POST /cluster/nodes/commit`, fase 2), e só então responde ao nó novo com a tabela, já efetivada em todo o cluster.  
**Nível:** C  
**Testes:**
  - `src/cluster/members/tests.rs::register_leaves_the_change_pending_and_commit_makes_it_effective`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster_api.rs::the_join_route_registers_the_node_and_answers_with_the_table`

---

### UC-ING-011

**Caso:** O nó novo recebe só a tabela de nós; a tabela de tópicos (e a de tokens) ele puxa depois por stream de um nó do cluster.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-013

**Caso:** Confirmação de **todos** os nós, sem quórum reduzido  
**Então:** com um nó sem confirmar o registro, o ingresso não se efetiva.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-014

**Caso:** `POST /cluster/nodes/register` com outra operação pendente  
**Então:** `409` `cluster.conflict`; `commit` de operação desconhecida: `404` `cluster.operation-unknown`.  
**Nível:** C  
**Testes:**
  - `src/cluster/members/tests.rs::all_three_phases_are_idempotent_by_operation_id`
  - `src/cluster/members/tests.rs::a_second_change_while_one_is_pending_is_a_conflict`
  - `src/cluster/members/tests.rs::commit_and_rollback_of_an_unknown_operation_are_unknown`
  - `tests/cluster.rs::a_second_pending_registration_conflicts_and_unknown_operations_are_refused`
  - `tests/cluster_api.rs::the_bodies_of_the_cluster_routes_are_validated`

---

### UC-ING-015

**Caso:** Falha **antes do commit** (teste de conexão, ou fase de registro sem confirmação de todos)  
**Então:** rollback do registro pendente, o nó é excluído (não chega à tabela), e o `join` responde `503` `cluster.registration-failed`.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-016

**Caso:** Os blocos que o nó assumiria são rebalanceados entre os restantes  
**Então:** o anel é recalculado sem esse nó.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-017

**Caso:** Nova tentativa  
**Então:** o nó repete o bootstrap do começo a cada `cluster.bootstrap.retry.interval` até `cluster.bootstrap.retry.max-attempts` (`0` = sem limite); esgotado, o nó encerra com erro. O mesmo intervalo vale para falha na listagem, no ingresso e na sincronização.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::the_node_gives_up_after_the_maximum_attempts`
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-018

**Caso:** Falha **no commit** (registro já confirmado por todos)  
**Então:** não é desfeita; o membro insiste até `write.commit.timeout`, não aborta o nó novo, e o `join` responde `503` `cluster.commit-timeout` (o nó foi aceito). Antes de repetir, o nó consulta `GET /cluster/nodes`.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_commit_that_does_not_arrive_does_not_undo_the_join`

---

### UC-ING-020

**Caso:** `taba.cluster.bootstrap.failures` conta as falhas por `reason` (teste de conexão, fase 1, fase 2); falha de bootstrap gera log `error`.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-021

**Caso:** Ao assumir a posição no anel, o nó passa a ser responsável por blocos como primário, secundário, terciário etc., e inicia a sincronização (casos `SIN`).  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ING-022

**Caso:** Prontidão  
**Então:** o nó só fica pronto (`/health/ready` `200`) quando termina toda a sincronização. Até lá consta na tabela e confirma registros gravando-os no banco de eventos, mas **não atende clientes, não coordena e não entrega**.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-ING-023

**Caso:** Depois de pronto, o nó passa a ser também nó gestor das filas voláteis cujo `id` cai na sua faixa do anel.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-ING-024

**Caso:** O estado do nó passa por `joining` e `syncing` até `active` (`taba.node.state`); `/health/ready` informa `bootstrapping` e `syncing` como motivo.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ING-027

**Caso:** Uma versão (época) da tabela de membros diferente entre nós indica propagação incompleta (condição `membership-divergent`, `taba.cluster.members.version`).  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-028

**Caso:** Cada listagem de membros pelo `cluster.entry-address` abre uma **conexão nova** (`Connection: close`), sem reaproveitar a de uma tentativa anterior  
**Então:** o balanceador decide o nó por conexão, e uma conexão reaproveitada prenderia o bootstrap do nó que reinicia no próprio nó (que ainda não tem a tabela e responde `cluster.node-syncing`).  
**Nível:** C  
**Testes:**
  - `src/cluster/transport/http/tests.rs::every_node_listing_opens_a_new_connection_so_the_balancer_can_pick_another_node`

---

### UC-ING-029

**Caso:** Um nó de versão **mais nova** entra num cluster com nós de versão anterior  
**Então:** ele os **retira, um a um**, pelo drain que já existe (a versão de cada membro vem do `version` do `NodeStatus`). O drain seguinte só começa depois que o anterior saiu da tabela; no máximo um membro está em `draining` por vez. O primeiro a sair é o de versão mais antiga (quem não publica versão conta como o mais antigo), e a convivência não é tratada com compatibilidade: a versão nova retira as anteriores.  
**Nível:** C  
**Testes:**
  - `tests/upgrade.rs::nodes_of_the_new_version_retire_the_old_ones_one_at_a_time`
  - `src/cluster/upgrade/tests.rs::the_newest_version_drains_the_oldest_member_first`
  - `src/cluster/upgrade/tests.rs::a_cluster_of_one_version_has_nothing_to_retire`
  - `src/cluster/upgrade/tests.rs::the_next_drain_waits_for_the_previous_one_to_leave`

---

### UC-ING-030

**Caso:** O drain do upgrade ainda não pode começar  
**Então:** ele **espera** se o cluster ficaria com menos de **3 nós ativos** depois do drain, se o nó que age ainda não está `active` (ou está em drain), se algum membro está em `syncing`, ou se algum membro não respondeu a consulta de versão (a rodada não decide nada).  
**Nível:** C  
**Testes:**
  - `tests/upgrade.rs::the_upgrade_does_not_drain_if_fewer_than_three_would_stay_active`
  - `src/cluster/upgrade/tests.rs::the_drain_does_not_start_if_it_would_leave_fewer_than_three_active`
  - `src/cluster/upgrade/tests.rs::the_drain_does_not_start_while_the_node_is_not_ready_or_a_member_is_syncing`
  - `src/cluster/upgrade/tests.rs::a_member_that_did_not_answer_makes_the_round_decide_nothing`

---

### UC-ING-031

**Caso:** Um nó não termina o drain pedido pelo upgrade dentro do prazo (`drain.timeout` mais 30 s)  
**Então:** o upgrade **para nesse nó** (não pede o drain de outro) e a condição `upgrade-stalled` (aviso) aparece em `/health/cluster`; o operador decide cancelar o drain do nó ou pará-lo. Quando o nó sai da tabela, a condição some.  
**Nível:** U  
**Testes:**
  - `src/cluster/upgrade/tests.rs::a_drain_that_does_not_finish_in_time_stops_the_upgrade_on_that_member`
  - `src/cluster/health/tests.rs::a_stalled_upgrade_raises_its_condition_as_a_warning`

---

### UC-ING-032

**Caso:** O nó decide se envia a um par algo que só uma versão nova entende  
**Então:** só envia a quem **anunciou** uma versão igual ou maior; quem não anunciou versão, não respondeu ou ainda não foi visto conta como versão anterior. O próprio nó sempre entende o que a sua versão trouxe.  
**Nível:** C  
**Testes:**
  - `tests/upgrade.rs::a_node_is_only_sent_what_its_version_announced_support_for`
  - `src/cluster/upgrade/tests.rs::a_member_supports_what_a_version_brought_only_if_it_announced_that_version`

---

### UC-ING-033

**Caso:** Um nó de versão **anterior** entra num cluster de versão mais nova  
**Então:** ele **não drena** os mais novos: os nós novos o retiram, e os três continuam ativos.  
**Nível:** C  
**Testes:**
  - `tests/upgrade.rs::an_older_node_that_joins_is_retired_and_does_not_drain_the_newer_ones`
  - `src/cluster/upgrade/tests.rs::an_earlier_version_does_not_drain_the_newer_ones`

---

### UC-ING-034

**Caso:** Vários nós da versão mais nova veem membros anteriores ao mesmo tempo  
**Então:** só **um age**: o de menor `nodeId` entre os da versão mais nova; os outros esperam. Assim dois nós não pedem drains diferentes ao mesmo tempo.  
**Nível:** U  
**Testes:**
  - `src/cluster/upgrade/tests.rs::only_the_lowest_id_among_the_newest_acts`

---

### UC-ING-035

**Caso:** O nó compara duas versões do programa  
**Então:** vale a precedência do semver: `0.1.9` vem antes de `0.1.10`, um candidato (`-rc.N`) vem antes da sua versão final, o prefixo `v` e o sufixo `+build` não contam, e um texto que não é versão não é nem anterior nem posterior (nunca é drenado por engano).  
**Nível:** U  
**Testes:**
  - `src/cluster/upgrade/tests.rs::versions_compare_by_precedence_and_a_candidate_comes_before_its_final`

---

### UC-ING-036

**Caso:** Um disco de **versão anterior** (sem manifesto) não é migrado: não é uma vida
**Então:** o nó novo segue o caminho de hoje: põe o disco em `_previous-life`, devolve só as perdas listadas e apaga a pasta ao fim.
**Nível:** U
**Testes:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`

---

### UC-ING-037

**Caso:** Enquanto um membro **não informa** desde quando grava o log de excluídos (um nó de versão anterior, ou que não responde), **nenhuma vida é restaurável**
**Então:** a data de corte do cluster fica indefinida e as vidas ficam fora da janela (seguem o caminho da lista de perdas); quando todos informam, a vida volta a ser restaurada.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::no_life_is_restorable_while_a_member_does_not_tell_when_its_log_started`

---


## FAL: Detecção de falhas, votação de exclusão e saída por falha

### UC-FAL-001

**Caso:** O controle de quem está online é feito pelo canal permanente em malha, sem coordenador; **não há ping separado**.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::there_is_no_ping_apart_from_the_channel_between_the_nodes`

---

### UC-FAL-002

**Caso:** Canal que cai ou fica sem sinal de vida  
**Então:** o nó que detectou **tenta reconectar** (`cluster.channel.reconnect.retries` e `backoff`) antes de agir. Um nó travado com a conexão aberta é detectado pela falta do `keepalive`.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded`

---

### UC-FAL-003

**Caso:** Reconexão antes de esgotar as tentativas  
**Então:** nada muda (nenhuma votação é aberta).  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_channel_that_returns_before_the_retries_end_changes_nothing`

---

### UC-FAL-004

**Caso:** Canal ainda fora depois das tentativas  
**Então:** é aberta a votação de exclusão entre os nós do cluster.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-005

**Caso:** A votação também é aberta quando a fila de saída do canal estoura o limite (`reason: queue-overflow`) e quando o nó entra em falha (`reason: node-failed`); o motivo normal é `channel-down`.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up`
  - `tests/failures.rs::a_queue_that_overflows_opens_the_vote_that_excludes_the_unreachable_node`

---

### UC-FAL-006

**Caso:** Enquanto a votação não conclui, os sinais para o nó ficam acumulados em memória; reconectando antes, o envio é retomado.  
**Nível:** C  
**Testes:**
  - `tests/channel.rs::a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns`

---

### UC-FAL-007

**Caso:** A exclusão exige `ceil(S × 51%)` votos de S saudáveis (`cluster.vote.quorum-percent`)  
**Então:** 2 de 3, 3 de 4, 3 de 5.  
**Nível:** U  
**Testes:**
  - `src/cluster/vote/tests.rs::the_quorum_is_the_ceiling_of_the_percentage_of_the_healthy_nodes`

---

### UC-FAL-008

**Caso:** Nó **saudável** é o que quem abre a votação enxerga com o canal ativo e que responde ao pedido de voto. Os suspeitos (fora do ar) **não votam e não entram na base**.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-009

**Caso:** Se vários nós caem ao mesmo tempo (por exemplo, uma zona), a votação segue só entre os saudáveis.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-010

**Caso:** Com menos saudáveis que `cluster.vote.min-voters` (padrão 2), nenhuma exclusão se efetiva e a condição `quorum-lost` aparece; o mínimo impede que um nó isolado exclua todos os demais.  
**Nível:** C  
**Testes:**
  - `tests/conditions.rs::with_two_of_three_nodes_out_the_quorum_is_at_risk_and_the_writes_can_fail`
  - `tests/conditions.rs::the_quorum_is_lost_only_for_who_sees_fewer_healthy_nodes_than_the_minimum`
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`
  - `tests/failures.rs::with_only_two_nodes_a_cut_excludes_nobody`

---

### UC-FAL-011

**Caso:** `POST /cluster/vote-remove` (`VoteRequest`  
**Então:** `voteId`, `candidateNodeId`, `openedBy`, `reason`) responde `VoteResponse` com `vote` `yes` ou `no`, conforme o que o votante enxerga do candidato (por exemplo, `no` se ele enxerga o canal ativo). Quem não responde não entra na base.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::vote_remove_answers_no_to_who_still_reaches_the_candidate_and_yes_to_who_does_not`

---

### UC-FAL-012

**Caso:** Com várias exclusões pendentes, elas são votadas uma de cada vez, cada uma sobre os saudáveis do momento.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-013

**Caso:** Risco aceito  
**Então:** numa partição de rede em que os dois lados têm saudáveis suficientes, cada lado pode excluir o outro; o lado excluído, ao voltar, refaz o bootstrap do zero (e, se **os dois** excluíram, o reencontro é o UC-FAL-026).  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FAL-014

**Caso:** Cluster de 3 nós com um fora do ar  
**Então:** os 2 saudáveis votam e o quórum de 2 se atinge.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-015

**Caso:** Votação e resultado são registrados (`taba.cluster.vote` por `role` e `outcome`; logs `warn` e `info`).  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-016

**Caso:** Atingido o quórum, o desregistro segue o protocolo de duas fases  
**Então:** `POST /cluster/nodes/unregister` (`reason: vote`) a todos os nós e `POST /cluster/nodes/commit`, que remove o nó da tabela em todo o cluster.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-017

**Caso:** Falha antes do commit  
**Então:** rollback. Depois de decidido: sem rollback, o coordenador insiste dentro de `write.commit.timeout` até confirmar a remoção em todos.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::an_exclusion_that_fails_before_the_commit_rolls_back_and_the_vote_is_held_again`
  - `tests/failures.rs::once_the_exclusion_is_decided_the_commit_is_insisted_until_every_node_confirms`

---

### UC-FAL-018

**Caso:** No commit, o anel é recalculado e os blocos do nó removido são **rebalanceados** entre os restantes; os novos primários das posições dele assumem as filas voláteis que ele gerenciava, sem transferir estado.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::when_the_manager_leaves_the_new_primary_of_its_positions_takes_over`

---

### UC-FAL-019

**Caso:** `taba.cluster.membership.changes` conta por `operation`, `reason` (`vote`, `drain`), `phase` e `outcome`.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-020

**Caso:** Concluída a exclusão, todos os nós descartam as filas de sinais para aquele nó.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-021

**Caso:** Entre a queda e o desregistro, as mensagens de que o nó era dono ficam sem entrega; depois do desregistro, o novo primário (herdeiro do bloco) as entrega.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary`

---

### UC-FAL-022

**Caso:** Depois da exclusão, cada mensagem do nó removido continua em pelo menos `replication.factor − 1` nós.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary`

---

### UC-FAL-023

**Caso:** Nó em **falha** (disco cheio, banco corrompido, banco de eventos acima de `cluster.sync.log.max-bytes`, ou sem origem de carga até `cluster.sync.timeout`) sinaliza isso ao cluster, e os demais ficam liberados para votar o desregistro dele.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up`
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`
  - `tests/operations.rs::a_corrupt_topic_database_fails_the_node`

---

### UC-FAL-024

**Caso:** As condições do cluster (lista, não estado único), cada uma com severidade  
**Então:** `member-unreachable`, `node-frozen`, `syncing`, `under-replicated`, `write-quorum-at-risk` (réplicas íntegras abaixo de `write.quorum`), `quorum-lost`, `membership-divergent`, `storage-pressure`; `ok` quando nenhuma.  
**Nível:** C  
**Testes:**
  - `src/cluster/health/tests.rs::merging_nodes_unions_the_conditions_and_flags_divergent_tables`
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/conditions.rs::a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk`

---

### UC-FAL-025

**Caso:** `under-replicated` (réplicas abaixo de `replication.factor`, mas **no ou acima** do quórum) é aviso transitório  
**Então:** as escritas continuam respondendo (menos as dos blocos cujo primário está fora do ar, até a exclusão, o que a condição `write-quorum-at-risk` anuncia: D107). `write-quorum-at-risk` é crítica: as escritas podem falhar por não fechar quórum, e também aparece quando o **primário** de algum bloco está fora do ar (o quórum sempre o inclui): D107.  
**Nível:** C  
**Testes:**
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/conditions.rs::with_two_of_three_nodes_out_the_quorum_is_at_risk_and_the_writes_can_fail`
  - `tests/conditions.rs::a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk`

---

### UC-FAL-026

**Caso:** O reencontro depois de uma partição em que **os dois lados** se excluíram (as tabelas têm a mesma versão e nenhum canal liga um lado ao outro)  
**Então:** de tempos em tempos (`cluster.channel.keepalive.interval` × 10) cada nó consulta a tabela de quem ele viu ser excluído; se ela é de outro cluster (sem este nó e sem nenhum membro em comum) e este lado perde (tem menos membros; num empate, o do menor `nodeId`), o nó se reagrupa como o perdedor da sondagem dos IPs antigos (UC-RTN-040): passa a memória para o disco, o disco vira a pasta de uma vida e ele entra de novo com ID novo, então o que aceitou enquanto esteve separado é restaurado no cluster unido (D181). A marca de votação de um nó que saiu da tabela é esquecida ao sair (D101). O nó que ainda está **sincronizando** quando as fontes passam a uma tabela mais nova sem ele (o ingresso que ele concluía foi desfeito por este reencontro) não espera o `cluster.sync.timeout` para entrar em falha: esquece a tabela e refaz o ingresso.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`
  - `tests/partition.rs::the_half_that_loses_the_reunion_keeps_what_it_accepted_while_apart`
  - `tests/failures.rs::a_syncing_node_that_the_sources_excluded_redoes_the_join_instead_of_failing`

---

### UC-FAL-027

**Caso:** O nó em `failed` não atende chamadas de cliente  
**Então:** num cluster repassa a chamada a um nó são (como o nó em drain, UC-FWD); sem outro nó, e num nó só, responde `503` `node.failed` com `Retry-After`. As rotas de operação seguem (é por elas que se vê a falha). O nó continua respondendo as chamadas entre nós até ser excluído (D78.2).  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_failed_node_forwards_the_client_calls_to_a_healthy_one_until_it_leaves`
  - `tests/failures.rs::a_failed_single_node_refuses_the_client_calls_with_503`

---

### UC-FAL-028

**Caso:** Um erro fatal de disco (cheio ou corrompido) no `_node.db` (definições de tópico, tokens, estado, perdas) também põe o nó em `failed`, como nos bancos de mensagens  
**Então:** o nó não pode guardar o que o cluster já confirmou. Os demais erros valem só para a operação. O nó só sai de `failed` reiniciando o processo (num cluster, depois de excluído, entra como nó novo; D78.1, D78.3).  
**Nível:** C  
**Testes:**
  - `src/storage/db/tests.rs::a_full_disk_in_the_node_database_fails_the_node_from_any_of_its_tables`
  - `src/storage/db/tests.rs::an_error_that_is_not_fatal_in_the_node_database_does_not_fail_the_node`

---

### UC-FAL-029

**Caso:** Quando um nó cai, todos os saudáveis abrem a votação, mas só o de **menor `nodeId`** entre os que votaram coordena o desregistro (fase 1 e commit); os outros esperam a vez (`k × write.commit.timeout`) e, achando o nó já fora da tabela, não registram nada (D98).  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion`

---

### UC-FAL-031

**Caso:** A regra do reencontro depois da partição simétrica escolhe **um só** perdedor  
**Então:** o lado com menos membros; no empate, o lado cujo menor `nodeId` é o menor. Os dois lados, comparando as mesmas duas tabelas, chegam à mesma conclusão: um refaz o ingresso e o outro não (D101).  
**Nível:** U  
**Testes:**
  - `src/cluster/service/tests.rs::the_reunion_has_exactly_one_losing_side_the_smaller_and_in_a_tie_the_one_of_the_lowest_id`
  - `tests/failures.rs::after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`

---

### UC-FAL-032

**Caso:** Os nós excluídos por votação que o nó guarda para o reencontro (D101) ficam só em memória, no máximo 64 (os mais antigos saem primeiro) e deixam de ser consultados quando voltam à tabela.  
**Nível:** U  
**Testes:**
  - `src/cluster/members/tests.rs::the_excluded_nodes_kept_for_the_reunion_are_the_last_64_and_leave_when_back_in_the_table`

---

### UC-FAL-033

**Caso:** Os parâmetros do reencontro e da recuperação valem como decididos (D101, D110)  
**Então:** `cluster.reunion.interval` é dez vezes `cluster.channel.keepalive.interval` (no mínimo 1 s), `cluster.message-loss.ttl` é de 14 dias e `cluster.recovery.timeout` é de 5 minutos, e cada um pode ser trocado pela configuração.  
**Nível:** U  
**Testes:**
  - `src/cluster/service/tests.rs::the_reunion_and_recovery_parameters_default_to_what_was_decided_and_can_be_changed`

---


## FRZ: Período de freezing

### UC-FRZ-001

**Caso:** Um nó desregistrado por falha que **continua o mesmo processo** (mesmo ID, mesma memória) e tenta se registrar de novo durante `cluster.freezing.duration`  
**Então:** o pedido de ingresso é rejeitado com `403` `cluster.node-frozen` e `Retry-After` com o tempo que falta.  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-002

**Caso:** O cluster guarda o ID e o instante do desregistro de cada nó em freezing.  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-003

**Caso:** Passado o freezing, o nó entra normalmente  
**Então:** ele volta a tentar depois do `Retry-After`, dentro do laço de nova tentativa do bootstrap.  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-004

**Caso:** Não há lista de espera  
**Então:** o cluster não guarda o pedido rejeitado. A rejeição é registrada no log ("rejeitado (freezing)").  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-005

**Caso:** Um **reinício** do processo gera um ID novo  
**Então:** o nó entra como nó novo, **sem freezing**.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`

---

### UC-FRZ-006

**Caso:** A saída por **drain** não gera freezing (é voluntária e o retorno é com ID novo).  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/drain_cluster.rs::after_leaving_the_node_returns_as_a_new_node_with_a_new_id`

---

### UC-FRZ-007

**Caso:** `taba.cluster.freezing` e a condição `node-frozen` refletem os nós em freezing.  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::the_nodes_in_freezing_show_in_the_metric_and_in_the_node_frozen_condition`

---

### UC-FRZ-009

**Caso:** O freezing sobrevive a quem coordena  
**Então:** a resposta do `join` e `GET /cluster/nodes` levam `frozen` (`nodeId`, `frozenAt` em ms) com os excluídos ainda em freezing; o nó que entra depois da exclusão o instala (sem subir a `version`, e a tabela guardada não o tem), e o `join` do excluído que cair nele é recusado com `cluster.node-frozen` (D106).  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::a_node_that_joins_after_the_exclusion_inherits_the_freezing_of_the_excluded`

---

### UC-FRZ-010

**Caso:** Depois do commit da exclusão por votação, cada nó que a efetivou enfileira para **todos** os pares o quadro `frozen` (`nodeId`, `frozenAt` em ms). Quem ainda não conhecia esse freezing o guarda — mesmo com o excluído ainda na tabela dele — e o repassa aos outros pares (D106).  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-FRZ-011

**Caso:** O membro que estava fora do ar no commit da exclusão recebe o `frozen` quando o canal volta (a fila acumulada ou o instantâneo) e passa a recusar o `join` do excluído com `cluster.node-frozen`, antes de qualquer reconciliação da tabela.  
**Nível:** C  
**Testes:**
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-FRZ-012

**Caso:** O `frozen` que o nó já conhece é um eco  
**Então:** ele não o repassa e não muda o instante guardado (o primeiro vale).  
**Nível:** U  
**Testes:**
  - `src/cluster/members/tests.rs::a_freezing_learned_twice_keeps_the_first_instant_and_only_the_first_is_news`

---

### UC-FRZ-013

**Caso:** Um `frozen` cujo prazo (`frozenAt` + `cluster.freezing.duration`) já passou é ignorado. Na fila de saída o `frozen` não expira nem é juntado ao de outro nó  
**Então:** só um `frozen` do mesmo `nodeId` o substitui.  
**Nível:** U  
**Testes:**
  - `src/cluster/channel/queue/tests.rs::the_frozen_signal_never_expires_and_only_the_same_node_replaces_it`
  - `src/cluster/members/tests.rs::a_freezing_that_already_ended_is_not_learned`

---

### UC-FRZ-014

**Caso:** O instantâneo que se envia ao abrir o canal leva um `frozen` para cada nó ainda em freezing (cobre a fila perdida por um reinício de quem a tinha).  
**Nível:** U, C  
**Testes:**
  - `src/cluster/members/tests.rs::the_frozen_list_has_only_the_nodes_still_in_freezing`
  - `tests/freezing.rs::the_channel_snapshot_carries_the_nodes_still_in_freezing`

---


## CNC: Concorrência entre mudanças de membros

### UC-CNC-001

**Caso:** Depois que um nó entra (commit do registro), nenhuma outra entrada, saída ou rebalanceamento é efetivada até ele ficar **pronto**; pedidos de bootstrap e votações de exclusão que chegarem nesse intervalo **esperam**. Só há uma mudança de membros por vez.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-002

**Caso:** Enquanto uma votação de exclusão está em andamento (da abertura até o commit do desregistro), um novo pedido de bootstrap fica em espera; só é processado quando a votação termina, com ou sem exclusão efetivada.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-003

**Caso:** O desregistro voluntário (drain) conta como saída de membro  
**Então:** espera outra mudança em curso e, enquanto corre, bloqueia novos pedidos de bootstrap.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::two_nodes_draining_at_once_leave_one_after_the_other`
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-004

**Caso:** Exceção  
**Então:** o **próprio nó que está se sincronizando**: se ele entra em falha, a votação do desregistro dele é liberada.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`

---

### UC-CNC-005

**Caso:** Um nó que cai durante a sincronização de outro só é desregistrado depois que esta termina ou falha.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_falls_while_another_syncs_is_excluded_only_when_the_sync_ends`

---

### UC-CNC-006

**Caso:** Os blocos de um nó em sincronização não mudam no meio da carga (o cluster não aceita outra entrada, saída ou rebalanceamento), e o banco de eventos não traz mudanças da tabela de membros.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-007

**Caso:** Um nó que reinicia sem memória (mesmo IP e porta) antes de a votação concluir  
**Então:** o pedido de bootstrap entra na fila e espera; concluída a votação (o nó antigo removido), é processado como registro comum de nó novo, sem tratamento especial pelo endereço repetido.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CNC-009

**Caso:** Duas mudanças de membros coordenadas por membros diferentes ao mesmo tempo sempre têm um vencedor  
**Então:** a de menor `operationId` (a mais antiga) vence; quem coordena uma mudança e recebe o registro de uma mais antiga descarta a sua e aceita a outra; o coordenador deslocado desfaz o que registrou, espera um tempo sorteado e recomeça (o ingresso, sem desistir antes de 60 s, o prazo do ingresso); o vencedor repete o `409` dos pares até `write.commit.timeout`. Um registro mantido por participante nunca é deslocado (D94).  
**Nível:** U, C  
**Testes:**
  - `src/cluster/members/tests.rs::a_coordinator_yields_to_an_older_registration_and_never_to_a_newer_one`
  - `src/cluster/members/tests.rs::a_registration_held_for_another_coordinator_is_never_displaced`
  - `tests/sync.rs::two_joins_coordinated_by_different_members_have_a_winner_and_neither_fails`

---

### UC-CNC-010

**Caso:** A regra do cruzamento (a mais antiga vence; quem coordena cede a uma mais antiga e nunca a uma mais nova) vale para qualquer par de mudanças de membros, não só dois ingressos  
**Então:** uma exclusão que se cruza com um ingresso segue a mesma regra nos dois sentidos (D94).  
**Nível:** C  
**Testes:**
  - `src/cluster/members/tests.rs::an_exclusion_and_an_entry_that_cross_follow_the_same_rule_in_both_directions`

---

### UC-CNC-013

**Caso:** O membro que **perdeu o commit de uma saída** (a exclusão por votação ou o drain) aplica a mudança quando o canal a entrega  
**Então:** o quadro `membership` leva a mudança e a versão da tabela, e o receptor a aplica sem depender da pendente. A tabela dele fica igual à dos outros, o nó sai do anel e entra em freezing (se foi exclusão). O registro pendente da mesma operação some, e o commit tardio é inofensivo.  
**Nível:** C, U  
**Testes:**
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_is_applied_without_the_pending_one_and_the_late_commit_is_harmless`
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_clears_the_pending_one_it_stands_for`
  - `src/cluster/members/tests.rs::a_change_already_applied_or_older_is_ignored`
  - `src/cluster/members/tests.rs::the_freezing_of_a_node_learned_by_the_membership_frame_counts_from_the_instant_it_was_learned`
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-CNC-014

**Caso:** O membro que **perdeu o commit de uma entrada** também a recebe pelo canal  
**Então:** o ingresso não se desfaz e o outro membro não fica para trás da tabela mesmo com o commit perdido.  
**Nível:** C  
**Testes:**
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_is_applied_without_the_pending_one_and_the_late_commit_is_harmless`
  - `tests/cluster.rs::a_commit_that_does_not_arrive_does_not_undo_the_join`
  - `tests/conditions.rs::a_delayed_commit_does_not_leave_the_tables_divergent_because_the_channel_carries_the_change`

---

### UC-CNC-015

**Caso:** Cada nó aplica só a versão **seguinte** à da sua tabela  
**Então:** uma versão igual ou menor é ignorada, uma adiante fica guardada (até 64, as mais próximas) até as anteriores chegarem, e uma mudança que não cabe na tabela (remover quem não é membro) é descartada. Chegar de filas de nós diferentes, fora de ordem, não muda o resultado.  
**Nível:** U  
**Testes:**
  - `src/cluster/members/tests.rs::a_change_ahead_of_the_next_version_waits_for_the_ones_before_it`
  - `src/cluster/members/tests.rs::a_change_that_does_not_fit_the_table_is_dropped_and_the_held_ones_are_bounded`
  - `tests/channel.rs::membership_changes_from_two_peers_arriving_out_of_order_apply_in_version_order`
  - `tests/channel.rs::a_membership_frame_that_removes_the_receiving_node_is_ignored`

---


## SIN: Sincronização distribuída de blocos

### UC-SIN-001

**Caso:** A sincronização é **escopada aos blocos afetados**  
**Então:** o restante do cluster continua operando, sem bloqueio global.  
**Nível:** C  
**Testes:**
  - `src/core/ring/tests.rs::the_ranges_of_the_blocks_cover_every_position_exactly_once`
  - `src/core/ring/tests.rs::a_node_syncs_only_the_blocks_it_is_responsible_for`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-002

**Caso:** O nó novo pede a **todos os nós responsáveis** pelo bloco a lista de IDs (`GET /cluster/blocks/{id}/ids`, NDJSON ordenado por ID).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `src/storage/messages/tests.rs::the_rows_of_a_block_come_by_position_in_id_order_with_range_and_limit`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-003

**Caso:** Uma mensagem já confirmada (ack) antes da listagem não aparece em nenhuma lista e **não é caso especial**  
**Então:** não há tombstone permanente.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-004

**Caso:** Consolida as listas pela **união** (não a interseção  
**Então:** uma réplica pode ter recebido uma mensagem que outra não), por *merge* ordenado dos streams, sem materializar todas as listas em memória.  
**Nível:** U, C  
**Testes:**
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`
  - `tests/sync.rs::the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection`

---

### UC-SIN-005

**Caso:** Divide a lista unida em faixas e **balanceia** as chamadas `GET /cluster/blocks/{id}/messages?from=&to=` entre os nós de origem (sem concentrar a carga num só); o que faltar numa faixa é buscado em outro nó; a rota de ID único é o último recurso; um stream cortado é retomado pelo último ID lido.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::block_messages_carry_the_body_and_stop_at_the_item_and_byte_ceilings`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection`

---

### UC-SIN-006

**Caso:** As linhas dos streams (tópicos e mensagens) são aplicadas **direto no armazenamento final** do nó, em lotes, sem gravar duas vezes (a carga não passa pelo banco de eventos).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-007

**Caso:** A sincronização aplica a mensagem com o arquivo e a linha no banco do tópico, e uma mensagem de tópico ainda desconhecido espera os tópicos (a carga de tópicos vem antes).  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`
  - `tests/sync.rs::a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition`

---

### UC-SIN-008

**Caso:** A mensagem já vencida por `ttl` é descartada na sincronização, em vez de copiada.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`

---

### UC-SIN-009

**Caso:** `GET /cluster/blocks/{id}/messages/{messageId}`  
**Então:** `200` `MessageRecord`, ou `404` `message.not-found`.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::block_messages_carry_the_body_and_stop_at_the_item_and_byte_ceilings`
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-010

**Caso:** Durante a sincronização o nó já consta na tabela e continua recebendo o protocolo normal, mas **não processa** os eventos  
**Então:** grava cada um no banco SQLite local `${storage.dir}/_sync/sync.db` (tabela `events`, número de sequência crescente) na ordem de chegada. Os eventos são registro, commit e rollback de tópicos, de tokens, de mensagens, de invisibilidade e de acks.  
**Nível:** C  
**Testes:**
  - `src/storage/synclog/tests.rs::events_are_kept_in_arrival_order_and_read_oldest_first`
  - `src/storage/synclog/tests.rs::the_database_lives_in_the_sync_folder_and_goes_away_with_discard`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-011

**Caso:** O nó **confirma o registro depois de gravar o evento** no banco (e por isso conta para o quórum normalmente). A gravação passa por uma thread única.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-012

**Caso:** Processamento em sequência  
**Então:** carga de tópicos e tokens, carga de blocos, processamento do banco de eventos, verificação final e troca para o modo normal.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition`

---

### UC-SIN-013

**Caso:** Processamento do banco de eventos **por cima** da carga  
**Então:** lê sempre a linha mais antiga, aplica o evento e **apaga a linha**, até esvaziar; não há posição guardada.  
**Nível:** U, C  
**Testes:**
  - `src/storage/synclog/tests.rs::events_are_kept_in_arrival_order_and_read_oldest_first`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-014

**Caso:** O processamento é idempotente e o evento vence a carga  
**Então:** criar o que existe pelo `id`, gravar o mesmo `invisibleUntil`, apagar o que não existe não muda nada; um ack ou remoção no banco apaga o que a carga trouxe velho.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-015

**Caso:** Novos eventos continuam chegando ao banco enquanto ele é processado.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-016

**Caso:** **Verificação final:** com a tabela de eventos quase vazia, o nó repete as listagens só para o que pretende manter e descarta o que já não aparece em nenhum nó de origem (cobre um ack cujo pedido nunca chegou ao nó).  
**Nível:** C  
**Testes:**
  - `src/storage/synclog/tests.rs::the_ids_of_the_load_are_kept_by_block`
  - `tests/sync.rs::the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore`

---

### UC-SIN-017

**Caso:** **Troca para o modo normal (*cutover*):** quando a tabela `events` fica vazia, sob o mesmo controle que a recepção usa para decidir o destino de cada evento (nenhum evento cai entre os dois modos), o nó passa a processar em tempo real e nada mais é gravado no banco; o `sync.db` é descartado e o nó fica íntegro.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-018

**Caso:** Um ack que chega durante a sincronização não "ressuscita" a mensagem  
**Então:** o evento é aplicado por cima da carga e a mensagem some.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-019

**Caso:** Pronto  
**Então:** cargas aplicadas, banco de eventos processado e descartado, verificação final feita. O nó reconstrói as listas de prontos e de invisíveis dos blocos de que é primário, recebe o instantâneo do canal, assume as filas voláteis e `/health/ready` responde `200`. Até lá não coordena nem entrega, e as chamadas de cliente são repassadas.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-020

**Caso:** **Falha da sincronização:** disco cheio ou com falha, banco de eventos acima de `cluster.sync.log.max-bytes`, ou banco local corrompido  
**Então:** o nó entra em falha e sinaliza ao cluster. Sem nó de origem para uma carga, tenta de novo e, esgotado `cluster.sync.timeout`, cai no mesmo caminho.  
**Nível:** C  
**Testes:**
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`
  - `tests/sync.rs::a_node_whose_join_commit_reaches_the_sources_late_waits_for_them_and_stays_in`

---

### UC-SIN-021

**Caso:** Limpeza dos nós que perderam o bloco  
**Então:** ver UC-ARM-022 e UC-ARM-023.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay`
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-SIN-022

**Caso:** Na FIFO, as publicações de um grupo cujo primário está sincronizando respondem `503` (UC-FIF-029); a sincronização leva as linhas com `deleteAt` sem rota nova.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-SIN-023

**Caso:** Métricas  
**Então:** `taba.sync.blocks.active`, `duration`, `ids.listed`, `messages.pulled` por `peer`, `bytes`, `log.events`, `log.bytes`, `phase` (`topics`, `blocks`, `replay`, `verification`, `ready`) e `failures` por `reason`. Logs de início e fim da sincronização de bloco (`info`).  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`

---

### UC-SIN-025

**Caso:** Uma mensagem listada que não pode ser obtida **não é declarada perdida de imediato** (pode ter sido confirmada e apagada enquanto o nó sincronizava)  
**Então:** tenta (1) a faixa em outro nó, (2) a rota de ID único, (3) relista os IDs só dos que falharam.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-026

**Caso:** Se o ID não aparece mais em nenhum nó, foi um ack  
**Então:** o caso é **descartado em silêncio** (sem log, sem registro).  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-027

**Caso:** Só o que continua listado e sem conteúdo em nenhum nó é **perda**  
**Então:** uma linha de log `error` com o nome do tópico, o ID, o bloco e os nós consultados, e um registro na tabela `losses` do nó e dos nós responsáveis pelo bloco (idempotente pela chave).  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-028

**Caso:** A perda **não compromete a saúde** do nó nem interrompe a sincronização, que segue para as demais mensagens. Não há métrica nem condição de saúde.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-029

**Caso:** A janela residual (um ack cujo pedido nunca chegou ao nó nem ao banco de eventos, por ter sido calculado antes de ele assumir a posição no anel) é tratada como o mesmo tipo de exceção rara  
**Então:** evento observável, não falha de saúde.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore`

---

### UC-SIN-030

**Caso:** O nó que as origens ainda não conhecem (elas respondem `cluster.not-member` até o commit do ingresso chegar) espera `write.retry.backoff` e lista o bloco de novo, até `cluster.sync.timeout`; se alguma origem atende, a sincronização segue com ela. Esgotado o prazo sem nenhuma origem, a sincronização falha com `no-source` e o nó vai a `failed` (D105).  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`
  - `tests/sync.rs::a_node_whose_join_commit_reaches_the_sources_late_waits_for_them_and_stays_in`

---

### UC-SIN-031

**Caso:** Um nó que entra copia, bloco a bloco, o log de excluídos de cada origem (`GET /cluster/blocks/{id}/excluded`) e passa a gravar **a partir de quando as origens gravam** (a mais recente das datas de início delas, lida no `NodeStatus`), não a partir do instante em que entrou  
**Então:** o `id` morto nas origens também está morto no log do nó novo, e a data de início dele é a das origens; se uma origem não manda o log ou a data (versão anterior, falha), o nó mantém a própria data, que é a conservadora. Só a primeira sincronização de um disco herda; depois a data só avança.  
**Nível:** C e U  
**Testes:**
  - `tests/excluded_log.rs::a_new_node_copies_the_log_of_its_sources_and_inherits_their_start`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-SIN-032

**Caso:** O cluster tem uma **data de corte do log de excluídos**: a mais recente das datas de início entre os membros ativos  
**Então:** a data aparece em `summary.excludedCutoff` do `GET /health/cluster`; antes dela, o log de algum membro pode não ter a exclusão, e a retomada não deve tratá-la como garantida.  
**Nível:** C  
**Testes:**
  - `tests/excluded_log.rs::the_cluster_cut_off_is_the_latest_start_among_the_active_members`

---


## STR: Streams NDJSON entre nós

### UC-STR-001

**Caso:** `Content-Type: application/x-ndjson`, um objeto JSON por linha; emissor e receptor trabalham com um buffer por linha, sem montar a resposta inteira em memória.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/tests.rs::a_stream_ends_with_the_end_line_and_without_it_the_chunk_is_a_failure`

---

### UC-STR-002

**Caso:** A ordem é por **ID ascendente** (GUID v7 comparado como texto na forma canônica em minúsculas).  
**Nível:** U  
**Testes:**
  - `src/cluster/replication/definitions/tests.rs::the_definition_streams_list_by_ascending_id_and_not_by_name`
  - `src/engine/messages/tests/blocks.rs::the_block_streams_come_in_ascending_id_order_whatever_the_order_of_arrival`

---

### UC-STR-003

**Caso:** `from` e `to` (ambos inclusivos e opcionais) delimitam a faixa; `limit` opcional reduz o teto de itens.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/dto_tests.rs::the_stream_query_carries_from_to_and_limit_and_all_three_are_optional`
  - `src/cluster/replication/definitions/tests.rs::from_is_inclusive_and_limit_lowers_the_ceiling_of_the_definition_streams`
  - `src/engine/messages/tests/blocks.rs::from_and_to_are_inclusive_and_limit_lowers_the_ceiling_of_the_block_streams`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-STR-004

**Caso:** O stream termina com a linha `StreamEnd` (`end: true`, `count`, `last`, `hasMore`); sem essa linha o trecho é considerado **falho**, mesmo que a conexão tenha fechado limpa.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/tests.rs::a_stream_ends_with_the_end_line_and_without_it_the_chunk_is_a_failure`

---

### UC-STR-006

**Caso:** Retomada depois de um corte ou de `hasMore`  
**Então:** o receptor repete com `from=<último ID lido>`; o emissor devolve de novo essa linha e o receptor a **ignora** (aplicação idempotente por ID). Sem token nem estado no servidor.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/dto_tests.rs::a_stretch_names_the_last_id_to_resume_from_and_the_resumption_asks_for_it_again`
  - `src/engine/messages/tests/blocks.rs::resuming_from_the_last_id_repeats_that_line_and_the_receiver_ignores_it`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-STR-009

**Caso:** Limite do tamanho da linha (`cluster.sync.stream.max-line-bytes`)  
**Então:** precisa caber a maior mensagem possível (hard-limit em base64 mais o envelope).  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::the_stream_line_limit_is_exactly_the_hard_limit_in_base64_plus_the_envelope`
**Observação:** Só a regra de configuração da partida está provada; o emissor ainda não impõe o tamanho da linha.

---

### UC-STR-011

**Caso:** Linhas de cada rota  
**Então:** `TopicDefinition`, `TokenRecord`, `{id, kind, key, version}` (lápides), `{id, topic}` (e `deleteAt` na FIFO) dos IDs do bloco, `{id, topic, position, atMs}` dos excluídos, `{id, topic}` das perdas e `MessageRecord` (sem `body` e com `deleteAt` na FIFO com exclusão lógica).  
**Nível:** U  
**Testes:**
  - `src/cluster/replication/definitions/tests.rs::the_topic_and_token_lines_are_the_record_plus_the_version_this_node_applied`
**Observação:** Só as linhas de definição e de token estão cobertas; as de lápide, IDs do bloco, excluídos e perdas ainda não têm teste unitário.

---

### UC-STR-012

**Caso:** O `MessageRecord` entre nós só tem duas formas de `body`  
**Então:** JSON nativo (`application/json`, `*/*+json`) ou base64 com `bodyEncoding: "base64"` (texto também vai em base64).  
**Nível:** U  
**Testes:**
  - `src/cluster/protocol/tests.rs::a_json_body_goes_native_and_the_rest_goes_in_base64`

---


## RTN: Retorno de nó excluído e recuperação de perdas

### UC-RTN-001

**Caso:** Um nó desregistrado por falha do canal que volta consulta **todos** os nós que consegue alcançar (`GET /cluster/message-loss`, NDJSON) e une as respostas (cada nó responde com a sua tabela `losses`). Essas rotas aceitam chamadas de um nó que ainda não é membro.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-002

**Caso:** Para cada par (tópico, ID), o nó procura a mensagem localmente  
**Então:** no disco (`${topic}/${id}.body` e o banco do tópico) e, se o processo é o mesmo que ficou fora (sem reinício), também em memória. Num reinício, só o disco conta, e o reinício **passa** por este procedimento: o processo novo põe de lado o disco da vida anterior e entra; depois de ativo, em segundo plano, consulta a lista de perdas dos membros que a listagem de entrada trouxe e devolve ao responsável o que esse disco tem delas (D111).  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has`

---

### UC-RTN-003

**Caso:** Para cada mensagem encontrada, o nó a envia ao responsável pelo bloco por `POST /cluster/messages/recover`; quem recebe grava **direto nos nós responsáveis**, idempotente pelo ID, sem as duas fases (nem registro pendente nem rollback), e confirma a linha quando `write.quorum` nós gravaram.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_recover_body_is_a_stream_with_a_mandatory_last_line`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`
  - `tests/recovery.rs::resending_a_recover_batch_ignores_what_was_already_applied`

---

### UC-RTN-004

**Caso:** Vai o corpo e o `deliveryCount`; a invisibilidade é **zerada** e a mensagem volta a ficar pronta para entrega.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`

---

### UC-RTN-005

**Caso:** Baixa do registro  
**Então:** o nó que aceita a mensagem apaga o par da sua tabela `losses` e avisa os demais (melhor esforço), para um segundo nó que volte com a mesma mensagem não a reenviar depois de ela já ter sido entregue e confirmada.  
**Nível:** C  
**Testes:**
  - `src/storage/losses/tests.rs::a_loss_is_removed_by_the_recovery_and_expires_by_age`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`

---

### UC-RTN-006

**Caso:** Quarentena  
**Então:** enquanto a recuperação não termina, o nó **não apaga** os dados antigos. Se algum nó não respondeu à consulta, tenta de novo a cada `cluster.bootstrap.retry.interval`, até `cluster.recovery.timeout`; só então segue.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query`
  - `tests/recovery.rs::when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has`

---

### UC-RTN-007

**Caso:** Concluída a recuperação (ou esgotado o prazo), o nó limpa seu estado local e inicia um **novo registro do zero**, o bootstrap completo, como nó novo e com nova posição no anel. O freezing só atrasa este passo.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has`

---

### UC-RTN-008

**Caso:** Tabela `losses`  
**Então:** chave (`topic`, `id`), campos `block` e `detected_at`; o nó que detecta grava e envia também aos nós responsáveis pelo bloco (a informação sobrevive à queda do detector); os registros expiram depois de `cluster.message-loss.ttl`.  
**Nível:** U, C  
**Testes:**
  - `src/storage/losses/tests.rs::a_loss_is_keyed_by_topic_and_id_and_listed_by_id`
  - `src/storage/losses/tests.rs::a_loss_is_removed_by_the_recovery_and_expires_by_age`
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-009

**Caso:** Mensagens já aplicadas num reenvio do `recover` são ignoradas (`taba.recover.messages` com `outcome` `sent`, `applied`, `skipped`).  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::the_recover_body_is_a_stream_with_a_mandatory_last_line`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::resending_a_recover_batch_ignores_what_was_already_applied`

---

### UC-RTN-010

**Caso:** O estado do nó durante a volta é `returning` (`taba.node.state`, `/health/ready` com `reason: returning`); registro em log das mensagens recuperadas (`info`).  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query`

---

### UC-RTN-012

**Caso:** `GET /cluster/message-loss` e `POST /cluster/messages/recover` aceitam quem ainda não é membro; `POST /cluster/message-loss` (registro) e `DELETE /cluster/message-loss/{topic}/{id}` (baixa) só aceitam membros (`403 cluster.not-member`). Registro e baixa são idempotentes (`changed`), e o registro expira depois de `cluster.message-loss.ttl`.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-013

**Caso:** O nó que saiu da tabela (drain) não faz a limpeza dos blocos que perdeu  
**Então:** as réplicas que ele guarda ficam para a recuperação procurar quando voltar.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`

---

### UC-RTN-014

**Caso:** O reinício do processo **não espera** a recuperação para entrar  
**Então:** o nó põe de lado o disco da vida anterior (`${storage.dir}/_lives/<ID antigo>`), entra com o disco vazio e fica `active` assim que a sincronização acaba, mesmo que um membro da listagem não responda (PRB-048).  
**Nível:** C  
**Testes:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`

---

### UC-RTN-015

**Caso:** Depois de ativo, o nó que reiniciou recupera em segundo plano  
**Então:** consulta a lista de perdas, devolve ao responsável o que o disco da vida anterior tem delas e **mantém** a pasta da vida (ela só é apagada no fim da restauração, [07-cluster.md](07-cluster.md), seção 10.8; a pasta `_previous-life`, de disco sem manifesto, é apagada). O membro que estava mudo é chamado de novo a cada `cluster.bootstrap.retry.interval`, até `cluster.recovery.timeout` (5 min, D110); esgotado, o nó segue com as listas que tem.  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute`

---

### UC-RTN-026

**Caso:** Uma vida **dentro da janela** do log (o `last_seen` dela é posterior à data de corte do cluster mais a folga) tem **todas** as mensagens enviadas ao **novo primário** do bloco, com o ID original, sem que nenhum nó tenha declarado perda; o que o primário já tem (`Existing`) é pulado
**Então:** o primário ativa pelo caminho de `recover` (grava e replica com o quórum); o `deliveryCount` fica, a invisibilidade e o recibo são zerados; o que o primário confirma sai da pasta da vida, e uma nova passada não reenvia nada. O log de excluídos da vida entra no do nó, e a data de início do log recua quando os intervalos se encontram sem buraco. Uma vida fora da janela segue o caminho da lista de perdas (UC-RTN-018).
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::a_life_inside_the_window_is_sent_whole_to_the_new_primary_with_its_original_id`
  - `src/engine/messages/tests/lives.rs::the_log_of_a_life_is_added_to_the_nodes_and_the_start_goes_back_only_if_they_meet`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_restore_and_dead_routes_refuse_a_wrong_media_type_and_an_unknown_parameter`

---

### UC-RTN-027

**Caso:** Mensagem de um tópico **excluído**, ou **recriado com outro `topicId`** (o da vida fica registrado junto do tópico, na pasta)
**Então:** o primário a descarta sem consultar mortos; o que foi descartado conta como resolvido, e sai da pasta da vida. Uma pasta de antes do registro não prova a recriação, e o tópico conta como o mesmo.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::messages_of_a_deleted_or_recreated_topic_are_discarded_without_asking_the_dead`
  - `src/engine/messages/tests/lives.rs::the_topic_of_a_message_of_a_life_is_the_same_gone_or_recreated`
  - `src/storage/messages/tests.rs::the_folder_of_a_topic_records_the_topic_id_it_was_created_for_and_keeps_it`

---

### UC-RTN-028

**Caso:** O `id` consta no **log de excluídos** do primário, ou no de **qualquer outro nó presente** (a consulta vai em lote, por tópico, a todos os presentes, não só aos donos do bloco)
**Então:** a mensagem é descartada; o log dos outros responde por `POST /cluster/topics/{topic}/messages/dead`.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::the_new_primary_discards_what_its_own_log_or_the_log_of_any_present_node_says_is_dead`
  - `src/engine/messages/tests/lives.rs::the_node_tells_which_ids_its_log_of_excluded_messages_has`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_restore_and_dead_routes_refuse_a_wrong_media_type_and_an_unknown_parameter`

---

### UC-RTN-029

**Caso:** Com o backup em S3, o **diário** do tópico registra a remoção do `id`; ou o S3 está inacessível
**Então:** a remoção no diário descarta a mensagem (o diário é lido uma vez por tópico, com cache curto); S3 inacessível é evidência ausente: o primário decide com o log local e o dos outros nós e registra um aviso.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::the_journal_in_s3_discards_what_it_removed_and_a_silent_s3_only_leaves_a_warning`

---

### UC-RTN-030

**Caso:** **Recebimento durável**: o primário grava a mensagem restaurada **em disco**, em todos os donos, e só confirma com o quórum
**Então:** só o que o primário confirmou é marcado como enviado (sai da pasta da vida); o que ele não confirmou fica na pasta e é reenviado até o prazo de `cluster.recovery.timeout`; depois de confirmado, nada é reenviado.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::a_restored_message_is_on_the_disk_of_every_owner_even_with_memory_to_spare`
  - `tests/restore_lives.rs::a_batch_the_primary_did_not_confirm_stays_in_the_life_and_is_sent_again`

---

### UC-RTN-031

**Caso:** **Votação** do fim da restauração: quando **todos os nós presentes** avisaram que terminaram de enviar suas vidas, o coordenador (o nó de menor ID) fecha a votação
**Então:** o status do cluster vira **restaurado do disco**; só então as filas FIFO são liberadas e as pastas das vidas apagadas (a pasta cujas mensagens os primários não confirmaram por inteiro fica para a próxima partida).
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `src/cluster/restore/tests.rs::the_coordinator_is_the_lowest_id_and_the_vote_needs_every_present_node`
  - `src/cluster/restore/tests.rs::the_status_goes_from_unknown_to_restoring_to_restored_and_remembers_force_and_missing`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-RTN-032

**Caso:** A restauração das mensagens antigas **espera os nós esperados** (os membros da maior tabela antiga) e, passado `G`, segue com um aviso e os nós presentes; no **modo estrito** fica retida até o operador forçar
**Então:** enquanto a votação não fecha a FIFO fica sem entrega e o cluster mostra `previous-life-pending`; os nós que a restauração ainda espera constam no estado do nó.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`

---

### UC-RTN-033

**Caso:** Nó que volta **depois da liberação**
**Então:** ele entra limpo: o status já é **restaurado do disco**, então suas vidas são restauradas por conta própria, sem votação e sem retenção, e as mensagens FIFO são ativadas em ordem de `id` (a ordem delas em relação às mensagens mais novas do grupo não é garantida); as pastas são apagadas quando os primários confirmaram tudo.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::a_node_that_comes_back_after_the_release_restores_its_lives_on_its_own_and_holds_nothing`

---

### UC-RTN-034

**Caso:** As **filas comuns** durante a restauração
**Então:** recebem e entregam assim que as definições estão distribuídas; as mensagens antigas entram depois, quando a restauração começa.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`

---

### UC-RTN-035

**Caso:** O operador **força** a restauração (`POST /cluster/restore/force`, espalhada para todos os nós)
**Então:** o status muda para restaurado do disco em todo nó presente, a FIFO retida é liberada e uma restauração retida pelo modo estrito segue com os nós presentes; serve também ao cluster travado pela perda de dois nós.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-RTN-036

**Caso:** Todos os nós de um cluster que caiu **voltam juntos**, com os discos, e **nenhum responde** ao endereço de entrada (nenhum é o `cluster.first-node`)
**Então:** cada nó tenta os nós da tabela da vida anterior (pelos IPs do manifesto); passado `cluster.restore.cold-start.wait`, o de **menor ID antigo** entre os que estão vivos parte sozinho e **assume o disco**, e os outros entram nele. As mensagens das vidas são restauradas, o status vira restaurado do disco em todos e as pastas das vidas são apagadas.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-RTN-037

**Caso:** O endereço de entrada não responde, mas há nós de pé
**Então:** o nó que volta acha o cluster pela **tabela de IPs da vida anterior** (do manifesto): o que responde com uma tabela é o cluster em que ele entra.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_restarted_node_finds_the_cluster_through_the_old_table_when_the_entry_address_is_dead`
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-RTN-038

**Caso:** Um nó **sem vida anterior** e sem `cluster.first-node`
**Então:** nunca parte sozinho: a listagem sem resposta continua sendo erro e ele tenta de novo. Só `cluster.first-node` cria um cluster que nunca existiu.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_node_with_no_previous_life_never_starts_alone_without_first_node`

---

### UC-RTN-039

**Caso:** Um nó que **entra** num cluster de pé tem no disco uma definição (tópico, token ou lápide) de **versão maior** que a do cluster, ou que o cluster não conhece
**Então:** antes de apagar o disco o nó guarda as definições e, ao ficar ativo, **oferece** cada uma ao cluster: a maior versão vence (lápide incluída) e o que vence é espalhado aos demais. Um disco mais velho que o cluster não impõe nada.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins`

---

### UC-RTN-040

**Caso:** Dois grupos de nós subiram sem se falar (cada um assumiu o disco) e se encontram de novo
**Então:** o grupo **menor** (empate: o do menor `nodeId`, D101) perde, **sem apagar nada**: o disco dele vira a pasta de uma vida (`_lives/<ID>`), as definições são oferecidas ao grupo vencedor e o nó refaz o join **com ID novo**. O que o grupo perdedor aceitou enquanto esteve separado é restaurado e conferido contra os mortos, e a fila (definição) continua no cluster.
**Nível:** C
**Testes:**
  - `tests/regroup.rs::the_smaller_group_that_meets_the_other_one_moves_its_disk_to_a_life_and_joins_with_a_new_id`

---

### UC-RTN-041

**Caso:** O cluster tem menos membros ativos que a tabela da vida anterior
**Então:** cada nó sonda, a cada `cluster.restore.probe.interval`, os IPs antigos que não estão na tabela; a sondagem **termina** quando a tabela completa, e nada fica visível.
**Nível:** C
**Testes:**
  - `tests/regroup.rs::the_probe_ends_when_the_table_completes_and_raises_no_condition`

---

### UC-RTN-042

**Caso:** O tempo da sondagem (`cluster.restore.probe.timeout`) acaba com nós da tabela antiga faltando
**Então:** o log avisa e `GET /health/cluster` mostra `members-missing` (aviso) enquanto a tabela estiver incompleta; o nó que falta entra depois pelo join normal.
**Nível:** C
**Testes:**
  - `tests/regroup.rs::when_the_probe_time_ends_with_nodes_missing_the_members_missing_condition_shows`

---


### UC-RTN-043

**Caso:** Um nó com `replication.factor=1`, sem `cluster.tls.enabled` e sem `cluster.entry-address` (o perfil de um nó)
**Então:** parte sozinho como um cluster de um nó, fica `active`, atende a fila comum e a FIFO (o quórum da FIFO é `min(2, fator)`), e `GET /health/cluster` mostra a condição `reduced-profile`; com `health.single-node-condition=false` ela não aparece.
**Nível:** C
**Testes:**
  - `tests/single_node.rs::a_node_with_factor_one_starts_alone_and_serves_the_plain_and_the_fifo_queue`
  - `tests/single_node.rs::the_reduced_profile_condition_can_be_hidden`
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-RTN-044

**Caso:** `auth.dev-token` com um segredo de pelo menos 32 caracteres, no perfil de um nó
**Então:** a partida cria o token de trabalho `dev` com `admin`, `publish` e `consume` (só o hash é guardado) e liga a autenticação: sem token `401`, com `tb_dev.<segredo>` as rotas respondem, e o segredo não aparece em `GET /tokens`.
**Nível:** C
**Testes:**
  - `tests/single_node.rs::the_dev_token_is_an_admin_work_token_created_at_the_start_and_turns_authentication_on`

---

### UC-RTN-045

**Caso:** Uma mensagem que o nó já tem (`Existing`) chega também na restauração de uma vida, com um `deliveryCount` maior
**Então:** o nó fica com o **maior** dos dois contadores (nunca diminui) e não cria a mensagem de novo; a invisibilidade e o recibo não mudam. Cada dono faz isso com a sua cópia (PRB-062).
**Nível:** U, C
**Testes:**
  - `src/engine/messages/tests/blocks.rs::of_two_copies_the_greater_delivery_count_prevails_and_never_decreases`
  - `tests/restore_lives.rs::a_message_the_primary_already_has_takes_the_greater_delivery_count_of_the_two_copies`

---

### UC-RTN-046

**Caso:** O cluster inteiro cai, e uma mensagem estava **só em memória** em todos os nós
**Então:** ela se perde: nenhum disco a tinha e nenhum nó a devolve depois da retomada; o que grava a memória é o drain (UC-DRN-010).
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::what_was_only_in_memory_on_every_node_is_lost_when_the_whole_cluster_falls`

---

### UC-RTN-047

**Caso:** Uma queda faz o log de excluídos perder as últimas entradas (e o apagamento da mensagem que elas acompanhavam)
**Então:** a mensagem que o consumidor já tinha confirmado volta como pronta e é entregue de novo; é aceito, porque o consumidor é idempotente (*at-least-once*, [07-cluster.md](07-cluster.md), seção 10.10).
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_fall_that_loses_the_last_entries_of_the_log_delivers_the_acknowledged_message_again`

---

### UC-RTN-048

**Caso:** As pastas das vidas anteriores crescem (várias vidas, com muitas mensagens grandes)
**Então:** elas não têm um teto próprio: tudo é restaurado e nada é recusado nem descartado pelo tamanho; o disco cheio segue a regra de sempre (erro fatal, o nó vai a `failed`, UC-FAL-028).
**Nível:** I
**Testes:**
  - `tests/messages.rs::the_folders_of_the_previous_lives_have_no_cap_of_their_own`
  - `src/storage/db/tests.rs::a_full_disk_in_the_node_database_fails_the_node_from_any_of_its_tables`

---
### UC-RTN-049

**Caso:** Uma vida cujo `last_seen` é anterior à data de corte do cluster (com a folga) chega a um nó que a guarda
**Então:** ela **não** é restaurada: nada dela é enviado, nenhum nó recebe a mensagem, e ela segue o caminho de hoje (só voltam as perdas listadas); ao fim da passada, que está completa porque não havia o que enviar, a pasta da vida é apagada.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::a_life_outside_the_window_is_not_restored_and_its_folder_goes_with_the_end_of_the_pass`

---

### UC-RTN-050

**Caso:** O novo primário já tem a mensagem que a vida envia (`Existing`)
**Então:** ela é **pulada**: não é ativada de novo, então a invisibilidade e o recibo do consumidor que a segura ficam como estavam e o `deliveryCount` não diminui; a linha sai da pasta da vida como enviada.
**Nível:** C
**Testes:**
  - `tests/restore_lives.rs::a_message_the_primary_already_has_is_skipped_and_keeps_its_invisibility_and_receipt`

---

### UC-RTN-051

**Caso:** Um nó que volta depois da liberação traz mensagens FIFO de um mesmo grupo na pasta da vida dele, fora de ordem
**Então:** elas são ativadas em ordem de `id`: o consumo (um por grupo de cada vez) as entrega em ordem; a ordem delas em relação às mensagens mais novas do grupo não é garantida.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::the_fifo_messages_of_a_node_that_comes_back_are_activated_in_id_order`

---

### UC-RTN-052

**Caso:** O cluster está **travado por perda de dois nós** (a tabela antiga tinha cinco, o modo estrito está ligado e `G` passou)
**Então:** a restauração e a FIFO seguem retidas até o operador **forçar** (`POST /cluster/restore/force`, em qualquer nó): o status muda em todo nó presente, o que estava retido segue, e forçar de novo não muda nada (`wasRestoring=false`).
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::a_cluster_stuck_by_the_loss_of_two_nodes_is_released_by_force_and_forcing_again_changes_nothing`

---

### UC-RTN-053

**Caso:** **Propriedade no cluster simulado:** três rodadas em que o cluster publica, confirma (ack) parte das mensagens e cai inteiro, e todos os nós voltam
**Então:** nenhuma mensagem confirmada antes da queda é entregue de novo depois do retorno, e o cluster entrega o que sobrou (a propriedade não é vazia). É a contrapartida em cluster do modelo do UC-RTN-021.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls`

---
### UC-RTN-054

**Caso:** Uma mensagem já consumida cujo registro de exclusão estava só num nó que não voltou, e a mesma mensagem com todos os nós esperados presentes
**Então:** com o nó ausente, nada é entregue de novo antes de `G`; passado `G`, a restauração segue com os nós presentes e a mensagem volta (a reentrega residual da seção 10.10 do 07-cluster.md, que o consumidor idempotente aceita). Com todos os nós esperados presentes, a restauração não espera `G` e a mensagem que o log de um deles marca como consumida não volta.
**Nível:** C
**Testes:**
  - `tests/restore_vote.rs::a_message_already_consumed_whose_log_is_on_a_node_that_never_came_back_returns_only_after_g`
  - `tests/restore_vote.rs::with_every_expected_node_present_a_message_already_consumed_does_not_wait_for_g_and_does_not_return`

---
### UC-RTN-055

**Caso:** Vários nós se conectam na partida a frio, e o coordenador (o de menor ID antigo) tem o disco mais velho
**Então:** a maior versão de cada definição vence em todos os nós, a definição que só um nó tinha chega ao cluster e as que todos tinham continuam lá.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator`

---
### UC-RTN-056

**Caso:** Um nó atrasado sobe sozinho, porque nenhum outro responde, e depois chega outro nó com uma versão maior de uma definição
**Então:** o nó que subiu sozinho serve as definições que tem; quando o outro nó responde, a janela termina e a versão maior vence nos dois.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_late_node_that_starts_alone_serves_what_it_has_and_the_higher_version_wins_when_another_answers`

---
### UC-RTN-057

**Caso:** A sondagem dos IPs antigos termina com um nó faltando, e o nó que faltava chega depois
**Então:** a condição `members-missing` aparece no `/health/cluster`; o nó que faltava entra pelo join normal, a tabela completa e a condição some.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::the_node_that_was_missing_when_the_probe_ended_joins_later_by_the_normal_join_and_the_condition_goes_away`

---
### UC-RTN-058

**Caso:** Uma definição oferecida por um nó que entrou chega a **todo membro da tabela**, não só aos peers que o canal conhece naquele instante
**Então:** além do quadro do canal, o nó envia um `definition-apply` a cada membro (uma mudança decidida por um dono faz o mesmo), então um nó que se registrou milissegundos depois e ainda sincroniza a grava com os eventos e a aplica; o lock da chave é solto antes do envio (doc 36, "Uma definição oferecida a um nó que ainda estava entrando").
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::an_offered_definition_goes_to_every_member_of_the_table_and_not_only_to_the_peers_the_channel_knows`

---
### UC-RTN-016

**Caso:** O que o disco da vida anterior tem e a lista de perdas não pede não volta a ninguém, e o disco novo nunca ressuscita uma mensagem antiga  
**Então:** depois da recuperação só ficam no nó as cópias que a sincronização trouxe.  
**Nível:** C  
**Testes:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`
  - `tests/recovery.rs::a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`

---

### UC-RTN-017

**Caso:** O **primeiro nó** de um cluster novo (`cluster.first-node=true`, listagem sem ninguém) não tem de quem recuperar  
**Então:** um disco **sem manifesto** (versão anterior) é apagado na partida, pelo caminho de sempre, e a pasta `_previous-life` não fica (D111); um disco **com manifesto** vira a pasta da vida (UC-RTN-023) e é restaurado pela partida a frio (UC-RTN-036; [07-cluster.md](07-cluster.md), seção 10.5).  
**Nível:** C  
**Testes:**
  - `tests/recovery.rs::the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life`

---

### UC-RTN-018

**Caso:** Uma **vida** (disco de uma execução anterior) só é restaurada se o `last_seen` dela é **posterior à data de corte do cluster mais a folga de relógio** (`cluster.restore.clock-slack`); sem data de corte conhecida, nenhuma vida é restaurável  
**Então:** a vida fora da janela segue o caminho das perdas listadas (identidade nova, disco descartado); a folga pende para o lado seguro (uma vida perto demais da data não é restaurada).  
**Nível:** U  
**Testes:**
  - `src/cluster/restore/tests.rs::a_life_is_restorable_only_if_seen_after_the_cutoff_plus_the_clock_slack`

---

### UC-RTN-019

**Caso:** O **manifesto** da vida (`manifest.json` na pasta dela) guarda o ID do nó, os membros ativos da tabela com os IPs e a porta, a versão da tabela, o `last_seen` e a marca d'água do maior `id`; é gravado por substituição atômica  
**Então:** ele é lido de volta igual, uma gravação nova (o `last_seen` periódico) substitui a anterior sem deixar arquivo temporário, e uma pasta sem manifesto legível (vida de versão anterior) não é uma vida restaurável.  
**Nível:** U  
**Testes:**
  - `src/cluster/restore/tests.rs::the_manifest_is_written_atomically_and_read_back`
  - `src/cluster/restore/tests.rs::a_folder_without_a_readable_manifest_is_not_a_life`

---

### UC-RTN-020

**Caso:** A restauração das mensagens antigas espera os **nós esperados** (os membros ativos da **maior** tabela antiga, pelos manifestos anunciados): com todos presentes ela começa na hora; faltando algum, espera o prazo `G` (`cluster.restore.grace`, 5 min)  
**Então:** passado `G` ela segue com um aviso e os nós presentes; com `cluster.restore.strict=true` ela fica retida até a operação de forçar, e forçar a libera mesmo antes de `G`.  
**Nível:** U  
**Testes:**
  - `src/cluster/restore/tests.rs::the_expected_nodes_are_the_members_of_the_biggest_old_table`
  - `src/cluster/restore/tests.rs::the_restoration_waits_for_the_expected_nodes_until_the_grace_period`
  - `src/cluster/restore/tests.rs::in_strict_mode_the_restoration_stays_held_after_the_grace_period_until_forced`

---

### UC-RTN-021

**Caso:** **Propriedade** (modelo de um cluster que para inteiro): com **todos os nós esperados presentes** e a regra da janela, nenhuma mensagem já confirmada (ack) é entregue de novo, sejam quais forem as paradas dos nós e os cortes dos logs  
**Então:** o teste varre 2000 histórias aleatórias e falha na primeira reentrega; o mesmo modelo **sem** a regra da janela produz reentregas (o teste prova que a propriedade tem dentes).  
**Nível:** U (modelo; o teste de cluster simulado é o UC-RTN-053)  
**Testes:**
  - `src/cluster/restore/tests.rs::with_every_expected_node_present_no_consumed_message_comes_back`
  - `src/cluster/restore/tests.rs::the_property_has_teeth_without_the_rule_of_the_window_a_consumed_message_comes_back`

---

### UC-RTN-022

**Caso:** **Propriedade** (o mesmo modelo, com nós ausentes): uma mensagem já confirmada só volta se **nenhum nó presente** registrou o ack dela; e, enquanto falta um nó esperado e `G` não passou, a restauração não começa (nem no modo estrito)  
**Então:** a reentrega residual da seção 10.10 do 07-cluster.md só acontece quando quem registrou a exclusão está ausente, e só depois de `G` (ou de forçar).  
**Nível:** U (modelo; o teste de cluster simulado vem com a restauração)  
**Testes:**
  - `src/cluster/restore/tests.rs::with_a_node_missing_only_what_every_logger_of_it_missed_can_come_back`
  - `src/cluster/restore/tests.rs::nothing_starts_before_the_grace_period_while_an_expected_node_is_missing`

---

### UC-RTN-023

**Caso:** O disco da vida anterior vira **sempre** uma pasta com o **ID antigo** no nome (`${storage.dir}/_lives/<ID antigo>`, com os tópicos e o manifesto), tenha o nó achado o cluster ou não; várias vidas convivem (um segundo reinício não apaga a primeira)
**Então:** o primeiro nó de um cluster novo também põe o disco com manifesto de lado, em vez de apagá-lo; uma vida sem mensagens guarda o manifesto, porque conta para os nós esperados; o ID da vida tem de ser um nome de pasta.
**Nível:** C
**Testes:**
  - `src/storage/messages/tests.rs::a_disk_with_a_manifest_becomes_the_folder_of_its_life_and_the_lives_coexist`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_first_node_does_not_wipe_a_disk_that_has_a_manifest_it_keeps_it_as_a_life`

---

### UC-RTN-024

**Caso:** A vida em execução mantém o **manifesto** na raiz do disco: o ID, os **membros ativos** da tabela com os IPs, a versão da tabela, o `last_seen` e a marca d'água
**Então:** é gravado quando o nó fica ativo e a cada mudança da tabela, o `last_seen` anda a cada 10 s e na parada limpa, e a escrita é atômica (arquivo temporário e troca). A marca d'água é o maior instante que o gerador de IDs usou ou foi mandado respeitar ([07-cluster.md](07-cluster.md), seção 10.9); vai vazia até o nó gerar um ID.
**Nível:** C
**Testes:**
  - `tests/recovery.rs::the_running_life_keeps_its_manifest_and_moves_last_seen_on_a_clean_stop`
  - `src/cluster/restore/tests.rs::the_manifest_is_written_atomically_and_read_back`
  - `src/cluster/restore/tests.rs::a_folder_without_a_readable_manifest_is_not_a_life`

---

### UC-RTN-025

**Caso:** A **quarentena não é apagada** pela recuperação de hoje: a pasta de cada vida fica até o fim da restauração (voto, [07-cluster.md](07-cluster.md), seção 10.8)
**Então:** a recuperação das perdas devolve o que a lista pede, e a pasta da vida continua com os tópicos e o manifesto, inclusive quando o prazo de `cluster.recovery.timeout` esgota. Só as linhas que um primário confirmou na restauração (UC-RTN-026) saem dela.
**Nível:** C
**Testes:**
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute`

---


## FWD: Repasse de chamadas de cliente

### UC-FWD-001

**Caso:** Um nó que ainda não está íntegro (sincronizando) **repassa** a chamada de cliente a outro nó em vez de devolver erro. Vale só para rotas de cliente; `/health/*`, `/metrics`, `/metrics/topics/*`, `/node/drain` e as de comunicação entre nós não são repassadas.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-003

**Caso:** Destino no drain  
**Então:** o **nó mais livre** (menor carga pelo sinal de vida, mais as chamadas já repassadas e em andamento por aquele destino), com empate sorteado; só entram nós prontos e fora de drain; a escolha é feita **a cada chamada**. Depois que o canal fecha, vale o último indicador e a última tabela conhecida.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/replication/forward/tests.rs::the_freest_destination_is_the_one_with_the_lowest_load`
  - `src/cluster/replication/forward/tests.rs::a_tie_leaves_every_tied_destination_to_be_drawn_and_no_candidate_leaves_none`
  - `tests/drain_cluster.rs::a_draining_node_forwards_each_call_to_the_node_with_the_lowest_load_of_the_last_heartbeat`

---

### UC-FWD-004

**Caso:** Repasse **transparente**  
**Então:** método, caminho, query, corpo e cabeçalhos seguem como vieram, e a resposta volta como o destino a deu (inclusive `409`, `413`, `429`); vale para todas as rotas de cliente, também o `GET` com long polling e `autoCreate`.  
**Nível:** C  
**Testes:**
  - `src/cluster/protocol/tests.rs::a_forwarded_reply_comes_back_as_the_destination_gave_it`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-009

**Caso:** Rate limit  
**Então:** aplicado uma vez, no nó que recebeu, por IP do cliente (o nó não valida o token); o destino não conta de novo, e a porta do cluster não tem rate limit. O IP do cliente já resolvido segue em cabeçalho interno.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-011

**Caso:** Publicação em fila FIFO  
**Então:** mesmo com o nó pronto, é repassada ao **primário do grupo** (o destino é o dono, não o nó mais livre), por `POST /cluster/topics/{topic}/fifo/publish`; o primário confere (`421` se não for o dono) e a repetição depois do corpo é segura.  
**Nível:** C  
**Testes:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---


## DRN: Parada e modo drain (inclui o drain como save)

### UC-DRN-001

**Caso:** Os estados são `active` → `draining` → `drained` (`drainState`).  
**Nível:** U  
**Testes:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-002

**Caso:** `SIGTERM` ou `SIGINT` com `drain.on-stop` ligado (padrão)  
**Então:** o nó entra em drain sozinho e, terminado o drain, **encerra com código 0**. Um segundo sinal durante o drain encerra na hora, sem terminar.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::the_signal_drain_waits_for_the_save_and_marks_the_trigger`
  - `src/cluster/drain/tests.rs::the_signal_waits_for_a_drain_the_api_already_started`

---

### UC-DRN-003

**Caso:** Com `drain.on-stop=false` o sinal encerra o processo na hora, como uma queda.  
**Nível:** C  
**Testes:**
  - `tests/messages.rs::without_drain_on_stop_what_was_only_in_memory_is_lost`

---

### UC-DRN-004

**Caso:** `POST /node/drain`  
**Então:** `202` `DrainStatus` com `trigger: "api"`, **sem encerrar** o processo; o nó fica em `drained`, fora do cluster e da rotação. Já em drain (`draining` ou `drained`): `200` sem efeito.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `src/cluster/drain/tests.rs::starting_a_drain_twice_changes_nothing`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-005

**Caso:** `GET /node/drain` devolve `DrainStatus` (`drainState`, `trigger` `signal` ou `api` ou `null`, `startedAt`, `pendingMessages`, que chega a 0 quando `drained`).  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-006

**Caso:** As rotas de drain valem só para o nó que recebe a chamada; um IP fora de `operations.allowed-ips`  
**Então:** `403` `ops.ip-not-allowed`.  
**Nível:** I  
**Testes:**
  - `tests/operations.rs::the_operations_routes_only_accept_the_allowed_ips`

---

### UC-DRN-007

**Caso:** Passo 1  
**Então:** `/health/ready` passa a `503` na hora e `/health/live` segue `200`. O nó continua recebendo chamadas de cliente mas **não atende nenhuma**: repassa todas ao nó mais livre até o processo encerrar. Os long pollings em andamento são respondidos vazios; os novos, repassados.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::a_long_poll_waiting_when_the_drain_starts_answers_empty`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-008

**Caso:** Passo 2  
**Então:** avisa os outros nós pelo canal com o sinal de estado "em drain" (idempotente): os pares deixam de pedir `take` a ele e de escolhê-lo como origem de carga; um `take` que ainda chegue é respondido vazio.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-DRN-009

**Caso:** Passo 3  
**Então:** o limite de memória efetivo cai a **zero**: toda mensagem nova, publicada ou recebida como réplica, vai direto para disco (arquivo e depois a linha). Os pares **continuam** mandando registros e commits ao nó até a saída.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::while_it_drains_the_node_writes_every_new_replica_straight_to_disk`

---

### UC-DRN-010

**Caso:** Passo 4  
**Então:** uma thread move para o disco as mensagens em memória **com os metadados** (`deliveryCount`, `invisibleUntil`, `receiptToken`), pelas threads de escrita de cada tópico, em paralelo entre tópicos, na ordem do overflow (primeiro os tópicos de menor taxa de leitura). As listas de prontos e de invisíveis são derivadas e refeitas depois.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `src/engine/messages/tests.rs::drain_saves_everything_in_memory_to_disk`
  - `tests/drain_cluster.rs::while_it_drains_the_node_writes_every_new_replica_straight_to_disk`
  - `tests/messages.rs::stopping_saves_memory_to_disk_and_the_next_start_resumes_it`

---

### UC-DRN-011

**Caso:** Passo 5  
**Então:** com a memória vazia, o nó coordena o **desregistro de si mesmo** pelo protocolo de duas fases (`POST /cluster/nodes/unregister` com `reason: drain` e `commit`), com a mesma regra de falha, **sem votação nem quórum de votação**. O commit tira o nó da tabela, recalcula o anel e rebalanceia os blocos; os novos primários assumem as filas voláteis dele.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-DRN-012

**Caso:** Passo 6  
**Então:** fecha o canal com os pares (já com o nó fora da tabela: ninguém reconecta nem abre votação) e o estado vira `drained`. Por sinal, o processo espera as chamadas repassadas em andamento (dentro de `drain.timeout`) e encerra; pela API, segue repassando as chamadas que receber.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::the_signal_drain_waits_for_the_save_and_marks_the_trigger`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-DRN-013

**Caso:** O desregistro voluntário segue a regra de uma mudança por vez  
**Então:** se há outra entrada, saída ou votação em curso, o pedido espera a vez, dentro de `drain.timeout`. Não há freezing.  
**Nível:** C  
**Testes:**
  - `tests/drain_cluster.rs::two_nodes_draining_at_once_leave_one_after_the_other`

---

### UC-DRN-014

**Caso:** Prazo estourado ou falha  
**Então:** o `drain.timeout` cobre o drain inteiro (passos 1 a 6); estourado, o nó registra o que sobrou em memória e a etapa em que parou (`error`) e encerra mesmo assim (se foi por sinal), caindo na detecção e votação de sempre. Erro de I/O ou disco cheio no passo 4 não interrompe: o nó segue, conta o erro e o inclui no resultado.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::an_expired_timeout_leaves_the_node_draining`

---

### UC-DRN-016

**Caso:** Antes da saída (`draining` até o início do passo 5)  
**Então:** cancela o drain, o nó volta a `active`, a gravar em memória, continua membro; `200` `DrainStatus`.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::canceling_while_draining_stops_the_move`
  - `tests/drain_cluster.rs::canceling_before_the_node_left_keeps_it_in_the_cluster`
  - `tests/operations.rs::deleting_the_drain_brings_the_node_back_with_its_messages`

---

### UC-DRN-017

**Caso:** Durante o desregistro (passo 5 em andamento)  
**Então:** `409` `drain.conflict`; tente de novo quando estiver `drained`. Com o processo já parando por sinal: `409` `drain.conflict`.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::with_the_process_stopping_by_signal_cancel_is_a_conflict`
  - `tests/drain_cluster.rs::canceling_a_drain_that_is_leaving_is_a_conflict`

---

### UC-DRN-018

**Caso:** Depois da saída (`drained`)  
**Então:** o nó entra como **nó novo**, com **ID novo**, bootstrap completo e nova posição no anel; responde `202`, `drainState` volta a `active`, e o estado do nó passa por `joining` e `syncing`. Antes de limpar o estado local faz a recuperação de mensagens perdidas (UC-RTN-001 a UC-RTN-006). Sem freezing.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::leaving_the_drained_state_returns_as_a_new_node`
  - `tests/drain_cluster.rs::after_leaving_the_node_returns_as_a_new_node_with_a_new_id`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`

---

### UC-DRN-019

**Caso:** `DELETE /node/drain` em `active`  
**Então:** `200`, nada a cancelar.  
**Nível:** C  
**Testes:**
  - `src/cluster/drain/tests.rs::canceling_without_a_drain_has_nothing_to_cancel`
  - `tests/operations.rs::deleting_the_drain_brings_the_node_back_with_its_messages`

---

### UC-DRN-020

**Caso:** Métricas e logs do drain  
**Então:** `taba.node.drain.state`, `taba.node.drain.pending`; log `info` (iniciado por sinal ou API, concluído, cancelado, volta como novo) e `error` (estouro de `drain.timeout` com o que sobrou, erro de I/O).  
**Nível:** C  
**Testes:**
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---


## DEV: Perfil de um nó (o antigo modo desenvolvimento)

### UC-DEV-002

**Caso:** Com `replication.factor=1` e sem `cluster.entry-address` o nó sobe sozinho, sem bootstrap; a porta do cluster é aberta e só serve `/cluster/*`, e a de cliente responde `404` a essas rotas.  
**Nível:** I  
**Testes:**
  - `tests/listeners.rs::a_node_with_factor_one_starts_alone_and_keeps_the_cluster_routes_off_the_client_port`

---

### UC-DEV-011

**Caso:** `GET /health/cluster` devolve o mesmo com `scope=cluster` e `scope=node`, em formato de cluster de um nó só; o Prometheus lê o nó, com uma série só.  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-DEV-012

**Caso:** A definição de tópicos é gravada no `_node.db` do próprio nó, sem bootstrap nem carga de tópicos de outro nó, e volta depois de um reinício (a restauração do disco).  
**Nível:** I  
**Testes:**
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---


## SEG: Portas, TLS mútuo e certificados do cluster

### UC-SEG-001

**Caso:** O nó atende em três listeners separados  
**Então:** cliente (`api.port`), cluster (`cluster.port`) e operação (`operations.port`, opcional). As três portas precisam ser diferentes; valores repetidos ou uma porta ocupada impedem o nó de subir.  
**Nível:** I  
**Testes:**
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`
  - `tests/startup.rs::an_occupied_api_port_stops_startup_naming_the_port`
  - `tests/startup.rs::an_occupied_cluster_port_stops_startup_in_production`

---

### UC-SEG-002

**Caso:** A porta de cliente responde `404` a `/cluster/*`; a porta do cluster responde `404` a tudo que não seja `/cluster/*`.  
**Nível:** I, C  
**Testes:**
  - `tests/cluster_api.rs::the_cluster_port_serves_only_cluster_routes_and_the_client_port_none`
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`

---

### UC-SEG-003

**Caso:** As rotas de operação ficam só na `operations.port` (sempre aberta, padrão 9090); na `api.port` existem apenas `/health/live` e `/health/ready` (sem token) e `/metrics` (qualquer token válido), e o resto responde `404`.  
**Nível:** I  
**Testes:**
  - `tests/listeners.rs::the_operations_routes_live_on_their_own_port_and_the_client_port_only_has_the_probes`

---

### UC-SEG-004

**Caso:** A porta do cluster não tem rate limit nem leitura de IP do cliente  
**Então:** os cabeçalhos de `client-ip.headers` são ignorados nela.  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-SEG-006

**Caso:** As rotas de operação não têm autenticação; a proteção é de rede (porta própria e `operations.allowed-ips`).  
**Nível:** I  
**Testes:**
  - `tests/mtls.rs::the_operations_routes_have_no_authentication`

---

### UC-SEG-007

**Caso:** Com a pasta de certificados (`cluster.tls.dir`, padrão `<secrets.dir>/tls`) existente, o TLS mútuo liga **sozinho** e a porta do cluster passa a exigir TLS; sem a pasta (padrão não existe e `cluster.tls.dir` não definido), atende em HTTP puro com aviso (`warn`) no log.  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::three_real_nodes_form_a_cluster_over_mutual_tls_and_replicate`
  - `tests/mtls.rs::without_the_certificates_folder_the_cluster_port_is_plain_http`
  - `tests/secrets.rs::a_node_mounting_the_folder_from_init_turns_authentication_on_by_itself`

---

### UC-SEG-008

**Caso:** Pasta existente sem ao menos um par válido (`.crt` + `.key`)  
**Então:** o nó **não sobe**. `cluster.tls.dir` definido que não existe: o nó não sobe.  
**Nível:** I  
**Testes:**
  - `src/cluster/tls/tests.rs::a_folder_without_a_valid_pair_does_not_open`
  - `tests/mtls.rs::a_folder_whose_pair_is_not_valid_stops_the_node_from_starting`
  - `tests/mtls.rs::a_certificates_folder_that_does_not_exist_is_a_start_error`

---

### UC-SEG-009

**Caso:** O nó **aceita** quem apresentar um certificado igual a um da pasta, ou emitido por um deles, desde que ainda não tenha expirado (`notAfter`); a data de início não conta para a confiança.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/tls/tests.rs::it_trusts_an_equal_certificate_and_ignores_the_start_date`
  - `src/cluster/tls/tests.rs::an_expired_certificate_is_neither_trusted_nor_presented`
  - `tests/mtls.rs::the_handshake_refuses_who_does_not_present_a_certificate_the_node_accepts`
  - `tests/mtls.rs::an_expired_certificate_is_not_accepted_and_is_not_presented`

---

### UC-SEG-010

**Caso:** O nó **apresenta** o par de início mais recente que já começou a valer (`notBefore` ≤ agora) e ainda não expirou, ao receber e ao abrir uma conexão (HTTP e WebSocket `/cluster/channel`).  
**Nível:** U, C  
**Testes:**
  - `src/cluster/tls/tests.rs::it_presents_the_newest_pair_that_has_already_started`
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-011

**Caso:** Cada lado exige que o outro apresente um certificado aceito; sem isso o handshake falha **antes** de qualquer resposta HTTP e nenhuma rota é alcançada. O nome do host não é verificado (os nós se conectam por IP).  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::three_real_nodes_form_a_cluster_over_mutual_tls_and_replicate`
  - `tests/mtls.rs::the_handshake_refuses_who_does_not_present_a_certificate_the_node_accepts`

---

### UC-SEG-012

**Caso:** Um `.crt` sem `.key` serve só para confiar; um par é `<nome>.crt` mais `<nome>.key` de mesmo nome. A pasta pode ter vários pares ao mesmo tempo.  
**Nível:** U  
**Testes:**
  - `src/cluster/tls/tests.rs::a_crt_without_key_only_serves_to_trust_and_a_key_without_crt_is_incomplete`

---

### UC-SEG-013

**Caso:** A pasta é recarregada sozinha a cada `cluster.tls.reload-interval` (padrão 60 s), sem reiniciar. Recarga com arquivo ilegível, par incompleto ou certificado inválido  
**Então:** o nó **mantém o conjunto anterior** e registra `error`; o conjunto só muda quando a pasta inteira é válida.  
**Nível:** C  
**Testes:**
  - `src/cluster/tls/tests.rs::a_reload_with_an_invalid_folder_keeps_the_previous_set_and_a_valid_one_replaces_it`
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-014

**Caso:** Rotação sem reiniciar  
**Então:** (1) gerar o certificado com início no futuro (`--starts-in`), (2) acrescentá-lo à pasta: cada nó o percebe em até `reload-interval` e passa a **confiar** nele, mas segue apresentando o antigo, (3) quando a data de início chega, todos passam a apresentar o novo sozinhos, (4) o antigo deixa de ser aceito quando expira.  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-015

**Caso:** Conexões já abertas (como o canal WebSocket) seguem com o certificado antigo até caírem, o que é inofensivo porque ele continua aceito até expirar.  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-016

**Caso:** Um certificado expirado deixa de ser aceito e de ser apresentado. A partir de `cluster.tls.expiry-warning` (30 dias) antes do `notAfter` do certificado em uso, o nó registra `warn` e expõe `taba.cluster.tls.expires_in`.  
**Nível:** C  
**Testes:**
  - `src/cluster/tls/tests.rs::an_expired_certificate_is_neither_trusted_nor_presented`
  - `tests/mtls.rs::an_expired_certificate_is_not_accepted_and_is_not_presented`

---

### UC-SEG-017

**Caso:** Uma CA na pasta (`.crt` de CA para confiar mais o par do próprio nó para apresentar)  
**Então:** os certificados emitidos por ela são aceitos.  
**Nível:** C  
**Testes:**
  - `src/cluster/tls/tests.rs::a_ca_in_the_folder_makes_the_certificates_it_issued_accepted`
  - `tests/mtls.rs::a_ca_in_the_folder_makes_the_certificates_it_issued_accepted`

---

### UC-SEG-018

**Caso:** Um chamador que não consta na tabela de membros recebe `403` `cluster.not-member` em qualquer rota `/cluster/*`, **exceto** as que aceitam quem ainda não é membro (`GET /cluster/nodes`, `POST /cluster/nodes/join`, `GET /cluster/message-loss`, `POST /cluster/messages/recover`); o repasse aceita também os nós que saíram por drain.  
**Nível:** C  
**Testes:**
  - `tests/cluster.rs::only_members_call_the_phase_routes_but_anyone_lists_and_asks_to_join`
  - `tests/cluster_api.rs::a_caller_that_is_not_a_member_is_refused_on_the_phase_routes`
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-SEG-019

**Caso:** `taba init --out <pasta>` cria de uma vez o primeiro certificado em `tls/` e o token raiz em `root-token` (mínimo 32 caracteres).  
**Nível:** I  
**Testes:**
  - `src/cli/tests.rs::init_needs_the_out_folder_and_defaults_to_five_years`
  - `src/secrets/tests.rs::init_creates_the_certificate_in_tls_and_a_root_token_of_at_least_32_characters`
  - `tests/secrets.rs::init_creates_the_certificate_and_the_root_token_and_refuses_to_run_twice`

---

### UC-SEG-020

**Caso:** `taba cert generate --days <n> [--starts-in <duração>] --out <pasta>` cria um par `cluster-<data de início>.crt` e `.key` (gerações sucessivas não se sobrescrevem), autoassinado e válido para servidor e cliente; `--starts-in` define o `notBefore` no futuro (padrão  
**Então:** agora, com folga de alguns minutos).  
**Nível:** I  
**Testes:**
  - `src/cli/tests.rs::cert_generate_takes_days_out_and_an_optional_start_delay`
  - `src/secrets/tests.rs::a_certificate_is_self_signed_and_valid_for_server_and_client`
  - `src/secrets/tests.rs::the_name_carries_the_start_date_and_the_key_is_a_pem_private_key`
  - `src/secrets/tests.rs::without_starts_in_it_starts_a_few_minutes_ago`
  - `src/secrets/tests.rs::nothing_is_overwritten_and_no_half_pair_is_left`
  - `src/secrets/tests.rs::init_never_overwrites_an_existing_root_token`
  - `tests/secrets.rs::cert_generate_adds_a_pair_named_after_its_start_date`

---

### UC-SEG-021

**Caso:** `taba token generate` imprime um segredo aleatório de 256 bits avulso.  
**Nível:** I  
**Testes:**
  - `src/cli/tests.rs::token_generate_takes_nothing_else`
  - `tests/secrets.rs::token_generate_prints_a_fresh_secret_each_time`

---


## OPS: Observabilidade (OpenTelemetry), estatísticas, métricas e saúde

### UC-OPS-001

**Caso:** `GET /health/live`  
**Então:** `200` `HealthResponse` com `health: "live"` enquanto o processo responde.  
**Nível:** I  
**Testes:**
  - `tests/listeners.rs::a_single_node_is_live_and_ready`

---

### UC-OPS-002

**Caso:** `GET /health/ready`  
**Então:** `200` com `health: "ready"` quando o nó está registrado (commit confirmado) e com **toda a sincronização concluída**; `503` com `health: "not-ready"` e `reason` (`bootstrapping`, `syncing`, `returning`, `draining`, `drained` ou `failed`) caso contrário.  
**Nível:** C  
**Testes:**
  - `src/cluster/state/tests.rs::a_failed_node_is_not_ready`
  - `tests/listeners.rs::a_single_node_is_live_and_ready`
  - `tests/listeners.rs::production_node_without_cluster_protocol_is_live_but_not_ready`
  - `tests/operations.rs::a_corrupt_topic_database_fails_the_node`

---

### UC-OPS-003

**Caso:** `/health/live` e `/health/ready` **não** devolvem `ErrorResponse`  
**Então:** o corpo é sempre `HealthResponse`. Essas duas rotas não consultam outros nós.  
**Nível:** I  
**Testes:**
  - `tests/listeners.rs::production_node_without_cluster_protocol_is_live_but_not_ready`

---

### UC-OPS-004

**Caso:** Em drain (`draining` ou `drained`) `/health/ready` responde `503` e `/health/live` segue `200`.  
**Nível:** C  
**Testes:**
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-OPS-005

**Caso:** `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node` nunca respondem `429`.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::the_json_routes_count_in_the_ops_category_and_metrics_does_not`

---

### UC-OPS-006

**Caso:** `GET /metrics` devolve texto do Prometheus (`text/plain; version=0.0.4`; OpenMetrics com `Accept: application/openmetrics-text`) **só com as métricas deste nó** e não aceita parâmetros; o nó não consulta outros nós para responder.  
**Nível:** I  
**Testes:**
  - `src/telemetry/prometheus/tests.rs::open_metrics_drops_total_from_the_family_and_ends_with_eof`
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-OPS-007

**Caso:** `/metrics` com `telemetry.prometheus.enabled` desligado  
**Então:** `404` `route.not-found`; IP fora de `operations.allowed-ips`: `403` `ops.ip-not-allowed`.  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::metrics_is_404_when_off_and_403_for_an_ip_outside_the_list`

---

### UC-OPS-011

**Caso:** Mapeamento de nomes OTel → Prometheus  
**Então:** pontos viram `_`, a unidade vira sufixo (`_seconds`, `_bytes`; `_ratio` só na unidade `1`, para razões de verdade), unidades anotadas entre chaves (`{message}`, `{flag}`) não geram sufixo, contadores ganham `_total`, histogramas viram `_bucket`/`_sum`/`_count`, atributos viram rótulos; `service.namespace/service.name` vira `job`, `service.instance.id` vira `instance`, e `target_info` leva os demais atributos de recurso.  
**Nível:** U  
**Testes:**
  - `src/telemetry/prometheus/tests.rs::names_map_from_opentelemetry_to_prometheus`
  - `src/telemetry/prometheus/tests.rs::counters_get_total_and_the_unit_is_not_repeated`
  - `src/telemetry/prometheus/tests.rs::histograms_become_bucket_sum_and_count_with_attributes_as_labels`
  - `src/telemetry/prometheus/tests.rs::the_resource_becomes_target_info`

---

### UC-OPS-012

**Caso:** Métricas de nó  
**Então:** cardinalidade limitada; **nunca** carregam tags nem o nome do tópico; `http.route` é sempre o **template** (`/topics/{topic}/messages`), nunca o caminho real; o ID da mensagem nunca é rótulo.  
**Nível:** U, I  
**Testes:**
  - `src/telemetry/metrics/tests.rs::counters_add_up_per_series`
  - `tests/metrics.rs::requests_are_measured_by_route_template_never_by_topic_or_path`

---

### UC-OPS-013

**Caso:** OTLP e `/metrics` são alimentados pelos mesmos instrumentos  
**Então:** as métricas são as mesmas nas duas saídas; o OTLP envia por *push* as métricas do próprio nó, sem fan-out e sem lista de alvos.  
**Nível:** I  
**Testes:**
  - `src/telemetry/otlp/metrics/tests.rs::each_instrument_kind_becomes_the_matching_otlp_aggregation`
  - `src/telemetry/otlp/metrics/tests.rs::the_request_carries_the_resource_the_scope_and_skips_families_without_series`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`

---

### UC-OPS-014

**Caso:** Métricas de tópico exportadas por `telemetry.topics.mode`  
**Então:** `off` (padrão: nenhuma), `by-tags` (agregado pelas tags de `telemetry.topics.group-by-tags`, no atributo `taba.topic.tag.<chave>`) e `per-topic` (uma série por tópico, em `messaging.destination.name`, até `telemetry.topics.max-topics`, o excedente agregado em `messaging.destination.name="_other"`). `taba.topic.oldest_timestamp` não tem série no nó sem mensagens do tópico.  
**Nível:** U, I  
**Testes:**
  - `tests/metrics.rs::topic_metrics_are_off_by_default`
  - `tests/metrics.rs::per_topic_mode_has_one_series_per_topic_and_no_oldest_without_messages`
  - `tests/metrics.rs::per_topic_mode_folds_the_topics_above_the_ceiling_into_other`
  - `tests/metrics.rs::by_tags_mode_sums_by_tag_value_and_never_names_a_topic`

---

### UC-OPS-016

**Caso:** Métricas de processo e host, com os nomes padrão  
**Então:** `process.uptime`, `process.cpu.utilization`, `process.memory.usage`, `process.thread.count` (threads do SO, sem as tasks do tokio), `process.open_file_descriptor.count`, e `taba.node.connections`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::publish_errors_are_counted_per_topic_and_not_for_missing_topics`
  - `src/telemetry/process/tests.rs::status_gives_the_resident_memory_and_the_threads`
  - `src/telemetry/process/tests.rs::stat_cpu_counts_from_the_last_parenthesis`
  - `src/telemetry/process/tests.rs::utilization_is_measured_between_two_readings`
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`
  - `tests/metrics.rs::on_linux_the_process_metrics_have_memory_threads_and_files`

---

### UC-OPS-017

**Caso:** `GET /health/cluster?scope=node`  
**Então:** leitura local, `200` `ClusterStatus` com um só item em `nodes`, cujo `status` é o `NodeStatus` (`nodeId`, `version`, `state`, `drainState`, `startedAt`, `collectedAt`, `storage`, `signals`), **sem** as métricas.  
**Nível:** I  
**Testes:**
  - `tests/operations.rs::cluster_health_describes_the_single_node`

---

### UC-OPS-018

**Caso:** `GET /health/cluster` (`scope=cluster`, o padrão)  
**Então:** consulta todos os nós (`GET /cluster/status`) e devolve `ClusterStatus` (`scope`, `collectedAt`, `partial`, `worstCondition`, `conditions`, `summary` com `members` por estado, `membersVersion`, `quorumAvailable`, `syncingBlocks`, e `nodes`). Um nó que não responde aparece com `reachable: false` e `status: null`, sem falhar a chamada; `scope` desconhecido: `400`.  
**Nível:** C  
**Testes:**
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/metrics.rs::free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure`
  - `tests/operations.rs::cluster_health_describes_the_single_node`

---

### UC-OPS-019

**Caso:** `GET /metrics/topics`  
**Então:** `Page<TopicStats>` paginada **sempre por nome**, com `scope` (`cluster` ou `node`), `tag.<chave>`, `condition`, `fields` (`name` e `id` vêm sempre) e `groupBy` (uma linha por valor da tag, `TopicStatsGroup`, com as quantidades somadas e as datas pelo menor valor).  
**Nível:** I, C  
**Testes:**
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`
  - `tests/topic_stats.rs::pages_by_name_and_validates_the_parameters`
  - `tests/topic_stats.rs::group_by_a_tag_gives_one_row_per_value_with_the_sums`
  - `tests/topic_stats.rs::the_json_routes_count_in_the_ops_category_and_metrics_does_not`

---

### UC-OPS-021

**Caso:** `GET /metrics/topics/{topic}`  
**Então:** `TopicStatsDetail` (`TopicStats` mais `topGroups`, até `stats.topics.top-groups`); `404` `topic.not-found`; num distribuidor devolve só `distributor`.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::the_detail_adds_the_largest_groups_and_404s_for_unknown_topics`

---

### UC-OPS-022

**Caso:** `GET /metrics/topics/{topic}/groups`  
**Então:** `Page<GroupStats>` por `groupId` (UC-GRP-015); `top` e `cursor` juntos: `400` `validation.failed`.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::the_groups_route_lists_pages_filters_and_tops_the_groups`

---

### UC-OPS-023

**Caso:** Visão do nó × visão do cluster no `/stats/topics` (`scope=node` ou o padrão, do cluster)  
**Então:** **só o primário conta** (senão a soma seria multiplicada pelo fator de replicação); o cluster soma `visibleMessages`, `invisibleMessages` e `depth`, e `oldestMessageAt` é a **menor** data entre os nós; `oldestAge = agora − oldestMessageAt`.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/blocks.rs::only_the_owner_counts_a_message_in_the_visible_invisible_and_oldest_numbers`
  - `src/http/topic_stats/raw/tests.rs::the_cluster_view_adds_what_each_primary_counts_and_takes_the_oldest_date`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-OPS-024

**Caso:** Um nó sem mensagens do tópico tem `oldestMessageAt` **ausente**, nunca zero (um zero venceria o "mais antigo").  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::the_counters_follow_each_transition_of_a_message`
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`

---

### UC-OPS-026

**Caso:** Cada nó mantém contadores locais por tópico, atualizados nas **transições** (publicação +1 visível; entrega −1 visível +1 invisível; vencimento −1 invisível +1 visível; ack −1 no estado em que estava), sem varrer mensagens a cada consulta.  
**Nível:** U  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::the_counters_follow_each_transition_of_a_message`

---

### UC-OPS-028

**Caso:** Condições do tópico  
**Então:** `idle` (sem publicação nem consumo há `stats.topics.idle-after`), `backlog` (`depth` ou `oldestAge` acima do limite), `stalled` (há mensagens mas nenhum consumo há `stats.topics.stalled-after`), `noisy-group`, `group-stalled` (só FIFO), `memory-pressure` (a maior parte em disco); `worstCondition` é a de maior severidade.  
**Nível:** U, I  
**Testes:**
  - `src/engine/messages/stats/tests.rs::a_limit_that_was_not_set_turns_its_condition_off`
  - `src/engine/messages/stats/tests.rs::backlog_fires_on_depth_or_on_age`
  - `src/engine/messages/stats/tests.rs::stalled_needs_messages_and_no_consumption_for_the_whole_period`
  - `src/engine/messages/stats/tests.rs::idle_counts_from_the_last_publish_or_consume_or_the_baseline`
  - `src/engine/messages/stats/tests.rs::noisy_group_needs_two_groups_and_more_than_the_share`
  - `src/engine/messages/stats/tests.rs::memory_pressure_is_most_of_the_topic_on_disk`
  - `src/engine/messages/stats/tests.rs::group_stalled_follows_what_the_index_found`
  - `src/engine/messages/stats/tests.rs::warnings_come_before_infos`
  - `tests/topic_stats.rs::noisy_group_fires_with_two_groups_and_the_largest_over_the_share`
  - `tests/topic_stats.rs::backlog_stalled_and_idle_follow_the_configured_limits_and_the_clock`
  - `tests/topic_stats.rs::memory_pressure_means_most_of_the_topic_is_on_disk`
  - `tests/topic_stats.rs::group_stalled_fires_when_a_fifo_head_is_delivered_too_often_without_ack`

---

### UC-OPS-029

**Caso:** A dead-letter aparece em `/metrics/topics` e em `/topics`, com `deadLetter: true`, `source` e as `tags` da fila de origem.  
**Nível:** I  
**Testes:**
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`

---

### UC-OPS-031

**Caso:** Traces  
**Então:** um por operação de cliente (publicação, consumo, ack), por bloco sincronizado (um span por lote, com o nó de origem), por registro e desregistro de nó e votação (um span por nó consultado); atributos: tags do tópico, ID da mensagem, bloco, nós, número da tentativa. O ID da mensagem vai em trace e log, nunca em métrica.  
**Nível:** C  
**Testes:**
  - `tests/otlp.rs::a_cluster_write_is_one_trace_with_a_register_and_a_commit_span_per_node`
  - `tests/otlp.rs::a_synced_block_is_one_trace_with_a_span_per_batch_naming_the_source_node`
  - `tests/otlp.rs::an_exclusion_vote_is_one_trace_with_a_span_per_voter`
  - `tests/otlp.rs::the_trace_context_travels_to_the_other_nodes_of_a_real_cluster`

---

### UC-OPS-032

**Caso:** Logs estruturados (JSON ou OTLP) com `trace_id` e `span_id` e as tags do tópico; o nível é `telemetry.logs.level`, com a severidade do OpenTelemetry; logs por mensagem no caminho quente ficam em `debug`.  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::the_log_format_defaults_to_text_and_comes_from_yaml_or_env`
  - `tests/otlp.rs::logs_carry_trace_id_span_id_severity_and_the_event_name`
  - `tests/otlp.rs::the_otlp_log_level_follows_telemetry_logs_level`

---

### UC-OPS-033

**Caso:** Eventos pontuais (não condições de saúde), como *log records* com `event.name`  
**Então:** `taba.distributor.no_target`, `taba.distributor.partial`, `taba.volatile.deleted`, `taba.volatile.check_blocked`, `taba.message.dead_lettered`, `taba.message.deduplicated`, `taba.message.expired` e `taba.message.lost`; o tópico vai em `messaging.destination.name`.  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::a_dead_lettered_message_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::an_expired_message_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::a_deduplicated_publication_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::a_distributor_raises_no_target_and_partial_and_counts_its_copies`

---

### UC-OPS-034

**Caso:** Overflow para disco  
**Então:** log `info` com **resumo periódico**, não por mensagem; erro de I/O: `error`.  
**Nível:** I  
**Testes:**
  - `src/engine/messages/tests/stats_events.rs::the_overflow_goes_to_the_log_as_one_periodic_summary_and_not_per_message`

---

### UC-OPS-035

**Caso:** Acesso às rotas de operação  
**Então:** a porta de operação responde a quem a alcançar; com `operations.allowed-ips`, só os IPs/CIDR da lista acessam (os demais: `403`), enquanto a `api.port` e suas sondas continuam abertas.  
**Nível:** I  
**Testes:**
  - `tests/operations.rs::the_operations_routes_only_accept_the_allowed_ips`

---

### UC-OPS-036

**Caso:** O nó não sobe se `operations.port` estiver ocupada ou se `operations.allowed-ips` tiver entrada inválida.  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::allowed_ips_must_be_valid_addresses_or_cidr`
  - `tests/startup.rs::an_occupied_operations_port_stops_startup`

---

### UC-OPS-038

**Caso:** Perda de mensagem não gera métrica nem condição (só log e a tabela `losses`)  
**Então:** ver UC-SIN-027 e UC-SIN-028.  
**Nível:** C  
**Testes:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-OPS-039

**Caso:** A exportação OTLP é configurada pelas variáveis padrão do SDK, sem prefixo `TABA_`  
**Então:** sem `OTEL_EXPORTER_OTLP_ENDPOINT` o OTLP fica desligado; com ele, o nó envia métricas, traces e logs; `OTEL_EXPORTER_OTLP_PROTOCOL` escolhe `grpc` (padrão) ou `http/protobuf`; `OTEL_EXPORTER_OTLP_HEADERS` e o certificado valem.  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::without_otel_exporter_otlp_endpoint_the_otlp_is_off`
  - `src/config/tests.rs::the_otlp_protocol_defaults_to_grpc_and_accepts_http_protobuf`
  - `src/config/tests.rs::an_invalid_otlp_setting_stops_the_start_with_every_problem_listed`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`
  - `tests/otlp.rs::otlp_defaults_to_grpc_and_sends_the_three_signals`
  - `tests/otlp.rs::otlp_over_grpc_trusts_the_certificate_in_otel_exporter_otlp_certificate`

---

### UC-OPS-040

**Caso:** Atributos de recurso em toda a telemetria  
**Então:** `service.name=taba`, `service.namespace` e `deployment.environment.name` vindos de `OTEL_RESOURCE_ATTRIBUTES`, `service.instance.id` igual ao ID do nó (novo a cada início) e `service.version`.  
**Nível:** U, I  
**Testes:**
  - `src/config/tests.rs::the_otel_variables_give_the_resource_attributes_and_the_service_name_wins`
  - `src/telemetry/otlp/metrics/tests.rs::the_request_carries_the_resource_the_scope_and_skips_families_without_series`
  - `src/telemetry/prometheus/tests.rs::the_resource_becomes_target_info`
  - `tests/metrics.rs::the_resource_attributes_come_from_the_otel_variables_and_the_node`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`

---

### UC-OPS-041

**Caso:** Nomes conforme o OpenTelemetry  
**Então:** nenhuma métrica ou atributo `msg.*`; HTTP em `http.server.request.duration`, `http.server.active_requests` e os tamanhos de corpo (sem contador de requisições à parte), com `http.request.method`, `http.route`, `http.response.status_code`, `error.type`, `url.scheme` e `server.port` (que distingue as três portas); chamadas entre nós em `http.client.request.duration`; unidade sem sufixo no nome, e `1` só em razões.  
**Nível:** U, I  
**Testes:**
  - `src/telemetry/metrics/tests.rs::counters_add_up_per_series`
  - `src/telemetry/metrics/tests.rs::names_follow_the_opentelemetry_vocabulary`
  - `src/telemetry/prometheus/tests.rs::histograms_become_bucket_sum_and_count_with_attributes_as_labels`
  - `tests/metrics.rs::requests_are_measured_by_route_template_never_by_topic_or_path`

---

### UC-OPS-043

**Caso:** Os spans de mensageria levam `messaging.system=taba`, `messaging.destination.name`, `messaging.operation.type` e `messaging.message.id`; as tags do tópico vão como `taba.topic.tag.<chave>`; os spans de falha e de evento de durabilidade levam `taba.durability_event=true`, para o Collector mantê-los na amostragem por cauda.  
**Nível:** C  
**Testes:**
  - `tests/otlp.rs::messaging_spans_carry_the_semantic_attributes_and_the_topic_tags`
  - `tests/otlp.rs::a_failed_operation_span_carries_the_durability_event_mark`
  - `tests/otlp.rs::a_cluster_write_is_one_trace_with_a_register_and_a_commit_span_per_node`

---

### UC-OPS-044

**Caso:** A amostragem do SDK segue `OTEL_TRACES_SAMPLER` e `OTEL_TRACES_SAMPLER_ARG` (padrão `parentbased_traceidratio`); um `traceparent` recebido com a decisão de amostrar é respeitado.  
**Nível:** I  
**Testes:**
  - `src/config/tests.rs::the_sampler_follows_otel_traces_sampler_and_defaults_to_parentbased_traceidratio`
  - `tests/otlp.rs::a_sampled_traceparent_is_respected_and_the_ratio_sampler_drops_the_rest`
  - `tests/otlp.rs::always_off_exports_no_spans`

---

### UC-OPS-045

**Caso:** O `GET /metrics` do nó e o OTLP funcionam num nó só (perfil de um nó).  
**Nível:** I  
**Testes:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-OPS-046

**Caso:** `GET /cluster/status` e `/nodes/{nodeId}/metrics`, `/drain`, `/backup`, `/stats/topics` com um token que não é o raiz, e com um `nodeId` desconhecido  
**Então:** `403 auth.forbidden` com `required: "root"`; com o token raiz, o estado do cluster, o texto Prometheus do nó e o estado do drain; um `nodeId` desconhecido é `404 node.not-found`.  
**Nível:** I  
**Testes:**
  - `tests/nodes_api.rs::the_cluster_and_node_routes_need_the_admin_permission_and_an_unknown_node_is_404`

---

### UC-OPS-047

**Caso:** `/nodes/{nodeId}/…` recebido por um nó que é, ou não é, o `nodeId`  
**Então:** O nó que é ele responde com os próprios dados e nada cruza a porta do cluster; senão chama o destino **uma vez** (`node-call`) e devolve a resposta como veio; iniciar o drain de outro nó por qualquer nó drena aquele; um nó drenado continua em `GET /nodes` como `drained` e continua endereçável; um `nodeId` desconhecido não chama ninguém.  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::a_node_answers_for_itself_or_calls_the_node_that_has_the_data_once`

---

### UC-OPS-048

**Caso:** `/nodes/{nodeId}/…` para um nó que não responde  
**Então:** `502 forward.bad-gateway` nomeando o nó, depois de uma única tentativa (sem nova tentativa).  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::a_node_that_does_not_answer_is_a_failure_that_names_it_with_no_retry`

---

### UC-OPS-049

**Caso:** `GET /stats/topics`, `/stats/topics/{topic}` e `/stats/topics/{topic}/groups` por qualquer nó de um cluster  
**Então:** Os números do cluster, completos (12 mensagens publicadas em três nós aparecem como 12 por qualquer nó), com `partial: false` e `missingNodes: []`; `?scope=node` é só aquele nó e os três nós somam o cluster; cada grupo aparece uma vez, com as mensagens de todos os nós.  
**Nível:** I, C  
**Testes:**
  - `tests/nodes_api.rs::the_statistics_are_the_clusters_not_one_nodes`

---

### UC-OPS-050

**Caso:** Um nó não responde a uma leitura consolidada (`/stats/topics…`, `/nodes`, `/cluster/status`)  
**Então:** `200` com `partial: true` e `missingNodes: [{ nodeId, reason }]` (`timeout` ou `unreachable`) nomeando esse nó; os números são os dos outros; quando o nó volta, `partial` é `false` e os números estão completos.  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::a_node_that_does_not_answer_makes_the_consolidated_reads_partial_and_says_which`

---

### UC-OPS-051

**Caso:** `GET /cluster/status` e `GET /cluster/backup` com o token raiz  
**Então:** O `NodeStatus` de cada nó traz `runtime` (`activeRequests`, `rateLimitRejected`, `channelRttP95Seconds`) ao lado de memória e disco; `/cluster/backup` lista o estado do backup em S3 de cada nó com o `nodeId`; sem backup, `422 backup.not-configured`.  
**Nível:** C  
**Testes:**
  - `tests/nodes_api.rs::the_cluster_status_carries_the_load_of_each_node_and_the_backup_is_consolidated`
  - `tests/nodes_api.rs::without_a_backup_the_cluster_backup_route_says_so`

---

### UC-OPS-052

**Caso:** `GET /health/cluster` com o token raiz  
**Então:** o `NodeStatus` de cada nó traz `excludedSince` (desde quando ele grava o log de excluídos) e o `summary` traz `excludedCutoff` (a data de corte do cluster, a mais recente entre os ativos), para o operador saber até onde a retomada confia no log.  
**Nível:** C  
**Testes:**
  - `tests/excluded_log.rs::the_cluster_cut_off_is_the_latest_start_among_the_active_members`

---

### UC-OPS-053

**Caso:** A restauração das vidas anteriores não terminou
**Então:** `GET /health/cluster` mostra a condição `previous-life-pending` (aviso) até a votação fechar ou o operador forçar; o log traz o aviso de restauração com nós ausentes.
**Nível:** C
**Testes:**
  - `src/cluster/health/tests.rs::a_restoration_in_progress_raises_previous_life_pending_as_a_warning`
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`

---


## CLR: Contrato das rotas do cluster (DTOs e regras comuns)

### UC-CLR-002

**Caso:** Todo `register`, `commit` e `rollback` leva o `operationId` (GUID v7 do coordenador) e é idempotente por ele. Um registro sem commit nem rollback expira após `write.pending.ttl`.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/members/tests.rs::a_registration_without_commit_or_rollback_expires_and_each_phase_repeats_by_its_operation_id`
  - `src/cluster/protocol/dto_tests.rs::every_phase_of_every_two_phase_call_carries_the_operation_id`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C. A expiração por `write.pending.ttl` só está provada nos registros de nós; nas operações de mensagem falta.

---

### UC-CLR-004

**Caso:** Corpo inválido nas rotas do cluster  
**Então:** `400` `validation.failed`, `validation.required` ou `validation.unknown-field`.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/protocol/dto_tests.rs::an_invalid_body_on_a_cluster_route_is_a_400_with_a_validation_code`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-CLR-005

**Caso:** Os DTOs do cluster (`Member`, `MemberTable`, `JoinRequest`, `NodeRegisterRequest`, `NodeUnregisterRequest`, `OperationRef`, `VoteRequest`, `VoteResponse`, `DefinitionProposal`, `DefinitionApply`, `TopicDefinition`, `TokenRecord`, `MessageRegisterRequest`, `VisibilityRegisterRequest`, `AckRegisterRequest`, `TakeRequest`, `TakeResponse`, `MessageRecord`, `IdleResponse`, `LossRecord`, `LossAck`, `RecoverResult`, `StreamEnd`) serializam e desserializam com os campos e tipos documentados, e os valores `null` de `ttl`, `maxDeliveries` e `maxMessageBytes` da `TopicDefinition` são enviados como `null`.  
**Nível:** U  
**Testes:**
  - `src/cluster/protocol/dto_tests.rs::the_cluster_dtos_have_the_documented_fields_and_types`

---

### UC-CLR-006

**Caso:** O DTO `TopicDefinition` é igual ao JSON guardado na tabela `topics`  
**Então:** os campos de `Topic` sem `effective`, `deadLetter` e `source`.  
**Nível:** U  
**Testes:**
  - `src/engine/topics/tests.rs::the_cluster_definition_is_the_topic_json_without_effective_dead_letter_and_source`

---

### UC-CLR-010

**Caso:** Códigos só do cluster  
**Então:** `cluster.not-member`, `cluster.node-frozen`, `cluster.join-unreachable`, `cluster.conflict`, `cluster.wrong-node`, `fifo.misdirected` e `cluster.operation-unknown`.  
**Nível:** U, C  
**Testes:**
  - `src/core/error/tests.rs::the_codes_only_the_cluster_uses_are_in_the_catalog_with_their_statuses`
**Observação:** Só a parte unitária (U) está provada; o nível completo é U, C.

---

### UC-OPS-054

**Caso:** A sondagem dos IPs antigos terminou com nós faltando
**Então:** `GET /health/cluster` mostra a condição `members-missing` (aviso) enquanto a tabela tiver menos membros ativos que a da vida anterior.
**Nível:** C
**Testes:**
  - `src/cluster/health/tests.rs::a_probe_that_ended_with_nodes_missing_raises_members_missing_as_a_warning`
  - `tests/regroup.rs::when_the_probe_time_ends_with_nodes_missing_the_members_missing_condition_shows`

---


### UC-OPS-055

**Caso:** O nó roda o perfil de um nó com algo relaxado (fator 1, TLS desligado ou `auth.dev-token`)
**Então:** `GET /health/cluster` mostra a condição `reduced-profile` (aviso, informativa); `health.single-node-condition=false` a esconde.
**Nível:** U
**Testes:**
  - `src/cluster/health/tests.rs::the_reduced_profile_is_an_informative_warning`

---
### UC-CLR-001

**Caso:** Toda rota `/cluster/*` só aceita quem consta na tabela de membros (`403` `cluster.not-member`), salvo as quatro exceções (UC-SEG-018).  
**Nível:** C  
**Testes:**
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-CLR-007

**Caso:** `GET /cluster/topics/{id}/idle` responde `IdleResponse` (`topicId`, `idleSeconds`, `sinceStart`); tópico desconhecido  
**Então:** `404` `topic.not-found`.  
**Nível:** C  
**Testes:**
  - `tests/volatile_cluster.rs::the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start`

---

### UC-CLR-009

**Caso:** `GET /cluster/status` devolve o `NodeStatus`; não existe `GET /cluster/metrics` (cada nó expõe as próprias métricas).  
**Nível:** C  
**Testes:**
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`

---


## CFG: Configuração e validações de partida

### UC-CFG-001

**Caso:** Todo parâmetro pode ser definido por variável de ambiente com o prefixo `TABA_` e a chave em maiúsculas com `.` e `-` trocados por `_` (`topics.distributor.max-targets` → `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`).  
**Nível:** U  
**Testes:**
  - `src/config/params/tests.rs::env_name_follows_the_documented_rule`

---

### UC-CFG-002

**Caso:** Os valores padrão documentados valem quando o parâmetro não é definido (por exemplo `replication.factor` 3, `write.quorum` 2 em rf 3, `delivery.visibility-timeout.default` 120, `delivery.max-messages.default` 10, `retention.ttl.default` 14 dias, `retention.max-deliveries.default` 10, `fifo.dedup.window` 300 s, `cluster.vote.quorum-percent` 51, `cluster.vote.min-voters` 2).  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::documented_defaults_apply_when_nothing_is_set`
  - `src/config/tests.rs::quorum_defaults_to_the_majority_of_the_replication_factor`

---

### UC-CFG-003

**Caso:** **Sem limite** significa que o teto não existe enquanto a variável não for definida (`topics.distributor.max-targets`, `topics.volatile.max-count`).  
**Nível:** U, I  
**Testes:**
  - `src/config/tests.rs::without_limit_means_the_ceiling_does_not_exist_until_set`

---

### UC-CFG-004

**Caso:** O nó não sobe, com mensagem explícita, em cada um destes casos  
**Então:** portas repetidas ou ocupadas; `cluster.entry-address` ausente em produção; `auth.enabled=true` sem token raiz, ou token raiz curto; `auth.root-token-file` ou `cluster.tls.dir` definido que não existe; pasta de certificados sem par válido; `operations.port` ocupada ou `operations.allowed-ips` inválida; `write.quorum` fora de 1 a `replication.factor` (recusado na validação); `storage.dir` que não pode ser criado ou gravado.  
**Nível:** I  
**Testes:**
  - `tests/startup.rs::an_occupied_api_port_stops_startup_naming_the_port`
  - `tests/startup.rs::an_occupied_cluster_port_stops_startup_in_production`
  - `tests/startup.rs::a_storage_dir_that_cannot_be_created_stops_startup`

---

### UC-CFG-005

**Caso:** `message.max-bytes` não pode passar de `message.hard-limit`, e `cluster.sync.stream.max-line-bytes` precisa caber o limite rígido em base64 mais o envelope.  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::default_message_size_cannot_pass_the_hard_limit`
  - `src/config/tests.rs::stream_line_limit_must_fit_the_hard_limit_in_base64`

---

### UC-CFG-008

**Caso:** Valor de configuração com tipo inválido (texto onde se espera número, por exemplo) impede a partida com mensagem que nomeia o parâmetro.  
**Nível:** U  
**Testes:**
  - `src/config/source/tests.rs::invalid_value_names_the_parameter_and_its_variable`
  - `src/config/tests.rs::wrong_type_stops_startup_naming_the_parameter`

---

### UC-CFG-009

**Caso:** A precedência é  
**Então:** variável de ambiente, depois o arquivo `.yml`, depois o padrão da tabela. Uma variável vazia vale como não definida.  
**Nível:** U  
**Testes:**
  - `src/config/source/tests.rs::environment_wins_over_file_and_file_over_default`
  - `src/config/source/tests.rs::empty_environment_value_counts_as_unset`

---

### UC-CFG-010

**Caso:** Um caminho de arquivo informado (`--config` ou `TABA_CONFIG_FILE`) que não existe impede a partida, com o caminho na mensagem; o caminho padrão (`/etc/taba/taba.yml`) ausente não impede.  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::an_explicit_config_file_that_does_not_exist_stops_startup`

---

### UC-CFG-011

**Caso:** Chave desconhecida no arquivo `.yml` impede a partida, com o nome da chave na mensagem; variável `TABA_*` desconhecida só gera um aviso no log.  
**Nível:** U  
**Testes:**
  - `src/config/source/tests.rs::unknown_yaml_key_is_rejected_by_name`
  - `src/config/source/tests.rs::unknown_taba_variable_becomes_a_warning`

---

### UC-CFG-012

**Caso:** Duração sem unidade é recusada (`60`); com `ms`, `s`, `m`, `h` ou `d` vale. Tamanho usa unidades binárias (`5MB` são 5.242.880 bytes) e aceita um número sozinho como bytes.  
**Nível:** U  
**Testes:**
  - `src/config/source/tests.rs::durations_need_a_unit`
  - `src/config/source/tests.rs::sizes_are_binary_and_accept_plain_bytes`

---

### UC-CFG-013

**Caso:** Os parâmetros `cluster.restore.*` (`grace` 5m, `strict` false, `clock-slack` 5s, `probe.timeout` 2h, `probe.interval` 10s) e os limites do log de excluídos (`storage.excluded.max-size` 100MB e `storage.excluded.max-age` 24h) têm padrão; `grace` ou `probe.interval` zerados, `probe.timeout` menor que `probe.interval` e os dois limites do log zerados são problemas de validação; um valor de tipo errado é recusado na leitura  
**Então:** o nó não sobe com valores inválidos e mostra a chave.  
**Nível:** U  
**Testes:**
  - `src/config/tests.rs::the_restore_parameters_have_defaults_and_invalid_values_stop_the_start`
  - `src/config/tests.rs::the_limits_of_the_excluded_log_have_defaults_and_invalid_values_stop_the_start`

---


### UC-CFG-014

**Caso:** `cluster.tls.enabled=false` e `auth.dev-token` só valem com `replication.factor=1`; `auth.dev-token` com menos de 32 caracteres é recusado; com fator 1 o `cluster.entry-address` deixa de ser obrigatório
**Então:** o nó não parte com o valor recusado e mostra a chave; o padrão continua o de produção (TLS ligado, entrada obrigatória).
**Nível:** U
**Testes:**
  - `src/config/tests.rs::the_one_node_profile_needs_no_entry_address_and_relaxes_only_with_factor_one`

---

## BKP: Backup opcional em S3

### UC-BKP-001

**Caso:** Sem `backup.s3.bucket` o recurso **não existe**  
**Então:** o nó é o de sempre e `GET /node/backup`, `POST /node/backup/flush` e `POST /cluster/backup/flush` respondem `422` `backup.not-configured`. Com o bucket configurado mas o **S3 fora do ar na partida**, o nó **sobe com aviso** e o cluster funciona como sem o backup (o recurso liga sozinho quando o S3 volta).  
**Nível:** I, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::without_a_backup_nothing_is_told_and_a_missing_body_is_still_a_loss`
  - `tests/backup.rs::with_s3_down_at_start_the_nodes_start_with_a_warning_and_the_cluster_works`
  - `tests/backup.rs::without_a_bucket_the_backup_does_not_exist_and_its_routes_say_so`
  - `tests/s3_store.rs::an_unreachable_s3_is_an_error_the_caller_can_swallow_not_a_panic`

---

### UC-BKP-002

**Caso:** A configuração `backup.*` tem os padrões documentados (`delay` 60 s, `meta.interval` 60 s, segmento de 8 MB, 5000 mensagens ou 60 s, batimento 15 s, `dead-after` 60 s, `drain.timeout` 30 s, evicção 85/70/90); as credenciais são **opcionais** (sem elas vale a cadeia padrão da AWS); `endpoint` liga o `path-style`; o segredo nunca aparece em `Debug`; valores contraditórios impedem a partida.  
**Nível:** U  
**Testes:**
  - `src/cluster/backup/tests.rs::the_backup_rules_come_from_the_configuration_with_the_documented_defaults`
  - `src/cluster/backup/tests.rs::the_example_configuration_files_are_valid`
  - `src/cluster/backup/tests.rs::the_s3_settings_follow_the_endpoint_and_the_credentials_are_optional`

---

### UC-BKP-003

**Caso:** O depósito de objetos (`ObjectStore`) faz `put`, `get`, `get_range`, `get_suffix`, `list` e `delete`, e a escrita condicional (`If-None-Match`, `If-Match`), tanto no fake em memória quanto no cliente do S3 (contra um S3 em HTTP); um `404` é "não existe", um `412` é "a condição falhou", e o S3 inacessível é um erro que o chamador engole. **O S3 nunca entra no caminho da publicação:** com ele fora do ar a publicação e o consumo seguem, e o `outbox` guarda os IDs até ele voltar.  
**Nível:** U, C  
**Testes:**
  - `src/storage/backup/store/tests.rs::a_store_without_conditional_writes_says_so_and_one_that_is_down_fails`
  - `src/storage/backup/store/tests.rs::conditional_writes_create_once_and_replace_only_the_known_version`
  - `src/storage/backup/store/tests.rs::the_memory_store_does_what_the_object_store_trait_promises`
  - `tests/backup.rs::when_the_primary_dies_the_new_primary_sends_what_was_still_pending`
  - `tests/s3_store.rs::the_s3_client_does_what_the_object_store_contract_says_against_an_s3_server`

---

### UC-BKP-004

**Caso:** O prefixo base (`backup.s3.prefix`) vai na frente de **todas** as chaves e fica fora das listagens; vazio, nada muda.  
**Nível:** U, C  
**Testes:**
  - `src/storage/backup/store/tests.rs::the_base_prefix_goes_in_front_of_every_key_and_out_of_the_listing`
  - `tests/s3_store.rs::the_base_prefix_is_a_folder_in_the_bucket`

---

### UC-BKP-005

**Caso:** Os nomes dos objetos seguem o desenho  
**Então:** `topics/<tópico>.json`, `tokens/<id>.json`, `nodes/<id>.json`, `messages/<tópico>/<topicId>/<primeiroId>_<escritor>_<seq>.seg`, `journal/…/<instante>_<escritor>_<seq>.jrn`; o `topicId` no caminho impede que um tópico recriado ressuscite mensagens do antigo.  
**Nível:** U  
**Testes:**
  - `src/storage/backup/keys/tests.rs::segments_of_a_recreated_topic_do_not_share_a_prefix_with_the_old_one`
  - `src/storage/backup/keys/tests.rs::the_key_layout_follows_the_documented_one`

---

### UC-BKP-006

**Caso:** O **segmento** termina num rodapé-índice (id, deslocamento, tamanho)  
**Então:** dá para ler o rodapé do fim do objeto sem baixá-lo (pedindo mais se o rodapé é maior que o palpite) e uma só mensagem por `Range`; lixo é recusado.  
**Nível:** U  
**Testes:**
  - `src/storage/backup/segment/tests.rs::a_segment_round_trips_with_its_footer_index`
  - `src/storage/backup/segment/tests.rs::a_short_tail_asks_for_the_whole_footer_and_garbage_is_refused`
  - `src/storage/backup/segment/tests.rs::metadata_survives_the_conversion_both_ways`
  - `src/storage/backup/segment/tests.rs::one_message_is_read_by_range_without_downloading_the_segment`

---

### UC-BKP-007

**Caso:** O **diário** guarda o último estado de cada mensagem alterada (contador, prazo, recibo, dead-letter) ou a remoção; o estado do diário sobrescreve o da cópia do segmento; um diário ilegível é um erro, não um pânico.  
**Nível:** U  
**Testes:**
  - `src/storage/backup/journal/tests.rs::a_broken_journal_is_an_error_not_a_panic`
  - `src/storage/backup/journal/tests.rs::a_journal_round_trips_states_and_removals`
  - `src/storage/backup/journal/tests.rs::a_state_record_overwrites_the_delivery_fields_of_the_segment_copy`

---

### UC-BKP-008

**Caso:** O `_backup.db` guarda o `outbox` (só IDs, do mais antigo ao mais novo, sem repetir), as referências das mensagens salvas, os segmentos que o nó gravou com quantas mensagens vivas, e o contador de sequência (que sobrevive ao reinício).  
**Nível:** U  
**Testes:**
  - `src/storage/backup/db/tests.rs::a_segment_is_reported_dead_only_when_its_last_message_dies`
  - `src/storage/backup/db/tests.rs::committing_a_segment_records_the_references_and_empties_the_outbox`
  - `src/storage/backup/db/tests.rs::sequence_numbers_never_repeat_and_a_topic_can_be_forgotten`
  - `src/storage/backup/db/tests.rs::the_outbox_keeps_only_ids_and_hands_them_out_oldest_first`

---

### UC-BKP-009

**Caso:** O **commit** de uma mensagem só insere uma linha no `outbox`  
**Então:** o motor apenas avisa o backup (`stored`) e **nunca espera o S3**.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::publishing_only_tells_the_backup_and_never_waits_for_it`
  - `src/storage/backup/db/tests.rs::the_outbox_keeps_only_ids_and_hands_them_out_oldest_first`
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down`

---

### UC-BKP-010

**Caso:** A mensagem confirmada e consumida **antes de `backup.delay`** nunca vai ao S3 (sai do `outbox` sem gerar upload); as que passam da janela sobem.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`

---

### UC-BKP-011

**Caso:** O segmento fecha pelo que vier primeiro  
**Então:** tamanho, **quantidade de mensagens** ou idade (`backup.segment.*`).  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::a_segment_closes_by_message_count_before_it_gets_old`

---

### UC-BKP-012

**Caso:** Só **depois do PUT confirmar** as linhas do lote saem do `outbox` e ganham referência; o S3 que falha deixa tudo como está, e o envio tenta de novo quando ele volta.  
**Nível:** U, C  
**Testes:**
  - `src/storage/backup/db/tests.rs::committing_a_segment_records_the_references_and_empties_the_outbox`
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down`

---

### UC-BKP-013

**Caso:** O estado mutável vai num **diário** a cada `backup.meta.interval`, só o último estado de cada mensagem; uma entrega é uma mudança de estado e um **ack é uma remoção** (nunca se apaga uma mensagem solta do bucket). Só entram no diário as mensagens já salvas, e a que morre **durante o upload** do segmento dela entra como remoção.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::a_delivery_is_a_state_change_and_an_ack_is_a_removal`
  - `src/storage/backup/db/tests.rs::a_message_that_died_during_the_upload_is_not_alive_and_is_journaled_as_removed`
  - `src/storage/backup/db/tests.rs::only_saved_messages_enter_the_journal_and_a_death_is_a_removal`
  - `tests/backup.rs::the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment`

---

### UC-BKP-014

**Caso:** O primário apaga um segmento do bucket **somente quando todas as mensagens dele morreram** (ack, `ttl`, dead-letter); parcialmente morto, fica.  
**Nível:** U, C  
**Testes:**
  - `src/storage/backup/db/tests.rs::a_message_that_died_during_the_upload_is_not_alive_and_is_journaled_as_removed`
  - `src/storage/backup/db/tests.rs::a_segment_is_reported_dead_only_when_its_last_message_dies`
  - `tests/backup.rs::the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment`

---

### UC-BKP-015

**Caso:** **Todas** as filas entram no backup  
**Então:** as voláteis e as dead-letter também.  
**Nível:** C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::a_message_that_goes_to_the_dead_letter_is_removed_from_the_queue_and_stored_in_the_other`
  - `tests/backup.rs::volatile_queues_and_dead_letters_are_backed_up_too`

---

### UC-BKP-016

**Caso:** A definição de tópico é gravada pelo **nó gestor** em `topics/<nome>.json`; com o S3 fora do ar a alteração **vale mesmo assim** (nunca é recusada) e é regravada quando ele volta.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change`

---

### UC-BKP-017

**Caso:** A remoção de tópico grava uma **lápide** na definição e apaga, em segundo plano, os segmentos e diários do `topicId`.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::removing_a_topic_leaves_a_tombstone_and_clears_its_messages_from_the_bucket`

---

### UC-BKP-018

**Caso:** Cada token vai num `tokens/<id>.json` com o hash salgado (**nunca o segredo**); a revogação apaga o objeto.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change`
  - `tests/backup.rs::tokens_go_to_the_bucket_one_file_each_and_a_revoked_token_disappears`

---

### UC-BKP-019

**Caso:** Cada nó grava `nodes/<id>.json` com id, ips, porta, estado e um `seq` que cresce a cada batimento; ao sair limpo (drain) remove o próprio arquivo e para de bater.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/backup/tests.rs::the_node_writes_its_own_heartbeat_with_a_growing_sequence`
  - `tests/backup.rs::every_node_beats_in_the_bucket_with_a_growing_sequence_and_leaves_on_drain`

---

### UC-BKP-020

**Caso:** Quem observa **não compara horários**  
**Então:** guarda o ETag de cada batimento e o instante (monotônico) em que ele mudou; o nó está **morto** quando o ETag não muda por `backup.dead-after`, e um arquivo visto pela primeira vez conta como fresco; sem arquivo, o nó está morto; com o S3 ilegível o resultado é **desconhecido** (vale o comportamento sem o backup).  
**Nível:** U  
**Testes:**
  - `src/cluster/backup/tests.rs::a_heartbeat_is_fresh_while_its_etag_changes_and_dead_when_it_stops`
  - `src/cluster/backup/tests.rs::a_node_without_a_file_is_dead_and_an_unreadable_bucket_says_nothing`
  - `tests/backup.rs::an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires`

---

### UC-BKP-021

**Caso:** Um nó **inalcançável com batimento fresco** está vivo em outra partição  
**Então:** a votação **não o exclui** até o batimento vencer; ao vencer, é excluído.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires`

---

### UC-BKP-022

**Caso:** **Três de cinco** nós caem ao mesmo tempo  
**Então:** os dois sobreviventes excluem os mortos depois de o batimento vencer, e as mensagens que só existiam nos mortos são **buscadas no S3** (só vira perda se o S3 também não tiver).  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`
  - `tests/backup.rs::three_of_five_nodes_fall_and_the_survivors_find_the_missing_messages_in_s3`

---

### UC-BKP-023

**Caso:** O nó que sobe e **não acha ninguém** batendo (todos vencidos) é o **sobrevivente único**  
**Então:** começa um cluster de um nó e restaura. Um sobrevivente **em execução** exclui os mortos sozinho, mesmo abaixo de `cluster.vote.min-voters`.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/backup/tests.rs::a_node_with_no_cluster_in_the_bucket_only_starts_alone_if_it_is_the_first_or_the_stop_list_says_so`
  - `src/cluster/backup/tests.rs::the_verdict_tells_a_live_cluster_from_a_dead_one_from_an_empty_bucket`
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::a_single_survivor_excludes_the_dead_even_below_the_minimum_of_voters`
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`

---

### UC-BKP-024

**Caso:** Se algum nó `member` bate no bucket, o nó que sobe **não está sozinho** e segue pelo caminho normal; um nó que também está subindo (`joining`) não prova que o cluster existe; sem rastro de cluster, só `first-node` (ou a lista da parada) autoriza começar sozinho.  
**Nível:** U  
**Testes:**
  - `src/cluster/backup/tests.rs::a_node_with_no_cluster_in_the_bucket_only_starts_alone_if_it_is_the_first_or_the_stop_list_says_so`
  - `src/cluster/backup/tests.rs::only_one_survivor_gets_the_restore_lock_and_the_other_waits_as_a_normal_node`
  - `src/cluster/backup/tests.rs::the_verdict_tells_a_live_cluster_from_a_dead_one_from_an_empty_bucket`

---

### UC-BKP-025

**Caso:** Dois restauradores ao mesmo tempo  
**Então:** a **trava** `restore/lock.json` (criada com `If-None-Match`) dá a vitória a um só; o outro espera e entra como nó normal; a trava vence por `backup.restore.lock-ttl` se o titular some. Num S3 **sem escrita condicional** o nó avisa que a trava não é segura e segue.  
**Nível:** U  
**Testes:**
  - `src/cluster/backup/tests.rs::only_one_survivor_gets_the_restore_lock_and_the_other_waits_as_a_normal_node`
  - `src/cluster/backup/tests.rs::without_conditional_writes_the_lock_is_unsafe_but_the_node_still_starts`
  - `src/storage/backup/store/tests.rs::a_store_without_conditional_writes_says_so_and_one_that_is_down_fails`
  - `tests/s3_store.rs::conditional_writes_use_if_none_match_and_if_match_and_a_412_is_not_an_error`

---

### UC-BKP-026

**Caso:** A restauração devolve definições, tokens (menos os vencidos) e mensagens  
**Então:** segmentos mais o último estado do diário; as mensagens que o diário removeu são descartadas.  
**Nível:** U, C  
**Testes:**
  - `src/cluster/backup/restore.rs::the_last_journal_state_wins_and_removals_drop_the_message`
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`

---

### UC-BKP-027

**Caso:** A limpeza de `nodes/` apaga só os arquivos de ids que **não são membros** e cujo batimento **venceu**; um arquivo fresco nunca é apagado (pode ser um nó vivo em outra partição).  
**Nível:** U  
**Testes:**
  - `src/cluster/backup/tests.rs::cleaning_nodes_removes_only_dead_files_of_nodes_that_are_not_in_the_cluster`

---

### UC-BKP-028

**Caso:** Com o disco acima de 85% o nó apaga o `.body` das mensagens que já têm referência no bucket, até 70%; a linha em `_topic.db` fica.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket`

---

### UC-BKP-029

**Caso:** A leitura de uma mensagem cujo corpo foi evictado busca o corpo no segmento por `Range`; sem o backup, o corpo sumido continua sendo perda.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::a_body_the_eviction_removed_from_disk_comes_back_from_the_backup`
  - `tests/backup.rs::a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket`

---

### UC-BKP-030

**Caso:** Durante o flush e o drain, ao chegar a 90% a passagem da memória para o disco **pausa** até a evicção liberar espaço (no máximo `backup.drain.timeout`).  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::the_drain_asks_the_backup_before_each_memory_to_disk_write`
  - `tests/backup.rs::at_ninety_percent_the_memory_to_disk_pass_pauses_until_the_disk_has_room`

---

### UC-BKP-031

**Caso:** O **drain** manda ao S3, com prioridade, tudo o que está em disco e o `outbox` **antes** de o nó sair do cluster; o nó só chega a `drained` depois do envio (ou do prazo) e remove o próprio batimento.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`

---

### UC-BKP-032

**Caso:** `POST /node/backup/flush` faz memória → disco, disco → S3 e `outbox` → S3, **sem desregistrar nem parar o nó**, e responde um relatório (`complete`).  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::the_node_flush_sends_everything_without_stopping_the_node`

---

### UC-BKP-033

**Caso:** `POST /cluster/backup/flush` repassa a todos os nós e junta as respostas; um nó lento aparece como pendente.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending`

---

### UC-BKP-034

**Caso:** `POST /cluster/stop` faz o drain em **todos** os nós e o coordenador grava `cluster/stop.json` com os nós vivos e a ordem em que pararam; com o S3 indisponível ainda faz o drain em disco e responde com **aviso** de que o backup não foi enviado.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent`

---

### UC-BKP-035

**Caso:** **Iniciar o cluster** é subir os processos  
**Então:** a restauração acontece sozinha, o primeiro nó usa a lista da parada para não esperar o prazo de morte, a trava e a lista são soltas ao fim. Uma queda (sem lista) espera o prazo de `backup.dead-after`.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`

---

### UC-BKP-036

**Caso:** O primário avisa as réplicas, em lote, de quais mensagens já estão no bucket; a réplica só guarda a referência do que ela tem; a mensagem que morre numa réplica esquece a referência sem tocar no diário nem na contagem do segmento (são do dono).  
**Nível:** U, C  
**Testes:**
  - `src/storage/backup/db/tests.rs::a_replica_keeps_the_reference_but_never_the_journal_or_the_segment_count`
  - `tests/backup.rs::the_primary_tells_the_replicas_which_messages_are_already_in_the_bucket`

---

### UC-BKP-037

**Caso:** Quando o primário morre ou deixa de ser o dono, o **herdeiro adota** o que ainda não está no bucket e o envia depois de `backup.delay`; o antigo dono esquece.  
**Nível:** U, C  
**Testes:**
  - `src/engine/messages/tests/backup.rs::a_replica_becomes_the_owner_and_adopts_and_the_old_owner_releases`
  - `tests/backup.rs::when_the_primary_dies_the_new_primary_sends_what_was_still_pending`

---

### UC-BKP-038

**Caso:** `GET /node/backup` mostra o estado do backup (S3 no ar, fila de envio, segmentos, o que ainda falta  
**Então:** `pending`).  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending`

---

### UC-BKP-039

**Caso:** Com o S3 fora do ar na parada o envio **não bloqueia para sempre**  
**Então:** passado `backup.drain.timeout` o nó termina o drain mesmo assim.  
**Nível:** C  
**Testes:**
  - `tests/backup.rs::with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent`

---

### UC-BKP-040

**Caso:** Um nó só (perfil de um nó) é dono de tudo e também faz o backup.  
**Nível:** I  
**Testes:**
  - `tests/backup.rs::a_single_node_owns_everything_and_backs_up_too`

---

### UC-BKP-041

**Caso:** Um nó termina de restaurar suas vidas e o S3 guarda mensagens vivas dos blocos de que ele é **primário** e que ninguém tem
**Então:** a varredura as traz de volta pelo caminho da restauração (tópico, mortos, quórum) e as deixa disponíveis; o que o nó já tem é pulado, e uma segunda passada não acrescenta nada.
**Nível:** C
**Testes:**
  - `tests/scan_s3.rs::the_scan_brings_back_the_messages_of_the_bucket_that_no_node_has`

---

### UC-BKP-042

**Caso:** O S3 guarda uma mensagem que o diário já registra como removida, ou que não tem remoção no diário mas que o log de excluídos do nó diz que morreu
**Então:** a varredura a descarta nos dois casos: o S3 não é prova de vida e não alarga a janela do log.
**Nível:** C
**Testes:**
  - `tests/scan_s3.rs::the_scan_discards_what_the_journal_removed_and_what_the_log_says_is_dead`

---

### UC-BKP-043

**Caso:** O S3 não responde na partida
**Então:** a varredura é pulada com um aviso e o nó segue só com o disco, servindo normalmente.
**Nível:** C
**Testes:**
  - `tests/scan_s3.rs::with_s3_down_the_scan_is_skipped_and_the_node_goes_on_with_its_disk`

---

### UC-BKP-044

**Caso:** A varredura consulta o diário de um tópico
**Então:** lê o diário **uma vez por tópico** e o guarda para a passada seguinte (a consulta distingue "removida" de "não encontrada" porque só o que o diário remove sai do conjunto).
**Nível:** C
**Testes:**
  - `tests/scan_s3.rs::the_scan_reads_the_journal_of_a_topic_once_and_keeps_it_for_the_next_pass`

---

### UC-BKP-045

**Caso:** O balde do S3 responde devagar durante a varredura da partida
**Então:** só as filas **FIFO** esperam (a passada que as segura inclui a varredura, e o aviso de "terminei" só sai no fim dela); o resto do cluster fica disponível, a fila comum publica e entrega, e quando o balde responde a varredura termina, o cluster fica restaurado e a FIFO é liberada.
**Nível:** C
**Testes:**
  - `tests/cold_start.rs::a_slow_bucket_holds_only_the_fifo_queues_the_rest_of_the_cluster_is_available`

---

## CHS: Caos com histórico verificado

### UC-CHS-001

**Caso:** Um cluster de três nós leva uma sequência de falhas sorteada de uma semente (um nó isolado, um par cortado nos dois sentidos ou em um só, respostas perdidas, 40% de perda em um enlace, 15 ms de atraso, um nó mudo, o relógio saltando 8 s, uma de cada vez) enquanto clientes publicam numa fila comum e numa fila FIFO com três grupos, consomem e dão ack  
**Então:** depois de curadas as falhas e esvaziadas as filas, o histórico gravado não tem violação de I1 (sem perda), I2 (sem fantasma), I3 (ack definitivo), I4 (ordem FIFO e um só armazenamento) nem I6 (toda perda declarada) ([37-garantias.md](37-garantias.md)); a rodada também prova que fez algo (mensagens confirmadas e entregues). A mesma semente sorteia as mesmas falhas; `TABA_CHAOS_SEEDS=de..até` varre uma faixa de sementes (a rodada noturna)  
**Nível:** C  
**Testes:**
  - `tests/chaos.rs::seed_1_keeps_the_guarantees`
  - `tests/chaos.rs::seed_2_keeps_the_guarantees`
  - `tests/chaos.rs::seed_3_keeps_the_guarantees`
  - `tests/chaos.rs::seed_4_keeps_the_guarantees`
  - `tests/chaos.rs::a_range_of_seeds_keeps_the_guarantees`

---

### UC-CHS-002

**Caso:** O verificador do histórico recebe um histórico que quebra uma garantia (uma publicação confirmada nunca entregue e não declarada, uma entrega que ninguém publicou, uma entrega depois de um ack `204`, um grupo FIFO fora de ordem ou guardado duas vezes)  
**Então:** ele aponta a violação com o código da garantia (`I1/I6`, `I2`, `I3`, `I4`); um histórico limpo, uma perda declarada e uma publicação indeterminada que foi entregue não são violações; o nemesis sorteia as mesmas falhas da mesma semente  
**Nível:** U  
**Testes:**
  - `tests/chaos.rs::a_clean_history_has_no_violation`
  - `tests/chaos.rs::a_confirmed_publication_never_delivered_is_a_loss_unless_declared`
  - `tests/chaos.rs::a_delivery_without_a_publication_is_a_phantom`
  - `tests/chaos.rs::a_message_delivered_after_its_ack_is_a_violation`
  - `tests/chaos.rs::a_group_delivered_out_of_order_or_stored_twice_is_a_violation`
  - `tests/chaos.rs::the_same_seed_draws_the_same_faults`

---

### UC-CHS-003

**Caso:** Um teste programa uma falha de rede: um corte em um sentido só, respostas que se perdem depois de a chamada rodar, perda num enlace sorteada de uma semente, ou a cura de tudo de uma vez  
**Então:** só o sentido programado falha (o outro ainda chega); uma chamada com a resposta perdida roda no destino e falha para quem chamou como erro de rede; a mesma semente perde as mesmas chamadas e outra semente perde outras; `heal_all` remove toda falha programada  
**Nível:** U  
**Testes:**
  - `src/cluster/transport/memory/tests.rs::a_one_way_cut_stops_one_direction_only_and_heals`
  - `src/cluster/transport/memory/tests.rs::a_lost_reply_runs_the_call_and_fails_the_caller`
  - `src/cluster/transport/memory/tests.rs::the_loss_of_a_link_repeats_from_the_seed`
  - `src/cluster/transport/memory/tests.rs::heal_all_removes_every_programmed_failure`

---

### UC-CHS-004

**Caso:** Uma fila com `maxDeliveries: 2` e a sua dead-letter rodam dentro da rodada de caos, com um consumidor que nunca dá ack nas mensagens veneno (`seq % 3 == 0`)
**Então:** toda mensagem veneno confirmada termina na dead-letter e não é entregue de novo pela sua fila depois disso; uma mensagem confirmada na sua fila nunca chega à dead-letter; o verificador também exige no máximo `maxDeliveries` entregas dessa fila. Um commit de entrega que chega ao dono depois de a própria invisibilidade acabar não devolve à lista de prontas uma mensagem que gastou as entregas: a próxima escolha a esconde por um instante e a próxima liberação a manda para a dead-letter ([D180](30-decisoes.md))  
**Nível:** C, U  
**Testes:**
  - `src/engine/messages/tests/cluster.rs::a_commit_that_arrives_after_its_invisibility_does_not_deliver_past_the_limit`
  - `tests/chaos.rs::a_poison_message_that_reaches_the_dead_letter_after_its_deliveries_is_clean`
  - `tests/chaos.rs::a_poison_message_that_never_reaches_the_dead_letter_is_a_violation`
  - `tests/chaos.rs::a_message_delivered_over_its_limit_is_a_violation`
  - `tests/chaos.rs::a_message_in_both_places_is_a_violation`
  - `tests/chaos.rs::seed_1_keeps_the_guarantees`
  - `tests/chaos.rs::seed_2_keeps_the_guarantees`
  - `tests/chaos.rs::seed_3_keeps_the_guarantees`
  - `tests/chaos.rs::seed_4_keeps_the_guarantees`
  - `tests/chaos.rs::a_range_of_seeds_keeps_the_guarantees`

---

### UC-CHS-005

**Caso:** O coordenador decidiu o commit de um ack, um nó perdeu o register (expirou enquanto o nó estava isolado) e o coordenador o registra de novo antes do commit, quando o recibo da entrega já expirou
**Então:** o register enviado de novo não confere o recibo (o ack vai como `discard`), e o dono aplica o ack que os outros nós já aplicaram; a mensagem não fica só no dono, e o grupo FIFO dela não trava ([D180](30-decisoes.md)). A rodada de caos confere de ponta a ponta (I1: toda mensagem confirmada é entregue)
**Nível:** U, C
**Testes:**
  - `src/cluster/protocol/tests.rs::the_register_repeated_after_the_decision_checks_no_receipt`
  - `tests/chaos.rs::seed_1_keeps_the_guarantees`
  - `tests/chaos.rs::seed_2_keeps_the_guarantees`
  - `tests/chaos.rs::seed_3_keeps_the_guarantees`
  - `tests/chaos.rs::seed_4_keeps_the_guarantees`

---

## BNC: Benchmark comparativo local (`bench/run.sh`)

### UC-BNC-001

**Caso:** O `bench/run.sh --dry-run` é executado com um perfil, alguns sistemas e um nível de durabilidade; ou com um sistema, um perfil, um workload ou uma durabilidade que não existe
**Então:** ele imprime o plano (sistemas, workloads, durabilidade, o limite de cada servidor, igual para todos, a parte do executor do OMB, a estimativa de tempo e a ordem dos sistemas em cada rodada, em rodízio) e não sobe nada; o nome desconhecido é recusado com uma mensagem e status diferente de zero. `--cpus` e `--memory` trocam o limite de cada servidor ([38-benchmarks.md](38-benchmarks.md))
**Nível:** U
**Testes:**
  - `bench/run.test.mjs::o dry-run imprime o plano, a estimativa e a ordem em rodízio, sem tocar no Docker`
  - `bench/run.test.mjs::o servidor recebe o mesmo limite para todos os sistemas e o executor fica com o resto`
  - `bench/run.test.mjs::um sistema, um perfil, um workload ou uma durabilidade desconhecidos são recusados`

---

### UC-BNC-002

**Caso:** Um sistema entra como uma pasta em `bench/systems/`; os workloads e os perfis são arquivos de `bench/`
**Então:** toda pasta tem um `compose.yml` (um serviço chamado `server`, healthcheck, os limites `BENCH_CPUS` e `BENCH_MEMORY`, imagem fixada), os arquivos `env-` e `driver-` dos dois níveis de durabilidade e um README; o `--systems <nome>` o encontra sem mudar o `run.sh`; cada workload recebe a duração e o aquecimento do perfil, e o perfil `standard` roda os seis
**Nível:** U
**Testes:**
  - `bench/run.test.mjs::cada pasta de bench/systems é um sistema completo, com imagem fixada e os dois níveis de durabilidade`
  - `bench/run.test.mjs::um sistema novo é encontrado pela pasta, sem mudar o run.sh`
  - `bench/run.test.mjs::os seis workloads usam a duração e o aquecimento do perfil`

---

### UC-BNC-003

**Caso:** O `bench/report.mjs` lê a pasta de uma execução (o JSON do OMB de cada rodada, `machine.json`, `failed.txt`)
**Então:** o `results.md` abre com a faixa "local, não representativo", mostra por workload e durabilidade a mediana das rodadas com o mínimo e o máximo (sem intervalo para uma rodada), lista as células que falharam, termina com as diferenças conhecidas de cada sistema e não aponta vencedor; os gráficos de latência e vazão saem em SVG
**Nível:** U
**Testes:**
  - `bench/report.test.mjs::a mediana e os nomes dos arquivos`
  - `bench/report.test.mjs::o relatório abre com a faixa de máquina local`
  - `bench/report.test.mjs::a tabela traz a mediana e o intervalo das rodadas`
  - `bench/report.test.mjs::os gráficos SVG saem e a lista das falhas aparece`
  - `bench/report.test.mjs::o relatório não elege vencedor e anota o RabbitMQ`

---

## Fontes e provas

- Os casos vieram de `doc/20a`, `20b` e `20c` (o catálogo histórico, removido no commit `efeb47a`); os testes, das linhas `/// UC:` do código. O teste `crates/taba-node/tests/uc_coverage.rs` confere os códigos.
- Os casos `UC-BKP` (backup em S3) não vêm do catálogo histórico: foram escritos à mão neste arquivo, no mesmo formato, e a lista de testes de cada um é conferida por `uc_coverage`.
- Os casos `UC-BNC` (benchmark local) foram escritos à mão neste arquivo; os testes deles são arquivos Node.js em `bench/`, que o `uc_coverage` também lê.
- Mantido à mão: um teste novo que cita um caso entra no cartão dele ([26-casos-de-uso.md](26-casos-de-uso.md)).
