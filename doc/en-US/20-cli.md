---
title: Command lines (`taba` and `taba-cli`)
summary: The project's two CLIs — `taba` (starts the node and creates secrets) and `taba-cli` (client: topics, messages, tokens, health, drain; profiles, output, exit codes, TUI, image and build).
type: reference
status: stable
verified-on: 2026-10-13 @ 71183b9
sources: [crates/taba-node/src/cli/mod.rs, crates/taba-node/src/main.rs, crates/taba-node/src/secrets, connectors/rust/cli/src, connectors/rust/cli/Dockerfile, connectors/rust/cli/scripts]
---

# Command lines

> **Do not confuse them:** there are **two** CLIs, with opposite roles.

| | **`taba`** | **`taba-cli`** |
|---|---|---|
| What it is | The **server**: the same binary starts the node | The **client**: talks to the API |
| Where it is | `crates/taba-node` (server binary) | `connectors/rust/cli` (on top of the Rust connector) |
| What it does | Starts a node; creates the secrets folder (`init`), certificates (`cert generate`) and root tokens (`token generate`) | All the commands of the client and operations APIs |
| Talks to the API? | **No** | **Yes**; it does **not** start a node nor touch the server's disk |
| Dependencies | tokio, axum, SQLite… ([05](05-dependencies.md)) | Only the connector (`rustls`) |

---

## `taba` (server)

```text
Usage: taba [--config <file.yml>]
       taba init --out <folder> [--days <n>]
       taba cert generate --days <n> [--starts-in <duration>] --out <folder>
       taba token generate
```

(The usage text printed by the binary is in Portuguese today.)

| Command | Does |
|---|---|
| `taba` (no subcommand) | **Starts the node.** Reads the configuration ([22-configuration.md](22-configuration.md)) and waits for the stop signal |
| `--config <file>` | YAML file (or `TABA_CONFIG_FILE`). Without it, uses `/etc/taba/taba.yml` **if it exists** |
| `taba init --out <folder> [--days <n>]` | Creates the **secrets folder**: the first cluster certificate (in `tls/`) and the root token (`root-token`). `--days`: validity, default **1825** |
| `taba cert generate --days <n> [--starts-in <d>] --out <folder>` | Creates **only** a certificate pair (rotation). `--days` is **required** here. `--starts-in` (`30m`, `14d`) postpones the start |
| `taba token generate` | Prints a random 256-bit secret (a standalone root token) |
| `--version`, `--help` | — |

The parameters also come from the `TABA_*` variables; **the variable wins over the file**. An unknown key in the `.yml` is **refused** (a typo does not pass silently). `main.rs` is only the command line and the signals; `SIGTERM` and `SIGINT` trigger the **drain** ([07-cluster.md](07-cluster.md)).

```bash
taba init --out ./secrets          # cluster certificate + root token
TABA_MODE=development TABA_STORAGE_DIR=/tmp/taba-dev/taba taba   # one node to test
```

The security of the artifacts created is in [14-security.md](14-security.md).

---

## `taba-cli` (client)

A small program that, from any terminal, does everything the API offers. It has **two roles**: **test and operate** a cluster without writing code, and be the **executable reference of the contract**: each command is a call of [16](16-client-api.md) or [17](17-operations-api.md), and what it does is what the other connectors must repeat.

- **Stateless:** each run makes one or more calls and ends. The connection lives in the **profile**.
- **No command-line library** (own argument reading) and no system dependencies: a **static binary** per platform.
- `taba-cli --version` shows the program version **and** the API contract version (`taba-cli 0.1.0 (contrato da API 1)`).

### Options of all commands

| Option | Variable | Default | Description |
|---|---|---|---|
| `--profile <name>` | `TABA_CLI_PROFILE` | the default profile | Which profile to use. A nonexistent name exits with `2` |
| `-o, --output <format>` | `TABA_CLI_OUTPUT` | `json` | `json` (the response as the server gave it, indented), `table` (columns) or `raw` (only what the command delivers, e.g. the message body) |
| `-q, --quiet` | — | — | Prints nothing on success |
| `--request-id <id>` | — | generated | Goes in `X-Request-Id` |
| `--help`, `--version` | — | — | — |

There are no `--url` or `--token` in the commands: the connection is chosen by the **profile** (`--profile`) or by the `TABA_CLI_*` variables.

### Commands

**Topics** ([16](16-client-api.md)):

| Command | Route | Options |
|---|---|---|
| `topic create <name>` | `POST /topics` | `--type queue\|distributor` (**required**), `--tag k=v` (repeatable), `--prefix p` (repeatable), `--volatile`, `--idle-timeout s`, `--fifo`, `--max-message-bytes n`, `--ttl s`, `--max-deliveries n` |
| `topic list` | `GET /topics` | `--name`, `--sort`, `--limit`, `--cursor`, `--all` (walks all pages) |
| `topic get <name>` | `GET /topics/{topic}` | — |
| `topic update <name>` | `PATCH` | `--tag` (replaces the map) or `--clear-tags`, `--prefix`/`--clear-prefixes`, `--idle-timeout`, `--max-message-bytes`/`--ttl`/`--max-deliveries` (`n` or `default`). Only what was given is sent |
| `topic delete <name>` | `DELETE` | `--yes` (without it, on a terminal, asks for confirmation; off a terminal, refuses with `2`). Removes **all messages** |

**Messages:**

| Command | Route | Options |
|---|---|---|
| `publish <topic> [body]` | `POST …/messages` | The body comes from **one** of: the argument, `--file`, `--stdin`. `--content-type`, shortcuts `--json`/`--text`, `--group-id`, `--deduplication-id`, `--id-only`, `--count n` (repeats the publication n times: light load test). **It does not validate the body** (the server answers `validation.invalid-body`). After `502`/`504` outside FIFO it **does not repeat on its own** |
| `consume <topic>` | `GET …/messages` | `--max`, `--visibility`, `--wait` (up to 20), `--auto-create` (+ `--persistent`, `--idle-timeout`, `--fifo`, `--tag`; they only apply with `--auto-create`), `--ack` (confirms each message right after printing it), `--exit-empty` (code `6` if empty). With `-o raw`, only the `body`, one per line |
| `subscribe <topic>` | `GET` in a loop + `DELETE` | Continuous long polling (`--wait`, default 20), one message at a time (`--max`, default 1). `--exec <command>` runs the command with the body on standard input and **confirms only if it exits with `0`**. An ack that arrives late (`409`/`404`) is ignored. `--limit n`; `Ctrl+C` ends |
| `ack <topic> <id>` | `DELETE` | `--receipt <receiptToken>` (required) |
| `visibility <topic> <id>` | `PUT …/visibility` | `--receipt` and `--seconds` (required; `0` returns it immediately) |

The `receiptToken` changes at each delivery and comes in the `Message`: that is why `consume` **without `--ack`** prints the **whole** message.

**Tokens** (root token only, and only with authentication on):

`token create` (`--name`, `--permission` repeatable or comma-separated, `--prefix`, `--expires-at`, `--token-only`), `token list`, `token get <id>`, `token update <id>` (`--name`, `--expires-at <date>|none`), `token revoke <id>` (`--yes`).

**Operations** (uses the profile's `ops-url`; no token):

| Command | Route |
|---|---|
| `health live` / `health ready` | `GET /health/live`, `/health/ready` (code `0` if `200`, `1` if not; prints the `reason`) |
| `health cluster [--scope cluster\|node]` | `GET /health/cluster` (with `-o table`: one row per node and the conditions) |
| `metrics [--openmetrics]` | `GET /metrics` (this node only; the text comes out as it came) |
| `stats topics`, `stats topic <name>`, `stats groups <topic>` | `GET /metrics/topics*` (`--scope`, `--tag`, `--condition`, `--fields`, `--group-by`, `--limit`, `--cursor`, `--all`) |
| `drain start`, `drain status`, `drain cancel` | `POST/GET/DELETE /node/drain` — **the node of the `ops-url`** |

**Local** (do not call the server): `version`, `config show` (the effective configuration and **where each value came from**, without the token), `profile …`, `help [command]`.

### Profiles

A **profile** is a named set of connection options, to switch between clusters (local, staging, production) with one word.

**File:** a single YAML in the user's configuration folder, created on the first `profile add` (`TABA_CLI_CONFIG` changes the path). On Unix, permission `0600`.

| System | File |
|---|---|
| Windows | `%APPDATA%\taba\config.yml` |
| Linux | `$XDG_CONFIG_HOME/taba/config.yml` or `~/.config/taba/config.yml` |
| macOS | `~/Library/Application Support/taba/config.yml` |

```yaml
default: local
profiles:
  local:
    url: http://127.0.0.1:8080
    token-file: C:\taba\secrets\root-token
  homologacao:
    url: https://taba.hml.example.com
    ops-url: https://taba-ops.hml.example.com
    ca-file: /etc/ssl/taba-hml-ca.pem
```

**Without configuration** the built-in default applies: `http://127.0.0.1:8080`, no token (enough for a local node in development mode).

| Field | Default | Description |
|---|---|---|
| `url` | `http://127.0.0.1:8080` | The **client API** (the client port or the load balancer) |
| `ops-url` | the `url` itself | The **operations API**: only `health`, `metrics`, `stats`, `drain`. With `operations.port`, set it |
| `token-file` | — | Reads the token from the first line of the file. It is the way to use the `root-token` |
| `token` | — | The token in clear text. **Prefer `token-file`** |
| `ca-file` | system roots | CA (PEM) to trust. There is **no** field to turn off TLS verification |
| `output` | `json` | Default format |
| `connect-timeout` / `timeout` / `max-attempts` | `5` s / `30` s / `3` | Open the connection / ordinary call (`consume` adds the `waitTime`) / attempts, counting the first |

**Precedence of each field:** command option → **`TABA_CLI_<FIELD>` variable** → chosen profile → built-in value. **Which profile:** `--profile`, then `TABA_CLI_PROFILE`, then the file's `default`.

**Profile commands:** `profile list`, `show [name]`, `add <name> [fields] [--default]`, `set <name> [fields] [--unset field]`, `remove <name> [--yes]`, `default [name|--none]`, `path`, `session …`.

**Interactive mode (TUI).** `taba-cli profile` with no subcommand, **on a terminal**, opens a profile screen navigated **with the arrow keys, like a form** (list, add, change, remove, choose the default). It is a screen over the same commands and writes the same file.

```text
 Perfis do taba-cli   /home/ana/.config/taba/config.yml

   nome         url                          token-file   padrão
 ▶ local        http://127.0.0.1:8080        root-token   ●
   homologacao  https://taba.hml.exemplo.com  —            ○

 ↑↓ escolher   Enter abrir   n novo   d padrão   Del remover   q sair
```

```text
 Perfil  local

 ▸ Nome              local
   url               http://127.0.0.1:8080
   ops-url           (http://127.0.0.1:8080)
   token-file        root-token
   token             (nenhum)
   ca-file           (raízes do sistema)
   output            ‹ (json) ›
   connect-timeout   (5)
   timeout           (30)
   max-attempts      (3)
   Perfil padrão     [x]

   [ Salvar ]  [ Testar conexão ]  [ Cancelar ]

 ↑↓/Tab campo   ←→ opção   Espaço marcar   Ctrl+S salvar   Esc cancelar
```

*(The screens above are illustrative and show the program's current Portuguese text, which comes from `connectors/rust/cli/src/tui`; the values are examples.)*

- **Validation on the spot:** a URL without `http://`/`https://`, a number out of range or a repeated name turn red and **Save** does not write.
- **Token in view:** `token` and `token-file` are shown and edited in clear (the file is **not encrypted**). `profile show` and `config show` do not show the token.
- **Test connection:** does `GET /topics?limit=1` with the values **not yet saved**.
- No library: ANSI and the terminal's **raw mode** (`stty` on Linux/macOS; `SetConsoleMode` on Windows 10+). `NO_COLOR` removes the colors. A terminal smaller than 60×16 shows a warning.
- **Off a terminal** (redirected input or output), `profile` does not open the screen: it prints the help and exits with `2`.

**Session profile (temporary).** A child program does not change the terminal's environment, so the command **prints** the lines the terminal must run:

```bash
eval "$(taba-cli profile session homologacao)"                                  # a saved profile
eval "$(taba-cli profile session --url http://10.0.0.5:8080 --token-file ./tok)" # temporary profile
eval "$(taba-cli profile session --clear)"                                       # back to the default
taba-cli profile session --url … --shell powershell | Invoke-Expression          # PowerShell
```

**The file belongs to the CLI:** it reads the YAML subset that it writes itself and **rewrites the whole file** at each change (through a temporary file and rename). Comments made by hand are **not preserved**.

### Output and exit codes

Success: the response goes to **standard output** (`204`s print nothing). API error: goes to **standard error**, as `code: message (requestId …)` plus the `details`; the `Retry-After` of `429` and `503` appears on a separate line.

| Code | When |
|---|---|
| `0` | Success (in `consume`, including without a message) |
| `1` | The API answered `4xx` or `5xx` |
| `2` | Wrong usage: unknown command or option, missing argument, nonexistent profile |
| `3` | Did not talk to the server (connection refused, DNS, TLS) |
| `4` | Timed out |
| `5` | Unreadable file (`--file`, `token-file`, `ca-file` or the `config.yml` itself) |
| `6` | `consume --exit-empty` and the response came empty |

### Examples (verified)

```bash
taba-cli topic create pedidos --type queue --fifo --tag time=financeiro --ttl 86400
echo '{"pedido": 9001}' | taba-cli publish pedidos --stdin --group-id cliente-42 --deduplication-id pedido-9001
taba-cli consume pedidos --max 5 --wait 20 --visibility 60
taba-cli consume pedidos --max 1 --ack -o raw
taba-cli ack pedidos <id> --receipt <receiptToken>
taba-cli subscribe pedidos --exec ./processa.sh
taba-cli health cluster -o table
taba-cli drain status
```

### Docker image and binaries

- **Image** (`connectors/rust/cli/Dockerfile`): minimal, **Alpine**, with only the `taba-cli` binary (static, musl), unprivileged user, `ENTRYPOINT` = `taba-cli`. Two stages; `docker buildx build --platform linux/amd64,linux/arm64`. In the root Compose it is the `taba-cli` service under the `tools` *profile*, already pointed at the nodes (`TABA_CLI_URL`, `TABA_CLI_OPS_URL`, `TABA_CLI_TOKEN_FILE`).
- **Binaries** (`scripts/build.sh` and `build.ps1`): six targets — Windows (x64, arm64), static Linux musl (x64, arm64) and macOS (x64, arm64). Without an argument, builds for the machine; `--target <target>` and `--all` (those the machine can). Output in `connectors/rust/cli/dist/<target>/` with `SHA256SUMS`; `--locked`, no symbols, size-optimized. Cross-building to macOS from another system is not supported by the scripts (the CI matrix uses one *runner* of each).
- **Build locally:** `cd connectors/rust && cargo build -p taba-cli`.

> **On Windows with Git Bash:** commands like `docker compose run … --entrypoint cat taba-cli /secrets/root-token` have the path converted; use `MSYS_NO_PATHCONV=1`.

## Sources and evidence

- **`taba`:** `crates/taba-node/src/cli/mod.rs` (`USAGE` and `parse`), `main.rs`, `secrets/`.
- **`taba-cli`:** `connectors/rust/cli/src/{args,commands,profile,settings,output,tui,yaml}`; the command list and the options were checked against `taba-cli --help` and `taba-cli <command> --help` of the compiled binary.
- Use cases: UC-CLI (36 cases, level `K`), UC-SEG; see [26a](26a-implemented-use-cases.md). Tests: `connectors/rust/cli/src/**/tests.rs`, `connectors/rust/cli/tests/commands.rs` and `packaging.rs`.
- History: `19b-cli.md`.
