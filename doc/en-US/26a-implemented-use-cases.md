---
title: Implemented use cases
summary: The catalogue of Taba use cases that have at least one test citing them, by area, with the case, the level and the tests that prove it.
type: reference
status: stable
verified-on: 2026-10-15 @ 56121c9
sources: [crates/taba-node/tests, crates/taba-node/src, connectors/rust, connectors/csharp]
---

# Implemented use cases

> **Maintained by hand.** Each card carries the case, the level and the tests that cite it (the `/// UC: UC-XXX-NNN` lines of the code). Format of the codes and coverage rule: [26-use-cases.md](26-use-cases.md). The cases **without a test** are in [26b-planned-use-cases.md](26b-planned-use-cases.md). This file is the English rendering of `doc/pt-BR/26a-casos-de-uso-implementados.md`: codes, levels and test names (`file::function`) are the same; only the prose of each case is translated.

**Scoreboard (2026-10-15, commit 56121c9, plus 15 cases of the server work for the TUI):** **817** of **909** cases have at least one test (89.9%); **92** do not; **42** are retired (the code is not reused).

Level: **U** unit, **I** a real node, **C** simulated cluster, **K** connector. Each case is a block: the **code** in the heading; below, **Case** (the situation), **Then** (the expected result), **Level** and **Tests** (`file::function`, relative to `crates/taba-node`, or to the repository root for the connectors).

## Coverage by area

| Area | Subject | With test | Without test |
|---|---|---:|---:|
| `TOP` | Topics: create, list, read, change and remove | 46 | 3 |
| `MSG` | Publishing and the message body | 19 | 1 |
| `CNS` | Consumption (`GET`), long polling and `autoCreate` | 39 | 1 |
| `ACK` | Confirmation of consumption | 13 | 0 |
| `VIS` | Change of invisibility | 10 | 2 |
| `RET` | Expiration (`ttl`), delivery limit and dead-letter | 18 | 2 |
| `DST` | Distributor Topic | 17 | 1 |
| `VOL` | Volatile queue and managing node | 23 | 0 |
| `FIF` | FIFO queue | 53 | 0 |
| `GRP` | Message groups and fair delivery | 17 | 0 |
| `AUT` | Authentication and access tokens | 34 | 5 |
| `LIM` | Rate limit and client IP | 13 | 2 |
| `PAG` | Pagination | 8 | 1 |
| `CNV` | Common API conventions (format, headers, errors, status) | 14 | 3 |
| `CNT` | Connectors | 35 | 0 |
| `CLI` | `taba-cli`, the client command line | 36 | 0 |
| `ESC` | Write: register + commit, quorum, rollback, commit ceiling | 29 | 9 |
| `ANE` | Hash ring, blocks and roles | 8 | 4 |
| `DEF` | Topic and token definition replicated in the cluster | 26 | 8 |
| `ENT` | Delivery on the owner: ready list, `take`, long polling | 18 | 13 |
| `CAN` | Permanent channel between nodes and signal queues | 28 | 0 |
| `INV` | Invisibility on the owner and on the replicas | 26 | 6 |
| `ARM` | Storage: memory, overflow, SQLite, integrity | 24 | 5 |
| `ING` | Node join (bootstrap) | 33 | 4 |
| `FAL` | Failure detection, exclusion vote and exit by failure | 32 | 1 |
| `FRZ` | Freezing period | 13 | 1 |
| `CNC` | Concurrency between membership changes | 12 | 3 |
| `SIN` | Distributed synchronization of blocks | 31 | 1 |
| `STR` | NDJSON streams between nodes | 8 | 7 |
| `RTN` | Return of an excluded node and recovery of losses | 45 | 0 |
| `FWD` | Forwarding of client calls | 4 | 8 |
| `DRN` | Stop and drain mode | 19 | 2 |
| `DEV` | One-node profile (the former development mode) | 3 | 4 |
| `SEG` | Ports, mutual TLS and cluster certificates | 20 | 0 |
| `OPS` | Observability (OpenTelemetry), statistics, metrics and health | 44 | 7 |
| `CLR` | Contract of the cluster routes (DTOs and common rules) | 8 | 2 |
| `CFG` | Configuration and startup validations | 11 | 2 |
| `BKP` | Optional S3 backup | 45 | 0 |

## TOP: Topics: create, list, read, change and remove

### UC-TOP-001

**Case:** Create a queue (`type: queue`) with a valid name  
**Then:** `201`, `Location: /topics/{name}`, `Topic` body with a GUID v7 `id`, `createdAt`, `tags: {}`, `volatile: false`, `fifo: false` and `effective` with the cluster defaults.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::creating_a_queue_returns_201_with_location_and_the_defaults`

---

### UC-TOP-002

**Case:** Create a distributor (`type: distributor`) with `prefixes`  
**Then:** `201`, and the `Topic` carries `prefixes` and does not carry the queue fields (`volatile`, `fifo`, `ttl`, `maxDeliveries`, `maxMessageBytes`, `effective`).  
**Level:** I  
**Tests:**
  - `tests/topics.rs::a_distributor_shows_prefixes_and_none_of_the_queue_fields`

---

### UC-TOP-003

**Case:** Create a volatile queue (`volatile: true`) without `idleTimeout`  
**Then:** the default of 600 s applies (`topics.volatile.idle-timeout.default`); with `idleTimeout` informed, the informed one applies.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-004

**Case:** Create a FIFO queue (`fifo: true`) with `replication.factor` of at least 2  
**Then:** `201` and `fifo: true` in the `Topic`.  
**Level:** I, C  
**Tests:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-005

**Case:** Create a queue with its own `ttl`, `maxDeliveries` and `maxMessageBytes`  
**Then:** the `Topic` carries the defined values and `effective` the same; without them, the three are `null` and `effective` carries the defaults (14 days, 10, 5 MB).  
**Level:** I  
**Tests:**
  - `tests/topics.rs::volatile_fifo_and_own_limits_come_back_as_defined`

---

### UC-TOP-006

**Case:** Every queue created gets, together, the `{name}-dead-letter` queue, which appears in `GET /topics` with `deadLetter: true` and `source`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-007

**Case:** Create with valid tags (up to 20; key of 1 to 64 and value of 0 to 128 characters of `A-Za-z0-9_-.:`)  
**Then:** `201` and the tags come back in the `Topic`.  
**Level:** I  
**Tests:**
  - `src/engine/topics/tests.rs::tags_at_the_limits_are_accepted_and_come_back_in_the_topic`

---

### UC-TOP-008

**Case:** Create with invalid tags (more than 20; empty key or longer than 64; value longer than 128; character outside the set)  
**Then:** `400` `validation.failed`, with the field in `details`.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::invalid_tags_fail_with_the_field_in_details`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-009

**Case:** Name outside `^[A-Za-z0-9_-]{1,100}$` (empty, 101 characters, space, accent, `/`)  
**Then:** `400` `validation.invalid-name`.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::topic_name_accepts_the_documented_charset_and_length`
  - `src/core/validation/tests.rs::topic_name_rejects_empty_long_and_foreign_characters`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-010

**Case:** Name ending in `-dead-letter`  
**Then:** `400` `validation.reserved-name`.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::topic_name_ending_in_the_dead_letter_suffix_is_reserved`
  - `src/engine/topics/tests.rs::name_and_tags_are_validated_with_the_field_in_details`
  - `tests/topics.rs::invalid_names_and_tags_are_400_with_the_field_in_details`

---

### UC-TOP-011

**Case:** The name is case-sensitive  
**Then:** `Pedidos` and `pedidos` coexist as distinct topics.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-012

**Case:** Create with a name that already exists  
**Then:** `409` `topic.already-exists`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-013

**Case:** Field that does not apply to the type (`prefixes` on a queue, `fifo`, `volatile`, `ttl`, `maxDeliveries` or `maxMessageBytes` on a distributor)  
**Then:** `400` `validation.not-applicable`.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::fields_that_do_not_apply_to_the_type_are_refused`
  - `tests/topics.rs::fields_out_of_place_or_out_of_range_are_400`

---

### UC-TOP-014

**Case:** `idleTimeout` outside 60 to 604800, `ttl` outside 60 to 31536000 or `maxDeliveries` outside 1 to 1000  
**Then:** `400` `validation.out-of-range`. The exact limits (59 and 60, 604800 and 604801 etc.) are accepted and refused as defined.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::ranges_are_inclusive_at_both_ends`
  - `src/engine/topics/tests.rs::idle_timeout_ttl_and_max_deliveries_have_exact_limits`
  - `tests/topics.rs::fields_out_of_place_or_out_of_range_are_400`

---

### UC-TOP-015

**Case:** `maxMessageBytes` above `message.hard-limit`  
**Then:** `400` `validation.max-bytes-above-hard-limit`. Above the general one (5 MB) and within the hard limit: accepted.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::max_message_bytes_may_exceed_the_default_but_not_the_hard_limit`
  - `tests/topics.rs::max_message_bytes_can_pass_the_default_but_not_the_hard_limit`

---

### UC-TOP-016

**Case:** Distributor with more than 20 prefixes, an empty prefix, one longer than 100 characters or with a character outside `A-Za-z0-9_-`  
**Then:** `400`.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::distributor_prefixes_follow_the_documented_limits`
  - `src/engine/topics/tests.rs::distributor_prefixes_are_validated`
  - `tests/topics.rs::distributor_prefixes_follow_the_limits`

---

### UC-TOP-017

**Case:** Distributor with an empty `prefixes` is accepted  
**Then:** does not copy the publication to anyone.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::a_distributor_shows_prefixes_and_none_of_the_queue_fields`

---

### UC-TOP-018

**Case:** Body with an unknown field  
**Then:** `400` `validation.unknown-field`. Without `name` or without `type`: `400` `validation.required`, with the field in `details`.  
**Level:** U, I  
**Tests:**
  - `tests/topics.rs::unknown_and_missing_fields_are_400_naming_the_field`

---

### UC-TOP-020

**Case:** Recreating a topic with the same name (after removing it) generates a different `id`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::names_are_case_sensitive_duplicates_conflict_and_recreating_changes_the_id`

---

### UC-TOP-021

**Case:** List without topics  
**Then:** `200` with `items: []` and no `nextCursor`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::listing_without_topics_is_an_empty_page`

---

### UC-TOP-022

**Case:** The listing comes ordered by name by default and includes the dead-letter queues.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-023

**Case:** `sort` by `name`, `id`, `createdAt` and each with `-` (descending); unknown `sort`  
**Then:** `400` `validation.failed`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::listing_sorts_and_filters`

---

### UC-TOP-024

**Case:** The `name` filter returns the topics whose name **contains** the text, case-sensitive.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::listing_sorts_and_filters`

---

### UC-TOP-026

**Case:** `GET /topics/{topic}` returns the definition; nonexistent topic  
**Then:** `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::reading_a_topic_returns_it_and_an_unknown_one_is_404`

---

### UC-TOP-027

**Case:** `GET` of the dead-letter returns `deadLetter: true`, `source` and the values of the source queue, read-only.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::every_queue_comes_with_a_read_only_dead_letter_in_the_listing`

---

### UC-TOP-028

**Case:** Partial `PATCH`  
**Then:** only the fields sent change. `tags`, when sent, **replaces the whole map**.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::patch_is_partial_tags_replace_and_null_goes_back_to_the_default`

---

### UC-TOP-029

**Case:** `null` in `ttl`, `maxDeliveries` or `maxMessageBytes` goes back to the cluster default (`effective` follows the default).  
**Level:** I  
**Tests:**
  - `tests/topics.rs::patch_is_partial_tags_replace_and_null_goes_back_to_the_default`

---

### UC-TOP-030

**Case:** A new `ttl` also applies to messages already stored, from the next sweep.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_new_ttl_applies_to_messages_already_stored`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-TOP-031

**Case:** A new `maxMessageBytes` only applies to the following publications; messages already accepted are not rejected.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-TOP-032

**Case:** `PATCH` of `prefixes` on a distributor applies from the commit and only to later publications.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::patching_distributor_prefixes_replaces_them`

---

### UC-TOP-034

**Case:** `PATCH` with an immutable field (`name`, `id`, `type`, `volatile`, `fifo`), **even with the current value**  
**Then:** `400` `validation.immutable-field`.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::patch_refuses_immutable_fields_even_with_the_current_value`
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-035

**Case:** `PATCH` with a field that does not apply to the type (`ttl` on a distributor)  
**Then:** `400` `validation.not-applicable`. Out of range or above the hard limit: `400`.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::patch_validates_applicability_and_ranges`
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-036

**Case:** `PATCH` on the dead-letter  
**Then:** `409` `topic.read-only`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::patch_refuses_immutable_misplaced_and_dead_letter_changes`

---

### UC-TOP-037

**Case:** `PATCH` on a nonexistent topic  
**Then:** `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::reading_a_topic_returns_it_and_an_unknown_one_is_404`

---

### UC-TOP-038

**Case:** Remove an empty topic  
**Then:** `204`; a later `GET` answers `404`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-039

**Case:** Remove a queue with ready messages, invisible ones (in processing) and without ack  
**Then:** `204`, always, with no check. All the messages disappear (memory, disk and metadata).  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests.rs::deleting_a_queue_drops_its_messages_and_those_of_its_dead_letter`
  - `src/engine/messages/tests.rs::removing_a_topic_removes_its_folder_with_database_and_bodies`
  - `tests/messages.rs::deleting_a_queue_drops_its_ready_and_in_flight_messages`

---

### UC-TOP-040

**Case:** Removing a queue also removes its dead-letter, with its messages.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-041

**Case:** `DELETE` on the dead-letter  
**Then:** `409` `topic.read-only`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-042

**Case:** `DELETE` of a nonexistent topic  
**Then:** `404` `topic.not-found` (including on the repetition after a `503` `cluster.commit-timeout`, which the client treats as success).  
**Level:** I, C  
**Tests:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-043

**Case:** Remove a distributor  
**Then:** `204`; the queues it copied to are not affected.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::delete_removes_the_topic_and_its_dead_letter`

---

### UC-TOP-044

**Case:** Prune on one node: `Messages::prune` empties the queue (visible and invisible messages, owned or replica, in memory or on disk)  
**Then:** It returns how many messages the node **owned**; the topic stays; a message published afterwards is delivered normally.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests/prune.rs::pruning_removes_the_visible_and_the_invisible_messages_and_counts_them`

---

### UC-TOP-045

**Case:** The same `pruneId` arrives again at a node  
**Then:** It answers the first count and removes nothing new (a message published after the first prune stays); a new `pruneId` prunes again.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests/prune.rs::a_repeated_prune_id_answers_the_same_and_removes_nothing_new`

---

### UC-TOP-046

**Case:** Prune of a topic that does not exist, or of a distributor  
**Then:** `404 topic.not-found`; `409 topic.wrong-type`.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests/prune.rs::pruning_an_unknown_topic_is_404_and_a_distributor_is_a_wrong_type`

---

### UC-TOP-047

**Case:** `DELETE /topics/{topic}/messages` on a cluster with messages visible and invisible, received by any node  
**Then:** `200` `{pruneId, removed, nodes}`: every node was emptied (nothing is delivered from any), `removed` counts each message once and not once per copy, the receiver prunes itself without the network, and the queue still publishes and delivers.  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::pruning_empties_the_queue_on_every_node_and_the_queue_keeps_working`

---

### UC-TOP-048

**Case:** Prune with a token without `manage`, or of a topic that does not exist  
**Then:** `403 auth.forbidden` (with `required: "manage"`); `404 topic.not-found`; with `manage` on the topic, `200`.  
**Level:** I  
**Tests:**
  - `tests/nodes_api.rs::pruning_an_unknown_topic_is_404_and_needs_the_manage_permission`

---

### UC-TOP-049

**Case:** A prune while a node does not answer  
**Then:** `503 cluster.prune-incomplete` naming that node (in the message and in `details`); the other nodes were emptied; repeating the call after the node returns empties it and answers `200`.  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::a_prune_that_fails_on_a_node_names_it_and_repeating_it_is_safe`

---


## MSG: Publishing and the message body

### UC-MSG-001

**Case:** Publish valid JSON, without `Content-Type`  
**Then:** `201`, `PublishResponse` with a GUID v7 `id`, and the message is left with `contentType: application/json`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::publishing_returns_increasing_guid_v7_ids`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`
  - `tests/messages.rs::the_json_body_comes_back_exactly_as_it_was_sent`

---

### UC-MSG-002

**Case:** Publish text (`text/plain; charset=utf-8`)  
**Then:** `201`, and the `contentType` keeps the `charset` parameter.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-003

**Case:** Publish binary (`image/png`)  
**Then:** `201`, and the bytes are stored exactly as they came, without reserializing or encoding.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-004

**Case:** `Content-Type` that is not a valid media type, or longer than 255 characters  
**Then:** `400` `validation.invalid-content-type`.  
**Level:** U, I  
**Tests:**
  - `src/core/content/tests.rs::rejects_what_is_not_a_media_type`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-005

**Case:** `application/json` (or `*/*+json`) body that is not valid JSON  
**Then:** `400` `validation.invalid-body`.  
**Level:** U, I  
**Tests:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-006

**Case:** `text/*` body that is not valid UTF-8  
**Then:** `400` `validation.invalid-body`.  
**Level:** U, I  
**Tests:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `src/engine/messages/tests.rs::publishing_validates_content_type_and_body`
  - `tests/messages.rs::invalid_content_type_and_invalid_bodies_are_400`

---

### UC-MSG-007

**Case:** A body of any other type is not validated (arbitrary bytes are accepted).  
**Level:** I  
**Tests:**
  - `src/core/content/tests.rs::only_json_and_utf8_text_are_validated`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-MSG-008

**Case:** The size counts the **bytes sent** (not the base64). A body of exactly `maxMessageBytes` is accepted; one byte more  
**Then:** `413` `message.too-large`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-MSG-009

**Case:** Without `maxMessageBytes` on the topic, `message.max-bytes` (5 MB) applies; a topic with a `maxMessageBytes` larger than the general one accepts larger messages, up to the hard limit.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::size_counts_bytes_against_the_topic_limit_or_the_general_one`
  - `tests/messages.rs::the_size_limit_counts_bytes_and_a_topic_may_exceed_the_general_one`

---

### UC-MSG-010

**Case:** Publish to a nonexistent topic  
**Then:** `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::unknown_topics_are_404`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-MSG-011

**Case:** Valid `groupId` (1 to 128 characters of `A-Za-z0-9_-.:`) on an ordinary queue  
**Then:** accepted and returned on the `GET`. Invalid (empty, more than 128, character outside the set): `400`.  
**Level:** U, I  
**Tests:**
  - `src/core/validation/tests.rs::message_keys_accept_1_to_128_chars_of_the_tag_set`
  - `src/engine/messages/tests.rs::group_id_is_validated_and_deduplication_is_fifo_only`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-MSG-012

**Case:** `deduplicationId` on a queue that is not FIFO  
**Then:** `400` `validation.not-applicable`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::group_id_is_validated_and_deduplication_is_fifo_only`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-MSG-013

**Case:** The `id` is generated by the coordinator node before the register, is a lowercase GUID v7 and sortable by time  
**Then:** two sequential publications have an increasing `id`.  
**Level:** U, I  
**Tests:**
  - `src/core/id/tests.rs::generator_is_strictly_increasing_even_within_the_same_millisecond`
  - `src/engine/messages/tests.rs::publishing_returns_increasing_guid_v7_ids`

---

### UC-MSG-014

**Case:** The response only goes out after the commit is confirmed on the quorum (the message is already deliverable at the instant the client receives the `id`).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-MSG-015

**Case:** Publishing through any node of the cluster works  
**Then:** the node that receives is the coordinator, and the message ends up on the nodes responsible for the block.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-MSG-016

**Case:** Without a quorum  
**Then:** `503` `cluster.quorum-unavailable` with `Retry-After`, and the message was **not** accepted (no `GET` sees it).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-MSG-018

**Case:** `PublishResponse` on an ordinary queue does not carry `duplicate`; the field only exists on the FIFO queue.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-MSG-019

**Case:** Publish with `X-Request-Id`  
**Then:** the response returns the same value.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-MSG-020

**Case:** The published message preserves the `groupId` and the `contentType` throughout the whole cycle (publishing, delivery, redelivery and dead-letter).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---


## CNS: Consumption (`GET`), long polling and `autoCreate`

### UC-CNS-001

**Case:** `GET` on a queue with messages  
**Then:** `200`, `messages` with `id`, `body`, `contentType`, `receiptToken`, `deliveryCount: 1`, `publishedAt` and `invisibleUntil`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::consuming_delivers_the_message_with_its_metadata`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`

---

### UC-CNS-002

**Case:** `GET` without ready messages  
**Then:** `200` with `messages: []`, **never** `204`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::consuming_delivers_the_message_with_its_metadata`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-003

**Case:** `maxMessages` not informed is 10 (`delivery.max-messages.default`); informed, returns up to that number; outside 1 to `delivery.max-messages.max`  
**Then:** `400` `validation.out-of-range`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::max_messages_defaults_to_10_and_must_be_in_range`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-004

**Case:** The `GET` returns what is ready, without waiting to fill `maxMessages` (with `waitTime` 0).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::max_messages_defaults_to_10_and_must_be_in_range`
  - `src/engine/messages/tests.rs::consuming_returns_what_is_ready_without_waiting_to_fill_the_batch`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-005

**Case:** `visibilityTimeout` not informed is 120 s; informed, the informed one applies; outside 0 to `delivery.visibility-timeout.max`  
**Then:** `400`. The returned `invisibleUntil` is `now + visibilityTimeout`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `src/engine/messages/tests.rs::visibility_timeout_zero_makes_the_message_deliverable_again_at_once`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-006

**Case:** A delivered message stays invisible  
**Then:** a second `GET` within the deadline does not return it.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-007

**Case:** Without an ack until the end of the deadline, the message is delivered again, with `deliveryCount` incremented and a **new** `receiptToken` (the previous one is no longer accepted).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_delivered_message_is_invisible_until_the_timeout_and_then_comes_back`
  - `tests/messages.rs::a_delivered_message_is_invisible_and_returns_with_a_new_receipt_after_the_timeout`

---

### UC-CNS-008

**Case:** A JSON body comes out as **native JSON** (object, list, text, number, boolean, `null`), exactly as it was sent, without `bodyEncoding`.  
**Level:** I  
**Tests:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::publishing_json_without_content_type_and_consuming_it_back`
  - `tests/messages.rs::the_json_body_comes_back_exactly_as_it_was_sent`

---

### UC-CNS-009

**Case:** A `text/*` body in UTF-8 (or without `charset`) comes out as a **string**, without `bodyEncoding`.  
**Level:** I  
**Tests:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-010

**Case:** A body of any other type, and `text/*` with another `charset`, comes out in **base64**, with `bodyEncoding: "base64"`.  
**Level:** I  
**Tests:**
  - `src/core/content/tests.rs::renders_json_text_and_base64`
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-011

**Case:** `*/*+json` (for example, `application/vnd.api+json`) is treated as JSON.  
**Level:** U, I  
**Tests:**
  - `src/core/content/tests.rs::json_kinds_include_plus_json_suffixes`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-012

**Case:** In a batch with JSON, text and binary, each message comes out in the form of its `contentType`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::each_message_keeps_its_content_type_and_group_in_a_mixed_batch`
  - `tests/messages.rs::a_mixed_batch_keeps_each_content_type_and_the_exact_bytes`

---

### UC-CNS-013

**Case:** There is no order guarantee between the messages of an ordinary queue; the test cannot require delivery order. The two ends of the GUID v7 are consumed interleaved (accordion effect).  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::inside_a_band_the_oldest_and_the_newest_alternate_like_an_accordion`

---

### UC-CNS-014

**Case:** The message only reaches the client after the invisibility is committed on the quorum.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-CNS-015

**Case:** A message expired by `ttl` (even before the sweep) is not delivered.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_delivered`
  - `tests/messages.rs::a_message_past_its_ttl_is_not_delivered`

---

### UC-CNS-016

**Case:** Consume from the dead-letter  
**Then:** ordinary `GET`, with `deadLetteredAt` and `deadLetterReason` on the messages.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_dead_letter_is_a_common_queue_for_publish_and_consume`
  - `tests/messages.rs::the_dead_letter_can_be_published_to_and_consumed`

---

### UC-CNS-017

**Case:** `GET` on a distributor  
**Then:** `409` `topic.wrong-type`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_cannot_be_consumed`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-CNS-018

**Case:** `GET` on a topic that does not exist, without `autoCreate`  
**Then:** `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::unknown_topics_are_404`
  - `src/engine/messages/tests.rs::deleting_the_topic_ends_a_long_poll_with_not_found`
  - `tests/messages.rs::unknown_topics_are_404_and_a_distributor_is_not_consumed`

---

### UC-CNS-019

**Case:** The invisibility register of a message fails and there are others ready  
**Then:** the rollback continues in another thread, the `GET` moves to the next ID without waiting, and the message that failed **never** enters the response.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-CNS-020

**Case:** At least one message captured and others failed  
**Then:** `200` with those it got (fewer than `maxMessages`).  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit`

---

### UC-CNS-021

**Case:** **No** message captured and there was a register failure  
**Then:** `503` `delivery.unavailable` with `Retry-After`, never an empty `200`.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-CNS-022

**Case:** The number of failures tolerated per `GET` is limited by `delivery.get.max-attempts`; when exhausted, UC-CNS-020 or UC-CNS-021 applies.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::a_get_tries_the_next_ids_after_a_failed_registration_up_to_the_attempt_limit`

---

### UC-CNS-023

**Case:** If the rollback does not reach a replica, the pending one expires by `write.pending.ttl` and the message stays ready as it was.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`

---

### UC-CNS-024

**Case:** `waitTime` from 0 to 20 is accepted (default 0, answers at once); outside the range  
**Then:** `400` `validation.out-of-range`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::wait_time_is_limited_to_0_20`
  - `tests/messages.rs::consume_parameters_are_checked`

---

### UC-CNS-025

**Case:** With `waitTime > 0` and an empty queue, the `GET` waits; a message published during the wait is returned **without** waiting for the end of the time.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::long_polling_returns_as_soon_as_a_message_arrives`
  - `src/engine/messages/tests.rs::long_polling_notices_a_message_whose_invisibility_ends`
  - `tests/messages.rs::long_polling_returns_as_soon_as_a_message_is_published`

---

### UC-CNS-026

**Case:** With `waitTime > 0` and no message arriving, it answers `200` with `messages: []` at the end of the time.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::long_polling_ends_with_an_empty_list`
  - `tests/messages.rs::long_polling_ends_with_an_empty_list_when_nothing_arrives`

---

### UC-CNS-027

**Case:** With long polling, the `GET` returns the first message that arrives, without waiting to fill `maxMessages`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::long_polling_returns_as_soon_as_a_message_arrives`
  - `tests/messages.rs::long_polling_returns_as_soon_as_a_message_is_published`

---

### UC-CNS-028

**Case:** The client disconnects during the wait  
**Then:** the listener is removed (no message is delivered to a dead connection, and the resource is released).  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_client_that_gives_up_does_not_swallow_the_next_message`

---

### UC-CNS-029

**Case:** Several `GET`s waiting on the same topic  
**Then:** each new message goes to the first in the listener queue, in order of arrival.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::waiting_gets_are_served_in_the_order_they_arrived`
  - `src/engine/messages/tests.rs::one_publish_with_several_messages_wakes_the_next_waiting_get_too`

---

### UC-CNS-030

**Case:** Long polling waiting on one node and a message published on another node of the cluster  
**Then:** the consumer receives the message (notice between nodes + `take`).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-CNS-031

**Case:** `autoCreate=true` on a name that does not exist creates a **volatile** queue by default (`volatile` default `true`), with `idleTimeout` 600 s, and already counts as a consumer.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_makes_a_volatile_queue_with_the_default_idle_timeout`
  - `src/engine/messages/tests.rs::without_auto_create_a_missing_topic_is_404_and_a_bad_flag_is_400`
  - `tests/volatile.rs::auto_create_on_get_makes_a_volatile_queue_and_the_get_counts`

---

### UC-CNS-032

**Case:** `autoCreate` with `volatile=false`, `idleTimeout`, `fifo=true` and `tag.<key>=<value>` creates the queue with those attributes.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_takes_the_creation_attributes_from_the_query`
  - `tests/volatile.rs::auto_create_takes_idle_timeout_fifo_volatile_and_tags_from_the_query`

---

### UC-CNS-033

**Case:** `autoCreate` on a topic that already exists and is a queue  
**Then:** the creation parameters are **ignored** and the `GET` is an ordinary consumption.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_on_an_existing_queue_ignores_the_creation_parameters`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-034

**Case:** `autoCreate` on a name that belongs to a distributor  
**Then:** `409` `topic.wrong-type`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_never_makes_a_distributor_and_refuses_bad_names`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-035

**Case:** `autoCreate` with a name ending in `-dead-letter`  
**Then:** `400` `validation.reserved-name`. Invalid name: `400` `validation.invalid-name`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_never_makes_a_distributor_and_refuses_bad_names`
  - `tests/volatile.rs::auto_create_is_idempotent_never_makes_a_distributor_and_refuses_bad_names`

---

### UC-CNS-036

**Case:** `autoCreate` that hits `topics.volatile.max-count`  
**Then:** `422` `topic.volatile-limit`. Without the ceiling defined, there is no limit.  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests.rs::auto_create_stops_at_the_volatile_ceiling_but_post_does_not`
  - `tests/volatile.rs::the_volatile_ceiling_refuses_auto_create_with_422`

---

### UC-CNS-037

**Case:** `autoCreate` of a FIFO queue with `replication.factor` 1 (the one-node profile)  
**Then:** the queue is created and takes messages: the FIFO quorum is `min(2, factor)` (`topic.fifo-needs-replication` is no longer returned).  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-CNS-038

**Case:** Several consumers with `autoCreate` at the same time on the same name  
**Then:** one creates, the others treat it as "already exists" and continue as an ordinary consumption, with no `409` error.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::consumers_racing_to_auto_create_the_same_name_all_succeed`

---

### UC-CNS-040

**Case:** `autoCreate` requires the `consume` **and** `create` permissions (with authentication on).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::creating_by_auto_create_needs_the_create_permission_but_consuming_does_not`
  - `tests/volatile.rs::auto_create_needs_consume_and_create_but_a_plain_consume_only_needs_consume`
  - `tests/volatile.rs::auto_create_stays_inside_the_token_prefixes`

---


## ACK: Confirmation of consumption

### UC-ACK-001

**Case:** Ack with the `receiptToken` of the last delivery  
**Then:** `204`, and the message disappears from the topic (no `GET` returns it, not even after the deadline).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::ack_removes_the_message_and_a_second_ack_is_not_found`
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-002

**Case:** Ack without `receiptToken` in the query  
**Then:** `400` `validation.required`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::missing_fields_are_reported_together`
  - `tests/messages.rs::ack_validates_the_request_and_the_topic`

---

### UC-ACK-003

**Case:** A second ack of the same message  
**Then:** `404` `message.not-found`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::ack_removes_the_message_and_a_second_ack_is_not_found`
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-004

**Case:** Ack with a stale token (the message was redelivered to another consumer)  
**Then:** `409` `message.stale-receipt`, and **nothing changes** (the message remains with the second consumer).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`

---

### UC-ACK-005

**Case:** Ack after the deadline expired but **before** redelivery  
**Then:** the token is still valid and the ack counts.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::only_the_last_delivery_token_acks_and_it_stays_valid_until_redelivery`
  - `src/engine/messages/tests.rs::ack_after_the_deadline_but_before_redelivery_still_works`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`
  - `tests/messages.rs::ack_after_the_deadline_but_before_redelivery_works`

---

### UC-ACK-006

**Case:** Ack of a message expired by `ttl`  
**Then:** `404` `message.not-found`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_found_for_ack_and_visibility`
  - `tests/messages.rs::a_message_past_its_ttl_cannot_be_acked_or_extended`

---

### UC-ACK-007

**Case:** Ack on a distributor  
**Then:** `409` `topic.wrong-type`. On a nonexistent topic: `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::ack_checks_the_topic_first`
  - `tests/messages.rs::ack_validates_the_request_and_the_topic`

---

### UC-ACK-008

**Case:** Without a quorum, the ack is canceled (rollback)  
**Then:** `503` `cluster.quorum-unavailable` and the message **remains available** (returns at the end of the deadline).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`

---

### UC-ACK-009

**Case:** The ack is decided by quorum **with the primary**  
**Then:** with the primary outside the quorum the ack does not take effect.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`

---

### UC-ACK-010

**Case:** Failure on a replica outside the quorum, ack already decided  
**Then:** does not cancel; the coordinator insists until `write.commit.timeout` and, when exhausted, the block stays below the full factor (event, not a health failure).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ACK-011

**Case:** On the ordinary queue the ack removes the message and the metadata without leaving a tombstone  
**Then:** the absence of the ID is enough.  
**Level:** I, C  
**Tests:**
  - `tests/messages.rs::ack_removes_the_message_for_good`

---

### UC-ACK-012

**Case:** Ack after the volatile queue was deleted  
**Then:** `404` `topic.not-found`.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::ack_and_visibility_after_the_topic_is_removed_are_404`

---

### UC-ACK-013

**Case:** Ack of a message on disk (overflow) removes the database row and the `.body` file, in that order.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_message_on_disk_is_delivered_again_and_acked_like_any_other`
  - `tests/messages.rs::the_dead_letter_receives_the_body_file_of_a_message_on_disk`

---


## VIS: Change of invisibility

### UC-VIS-001

**Case:** `PUT` with a valid `receiptToken` and `visibilityTimeout` from 0 to `delivery.visibility-timeout.max`  
**Then:** `200` `VisibilityResponse` with `id` and the new `invisibleUntil`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `src/engine/messages/tests.rs::visibility_works_on_a_message_on_disk`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-002

**Case:** The new deadline is `now + visibilityTimeout`, **not** an addition to the current deadline (shortening also works).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-003

**Case:** `visibilityTimeout: 0` releases the message immediately  
**Then:** the returned `invisibleUntil` is the current instant and the next `GET` delivers it.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_zero_releases_the_message_at_once_and_wakes_a_long_poll`
  - `tests/messages.rs::visibility_zero_releases_the_message_for_the_next_get`

---

### UC-VIS-004

**Case:** The `PUT` does **not** increment `deliveryCount` and keeps the same `receiptToken` (only a new delivery generates another).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-005

**Case:** Without `receiptToken` (query) or without `visibilityTimeout` (body)  
**Then:** `400` `validation.required`. Time out of range: `400` `validation.out-of-range`. Unknown field in the body: `400` `validation.unknown-field`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::missing_fields_are_reported_together`
  - `src/engine/messages/tests.rs::visibility_rejects_out_of_range_and_stale_tokens`
  - `tests/messages.rs::visibility_validates_query_and_body`

---

### UC-VIS-006

**Case:** `PUT` with a stale token (message redelivered)  
**Then:** `409` `message.stale-receipt`, nothing changes.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_rejects_out_of_range_and_stale_tokens`
  - `tests/messages.rs::a_stale_token_is_409_and_changes_nothing`

---

### UC-VIS-007

**Case:** `PUT` of a message already confirmed, expired by `ttl` or of a removed topic  
**Then:** `404` (`message.not-found` or `topic.not-found`).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_message_past_its_ttl_is_not_found_for_ack_and_visibility`
  - `tests/messages.rs::a_message_past_its_ttl_cannot_be_acked_or_extended`
  - `tests/messages.rs::ack_and_visibility_after_the_topic_is_removed_are_404`

---

### UC-VIS-009

**Case:** Repeating the same `PUT` gives the same result (idempotent, because it replaces the value).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-010

**Case:** With the deadline extended, the message is **not** redelivered until the new deadline expires.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_replaces_the_deadline_without_touching_count_or_token`
  - `tests/messages.rs::visibility_replaces_the_deadline_and_keeps_count_and_token`

---

### UC-VIS-011

**Case:** `PUT` with `0` on a message that is already at `maxDeliveries`  
**Then:** goes to the dead-letter instead of returning to the ready list.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::visibility_zero_on_an_exhausted_message_sends_it_to_the_dead_letter`
  - `src/engine/messages/tests.rs::visibility_zero_below_the_limit_just_releases_the_message`
  - `tests/messages.rs::visibility_zero_on_the_last_delivery_sends_the_message_to_the_dead_letter`

---


## RET: Expiration (`ttl`), delivery limit and dead-letter

### UC-RET-001

**Case:** A message with more than `ttl` since publication is discarded, delivered or not, and does **not** go to the dead-letter.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_index_only_says_when_a_ttl_check_is_worth_a_read`
  - `src/engine/messages/tests.rs::sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter`
  - `src/engine/messages/tests.rs::ttl_and_the_dead_letter_work_on_messages_on_disk`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`
  - `src/engine/messages/tests/bounded.rs::the_ttl_removes_the_ready_messages_that_wait_outside_the_window`

---

### UC-RET-002

**Case:** The deadline is `ID instant + ttl` computed with the topic's **current** `ttl`  
**Then:** reducing the `ttl` by `PATCH` expires messages already stored.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_new_ttl_applies_to_messages_already_stored`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-RET-003

**Case:** The `ttl` default (14 days) applies at the time of the check  
**Then:** changing `retention.ttl.default` affects queues without their own `ttl`, and not those that defined it.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::changing_the_default_ttl_affects_the_queues_without_their_own_ttl_only`
**Note:** Only the unit part (U) is proven; the full level is U, I.

---

### UC-RET-004

**Case:** Expiration also applies to an **invisible** message (in processing)  
**Then:** it disappears, and the following ack receives `404`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::sweeping_drops_expired_messages_even_in_flight_and_not_to_the_dead_letter`
  - `tests/messages.rs::a_shorter_ttl_expires_what_is_already_stored_even_in_flight`

---

### UC-RET-005

**Case:** The owner does not deliver an expired message between two sweeps.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_background_sweeper_moves_messages_without_anyone_consuming`
  - `tests/messages.rs::the_background_sweeper_moves_it_even_if_nobody_touches_the_source_queue`

---

### UC-RET-006

**Case:** Each node's sweep (`retention.sweep.interval`) deletes the expired copies in memory and on disk, without propagating anything.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::lowering_max_deliveries_sends_the_ready_messages_over_the_new_limit_to_the_dead_letter`
  - `src/engine/messages/tests/cluster.rs::each_node_expires_its_own_replicas_by_ttl`
  - `tests/replication.rs::every_node_expires_its_own_copies_by_ttl_without_propagating`

---

### UC-RET-007

**Case:** Each expired message generates the `taba.message.expired` event and counts in `taba.retention.expired`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::an_expired_message_raises_the_event_and_the_counters`
  - `tests/metrics.rs::redeliveries_dead_letters_and_expirations_are_counted`

---

### UC-RET-008

**Case:** `maxDeliveries: N`  
**Then:** when the invisibility of the N-th delivery expires without an ack, the message goes to the dead-letter and the (N+1)-th delivery **never happens**.  
**Level:** I  
**Tests:**
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

**Case:** In the dead-letter the message keeps the **same `id`**, with `deliveryCount: 0`, `invisibleUntil` cleared, `deadLetteredAt` and `deadLetterReason: max-deliveries`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::after_max_deliveries_the_message_goes_to_the_dead_letter_with_the_same_id`
  - `tests/messages.rs::the_message_goes_to_the_dead_letter_after_max_deliveries_without_ack`

---

### UC-RET-010

**Case:** The message in the dead-letter falls on the same block and the same nodes as the original message (same `id`).  
**Level:** U, C  
**Tests:**
  - `src/core/ring/tests.rs::the_dead_letter_copy_falls_in_the_same_block_and_nodes_as_the_original`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-RET-011

**Case:** The move to the dead-letter is **one operation** of register + commit (creates in the dead-letter and removes from the source queue). Failure before the quorum  
**Then:** rollback, the message stays in the queue and the owner tries again on the next round.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure`

---

### UC-RET-012

**Case:** The dead-letter has no definition of its own  
**Then:** there is no row in the `topics` table and the settings are those of the source queue.  
**Level:** I  
**Tests:**
  - `src/engine/topics/tests.rs::the_dead_letter_is_derived_from_its_queue_and_never_stored`

---

### UC-RET-013

**Case:** The dead-letter has no dead-letter  
**Then:** the message there only leaves by ack or by `ttl` (counted from `deadLetteredAt`).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_dead_letter_has_no_dead_letter_and_its_ttl_counts_from_deadlettering`
  - `tests/messages.rs::the_dead_letter_never_has_its_own_dead_letter`

---

### UC-RET-014

**Case:** The dead-letter is an ordinary topic for consumption, ack and publishing  
**Then:** a corrected message can be returned to the source queue.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_dead_letter_message_can_be_published_back_to_the_source`
  - `tests/messages.rs::a_corrected_message_can_be_published_back_and_the_dead_one_acked`

---

### UC-RET-015

**Case:** The dead-letter suffix is configurable (`topics.dead-letter.suffix`) and becomes the reserved name.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::the_dead_letter_suffix_is_configurable_and_becomes_the_reserved_name`
**Note:** Only the unit part (U) is proven; the full level is U, I.

---

### UC-RET-016

**Case:** The dead-letter is never a destination of a distributor, even if the name starts with the prefix.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_distributor_never_copies_into_a_dead_letter`

---

### UC-RET-017

**Case:** The `GET` on the dead-letter does not count as a consumer of the source volatile queue.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_get_on_the_dead_letter_does_not_count_as_a_consumer`

---

### UC-RET-018

**Case:** Each message sent to the dead-letter generates the `taba.message.dead_lettered` event (queue, ID, `deliveryCount`) and counts in `taba.delivery.dead_lettered`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::a_dead_lettered_message_raises_the_event_and_the_counters`
  - `tests/metrics.rs::redeliveries_dead_letters_and_expirations_are_counted`

---


## DST: Distributor Topic

### UC-DST-001

**Case:** Publishing to a distributor copies the message to **each** queue whose name starts with one of the prefixes  
**Then:** `200` `DistributorPublishResponse` with `matched` and one entry in `deliveries` per queue, each copy with its own `id`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-002

**Case:** Each copy is independent  
**Then:** the same content and `contentType`, different `id`, and consumption and ack of one do not affect the others.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-003

**Case:** No queue matches  
**Then:** `200` with `matched: 0` and `deliveries: []`, the message is discarded, and the `taba.distributor.no_target` event is recorded.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_copies_the_message_to_every_matching_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-004

**Case:** A queue that matches **two or more prefixes of the same** distributor receives **one** copy.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::a_queue_matching_two_prefixes_of_one_distributor_gets_one_copy`

---

### UC-DST-005

**Case:** A queue that matches prefixes of **different distributors** receives one copy of each publication on each.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-006

**Case:** Only queues are destinations  
**Then:** a distributor never receives a copy (no loops).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-007

**Case:** The match is made at publish time  
**Then:** a queue created later only receives what is published afterwards.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::the_match_is_made_when_publishing_and_a_new_prefix_counts_from_then_on`

---

### UC-DST-009

**Case:** A copy without `groupId` to a FIFO queue fails only for that queue, with `validation.group-id-required` in the item; the other copies continue.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_copy_to_a_fifo_queue_without_group_fails_only_that_queue`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-DST-010

**Case:** The informed `groupId` and `deduplicationId` go to all the copies; without `deduplicationId`, each copy uses the body hash.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::the_group_and_the_deduplication_key_go_to_every_copy`

---

### UC-DST-011

**Case:** With `topics.distributor.max-targets` defined and the publication matching more queues  
**Then:** `422` `distributor.too-many-targets`, and **no** copy is made. Without the ceiling, there is no limit.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_over_max_targets_refuses_the_whole_publication`
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`
  - `tests/messages.rs::max_targets_refuses_the_whole_publication`

---

### UC-DST-012

**Case:** On a distributor the general `message.max-bytes` applies, and the copies are not checked again against each queue's `maxMessageBytes`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_checks_the_general_size_and_not_that_of_each_queue`

---

### UC-DST-013

**Case:** The `PATCH` of `prefixes` applies from the commit, only to later publications.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::the_match_is_made_when_publishing_and_a_new_prefix_counts_from_then_on`

---

### UC-DST-014

**Case:** The distributor does not store messages  
**Then:** there is no message in memory or on disk for it, and no ready list.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_keeps_no_message_and_has_no_ack`

---

### UC-DST-015

**Case:** There is no order between the destination queues. The test cannot require order.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::a_queue_matching_two_prefixes_of_one_distributor_gets_one_copy`
  - `src/engine/messages/tests/distributor.rs::distributors_with_the_same_prefix_each_send_a_copy_and_never_to_a_distributor`

---

### UC-DST-016

**Case:** `GET` and ack on a distributor  
**Then:** `409` `topic.wrong-type`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::a_distributor_keeps_no_message_and_has_no_ack`

---

### UC-DST-017

**Case:** The `publish` permission is checked only on the distributor; the destination queues are not checked again (nor the token's prefixes).  
**Level:** I  
**Tests:**
  - `tests/auth.rs::publishing_to_a_distributor_checks_only_the_distributor`

---

### UC-DST-018

**Case:** The distributor's metrics are only `received`, `copies` and `copyErrors` (the queue ones apply to the destination queues).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::a_distributor_raises_no_target_and_partial_and_counts_its_copies`
  - `tests/topic_stats.rs::a_distributor_only_shows_what_it_received_copied_and_failed_to_copy`

---


## VOL: Volatile queue and managing node

### UC-VOL-001

**Case:** A volatile queue with no `GET` for `idleTimeout` seconds is deleted, with the messages and the dead-letter, and the `taba.volatile.deleted` event is recorded.  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests.rs::an_idle_volatile_queue_is_deleted_with_its_messages_and_dead_letter`
  - `src/engine/messages/tests.rs::persistent_queues_and_distributors_are_never_deleted_for_idleness`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`
  - `tests/volatile.rs::an_ordinary_queue_never_expires`
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-002

**Case:** The clock starts at **creation**  
**Then:** the deadline is `max(createdAt, last GET on any node) + idleTimeout`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::an_idle_volatile_queue_is_deleted_with_its_messages_and_dead_letter`
  - `src/engine/messages/tests.rs::the_clock_runs_from_the_creation_and_is_reset_by_each_get`
  - `src/engine/messages/tests.rs::a_recreated_queue_does_not_inherit_the_last_get_of_the_old_one`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`

---

### UC-VOL-003

**Case:** **Any** `GET` counts as a consumer (with or without a message, with or without long polling), and the instant is recorded at the **start** of the call.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::any_get_counts_with_or_without_messages_and_a_long_poll_counts_from_its_start`
  - `tests/volatile.rs::a_queue_without_consumers_is_deleted_by_the_sweeper_and_a_get_postpones_it`

---

### UC-VOL-004

**Case:** Publishing does **not** count as a consumer  
**Then:** a volatile queue with only producers is deleted, and the messages are lost.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::publishing_does_not_count_as_a_consumer`
  - `tests/volatile.rs::publishing_and_reading_the_dead_letter_do_not_keep_a_volatile_queue_alive`

---

### UC-VOL-005

**Case:** The `GET` on the volatile queue's dead-letter does not count as a consumer.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_get_on_the_dead_letter_does_not_count_as_a_consumer`
  - `tests/volatile.rs::publishing_and_reading_the_dead_letter_do_not_keep_a_volatile_queue_alive`

---

### UC-VOL-006

**Case:** The manager is the primary node of the hash of the topic `id` on the ring; any node reaches the same manager from the topic and member tables.  
**Level:** U, C  
**Tests:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`
  - `tests/volatile_cluster.rs::many_queues_are_managed_by_different_nodes_and_all_expire`

---

### UC-VOL-007

**Case:** The list of managed topics is derived, not stored  
**Then:** each node computes it locally and it changes by itself with the member or topic table.  
**Level:** U, C  
**Tests:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-VOL-008

**Case:** At the deadline the manager asks **all the nodes** (`GET /cluster/topics/{id}/idle`) how long ago the last `GET` was; each node answers an elapsed duration, not an instant.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-009

**Case:** If some node had a `GET` more recent than `idleTimeout`, the queue is **not** deleted  
**Then:** the manager reschedules for `idleTimeout` minus the smallest duration received.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_manager_asks_every_node_and_deletes_only_when_none_had_a_get`

---

### UC-VOL-010

**Case:** If some node does not answer, the manager **does not delete** and tries again after `topics.volatile.idle-check.retry-interval`; the `taba.volatile.check_blocked` event is recorded.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive`

---

### UC-VOL-011

**Case:** A node with no `GET` record for the topic (just started or restarted) answers the time since it started, with `sinceStart: true`  
**Then:** a restart can only **postpone** the deletion.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start`

---

### UC-VOL-012

**Case:** The deletion uses the register + commit of all nodes with the topic's `id` (`expectedId`)  
**Then:** a queue deleted and **recreated** along the way is not deleted by mistake.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::a_recreated_queue_is_not_deleted_by_the_old_deadline`

---

### UC-VOL-013

**Case:** Residual race  
**Then:** a `GET` after the final check and before the commit receives `404`, or recreates the queue (with `autoCreate`) with a new `id` and without the old messages.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::a_get_after_the_final_deletion_is_not_found_or_recreates_an_empty_queue_with_a_new_id`

---

### UC-VOL-014

**Case:** A consumer with an invisible message receives `404` when trying the ack after the deletion.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::an_ack_after_the_queue_was_deleted_is_not_found`

---

### UC-VOL-015

**Case:** Manager down and not yet deregistered  
**Then:** none of its queues is deleted; the queue keeps receiving messages and consumers. Once the node is deregistered, the new manager takes over.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::a_node_that_does_not_answer_or_a_manager_that_is_down_keeps_the_queue_alive`

---

### UC-VOL-016

**Case:** Node join  
**Then:** the new node only manages after becoming ready; until then no queue of that stretch of the ring is deleted. The existing nodes give up the queues whose position passed to the new one.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-VOL-017

**Case:** Node leave (deregistration commit)  
**Then:** the new primary of the removed node's positions takes over its volatile queues, without transferring state.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::when_the_manager_leaves_the_new_primary_of_its_positions_takes_over`

---

### UC-VOL-018

**Case:** Node register failure and return of an excluded node  
**Then:** each node's set of managed queues goes back to the previous one. A topic created or deleted updates the set when applying the commit.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_managed_set_of_every_node_goes_back_when_the_excluded_node_returns`

---

### UC-VOL-019

**Case:** On taking over a queue, the new manager does the idleness query **immediately** and computes the real deadline; many queues changing manager spread the queries over time (`topics.volatile.takeover.jitter`).  
**Level:** C  
**Tests:**
  - `src/cluster/volatile/tests.rs::the_takeover_delay_is_stable_per_queue_and_spreads_the_queries_inside_the_window`

---

### UC-VOL-020

**Case:** Deletion in progress during the manager change  
**Then:** both attempts are harmless (the second is rejected without effect by the `id`).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::a_get_after_the_final_deletion_is_not_found_or_recreates_an_empty_queue_with_a_new_id`

---

### UC-VOL-021

**Case:** `PATCH` of `idleTimeout` applies at the next check; `volatile` and the type are immutable.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::patching_the_idle_timeout_applies_to_the_next_check`
  - `tests/volatile.rs::volatile_and_the_type_are_immutable_but_the_idle_timeout_can_change`

---

### UC-VOL-022

**Case:** A FIFO queue can be volatile, with the same rules.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_fifo_queue_can_be_volatile`

---

### UC-VOL-023

**Case:** Deletions of volatile queues and membership changes can run in parallel, with no wait between them.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::many_queues_are_managed_by_different_nodes_and_all_expire`

---


## FIF: FIFO queue

### UC-FIF-001

**Case:** `fifo` is immutable  
**Then:** `PATCH` with `fifo` answers `400` `validation.immutable-field`. A distributor cannot be FIFO (`400` `validation.not-applicable`).  
**Level:** I  
**Tests:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`

---

### UC-FIF-002

**Case:** Publish to a FIFO without `groupId`  
**Then:** `400` `validation.group-id-required`. Invalid `groupId`: `400`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::a_fifo_queue_requires_the_group_id`
  - `src/engine/messages/tests.rs::a_fifo_publish_needs_a_group_and_says_whether_it_was_a_duplicate`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`
  - `tests/messages.rs::group_and_deduplication_ids_are_validated`

---

### UC-FIF-003

**Case:** Publish with a valid `groupId`  
**Then:** `201` `PublishResponse` with `id` and `duplicate: false`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_fifo_publish_needs_a_group_and_says_whether_it_was_a_duplicate`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-004

**Case:** Duplicate publication (same group, same `deduplicationId`, within the window)  
**Then:** `200` (not `201`) with the **original's** `id` and `duplicate: true`, and no new message is created.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_repeated_publish_in_the_window_returns_the_original_and_creates_nothing`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-005

**Case:** Without `deduplicationId`, the SHA-256 of the body **bytes**, in hexadecimal, applies; the `contentType` does not enter the hash. Two identical messages in the same group become one.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::without_a_deduplication_id_the_sha256_of_the_body_is_the_key`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-006

**Case:** The `deduplicationId` is returned on the `GET` (the informed one or the hash).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_repeated_publish_in_the_window_returns_the_original_and_creates_nothing`
  - `tests/fifo.rs::publishing_needs_a_group_and_a_repeat_is_a_200_with_the_original_id`

---

### UC-FIF-007

**Case:** After the window (`fifo.dedup.window`, default 300 s, counted from the original's ID instant), the same key creates a new message.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_window_ends_and_the_scope_is_the_group`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-008

**Case:** The scope is the group  
**Then:** the same `deduplicationId` in different groups is **not** a duplicate.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_window_ends_and_the_scope_is_the_group`
  - `tests/fifo.rs::the_body_hash_is_the_default_key_the_scope_is_the_group_and_the_window_ends`

---

### UC-FIF-009

**Case:** Two simultaneous publications with the same key are serialized by the primary  
**Then:** the second waits. If the first committed, it returns the same `id`; if it suffered a rollback, it proceeds as new.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::simultaneous_publications_with_the_same_key_create_one_message`

---

### UC-FIF-010

**Case:** Repeating a publication that gave `502` or `504` is safe  
**Then:** the primary deduplicates and the duplicate returns the original's `id`.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-011

**Case:** Deduplication still applies **after the ack** of the original, while the window has not passed (the row with `deleteAt` still exists).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`
  - `tests/fifo.rs::deduplication_still_holds_after_the_ack`

---

### UC-FIF-012

**Case:** The next message of a group is only delivered after the **ack** of the previous one (or its exit by `ttl` or dead-letter).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_next_message_of_a_group_waits_for_the_ack_of_the_previous_one`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-013

**Case:** Different groups run in parallel  
**Then:** with two free groups, the `GET` delivers one message from each.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::different_groups_go_in_parallel_and_a_batch_never_has_two_of_the_same_group`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-014

**Case:** Each message of the `GET` batch comes from a **different** group; never two of the same group in the same batch.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::with_a_zero_visibility_the_group_is_free_again_after_the_batch`
  - `src/engine/messages/tests.rs::different_groups_go_in_parallel_and_a_batch_never_has_two_of_the_same_group`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-015

**Case:** Within the group the delivery order is the `id` order. A producer that publishes in sequence (waits for the response) has the order preserved; concurrent producers stay in the order in which they reach the primary.  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-016

**Case:** Redelivery  
**Then:** once the invisibility expires without an ack, the **same** message comes back as the head of the group (`deliveryCount` goes up) and the group becomes free again; the order is kept.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::an_expired_or_released_head_comes_back_first_and_the_order_holds`

---

### UC-FIF-017

**Case:** `PUT .../visibility` with `0` on the head releases the group, like an expiry.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::an_expired_or_released_head_comes_back_first_and_the_order_holds`

---

### UC-FIF-018

**Case:** The ack of the head releases the group, and the next message enters the ready list.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::the_next_message_of_a_group_waits_for_the_ack_of_the_previous_one`
  - `tests/fifo.rs::one_message_in_flight_per_group_in_order_and_groups_in_parallel`

---

### UC-FIF-019

**Case:** Between free groups the oldest head wins; fair delivery by bands does **not** apply on FIFO.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_fifo_list_has_one_entry_per_free_group_the_oldest_head_first`
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`

---

### UC-FIF-020

**Case:** The ready list has one entry per free group (the head), ordered by ID; a group with a message in flight is not in the list.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_fifo_list_has_one_entry_per_free_group_the_oldest_head_first`
  - `src/engine/messages/tests.rs::inside_a_group_the_order_is_the_id_and_among_free_groups_the_oldest_head_wins`

---

### UC-FIF-021

**Case:** `maxDeliveries` reached by the head  
**Then:** goes to the dead-letter and the group continues with the next one; the order of the rest is kept. The dead-letter of a FIFO is **not FIFO**.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_head_over_max_deliveries_goes_to_the_dead_letter_and_the_group_goes_on`
  - `src/engine/messages/tests.rs::a_message_that_went_to_the_dead_letter_still_deduplicates_in_the_window`
  - `tests/fifo.rs::the_dead_letter_of_a_fifo_is_a_common_queue`

---

### UC-FIF-022

**Case:** The head expired by `ttl` is discarded and the group is released for the next one.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::a_group_is_freed_by_the_end_of_the_invisibility_the_ack_or_the_expiry_of_the_head`
  - `src/engine/messages/tests.rs::a_head_past_its_ttl_is_dropped_and_the_group_is_freed`

---

### UC-FIF-023

**Case:** The position on the ring is `hash(topic id + ":" + groupId)`  
**Then:** all the messages of a group fall in the same block, with the same primary and the same replicas; different groups have different positions.  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::a_fifo_group_has_one_position_for_all_its_messages`

---

### UC-FIF-024

**Case:** Recreating the topic (another `id`) changes the position of the groups.  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::recreating_a_fifo_topic_moves_its_groups_in_the_ring`

---

### UC-FIF-025

**Case:** The dead-letter of a FIFO uses the source queue's `id` (and the `groupId`) for the position, so the move remains a single operation.  
**Level:** U, C  
**Tests:**
  - `tests/fifo_cluster.rs::the_dead_letter_of_a_fifo_stays_in_the_block_of_the_group`

---

### UC-FIF-026

**Case:** The node that receives the publication forwards it to the **group's primary**, which coordinates (generates the ID, checks deduplication, does the register + commit). If the node itself is the primary, there is no forwarding.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-027

**Case:** Monotonic ID per group  
**Then:** `max(now, last group ID + 1)`, using the counter bits of the GUID v7 in the same millisecond; with the primary's clock behind, the IDs still grow.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_id_of_a_group_keeps_growing_even_if_the_clock_of_the_new_primary_is_behind`

---

### UC-FIF-028

**Case:** The primary recomputes the group's owner on receiving the forwarded publication  
**Then:** if it is not it, it answers `421` `fifo.misdirected`; the forwarding node tries again (re-reads its own table), up to `fifo.forward.max-attempts`, and when exhausted the client receives `503` `fifo.primary-unavailable` with `Retry-After`.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_node_that_is_not_the_primary_of_the_group_answers_421`

---

### UC-FIF-029

**Case:** The group's primary still synchronizing (new node)  
**Then:** the group's publications answer `503` with `Retry-After`; the old primary does **not** coordinate.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-030

**Case:** Primary down and not yet deregistered  
**Then:** the group's publications answer `503` until the deregistration.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-031

**Case:** The write, delivery and ack quorum **always includes the primary and the secondary**, and is never below 2; a `write.quorum` greater than 2 also includes them.  
**Level:** U, C  
**Tests:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-032

**Case:** Secondary down and not yet deregistered  
**Then:** the group's write, delivery and ack answer `503`, even with a healthy tertiary; it goes away when it is deregistered.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_primary_or_a_secondary_that_is_down_but_still_in_the_table_stops_the_group`

---

### UC-FIF-033

**Case:** Failover without reordering  
**Then:** the primary confirms `m1`, releases `m2`, and goes down; the new primary (the secondary) already has the ack of `m1`, and the consumer does **not** see `m1, m2, m1`.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-034

**Case:** The new primary inherits the deduplication without loss (the records exist on all nodes).  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-035

**Case:** On taking over a block, the new primary rebuilds per group the head, the free or busy state (a group with an invisible message in the future enters busy) and the base of the monotonic ID.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::when_the_primary_leaves_the_secondary_takes_the_group_without_reordering_or_forgetting`

---

### UC-FIF-036

**Case:** The ack on FIFO **does not delete the row**  
**Then:** it writes `deleteAt` (= ID instant + `fifo.dedup.window`) and deletes the body (`.body` and the in-memory copy) and the delivery metadata; the short row keeps `id`, position, `group_id`, `deduplicationId` and `deleteAt`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`
  - `src/storage/messages/tests.rs::a_tombstone_keeps_a_short_row_and_drops_the_body`
  - `src/storage/messages/tests.rs::a_tombstone_works_for_a_message_that_had_no_row`

---

### UC-FIF-037

**Case:** If `deleteAt` has already passed at the ack, the row is deleted at once, as on the ordinary queue. The same applies to the message that goes to the dead-letter.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::an_ack_after_the_window_deletes_the_row_at_once`

---

### UC-FIF-038

**Case:** The row with `deleteAt` exists on all nodes, replicated by the ack's register + commit, decided by quorum.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-039

**Case:** Each node's sweep deletes the rows expired by the `deleteAt` index every `retention.sweep.interval`, without propagating anything.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::the_sweep_deletes_the_rows_whose_delete_at_passed`
  - `src/storage/messages/tests.rs::sweeping_deletes_only_the_rows_whose_delete_at_passed`

---

### UC-FIF-040

**Case:** The row with `deleteAt` is ignored by the ready list, by the queue and group counters, by `depth`, by the `ttl`, by the `MIN(id)` of `oldestMessageAt`, by the `GET`, by `PUT .../visibility` and by a second ack (which answer as a nonexistent message). Only deduplication and the sweep see it.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::an_ack_leaves_a_short_row_that_keeps_deduplicating_and_is_invisible_to_everything_else`

---

### UC-FIF-041

**Case:** The ID listing and the block's message stream include the row with `deleteAt`, **without a body**, and the event database writes the `deleteAt` idempotently.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::the_short_line_of_a_confirmed_fifo_message_travels_without_a_body`
  - `src/storage/messages/tests.rs::the_short_fifo_row_is_listed_with_its_delete_at`
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-FIF-042

**Case:** `autoCreate` accepts `fifo=true`, with any `replication.factor` (UC-CNS-037).  
**Level:** I, C  
**Tests:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-FIF-043

**Case:** A distributor with a FIFO among the destinations keeps the per-group order on each queue (each copy is coordinated by the primary of the destination queue's group).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests.rs::a_distributor_copy_without_a_group_fails_only_that_fifo_queue`
  - `tests/fifo_cluster.rs::a_distributor_keeps_the_order_by_group_in_a_fifo_target`

---

### UC-FIF-044

**Case:** FIFO metrics and conditions  
**Then:** `taba.fifo.deduplicated`, `taba.fifo.forward` (`ok`, `misdirected`, `error`), `taba.fifo.groups.busy`, event `taba.message.deduplicated`, and the `group-stalled` condition when the head goes past `stats.topics.group-stalled-deliveries` deliveries without an ack.  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::a_deduplicated_publication_raises_the_event_and_the_counters`
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`
  - `tests/topic_stats.rs::group_stalled_fires_when_a_fifo_head_is_delivered_too_often_without_ack`

---

### UC-FIF-045

**Case:** The `noisy-group` condition indicates the hot group of a FIFO.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::noisy_group_points_at_the_hot_group_of_a_fifo_queue`

---

### UC-FIF-048

**Case:** The FIFO queue **during the restoration**
**Then:** it receives messages and does not deliver them until the status is **restored from disk**; the plain queues are not held.
**Level:** C
**Tests:**
  - `src/engine/messages/tests/hold.rs::a_held_fifo_queue_receives_and_does_not_deliver_until_it_is_released`
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`

---

### UC-FIF-049

**Case:** The **generator of IDs** of the new life
**Then:** it starts above the **watermark** (the largest `id` of the old lives, in the manifest of each one and announced between the nodes): the new `id`s are greater than the old ones and no `id` already returned to a producer changes.
**Level:** C
**Tests:**
  - `src/core/id/tests.rs::a_generator_with_a_floor_never_goes_below_it_and_reports_its_watermark`
  - `src/engine/messages/tests/hold.rs::the_ids_of_the_new_life_start_above_the_watermark_of_the_old_ones`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-050

**Case:** New messages during the restoration
**Then:** they go into the **provisional list** of the group's primary, with a normal `id`, and **nothing is deduplicated**: every publication is accepted, including one that repeats a key.
**Level:** C
**Tests:**
  - `src/engine/messages/tests/hold.rs::while_held_nothing_is_deduplicated_and_the_duplicate_answer_never_comes`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-051

**Case:** The **release**
**Then:** the primary deduplicates the provisional list against the restored keys and inside the list, in `id` order; it discards the duplicates (an ack on the owners with no receipt) and only then releases the delivery.
**Level:** C
**Tests:**
  - `src/engine/messages/tests/hold.rs::the_release_lists_as_duplicates_what_repeats_a_restored_key_or_an_earlier_publication`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-FIF-052

**Case:** The producer of a message that is discarded as a duplicate at the release
**Then:** it had received `duplicate=false` (an accepted exception; PRB-063 for the limit of the window).
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-053

**Case:** The restored FIFO messages
**Then:** they reach the group's primary by the path of the restoration and stay without delivery until the release; they keep the key before any provisional one.
**Level:** C
**Tests:**
  - `src/engine/messages/tests/hold.rs::the_release_lists_as_duplicates_what_repeats_a_restored_key_or_an_earlier_publication`
  - `tests/restore_vote.rs::the_release_discards_what_the_fifo_accepted_that_repeats_a_restored_key_and_ids_start_above_the_watermark`

---

### UC-FIF-047

**Case:** The `receiptToken` of a FIFO message carries the group position (`{position}-{guid}`)  
**Then:** the ack and the invisibility change, from any node, find the block without knowing the group; a malformed receipt is `409` `message.stale-receipt`.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---

### UC-FIF-046

**Case:** On a single node (one-node profile, factor 1) the FIFO works the same  
**Then:** the node is the primary of all groups (no forwarding), the quorum is 1 and `replication.factor` 1 is accepted.  
**Level:** I  
**Tests:**
  - `tests/fifo.rs::fifo_is_immutable_works_with_auto_create_on_a_single_node`
  - `tests/fifo.rs::a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`

---


## GRP: Message groups and fair delivery

### UC-GRP-001

**Case:** The `groupId` is optional; a message without a group belongs to an empty group. A topic where nobody uses `groupId` has a single group and behaves like the simple ready list.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/index/tests.rs::messages_without_a_group_are_one_group_of_the_empty_name`

---

### UC-GRP-002

**Case:** The `groupId` is immutable, stored with the message (memory or `group_id` column) and returned on the `GET`; in the dead-letter the message keeps the group.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-003

**Case:** On an ordinary queue the group does **not** order or serialize delivery  
**Then:** two messages of the same group can be in flight at the same time.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_small_group_is_delivered_before_the_big_one_is_exhausted`
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-004

**Case:** Each owner keeps per group the counters of **ready** and **in processing**. Publishing  
**Then:** +1 ready. Delivery: ready becomes in processing. Invisibility expiry (and `PUT` with `0`): returns to ready. Ack, expiry by `ttl` and dead-letter deduct.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_counters_follow_publish_delivery_expiry_ack_and_dead_letter`

---

### UC-GRP-005

**Case:** `size = in processing + weight × ready` with the weight of `delivery.fair.ready-weight` (default 0.1). With weight `0` only what is in processing counts; with `1` the whole backlog counts.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_group_size_weighs_ready_messages_by_the_configured_weight`

---

### UC-GRP-006

**Case:** The band is `⌊log2(size)⌋`, and a size smaller than 1 counts as 1  
**Then:** size 1 is band 0, 2 and 3 band 1, 4 to 7 band 2.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_band_is_the_floor_of_log2_with_a_minimum_size_of_one`

---

### UC-GRP-007

**Case:** The refill of the ready list walks the groups from smallest to largest and takes **one** message from each group per round, until the list is full.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`

---

### UC-GRP-008

**Case:** The list is ordered by band and, within it, by the accordion. A new message enters at the position of its group's band, ahead of those of larger groups.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`

---

### UC-GRP-009

**Case:** On taking an ID, the `GET` checks the group's current band  
**Then:** if it changed since the ID entered the list, the ID is repositioned and the next one is considered.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_band_is_checked_again_at_every_pick`

---

### UC-GRP-010

**Case:** Observable effect  
**Then:** a group with 10 thousand messages and another with 5 messages published together: the 5 of the small group are delivered before the large one is exhausted.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::the_small_group_is_served_before_the_big_one_is_exhausted`
  - `src/engine/messages/tests.rs::a_small_group_is_delivered_before_the_big_one_is_exhausted`
  - `tests/messages.rs::a_noisy_producer_does_not_delay_a_small_one_over_http`

---

### UC-GRP-011

**Case:** Guaranteed progress  
**Then:** every `delivery.fair.normal-every` deliveries (default 10), one takes the oldest message of the list, of any group; the large group never waits without limit. With `0`, the guarantee is turned off.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/index/tests.rs::every_nth_delivery_takes_the_oldest_of_any_group`
  - `src/engine/messages/index/tests.rs::with_normal_every_zero_the_guarantee_is_off`
  - `src/engine/messages/tests.rs::every_nth_delivery_takes_the_oldest_message_so_the_big_group_never_starves`

---

### UC-GRP-012

**Case:** Above `delivery.fair.max-groups`, new groups share a single counter (treated as a single group), which appears in `GroupStats` with `overflow: true` and without `groupId`.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::groups_above_the_ceiling_share_one_counter`
  - `tests/topic_stats.rs::the_shared_counter_shows_as_overflow_and_an_empty_group_disappears`
  - `tests/fifo.rs::a_restart_keeps_the_busy_groups_and_the_rows_that_deduplicate`

---

### UC-GRP-013

**Case:** The per-group counts are derived  
**Then:** rebuilt from the metadata and the `group_id` column when the node takes over blocks or restarts.  
**Level:** C  
**Tests:**

---

### UC-GRP-014

**Case:** On a distributor, the `groupId` is passed on to all the copies.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/distributor.rs::the_group_and_the_deduplication_key_go_to_every_copy`

---

### UC-GRP-015

**Case:** `GET /metrics/topics/{topic}/groups` returns messages, share, age and rates per `groupId`; `top=N` returns the `N` largest without pagination; `groupId=` returns only that one; a group with no messages and no activity disappears.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::group_snapshots_show_counts_bands_and_the_oldest_of_each_group`
  - `tests/topic_stats.rs::the_groups_route_lists_pages_filters_and_tops_the_groups`
  - `tests/topic_stats.rs::the_shared_counter_shows_as_overflow_and_an_empty_group_disappears`

---

### UC-GRP-016

**Case:** The `noisy-group` condition fires when there are two or more groups and the largest occupies more than `stats.topics.noisy-group-share` of `depth`.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/stats/tests.rs::noisy_group_needs_two_groups_and_more_than_the_share`
  - `tests/topic_stats.rs::noisy_group_fires_with_two_groups_and_the_largest_over_the_share`

---

### UC-GRP-017

**Case:** The `taba.delivery.fair.wait` metric measures the wait per band  
**Then:** the low bands wait less than the high ones.  
**Level:** I  
**Tests:**
  - `src/engine/messages/stats/tests.rs::the_wait_is_kept_per_band_and_the_last_band_is_15_or_more`
  - `tests/metrics.rs::the_wait_is_measured_per_band_and_small_groups_wait_less`

---


## AUT: Authentication and access tokens

### UC-AUT-001

**Case:** Without a root token (no `root-token` in the secrets folder, no `auth.root-token-file` and no `auth.root-token`), no client route requires anything.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::without_a_root_token_nothing_matches`
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-002

**Case:** With a root token (through the secrets folder, the file or `auth.root-token`), authentication **turns on by itself** and the client routes require `Authorization: Bearer`.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::an_inline_root_token_matches_only_itself`
  - `tests/auth.rs::with_a_root_token_every_client_route_asks_for_a_valid_one_and_the_401_never_says_why`
  - `tests/secrets.rs::a_node_mounting_the_folder_from_init_turns_authentication_on_by_itself`

---

### UC-AUT-003

**Case:** `auth.enabled=false` forces authentication off, even with the folder mounted.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::auth_disabled_overrides_the_root_token`

---

### UC-AUT-004

**Case:** `auth.enabled=true` without a root token prevents the node from starting; so does a defined `auth.root-token-file` that does not exist.  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::forcing_auth_without_a_root_token_stops_startup`
  - `src/config/tests.rs::a_named_root_token_file_that_does_not_exist_stops_startup`

---

### UC-AUT-005

**Case:** Root token with fewer than 32 characters  
**Then:** the node does not start.  
**Level:** I  
**Tests:**
  - `src/secrets/tests.rs::init_creates_the_certificate_in_tls_and_a_root_token_of_at_least_32_characters`
  - `src/secrets/tests.rs::generated_root_tokens_are_random_and_long_enough`

---

### UC-AUT-006

**Case:** `auth.root-token-file` accepts two lines (the current and the previous) and is re-read every `auth.reload-interval`  
**Then:** changing the secret does not restart the node, both are valid during the swap, and the tokens already created keep working.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::the_root_token_file_takes_two_lines_and_is_reloaded`
  - `src/http/auth/tests.rs::a_failed_reload_keeps_the_previous_secret`
  - `tests/auth.rs::the_root_token_file_is_reloaded_without_restarting`

---

### UC-AUT-007

**Case:** Without a root token and without `auth.dev-token` authentication is not required.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-008

**Case:** Without a token, malformed, unknown, revoked or expired  
**Then:** `401` `auth.unauthorized` with `WWW-Authenticate: Bearer`; the response is the same in the five cases (it does not say which).  
**Level:** U, I  
**Tests:**
  - `src/core/token/tests.rs::malformed_tokens_do_not_parse`
  - `src/engine/tokens/tests.rs::every_invalid_token_is_just_none`
  - `src/engine/tokens/tests.rs::an_expired_token_is_refused_and_the_sweep_erases_it`
  - `tests/auth.rs::with_a_root_token_every_client_route_asks_for_a_valid_one_and_the_401_never_says_why`

---

### UC-AUT-009

**Case:** `publish` allows only `POST /topics/{topic}/messages`; `consume` allows `GET`, `PUT .../visibility` and `DELETE` of messages; `create` allows `POST /topics` and `autoCreate` (together with `consume`); `manage` allows `PATCH` and `DELETE` on `/topics/{topic}` **and includes `create`**; `admin` (the tokens and the cluster) includes `manage` and `create`. Each route outside the permission  
**Then:** `403` `auth.forbidden`.  
**Level:** U, I  
**Tests:**
  - `src/core/token/tests.rs::manage_includes_create_and_admin_includes_manage_and_nothing_else_does`
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-010

**Case:** `create` only **creates**  
**Then:** changing and removing topics requires `manage` (or `admin`, which includes it).  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-011

**Case:** Every valid token queries `GET /topics` and `GET /topics/{topic}` within its prefixes.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::permissions_gate_each_route`

---

### UC-AUT-012

**Case:** Topic prefixes  
**Then:** the token only reaches topics whose name **starts with** some prefix; a topic outside: `403` `auth.out-of-scope`. Without prefixes, it reaches all.  
**Level:** U, I  
**Tests:**
  - `src/core/token/tests.rs::prefixes_limit_the_topics_a_token_reaches`
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`
  - `tests/volatile.rs::auto_create_stays_inside_the_token_prefixes`

---

### UC-AUT-013

**Case:** `GET /topics` with a prefix token returns only the topics within them.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`

---

### UC-AUT-014

**Case:** `POST /topics` with `create` only accepts a name within the prefixes; on a distributor, also each of its distribution `prefixes` (otherwise the token would inject messages into queues out of reach). The `PATCH` of `prefixes` follows the same rule.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::a_distribution_prefix_must_sit_inside_the_token_prefixes`
  - `tests/auth.rs::a_token_only_creates_inside_its_prefixes_including_the_distribution_prefixes`

---

### UC-AUT-015

**Case:** The dead-letter is reached by the same prefix as the source queue.  
**Level:** I  
**Tests:**
  - `src/core/token/tests.rs::prefixes_limit_the_topics_a_token_reaches`
  - `tests/auth.rs::prefixes_confine_a_token_to_its_topics_including_the_dead_letter`

---

### UC-AUT-016

**Case:** On a distributor, the permission is checked only on it; the copies on the destination queues are not checked again.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::publishing_to_a_distributor_checks_only_the_distributor`

---

### UC-AUT-017

**Case:** The root token has full access (all permissions, no prefixes) and is the only one that uses `/tokens`; in logs and traces it appears as `token.id=root`.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::root_and_open_reach_everything_and_a_token_only_what_it_has`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::only_the_root_and_admin_tokens_manage_tokens`

---

### UC-AUT-018

**Case:** A token with an expired `expiresAt` is refused with `401`; each node ignores it and deletes it in the local sweep (`retention.sweep.interval`), without propagating.  
**Level:** C  
**Tests:**
  - `src/core/token/tests.rs::a_token_expires_at_the_given_instant`
  - `src/engine/tokens/tests.rs::an_expired_token_is_refused_and_the_sweep_erases_it`
  - `tests/auth.rs::an_expired_token_is_a_401`

---

### UC-AUT-020

**Case:** The token format is `tb_<id>.<secret>`, with a GUID v7 `id` and a secret of 256 random bits; the cluster stores only the salted hash of the secret (`SHA-256(salt ‖ secret)`, random salt per token).  
**Level:** U  
**Tests:**
  - `src/core/token/tests.rs::the_token_is_tb_id_dot_a_256_bit_secret`
  - `src/core/token/tests.rs::only_a_salted_hash_is_kept_and_it_verifies_the_secret`
  - `src/core/token/tests.rs::every_token_has_its_own_salt_so_equal_secrets_hash_differently`
  - `src/engine/tokens/tests.rs::creating_returns_the_full_token_once_and_it_authenticates`

---

### UC-AUT-021

**Case:** The complete token and the secret never go to logs or traces; only the `id` (`token.id`). The token `id` is never a metric attribute.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/metrics.rs::auth_requests_are_counted_by_outcome_and_the_token_id_is_never_a_label`

---

### UC-AUT-022

**Case:** `POST /tokens` with the root token  
**Then:** `201` `TokenCreated`, with `Location: /tokens/{id}` and the `token` field (the complete secret), which appears **only here**.  
**Level:** I  
**Tests:**
  - `src/engine/tokens/tests.rs::creating_returns_the_full_token_once_and_it_authenticates`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`

---

### UC-AUT-023

**Case:** Invalid `POST /tokens` (`permissions` empty or unknown, `name` outside the pattern, invalid prefix or above 20, `expiresAt` in the past)  
**Then:** `400`.  
**Level:** U, I  
**Tests:**
  - `src/engine/tokens/tests.rs::invalid_requests_are_400_with_the_field`
  - `tests/auth.rs::an_invalid_token_request_is_400`

---

### UC-AUT-024

**Case:** `/tokens` with a token that is not the root  
**Then:** `403` `auth.forbidden`. Without a token: `401`.  
**Level:** I  
**Tests:**
  - `src/http/auth/tests.rs::each_route_asks_for_its_own_permission`
  - `tests/auth.rs::only_the_root_and_admin_tokens_manage_tokens`

---

### UC-AUT-025

**Case:** `/tokens` with authentication off  
**Then:** `404` `route.not-found`.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::without_a_root_token_nothing_is_required_and_tokens_are_404`

---

### UC-AUT-026

**Case:** `GET /tokens` is paginated, never returns the secret, accepts `sort` by `id` and `createdAt` (not `name`, which is changeable  
**Then:** `400`) and the `name` filter by fragment.  
**Level:** I  
**Tests:**
  - `src/engine/tokens/tests.rs::listing_is_paginated_by_id_or_creation_and_filters_by_name`
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::listing_tokens_is_paginated_filtered_and_refuses_sorting_by_name`

---

### UC-AUT-027

**Case:** `GET /tokens/{id}`  
**Then:** `200` `Token`, or `404` `token.not-found`.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::creating_a_token_returns_the_secret_once_and_reading_never_does`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-028

**Case:** `PATCH /tokens/{id}` changes only `name` and `expiresAt` (`null` removes the deadline); permissions and prefixes are immutable (`400` `validation.immutable-field`).  
**Level:** I  
**Tests:**
  - `src/engine/tokens/tests.rs::patch_changes_only_the_name_and_the_expiry`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-029

**Case:** `DELETE /tokens/{id}`  
**Then:** `204`; the following calls with the token answer `401`, on **all** nodes, from the commit; a second `DELETE`: `404`.  
**Level:** I, C  
**Tests:**
  - `src/engine/tokens/tests.rs::revoking_makes_the_token_unknown_and_a_second_revoke_is_404`
  - `tests/auth.rs::patching_and_revoking_tokens`

---

### UC-AUT-032

**Case:** Each token is a row of the `tokens` table of `_node.db`, written at the commit on each node, with the salt and the hash of the secret (never the secret).  
**Level:** I, C  
**Tests:**
  - `src/engine/tokens/tests.rs::a_failed_write_leaves_no_visible_token`
  - `src/engine/tokens/tests.rs::the_store_keeps_the_tokens_that_were_created`
  - `src/storage/tokens/tests.rs::tokens_are_rows_of_the_node_db_with_the_hash_and_the_salt`
  - `src/storage/tokens/tests.rs::the_secret_is_never_in_the_database_file`

---

### UC-AUT-035

**Case:** The operations routes (on the operations port) and the cluster ones do not use the access token.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::the_operations_routes_do_not_use_the_access_token`

---

### UC-AUT-036

**Case:** Events  
**Then:** token created, changed and revoked (`info`, with the `id`); call refused by `401`/`403` (`warn`, sampled at volume). `taba.auth.requests` counts by `outcome` (`ok`, `unauthorized`, `forbidden`) and `taba.auth.tokens` shows how many the node knows.  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::auth_requests_are_counted_by_outcome_and_the_token_id_is_never_a_label`
  - `tests/metrics.rs::without_authentication_there_are_no_auth_series`

---

### UC-AUT-037

**Case:** A valid token calls a route it has no permission for  
**Then:** `403 auth.forbidden` carries `required`: `root` for the root-only routes, the name of the permission (`manage`, `admin`...) for the others; the other errors have no such field.  
**Level:** U, I  
**Tests:**
  - `src/core/error/tests.rs::a_forbidden_error_says_what_was_missing_and_other_errors_leave_the_field_out`
  - `tests/nodes_api.rs::a_forbidden_answer_says_what_the_token_lacked`

---

### UC-AUT-038

**Case:** `GET /auth/me`  
**Then:** Any valid token gets its `id`, `name`, `permissions`, `topicPrefixes`, `expiresAt` and `root: false`; the root token gets `root: true` and every permission; without a token, `401`.  
**Level:** I  
**Tests:**
  - `tests/nodes_api.rs::auth_me_says_who_the_token_is_and_what_it_can_do`

---

### UC-AUT-039

**Case:** What a token that is not the root may see: `GET /nodes`, `GET /stats/topics`, `/stats/topics/{topic}` and `/groups`  
**Then:** `GET /nodes` lists the nodes without addresses; the statistics list only has the topics within the token's prefixes and `/stats/topics/{topic}` outside them is `403 auth.out-of-scope`; the cluster and node routes ask for the root token.  
**Level:** U, I  
**Tests:**
  - `src/http/auth/tests.rs::the_monitoring_routes_split_between_any_token_and_the_root_token`
  - `tests/nodes_api.rs::any_valid_token_lists_the_nodes_and_sees_the_statistics_of_its_prefixes`

---


### UC-AUT-040

**Case:** On the client port the probes are open and `/metrics` takes any valid token  
**Then:** `GET /health/live` and `GET /health/ready` answer without a token; `GET /metrics` answers `401` without a token or with an invalid one and `200` with any valid token (no particular permission); the rest of the operations routes do not exist there (`404`). On the operations port `/metrics` needs no token.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::on_the_client_port_the_probes_are_open_and_the_metrics_take_any_valid_token`

---

### UC-AUT-041

**Case:** Every route of the client document has its requirement decided in the authorization table
**Then:** a route is matched by the template it is registered with, and the documents register it with the template they show; every documented route, but the probes (open on purpose), has an entry in `need()`, so renaming a path parameter cannot make a route skip its permission.
**Level:** U
**Tests:**
  - `src/http/auth/tests.rs::every_route_of_the_client_document_has_its_requirement_decided`

---

## LIM: Rate limit and client IP

### UC-LIM-001

**Case:** Publishing, consumption or administrative limit exceeded  
**Then:** `429` `rate-limit.exceeded` with `Retry-After`.  
**Level:** I  
**Tests:**
  - `src/http/ratelimit/tests.rs::the_retry_after_is_the_wait_for_a_whole_token`
  - `src/http/ratelimit/tests.rs::a_category_without_a_rate_is_unlimited`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`
  - `tests/ratelimit.rs::admin_and_consume_have_their_own_counters`
  - `tests/ratelimit.rs::without_configured_rates_nothing_is_limited`

---

### UC-LIM-002

**Case:** The categories are independent  
**Then:** publishing (`POST .../messages`), consumption (`GET`, `PUT .../visibility`, `DELETE` of a message) and administrative (`/topics`, `/tokens`). Exceeding one does not affect the others.  
**Level:** I  
**Tests:**
  - `src/http/ratelimit/tests.rs::categories_and_clients_count_separately`
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`
  - `tests/ratelimit.rs::admin_and_consume_have_their_own_counters`

---

### UC-LIM-003

**Case:** The limit is rate plus burst (`ratelimit.burst`)  
**Then:** short peaks within the burst are not rejected.  
**Level:** U, I  
**Tests:**
  - `src/http/ratelimit/tests.rs::a_burst_is_absorbed_and_then_the_rate_applies`
  - `src/http/ratelimit/tests.rs::without_a_burst_the_capacity_is_one_second_of_the_rate`
  - `src/http/ratelimit/tests.rs::tokens_never_pile_up_past_the_burst`
  - `tests/ratelimit.rs::exceeding_a_category_gives_429_with_retry_after_and_only_that_category`

---

### UC-LIM-004

**Case:** With a valid token (including the root), the counter key is the token `id` in each category, even changing IP; a `403` also counts on the token.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-005

**Case:** Without a valid token (authentication off, `401`, operations routes), the key is the client IP. Trying to guess tokens is not left without a limit.  
**Level:** I  
**Tests:**
  - `src/http/ratelimit/tests.rs::idle_counters_are_dropped_when_the_map_grows`
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`
  - `tests/ratelimit.rs::without_a_token_the_counter_is_per_client_ip_from_the_headers`

---

### UC-LIM-006

**Case:** Authentication comes before the rate limit, and a token only becomes a key **after being validated** (nobody exhausts another's limit by citing its `id`).  
**Level:** U, I  
**Tests:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-007

**Case:** All tokens have the same limit; there is no per-token limit.  
**Level:** I  
**Tests:**
  - `tests/auth.rs::the_rate_limit_counts_by_token_when_valid_and_by_ip_otherwise`

---

### UC-LIM-009

**Case:** `/health/live`, `/health/ready`, `/metrics` and `/health/cluster?scope=node` never answer `429`.  
**Level:** I  
**Tests:**
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::probes_are_exempt_and_the_cluster_query_has_its_own_category`

---

### UC-LIM-010

**Case:** `/health/cluster?scope=cluster` and `/metrics/topics` have their own category (`ratelimit.ops.rps`, per IP, with the same burst).  
**Level:** I  
**Tests:**
  - `src/http/ratelimit/tests.rs::routes_are_classified_by_category_and_the_probes_are_exempt`
  - `tests/ratelimit.rs::probes_are_exempt_and_the_cluster_query_has_its_own_category`

---

### UC-LIM-011

**Case:** The client IP comes from the first address of the first present header of `client-ip.headers` (default `X-Forwarded-For`), only if the connection comes from `client-ip.trusted-proxies` (empty  
**Then:** accepts from any origin). Absent or invalid header: the connection address applies.  
**Level:** U, I  
**Tests:**
  - `src/http/client_ip/tests.rs::the_first_address_of_the_first_present_header_wins`
  - `src/http/client_ip/tests.rs::a_missing_or_invalid_header_falls_back_to_the_connection`
  - `src/http/client_ip/tests.rs::addresses_with_a_port_and_ipv6_are_understood`
  - `src/http/client_ip/tests.rs::headers_are_only_trusted_from_the_trusted_proxies`
  - `tests/ratelimit.rs::without_a_token_the_counter_is_per_client_ip_from_the_headers`
  - `tests/ratelimit.rs::the_headers_only_count_from_the_trusted_proxies`

---

### UC-LIM-012

**Case:** With `client-ip.headers.enabled=false` the connection address always applies.  
**Level:** U, I  
**Tests:**
  - `src/http/client_ip/tests.rs::disabled_headers_always_use_the_connection`
  - `tests/ratelimit.rs::with_the_headers_disabled_a_forged_one_does_not_escape_the_limit`

---

### UC-LIM-013

**Case:** The resolved IP also applies to `operations.allowed-ips`.  
**Level:** I  
**Tests:**
  - `tests/ratelimit.rs::the_resolved_ip_is_the_one_checked_against_allowed_ips`

---

### UC-LIM-015

**Case:** `taba.ratelimit.rejected` counts the rejections by `category` (`publish`, `consume`, `admin`, `ops`) and `key` (`token` or `ip`).  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::rate_limit_rejections_are_counted_by_category_and_key_and_metrics_is_exempt`

---


## PAG: Pagination

### UC-PAG-001

**Case:** The response is `Page<T>`  
**Then:** `items` and, if there are more, `nextCursor`; **on the last page `nextCursor` does not exist**.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-002

**Case:** `limit` out of range  
**Then:** `400` `validation.out-of-range`. Malformed `cursor`: `400` `validation.invalid-cursor`.  
**Level:** U, I  
**Tests:**
  - `src/core/pagination/tests.rs::malformed_cursor_is_invalid_cursor`
  - `src/core/pagination/tests.rs::limit_must_be_between_one_and_the_maximum`
  - `src/engine/topics/tests.rs::a_cursor_only_works_with_the_sort_and_filter_that_made_it`
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-003

**Case:** Walking all the pages returns every item that existed during the whole walk, **without repeating**.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-005

**Case:** Cursor used with a `sort` or filters different from those that generated it  
**Then:** `400` `validation.invalid-cursor`.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::a_cursor_only_works_with_the_sort_and_filter_that_made_it`
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-006

**Case:** Repeating a cursor returns the same page while the data does not change, and the cursor does not expire.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::walking_the_pages_returns_everything_once_and_cursors_are_stable_and_bound_to_the_query`

---

### UC-PAG-007

**Case:** The order is total and stable  
**Then:** every ordering ends in a unique, immutable field (name or `id`); `sort=-createdAt` means `(createdAt desc, id asc)`; items with the same `createdAt` are not lost between pages.  
**Level:** U, I  
**Tests:**
  - `src/engine/topics/tests.rs::walking_every_page_returns_each_item_once_even_with_equal_creation_times`

---

### UC-PAG-008

**Case:** There is no `page`, `offset` or `total`.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::pages_carry_next_cursor_only_when_there_is_more_and_bad_params_are_400`

---

### UC-PAG-009

**Case:** `GET /metrics/topics` does not accept `sort` (`400`) and always paginates by name.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::pages_by_name_and_validates_the_parameters`

---

## CNV: Common API conventions (format, headers, errors, status)

### UC-CNV-001

**Case:** JSON in UTF-8, fields in `camelCase`, enumerations in lowercase with a hyphen, RFC 3339 dates in UTC with milliseconds (`2026-10-06T12:00:00.000Z`), durations in integer seconds, sizes in bytes, lowercase GUID v7 identifiers.  
**Level:** U, I  
**Tests:**
  - `src/core/id/tests.rs::generated_ids_are_lowercase_canonical_v7`
  - `src/core/time/tests.rs::formats_as_rfc3339_utc_with_milliseconds`

---

### UC-CNV-002

**Case:** Absent field = does not apply to the resource; `null` field = applies and has no value of its own (`ttl: null` uses the default).  
**Level:** I  
**Tests:**
  - `src/core/topic/tests.rs::queue_json_has_null_for_defaults_and_distributor_json_omits_queue_fields`

---

### UC-CNV-003

**Case:** Request with a JSON body and a `Content-Type` other than `application/json`  
**Then:** `415` `media.unsupported`. The exception is message publishing, which accepts any media type.  
**Level:** I  
**Tests:**
  - `tests/topics.rs::a_json_route_with_another_content_type_is_415`

---

### UC-CNV-004

**Case:** `X-Request-Id` of 1 to 128 characters of `A-Za-z0-9_-.:`  
**Then:** returned equal in the response. Without it, the node generates a GUID v7. It is **always** in the response and equal to the `requestId` of the error body.  
**Level:** I  
**Tests:**
  - `src/http/common/tests.rs::client_request_id_is_echoed_in_header_and_error_body`
  - `src/http/common/tests.rs::missing_or_invalid_request_id_is_replaced_by_a_guid_v7`
  - `src/http/common/tests.rs::unknown_route_is_a_404_error_response_with_matching_request_id`
  - `tests/listeners.rs::every_response_carries_the_request_id`

---

### UC-CNV-006

**Case:** Every error response (`4xx` and `5xx`) of a JSON route is `ErrorResponse`  
**Then:** `code` (format `group.reason`), `message`, `requestId` and, when there is more than one problem, `details` (`field`, `code`, `message`). The status goes only in the status line.  
**Level:** U, I  
**Tests:**
  - `src/core/error/tests.rs::codes_follow_group_dot_reason`
  - `src/core/error/tests.rs::error_response_has_no_status_nor_retry_after`
  - `src/core/error/tests.rs::details_carry_field_code_and_message`
  - `src/http/common/tests.rs::unknown_route_is_a_404_error_response_with_matching_request_id`

---

### UC-CNV-007

**Case:** `Retry-After` only in the header (never in the body), on `429`, `503` and `403` for freezing; `WWW-Authenticate: Bearer` on `401`; `Location` on `201`.  
**Level:** I  
**Tests:**
  - `src/http/common/tests.rs::retry_after_goes_only_in_the_header_and_401_asks_for_a_bearer_token`

---

### UC-CNV-008

**Case:** Each catalogue `code` has a fixed HTTP status; the whole catalogue is checked by a table test (code → status).  
**Level:** U  
**Tests:**
  - `src/core/error/tests.rs::every_code_has_the_documented_status`

---

### UC-CNV-009

**Case:** Unknown field in the request  
**Then:** `400` `validation.unknown-field`. Required field missing: `400` `validation.required`, with the field in `details`.  
**Level:** U, I  
**Tests:**
  - `tests/topics.rs::unknown_and_missing_fields_are_400_naming_the_field`

---

### UC-CNV-010

**Case:** `POST`, `PATCH` and `DELETE` that do not expect a body ignore whatever comes; `204` never has a body.  
**Level:** I  
**Tests:**
  - `tests/operations.rs::a_post_that_expects_no_body_ignores_the_one_it_gets`
  - `tests/topics.rs::a_delete_ignores_the_body_it_gets_and_the_204_has_none`

---

### UC-CNV-011

**Case:** The client port answers `404` `route.not-found` to `/cluster/*`; the cluster port answers `404` to anything that is not `/cluster/*`.  
**Level:** I, C  
**Tests:**
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`

---

### UC-CNV-012

**Case:** Unexpected node failure  
**Then:** `500` `internal.error` with `ErrorResponse`.  
**Level:** U, I  
**Tests:**
  - `src/http/common/tests.rs::a_panic_in_a_handler_becomes_500_internal_error`

---


### UC-CNV-014

**Case:** An error inside a partial result (`200` of the distributor) carries `error` with `code` and `message`, **without status**  
**Then:** the item's status is that of the `code` in the catalogue.  
**Level:** I  
**Tests:**
  - `tests/messages.rs::a_distributor_copies_to_matching_queues_and_reports_each_one`

---

### UC-CNV-016

**Case:** `Accept: application/openmetrics-text` on `/metrics` asks for OpenMetrics; the default is Prometheus text.  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-CNV-017

**Case:** The reused DTOs `Ack`, `StreamEnd` and `ConditionEntry` have the fields and types defined in 06d §8.3.7 (the `severity` of `ConditionEntry` only takes `info`, `warning` or `critical`).  
**Level:** U  
**Tests:**
  - `src/cluster/health/tests.rs::a_condition_entry_has_a_name_and_a_severity_of_the_document`
  - `src/cluster/protocol/dto_tests.rs::the_ack_and_the_stream_end_have_the_fields_and_types_of_the_document`
**Note:** `Severity` today only has `warning` and `critical`: no condition produces `info`.

---

### UC-CNV-018

**Case:** Each port serves the OpenAPI document of its API and an interface, without a token
**Then:** `GET /openapi/client.json` and `/docs` on `api.port`, `GET /openapi/monitoring.json` and `/docs` on `operations.port`, open even with authentication on; each port has only its own document (`404` for the other); the interface is embedded (nothing loaded from the internet); the document lists only the routes the node has (no `/tokens` without authentication, no `/metrics` with Prometheus off) and is the one `taba openapi` prints. Every operation has an `operationId`, a tag and responses, every error answer is an `ErrorResponse` (the probes, a `HealthResponse`), and the probes need no token where the rest does.
**Level:** I, U
**Tests:**
  - `tests/openapi.rs::each_port_serves_its_own_document_and_an_interface_without_a_token`
  - `tests/openapi.rs::the_document_only_lists_the_routes_the_node_has`
  - `tests/openapi.rs::every_operation_is_identified_described_and_has_its_security_decided`
  - `tests/openapi.rs::the_documentation_of_the_monitoring_port_follows_its_ip_filter`

---

### UC-CNV-019

**Case:** The answers a node really gives are the ones the documents describe
**Then:** for each route exercised (topics, messages with JSON, text and binary bodies and a distributor, tokens, who the caller is, the nodes, the cluster, the statistics, the probes and the monitoring routes) the status is listed in the document and the JSON body matches the schema documented for that status; the check itself refuses a body that does not match and a status that is not listed.
**Level:** I
**Tests:**
  - `tests/openapi.rs::the_answers_of_the_client_api_match_the_document`
  - `tests/openapi.rs::the_answers_of_the_tokens_routes_match_the_document`
  - `tests/openapi.rs::the_answers_of_the_monitoring_port_match_the_document`
  - `tests/openapi.rs::the_check_refuses_a_body_that_does_not_match_its_schema`
  - `tests/openapi.rs::the_check_refuses_a_status_the_document_does_not_list`

---

### UC-CNV-020

**Case:** The documents in `openapi/` are the ones the server generates
**Then:** `openapi/client.json` and `openapi/monitoring.json` equal what `taba openapi` prints (the `info.version` aside); otherwise the test fails and says how to generate them again.
**Level:** I
**Tests:**
  - `tests/openapi.rs::the_published_documents_are_the_ones_the_server_generates`

---

## CNT: Connectors

### UC-CNT-001

**Case:** The connector offers the nine operations (`createTopic`, `getTopic`, `listTopics`, `updateTopic`, `deleteTopic`, `publish`, `consume`, `ack`, `changeVisibility`), each building the verb, path, query, headers and body of the corresponding route.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_nine_operations_build_the_expected_routes`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::create_topic_sends_only_what_was_filled`
  - `connectors/rust/client/src/client/tests.rs::topic_operations_build_the_expected_routes`

---

### UC-CNT-002

**Case:** `baseUrl` is required. The defaults are `connectTimeout` 5 s, `requestTimeout` 30 s, `maxAttempts` 3 and `userAgent` `taba-<language>/<version>`.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::config_defaults_and_required_base_url`
  - `connectors/rust/client/src/client/tests.rs::config_defaults_and_required_base_url`

---

### UC-CNT-003

**Case:** With `token`, all calls carry `Authorization: Bearer`; without it, none carries the header.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_token_goes_in_every_call_and_without_it_no_call_carries_authorization`
  - `connectors/rust/client/src/client/tests.rs::headers_carry_the_token_the_agent_and_the_optional_ids`
  - `connectors/rust/client/src/client/tests.rs::without_a_token_no_call_carries_authorization`

---

### UC-CNT-004

**Case:** `publish` receives the bytes and the `Content-Type`; `publishRawJson` and `publishText` only choose the `Content-Type`; the options `groupId`, `deduplicationId`, `contentType` and `requestId` become the correct query and headers. The connector does not validate the body.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_distributor_answer_lists_each_copy_and_the_partial_failures`
  - `connectors/rust/client/src/client/tests.rs::publish_sends_raw_bytes_with_content_type_and_query`

---

### UC-CNT-005

**Case:** `consume` sends only the values the person informed (`maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate`, `volatile`, `idleTimeout`, `fifo`, `tags` as `tag.<key>`) and **applies no defaults of its own**.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_sends_only_what_was_given`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::auto_create_parameters_are_left_out_without_auto_create`
  - `connectors/rust/client/src/client/tests.rs::consume_sends_only_what_was_given_and_reads_for_wait_time_plus_ten`
  - `connectors/rust/client/src/client/tests.rs::auto_create_parameters_are_left_out_without_auto_create`

---

### UC-CNT-006

**Case:** `ack` and `changeVisibility` send the `receiptToken` in the query.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::ack_and_visibility_put_the_receipt_token_in_the_query`
  - `connectors/rust/client/src/client/tests.rs::ack_and_visibility_put_the_receipt_token_in_the_query`

---

### UC-CNT-007

**Case:** `listTopics` accepts `limit`, `cursor` and `name`, and offers an iterator over all the pages where the language allows.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::list_all_topics_follows_next_cursor_until_the_last_page`
  - `connectors/rust/client/src/client/tests.rs::list_all_topics_follows_next_cursor_until_the_last_page`

---

### UC-CNT-008

**Case:** The read timeout of `consume` is `waitTime + 10 s`, never the `requestTimeout`  
**Then:** a `waitTime` of 20 s with a `requestTimeout` of 30 s does not drop the call.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::long_polling_waits_on_an_empty_queue_and_returns_as_soon_as_a_message_arrives`
  - `connectors/rust/client/src/client/tests.rs::consume_sends_only_what_was_given_and_reads_for_wait_time_plus_ten`

---

### UC-CNT-009

**Case:** A timeout in `consume` is **not** repeated  
**Then:** the connector returns the timeout error (`client.timeout`).  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_timeout_is_not_repeated_and_is_a_client_side_error`
  - `connectors/rust/client/src/client/tests.rs::a_timeout_after_sending_is_not_repeated`
  - `connectors/rust/client/src/transport/tests.rs::a_silent_server_times_out_after_the_request_was_sent`

---

### UC-CNT-010

**Case:** `429` and `503` are repeated up to `maxAttempts` waiting for the `Retry-After`; without it, they wait 1 s, 2 s, 4 s.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::retries_429_and_503_waiting_for_retry_after_then_backoff`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts`
  - `connectors/rust/client/src/client/tests.rs::retries_429_and_503_waiting_for_retry_after_then_backoff`
  - `connectors/rust/client/src/client/tests.rs::backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts`

---

### UC-CNT-011

**Case:** On a `503` of an operation that creates or deletes a topic, the `409` `topic.already-exists` and the `404` of the repetition count as success.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/rust/client/src/client/tests.rs::after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success`
  - `connectors/rust/client/src/client/tests.rs::without_a_previous_503_a_409_or_404_is_a_plain_error`

---

### UC-CNT-012

**Case:** `502` and `504` are only repeated on publishing to a FIFO queue; outside it the error rises without repeating.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::gateway_errors_are_only_repeated_when_publishing_to_a_fifo`
  - `connectors/rust/client/src/client/tests.rs::gateway_errors_are_only_repeated_when_publishing_to_a_fifo`

---

### UC-CNT-013

**Case:** Network failure **before** sending the body (connection refused, DNS) is repeated like a `503`; **after** sending the body it is not repeated.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::network_failure_before_sending_is_repeated_and_after_is_not`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_real_refused_connection_is_a_network_error_after_the_attempts`
  - `connectors/rust/client/src/client/tests.rs::network_failure_before_sending_is_repeated_and_after_is_not`
  - `connectors/rust/client/src/http/tests.rs::tells_apart_a_connection_closed_before_any_byte_and_after_some`
  - `connectors/rust/client/src/transport/tests.rs::a_refused_connection_is_a_failure_before_sending`
  - `connectors/rust/client/src/transport/tests.rs::a_silent_server_times_out_after_the_request_was_sent`
  - `connectors/rust/client/src/transport/tests.rs::a_connection_dropped_mid_response_is_an_io_failure`

---

### UC-CNT-014

**Case:** Any `4xx` (except `429`) is never repeated.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_4xx_other_than_429_is_never_repeated`
  - `connectors/rust/client/src/client/tests.rs::without_a_previous_503_a_409_or_404_is_a_plain_error`
  - `connectors/rust/client/src/client/tests.rs::a_4xx_other_than_429_is_never_repeated`

---

### UC-CNT-015

**Case:** The message `body` comes out as **bytes**  
**Then:** JSON becomes the JSON text as it came; `text/*` in UTF-8 becomes the string in UTF-8; `bodyEncoding: base64` becomes the decoded bytes.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::text_json_and_binary_bodies_come_back_as_the_same_bytes`
  - `connectors/rust/client/src/client/tests.rs::messages_come_back_as_bytes_for_json_text_and_base64`

---

### UC-CNT-016

**Case:** The `Message` carries `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt`, `invisibleUntil` and, when present, `groupId`, `deduplicationId`, `deadLetteredAt` and `deadLetterReason`; unknown fields in the response are ignored.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/rust/client/src/client/tests.rs::messages_come_back_as_bytes_for_json_text_and_base64`

---

### UC-CNT-017

**Case:** A single form of error per connector, with `status`, `code`, `message`, `requestId`, `retryAfter` and `details`; network and timeout errors have `status` zero and `code` `client.network` or `client.timeout`.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_timeout_is_not_repeated_and_is_a_client_side_error`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_error_carries_the_error_response_fields_and_the_headers`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::a_non_json_error_still_reports_a_status_code`
  - `connectors/rust/client/src/client/tests.rs::a_timeout_after_sending_is_not_repeated`
  - `connectors/rust/client/src/client/tests.rs::the_error_carries_the_error_response_fields`
  - `connectors/rust/client/src/client/tests.rs::retry_after_and_request_id_come_from_the_headers_and_a_non_json_error_still_reports`

---

### UC-CNT-018

**Case:** The connector sends `Accept: application/json`, `User-Agent` and, if informed, `X-Request-Id` and `traceparent`, and reads `Retry-After` and `X-Request-Id` from the response.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::headers_carry_the_agent_and_the_optional_ids`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_error_carries_the_error_response_fields_and_the_headers`
  - `connectors/rust/client/src/client/tests.rs::headers_carry_the_token_the_agent_and_the_optional_ids`
  - `connectors/rust/client/src/client/tests.rs::retry_after_and_request_id_come_from_the_headers_and_a_non_json_error_still_reports`
  - `connectors/rust/client/src/client/tests.rs::the_reply_exposes_the_request_id_header`

---

### UC-CNT-019

**Case:** HTTP/1.1 with a persistent connection when the HTTP library offers it at no cost; no HTTP/2.  
**Level:** K  
**Tests:**
  - `connectors/rust/client/src/transport/tests.rs::sends_http_1_1_over_a_plain_socket_and_reads_the_reply`

---

### UC-CNT-020

**Case:** Thread safety  
**Then:** the client is safe for concurrent use in Java, C#, Go, Rust and Node; in C, C++, Python, PHP and Ruby, one client per thread applies, and each folder's README says how.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::many_threads_can_use_one_client_at_the_same_time`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::cancelling_the_token_cancels_the_call_without_a_client_error`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::many_parallel_publishers_share_one_client`
  - `connectors/rust/client/src/client/tests.rs::the_client_is_safe_to_share_between_threads`
  - `connectors/rust/client/src/client/tests.rs::many_threads_can_use_one_client_at_the_same_time`

---

### UC-CNT-021

**Case:** The minimal JSON reader passes the common set of documents in `contrato/fixtures/` (escapes, Unicode and surrogate pairs, large numbers, deep nesting, spaces and line breaks, empty values) and delivers the `body` as a text fragment without reserializing.  
**Level:** K  
**Tests:**
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

**Case:** The own base64 (C, C++, Rust) encodes and decodes like the standard one, including with padding (`=`) and arbitrary bytes.  
**Level:** K  
**Tests:**
  - `connectors/rust/client/src/base64/tests.rs::matches_the_rfc_4648_vectors`
  - `connectors/rust/client/src/base64/tests.rs::round_trips_arbitrary_bytes_and_the_png_header`
  - `connectors/rust/client/src/base64/tests.rs::accepts_missing_padding_and_rejects_garbage`

---

### UC-CNT-023

**Case:** No external dependencies beyond the language's standard library (in C, only `libcurl`); in C++ the single header wraps `libtaba`.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::the_only_package_is_the_reference_assemblies_for_the_classic_target`
  - `connectors/rust/client/src/client/tests.rs::the_only_dependencies_are_rustls_and_its_root_certificates`

---

### UC-CNT-024

**Case:** The code builds and passes the tests **on the base version and the latest** of the language, without using a feature later than the base version.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::the_library_targets_classic_and_modern_dotnet_in_csharp_7_3`

---

### UC-CNT-025

**Case:** Text goes back and forth always in UTF-8, explicitly, whatever the local *charset*.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_sends_raw_bytes_with_content_type_and_query`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::messages_come_back_as_bytes_for_json_text_and_base64`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::quote_round_trips_through_decode`
  - `connectors/rust/client/src/json/tests.rs::quote_round_trips_through_decode`

---

### UC-CNT-026

**Case:** The fixtures of each route check the built request and the reading of a ready-made response  
**Then:** `ErrorResponse`, `Page<T>`, JSON body, text and base64, and the `Retry-After`.  
**Level:** K  
**Tests:**
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

---

### UC-CNT-027

**Case:** The integration scenarios pass against a Taba in development mode  
**Then:** create topic, publish, consume with long polling, ack, invisibility, FIFO queue with a duplicate, distributor with partial failure and a token without permission.  
**Level:** K  
**Tests:**
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

**Case:** Each connector declares the version of the API contract it implements, and ignores new fields that the server adds.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared`
  - `connectors/csharp/Taba.Client.Tests/JsonTests.cs::fields_look_up_typed_values_and_ignore_unknown_ones`
  - `connectors/rust/client/src/client/tests.rs::unknown_fields_in_responses_are_ignored`
  - `connectors/rust/client/src/json/tests.rs::fields_look_up_typed_values_and_ignore_unknown_ones`

---

### UC-CNT-029

**Case:** `updateTopic` goes out as a real `PATCH`, without `X-HTTP-Method-Override`, in each connector, and the connector makes no HTTP connection of its own for that verb.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_nine_operations_build_the_expected_routes`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::update_topic_is_a_real_patch_without_a_method_override_header`
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::patch_uses_the_http_method_and_never_the_override_header`
  - `connectors/rust/client/src/client/tests.rs::topic_operations_build_the_expected_routes`

---

### UC-CNT-030

**Case:** The connector uses the platform's TLS, with no TLS stack of its own (in Rust, `rustls`), requires TLS 1.2 as a minimum (where the library allows configuring) and **never** turns off certificate verification.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/PackagingTests.cs::tls_is_the_platforms_and_certificate_checks_are_never_turned_off`
  - `connectors/rust/client/src/transport/tests.rs::tls_config_accepts_a_pem_ca_and_rejects_files_without_certificates`

---

### UC-CNT-031

**Case:** The `subscribe(handler)` (long polling, handler call and automatic ack).  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_sync_subscribe_blocks_until_cancelled`
  - `connectors/csharp/Taba.Client.Tests/IntegrationTests.cs::subscribe_handles_and_acks_every_message_until_cancelled`
  - `connectors/rust/client/src/client/tests.rs::subscribe_acks_only_what_the_handler_accepted_and_stops_on_demand`

---

### UC-CNT-032

**Case:** In languages with an async model (C#, Java, Python 3.5+, Node.js, Go and Rust), the nine operations have the async form, with the same request building, the same call retry, the same `consume` timeout and the same errors as the synchronous one. C, C++, PHP and Ruby only have the synchronous one.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_async_forms_do_the_same_as_the_sync_ones`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::every_operation_has_a_sync_and_an_async_form`

---

### UC-CNT-033

**Case:** In languages with platform serialization (C#, Java, Go, Node.js, Python...), `publishJson(topic, object)` serializes the object (default serializer or the one from the configuration) and publishes as `application/json`; ready JSON goes in `publishRawJson`.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::publish_json_serializes_the_object_and_sends_it_as_json`
  - `connectors/rust/client/src/client/handlers/tests.rs::publish_object_serializes_and_sends_json`

---

### UC-CNT-034

**Case:** `consume(topic, options, handler)` (and `consume<T>`, with the body already deserialized; a body that is not a `T` gives `client.invalid-response` without deleting) delivers each message, with all the properties, to a context with `delete` (default `true`  
**Then:** deletes if the `handler` ends without an error; `false` leaves it in the queue; an error does not delete and rises) and `changeVisibility`; while the `handler` runs, a task renews the invisibility every third of it, and the value changed by the `handler` applies to the following renewals.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::consume_of_t_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t`
  - `connectors/csharp/Taba.Client.Tests/ClientTests.cs::the_sync_consume_with_a_handler_works_like_the_async_one`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_with_deletes_by_default_keeps_on_request_and_never_after_an_error`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_with_renews_the_visibility_while_the_handler_runs_and_lets_it_change`
  - `connectors/rust/client/src/client/handlers/tests.rs::change_visibility_in_the_handler_becomes_the_renewal_value`
  - `connectors/rust/client/src/client/handlers/tests.rs::consume_typed_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t`

---

### UC-CNT-035

**Case:** In a language with a dependency injection and options container (C#  
**Then:** `AddTabaClient`), the client is registered as a *singleton* of the interface, with the configuration coming from the options mechanism (code, `appsettings.json` or `Configure`), the container's serializer applies when the configuration does not bring one, and an invalid address fails with `client.invalid-config` when resolving the client.  
**Level:** K  
**Tests:**
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_registers_one_singleton_with_the_config_from_code`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_binds_a_configuration_section_and_uses_a_registered_serializer`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::add_taba_client_with_a_section_path_reads_the_configuration_only_when_the_client_is_resolved`
  - `connectors/csharp/Taba.Client.Tests/DependencyInjectionTests.cs::an_invalid_base_url_fails_when_the_client_is_resolved_with_client_invalid_config`

---


## CLI: `taba-cli`, the client command line

### UC-CLI-001

**Case:** The connection comes from the profile; the API commands have no connection option (`--url`, `--token` etc.), only `--profile`, `-o`, `-q` and `--request-id`. The precedence of each field is `TABA_CLI_*` variable, profile and built-in value. There is no `ops-url`: the CLI only uses the client API.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/args/tests.rs::reads_values_flags_and_positionals_in_any_order`
  - `connectors/rust/cli/src/args/tests.rs::accepts_equals_attached_short_values_and_a_dash_value`
  - `connectors/rust/cli/src/args/tests.rs::repeated_options_keep_every_value_and_the_last_wins_for_single_reads`
  - `connectors/rust/cli/src/args/tests.rs::double_dash_ends_options_and_a_lone_dash_is_positional`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`
  - `connectors/rust/cli/tests/commands.rs::the_default_profile_gives_the_url_and_the_token_from_its_file`

---

### UC-CLI-002

**Case:** The token goes in `Authorization: Bearer`, coming from the profile's `token-file` field (first line) or `token`, or from `TABA_CLI_TOKEN`; without a token, the call goes without the header. `config show` never prints the token.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/settings/tests.rs::the_token_comes_from_the_highest_level_and_the_file_wins_a_tie`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`
  - `connectors/rust/cli/tests/commands.rs::the_default_profile_gives_the_url_and_the_token_from_its_file`
  - `connectors/rust/cli/tests/commands.rs::the_token_variable_works_without_any_profile`

---

### UC-CLI-003

**Case:** There is no option to turn off certificate verification; the profile's `ca-file` field adds a CA to trust.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::there_is_no_option_to_turn_off_certificate_checks_and_a_missing_ca_file_exits_5`

---

### UC-CLI-004

**Case:** The `json` output is the server's response with no fields added; `table` and `raw` only change the form. `204`s print nothing, and `--quiet` prints nothing on success.  
**Level:** K  
**Tests:**
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

**Case:** The API error goes to standard error with `code`, message and `requestId` (and the `Retry-After` of `429` and `503`), and the exit code is `1`. Wrong usage exits with `2`, connection failure with `3`, timeout with `4` and unreadable file with `5`.  
**Level:** K  
**Tests:**
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

**Case:** `topic create` sends only the options given, with `--tag`, `--prefix` repeatable, and the server refuses what does not apply to the type.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::topic_create_sends_only_the_options_given`
  - `connectors/rust/cli/tests/commands.rs::topic_create_needs_a_valid_type_and_well_formed_numbers_and_tags`
  - `connectors/rust/cli/tests/commands.rs::the_server_refusal_for_a_field_that_does_not_fit_the_type_is_shown`

---

### UC-CLI-007

**Case:** `topic list` passes on `name`, `sort`, `limit` and `cursor`; with `--all` it walks all the pages by `nextCursor` and prints all the items.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::topic_list_passes_name_sort_limit_and_cursor`
  - `connectors/rust/cli/tests/commands.rs::topic_list_all_follows_every_page_and_prints_all_the_items`

---

### UC-CLI-008

**Case:** `topic get` and `topic update`  
**Then:** the `update` sends only what was given, `default` becomes `null` and `--clear-tags` sends the empty map; the immutable fields do not exist as an option.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::topic_get_and_update_send_only_what_was_given`
  - `connectors/rust/cli/tests/commands.rs::immutable_fields_are_not_options_and_an_empty_update_is_a_usage_error`

---

### UC-CLI-009

**Case:** `topic delete` and `token revoke` ask for confirmation on a terminal and, off it, only proceed with `--yes` (without it they exit with `2`).  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::delete_without_yes_refuses_outside_a_terminal`
  - `connectors/rust/cli/tests/commands.rs::in_a_terminal_delete_asks_and_only_yes_goes_on`
  - `connectors/rust/cli/tests/commands.rs::yes_skips_the_question`

---

### UC-CLI-010

**Case:** `publish` accepts the body by argument, by `--file` (raw bytes) or by `--stdin`, with `--content-type` (default `application/json`), `--json`, `--text`, `--group-id` and `--deduplication-id`. Does not validate the body, and `--id-only` prints only the `id`.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::publish_takes_the_body_from_the_argument_the_file_or_stdin`
  - `connectors/rust/cli/tests/commands.rs::publish_wants_exactly_one_body_source_and_one_content_type_choice`
  - `connectors/rust/cli/tests/commands.rs::publish_id_only_and_count`
  - `connectors/rust/cli/tests/commands.rs::publish_does_not_validate_the_body_the_server_does`
  - `connectors/rust/cli/tests/commands.rs::a_distributor_answer_is_printed_whole_even_with_id_only`

---

### UC-CLI-011

**Case:** `publish` does not repeat the call by itself after `502` or `504` outside FIFO; on FIFO the repetition is safe.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::after_a_502_outside_a_fifo_publish_is_not_repeated`
  - `connectors/rust/cli/tests/commands.rs::after_a_502_on_a_fifo_publish_is_repeated_because_the_primary_deduplicates`

---

### UC-CLI-012

**Case:** `consume` sends only the parameters informed (`maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate` and its parameters) and prints the whole message, with the `receiptToken`. `--exit-empty` exits with `6` when the list comes empty, and `--ack` confirms each delivered message.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::consume_sends_only_the_given_parameters_and_prints_the_whole_message_with_the_receipt`
  - `connectors/rust/cli/tests/commands.rs::the_auto_create_options_need_auto_create`
  - `connectors/rust/cli/tests/commands.rs::consume_raw_prints_only_the_body_one_per_line_and_ack_confirms_each_message`
  - `connectors/rust/cli/tests/commands.rs::exit_empty_returns_6_only_when_nothing_came`
  - `connectors/rust/cli/tests/commands.rs::consume_binary_bodies_come_out_decoded_in_raw_mode`

---

### UC-CLI-013

**Case:** `subscribe` consumes in a loop with long polling; with `--exec`, it confirms only if the command exits with `0`, and without it, prints and confirms. `--limit` ends after `n` messages, and `Ctrl+C` ends without confirming the message in progress.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::subscribe_prints_each_message_confirms_it_and_stops_at_the_limit`
  - `connectors/rust/cli/tests/commands.rs::subscribe_exec_runs_the_command_with_the_body_and_confirms_only_on_exit_zero`
  - `connectors/rust/cli/tests/commands.rs::subscribe_keeps_going_when_the_receipt_went_stale_during_processing`

---

### UC-CLI-014

**Case:** `ack` and `visibility` require `--receipt` (and `--seconds` on the second); a stale `receiptToken` returns `message.stale-receipt` (`409`) and exit code `1`.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::ack_and_visibility_need_the_receipt_and_visibility_needs_seconds`
  - `connectors/rust/cli/tests/commands.rs::a_stale_receipt_is_a_409_error_with_exit_1`

---

### UC-CLI-015

**Case:** `token create` sends name, permissions, prefixes and `expiresAt`, and shows the secret only once (`--token-only` prints only it); `token list` and `token get` never show the secret; `token update` changes `name` and `expiresAt` (`none` removes the deadline). Without authentication, the `404 route.not-found` is shown as an error.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::token_create_sends_name_permissions_prefixes_and_expiry`
  - `connectors/rust/cli/tests/commands.rs::token_create_validates_the_permissions_without_the_server`
  - `connectors/rust/cli/tests/commands.rs::token_list_get_and_update_use_the_token_routes`
  - `connectors/rust/cli/tests/commands.rs::without_authentication_the_tokens_routes_answer_404_and_the_cli_says_so`

---

### UC-CLI-016

**Case:** `cluster status` and `node list` use the client API with the profile's token; `cluster status` accepts `--scope`, and the `health` commands do not exist because the CLI never calls the monitoring API.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/output/tests.rs::the_cluster_status_table_has_one_row_per_node_and_per_condition`
  - `connectors/rust/cli/tests/commands.rs::cluster_status_goes_through_the_client_api_with_the_token`
  - `connectors/rust/cli/tests/commands.rs::cluster_status_validates_the_scope`
  - `connectors/rust/cli/tests/commands.rs::the_cli_has_no_health_commands_because_it_never_calls_the_monitoring_api`

---

### UC-CLI-017

**Case:** `metrics` (with `--openmetrics` and `--node`), `stats topics`, `stats topic` and `stats groups` go through the client API (`/metrics`, `/nodes/{id}/metrics`, `/stats/topics*`) and pass on `scope`, `tag`, `condition`, `fields`, `groupBy` and pagination.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::metrics_prints_the_text_as_it_came_and_openmetrics_changes_the_accept_header`
  - `connectors/rust/cli/tests/commands.rs::stats_pass_scope_tags_condition_fields_group_by_and_pagination`

---

### UC-CLI-018

**Case:** `drain start`, `drain status` and `drain cancel` act on the node named by the required `--node` option, through `/nodes/{id}/drain` of the client API, and show the `DrainStatus`.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::drain_acts_on_the_node_named_by_the_option_through_the_client_api`
  - `connectors/rust/cli/tests/commands.rs::drain_without_a_node_is_a_usage_error_that_calls_nothing`

---

### UC-CLI-019

**Case:** `version` shows the program version and the contract version; `help <command>` shows the help of each command.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::version_shows_the_program_and_the_contract_version`
  - `connectors/rust/cli/tests/commands.rs::help_lists_the_commands_and_shows_each_one`

---

### UC-CLI-020

**Case:** The Docker image (Alpine) runs `taba-cli` as an unprivileged user, with the `ENTRYPOINT` on the program, and works against Compose.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/packaging.rs::the_dockerfile_builds_a_static_binary_and_runs_it_as_an_unprivileged_user_on_alpine`
  - `connectors/rust/cli/tests/packaging.rs::the_docker_script_offers_the_two_linux_platforms`
  - `connectors/rust/cli/tests/packaging.rs::the_compose_file_has_the_init_container_three_nodes_haproxy_and_the_cli_tool`
  - `connectors/rust/cli/tests/packaging.rs::the_haproxy_example_balances_the_client_api_and_passes_the_cluster_port_through_in_tcp`
  - `connectors/rust/cli/tests/packaging.rs::the_node_image_runs_the_taba_binary_without_privileges`

---

### UC-CLI-021

**Case:** The scripts `build.sh` and `build.ps1` produce the platform binary, and with `--target` the targets of the table in 19b §18.11.8, with `SHA256SUMS`.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/packaging.rs::the_unix_script_covers_the_four_unix_targets_with_checksums_and_a_locked_build`
  - `connectors/rust/cli/tests/packaging.rs::the_windows_script_covers_the_two_windows_targets_with_checksums_and_a_locked_build`
  - `connectors/rust/cli/tests/packaging.rs::the_scripts_do_not_try_to_cross_to_macos_from_another_system`

---

### UC-CLI-022

**Case:** Without a configuration file and without a variable or profile, `taba-cli` uses `http://127.0.0.1:8080` and no token.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/settings/tests.rs::without_file_variables_or_profile_the_built_in_defaults_apply`
  - `connectors/rust/cli/tests/commands.rs::without_any_configuration_it_calls_localhost_8080_without_a_token`

---

### UC-CLI-023

**Case:** The profile file sits in the user's configuration folder (`%APPDATA%\taba\config.yml`, `~/.config/taba/config.yml` or `~/Library/Application Support/taba/config.yml`); `TABA_CLI_CONFIG` changes the path, and on Unix the file is born with permission `0600`. `profile path` prints it.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/profile/tests.rs::the_file_lives_in_the_user_config_folder_of_each_system`
  - `connectors/rust/cli/src/profile/tests.rs::taba_cli_config_overrides_the_path`
  - `connectors/rust/cli/tests/commands.rs::profile_path_prints_the_config_file_location`
  - `connectors/rust/cli/tests/commands.rs::the_default_location_follows_the_system_when_the_override_is_absent`

---

### UC-CLI-024

**Case:** `profile add` creates a profile (the first becomes the default; `--default` makes another the default) and refuses, with `2`, a name that already exists or is invalid; `profile set` changes and `--unset` deletes fields; `profile remove` asks for confirmation or `--yes`, and removing the default leaves the file without a default.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/profile/tests.rs::the_first_profile_becomes_the_default_and_names_are_validated`
  - `connectors/rust/cli/src/profile/tests.rs::fields_are_validated_when_set`
  - `connectors/rust/cli/src/profile/tests.rs::a_file_with_the_old_ops_url_field_still_loads_and_drops_it_when_written_again`
  - `connectors/rust/cli/src/profile/tests.rs::removing_the_default_leaves_the_file_without_one`
  - `connectors/rust/cli/tests/commands.rs::profile_add_creates_the_file_the_first_profile_is_the_default_and_a_duplicate_exits_2`
  - `connectors/rust/cli/tests/commands.rs::profile_set_changes_fields_and_unset_clears_them`
  - `connectors/rust/cli/tests/commands.rs::profile_remove_asks_outside_yes_and_removing_the_default_leaves_none`

---

### UC-CLI-025

**Case:** `profile default` shows, sets or (with `--none`) removes the default; `profile list` marks the default and the profile in use; `profile show` never prints the token.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/profile/tests.rs::removing_the_default_leaves_the_file_without_one`
  - `connectors/rust/cli/src/profile/tests.rs::set_default_needs_an_existing_profile`
  - `connectors/rust/cli/tests/commands.rs::profile_default_shows_sets_and_clears_and_list_marks_default_and_session`
  - `connectors/rust/cli/tests/commands.rs::profile_show_never_prints_the_token`

---

### UC-CLI-026

**Case:** The profile used is that of `--profile`, otherwise that of `TABA_CLI_PROFILE`, otherwise the file's `default`; a `TABA_CLI_*` variable overrides only the field it covers, and a nonexistent profile exits with `2`. `config show` says the origin of each value (variable, profile, default).  
**Level:** K  
**Tests:**
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

**Case:** `profile session` writes nothing to disk  
**Then:** prints the lines for the terminal to apply (`TABA_CLI_PROFILE` with a name; the `TABA_CLI_*` variables with fields, which form the temporary profile; the undo with `--clear`), in the syntax of `--shell sh`, `powershell` or `cmd`, with the values escaped.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/commands/profile/tests.rs::session_lines_for_a_saved_profile_and_for_loose_fields`
  - `connectors/rust/cli/src/commands/profile/tests.rs::clearing_unsets_the_profile_and_every_field_variable`
  - `connectors/rust/cli/src/commands/profile/tests.rs::values_are_escaped_for_the_chosen_shell`
  - `connectors/rust/cli/src/commands/profile/tests.rs::shell_names_are_checked`
  - `connectors/rust/cli/tests/commands.rs::profile_session_prints_lines_and_writes_nothing`
  - `connectors/rust/cli/tests/commands.rs::profile_session_defaults_to_powershell_on_windows_and_checks_its_arguments`

---

### UC-CLI-028

**Case:** The profile file is read and rewritten whole, atomically (temporary file and rename); invalid YAML or an unreadable file exits with `5` and the error line, and an interrupted write does not leave the file half-written.  
**Level:** K  
**Tests:**
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

**Case:** `taba-cli profile` without a subcommand, on a terminal, opens the interactive screen  
**Then:** the profile list navigated with `↑`/`↓`, with the default marked (`●`) and the profile in use in the session; `Enter` opens the form, `n` creates, `d` sets the default, `q`/`Esc` exit.  
**Level:** K  
**Tests:**
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

**Case:** The profile form is navigated with `↑`/`↓`/`Tab`; text fields are edited directly, `output` is a choice (`←`/`→`) and `Perfil padrão` is a box (`Space`); an empty field shows the value that will apply and deleting the text is equivalent to `--unset`; `Ctrl+S` or the Save button writes the same as `profile add`/`set`.  
**Level:** K  
**Tests:**
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

**Case:** The form validates on the spot (URL, numbers, repeated name or with invalid characters)  
**Then:** the error appears in red under the field and Save does not write while there is an error.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/tui/form/tests.rs::invalid_values_get_a_message_under_the_field`
  - `connectors/rust/cli/src/tui/form/tests.rs::the_name_is_required_and_checked_but_an_edited_profile_may_keep_its_own_name`
  - `connectors/rust/cli/src/tui/screen/tests.rs::save_is_refused_while_a_field_is_invalid`

---

### UC-CLI-032

**Case:** The Test connection button does `GET /topics?limit=1` with the not-yet-saved values of the form and shows the result on one line; it only informs and does not prevent saving.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/tui/screen/tests.rs::test_connection_uses_the_unsaved_values_and_only_reports`
  - `connectors/rust/cli/src/tui/screen/tests.rs::testing_is_refused_while_the_form_has_errors`
  - `connectors/rust/cli/src/tui/tests.rs::the_connection_config_uses_builtin_values_where_the_profile_is_silent`
  - `connectors/rust/cli/src/tui/tests.rs::an_unreadable_token_file_or_ca_file_is_reported`
  - `connectors/rust/cli/tests/commands.rs::the_interactive_test_button_calls_the_client_api_with_the_unsaved_values`

---

### UC-CLI-033

**Case:** Remove (`Del` or `r`) asks for confirmation `[s/N]` before deleting; `Esc` on a changed form asks for confirmation; `Ctrl+C` on any screen exits and restores the terminal; what was confirmed is already written and the unsaved form is discarded.  
**Level:** K  
**Tests:**
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

**Case:** `token` and `token-file` are shown and edited in view in the form (the token in `config.yml` is not encrypted); `profile show` and `config show` still do not show the token.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/tui/form/tests.rs::the_token_is_shown_in_plain_text`

---

### UC-CLI-035

**Case:** The screen uses the alternate screen, ANSI and raw-mode input (`stty` on Linux and macOS, `SetConsoleMode` on Windows) and restores the terminal on exit; `NO_COLOR` removes the colors; a terminal smaller than 60x16 shows a warning, and the screen redraws when the size changes.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/src/tui/screen/tests.rs::a_terminal_smaller_than_60_by_16_shows_a_warning_instead_of_a_broken_screen`
  - `connectors/rust/cli/src/tui/screen/tests.rs::colors_are_only_emitted_when_asked_for`

---

### UC-CLI-036

**Case:** Off a terminal (redirected input or output), `taba-cli profile` without a subcommand does not open the screen  
**Then:** prints the help of the profile commands and exits with `2`.  
**Level:** K  
**Tests:**
  - `connectors/rust/cli/tests/commands.rs::outside_a_terminal_the_profile_command_prints_help_and_exits_2`

---


## ESC: Write: register + commit, quorum, rollback, commit ceiling

### UC-ESC-001

**Case:** The coordinator generates the GUID v7 **before** the register phase and computes, from the ID, the block and the responsible nodes (primary, secondary, tertiary).  
**Level:** U, C  
**Tests:**
  - `src/core/ring/tests.rs::the_id_alone_gives_the_block_and_the_primary_secondary_and_tertiary`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-ESC-002

**Case:** Phase 1 (register) goes to **all** `replication.factor` responsible nodes, not only to the quorum; each one writes the message as **pending** in memory and confirms.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-003

**Case:** Once the register's quorum of confirmations is reached (primary included), the coordinator decides the commit, calls the commit on the quorum nodes and **continues** the register + commit on the other replicas in the background, without blocking the response.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-004

**Case:** The response to the client with the ID goes out as soon as the **commit is confirmed on the quorum** (primary included); the replicas outside the quorum may still be pending at that instant.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-005

**Case:** Full durability (commit confirmed on all `replication.factor` replicas) is reached afterwards, in the background.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ESC-006

**Case:** The coordinator may be one of the destination nodes (including the primary), with no network call to itself.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-007

**Case:** The node that receives the request is the coordinator, whichever node it is.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-008

**Case:** The default of `write.quorum` is `floor(rf / 2) + 1`  
**Then:** 2 with rf 3, 3 with rf 5, 1 with rf 1.  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::write_quorum_defaults_to_floor_of_half_the_factor_plus_one`

---

### UC-ESC-009

**Case:** `write.quorum` is validated between 1 and `replication.factor`; outside that, the configuration is refused. Below the majority is accepted.  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::write_quorum_must_be_between_one_and_the_factor_and_below_the_majority_is_accepted`

---

### UC-ESC-010

**Case:** The quorum **always includes the primary**  
**Then:** with the primary not answering and the secondary and tertiary confirming, the commit is **not** decided, even if the number of confirmations is that of the quorum.  
**Level:** U, C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-011

**Case:** The quorum is the primary plus `write.quorum − 1` **any** other replicas.  
**Level:** U, C  
**Tests:**
  - `tests/replication.rs::the_answer_comes_only_after_the_quorum_committed_whichever_node_took_the_call`

---

### UC-ESC-012

**Case:** `write.quorum` is a **single** value for write, delivery (invisibility) and ack.  
**Level:** U, C  
**Tests:**
  - `src/cluster/service/tests.rs::write_quorum_is_a_single_value_for_write_delivery_and_ack`
  - `src/config/tests.rs::there_is_only_one_write_quorum_parameter`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-ESC-013

**Case:** Without enough nodes for the quorum, the operation fails **before** deciding (rollback of the register) and the API answers `503` `cluster.quorum-unavailable` with `Retry-After`.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-014

**Case:** With rf 3 and quorum 2, the write keeps working with **one** responsible node down. With two down (the primary and one), the write fails with `503`.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-015

**Case:** Register failure on a node before the quorum  
**Then:** the coordinator repeats the call to that node according to `write.retry.attempts` and `write.retry.backoff`.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_lost_call_is_retried_before_the_quorum_gives_up`

---

### UC-ESC-016

**Case:** Retries exhausted **without a quorum**  
**Then:** error to the client and **rollback** to all the nodes that had already confirmed, to discard the pending register. No node made anything effective.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-017

**Case:** A register without a commit or rollback is discarded by itself after `write.pending.ttl`; the rollback only speeds up the release.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_rollback_that_does_not_arrive_leaves_a_pending_that_expires_by_itself`

---

### UC-ESC-019

**Case:** A message discarded by rollback never appears to a consumer, and the generated ID is not reused.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-020

**Case:** Register or commit failure on a replica **outside the quorum**, after the decision  
**Then:** **there is never a rollback**; the coordinator insists, within `write.commit.timeout`.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-021

**Case:** The replica confirms within the ceiling  
**Then:** it starts to count as durable, with no new notice to the client (the response was already given).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-022

**Case:** The ceiling runs out without confirmation  
**Then:** the block stays below the full factor (never below the quorum), the `under-replicated` event (non-critical) is generated, `taba.write.commit_timeouts` goes up, the coordinator **keeps trying** in the background, and the node's health does not change.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`

---

### UC-ESC-023

**Case:** The commit is idempotent  
**Then:** repeating it on a replica that already applied it has no effect.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::applying_a_commit_twice_changes_nothing`

---

### UC-ESC-024

**Case:** When the late replica returns, the background commit reaches it and the block goes back to the full factor (the `under-replicated` condition goes away).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::with_a_node_down_only_the_publications_it_owns_fail_and_the_replica_catches_up`
  - `tests/replication.rs::an_ack_decided_by_the_quorum_is_not_undone_by_a_replica_that_is_down`

---

### UC-ESC-025

**Case:** `POST /cluster/topics/{topic}/messages/{id}/register`, `commit` and `rollback` (phases 1, 2 and abort) answer `200` `Ack` with the `operationId` and `accepted: true`.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-ESC-026

**Case:** All the phases are **idempotent by `operationId`**  
**Then:** repeating `register`, `commit` or `rollback` returns the same `Ack`, with no new effect.  
**Level:** U, C  
**Tests:**
  - `src/cluster/replication/replica/tests.rs::repeating_register_or_commit_returns_the_same_ack_without_a_new_effect`
  - `src/cluster/replication/replica/tests.rs::repeating_rollback_returns_the_same_ack_and_the_message_never_appears`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-ESC-029

**Case:** The `register` with `kind: "dead-letter"` creates the message in the dead-letter (with `deliveryCount: 0`, null `invisibleUntil`, `deadLetteredAt`, `deadLetterReason`) and removes the one from `sourceTopic` at the commit, in a single operation.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node`

---

### UC-ESC-030

**Case:** Node, topic and token require the confirmation of **all** nodes, with no reduced quorum; a message decides by quorum.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-ESC-037

**Case:** On the ack of a message, **every node that keeps a copy** (the primary and the replicas, in memory or on disk) writes the exclusion record (`id`, ring position and instant) to the topic's **log of excluded messages**, in the same transaction that deletes the message; on the FIFO queue the record goes with the short deduplication row  
**Then:** the id is recorded as dead on every node that had the copy; a message that was not acknowledged is not recorded. A message that was only in memory is recorded by its `id` and position.  
**Level:** U and C  
**Tests:**
  - `tests/excluded_log.rs::an_ack_records_the_exclusion_on_every_node_that_keeps_a_copy`
  - `src/storage/messages/excluded/tests.rs::removing_with_exclusion_deletes_the_message_and_records_it_in_the_same_transaction`
  - `src/storage/messages/excluded/tests.rs::a_message_that_was_only_in_memory_is_recorded_by_its_id_and_position`
  - `src/storage/messages/excluded/tests.rs::the_fifo_ack_records_the_exclusion_together_with_the_short_row`
  - `src/storage/messages/excluded/tests.rs::rows_are_listed_by_the_ranges_of_a_block_in_id_order`

---

### UC-ESC-038

**Case:** On sending a message to the dead-letter (`maxDeliveries` used up), every node that keeps a copy records the exclusion in the **source queue's** log  
**Then:** the id is dead in the source queue and still alive in the dead-letter (the log is per topic).  
**Level:** U and C  
**Tests:**
  - `tests/excluded_log.rs::the_dead_letter_records_the_exclusion_in_the_source_queue_on_every_node`
  - `src/storage/messages/excluded/tests.rs::moving_to_the_dead_letter_records_the_exclusion_in_the_source_only`

---


## ANE: Hash ring, blocks and roles

### UC-ANE-001

**Case:** The message ID becomes a hash, which falls at a ring position, which belongs to a block with the responsible nodes in order (primary, secondary, tertiary).  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::a_message_id_becomes_a_position_that_belongs_to_a_block_with_ordered_owners`

---

### UC-ANE-002

**Case:** Any node reaches the same primary for the same ID, from the ID and the member table alone.  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::any_node_reaches_the_same_owners_from_the_ids_alone`

---

### UC-ANE-003

**Case:** The nodes responsible for a block are always **distinct physical nodes**  
**Then:** when walking the ring, virtual-node positions of an already chosen physical node are skipped.  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::owners_are_distinct_physical_nodes`

---

### UC-ANE-004

**Case:** Each physical node occupies `ring.virtual-nodes` positions; more virtual nodes distribute the blocks better (the variation in the number of blocks per node decreases).  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::more_virtual_nodes_spread_the_blocks_more_evenly`

---

### UC-ANE-005

**Case:** The GUID v7 hash distributes the messages uniformly (the timestamp in the high bits does not concentrate), both for the message and for the topic `id` (managing node).  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::the_hash_spreads_time_ordered_ids_over_the_whole_ring`

---

### UC-ANE-008

**Case:** The block's primary is the delivery owner of its messages.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ANE-009

**Case:** On a FIFO queue the position is the hash of `topic id + ":" + groupId`, not that of the message ID (UC-FIF-023).  
**Level:** U  
**Tests:**
  - `src/core/ring/tests.rs::a_fifo_group_has_one_position_for_all_its_messages`

---

### UC-ANE-010

**Case:** When the member table changes (join, leave, register failure), the ring is recomputed and the roles of each block change accordingly; without the node that failed, the ring goes back to the previous one.  
**Level:** U, C  
**Tests:**
  - `src/cluster/members/tests.rs::the_ring_follows_the_table`
  - `src/core/ring/tests.rs::the_ring_is_recalculated_from_the_members_and_goes_back_without_the_failed_node`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---


## DEF: Topic and token definition replicated in the cluster

### UC-DEF-010

**Case:** The file stores **only the definition**  
**Then:** consumption data (depth, rates, histograms) never goes into it.  
**Level:** I  
**Tests:**
  - `tests/stored_definition.rs::the_file_keeps_only_the_definition_and_never_consumption_data`

---

### UC-DEF-011

**Case:** The dead-letter has no definition row in the `topics` table  
**Then:** it is derived from the source queue, created and deleted in the same operation.  
**Level:** I  
**Tests:**
  - `src/engine/topics/tests.rs::the_dead_letter_is_derived_from_its_queue_and_never_stored`

---

### UC-DEF-012

**Case:** Each token is a row of the `tokens` table of `_node.db` (with `id`, `name`, permissions, prefixes, dates, the salt and the salted hash of the secret, never the secret).  
**Level:** I  
**Tests:**
  - `tests/stored_definition.rs::each_token_is_a_row_with_the_salted_hash_and_never_the_secret`

---

### UC-DEF-025

**Case:** Each definition has an **owner** on the ring  
**Then:** the first active node at the hash position of `topic:<name>` (or `token:<id>`). The node that receives the client's call **proposes** the change to the owner (`definition-propose`), which decides; the API only answers when the change already also holds on the node that received the call.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner`
  - `tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers`

---

### UC-DEF-026

**Case:** The owner gives each change the next **version** (1, 2, 3...). Each node only applies a change of a version **greater than or equal to** the one it has  
**Then:** repeating is harmless and a late change (lower version) is ignored. All nodes end on the last version.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back`
  - `tests/definitions.rs::each_change_gets_the_next_version_and_every_node_ends_on_the_last`

---

### UC-DEF-027

**Case:** Removal leaves a **tombstone** with the version  
**Then:** a late creation or change, of a lower version, does not resurrect the topic (nor the revoked token).  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_late_change_does_not_bring_a_removed_topic_back`

---

### UC-DEF-028

**Case:** Two creations of the same name at the same time, on different nodes, reach the same owner, which serializes them  
**Then:** one answers `201` and the other `409` `topic.already-exists`.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict`

---

### UC-DEF-029

**Case:** The owner checks the change against its own state before deciding  
**Then:** `create` of an existing name is `409` `topic.already-exists`; `update` or `delete` of an unknown topic is `404` `topic.not-found` (of a token, `token.not-found`); `delete` with an `expectedId` that does not match is `409` `cluster.conflict`. Nothing is applied on any node.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::two_nodes_creating_the_same_name_at_once_leave_one_topic_and_one_conflict`
  - `tests/definitions.rs::the_owner_refuses_a_change_that_does_not_fit_the_current_state`
  - `tests/replication.rs::topic_definitions_reach_every_node_before_the_api_answers`

---

### UC-DEF-030

**Case:** Before answering, the owner writes the change to an **heir**  
**Then:** the first other owner of the position that answers. If there are other owners and none answers, the change is refused with `503` `cluster.quorum-unavailable` and is not applied on any node.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_change_with_no_other_owner_reachable_fails_with_503`

---

### UC-DEF-031

**Case:** A node that does not answer does not prevent the change  
**Then:** the owner answers after the nodes that answer within `write.commit.timeout` and the owner and the heir queue the definition state for all peers in the channel's `definition` frame (D127); the node that did not answer receives it when the channel returns, or is excluded if the queue overflows. The node that returns starts to have the definition (or the revocation).  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_topic_created_while_a_node_is_unreachable_reaches_it_when_it_returns`
  - `tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`

---

### UC-DEF-032

**Case:** Owner down and still in the table (until its exclusion)  
**Then:** the change fails with `503` `cluster.quorum-unavailable` and `Retry-After`, applying nothing, and the client repeats the call. If the owner is in failure, the code is `node.failed`. Reading stays local.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_change_whose_owner_is_unreachable_fails_with_503`

---

### UC-DEF-033

**Case:** `POST /cluster/definitions/propose` (`DefinitionProposal`  
**Then:** kind, operation, key, body and `expectedId`) and `POST /cluster/definitions/apply` (`DefinitionApply`: kind, key, version, `deleted` and body) answer with the decided `DefinitionApply`. The streams `GET /cluster/topics` and `/cluster/tokens` carry the `version` of each row. The token body carries only the `salt` and the `secretHash`, never the secret.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_routes_are_the_ones_of_the_document`
  - `tests/definitions.rs::a_change_received_by_a_node_that_is_not_the_owner_is_decided_by_the_owner`

---

### UC-DEF-034

**Case:** Tokens follow the same rule, with the token `id` as the key  
**Then:** creation, change and revocation go through the owner. Revocation takes effect at once on the nodes that answer and reaches the node that was down when it returns; `token.not-found` on revoking an unknown token.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_token_revoked_while_a_node_is_unreachable_stops_working_there_when_it_returns`
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-DEF-035

**Case:** Two copies of a definition with the **same version and different content** (the new attempt that reuses the version, or two nodes that thought they were owners) end up equal on every node, whatever the arrival order  
**Then:** the one with the **lowest fingerprint** wins (SHA-256 of the normalized body; removal has a single one). The tie is warned in the log and counted in `taba.cluster.definitions.conflicts`; repeating the equal copy is not a tie. It applies in `apply`, in the new node's load and in the reconciliation.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::two_copies_with_the_same_version_end_the_same_in_any_arrival_order`

---

### UC-DEF-036

**Case:** The node that synchronizes reads the source's **tombstones** (`GET /cluster/definitions/tombstones`  
**Then:** kind, key and the version of the removal, which the source still keeps) right after the topic and token streams. That way it knows the version already used by a removed definition: a late creation of a lower version does not resurrect it, and the recreation of the same name by it, if it becomes the owner, comes out with the next version and not with 1. A tombstone lower than the live version the node has is ignored; higher, it removes the definition.  
**Level:** C, U  
**Tests:**
  - `tests/definitions.rs::a_new_node_learns_the_tombstones_and_a_late_creation_does_not_bring_the_topic_back`

---

### UC-DEF-037

**Case:** The **heir** also takes the definition to the peers  
**Then:** the owner writes to it with `relay` and the heir queues the `definition` frame for all. If the owner does not reach a node (or goes down), the node receives the definition, and the removal (tombstone), through the heir.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::the_heir_carries_the_definition_to_a_node_the_owner_cannot_reach`

---

### UC-DEF-038

**Case:** A node that was down during several changes of the same definition receives only its **latest state** when the channel returns  
**Then:** the new frame replaces the old one in the peer's queue. There is no pending list on the owner.  
**Level:** C  
**Tests:**
  - `tests/definitions.rs::a_node_that_was_down_gets_the_latest_state_of_each_definition_from_the_queue`

---

### UC-DEF-039

**Case:** Each node writes to the `definition_versions` table of `_node.db` the **version** of every topic and token it applied and the **tombstone** of every removal, at the same point where it writes the definition  
**Then:** the rows survive a process restart and are read at start **together with the definitions** the node resumes from disk: a row stays only if its definition is there (a topic with the same `topicId`), and a tombstone stays until it expires (1 hour). Removing the definition, the new node's `wipe` and the join's final check delete the rows.  
**Level:** U and C  
**Tests:**
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

**Case:** A topic's tombstone and version keep the topic's **`topicId`** (a topic recreated with the same name has another `topicId`)  
**Then:** the old topic's tombstone neither erases nor describes the recreated topic: at start, a row with another `topicId` is discarded and the topic stays. The tombstone reaches the new node with the `topicId` in the `GET /cluster/definitions/tombstones` stream (optional `topicId` field: a source that does not send it is still understood).  
**Level:** U and C  
**Tests:**
  - `cluster/replication/definitions/tests.rs::a_row_of_another_life_of_the_topic_does_not_describe_the_recreated_one`
  - `tests/definition_versions.rs::every_node_writes_the_version_and_the_tombstone_with_the_topic_id_to_its_database`
  - `tests/definition_versions.rs::a_new_node_stores_the_tombstones_it_learns_with_the_topic_id`

---

### UC-DEF-041

**Case:** A node **takes over the disk** on a cold start
**Then:** it reads topics, tokens, versions and tombstones from `_node.db`, serves with them without waiting for the cluster, and the nodes that join receive them from it. The node's log of excluded messages goes on from the previous life's (the start date goes back to its own), or no life would be inside the window.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-DEF-042

**Case:** A node that **joins a cluster that is already up**
**Then:** it joins clean: the definitions of its disk are deleted before the synchronization and do not override the cluster's (a topic deleted while it was away does not come back).
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_node_that_joins_a_running_cluster_does_not_impose_the_definitions_of_its_disk`

---

### UC-DEF-009

**Case:** The topic definition is always persisted in the `topics` table of `${storage.dir}/_node.db` (one row per topic, no file per topic), written at the commit on each node, and updated or removed with the definition.  
**Level:** I, C  
**Tests:**
  - `src/storage/topics/tests.rs::saving_again_replaces_the_row`
  - `src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---

### UC-DEF-015

**Case:** The new node pulls the topic table through `GET /cluster/topics` (NDJSON stream ordered by `id`) from a node with the **lowest load** by the heartbeat; tied or without load data, a **random** node.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins`

---

### UC-DEF-016

**Case:** The topic table is **not** in the join response (only the node table).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_new_node_pulls_the_topic_table_after_it_joins`

---

### UC-DEF-022

**Case:** The tokens are pulled through `GET /cluster/tokens` together with the topic table, from the same source and with the same rules (direct load, events in the database, final check).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::tokens_reach_every_node_and_a_new_node_pulls_them`

---

### UC-DEF-024

**Case:** Each topic's definition is written to the `topics` table of `_node.db` in a transaction (never a half row), and names that differ only in uppercase (`Pedidos` and `pedidos`) are different topics, on any disk.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::topics_that_differ_only_in_case_keep_separate_folders`
  - `src/storage/messages/tests.rs::topic_folders_are_safe_for_case_insensitive_disks`
  - `src/storage/topics/tests.rs::definitions_are_not_files_and_names_differing_in_case_are_different_topics`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---


## ENT: Delivery on the owner: ready list, `take`, long polling

### UC-ENT-001

**Case:** Each message has a single delivery owner  
**Then:** the block's primary. Only it puts it in its ready list; a replica does not enter the list.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::only_the_owner_puts_a_message_in_the_delivery_lists`
  - `tests/replication.rs::a_publication_reaches_every_responsible_node_and_only_the_primary_owns_it`

---

### UC-ENT-002

**Case:** With three nodes A, B and C, each is primary of part of the messages  
**Then:** each one's ready list has only its own, and the union of the lists is the complete topic.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-ENT-004

**Case:** When a block's primary changes (join, leave, rebalancing), the thread redoes the filter of the affected blocks. A block in synchronization does not deliver.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---

### UC-ENT-008

**Case:** The ready list is kept per topic on each node, in memory, with `delivery.ready-list.size` IDs (default 1000).  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_ready_list_holds_at_most_the_configured_size_and_the_counters_stay_exact`
  - `src/engine/messages/tests/expiry.rs::a_small_ready_list_still_delivers_every_message_exactly_once`
  - `src/engine/messages/tests/bounded.rs::the_index_keeps_no_more_entries_than_the_window_and_the_list_hold`
  - `src/engine/messages/tests/bounded.rs::reopening_a_topic_rebuilds_the_counts_and_the_window_from_the_disk`
  - `src/engine/messages/tests/cluster.rs::the_owner_reserves_and_aborts_with_small_lists_as_with_large_ones`
  - `src/engine/messages/tests/bounded.rs::the_oldest_message_comes_from_the_age_order_of_memory_and_disk`

---

### UC-ENT-009

**Case:** A thread refills the list when the semaphore opens room (committed delivery, ID leaving), refilling the amount that opened, without querying for nothing.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::only_the_room_that_opened_is_refilled_and_a_full_list_costs_nothing`
  - `src/engine/messages/tests/bounded.rs::ack_and_visibility_work_on_a_message_the_index_only_counts`

---

### UC-ENT-010

**Case:** The refill follows the accordion  
**Then:** two cursors over the node's messages (from the oldest forwards and from the newest backwards, by GUID v7, in memory and on disk), alternating the two ends.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_refill_walks_two_cursors_alternating_the_oldest_and_the_newest_end`

---

### UC-ENT-011

**Case:** Delivery consumes the two ends **interleaved**  
**Then:** the oldest, the newest, the second oldest, the second newest.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::delivery_interleaves_the_two_ends_oldest_newest_second_oldest_second_newest`

---

### UC-ENT-012

**Case:** A new message that arrives at the owner enters the list directly if there is room and the new-end cursor is at the end; otherwise it is found later by the thread.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::a_new_message_enters_the_list_if_there_is_room_and_nothing_waits_else_the_cursor_finds_it`

---

### UC-ENT-013

**Case:** On receiving the commit of a delivery, the node removes the ID from the list. The primary is always in the quorum, so the message stops being a candidate for a new delivery on the very node that delivered it at the same instant the client receives it.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts`
  - `src/engine/messages/tests/cluster.rs::the_owner_reserves_and_aborts_with_small_lists_as_with_large_ones`

---

### UC-ENT-014

**Case:** The message whose invisibility expires returns to the list by the GUID v7 position, **not** by the scan; if the list is full, the **middle** ID leaves to make room and is refilled later (no message is lost).  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::a_returning_message_enters_by_its_position_and_the_middle_id_leaves_when_full`

---

### UC-ENT-015

**Case:** The `GET` only queries the list, without scanning memory and disk at each call.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::choosing_the_next_message_only_reads_the_list_and_scans_nothing`
  - `src/engine/messages/tests/expiry.rs::a_small_ready_list_still_delivers_every_message_exactly_once`
  - `src/engine/messages/tests/bounded.rs::small_lists_choose_the_same_messages_as_large_ones`
  - `src/storage/messages/tests.rs::the_end_of_a_group_is_read_by_an_index_in_id_order`

---

### UC-ENT-017

**Case:** Scenario 2  
**Then:** empty local list and another node has: the node queries the in-memory table, chooses a node that has ready messages, calls `POST /cluster/topics/{topic}/messages/take`; the owner does the register + commit and returns the message, which the node passes on to the client.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-ENT-019

**Case:** The peer is drawn with a **weight** proportional to the ready messages reported in the heartbeat; without quantity information, the draw is uniform.  
**Level:** U  
**Tests:**
  - `src/cluster/replication/signals/tests.rs::the_peer_is_drawn_with_a_weight_proportional_to_the_ready_ones_it_said`
  - `src/cluster/replication/signals/tests.rs::without_the_quantity_of_some_peer_the_draw_is_uniform`
  - `src/cluster/replication/signals/tests.rs::a_peer_that_said_zero_still_has_a_chance_and_every_peer_comes_once`

---

### UC-ENT-022

**Case:** `take` on a node in drain answers an empty list; on a node in synchronization, `503` `cluster.node-syncing`.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-ENT-023

**Case:** Scenario 3  
**Then:** no node has a message: the `GET` with `waitTime > 0` becomes a local listener, and the node tells the others, only once, "the topic has listeners here".  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-024

**Case:** A new ready message on the **node itself** goes straight to the first local listener, with no traffic between nodes.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-025

**Case:** A new ready message on **another node** with a known listener  
**Then:** the owner sends the notice, the listener's node does the `take` only with that node and delivers to the listener. If the message was already taken by another, the listener goes back to waiting.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_long_poll_on_one_node_is_woken_by_a_message_that_arrives_at_another`

---

### UC-ENT-031

**Case:** Delivery metrics  
**Then:** `taba.delivery.ready`, `taba.delivery.redeliveries`, `taba.delivery.longpoll.waiting`, `taba.delivery.longpoll.outcome` (`delivered`, `empty`, `disconnected`) and `taba.delivery.take` (`in` and `out`; `delivered`, `empty`, `error`).  
**Level:** C  
**Tests:**
  - `tests/metrics.rs::long_polling_is_counted_while_waiting_and_by_outcome`

---


## CAN: Permanent channel between nodes and signal queues

### UC-CAN-001

**Case:** There is one WebSocket channel per pair of nodes (full mesh), shared by all topics  
**Then:** N nodes have N−1 channels each (`taba.cluster.channel.connections`).  
**Level:** C  
**Tests:**
  - `tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it`

---

### UC-CAN-002

**Case:** The channel carries only small signals, never the content of the messages nor long lists.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content`

---

### UC-CAN-003

**Case:** `GET /cluster/channel` upgrades to WebSocket (`101`) only for whoever is in the member table; whoever is not receives `403` `cluster.not-member` **before** the upgrade.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered`

---

### UC-CAN-004

**Case:** The frames are JSON with the `type` field  
**Then:** `keepalive`, `listeners`, `ready`, `notify`, `node-state` and `confirm`, each with the defined fields.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/tests.rs::channel_frames_are_small_json_signals_with_a_type_and_never_carry_message_content`

---

### UC-CAN-005

**Case:** The `keepalive` is a message of its own (not the WebSocket ping/pong), sent every `cluster.channel.keepalive.interval`, with the node's date and time, the load, the monitoring summary and the ready messages per topic (only topics with ready ones, up to `cluster.channel.keepalive.max-topics`).  
**Level:** C  
**Tests:**
  - `tests/channel.rs::the_keepalive_carries_the_time_the_state_the_version_and_the_busiest_topics_up_to_the_limit`

---

### UC-CAN-006

**Case:** The `keepalive` **has no `seq`**, is neither confirmed nor resent  
**Then:** the next one replaces it.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/tests.rs::channel_messages_are_json_text_and_the_confirm_is_told_by_its_type`
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-007

**Case:** The state signals are sent **only on changes** (listeners 0→1 and →0; ready empty→not empty and the reverse), not at each call.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_ready_signal_fires_only_when_the_state_changes`

---

### UC-CAN-008

**Case:** On opening the channel, each side first sends a snapshot (a `listeners` and a `ready` for each topic in which the state is true) and only then the changes. A node in synchronization only consumes it when it becomes ready.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::a_channel_opens_with_a_snapshot_of_the_state_the_listeners_and_the_ready_topics`

---

### UC-CAN-009

**Case:** Every frame, except `keepalive` and `confirm`, carries an increasing `seq` per channel and per direction. The receiver returns the `confirm` (possibly grouping several `seqs`) **after processing**.  
**Level:** U, C  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-010

**Case:** The sender only removes the signal from the output queue on receiving the `confirm`; if the channel drops, it resends on reconnection what was left unconfirmed, and what was confirmed is never repeated.  
**Level:** C  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::frames_have_increasing_seq_and_leave_the_queue_only_when_confirmed`
  - `tests/channel.rs::a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns`

---

### UC-CAN-011

**Case:** A `seq` already processed and resent (lost confirmation) is ignored by the receiver (it is enough to keep the highest processed `seq`).  
**Level:** U, C  
**Tests:**
  - `tests/channel.rs::frames_are_confirmed_after_processing_and_a_repeated_seq_is_ignored`

---

### UC-CAN-012

**Case:** The node that joins or returns actively connects to each node of the table it received as a response; the others only accept. A node only accepts a connection from whoever is in its table.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::the_node_that_joins_calls_every_member_and_only_members_are_answered`

---

### UC-CAN-013

**Case:** Drop with both nodes alive  
**Then:** the node that detects it first reconnects. If both open a connection at the same time, the node with the **lowest ID hash** closes the one it opened and keeps the one opened by the other: always one channel per pair.  
**Level:** U, C  
**Tests:**
  - `src/cluster/channel/tests.rs::of_two_connections_the_one_opened_by_the_node_with_the_larger_hash_stays`
  - `src/cluster/channel/tests.rs::the_same_side_opening_again_replaces_the_dead_connection`
  - `src/cluster/channel/tests.rs::both_nodes_keep_the_same_connection_in_any_order_of_arrival`
  - `tests/channel.rs::two_connections_at_once_end_with_a_single_channel_per_pair`

---

### UC-CAN-014

**Case:** Process restart  
**Then:** the node loses memory (including the member table, which is not persisted) and returns as a new node, through the register; it is the one that opens the channels.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CAN-015

**Case:** On each owner node there is one responsible per topic (active only while there are listeners on some node and new messages to announce). The messages that become ready are accumulated for `cluster.notify.window` (default 100 ms) and become **one** notice, with the quantity.  
**Level:** U, C  
**Tests:**
  - `tests/notify.rs::the_ready_messages_of_a_window_become_one_notice_with_the_quantity`

---

### UC-CAN-016

**Case:** The notice goes to **one** node with a listener at a time, rotating among those that have one; the responsible waits `cluster.notify.wait` for the node to come fetch (`take`). If it does not come, it moves on to the next node with a listener and repeats the cycle, until the announced messages or the nodes with a listener run out.  
**Level:** C  
**Tests:**
  - `tests/notify.rs::the_notice_goes_to_one_listener_at_a_time_and_passes_on_when_nobody_comes`

---

### UC-CAN-017

**Case:** The notice is a hint  
**Then:** whoever receives it goes to fetch and may find nothing (`taba.cluster.channel.notifications` with `outcome` `fetched` or `empty`).  
**Level:** C  
**Tests:**
  - `tests/notify.rs::a_notice_is_a_hint_that_ends_fetched_or_empty`

---

### UC-CAN-018

**Case:** There is one output queue per destination node (a write thread empties it without blocking the producer) and one input queue per source node (processing separate from the socket read).  
**Level:** U, C  
**Tests:**
  - `tests/channel.rs::each_peer_has_an_out_queue_and_an_in_queue_that_drain`

---

### UC-CAN-019

**Case:** With the channel down, **state** signals of the same topic replace each other in the queue (only the last matters); old notices are discarded (`taba.cluster.channel.queue.dropped` with `coalesced` and `expired`).  
**Level:** U  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::state_signals_of_the_same_topic_replace_each_other_and_notices_do_not`
  - `src/cluster/channel/queue/tests.rs::old_notices_are_dropped_and_old_states_are_kept`

---

### UC-CAN-020

**Case:** The output queue exceeds `cluster.channel.queue.max-size` or `cluster.channel.queue.max-age`  
**Then:** the node whose channel is down is treated as **failed** and the exclusion vote is opened, without waiting for the end of the reconnections (`dropped` with `overflow`).  
**Level:** C  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::the_queue_overflows_by_size_or_by_age`
  - `tests/channel.rs::a_queue_that_waits_too_long_opens_the_vote_without_waiting_for_the_retries`

---

### UC-CAN-021

**Case:** Reconnection **before** the vote concludes  
**Then:** nothing changes, the nodes resume sending the pending signals from where they stopped.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::a_channel_that_returns_before_the_retries_end_changes_nothing`

---

### UC-CAN-022

**Case:** Vote concluded  
**Then:** all nodes discard the queues (output and input) for the excluded node and stop accumulating. If the node returns, it redoes the bootstrap with new, empty queues.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::once_the_exclusion_is_decided_the_nodes_drop_the_channel_and_the_return_starts_with_empty_queues`

---

### UC-CAN-023

**Case:** The `node-state` signal with `state: draining`  
**Then:** the peers stop asking `take` of that node and choosing it as a load source.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-CAN-024

**Case:** Channel metrics  
**Then:** `taba.cluster.channel.state` per peer, `rtt`, `reconnects`, `queue.out` and `queue.in` (the input queue per source node).  
**Level:** C  
**Tests:**
  - `tests/channel.rs::every_node_has_one_channel_to_each_other_node_and_measures_it`

---

### UC-CAN-025

**Case:** The channel call carries the destination's `nodeId` (`to`)  
**Then:** the new process that serves the same IP and port (another ID, for example a quick restart) answers `421` `cluster.wrong-node` before any other check, and the sender counts the channel as down. The vote opens, the old node leaves the table and the new one joins (D100).  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CAN-026

**Case:** The channel call **without** `to` (empty) does not check the destination  
**Then:** whoever calls without knowing the ID of the node it looks for (the test route, a version that does not send it) is treated as before, without `421` (D100).  
**Level:** C  
**Tests:**
  - `tests/channel.rs::a_channel_call_without_the_destination_id_is_not_checked_against_it`

---

### UC-CAN-027

**Case:** The channel's `membership` frame **is neither replaced nor expires**  
**Then:** unlike `ready` (which the last replaces) and `notify` (which expires), a membership change stays in the output queue until the peer's `confirm`. A queue that grows too much already makes the peer a failed node.  
**Level:** U  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::a_membership_change_is_never_replaced_and_never_expires`

---

### UC-CAN-028

**Case:** The channel's `definition` frame is **replaced** by the newest of the same definition (same kind and same key) in the output queue; another kind or another name does not replace it, and it does not expire.  
**Level:** U  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::a_definition_frame_is_replaced_by_the_newer_of_the_same_definition_and_never_expires`

---


## INV: Invisibility on the owner and on the replicas

### UC-INV-001

**Case:** The metadata are `deliveryCount`, `invisibleUntil` (absolute date) and `receiptToken`, stored and replicated together  
**Then:** in the same register + commit, in memory or in the topic's local database.  
**Level:** U, C  
**Tests:**
  - `src/storage/messages/tests.rs::the_delivery_metadata_lives_in_the_message_row_and_survives_a_reopening`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-INV-003

**Case:** The `receiptToken` is generated by the owner at each delivery; the node compares it with that of the metadata on ack and on `PUT`, and answers `409` if different, changing nothing. The change keeps the same token.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message`

---

### UC-INV-004

**Case:** Message in memory  
**Then:** metadata in memory. Message on disk: row in the topic database's `messages` table (`id`, position, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`), updated at the commit and removed with the message on ack. Changing the invisibility of a message on disk updates only the row, without touching the body.  
**Level:** I  
**Tests:**
  - `src/engine/messages/index/tests.rs::an_inserted_message_with_a_future_deadline_starts_in_processing`

---

### UC-INV-005

**Case:** The states of the message for the owner  
**Then:** **ready** (`invisibleUntil` absent or in the past), **invisible** (in the future) and **removed** (ack).  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::a_message_is_ready_invisible_or_removed_by_its_deadline`

---

### UC-INV-006

**Case:** Delivery  
**Then:** propagates a new `invisibleUntil` and a new `receiptToken`, `deliveryCount` +1, removes the ID from the ready list and inserts ID and deadline in the owner's invisible list.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::a_candidate_is_reserved_until_the_quorum_commits_or_aborts`
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-INV-007

**Case:** Change  
**Then:** propagates the new `invisibleUntil`, `deliveryCount` does not change, ready list does not change (with `0`, the owner inserts the ID, as on an expiry) and the invisible list replaces the deadline (with `0`, removes).  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation`

---

### UC-INV-008

**Case:** Ack  
**Then:** removes the message and the metadata and removes the entry from the invisible list.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_ack_removes_the_copy_and_only_the_owner_counts_it`
  - `tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused`
  - `tests/replication.rs::an_ack_of_a_gone_message_answers_404_even_when_the_primary_answers_last`

---

### UC-INV-010

**Case:** The register is exclusive  
**Then:** the node rejects if there is already another pending for the message (`409` `cluster.conflict`) or, on a delivery, if it is already invisible. On change and ack, also if the `receiptToken` does not match (`409` `message.stale-receipt`).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_registration_of_a_visibility_is_checked_against_the_message`
  - `tests/replication.rs::an_ack_on_any_node_removes_the_message_everywhere_and_a_stale_token_is_refused`
  - `tests/replication.rs::an_ack_of_a_gone_message_answers_404_even_when_the_primary_answers_last`

---

### UC-INV-012

**Case:** Failure before the quorum  
**Then:** retry and, when exhausted, rollback of the pending one; the owner's lists are **not** touched and the message stays as it was. Failure on the replicas outside the quorum: no rollback, the owner insists up to the ceiling.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::an_ack_that_cannot_reach_the_primary_fails_and_the_message_stays`
  - `tests/replication.rs::a_get_whose_registrations_fail_answers_503_and_leaves_the_messages_ready`

---

### UC-INV-013

**Case:** `POST .../visibility/register` with `kind` `delivery` (carrying `deliveryCount`, new token) or `change` (the client's `receiptToken` must match the current one); `commit` and `rollback` with `OperationRef`; errors `404` `message.not-found`, `409` `cluster.conflict` and `409` `message.stale-receipt`.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-INV-014

**Case:** The delivery's `commit` removes the ID from the node's ready list, and the primary **always receives** that call before the response to the client.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::a_get_on_any_node_delivers_what_the_other_nodes_own`

---

### UC-INV-016

**Case:** The expiry **does not propagate**  
**Then:** nothing is written to the metadata nor sent to the replicas; `deliveryCount` only goes up at the next delivery.  
**Level:** C  
**Tests:**
  - `tests/replication.rs::the_visibility_can_be_changed_and_an_expired_one_is_redelivered_without_propagation`

---

### UC-INV-017

**Case:** The owner's thread looks at the head of the invisible list, sleeps until its deadline (with a short ceiling to react to changes and earlier entries) and, on expiry, removes the entry and **checks** the metadata  
**Then:** the message still exists, the node is still the owner and the deadline is still the entry's; otherwise it discards and moves on.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_head_is_taken_out_and_checked_and_a_pair_that_is_not_the_messages_is_dropped`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/engine/messages/tests/bounded.rs::reopening_a_topic_rebuilds_the_counts_and_the_window_from_the_disk`

---

### UC-INV-018

**Case:** After the check, the message enters the ready list by the GUID v7 position (near the old end); with the list full, the middle ID leaves.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::after_the_check_the_message_joins_the_ready_list_by_its_position_and_the_middle_leaves`

---

### UC-INV-019

**Case:** Once the check passes with `deliveryCount ≥ maxDeliveries`, the message does **not** return to the list  
**Then:** it goes to the dead-letter. A message expired by `ttl` is also discarded, without returning.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::the_owner_queues_the_dead_letter_move_once_and_can_try_again_after_a_failure`
  - `tests/replication.rs::a_message_that_spent_its_deliveries_moves_to_the_dead_letter_on_every_node`

---

### UC-INV-020

**Case:** On the FIFO queue, the message that comes back is the head of the group; the group stops being busy and enters the ready list by the ID position, without the middle-of-the-list rule.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/tests.rs::in_a_fifo_queue_the_returning_message_is_the_group_head_and_frees_the_group`
  - `src/engine/messages/index/bounded_tests.rs::on_a_fifo_queue_the_ready_list_has_no_ceiling_and_no_middle_rule`
  - `src/engine/messages/tests/bounded.rs::small_lists_choose_the_same_messages_as_large_ones`

---

### UC-INV-021

**Case:** If the invisible list empties and there are still rows in the expiry table, the thread does the reload before sleeping.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::when_the_list_empties_with_pairs_in_the_table_the_reload_is_asked_for_before_sleeping`

---

### UC-INV-022

**Case:** The invisible list is, per topic and only on the owner, a list of (ID, `invisibleUntil`) pairs ordered by deadline, with at most `delivery.invisible-list.size` entries; lookup by ID for change and ack.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_invisible_list_is_ordered_by_deadline_with_a_ceiling_and_found_by_id`
  - `src/engine/messages/tests/bounded.rs::the_index_keeps_no_more_entries_than_the_window_and_the_list_hold`

---

### UC-INV-023

**Case:** Inclusion rule with the list full  
**Then:** if the new deadline is **earlier** than the last one in the list, the new pair enters and the last goes to the table; otherwise the new pair goes straight to the table.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::with_a_full_list_an_earlier_deadline_enters_and_the_last_goes_to_the_table`

---

### UC-INV-024

**Case:** Invariant  
**Then:** everything in the list expires before everything in the table; the thread never queries the table while the list does not empty.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`

---

### UC-INV-025

**Case:** Reload  
**Then:** below `delivery.invisible.reload-threshold` (or empty), the thread reads from the table the pairs with the lowest `invisibleUntil` (an ordered query with a limit by the index), moves them to the list and deletes the rows read.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`
  - `src/engine/messages/index/bounded_tests.rs::the_reload_starts_below_the_threshold_reads_the_earliest_and_deletes_the_rows_read`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/engine/messages/tests/bounded.rs::ack_and_visibility_work_on_a_message_the_index_only_counts`

---

### UC-INV-026

**Case:** No old entries  
**Then:** ack, change and new delivery delete or update the table row in the same commit that updates the list. There is no lazy removal.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::the_list_always_expires_before_the_table_and_the_table_follows_every_change`
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
**Note:** A deadline change, the ack and a new delivery delete or update the row in the same step that updates the list; if the write fails, the pair stays queued and is retried on the next change (only the delivery undoes the operation, see UC-INV-027).

---

### UC-INV-027

**Case:** The write to the table is part of the commit on the owner  
**Then:** if it fails, the commit does not complete and the delivery is undone before the response to the client.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/expiry.rs::a_failed_table_write_undoes_the_delivery_before_the_answer`
  - `src/engine/messages/tests/cluster.rs::a_failed_table_write_undoes_a_reservation_of_the_owner`
**Note:** Proven on the single-node path (`take`); the cluster path (`take_candidates`) applies the same rule but has no test yet.

---

### UC-INV-028

**Case:** With room in the list, no write beyond the metadata.  
**Level:** U  
**Tests:**
  - `src/engine/messages/index/bounded_tests.rs::with_room_in_the_list_nothing_is_written_beyond_the_metadata`

---

### UC-INV-029

**Case:** New owner of a block  
**Then:** when synchronization finishes, it walks through the metadata of the messages of which it is now primary: those with `invisibleUntil` in the future enter the invisible list (or the expiry table, by the inclusion rule) and the others become candidates of the ready list.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---

### UC-INV-030

**Case:** Whoever stops being the owner cleans nothing at once  
**Then:** the entries that remain fail the owner check and are discarded when read.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/cluster.rs::when_the_ring_changes_the_owner_is_recomputed_from_the_metadata`

---


## ARM: Storage: memory, overflow, SQLite, integrity

### UC-ARM-001

**Case:** The message is written first to memory; on reaching `memory.limit`, the excess is persisted to disk.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`
  - `tests/messages.rs::without_a_memory_limit_nothing_goes_to_disk_by_overflow`

---

### UC-ARM-002

**Case:** The body goes to `${topic}/${id}.body` (one folder per topic, one file per message), with the **raw bytes**, without reserializing or encoding; the `contentType` stays in the metadata.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::what_does_not_fit_in_memory_overflows_to_disk`
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-003

**Case:** When reading a message that is not in memory, it is fetched from disk by the same path.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::delivery_merges_memory_and_disk_in_id_order_with_the_exact_bytes`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-004

**Case:** Memory is not divided in a fixed way among the topics  
**Then:** topics with a higher read rate keep more messages in memory; those with a lower rate go to disk first.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk`
  - `src/engine/messages/tests.rs::above_its_reserve_a_topic_only_takes_memory_from_topics_that_are_read_less`
  - `src/engine/messages/tests.rs::a_topic_that_is_read_more_is_never_demoted_by_one_that_is_read_less`

---

### UC-ARM-005

**Case:** The read rate is the moving average of messages consumed per second in the `memory.rate.window` window (5 minutes), the same as `publishRate` and `consumeRate`.  
**Level:** U  
**Tests:**
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

**Case:** Each topic has a memory reserve (`memory.topic.reserve`, 1000 messages), **independent of the rate**  
**Then:** a new topic, with no read history, does not start by throwing the first messages to disk. The division by rate only applies above the reserves.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests.rs::a_new_topic_keeps_its_reserve_by_pushing_the_excess_of_a_low_rate_topic_to_disk`

---

### UC-ARM-007

**Case:** What stays in memory are the messages of the ready list and the newly arrived; the rest is a candidate for disk, starting with the topic of the lowest rate. LRU/LFU is not used.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests.rs::demotion_takes_the_middle_of_the_ready_list_and_keeps_the_ends`
  - `src/engine/messages/tests.rs::messages_in_processing_are_demoted_before_the_ready_ones`

---

### UC-ARM-008

**Case:** The ready list and the messages in memory do not change because of the database  
**Then:** it only takes care of what is on disk.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests.rs::the_database_only_holds_what_is_on_disk_and_memory_messages_never_touch_it`

---

### UC-ARM-009

**Case:** `taba.storage.*`  
**Then:** memory used and limit, messages in memory and on disk, disk used and free, overflow, disk and memory reads, duration and disk errors.  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::storage_metrics_count_overflow_and_reads_by_tier`
  - `tests/metrics.rs::free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure`

---

### UC-ARM-010

**Case:** Each node keeps one database per topic, `${topic}/_topic.db`, strictly local  
**Then:** it is never copied, sent or synchronized with other nodes.  
**Level:** I  
**Tests:**
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-011

**Case:** The `messages` table has one row per message on disk (`id`, ring position, `group_id`, `contentType`, `deliveryCount`, `invisibleUntil`, `receiptToken`; on FIFO, `deduplicationId` and `deleteAt`) with indexes by `id`, `invisibleUntil`, position, `(group_id, id)`, `(group_id, deduplicationId)` and `deleteAt`.  
**Level:** I  
**Tests:**
  - `src/storage/messages/tests.rs::a_message_is_a_body_file_plus_a_row_in_the_topic_database`
  - `src/storage/messages/tests.rs::a_topic_database_from_before_the_fifo_columns_is_upgraded_in_place`

---

### UC-ARM-012

**Case:** The `expiry` table stores the pairs that did not fit in the invisible list, with an index by deadline.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/expiry.rs::pairs_over_the_ceiling_wait_in_the_expiry_table_and_come_back_on_time`
  - `src/storage/messages/tests.rs::the_expiry_table_keeps_the_pairs_that_did_not_fit_ordered_by_deadline`

---

### UC-ARM-013

**Case:** Every **write** to a topic's database goes through a **single writer** (one command queue per topic, executed one at a time); long reads use their own connections in WAL mode and do not block the writer.  
**Level:** U, I  
**Tests:**
  - `src/engine/messages/tests.rs::a_slow_topic_never_blocks_the_other_topics`

---

### UC-ARM-014

**Case:** Write order  
**Then:** on write, first the body file, then the row; on ack, first the row, then the file. A crash leaves at most a file without a row or a row without a file.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::a_message_on_disk_is_delivered_again_and_acked_like_any_other`
  - `src/storage/messages/tests.rs::removing_deletes_the_row_and_then_the_file`
  - `tests/messages.rs::messages_over_the_memory_limit_overflow_to_disk_and_come_back_intact`

---

### UC-ARM-015

**Case:** The node that restarts after a crash comes back as new and rebuilds from the cluster.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ARM-017

**Case:** `storage.db.synchronous` (default `NORMAL` with WAL) is applied when opening the database.  
**Level:** I  
**Tests:**
  - `src/storage/db/tests.rs::opens_in_wal_mode_with_the_configured_synchronous`
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---

### UC-ARM-021

**Case:** The node creates `storage.dir` (default `/var/lib/taba`, the last folder is named `taba`) at start; if it cannot create or write, it does not start.  
**Level:** I  
**Tests:**
  - `tests/startup.rs::a_storage_dir_that_cannot_be_created_stops_startup`
  - `tests/startup.rs::startup_creates_storage_dir_when_missing`

---

### UC-ARM-022

**Case:** A node that loses a block (ring change) **deletes nothing at once**  
**Then:** it keeps the block's messages and can serve as a synchronization source. It only deletes (memory, `.body` and database rows, by ring position range) when **the whole cluster is healthy**.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay`
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-ARM-023

**Case:** If the new node does not become healthy, the ring goes back to the previous one and the old node is responsible again, with the messages intact, nothing to redo. If the ring changes again before the cleanup, the node recomputes which blocks are still not its own.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-ARM-024

**Case:** A message received as a replica or published in drain mode goes straight to disk (effective memory limit zero).  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests.rs::drain_saves_everything_in_memory_to_disk`

---

### UC-ARM-026

**Case:** The log of excluded messages has an **age limit** (`storage.excluded.max-age`, 24 h) and a **size limit** (`storage.excluded.max-size`, 100 MB, adding all topics)  
**Then:** the oldest rows go first, from whichever topic, until the log fits; the trimming runs every 30 s.  
**Level:** U  
**Tests:**
  - `src/storage/messages/excluded/tests.rs::the_limits_drop_the_oldest_rows_first_and_move_the_start_forward`
  - `src/storage/messages/excluded/tests.rs::the_size_limit_counts_every_topic_and_drops_the_oldest_of_all`

---

### UC-ARM-027

**Case:** The log keeps the **start of the continuous recording**: it begins when the log is created (in `_node.db`, it survives a restart) and only moves forward when a row is discarded by a limit  
**Then:** it becomes the instant of the newest discarded row; without a discard, a cluster with no exclusions for days still covers those days.  
**Level:** U  
**Tests:**
  - `src/storage/messages/excluded/tests.rs::the_limits_drop_the_oldest_rows_first_and_move_the_start_forward`
  - `src/storage/messages/excluded/tests.rs::the_start_of_the_recording_does_not_move_without_a_discarded_row`
  - `src/storage/messages/excluded/tests.rs::the_start_of_the_recording_is_kept_in_the_node_database`

---

### UC-ARM-028

**Case:** The log of a **past life** is incorporated into the current one (`import_excluded`) and the start only goes back if the old interval and the current one meet, with no hole  
**Then:** the rows add up (the oldest instant of an `id` stays); with a hole, the start stays where it is.  
**Level:** U  
**Tests:**
  - `src/storage/messages/excluded/tests.rs::a_past_life_adds_its_rows_and_moves_the_start_back_only_if_there_is_no_hole`

---

### UC-ARM-029

**Case:** The record is written in the **same transaction** that deletes the message, with `storage.db.synchronous` as configured (`NORMAL` by default): no extra fsync on the ack. A topic database from an earlier version gets the `excluded` table when it opens  
**Then:** the message goes and the record exists, or neither does; no synchronization beyond the configured one.  
**Level:** U  
**Tests:**
  - `src/storage/messages/excluded/tests.rs::removing_with_exclusion_deletes_the_message_and_records_it_in_the_same_transaction`
  - `src/storage/messages/excluded/tests.rs::recording_adds_no_synchronization_beyond_the_configured_one`
  - `src/storage/messages/excluded/tests.rs::a_topic_database_from_before_the_log_gets_the_table`

---


## ING: Node join (bootstrap)

### UC-ING-001

**Case:** With `replication.factor` above 1, without `cluster.entry-address` the node **does not start** (with factor 1 the node starts alone).  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::the_entry_address_must_be_host_and_port`
  - `tests/startup.rs::a_cluster_without_the_entry_address_does_not_start`

---

### UC-ING-002

**Case:** On starting, the node calls `GET /cluster/nodes` at the load balancer address (cluster port) and receives the `MemberTable` (`version` and `members` with `nodeId`, `addresses`, `port`, `state`). Any node answers; a caller that is not yet a member is accepted.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::the_first_node_starts_the_cluster_alone`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster.rs::only_members_call_the_phase_routes_but_anyone_lists_and_asks_to_join`
  - `tests/cluster_api.rs::the_cluster_port_serves_only_cluster_routes_and_the_client_port_none`

---

### UC-ING-003

**Case:** The node chooses a member **at random** from the list and sends `POST /cluster/nodes/join` (`JoinRequest`  
**Then:** `nodeId`, `addresses`, `port`, `version`), in a single synchronous request; that member coordinates the register. If the member does not answer, it tries another from the list.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::the_join_request_has_the_documented_fields`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster_api.rs::the_join_route_registers_the_node_and_answers_with_the_table`

---

### UC-ING-004

**Case:** If the listing fails, the node tries again every `cluster.bootstrap.retry.interval`.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::without_the_first_node_flag_the_node_keeps_trying_and_stays_out_of_rotation`

---

### UC-ING-005

**Case:** Only the **first node** of a new cluster (`cluster.first-node=true`), when the listing finds no one, starts the cluster by itself as the only member. Without the flag, a listing with no answer is an error and the node tries again.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::the_first_node_starts_the_cluster_alone`
  - `tests/cluster.rs::without_the_first_node_flag_the_node_keeps_trying_and_stays_out_of_rotation`

---

### UC-ING-006

**Case:** `GET /cluster/nodes` on a node that does not yet have the table (synchronizing)  
**Then:** `503` `cluster.node-syncing`.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_node_that_has_no_table_yet_answers_503`
  - `tests/cluster_api.rs::a_node_without_the_table_answers_503_with_retry_after`

---

### UC-ING-007

**Case:** The node names itself with a **GUID v7** at each process start, only in memory, and identifies itself by it, including in the join request.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/dto_tests.rs::the_node_names_itself_with_a_new_guid_v7_at_each_start_and_the_join_request_carries_it`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-ING-008

**Case:** The member table stores the IP and the **cluster port** of each node, and every call between nodes goes straight to them, without a load balancer (only the initial `GET /cluster/nodes` goes through it).  
**Level:** C  
**Tests:**
  - `src/cluster/members/tests.rs::callers_are_recognized_by_the_addresses_in_the_table`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-009

**Case:** The member that receives the `join` **tests the informed IPs**, connecting back to the cluster port. Failure  
**Then:** `422` `cluster.join-unreachable`, the node is excluded and repeats the bootstrap.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_new_node_the_member_cannot_connect_back_to_is_refused`

---

### UC-ING-010

**Case:** Successful connection  
**Then:** the member sends **all nodes** the register request (`POST /cluster/nodes/register`, phase 1); with all confirming, it sends the commit (`POST /cluster/nodes/commit`, phase 2), and only then answers the new node with the table, already in effect across the cluster.  
**Level:** C  
**Tests:**
  - `src/cluster/members/tests.rs::register_leaves_the_change_pending_and_commit_makes_it_effective`
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`
  - `tests/cluster_api.rs::the_join_route_registers_the_node_and_answers_with_the_table`

---

### UC-ING-011

**Case:** The new node receives only the node table; the topic table (and the token one) it pulls afterwards by stream from a node of the cluster.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-013

**Case:** Confirmation of **all** nodes, with no reduced quorum  
**Then:** with a node not confirming the register, the join does not take effect.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-014

**Case:** `POST /cluster/nodes/register` with another operation pending  
**Then:** `409` `cluster.conflict`; `commit` of an unknown operation: `404` `cluster.operation-unknown`.  
**Level:** C  
**Tests:**
  - `src/cluster/members/tests.rs::all_three_phases_are_idempotent_by_operation_id`
  - `src/cluster/members/tests.rs::a_second_change_while_one_is_pending_is_a_conflict`
  - `src/cluster/members/tests.rs::commit_and_rollback_of_an_unknown_operation_are_unknown`
  - `tests/cluster.rs::a_second_pending_registration_conflicts_and_unknown_operations_are_refused`
  - `tests/cluster_api.rs::the_bodies_of_the_cluster_routes_are_validated`

---

### UC-ING-015

**Case:** Failure **before the commit** (connection test, or register phase without confirmation from all)  
**Then:** rollback of the pending register, the node is excluded (does not reach the table), and the `join` answers `503` `cluster.registration-failed`.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-016

**Case:** The blocks the node would take over are rebalanced among the others  
**Then:** the ring is recomputed without that node.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-017

**Case:** New attempt  
**Then:** the node repeats the bootstrap from the start every `cluster.bootstrap.retry.interval` up to `cluster.bootstrap.retry.max-attempts` (`0` = no limit); when exhausted, the node exits with an error. The same interval applies to failure in listing, join and synchronization.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::the_node_gives_up_after_the_maximum_attempts`
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-018

**Case:** Failure **at the commit** (register already confirmed by all)  
**Then:** it is not undone; the member insists up to `write.commit.timeout`, does not abort the new node, and the `join` answers `503` `cluster.commit-timeout` (the node was accepted). Before repeating, the node queries `GET /cluster/nodes`.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_commit_that_does_not_arrive_does_not_undo_the_join`

---

### UC-ING-020

**Case:** `taba.cluster.bootstrap.failures` counts failures by `reason` (connection test, phase 1, phase 2); a bootstrap failure generates an `error` log.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::a_member_that_does_not_confirm_aborts_the_join_and_the_node_tries_again`

---

### UC-ING-021

**Case:** On taking its position on the ring, the node becomes responsible for blocks as primary, secondary, tertiary etc., and starts synchronization (`SIN` cases).  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ING-022

**Case:** Readiness  
**Then:** the node only becomes ready (`/health/ready` `200`) when it finishes the whole synchronization. Until then it is in the table and confirms registers by writing them to the event database, but **does not serve clients, does not coordinate and does not deliver**.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-ING-023

**Case:** Once ready, the node also becomes the managing node of the volatile queues whose `id` falls in its range of the ring.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_managed_set_is_derived_from_the_table_and_a_new_node_manages_only_once_ready`

---

### UC-ING-024

**Case:** The node state goes through `joining` and `syncing` until `active` (`taba.node.state`); `/health/ready` reports `bootstrapping` and `syncing` as the reason.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-ING-027

**Case:** A different version (epoch) of the member table between nodes indicates incomplete propagation (condition `membership-divergent`, `taba.cluster.members.version`).  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::nodes_join_through_the_balancer_and_end_with_the_same_table`

---

### UC-ING-028

**Case:** Each member listing through `cluster.entry-address` opens a **new connection** (`Connection: close`), without reusing that of a previous attempt  
**Then:** the load balancer decides the node per connection, and a reused connection would trap the bootstrap of the node that restarts on the node itself (which does not yet have the table and answers `cluster.node-syncing`).  
**Level:** C  
**Tests:**
  - `src/cluster/transport/http/tests.rs::every_node_listing_opens_a_new_connection_so_the_balancer_can_pick_another_node`

---

### UC-ING-029

**Case:** A node of a **newer** version joins a cluster that has nodes of an earlier version  
**Then:** it **retires them, one at a time**, through the drain that already exists (each member's version comes from the `version` of its `NodeStatus`). The next drain only starts after the previous one left the table; at most one member is in `draining` at a time. The oldest goes first (a member that publishes no version counts as the oldest), and versions are not made compatible with each other: the new one retires the earlier ones.  
**Level:** C  
**Tests:**
  - `tests/upgrade.rs::nodes_of_the_new_version_retire_the_old_ones_one_at_a_time`
  - `src/cluster/upgrade/tests.rs::the_newest_version_drains_the_oldest_member_first`
  - `src/cluster/upgrade/tests.rs::a_cluster_of_one_version_has_nothing_to_retire`
  - `src/cluster/upgrade/tests.rs::the_next_drain_waits_for_the_previous_one_to_leave`

---

### UC-ING-030

**Case:** The upgrade's drain cannot start yet  
**Then:** it **waits** if the cluster would be left with fewer than **3 active nodes** after the drain, if the acting node is not yet `active` (or is draining), if some member is `syncing`, or if some member did not answer the version query (the round decides nothing).  
**Level:** C  
**Tests:**
  - `tests/upgrade.rs::the_upgrade_does_not_drain_if_fewer_than_three_would_stay_active`
  - `src/cluster/upgrade/tests.rs::the_drain_does_not_start_if_it_would_leave_fewer_than_three_active`
  - `src/cluster/upgrade/tests.rs::the_drain_does_not_start_while_the_node_is_not_ready_or_a_member_is_syncing`
  - `src/cluster/upgrade/tests.rs::a_member_that_did_not_answer_makes_the_round_decide_nothing`

---

### UC-ING-031

**Case:** A node does not finish the drain the upgrade asked for within the deadline (`drain.timeout` plus 30 s)  
**Then:** the upgrade **stops on that node** (it does not ask another to drain) and the `upgrade-stalled` condition (warning) shows up in `/health/cluster`; the operator decides whether to cancel the node's drain or stop it. When the node leaves the table the condition goes away.  
**Level:** U  
**Tests:**
  - `src/cluster/upgrade/tests.rs::a_drain_that_does_not_finish_in_time_stops_the_upgrade_on_that_member`
  - `src/cluster/health/tests.rs::a_stalled_upgrade_raises_its_condition_as_a_warning`

---

### UC-ING-032

**Case:** The node decides whether to send a peer something only a newer version understands  
**Then:** it only sends to whoever **announced** an equal or greater version; whoever announced no version, did not answer or has not been seen yet counts as an earlier version. The node itself always understands what its own version brought.  
**Level:** C  
**Tests:**
  - `tests/upgrade.rs::a_node_is_only_sent_what_its_version_announced_support_for`
  - `src/cluster/upgrade/tests.rs::a_member_supports_what_a_version_brought_only_if_it_announced_that_version`

---

### UC-ING-033

**Case:** A node of an **earlier** version joins a cluster of a newer version  
**Then:** it **does not drain** the newer ones: the new nodes retire it, and the three remain active.  
**Level:** C  
**Tests:**
  - `tests/upgrade.rs::an_older_node_that_joins_is_retired_and_does_not_drain_the_newer_ones`
  - `src/cluster/upgrade/tests.rs::an_earlier_version_does_not_drain_the_newer_ones`

---

### UC-ING-034

**Case:** Several nodes of the newest version see earlier members at the same time  
**Then:** only **one acts**: the lowest `nodeId` among those of the newest version; the others wait. So two nodes do not ask for different drains at once.  
**Level:** U  
**Tests:**
  - `src/cluster/upgrade/tests.rs::only_the_lowest_id_among_the_newest_acts`

---

### UC-ING-035

**Case:** The node compares two program versions  
**Then:** semver precedence applies: `0.1.9` comes before `0.1.10`, a candidate (`-rc.N`) comes before its final, the `v` prefix and the `+build` suffix do not count, and text that is not a version is neither earlier nor later (it is never drained by mistake).  
**Level:** U  
**Tests:**
  - `src/cluster/upgrade/tests.rs::versions_compare_by_precedence_and_a_candidate_comes_before_its_final`

---

### UC-ING-036

**Case:** A disk from an **earlier version** (no manifest) is not migrated: it is not a life
**Then:** the new node follows today's path: it puts the disk in `_previous-life`, returns only the listed losses and deletes the folder at the end.
**Level:** U
**Tests:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`

---

### UC-ING-037

**Case:** While a member **does not tell** since when it records the log of excluded messages (a node of an earlier version, or one that does not answer), **no life is restorable**
**Then:** the cluster cut-off date is undefined and the lives are outside the window (they follow the path of the loss list); when everybody tells, the life is restored again.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::no_life_is_restorable_while_a_member_does_not_tell_when_its_log_started`

---


## FAL: Failure detection, exclusion vote and exit by failure

### UC-FAL-001

**Case:** Control of who is online is done by the permanent mesh channel, with no coordinator; **there is no separate ping**.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::there_is_no_ping_apart_from_the_channel_between_the_nodes`

---

### UC-FAL-002

**Case:** Channel that drops or is left without a heartbeat  
**Then:** the node that detected it **tries to reconnect** (`cluster.channel.reconnect.retries` and `backoff`) before acting. A hung node with the connection open is detected by the lack of `keepalive`.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stops_answering_with_the_connection_open_is_detected_and_excluded`

---

### UC-FAL-003

**Case:** Reconnection before the attempts run out  
**Then:** nothing changes (no vote is opened).  
**Level:** C  
**Tests:**
  - `tests/channel.rs::a_channel_that_returns_before_the_retries_end_changes_nothing`

---

### UC-FAL-004

**Case:** Channel still down after the attempts  
**Then:** the exclusion vote is opened among the nodes of the cluster.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-005

**Case:** The vote is also opened when the channel's output queue exceeds the limit (`reason: queue-overflow`) and when the node goes into failure (`reason: node-failed`); the normal reason is `channel-down`.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up`
  - `tests/failures.rs::a_queue_that_overflows_opens_the_vote_that_excludes_the_unreachable_node`

---

### UC-FAL-006

**Case:** While the vote does not conclude, the signals to the node accumulate in memory; on reconnecting before, sending is resumed.  
**Level:** C  
**Tests:**
  - `tests/channel.rs::a_signal_without_confirm_waits_in_the_queue_and_goes_out_when_the_channel_returns`

---

### UC-FAL-007

**Case:** The exclusion requires `ceil(S × 51%)` votes of S healthy nodes (`cluster.vote.quorum-percent`)  
**Then:** 2 of 3, 3 of 4, 3 of 5.  
**Level:** U  
**Tests:**
  - `src/cluster/vote/tests.rs::the_quorum_is_the_ceiling_of_the_percentage_of_the_healthy_nodes`

---

### UC-FAL-008

**Case:** A **healthy** node is one that whoever opens the vote sees with the channel active and that answers the vote request. The suspects (down) **do not vote and do not enter the base**.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-009

**Case:** If several nodes go down at the same time (for example, a zone), the vote continues only among the healthy ones.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-010

**Case:** With fewer healthy nodes than `cluster.vote.min-voters` (default 2), no exclusion takes effect and the `quorum-lost` condition appears; the minimum prevents an isolated node from excluding all the others.  
**Level:** C  
**Tests:**
  - `tests/conditions.rs::with_two_of_three_nodes_out_the_quorum_is_at_risk_and_the_writes_can_fail`
  - `tests/conditions.rs::the_quorum_is_lost_only_for_who_sees_fewer_healthy_nodes_than_the_minimum`
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`
  - `tests/failures.rs::with_only_two_nodes_a_cut_excludes_nobody`

---

### UC-FAL-011

**Case:** `POST /cluster/vote-remove` (`VoteRequest`  
**Then:** `voteId`, `candidateNodeId`, `openedBy`, `reason`) answers `VoteResponse` with `vote` `yes` or `no`, according to what the voter sees of the candidate (for example, `no` if it sees the channel active). Whoever does not answer does not enter the base.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::vote_remove_answers_no_to_who_still_reaches_the_candidate_and_yes_to_who_does_not`

---

### UC-FAL-012

**Case:** With several pending exclusions, they are voted one at a time, each over the healthy nodes of the moment.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::when_several_nodes_fall_together_the_vote_goes_on_among_the_healthy_ones_one_at_a_time`

---

### UC-FAL-013

**Case:** Accepted risk  
**Then:** in a network partition where both sides have enough healthy nodes, each side may exclude the other; the excluded side, on returning, redoes the bootstrap from scratch (and, if **both** excluded, the reunion is UC-FAL-026).  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FAL-014

**Case:** 3-node cluster with one down  
**Then:** the 2 healthy ones vote and the quorum of 2 is reached.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-015

**Case:** Vote and result are recorded (`taba.cluster.vote` by `role` and `outcome`; `warn` and `info` logs).  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-016

**Case:** Once the quorum is reached, the deregistration follows the two-phase protocol  
**Then:** `POST /cluster/nodes/unregister` (`reason: vote`) to all nodes and `POST /cluster/nodes/commit`, which removes the node from the table across the cluster.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-017

**Case:** Failure before the commit  
**Then:** rollback. After it is decided: no rollback, the coordinator insists within `write.commit.timeout` until the removal is confirmed on all.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::an_exclusion_that_fails_before_the_commit_rolls_back_and_the_vote_is_held_again`
  - `tests/failures.rs::once_the_exclusion_is_decided_the_commit_is_insisted_until_every_node_confirms`

---

### UC-FAL-018

**Case:** At the commit, the ring is recomputed and the removed node's blocks are **rebalanced** among the others; the new primaries of its positions take over the volatile queues it managed, without transferring state.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::when_the_manager_leaves_the_new_primary_of_its_positions_takes_over`

---

### UC-FAL-019

**Case:** `taba.cluster.membership.changes` counts by `operation`, `reason` (`vote`, `drain`), `phase` and `outcome`.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-020

**Case:** Once the exclusion is complete, all nodes discard the signal queues for that node.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_stays_down_is_excluded_by_two_of_three_and_every_table_changes`

---

### UC-FAL-021

**Case:** Between the drop and the deregistration, the messages of which the node was the owner stay undelivered; after the deregistration, the new primary (the block's heir) delivers them.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary`

---

### UC-FAL-022

**Case:** After the exclusion, each message of the removed node remains on at least `replication.factor − 1` nodes.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::the_messages_of_the_excluded_node_wait_and_are_delivered_by_the_new_primary`

---

### UC-FAL-023

**Case:** A node in **failure** (full disk, corrupted database, event database above `cluster.sync.log.max-bytes`, or without a load source until `cluster.sync.timeout`) signals this to the cluster, and the others are free to vote its deregistration.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_announces_its_failure_is_excluded_while_its_channel_is_still_up`
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`
  - `tests/operations.rs::a_corrupt_topic_database_fails_the_node`

---

### UC-FAL-024

**Case:** The cluster conditions (a list, not a single state), each with a severity  
**Then:** `member-unreachable`, `node-frozen`, `syncing`, `under-replicated`, `write-quorum-at-risk` (healthy replicas below `write.quorum`), `quorum-lost`, `membership-divergent`, `storage-pressure`; `ok` when none.  
**Level:** C  
**Tests:**
  - `src/cluster/health/tests.rs::merging_nodes_unions_the_conditions_and_flags_divergent_tables`
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/conditions.rs::a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk`

---

### UC-FAL-025

**Case:** `under-replicated` (replicas below `replication.factor`, but **at or above** the quorum) is a transient warning  
**Then:** writes keep answering (except those of blocks whose primary is down, until the exclusion, which the `write-quorum-at-risk` condition announces: D107). `write-quorum-at-risk` is critical: writes may fail for not closing the quorum, and it also appears when the **primary** of some block is down (the quorum always includes it): D107.  
**Level:** C  
**Tests:**
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/conditions.rs::with_two_of_three_nodes_out_the_quorum_is_at_risk_and_the_writes_can_fail`
  - `tests/conditions.rs::a_node_that_syncs_shows_as_syncing_and_under_replicated_but_not_at_risk`

---

### UC-FAL-026

**Case:** The reunion after a partition in which **both sides** excluded each other (the tables have the same version and no channel links one side to the other)  
**Then:** from time to time (`cluster.channel.keepalive.interval` × 10) each node queries the table of whoever it saw being excluded; if it is from another cluster (without this node and with no member in common) and this side loses (has fewer members; on a tie, the one with the lowest `nodeId`), the node redoes the join from scratch, like the excluded one that returns (UC-FAL-013). The vote mark of a node that left the table is forgotten on leaving (D101). A node that is still **synchronizing** when the sources move to a newer table without it (the join it was finishing was undone by this reunion) does not wait for `cluster.sync.timeout` to enter failure: it forgets the table and redoes the join.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`
  - `tests/failures.rs::a_syncing_node_that_the_sources_excluded_redoes_the_join_instead_of_failing`

---

### UC-FAL-027

**Case:** The node in `failed` does not serve client calls  
**Then:** in a cluster it forwards the call to a healthy node (like the node in drain, UC-FWD); without another node, and on a single node, it answers `503` `node.failed` with `Retry-After`. The operations routes continue (it is through them that the failure is seen). The node keeps answering node-to-node calls until it is excluded (D78.2).  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_failed_node_forwards_the_client_calls_to_a_healthy_one_until_it_leaves`
  - `tests/failures.rs::a_failed_single_node_refuses_the_client_calls_with_503`

---

### UC-FAL-028

**Case:** A fatal disk error (full or corrupted) in `_node.db` (topic definitions, tokens, state, losses) also puts the node in `failed`, as in the message databases  
**Then:** the node cannot store what the cluster already confirmed. The other errors only apply to the operation. The node only leaves `failed` by restarting the process (in a cluster, after being excluded, it joins as a new node; D78.1, D78.3).  
**Level:** C  
**Tests:**
  - `src/storage/db/tests.rs::a_full_disk_in_the_node_database_fails_the_node_from_any_of_its_tables`
  - `src/storage/db/tests.rs::an_error_that_is_not_fatal_in_the_node_database_does_not_fail_the_node`

---

### UC-FAL-029

**Case:** When a node goes down, all the healthy ones open the vote, but only the one with the **lowest `nodeId`** among those that voted coordinates the deregistration (phase 1 and commit); the others wait their turn (`k × write.commit.timeout`) and, finding the node already out of the table, register nothing (D98).  
**Level:** C  
**Tests:**
  - `tests/failures.rs::only_the_lowest_node_id_among_the_healthy_coordinates_the_exclusion`

---

### UC-FAL-031

**Case:** The reunion rule after the symmetric partition chooses **a single** loser  
**Then:** the side with fewer members; on a tie, the side whose lowest `nodeId` is the lowest. Both sides, comparing the same two tables, reach the same conclusion: one redoes the join and the other does not (D101).  
**Level:** U  
**Tests:**
  - `src/cluster/service/tests.rs::the_reunion_has_exactly_one_losing_side_the_smaller_and_in_a_tie_the_one_of_the_lowest_id`
  - `tests/failures.rs::after_a_partition_where_each_side_excluded_the_other_the_losing_side_joins_again`

---

### UC-FAL-032

**Case:** The nodes excluded by vote that the node keeps for the reunion (D101) stay only in memory, at most 64 (the oldest leave first) and stop being queried when they return to the table.  
**Level:** U  
**Tests:**
  - `src/cluster/members/tests.rs::the_excluded_nodes_kept_for_the_reunion_are_the_last_64_and_leave_when_back_in_the_table`

---

### UC-FAL-033

**Case:** The reunion and recovery parameters apply as decided (D101, D110)  
**Then:** `cluster.reunion.interval` is ten times `cluster.channel.keepalive.interval` (at least 1 s), `cluster.message-loss.ttl` is 14 days and `cluster.recovery.timeout` is 5 minutes, and each can be changed by the configuration.  
**Level:** U  
**Tests:**
  - `src/cluster/service/tests.rs::the_reunion_and_recovery_parameters_default_to_what_was_decided_and_can_be_changed`

---


## FRZ: Freezing period

### UC-FRZ-001

**Case:** A node deregistered for failure that **remains the same process** (same ID, same memory) and tries to register again during `cluster.freezing.duration`  
**Then:** the join request is rejected with `403` `cluster.node-frozen` and `Retry-After` with the remaining time.  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-002

**Case:** The cluster stores the ID and the deregistration instant of each node in freezing.  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-003

**Case:** Once freezing has passed, the node joins normally  
**Then:** it tries again after the `Retry-After`, within the bootstrap's retry loop.  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-004

**Case:** There is no waiting list  
**Then:** the cluster does not store the rejected request. The rejection is recorded in the log ("rejected (freezing)").  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::an_excluded_node_that_stays_the_same_process_is_refused_until_the_freezing_ends`

---

### UC-FRZ-005

**Case:** A process **restart** generates a new ID  
**Then:** the node joins as a new node, **without freezing**.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`

---

### UC-FRZ-006

**Case:** Leaving by **drain** does not generate freezing (it is voluntary and the return is with a new ID).  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/drain_cluster.rs::after_leaving_the_node_returns_as_a_new_node_with_a_new_id`

---

### UC-FRZ-007

**Case:** `taba.cluster.freezing` and the `node-frozen` condition reflect the nodes in freezing.  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::the_nodes_in_freezing_show_in_the_metric_and_in_the_node_frozen_condition`

---

### UC-FRZ-009

**Case:** Freezing survives whoever coordinates  
**Then:** the response of `join` and `GET /cluster/nodes` carry `frozen` (`nodeId`, `frozenAt` in ms) with the excluded ones still in freezing; the node that joins after the exclusion installs it (without raising the `version`, and the stored table does not have it), and the `join` of the excluded one that falls on it is refused with `cluster.node-frozen` (D106).  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::a_node_that_joins_after_the_exclusion_inherits_the_freezing_of_the_excluded`

---

### UC-FRZ-010

**Case:** After the commit of the exclusion by vote, each node that made it effective queues the `frozen` frame (`nodeId`, `frozenAt` in ms) for **all** peers. Whoever did not yet know that freezing stores it — even with the excluded one still in its table — and passes it on to the other peers (D106).  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-FRZ-011

**Case:** The member that was down at the exclusion commit receives the `frozen` when the channel returns (the accumulated queue or the snapshot) and starts to refuse the excluded one's `join` with `cluster.node-frozen`, before any table reconciliation.  
**Level:** C  
**Tests:**
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-FRZ-012

**Case:** The `frozen` that the node already knows is an echo  
**Then:** it does not pass it on and does not change the stored instant (the first one counts).  
**Level:** U  
**Tests:**
  - `src/cluster/members/tests.rs::a_freezing_learned_twice_keeps_the_first_instant_and_only_the_first_is_news`

---

### UC-FRZ-013

**Case:** A `frozen` whose deadline (`frozenAt` + `cluster.freezing.duration`) has already passed is ignored. In the output queue the `frozen` neither expires nor is merged with another node's  
**Then:** only a `frozen` of the same `nodeId` replaces it.  
**Level:** U  
**Tests:**
  - `src/cluster/channel/queue/tests.rs::the_frozen_signal_never_expires_and_only_the_same_node_replaces_it`
  - `src/cluster/members/tests.rs::a_freezing_that_already_ended_is_not_learned`

---

### UC-FRZ-014

**Case:** The snapshot sent on opening the channel carries a `frozen` for each node still in freezing (covers the queue lost by a restart of whoever had it).  
**Level:** U, C  
**Tests:**
  - `src/cluster/members/tests.rs::the_frozen_list_has_only_the_nodes_still_in_freezing`
  - `tests/freezing.rs::the_channel_snapshot_carries_the_nodes_still_in_freezing`

---


## CNC: Concurrency between membership changes

### UC-CNC-001

**Case:** After a node joins (register commit), no other join, leave or rebalance takes effect until it is **ready**; bootstrap requests and exclusion votes that arrive in that interval **wait**. There is only one membership change at a time.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-002

**Case:** While an exclusion vote is in progress (from opening to the deregistration commit), a new bootstrap request waits; it is only processed when the vote ends, with or without an exclusion in effect.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-003

**Case:** The voluntary deregistration (drain) counts as a member leaving  
**Then:** it waits for another change in progress and, while it runs, blocks new bootstrap requests.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::two_nodes_draining_at_once_leave_one_after_the_other`
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-004

**Case:** Exception  
**Then:** the **node that is synchronizing itself**: if it goes into failure, the vote for its deregistration is released.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`

---

### UC-CNC-005

**Case:** A node that goes down during another's synchronization is only deregistered after that one finishes or fails.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_falls_while_another_syncs_is_excluded_only_when_the_sync_ends`

---

### UC-CNC-006

**Case:** The blocks of a node in synchronization do not change in the middle of the load (the cluster accepts no other join, leave or rebalance), and the event database does not bring member table changes.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::no_other_membership_change_takes_effect_until_the_syncing_node_is_ready`

---

### UC-CNC-007

**Case:** A node that restarts without memory (same IP and port) before the vote concludes  
**Then:** the bootstrap request enters the queue and waits; once the vote concludes (the old node removed), it is processed as an ordinary new-node register, with no special treatment for the repeated address.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_node_that_restarts_with_the_same_address_waits_for_the_vote_and_joins_as_a_new_node`
  - `tests/failures.rs::a_node_that_restarts_before_the_fall_is_detected_answers_wrong_node_and_is_replaced`

---

### UC-CNC-009

**Case:** Two membership changes coordinated by different members at the same time always have a winner  
**Then:** the one with the lowest `operationId` (the oldest) wins; whoever coordinates a change and receives the register of an older one discards its own and accepts the other; the displaced coordinator undoes what it registered, waits a random time and restarts (the join, without giving up before 60 s, the join deadline); the winner repeats the peers' `409` up to `write.commit.timeout`. A register held by a participant is never displaced (D94).  
**Level:** U, C  
**Tests:**
  - `src/cluster/members/tests.rs::a_coordinator_yields_to_an_older_registration_and_never_to_a_newer_one`
  - `src/cluster/members/tests.rs::a_registration_held_for_another_coordinator_is_never_displaced`
  - `tests/sync.rs::two_joins_coordinated_by_different_members_have_a_winner_and_neither_fails`

---

### UC-CNC-010

**Case:** The crossing rule (the oldest wins; whoever coordinates yields to an older one and never to a newer one) applies to any pair of membership changes, not only two joins  
**Then:** an exclusion that crosses a join follows the same rule in both directions (D94).  
**Level:** C  
**Tests:**
  - `src/cluster/members/tests.rs::an_exclusion_and_an_entry_that_cross_follow_the_same_rule_in_both_directions`

---

### UC-CNC-013

**Case:** The member that **missed the commit of a leave** (the exclusion by vote or the drain) applies the change when the channel delivers it  
**Then:** the `membership` frame carries the change and the table version, and the receiver applies it without depending on the pending one. Its table becomes equal to the others', the node leaves the ring and enters freezing (if it was an exclusion). The pending register of the same operation disappears, and the late commit is harmless.  
**Level:** C, U  
**Tests:**
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_is_applied_without_the_pending_one_and_the_late_commit_is_harmless`
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_clears_the_pending_one_it_stands_for`
  - `src/cluster/members/tests.rs::a_change_already_applied_or_older_is_ignored`
  - `src/cluster/members/tests.rs::the_freezing_of_a_node_learned_by_the_membership_frame_counts_from_the_instant_it_was_learned`
  - `tests/freezing.rs::a_member_that_missed_the_exclusion_commit_learns_the_freezing_from_the_channel`

---

### UC-CNC-014

**Case:** The member that **missed the commit of a join** also receives it through the channel  
**Then:** the join is not undone and the other member does not fall behind the table even with the lost commit.  
**Level:** C  
**Tests:**
  - `src/cluster/members/tests.rs::a_change_told_by_the_channel_is_applied_without_the_pending_one_and_the_late_commit_is_harmless`
  - `tests/cluster.rs::a_commit_that_does_not_arrive_does_not_undo_the_join`
  - `tests/conditions.rs::a_delayed_commit_does_not_leave_the_tables_divergent_because_the_channel_carries_the_change`

---

### UC-CNC-015

**Case:** Each node applies only the version **next** to its table's  
**Then:** an equal or lower version is ignored, one ahead is kept (up to 64, the nearest) until the previous ones arrive, and a change that does not fit the table (removing someone who is not a member) is discarded. Arriving from queues of different nodes, out of order, does not change the result.  
**Level:** U  
**Tests:**
  - `src/cluster/members/tests.rs::a_change_ahead_of_the_next_version_waits_for_the_ones_before_it`
  - `src/cluster/members/tests.rs::a_change_that_does_not_fit_the_table_is_dropped_and_the_held_ones_are_bounded`
  - `tests/channel.rs::membership_changes_from_two_peers_arriving_out_of_order_apply_in_version_order`
  - `tests/channel.rs::a_membership_frame_that_removes_the_receiving_node_is_ignored`

---


## SIN: Distributed synchronization of blocks

### UC-SIN-001

**Case:** Synchronization is **scoped to the affected blocks**  
**Then:** the rest of the cluster continues operating, without a global block.  
**Level:** C  
**Tests:**
  - `src/core/ring/tests.rs::the_ranges_of_the_blocks_cover_every_position_exactly_once`
  - `src/core/ring/tests.rs::a_node_syncs_only_the_blocks_it_is_responsible_for`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-002

**Case:** The new node asks **all the nodes responsible** for the block for the ID list (`GET /cluster/blocks/{id}/ids`, NDJSON ordered by ID).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `src/storage/messages/tests.rs::the_rows_of_a_block_come_by_position_in_id_order_with_range_and_limit`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-003

**Case:** A message already confirmed (ack) before the listing does not appear in any list and is **not a special case**  
**Then:** there is no permanent tombstone.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-004

**Case:** Consolidates the lists by the **union** (not the intersection  
**Then:** a replica may have received a message that another did not), by ordered *merge* of the streams, without materializing all the lists in memory.  
**Level:** U, C  
**Tests:**
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`
  - `tests/sync.rs::the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection`

---

### UC-SIN-005

**Case:** Splits the merged list into ranges and **balances** the `GET /cluster/blocks/{id}/messages?from=&to=` calls across the source nodes (without concentrating the load on one); what is missing in a range is fetched from another node; the single-ID route is the last resort; a cut stream is resumed from the last ID read.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::block_messages_carry_the_body_and_stop_at_the_item_and_byte_ceilings`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::the_load_is_the_union_of_the_lists_of_the_sources_not_their_intersection`

---

### UC-SIN-006

**Case:** The stream lines (topics and messages) are applied **straight to the node's final storage**, in batches, without writing twice (the load does not go through the event database).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-007

**Case:** Synchronization applies the message with the file and the row in the topic database, and a message of a still unknown topic waits for the topics (the topic load comes first).  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::the_ids_of_a_block_come_from_memory_and_disk_in_order`
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`
  - `tests/sync.rs::a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition`

---

### UC-SIN-008

**Case:** A message already expired by `ttl` is discarded in synchronization, instead of copied.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`

---

### UC-SIN-009

**Case:** `GET /cluster/blocks/{id}/messages/{messageId}`  
**Then:** `200` `MessageRecord`, or `404` `message.not-found`.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::block_messages_carry_the_body_and_stop_at_the_item_and_byte_ceilings`
  - `tests/sync.rs::the_block_routes_list_ids_in_order_and_serve_ranges_and_single_messages`

---

### UC-SIN-010

**Case:** During synchronization the node is already in the table and keeps receiving the normal protocol, but **does not process** the events  
**Then:** it writes each one to the local SQLite database `${storage.dir}/_sync/sync.db` (table `events`, increasing sequence number) in order of arrival. The events are register, commit and rollback of topics, tokens, messages, invisibility and acks.  
**Level:** C  
**Tests:**
  - `src/storage/synclog/tests.rs::events_are_kept_in_arrival_order_and_read_oldest_first`
  - `src/storage/synclog/tests.rs::the_database_lives_in_the_sync_folder_and_goes_away_with_discard`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-011

**Case:** The node **confirms the register after writing the event** to the database (and so counts toward the quorum normally). The write goes through a single thread.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-012

**Case:** Sequential processing  
**Then:** topic and token load, block load, event database processing, final check and switch to normal mode.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`
  - `tests/sync.rs::a_message_of_a_topic_created_after_the_topic_load_waits_for_the_definition`

---

### UC-SIN-013

**Case:** Processing of the event database **on top of** the load  
**Then:** always reads the oldest row, applies the event and **deletes the row**, until it is empty; there is no stored position.  
**Level:** U, C  
**Tests:**
  - `src/storage/synclog/tests.rs::events_are_kept_in_arrival_order_and_read_oldest_first`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-014

**Case:** The processing is idempotent and the event wins over the load  
**Then:** creating what exists by `id`, writing the same `invisibleUntil`, deleting what does not exist changes nothing; an ack or removal in the database deletes what the load brought old.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::loading_skips_what_expired_and_what_the_node_already_has_and_notes_unknown_topics`
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-015

**Case:** New events keep arriving at the database while it is processed.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-016

**Case:** **Final check:** with the event table almost empty, the node repeats the listings only for what it intends to keep and discards what no longer appears on any source node (covers an ack whose request never reached the node).  
**Level:** C  
**Tests:**
  - `src/storage/synclog/tests.rs::the_ids_of_the_load_are_kept_by_block`
  - `tests/sync.rs::the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore`

---

### UC-SIN-017

**Case:** **Switch to normal mode (*cutover*):** when the `events` table becomes empty, under the same control that the reception uses to decide the destination of each event (no event falls between the two modes), the node starts to process in real time and nothing more is written to the database; `sync.db` is discarded and the node becomes healthy.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-018

**Case:** An ack that arrives during synchronization does not "resurrect" the message  
**Then:** the event is applied on top of the load and the message disappears.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::events_that_arrive_during_the_sync_are_logged_and_replayed_over_the_load`

---

### UC-SIN-019

**Case:** Ready  
**Then:** loads applied, event database processed and discarded, final check done. The node rebuilds the ready and invisible lists of the blocks of which it is primary, receives the channel snapshot, takes over the volatile queues and `/health/ready` answers `200`. Until then it neither coordinates nor delivers, and client calls are forwarded.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_new_node_loads_its_blocks_but_does_not_deliver_until_it_is_ready`

---

### UC-SIN-020

**Case:** **Synchronization failure:** full or failing disk, event database above `cluster.sync.log.max-bytes`, or corrupted local database  
**Then:** the node goes into failure and signals the cluster. Without a source node for a load, it tries again and, when `cluster.sync.timeout` is exhausted, falls on the same path.  
**Level:** C  
**Tests:**
  - `tests/failures.rs::a_syncing_node_that_fails_is_excluded_by_the_others`
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`
  - `tests/sync.rs::a_node_whose_join_commit_reaches_the_sources_late_waits_for_them_and_stays_in`

---

### UC-SIN-021

**Case:** Cleanup of the nodes that lost the block  
**Then:** see UC-ARM-022 and UC-ARM-023.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::copies_of_blocks_the_node_no_longer_serves_are_forgotten_and_the_rest_stay`
  - `tests/sync.rs::a_node_that_lost_a_block_keeps_its_messages_until_the_whole_cluster_is_ready`

---

### UC-SIN-022

**Case:** On FIFO, the publications of a group whose primary is synchronizing answer `503` (UC-FIF-029); synchronization carries the rows with `deleteAt` with no new route.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::the_ack_line_lives_in_every_node_and_a_new_primary_only_coordinates_once_ready`

---

### UC-SIN-023

**Case:** Metrics  
**Then:** `taba.sync.blocks.active`, `duration`, `ids.listed`, `messages.pulled` by `peer`, `bytes`, `log.events`, `log.bytes`, `phase` (`topics`, `blocks`, `replay`, `verification`, `ready`) and `failures` by `reason`. Logs of the start and end of block synchronization (`info`).  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`

---

### UC-SIN-025

**Case:** A listed message that cannot be obtained is **not declared lost at once** (it may have been confirmed and deleted while the node synchronized)  
**Then:** it tries (1) the range on another node, (2) the single-ID route, (3) re-lists the IDs of only those that failed.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-026

**Case:** If the ID no longer appears on any node, it was an ack  
**Then:** the case is **silently discarded** (no log, no record).  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-027

**Case:** Only what remains listed and without content on any node is a **loss**  
**Then:** an `error` log line with the topic name, the ID, the block and the nodes queried, and a record in the `losses` table of the node and of the nodes responsible for the block (idempotent by the key).  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-028

**Case:** The loss does **not compromise the health** of the node nor interrupt the synchronization, which continues with the other messages. There is no metric or health condition.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-SIN-029

**Case:** The residual window (an ack whose request never reached the node or the event database, having been computed before it took its position on the ring) is treated as the same kind of rare exception  
**Then:** observable event, not a health failure.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::the_final_check_drops_what_the_load_brought_and_no_source_lists_anymore`

---

### UC-SIN-030

**Case:** The node that the sources do not yet know (they answer `cluster.not-member` until the join commit arrives) waits `write.retry.backoff` and lists the block again, until `cluster.sync.timeout`; if some source serves, synchronization continues with it. When the deadline is exhausted with no source, synchronization fails with `no-source` and the node goes to `failed` (D105).  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_node_with_no_source_for_a_load_fails_after_the_timeout`
  - `tests/sync.rs::a_node_whose_join_commit_reaches_the_sources_late_waits_for_them_and_stays_in`

---

### UC-SIN-031

**Case:** A node that joins copies, block by block, each source's log of excluded messages (`GET /cluster/blocks/{id}/excluded`) and records **from when the sources record** (the most recent of their start dates, read in the `NodeStatus`), not from the instant it joined  
**Then:** the `id` that is dead on the sources is dead in the new node's log too, and its start date is the sources'; if a source does not send the log or the date (earlier version, failure), the node keeps its own date, which is the conservative one. Only the first synchronization of a disk inherits; after that the date only moves forward.  
**Level:** C and U  
**Tests:**
  - `tests/excluded_log.rs::a_new_node_copies_the_log_of_its_sources_and_inherits_their_start`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-SIN-032

**Case:** The cluster has a **cut-off date for the log of excluded messages**: the most recent of the start dates among the active members  
**Then:** the date appears in `summary.excludedCutoff` of `GET /health/cluster`; before it, some member's log may lack the exclusion, and the restart must not treat it as guaranteed.  
**Level:** C  
**Tests:**
  - `tests/excluded_log.rs::the_cluster_cut_off_is_the_latest_start_among_the_active_members`

---


## STR: NDJSON streams between nodes

### UC-STR-001

**Case:** `Content-Type: application/x-ndjson`, one JSON object per line; sender and receiver work with one buffer per line, without building the whole response in memory.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/tests.rs::a_stream_ends_with_the_end_line_and_without_it_the_chunk_is_a_failure`

---

### UC-STR-002

**Case:** The order is by **ascending ID** (GUID v7 compared as text in canonical lowercase form).  
**Level:** U  
**Tests:**
  - `src/cluster/replication/definitions/tests.rs::the_definition_streams_list_by_ascending_id_and_not_by_name`
  - `src/engine/messages/tests/blocks.rs::the_block_streams_come_in_ascending_id_order_whatever_the_order_of_arrival`

---

### UC-STR-003

**Case:** `from` and `to` (both inclusive and optional) delimit the range; optional `limit` lowers the item ceiling.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/dto_tests.rs::the_stream_query_carries_from_to_and_limit_and_all_three_are_optional`
  - `src/cluster/replication/definitions/tests.rs::from_is_inclusive_and_limit_lowers_the_ceiling_of_the_definition_streams`
  - `src/engine/messages/tests/blocks.rs::from_and_to_are_inclusive_and_limit_lowers_the_ceiling_of_the_block_streams`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-STR-004

**Case:** The stream ends with the `StreamEnd` line (`end: true`, `count`, `last`, `hasMore`); without that line the chunk is considered **failed**, even if the connection closed cleanly.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/tests.rs::a_stream_ends_with_the_end_line_and_without_it_the_chunk_is_a_failure`

---

### UC-STR-006

**Case:** Resume after a cut or `hasMore`  
**Then:** the receiver repeats with `from=<last ID read>`; the sender returns that line again and the receiver **ignores** it (idempotent application by ID). No token or state on the server.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/dto_tests.rs::a_stretch_names_the_last_id_to_resume_from_and_the_resumption_asks_for_it_again`
  - `src/engine/messages/tests/blocks.rs::resuming_from_the_last_id_repeats_that_line_and_the_receiver_ignores_it`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-STR-009

**Case:** Line size limit (`cluster.sync.stream.max-line-bytes`)  
**Then:** the largest possible message must fit (hard-limit in base64 plus the envelope).  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::the_stream_line_limit_is_exactly_the_hard_limit_in_base64_plus_the_envelope`
**Note:** Only the startup configuration rule is proven; the sender does not yet enforce the line size.

---

### UC-STR-011

**Case:** Lines of each route  
**Then:** `TopicDefinition`, `TokenRecord`, `{id, kind, key, version}` (tombstones), `{id, topic}` (and `deleteAt` on the FIFO) for the block's IDs, `{id, topic, position, atMs}` for the excluded, `{id, topic}` for the losses and `MessageRecord` (without `body` and with `deleteAt` on the FIFO with logical deletion).  
**Level:** U  
**Tests:**
  - `src/cluster/replication/definitions/tests.rs::the_topic_and_token_lines_are_the_record_plus_the_version_this_node_applied`
**Note:** Only the definition and token lines are covered; the tombstone, block ID, excluded and loss lines have no unit test yet.

---

### UC-STR-012

**Case:** The `MessageRecord` between nodes only has two forms of `body`  
**Then:** native JSON (`application/json`, `*/*+json`) or base64 with `bodyEncoding: "base64"` (text also goes in base64).  
**Level:** U  
**Tests:**
  - `src/cluster/protocol/tests.rs::a_json_body_goes_native_and_the_rest_goes_in_base64`

---


## RTN: Return of an excluded node and recovery of losses

### UC-RTN-001

**Case:** A node deregistered for channel failure that returns queries **all** the nodes it can reach (`GET /cluster/message-loss`, NDJSON) and merges the responses (each node answers with its `losses` table). These routes accept calls from a node that is not yet a member.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-002

**Case:** For each (topic, ID) pair, the node looks for the message locally  
**Then:** on disk (`${topic}/${id}.body` and the topic database) and, if the process is the same one that was out (no restart), also in memory. On a restart, only the disk counts, and the restart **goes through** this procedure: the new process sets aside the disk of the previous life and joins; after becoming active, in the background, it queries the loss list of the members that the entry listing brought and returns to the responsible node what that disk has of them (D111).  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has`

---

### UC-RTN-003

**Case:** For each message found, the node sends it to the block's responsible node through `POST /cluster/messages/recover`; whoever receives writes **directly to the responsible nodes**, idempotent by ID, without the two phases (neither pending register nor rollback), and confirms the row when `write.quorum` nodes wrote.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_recover_body_is_a_stream_with_a_mandatory_last_line`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`
  - `tests/recovery.rs::resending_a_recover_batch_ignores_what_was_already_applied`

---

### UC-RTN-004

**Case:** The body and the `deliveryCount` go; the invisibility is **cleared** and the message becomes ready for delivery again.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`

---

### UC-RTN-005

**Case:** Clearing the record  
**Then:** the node that accepts the message deletes the pair from its `losses` table and notifies the others (best effort), so a second node that returns with the same message does not resend it after it was already delivered and confirmed.  
**Level:** C  
**Tests:**
  - `src/storage/losses/tests.rs::a_loss_is_removed_by_the_recovery_and_expires_by_age`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`

---

### UC-RTN-006

**Case:** Quarantine  
**Then:** while recovery does not finish, the node **does not delete** the old data. If some node did not answer the query, it tries again every `cluster.bootstrap.retry.interval`, until `cluster.recovery.timeout`; only then does it proceed.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query`
  - `tests/recovery.rs::when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has`

---

### UC-RTN-007

**Case:** Once recovery is complete (or the deadline exhausted), the node clears its local state and starts a **new register from scratch**, the full bootstrap, as a new node and with a new position on the ring. Freezing only delays this step.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::when_the_recovery_timeout_runs_out_the_node_goes_on_with_the_lists_it_has`

---

### UC-RTN-008

**Case:** `losses` table  
**Then:** key (`topic`, `id`), fields `block` and `detected_at`; the node that detects it writes and also sends to the nodes responsible for the block (the information survives the detector's failure); the records expire after `cluster.message-loss.ttl`.  
**Level:** U, C  
**Tests:**
  - `src/storage/losses/tests.rs::a_loss_is_keyed_by_topic_and_id_and_listed_by_id`
  - `src/storage/losses/tests.rs::a_loss_is_removed_by_the_recovery_and_expires_by_age`
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-009

**Case:** Messages already applied in a resend of the `recover` are ignored (`taba.recover.messages` with `outcome` `sent`, `applied`, `skipped`).  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::the_recover_body_is_a_stream_with_a_mandatory_last_line`
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::resending_a_recover_batch_ignores_what_was_already_applied`

---

### UC-RTN-010

**Case:** The node state during the return is `returning` (`taba.node.state`, `/health/ready` with `reason: returning`); log of the recovered messages (`info`).  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::an_excluded_node_sends_back_the_lost_messages_it_still_has_and_then_rejoins`
  - `tests/recovery.rs::the_node_keeps_its_old_data_while_a_node_does_not_answer_the_loss_query`

---

### UC-RTN-012

**Case:** `GET /cluster/message-loss` and `POST /cluster/messages/recover` accept whoever is not yet a member; `POST /cluster/message-loss` (register) and `DELETE /cluster/message-loss/{topic}/{id}` (clear) only accept members (`403 cluster.not-member`). Register and clear are idempotent (`changed`), and the record expires after `cluster.message-loss.ttl`.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::the_loss_routes_take_a_caller_that_is_not_a_member_for_the_list_and_the_recovery_only`

---

### UC-RTN-013

**Case:** The node that left the table (drain) does not clean up the blocks it lost  
**Then:** the replicas it keeps stay for the recovery to look for when it returns.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`

---

### UC-RTN-014

**Case:** The process restart **does not wait** for recovery to join  
**Then:** the node sets aside the disk of the previous life (`${storage.dir}/_lives/<old ID>`), joins with an empty disk and becomes `active` as soon as synchronization finishes, even if a member of the listing does not answer (PRB-048).  
**Level:** C  
**Tests:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`

---

### UC-RTN-015

**Case:** After becoming active, the node that restarted recovers in the background  
**Then:** queries the loss list, returns to the responsible node what the disk of the previous life has of them and **keeps** the folder of the life (it is only deleted when the restoration ends, [07-cluster.md](07-cluster.md), section 10.8; the `_previous-life` folder, of a disk with no manifest, is deleted). The member that was mute is called again every `cluster.bootstrap.retry.interval`, until `cluster.recovery.timeout` (5 min, D110); when exhausted, the node proceeds with the lists it has.  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute`

---

### UC-RTN-026

**Case:** A life **inside the window** of the log (its `last_seen` is after the cluster cut-off date plus the slack) has **all** its messages sent to the **new primary** of the block, with the original ID, without any node having declared a loss; what the primary already has (`Existing`) is skipped
**Then:** the primary activates through the path of `recover` (it writes and replicates with the quorum); the `deliveryCount` stays, the invisibility and the receipt are zeroed; what the primary confirms leaves the folder of the life, and a new pass sends nothing again. The life's log of excluded messages is added to the node's, and the start of the log goes back when the intervals meet with no hole. A life outside the window follows the path of the loss list (UC-RTN-018).
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::a_life_inside_the_window_is_sent_whole_to_the_new_primary_with_its_original_id`
  - `src/engine/messages/tests/lives.rs::the_log_of_a_life_is_added_to_the_nodes_and_the_start_goes_back_only_if_they_meet`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_restore_and_dead_routes_refuse_a_wrong_media_type_and_an_unknown_parameter`

---

### UC-RTN-027

**Case:** A message of a topic that was **deleted**, or **recreated with another `topicId`** (the life's one is recorded with the topic, in the folder)
**Then:** the primary discards it without asking the dead; what was discarded counts as resolved, and leaves the folder of the life. A folder from before the record does not prove the recreation, and the topic counts as the same.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::messages_of_a_deleted_or_recreated_topic_are_discarded_without_asking_the_dead`
  - `src/engine/messages/tests/lives.rs::the_topic_of_a_message_of_a_life_is_the_same_gone_or_recreated`
  - `src/storage/messages/tests.rs::the_folder_of_a_topic_records_the_topic_id_it_was_created_for_and_keeps_it`

---

### UC-RTN-028

**Case:** The `id` is in the primary's **log of excluded messages**, or in the one of **any other present node** (the query goes in a batch, per topic, to all the present nodes, not only to the block's owners)
**Then:** the message is discarded; the other nodes' log answers through `POST /cluster/topics/{topic}/messages/dead`.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::the_new_primary_discards_what_its_own_log_or_the_log_of_any_present_node_says_is_dead`
  - `src/engine/messages/tests/lives.rs::the_node_tells_which_ids_its_log_of_excluded_messages_has`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`
  - `src/cluster/protocol/tests.rs::the_restore_and_dead_routes_refuse_a_wrong_media_type_and_an_unknown_parameter`

---

### UC-RTN-029

**Case:** With the S3 backup, the topic's **journal** registers the removal of the `id`; or S3 is unreachable
**Then:** the removal in the journal discards the message (the journal is read once per topic, with a short cache); an unreachable S3 is absent evidence: the primary decides with its own log and the other nodes' and records a warning.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::the_journal_in_s3_discards_what_it_removed_and_a_silent_s3_only_leaves_a_warning`

---

### UC-RTN-030

**Case:** **Durable receipt**: the primary writes the restored message **to disk**, on every owner, and only confirms with the quorum
**Then:** only what the primary confirmed is marked as sent (it leaves the folder of the life); what it did not confirm stays in the folder and is sent again until the `cluster.recovery.timeout` deadline; once confirmed, nothing is sent again.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::a_restored_message_is_on_the_disk_of_every_owner_even_with_memory_to_spare`
  - `tests/restore_lives.rs::a_batch_the_primary_did_not_confirm_stays_in_the_life_and_is_sent_again`

---

### UC-RTN-031

**Case:** **Vote** to end the restoration: when **every present node** announced that it finished sending its lives, the coordinator (the node with the lowest ID) closes the vote
**Then:** the cluster status becomes **restored from disk**; only then are the FIFO queues released and the folders of the lives deleted (a folder whose messages the primaries did not all confirm stays for the next start).
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `src/cluster/restore/tests.rs::the_coordinator_is_the_lowest_id_and_the_vote_needs_every_present_node`
  - `src/cluster/restore/tests.rs::the_status_goes_from_unknown_to_restoring_to_restored_and_remembers_force_and_missing`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-RTN-032

**Case:** The restoration of the old messages **waits for the expected nodes** (the members of the biggest old table) and, after `G`, goes on with a warning and the nodes that are present; in **strict mode** it stays held until the operator forces it
**Then:** while the vote does not close the FIFO stays without delivery and the cluster shows `previous-life-pending`; the nodes the restoration still waits for are in the state of the node.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`

---

### UC-RTN-033

**Case:** A node that comes back **after the release**
**Then:** it enters clean: the status is already **restored from disk**, so its lives are restored on their own, with no vote and no hold, and its FIFO messages are activated in `id` order (their order in relation to newer messages of the group is not guaranteed); the folders are deleted when the primaries confirmed everything.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::a_node_that_comes_back_after_the_release_restores_its_lives_on_its_own_and_holds_nothing`

---

### UC-RTN-034

**Case:** The **plain queues** during the restoration
**Then:** they receive and deliver as soon as the definitions are distributed; the old messages come in later, when the restoration starts.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`

---

### UC-RTN-035

**Case:** The operator **forces** the restoration (`POST /cluster/restore/force`, spread to every node)
**Then:** the status changes to restored from disk on every present node, the held FIFO is released and a restoration held by the strict mode goes on with the nodes that are present; it also serves a cluster stuck by the loss of two nodes.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`
  - `src/cluster/protocol/tests.rs::every_request_survives_the_trip_to_http_and_back`

---

### UC-RTN-036

**Case:** Every node of a cluster that fell **comes back together**, with its disks, and **no one answers** the entry address (none is the `cluster.first-node`)
**Then:** each node tries the nodes of the previous life's table (by the IPs in the manifest); after `cluster.restore.cold-start.wait`, the one with the **lowest old ID** among those alive starts alone and **takes over the disk**, and the others join it. The messages of the lives are restored, the status becomes restored from disk on all of them and the folders of the lives are deleted.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-RTN-037

**Case:** The entry address does not answer, but there are nodes up
**Then:** the node that comes back finds the cluster through the **previous life's table of IPs** (from the manifest): the one that answers with a table is the cluster it joins.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_restarted_node_finds_the_cluster_through_the_old_table_when_the_entry_address_is_dead`
  - `tests/cold_start.rs::every_node_comes_back_at_once_and_the_one_with_the_lowest_old_id_starts_the_cluster`

---

### UC-RTN-038

**Case:** A node **with no previous life** and no `cluster.first-node`
**Then:** never starts alone: a listing with no answer is still an error and it tries again. Only `cluster.first-node` creates a cluster that never existed.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_node_with_no_previous_life_never_starts_alone_without_first_node`

---

### UC-RTN-039

**Case:** A node that **joins** a running cluster has on its disk a definition (topic, token or tombstone) with a **higher version** than the cluster's, or one the cluster does not know
**Then:** before deleting the disk the node keeps the definitions and, once active, **offers** each one to the cluster: the highest version wins (tombstone included) and what wins is spread to the others. A disk older than the cluster imposes nothing.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_node_that_joins_offers_the_definitions_of_its_disk_and_the_highest_version_wins`

---

### UC-RTN-040

**Case:** Two groups of nodes came up without talking to each other (each took over the disk) and meet again
**Then:** the **smaller** group (tie: the one of the lowest `nodeId`, D101) loses, **wiping nothing**: its disk becomes the folder of a life (`_lives/<ID>`), its definitions are offered to the winning group and the node redoes the join **with a new ID**. What the losing group accepted while it was apart is restored and checked against the dead, and the queue (a definition) stays in the cluster.
**Level:** C
**Tests:**
  - `tests/regroup.rs::the_smaller_group_that_meets_the_other_one_moves_its_disk_to_a_life_and_joins_with_a_new_id`

---

### UC-RTN-041

**Case:** The cluster has fewer active members than the table of the previous life
**Then:** each node probes, every `cluster.restore.probe.interval`, the old IPs that are not in the table; the probe **ends** when the table completes, and nothing is shown.
**Level:** C
**Tests:**
  - `tests/regroup.rs::the_probe_ends_when_the_table_completes_and_raises_no_condition`

---

### UC-RTN-042

**Case:** The probe time (`cluster.restore.probe.timeout`) ends with nodes of the old table missing
**Then:** the log warns and `GET /health/cluster` shows `members-missing` (warning) while the table is incomplete; the node that is missing enters later through the normal join.
**Level:** C
**Tests:**
  - `tests/regroup.rs::when_the_probe_time_ends_with_nodes_missing_the_members_missing_condition_shows`

---


### UC-RTN-043

**Case:** A node with `replication.factor=1`, without `cluster.tls.enabled` and without `cluster.entry-address` (the one-node profile)
**Then:** starts alone as a cluster of one, becomes `active`, serves the plain and the FIFO queue (the FIFO quorum is `min(2, factor)`), and `GET /health/cluster` shows the `reduced-profile` condition; with `health.single-node-condition=false` it does not show.
**Level:** C
**Tests:**
  - `tests/single_node.rs::a_node_with_factor_one_starts_alone_and_serves_the_plain_and_the_fifo_queue`
  - `tests/single_node.rs::the_reduced_profile_condition_can_be_hidden`
  - `tests/fifo_cluster.rs::auto_create_accepts_fifo_with_any_replication_factor`

---

### UC-RTN-044

**Case:** `auth.dev-token` with a secret of at least 32 characters, in the one-node profile
**Then:** the start creates the work token `dev` with `admin`, `publish` and `consume` (only the hash is stored) and turns authentication on: no token gets `401`, with `tb_dev.<secret>` the routes answer, and the secret does not show in `GET /tokens`.
**Level:** C
**Tests:**
  - `tests/single_node.rs::the_dev_token_is_an_admin_work_token_created_at_the_start_and_turns_authentication_on`

---

### UC-RTN-045

**Case:** A message the node already keeps (`Existing`) also arrives in the restoration of a life, with a greater `deliveryCount`
**Then:** the node keeps the **greater** of the two counters (it never decreases) and does not create the message again; invisibility and receipt do not change. Each owner does it with its own copy (PRB-062).
**Level:** U, C
**Tests:**
  - `src/engine/messages/tests/blocks.rs::of_two_copies_the_greater_delivery_count_prevails_and_never_decreases`
  - `tests/restore_lives.rs::a_message_the_primary_already_has_takes_the_greater_delivery_count_of_the_two_copies`

---

### UC-RTN-046

**Case:** The whole cluster falls, and a message was **only in memory** on every node
**Then:** it is lost: no disk had it and no node hands it back after the resumption; what writes memory is the drain (UC-DRN-010).
**Level:** C
**Tests:**
  - `tests/cold_start.rs::what_was_only_in_memory_on_every_node_is_lost_when_the_whole_cluster_falls`

---

### UC-RTN-047

**Case:** A fall makes the log of excluded messages lose its last entries (and the deletion of the message they went with)
**Then:** the message the consumer had already acknowledged comes back ready and is delivered again; it is accepted, because the consumer is idempotent (*at-least-once*, [07-cluster.md](07-cluster.md), section 10.10).
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_fall_that_loses_the_last_entries_of_the_log_delivers_the_acknowledged_message_again`

---

### UC-RTN-048

**Case:** The folders of the previous lives grow (several lives, with many large messages)
**Then:** they have no cap of their own: everything is restored and nothing is refused or discarded for its size; a full disk follows the usual rule (a fatal error, the node goes to `failed`, UC-FAL-028).
**Level:** I
**Tests:**
  - `tests/messages.rs::the_folders_of_the_previous_lives_have_no_cap_of_their_own`
  - `src/storage/db/tests.rs::a_full_disk_in_the_node_database_fails_the_node_from_any_of_its_tables`

---
### UC-RTN-049

**Case:** A life whose `last_seen` is earlier than the cluster cutoff date (with the margin) reaches a node that keeps it
**Then:** it is **not** restored: nothing of it is sent, no node receives the message, and it follows today's path (only the listed losses come back); at the end of the pass, which is complete because there was nothing to send, the folder of the life is deleted.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::a_life_outside_the_window_is_not_restored_and_its_folder_goes_with_the_end_of_the_pass`

---

### UC-RTN-050

**Case:** The new primary already has the message the life sends (`Existing`)
**Then:** it is **skipped**: it is not activated again, so the invisibility and the receipt of the consumer holding it stay as they were and the `deliveryCount` does not decrease; the row leaves the folder of the life as sent.
**Level:** C
**Tests:**
  - `tests/restore_lives.rs::a_message_the_primary_already_has_is_skipped_and_keeps_its_invisibility_and_receipt`

---

### UC-RTN-051

**Case:** A node that comes back after the release brings FIFO messages of one group in the folder of its life, out of order
**Then:** they are activated in `id` order: consuming (one per group at a time) delivers them in order; their order relative to the newer messages of the group is not guaranteed.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::the_fifo_messages_of_a_node_that_comes_back_are_activated_in_id_order`

---

### UC-RTN-052

**Case:** The cluster is **stuck by the loss of two nodes** (the old table had five, strict mode is on and `G` passed)
**Then:** the restoration and the FIFO stay held until the operator **forces** (`POST /cluster/restore/force`, on any node): the status changes on every present node, what was held goes on, and forcing again changes nothing (`wasRestoring=false`).
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::a_cluster_stuck_by_the_loss_of_two_nodes_is_released_by_force_and_forcing_again_changes_nothing`

---

### UC-RTN-053

**Case:** **Property on the simulated cluster:** three rounds in which the cluster publishes, confirms (ack) part of the messages and falls whole, and every node comes back
**Then:** no message confirmed before the fall is delivered again after the return, and the cluster delivers what was left (the property is not vacuous). It is the cluster counterpart of the model in UC-RTN-021.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::with_every_node_back_no_message_that_was_confirmed_is_delivered_again_whatever_the_falls`

---
### UC-RTN-054

**Case:** A message already consumed whose exclusion record was only on a node that did not come back, and the same message with every expected node present
**Then:** with the node absent, nothing is delivered again before `G`; once `G` passes, the restoration goes on with the nodes that are present and the message comes back (the residual redelivery of section 10.10 of 07-cluster.md, which the idempotent consumer accepts). With every expected node present, the restoration does not wait for `G` and the message that the log of one of them marks as consumed does not come back.
**Level:** C
**Tests:**
  - `tests/restore_vote.rs::a_message_already_consumed_whose_log_is_on_a_node_that_never_came_back_returns_only_after_g`
  - `tests/restore_vote.rs::with_every_expected_node_present_a_message_already_consumed_does_not_wait_for_g_and_does_not_return`

---
### UC-RTN-055

**Case:** Several nodes connect in the cold start, and the coordinator (the one with the lowest old ID) has the oldest disk
**Then:** the highest version of each definition wins on every node, the definition that only one node had reaches the cluster and the ones everyone had are still there.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::when_several_nodes_connect_the_highest_version_of_each_definition_wins_even_over_the_disk_of_the_coordinator`

---
### UC-RTN-056

**Case:** A late node starts alone, because no other answers, and then another node arrives with a higher version of a definition
**Then:** the node that started alone serves the definitions it has; when the other node answers, the window ends and the higher version wins on both.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_late_node_that_starts_alone_serves_what_it_has_and_the_higher_version_wins_when_another_answers`

---
### UC-RTN-057

**Case:** The probe of the old IPs ends with a node missing, and the node that was missing arrives later
**Then:** the `members-missing` condition shows in `/health/cluster`; the node that was missing enters through the normal join, the table completes and the condition goes away.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::the_node_that_was_missing_when_the_probe_ended_joins_later_by_the_normal_join_and_the_condition_goes_away`

---
### UC-RTN-016

**Case:** What the disk of the previous life has and the loss list does not ask for does not go back to anyone, and the new disk never resurrects an old message  
**Then:** after recovery only the copies that synchronization brought remain on the node.  
**Level:** C  
**Tests:**
  - `src/storage/messages/tests.rs::the_old_disk_is_set_aside_and_the_node_goes_on_with_an_empty_one`
  - `tests/recovery.rs::a_process_that_restarts_hands_back_the_lost_messages_its_old_disk_has`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`

---

### UC-RTN-017

**Case:** The **first node** of a new cluster (`cluster.first-node=true`, listing with no one) has no one to recover from  
**Then:** a disk with **no manifest** (earlier version) is deleted at start, by the usual path, and the `_previous-life` folder does not stay (D111); a disk **with a manifest** becomes the folder of the life (UC-RTN-023) and is restored by the cold start (UC-RTN-036; [07-cluster.md](07-cluster.md), section 10.5).  
**Level:** C  
**Tests:**
  - `tests/recovery.rs::the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life`

---

### UC-RTN-018

**Case:** A **life** (the disk of an earlier run) is restored only if its `last_seen` is **later than the cluster cut-off date plus the clock slack** (`cluster.restore.clock-slack`); with no known cut-off date, no life is restorable  
**Then:** a life outside the window follows the path of the listed losses (new identity, disk discarded); the slack leans to the safe side (a life too close to the date is not restored).  
**Level:** U  
**Tests:**
  - `src/cluster/restore/tests.rs::a_life_is_restorable_only_if_seen_after_the_cutoff_plus_the_clock_slack`

---

### UC-RTN-019

**Case:** The life's **manifest** (`manifest.json` in its folder) keeps the node ID, the active members of the table with their IPs and port, the table version, the `last_seen` and the watermark of the largest `id`; it is written by atomic replacement  
**Then:** it reads back the same, a new write (the periodic `last_seen`) replaces the previous one leaving no temporary file, and a folder without a readable manifest (a life of an earlier version) is not a restorable life.  
**Level:** U  
**Tests:**
  - `src/cluster/restore/tests.rs::the_manifest_is_written_atomically_and_read_back`
  - `src/cluster/restore/tests.rs::a_folder_without_a_readable_manifest_is_not_a_life`

---

### UC-RTN-020

**Case:** The restoration of the old messages waits for the **expected nodes** (the active members of the **biggest** old table, by the manifests announced): with all of them present it starts at once; with one missing it waits for the grace period `G` (`cluster.restore.grace`, 5 min)  
**Then:** after `G` it goes on with a warning and the nodes that are present; with `cluster.restore.strict=true` it stays held until the force operation, and forcing releases it even before `G`.  
**Level:** U  
**Tests:**
  - `src/cluster/restore/tests.rs::the_expected_nodes_are_the_members_of_the_biggest_old_table`
  - `src/cluster/restore/tests.rs::the_restoration_waits_for_the_expected_nodes_until_the_grace_period`
  - `src/cluster/restore/tests.rs::in_strict_mode_the_restoration_stays_held_after_the_grace_period_until_forced`

---

### UC-RTN-021

**Case:** **Property** (model of a cluster that stops as a whole): with **every expected node present** and the rule of the window, no message already acknowledged (ack) is delivered again, whatever the nodes' stops and the log trims  
**Then:** the test sweeps 2000 random histories and fails on the first redelivery; the same model **without** the window rule does produce redeliveries (the test proves the property has teeth).  
**Level:** U (model; the simulated-cluster test is UC-RTN-053)  
**Tests:**
  - `src/cluster/restore/tests.rs::with_every_expected_node_present_no_consumed_message_comes_back`
  - `src/cluster/restore/tests.rs::the_property_has_teeth_without_the_rule_of_the_window_a_consumed_message_comes_back`

---

### UC-RTN-022

**Case:** **Property** (the same model, with absent nodes): a message already acknowledged only comes back if **no present node** logged its ack; and, while an expected node is missing and `G` has not passed, the restoration does not start (not even in strict mode)  
**Then:** the residual redelivery of section 10.10 of 07-cluster.md only happens when whoever logged the exclusion is absent, and only after `G` (or after forcing).  
**Level:** U (model; the simulated-cluster test comes with the restoration)  
**Tests:**
  - `src/cluster/restore/tests.rs::with_a_node_missing_only_what_every_logger_of_it_missed_can_come_back`
  - `src/cluster/restore/tests.rs::nothing_starts_before_the_grace_period_while_an_expected_node_is_missing`

---

### UC-RTN-023

**Case:** The disk of the previous life **always** becomes a folder with the **old ID** in its name (`${storage.dir}/_lives/<old ID>`, with the topics and the manifest), whether the node found the cluster or not; several lives coexist (a second restart does not erase the first)
**Then:** the first node of a new cluster also sets a disk with a manifest aside instead of wiping it; a life with no messages keeps the manifest, because it counts for the expected nodes; the ID of the life has to be a folder name.
**Level:** C
**Tests:**
  - `src/storage/messages/tests.rs::a_disk_with_a_manifest_becomes_the_folder_of_its_life_and_the_lives_coexist`
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_first_node_does_not_wipe_a_disk_that_has_a_manifest_it_keeps_it_as_a_life`

---

### UC-RTN-024

**Case:** The running life keeps the **manifest** in the root of the disk: the ID, the **active members** of the table with the IPs, the table version, `last_seen` and the watermark
**Then:** it is written when the node becomes active and at every change of the table, `last_seen` moves every 10 s and on a clean stop, and the write is atomic (temporary file and swap). The watermark is the largest instant the ID generator used or was told to stay above ([07-cluster.md](07-cluster.md), section 10.9); it is empty until the node generates an ID.
**Level:** C
**Tests:**
  - `tests/recovery.rs::the_running_life_keeps_its_manifest_and_moves_last_seen_on_a_clean_stop`
  - `src/cluster/restore/tests.rs::the_manifest_is_written_atomically_and_read_back`
  - `src/cluster/restore/tests.rs::a_folder_without_a_readable_manifest_is_not_a_life`

---

### UC-RTN-025

**Case:** The **quarantine is not deleted** by today's recovery: the folder of each life stays until the restoration ends (vote, [07-cluster.md](07-cluster.md), section 10.8)
**Then:** the loss recovery returns what the list asks for, and the folder of the life keeps its topics and manifest, even when the `cluster.recovery.timeout` deadline runs out. Only the rows a primary confirmed in the restoration (UC-RTN-026) leave it.
**Level:** C
**Tests:**
  - `tests/recovery.rs::a_restarted_process_joins_without_waiting_and_recovers_its_old_disk_afterwards`
  - `tests/recovery.rs::the_recovery_of_a_restarted_process_ends_at_the_timeout_even_if_a_node_stays_mute`

---


## FWD: Forwarding of client calls

### UC-FWD-001

**Case:** A node that is not yet healthy (synchronizing) **forwards** the client call to another node instead of returning an error. It only applies to client routes; `/health/*`, `/metrics`, `/metrics/topics/*`, `/node/drain` and the node-to-node communication ones are not forwarded.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-004

**Case:** **Transparent** forwarding  
**Then:** method, path, query, body and headers go as they came, and the response comes back as the destination gave it (including `409`, `413`, `429`); it applies to all client routes, also the `GET` with long polling and `autoCreate`.  
**Level:** C  
**Tests:**
  - `src/cluster/protocol/tests.rs::a_forwarded_reply_comes_back_as_the_destination_gave_it`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-009

**Case:** Rate limit  
**Then:** applied once, on the node that received, per client IP (the node does not validate the token); the destination does not count again, and the cluster port has no rate limit. The already resolved client IP goes in an internal header.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-FWD-011

**Case:** Publishing to a FIFO queue  
**Then:** even with the node ready, it is forwarded to the **group's primary** (the destination is the owner, not the freest node), through `POST /cluster/topics/{topic}/fifo/publish`; the primary checks (`421` if it is not the owner) and repetition after the body is safe.  
**Level:** C  
**Tests:**
  - `tests/fifo_cluster.rs::a_publication_from_any_node_is_coordinated_by_the_primary_of_the_group`

---


## DRN: Stop and drain mode (includes drain as save)

### UC-DRN-001

**Case:** The states are `active` → `draining` → `drained` (`drainState`).  
**Level:** U  
**Tests:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-002

**Case:** `SIGTERM` or `SIGINT` with `drain.on-stop` on (default)  
**Then:** the node enters drain by itself and, when the drain ends, **exits with code 0**. A second signal during the drain exits at once, without finishing.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::the_signal_drain_waits_for_the_save_and_marks_the_trigger`
  - `src/cluster/drain/tests.rs::the_signal_waits_for_a_drain_the_api_already_started`

---

### UC-DRN-003

**Case:** With `drain.on-stop=false` the signal ends the process at once, like a crash.  
**Level:** C  
**Tests:**
  - `tests/messages.rs::without_drain_on_stop_what_was_only_in_memory_is_lost`

---

### UC-DRN-004

**Case:** `POST /node/drain`  
**Then:** `202` `DrainStatus` with `trigger: "api"`, **without exiting** the process; the node stays `drained`, out of the cluster and the rotation. Already in drain (`draining` or `drained`): `200` with no effect.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `src/cluster/drain/tests.rs::starting_a_drain_twice_changes_nothing`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-005

**Case:** `GET /node/drain` returns `DrainStatus` (`drainState`, `trigger` `signal` or `api` or `null`, `startedAt`, `pendingMessages`, which reaches 0 when `drained`).  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-006

**Case:** The drain routes apply only to the node that receives the call; an IP outside `operations.allowed-ips`  
**Then:** `403` `ops.ip-not-allowed`.  
**Level:** I  
**Tests:**
  - `tests/operations.rs::the_operations_routes_only_accept_the_allowed_ips`

---

### UC-DRN-007

**Case:** Step 1  
**Then:** `/health/ready` becomes `503` at once and `/health/live` stays `200`. The node keeps receiving client calls but **serves none**: forwards all to the freest node until the process exits. Long pollings in progress are answered empty; new ones, forwarded.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::a_long_poll_waiting_when_the_drain_starts_answers_empty`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-DRN-008

**Case:** Step 2  
**Then:** notifies the other nodes through the channel with the "in drain" state signal (idempotent): the peers stop asking it for `take` and stop choosing it as a load source; a `take` that still arrives is answered empty.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`
  - `tests/drain_cluster.rs::a_draining_node_answers_take_empty_and_the_peers_stop_asking`

---

### UC-DRN-009

**Case:** Step 3  
**Then:** the effective memory limit drops to **zero**: every new message, published or received as a replica, goes straight to disk (file and then the row). The peers **keep** sending registers and commits to the node until it leaves.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::while_it_drains_the_node_writes_every_new_replica_straight_to_disk`

---

### UC-DRN-010

**Case:** Step 4  
**Then:** a thread moves to disk the messages in memory **with the metadata** (`deliveryCount`, `invisibleUntil`, `receiptToken`), through each topic's write threads, in parallel across topics, in the overflow order (topics with the lowest read rate first). The ready and invisible lists are derived and rebuilt afterwards.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::the_api_drain_moves_memory_to_disk_and_ends_drained`
  - `src/engine/messages/tests.rs::drain_saves_everything_in_memory_to_disk`
  - `tests/drain_cluster.rs::while_it_drains_the_node_writes_every_new_replica_straight_to_disk`
  - `tests/messages.rs::stopping_saves_memory_to_disk_and_the_next_start_resumes_it`

---

### UC-DRN-011

**Case:** Step 5  
**Then:** with memory empty, the node coordinates its **own deregistration** through the two-phase protocol (`POST /cluster/nodes/unregister` with `reason: drain` and `commit`), with the same failure rule, **without a vote or a voting quorum**. The commit takes the node out of the table, recomputes the ring and rebalances the blocks; the new primaries take over its volatile queues.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-DRN-012

**Case:** Step 6  
**Then:** closes the channel with the peers (already with the node outside the table: nobody reconnects or opens a vote) and the state becomes `drained`. By signal, the process waits for the forwarded calls in progress (within `drain.timeout`) and exits; through the API, it keeps forwarding the calls it receives.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::the_signal_drain_waits_for_the_save_and_marks_the_trigger`
  - `tests/drain_cluster.rs::a_draining_node_forwards_client_calls_and_leaves_the_cluster_without_a_vote`

---

### UC-DRN-013

**Case:** The voluntary deregistration follows the one-change-at-a-time rule  
**Then:** if there is another join, leave or vote in progress, the request waits its turn, within `drain.timeout`. There is no freezing.  
**Level:** C  
**Tests:**
  - `tests/drain_cluster.rs::two_nodes_draining_at_once_leave_one_after_the_other`

---

### UC-DRN-014

**Case:** Deadline exceeded or failure  
**Then:** the `drain.timeout` covers the whole drain (steps 1 to 6); when exceeded, the node records what is left in memory and the step where it stopped (`error`) and exits anyway (if by signal), falling back to the usual detection and vote. An I/O error or full disk in step 4 does not interrupt: the node continues, counts the error and includes it in the result.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::an_expired_timeout_leaves_the_node_draining`

---

### UC-DRN-016

**Case:** Before leaving (`draining` until the start of step 5)  
**Then:** cancels the drain, the node goes back to `active`, to writing in memory, remains a member; `200` `DrainStatus`.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::canceling_while_draining_stops_the_move`
  - `tests/drain_cluster.rs::canceling_before_the_node_left_keeps_it_in_the_cluster`
  - `tests/operations.rs::deleting_the_drain_brings_the_node_back_with_its_messages`

---

### UC-DRN-017

**Case:** During deregistration (step 5 in progress)  
**Then:** `409` `drain.conflict`; try again when it is `drained`. With the process already stopping by signal: `409` `drain.conflict`.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::with_the_process_stopping_by_signal_cancel_is_a_conflict`
  - `tests/drain_cluster.rs::canceling_a_drain_that_is_leaving_is_a_conflict`

---

### UC-DRN-018

**Case:** After leaving (`drained`)  
**Then:** the node joins as a **new node**, with a **new ID**, full bootstrap and a new position on the ring; answers `202`, `drainState` goes back to `active`, and the node state goes through `joining` and `syncing`. Before clearing the local state it does the recovery of lost messages (UC-RTN-001 to UC-RTN-006). No freezing.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::leaving_the_drained_state_returns_as_a_new_node`
  - `tests/drain_cluster.rs::after_leaving_the_node_returns_as_a_new_node_with_a_new_id`
  - `tests/recovery.rs::a_node_that_left_by_drain_recovers_the_lost_messages_before_it_returns_as_a_new_node`

---

### UC-DRN-019

**Case:** `DELETE /node/drain` on `active`  
**Then:** `200`, nothing to cancel.  
**Level:** C  
**Tests:**
  - `src/cluster/drain/tests.rs::canceling_without_a_drain_has_nothing_to_cancel`
  - `tests/operations.rs::deleting_the_drain_brings_the_node_back_with_its_messages`

---

### UC-DRN-020

**Case:** Drain metrics and logs  
**Then:** `taba.node.drain.state`, `taba.node.drain.pending`; `info` log (started by signal or API, completed, canceled, returns as new) and `error` (`drain.timeout` exceeded with what was left, I/O error).  
**Level:** C  
**Tests:**
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---


## DEV: One-node profile (the former development mode)

### UC-DEV-002

**Case:** With `replication.factor=1` and no `cluster.entry-address` the node starts alone, with no bootstrap; the cluster port is opened and serves only `/cluster/*`, and the client port answers `404` to those routes.  
**Level:** I  
**Tests:**
  - `tests/listeners.rs::a_node_with_factor_one_starts_alone_and_keeps_the_cluster_routes_off_the_client_port`

---

### UC-DEV-011

**Case:** `GET /health/cluster` returns the same with `scope=cluster` and `scope=node`, in the format of a one-node cluster; Prometheus reads the node, with a single series.  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-DEV-012

**Case:** The topic definition is written to the node's own `_node.db`, with no bootstrap or topic load from another node, and it comes back after a restart (the restoration of the disk).  
**Level:** I  
**Tests:**
  - `tests/topics.rs::definitions_are_saved_in_the_node_database_and_resumed_after_a_restart`

---


## SEG: Ports, mutual TLS and cluster certificates

### UC-SEG-001

**Case:** The node serves on three separate listeners  
**Then:** client (`api.port`), cluster (`cluster.port`) and operations (`operations.port`, optional). The three ports must be different; repeated values or a port in use prevent the node from starting.  
**Level:** I  
**Tests:**
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`
  - `tests/startup.rs::an_occupied_api_port_stops_startup_naming_the_port`
  - `tests/startup.rs::an_occupied_cluster_port_stops_startup_in_production`

---

### UC-SEG-002

**Case:** The client port answers `404` to `/cluster/*`; the cluster port answers `404` to anything that is not `/cluster/*`.  
**Level:** I, C  
**Tests:**
  - `tests/cluster_api.rs::the_cluster_port_serves_only_cluster_routes_and_the_client_port_none`
  - `tests/listeners.rs::client_port_hides_cluster_routes_and_cluster_port_serves_only_cluster_routes`

---

### UC-SEG-003

**Case:** The operations routes live only on `operations.port` (always open, default 9090); on `api.port` only `/health/live` and `/health/ready` (no token) and `/metrics` (any valid token) exist, and the rest answers `404`.  
**Level:** I  
**Tests:**
  - `tests/listeners.rs::the_operations_routes_live_on_their_own_port_and_the_client_port_only_has_the_probes`

---

### UC-SEG-004

**Case:** The cluster port has no rate limit nor client IP reading  
**Then:** the `client-ip.headers` headers are ignored on it.  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-SEG-006

**Case:** The operations routes have no authentication; protection is by network (own port and `operations.allowed-ips`).  
**Level:** I  
**Tests:**
  - `tests/mtls.rs::the_operations_routes_have_no_authentication`

---

### UC-SEG-007

**Case:** With the certificate folder (`cluster.tls.dir`, default `<secrets.dir>/tls`) existing, mutual TLS turns on **by itself** and the cluster port starts to require TLS; without the folder (default does not exist and `cluster.tls.dir` not defined), it serves plain HTTP with a warning (`warn`) in the log.  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::three_real_nodes_form_a_cluster_over_mutual_tls_and_replicate`
  - `tests/mtls.rs::without_the_certificates_folder_the_cluster_port_is_plain_http`
  - `tests/secrets.rs::a_node_mounting_the_folder_from_init_turns_authentication_on_by_itself`

---

### UC-SEG-008

**Case:** Existing folder without at least one valid pair (`.crt` + `.key`)  
**Then:** the node **does not start**. Defined `cluster.tls.dir` that does not exist: the node does not start.  
**Level:** I  
**Tests:**
  - `src/cluster/tls/tests.rs::a_folder_without_a_valid_pair_does_not_open`
  - `tests/mtls.rs::a_folder_whose_pair_is_not_valid_stops_the_node_from_starting`
  - `tests/mtls.rs::a_certificates_folder_that_does_not_exist_is_a_start_error`

---

### UC-SEG-009

**Case:** The node **accepts** whoever presents a certificate equal to one in the folder, or issued by one of them, as long as it has not expired (`notAfter`); the start date does not count for trust.  
**Level:** U, C  
**Tests:**
  - `src/cluster/tls/tests.rs::it_trusts_an_equal_certificate_and_ignores_the_start_date`
  - `src/cluster/tls/tests.rs::an_expired_certificate_is_neither_trusted_nor_presented`
  - `tests/mtls.rs::the_handshake_refuses_who_does_not_present_a_certificate_the_node_accepts`
  - `tests/mtls.rs::an_expired_certificate_is_not_accepted_and_is_not_presented`

---

### UC-SEG-010

**Case:** The node **presents** the pair with the most recent start that is already valid (`notBefore` ≤ now) and not yet expired, when receiving and when opening a connection (HTTP and WebSocket `/cluster/channel`).  
**Level:** U, C  
**Tests:**
  - `src/cluster/tls/tests.rs::it_presents_the_newest_pair_that_has_already_started`
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-011

**Case:** Each side requires the other to present an accepted certificate; without it the handshake fails **before** any HTTP response and no route is reached. The host name is not verified (the nodes connect by IP).  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::three_real_nodes_form_a_cluster_over_mutual_tls_and_replicate`
  - `tests/mtls.rs::the_handshake_refuses_who_does_not_present_a_certificate_the_node_accepts`

---

### UC-SEG-012

**Case:** A `.crt` without a `.key` only serves to trust; a pair is `<name>.crt` plus `<name>.key` of the same name. The folder can have several pairs at the same time.  
**Level:** U  
**Tests:**
  - `src/cluster/tls/tests.rs::a_crt_without_key_only_serves_to_trust_and_a_key_without_crt_is_incomplete`

---

### UC-SEG-013

**Case:** The folder is reloaded by itself every `cluster.tls.reload-interval` (default 60 s), without restarting. Reload with an unreadable file, incomplete pair or invalid certificate  
**Then:** the node **keeps the previous set** and logs `error`; the set only changes when the whole folder is valid.  
**Level:** C  
**Tests:**
  - `src/cluster/tls/tests.rs::a_reload_with_an_invalid_folder_keeps_the_previous_set_and_a_valid_one_replaces_it`
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-014

**Case:** Rotation without restarting  
**Then:** (1) generate the certificate with a start in the future (`--starts-in`), (2) add it to the folder: each node notices it within `reload-interval` and starts to **trust** it, but keeps presenting the old one, (3) when the start date arrives, all start presenting the new one by themselves, (4) the old one stops being accepted when it expires.  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-015

**Case:** Connections already open (like the WebSocket channel) continue with the old certificate until they drop, which is harmless because it remains accepted until it expires.  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::rotation_needs_no_restart_the_new_pair_is_trusted_first_and_presented_when_it_starts`

---

### UC-SEG-016

**Case:** An expired certificate stops being accepted and presented. From `cluster.tls.expiry-warning` (30 days) before the `notAfter` of the certificate in use, the node logs `warn` and exposes `taba.cluster.tls.expires_in`.  
**Level:** C  
**Tests:**
  - `src/cluster/tls/tests.rs::an_expired_certificate_is_neither_trusted_nor_presented`
  - `tests/mtls.rs::an_expired_certificate_is_not_accepted_and_is_not_presented`

---

### UC-SEG-017

**Case:** A CA in the folder (a CA `.crt` to trust plus the node's own pair to present)  
**Then:** the certificates issued by it are accepted.  
**Level:** C  
**Tests:**
  - `src/cluster/tls/tests.rs::a_ca_in_the_folder_makes_the_certificates_it_issued_accepted`
  - `tests/mtls.rs::a_ca_in_the_folder_makes_the_certificates_it_issued_accepted`

---

### UC-SEG-018

**Case:** A caller that is not in the member table receives `403` `cluster.not-member` on any `/cluster/*` route, **except** those that accept whoever is not yet a member (`GET /cluster/nodes`, `POST /cluster/nodes/join`, `GET /cluster/message-loss`, `POST /cluster/messages/recover`); the forwarding also accepts the nodes that left by drain.  
**Level:** C  
**Tests:**
  - `tests/cluster.rs::only_members_call_the_phase_routes_but_anyone_lists_and_asks_to_join`
  - `tests/cluster_api.rs::a_caller_that_is_not_a_member_is_refused_on_the_phase_routes`
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-SEG-019

**Case:** `taba init --out <folder>` creates at once the first certificate in `tls/` and the root token in `root-token` (minimum 32 characters).  
**Level:** I  
**Tests:**
  - `src/cli/tests.rs::init_needs_the_out_folder_and_defaults_to_five_years`
  - `src/secrets/tests.rs::init_creates_the_certificate_in_tls_and_a_root_token_of_at_least_32_characters`
  - `tests/secrets.rs::init_creates_the_certificate_and_the_root_token_and_refuses_to_run_twice`

---

### UC-SEG-020

**Case:** `taba cert generate --days <n> [--starts-in <duration>] --out <folder>` creates a pair `cluster-<start date>.crt` and `.key` (successive generations do not overwrite each other), self-signed and valid for server and client; `--starts-in` sets the `notBefore` in the future (default  
**Then:** now, with a margin of a few minutes).  
**Level:** I  
**Tests:**
  - `src/cli/tests.rs::cert_generate_takes_days_out_and_an_optional_start_delay`
  - `src/secrets/tests.rs::a_certificate_is_self_signed_and_valid_for_server_and_client`
  - `src/secrets/tests.rs::the_name_carries_the_start_date_and_the_key_is_a_pem_private_key`
  - `src/secrets/tests.rs::without_starts_in_it_starts_a_few_minutes_ago`
  - `src/secrets/tests.rs::nothing_is_overwritten_and_no_half_pair_is_left`
  - `src/secrets/tests.rs::init_never_overwrites_an_existing_root_token`
  - `tests/secrets.rs::cert_generate_adds_a_pair_named_after_its_start_date`

---

### UC-SEG-021

**Case:** `taba token generate` prints a standalone random 256-bit secret.  
**Level:** I  
**Tests:**
  - `src/cli/tests.rs::token_generate_takes_nothing_else`
  - `tests/secrets.rs::token_generate_prints_a_fresh_secret_each_time`

---


## OPS: Observability (OpenTelemetry), statistics, metrics and health

### UC-OPS-001

**Case:** `GET /health/live`  
**Then:** `200` `HealthResponse` with `health: "live"` while the process answers.  
**Level:** I  
**Tests:**
  - `tests/listeners.rs::a_single_node_is_live_and_ready`

---

### UC-OPS-002

**Case:** `GET /health/ready`  
**Then:** `200` with `health: "ready"` when the node is registered (commit confirmed) and with **all synchronization finished**; `503` with `health: "not-ready"` and `reason` (`bootstrapping`, `syncing`, `returning`, `draining`, `drained` or `failed`) otherwise.  
**Level:** C  
**Tests:**
  - `src/cluster/state/tests.rs::a_failed_node_is_not_ready`
  - `tests/listeners.rs::a_single_node_is_live_and_ready`
  - `tests/listeners.rs::production_node_without_cluster_protocol_is_live_but_not_ready`
  - `tests/operations.rs::a_corrupt_topic_database_fails_the_node`

---

### UC-OPS-003

**Case:** `/health/live` and `/health/ready` do **not** return `ErrorResponse`  
**Then:** the body is always `HealthResponse`. These two routes do not query other nodes.  
**Level:** I  
**Tests:**
  - `tests/listeners.rs::production_node_without_cluster_protocol_is_live_but_not_ready`

---

### UC-OPS-004

**Case:** In drain (`draining` or `drained`) `/health/ready` answers `503` and `/health/live` stays `200`.  
**Level:** C  
**Tests:**
  - `tests/operations.rs::the_api_drain_takes_the_node_out_of_rotation_and_saves_the_memory`

---

### UC-OPS-005

**Case:** `/health/live`, `/health/ready`, `/metrics` and `/health/cluster?scope=node` never answer `429`.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::the_json_routes_count_in_the_ops_category_and_metrics_does_not`

---

### UC-OPS-006

**Case:** `GET /metrics` returns Prometheus text (`text/plain; version=0.0.4`; OpenMetrics with `Accept: application/openmetrics-text`) **only with this node's metrics** and accepts no parameters; the node does not query other nodes to answer.  
**Level:** I  
**Tests:**
  - `src/telemetry/prometheus/tests.rs::open_metrics_drops_total_from_the_family_and_ends_with_eof`
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-OPS-007

**Case:** `/metrics` with `telemetry.prometheus.enabled` off  
**Then:** `404` `route.not-found`; IP outside `operations.allowed-ips`: `403` `ops.ip-not-allowed`.  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::metrics_is_404_when_off_and_403_for_an_ip_outside_the_list`

---

### UC-OPS-011

**Case:** OTel → Prometheus name mapping  
**Then:** dots become `_`, the unit becomes a suffix (`_seconds`, `_bytes`; `_ratio` only on unit `1`, for real ratios), units annotated in braces (`{message}`, `{flag}`) generate no suffix, counters gain `_total`, histograms become `_bucket`/`_sum`/`_count`, attributes become labels; `service.namespace/service.name` becomes `job`, `service.instance.id` becomes `instance`, and `target_info` carries the other resource attributes.  
**Level:** U  
**Tests:**
  - `src/telemetry/prometheus/tests.rs::names_map_from_opentelemetry_to_prometheus`
  - `src/telemetry/prometheus/tests.rs::counters_get_total_and_the_unit_is_not_repeated`
  - `src/telemetry/prometheus/tests.rs::histograms_become_bucket_sum_and_count_with_attributes_as_labels`
  - `src/telemetry/prometheus/tests.rs::the_resource_becomes_target_info`

---

### UC-OPS-012

**Case:** Node metrics  
**Then:** bounded cardinality; they **never** carry tags or the topic name; `http.route` is always the **template** (`/topics/{topic}/messages`), never the real path; the message ID is never a label.  
**Level:** U, I  
**Tests:**
  - `src/telemetry/metrics/tests.rs::counters_add_up_per_series`
  - `tests/metrics.rs::requests_are_measured_by_route_template_never_by_topic_or_path`

---

### UC-OPS-013

**Case:** OTLP and `/metrics` are fed by the same instruments  
**Then:** the metrics are the same on both outputs; OTLP *pushes* the node's own metrics, with no fan-out and no target list.  
**Level:** I  
**Tests:**
  - `src/telemetry/otlp/metrics/tests.rs::each_instrument_kind_becomes_the_matching_otlp_aggregation`
  - `src/telemetry/otlp/metrics/tests.rs::the_request_carries_the_resource_the_scope_and_skips_families_without_series`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`

---

### UC-OPS-014

**Case:** Topic metrics exported by `telemetry.topics.mode`  
**Then:** `off` (default: none), `by-tags` (aggregated by the tags of `telemetry.topics.group-by-tags`, in the attribute `taba.topic.tag.<key>`) and `per-topic` (one series per topic, in `messaging.destination.name`, up to `telemetry.topics.max-topics`, the excess aggregated in `messaging.destination.name="_other"`). `taba.topic.oldest_timestamp` has no series on a node without messages of the topic.  
**Level:** U, I  
**Tests:**
  - `tests/metrics.rs::topic_metrics_are_off_by_default`
  - `tests/metrics.rs::per_topic_mode_has_one_series_per_topic_and_no_oldest_without_messages`
  - `tests/metrics.rs::per_topic_mode_folds_the_topics_above_the_ceiling_into_other`
  - `tests/metrics.rs::by_tags_mode_sums_by_tag_value_and_never_names_a_topic`

---

### UC-OPS-016

**Case:** Process and host metrics, with the standard names  
**Then:** `process.uptime`, `process.cpu.utilization`, `process.memory.usage`, `process.thread.count` (OS threads, without tokio tasks), `process.open_file_descriptor.count`, and `taba.node.connections`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::publish_errors_are_counted_per_topic_and_not_for_missing_topics`
  - `src/telemetry/process/tests.rs::status_gives_the_resident_memory_and_the_threads`
  - `src/telemetry/process/tests.rs::stat_cpu_counts_from_the_last_parenthesis`
  - `src/telemetry/process/tests.rs::utilization_is_measured_between_two_readings`
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`
  - `tests/metrics.rs::on_linux_the_process_metrics_have_memory_threads_and_files`

---

### UC-OPS-017

**Case:** `GET /health/cluster?scope=node`  
**Then:** local read, `200` `ClusterStatus` with a single item in `nodes`, whose `status` is the `NodeStatus` (`nodeId`, `version`, `state`, `drainState`, `startedAt`, `collectedAt`, `storage`, `signals`), **without** the metrics.  
**Level:** I  
**Tests:**
  - `tests/operations.rs::cluster_health_describes_the_single_node`

---

### UC-OPS-018

**Case:** `GET /health/cluster` (`scope=cluster`, the default)  
**Then:** queries all the nodes (`GET /cluster/status`) and returns `ClusterStatus` (`scope`, `collectedAt`, `partial`, `worstCondition`, `conditions`, `summary` with `members` by state, `membersVersion`, `quorumAvailable`, `syncingBlocks`, and `nodes`). A node that does not answer appears with `reachable: false` and `status: null`, without failing the call; unknown `scope`: `400`.  
**Level:** C  
**Tests:**
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`
  - `tests/conditions.rs::a_node_that_does_not_answer_is_unreachable_and_the_blocks_it_leads_are_at_risk`
  - `tests/metrics.rs::free_disk_space_is_a_metric_and_below_the_minimum_is_storage_pressure`
  - `tests/operations.rs::cluster_health_describes_the_single_node`

---

### UC-OPS-019

**Case:** `GET /metrics/topics`  
**Then:** `Page<TopicStats>` paginated **always by name**, with `scope` (`cluster` or `node`), `tag.<key>`, `condition`, `fields` (`name` and `id` always come) and `groupBy` (one row per tag value, `TopicStatsGroup`, with the quantities added and the dates by the smallest value).  
**Level:** I, C  
**Tests:**
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`
  - `tests/topic_stats.rs::pages_by_name_and_validates_the_parameters`
  - `tests/topic_stats.rs::group_by_a_tag_gives_one_row_per_value_with_the_sums`
  - `tests/topic_stats.rs::the_json_routes_count_in_the_ops_category_and_metrics_does_not`

---

### UC-OPS-021

**Case:** `GET /metrics/topics/{topic}`  
**Then:** `TopicStatsDetail` (`TopicStats` plus `topGroups`, up to `stats.topics.top-groups`); `404` `topic.not-found`; on a distributor it returns only `distributor`.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::the_detail_adds_the_largest_groups_and_404s_for_unknown_topics`

---

### UC-OPS-022

**Case:** `GET /metrics/topics/{topic}/groups`  
**Then:** `Page<GroupStats>` by `groupId` (UC-GRP-015); `top` and `cursor` together: `400` `validation.failed`.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::the_groups_route_lists_pages_filters_and_tops_the_groups`

---

### UC-OPS-023

**Case:** Node view × cluster view in `/stats/topics` (`scope=node` or the default, the cluster)  
**Then:** **only the primary counts** (otherwise the sum would be multiplied by the replication factor); the cluster adds `visibleMessages`, `invisibleMessages` and `depth`, and `oldestMessageAt` is the **earliest** date among the nodes; `oldestAge = now − oldestMessageAt`.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/blocks.rs::only_the_owner_counts_a_message_in_the_visible_invisible_and_oldest_numbers`
  - `src/http/topic_stats/raw/tests.rs::the_cluster_view_adds_what_each_primary_counts_and_takes_the_oldest_date`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-OPS-024

**Case:** A node without messages of the topic has `oldestMessageAt` **absent**, never zero (a zero would win the "oldest").  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::the_counters_follow_each_transition_of_a_message`
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`

---

### UC-OPS-026

**Case:** Each node keeps local counters per topic, updated on the **transitions** (publishing +1 visible; delivery −1 visible +1 invisible; expiry −1 invisible +1 visible; ack −1 in the state it was in), without scanning messages at each query.  
**Level:** U  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::the_counters_follow_each_transition_of_a_message`

---

### UC-OPS-028

**Case:** Topic conditions  
**Then:** `idle` (no publishing or consumption for `stats.topics.idle-after`), `backlog` (`depth` or `oldestAge` above the limit), `stalled` (there are messages but no consumption for `stats.topics.stalled-after`), `noisy-group`, `group-stalled` (FIFO only), `memory-pressure` (most on disk); `worstCondition` is the one of highest severity.  
**Level:** U, I  
**Tests:**
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

**Case:** The dead-letter appears in `/metrics/topics` and in `/topics`, with `deadLetter: true`, `source` and the source queue's `tags`.  
**Level:** I  
**Tests:**
  - `tests/topic_stats.rs::lists_the_topics_by_name_with_their_numbers_and_the_dead_letter_too`

---

### UC-OPS-031

**Case:** Traces  
**Then:** one per client operation (publishing, consumption, ack), per synchronized block (one span per batch, with the source node), per node register and deregister and vote (one span per node queried); attributes: topic tags, message ID, block, nodes, attempt number. The message ID goes in trace and log, never in a metric.  
**Level:** C  
**Tests:**
  - `tests/otlp.rs::a_cluster_write_is_one_trace_with_a_register_and_a_commit_span_per_node`
  - `tests/otlp.rs::a_synced_block_is_one_trace_with_a_span_per_batch_naming_the_source_node`
  - `tests/otlp.rs::an_exclusion_vote_is_one_trace_with_a_span_per_voter`
  - `tests/otlp.rs::the_trace_context_travels_to_the_other_nodes_of_a_real_cluster`

---

### UC-OPS-032

**Case:** Structured logs (JSON or OTLP) with `trace_id` and `span_id` and the topic tags; the level is `telemetry.logs.level`, with the OpenTelemetry severity; per-message logs on the hot path stay at `debug`.  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::the_log_format_defaults_to_text_and_comes_from_yaml_or_env`
  - `tests/otlp.rs::logs_carry_trace_id_span_id_severity_and_the_event_name`
  - `tests/otlp.rs::the_otlp_log_level_follows_telemetry_logs_level`

---

### UC-OPS-033

**Case:** Point-in-time events (not health conditions), as *log records* with `event.name`  
**Then:** `taba.distributor.no_target`, `taba.distributor.partial`, `taba.volatile.deleted`, `taba.volatile.check_blocked`, `taba.message.dead_lettered`, `taba.message.deduplicated`, `taba.message.expired` and `taba.message.lost`; the topic goes in `messaging.destination.name`.  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::a_dead_lettered_message_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::an_expired_message_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::a_deduplicated_publication_raises_the_event_and_the_counters`
  - `src/engine/messages/tests/stats_events.rs::a_distributor_raises_no_target_and_partial_and_counts_its_copies`

---

### UC-OPS-034

**Case:** Overflow to disk  
**Then:** `info` log with a **periodic summary**, not per message; I/O error: `error`.  
**Level:** I  
**Tests:**
  - `src/engine/messages/tests/stats_events.rs::the_overflow_goes_to_the_log_as_one_periodic_summary_and_not_per_message`

---

### UC-OPS-035

**Case:** Access to the operations routes  
**Then:** the operations port answers whoever reaches it; with `operations.allowed-ips`, only the IPs/CIDR in the list access (the others: `403`), while `api.port` and its probes stay open.  
**Level:** I  
**Tests:**
  - `tests/operations.rs::the_operations_routes_only_accept_the_allowed_ips`

---

### UC-OPS-036

**Case:** The node does not start if `operations.port` is in use or if `operations.allowed-ips` has an invalid entry.  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::allowed_ips_must_be_valid_addresses_or_cidr`
  - `tests/startup.rs::an_occupied_operations_port_stops_startup`

---

### UC-OPS-038

**Case:** Message loss generates no metric or condition (only log and the `losses` table)  
**Then:** see UC-SIN-027 and UC-SIN-028.  
**Level:** C  
**Tests:**
  - `tests/sync.rs::a_message_that_is_listed_but_has_no_content_anywhere_is_a_loss_and_the_sync_goes_on`

---

### UC-OPS-039

**Case:** OTLP export is configured by the SDK's standard variables, without a `TABA_` prefix  
**Then:** without `OTEL_EXPORTER_OTLP_ENDPOINT` OTLP stays off; with it, the node sends metrics, traces and logs; `OTEL_EXPORTER_OTLP_PROTOCOL` chooses `grpc` (default) or `http/protobuf`; `OTEL_EXPORTER_OTLP_HEADERS` and the certificate apply.  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::without_otel_exporter_otlp_endpoint_the_otlp_is_off`
  - `src/config/tests.rs::the_otlp_protocol_defaults_to_grpc_and_accepts_http_protobuf`
  - `src/config/tests.rs::an_invalid_otlp_setting_stops_the_start_with_every_problem_listed`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`
  - `tests/otlp.rs::otlp_defaults_to_grpc_and_sends_the_three_signals`
  - `tests/otlp.rs::otlp_over_grpc_trusts_the_certificate_in_otel_exporter_otlp_certificate`

---

### UC-OPS-040

**Case:** Resource attributes on all telemetry  
**Then:** `service.name=taba`, `service.namespace` and `deployment.environment.name` coming from `OTEL_RESOURCE_ATTRIBUTES`, `service.instance.id` equal to the node ID (new at each start) and `service.version`.  
**Level:** U, I  
**Tests:**
  - `src/config/tests.rs::the_otel_variables_give_the_resource_attributes_and_the_service_name_wins`
  - `src/telemetry/otlp/metrics/tests.rs::the_request_carries_the_resource_the_scope_and_skips_families_without_series`
  - `src/telemetry/prometheus/tests.rs::the_resource_becomes_target_info`
  - `tests/metrics.rs::the_resource_attributes_come_from_the_otel_variables_and_the_node`
  - `tests/otlp.rs::otlp_over_http_protobuf_sends_metrics_traces_and_logs_with_the_headers`

---

### UC-OPS-041

**Case:** Names according to OpenTelemetry  
**Then:** no `msg.*` metric or attribute; HTTP in `http.server.request.duration`, `http.server.active_requests` and the body sizes (without a separate request counter), with `http.request.method`, `http.route`, `http.response.status_code`, `error.type`, `url.scheme` and `server.port` (which distinguishes the three ports); node-to-node calls in `http.client.request.duration`; unit with no suffix in the name, and `1` only on ratios.  
**Level:** U, I  
**Tests:**
  - `src/telemetry/metrics/tests.rs::counters_add_up_per_series`
  - `src/telemetry/metrics/tests.rs::names_follow_the_opentelemetry_vocabulary`
  - `src/telemetry/prometheus/tests.rs::histograms_become_bucket_sum_and_count_with_attributes_as_labels`
  - `tests/metrics.rs::requests_are_measured_by_route_template_never_by_topic_or_path`

---

### UC-OPS-043

**Case:** Messaging spans carry `messaging.system=taba`, `messaging.destination.name`, `messaging.operation.type` and `messaging.message.id`; the topic tags go as `taba.topic.tag.<key>`; failure and durability-event spans carry `taba.durability_event=true`, so the Collector keeps them in tail sampling.  
**Level:** C  
**Tests:**
  - `tests/otlp.rs::messaging_spans_carry_the_semantic_attributes_and_the_topic_tags`
  - `tests/otlp.rs::a_failed_operation_span_carries_the_durability_event_mark`
  - `tests/otlp.rs::a_cluster_write_is_one_trace_with_a_register_and_a_commit_span_per_node`

---

### UC-OPS-044

**Case:** The SDK sampling follows `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` (default `parentbased_traceidratio`); a received `traceparent` with the decision to sample is respected.  
**Level:** I  
**Tests:**
  - `src/config/tests.rs::the_sampler_follows_otel_traces_sampler_and_defaults_to_parentbased_traceidratio`
  - `tests/otlp.rs::a_sampled_traceparent_is_respected_and_the_ratio_sampler_drops_the_rest`
  - `tests/otlp.rs::always_off_exports_no_spans`

---

### UC-OPS-045

**Case:** The node's `GET /metrics` and the OTLP work on a single node (one-node profile).  
**Level:** I  
**Tests:**
  - `tests/metrics.rs::metrics_is_the_prometheus_text_of_this_node_only`

---

### UC-OPS-046

**Case:** `GET /cluster/status` and `/nodes/{nodeId}/metrics`, `/drain`, `/backup`, `/stats/topics` with a token that is not the root, and with an unknown `nodeId`  
**Then:** `403 auth.forbidden` with `required: "root"`; with the root token, the cluster status, the node's Prometheus text and the drain status; an unknown `nodeId` is `404 node.not-found`.  
**Level:** I  
**Tests:**
  - `tests/nodes_api.rs::the_cluster_and_node_routes_need_the_admin_permission_and_an_unknown_node_is_404`

---

### UC-OPS-047

**Case:** `/nodes/{nodeId}/…` received by a node that is, or is not, the `nodeId`  
**Then:** The node that is it answers from its own data and nothing crosses the cluster port; otherwise it calls the target **once** (`node-call`) and returns the answer as it came; starting the drain of another node through any node drains that one; a drained node stays in `GET /nodes` as `drained` and stays addressable; an unknown `nodeId` calls no one.  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::a_node_answers_for_itself_or_calls_the_node_that_has_the_data_once`

---

### UC-OPS-048

**Case:** `/nodes/{nodeId}/…` for a node that does not answer  
**Then:** `502 forward.bad-gateway` naming the node, after a single attempt (no retry).  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::a_node_that_does_not_answer_is_a_failure_that_names_it_with_no_retry`

---

### UC-OPS-049

**Case:** `GET /stats/topics`, `/stats/topics/{topic}` and `/stats/topics/{topic}/groups` through any node of a cluster  
**Then:** The cluster's numbers, complete (12 messages published on three nodes show 12 through every node), with `partial: false` and `missingNodes: []`; `?scope=node` is that node alone and the three nodes add up to the cluster; each group shows once with the messages of every node.  
**Level:** I, C  
**Tests:**
  - `tests/nodes_api.rs::the_statistics_are_the_clusters_not_one_nodes`

---

### UC-OPS-050

**Case:** A node does not answer a consolidated read (`/stats/topics…`, `/nodes`, `/cluster/status`)  
**Then:** `200` with `partial: true` and `missingNodes: [{ nodeId, reason }]` (`timeout` or `unreachable`) naming that node; the numbers are those of the others; when the node returns, `partial` is `false` and the numbers are complete.  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::a_node_that_does_not_answer_makes_the_consolidated_reads_partial_and_says_which`

---

### UC-OPS-051

**Case:** `GET /cluster/status` and `GET /cluster/backup` with the root token  
**Then:** Each node's `NodeStatus` carries `runtime` (`activeRequests`, `rateLimitRejected`, `channelRttP95Seconds`) beside memory and disk; `/cluster/backup` lists the S3 backup state of every node with `nodeId`; without a backup, `422 backup.not-configured`.  
**Level:** C  
**Tests:**
  - `tests/nodes_api.rs::the_cluster_status_carries_the_load_of_each_node_and_the_backup_is_consolidated`
  - `tests/nodes_api.rs::without_a_backup_the_cluster_backup_route_says_so`

---

### UC-OPS-052

**Case:** `GET /health/cluster` with the root token  
**Then:** each node's `NodeStatus` carries `excludedSince` (since when it records the log of excluded messages) and `summary` carries `excludedCutoff` (the cluster's cut-off date, the most recent among the active ones), so the operator knows how far the restart trusts the log.  
**Level:** C  
**Tests:**
  - `tests/excluded_log.rs::the_cluster_cut_off_is_the_latest_start_among_the_active_members`

---

### UC-OPS-053

**Case:** The restoration of the previous lives has not ended
**Then:** `GET /health/cluster` shows the `previous-life-pending` condition (warning) until the vote closes or the operator forces; the log has the warning of a restoration with absent nodes.
**Level:** C
**Tests:**
  - `src/cluster/health/tests.rs::a_restoration_in_progress_raises_previous_life_pending_as_a_warning`
  - `tests/restore_vote.rs::the_cluster_waits_for_the_expected_nodes_then_the_vote_restores_it_and_releases_the_fifo`
  - `tests/restore_vote.rs::in_strict_mode_the_restoration_stays_held_after_g_until_the_operator_forces_it`

---


## CLR: Contract of the cluster routes (DTOs and common rules)

### UC-CLR-002

**Case:** Every `register`, `commit` and `rollback` carries the `operationId` (GUID v7 of the coordinator) and is idempotent by it. A register with no commit or rollback expires after `write.pending.ttl`.  
**Level:** U, C  
**Tests:**
  - `src/cluster/members/tests.rs::a_registration_without_commit_or_rollback_expires_and_each_phase_repeats_by_its_operation_id`
  - `src/cluster/protocol/dto_tests.rs::every_phase_of_every_two_phase_call_carries_the_operation_id`
**Note:** Only the unit part (U) is proven; the full level is U, C. Expiry by `write.pending.ttl` is proven only for node registrations; it is missing for message operations.

---

### UC-CLR-004

**Case:** Invalid body on the cluster routes  
**Then:** `400` `validation.failed`, `validation.required` or `validation.unknown-field`.  
**Level:** U, C  
**Tests:**
  - `src/cluster/protocol/dto_tests.rs::an_invalid_body_on_a_cluster_route_is_a_400_with_a_validation_code`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-CLR-005

**Case:** The cluster DTOs (`Member`, `MemberTable`, `JoinRequest`, `NodeRegisterRequest`, `NodeUnregisterRequest`, `OperationRef`, `VoteRequest`, `VoteResponse`, `DefinitionProposal`, `DefinitionApply`, `TopicDefinition`, `TokenRecord`, `MessageRegisterRequest`, `VisibilityRegisterRequest`, `AckRegisterRequest`, `TakeRequest`, `TakeResponse`, `MessageRecord`, `IdleResponse`, `LossRecord`, `LossAck`, `RecoverResult`, `StreamEnd`) serialize and deserialize with the documented fields and types, and the `null` values of `ttl`, `maxDeliveries` and `maxMessageBytes` of the `TopicDefinition` are sent as `null`.  
**Level:** U  
**Tests:**
  - `src/cluster/protocol/dto_tests.rs::the_cluster_dtos_have_the_documented_fields_and_types`

---

### UC-CLR-006

**Case:** The `TopicDefinition` DTO equals the JSON stored in the `topics` table  
**Then:** the `Topic` fields without `effective`, `deadLetter` and `source`.  
**Level:** U  
**Tests:**
  - `src/engine/topics/tests.rs::the_cluster_definition_is_the_topic_json_without_effective_dead_letter_and_source`

---

### UC-CLR-010

**Case:** Cluster-only codes  
**Then:** `cluster.not-member`, `cluster.node-frozen`, `cluster.join-unreachable`, `cluster.conflict`, `cluster.wrong-node`, `fifo.misdirected` and `cluster.operation-unknown`.  
**Level:** U, C  
**Tests:**
  - `src/core/error/tests.rs::the_codes_only_the_cluster_uses_are_in_the_catalog_with_their_statuses`
**Note:** Only the unit part (U) is proven; the full level is U, C.

---

### UC-OPS-054

**Case:** The probe of the old IPs ended with nodes missing
**Then:** `GET /health/cluster` shows the `members-missing` condition (warning) while the table has fewer active members than the previous life's.
**Level:** C
**Tests:**
  - `src/cluster/health/tests.rs::a_probe_that_ended_with_nodes_missing_raises_members_missing_as_a_warning`
  - `tests/regroup.rs::when_the_probe_time_ends_with_nodes_missing_the_members_missing_condition_shows`

---


### UC-OPS-055

**Case:** The node runs the one-node profile with something relaxed (factor 1, TLS off or `auth.dev-token`)
**Then:** `GET /health/cluster` shows the `reduced-profile` condition (warning, informative); `health.single-node-condition=false` hides it.
**Level:** U
**Tests:**
  - `src/cluster/health/tests.rs::the_reduced_profile_is_an_informative_warning`

---
### UC-CLR-001

**Case:** Every `/cluster/*` route only accepts whoever is in the member table (`403` `cluster.not-member`), except the four exceptions (UC-SEG-018).  
**Level:** C  
**Tests:**
  - `tests/mtls.rs::only_members_use_the_cluster_routes_and_the_caller_is_the_connection_ip_not_a_header`

---

### UC-CLR-007

**Case:** `GET /cluster/topics/{id}/idle` answers `IdleResponse` (`topicId`, `idleSeconds`, `sinceStart`); unknown topic  
**Then:** `404` `topic.not-found`.  
**Level:** C  
**Tests:**
  - `tests/volatile_cluster.rs::the_idle_route_answers_a_duration_and_says_when_it_counts_from_the_start`

---

### UC-CLR-009

**Case:** `GET /cluster/status` returns the `NodeStatus`; there is no `GET /cluster/metrics` (each node exposes its own metrics).  
**Level:** C  
**Tests:**
  - `tests/conditions.rs::every_node_answers_its_status_and_a_healthy_cluster_reports_ok`

---


## CFG: Configuration and startup validations

### UC-CFG-001

**Case:** Every parameter can be set by an environment variable with the `TABA_` prefix and the key in uppercase with `.` and `-` replaced by `_` (`topics.distributor.max-targets` → `TABA_TOPICS_DISTRIBUTOR_MAX_TARGETS`).  
**Level:** U  
**Tests:**
  - `src/config/params/tests.rs::env_name_follows_the_documented_rule`

---

### UC-CFG-002

**Case:** The documented default values apply when the parameter is not defined (for example `replication.factor` 3, `write.quorum` 2 at rf 3, `delivery.visibility-timeout.default` 120, `delivery.max-messages.default` 10, `retention.ttl.default` 14 days, `retention.max-deliveries.default` 10, `fifo.dedup.window` 300 s, `cluster.vote.quorum-percent` 51, `cluster.vote.min-voters` 2).  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::documented_defaults_apply_when_nothing_is_set`
  - `src/config/tests.rs::quorum_defaults_to_the_majority_of_the_replication_factor`

---

### UC-CFG-003

**Case:** **No limit** means that the ceiling does not exist while the variable is not defined (`topics.distributor.max-targets`, `topics.volatile.max-count`).  
**Level:** U, I  
**Tests:**
  - `src/config/tests.rs::without_limit_means_the_ceiling_does_not_exist_until_set`

---

### UC-CFG-004

**Case:** The node does not start, with an explicit message, in each of these cases  
**Then:** repeated or occupied ports; `cluster.entry-address` absent in production; `auth.enabled=true` without a root token, or a short root token; a defined `auth.root-token-file` or `cluster.tls.dir` that does not exist; certificate folder without a valid pair; `operations.port` in use or invalid `operations.allowed-ips`; `write.quorum` outside 1 to `replication.factor` (refused in validation); `storage.dir` that cannot be created or written.  
**Level:** I  
**Tests:**
  - `tests/startup.rs::an_occupied_api_port_stops_startup_naming_the_port`
  - `tests/startup.rs::an_occupied_cluster_port_stops_startup_in_production`
  - `tests/startup.rs::a_storage_dir_that_cannot_be_created_stops_startup`

---

### UC-CFG-005

**Case:** `message.max-bytes` cannot exceed `message.hard-limit`, and `cluster.sync.stream.max-line-bytes` must fit the hard limit in base64 plus the envelope.  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::default_message_size_cannot_pass_the_hard_limit`
  - `src/config/tests.rs::stream_line_limit_must_fit_the_hard_limit_in_base64`

---

### UC-CFG-008

**Case:** A configuration value with an invalid type (text where a number is expected, for example) prevents the start with a message that names the parameter.  
**Level:** U  
**Tests:**
  - `src/config/source/tests.rs::invalid_value_names_the_parameter_and_its_variable`
  - `src/config/tests.rs::wrong_type_stops_startup_naming_the_parameter`

---

### UC-CFG-009

**Case:** The precedence is  
**Then:** environment variable, then the `.yml` file, then the table default. An empty variable counts as not defined.  
**Level:** U  
**Tests:**
  - `src/config/source/tests.rs::environment_wins_over_file_and_file_over_default`
  - `src/config/source/tests.rs::empty_environment_value_counts_as_unset`

---

### UC-CFG-010

**Case:** A given file path (`--config` or `TABA_CONFIG_FILE`) that does not exist prevents the start, with the path in the message; the absent default path (`/etc/taba/taba.yml`) does not.  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::an_explicit_config_file_that_does_not_exist_stops_startup`

---

### UC-CFG-011

**Case:** An unknown key in the `.yml` file prevents the start, with the key name in the message; an unknown `TABA_*` variable only generates a warning in the log.  
**Level:** U  
**Tests:**
  - `src/config/source/tests.rs::unknown_yaml_key_is_rejected_by_name`
  - `src/config/source/tests.rs::unknown_taba_variable_becomes_a_warning`

---

### UC-CFG-012

**Case:** A duration without a unit is refused (`60`); with `ms`, `s`, `m`, `h` or `d` it applies. A size uses binary units (`5MB` is 5,242,880 bytes) and accepts a number alone as bytes.  
**Level:** U  
**Tests:**
  - `src/config/source/tests.rs::durations_need_a_unit`
  - `src/config/source/tests.rs::sizes_are_binary_and_accept_plain_bytes`

---

### UC-CFG-013

**Case:** The `cluster.restore.*` parameters (`grace` 5m, `strict` false, `clock-slack` 5s, `probe.timeout` 2h, `probe.interval` 10s) and the limits of the excluded log (`storage.excluded.max-size` 100MB and `storage.excluded.max-age` 24h) have defaults; a zero `grace` or `probe.interval`, a `probe.timeout` shorter than `probe.interval` and a zero limit of the log are validation problems; a value of the wrong type is refused when read  
**Then:** the node does not start with invalid values and shows the key.  
**Level:** U  
**Tests:**
  - `src/config/tests.rs::the_restore_parameters_have_defaults_and_invalid_values_stop_the_start`
  - `src/config/tests.rs::the_limits_of_the_excluded_log_have_defaults_and_invalid_values_stop_the_start`

---


### UC-CFG-014

**Case:** `cluster.tls.enabled=false` and `auth.dev-token` are only accepted with `replication.factor=1`; `auth.dev-token` shorter than 32 characters is refused; with factor 1 `cluster.entry-address` is no longer required
**Then:** the node does not start with the refused value and shows the key; the default stays the production one (TLS on, entry address required).
**Level:** U
**Tests:**
  - `src/config/tests.rs::the_one_node_profile_needs_no_entry_address_and_relaxes_only_with_factor_one`

---

## BKP: Optional S3 backup

### UC-BKP-001

**Case:** Without `backup.s3.bucket` the feature **does not exist**  
**Then:** the node is the usual one and `GET /node/backup`, `POST /node/backup/flush` and `POST /cluster/backup/flush` answer `422` `backup.not-configured`. With the bucket configured but **S3 down at start**, the node **starts with a warning** and the cluster works as without the backup (the feature turns on by itself when S3 returns).  
**Level:** I, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::without_a_backup_nothing_is_told_and_a_missing_body_is_still_a_loss`
  - `tests/backup.rs::with_s3_down_at_start_the_nodes_start_with_a_warning_and_the_cluster_works`
  - `tests/backup.rs::without_a_bucket_the_backup_does_not_exist_and_its_routes_say_so`
  - `tests/s3_store.rs::an_unreachable_s3_is_an_error_the_caller_can_swallow_not_a_panic`

---

### UC-BKP-002

**Case:** The `backup.*` configuration has the documented defaults (`delay` 60 s, `meta.interval` 60 s, segment of 8 MB, 5000 messages or 60 s, heartbeat 15 s, `dead-after` 60 s, `drain.timeout` 30 s, eviction 85/70/90); the credentials are **optional** (without them the default AWS chain applies); `endpoint` turns on `path-style`; the secret never appears in `Debug`; contradictory values prevent the start.  
**Level:** U  
**Tests:**
  - `src/cluster/backup/tests.rs::the_backup_rules_come_from_the_configuration_with_the_documented_defaults`
  - `src/cluster/backup/tests.rs::the_example_configuration_files_are_valid`
  - `src/cluster/backup/tests.rs::the_s3_settings_follow_the_endpoint_and_the_credentials_are_optional`

---

### UC-BKP-003

**Case:** The object store (`ObjectStore`) does `put`, `get`, `get_range`, `get_suffix`, `list` and `delete`, and conditional write (`If-None-Match`, `If-Match`), both on the in-memory fake and on the S3 client (against an S3 over HTTP); a `404` is "does not exist", a `412` is "the condition failed", and an unreachable S3 is an error the caller swallows. **S3 never enters the publishing path:** with it down, publishing and consumption continue, and the `outbox` keeps the IDs until it returns.  
**Level:** U, C  
**Tests:**
  - `src/storage/backup/store/tests.rs::a_store_without_conditional_writes_says_so_and_one_that_is_down_fails`
  - `src/storage/backup/store/tests.rs::conditional_writes_create_once_and_replace_only_the_known_version`
  - `src/storage/backup/store/tests.rs::the_memory_store_does_what_the_object_store_trait_promises`
  - `tests/backup.rs::when_the_primary_dies_the_new_primary_sends_what_was_still_pending`
  - `tests/s3_store.rs::the_s3_client_does_what_the_object_store_contract_says_against_an_s3_server`

---

### UC-BKP-004

**Case:** The base prefix (`backup.s3.prefix`) goes in front of **all** the keys and stays out of the listings; empty, nothing changes.  
**Level:** U, C  
**Tests:**
  - `src/storage/backup/store/tests.rs::the_base_prefix_goes_in_front_of_every_key_and_out_of_the_listing`
  - `tests/s3_store.rs::the_base_prefix_is_a_folder_in_the_bucket`

---

### UC-BKP-005

**Case:** The object names follow the design  
**Then:** `topics/<topic>.json`, `tokens/<id>.json`, `nodes/<id>.json`, `messages/<topic>/<topicId>/<firstId>_<writer>_<seq>.seg`, `journal/…/<instant>_<writer>_<seq>.jrn`; the `topicId` in the path prevents a recreated topic from resurrecting messages of the old one.  
**Level:** U  
**Tests:**
  - `src/storage/backup/keys/tests.rs::segments_of_a_recreated_topic_do_not_share_a_prefix_with_the_old_one`
  - `src/storage/backup/keys/tests.rs::the_key_layout_follows_the_documented_one`

---

### UC-BKP-006

**Case:** The **segment** ends in an index footer (id, offset, size)  
**Then:** the footer can be read from the end of the object without downloading it (asking for more if the footer is larger than the guess) and a single message by `Range`; garbage is refused.  
**Level:** U  
**Tests:**
  - `src/storage/backup/segment/tests.rs::a_segment_round_trips_with_its_footer_index`
  - `src/storage/backup/segment/tests.rs::a_short_tail_asks_for_the_whole_footer_and_garbage_is_refused`
  - `src/storage/backup/segment/tests.rs::metadata_survives_the_conversion_both_ways`
  - `src/storage/backup/segment/tests.rs::one_message_is_read_by_range_without_downloading_the_segment`

---

### UC-BKP-007

**Case:** The **journal** stores the last state of each changed message (counter, deadline, receipt, dead-letter) or the removal; the journal's state overrides that of the segment copy; an unreadable journal is an error, not a panic.  
**Level:** U  
**Tests:**
  - `src/storage/backup/journal/tests.rs::a_broken_journal_is_an_error_not_a_panic`
  - `src/storage/backup/journal/tests.rs::a_journal_round_trips_states_and_removals`
  - `src/storage/backup/journal/tests.rs::a_state_record_overwrites_the_delivery_fields_of_the_segment_copy`

---

### UC-BKP-008

**Case:** `_backup.db` stores the `outbox` (only IDs, from oldest to newest, without repeating), the references of the saved messages, the segments the node wrote with how many live messages, and the sequence counter (which survives a restart).  
**Level:** U  
**Tests:**
  - `src/storage/backup/db/tests.rs::a_segment_is_reported_dead_only_when_its_last_message_dies`
  - `src/storage/backup/db/tests.rs::committing_a_segment_records_the_references_and_empties_the_outbox`
  - `src/storage/backup/db/tests.rs::sequence_numbers_never_repeat_and_a_topic_can_be_forgotten`
  - `src/storage/backup/db/tests.rs::the_outbox_keeps_only_ids_and_hands_them_out_oldest_first`

---

### UC-BKP-009

**Case:** The **commit** of a message only inserts a row in the `outbox`  
**Then:** the engine only notifies the backup (`stored`) and **never waits for S3**.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::publishing_only_tells_the_backup_and_never_waits_for_it`
  - `src/storage/backup/db/tests.rs::the_outbox_keeps_only_ids_and_hands_them_out_oldest_first`
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down`

---

### UC-BKP-010

**Case:** A message confirmed and consumed **before `backup.delay`** never goes to S3 (leaves the `outbox` without generating an upload); those that pass the window are uploaded.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`

---

### UC-BKP-011

**Case:** The segment closes at whichever comes first  
**Then:** size, **number of messages** or age (`backup.segment.*`).  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::a_segment_closes_by_message_count_before_it_gets_old`

---

### UC-BKP-012

**Case:** Only **after the PUT confirms** do the batch rows leave the `outbox` and gain a reference; an S3 that fails leaves everything as it is, and the upload tries again when it returns.  
**Level:** U, C  
**Tests:**
  - `src/storage/backup/db/tests.rs::committing_a_segment_records_the_references_and_empties_the_outbox`
  - `tests/backup.rs::a_message_consumed_before_the_delay_never_reaches_s3_and_the_rest_does`
  - `tests/backup.rs::publishing_never_waits_for_s3_and_the_outbox_keeps_the_ids_while_it_is_down`

---

### UC-BKP-013

**Case:** The mutable state goes in a **journal** every `backup.meta.interval`, only the last state of each message; a delivery is a state change and an **ack is a removal** (a loose message is never deleted from the bucket). Only already saved messages enter the journal, and one that dies **during the upload** of its segment enters as a removal.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::a_delivery_is_a_state_change_and_an_ack_is_a_removal`
  - `src/storage/backup/db/tests.rs::a_message_that_died_during_the_upload_is_not_alive_and_is_journaled_as_removed`
  - `src/storage/backup/db/tests.rs::only_saved_messages_enter_the_journal_and_a_death_is_a_removal`
  - `tests/backup.rs::the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment`

---

### UC-BKP-014

**Case:** The primary deletes a segment from the bucket **only when all its messages have died** (ack, `ttl`, dead-letter); partially dead, it stays.  
**Level:** U, C  
**Tests:**
  - `src/storage/backup/db/tests.rs::a_message_that_died_during_the_upload_is_not_alive_and_is_journaled_as_removed`
  - `src/storage/backup/db/tests.rs::a_segment_is_reported_dead_only_when_its_last_message_dies`
  - `tests/backup.rs::the_journal_keeps_the_last_state_and_an_ack_is_a_removal_that_empties_the_segment`

---

### UC-BKP-015

**Case:** **All** queues enter the backup  
**Then:** volatile and dead-letter ones too.  
**Level:** C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::a_message_that_goes_to_the_dead_letter_is_removed_from_the_queue_and_stored_in_the_other`
  - `tests/backup.rs::volatile_queues_and_dead_letters_are_backed_up_too`

---

### UC-BKP-016

**Case:** The topic definition is written by the **managing node** in `topics/<name>.json`; with S3 down the change **still holds** (is never refused) and is rewritten when it returns.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change`

---

### UC-BKP-017

**Case:** Topic removal writes a **tombstone** in the definition and deletes, in the background, the segments and journals of the `topicId`.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::removing_a_topic_leaves_a_tombstone_and_clears_its_messages_from_the_bucket`

---

### UC-BKP-018

**Case:** Each token goes in a `tokens/<id>.json` with the salted hash (**never the secret**); revocation deletes the object.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::definitions_and_tokens_go_to_the_bucket_and_a_failure_never_refuses_the_change`
  - `tests/backup.rs::tokens_go_to_the_bucket_one_file_each_and_a_revoked_token_disappears`

---

### UC-BKP-019

**Case:** Each node writes `nodes/<id>.json` with id, ips, port, state and a `seq` that grows at each heartbeat; on leaving cleanly (drain) it removes its own file and stops beating.  
**Level:** U, C  
**Tests:**
  - `src/cluster/backup/tests.rs::the_node_writes_its_own_heartbeat_with_a_growing_sequence`
  - `tests/backup.rs::every_node_beats_in_the_bucket_with_a_growing_sequence_and_leaves_on_drain`

---

### UC-BKP-020

**Case:** Whoever observes **does not compare times**  
**Then:** it keeps the ETag of each heartbeat and the (monotonic) instant when it changed; the node is **dead** when the ETag does not change for `backup.dead-after`, and a file seen for the first time counts as fresh; without a file, the node is dead; with S3 unreadable the result is **unknown** (the behavior without the backup applies).  
**Level:** U  
**Tests:**
  - `src/cluster/backup/tests.rs::a_heartbeat_is_fresh_while_its_etag_changes_and_dead_when_it_stops`
  - `src/cluster/backup/tests.rs::a_node_without_a_file_is_dead_and_an_unreadable_bucket_says_nothing`
  - `tests/backup.rs::an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires`

---

### UC-BKP-021

**Case:** A node **unreachable with a fresh heartbeat** is alive in another partition  
**Then:** the vote **does not exclude it** until the heartbeat expires; on expiring, it is excluded.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::an_unreachable_node_with_a_fresh_heartbeat_is_not_excluded_until_the_heartbeat_expires`

---

### UC-BKP-022

**Case:** **Three of five** nodes go down at the same time  
**Then:** the two survivors exclude the dead after the heartbeat expires, and the messages that only existed on the dead are **fetched from S3** (it only becomes a loss if S3 does not have them either).  
**Level:** C  
**Tests:**
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`
  - `tests/backup.rs::three_of_five_nodes_fall_and_the_survivors_find_the_missing_messages_in_s3`

---

### UC-BKP-023

**Case:** The node that starts and **finds no one** beating (all expired) is the **single survivor**  
**Then:** it starts a one-node cluster and restores. A **running** survivor excludes the dead by itself, even below `cluster.vote.min-voters`.  
**Level:** U, C  
**Tests:**
  - `src/cluster/backup/tests.rs::a_node_with_no_cluster_in_the_bucket_only_starts_alone_if_it_is_the_first_or_the_stop_list_says_so`
  - `src/cluster/backup/tests.rs::the_verdict_tells_a_live_cluster_from_a_dead_one_from_an_empty_bucket`
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::a_single_survivor_excludes_the_dead_even_below_the_minimum_of_voters`
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`

---

### UC-BKP-024

**Case:** If some `member` node beats in the bucket, the node that starts **is not alone** and proceeds by the normal path; a node that is also starting (`joining`) does not prove that the cluster exists; without a trace of a cluster, only `first-node` (or the stop list) authorizes starting alone.  
**Level:** U  
**Tests:**
  - `src/cluster/backup/tests.rs::a_node_with_no_cluster_in_the_bucket_only_starts_alone_if_it_is_the_first_or_the_stop_list_says_so`
  - `src/cluster/backup/tests.rs::only_one_survivor_gets_the_restore_lock_and_the_other_waits_as_a_normal_node`
  - `src/cluster/backup/tests.rs::the_verdict_tells_a_live_cluster_from_a_dead_one_from_an_empty_bucket`

---

### UC-BKP-025

**Case:** Two restorers at the same time  
**Then:** the `restore/lock.json` **lock** (created with `If-None-Match`) gives the win to one; the other waits and joins as a normal node; the lock expires by `backup.restore.lock-ttl` if the holder disappears. On an S3 **without conditional write** the node warns that the lock is not safe and continues.  
**Level:** U  
**Tests:**
  - `src/cluster/backup/tests.rs::only_one_survivor_gets_the_restore_lock_and_the_other_waits_as_a_normal_node`
  - `src/cluster/backup/tests.rs::without_conditional_writes_the_lock_is_unsafe_but_the_node_still_starts`
  - `src/storage/backup/store/tests.rs::a_store_without_conditional_writes_says_so_and_one_that_is_down_fails`
  - `tests/s3_store.rs::conditional_writes_use_if_none_match_and_if_match_and_a_412_is_not_an_error`

---

### UC-BKP-026

**Case:** The restore returns definitions, tokens (minus the expired) and messages  
**Then:** segments plus the last state of the journal; the messages the journal removed are discarded.  
**Level:** U, C  
**Tests:**
  - `src/cluster/backup/restore.rs::the_last_journal_state_wins_and_removals_drop_the_message`
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::after_a_crash_the_survivor_waits_for_the_heartbeats_to_expire_and_restores`

---

### UC-BKP-027

**Case:** The cleanup of `nodes/` only deletes the files of ids that **are not members** and whose heartbeat **expired**; a fresh file is never deleted (it may be a live node in another partition).  
**Level:** U  
**Tests:**
  - `src/cluster/backup/tests.rs::cleaning_nodes_removes_only_dead_files_of_nodes_that_are_not_in_the_cluster`

---

### UC-BKP-028

**Case:** With the disk above 85% the node deletes the `.body` of messages that already have a reference in the bucket, down to 70%; the row in `_topic.db` stays.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket`

---

### UC-BKP-029

**Case:** Reading a message whose body was evicted fetches the body from the segment by `Range`; without the backup, the vanished body remains a loss.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::a_body_the_eviction_removed_from_disk_comes_back_from_the_backup`
  - `tests/backup.rs::a_full_disk_evicts_the_bodies_already_in_s3_and_reading_one_goes_back_to_the_bucket`

---

### UC-BKP-030

**Case:** During flush and drain, on reaching 90% the move from memory to disk **pauses** until eviction frees space (at most `backup.drain.timeout`).  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::the_drain_asks_the_backup_before_each_memory_to_disk_write`
  - `tests/backup.rs::at_ninety_percent_the_memory_to_disk_pass_pauses_until_the_disk_has_room`

---

### UC-BKP-031

**Case:** The **drain** sends to S3, with priority, everything on disk and the `outbox` **before** the node leaves the cluster; the node only reaches `drained` after the upload (or the deadline) and removes its own heartbeat.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`

---

### UC-BKP-032

**Case:** `POST /node/backup/flush` does memory → disk, disk → S3 and `outbox` → S3, **without deregistering or stopping the node**, and answers a report (`complete`).  
**Level:** C  
**Tests:**
  - `tests/backup.rs::the_node_flush_sends_everything_without_stopping_the_node`

---

### UC-BKP-033

**Case:** `POST /cluster/backup/flush` forwards to all nodes and gathers the responses; a slow node appears as pending.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending`

---

### UC-BKP-034

**Case:** `POST /cluster/stop` does the drain on **all** nodes and the coordinator writes `cluster/stop.json` with the live nodes and the order in which they stopped; with S3 unavailable it still does the disk drain and answers with a **warning** that the backup was not sent.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`
  - `tests/backup.rs::with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent`

---

### UC-BKP-035

**Case:** **Starting the cluster** is starting the processes  
**Then:** the restore happens by itself, the first node uses the stop list to avoid waiting for the death deadline, the lock and the list are released at the end. A crash (no list) waits for the `backup.dead-after` deadline.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::a_planned_stop_flushes_everything_and_the_next_start_restores_the_cluster`

---

### UC-BKP-036

**Case:** The primary tells the replicas, in a batch, which messages are already in the bucket; the replica only stores the reference of what it has; a message that dies on a replica forgets the reference without touching the journal or the segment count (those belong to the owner).  
**Level:** U, C  
**Tests:**
  - `src/storage/backup/db/tests.rs::a_replica_keeps_the_reference_but_never_the_journal_or_the_segment_count`
  - `tests/backup.rs::the_primary_tells_the_replicas_which_messages_are_already_in_the_bucket`

---

### UC-BKP-037

**Case:** When the primary dies or stops being the owner, the **heir adopts** what is not yet in the bucket and sends it after `backup.delay`; the old owner forgets.  
**Level:** U, C  
**Tests:**
  - `src/engine/messages/tests/backup.rs::a_replica_becomes_the_owner_and_adopts_and_the_old_owner_releases`
  - `tests/backup.rs::when_the_primary_dies_the_new_primary_sends_what_was_still_pending`

---

### UC-BKP-038

**Case:** `GET /node/backup` shows the backup state (S3 up, send queue, segments, what is still missing  
**Then:** `pending`).  
**Level:** C  
**Tests:**
  - `tests/backup.rs::the_cluster_flush_asks_every_node_and_a_slow_one_shows_up_as_pending`

---

### UC-BKP-039

**Case:** With S3 down at stop, the upload **does not block forever**  
**Then:** after `backup.drain.timeout` the node finishes the drain anyway.  
**Level:** C  
**Tests:**
  - `tests/backup.rs::with_s3_down_the_stop_still_drains_every_node_and_warns_that_the_backup_was_not_sent`

---

### UC-BKP-040

**Case:** A single node (one-node profile) owns everything and also does the backup.  
**Level:** I  
**Tests:**
  - `tests/backup.rs::a_single_node_owns_everything_and_backs_up_too`

---

### UC-BKP-041

**Case:** A node finishes restoring its lives and S3 holds live messages of the blocks it is **primary** of that nobody has
**Then:** the scan brings them back through the path of the restoration (topic, dead, quorum) and leaves them available; what the node already has is skipped, and a second pass adds nothing.
**Level:** C
**Tests:**
  - `tests/scan_s3.rs::the_scan_brings_back_the_messages_of_the_bucket_that_no_node_has`

---

### UC-BKP-042

**Case:** S3 holds a message that the journal already records as removed, or one with no removal in the journal that the node's log of excluded messages says is dead
**Then:** the scan discards it in both cases: S3 is not proof of life and does not widen the window of the log.
**Level:** C
**Tests:**
  - `tests/scan_s3.rs::the_scan_discards_what_the_journal_removed_and_what_the_log_says_is_dead`

---

### UC-BKP-043

**Case:** S3 does not answer at the start
**Then:** the scan is skipped with a warning and the node goes on with its disk only, serving normally.
**Level:** C
**Tests:**
  - `tests/scan_s3.rs::with_s3_down_the_scan_is_skipped_and_the_node_goes_on_with_its_disk`

---

### UC-BKP-044

**Case:** The scan consults the journal of a topic
**Then:** it reads the journal **once per topic** and keeps it for the next pass (the query tells "removed" from "not found" because only what the journal removes leaves the set).
**Level:** C
**Tests:**
  - `tests/scan_s3.rs::the_scan_reads_the_journal_of_a_topic_once_and_keeps_it_for_the_next_pass`

---

### UC-BKP-045

**Case:** The S3 bucket answers slowly during the scan at the start
**Then:** only the **FIFO** queues wait (the pass that holds them includes the scan, and the "I am done" notice only goes out at its end); the rest of the cluster is available, the plain queue publishes and delivers, and when the bucket answers the scan ends, the cluster is restored and the FIFO is released.
**Level:** C
**Tests:**
  - `tests/cold_start.rs::a_slow_bucket_holds_only_the_fifo_queues_the_rest_of_the_cluster_is_available`

---

## Sources and evidence

- The cases came from `doc/20a`, `20b` and `20c` (the historical catalogue, removed in commit `efeb47a`); the tests, from the `/// UC:` lines of the code. The test `crates/taba-node/tests/uc_coverage.rs` checks the codes.
- The `UC-BKP` cases (S3 backup) do not come from the historical catalogue: they were written by hand in this file, in the same format, and the test list of each one is checked by `uc_coverage`.
- Maintained by hand: a new test that cites a case goes into its card ([26-use-cases.md](26-use-cases.md)).
