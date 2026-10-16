---
título: Linhas de comando (`taba` e `taba-cli`)
resumo: As duas CLIs do projeto — o `taba` (sobe o nó e cria segredos) e o `taba-cli` (cliente — tópicos, mensagens, tokens, saúde, drain; perfis, saída, códigos de saída, TUI, imagem e build).
tipo: referência
status: estável
verificado-em: 2026-10-16 @ ddb087a
fontes: [crates/taba-node/src/cli/mod.rs, crates/taba-node/src/main.rs, crates/taba-node/src/secrets, connectors/rust/cli/src, connectors/rust/cli/Dockerfile, connectors/rust/cli/scripts]
---

# Linhas de comando

> **Não confunda:** há **duas** CLIs, com papéis opostos.

| | **`taba`** | **`taba-cli`** |
|---|---|---|
| O que é | O **servidor**: o mesmo binário sobe o nó | O **cliente**: fala com a API |
| Onde está | `crates/taba-node` (binário do servidor) | `connectors/rust/cli` (sobre o conector Rust) |
| O que faz | Sobe um nó; cria a pasta de segredos (`init`), certificados (`cert generate`) e tokens raiz (`token generate`) | Todos os comandos das APIs de cliente e de operação |
| Fala com a API? | **Não** | **Sim**; **não** sobe nó nem toca no disco do servidor |
| Dependências | tokio, axum, SQLite… ([05](05-dependencias.md)) | Só o conector (`rustls`) |

---

## `taba` (servidor)

```text
Uso: taba [--config <arquivo.yml>]
     taba init --out <pasta> [--days <n>]
     taba cert generate --days <n> [--starts-in <duração>] --out <pasta>
     taba token generate
```

| Comando | Faz |
|---|---|
| `taba` (sem subcomando) | **Sobe o nó.** Lê a configuração ([22-configuracao.md](22-configuracao.md)) e espera o sinal de parada |
| `--config <arquivo>` | Arquivo YAML (ou `TABA_CONFIG_FILE`). Sem ele, usa `/etc/taba/taba.yml` **se existir** |
| `taba init --out <pasta> [--days <n>]` | Cria a **pasta de segredos**: o primeiro certificado do cluster (em `tls/`) e o token raiz (`root-token`). `--days`: validade, padrão **1825** |
| `taba cert generate --days <n> [--starts-in <d>] --out <pasta>` | Cria **só** um par de certificado (rotação). `--days` é **obrigatório** aqui. `--starts-in` (`30m`, `14d`) adia o início |
| `taba token generate` | Imprime um segredo aleatório de 256 bits (um token raiz avulso) |
| `--version`, `--help` | — |

Os parâmetros também vêm das variáveis `TABA_*`; **a variável vence o arquivo**. Chave desconhecida no `.yml` é **recusada** (um erro de digitação não passa em silêncio). `main.rs` é só a linha de comando e os sinais; `SIGTERM` e `SIGINT` disparam o **drain** ([07-cluster.md](07-cluster.md)).

```bash
taba init --out ./secrets          # certificado do cluster + token raiz
TABA_MODE=development TABA_STORAGE_DIR=/tmp/taba-dev/taba taba   # um nó para testar
```

A segurança dos artefatos criados está em [14-seguranca.md](14-seguranca.md).

---

## `taba-cli` (cliente)

Um programa pequeno que, de qualquer terminal, faz tudo o que a API oferece. Tem **dois papéis**: **testar e operar** um cluster sem escrever código, e ser a **referência executável do contrato**: cada comando é uma chamada de [16](16-api-cliente.md) ou [17](17-api-operacao.md), e o que ele faz é o que os outros conectores devem repetir.

- **Sem estado:** cada execução faz uma ou mais chamadas e termina. A conexão mora no **perfil**.
- **Sem biblioteca de linha de comando** (leitura de argumentos própria) e sem dependências no sistema: um **binário estático** por plataforma. Só a interface em tela cheia (feature `tui` do Cargo, ligada por padrão) usa bibliotecas de terceiros (`ratatui`, `rust-i18n`, `sys-locale`); `--no-default-features` dá o CLI enxuto, sem elas, e os comandos diretos nunca ligam o terminal em modo cru.
- `taba-cli --version` mostra a versão do programa **e** a do contrato da API (`taba-cli 0.1.0 (contrato da API 1)`).

### Opções de todos os comandos

| Opção | Variável | Padrão | Descrição |
|---|---|---|---|
| `--profile <nome>` | `TABA_CLI_PROFILE` | o perfil padrão (ou o único, quando o arquivo tem um só perfil) | Qual perfil usar. Nome inexistente sai com `2` |
| `-o, --output <formato>` | `TABA_CLI_OUTPUT` | `json` | `json` (a resposta como o servidor a deu, indentada), `table` (colunas) ou `raw` (só o que o comando entrega, ex.: o corpo da mensagem) |
| `-q, --quiet` | — | — | Não imprime nada em caso de sucesso |
| `--request-id <id>` | — | gerado | Vai em `X-Request-Id` |
| `--lang <código>` | `TABA_CLI_LANG` | o do sistema | Idioma da interface em tela cheia (`taba-cli tui`): `en-US` ou `pt-BR`. Os comandos continuam em inglês |
| `--help`, `--version` | — | — | — |

Não existem `--url` nem `--token` nos comandos: a conexão se escolhe pelo **perfil** (`--profile`) ou pelas variáveis `TABA_CLI_*`.

### Comandos

**Tópicos** ([16](16-api-cliente.md)):

| Comando | Rota | Opções |
|---|---|---|
| `topic create <nome>` | `POST /topics` | `--type queue\|distributor` (**obrigatório**), `--tag k=v` (repetível), `--prefix p` (repetível), `--volatile`, `--idle-timeout s`, `--fifo`, `--max-message-bytes n`, `--ttl s`, `--max-deliveries n` |
| `topic list` | `GET /topics` | `--name`, `--sort`, `--limit`, `--cursor`, `--all` (percorre todas as páginas) |
| `topic get <nome>` | `GET /topics/{topic}` | — |
| `topic update <nome>` | `PATCH` | `--tag` (substitui o mapa) ou `--clear-tags`, `--prefix`/`--clear-prefixes`, `--idle-timeout`, `--max-message-bytes`/`--ttl`/`--max-deliveries` (`n` ou `default`). Só o que foi dado é enviado |
| `topic delete <nome>` | `DELETE` | `--yes` (sem ele, num terminal, pede confirmação; fora de um terminal, recusa com `2`). Remove **todas as mensagens** |

**Mensagens:**

| Comando | Rota | Opções |
|---|---|---|
| `publish <tópico> [corpo]` | `POST …/messages` | O corpo vem de **um** destes: o argumento, `--file`, `--stdin`. `--content-type`, atalhos `--json`/`--text`, `--group-id`, `--deduplication-id`, `--id-only`, `--count n` (repete a publicação n vezes: teste leve de carga). **Não valida o corpo** (o servidor responde `validation.invalid-body`). Depois de `502`/`504` fora da FIFO **não repete sozinho** |
| `consume <tópico>` | `GET …/messages` | `--max`, `--visibility`, `--wait` (até 20), `--auto-create` (+ `--persistent`, `--idle-timeout`, `--fifo`, `--tag`; só valem com `--auto-create`), `--ack` (confirma cada mensagem logo depois de imprimi-la), `--exit-empty` (código `6` se vazia). Com `-o raw`, só o `body`, um por linha |
| `subscribe <tópico>` | `GET` em laço + `DELETE` | Long polling contínuo (`--wait`, padrão 20), uma mensagem por vez (`--max`, padrão 1). `--exec <comando>` roda o comando com o corpo na entrada padrão e **confirma só se sair com `0`**. Ack que chega tarde (`409`/`404`) é ignorado. `--limit n`; `Ctrl+C` encerra |
| `ack <tópico> <id>` | `DELETE` | `--receipt <receiptToken>` (obrigatório) |
| `visibility <tópico> <id>` | `PUT …/visibility` | `--receipt` e `--seconds` (obrigatórios; `0` devolve na hora) |

O `receiptToken` muda a cada entrega e vem no `Message`: por isso `consume` **sem `--ack`** imprime a mensagem **inteira**.

**Tokens** (só o token raiz, e só com autenticação ligada):

`token create` (`--name`, `--permission` repetível ou separadas por vírgula, `--prefix`, `--expires-at`, `--token-only`), `token list`, `token get <id>`, `token update <id>` (`--name`, `--expires-at <data>|none`), `token revoke <id>` (`--yes`).

**Operação** (usa o `ops-url` do perfil; sem token):

| Comando | Rota |
|---|---|
| `health live` / `health ready` | `GET /health/live`, `/health/ready` (código `0` se `200`, `1` se não; imprime o `reason`) |
| `health cluster [--scope cluster\|node]` | `GET /health/cluster` (com `-o table`: uma linha por nó e as condições) |
| `metrics [--openmetrics]` | `GET /metrics` (só este nó; o texto sai como veio) |
| `stats topics`, `stats topic <nome>`, `stats groups <tópico>` | `GET /metrics/topics*` (`--scope`, `--tag`, `--condition`, `--fields`, `--group-by`, `--limit`, `--cursor`, `--all`) |
| `drain start`, `drain status`, `drain cancel` | `POST/GET/DELETE /node/drain` — **o nó do `ops-url`** |

**Locais** (não chamam o servidor): `version`, `config show` (a configuração efetiva e **de onde veio cada valor**, sem o token), `profile …`, `help [comando]`.

**Interface em tela cheia** (o guia de uso está em [20b](20b-cli-ui.md)):

| Comando | O que faz | Opções |
|---|---|---|
| `tui` | Abre a interface (Cluster, Tópicos com gráficos e mensagens, Tokens e Perfis) sobre o mesmo perfil e o mesmo token dos comandos. Precisa de um terminal: fora dele sai com `2` e imprime a ajuda | `--no-mouse`, `--history-retention <tempo>` (padrão `1h`, no máximo `6h`), `--history-interval <tempo>` (padrão `5s`, no mínimo `1s`) |

`taba-cli` sem comando abre esta interface quando há terminal (igual a `taba-cli tui`); sem terminal imprime a ajuda. As teclas, as telas, o que cada ação pede do token e a configuração estão em [20b-cli-ui.md](20b-cli-ui.md).

### Perfis

Um **perfil** é um conjunto nomeado de opções de conexão, para alternar entre clusters (local, homologação, produção) com uma palavra.

**Arquivo:** YAML único na pasta de configuração do usuário, criado no primeiro `profile add` (`TABA_CLI_CONFIG` troca o caminho). Em Unix, permissão `0600`.

| Sistema | Arquivo |
|---|---|
| Windows | `%APPDATA%\taba\config.yml` |
| Linux | `$XDG_CONFIG_HOME/taba/config.yml` ou `~/.config/taba/config.yml` |
| macOS | `~/Library/Application Support/taba/config.yml` |

```yaml
default: local
profiles:
  local:
    url: http://127.0.0.1:8080
    token-file: C:\taba\secrets\root-token
  homologacao:
    url: https://taba.hml.exemplo.com
    ops-url: https://taba-ops.hml.exemplo.com
    ca-file: /etc/ssl/taba-hml-ca.pem
```

**Sem configuração** vale o padrão embutido: `http://127.0.0.1:8080`, sem token (basta para um nó local em modo desenvolvimento).

| Campo | Padrão | Descrição |
|---|---|---|
| `url` | `http://127.0.0.1:8080` | A **API de cliente** (a porta de cliente ou o balanceador) |
| `ops-url` | o próprio `url` | A **API de operação**: só `health`, `metrics`, `stats`, `drain`. Com `operations.port`, defina-o |
| `token-file` | — | Lê o token da primeira linha do arquivo. É o jeito de usar o `root-token` |
| `token` | — | O token em texto claro. **Prefira `token-file`** |
| `ca-file` | raízes do sistema | CA (PEM) a confiar. **Não existe** campo para desligar a verificação de TLS |
| `output` | `json` | Formato padrão |
| `connect-timeout` / `timeout` / `max-attempts` | `5` s / `30` s / `3` | Abrir a conexão / chamada comum (o `consume` soma o `waitTime`) / tentativas, contando a primeira |

**Precedência de cada campo:** opção do comando → **variável `TABA_CLI_<CAMPO>`** → perfil escolhido → valor embutido. **Qual perfil:** `--profile`, depois `TABA_CLI_PROFILE`, depois o `default` do arquivo; com um só perfil e sem padrão, esse perfil.

**Interface em tela cheia:** o mesmo arquivo guarda duas seções que não são perfis, `ui` (o idioma) e `history` (o histórico dos gráficos), e a interface escolhe o perfil a abrir pelas mesmas regras de `--profile` e `TABA_CLI_PROFILE`. Tudo isso está em [20b-cli-ui.md](20b-cli-ui.md#configurar).

**Comandos de perfil:** `profile list`, `show [nome]`, `add <nome> [campos] [--default]`, `set <nome> [campos] [--unset campo]`, `remove <nome> [--yes]`, `default [nome|--none]`, `path`, `session …`.

**Tela interativa de perfis.** `taba-cli profile` sem subcomando, **num terminal**, abre uma tela de perfis navegada **com as setas, como um formulário** (lista, cadastra, altera, remove, escolhe o padrão). É uma tela sobre os mesmos comandos e grava o mesmo arquivo. A aba Perfis da interface em tela cheia ([20b](20b-cli-ui.md#perfis)) faz o mesmo e mais (conectar com o perfil), e a substituirá.

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

*(As telas acima são ilustrativas: o texto vem de `connectors/rust/cli/src/tui`; os valores são exemplos.)*

- **Validação na hora:** URL sem `http://`/`https://`, número fora da faixa ou nome repetido ficam em vermelho e **Salvar** não grava.
- **Token à vista:** `token` e `token-file` são mostrados e editados em claro (o arquivo **não é criptografado**). `profile show` e `config show` não mostram o token.
- **Testar conexão:** faz `GET /topics?limit=1` com os valores **ainda não salvos**.
- Sem biblioteca: ANSI e **modo cru** do terminal (`stty` em Linux/macOS; `SetConsoleMode` no Windows 10+). `NO_COLOR` tira as cores. Terminal menor que 60×16 mostra um aviso.
- **Fora de um terminal** (entrada ou saída redirecionada), `profile` não abre a tela: imprime a ajuda e sai com `2`.

**Perfil de sessão (temporário).** Um programa filho não altera o ambiente do terminal, então o comando **imprime** as linhas que o terminal deve executar:

```bash
eval "$(taba-cli profile session homologacao)"                                  # um perfil salvo
eval "$(taba-cli profile session --url http://10.0.0.5:8080 --token-file ./tok)" # perfil temporário
eval "$(taba-cli profile session --clear)"                                       # volta ao padrão
taba-cli profile session --url … --shell powershell | Invoke-Expression          # PowerShell
```

**O arquivo é do CLI:** lê o subconjunto de YAML que ele mesmo escreve e **reescreve o arquivo inteiro** a cada alteração (por arquivo temporário e renomeação). Comentários feitos à mão **não são preservados**.

### Saída e códigos de saída

Sucesso: a resposta vai para a **saída padrão** (os `204` não imprimem nada). Erro da API: vai para a **saída de erro**, como `código: mensagem (requestId …)` mais os `details`; o `Retry-After` do `429` e do `503` aparece numa linha à parte.

| Código | Quando |
|---|---|
| `0` | Sucesso (no `consume`, inclusive sem mensagem) |
| `1` | A API respondeu `4xx` ou `5xx` |
| `2` | Uso errado: comando ou opção desconhecida, argumento que falta, perfil inexistente |
| `3` | Não falou com o servidor (conexão recusada, DNS, TLS) |
| `4` | Estourou o tempo limite |
| `5` | Arquivo ilegível (`--file`, `token-file`, `ca-file` ou o próprio `config.yml`) |
| `6` | `consume --exit-empty` e a resposta veio vazia |

### Exemplos (verificados)

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

### Imagem Docker e binários

- **Imagem** (`connectors/rust/cli/Dockerfile`): mínima, **Alpine**, só com o binário `taba-cli` (estático, musl), usuário sem privilégios, `ENTRYPOINT` = `taba-cli`. Duas etapas; `docker buildx build --platform linux/amd64,linux/arm64`. No Compose da raiz é o serviço `taba-cli` sob o *profile* `tools`, já apontado para os nós (`TABA_CLI_URL`, `TABA_CLI_OPS_URL`, `TABA_CLI_TOKEN_FILE`).
- **Binários** (`scripts/build.sh` e `build.ps1`): seis alvos — Windows (x64, arm64), Linux estático musl (x64, arm64) e macOS (x64, arm64). Sem argumento, constrói para a máquina; `--target <alvo>` e `--all` (os que a máquina consegue). Saída em `connectors/rust/cli/dist/<alvo>/` com `SHA256SUMS`; `--locked`, sem símbolos, otimizado para tamanho. Cruzar para macOS a partir de outro sistema não é suportado pelos scripts (a matriz do CI usa um *runner* de cada).
- **Compilar localmente:** `cd connectors/rust && cargo build -p taba-cli`.

> **No Windows com Git Bash:** comandos como `docker compose run … --entrypoint cat taba-cli /secrets/root-token` têm o caminho convertido; use `MSYS_NO_PATHCONV=1`.

## Fontes e provas

- **`taba`:** `crates/taba-node/src/cli/mod.rs` (`USAGE` e `parse`), `main.rs`, `secrets/`.
- **`taba-cli`:** `connectors/rust/cli/src/{args,commands,profile,settings,output,tui,yaml}` (a interface em tela cheia está em `tui/`, com os catálogos de idioma em `connectors/rust/cli/locales/`); a lista de comandos e as opções foram conferidas contra `taba-cli --help` e `taba-cli <comando> --help` do binário compilado.
- Casos de uso: UC-CLI (36 casos, nível `K`), UC-SEG; veja [26a](26a-casos-de-uso-implementados.md). Testes: `connectors/rust/cli/src/**/tests.rs`, `connectors/rust/cli/tests/commands.rs` e `packaging.rs`.
- Histórico: `19b-cli.md`.
