---
título: Plano — interface interativa (TUI) do taba-cli
resumo: Plano e requisitos por tela do `taba-cli tui`, uma interface em tela cheia feita em Rust com ratatui para acompanhar e operar um cluster, gerenciar tokens e tópicos e testar mensagens — com os requisitos de cada tela, mapa de teclas, fontes de dados, idiomas, testes e fases.
tipo: plano
status: planejado
verificado-em: 2026-10-15 @ 56121c9
fontes: [connectors/rust/cli/src, connectors/rust/client/src, doc/pt-BR/16-api-cliente.md, doc/pt-BR/17-api-operacao.md, doc/pt-BR/13-observabilidade.md, doc/pt-BR/20-cli.md]
---

# Plano — interface interativa (TUI) do `taba-cli`

> **Status: ⬜ planejado.** Nada aqui existe ainda; as decisões da [seção 2](#2-decisões) estão tomadas e o plano aguarda o sinal verde para ser construído. Contexto que você precisa antes: [20-cli.md](20-cli.md) (comandos e perfis de hoje), [17-api-operacao.md](17-api-operacao.md) e [16-api-cliente.md](16-api-cliente.md) (as rotas que as telas chamam), [13-observabilidade.md](13-observabilidade.md) (métricas e condições). Onde este plano descobre que a API não oferece algo que o pedido exige, diz isso na [seção 3](#3-o-que-a-api-oferece-e-o-que-não-oferece) e pede decisão na [seção 2](#2-decisões).

## 1. Objetivo e escopo

Um novo comando, **`taba-cli tui`**, abre uma interface em tela cheia (Rust + [ratatui](https://ratatui.rs)) sobre o mesmo conector (`taba-client`) e os mesmos perfis que os comandos já usam. É uma *tela sobre a API existente*, como a tela de perfis de hoje: não cria recurso no servidor, exceto o que a [seção 2](#2-decisões) decidir.

| # | Área | O que a pessoa pode fazer |
|---|---|---|
| 1 | **Cluster** | Ver como o cluster está (condições, estado de cada nó, memória, disco, sincronização); **drenar** um nó, **devolvê-lo**, forçar **backup**, **parar o cluster**; ver gráficos de monitoramento |
| 2 | **Tokens** | Listar, filtrar, **criar** (o segredo aparece uma vez), **alterar** nome e validade, **revogar**; "alterar" o que é imutável por clonagem |
| 3 | **Tópicos** | Listar, filtrar, **criar**, **alterar**, **apagar**; **gráficos de monitoramento** por tópico e grupos |
| 3.2 | **Mensagens** | **Publicar** (teste), **escutar** (ao vivo), **navegar**, **ack**/mudar visibilidade, **prune** de uma fila |

Requisitos que valem para todas as telas:

- **Navegação fluida:** toda lista e todo texto rolam (setas, `PgUp`/`PgDn`, `Home`/`End`, roda do mouse) com barra de rolagem; o foco anda entre painéis; **toda ação tem atalho** e nada exige o mouse ([seção 5](#5-teclado-e-navegação)).
- **Vários idiomas:** `en-US` e `pt-BR` primeiro, outros acrescentando um arquivo ([seção 8](#8-idiomas-i18n)).
- **As mesmas regras de segurança dos comandos:** ação que apaga ou interrompe pede confirmação; o segredo do token nunca vai para um log.

**Fora do escopo:** iniciar ou matar o *processo* do nó (a API não tem rota para isso; é trabalho do orquestrador — ver D-TUI-3), editar a configuração do servidor e substituir os comandos do `taba-cli` (eles ficam; a TUI só acrescenta).

## 2. Decisões

Todas as decisões abaixo foram tomadas pelo dono do projeto e estão em vigor: o restante deste plano as segue. Elas mudam também o servidor, por isso o plano lista trabalho no servidor ([seção 12](#12-impacto-no-que-existe)). Quando o trabalho começar, entram em [30-decisoes.md](30-decisoes.md) (com o próximo `D…` livre) e as perguntas em aberto em [31-pendencias.md](31-pendencias.md).

| Código | Assunto | Decisão |
|---|---|---|
| **D-TUI-1** | **Dependências e modo de terminal.** Hoje o `taba-cli` depende só do conector ([05-dependencias.md](05-dependencias.md), regra de "zero dependências") e tem terminal próprio em modo raw. | **Dois modos.** Os **comandos diretos** (`topic list`, `token create`, `drain`…) rodam exatamente como hoje: saída simples, sem controle de tela, sem nenhum código do `ratatui`. O **modo interativo** — `taba-cli tui` e a tela de perfis (D-TUI-8) — é o único que liga o `crossterm` (modo raw, tela alternativa, devolvida ao sair e em caso de panic) e desenha com o `ratatui` (tabelas, gráficos, listas). As duas bibliotecas (~25 crates transitivas) vêm atrás de uma feature do Cargo `tui`, **ligada por padrão** nos binários publicados e na imagem Docker; `--no-default-features` dá o CLI enxuto de hoje. O conector (`taba-client`) continua sem dependências. Fixar versões (ratatui 0.30.x, crossterm 0.29.x na data de escrita; conferir ao começar). |
| **D-TUI-2** | **Prune não tem rota.** A API de cliente não tem como esvaziar uma fila ([seção 3](#3-o-que-a-api-oferece-e-o-que-não-oferece)). | **O prune é criado no `taba-node` e exposto na API de cliente:** `DELETE /topics/{topic}/messages` (permissão `admin`; responde `{"removed":n}` ou a falha). O nó que recebe **repassa o prune a todos os outros membros** por uma nova rota da API do cluster (`POST /cluster/topics/{topic}/prune`, mTLS, só membros — o mesmo padrão de repasse de `POST /cluster/backup/flush`), poda a própria cópia, espera todos e responde **sucesso só se todos tiverem sucesso**, senão falha dizendo quais nós falharam. A chamada é idempotente por um `pruneId` e, por ser rara, **não tem mecanismo de repetição**: na falha, a pessoa repete. Passa pelo balanceador, então não precisa de URL por nó. A TUI **não tem laço no cliente**. Detalhes na seção 6.8 e na seção 12. |
| **D-TUI-3** | **"Iniciar" e "parar" um nó.** A API só tem *drain* (parar de receber, gravar em disco, sair do cluster) e *retorno*. O processo continua rodando como `drained`. | **Mesmo tratamento do prune: criado no `taba-node`, na API de cliente, com o token raiz** (a única credencial do cluster todo; a permissão `admin` é por tópico). *Parar nó* = drain; *Iniciar nó* = retorno do drain (volta como nó novo); *Parar cluster* e *flush do backup* valem para o cluster todo. Hoje essas rotas estão na API de operação, sem token, protegidas pela rede e por nó; a mudança as expõe na API de cliente (`POST`/`GET`/`DELETE /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop`; os nomes finais saem na mudança do servidor). Matar ou subir processos **não** é oferecido (trabalho do orquestrador). |
| **D-TUI-4** | **Qual nó recebe uma ação de nó?** O `drain` vale só para o nó que recebe a chamada, **não pelo balanceador**, e o status não traz a URL de operação de cada nó. | **Mesmo tratamento do prune: o nó que recebe a chamada chama internamente o nó certo.** Toda rota endereçada a um nó leva o `nodeId` (`/nodes/{nodeId}/…`, descoberto em `GET /health/cluster`); o nó que a recebe, pelo balanceador ou por qualquer endereço, repassa a chamada ao nó alvo pela porta do cluster (mTLS, só membros; um novo `/cluster/node-call/{caminho}` que executa a API de operação do alvo, porque o `/cluster/forward/{caminho}` existente exige o alvo ativo e um nó em drain precisa continuar alcançável para ser devolvido: D154), o nó que recebeu a chamada confere o token, e devolve a resposta como veio. **A regra, a mesma de uma mensagem:** o nó que recebe a chamada compara o `nodeId` do caminho com o seu; se **for** ele, responde com os próprios dados; se **não** for, chama uma vez, pela porta do cluster, o nó que tem o dado, e nunca repassa de novo uma chamada já repassada; um `nodeId` desconhecido é `404 node.not-found`, e um alvo que não responde é uma falha que o nomeia (sem nova tentativa). Vale também para as métricas de nó da D-TUI-9. Se o alvo não responde: falha dizendo o nó. Isso também serve o `GET /metrics` do nó alvo para o amostrador (D-TUI-5). **Sem campo `nodes` e sem URL por nó no perfil.** |
| **D-TUI-5** | **Gráficos precisam de histórico; a API só dá o presente.** | Desde o momento em que o app abre, a TUI **amostra em segundo plano durante toda a sessão** em buffers circulares na memória: por padrão **1 hora de histórico com uma amostra a cada 5 s** (720 amostras por série), ambos **configuráveis** (`history.retention`, `history.interval`; flags `--history-retention`, `--history-interval`). A amostra mais antiga é sobrescrita. Nada é gravado em disco e o histórico se perde ao sair. Para histórico longo, usar o Prometheus ([13-observabilidade.md](13-observabilidade.md)); a TUI avisa isso no gráfico. |
| **D-TUI-6** | **Ler uma fila.** Não existe rota "espiar"; ler uma fila é *entregar* a mensagem. | **A TUI nunca lê uma fila por conta própria.** A leitura só acontece quando a pessoa pede (*Navegar* ou *Escutar*), e o *Navegar* pergunta antes **quantas mensagens** ler e **por quanto tempo ficam invisíveis** (tempo de visibilidade). O popup informa que cada leitura **conta uma entrega** (`deliveryCount`) e pode mandar a mensagem para a dead-letter quando há `maxDeliveries` (mostra esse limite). As mensagens voltam sozinhas quando o tempo acaba; a tela também oferece "devolver agora" (`PUT …/visibility` com `0`). Nunca faz ack sozinha. |
| **D-TUI-7** | **Traduções.** O ratatui não tem i18n. | **`rust-i18n` (4.2.x) + `sys-locale` (0.3.x).** Os catálogos são arquivos YAML embutidos na compilação (`locales/en-US.yml`, `locales/pt-BR.yml`), usados com a macro `t!("chave", nome = valor)`; o `en-US` é o idioma de reserva e a fonte. Os plurais são chaves `chave.one` / `chave.other` escolhidas por uma pequena regra por idioma. O `sys-locale` lê o idioma do sistema. Um teste nosso compara as chaves e variáveis dos arquivos (o rust-i18n não confere a paridade) e quebra a compilação numa diferença. Fluent e gettext foram avaliados e deixados de fora: mais pesados, e sua força está em idiomas de plural complexo. |
| **D-TUI-8** | **A tela de perfis antiga** (terminal raw próprio, `term.rs`, `keys`). | Manter até a fase 6; depois **refazê-la em ratatui como a aba "Perfis"** e apagar `tui/term.rs` e `tui/keys` (menos código no total). O `taba-cli profile` continua abrindo-a. |
| **D-TUI-9** | **Métricas e status precisam de token.** As rotas `GET /health/cluster`, `GET /metrics`, `GET /metrics/topics*` e `/node/*` ficam na API de operação, **sem autenticação** (D63), porque servem o monitoramento contínuo (Prometheus, balanceador). Elas continuam como estão. A TUI não pode usá-las: tem uma URL e um token. | **Cópias autenticadas na API de cliente**, só de leitura, para a TUI e outros clientes: `GET /cluster/status` (o que o `/health/cluster` responde), `GET /nodes/{nodeId}/metrics` (o texto Prometheus do nó, repassado como na D-TUI-4), `GET /stats/topics`, `GET /stats/topics/{topic}` e `GET /stats/topics/{topic}/groups` (o `TopicStats` da API de operação). Mais duas rotas completam o conjunto: `GET /nodes` (a lista de nós do cluster — um cliente pode precisar dela) e `GET /auth/me` (quem é o token: nome, permissões, prefixos, validade e se é o token raiz). Os nomes finais saem na mudança do servidor. **Quem pode chamar — o princípio: tudo que um token simples pode ver dentro do seu escopo, ele vê sem ser administrador.** `GET /nodes` (id, estado, estado do drain, versão; **sem endereços**), `GET /auth/me` e as rotas de tópico (`/stats/topics…`, qualquer token válido, **filtradas pelos seus prefixos**, como o `GET /topics` já é) — qualquer token válido. O detalhe do cluster (`GET /cluster/status`: memória, disco, sinais, condições, endereços) e tudo de um nó (`/nodes/{nodeId}/…`) — só o **token raiz**. **O que é negado diz o que falta:** um `403 auth.forbidden` leva um campo novo `required` (por exemplo o token raiz, ou a permissão `admin` sobre o tópico), para a TUI e qualquer cliente poderem dizer "seu token precisa ser o token raiz para ver isto". Ver a matriz na seção 9. Mesma classe de limite de taxa das rotas administrativas (`ratelimit.admin.rps`); o amostrador (D-TUI-5) faz duas chamadas a cada 5 s, bem dentro dele. Com essas rotas a API de cliente corrige a **agregação** entre nós das métricas de tópico (achado A de [31-pendencias.md](31-pendencias.md)), ou o amostrador soma a visão `scope=node` de cada nó; a mudança do servidor escolhe uma. As rotas de operação não mudam. |

## 3. O que a API oferece e o que não oferece

Conferido contra [16](16-api-cliente.md) e [17](17-api-operacao.md) em 2026-10-15. Esta tabela é o contrato entre as telas e o servidor.

| Necessidade | Rota | Situação |
|---|---|---|
| Condições do cluster, nós, memória/disco | `GET /health/cluster` (`ClusterStatus` / `NodeStatus`) | ✅ na API de operação, **sem token**; ❌ com token → D-TUI-9 (`GET /cluster/status`) |
| Descobrir o id de cada nó | `GET /health/cluster` (o `nodeId` de cada membro) | ✅ |
| Drain / estado do drain / retorno | `POST`, `GET`, `DELETE /node/drain` — **no próprio nó** | ✅ |
| Estado do backup / flush / parar o cluster | `GET /node/backup`, `POST /node/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop` | ✅ (`422 backup.not-configured` sem S3) |
| Monitoramento do nó | `GET /metrics` (texto Prometheus, **só este nó**, sem token) | ✅ para monitoramento; ❌ para a TUI → D-TUI-9 (`GET /nodes/{nodeId}/metrics`, token raiz); a TUI precisa de um leitor pequeno do texto |
| Monitoramento de tópico e grupos | `GET /metrics/topics`, `/{topic}`, `/{topic}/groups` (sem token) | ❌ com token → D-TUI-9 (`GET /stats/topics…`). ⚠️ `scope=cluster` **não** agrega entre nós hoje (achado A em [31](31-pendencias.md)): a TUI mostra a visão do nó que atende e avisa |
| **Histórico** das métricas | — | ❌ → D-TUI-5 |
| Tópicos: criar / listar / ler / alterar / apagar | `POST`, `GET`, `GET`, `PATCH`, `DELETE /topics…` | ✅ |
| Tokens: criar / listar / ler / alterar / revogar | `/tokens`, `/tokens/{id}` (só token raiz) | ✅ — permissões e prefixos são **imutáveis** (clonar, ver 6.7) |
| Publicar / consumir / ack / visibilidade | rotas de `…/messages` | ✅ |
| **Prune** (esvaziar uma fila) | — | ❌ → D-TUI-2: **criado no `taba-node`** (rota de cliente + rota de cluster) antes de a TUI usá-lo |
| **Espiar** (ler sem entregar) | — | ❌ → D-TUI-6 |
| Iniciar ou matar o **processo** de um nó | — | ❌ → D-TUI-3 |
| Chamar um **nó específico** por qualquer nó (drain, métricas, backup daquele nó) | — (as rotas de operação são por nó, sem token) | ❌ → D-TUI-3 / D-TUI-4: **criado no `taba-node`** (rota de cliente + repasse interno) |

## 4. Arquitetura

### 4.1 Formato

A interface segue o padrão *Elm*, que mantém a lógica pura e testável e deixa o ratatui como camada fina:

```text
 teclado / mouse ──► Event ──► update(Model, Msg) ──► Model' + [Effect]
                                       ▲                       │
   threads de trabalho (HTTP, bloqueante) ◄┘ Msg::Loaded/Failed │ Effect::Call(...)
                                                               ▼
                                              view(Model') ──► Frame do ratatui
```

- **`Model`**: todo o estado (aba ativa, estado de cada tela, dados carregados por último, modais, idioma). Dados puros, sem E/S.
- **`Msg`**: toda entrada — uma tecla, um tique, uma resposta de trabalhador, um redimensionamento do terminal.
- **`update`**: função pura; devolve **efeitos** (`Call`, `StartListen`, `StopListen`, `Quit`). Nunca bloqueia.
- **`view`**: desenha o modelo com widgets do ratatui. Sem decisões além do layout.
- **Trabalhadores:** o `taba-client` é **bloqueante** e o CLI não tem runtime assíncrono; então os efeitos rodam num pequeno conjunto de threads (4) que devolvem a resposta por um canal `mpsc` marcada com o *id da requisição* e uma *geração* (resposta de uma tela já abandonada é descartada). O `listen` tem thread própria com uma flag de parada.
- **Atualização:** um tique a cada 250 ms move animações e temporizadores; cada tela declara seu período de consulta (2 s nos painéis, 5 s nas listas). A atualização pausa quando o terminal perde o foco ou há um modal aberto. Uma chamada que falha mantém o último dado, marca-o como *velho* (apagado, "última atualização 14:02:09") e recua (×2, até 30 s; o `Retry-After` de `429`/`503` é obedecido).

### 4.2 Módulos (dentro de `connectors/rust/cli/src/tui/`)

```text
tui/
  mod.rs            run(): prepara/devolve o terminal (seguro contra panic), laço de eventos
  model.rs  msg.rs  update.rs            o núcleo puro
  effect.rs  worker.rs                   os efeitos e o conjunto de threads
  view/             um arquivo por tela + widgets/ (confirmação, formulário, ajuda, gráfico…)
  screens/          cluster/ topics/ tokens/ messages/ profiles/  (estado + update + view de cada)
  keymap.rs         ação ⇄ teclas (tabela única; alimenta a ajuda e o rodapé)
  i18n/             configuração do `rust-i18n`, regra de plural, formatos ; ../locales/en-US.yml pt-BR.yml
  prom.rs           leitor do texto Prometheus (métricas do nó)
  history.rs        buffers circulares dos gráficos
  theme.rs          cores, `NO_COLOR`, alternativa em ASCII
```

Valem as regras de [24-convencoes.md](24-convencoes.md): o `mod.rs` começa com um comentário `//!`, testes unitários ficam num `tests.rs` ao lado, identificadores em inglês, nenhum `unwrap()` fora de testes, e tudo que toca o mundo externo (HTTP, relógio, terminal) fica atrás de um trait para os testes injetarem falsos.

### 4.3 O terminal

Backend `crossterm`; **tela alternativa**, **modo raw**, captura de mouse (roda e clique) e **colagem em bloco** (colar um corpo JSON é um evento só). Um *hook* de panic e o `Drop` devolvem o terminal. No Windows, só `KeyEventKind::Press` é tratado (o crossterm também informa as liberações ali). `NO_COLOR` tira as cores; `TABA_CLI_ASCII=1` (ou um locale que não seja UTF-8) troca os caracteres de caixa e de gráfico por ASCII. Tamanho mínimo **80×24**; abaixo disso, um aviso centralizado (`Paragraph` numa área com `Clear`) substitui a tela. Fora de um terminal, `taba-cli tui` sai com `2` e imprime a ajuda, como o `profile`.

## 5. Teclado e navegação

**Princípio:** setas e teclas do Vim fazem a mesma coisa; as letras são mnemônicas **e iguais em todos os idiomas** (o texto do rodapé é traduzido, as teclas não); cada tela mostra suas teclas no rodapé e a lista completa em `?`.

### 5.1 Teclas globais

| Teclas | Ação |
|---|---|
| `1`…`5` · `Alt+←/→` · `[` `]` | Ir a uma aba · aba anterior / seguinte |
| `Tab` · `Shift+Tab` | **Painel** seguinte / anterior dentro da tela |
| `↑ ↓` ou `k j` | Mover a seleção / rolar uma linha |
| `PgUp PgDn` · `Ctrl+u Ctrl+d` | Rolar uma página / meia página |
| `Home End` ou `g G` | Primeiro / último |
| `← →` ou `h l` | Recolher-expandir, trocar de aba dentro da tela, andar entre campos |
| `Enter` · `Esc` | Abrir / confirmar · fechar / cancelar / voltar |
| `/` | Filtrar a lista (digite; `Enter` mantém, `Esc` limpa) |
| `s` | Alternar a coluna de ordenação; `S` inverte (onde a API ordena) |
| `r` | Atualizar agora |
| `p` | Pausar / retomar a atualização automática |
| `:` | **Paleta de comandos**: digite para achar qualquer ação de qualquer tela e executá-la (busca aproximada) |
| `L` | Escolher o idioma |
| `?` · `F1` | Ajuda (teclas desta tela, depois as globais) |
| `q` · `Ctrl+c` | Sair (pergunta se há um ouvinte rodando ou um formulário alterado) |
| `Ctrl+s` | Salvar / enviar o formulário aberto |
| Mouse | A roda rola o painel sob o ponteiro; o clique seleciona/foca; **nunca é obrigatório** |

### 5.2 Teclas por tela (resumo — o detalhe está em cada tela da seção 6)

| Tela | Teclas |
|---|---|
| Cluster | `Enter` detalhe do nó · `d` drenar · `u` devolver (desfazer o drain) · `b` flush de backup · `B` flush de **todos** os nós · `S` parar o cluster · `m` monitoramento |
| Tópicos | `n` novo · `e` editar · `Del` apagar · `Enter` abrir · `m` monitoramento · `x` bancada de mensagens |
| Tokens | `n` novo · `e` editar · `c` clonar · `Del` revogar · `y` mostrar o id |
| Mensagens | `Ctrl+Enter` publicar · `l` começar/parar de escutar · `a` ack · `v` visibilidade · `A` ack em todas · `P` prune · `b` navegar · `c` limpar a visão |

## 6. Telas

Cada tela é descrita pelo que **queremos** dela: um objetivo e requisitos numerados (`TUI-<tela>-<nn>`). Testes, pull requests e as fases da seção 11 citam estes IDs. O visual da tela é decidido quando ela for construída, com os widgets do ratatui que servirem (`Table`, `Tabs`, `Sparkline`, `Chart`, `LineGauge`, `Scrollbar`, `Clear`…); um protótipo visual foi desenhado em 2026-10-15 e descartado para manter este documento leve — as lições dele ficam como as regras de layout do TUI-FRM-09.

### 6.1 O quadro (todas as telas)

**Objetivo.** Contexto sempre visível, navegação e regras que toda tela obedece.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-FRM-01 | Uma linha de status mostra o perfil, a URL, a pior condição do cluster (palavra **e** ícone, nunca só cor) e o relógio. | `GET /health/cluster` |
| TUI-FRM-02 | Cinco abas — Cluster, Tópicos, Tokens, Mensagens, Perfis — acessíveis por `1`…`5`, `[` `]` e `Alt+←/→`. | — |
| TUI-FRM-03 | O rodapé mostra as teclas do painel em foco, o estado da atualização (`⟳ 2s`, pausado, velho com a hora do último dado bom) e o idioma. | — |
| TUI-FRM-04 | Toda lista e todo painel de texto rolam (setas, `PgUp`/`PgDn`, `Home`/`End`, roda do mouse) e mostram a barra de rolagem só quando o conteúdo excede. | — |
| TUI-FRM-05 | Toda ação tem atalho de teclado; o mouse é opcional e nunca obrigatório. | — |
| TUI-FRM-06 | Abaixo de 80×24 um aviso substitui a tela; o redimensionamento redesenha na hora. | — |
| TUI-FRM-07 | No máximo um popup por vez; `Esc` fecha o do topo; o botão padrão de um popup destrutivo é **Cancelar**. | — |
| TUI-FRM-08 | Offline, `401`/`403` e `429`/`503` aparecem no quadro (ver seção 9) sem perder o último dado bom. | — |
| TUI-FRM-09 | Regras de layout: a linha selecionada é uma faixa em negrito sobre cinza-escuro (não `reversed`, que inverte as cores da célula); todo `Sparkline` recebe `.max(limite)`; um `Gauge` nunca vai dentro de uma célula de `Table`; `NO_COLOR` e o modo ASCII funcionam. | — |
| TUI-FRM-10 | **Uma permissão que falta é explicada, nunca escondida.** O que o token não pode fazer continua visível porém desabilitado, ou é trocado no lugar por um aviso: "Seu token precisa ser o token raiz para ver isto" / "…a permissão `admin` sobre `pedidos.*`". O texto vem do campo `required` do `403 auth.forbidden` e, antes de qualquer chamada, de `GET /auth/me` mais a matriz da seção 9; o token do perfil e o que ele pode fazer aparecem na ajuda (`?`). Nunca uma área em branco, nunca um "403" seco. | `GET /auth/me` · `auth.forbidden` · D-TUI-9 |

### 6.2 Cluster — o painel

**Objetivo.** Ver de relance como o cluster está e operar seus nós.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-CLU-01 | Uma lista das condições ativas, a pior primeiro, cada uma com ícone de severidade e palavra; sem nenhuma, uma linha "saudável". `Enter` numa condição explica o que significa e o que fazer. | `GET /health/cluster` · [13](13-observabilidade.md) · [23](23-operacao.md) |
| TUI-CLU-02 | Um resumo: membros por estado, quórum disponível, versão da tabela de membros, "parcial" quando um nó não respondeu. | `ClusterStatus.summary`, `partial` |
| TUI-CLU-03 | Uma tabela de nós: id, estado, estado do drain, memória usada como barra com %, mensagens em disco, disco livre, blocos em sincronização. Um nó inacessível fica vermelho e diz isso. | `NodeStatus` (`state`, `drainState`, `storage`, `signals`) |
| TUI-CLU-04 | Dois minigráficos ao vivo do nó selecionado (memória x limite; mensagens prontas), últimos 5 minutos. | `GET /metrics` do nó |
| TUI-CLU-05 | `Enter` abre o detalhe do nó; `m` abre o monitoramento completo (6.4). | — |
| TUI-CLU-06 | Ações de nó direto da lista: `d` drenar, `u` devolver, `b` flush de backup, `B` flush de backup em todos os nós, `S` parar o cluster — cada uma atrás de uma confirmação (6.3). | `/nodes/{nodeId}/drain`, `/nodes/{nodeId}/backup/flush`, `/cluster/backup/flush`, `/cluster/stop` · D-TUI-3 |
| TUI-CLU-07 | Uma ação que não pode rodar (o token não é o token raiz, o nó não respondeu, ou o servidor não tem a rota) aparece desabilitada **com o motivo**. | D-TUI-3, D-TUI-4 |
| TUI-CLU-08 | Atualiza a cada 2 s; `p` pausa; `r` atualiza agora. Em um terminal de 120 colunas ou mais, os gráficos vão para uma coluna lateral. | — |
| TUI-CLU-09 | Com um token que não é o raiz, a aba Cluster ainda lista os nós (id, estado, estado do drain, versão, de `GET /nodes`) e troca os detalhes, os gráficos e as ações pelo aviso de TUI-FRM-10. | `GET /nodes` · D-TUI-9 |

### 6.3 Detalhe do nó e ações do cluster

**Objetivo.** Entender um nó a fundo e executar as operações que o afetam.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-NOD-01 | Identidade: id, estado, estado do drain, versão, modo (production/development), hora de início e tempo no ar. | `NodeStatus` |
| TUI-NOD-02 | Armazenamento: memória usada x limite como medidor (amarelo a 80 %, vermelho a 95 %), mensagens em memória e em disco, disco livre. | `NodeStatus.storage` |
| TUI-NOD-03 | Sinais: canais abertos, quórum, blocos em sincronização, blocos abaixo da replicação e abaixo do quórum, nós congelados. | `NodeStatus.signals` |
| TUI-NOD-04 | Bloco do backup S3, só quando configurado: acessível, pendentes, último sucesso, segmentos. Ausente em `422`. | `GET /node/backup` |
| TUI-NOD-05 | **Drain:** confirmação dizendo o que vai acontecer; depois o progresso de `pendingMessages` até `drained`. Um nó já em drain ou drenado oferece **devolver** (volta como nó novo). | `POST`/`GET`/`DELETE /nodes/{nodeId}/drain` · D-TUI-3 |
| TUI-NOD-06 | **Flush de backup** deste nó ou de todos, com um relatório: completo ou não, por nó, o que ainda está pendente, tempo esgotado. | `FlushReport`, `ClusterFlushReport` · D-TUI-3 |
| TUI-NOD-07 | **Parar cluster:** a pessoa digita o nome do cluster para habilitar o botão; o popup diz que os processos continuam rodando como `drained` e que pará-los é com o orquestrador; um relatório lista os nós e qualquer aviso (`stopList: false`). | `POST /cluster/stop` · D-TUI-3 |

### 6.4 Gráficos de monitoramento (nó e tópico)

**Objetivo.** Acompanhar os números ao longo do tempo, de um nó ou de um tópico.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-MON-01 | Uma escolha de séries. **Nó:** memória x limite, mensagens prontas, mensagens em disco, requisições em andamento, rejeições `429`, RTT do canal. **Tópico:** taxas de publicação, consumo e ack, profundidade, idade da mais antiga, latência de publicação p50/p95/p99. | `GET /metrics` · `GET /stats/topics/{t}` |
| TUI-MON-02 | Um gráfico de linhas com eixo de tempo e escala tirada dos dados (com folga); um limite, quando existe, é desenhado como segunda linha. Cada linha tem nome na legenda **e** marcador distinto. | — |
| TUI-MON-03 | Uma janela de 1, 5, 15, 30 ou 60 minutos, nunca acima da retenção (`+`/`-`), pausa (`Space`), escala automática ou fixa (`z`). | — |
| TUI-MON-04 | Uma linha de resumo: agora, mín, máx, limite, número de amostras. | — |
| TUI-MON-05 | Um aviso de que o histórico é só o que foi amostrado desde que o app abriu, até a retenção (para dias, use o Prometheus). Um gráfico aberto cedo mostra uma linha curta e diz "n min coletados" em vez de fingir uma janela cheia; nada vai ao disco. | D-TUI-5 |
| TUI-MON-06 | As taxas são calculadas a partir de contadores; um contador que diminui (nó reiniciado) é tratado como reinício, não como taxa negativa. | — |
| TUI-MON-07 | `e` exporta a série visível para CSV; o caminho é pedido num popup (a TUI nunca grava um arquivo sem perguntar). | — |
| TUI-MON-08 | **Só tópico:** uma segunda aba, Grupos — uma tabela de mensagens, parcela, taxas e idade da mais antiga por `groupId`, para achar o produtor barulhento; o grupo de excedente é marcado. | `GET /stats/topics/{t}/groups` |

**Os gráficos de 6.4, um a um.** Os gráficos de nó leem `GET /nodes/{nodeId}/metrics` (texto Prometheus, **token raiz**); os de tópico leem `GET /stats/topics/{topic}` (token com permissão sobre o tópico). Os dois alimentam o histórico da sessão da D-TUI-5, então todos têm a mesma janela e pausa.

| Gráfico | Séries (campo de origem) | Telas |
|---|---|---|
| **Nó — memória** | `taba.storage.memory.usage` contra `taba.storage.memory.limit` (o limite é uma segunda linha) | Cluster (mini), Monitoramento do nó |
| **Nó — mensagens prontas** | `taba.storage.memory.messages` | Cluster (mini), Monitoramento do nó |
| **Nó — mensagens em disco** | `taba.storage.disk.messages` | Monitoramento do nó |
| **Nó — requisições em andamento** | `http.server.active_requests` | Monitoramento do nó |
| **Nó — rejeições `429`** | taxa de `taba.ratelimit.rejected` | Monitoramento do nó |
| **Nó — RTT do canal** | `taba.cluster.channel.rtt` (média e p95), uma linha por par | Monitoramento do nó |
| **Tópico — fluxo** | `publishRate`, `consumeRate`, `ackRate` | Monitoramento do tópico, Mensagens (painel 2) |
| **Tópico — profundidade** | `depth`; `visibleMessages` e `invisibleMessages` | Monitoramento do tópico, Mensagens (painel 1) |
| **Tópico — mensagem mais antiga** | idade a partir de `oldestMessageAt` | Monitoramento do tópico, Mensagens (painel 3) |
| **Tópico — latência de publicação** | `publishLatency.p50`, `.p95`, `.p99` | Monitoramento do tópico |
| **Tópico — grupos** (tabela, não gráfico) | `GET /stats/topics/{topic}/groups` | Monitoramento do tópico, aba Grupos |

A série "atrasadas" (delayed) do painel de referência não tem fonte hoje: as métricas de tópico não têm contagem de mensagens atrasadas, então ela não é desenhada.

### 6.5 Tópicos — lista

**Objetivo.** Achar um tópico rápido, ver quais pedem atenção, e abrir ou criar um.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-TOP-01 | Uma tabela: nome, tipo (com marcas FIFO / volátil), profundidade, taxa de publicação e de consumo, idade da mais antiga, estado (a pior condição como palavra + ícone). | `GET /topics` + `GET /stats/topics` |
| TUI-TOP-02 | A paginação por cursor é invisível: rolar até o fim carrega a página seguinte; uma marca de carregamento aparece enquanto isso; `G` vai à última linha carregada. | `limit`, `cursor` |
| TUI-TOP-03 | Filtro por nome (enviado ao servidor), por tag e por condição; ordenação por coluna (`s`, `S` inverte). | `name`, `tag.<k>`, `condition`, `sort` |
| TUI-TOP-04 | Um tópico dead-letter aparece apagado e aponta para o de origem; um distribuidor mostra "—" onde a profundidade não se aplica. | `deadLetter`, `source` |
| TUI-TOP-05 | Uma linha sob a tabela resume o tópico selecionado: tipo, ttl, máximo de entregas, tags, prefixos. | `Topic` |
| TUI-TOP-06 | Teclas: `n` novo, `e` editar, `Del` apagar, `Enter` abrir, `m` monitoramento, `x` mensagens. | — |

### 6.6 Tópico — detalhe e formulário

**Objetivo.** Criar, entender, alterar e apagar um tópico sem sair da tela.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-TPF-01 | Formulário de criação com todos os campos da API: nome, tipo, FIFO, volátil, tempo ocioso, bytes máximos da mensagem, ttl, máximo de entregas, tags (repetível), prefixos (repetível). | `POST /topics` |
| TUI-TPF-02 | Validação enquanto se digita, com as regras do servidor; o botão de enviar fica desabilitado enquanto houver erro; um `4xx` do servidor mostra o `code: message` no formulário. | [16](16-api-cliente.md) |
| TUI-TPF-03 | A edição usa o mesmo formulário: tipo, FIFO e volátil aparecem travados ("não muda depois"); só os campos alterados são enviados; os três limites aceitam um número ou `default`. | `PATCH /topics/{t}` |
| TUI-TPF-04 | Apagar pede que a pessoa **digite o nome do tópico** e avisa que todas as mensagens se perdem. | `DELETE /topics/{t}` |
| TUI-TPF-05 | Abrir um tópico mostra abas: Resumo (números e condições), Configuração (este formulário), Monitoramento, Grupos, Mensagens. | — |
| TUI-TPF-06 | Sair de um formulário com alterações não salvas pergunta antes; `Ctrl+S` salva; `Esc` cancela. | — |

### 6.7 Tokens

**Objetivo.** Gerenciar tokens de acesso com segurança, tratando o segredo com cuidado.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-TOK-01 | Uma tabela: nome, id, permissões, prefixos, validade; ordenação por `id` ou `createdAt`; filtro por nome (enviado ao servidor). O segredo nunca está na lista. | `GET /tokens` |
| TUI-TOK-02 | Tokens expirados ficam vermelhos e dizem "expirado"; os que expiram em até 7 dias ficam amarelos. | `expiresAt` |
| TUI-TOK-03 | Criar: nome, as quatro permissões (`publish`, `consume`, `create`, `admin`), prefixos de tópico, data de validade ou nenhuma. | `POST /tokens` |
| TUI-TOK-04 | O segredo aparece **uma vez**, num popup: copiar (sequência OSC 52 do terminal), salvar num arquivo com modo `0600`; mascarado após 30 s ou ao perder o foco; nunca em log, nunca no histórico da paleta, nunca em texto de erro; fechar sem copiar pergunta de novo. | `token` of the `201` |
| TUI-TOK-05 | Editar muda só nome e validade (`none` remove o prazo); permissões e prefixos aparecem travados com o motivo e uma indicação do Clonar. | `PATCH /tokens/{id}` |
| TUI-TOK-06 | Clonar (`c`) abre Criar preenchido a partir do token selecionado; depois de criar, oferece revogar o antigo (padrão **Não**). | — |
| TUI-TOK-07 | Revogar pede confirmação e diz que vale em todos os nós. | `DELETE /tokens/{id}` |
| TUI-TOK-08 | Sem token raiz a aba explica o `403` e qual perfil/token está em uso, em vez de uma lista vazia. | `403` |

### 6.8 Mensagens — a bancada

**Objetivo.** Testar um tópico de ponta a ponta: publicar, escutar, inspecionar, confirmar e podar.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-MSG-01 | Abre num tópico escolhido de um seletor, ou direto da tela de Tópicos (`x`); mostra seu tipo e a profundidade atual. | `GET /stats/topics/{t}` |
| TUI-MSG-02 | **Publicar:** corpo de várias linhas, tipo de conteúdo (`json`, `text`, outro), `groupId`, `deduplicationId`, contagem de repetição; `Ctrl+Enter` envia; o resultado (id, `duplicate` ou o erro) continua visível. Em JSON uma checagem leve avisa mas não bloqueia — quem decide é o servidor. | `POST …/messages` |
| TUI-MSG-03 | **Escutar** (`l`): long polling ao vivo; cada mensagem chega já entregue (invisível para os outros pelo seu tempo de visibilidade); o **auto-ack** pode estar ligado ou desligado; `a` ack manual, `v` muda a visibilidade, `A` ack em todas. Parar com mensagens sem ack pergunta antes. | `GET …/messages?wait`, `DELETE`, `PUT …/visibility` |
| TUI-MSG-04 | Uma tabela do que foi recebido (hora, id, contagem de entregas, grupo, corpo), a mais nova primeiro; enquanto se lê, as chegadas novas não mexem na seleção (aparece um selo "↑ n novas"); guarda as últimas 1 000. | — |
| TUI-MSG-05 | Um painel de detalhe da mensagem selecionada: corpo indentado quando é JSON, com rolagem própria; contagem de entregas, invisível até, recibo; um corpo acima de 64 KiB aparece truncado com seu tamanho e pode ser carregado inteiro. | `Message` |
| TUI-MSG-06 | **Navegar** (`b`), só a pedido: um popup pergunta **quantas mensagens** (padrão 10, até o limite do servidor) e o **tempo de visibilidade** (padrão 30 s) e informa que cada leitura **conta uma entrega** e, havendo `maxDeliveries`, pode mandar a mensagem para a dead-letter (mostra o limite); pede confirmação. As mensagens lidas aparecem na lista como *invisível até hh:mm:ss*; `v` com `0` as devolve agora. Abrir a tela, escolher um tópico ou atualizar **nunca** lê a fila. | D-TUI-6 |
| TUI-MSG-07 | **Prune** (`P`): a pessoa digita o nome do tópico; o popup mostra quantas mensagens serão perdidas (`depth`, visíveis + invisíveis, do amostrador) e que **todos os nós** serão avisados; uma única visão de espera ("podando em n nós…") sem barra de progresso; no fim: sucesso com o número removido, ou falha dizendo quais nós não concluíram e que a fila pode ter ficado parcialmente podada (a pessoa pode repetir). Mensagens publicadas nesse meio-tempo não são cobertas, e o resultado diz isso. Desabilitado quando o servidor não tem a rota (`404`/`405`), com o motivo. | `DELETE /topics/{topic}/messages` · D-TUI-2 |
| TUI-MSG-08 | Um distribuidor não pode ser escutado, navegado nem podado (só copia): essas teclas ficam desabilitadas com o motivo. | [09-topicos.md](09-topicos.md) |
| TUI-MSG-09 | As permissões são respeitadas: sem `consume`, escutar/navegar/prune ficam desabilitados; sem `publish`, o painel de publicar; o rodapé diz por quê. Um `429` mostra a contagem regressiva do `Retry-After`. | `401`, `403`, `429` |
| TUI-MSG-10 | **Contadores no cabeçalho** acima da lista, atualizados pelo amostrador (a cada 5 s por padrão): **visíveis**, **invisíveis** (em processamento), profundidade e idade da mais antiga, como o "disponíveis / em voo" do SQS. | `GET /stats/topics/{t}` (`visibleMessages`, `invisibleMessages`, `depth`, `oldestMessageAt`) · D-TUI-9 |
| TUI-MSG-11 | A lista mostra só colunas mínimas: hora, id, grupo, contagem de entregas, estado (`visível` / `invisível até hh:mm:ss`), tamanho e um trecho do corpo (uma linha). A API não lista as mensagens de uma fila (não há peek); as linhas são as recebidas por escutar ou navegar (TUI-MSG-04, -06), e os contadores do cabeçalho (TUI-MSG-10) são a verdade completa sobre a fila. A tela diz isso. | D-TUI-6 |
| TUI-MSG-12 | **Pesquisa** (`/`): filtra as linhas carregadas por texto (id, grupo, corpo); `n` / `N` pulam para o próximo / anterior resultado; o trecho achado é destacado; `Esc` limpa; um contador mostra "achados / linhas". A pesquisa nunca chama o servidor. | — |
| TUI-MSG-13 | **Ordenação** (`s` abre um menu, `S` inverte a direção): por hora, id, grupo, contagem de entregas, tamanho ou invisível-até; a chave e sua seta aparecem no cabeçalho da tabela; a seleção acompanha a mensagem, não o número da linha. O padrão é hora, mais nova primeiro. | — |
| TUI-MSG-14 | **Gráficos da seleção.** Selecionar uma linha (ou o cabeçalho do tópico) abre um painel lateral com três gráficos pequenos **empilhados na vertical**, com o eixo de tempo em comum, do tópico da mensagem: (1) **mensagens — visíveis e invisíveis** (duas linhas; eixo direito para a menor); (2) **fluxo — publicadas, consumidas, confirmadas, exclusões/recebimentos vazios** como taxas; (3) **idade máxima — a mensagem mais antiga**. São os mesmos três painéis do dashboard de fila de referência, um sobre o outro para caberem num painel estreito. | `GET /stats/topics/{t}` |
| TUI-MSG-15 | Os gráficos reusam as regras do monitoramento (TUI-MON-02 a -06): janela `+`/`-`, pausa, escala tirada dos dados, legenda com nome **e** marcador, uma linha mín / média / máx / último sob cada gráfico, e o aviso "n min coletados". Os gráficos leem o mesmo histórico da sessão que a tela de Monitoramento, então abrir um não acrescenta polling. | D-TUI-5 |
| TUI-MSG-16 | `Tab` move o foco entre lista, detalhe e gráficos; `c` mostra ou esconde o painel de gráficos (escondido por padrão abaixo de 100 colunas, onde o painel de detalhe tem prioridade); o painel rola na vertical quando o terminal é baixo. | — |

### 6.9 Ajuda, paleta, idioma e perfis

**Objetivo.** Nunca ficar perdido: achar qualquer tecla ou ação, mudar o idioma, trocar de cluster.

| ID | Requisito | API / fonte |
|---|---|---|
| TUI-HLP-01 | `?` abre a ajuda da tela atual e depois as teclas globais; é gerada da mesma tabela de teclas que o programa usa, então não pode discordar dela. | `keymap.rs` |
| TUI-HLP-02 | `:` abre uma paleta de comandos: digitar acha qualquer ação de qualquer tela (busca aproximada, no idioma atual e em inglês) e a executa. | — |
| TUI-HLP-03 | `L` escolhe o idioma (English, Português); vale na hora e fica gravado. | `ui.language` |
| TUI-HLP-04 | A aba Perfis lista, cria, edita, remove e define o perfil padrão, e testa uma conexão com valores ainda não salvos; ela substitui a tela de perfis de hoje. | [20-cli.md](20-cli.md) · D-TUI-8 |
| TUI-HLP-05 | Trocar de perfil reconecta todas as telas, zera os históricos e mostra o novo perfil na linha de status. | — |

## 7. Dados e atualização

| Tela | Chamadas | Período | Notas |
|---|---|---|---|
| Cluster | `GET /cluster/status` · por nó selecionado: `GET /nodes/{nodeId}/metrics`, `GET /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup` | 2 s · 5 s no detalhe | Tudo passa pela única URL do perfil; o nó que recebe alcança o nó certo (D-TUI-4) |
| Lista de tópicos | `GET /topics` (paginado) · `GET /stats/topics?fields=name,depth,publishRate,consumeRate,oldestAge,worstCondition` | 5 s | Estatísticas juntadas por `name` |
| Monitoramento de tópico | `GET /stats/topics/{t}` (as séries vêm do amostrador de fundo) · `/stats/topics/{t}/groups?top=20` | `history.interval` (5 s) | Os grupos só com a tela aberta |
| Tokens | `GET /tokens` (paginado) | 10 s e após cada mudança | |
| Mensagens | `GET …/messages` (escutar/navegar), `POST`, `DELETE`, `PUT …/visibility` | sob demanda | O rótulo de profundidade vem do amostrador (`GET /stats/topics/{t}`) a cada 5 s |

O **texto Prometheus** (`GET /nodes/{nodeId}/metrics`) é lido por `prom.rs`: só nomes, rótulos e valores (`# TYPE` para os contadores, de modo que uma taxa é a diferença entre duas amostras dividida pelo tempo). Contador que reinicia (nó reiniciado) é detectado pelo valor que diminui.

**Histórico** (`history.rs`): um **amostrador** em segundo plano começa com o app e, a cada `history.interval` (padrão **5 s**, mínimo 1 s), lê `GET /cluster/status` (carga, memória e disco de todos os nós, numa chamada) e `GET /stats/topics` (todos os tópicos do cluster, numa chamada) para um buffer circular de tamanho fixo por série; o tamanho é `history.retention / history.interval` (padrão 1 h / 5 s = 720 amostras). Ele continua rodando seja qual for a tela aberta, pausa num `429` (respeitando o `Retry-After`) e é zerado ao trocar de perfil. A memória é limitada: séries × amostras × 16 bytes (cerca de 90 KB por tópico com 8 séries), e o número de tópicos acompanhados tem teto (padrão 200, os mais ativos primeiro; o teto aparece na linha de status quando se aplica). As opções ficam no arquivo de configuração (`[history]`), com os padrões acima; uma retenção acima de 6 h ou um intervalo abaixo de 1 s é recusado com uma mensagem.

## 8. Idiomas (i18n)

- **Catálogo** (D-TUI-7): `connectors/rust/cli/locales/en-US.yml` e `pt-BR.yml`, carregados por `rust_i18n::i18n!("locales", fallback = "en-US")` e embutidos no binário. Uma chave por texto, `%{nome}` como variável, formas de plural como `chave.one` / `chave.other` (uma pequena regra `plural(idioma, n)` por idioma escolhe). O inglês (`en-US`) é a **fonte**; todo outro arquivo precisa ter exatamente as mesmas chaves.
- **Uso:** `t!("cluster.drain.confirm.title")`, `t!("topics.count", n = 24)`. Chave que falta num idioma cai para o `en-US` e depois para a própria chave; nunca derruba a tela, e o teste de paridade a pega antes do release.
- **Escolha do idioma**, vale o primeiro que existir: `--lang <código>` · `TABA_CLI_LANG` · `ui.language` no arquivo de configuração (gravado por `L`) · o locale do sistema (`LC_ALL`, `LC_MESSAGES`, `LANG`; no Windows, o idioma da interface do usuário) · `en-US`. `pt`, `pt_BR`, `pt-BR.UTF-8` casam com `pt-BR`.
- **O que se traduz:** todo rótulo, título, dica de rodapé, mensagem de validação, popup, estado vazio e texto de ajuda. **O que não:** teclas, rotas, campos JSON, **códigos** de erro (`topic.not-found`), nomes de métricas, nomes de condições — são identificadores ([24-convencoes.md](24-convencoes.md)). O texto `message` do próprio servidor aparece como veio, depois da moldura traduzida ("O servidor respondeu:").
- **Segurança de layout:** as larguras seguem as regras do `unicode-width` que o `ratatui` já traz (sem crate extra); as colunas se dimensionam pelo cabeçalho **traduzido**; o pt-BR é ~20–30 % mais longo que o en-US, então as telas de 80 colunas são testadas nos **dois** idiomas.
- **Formatos:** datas e durações por idioma (`2026-10-15 14:02` x `15/10/2026 14:02`; `3 s`, `2 h 11 min`); agrupamento de números (`1,204` x `1.204`); tamanhos com unidades binárias (`MiB`, `GiB`) nos dois.
- **Acrescentar um idioma:** copiar o `en-US.yml`, traduzir, incluir o código na tabela de idiomas e (se tiver regra de plural diferente das duas conhecidas) em `plural()`; o teste de paridade diz o que falta. A regra de documentação de [24-convencoes.md](24-convencoes.md#documentação-em-vários-idiomas) continua valendo para este plano: as pastas de idioma mudam juntas.

## 9. Erros, segurança e resiliência

| Situação | Comportamento |
|---|---|
| Servidor inacessível / tempo esgotado | O cabeçalho mostra `✖ offline`; dados apagados com a idade; nova tentativa com recuo; ações desabilitadas; nunca um travamento |
| `401` / `403` | A aba mostra que permissão falta (`só token raiz`, `admin`…) e qual perfil/token está em uso; ações que precisam dela ficam escondidas, não cinzas sem motivo |
| `429` / `503` | Obedece o `Retry-After`; o rodapé mostra a contagem regressiva |
| `4xx` de formulário | `code: message (requestId …)` na linha de erro do formulário; o id da requisição pode ser copiado (`y`) para os logs do servidor |
| Um nó não responde à chamada repassada | A ação diz qual nó falhou e mantém os dados dos outros |
| Ação perigosa | Popup de confirmação, padrão **Cancelar**; confirmação digitada para *parar cluster*, *apagar tópico*, *prune* |
| Segredos | O segredo do token é mascarado após 30 s; nunca em log, histórico, paleta ou texto de erro; o `token` do perfil aparece como hoje (`token-file` é preferível); copiar por OSC 52 ou um arquivo `0600` |
| Terminal pequeno / redimensionado | Redesenha ao redimensionar; abaixo de 80×24 um aviso substitui a tela |
| Panic | O hook devolve o terminal primeiro e só depois imprime o panic |
| Disco lento / corpos grandes | Corpos acima de 64 KiB aparecem truncados com o tamanho (`Enter` carrega o resto no painel de detalhe); *Recebidas* guarda as últimas 1 000 mensagens |

**O que cada área pede do token.** A TUI usa esta tabela para habilitar ou desabilitar ações antes de chamar; quem decide continua sendo o servidor.

| Área / ação | Token mínimo |
|---|---|
| Quem sou eu (`GET /auth/me`), lista de nós (`GET /nodes`) | qualquer token válido |
| Tópicos: listar, ler, estatísticas e grupos | qualquer token válido, **dentro dos seus prefixos** |
| Criar um tópico | `create` (nome dentro dos prefixos) |
| Alterar ou apagar um tópico | `admin` sobre o tópico |
| Publicar | `publish` |
| Escutar, navegar, ack, mudar visibilidade | `consume` |
| Prune de uma fila | `admin` sobre o tópico |
| Tokens (listar, criar, apagar) | token raiz |
| Detalhe do cluster, métricas de nó, drain, retorno, backup, parar o cluster | token raiz |


## 10. Testes

| Nível | O quê |
|---|---|
| **Unitário (núcleo puro)** | `update()` por tela: sequências de teclas entrando, modelo e efeitos saindo — sem terminal, sem rede. Navegação, filtros, formulários e sua validação, fluxos de confirmação, buffer circular do histórico, `prom.rs`, unicidade do mapa de teclas (nenhuma ação repetida na mesma tecla num contexto) |
| **Desenho** | `TestBackend` do ratatui: cada tela desenhada em **80×24** e **120×40**, em **en-US e pt-BR**, comparada com instantâneos de texto guardados (arquivos irmãos em `tests/`, em texto simples para o diff ser legível). Ao menos um instantâneo por tela; os requisitos da seção 6 (`TUI-…`) são os critérios de aceite e cada teste diz os IDs que cobre |
| **i18n** | Paridade (mesmas chaves, mesmas variáveis em todos os arquivos YAML); nenhuma chave usada no código falta no catálogo; toda chave do catálogo é usada |
| **Integração** | Os trabalhadores com um **`Transport` falso** (`taba_client::testing`, que já existe): respostas roteirizadas da API → Msg → modelo. Inclui o ouvinte (long poll, ack, parada), a paginação, o recuo de `429` e o dado velho |
| **Contra um nó** | Opcional: os testes de cluster do `taba-node` sobem um nó real em desenvolvimento; um cenário dirige o núcleo da TUI sem tela por criar tópico → publicar → escutar → ack → prune → apagar token. Os códigos de caso de uso da TUI (uma área nova) entram no catálogo quando isto for construído (os arquivos gerados 26a/26b seguem a regra de geração do repositório, não edição à mão) |
| **Lista manual** | Windows Terminal, PowerShell, cmd, Git Bash (mintty), Terminal/iTerm do macOS, console do Linux, `tmux`, SSH, 80×24 e uma janela 4K — uma tabela no PR |

Nota de disco (pouco espaço nesta máquina, ver a memória do projeto): rodar só os testes da crate do CLI (`cargo test -p taba-cli`) e `cargo clean -p taba-cli` depois; ratatui e crossterm aumentam o tempo e o tamanho da compilação.

## 11. Fases

Cada fase termina com algo utilizável, testes verdes e a documentação atualizada **em todas as pastas de idioma**.

| Fase | Entrega | Pronta quando |
|---|---|---|
| **0 — Fundação** | Feature `tui`, entrada `taba-cli tui`, preparo/devolução do terminal, laço de eventos, núcleo Elm, trabalhadores, quadro (6.1), mapa de teclas + ajuda (`?`) + paleta (`:`), catálogo i18n com os dois idiomas, tema/`NO_COLOR`, popup de confirmação | As abas vazias navegam; a troca de idioma funciona; os testes de desenho e de i18n passam |
| **1 — Cluster** | 6.2, 6.3 (primeiro só leitura, com o que existe hoje; as ações depois da mudança no servidor das D-TUI-3/4), `prom.rs`, `history.rs`, gráficos do nó (6.4) | Bate com `taba-cli health cluster` e `drain *`; um drain e um retorno feitos pela TUI no cluster do Compose |
| **2 — Tópicos** | 6.5, 6.6, monitoramento de tópico e grupos (6.4) | Criar/editar/apagar iguais aos comandos `topic *`; a paginação com mais de 1 000 tópicos é suave |
| **3 — Tokens** | 6.7 com popup do segredo e clonagem | Igual aos comandos `token *`; o segredo nunca fica visível depois de fechar |
| **4 — Mensagens** | 6.8 publicar, escutar, ack, visibilidade, navegar (D-TUI-6) | Uma mensagem publicada na TUI é recebida na mesma TUI e pelo `taba-cli consume` |
| **5 — Prune** | **Servidor primeiro:** a rota de cliente, a rota de cluster e o repasse no `taba-node` (D-TUI-2), com as docs 16 e 18, erros, caso de uso e testes; depois a ação na TUI | O prune num cluster Compose de 3 nós esvazia o tópico em todos os nós e responde sucesso; com um nó parado responde falha dizendo qual |
| **6 — Perfis e limpeza** | Aba Perfis (D-TUI-8); remover `tui/term.rs` e `tui/keys`; acabamento: mouse, modo ASCII, layouts estreitos | O código antigo da TUI de perfis some; `taba-cli profile` não muda para quem usa |
| **7 — Docs e release** | Atualizar [20-cli.md](20-cli.md), [05-dependencias.md](05-dependencias.md), [24-convencoes.md](24-convencoes.md) (regra de zero dependências do CLI), [21-conectores.md](21-conectores.md) se preciso, o `README.md` do CLI, scripts de build e Dockerfile (feature ligada), [30-decisoes.md](30-decisoes.md); capturas/gravações | `verificar.mjs` e `verificar-idiomas.mjs` passam; os binários dos seis alvos compilam |

**Riscos.** (1) Os alvos estáticos `musl` e `arm64` precisam continuar compilando com ratatui/crossterm — conferir na fase 0 antes de qualquer outra coisa. (2) O binário cresce; medir contra o `dist/` de hoje e registrar. (3) As ações de nó dependem da mudança no servidor das D-TUI-3/4; até ela existir ficam desabilitadas com o motivo. (4) O *prune* depende de uma mudança no servidor: até ela existir, a TUI mostra a ação desabilitada; nunca afirma uma fila esvaziada que não consegue provar (uma falha pode deixar a fila parcial, e a tela diz isso). (5) Uma mensagem podada não pode voltar de uma réplica, da sincronização quando um nó retorna, nem do backup no S3: a mudança no servidor precisa definir isso (ver seção 12).

## 12. Impacto no que existe

| O quê | Mudança |
|---|---|
| `connectors/rust/cli/Cargo.toml` | Feature `tui` (ligada por padrão): `ratatui`, `crossterm`, `rust-i18n`, `sys-locale` |
| `src/app.rs`, `src/commands/mod.rs` | Novo comando `tui`; `--lang` global |
| `src/profile`, `src/settings` | Novas opções `ui.language` e `[history]` (arquivo); valem as regras de precedência de [20-cli.md](20-cli.md) (`TABA_CLI_LANG`). **Nenhum campo novo no perfil** |
| `src/tui/` | A nova árvore de 4.2; `term.rs` e `keys` antigos removidos na fase 6 |
| Servidor | ✅ **Feito no `taba-node` em 2026-10-15 (D152 a D155 de [30-decisoes.md](30-decisoes.md); rotas em [16-api-cliente.md](16-api-cliente.md), [18-api-cluster.md](18-api-cluster.md), erros em [19-erros-e-limites.md](19-erros-e-limites.md); testes em `tests/nodes_api.rs`).** O que mudou do plano: a chamada interna é um novo `/cluster/node-call/{caminho}`, não o `/cluster/forward/{caminho}`; `GET /stats/topics` é a **do cluster** (D156), então o amostrador faz uma chamada e não soma nós; toda leitura consolidada diz `partial` e `missingNodes`, e `GET /cluster/status` traz a carga de cada nó (`runtime`); `GET /cluster/backup` consolida o backup; `required` é um texto (`root` ou o nome de uma permissão); um prune não cobre um nó sincronizando (PRB-058), e um nó que saiu por drain só é endereçável enquanto o receptor se lembra dele (PRB-061). O trabalho da TUI em si ainda está por fazer. Requisito original: **Obrigatório para as ações de nó e o monitoramento (D-TUI-3/4/9, fase 1):** `GET /nodes`, `GET /auth/me`, `GET /cluster/status`, `GET /stats/topics…` e o campo `required` do `403 auth.forbidden`; e, para as ações, as rotas `/nodes/{nodeId}/…` e as do cluster todo na API de cliente com o token raiz (doc 16), o repasse interno ao nó alvo (doc 18), erros, casos de uso e testes (nó alvo fora do ar, token sem `admin`, alvo drenado). **Obrigatório para o prune (D-TUI-2, fase 5):** `DELETE /topics/{topic}/messages` na API de cliente (doc 16), `POST /cluster/topics/{topic}/prune` na API do cluster (doc 18), o repasse e a poda local (armazenamento, lista de prontas, mensagens invisíveis, réplicas), seus `ErrorCode`s, caso de uso e testes, inclusive um nó fora do ar durante o prune e um nó que volta depois (não pode ressuscitar mensagens podadas). Nada mais é preciso no servidor |
| Documentação | Este arquivo; depois os arquivos da fase 7, em `pt-BR` e `en-US` juntos |

## 13. Perguntas em aberto

| # | Pergunta | Padrão se não for respondida |
|---|---|---|
| Q1 | Um perfil deve poder guardar `start-command`/`stop-command` opcionais por nó (ex.: `docker compose start taba-2`) para a TUI realmente subir e matar processos? | Não — fora da API; a D-TUI-3 mantém só drain/retorno |
| Q2 | O histórico de um gráfico deve sobreviver a sair da tela? | **Decidido:** sim — o amostrador roda durante toda a sessão, 1 h a 5 s por padrão, configurável (D-TUI-5) |
| Q3 | Ponto de entrada: só `taba-cli tui`, ou também `taba-cli` sem argumentos num terminal? | Só `tui`; sem argumentos continua imprimindo a ajuda |
| Q4 | Exportar pela TUI (CSV de um gráfico, corpo de mensagem para arquivo): manter ou cortar para reduzir o escopo? | Manter CSV e exportação de corpo, os dois com popup de caminho |
| Q5 | Nível de suporte ao mouse: roda e clique, ou só teclado? | Roda e clique, opcional (`--no-mouse`) |

## Fontes e provas

- O CLI de hoje: `connectors/rust/cli/src/{app.rs,commands,profile,settings,tui}` (terminal raw próprio, tela de perfis, trait `Services` para testes) e `connectors/rust/client/src` (`Client` bloqueante, transporte `testing`); [20-cli.md](20-cli.md).
- As rotas, parâmetros e DTOs: [16-api-cliente.md](16-api-cliente.md), [17-api-operacao.md](17-api-operacao.md); a ausência de rota de purge/prune e de rota de espiar foi conferida buscando nesta pasta e na API de cliente em 2026-10-15.
- Métricas, condições e as condições de tópico "memory-pressure/backlog/stalled/noisy-group": [13-observabilidade.md](13-observabilidade.md).
- Regra de dependências que será alterada: [05-dependencias.md](05-dependencias.md). Versões das bibliotecas: `cargo search` em 2026-10-15 (ratatui 0.30.2, crossterm 0.29.0).
- Nada deste plano está implementado; ainda não há testes nem casos de uso para ele.
