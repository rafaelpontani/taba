---
title: Full-screen interface of taba-cli (TUI)
summary: How to use the full-screen interface of `taba-cli` as it is today — how to open it, the options and configuration, the screens (Cluster, Topics with charts and messages, Tokens, Profiles), every key, what each action asks of the token and what does not exist yet.
type: guide
status: partial
verified-on: 2026-10-16 @ ddb087a
sources: [connectors/rust/cli/src/tui, connectors/rust/cli/src/commands/tui.rs, connectors/rust/cli/src/settings, connectors/rust/cli/locales, connectors/rust/cli/tests, doc/en-US/20-cli.md, doc/en-US/14-security.md]
---

# Full-screen interface of `taba-cli` (TUI)

> **Status: 🟨 partial.** Ready: **Cluster**, **Topics** (list, forms, charts and messages), **Tokens** and **Profiles**. Missing: listening to messages in a loop, repeating a publication, searching and sorting the messages read, and the Configuration and Groups tabs of a topic (see [Not built yet](#not-built-yet)). The one-line commands are in [20-cli.md](20-cli.md).

`taba-cli tui` opens a full-screen terminal interface to **watch and operate** a cluster: see the health of the nodes, drain them, create and delete topics, see the charts of a queue, publish and read messages, manage tokens and switch profiles. It uses the **same profile, the same token and the same routes** as the commands ([20-cli.md](20-cli.md)); it **creates nothing on the server**, and everything it does can also be done with the commands.

## Open

```bash
taba-cli                                  # on a terminal, opens the interface
taba-cli tui --profile staging            # with a specific profile
taba-cli tui --lang en-US --history-retention 30m
```

| Option | Variable | Default | What it does |
|---|---|---|---|
| `--profile <name>` | `TABA_CLI_PROFILE` | the default profile | Which profile to use. It fixes the choice: the profile picker does not open |
| `--lang <code>` | `TABA_CLI_LANG` | the system's | Language: `en-US` or `pt-BR` |
| `--no-mouse` | — | mouse on | Does not capture the mouse (the scroll wheel) |
| `--history-retention <time>` | — | `1h` (at most `6h`) | How much chart history to keep |
| `--history-interval <time>` | — | `5s` (at least `1s`) | How often to measure for the charts |
| — | `NO_COLOR` | — | When set and not empty, takes the colors away |
| — | `TABA_CLI_ASCII` | — | With `1`, swaps icons and bars for ASCII (also applies when the locale is not UTF-8) |

- **It needs a terminal** (input and output). Outside one, `taba-cli tui` prints the help and exits with `2`; `taba-cli` with no command does the same.
- **Minimum size: 60 columns by 16 lines.** Below that a notice replaces the screen; resizing brings it back by itself. The interface is designed for 80×24 or more.
- The interface is the `tui` Cargo feature, **on by default** in the binaries and in the Docker image. A build with `--no-default-features` does not have it and `tui` answers with a usage error.
- The direct commands (`topic list`, `publish`…) **never** put the terminal in raw mode: only `tui` does.

## Configure

The interface reads the same `config.yml` as the profiles ([20-cli.md](20-cli.md#profiles)). Besides the profiles, it keeps two sections that are only its own:

```yaml
ui:
  language: en-US
history:
  retention: 1h
  interval: 5s
```

| Key | Written by | Default | What it does |
|---|---|---|---|
| `ui.language` | the `l` key | the system's language | The chosen language, remembered for the next time |
| `history.retention` | you | `1h` | How much chart history to keep (`90`, `90s`, `5m`, `1h`; at most `6h`) |
| `history.interval` | you | `5s` | The interval between measurements (minimum `1s`) |

- **Language**, the first that exists: `--lang` → `TABA_CLI_LANG` → `ui.language` → the system's language (`LC_ALL`, `LC_MESSAGES`, `LANG`; on Windows, the interface language) → `en-US`. `pt`, `pt_BR` and `pt-BR.UTF-8` count as `pt-BR`. The `l` key switches at once and writes `ui.language`. Labels, hints and notices are translated; keys, routes, JSON fields, error codes and condition names are **not**, and the server's `message` appears as it sent it.
- **History:** the measurements are good **only during the session** (memory; nothing goes to disk). A chart opened early says how many minutes there are. For long history use Prometheus ([13-observability.md](13-observability.md)). The command-line option wins over the key in the file.
- **Which profile opens:** `--profile`, `TABA_CLI_PROFILE` or `TABA_CLI_URL` fix the choice. With none of them and **more than one** profile, a box opens to choose, with the default profile (●) already selected: `Enter` connects and `Esc` takes the default. With **one** profile it takes it without asking. With **no profile**, it opens the form of the first one (already marked as the default); `Esc` skips it and uses the built-in address (`http://127.0.0.1:8080`, no token). Nothing is read from the server before the choice. The `o` key opens the same box on any screen, and there `Esc` only closes it.
- **The token** comes from the profile (`token-file` or `token`). Nearly everything the interface shows and does about the cluster needs the **root token** or `admin` (table below); with an ordinary token it shows the list of nodes and says what is missing.

### What each action asks of the token

The interface enables or disables each action **before** calling, and an action that cannot run appears dimmed **with the reason**; the one who decides is always the server ([14-security.md](14-security.md)). `manage` and `admin` **include** `create`, `publish` and `consume`.

| Action | Minimum token |
|---|---|
| See the list of nodes, the topics and the statistics (within the token's prefixes) | any valid token |
| Create a topic | `create` |
| Edit, delete or **empty** a topic | `manage` over the topic |
| Publish a message | `publish` |
| Read, acknowledge and give back messages | `consume` |
| The Tokens tab; conditions, detail, charts, drain, return and backup of nodes | `admin` (or the root token) |
| Stop the cluster; create, change or revoke an `admin` token | the root token |

## The screen

```text
 taba-cli · local · http://127.0.0.1:8080        ● healthy          14:32 UTC
 [1 Cluster]  2 Topics   3 Tokens   4 Profiles
┌──────────────────────────────────────────────────────────────────────────┐
│                       the content of the focused tab                     │
└──────────────────────────────────────────────────────────────────────────┘
 Enter actions · n new · / filter · h help · q quit              ⟳ 2s  en-US
```

*(Illustrative: the real drawing changes with the tab and the width.)*

- **Status line:** the profile, the URL, the cluster's **worst condition** (always a word **and** an icon, never color alone) and the clock (UTC). When a node did not answer the read, the badge **incomplete: node X did not answer** appears; the interface never adds nodes up on its own.
- **Tabs:** Cluster, Topics, Tokens and Profiles (`1` to `4`). **There is no messages tab**: a message always belongs to a topic, so messages are opened from the Topics tab.
- **Footer:** the keys of the focused screen, the refresh state (`⟳ 2s`, paused, or **stale** with the time of the last good data) and the language. At 80 columns some hints are cut off; `h` shows them all.
- **Offline, `401`, `403`, `429` or `503`:** the frame shows `✖ offline` (or what is missing) **without erasing the last good data**, tries again at growing intervals (up to 30 s) and obeys `Retry-After`. It never hangs.
- **Scrolling:** everything scrolls (arrows, `PgUp`/`PgDn`, `Home`/`End` and the mouse wheel); the scrollbar only shows when the content overflows. The mouse is optional: **every action has a key**. Only the wheel is used (clicking does nothing).
- **Refresh:** Cluster every 2 s; the topic and token lists every 5–10 s; paused while a form or box is open.

## Keys

Arrows and Vim keys do the same (`↑↓` and `k j`). The letters are mnemonic and **the same in both languages**. A lowercase letter is the ordinary action; uppercase is the critical or the "all" one. `h` (or `?`, `F1`) shows the full list for the focused screen, and `:` opens the **command palette**, which finds any action by name (in the current language or in English) and runs it.

| Keys | Action |
|---|---|
| `1` to `4` · `[` `]` · `Alt+←` `Alt+→` | Go to a tab · previous / next |
| `Tab` · `Shift+Tab` | Next / previous panel |
| `↑ ↓` `k j` · `PgUp PgDn` `Ctrl+u Ctrl+d` · `Home End` | Move · scroll · first / last (`g G` too, where `g` is not another action) |
| `Enter` | On a list: opens the **action menu** of the selected item (inside a box: confirms) |
| `Esc` | Closes or cancels; goes up a level (from the full screen of a chart to the list of charts, then to the topic) |
| `/` · `s` `S` | Filter by part of the name · sort / reverse |
| `r` · `p` | Refresh now · pause or resume the refresh |
| `l` · `o` | Language · switch profile |
| `h` `?` `F1` · `:` | Help · command palette |
| `Ctrl+s` | Send the open form |
| `q` `Ctrl+c` | Quit (asks if a form was changed) |

| Screen | Own keys |
|---|---|
| **Cluster** | `Enter` actions of the node · `i` detail · `d` drain · `u` return · `b` backup flush of this node · `B` flush of all · `S` stop the cluster · `m` charts of the cluster |
| **Topics** | `Enter` actions of the topic · `n` new · `e` edit · `Del` `d` delete · `v` show or hide the dead-letter queues · `i` summary · `g` charts · `x` messages |
| **Messages** (of a topic) | `Enter` open the message · `p` publish · `r` read · `a` acknowledge (ack) · `A` acknowledge all · `v` give back to the queue · `P` empty the queue · `Esc` back to the list |
| **Charts** (node or topic) | `↑ ↓ Home End` choose the chart · `Enter` full screen · `+` `-` window · `Space` freeze · `z` scale · `e` export CSV |
| **Tokens** | `Enter` actions of the token · `n` new · `e` edit · `c` clone · `Del` `x` revoke |
| **Profiles** | `Enter` actions of the profile · `c` connect · `n` new · `e` edit · `d` make default · `Del` `x` remove · `t` test the connection |

**Forms.** `Tab` and `Shift+Tab` (or `↑ ↓` between fields) move through the fields, `Space` ticks a box, `←` `→` change an option, `Ctrl+s` sends and `Esc` cancels. The server's error (`code: message`) appears **inside** the form, which stays open. A block paste works in any text field (and so does AltGr, like `/` and `@` on an ABNT2 keyboard).

**Confirmations.** An action that deletes or interrupts asks for confirmation and the default button is **Cancel**. For *stop the cluster*, *delete a topic* and *empty the queue* the person **types the name** (of the cluster or of the topic).

### The action menu

`Enter` on a row of Topics, Tokens, Profiles or the list of nodes opens a menu of what can be done **with that item**, with the name of the target in each entry (for example *Edit the topic*, *Delete the topic*, *Publish a message*, *Read messages*) so a topic is not confused with a message. `↑ ↓` choose, `Enter` runs, `Esc` closes. An entry that cannot run now appears dimmed, and the line under the menu gives the reason. The shortcut keys of each screen still work straight from the list.

| Item | Menu entries |
|---|---|
| Topic | Summary · Charts · Publish a message · Read messages · Empty the queue · Edit · Delete |
| Node | Detail · Charts · Drain · Return · Flush this node · Flush all · Stop the cluster |
| Token | Edit · Clone · Revoke |
| Profile | Connect · Edit · Make default · Test · Remove |

## Cluster

The Cluster tab shows, from top to bottom, the active **conditions** of the cluster (the worst first, with icon and word; with none, a "healthy" line; `Enter` on a condition explains what it means and what to do), the **table of nodes** and, below, **three charts of the selected node** (memory, messages in memory and requests in flight). `Tab` switches between the conditions and the nodes.

- **Table of nodes:** id (start…end, because the ids of nodes created together begin alike), state, drain, memory as a bar with %, messages on disk, blocks being synchronized and requests in flight; from 100 columns, also free disk and `429` rejections. An unreachable node turns red and says so; a node with no memory limit (development mode) shows "no limit".
- **Node detail** (`i`): id, state, drain, version, mode, start and uptime; memory against the limit (yellow at 80 %, red at 95 %); messages in memory and on disk; the node's signals (channels, quorum, blocks being synchronized, below replication, frozen nodes); and the **S3 backup**, only when configured (reachable, pending, last success, segments).
- **Drain** (`d`): shows what happens and follows `pendingMessages` until `drained`. A node in drain or drained offers **return** (`u`); it comes back as a new node. A **single-node** cluster in drain refuses everything, including its own return (`503 cluster.node-draining`): a server limit (PRB-061).
- **Backup flush** (`b` one node, `B` all): shows a report per node (complete or not, pending, timed out). Without S3 configured the server answers `422 backup.not-configured` and the interface says so.
- **Stop the cluster** (`S`): asks for the **cluster name typed**. The nodes become `drained` and stay running as processes; stopping them is up to the orchestrator. It needs the operations API on its own port; without it the key appears dimmed, with the reason.
- **Charts of the cluster** (`m`): a **list of six indicators on the left** (memory against the limit, ready messages, messages on disk, requests in flight, `429` rejections, channel RTT), each with its current value; the chart of the indicator under the cursor is drawn beside it. `↑ ↓` choose, `Enter` gives it the **full screen** and `Esc` goes back (first from the full screen, then from the charts).
- **With an ordinary token** the tab shows only the list of nodes and replaces the rest with a notice that the token must be `admin` or the root.

## Topics

The list shows the name, the type (`queue`, `fifo queue`, `volatile queue`, `distributor`, `dead-letter`), the depth, the publish and consume rates, the age of the oldest and the state (icon and the worst condition, untranslated). Dead-letter queues stay hidden until `v` and, when shown, appear dimmed and with no limits in the detail, since they follow the source queue; a distributor shows "—" where there is no queue. `/` filters by part of the name and `s` sorts (name, newest, oldest, depth). The list reads pages of 200, up to 5 000 topics. If the statistics fail, the table shows the topics without numbers and says "numbers unavailable".

Under the table sit **three charts of the selected queue**, side by side (messages, flow and age), with the current values in the title; under a distributor the summary appears. They use the same reading as the topic charts, one per interval and only while the list is shown.

- **Create** (`n`): name, type (queue or distributor), FIFO, volatile, idle timeout, maximum message bytes, ttl, maximum deliveries, tags (`key=value`) and prefixes. Fields that do not apply to the type are locked. Validation happens on send; the rest is the server's.
- **Edit** (`e`): type, FIFO and volatile appear **locked** with the reason ("does not change later"). Only what changed goes to the server, and the three limits accept a number or `default` (back to the node's default).
- **Delete** (`Del` or `d`): asks for the topic name typed and warns that the messages (and the dead-letter queue) are lost.
- **Summary** (`i`): in a box, the name, the id, the creation, the tags and the limits.
- **Charts of the topic** (`g`): a screen headed `Topics › name · Charts`. On the left, the list of three indicators, each with its current value; beside it, the chart of the one under the cursor; `Enter` gives it the full screen.
  - **Messages:** visible and invisible.
  - **Flow** per second: in (published), out (delivered) and deleted (ack).
  - **Age** of the oldest in the queue and of the **three largest groups** (the noisy groups).
- A distributor has no queue: it has no queue charts and the keys that depend on one are refused with the reason.

## Messages of a topic

`x` on the list (or *Read messages* and *Publish a message* of the menu) opens the screen `Topics › name · Messages`. At the top are the queue counters (visible, invisible, depth, the oldest); below, the table of what **you read** on this screen.

> **The API does not list the messages of a queue.** The table shows only what the interface read on request, and the counters at the top are the truth about the queue. **Opening the screen, choosing a topic or refreshing never reads the queue:** reading is *delivering* (it counts a delivery and makes the message invisible to other consumers), so it only happens when you ask.

- **Read** (`r`): opens a form with **how many messages** (default 10) and the **invisibility time** (default 30 s); `Enter` moves from one field to the next and sends on the last. Each reading **replaces the list** with what it brought (the first row is selected; if nothing came, the list is empty and a line says so). Rows from an earlier reading disappear, and a still-held message from them can no longer be acknowledged from here (the receipt was in the row): it goes back to the queue when the time runs out.
- **The table:** time, id, group, deliveries, state (`invisible 27s`, `visible again`, `deleted`), size and a snippet of the body.
- **Open a message** (`Enter`): a box with the state, the group, the type, the size, the deliveries, the time of reading and the body. While the hold lasts, the remaining seconds appear live and there are two actions: `v` **changes the invisibility time** (a form with 30 s; `0` gives the message back to the queue) and `d` (or `Del`, or `a`) **deletes** it (ack). When the time **runs out** with the message open, the box warns that the receipt no longer works and that the message **can no longer be changed or deleted from here** (the queue delivers it again), and those keys are dimmed. `Esc`, `Enter` or `q` close the box.
- **How the body is shown:** by default, **exactly as it was sent** (the original bytes may be needed). `f` shows it **formatted** when it is valid JSON (re-indented with two spaces) or simple YAML (maps and texts; lists and other YAML stay as sent); a body that is neither says so and is shown as sent. `b` shows the bytes **as base64** (lines of 76). Pressing the key in use goes back to *as sent*, and the border of the body names the way in use. Each row keeps the first 16 KiB of the body for these views; a larger body says it was cut.
- **Publish** (`p`): the body is a **multi-line editor** (numbered lines; `Enter` breaks the line, arrows move, `↑` on the first line and `↓` on the last go to the neighboring field, a paste keeps the lines), the content type, the **Body is base64** box, the group id and the deduplication id (a FIFO queue uses both). With the box ticked, the typed text is read as base64 and what is sent is the **decoded bytes**; text that is not base64 is refused in the form. `Ctrl+s` sends, and the result (the message id, or the refusal) stays on a line under the table.
- **Acknowledge and give back:** `a` acknowledges (deletes) the selected message, `A` all that are still held, `v` gives the selected one back to the queue. The receipt is only good while the message is held.
- **Empty the queue** (`P`): asks for the topic name typed, says how many messages are lost and that **every node** will be told. At the end it shows how many were removed, or the failure `503 cluster.prune-incomplete` with the nodes that did not finish (repeating is safe). Messages published during the emptying are not covered.
- The keys are refused, with the reason, on a **distributor** and on a **dead-letter** queue (nothing is published to it) and when the token lacks the permission (`publish`, `consume` or `manage`).

## Tokens

A table with name, id, permissions, prefixes and expiry; the **secret is never in the list**. An expired token appears in red and one that expires within 7 days, in yellow. `/` filters by part of the name and `s` sorts (newest, oldest, name, id).

- **Create** (`n`): name, the permissions (`publish`, `consume`, `create`, `manage`, `admin`), prefixes and expiry. `admin` has no prefixes and only the root token can tick it. The expiry accepts empty or `none` (no deadline), `30d` `12h` `90m` (from now), a date (`2027-01-31`, start of the day in UTC) or an RFC 3339 instant.
- **The secret appears once**, in a popup: `c` copies it (OSC 52, if the terminal allows) and `s` writes it to `taba-token-<id>.txt` in the current folder (`0600` on Unix, never over an existing file). After 30 s, or when the window loses focus, it is hidden (dots), and `c` still copies it. Closing without copying or saving asks for a second `Esc`. It never goes to a log, an error or the footer notice.
- **Edit** (`e`): only the name and the expiry; permissions and prefixes appear locked with the reason (to change them, **clone**).
- **Clone** (`c`): opens *Create* already filled. In the secret popup, the box **Revoke the old one on close** (`r`) starts **unticked**.
- **Revoke** (`Del` or `x`): a confirmation that starts on **Cancel**; it applies on every node.
- Without the root token or `admin`, the tab explains the `403` and which token is in use, instead of showing an empty list; without authentication on the server, it says there are no tokens.

## Profiles

The list shows the profiles of `config.yml`, with the default (●) and the one in use. `Enter` opens the menu; `c` **connects**: the next calls go to the profile's server, the histories and the screens start over, and a reply still coming from the old server is discarded.

- **New** (`n`) and **edit** (`e`): name, `url`, `ops-url`, `token-file`, `token`, `ca-file` and the default-profile box; the other fields of the profile (`output` and the timeouts) are kept as they are. `Ctrl+t` tests the values **not yet saved**.
- **Default** (`d`), **remove** (`Del` or `x`, with a confirmation on **Cancel**) and **test** (`t`, which does `GET /topics?limit=1`).
- Saving only changes `default` and `profiles` in the file: `ui` and `history` stay as they are. The file is rewritten whole and **comments made by hand are not preserved**.
- A token typed in the form goes to the file **as typed** (`config.yml` is not encrypted) and the screen never draws it (it shows as dots); the form recommends the `token-file`.
- The command `taba-cli profile`, with no subcommand, still opens the old profile screen ([20-cli.md](20-cli.md#profiles)); the Profiles tab replaces it.

## Charts: window, scale and export

They apply to the charts of the node, of the cluster and of the topic.

| Key | What it does |
|---|---|
| `+` · `-` | Larger · smaller window (1, 5, 15, 30 or 60 min, never above the retention) |
| `Space` | Freezes the chart (and resumes) |
| `z` | Automatic scale, or from zero |
| `e` | Exports the visible series as CSV: opens a popup with the file name (`taba-<node>-<series>.csv` or `taba-<topic>-charts.csv`) and **never writes over an existing file** |
| `Enter` · `Esc` | Full screen · leave it |

Each line has a name in the legend **and** its own marker, not just color. The title shows the current value. Rates come from counters: a counter that goes down (a restarted node) is a restart, never a negative rate. A partial reading (`partial`) is kept marked as such. The history is only what was sampled since the interface opened, and topic numbers come from the reading **of one node** when the server returns only that (PRB-059), so cluster totals may be below the real ones.

## Not built yet

| What | Situation |
|---|---|
| **Listening** to a queue (long polling with auto-ack) | Not done: today you only read on request (`r`). For a continuous loop use `taba-cli subscribe` |
| **Repeating** a publication *n* times | Not done: use `taba-cli publish --count` |
| **Searching** and **sorting** the messages read, and the charts of the selection | Not done |
| The **Configuration** and **Groups** tabs of a topic | Not done; the noisy groups appear in the age chart |
| Filtering topics by **tag** and by **condition** | Not done; only the name |
| Mouse click; a narrow layout designed for under 80 columns | Only the mouse wheel; under 80 columns some hints are cut off |
| **Starting** or **killing** the process of a node | Out of scope: it belongs to the orchestrator |

## Common problems

| Symptom | Cause and way out |
|---|---|
| `taba-cli tui` prints the help and exits with `2` | There is no terminal (input or output redirected); run it on a terminal |
| "I need at least 60 columns by 16 lines" | Enlarge the window; the screen comes back by itself |
| The Cluster tab only lists the nodes and asks for the root token | The profile's token is neither `admin` nor root; use `o` for another profile |
| A key appears dimmed with a reason on the line below | The token lacks the permission, the node is not in a state that accepts the action, or the operations port is missing |
| Strange symbols instead of icons and bars | `TABA_CLI_ASCII=1` |
| No colors | `NO_COLOR` is set |
| "incomplete: node X did not answer" | A node did not answer in 5 s; the numbers are those of the other nodes |
| The open message says it can no longer be changed | The invisibility time ran out; read again (`r`) |
| Timeouts and network failures | The footer shows `stale` and the time of the last good data; the interface tries again by itself |

## Sources and evidence

- Code: `connectors/rust/cli/src/tui` (one module per folder, with `tests.rs` beside it), `src/commands/tui.rs`, `src/settings` and the language catalogs in `connectors/rust/cli/locales/{en-US,pt-BR}.yml` (a test checks that both have the same keys).
- Tests: `cargo test -p taba-cli` (unit tests of the core, screen snapshots at 80×24 and 120×40 in both languages in `tests/snapshots/`, `tests/commands.rs`, `tests/packaging.rs` and, optionally, `tests/live_node.rs` against a real node with `TABA_LIVE_URL`). The interface has not yet been exercised end to end on a real terminal against a cluster on Compose.
- The permissions: [14-security.md](14-security.md). The routes: [16-client-api.md](16-client-api.md) and [17-operations-api.md](17-operations-api.md). The commands and the profiles: [20-cli.md](20-cli.md).
