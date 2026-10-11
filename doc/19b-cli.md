# A linha de comando `taba-cli`

[← Índice](README.md)

## 18.11 O `taba-cli`: o primeiro conector

O `taba-cli` é a **linha de comando do cliente**: um programa pequeno que, de qualquer terminal, faz tudo o que a API do nó oferece a quem usa o Taba. Ele é o **primeiro conector** do projeto ([19-conectores.md](19-conectores.md)) e tem dois papéis:

1. **Testar e operar um cluster sem escrever código.** Subir o Compose, criar um tópico, publicar, consumir, ver a saúde e esvaziar um nó são comandos de uma linha.
2. **Ser a referência executável do contrato.** Como cada comando é uma chamada da API de cliente ([06a](06a-api-cliente.md)) ou de operação ([06b](06b-api-operacao.md)), o que ele faz é o que os outros conectores devem repetir, e os cenários de conformidade ([19-conectores.md](19-conectores.md), seção 18.7) podem ser escritos como roteiros dele.

Não confundir com o binário `taba` do servidor: o `taba` sobe o nó e cria a pasta de segredos (`taba init`, `taba cert generate`, `taba token generate`, [06-api.md](06-api.md), seção 8.0.1) e **não** fala com a API de cliente. O `taba-cli` fala com a API e **não** sobe nó nem toca no disco do servidor.

### Como é feito

- **Linguagem:** Rust, em `connectors/rust/cli/`, num workspace do Cargo dos conectores (`connectors/rust/Cargo.toml`, fora do workspace do servidor), sobre o conector Rust (`connectors/rust/client/`, [19a](19a-conectores-linguagens.md), seção 18.10.3), que traz o HTTP/1.1, o TLS 1.2 ou superior (`rustls`) e o leitor de JSON mínimo. O CLI acrescenta só a leitura dos argumentos (própria, como a do `taba`, sem biblioteca de linhas de comando) e a saída.
- **Nada de estado de execução:** cada execução faz uma ou mais chamadas e termina; não há processo residente nem sessão no servidor. A configuração de conexão vem dos **perfis** guardados num arquivo do usuário (seção 18.11.2).
- **Um binário estático por plataforma**, sem dependências no sistema (seção 18.11.8), e uma imagem Docker mínima (seção 18.11.7).

### 18.11.1 Opções dos comandos

Os comandos ficam simples de propósito: **a conexão não se passa em cada chamada, ela mora no perfil** (seção 18.11.2). Não existem `--url`, `--token` nem as demais opções de conexão nos comandos de API; quem quer outro servidor troca o perfil (`--profile`) ou muda o perfil com `profile set`. Sobram estas opções, que valem antes ou depois do comando (`taba-cli topic list -o table`):

| Opção | Variável | Padrão | Descrição |
|---|---|---|---|
| `--profile <nome>` | `TABA_CLI_PROFILE` | o perfil padrão, se houver | Qual perfil usar nesta chamada. Se o nome não existir, sai com `2`. |
| `--request-id <id>` | — | gerado pelo servidor | Vai em `X-Request-Id` e volta no erro. |
| `-o, --output <formato>` | `TABA_CLI_OUTPUT` | `json` | `json` (a resposta como o servidor a devolveu, indentada), `table` (colunas para listas e leituras) ou `raw` (só o que o comando entrega, por exemplo o corpo da mensagem). |
| `-q, --quiet` | — | desligado | Não imprime nada em caso de sucesso (o código de saída basta). |
| `--help`, `--version` | — | — | A ajuda do comando, e a versão do programa **mais a do contrato** da API que ele implementa. |

### 18.11.2 Perfis e o arquivo de configuração

Um **perfil** é um conjunto nomeado de opções de conexão, para não repetir `--url` e `--token-file` a cada chamada e para alternar entre clusters (local, homologação, produção) com uma palavra.

**Onde fica.** Um único arquivo YAML na pasta de configuração do usuário, criado no primeiro `profile add`:

| Sistema | Arquivo |
|---|---|
| Windows | `%APPDATA%\taba\config.yml` |
| Linux | `$XDG_CONFIG_HOME/taba/config.yml`, ou `~/.config/taba/config.yml` |
| macOS | `~/Library/Application Support/taba/config.yml` |

A variável `TABA_CLI_CONFIG` troca o caminho (útil em contêiner e em testes). Em Unix o arquivo é criado com permissão `0600`, já que pode conter um token.

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
    token-file: ~/.taba/hml-token
```

**Sem configuração.** Se o arquivo não existe, ou não há perfil padrão, vale o padrão embutido: `http://127.0.0.1:8080`, sem token. Isso basta para um nó local em modo desenvolvimento.

**Qual perfil vale.** `--profile`, depois `TABA_CLI_PROFILE`, depois o `default` do arquivo. Uma variável `TABA_CLI_*` sobrepõe só o campo que ela cobre; o resto vem do perfil.

**Os campos de um perfil.** Cada perfil aceita estas chaves (e as opções de mesmo nome em `profile add`, `profile set` e `profile session`):

| Campo | Padrão | Descrição |
|---|---|---|
| `url` | `http://127.0.0.1:8080` | Endereço da **API de cliente** (a porta `api.port`, ou o balanceador). `http://` ou `https://`. |
| `ops-url` | o próprio `url` | Endereço da **API de operação**, usado só pelos comandos `health`, `metrics`, `stats` e `drain`. Sem `operations.port`, o nó serve as rotas de operação na **porta da API de cliente**, por isso o padrão é o `url`; com uma porta própria (`9090`, como no exemplo de [06-api.md](06-api.md)), dê o `ops-url`. |
| `token-file` | nenhum | Lê o token do arquivo (a primeira linha). É o jeito de usar o `root-token` que o `taba init` criou. |
| `token` | nenhum | O token em texto claro, que vai em `Authorization: Bearer`. Prefira `token-file`. |
| `ca-file` | raízes do sistema | Certificado de CA (PEM) a confiar, para um Taba com `https://` e certificado próprio. **Não existe** campo para desligar a verificação (UC-CNT-030). |
| `output` | `json` | O formato de saída padrão (`-o` sobrepõe). |
| `connect-timeout` | `5` | Segundos para abrir a conexão. |
| `timeout` | `30` | Segundos totais de uma chamada comum. O `consume` com `--wait` soma o `waitTime`. |
| `max-attempts` | `3` | Tentativas, contando a primeira (regras de repetição: seção 18.4 de [19-conectores.md](19-conectores.md)). |

**Variáveis de ambiente.** Cada campo tem a variável `TABA_CLI_<CAMPO>` (`TABA_CLI_URL`, `TABA_CLI_OPS_URL`, `TABA_CLI_TOKEN_FILE`, `TABA_CLI_TOKEN`, `TABA_CLI_CA_FILE`, `TABA_CLI_OUTPUT`, `TABA_CLI_CONNECT_TIMEOUT`, `TABA_CLI_TIMEOUT`, `TABA_CLI_MAX_ATTEMPTS`), com o prefixo para não colidir com o `TABA_*` do servidor ([10-configuracao.md](10-configuracao.md)). Elas são o **perfil de sessão** (abaixo) e o jeito de configurar um contêiner sem arquivo.

**Precedência de cada campo:** opção do comando (`-o`), **variável de ambiente**, **perfil escolhido**, valor embutido. Se `token` e `token-file` estão definidos, vale o de maior precedência (variável, depois perfil) e, no mesmo nível, o `token-file`.

**Comandos** (locais, não chamam o servidor):

| Comando | O que faz |
|---|---|
| `profile list` | Lista os perfis, marcando o padrão e o que está em uso na sessão. A saída segue o `-o` como as demais (um item por perfil, com o `token` mostrado só como `(definido)`). |
| `profile show [nome]` | Mostra um perfil (o em uso, se sem nome), **sem o token**. |
| `profile add <nome> [campos] [--default]` | Cria o perfil com os campos dados (`--url`, `--ops-url`, `--token-file`, `--token`, `--ca-file`, `--output`, `--connect-timeout`, `--timeout`, `--max-attempts`). Nestes comandos, `--output` é o campo `output` do perfil. Se já existe, sai com `2`. `--default` o torna o padrão; o primeiro perfil criado já vira o padrão. |
| `profile set <nome> [campos] [--unset <campo>]` | Altera os campos dados e apaga os de `--unset` (repetível). Perfil inexistente sai com `2`. |
| `profile remove <nome>` | Remove o perfil. Se era o padrão, o arquivo fica sem padrão. Pede confirmação num terminal e, fora dele, exige `--yes` (como o `topic delete`). |
| `profile default [nome]` | Sem nome, mostra o padrão; com nome, o define. `--none` o remove. |
| `profile path` | Imprime o caminho do arquivo de configuração. |
| `profile session ...` | O **perfil de sessão**, abaixo. |
| `profile` (sem subcomando) | Abre o **modo interativo** de perfis, abaixo. |

**Modo interativo.** `taba-cli profile` sem subcomando, **num terminal**, abre uma tela de perfis navegada **com as setas, como um formulário**: lista, cadastra, altera, remove e escolhe o padrão sem decorar opções. É só uma tela por cima dos mesmos comandos (`profile add`, `set`, `remove`, `default`) e grava o mesmo arquivo, do mesmo jeito.

*Lista de perfis:*

```text
 Perfis do taba-cli                         C:\Users\ana\AppData\Roaming\taba\config.yml

   nome          url                        token-file    padrão
 ▶ local         http://127.0.0.1:8080      —             ●
   homologacao   https://taba-hml:8080      /seg/tok      ○
   producao      https://taba.exemplo.com   —             ○

 ↑↓ escolher   Enter abrir   n novo   d padrão   Del remover   q sair
```

*Formulário de um perfil (novo ou existente):*

```text
 Perfil  producao

   Nome             producao
   url              https://taba.exemplo.com▌
   ops-url          (https://taba.exemplo.com)
   token-file       /seg/prod.tok
   token            eyJhbGciOi...
   ca-file          (raízes do sistema)
   output           ‹ json ›
   connect-timeout  (5)
   timeout          (30)
   max-attempts     (3)
   Perfil padrão    [x]

   [ Salvar ]  [ Testar conexão ]  [ Cancelar ]

 ↑↓/Tab campo   ←→ opção   Espaço marcar   Ctrl+S salvar   Esc cancelar
```

- **Navegação:** `↑`/`↓` (ou `Tab`/`Shift+Tab`) movem entre os campos e os botões. Num campo de texto, digita-se direto (`←`/`→`, `Home`, `End`, `Backspace` e `Delete` editam a linha). `output` é uma escolha, trocada com `←`/`→`; `Perfil padrão` é uma caixa, marcada com `Espaço`. Em um botão, `Enter` o aciona. `Ctrl+S` salva de qualquer campo.
- **Valores padrão:** um campo vazio mostra, entre parênteses e em cinza, o valor que valerá (o embutido, ou o derivado, como o `ops-url`). Apagar o texto de um campo volta a ele; é o mesmo que `--unset`.
- **Validação na hora:** um campo inválido (URL sem `http://` ou `https://`, número fora da faixa, nome repetido ou com caracteres que não são letra, dígito, `-` ou `_`) fica em vermelho com a mensagem logo abaixo, e **Salvar** não grava enquanto houver erro.
- **Token:** `token-file` e `token` são mostrados e editados **à vista**, como os outros campos. O token no arquivo de configuração **não é criptografado** (o arquivo é `0600` em Unix), então a tela não finge segredo; quem não quer o token visível num terminal compartilhado usa `token-file`. Os comandos `profile show` e `config show` continuam sem mostrar o token.
- **Testar conexão:** faz um `GET /topics?limit=1` na API de cliente (confere o endereço, o TLS e o token de uma vez) com os valores **ainda não salvos** do formulário e mostra o resultado (ok com o tempo, ou o erro) numa linha; só informa, não impede de salvar.
- **Lista:** o padrão é marcado com `●`, e o perfil em uso na sessão (`TABA_CLI_PROFILE`), com `(sessão)`. `d` torna o perfil escolhido o padrão. `Del` (ou `r`) remove, depois de uma linha de confirmação `Remover "producao"? [s/N]`. `n` abre o formulário de um perfil novo; o primeiro perfil criado vira o padrão. Com a lista vazia, a tela diz que não há perfis e oferece `n`.
- **Sair:** `q` ou `Esc` na lista, e `Ctrl+C` em qualquer tela, encerram e **restauram o terminal**. Cada ação já foi gravada quando confirmada; o formulário aberto e não salvo é descartado, e `Esc` nele pede confirmação se algo mudou.
- **Terminal:** a tela usa sequências ANSI (cores, cursor e tela alternada, que devolve o terminal como estava ao sair) e a entrada em **modo cru**, sem biblioteca: no Linux e no macOS, por `stty`; no Windows, pelo `SetConsoleMode` do console com sequências virtuais (Windows 10 em diante). `NO_COLOR` tira as cores, mantendo a navegação. Um terminal menor que 60 colunas por 16 linhas mostra um aviso pedindo para aumentá-lo, em vez de desenhar quebrado, e a tela se redesenha, com o tamanho novo, a cada tecla.
- **Fora de um terminal** (entrada ou saída redirecionada), `taba-cli profile` não abre a tela: imprime a ajuda dos comandos de perfil e sai com `2`. Scripts usam os subcomandos.

**Perfil de sessão (temporário).** O que vale só para o terminal aberto, sem tocar no arquivo. Um programa filho não altera o ambiente do terminal que o chamou, então o comando **imprime** as linhas que o terminal deve executar, e a pessoa as aplica:

```bash
eval "$(taba-cli profile session homologacao)"                 # escolhe um perfil salvo
eval "$(taba-cli profile session --url http://10.0.0.5:8080 --token-file ./tok)"   # perfil temporário
eval "$(taba-cli profile session --clear)"                      # volta ao padrão
```
```powershell
taba-cli profile session --url http://10.0.0.5:8080 --token-file .\tok --shell powershell | Invoke-Expression
```

- Com um **nome**, imprime `TABA_CLI_PROFILE=<nome>`; com **campos**, imprime as variáveis `TABA_CLI_URL`, `TABA_CLI_TOKEN_FILE` e as demais correspondentes (o perfil temporário é só o conjunto delas); com `--clear`, desfaz as duas coisas.
- `--shell sh|powershell|cmd` escolhe a sintaxe (padrão: `powershell` no Windows e `sh` nos outros). Os valores saem escapados para o *shell* escolhido.
- Nada é gravado em disco, e o `--token` dado a um perfil de sessão vai só para a variável do terminal.

**O arquivo é do CLI.** O `taba-cli` lê o subconjunto de YAML que ele mesmo escreve (mapas e textos, comentários com `#`) com um leitor próprio, e **reescreve o arquivo inteiro** a cada `profile add/set/remove/default` (por arquivo temporário e renomeação, para não deixar o arquivo pela metade); comentários feitos à mão não são preservados. Arquivo ilegível ou YAML inválido sai com `5`, dizendo a linha.

**`config show`** mostra a configuração efetiva e **de onde veio cada valor** (`variável <nome>`, `perfil <nome>` ou `padrão`), uma linha por campo (`field`, `value`, `source`) e o perfil em uso. O token aparece só como `(definido)`.

### 18.11.3 Saída e códigos de saída

- **Sucesso:** a resposta vai para a saída padrão. Com `-o json`, é o JSON do servidor, sem campos acrescentados. Os comandos que devolvem `204` não imprimem nada.
- **Erro da API:** vai para a **saída de erro**, no formato `código: mensagem (requestId ...)`, mais os `details` se houver. Com o formato `json` **escolhido** (`-o json`, o `output` do perfil ou `TABA_CLI_OUTPUT`; o `json` embutido não conta), é o `ErrorResponse` como veio, indentado ([06d](06d-api-convencoes.md), seção 8.3.6). O `Retry-After` do `429` e do `503` aparece numa linha à parte.
- **Códigos de saída:**

| Código | Quando |
|---|---|
| `0` | Sucesso. No `consume`, inclusive quando não havia mensagem. |
| `1` | A API respondeu `4xx` ou `5xx` (o `ErrorResponse` foi impresso). |
| `2` | Uso errado: comando ou opção desconhecida, argumento que falta, valor fora da faixa verificável sem chamar o servidor. |
| `3` | Não conseguiu falar com o servidor (conexão recusada, DNS, TLS). |
| `4` | Estourou o tempo limite. |
| `5` | Arquivo ilegível (`--file`, o `token-file` ou o `ca-file` do perfil, ou o próprio `config.yml`). |

O comando `consume --exit-empty` (abaixo) devolve `6` quando a resposta vem vazia, para os scripts. Se quem lê a saída a fecha (`taba-cli ... | head`), o programa para em silêncio, com `0`.

### 18.11.4 Tópicos

Rotas: [06a](06a-api-cliente.md), "Tópicos". O token precisa da permissão da rota.

| Comando | Rota | Opções e argumentos |
|---|---|---|
| `topic create <nome>` | `POST /topics` | `--type queue\|distributor` (obrigatório); `--tag <k=v>` (repetível); `--prefix <p>` (repetível, só `distributor`); `--volatile` e `--idle-timeout <s>`; `--fifo`; `--max-message-bytes <n>`; `--ttl <s>`; `--max-deliveries <n>`. O que não vale para o tipo é recusado pelo servidor (`validation.not-applicable`). |
| `topic list` | `GET /topics` | `--name <texto>` (o nome **contém**); `--sort name\|id\|createdAt` (com `-` na frente, decrescente); `--limit <n>`; `--cursor <c>`; `--all` (percorre todas as páginas e imprime todos os itens). |
| `topic get <nome>` | `GET /topics/{topic}` | — |
| `topic update <nome>` | `PATCH /topics/{topic}` | `--tag <k=v>` (repetível; **substitui o mapa inteiro**) ou `--clear-tags`; `--prefix <p>` (repetível; `--clear-prefixes`); `--idle-timeout <s>`; `--max-message-bytes <n>\|default`; `--ttl <s>\|default`; `--max-deliveries <n>\|default` (`default` envia `null` e volta ao padrão do servidor). Só o que foi dado é enviado. Os campos imutáveis (`name`, `type`, `volatile`, `fifo`) não existem como opção. |
| `topic delete <nome>` | `DELETE /topics/{topic}` | `--yes` (sem ele, e num terminal, pede confirmação; fora de um terminal, recusa com o código `2`). Remove também **todas as mensagens**. |

Exemplos:

```bash
taba-cli topic create pedidos --type queue --fifo --tag time=financeiro --ttl 86400
taba-cli topic list --name pedido --sort=-createdAt --all -o table
taba-cli topic update pedidos --ttl default --tag time=pagamentos
taba-cli topic delete pedidos --yes
```

### 18.11.5 Mensagens

Rotas: [06a](06a-api-cliente.md), "Mensagens".

| Comando | Rota | Opções e argumentos |
|---|---|---|
| `publish <tópico> [corpo]` | `POST /topics/{topic}/messages` | O corpo vem de **um** destes: o argumento, `--file <arquivo>` (bytes crus) ou `--stdin`. `--content-type <tipo>` (padrão `application/json`); atalhos `--json` e `--text`, que só escolhem o `Content-Type`; `--group-id <g>`; `--deduplication-id <d>`. O CLI **não valida o corpo**: o servidor responde `validation.invalid-body`. Imprime o `PublishResponse` (ou o `DistributorPublishResponse`); `--id-only` imprime só o `id`. `--count <n>` repete a publicação `n` vezes (cada uma é uma chamada, para um teste leve de carga). Depois de um `502` ou `504` fora da FIFO o CLI **não repete** a publicação sozinho (repetir duplicaria a mensagem); na FIFO a repetição é segura e segue a regra geral. Como o CLI não sabe de antemão se o tópico é FIFO, é só nesse erro que ele faz um `GET /topics/{topic}` para descobrir. |
| `consume <tópico>` | `GET /topics/{topic}/messages` | `--max <n>` (`maxMessages`); `--visibility <s>` (`visibilityTimeout`); `--wait <s>` (`waitTime`, até 20); `--auto-create` com `--persistent` (a fila nasce não volátil), `--idle-timeout <s>`, `--fifo`, `--tag <k=v>`; `--ack` (confirma cada mensagem entregue logo depois de imprimi-la); `--exit-empty` (código `6` se vier vazia). Com `-o raw`, imprime só o `body` de cada mensagem, um por linha. As opções `--persistent`, `--idle-timeout`, `--fifo` e `--tag` só valem com `--auto-create` (sem ele, o CLI sai com `2`). O CLI manda só o que foi informado e não aplica padrões. |
| `subscribe <tópico>` | `GET` em laço e `DELETE` | O laço de consumo da superfície dos conectores (D71.1): long polling contínuo (`--wait`, padrão 20), uma mensagem por vez (`--max`, padrão 1). Para cada mensagem: com `--exec <comando>`, roda o comando com o corpo na entrada padrão e **confirma só se ele sai com `0`**; sem `--exec`, imprime a mensagem e confirma. `--visibility <s>`. Um ack que chega depois do prazo (`409 message.stale-receipt` ou `404`) é ignorado, e o laço segue. `--limit <n>` termina depois de `n` mensagens. `Ctrl+C` encerra o programa na hora; a mensagem em andamento não é confirmada e volta à fila no fim da invisibilidade. |
| `ack <tópico> <id>` | `DELETE /topics/{topic}/messages/{id}` | `--receipt <receiptToken>` (obrigatório): o token **da última entrega**. |
| `visibility <tópico> <id>` | `PUT /topics/{topic}/messages/{id}/visibility` | `--receipt <receiptToken>` (obrigatório) e `--seconds <s>` (obrigatório; `0` devolve a mensagem na hora). Imprime o `VisibilityResponse`. |

O `receiptToken` muda a cada entrega e vem no `Message`. Por isso `consume` sem `--ack` imprime a mensagem **inteira** (com `receiptToken`), e `ack` e `visibility` o recebem por `--receipt`.

Exemplos:

```bash
echo '{"pedido": 9001}' | taba-cli publish pedidos --stdin --group-id cliente-42 --deduplication-id pedido-9001
taba-cli publish pedidos --text "olá" --id-only
taba-cli consume pedidos --max 5 --wait 20 --visibility 60
taba-cli consume pedidos --max 1 --ack -o raw
taba-cli ack pedidos 01927f3c-2b30-7a44-9d55-6e7f8091a2b3 --receipt rt_...
taba-cli subscribe pedidos --exec ./processa.sh
```

### 18.11.6 Tokens e operação

**Tokens** (`/tokens`, [06a](06a-api-cliente.md), "Tokens de acesso"): só o **token raiz** os usa, e só com a autenticação ligada. Sem ela (modo desenvolvimento), o servidor responde `404 route.not-found`.

| Comando | Rota | Opções e argumentos |
|---|---|---|
| `token create` | `POST /tokens` | `--name <n>` (obrigatório); `--permission <p>` (repetível ou separadas por vírgula: `publish`, `consume`, `create`, `admin`; obrigatório); `--prefix <p>` (repetível); `--expires-at <data>` (RFC 3339, no futuro). O segredo (`token`) aparece **só nesta resposta**; `--token-only` imprime só ele. |
| `token list` | `GET /tokens` | `--name <texto>`, `--sort id\|createdAt`, `--limit`, `--cursor`, `--all`. Nunca mostra o segredo. |
| `token get <id>` | `GET /tokens/{id}` | — |
| `token update <id>` | `PATCH /tokens/{id}` | `--name <n>`; `--expires-at <data>\|none` (`none` remove o prazo). Permissões e prefixos são imutáveis. |
| `token revoke <id>` | `DELETE /tokens/{id}` | `--yes`, como em `topic delete`. |

**Operação** (API de operação, [06b](06b-api-operacao.md); usa o `ops-url` do perfil). Não leva token: as rotas de operação não são autenticadas e se protegem pela rede e por `operations.allowed-ips`.

| Comando | Rota | Opções e argumentos |
|---|---|---|
| `health live` | `GET /health/live` | O código de saída é `0` se `200`, `1` se não. As rotas de saúde não repetem a chamada (o orquestrador pergunta de novo sozinho): um `503` é impresso (o corpo é o do próprio endpoint, não um `ErrorResponse`) e sai com `1`. |
| `health ready` | `GET /health/ready` | Imprime o `reason` quando o nó não está pronto (`503`). |
| `health cluster` | `GET /health/cluster` | `--scope cluster\|node` (padrão `cluster`). Com `-o table`, uma linha por nó e as condições. |
| `metrics` | `GET /metrics` | `--openmetrics` (envia `Accept: application/openmetrics-text`). Só este nó. O texto sai como veio, qualquer que seja o `-o`. |
| `stats topics` | `GET /metrics/topics` | `--scope cluster\|node`; `--tag <k=v>`; `--condition <c>`; `--fields <a,b>`; `--group-by <tag>`; `--limit`, `--cursor`, `--all`. |
| `stats topic <nome>` | `GET /metrics/topics/{topic}` | `--scope`. |
| `stats groups <tópico>` | `GET /metrics/topics/{topic}/groups` | `--limit`, `--cursor`, `--all`. |
| `drain start` | `POST /node/drain` | Põe **o nó do `ops-url` do perfil** em drain. |
| `drain status` | `GET /node/drain` | — |
| `drain cancel` | `DELETE /node/drain` | Cancela o drain, ou faz o nó que já saiu voltar como nó novo ([06b](06b-api-operacao.md), "Drain"). |

**Comandos locais, sem chamada:** `version` (versão do programa e do contrato), `config show` (a configuração efetiva, **sem o token**) e `profile ...` (seção 18.11.2). O `taba-cli help [comando]` mostra a ajuda de cada um.

**Fora do `taba-cli`:** a API do cluster (`/cluster/*`, [06c](06c-api-cluster.md)), que só os nós usam e exige TLS mútuo, e os subcomandos de segredos do servidor (`taba init`, `taba cert generate`, `taba token generate`), que ficam no binário `taba`.

### 18.11.7 A imagem Docker

O `connectors/rust/cli/Dockerfile` gera uma imagem **mínima, em Alpine**, só com o binário `taba-cli` (estático, ligado à musl), sem shell de aplicação nem outros pacotes além dos certificados de CA do sistema:

- **Construção em duas etapas:** o contexto é a raiz do repositório e o `Dockerfile` copia só a pasta `connectors/`. A primeira etapa (imagem do Rust em Alpine) compila para `x86_64-unknown-linux-musl` ou `aarch64-unknown-linux-musl`, conforme a plataforma pedida em `docker buildx build --platform linux/amd64,linux/arm64`; a segunda parte de `alpine` e copia o binário.
- **Uso:** o `ENTRYPOINT` é o `taba-cli`, então o que vem depois do nome da imagem são os comandos:

```bash
docker run --rm --network host -e TABA_CLI_URL=http://127.0.0.1:8080 taba-cli topic list
docker run --rm -e TABA_CLI_URL=http://taba-1:8080 -e TABA_CLI_TOKEN_FILE=/secrets/root-token \
  -v "$PWD/secrets:/secrets:ro" --network taba_default taba-cli topic list
```

- **No Compose da raiz do repositório** o `taba-cli` é um serviço sob o *profile* `tools`, apontado para os nós, de modo que `docker compose run --rm taba-cli health cluster` funciona sem instalar nada ([README.md](../README.md)).
- Roda como usuário sem privilégios.

### 18.11.8 Scripts de build e plataformas

O programa é distribuído como **um binário por plataforma**, para estas seis combinações:

| Sistema | x64 | arm64 | Alvo do Rust |
|---|---|---|---|
| Windows | sim | sim | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` |
| Linux (estático, musl) | sim | sim | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` |
| macOS | sim (Intel) | sim (Apple Silicon) | `x86_64-apple-darwin`, `aarch64-apple-darwin` |

Os scripts ficam em `connectors/rust/cli/scripts/`:

- **`build.sh`** (Linux e macOS) e **`build.ps1`** (Windows). Sem argumento, constroem para a **plataforma da máquina**. `--target <alvo>` constrói para um alvo da tabela, e `--all` constrói **todos os que a máquina consegue**. O resultado vai para `connectors/rust/cli/dist/<alvo>/`, com o binário e um arquivo `SHA256SUMS`.
- **Quem constrói o quê:** o Linux e o macOS constroem os dois alvos do Linux (com `cargo-zigbuild` ou o `cross`, para o `aarch64`); o macOS constrói os dois do macOS; o Windows constrói os dois do Windows. Cruzar para o macOS a partir de outro sistema não é suportado pelos scripts: a matriz de publicação (um *runner* de cada sistema) faz as seis.
- **Reprodutível:** o `Cargo.lock` vale (`--locked`), e o binário sai sem símbolos (`strip`) e otimizado para tamanho.
- **A imagem Docker** tem o seu `docker buildx` (`scripts/docker.sh`), com as duas plataformas Linux.

### 18.11.9 Casos de uso e testes

Os casos estão em [20a](20a-casos-de-uso-cliente.md), área `CLI`, no nível `K` (conector). Os testes ficam em `connectors/rust/cli/`:

- **Unitários** nos módulos (`src/**/tests.rs`): a leitura de argumentos, o YAML dos perfis, a escolha do perfil e a precedência, a tabela e o JSON indentado, o perfil de sessão e a tela interativa (teclas, formulário, validação), tudo sem terminal nem rede.
- **Os comandos de ponta a ponta** (`tests/commands.rs`): rodam o programa inteiro (`taba_cli::run`) sobre o transporte de mentira do conector (`taba_client::testing`) e conferem a chamada que o CLI monta (rota, parâmetros, cabeçalhos e corpo), o que ele imprime e o código de saída, com os perfis num arquivo temporário e a tela interativa dirigida por teclas combinadas.
- **A embalagem** (`tests/packaging.rs`): o `Dockerfile` e os scripts de build dizem o que as seções 18.11.7 e 18.11.8 prometem (os seis alvos, `--locked`, Alpine, usuário sem privilégios). O build de verdade e o terminal em modo cru conferem-se à mão e no CI.
- **Contra um Taba de verdade:** os cenários de `connectors/contrato/` ([19-conectores.md](19-conectores.md), seção 18.7) e as **fixtures** compartilhadas com os outros conectores entram com a fase 0 do plano; até lá, a conferência é um Taba em modo desenvolvimento rodado à mão.
