---
title: Plan — interactive interface (TUI) of taba-cli
summary: Plan for `taba-cli tui`, a full-screen interface (Rust + ratatui) to watch and operate a cluster, manage tokens and topics and test messages — on top of routes the server already has. It holds the decisions, the architecture, the requirements per screen, the key map and the phases.
type: plan
status: partial
verified-on: 2026-10-15 @ 44437e9
sources: [connectors/rust/cli/src, connectors/rust/client/src, doc/en-US/16-client-api.md, doc/en-US/17-operations-api.md, doc/en-US/13-observability.md, doc/en-US/20-cli.md]
---

# Plan — interactive interface (TUI) of `taba-cli`

> **Status: 🟨 phases 0, 1 and 3 done, and the Profiles tab of phase 5 (2026-10-15); phases 2 and 4, the end of phase 5 and phase 6 remain.** Every route the TUI calls already exists in the client API (built in `taba-node` on 2026-10-15: D152 to D156 of [30-decisions.md](30-decisions.md)). The foundation and the Cluster tab are in `connectors/rust/cli` (record in section 11); the Tokens and Profiles tabs are done (record in section 11); the Topics and Messages tabs remain. Context: [20-cli.md](20-cli.md) (today's commands and profiles), [16-client-api.md](16-client-api.md) (the routes), [13-observability.md](13-observability.md) (metrics and conditions).

## 1. Goal and scope

`taba-cli tui` opens a full-screen interface ([ratatui](https://ratatui.rs)) on the same connector (`taba-client`) and the same profiles as the commands. It is a screen over the existing API: it **creates nothing on the server**.

| Area | What the person does |
|---|---|
| **Cluster** | See the conditions and the state of each node (memory, disk, sync); **drain** and **return** a node; **backup flush**; **stop the cluster**; charts |
| **Tokens** | List, **create** (the secret shows once), **change** name and expiry, **revoke**, clone |
| **Topics** | List, filter, **create**, **change**, **delete**; charts and groups |
| **Messages** | **Publish**, **listen**, **browse**, **ack** and visibility, **prune** |
| **Profiles** | Today's profile screen, rebuilt (D-TUI-8): list, create, edit, remove, choose the default, test the connection and **connect** with another profile; on opening with no profile given, the choice of the profile |

For every screen: everything scrolls (arrows, `PgUp`/`PgDn`, `Home`/`End`, mouse wheel); **every action has a shortcut** and the mouse is never required; texts in `en-US` and `pt-BR`; an action that deletes or interrupts asks for confirmation; a token secret never goes to a log.

**Out of scope:** starting or killing a node's *process* (the orchestrator's job), editing the server configuration and replacing the `taba-cli` commands (they stay; the TUI only adds).

## 2. Decisions

Made by the project owner; the plan follows them. When built, the ones that change a repository rule go into [30-decisions.md](30-decisions.md).

| Code | Decision |
|---|---|
| **D-TUI-1** | **Two modes.** The direct commands (`topic list`, `token create`, `drain`…) run as today: plain output, no `ratatui`. Only the interactive mode (`taba-cli tui` and the Profiles tab) turns on `crossterm` (raw mode, alternate screen, restored on exit and on panic) and `ratatui`. Both sit behind the Cargo feature `tui`, **on by default** in the binaries and the Docker image; `--no-default-features` gives today's lean CLI. `taba-client` stays dependency-free. Pin versions when starting (ratatui 0.30.x, crossterm 0.29.x). |
| **D-TUI-2** | **Prune: only through the client API.** `DELETE /topics/{topic}/messages` (permission `admin`; D152). The server forwards to the other nodes, is idempotent by `pruneId` and **has no retry**: on failure, the person repeats. The TUI has no loop on the client. |
| **D-TUI-3** | **Node and cluster actions use the root token**, always through the profile's single URL: `POST`/`GET`/`DELETE /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop`. *Stop node* = drain; *Start node* = return (comes back as a new node). Starting or killing the process is **not** offered. |
| **D-TUI-4** | **Node routes** (`/nodes/{nodeId}/…`, ids from `GET /nodes`): the node that receives answers if it is that node, otherwise calls the owner once; an unknown `nodeId` is `404 node.not-found`; a node that does not answer is `502` naming it (D154). **No per-node URL in the profile.** |
| **D-TUI-5** | **History lives only in the session.** From the moment the app opens, a background sampler keeps **1 h at one sample per 5 s** (720 samples per series) in circular buffers; configurable (`history.retention`, `history.interval`; `--history-retention`, `--history-interval`). Nothing goes to disk. For long history, Prometheus ([13-observability.md](13-observability.md)). |
| **D-TUI-6** | **The TUI never reads a queue on its own.** Reading is *delivering*; it happens only when the person asks (*Browse* or *Listen*). *Browse* asks **how many messages** and the **visibility time**, and warns that each read **counts a delivery** (`deliveryCount`) and may send the message to the dead-letter (it shows `maxDeliveries`). There is "return now" (`PUT …/visibility` with `0`). It never acks on its own. |
| **D-TUI-7** | **Translations:** `rust-i18n` 4.2.x + `sys-locale` 0.3.x; embedded YAML catalogs (`locales/en-US.yml`, `locales/pt-BR.yml`), `t!("key", name = value)`, plural by `key.one`/`key.other`; `en-US` is the source and the fallback. A test of ours compares the catalogs' keys and variables. |
| **D-TUI-8** | **The old profile screen** (`tui/term.rs`, `tui/keys`): kept until phase 5; the **Profiles** tab already exists (2026-10-15), and the old screen is only deleted when `taba-cli profile` starts opening the TUI. `taba-cli profile` keeps opening it. |
| **D-TUI-9** | **The TUI's reads use the token, never the operations API** (which stays without a token, for Prometheus and the load balancer). Who sees what: any valid token sees `GET /nodes` (no addresses), `GET /auth/me` and `/stats/topics…` (filtered by its prefixes); only the **root token** sees `GET /cluster/status`, `GET /cluster/backup` and everything of a node. A `403 auth.forbidden` carries `required` (`root` or the permission name), and the TUI uses it to say what is missing (TUI-FRM-10). |
| **D-TUI-10** | **Consolidated reads, with a notice when incomplete (D156).** `GET /stats/topics…`, `/nodes`, `/cluster/status` and `/cluster/backup` are already **the cluster's**, and carry `partial` and `missingNodes: [{nodeId, reason}]` when a node did not answer within 5 s. The TUI **always shows an "incomplete: node X did not answer" badge** when `partial` is `true`, never adds nodes up on its own, and `?scope=node` is kept only for the node detail screen. |

## 3. The routes the screens call

All of them exist. The table is the contract between the screens and the server (details in [16-client-api.md](16-client-api.md)).

| Need | Route | Token |
|---|---|---|
| Who am I | `GET /auth/me` | any |
| List of nodes (id, state, drain, version) | `GET /nodes` | any |
| Conditions, nodes, memory, disk, load (`runtime`) | `GET /cluster/status` | root |
| S3 backup of all nodes | `GET /cluster/backup` (`422 backup.not-configured` without S3) | root |
| A node's metrics (Prometheus text) | `GET /nodes/{nodeId}/metrics` | root |
| Drain / state / return | `POST`, `GET`, `DELETE /nodes/{nodeId}/drain` | root |
| A node's backup / flush | `GET /nodes/{nodeId}/backup`, `POST …/backup/flush` | root |
| Flush of all / stop the cluster | `POST /cluster/backup/flush`, `POST /cluster/stop` ¹ | root |
| Topic statistics and groups | `GET /stats/topics`, `/{topic}`, `/{topic}/groups` | any (prefixes) |
| Topics | `POST`, `GET`, `PATCH`, `DELETE /topics…` | `create` / `admin` |
| Tokens | `/tokens`, `/tokens/{id}` (permissions and prefixes are **immutable**: clone) | root |
| Messages | `…/messages` (publish, consume, ack, visibility) | `publish` / `consume` |
| **Prune** | `DELETE /topics/{topic}/messages` | `admin` on the topic |

¹ Only mounted when the operations API has its own port (PRB-060). Without it the TUI shows them disabled with the reason.

**Limits the TUI accepts and shows:** there is no "peek" (D-TUI-6); there is no count of delayed messages (the "delayed" series is not drawn); latency percentiles and the cluster's number of groups are **approximations** (the largest value among the nodes, PRB-059); prune does not cover a syncing node (PRB-058) nor messages published during it, and the result says so; a node in drain is only addressable while the receiver remembers it (PRB-061).

## 4. Architecture

*Elm* pattern: pure, testable logic, with ratatui as a thin layer.

```text
 keyboard / mouse ──► Event ──► update(Model, Msg) ──► Model' + [Effect]
                                       ▲                        │
   worker threads (HTTP, blocking) ◄───┘ Msg::Loaded/Failed     │ Effect::Call(...)
                                                                ▼
                                              view(Model') ──► ratatui Frame
```

- **`Model`** is all the state, with no I/O. **`Msg`** is every input (key, tick, response, resize). **`update`** is pure and returns **effects** (`Call`, `StartListen`, `StopListen`, `Quit`); it never blocks. **`view`** only draws.
- **Workers:** `taba-client` is blocking; effects run on 4 threads and answer through an `mpsc` channel tagged with a *request id* and a *generation* (a response for an abandoned screen is dropped). `listen` has its own thread with a stop flag.
- **Refresh:** 250 ms tick; polling every 2 s on panels and 5 s on lists; it pauses without focus or with a modal open. A failing call keeps the last data, marks it *stale* and backs off (×2, up to 30 s; the `Retry-After` of `429`/`503` is obeyed).
- **Calls:** the connector has only `Client::call` for what is not a topic or a message; the TUI gains, inside `tui/`, an `api` module with typed functions per route (`cluster_status`, `node_drain`, `prune`…) over `Client::call`. **Nothing is added to `taba-client`.**

```text
tui/
  mod.rs            run(): sets up/restores the terminal (panic-safe), event loop
  model.rs msg.rs update.rs effect.rs worker.rs   the core and the workers
  api.rs            the routes of section 3, typed, over Client::call
  screens/          cluster/ topics/ tokens/ messages/ profiles/  (state + update + view)
  view/widgets/     confirmation, form, help, chart, incomplete badge…
  keymap.rs         action ⇄ keys (single table; feeds the help and the footer)
  i18n/  ../locales/en-US.yml pt-BR.yml
  prom.rs           reader of the Prometheus text (node metrics)
  history.rs        circular buffers
  theme.rs          colors, `NO_COLOR`, ASCII fallback
```

The rules of [24-conventions.md](24-conventions.md) apply: `mod.rs` starts with `//!`, tests in a sibling `tests.rs`, identifiers in English, no `unwrap()` outside tests, and everything that touches the outside world (HTTP, clock, terminal) sits behind a trait.

**Terminal:** `crossterm`, alternate screen, raw mode, mouse and bracketed paste; a panic hook and `Drop` restore the terminal; on Windows only `KeyEventKind::Press`. `NO_COLOR` removes colors; `TABA_CLI_ASCII=1` (or a non-UTF-8 locale) uses ASCII. Minimum **80×24**, below it a notice. Outside a terminal, `taba-cli tui` exits with `2` and prints the help.

## 5. Keyboard

Arrows and Vim keys do the same; the letters are mnemonic **and equal in every language**; the footer shows the keys of the panel and `?` the full list.

| Keys | Action |
|---|---|
| `1`…`5` · `[` `]` · `Alt+←/→` | Go to a tab · previous / next |
| `Tab` · `Shift+Tab` | Next / previous panel |
| `↑ ↓` `k j` · `PgUp PgDn` `Ctrl+u Ctrl+d` · `Home End` `g G` | Move, scroll, first / last |
| `← →` `h l` | Collapse-expand, switch inner tab, move between fields |
| `Enter` · `Esc` | Open / confirm · close / cancel |
| `/` · `s` `S` · `r` · `p` | Filter · sort / reverse · refresh now · pause |
| `:` | Command palette (fuzzy search of any action) |
| `l` · `o` · `?` `F1` · `q` `Ctrl+c` · `Ctrl+s` | Language · switch profile · help · quit (asks if a listener or a changed form exists) · submit a form |
| Mouse | The wheel scrolls the panel under the pointer; a click focuses; optional (`--no-mouse`) |

| Screen | Keys |
|---|---|
| Cluster | `Enter` detail · `d` drain · `u` return · `b` flush this node · `B` flush all · `S` stop the cluster · `m` monitoring |
| Topics | `n` new · `e` edit · `Del` delete · `Enter` open · `m` monitoring · `x` messages |
| Tokens | `n` new · `e` edit · `c` clone · `Del` `x` revoke · `/` filter · `s` sort |
| Profiles | `Enter` connect · `n` new · `e` edit · `d` default · `Del` `x` remove · `t` test |
| Messages | `Ctrl+Enter` publish · `l` listen · `a` ack · `v` visibility · `A` ack all · `P` prune · `b` browse · `c` charts |

## 6. Requirements per screen

Numbered requirements (`TUI-<screen>-<nn>`); tests and phases cite the IDs. The look is decided at build time, with `Table`, `Tabs`, `Sparkline`, `Chart`, `LineGauge`, `Scrollbar` and `Clear`.

### 6.1 The frame (all screens)

| ID | Requirement |
|---|---|
| TUI-FRM-01 | Status line: profile, URL, the cluster's worst condition (word **and** icon, never color alone) and the clock. Source: `GET /cluster/status`; with a plain token, only `GET /nodes`. |
| TUI-FRM-02 | Five tabs (Cluster, Topics, Messages, Tokens, Profiles) by `1`…`5`, `[` `]`, `Alt+←/→`. |
| TUI-FRM-03 | Footer: keys of the focused panel, refresh state (`⟳ 2s`, paused, stale with the time of the last good data) and language. |
| TUI-FRM-04 | Every list and text scrolls; the scrollbar shows only when the content exceeds. |
| TUI-FRM-05 | Every action has a shortcut; the mouse is optional. |
| TUI-FRM-06 | Below 80×24 a notice replaces the screen; resizing redraws at once. |
| TUI-FRM-07 | One popup at a time; `Esc` closes the top one; a destructive popup's default button is **Cancel**. |
| TUI-FRM-08 | Offline, `401`/`403`, `429`/`503` and **`partial: true`** (D-TUI-10) show in the frame without losing the last good data. |
| TUI-FRM-09 | Layout: the selected row is a bold band over dark gray (not `reversed`); every `Sparkline` gets `.max(limit)`; a `Gauge` never inside a `Table` cell; `NO_COLOR` and ASCII work. |
| TUI-FRM-10 | **A missing permission is explained, never hidden:** what the token cannot do stays visible and disabled, or is replaced by a notice ("your token must be the root token to see this"). The text comes from `required` and, before any call, from `GET /auth/me` plus the matrix of section 9. |

### 6.2 Cluster and node detail

| ID | Requirement | Source |
|---|---|---|
| TUI-CLU-01 | Active conditions, the worst first, with icon and word; with none, a "healthy" line. `Enter` explains what it means and what to do. | `GET /cluster/status` · [23](23-operations.md) |
| TUI-CLU-02 | Summary: members by state, quorum, member-table version, incomplete badge. | `summary`, `partial`, `missingNodes` |
| TUI-CLU-03 | Node table: id, state, drain, memory as a bar with %, messages on disk, free disk, blocks syncing, active requests and `429` rejections (`runtime`). An unreachable node turns red and says so. | `NodeStatus` |
| TUI-CLU-04 | Two minicharts of the selected node (memory x limit; ready messages), last 5 min. | `GET /nodes/{id}/metrics` |
| TUI-CLU-05 | Actions `d` `u` `b` `B` `S`, each behind a confirmation; `S` asks for the cluster name typed. An action that cannot run shows disabled **with the reason** (not root, node not answering, route without an operations port). | D-TUI-3, PRB-060 |
| TUI-CLU-06 | Refreshes every 2 s; `p` pauses; `r` refreshes; from 120 columns the charts go to a side column. | — |
| TUI-CLU-07 | With a plain token, lists the nodes (`GET /nodes`) and swaps details, charts and actions for the notice of TUI-FRM-10. | D-TUI-9 |
| TUI-NOD-01 | Detail: id, state, drain, version, mode, start and uptime; memory x limit (yellow at 80 %, red at 95 %); messages in memory and on disk; signals (channels, quorum, blocks syncing, below replication and quorum, frozen nodes). | `NodeStatus` |
| TUI-NOD-02 | S3 backup, only when configured (reachable, pending, last success, segments); absent on `422`. | `GET /nodes/{id}/backup` |
| TUI-NOD-03 | **Drain:** a confirmation saying what happens, then the progress of `pendingMessages` until `drained`. A node in drain or drained offers **return**. | `…/drain` |
| TUI-NOD-04 | **Backup flush** (one node or all) with a report per node: complete or not, pending, timed out. | `FlushReport` |
| TUI-NOD-05 | **Stop cluster:** the popup says the processes keep running as `drained` and that stopping them is the orchestrator's job; the report lists the nodes and warnings. | `POST /cluster/stop` |

### 6.3 Charts (node and topic)

| ID | Requirement |
|---|---|
| TUI-MON-01 | Choice of series. **Node:** memory x limit, ready messages, messages on disk, active requests, `429` rejections, channel RTT. **Topic:** publish, consume and ack rates; depth (visible and invisible); age of the oldest; publish latency p50/p95/p99. |
| TUI-MON-02 | Line chart with a time axis and a scale taken from the data; a limit is a second line; each line has a legend name **and** a distinct marker. |
| TUI-MON-03 | Window of 1, 5, 15, 30 or 60 min, never above the retention (`+`/`-`); pause (`Space`); automatic or fixed scale (`z`). |
| TUI-MON-04 | Summary line: now, min, max, limit, number of samples. |
| TUI-MON-05 | A notice that the history is only what was sampled since the app opened; a chart opened early says "n min collected" instead of faking a full window. |
| TUI-MON-06 | Rates come from counters; a counter that decreases (restarted node) is a restart, never a negative rate. |
| TUI-MON-07 | `e` exports the visible series as CSV, with the path asked in a popup (never writes without asking). |
| TUI-MON-08 | **Topic only:** a Groups tab (messages, share, rates, age of the oldest per `groupId`; overflow marked). With a badge saying the cluster's number of groups is approximate (PRB-059). |

Sources: node from `GET /nodes/{id}/metrics` (root token; `taba.storage.memory.usage`/`limit`, `taba.storage.memory.messages`, `taba.storage.disk.messages`, `http.server.active_requests`, `taba.ratelimit.rejected`, `taba.cluster.channel.rtt`); topic from `GET /stats/topics/{t}` and `…/groups`. Both feed the history of section 7.

### 6.4 Topics

| ID | Requirement | Source |
|---|---|---|
| TUI-TOP-01 | Table: name, type (FIFO / volatile marks), depth, publish and consume rates, age of the oldest, state (worst condition, word + icon). | `GET /topics` + `GET /stats/topics` |
| TUI-TOP-02 | Invisible cursor pagination: scrolling to the end loads the next page, with a loading mark. | `limit`, `cursor` |
| TUI-TOP-03 | Filter by name (on the server), tag and condition; sort by column. | `name`, `tag.<k>`, `condition`, `sort` |
| TUI-TOP-04 | A dead-letter shows dimmed and points to its source; a distributor shows "—" where it does not apply. | `deadLetter`, `source` |
| TUI-TOP-05 | A line under the table summarizes the selected one (type, ttl, max deliveries, tags, prefixes). | `Topic` |
| TUI-TPF-01 | Create form with every field: name, type, FIFO, volatile, idle time, max bytes, ttl, max deliveries, tags and prefixes (repeatable). | `POST /topics` |
| TUI-TPF-02 | Validation while typing (server rules); submit disabled on error; a `4xx` shows `code: message` in the form. | [16](16-client-api.md) |
| TUI-TPF-03 | Editing in the same form: type, FIFO and volatile locked ("does not change afterwards"); only changed fields are sent; the three limits take a number or `default`. | `PATCH` |
| TUI-TPF-04 | Delete asks for the topic name typed and warns the messages are lost. | `DELETE` |
| TUI-TPF-05 | Opening a topic shows tabs: Summary, Configuration, Monitoring, Groups, Messages. Leaving a changed form asks; `Ctrl+S` saves. | — |

### 6.5 Tokens

| ID | Requirement | Source |
|---|---|---|
| TUI-TOK-01 | Table: name, id, permissions, prefixes, expiry; sort by `id` or `createdAt`; filter by name. The secret is never in the list. Expired in red; expiring within 7 days in yellow. | `GET /tokens` |
| TUI-TOK-02 | Create: name, the four permissions (`publish`, `consume`, `create`, `admin`), prefixes, expiry or none. | `POST /tokens` |
| TUI-TOK-03 | The secret shows **once**, in a popup: copy (OSC 52), save to a `0600` file; masked after 30 s or on losing focus; never in a log, the palette history or an error; closing without copying asks again. | `token` of the `201` |
| TUI-TOK-04 | Edit changes only name and expiry (`none` removes the deadline); permissions and prefixes show locked with the reason and a hint to Clone. | `PATCH` |
| TUI-TOK-05 | Clone (`c`) opens Create pre-filled; after creating, offers to revoke the old one (default **No**). Revoke asks for confirmation and says it applies on every node. | `DELETE` |
| TUI-TOK-06 | Without the root token the tab explains the `403` and which profile/token is in use, instead of an empty list. | `required` |

### 6.6 Messages — the workbench

| ID | Requirement | Source |
|---|---|---|
| TUI-MSG-01 | Opens on a topic chosen in a selector, or straight from the list (`x`); shows type and depth. | `GET /stats/topics/{t}` |
| TUI-MSG-02 | **Publish:** multi-line body, content type, `groupId`, `deduplicationId`, repeat count; `Ctrl+Enter` sends; the result (id, `duplicate` or error) stays visible. For JSON, a light check warns without blocking. | `POST …/messages` |
| TUI-MSG-03 | **Listen** (`l`): long polling; each message arrives already delivered; **auto-ack** switchable; `a` ack, `v` visibility, `A` ack all; stopping with un-acked messages asks. | `GET …/messages?wait`, `DELETE`, `PUT …/visibility` |
| TUI-MSG-04 | Table of what was received (time, id, group, deliveries, state `visible`/`invisible until hh:mm:ss`, size, a body snippet), newest first; new arrivals do not move the selection ("↑ n new" badge); keeps the last 1 000. The screen says that **the API does not list a queue's messages**: the rows are the received ones, and the header counters are the truth about the queue. | D-TUI-6 |
| TUI-MSG-05 | Detail of the selected one: indented body if JSON, with scrolling; deliveries, invisible until, receipt; above 64 KiB it shows truncated with its size and loads whole on request. | `Message` |
| TUI-MSG-06 | **Browse** (`b`), only on request: a popup with quantity (default 10) and visibility time (default 30 s), the warning of D-TUI-6 and a confirmation; the read ones show as *invisible until*; `v` with `0` returns them now. Opening the screen, choosing a topic or refreshing **never** reads the queue. | D-TUI-6 |
| TUI-MSG-07 | **Prune** (`P`): the person types the topic name; the popup shows how many messages will be lost (`depth`) and that **every node** will be told; a single wait ("pruning on n nodes…"); at the end, the `removed`, or the `503 cluster.prune-incomplete` failure saying which nodes did not finish and that the queue may be partly pruned (repeating is safe). The result warns that messages published during the prune are not covered. | `DELETE /topics/{t}/messages` |
| TUI-MSG-08 | A distributor is not listened to, browsed or pruned: those keys show disabled with the reason. Without `consume`, listen, browse and prune are disabled; without `publish`, the publish panel; a `429` shows the `Retry-After` countdown. | [09](09-topics.md) |
| TUI-MSG-09 | **Counters in the header**, from the sampler: visible, invisible, depth, age of the oldest. | `GET /stats/topics/{t}` |
| TUI-MSG-10 | **Search** (`/`) over the loaded rows (id, group, body), `n`/`N` jump, the found snippet highlighted, "found / rows", no server call. **Sorting** (`s`, `S`) by time, id, group, deliveries, size or invisible-until; the selection follows the message. | — |
| TUI-MSG-11 | **Selection charts** (`c`; hidden below 100 columns): three stacked charts with a shared axis — visible and invisible messages; flow (published, consumed, acked); age of the oldest. They reuse the rules of TUI-MON-02 to -06 and the same history (they add no polling). | D-TUI-5 |

### 6.7 Help, palette, language and profiles

| ID | Requirement |
|---|---|
| TUI-HLP-01 | `?` shows the screen's keys and the global ones, **generated from the same table** (`keymap.rs`) the program uses. |
| TUI-HLP-02 | `:` opens the palette: finds any action of any screen (in the current language and in English) and runs it. |
| TUI-HLP-03 | `l` picks the language (English, Português); it applies at once and is saved in `ui.language`. |
| TUI-HLP-04 | The Profiles tab lists, creates, edits, removes, sets the default and tests a connection with unsaved values; it replaces today's screen ([20-cli.md](20-cli.md)). |
| TUI-HLP-05 | Switching profile reconnects everything, resets the histories and shows the new profile in the status line. |
| TUI-HLP-06 | **On opening with no profile given** (no `--profile`, `TABA_CLI_PROFILE` or `TABA_CLI_URL`) **and with more than one profile**, the TUI opens a box to choose: the **default** profile (●) comes selected, `Enter` connects and `Esc` uses the default. With a single profile (or none) it takes it, without asking. Nothing is read from a server before the choice. The same box opens later with `o`, and `Esc` only closes it. |

## 7. Data and history

| Screen | Calls | Period |
|---|---|---|
| Cluster | `GET /cluster/status`; for the selected node: `…/metrics`, `…/drain`, `…/backup` | 2 s · 5 s in the detail |
| Topic list | `GET /topics` (paged) + `GET /stats/topics?fields=name,depth,publishRate,consumeRate,oldestAge,worstCondition` | 5 s |
| Topic monitoring | `GET /stats/topics/{t}` (series from the sampler) · `…/groups?top=20` (only with the screen open) | `history.interval` |
| Tokens | `GET /tokens` (paged) | 10 s and after each change |
| Messages | `…/messages` (listen, browse, `POST`, `DELETE`, `PUT …/visibility`) | on demand; depth from the sampler |

`prom.rs` reads the Prometheus text (names, labels, values; `# TYPE` for the counters) and computes the rate from the difference between two samples.

**The sampler** (`history.rs`) runs through the whole session and, every `history.interval` (default 5 s, minimum 1 s), makes **two calls**: `GET /cluster/status` (load, memory and disk of every node) and `GET /stats/topics` (every topic, already consolidated). Each series is a circular buffer of `retention / interval` samples (default 720). It pauses on a `429` (obeying `Retry-After`) and is reset on a profile switch. The sample of a `partial` read is stored **marked as incomplete** and the chart shows it so. Memory is bounded (series × samples × 16 bytes) and the number of followed topics has a ceiling (default 200, the busiest first; the ceiling shows in the status line). A retention above 6 h or an interval below 1 s is refused.

## 8. Languages (i18n)

- **Catalog** (D-TUI-7): `connectors/rust/cli/locales/{en-US,pt-BR}.yml`, `rust_i18n::i18n!("locales", fallback = "en-US")`. Every other file has exactly the keys and variables of `en-US`.
- **A missing key** falls back to `en-US` and then to the key itself; it never crashes the screen, and the parity test catches it before release.
- **Language choice**, the first that exists: `--lang` · `TABA_CLI_LANG` · `ui.language` (saved by `l`) · the system locale (`LC_ALL`, `LC_MESSAGES`, `LANG`; on Windows, the interface language) · `en-US`. `pt`, `pt_BR`, `pt-BR.UTF-8` match `pt-BR`.
- **Translated:** label, title, hint, validation, popup, empty state and help. **Not translated:** key, route, JSON field, error **code**, metric or condition name. The server's own `message` shows as it came, after a translated frame.
- **Layout:** widths by `unicode-width`; columns sized by the **translated** header; pt-BR is 20–30 % longer, so 80 columns is tested in **both** languages.
- **Formats:** dates and durations per language; thousands separator per language; binary sizes (`MiB`, `GiB`) in both.
- **Adding a language:** copy `en-US.yml`, translate, add the code to the table and, if the plural differs, to `plural()`.

## 9. Errors, security and token

| Situation | Behavior |
|---|---|
| Server unreachable | `✖ offline`; data dimmed with its age; retry with backoff; actions disabled; never a hang |
| `401` / `403` | Shows what is missing (`required`) and the profile in use; an unavailable action is disabled **with the reason** |
| `429` / `503` | Obeys `Retry-After`; the footer counts down |
| `partial: true` | An "incomplete" badge with the nodes of `missingNodes`; the numbers are the other nodes' |
| `502`/`504` on a node action | Says which node failed; keeps the other nodes' data |
| Form `4xx` | `code: message (requestId …)` in the form; `y` copies the request id |
| Dangerous action | Confirmation, default **Cancel**; typed for *stop cluster*, *delete topic*, *prune* |
| Secrets | Token masked after 30 s; never in a log, history, palette or error; copy by OSC 52 or a `0600` file |
| Panic | The hook restores the terminal before printing |

**What each action asks of the token** (the TUI enables or disables before calling; the server decides):

| Area / action | Minimum token |
|---|---|
| `GET /auth/me`, `GET /nodes` | any valid token |
| Topics: list, read, statistics, groups | any valid token, **within its prefixes** |
| Create a topic | `create` |
| Change, delete or prune a topic | `admin` on the topic |
| Publish · listen, browse, ack, visibility | `publish` · `consume` |
| Tokens; cluster detail; a node's metrics, drain, return and backup; stop the cluster | root token |

## 10. Tests

| Level | What |
|---|---|
| **Pure core** | `update()` per screen: keys in, model and effects out, no terminal and no network. Navigation, filters, forms, confirmations, circular buffer, `prom.rs`, uniqueness of the key map |
| **Drawing** | `TestBackend`: each screen at **80×24** and **120×40**, in **en-US and pt-BR**, against text snapshots in `tests/`; each test says the `TUI-…` IDs it covers |
| **i18n** | Parity of keys and variables; every used key exists; every catalog key is used |
| **Integration** | The workers with the fake `Transport` (`taba_client::testing`): scripted responses → `Msg` → model. Includes the listener, pagination, the `429` backoff, stale data and the `partial` badge |
| **Against a node** | Optional: a scenario drives the TUI core, without a screen, against a real node of the `taba-node` tests: create topic → publish → listen → ack → prune → delete token. The TUI use cases (a new area) enter the catalog when built |
| **Manual** | Windows Terminal, PowerShell, cmd, Git Bash, Terminal/iTerm, Linux console, `tmux`, SSH, 80×24 and a 4K window — a table in the PR |

Little disk on this machine: run only `cargo test -p taba-cli` and `cargo clean -p taba-cli` afterwards.

## 11. Phases

Each phase ends with something usable, green tests and docs updated **in every language**. The routes already exist, so no phase depends on the server.

| Phase | Delivery | Done when |
|---|---|---|
| **0 — Foundation** ✅ | Feature `tui`, `taba-cli tui`, a panic-safe terminal, event loop, Elm core, workers, `api.rs`, the frame (6.1), key map, help, palette, i18n in both languages, theme/`NO_COLOR`, confirmation. **First:** check that `musl` and `arm64` build with ratatui/crossterm and measure the binary against `dist/` | The empty tabs navigate; switching language works; drawing and i18n tests pass |
| **1 — Cluster** ✅ | 6.2, `prom.rs`, `history.rs`, node charts (6.3), the `partial` badge, node and cluster actions | Matches `taba-cli health cluster` and `drain *`; a drain and a return through the TUI on the 3-node Compose |
| **2 — Topics** | 6.4 and topic monitoring and groups | Create, edit and delete equal to the `topic *` commands; smooth paging with more than 1 000 topics |
| **3 — Tokens** ✅ | 6.5 with the secret popup and cloning | Equal to the `token *` commands; the secret is never visible after closing |
| **4 — Messages** | 6.6: publish, listen, ack, visibility, browse, **prune** and the selection charts | A message published in the TUI is received in the same TUI and by `taba-cli consume`; prune on a 3-node Compose empties every node, and with a node stopped fails naming it |
| **5 — Profiles and cleanup** 🟨 (the Profiles tab is done) | The Profiles tab (D-TUI-8); remove `tui/term.rs` and `tui/keys`; mouse, ASCII, narrow layouts | The old code is gone; `taba-cli profile` does not change for those who use it |
| **6 — Docs and release** | [20-cli.md](20-cli.md), [05-dependencies.md](05-dependencies.md), [24-conventions.md](24-conventions.md) (the CLI's zero-dependency rule), the CLI `README.md`, build scripts and Dockerfile, [30-decisions.md](30-decisions.md); captures | `verificar.mjs` and `verificar-idiomas.mjs` pass; the binaries of the six targets build |

**Risks.** (1) `musl` and `arm64` with ratatui/crossterm (phase 0). (2) The binary grows; measure and record. (3) The TUI never claims an emptied queue it cannot prove: the `removed` and the partial failure are shown as the server gives them.

### Record of phase 3 and the Profiles tab (2026-10-15)

- **Order of the tabs:** Cluster, Topics, **Messages, Tokens**, Profiles (`3` is Messages and `4` is Tokens).
- **Where it is:** `src/tui/screens/{tokens,profiles}` (state, keys and drawing), `src/tui/editor` (the form of fields and checkboxes, with its tests), `src/tui/view/modal/forms.rs` (the popups of the form, the secret and the choice of profile); `taba-client` did not change.
- **Tokens (TUI-TOK-01 to 06):** a paged list (200 at a time, up to 25 pages) with a filter by part of the name (`/`) and an order (`s`: newest, oldest, name, id); an expired token is red and one that expires within 7 days is yellow. Create (`n`), edit name and expiry (`e`; permissions and prefixes show locked with the reason), clone (`c`) and revoke (`Del` or `x`, with a confirmation that starts on **Cancel**). The expiry takes empty or `none` (no deadline), `30d` `12h` `90m` (from now), a date (`2027-01-31`, the start of that day in UTC) or an RFC 3339 instant. The server's `400` shows in the form itself, which stays open. Without the root token the tab says which token is in use; with no authentication on the server it says there are no tokens.
- **The secret (TUI-TOK-03):** it exists only inside the popup: never in the footer notice, in an error or in a log. `c` sends it to the clipboard (OSC 52, if the terminal allows it); `s` writes it to `taba-token-<id>.txt` in the current folder (`0600` on Unix, never over an existing file: the plan spoke of choosing the path). It is hidden (dots) after 30 s or when the window loses the focus, and `c` still copies it. Closing without copying or saving asks for a second `Esc`. On a clone, the box **Revoke the old one on closing** (`r`) starts **unticked** and lives inside the secret popup rather than after it (the popup is the only place the secret lives).
- **Profiles (TUI-HLP-04 to 06):** a list with the default (●) and the profile in use; `n` new, `e` edit (name, `url`, `ops-url`, `token-file`, `token`, `ca-file` and the default box; the profile's other fields, such as `output` and the timeouts, are kept), `d` makes it the default, `Del` or `x` removes (confirmation on **Cancel**), `t` tests the profile's connection and `Ctrl+T`, in the form, tests **values not yet saved** (`GET /topics?limit=1`). `Enter` **connects**: the next calls go to the profile's server, the history and the screens start over and a reply still coming from the old server is dropped. Saving only replaces `default` and `profiles` in the file (the language and `[history]` stay as they are on disk). A token typed in the form stays in the file as typed, and the screen never draws it (it shows as dots); the form recommends a token file.
- **Differences from the plan:** `o` is **switch profile** on every screen; the keys became lowercase (`l` language, `o` profile) because they are common actions; capitals are kept for the critical or irreversible. `h` and `l` are no longer arrows (only `← →`). On the Messages tab, `l` listen would shadow `l` language: pick another key when building it. `Ctrl+T` and `t` are new keys. The old screen (`taba-cli profile`) stays as it was, until the cleanup of phase 5.
- **Tests:** `cargo test -p taba-cli` passes (272 unit, 76 command, 54 screen snapshots); `clippy` has no warnings. The manual table of section 10 and a real terminal (OSC 52, focus, paste) were **not** exercised here.

### Record of phases 0 and 1 (2026-10-15)

- **Where it is:** `connectors/rust/cli/src/tui/{api,app,effect,history,i18n,keymap,model,msg,prom,screens,theme,update,view,worker}` (one module per folder, with `tests.rs`), `src/commands/tui.rs` and `locales/{en-US,pt-BR}.yml`. `ui.language` and `[history]` live in `config.yml` (`--lang`, `TABA_CLI_LANG`, `--history-retention`, `--history-interval`); `taba-client` did not change. The old profiles screen (D-TUI-8) stays as it was.
- **Done:** the frame (status line, tabs, footer, a warning below 80×24), a single key map that generates the help and the palette, popups that default to **Cancel**, language in two catalogs with a parity test, `NO_COLOR`, and the whole **Cluster** tab (TUI-CLU-01 to 07, TUI-NOD-01 to 05, TUI-MON-01 to 07 for the node, the `partial` badge). The Topics, Tokens, Messages and Profiles tabs open, navigate and say what is coming.
- **Measures:** a Windows x86_64 `release` binary (`opt-level=z`, LTO, no symbols, as `build.ps1` does): **1,307,648 B without the feature** (the one in `dist/` is 1,304,064 B; the difference is the two new `config.yml` sections) and **1,750,528 B with it**: **+446 KB, +34 %**.
- **Targets:** `ratatui`, `crossterm`, `rust-i18n` and `sys-locale` pass `cargo check` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin`. The whole `taba-cli` was **not built** for them here: the `ring` of `rustls` (already a dependency) needs a C cross compiler (`zigbuild`/`cross`) that this machine does not have; that is left to the CI and `build.sh`.
- **Tests:** `cargo test -p taba-cli` passes (232 unit tests, 76 command tests, 37 screen snapshots in `tests/snapshots/` in both sizes and languages); each test cites the `TUI-…` IDs. `tests/live_node.rs` (optional, `TABA_LIVE_URL`) drives the core against a real node: it read `/auth/me`, `/cluster/status`, `/nodes/{id}/metrics` and the `422 backup.not-configured` of a development node. The binary opened and drew in a pseudo-terminal against that node.
- **Not verified:** the **drain and return on the 3-node Compose** (the Docker daemon was stopped; it is the done-when of phase 1 and stays open), the keys and the terminal being given back in real terminals (the manual table of section 10) and the focus listener outside Windows.
- **Differences from the plan, and why:** a node id shows as start…end (`01a1…76b316`), because GUID v7 ids of nodes made together start the same; the status line clock is UTC (no time-zone library); `Space` freezes the chart and `z` switches to the zero-based scale; the "disk free" and "429" columns only show from 100 columns; ASCII swaps symbols, bars and markers, but the box borders stay Unicode (phase 5); a node with no memory limit (development mode) shows "no limit", and `signals.channels` arrives as a map, not a number.
- **A server limit the TUI shows:** a lone node in drain refuses **everything**, its own return included (`503 cluster.node-draining`); only in a cluster with another node, or through the operations API, can it be returned. It is PRB-061 up close.
- **Left to close the cycle (phase 6):** [05-dependencies.md](05-dependencies.md), the zero-dependency rule in [24-conventions.md](24-conventions.md), the CLI `Dockerfile`, [30-decisions.md](30-decisions.md) (D-TUI-1 to 10) and the screenshots.


## 12. Impact on what exists

| What | Change |
|---|---|
| `connectors/rust/cli/Cargo.toml` | Feature `tui` (default): `ratatui`, `crossterm`, `rust-i18n`, `sys-locale` |
| `src/app.rs`, `src/commands/mod.rs` | The `tui` command; global `--lang` |
| `src/settings` | `ui.language` and `[history]`, with the precedence of [20-cli.md](20-cli.md) (`TABA_CLI_LANG`). **No new profile field** |
| `src/tui/` | The tree of section 4; `term.rs` and `keys` leave in phase 5 |
| `connectors/rust/client` | Nothing |
| `taba-node` | **Nothing.** Done and tested in `tests/nodes_api.rs` (cases UC-TOP-044 to 049, UC-AUT-037 to 039, UC-OPS-046 to 051) |
| Documentation | This file and the phase 6 ones, in `pt-BR` and `en-US` together |

## 13. Open questions

| # | Question | Default if unanswered |
|---|---|---|
| Q1 | A profile with per-node `start-command`/`stop-command` to start and kill processes? | No: outside the API |
| Q2 | Does `taba-cli` with no arguments in a terminal open the TUI? | No: only `tui`; with no arguments it prints the help |
| Q3 | Keep the export (chart CSV, message body)? | Keep, with a path popup |
| Q4 | Mouse: wheel and click, or keyboard only? | Wheel and click, optional (`--no-mouse`) |

## Sources and evidence

- Today's CLI: `connectors/rust/cli/src/{app.rs,commands,profile,settings,tui}` and `connectors/rust/client/src` (blocking `Client`, `testing` transport); [20-cli.md](20-cli.md).
- Routes, parameters and DTOs: [16-client-api.md](16-client-api.md), [17-operations-api.md](17-operations-api.md), [18-cluster-api.md](18-cluster-api.md); decisions D152 to D156 in [30-decisions.md](30-decisions.md); limits PRB-058 to PRB-061 in [31-pending-items.md](31-pending-items.md).
- Metrics and conditions: [13-observability.md](13-observability.md). Library versions: `cargo search` on 2026-10-15 (ratatui 0.30.2, crossterm 0.29.0).
- The TUI of phases 0 and 1: `connectors/rust/cli/src/tui`, `locales/` and `tests/{snapshots,live_node.rs}`. There are no TUI use cases in the catalog yet: the tests cite the `TUI-…` IDs of this document.
