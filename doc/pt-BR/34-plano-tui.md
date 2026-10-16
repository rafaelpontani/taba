---
título: Plano — interface interativa (TUI) do taba-cli
resumo: Plano do `taba-cli tui`, uma interface em tela cheia (Rust + ratatui) para acompanhar e operar um cluster, gerenciar tokens e tópicos e testar mensagens — sobre rotas que o servidor já tem. Traz as decisões, a arquitetura, os requisitos por tela, o mapa de teclas e as fases.
tipo: plano
status: parcial
verificado-em: 2026-10-15 @ 44437e9
fontes: [connectors/rust/cli/src, connectors/rust/client/src, doc/pt-BR/16-api-cliente.md, doc/pt-BR/17-api-operacao.md, doc/pt-BR/13-observabilidade.md, doc/pt-BR/20-cli.md]
---

# Plano — interface interativa (TUI) do `taba-cli`

> **Status: 🟨 fases 0, 1 e 3 feitas e a aba Perfis da fase 5 (2026-10-15); faltam as fases 2 e 4, o fim da fase 5 e a 6.** Todas as rotas que a TUI chama já existem na API de cliente (feitas no `taba-node` em 2026-10-15: D152 a D156 de [30-decisoes.md](30-decisoes.md)). A fundação e a aba Cluster estão em `connectors/rust/cli` (registro na seção 11); a aba Tokens e a aba Perfis estão feitas (registro na seção 11); faltam as abas Tópicos e Mensagens. Contexto: [20-cli.md](20-cli.md) (comandos e perfis de hoje), [16-api-cliente.md](16-api-cliente.md) (as rotas), [13-observabilidade.md](13-observabilidade.md) (métricas e condições).

## 1. Objetivo e escopo

`taba-cli tui` abre uma interface em tela cheia ([ratatui](https://ratatui.rs)) sobre o mesmo conector (`taba-client`) e os mesmos perfis dos comandos. É uma tela sobre a API existente: **não cria nada no servidor**. Sem comando (`taba-cli`) ela abre a mesma interface quando há terminal.

| Área | O que a pessoa faz |
|---|---|
| **Cluster** | Ver condições e o estado de cada nó (memória, disco, sincronização); **drenar** e **devolver** um nó; **flush de backup**; **parar o cluster**; gráficos |
| **Tokens** | Listar, **criar** (o segredo aparece uma vez), **alterar** nome e validade, **revogar**, clonar |
| **Tópicos** | Listar, filtrar, **criar**, **alterar**, **apagar**; gráficos e grupos |
| **Mensagens** | **Publicar**, **escutar**, **navegar**, **ack** e visibilidade, **prune** |
| **Perfis** | A tela de perfis de hoje, refeita (D-TUI-8): listar, criar, editar, remover, escolher o padrão, testar a conexão e **conectar** com outro perfil; ao abrir sem perfil informado, a escolha do perfil |

Valem para todas as telas: tudo rola (setas, `PgUp`/`PgDn`, `Home`/`End`, roda do mouse); **toda ação tem atalho** e o mouse nunca é obrigatório; textos em `en-US` e `pt-BR`; ação que apaga ou interrompe pede confirmação; o segredo do token nunca vai para um log.

**Fora do escopo:** iniciar ou matar o *processo* de um nó (é do orquestrador), editar a configuração do servidor e substituir os comandos do `taba-cli` (eles ficam; a TUI só acrescenta).

## 2. Decisões

Tomadas pelo dono do projeto; o plano as segue. Ao construir, as que mudam regra do repositório entram em [30-decisoes.md](30-decisoes.md).

| Código | Decisão |
|---|---|
| **D-TUI-1** | **Dois modos.** Os comandos diretos (`topic list`, `token create`, `drain`…) rodam como hoje: saída simples, sem `ratatui`. Só o modo interativo (`taba-cli tui` e a aba Perfis) liga o `crossterm` (modo raw, tela alternativa, devolvida ao sair e em panic) e o `ratatui`. Ambos ficam atrás da feature `tui` do Cargo, **ligada por padrão** nos binários e na imagem Docker; `--no-default-features` dá o CLI enxuto de hoje. O `taba-client` continua sem dependências. Fixar versões ao começar (ratatui 0.30.x, crossterm 0.29.x). |
| **D-TUI-2** | **Prune: só pela API de cliente.** `DELETE /topics/{topic}/messages` (permissão `admin`; D152). O servidor repassa aos outros nós, é idempotente por `pruneId` e **não tem repetição**: na falha, a pessoa repete. A TUI não tem laço no cliente. |
| **D-TUI-3** | **Ações de nó e de cluster pelo token raiz**, sempre pela única URL do perfil: `POST`/`GET`/`DELETE /nodes/{nodeId}/drain`, `GET /nodes/{nodeId}/backup`, `POST /nodes/{nodeId}/backup/flush`, `POST /cluster/backup/flush`, `POST /cluster/stop`. *Parar nó* = drain; *Iniciar nó* = devolver (volta como nó novo). Subir ou matar o processo **não** é oferecido. |
| **D-TUI-4** | **Rotas de nó** (`/nodes/{nodeId}/…`, ids de `GET /nodes`): o nó que recebe responde se for ele, senão chama o dono uma vez; `nodeId` desconhecido é `404 node.not-found`; um nó que não responde é `502` nomeando-o (D154). **Sem URL por nó no perfil.** |
| **D-TUI-5** | **Histórico só na sessão.** Desde que o app abre, um amostrador em segundo plano guarda **1 h a cada 5 s** (720 amostras por série) em buffers circulares; configurável (`history.retention`, `history.interval`; `--history-retention`, `--history-interval`). Nada vai ao disco. Para histórico longo, o Prometheus ([13-observabilidade.md](13-observabilidade.md)). |
| **D-TUI-6** | **A TUI nunca lê uma fila por conta própria.** Ler é *entregar*; só acontece quando a pessoa pede (*Navegar* ou *Escutar*). O *Navegar* pergunta **quantas mensagens** e o **tempo de visibilidade**, e avisa que cada leitura **conta uma entrega** (`deliveryCount`) e pode mandar a mensagem à dead-letter (mostra `maxDeliveries`). Há "devolver agora" (`PUT …/visibility` com `0`). Nunca faz ack sozinha. |
| **D-TUI-7** | **Traduções:** `rust-i18n` 4.2.x + `sys-locale` 0.3.x; catálogos YAML embutidos (`locales/en-US.yml`, `locales/pt-BR.yml`), `t!("chave", nome = valor)`, plural por `chave.one`/`chave.other`; `en-US` é a fonte e a reserva. Um teste nosso compara chaves e variáveis dos catálogos. |
| **D-TUI-8** | **Tela de perfis antiga** (`tui/term.rs`, `tui/keys`): mantida até a fase 5; a aba **Perfis** já existe (2026-10-15), e a tela antiga só será apagada quando o `taba-cli profile` passar a abrir a TUI. `taba-cli profile` continua abrindo-a. |
| **D-TUI-9** | **Leituras da TUI usam o token, nunca a API de operação** (essa segue sem token, para Prometheus e balanceador). Quem vê o quê: qualquer token válido vê `GET /nodes` (sem endereços), `GET /auth/me` e `/stats/topics…` (filtrado pelos seus prefixos); só o **token raiz** vê `GET /cluster/status`, `GET /cluster/backup` e tudo de um nó. Um `403 auth.forbidden` traz `required` (`root` ou o nome da permissão), e a TUI o usa para dizer o que falta (TUI-FRM-10). |
| **D-TUI-10** | **Leituras consolidadas, com aviso de incompleto (D156).** `GET /stats/topics…`, `/nodes`, `/cluster/status` e `/cluster/backup` já são **do cluster**, e trazem `partial` e `missingNodes: [{nodeId, reason}]` quando um nó não respondeu em 5 s. A TUI **mostra sempre um selo "incompleto: nó X não respondeu"** quando `partial` é `true`, nunca soma nós por conta própria, e `?scope=node` fica só para a tela de detalhe do nó. |

## 3. As rotas que as telas chamam

Todas existem. A tabela é o contrato entre as telas e o servidor (detalhes em [16-api-cliente.md](16-api-cliente.md)).

| Necessidade | Rota | Token |
|---|---|---|
| Quem sou eu | `GET /auth/me` | qualquer |
| Lista de nós (id, estado, drain, versão) | `GET /nodes` | qualquer |
| Condições, nós, memória, disco, carga (`runtime`) | `GET /cluster/status` | raiz |
| Backup S3 de todos os nós | `GET /cluster/backup` (`422 backup.not-configured` sem S3) | raiz |
| Métricas de um nó (texto Prometheus) | `GET /nodes/{nodeId}/metrics` | raiz |
| Drain / estado / devolver | `POST`, `GET`, `DELETE /nodes/{nodeId}/drain` | raiz |
| Backup de um nó / flush | `GET /nodes/{nodeId}/backup`, `POST …/backup/flush` | raiz |
| Flush de todos / parar o cluster | `POST /cluster/backup/flush`, `POST /cluster/stop` ¹ | raiz |
| Estatísticas de tópico e grupos | `GET /stats/topics`, `/{topic}`, `/{topic}/groups` | qualquer (prefixos) |
| Tópicos | `POST`, `GET`, `PATCH`, `DELETE /topics…` | `create` / `admin` |
| Tokens | `/tokens`, `/tokens/{id}` (permissões e prefixos são **imutáveis**: clonar) | raiz |
| Mensagens | `…/messages` (publicar, consumir, ack, visibilidade) | `publish` / `consume` |
| **Prune** | `DELETE /topics/{topic}/messages` | `admin` no tópico |

¹ Só montadas quando a API de operação tem porta própria (PRB-060). Sem ela a TUI as mostra desabilitadas com o motivo.

**Limites que a TUI aceita e mostra:** não há "espiar" (D-TUI-6); não há contagem de mensagens atrasadas (a série "atrasadas" não é desenhada); percentis de latência e número de grupos do cluster são **aproximações** (o maior valor entre os nós, PRB-059); o prune não cobre um nó sincronizando (PRB-058) nem mensagens publicadas durante ele, e o resultado diz isso; um nó em drain só é endereçável enquanto o receptor se lembra dele (PRB-061).

## 4. Arquitetura

Padrão *Elm*: lógica pura e testável, ratatui como camada fina.

```text
 teclado / mouse ──► Event ──► update(Model, Msg) ──► Model' + [Effect]
                                       ▲                       │
   threads de trabalho (HTTP, bloqueante) ◄┘ Msg::Loaded/Failed │ Effect::Call(...)
                                                               ▼
                                              view(Model') ──► Frame do ratatui
```

- **`Model`** é todo o estado, sem E/S. **`Msg`** é toda entrada (tecla, tique, resposta, redimensionamento). **`update`** é pura e devolve **efeitos** (`Call`, `StartListen`, `StopListen`, `Quit`); nunca bloqueia. **`view`** só desenha.
- **Trabalhadores:** o `taba-client` é bloqueante; os efeitos rodam em 4 threads e devolvem por um canal `mpsc` marcado com *id da requisição* e *geração* (resposta de tela abandonada é descartada). O `listen` tem thread própria com flag de parada.
- **Atualização:** tique de 250 ms; consulta a cada 2 s nos painéis e 5 s nas listas; pausa sem foco ou com modal aberto. Uma chamada que falha mantém o último dado, marca-o como *velho* e recua (×2, até 30 s; `Retry-After` de `429`/`503` é obedecido).
- **Chamadas:** o conector tem só `Client::call` para o que não é tópico ou mensagem; a TUI ganha, dentro de `tui/`, um módulo `api` com funções tipadas por rota (`cluster_status`, `node_drain`, `prune`…) sobre `Client::call`. **Não se acrescenta nada ao `taba-client`.**

```text
tui/
  mod.rs            run(): prepara/devolve o terminal (seguro contra panic), laço de eventos
  model.rs msg.rs update.rs effect.rs worker.rs   o núcleo e os trabalhadores
  api.rs            as rotas da seção 3, tipadas, sobre Client::call
  screens/          cluster/ topics/ tokens/ messages/ profiles/  (estado + update + view)
  view/widgets/     confirmação, formulário, ajuda, gráfico, selo de incompleto…
  keymap.rs         ação ⇄ teclas (tabela única; alimenta a ajuda e o rodapé)
  i18n/  ../locales/en-US.yml pt-BR.yml
  prom.rs           leitor do texto Prometheus (métricas do nó)
  history.rs        buffers circulares
  theme.rs          cores, `NO_COLOR`, alternativa em ASCII
```

Valem as regras de [24-convencoes.md](24-convencoes.md): `mod.rs` começa com `//!`, testes em `tests.rs` ao lado, identificadores em inglês, sem `unwrap()` fora de testes, e tudo que toca o mundo externo (HTTP, relógio, terminal) fica atrás de um trait.

**Terminal:** `crossterm`, tela alternativa, modo raw, mouse e colagem em bloco; hook de panic e `Drop` devolvem o terminal; no Windows só `KeyEventKind::Press`. `NO_COLOR` tira as cores; `TABA_CLI_ASCII=1` (ou locale não UTF-8) usa ASCII. Mínimo **80×24**, abaixo um aviso. Fora de um terminal, `taba-cli tui` sai com `2` e imprime a ajuda.

## 5. Teclado

Setas e teclas do Vim fazem o mesmo; as letras são mnemônicas **e iguais em todos os idiomas**; o rodapé mostra as teclas do painel e `h` a lista completa.

| Teclas | Ação |
|---|---|
| `1`…`5` · `[` `]` · `Alt+←/→` | Ir a uma aba · anterior / seguinte |
| `Tab` · `Shift+Tab` | Painel seguinte / anterior |
| `↑ ↓` `k j` · `PgUp PgDn` `Ctrl+u Ctrl+d` · `Home End` `g G` | Mover, rolar, primeiro / último |
| `← →` `h l` | Recolher-expandir, trocar de aba interna, andar entre campos |
| `Enter` · `Esc` | Abrir / confirmar · fechar / cancelar |
| `/` · `s` `S` · `r` · `p` | Filtrar · ordenar / inverter · atualizar agora · pausar |
| `:` | Paleta de comandos (busca aproximada de qualquer ação) |
| `l` · `o` · `h` `?` `F1` · `q` `Ctrl+c` · `Ctrl+s` | Idioma · trocar de perfil · ajuda · sair (pergunta se há ouvinte ou formulário alterado) · enviar formulário |
| Mouse | A roda rola o painel sob o ponteiro; o clique foca; opcional (`--no-mouse`) |

| Tela | Teclas |
|---|---|
| Cluster | `Enter` detalhe · `d` drenar · `u` devolver · `b` flush deste nó · `B` flush de todos · `S` parar o cluster · `m` monitoramento |
| Tópicos | `n` novo · `e` editar · `Del` apagar · `Enter` abrir · `m` monitoramento · `x` mensagens |
| Tokens | `n` novo · `e` editar · `c` clonar · `Del` `x` revogar · `/` filtrar · `s` ordenar |
| Perfis | `Enter` conectar · `n` novo · `e` editar · `d` padrão · `Del` `x` remover · `t` testar |
| Mensagens | `Ctrl+Enter` publicar · `l` escutar · `a` ack · `v` visibilidade · `A` ack em todas · `P` prune · `b` navegar · `c` gráficos |

## 6. Requisitos por tela

Requisitos numerados (`TUI-<tela>-<nn>`); testes e fases citam os IDs. O visual é decidido na construção, com `Table`, `Tabs`, `Sparkline`, `Chart`, `LineGauge`, `Scrollbar` e `Clear`.

### 6.1 O quadro (todas as telas)

| ID | Requisito |
|---|---|
| TUI-FRM-01 | Linha de status: perfil, URL, pior condição do cluster (palavra **e** ícone, nunca só cor) e relógio. Fonte: `GET /cluster/status`; com token comum, só `GET /nodes`. |
| TUI-FRM-02 | Cinco abas (Cluster, Tópicos, Mensagens, Tokens, Perfis) por `1`…`5`, `[` `]`, `Alt+←/→`. |
| TUI-FRM-03 | Rodapé: teclas do painel em foco, estado da atualização (`⟳ 2s`, pausado, velho com a hora do último dado bom) e idioma. |
| TUI-FRM-04 | Toda lista e todo texto rolam; a barra de rolagem só aparece quando o conteúdo excede. |
| TUI-FRM-05 | Toda ação tem atalho; o mouse é opcional. Caracteres digitados com AltGr (`/`, `?`, `@` no ABNT2 e outros layouts; o Windows informa Ctrl+Alt) funcionam como teclas e nos campos de texto. |
| TUI-FRM-06 | Abaixo de 80×24 um aviso substitui a tela; redimensionar redesenha na hora. |
| TUI-FRM-07 | Um popup por vez; `Esc` fecha o do topo; o botão padrão de um popup destrutivo é **Cancelar**. |
| TUI-FRM-08 | Offline, `401`/`403`, `429`/`503` e **`partial: true`** (D-TUI-10) aparecem no quadro sem perder o último dado bom. |
| TUI-FRM-09 | Layout: a linha selecionada é faixa em negrito sobre cinza-escuro (não `reversed`); todo `Sparkline` recebe `.max(limite)`; `Gauge` nunca dentro de célula de `Table`; `NO_COLOR` e ASCII funcionam. |
| TUI-FRM-11 | `taba-cli` sem comando abre a interface quando a entrada e a saída são um terminal; senão imprime a ajuda. |
| TUI-FRM-10 | **Permissão que falta é explicada, nunca escondida:** o que o token não pode fica visível e desabilitado, ou trocado por um aviso ("seu token precisa ser o token raiz para ver isto"). O texto vem de `required` e, antes de qualquer chamada, de `GET /auth/me` com a matriz da seção 9. |

### 6.2 Cluster e detalhe do nó

| ID | Requisito | Fonte |
|---|---|---|
| TUI-CLU-01 | Condições ativas, a pior primeiro, com ícone e palavra; sem nenhuma, uma linha "saudável". `Enter` explica o que significa e o que fazer. | `GET /cluster/status` · [23](23-operacao.md) |
| TUI-CLU-02 | Resumo: membros por estado, quórum, versão da tabela de membros, selo de incompleto. | `summary`, `partial`, `missingNodes` |
| TUI-CLU-03 | Tabela de nós: id, estado, drain, memória como barra com %, mensagens em disco, disco livre, blocos em sincronização, requisições em andamento e rejeições `429` (`runtime`). Nó inacessível fica vermelho e diz isso. | `NodeStatus` |
| TUI-CLU-04 | Dois minigráficos do nó selecionado (memória x limite; mensagens prontas), últimos 5 min. | `GET /nodes/{id}/metrics` |
| TUI-CLU-05 | Ações `d` `u` `b` `B` `S`, cada uma atrás de confirmação; `S` pede o nome do cluster digitado. Ação que não pode rodar aparece desabilitada **com o motivo** (não é raiz, nó sem resposta, rota sem porta de operação). | D-TUI-3, PRB-060 |
| TUI-CLU-06 | Atualiza a cada 2 s; `p` pausa; `r` atualiza; a partir de 120 colunas os gráficos vão para uma coluna lateral. | — |
| TUI-CLU-07 | Com token comum, lista os nós (`GET /nodes`) e troca detalhes, gráficos e ações pelo aviso de TUI-FRM-10. | D-TUI-9 |
| TUI-NOD-01 | Detalhe: id, estado, drain, versão, modo, início e tempo no ar; memória x limite (amarelo a 80 %, vermelho a 95 %); mensagens em memória e disco; sinais (canais, quórum, blocos em sincronização, abaixo da replicação e do quórum, nós congelados). | `NodeStatus` |
| TUI-NOD-02 | Backup S3, só quando configurado (acessível, pendentes, último sucesso, segmentos); ausente em `422`. | `GET /nodes/{id}/backup` |
| TUI-NOD-03 | **Drain:** confirmação dizendo o que acontece, depois o progresso de `pendingMessages` até `drained`. Nó em drain ou drenado oferece **devolver**. | `…/drain` |
| TUI-NOD-04 | **Flush de backup** (um nó ou todos) com relatório por nó: completo ou não, pendentes, tempo esgotado. | `FlushReport` |
| TUI-NOD-05 | **Parar cluster:** o popup diz que os processos continuam como `drained` e que pará-los é com o orquestrador; o relatório lista os nós e avisos. | `POST /cluster/stop` |

### 6.3 Gráficos (nó e tópico)

| ID | Requisito |
|---|---|
| TUI-MON-01 | Escolha de séries. **Nó:** memória x limite, mensagens prontas, mensagens em disco, requisições em andamento, rejeições `429`, RTT do canal. **Tópico:** taxas de publicação, consumo e ack; profundidade (visíveis e invisíveis); idade da mais antiga; latência de publicação p50/p95/p99. |
| TUI-MON-02 | Gráfico de linhas com eixo de tempo e escala dos dados; um limite é uma segunda linha; cada linha tem nome na legenda **e** marcador distinto. |
| TUI-MON-03 | Janela de 1, 5, 15, 30 ou 60 min, nunca acima da retenção (`+`/`-`); pausa (`Space`); escala automática ou fixa (`z`). |
| TUI-MON-04 | Linha de resumo: agora, mín, máx, limite, nº de amostras. |
| TUI-MON-05 | Aviso de que o histórico é só o amostrado desde que o app abriu; um gráfico aberto cedo diz "n min coletados" em vez de fingir janela cheia. |
| TUI-MON-06 | Taxas vêm de contadores; contador que diminui (nó reiniciado) é reinício, nunca taxa negativa. |
| TUI-MON-07 | `e` exporta a série visível em CSV, com o caminho pedido num popup (nunca grava sem perguntar). |
| TUI-MON-08 | **Só tópico:** aba Grupos (mensagens, parcela, taxas, idade da mais antiga por `groupId`; excedente marcado). Com selo de que o número de grupos do cluster é aproximado (PRB-059). |

Fontes: nó em `GET /nodes/{id}/metrics` (token raiz; `taba.storage.memory.usage`/`limit`, `taba.storage.memory.messages`, `taba.storage.disk.messages`, `http.server.active_requests`, `taba.ratelimit.rejected`, `taba.cluster.channel.rtt`); tópico em `GET /stats/topics/{t}` e `…/groups`. Os dois alimentam o histórico da seção 7.

### 6.4 Tópicos

| ID | Requisito | Fonte |
|---|---|---|
| TUI-TOP-01 | Tabela: nome, tipo (marcas FIFO / volátil), profundidade, taxas de publicação e consumo, idade da mais antiga, estado (pior condição, palavra + ícone). | `GET /topics` + `GET /stats/topics` |
| TUI-TOP-02 | Paginação por cursor invisível: rolar ao fim carrega a próxima página, com marca de carregamento. | `limit`, `cursor` |
| TUI-TOP-03 | Filtro por nome (no servidor), tag e condição; ordenação por coluna. | `name`, `tag.<k>`, `condition`, `sort` |
| TUI-TOP-04 | Dead-letter aparece apagado e aponta a origem; distribuidor mostra "—" onde não se aplica. | `deadLetter`, `source` |
| TUI-TOP-05 | Uma linha sob a tabela resume o selecionado (tipo, ttl, máximo de entregas, tags, prefixos). | `Topic` |
| TUI-TPF-01 | Formulário de criação com todos os campos: nome, tipo, FIFO, volátil, tempo ocioso, bytes máximos, ttl, máximo de entregas, tags e prefixos (repetíveis). | `POST /topics` |
| TUI-TPF-02 | Validação enquanto digita (regras do servidor); enviar desabilitado com erro; `4xx` mostra `code: message` no formulário. | [16](16-api-cliente.md) |
| TUI-TPF-03 | Edição no mesmo formulário: tipo, FIFO e volátil travados ("não muda depois"); só os campos alterados vão; os três limites aceitam número ou `default`. | `PATCH` |
| TUI-TPF-04 | Apagar pede o nome do tópico digitado e avisa que as mensagens se perdem. | `DELETE` |
| TUI-TPF-05 | Abrir um tópico mostra abas: Resumo, Configuração, Monitoramento, Grupos, Mensagens. Sair de formulário alterado pergunta; `Ctrl+S` salva. | — |

### 6.5 Tokens

| ID | Requisito | Fonte |
|---|---|---|
| TUI-TOK-01 | Tabela: nome, id, permissões, prefixos, validade; ordena por `id` ou `createdAt`; filtra por nome. O segredo nunca está na lista. Expirado em vermelho; expira em até 7 dias em amarelo. | `GET /tokens` |
| TUI-TOK-02 | Criar: nome, as cinco permissões (`publish`, `consume`, `create`, `manage`, `admin`; D157), prefixos, validade ou nenhuma. `admin` não tem prefixos e só o token raiz pode marcá-lo. | `POST /tokens` |
| TUI-TOK-03 | O segredo aparece **uma vez**, num popup: copiar (OSC 52), salvar em arquivo `0600`; mascarado após 30 s ou ao perder o foco; nunca em log, histórico da paleta ou erro; fechar sem copiar pergunta de novo. | `token` do `201` |
| TUI-TOK-04 | Editar muda só nome e validade (`none` remove o prazo); permissões e prefixos aparecem travados com o motivo e a indicação do Clonar. | `PATCH` |
| TUI-TOK-05 | Clonar (`c`) abre Criar preenchido; depois de criar, oferece revogar o antigo (padrão **Não**). Revogar pede confirmação e diz que vale em todos os nós. | `DELETE` |
| TUI-TOK-06 | Sem o token raiz nem a permissão `admin` a aba explica o `403` e qual perfil/token está em uso, em vez de lista vazia. | `required` |

### 6.6 Mensagens — a bancada

| ID | Requisito | Fonte |
|---|---|---|
| TUI-MSG-01 | Abre num tópico escolhido num seletor, ou direto da lista (`x`); mostra tipo e profundidade. | `GET /stats/topics/{t}` |
| TUI-MSG-02 | **Publicar:** corpo de várias linhas, tipo de conteúdo, `groupId`, `deduplicationId`, repetições; `Ctrl+Enter` envia; o resultado (id, `duplicate` ou erro) fica visível. Em JSON, uma checagem leve avisa sem bloquear. | `POST …/messages` |
| TUI-MSG-03 | **Escutar** (`l`): long polling; cada mensagem chega já entregue; **auto-ack** ligável; `a` ack, `v` visibilidade, `A` ack em todas; parar com mensagens sem ack pergunta. | `GET …/messages?wait`, `DELETE`, `PUT …/visibility` |
| TUI-MSG-04 | Tabela do recebido (hora, id, grupo, entregas, estado `visível`/`invisível até hh:mm:ss`, tamanho, trecho do corpo), mais nova primeiro; chegadas novas não mexem na seleção (selo "↑ n novas"); guarda as últimas 1 000. A tela diz que **a API não lista as mensagens de uma fila**: as linhas são as recebidas, e os contadores do cabeçalho são a verdade da fila. | D-TUI-6 |
| TUI-MSG-05 | Detalhe da selecionada: corpo indentado se JSON, com rolagem; entregas, invisível até, recibo; acima de 64 KiB aparece truncado com o tamanho e carrega inteiro sob pedido. | `Message` |
| TUI-MSG-06 | **Navegar** (`b`), só a pedido: popup com quantidade (padrão 10) e tempo de visibilidade (padrão 30 s), o aviso da D-TUI-6 e confirmação; as lidas aparecem como *invisível até*; `v` com `0` devolve agora. Abrir a tela, escolher tópico ou atualizar **nunca** lê a fila. | D-TUI-6 |
| TUI-MSG-07 | **Prune** (`P`): a pessoa digita o nome do tópico; o popup mostra quantas mensagens serão perdidas (`depth`) e que **todos os nós** serão avisados; uma única espera ("podando em n nós…"); no fim, o `removed`, ou a falha `503 cluster.prune-incomplete` dizendo quais nós não concluíram e que a fila pode ter ficado parcial (repetir é seguro). O resultado avisa que mensagens publicadas durante o prune não são cobertas. | `DELETE /topics/{t}/messages` |
| TUI-MSG-08 | Distribuidor não é escutado, navegado nem podado: essas teclas ficam desabilitadas com o motivo. Sem `consume`, escutar, navegar e prune ficam desabilitados; sem `publish`, o painel de publicar; um `429` mostra a contagem do `Retry-After`. | [09](09-topicos.md) |
| TUI-MSG-09 | **Contadores no cabeçalho**, do amostrador: visíveis, invisíveis, profundidade, idade da mais antiga. | `GET /stats/topics/{t}` |
| TUI-MSG-10 | **Pesquisa** (`/`) nas linhas carregadas (id, grupo, corpo), `n`/`N` pulam, trecho destacado, "achados / linhas", sem chamar o servidor. **Ordenação** (`s`, `S`) por hora, id, grupo, entregas, tamanho ou invisível-até; a seleção acompanha a mensagem. | — |
| TUI-MSG-11 | **Gráficos da seleção** (`c`; escondidos abaixo de 100 colunas): três gráficos empilhados com eixo comum — mensagens visíveis e invisíveis; fluxo (publicadas, consumidas, confirmadas); idade da mais antiga. Reusam as regras de TUI-MON-02 a -06 e o mesmo histórico (não acrescentam polling). | D-TUI-5 |

### 6.7 Ajuda, paleta, idioma e perfis

| ID | Requisito |
|---|---|
| TUI-HLP-01 | `h` mostra as teclas da tela e as globais, **geradas da mesma tabela** (`keymap.rs`) que o programa usa. |
| TUI-HLP-02 | `:` abre a paleta: acha qualquer ação de qualquer tela (no idioma atual e em inglês) e a executa. |
| TUI-HLP-03 | `l` escolhe o idioma (English, Português); vale na hora e fica gravado em `ui.language`. |
| TUI-HLP-04 | A aba Perfis lista, cria, edita, remove, define o padrão e testa uma conexão com valores não salvos; substitui a tela de hoje ([20-cli.md](20-cli.md)). |
| TUI-HLP-05 | Trocar de perfil reconecta tudo, zera os históricos e mostra o novo perfil na linha de status. |
| TUI-HLP-06 | **Ao abrir sem perfil informado** (sem `--profile`, `TABA_CLI_PROFILE` ou `TABA_CLI_URL`) **e com mais de um perfil**, a TUI abre uma caixa para escolher: o perfil **padrão** (●) já vem selecionado, `Enter` conecta e `Esc` usa o padrão. Com um perfil só assume-o, sem perguntar. **Sem nenhum perfil**, abre direto o formulário do primeiro (já marcado como padrão): ao salvar, conecta com ele; `Esc` pula e usa o endereço embutido. Nada é lido de um servidor antes da escolha. A mesma caixa abre depois com `o`, e `Esc` só a fecha. |

## 7. Dados e histórico

| Tela | Chamadas | Período |
|---|---|---|
| Cluster | `GET /cluster/status`; do nó selecionado: `…/metrics`, `…/drain`, `…/backup` | 2 s · 5 s no detalhe |
| Lista de tópicos | `GET /topics` (paginado) + `GET /stats/topics?fields=name,depth,publishRate,consumeRate,oldestAge,worstCondition` | 5 s |
| Monitoramento de tópico | `GET /stats/topics/{t}` (séries do amostrador) · `…/groups?top=20` (só com a tela aberta) | `history.interval` |
| Tokens | `GET /tokens` (paginado) | 10 s e após cada mudança |
| Mensagens | `…/messages` (escutar, navegar, `POST`, `DELETE`, `PUT …/visibility`) | sob demanda; profundidade pelo amostrador |

`prom.rs` lê o texto Prometheus (nomes, rótulos, valores; `# TYPE` para os contadores) e calcula a taxa pela diferença entre duas amostras.

**O amostrador** (`history.rs`) roda durante toda a sessão e a cada `history.interval` (padrão 5 s, mínimo 1 s) faz **duas chamadas**: `GET /cluster/status` (carga, memória e disco de todos os nós) e `GET /stats/topics` (todos os tópicos, já consolidados). Cada série é um buffer circular de `retention / interval` amostras (padrão 720). Pausa num `429` (respeitando o `Retry-After`) e é zerado ao trocar de perfil. A amostra de uma leitura `partial` é guardada **marcada como incompleta** e o gráfico a mostra assim. A memória é limitada (séries × amostras × 16 bytes) e o número de tópicos acompanhados tem teto (padrão 200, os mais ativos primeiro; o teto aparece na linha de status). Retenção acima de 6 h ou intervalo abaixo de 1 s é recusado.

## 8. Idiomas (i18n)

- **Catálogo** (D-TUI-7): `connectors/rust/cli/locales/{en-US,pt-BR}.yml`, `rust_i18n::i18n!("locales", fallback = "en-US")`. Todo outro arquivo tem exatamente as chaves e variáveis do `en-US`.
- **Chave que falta** cai para `en-US` e depois para a própria chave; nunca derruba a tela, e o teste de paridade a pega antes do release.
- **Escolha do idioma**, o primeiro que existir: `--lang` · `TABA_CLI_LANG` · `ui.language` (gravado por `l`) · locale do sistema (`LC_ALL`, `LC_MESSAGES`, `LANG`; no Windows, o idioma da interface) · `en-US`. `pt`, `pt_BR`, `pt-BR.UTF-8` casam com `pt-BR`.
- **Traduz-se** rótulo, título, dica, validação, popup, estado vazio e ajuda. **Não se traduz** tecla, rota, campo JSON, **código** de erro, nome de métrica ou de condição. O `message` do servidor aparece como veio, depois de uma moldura traduzida.
- **Layout:** larguras por `unicode-width`; colunas dimensionadas pelo cabeçalho **traduzido**; o pt-BR é 20–30 % mais longo, então 80 colunas é testado nos **dois** idiomas.
- **Formatos:** datas e durações por idioma; separador de milhar por idioma; tamanhos binários (`MiB`, `GiB`) nos dois.
- **Acrescentar um idioma:** copiar o `en-US.yml`, traduzir, incluir o código na tabela e, se o plural for diferente, em `plural()`.

## 9. Erros, segurança e token

| Situação | Comportamento |
|---|---|
| Servidor inacessível | `✖ offline`; dados apagados com a idade; nova tentativa com recuo; ações desabilitadas; nunca um travamento |
| `401` / `403` | Mostra o que falta (`required`) e o perfil em uso; ação indisponível fica desabilitada **com o motivo** |
| `429` / `503` | Obedece o `Retry-After`; o rodapé conta o tempo |
| `partial: true` | Selo "incompleto" com os nós de `missingNodes`; os números são os dos outros nós |
| `502`/`504` numa ação de nó | Diz qual nó falhou; mantém os dados dos outros |
| `4xx` de formulário | `code: message (requestId …)` no formulário; `y` copia o id da requisição |
| Ação perigosa | Confirmação, padrão **Cancelar**; digitada para *parar cluster*, *apagar tópico*, *prune* |
| Segredos | Token mascarado após 30 s; nunca em log, histórico, paleta ou erro; copiar por OSC 52 ou arquivo `0600` |
| Panic | O hook devolve o terminal antes de imprimir |

**O que cada ação pede do token** (a TUI habilita ou desabilita antes de chamar; quem decide é o servidor):

| Área / ação | Token mínimo |
|---|---|
| `GET /auth/me`, `GET /nodes` | qualquer token válido |
| Tópicos: listar, ler, estatísticas, grupos | qualquer token válido, **dentro dos seus prefixos** |
| Criar um tópico | `create` |
| Alterar, apagar ou podar um tópico | `admin` sobre o tópico |
| Publicar · escutar, navegar, ack, visibilidade | `publish` · `consume` |
| Tokens; detalhe do cluster; métricas, drain, retorno e backup de nó | permissão `admin` (ou o token raiz; D157) |
| Parar o cluster; criar, alterar ou revogar um token `admin` | token raiz |

## 10. Testes

| Nível | O quê |
|---|---|
| **Núcleo puro** | `update()` por tela: teclas entrando, modelo e efeitos saindo, sem terminal nem rede. Navegação, filtros, formulários, confirmações, buffer circular, `prom.rs`, unicidade do mapa de teclas |
| **Desenho** | `TestBackend`: cada tela em **80×24** e **120×40**, em **en-US e pt-BR**, contra instantâneos de texto em `tests/`; cada teste diz os IDs `TUI-…` que cobre |
| **i18n** | Paridade de chaves e variáveis; toda chave usada existe; toda chave do catálogo é usada |
| **Integração** | Os trabalhadores com o `Transport` falso (`taba_client::testing`): respostas roteirizadas → `Msg` → modelo. Inclui o ouvinte, a paginação, o recuo de `429`, o dado velho e o selo `partial` |
| **Contra um nó** | Opcional: um cenário dirige o núcleo da TUI, sem tela, contra um nó real dos testes do `taba-node`: criar tópico → publicar → escutar → ack → prune → apagar token. Os casos de uso da TUI (área nova) entram no catálogo ao construir |
| **Manual** | Windows Terminal, PowerShell, cmd, Git Bash, Terminal/iTerm, console do Linux, `tmux`, SSH, 80×24 e uma janela 4K — uma tabela no PR |

Disco curto nesta máquina: rodar só `cargo test -p taba-cli` e `cargo clean -p taba-cli` depois.

## 11. Fases

Cada fase termina com algo utilizável, testes verdes e docs atualizadas **em todos os idiomas**. As rotas já existem, então nenhuma fase depende do servidor.

| Fase | Entrega | Pronta quando |
|---|---|---|
| **0 — Fundação** ✅ | Feature `tui`, `taba-cli tui`, terminal seguro contra panic, laço de eventos, núcleo Elm, trabalhadores, `api.rs`, quadro (6.1), mapa de teclas, ajuda, paleta, i18n nos dois idiomas, tema/`NO_COLOR`, confirmação. **Primeiro:** conferir que `musl` e `arm64` compilam com ratatui/crossterm e medir o binário contra `dist/` | As abas vazias navegam; trocar de idioma funciona; testes de desenho e i18n passam |
| **1 — Cluster** ✅ | 6.2, `prom.rs`, `history.rs`, gráficos do nó (6.3), selo `partial`, ações de nó e de cluster | Bate com `taba-cli health cluster` e `drain *`; um drain e um retorno pela TUI no Compose de 3 nós |
| **2 — Tópicos** | 6.4 e monitoramento de tópico e grupos | Criar, editar e apagar iguais aos comandos `topic *`; paginação suave com mais de 1 000 tópicos |
| **3 — Tokens** ✅ | 6.5 com o popup do segredo e a clonagem | Igual aos comandos `token *`; o segredo nunca fica visível depois de fechar |
| **4 — Mensagens** | 6.6: publicar, escutar, ack, visibilidade, navegar, **prune** e gráficos da seleção | Uma mensagem publicada na TUI é recebida na mesma TUI e por `taba-cli consume`; o prune num Compose de 3 nós esvazia todos os nós, e com um nó parado falha nomeando-o |
| **5 — Perfis e limpeza** 🟨 (a aba Perfis está feita) | Aba Perfis (D-TUI-8); remover `tui/term.rs` e `tui/keys`; mouse, ASCII, layouts estreitos | O código antigo some; `taba-cli profile` não muda para quem usa |
| **6 — Docs e release** | [20-cli.md](20-cli.md), [05-dependencias.md](05-dependencias.md), [24-convencoes.md](24-convencoes.md) (regra de zero dependências do CLI), `README.md` do CLI, scripts de build e Dockerfile, [30-decisoes.md](30-decisoes.md); capturas | `verificar.mjs` e `verificar-idiomas.mjs` passam; os binários dos seis alvos compilam |

**Riscos.** (1) `musl` e `arm64` com ratatui/crossterm (fase 0). (2) O binário cresce; medir e registrar. (3) A TUI nunca afirma uma fila esvaziada que não prova: o `removed` e a falha parcial são mostrados como o servidor os dá.

### Registro da fase 3 e da aba Perfis (2026-10-15)

- **Ordem das abas:** Cluster, Tópicos, **Mensagens, Tokens**, Perfis (`3` é Mensagens e `4` é Tokens).
- **Onde está:** `src/tui/screens/{tokens,profiles}` (estado, teclas e desenho), `src/tui/editor` (o formulário de campos e caixas de marcar, com os testes), `src/tui/view/modal/forms.rs` (os popups do formulário, do segredo e da escolha do perfil) e, no `taba-client`, nada mudou.
- **Tokens (TUI-TOK-01 a 06):** lista paginada (de 200 em 200, até 25 páginas) com filtro por parte do nome (`/`) e ordem (`s`: mais novo, mais antigo, nome, id); o expirado fica em vermelho e o que expira em até 7 dias, em amarelo. Criar (`n`), editar nome e validade (`e`; permissões e prefixos aparecem travados com o motivo), clonar (`c`) e revogar (`Del` ou `x`, com confirmação que começa em **Cancelar**). A validade aceita vazio ou `none` (sem prazo), `30d` `12h` `90m` (a partir de agora), uma data (`2027-01-31`, início do dia em UTC) ou um instante RFC 3339. O `400` do servidor aparece no próprio formulário, que continua aberto. Sem o token raiz a aba diz qual token está em uso; sem autenticação no servidor diz que não há tokens.
- **O segredo (TUI-TOK-03):** só existe dentro do popup: nunca no aviso do rodapé, no erro ou num log. `c` o envia à área de transferência (OSC 52, se o terminal permitir); `s` o grava em `taba-token-<id>.txt` na pasta atual (`0600` no Unix, nunca sobre um arquivo existente: o plano falava em escolher o caminho). Fica escondido (pontos) após 30 s ou quando a janela perde o foco, e `c` ainda o copia. Fechar sem copiar nem salvar pede um segundo `Esc`. Num clone, a caixa **Revogar o antigo ao fechar** (`r`) começa **desmarcada** e fica dentro do popup do segredo, e não depois dele (o popup é o único lugar onde o segredo vive).
- **Perfis (TUI-HLP-04 a 06):** lista com o padrão (●) e o perfil em uso; `n` novo, `e` editar (nome, `url`, `ops-url`, `token-file`, `token`, `ca-file` e a caixa do padrão; os outros campos do perfil, como `output` e os tempos, são mantidos), `d` torna padrão, `Del` ou `x` remove (confirmação em **Cancelar**), `t` testa a conexão do perfil e `Ctrl+T`, no formulário, testa **valores ainda não salvos** (`GET /topics?limit=1`). `Enter` **conecta**: os próximos chamados vão ao servidor do perfil, o histórico e as telas recomeçam e a resposta que ainda vinha do servidor antigo é descartada. Gravar só troca `default` e `profiles` no arquivo (o idioma e o `[history]` ficam como estão no disco). Um token escrito no formulário fica no arquivo como foi digitado, e a tela nunca o desenha (aparece como pontos); o formulário recomenda o arquivo de token.
- **Diferenças do plano:** `o` é **trocar de perfil** em todas as telas; as teclas passaram a minúsculas (`l` idioma, `o` perfil) por serem ações comuns; maiúscula fica para o crítico ou irreversível. `h` e `l` deixaram de ser setas (só `← →`). Na aba Mensagens, `l` escutar vai sombrear `l` idioma: escolher outra tecla ao fazê-la. `Ctrl+T` e `t` são teclas novas. A tela antiga (`taba-cli profile`) continua como estava, até a limpeza da fase 5.
- **Testes:** `cargo test -p taba-cli` passa (272 unitários, 76 de comandos, 54 instantâneos de tela); `clippy` sem avisos. A tabela manual da seção 10 e o terminal real (OSC 52, foco, colar) **não** foram exercitados aqui.

### Registro das fases 0 e 1 (2026-10-15)

- **Onde está:** `connectors/rust/cli/src/tui/{api,app,effect,history,i18n,keymap,model,msg,prom,screens,theme,update,view,worker}` (um módulo por pasta, com `tests.rs`), `src/commands/tui.rs` e `locales/{en-US,pt-BR}.yml`. `ui.language` e `[history]` moram no `config.yml` (`--lang`, `TABA_CLI_LANG`, `--history-retention`, `--history-interval`); o `taba-client` não mudou. A tela de perfis antiga (D-TUI-8) segue como estava.
- **Feito:** o quadro (linha de status, abas, rodapé, aviso abaixo de 80×24), mapa de teclas único que gera a ajuda e a paleta, popups com padrão **Cancelar**, idioma em dois catálogos com teste de paridade, `NO_COLOR`, e a aba **Cluster** inteira (TUI-CLU-01 a 07, TUI-NOD-01 a 05, TUI-MON-01 a 07 para o nó, selo `partial`). As abas Tópicos, Tokens, Mensagens e Perfis abrem, navegam e dizem o que vem.
- **Medidas:** binário `release` do Windows x86_64 (`opt-level=z`, LTO, sem símbolos, como o `build.ps1`): **1 307 648 B sem a feature** (o de `dist/` tem 1 304 064 B; a diferença são as duas seções novas do `config.yml`) e **1 750 528 B com ela**: **+446 KB, +34 %**.
- **Alvos:** `ratatui`, `crossterm`, `rust-i18n` e `sys-locale` passam no `cargo check` para `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` e `aarch64-apple-darwin`. O `taba-cli` inteiro **não foi compilado** para eles aqui: o `ring` do `rustls` (que já era dependência) pede um compilador C de cruzamento (`zigbuild`/`cross`) que esta máquina não tem; isso fica para a CI e o `build.sh`.
- **Testes:** `cargo test -p taba-cli` passa (232 unitários, 76 de comandos, 37 instantâneos de tela em `tests/snapshots/` nos dois tamanhos e idiomas); cada teste cita os IDs `TUI-…`. `tests/live_node.rs` (opcional, `TABA_LIVE_URL`) dirige o núcleo contra um nó real: leu o `/auth/me`, `/cluster/status`, `/nodes/{id}/metrics` e o `422 backup.not-configured` de um nó em desenvolvimento. O binário abriu e desenhou em um pseudo-terminal contra esse nó.
- **Não verificado:** o **drain e o retorno no Compose de 3 nós** (o daemon do Docker estava parado; é o critério de pronto da fase 1 e continua aberto), as teclas e a devolução do terminal em terminais reais (a tabela manual da seção 10) e o ouvinte de foco fora do Windows.
- **Diferenças do plano, e por quê:** o id de um nó aparece como início…fim (`01a1…76b316`), porque ids GUID v7 de nós criados juntos começam iguais; o relógio da linha de status é UTC (sem biblioteca de fuso); `Space` congela o gráfico e `z` troca para a escala a partir do zero; as colunas "disco livre" e "429" só aparecem a partir de 100 colunas; o ASCII troca símbolos, barras e marcadores, mas as bordas das caixas continuam Unicode (fase 5); um nó sem limite de memória (modo desenvolvimento) mostra "sem limite", e `signals.channels` chega como mapa, não como número.
- **Limite do servidor que a TUI mostra:** um nó único em drain recusa **tudo**, inclusive o próprio retorno (`503 cluster.node-draining`); só num cluster com outro nó, ou pela API de operação, dá para devolvê-lo. É o PRB-061 visto de perto.
- **Falta para fechar o ciclo (fase 6):** [05-dependencias.md](05-dependencias.md), a regra de zero dependências de [24-convencoes.md](24-convencoes.md), o `Dockerfile` do CLI, [30-decisoes.md](30-decisoes.md) (D-TUI-1 a 10) e as capturas.


## 12. Impacto no que existe

| O quê | Mudança |
|---|---|
| `connectors/rust/cli/Cargo.toml` | Feature `tui` (padrão): `ratatui`, `crossterm`, `rust-i18n`, `sys-locale` |
| `src/app.rs`, `src/commands/mod.rs` | Comando `tui`; `--lang` global |
| `src/settings` | `ui.language` e `[history]`, com a precedência de [20-cli.md](20-cli.md) (`TABA_CLI_LANG`). **Nenhum campo novo no perfil** |
| `src/tui/` | A árvore da seção 4; `term.rs` e `keys` saem na fase 5 |
| `connectors/rust/client` | Nada |
| `taba-node` | **Nada.** Feito e testado em `tests/nodes_api.rs` (casos UC-TOP-044 a 049, UC-AUT-037 a 039, UC-OPS-046 a 051) |
| Documentação | Este arquivo e os da fase 6, em `pt-BR` e `en-US` juntos |

## 13. Perguntas em aberto

| # | Pergunta | Padrão se não for respondida |
|---|---|---|
| Q1 | Perfil com `start-command`/`stop-command` por nó para subir e matar processos? | Não: fora da API |
| Q2 | `taba-cli` sem argumentos num terminal abre a TUI? | Não: só `tui`; sem argumentos imprime a ajuda |
| Q3 | Manter a exportação (CSV de gráfico, corpo de mensagem)? | Manter, com popup de caminho |
| Q4 | Mouse: roda e clique, ou só teclado? | Roda e clique, opcional (`--no-mouse`) |

## Fontes e provas

- O CLI de hoje: `connectors/rust/cli/src/{app.rs,commands,profile,settings,tui}` e `connectors/rust/client/src` (`Client` bloqueante, transporte `testing`); [20-cli.md](20-cli.md).
- Rotas, parâmetros e DTOs: [16-api-cliente.md](16-api-cliente.md), [17-api-operacao.md](17-api-operacao.md), [18-api-cluster.md](18-api-cluster.md); decisões D152 a D156 em [30-decisoes.md](30-decisoes.md); limites PRB-058 a PRB-061 em [31-pendencias.md](31-pendencias.md).
- Métricas e condições: [13-observabilidade.md](13-observabilidade.md). Versões das bibliotecas: `cargo search` em 2026-10-15 (ratatui 0.30.2, crossterm 0.29.0).
- A TUI das fases 0 e 1: `connectors/rust/cli/src/tui`, `locales/` e `tests/{snapshots,live_node.rs}`. Ainda não há casos de uso da TUI no catálogo: os testes citam os IDs `TUI-…` deste documento.
