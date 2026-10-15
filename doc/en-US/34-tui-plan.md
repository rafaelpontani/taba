---
title: Plan — interactive interface (TUI) of taba-cli
summary: Plan and per-screen requirements for `taba-cli tui`, a full-screen interface in Rust with ratatui to watch and operate a cluster, manage tokens and topics, and test messages — with the requirements of each screen, keyboard map, data sources, languages, tests and phases.
type: plan
status: planned
verified-on: 2026-10-15 @ 56121c9
sources: [connectors/rust/cli/src, connectors/rust/client/src, doc/en-US/16-client-api.md, doc/en-US/17-operations-api.md, doc/en-US/13-observability.md, doc/en-US/20-cli.md]
---

# Plan — interactive interface (TUI) of `taba-cli`

> **Status: ⬜ planned.** Nothing here exists yet; the decisions of [section 2](#2-decisions) are taken and the plan awaits the go-ahead to be built. Context you need first: [20-cli.md](20-cli.md) (today's commands and profiles), [17-operations-api.md](17-operations-api.md) and [16-client-api.md](16-client-api.md) (the routes the screens call), [13-observability.md](13-observability.md) (metrics and conditions). Where this plan finds that the API does not offer something the request needs, it says so in [section 3](#3-what-the-api-does-and-does-not-offer) and asks for a decision in [section 2](#2-decisions).

## 1. Goal and scope

A new command, **`taba-cli tui`**, opens a full-screen interface (Rust + [ratatui](https://ratatui.rs)) on top of the same connector (`taba-client`) and the same profiles the commands already use. It is a *screen over the existing API*, like the profile screen of today: it adds no server feature except what [section 2](#2-decisions) decides.

| # | Area | What the person can do |
|---|---|---|
| 1 | **Cluster** | See how the cluster is (conditions, each node's state, memory, disk, synchronization); **drain** a node, **return** it, force **backup**, **stop the cluster**; see monitoring charts |
| 2 | **Tokens** | List, filter, **create** (the secret is shown once), **change** name and expiry, **revoke**; "change" what is immutable by cloning |
| 3 | **Topics** | List, filter, **create**, **change**, **delete**; per-topic **monitoring charts** and groups |
| 3.2 | **Messages** | **Publish** (test), **listen** (live), **browse**, **ack**/change visibility, **prune** a queue |

Requirements that cut across all screens:

- **Fluid navigation:** every list and text scrolls (arrows, `PgUp`/`PgDn`, `Home`/`End`, mouse wheel) with a scrollbar; focus moves between panes; **every action has a shortcut** and nothing is mouse-only ([section 5](#5-keyboard-and-navigation)).
- **Several languages:** `en-US` and `pt-BR` first, others by adding one file ([section 8](#8-languages-i18n)).
- **Same safety rules as the commands:** actions that delete or interrupt ask for confirmation; the token secret is never written to a log.

**Not in scope:** starting or killing the node *process* (the API has no such route; that is the orchestrator's job — see D-TUI-3), editing the server configuration, and replacing `taba-cli` commands (they stay; the TUI is additive).

## 2. Decisions

All the decisions below were taken by the project owner and are in force: the rest of this plan follows them. They change the server too, which is why the plan lists server work ([section 12](#12-impact-on-what-exists)). When the work starts, they are registered in [30-decisions.md](30-decisions.md) (with the next free `D…`) and the open questions in [31-pending-items.md](31-pending-items.md).

| Code | Subject | Decision |
|---|---|---|
| **D-TUI-1** | **Dependencies and terminal mode.** Today `taba-cli` depends only on the connector ([05-dependencies.md](05-dependencies.md), "zero dependencies" rule) and has its own raw-mode terminal. | **Two modes.** The **direct commands** (`topic list`, `token create`, `drain`…) run exactly as today: plain output, no screen control, no `ratatui` code involved. The **interactive mode** — `taba-cli tui` and the profiles screen (D-TUI-8) — is the only one that starts `crossterm` (raw mode, alternate screen, restored on exit and on panic) and draws with `ratatui` (tables, charts, lists). The two libraries (~25 transitive crates) come behind a Cargo feature `tui`, **on by default** in released binaries and in the Docker image; `--no-default-features` gives the lean CLI of today. The connector (`taba-client`) stays dependency-free. Pin versions (ratatui 0.30.x, crossterm 0.29.x at the time of writing; re-check when starting). |
| **D-TUI-2** | **Prune has no route.** The client API has no way to empty a queue ([section 3](#3-what-the-api-does-and-does-not-offer)). | **Prune is created in `taba-node` and exposed in the client API:** `DELETE /topics/{topic}/messages` (`admin` permission; answers `{"removed":n}` or the failure). The node that receives it **forwards the prune to every other member** through a new cluster API route (`POST /cluster/topics/{topic}/prune`, mTLS, members only — the same fan-out pattern as `POST /cluster/backup/flush`), prunes its own copy, waits for all and answers **success only if all succeed**, otherwise failure naming the nodes that failed. The call is idempotent by a `pruneId` and, being rare, has **no retry machinery**: on failure the person repeats it. It goes through the load balancer, so it needs no per-node URL. The TUI has **no client-side loop**. Details in section 6.8 and section 12. |
| **D-TUI-3** | **"Start" and "stop" a node.** The API only has *drain* (stop receiving, write to disk, leave the cluster) and *return*. The process keeps running as `drained`. | **Same treatment as prune: created in `taba-node`, in the client API, with the root token** (the only cluster-wide credential; the `admin` permission is per topic). *Stop node* = drain; *Start node* = return from drain (it comes back as a new node); *Stop cluster* and *backup flush* are cluster-wide. Today these routes are in the operations API, without a token, protected by network and per node; the change exposes them in the client API (`POST`/`GET`/`DELETE /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop`; final names are set in the server change). Killing or launching processes is **not** offered (orchestrator job). |
| **D-TUI-4** | **Which node does a node action reach?** `drain` applies only to the node that receives the call, **not through the load balancer**, and the status does not carry each node's operations URL. | **Same treatment as prune: the node that receives the call calls the right node internally.** Every node-addressed route carries the `nodeId` (`/nodes/{nodeId}/…`, learned from `GET /health/cluster`); the node that receives it, through the load balancer or any address, forwards the call to the target over the cluster port (mTLS, members only; a new `/cluster/node-call/{path}` that runs the operations API of the target, because the existing `/cluster/forward/{path}` needs the target to be active and a node in drain must stay reachable to be returned: D154), the node that received the call checks the token, and it returns the answer as it came. **The rule, the same as for a message:** the node that receives the call compares the `nodeId` of the path with its own; if it **is** that node, it answers from its own data; if it is **not**, it calls the node that has the data once, over the cluster port, and never forwards a forwarded call again; an unknown `nodeId` is `404 node.not-found`, and a target that does not answer is a failure that names it (no retry). The same applies to the node metrics of D-TUI-9. If the target does not answer: failure naming the node. This also serves the target node's `GET /metrics` for the sampler (D-TUI-5). **No `nodes` field and no per-node URL in the profile.** |
| **D-TUI-5** | **Charts need history; the API gives only the present.** | From the moment the app opens, the TUI **samples in the background for the whole session** into in-memory ring buffers: by default **1 hour of history at one sample every 5 s** (720 samples per series), both **configurable** (`history.retention`, `history.interval`; flags `--history-retention`, `--history-interval`). The oldest sample is overwritten. Nothing is stored on disk and the history is lost on exit. For long history, use Prometheus ([13-observability.md](13-observability.md)); the TUI says so on the chart. |
| **D-TUI-6** | **Reading a queue.** There is no "peek" route; reading a queue means *delivering* the message. | **The TUI never reads a queue on its own.** Reading happens only when the person asks (*Browse* or *Listen*), and *Browse* asks first **how many messages** to read and **for how long they stay invisible** (visibility time). The popup states that each read **counts a delivery** (`deliveryCount`) and can push a message to the dead-letter when `maxDeliveries` is set (it shows that limit). The messages come back by themselves when the time ends; the screen also offers "return now" (`PUT …/visibility` with `0`). It never acks on its own. |
| **D-TUI-7** | **Translations.** ratatui has no i18n. | **`rust-i18n` (4.2.x) + `sys-locale` (0.3.x).** Catalogs are YAML files embedded at compile time (`locales/en-US.yml`, `locales/pt-BR.yml`), used with the `t!("key", name = value)` macro; `en-US` is the fallback and the source. Plurals are `key.one` / `key.other` keys chosen by a small rule per language. `sys-locale` reads the OS language. A test of ours compares the key sets and placeholders of the files (rust-i18n does not check parity itself) and fails the build on a difference. Fluent and gettext were considered and left out: heavier, and their strength is languages with complex plurals. |
| **D-TUI-8** | **The old profile screen** (own raw terminal, `term.rs`, `keys`). | Keep until phase 6; then **rebuild it in ratatui as the "Profiles" tab** and delete `tui/term.rs` and `tui/keys` (net code reduction). `taba-cli profile` keeps opening it. |
| **D-TUI-9** | **Metrics and status need a token.** The routes `GET /health/cluster`, `GET /metrics`, `GET /metrics/topics*` and `/node/*` live in the operations API, **without authentication** (D63), because they serve continuous monitoring (Prometheus, load balancer). They stay as they are. The TUI cannot use them: it has one URL and one token. | **Authenticated copies in the client API**, read-only, for the TUI and other clients: `GET /cluster/status` (what `/health/cluster` answers), `GET /nodes/{nodeId}/metrics` (the node's Prometheus text, forwarded as in D-TUI-4), `GET /stats/topics`, `GET /stats/topics/{topic}` and `GET /stats/topics/{topic}/groups` (the `TopicStats` of the operations API). Two more routes complete the set: `GET /nodes` (the list of the cluster's nodes — a client may need it) and `GET /auth/me` (who the token is: name, permissions, prefixes, expiry, and whether it is the root token). Final names are set in the server change. **Who may call — the principle: anything a plain token can see within its scope, it sees without being an administrator.** `GET /nodes` (id, state, drain state, version; **no addresses**), `GET /auth/me` and the topic routes (`/stats/topics…`, every valid token, **filtered by its prefixes**, as `GET /topics` already is) — any valid token. Cluster detail (`GET /cluster/status`: memory, disk, signals, conditions, addresses) and everything of a node (`/nodes/{nodeId}/…`) — the **root token** only. **What is denied says what is needed:** a `403 auth.forbidden` carries a new field `required` (e.g. the root token, or the permission `admin` on the topic), so that the TUI and any client can say "your token needs the root token to see this". See the matrix in section 9. Same rate-limit class as the administrative routes (`ratelimit.admin.rps`); the sampler (D-TUI-5) makes two calls every 5 s, well inside it. The client API fixes, with these routes, the cluster **aggregation** of topic metrics (finding A of [31-pending-items.md](31-pending-items.md)), or the sampler sums the `scope=node` view of each node; the server change picks one. The operations routes are unchanged. |

## 3. What the API does and does not offer

Checked against [16](16-client-api.md) and [17](17-operations-api.md) on 2026-10-15. This table is the contract between the screens and the server.

| Need | Route | Status |
|---|---|---|
| Cluster conditions, nodes, memory/disk | `GET /health/cluster` (`ClusterStatus` / `NodeStatus`) | ✅ on the operations API, **without a token**; ❌ with a token → D-TUI-9 (`GET /cluster/status`) |
| Learn each node's id | `GET /health/cluster` (the `nodeId` of each member) | ✅ |
| Drain / drain status / return | `POST`, `GET`, `DELETE /node/drain` — **on the node itself** | ✅ |
| Backup state / flush / stop cluster | `GET /node/backup`, `POST /node/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop` | ✅ (`422 backup.not-configured` without S3) |
| Node monitoring | `GET /metrics` (Prometheus text, **this node only**, no token) | ✅ for monitoring; ❌ for the TUI → D-TUI-9 (`GET /nodes/{nodeId}/metrics`, root token); the TUI needs a small text parser |
| Topic monitoring and groups | `GET /metrics/topics`, `/{topic}`, `/{topic}/groups` (no token) | ❌ with a token → D-TUI-9 (`GET /stats/topics…`). ⚠️ `scope=cluster` does **not** aggregate across nodes today (finding A in [31](31-pending-items.md)): the TUI shows the serving node's view and says so |
| Metric **history** | — | ❌ → D-TUI-5 |
| Topics: create / list / get / change / delete | `POST`, `GET`, `GET`, `PATCH`, `DELETE /topics…` | ✅ |
| Tokens: create / list / get / change / revoke | `/tokens`, `/tokens/{id}` (root token only) | ✅ — permissions and prefixes are **immutable** (clone, see 6.7) |
| Publish / consume / ack / visibility | `…/messages` routes | ✅ |
| **Prune** (empty a queue) | — | ❌ → D-TUI-2: **created in `taba-node`** (client route + cluster route) before the TUI uses it |
| **Peek** (read without delivering) | — | ❌ → D-TUI-6 |
| Start or kill a node **process** | — | ❌ → D-TUI-3 |
| Call a **specific node** through any node (drain, metrics, backup of that node) | — (operations routes are per node, without a token) | ❌ → D-TUI-3 / D-TUI-4: **created in `taba-node`** (client route + internal forward) |

## 4. Architecture

### 4.1 Shape

The interface follows the *Elm* pattern, which keeps the logic pure and testable and leaves ratatui as a thin layer:

```text
 keyboard / mouse ──► Event ──► update(Model, Msg) ──► Model' + [Effect]
                                       ▲                       │
   worker threads (HTTP, blocking) ────┘ Msg::Loaded/Failed    │ Effect::Call(...)
                                                               ▼
                                              view(Model') ──► ratatui Frame
```

- **`Model`**: all state (active tab, per-screen state, data last loaded, modals, language). Plain data, no I/O.
- **`Msg`**: every input — a key, a tick, an answer from a worker, a terminal resize.
- **`update`**: pure function; returns **effects** (`Call`, `StartListen`, `StopListen`, `Quit`). Never blocks.
- **`view`**: draws the model with ratatui widgets. No decisions here beyond layout.
- **Workers:** `taba-client` is **blocking** and the CLI has no async runtime, so the effects run on a small pool of threads (4) that send the answer back through an `mpsc` channel tagged with the *request id* and a *generation* (an answer for a screen already left is dropped). `listen` has its own thread with a stop flag.
- **Refresh:** a tick every 250 ms drives animation and timers; each screen declares its polling period (2 s dashboards, 5 s lists). Refresh pauses when the terminal loses focus or a modal is open. A failing call keeps the last data, marks it *stale* (dimmed, "last update 14:02:09") and backs off (×2, up to 30 s; `Retry-After` of `429`/`503` is obeyed).

### 4.2 Modules (inside `connectors/rust/cli/src/tui/`)

```text
tui/
  mod.rs            run(): terminal setup/teardown (panic-safe), event loop
  model.rs  msg.rs  update.rs            the pure core
  effect.rs  worker.rs                   effects and the thread pool
  view/             one file per screen + widgets/ (confirm, form, help, chart…)
  screens/          cluster/ topics/ tokens/ messages/ profiles/  (state + update + view each)
  keymap.rs         action ⇄ key bindings (single table; feeds the help and the footer)
  i18n/             `rust-i18n` setup, plural rule, formats ; ../locales/en-US.yml pt-BR.yml
  prom.rs           Prometheus text parser (node metrics)
  history.rs        ring buffers for charts
  theme.rs          colors, `NO_COLOR`, ASCII fallback
```

Rules from [24-conventions.md](24-conventions.md) apply: `mod.rs` starts with a `//!` comment, unit tests live in a sibling `tests.rs`, identifiers in English, no `unwrap()` outside tests, and everything that touches the outside (HTTP, clock, terminal) sits behind a trait so tests inject fakes.

### 4.3 The terminal

`crossterm` backend; **alternate screen**, **raw mode**, mouse capture (wheel and click) and **bracketed paste** (so pasting a JSON body is one event). A panic hook and `Drop` restore the terminal. Windows: only `KeyEventKind::Press` is handled (crossterm also reports releases there). `NO_COLOR` removes colors; `TABA_CLI_ASCII=1` (or a non-UTF-8 locale) swaps box and chart glyphs for ASCII. Minimum size **80×24**; below it, a centered notice (`Paragraph` in a `Clear`ed area) replaces the screen. Off a terminal, `taba-cli tui` exits with `2` and prints the help, like `profile`.

## 5. Keyboard and navigation

**Principle:** arrows and Vim keys do the same thing; letters are mnemonic **and fixed across languages** (the footer text is translated, the keys are not); every screen shows its keys in the footer and the full list under `?`.

### 5.1 Global keys

| Keys | Action |
|---|---|
| `1`…`5` · `Alt+←/→` · `[` `]` | Go to a tab · previous / next tab |
| `Tab` · `Shift+Tab` | Next / previous **pane** inside the screen |
| `↑ ↓` or `k j` | Move selection / scroll one line |
| `PgUp PgDn` · `Ctrl+u Ctrl+d` | Scroll a page / half page |
| `Home End` or `g G` | First / last |
| `← →` or `h l` | Collapse-expand, change tab inside a screen, move between fields |
| `Enter` · `Esc` | Open / confirm · close / cancel / back |
| `/` | Filter the list (type; `Enter` keeps, `Esc` clears) |
| `s` | Cycle the sort column; `S` reverse (where the API sorts) |
| `r` | Refresh now |
| `p` | Pause / resume automatic refresh |
| `:` | **Command palette**: type to find any action in any screen and run it (fuzzy) |
| `L` | Choose the language |
| `?` · `F1` | Help (keys of this screen, then the global ones) |
| `q` · `Ctrl+c` | Quit (asks if a listener is running or a form is dirty) |
| `Ctrl+s` | Save / submit the open form |
| Mouse | Wheel scrolls the pane under the pointer; click selects/focuses; **never required** |

### 5.2 Keys by screen (summary — the detail is in each screen of section 6)

| Screen | Keys |
|---|---|
| Cluster | `Enter` node detail · `d` drain · `u` return (undo drain) · `b` backup flush · `B` flush **all** nodes · `S` stop cluster · `m` monitoring |
| Topics | `n` new · `e` edit · `Del` delete · `Enter` open · `m` monitoring · `x` messages workbench |
| Tokens | `n` new · `e` edit · `c` clone · `Del` revoke · `y` show id |
| Messages | `Ctrl+Enter` publish · `l` start/stop listening · `a` ack · `v` visibility · `A` ack all · `P` prune · `b` browse · `c` clear the view |

## 6. Screens

Each screen is described by what we **want** from it: an objective and numbered requirements (`TUI-<screen>-<nn>`). Tests, pull requests and the phases of section 11 cite these IDs. How the screen looks is decided when it is built, with the ratatui widgets that fit (`Table`, `Tabs`, `Sparkline`, `Chart`, `LineGauge`, `Scrollbar`, `Clear`…); a visual prototype was drawn on 2026-10-15 and discarded to keep this document light — its lessons are kept as the layout rules of TUI-FRM-09.

### 6.1 The frame (all screens)

**Objective.** Always-visible context, navigation and rules that every screen obeys.

| ID | Requirement | API / source |
|---|---|---|
| TUI-FRM-01 | A status line shows the profile, the URL, the worst cluster condition (word **and** icon, never color alone) and the clock. | `GET /health/cluster` |
| TUI-FRM-02 | Five tabs — Cluster, Topics, Tokens, Messages, Profiles — reachable by `1`…`5`, `[` `]` and `Alt+←/→`. | — |
| TUI-FRM-03 | The footer shows the keys of the focused pane, the refresh state (`⟳ 2s`, paused, stale with the time of the last good data) and the language. | — |
| TUI-FRM-04 | Every list and text pane scrolls (arrows, `PgUp`/`PgDn`, `Home`/`End`, mouse wheel) and shows a scrollbar only when the content overflows. | — |
| TUI-FRM-05 | Every action has a keyboard shortcut; the mouse is optional and never required. | — |
| TUI-FRM-06 | Below 80×24 a notice replaces the screen; a resize redraws at once. | — |
| TUI-FRM-07 | At most one popup at a time; `Esc` closes the topmost; the default button of a destructive popup is **Cancel**. | — |
| TUI-FRM-08 | Offline, `401`/`403` and `429`/`503` are shown in the frame (see section 9) without losing the last good data. | — |
| TUI-FRM-09 | Layout rules: the selected row is a bold band on dark gray (not `reversed`, which inverts cell colors); a `Sparkline` always gets `.max(limit)`; a `Gauge` never goes inside a `Table` cell; `NO_COLOR` and ASCII mode work. | — |
| TUI-FRM-10 | **A missing permission is explained, never hidden.** What the token cannot do stays visible but disabled, or is replaced in its place by a notice: "Your token needs the root token to see this" / "…the `admin` permission on `orders.*`". The text comes from the `required` field of `403 auth.forbidden` and, before any call, from `GET /auth/me` plus the matrix of section 9; the profile's token and what it can do are shown in the help (`?`). Never a blank area, never a bare "403". | `GET /auth/me` · `auth.forbidden` · D-TUI-9 |

### 6.2 Cluster — the dashboard

**Objective.** See at a glance how the cluster is, and operate its nodes.

| ID | Requirement | API / source |
|---|---|---|
| TUI-CLU-01 | A list of the active conditions, worst first, each with severity icon and word; with none, a "healthy" line. `Enter` on a condition explains what it means and what to do. | `GET /health/cluster` · [13](13-observability.md) · [23](23-operations.md) |
| TUI-CLU-02 | A summary: members by state, quorum available, members version, "partial" when a node did not answer. | `ClusterStatus.summary`, `partial` |
| TUI-CLU-03 | A table of nodes: id, state, drain state, memory used as a bar with %, disk messages, free disk, syncing blocks. An unreachable node is red and says so. | `NodeStatus` (`state`, `drainState`, `storage`, `signals`) |
| TUI-CLU-04 | Two live mini charts for the selected node (memory vs its limit; ready messages), last 5 minutes. | `GET /metrics` do nó |
| TUI-CLU-05 | `Enter` opens the node detail; `m` opens the full monitoring (6.4). | — |
| TUI-CLU-06 | Node actions straight from the list: `d` drain, `u` return, `b` flush backup, `B` flush backup on all nodes, `S` stop the cluster — each behind a confirmation (6.3). | `/nodes/{nodeId}/drain`, `/nodes/{nodeId}/backup/flush`, `/cluster/backup/flush`, `/cluster/stop` · D-TUI-3 |
| TUI-CLU-07 | An action that cannot run (the token is not the root token, the node did not answer, or the server has no such route) is shown disabled **with the reason**. | D-TUI-3, D-TUI-4 |
| TUI-CLU-08 | Refreshes every 2 s; `p` pauses; `r` refreshes now. On a terminal of 120 columns or more, the charts move to a side column. | — |
| TUI-CLU-09 | With a token that is not the root, the Cluster tab still lists the nodes (id, state, drain state, version, from `GET /nodes`) and replaces the details, the charts and the actions by the notice of TUI-FRM-10. | `GET /nodes` · D-TUI-9 |

### 6.3 Node detail and cluster actions

**Objective.** Understand one node in depth and run the operations that affect it.

| ID | Requirement | API / source |
|---|---|---|
| TUI-NOD-01 | Identity: id, state, drain state, version, mode (production/development), start time and uptime. | `NodeStatus` |
| TUI-NOD-02 | Storage: memory used vs limit as a gauge (yellow at 80 %, red at 95 %), messages in memory and on disk, free disk. | `NodeStatus.storage` |
| TUI-NOD-03 | Signals: open channels, quorum, syncing blocks, blocks below replication and below quorum, frozen nodes. | `NodeStatus.signals` |
| TUI-NOD-04 | S3 backup block, only when configured: reachable, pending, last success, segments. Absent on `422`. | `GET /node/backup` |
| TUI-NOD-05 | **Drain:** confirmation saying what will happen; then progress of `pendingMessages` until `drained`. A node already draining or drained offers **return** instead (it comes back as a new node). | `POST`/`GET`/`DELETE /nodes/{nodeId}/drain` · D-TUI-3 |
| TUI-NOD-06 | **Backup flush** of this node or of all nodes, with a report: complete or not, per node, what is still pending, timeout. | `FlushReport`, `ClusterFlushReport` · D-TUI-3 |
| TUI-NOD-07 | **Stop cluster:** the person types the cluster name to enable the button; the popup says the processes keep running as `drained` and that stopping them is up to the orchestrator; a report lists the nodes and any warning (`stopList: false`). | `POST /cluster/stop` · D-TUI-3 |

### 6.4 Monitoring charts (node and topic)

**Objective.** Follow the numbers over time, for a node or for a topic.

| ID | Requirement | API / source |
|---|---|---|
| TUI-MON-01 | A choice of series. **Node:** memory vs limit, ready messages, disk messages, requests in progress, `429` rejections, channel RTT. **Topic:** publish, consume and ack rates, depth, oldest age, publish latency p50/p95/p99. | `GET /metrics` · `GET /stats/topics/{t}` |
| TUI-MON-02 | A line chart with a time axis and a scale taken from the data (with headroom); a limit, when it exists, is drawn as a second line. Each line has a legend name **and** a distinct marker. | — |
| TUI-MON-03 | A window of 1, 5, 15, 30 or 60 minutes, never above the retention (`+`/`-`), pause (`Space`), auto-scale or fixed scale (`z`). | — |
| TUI-MON-04 | A summary line: now, min, max, limit, number of samples. | — |
| TUI-MON-05 | A notice that the history is only what was sampled since the app opened, up to the retention (for days, use Prometheus). A chart opened early shows a short line, and says "n min collected" instead of faking a full window; nothing goes to disk. | D-TUI-5 |
| TUI-MON-06 | Rates are computed from counters; a counter that goes down (node restarted) is treated as a reset, not a negative rate. | — |
| TUI-MON-07 | `e` exports the visible series to CSV; the path is asked in a popup (the TUI never writes a file without asking). | — |
| TUI-MON-08 | **Topic only:** a second tab, Groups — a table of messages, share, rates and oldest age per `groupId`, to find the noisy producer; the overflow group is marked. | `GET /stats/topics/{t}/groups` |

**Charts of 6.4, one by one.** The node charts read `GET /nodes/{nodeId}/metrics` (Prometheus text, **root token**); the topic charts read `GET /stats/topics/{topic}` (token with a permission on the topic). Both feed the session history of D-TUI-5, so all of them have the same window and pause.

| Chart | Series (source field) | Screens |
|---|---|---|
| **Node — memory** | `taba.storage.memory.usage` against `taba.storage.memory.limit` (the limit is a second line) | Cluster (mini), Node monitoring |
| **Node — ready messages** | `taba.storage.memory.messages` | Cluster (mini), Node monitoring |
| **Node — messages on disk** | `taba.storage.disk.messages` | Node monitoring |
| **Node — requests in progress** | `http.server.active_requests` | Node monitoring |
| **Node — `429` rejections** | rate of `taba.ratelimit.rejected` | Node monitoring |
| **Node — channel RTT** | `taba.cluster.channel.rtt` (mean and p95), one line per peer | Node monitoring |
| **Topic — flow** | `publishRate`, `consumeRate`, `ackRate` | Topic monitoring, Messages (panel 2) |
| **Topic — depth** | `depth`; `visibleMessages` and `invisibleMessages` | Topic monitoring, Messages (panel 1) |
| **Topic — oldest message** | age from `oldestMessageAt` | Topic monitoring, Messages (panel 3) |
| **Topic — publish latency** | `publishLatency.p50`, `.p95`, `.p99` | Topic monitoring |
| **Topic — groups** (table, not a chart) | `GET /stats/topics/{topic}/groups` | Topic monitoring, Groups tab |

The "delayed" series of the reference panel has no source today: the topic metrics have no count of delayed messages, so it is not drawn.

### 6.5 Topics — list

**Objective.** Find a topic fast, see which ones need attention, and open or create one.

| ID | Requirement | API / source |
|---|---|---|
| TUI-TOP-01 | A table: name, type (with FIFO / volatile marks), depth, publish and consume rate, oldest age, state (the worst condition as word + icon). | `GET /topics` + `GET /stats/topics` |
| TUI-TOP-02 | Paging by cursor is invisible: scrolling to the end loads the next page; a loading mark shows while it does; `G` goes to the last loaded row. | `limit`, `cursor` |
| TUI-TOP-03 | Filter by name (sent to the server), by tag and by condition; sort by a column (`s`, `S` reverses). | `name`, `tag.<k>`, `condition`, `sort` |
| TUI-TOP-04 | A dead-letter topic is dimmed and points to its source; a distributor shows "—" where depth does not apply. | `deadLetter`, `source` |
| TUI-TOP-05 | A line under the table summarizes the selected topic: type, ttl, max deliveries, tags, prefixes. | `Topic` |
| TUI-TOP-06 | Keys: `n` new, `e` edit, `Del` delete, `Enter` open, `m` monitoring, `x` messages. | — |

### 6.6 Topic — detail and form

**Objective.** Create, understand, change and delete a topic without leaving the screen.

| ID | Requirement | API / source |
|---|---|---|
| TUI-TPF-01 | Create form with every field of the API: name, type, FIFO, volatile, idle timeout, max message bytes, ttl, max deliveries, tags (repeatable), prefixes (repeatable). | `POST /topics` |
| TUI-TPF-02 | Validation as you type with the server rules; the submit button stays disabled while there is an error; a server `4xx` shows its `code: message` in the form. | [16](16-client-api.md) |
| TUI-TPF-03 | Edit uses the same form: type, FIFO and volatile are shown locked ("cannot change later"); only the changed fields are sent; the three limits accept a number or `default`. | `PATCH /topics/{t}` |
| TUI-TPF-04 | Delete asks the person to **type the topic name** and warns that all its messages are lost. | `DELETE /topics/{t}` |
| TUI-TPF-05 | Opening a topic shows tabs: Overview (numbers and conditions), Settings (this form), Monitoring, Groups, Messages. | — |
| TUI-TPF-06 | Leaving a form with unsaved changes asks first; `Ctrl+S` saves; `Esc` cancels. | — |

### 6.7 Tokens

**Objective.** Manage access tokens safely, treating the secret with care.

| ID | Requirement | API / source |
|---|---|---|
| TUI-TOK-01 | A table: name, id, permissions, prefixes, expiry; sort by `id` or `createdAt`; filter by name (sent to the server). The secret is never in the list. | `GET /tokens` |
| TUI-TOK-02 | Expired tokens are red and say "expired"; those expiring within 7 days are yellow. | `expiresAt` |
| TUI-TOK-03 | Create: name, the four permissions (`publish`, `consume`, `create`, `admin`), topic prefixes, expiry date or none. | `POST /tokens` |
| TUI-TOK-04 | The secret appears **once**, in a popup: copy (OSC 52 terminal sequence), save to a file with mode `0600`; masked after 30 s or on focus loss; never logged, never in the palette history, never in an error text; closing without copying asks again. | `token` of the `201` |
| TUI-TOK-05 | Edit changes only name and expiry (`none` removes the deadline); permissions and prefixes are shown locked with the reason and a pointer to Clone. | `PATCH /tokens/{id}` |
| TUI-TOK-06 | Clone (`c`) opens Create pre-filled from the selected token; after creating, it offers to revoke the old one (default **No**). | — |
| TUI-TOK-07 | Revoke asks for confirmation and says it takes effect on all nodes. | `DELETE /tokens/{id}` |
| TUI-TOK-08 | Without a root token the tab explains the `403` and which profile/token is in use, instead of an empty list. | `403` |

### 6.8 Messages — the workbench

**Objective.** Test a topic end to end: publish, listen, inspect, acknowledge and prune.

| ID | Requirement | API / source |
|---|---|---|
| TUI-MSG-01 | Opens on a topic chosen from a picker, or straight from the Topics screen (`x`); shows its type and current depth. | `GET /stats/topics/{t}` |
| TUI-MSG-02 | **Publish:** multi-line body, content type (`json`, `text`, other), `groupId`, `deduplicationId`, repeat count; `Ctrl+Enter` sends; the result (id, `duplicate`, or the error) stays visible. For JSON a soft validity check warns but does not block — the server decides. | `POST …/messages` |
| TUI-MSG-03 | **Listen** (`l`): live long polling; each message arrives already delivered (invisible to others for its visibility time); **auto-ack** can be on or off; manual `a` ack, `v` change visibility, `A` ack all. Stopping with messages not acked asks first. | `GET …/messages?wait`, `DELETE`, `PUT …/visibility` |
| TUI-MSG-04 | A table of what was received (time, id, delivery count, group, body), newest first; while reading, new arrivals do not move the selection (a "↑ n new" badge shows); keeps the last 1 000. | — |
| TUI-MSG-05 | A detail pane for the selected message: body pretty-printed when JSON, with its own scroll; delivery count, invisible-until, receipt token; a body above 64 KiB is truncated with its size and can be loaded in full. | `Message` |
| TUI-MSG-06 | **Browse** (`b`), only on request: a popup asks **how many messages** (default 10, up to the server limit) and the **visibility time** (default 30 s) and states that each read **counts a delivery** and, with `maxDeliveries`, can send a message to the dead-letter (shows the limit); it asks to confirm. The messages read appear in the list as *invisible until hh:mm:ss*; `v` with `0` returns them now. Opening the screen, selecting a topic or refreshing **never** reads the queue. | D-TUI-6 |
| TUI-MSG-07 | **Prune** (`P`): the person types the topic name; the popup shows how many messages will be lost (`depth`, visible + invisible, from the sampler) and that **every node** will be asked; a single wait view ("pruning on n nodes…") with no progress bar; at the end: success with the number removed, or failure naming the nodes that did not conclude and saying the queue may be partly pruned (the person can repeat). Messages published meanwhile are not covered, and the result says so. Disabled when the server has no prune route (`404`/`405`), with the reason. | `DELETE /topics/{topic}/messages` · D-TUI-2 |
| TUI-MSG-08 | A distributor cannot be listened to, browsed or pruned (it only copies): those keys are disabled with the reason. | [09-topics.md](09-topics.md) |
| TUI-MSG-09 | Permissions are respected: without `consume` listen/browse/prune are disabled; without `publish` the publish pane is; the footer says why. A `429` shows the `Retry-After` countdown. | `401`, `403`, `429` |
| TUI-MSG-10 | **Header counters** above the list, refreshed with the sampler (every 5 s by default): **visible**, **invisible** (in processing), depth and oldest age, as in SQS' "available / in flight". | `GET /stats/topics/{t}` (`visibleMessages`, `invisibleMessages`, `depth`, `oldestMessageAt`) · D-TUI-9 |
| TUI-MSG-11 | The list shows minimal columns only: time, id, group, delivery count, state (`visible` / `invisible until hh:mm:ss`), size and a body excerpt (one line). The API does not list the messages of a queue (there is no peek); the rows are those received by listen or browse (TUI-MSG-04, -06), and the header counters (TUI-MSG-10) are the whole truth about the queue. The screen says so. | D-TUI-6 |
| TUI-MSG-12 | **Search** (`/`): filters the loaded rows by text (id, group, body); `n` / `N` jump to the next / previous hit; the match is highlighted; `Esc` clears; a counter shows "hits / rows". Search never calls the server. | — |
| TUI-MSG-13 | **Sort** (`s` opens a menu, `S` flips the direction): by time, id, group, delivery count, size or invisible-until; the sort key and its arrow show in the table header; the selection follows the message, not the row number. The default is time, newest first. | — |
| TUI-MSG-14 | **Charts of the selection.** Selecting a row (or the topic header) opens a side panel with three small charts **stacked vertically**, sharing the time axis, for the message's topic: (1) **messages — visible and invisible** (two lines; right axis for the smaller one); (2) **flow — published, consumed, acked, deleted/empty receives** as rates; (3) **maximum age — oldest message**. They are the same three panels as the reference queue dashboard, one above the other so they fit a narrow panel. | `GET /stats/topics/{t}` |
| TUI-MSG-15 | The charts reuse the monitoring rules (TUI-MON-02 to -06): window `+`/`-`, pause, scale from the data, a legend with name **and** marker, a min / mean / max / last line under each chart, and the "n min collected" notice. The charts read the same session-wide history as the Monitoring screen, so opening one does not add polling. | D-TUI-5 |
| TUI-MSG-16 | `Tab` moves focus between list, detail and charts; `c` shows or hides the charts panel (hidden by default under 100 columns, where the detail pane has priority); the panel scrolls vertically when the terminal is short. | — |

### 6.9 Help, palette, language and profiles

**Objective.** Never be lost: find any key or action, change language, switch cluster.

| ID | Requirement | API / source |
|---|---|---|
| TUI-HLP-01 | `?` opens help for the current screen, then the global keys; it is generated from the same key table the program uses, so it cannot disagree with it. | `keymap.rs` |
| TUI-HLP-02 | `:` opens a command palette: typing finds any action of any screen (fuzzy, in the current language and in English) and runs it. | — |
| TUI-HLP-03 | `L` picks the language (English, Português); it applies at once and is saved. | `ui.language` |
| TUI-HLP-04 | The Profiles tab lists, creates, edits, removes and sets the default profile, and tests a connection with values not yet saved; it replaces today's profile screen. | [20-cli.md](20-cli.md) · D-TUI-8 |
| TUI-HLP-05 | Switching profile reconnects every screen, clears the histories and shows the new profile in the status line. | — |

## 7. Data and refresh

| Screen | Calls | Period | Notes |
|---|---|---|---|
| Cluster | `GET /cluster/status` · per selected node: `GET /nodes/{nodeId}/metrics`, `GET /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup` | 2 s · 5 s on the detail | Everything goes through the profile's one URL; the receiving node reaches the right node (D-TUI-4) |
| Topics list | `GET /topics` (paged) · `GET /stats/topics?fields=name,depth,publishRate,consumeRate,oldestAge,worstCondition` | 5 s | Stats merged by `name` |
| Topic monitoring | `GET /stats/topics/{t}` (the series come from the background sampler) · `/stats/topics/{t}/groups?top=20` | `history.interval` (5 s) | Groups only while the screen is open |
| Tokens | `GET /tokens` (paged) | 10 s and after every change | |
| Messages | `GET …/messages` (listen/browse), `POST`, `DELETE`, `PUT …/visibility` | on demand | Depth label from the sampler (`GET /stats/topics/{t}`) every 5 s |

**Prometheus text** (`GET /nodes/{nodeId}/metrics`) is read by `prom.rs`: names, labels and values only (`# TYPE` for counters, so a rate is computed as the difference between two samples divided by the time). Counters that reset (node restarted) are detected by the value going down.

**History** (`history.rs`): a background **sampler** starts with the app and, every `history.interval` (default **5 s**, minimum 1 s), reads `GET /cluster/status` (load, memory and disk of every node, in one call) and `GET /stats/topics` (all topics of the cluster, in one call) into one fixed-size ring buffer per series; the size is `history.retention / history.interval` (default 1 h / 5 s = 720 samples). It keeps running whatever screen is open, pauses on a `429` (honouring `Retry-After`) and is cleared on a profile switch. Memory is bounded: series × samples × 16 bytes (about 90 KB for a topic with 8 series), and the number of topics tracked is capped (default 200, the busiest first; the cap is shown in the status line when it applies). The settings live in the config file (`[history]`), with the defaults above; a retention above 6 h or an interval below 1 s is refused with a message.

## 8. Languages (i18n)

- **Catalog** (D-TUI-7): `connectors/rust/cli/locales/en-US.yml` and `pt-BR.yml`, loaded by `rust_i18n::i18n!("locales", fallback = "en-US")` and embedded in the binary. One key per text, `%{name}` placeholders, plural forms as `key.one` / `key.other` (a small `plural(lang, n)` rule per language chooses). English (`en-US`) is the **source**; every other file must have exactly the same keys.
- **Use:** `t!("cluster.drain.confirm.title")`, `t!("topics.count", n = 24)`. A missing key in a language falls back to `en-US`, then to the key itself; it never crashes the screen, and the parity test catches it before release.
- **Choosing the language**, first match wins: `--lang <code>` · `TABA_CLI_LANG` · `ui.language` in the config file (set by `L`) · the OS locale (`LC_ALL`, `LC_MESSAGES`, `LANG`; on Windows the user's UI language) · `en-US`. `pt`, `pt_BR`, `pt-BR.UTF-8` all match `pt-BR`.
- **What is translated:** every label, title, footer hint, validation message, popup, empty state and help text. **What is not:** keys on the keyboard, routes, JSON fields, error **codes** (`topic.not-found`), metric names, condition names — these are identifiers ([24-conventions.md](24-conventions.md)). The server's own `message` text is shown as received, after the translated frame ("The server answered:").
- **Layout safety:** widths use the `unicode-width` rules that `ratatui` already brings (no extra crate); columns size from the **translated** header; pt-BR is ~20–30 % longer than en-US, so the 80-column screens are tested in **both** languages.
- **Formats:** dates and durations by language (`2026-10-15 14:02` vs `15/10/2026 14:02`; `3 s`, `2 h 11 min`); number grouping (`1,204` vs `1.204`); sizes with binary units (`MiB`, `GiB`) in both.
- **Adding a language:** copy `en-US.yml`, translate, add the code to the language table and (if it has plural rules different from the two known) to `plural()`; the parity test tells what is missing. The documentation rule of [24-conventions.md](24-conventions.md#documentation-in-several-languages) still applies to this plan: the language folders change together.

## 9. Errors, security and resilience

| Situation | Behaviour |
|---|---|
| Server unreachable / timeout | Header shows `✖ offline`; data dimmed with its age; retry with backoff; actions disabled; never a crash |
| `401` / `403` | The tab shows what permission is missing (`root token only`, `admin`…) and which profile/token is in use; actions that need it are hidden, not grayed without reason |
| `429` / `503` | Obey `Retry-After`; the footer shows the countdown |
| Form `4xx` | `code: message (requestId …)` in the form's error line; the request id can be copied (`y`) for the server logs |
| A node does not answer the forwarded call | The action says which node failed and keeps the others' data |
| Dangerous action | Confirm popup, default **Cancel**; typed confirmation for *stop cluster*, *delete topic*, *prune* |
| Secrets | Token secret masked after 30 s; never in logs, history, palette or error text; profile `token` shown as today (`token-file` preferred); copy by OSC 52 or a `0600` file |
| Terminal too small / resized | Redraw on resize; below 80×24 a notice replaces the screen |
| Panic | Hook restores the terminal first, then prints the panic |
| Slow disk / big payloads | Bodies above 64 KiB are shown truncated with their size (`Enter` to load the rest into the detail pane); `Received` keeps the last 1 000 messages |

**What each area asks of the token.** The TUI uses this table to enable or disable actions before calling; the server is still the one that decides.

| Area / action | Minimum token |
|---|---|
| Who am I (`GET /auth/me`), node list (`GET /nodes`) | any valid token |
| Topics: list, read, statistics and groups | any valid token, **within its prefixes** |
| Create a topic | `create` (name within the prefixes) |
| Change or delete a topic | `admin` on the topic |
| Publish | `publish` |
| Listen, browse, ack, change visibility | `consume` |
| Prune a queue | `admin` on the topic |
| Tokens (list, create, delete) | root token |
| Cluster detail, node metrics, drain, return, backup, stop the cluster | root token |


## 10. Tests

| Level | What |
|---|---|
| **Unit (pure core)** | `update()` per screen: key sequences in, model and effects out — no terminal, no network. Navigation, filters, forms and their validation, confirmation flows, history ring buffer, `prom.rs`, keymap uniqueness (no two actions on one key in one context) |
| **Render** | ratatui `TestBackend`: each screen rendered at **80×24** and **120×40**, in **en-US and pt-BR**, compared with stored text snapshots (`tests/` sibling files, in plain text so diffs are readable). At least one snapshot per screen; the requirements of section 6 (`TUI-…`) are the acceptance criteria and each test names the IDs it covers |
| **i18n** | Parity (same keys, same placeholders in all the YAML files); no key used in code missing from the catalog; every key in the catalog used |
| **Integration** | The workers with a **fake `Transport`** (`taba_client::testing`, which already exists): scripted API answers → Msg → model. Includes the listener (long poll, ack, stop), paging, `429` backoff and stale data |
| **Against a node** | Optional: the cluster tests of `taba-node` start a real development node; one scenario drives the TUI core headless through create topic → publish → listen → ack → prune → delete token. Use-case codes for the TUI (a new area) are added to the catalog when this is built (the generated files 26a/26b follow the repository's generation rule, not hand edits) |
| **Manual checklist** | Windows Terminal, PowerShell, cmd, Git Bash (mintty), macOS Terminal/iTerm, a Linux console, `tmux`, SSH, 80×24 and a 4K window — a table in the PR |

Disk note (this machine's low space, see the project memory): run the CLI crate's tests alone (`cargo test -p taba-cli`) and `cargo clean -p taba-cli` afterwards; ratatui and crossterm add build time and size.

## 11. Phases

Each phase ends with something usable, tests green and the documentation updated **in all language folders**.

| Phase | Deliverable | Done when |
|---|---|---|
| **0 — Foundation** | Feature `tui`, `taba-cli tui` entry, terminal setup/teardown, event loop, Elm core, workers, frame (6.1), keymap + help (`?`) + palette (`:`), i18n catalog with the two languages, theme/`NO_COLOR`, confirm popup | Empty tabs navigate; language switch works; render and i18n tests pass |
| **1 — Cluster** | 6.2, 6.3 (read-only first with what exists today; the actions after the server change of D-TUI-3/4), `prom.rs`, `history.rs`, node charts (6.4) | Matches `taba-cli health cluster` and `drain *`; a drain and a return done from the TUI on the Compose cluster |
| **2 — Topics** | 6.5, 6.6, topic monitoring and groups (6.4) | Create/edit/delete equal to `topic *` commands; paging over 1 000+ topics is smooth |
| **3 — Tokens** | 6.7 with secret popup and clone | Equal to `token *` commands; secret never visible after closing |
| **4 — Messages** | 6.8 publish, listen, ack, visibility, browse (D-TUI-6) | A message published in the TUI is received in the same TUI and by `taba-cli consume` |
| **5 — Prune** | **Server first:** the client route, the cluster route and the fan-out in `taba-node` (D-TUI-2), with docs 16 and 18, errors, use case and tests; then the TUI action | Prune on a 3-node Compose cluster empties the topic on every node and answers success; with one node stopped it answers failure naming that node |
| **6 — Profiles and clean-up** | Profiles tab (D-TUI-8); remove `tui/term.rs` and `tui/keys`; polish: mouse, ASCII mode, narrow layouts | Old profile TUI code gone; `taba-cli profile` unchanged for the user |
| **7 — Docs and release** | Update [20-cli.md](20-cli.md), [05-dependencies.md](05-dependencies.md), [24-conventions.md](24-conventions.md) (no-dependency rule for the CLI), [21-connectors.md](21-connectors.md) if needed, the CLI `README.md`, build scripts and Dockerfile (feature on), [30-decisions.md](30-decisions.md); screenshots/recordings | `verificar.mjs` and `verificar-idiomas.mjs` pass; binaries for the six targets build |

**Risks.** (1) The static `musl` and `arm64` targets must still build with ratatui/crossterm — check in phase 0 before anything else. (2) Binary size grows; measure against today's `dist/` and record it. (3) Node actions depend on the server change of D-TUI-3/4; until it ships they are disabled with the reason. (4) *Prune* depends on a server change: until it ships, the TUI shows the action disabled; it never claims an emptied queue it cannot prove (a failure leaves the queue possibly partial, and the screen says so). (5) A pruned message must not come back from a replica, from synchronization after a node returns, or from the S3 backup: the server change must define this (see section 12).

## 12. Impact on what exists

| What | Change |
|---|---|
| `connectors/rust/cli/Cargo.toml` | Feature `tui` (default on): `ratatui`, `crossterm`, `rust-i18n`, `sys-locale` |
| `src/app.rs`, `src/commands/mod.rs` | New command `tui`; global `--lang` |
| `src/profile`, `src/settings` | New settings `ui.language` and `[history]` (file); precedence rules of [20-cli.md](20-cli.md) apply (`TABA_CLI_LANG`). **No new profile field** |
| `src/tui/` | The new tree of 4.2; old `term.rs`, `keys` removed in phase 6 |
| Server | ✅ **Done in `taba-node` on 2026-10-15 (D152 to D155 of [30-decisions.md](30-decisions.md); routes in [16-client-api.md](16-client-api.md), [18-cluster-api.md](18-cluster-api.md), errors in [19-errors-and-limits.md](19-errors-and-limits.md); tests in `tests/nodes_api.rs`).** What changed from the plan: the internal call is a new `/cluster/node-call/{path}`, not `/cluster/forward/{path}`; `GET /stats/topics` is the **cluster's** (D156), so the sampler makes one call and does not sum nodes; every consolidated read says `partial` and `missingNodes`, and `GET /cluster/status` carries the load of each node (`runtime`); `GET /cluster/backup` consolidates the backup; `required` is a string (`root` or a permission name); a prune does not cover a syncing node (PRB-058), and a node that left by drain is addressable only while the receiver remembers it (PRB-061). The TUI work itself is still to do. Original requirement: **Required for node actions and monitoring (D-TUI-3/4/9, phase 1):** `GET /nodes`, `GET /auth/me`, `GET /cluster/status`, `GET /stats/topics…` and the `required` field of `403 auth.forbidden`; and, for the actions, the `/nodes/{nodeId}/…` and cluster-wide routes in the client API with the root token (doc 16), the internal forward to the target node (doc 18), errors, use cases and tests (target node down, token without `admin`, drained target). **Required for prune (D-TUI-2, phase 5):** `DELETE /topics/{topic}/messages` in the client API (doc 16), `POST /cluster/topics/{topic}/prune` in the cluster API (doc 18), the fan-out and the local prune (store, ready list, invisible messages, replicas), its `ErrorCode`s, use case and tests, including a node down during the prune and a node that returns afterwards (it must not resurrect pruned messages). Nothing else is needed on the server |
| Documentation | This file; then the files of phase 7, in `pt-BR` and `en-US` together |

## 13. Open questions

| # | Question | Default if not answered |
|---|---|---|
| Q1 | Should a profile be able to hold optional `start-command`/`stop-command` per node (e.g. `docker compose start taba-2`) so the TUI really starts and kills processes? | No — outside the API; D-TUI-3 keeps drain/return only |
| Q2 | Should the history of a chart survive leaving the screen? | **Decided:** yes — the sampler runs for the whole session, 1 h at 5 s by default, configurable (D-TUI-5) |
| Q3 | Entry point: only `taba-cli tui`, or also `taba-cli` with no arguments on a terminal? | Only `tui`; no arguments keeps printing the help |
| Q4 | Export from the TUI (CSV of a chart, a message body to file): keep, or drop to reduce scope? | Keep CSV and body export, both with a path popup |
| Q5 | Mouse support level: wheel and click, or keyboard only? | Wheel and click, optional (`--no-mouse`) |

## Sources and evidence

- The CLI today: `connectors/rust/cli/src/{app.rs,commands,profile,settings,tui}` (own raw-mode terminal, profile screen, `Services` trait for tests) and `connectors/rust/client/src` (blocking `Client`, `testing` transport); [20-cli.md](20-cli.md).
- The routes, parameters and DTOs: [16-client-api.md](16-client-api.md), [17-operations-api.md](17-operations-api.md); the absence of a purge/prune route and of a peek route was checked by searching this folder and the client API on 2026-10-15.
- Metrics, conditions and the "memory-pressure/backlog/stalled/noisy-group" topic conditions: [13-observability.md](13-observability.md).
- Dependency rule being changed: [05-dependencies.md](05-dependencies.md). Library versions: `cargo search` on 2026-10-15 (ratatui 0.30.2, crossterm 0.29.0).
- Nothing in this plan is implemented; there are no tests or use cases for it yet.
