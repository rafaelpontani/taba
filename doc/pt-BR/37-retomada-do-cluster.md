---
título: Retomada do cluster após queda total (proposta)
resumo: Proposta em discussão para o achado D — parar o cluster inteiro em produção perde mensagens e tópicos — com definições que persistem, mensagens antigas em quarentena por vida, um log de excluídos replicado e uma restauração em que cada nó envia ao novo primário, que decide. As etapas 0 a 3 estão implementadas; faltam o perfil de um nó e os casos que ainda não têm teste.
tipo: explicação
status: planejado
verificado-em: 2026-10-21 @ 9ae59ba
fontes: [crates/taba-node/src/storage/versions/mod.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/src/cluster/upgrade/mod.rs, crates/taba-node/src/node.rs, crates/taba-node/src/cluster/service/mod.rs, crates/taba-node/src/cluster/recovery/mod.rs, crates/taba-node/src/cluster/sync/mod.rs, crates/taba-node/src/cluster/drain/mod.rs, crates/taba-node/src/cluster/vote/mod.rs, crates/taba-node/src/cluster/replication/definitions.rs, crates/taba-node/src/cluster/backup/restore.rs, crates/taba-node/src/cluster/transport/mod.rs, crates/taba-node/src/cluster/replication/messages_api.rs, crates/taba-node/src/core/id/mod.rs, crates/taba-node/src/storage/messages/mod.rs, crates/taba-node/src/engine/messages/mod.rs, crates/taba-node/tests/recovery.rs]
---

# Retomada do cluster após queda total (proposta)

> ⬜ **Proposta em discussão. As etapas 0 a 3 estão implementadas; faltam a 4 (perfil de um nó) e a 5 (cobertura), veja "Estado da execução".** Ele existe para fechar o desenho antes do código. Quando for decidido, vira uma `D` em [30-decisoes.md](30-decisoes.md), sai de [31-pendencias.md](31-pendencias.md) (achado D) e este arquivo passa a descrever o que existe. Leia antes: [11-durabilidade.md](11-durabilidade.md), [07-cluster.md](07-cluster.md).

## Estado da execução

Execução por etapas (seção 7), tudo no working tree, sem commit. Ao terminar tudo, este documento é reescrito como descrição do que existe (manual de operação), e o modo desenvolvimento passa a ser só configuração.

| Etapa | Estado | O que ficou |
|---|---|---|
| 0. Versão no ingresso e drain das anteriores | ✅ **feita** | Código em `cluster/upgrade/` (regra pura `decide`, laço `run_upgrade`, condição `upgrade-stalled`, `peer_supports`); decisão **D165**; casos **UC-ING-029 a UC-ING-035** (catálogo 26a); testes em `tests/upgrade.rs`, `cluster/upgrade/tests.rs` e `cluster/health/tests.rs`; operação em [23-operacao.md](23-operacao.md#atualizar-a-versão-rolling) |
| 1. Persistir versões e lápides das definições | ✅ **feita** | Tabela `definition_versions` no `_node.db` (esquema 6, `storage/versions`); `Definitions` grava a cada aplicação e lê na partida **só quando retoma as definições do disco**; o `topicId` vai na lápide e no fluxo de lápides; decisão **D166**; casos **UC-DEF-039 e UC-DEF-040**; testes em `storage/versions/tests.rs`, `cluster/replication/definitions/tests.rs` e `tests/definition_versions.rs` |
| 2. Log de excluídos durável | ✅ **feita** | **2a:** tabela `excluded` no banco de cada tópico (`storage/messages/excluded`), gravada na mesma transação do ack, do `prune`, do envio à dead-letter e do ack FIFO em todo nó que guarda cópia; limites `storage.excluded.max-size`/`max-age` e poda a cada 30 s; data de início em `_node.db` (`state.excluded_since`); `merged_since` e `import_excluded` para o log de vidas passadas; decisão **D167**; casos **UC-ESC-037, UC-ESC-038, UC-ARM-026 a UC-ARM-029**. **2b:** pedido `BlockExcluded` (`GET /cluster/blocks/{id}/excluded`) copiado junto com cada bloco na sincronização (`cluster/sync`); o nó novo herda a data de início das origens (marca `excluded_inherited`); `excludedSince` no `NodeStatus` e `summary.excludedCutoff` no `GET /health/cluster`; decisão **D168**; casos **UC-SIN-031, UC-SIN-032, UC-OPS-052**; testes em `storage/messages/excluded/tests.rs` e `tests/excluded_log.rs`. NOVO-ING-05/06 ficam na etapa 3 |
| 3a. Regras puras, manifesto e parâmetros | ✅ **feita** | módulo `cluster/restore` com as regras puras (vida restaurável, nós esperados, começo da restauração com `G`, modo estrito e forçar), o manifesto da vida (`manifest.json`) e os parâmetros `cluster.restore.*`; o **teste de propriedade da seção 4.8** sobre um modelo dessas regras; decisão **D169**; casos **UC-RTN-018 a UC-RTN-022 e UC-CFG-013**; testes em `cluster/restore/tests.rs` e `config/tests.rs`. |
| 3b. Pasta por vida e manifesto | ✅ **feita** | o disco da vida anterior vira a pasta `_lives/<ID antigo>` com o manifesto (sempre, também no primeiro nó); o manifesto da vida em execução é gravado ao ficar ativo, a cada mudança da tabela, a cada 10 s e na parada limpa; a recuperação das perdas não apaga mais a pasta da vida (só `_previous-life`, de disco sem manifesto, como hoje); casos **UC-RTN-023 a UC-RTN-025 e UC-ING-036**; testes em `tests/recovery.rs` e `storage/messages/tests.rs`. **Ficou para depois (feito na 3d):** o anúncio dos manifestos entre os nós, a marca d'água e o apagar das pastas das vidas. |
| 3c. Envio ao novo primário e conferência de mortos | ✅ **feita** | cada nó lê as pastas `_lives/<ID>` das vidas dentro da janela do log (a data de corte é a maior `excludedSince` entre os ativos; se um membro não a informa, nenhuma vida é restaurável, o que cobre o nó de versão anterior) e envia as mensagens com o ID original ao novo primário (`POST /cluster/topics/{topic}/messages/restore`); o primário confere o tópico (e o `topicId`, gravado em `topic_meta` na pasta do tópico), o log local, o log dos outros presentes (`POST …/messages/dead`, em lote) e o diário do S3, e ativa pelo caminho de `recover` com `durable=true` (disco em todos os donos, quórum); o que o primário confirma sai da pasta da vida (a marca de "enviada"); o log de excluídos da vida entra no do nó e a data de início recua se os intervalos se encontram (`excludedSinceMs` no manifesto); decisão **D170**; casos **UC-RTN-026 a UC-RTN-030 e UC-ING-037**; testes em `tests/restore_lives.rs`. O maior `deliveryCount` entre cópias é tomado na etapa 5a (NOVO-RTN-14, UC-RTN-045). |
| 3d. Fim da restauração: votação, FIFO e forçar | ✅ **feita** | o fim da restauração. Cada nó tem um status (`unknown`, `restoring`, `restored`); o nó que entra pergunta aos outros (`GET /cluster/restore`) e, se algum restaura, entra na restauração; senão restaura as próprias vidas sem votação. Em restauração os nós se consultam a cada 500 ms (manifestos, `finished`, `forced`): a passada espera os nós esperados ou `G` (`restore::start`), cada nó anuncia `finished` quando tudo o que enviou foi confirmado e o coordenador (menor ID) chama `POST /cluster/restore/decided` quando todos os membros ativos terminaram; quem decide libera a FIFO (o primário do grupo descarta as duplicatas da lista provisória por um ack sem recibo, `discard`), marca **restaurado do disco** e as pastas das vidas são apagadas se tudo foi confirmado. Forçar: `POST /cluster/restore/force` (token root na API de clientes, sem token na porta de operações), espalhado a todos os nós. Condição `previous-life-pending` no `/health/cluster`. FIFO retida: o `take` não entrega, as publicações são aceitas sem deduplicação e o gerador de IDs fica acima da marca d'água (gravada no manifesto, anunciada entre os nós); a partida a frio chama `begin_restoration` (3e); decisão **D171**; casos **UC-RTN-031 a UC-RTN-035, UC-OPS-053 e UC-FIF-048 a UC-FIF-053**; testes em `tests/restore_vote.rs`, `engine/messages/tests/hold.rs`, `cluster/restore/tests.rs`, `cluster/health/tests.rs` e `core/id/tests.rs`; limites em **PRB-063** (a deduplicação vale na janela `fifo.dedup.window`; a lista provisória está na memória do primário). |
| 3e. Partida a frio (assumir o disco) | ✅ **feita** | a partida a frio. O nó de produção lê as definições do disco só quando parte sozinho; sem resposta no endereço de entrada, o nó com vida anterior tenta os IPs da tabela do manifesto e parte sozinho o de menor ID antigo entre os vivos (`cluster.restore.cold-start.wait`), assume o disco, continua o log de excluídos da vida anterior e chama `begin_restoration`; quem chega com o cluster de pé guarda as definições do disco, apaga-as e recebe as do cluster, e depois **oferece** as que guardou (a maior versão de cada uma vence, lápide incluída); sem vida anterior só `cluster.first-node` parte sozinho; decisão **D172**; casos **UC-RTN-036 a UC-RTN-039, UC-DEF-041 e UC-DEF-042** (NOVO-RTN-28 a 32 e NOVO-DEF-04, 05); testes em `tests/cold_start.rs`; limites em **PRB-064**. |
| 3f. Varredura do S3 na partida | ✅ **feita** | NOVO-BKP-01, 02, 04, 05 e 06 (seção 4.13); a retenção da FIFO (NOVO-BKP-03) foi provada na etapa 5a (UC-BKP-045). Casos **UC-BKP-041 a UC-BKP-044**. |
| 3g. Sondagem dos IPs antigos e reencontro dos grupos | ✅ **feita** | todo nó lê a tabela da vida anterior ao começar o bootstrap e, enquanto o cluster tem menos membros ativos que ela, sonda os IPs que faltam (`cluster.restore.probe.interval`, até `cluster.restore.probe.timeout`); a tabela de outro grupo decide pelo D101 e o **menor** perde sem apagar nada: o disco vira a pasta de uma vida, as definições são oferecidas ao vencedor e o nó refaz o join com ID novo; o tempo que acaba com nós faltando deixa a condição `members-missing`; decisão **D174**; casos **UC-RTN-040 a UC-RTN-042 e UC-OPS-054** (NOVO-RTN-33 a 36); testes em `tests/regroup.rs` e `cluster/health/tests.rs`; limites em **PRB-066**. |
| 4a. O perfil de um nó no caminho de produção | ✅ **feita** | `replication.factor=1` sem `cluster.entry-address` parte sozinho (D175); `cluster.tls.enabled`, `auth.dev-token` (token `dev`) e `health.single-node-condition` (condição `reduced-profile`); quórum da FIFO `min(2, fator)` e fim da recusa de FIFO com fator 1. O `mode=development` ainda existe. Casos **UC-RTN-043 e 044, UC-CFG-014 e UC-OPS-055**. |
| 4b. Migrar os testes e as peças para o perfil | ✅ **feita (testes)** | `TestNode` e os testes de integração foram para o perfil (UC-RTN-043 e 044 e todas as suítes). `Mode::Development` no drain e no estado, o backup `Solo` e os testes unitários que montam um nó de desenvolvimento saem com o `Mode` na 4c. |
| 4c. Remover o `mode`, o `resume-on-start` e o carregamento direto | ✅ **feita** | Saíram o `Mode`, `development.resume-on-start`, `Solo`, a marca de save, o carregamento direto de tópicos, tokens e mensagens (`Topics::open`, `Tokens::open` e o `resume` do `Messages`), o `status.mode` do `NodeStatus` e o código `topic.fifo-needs-replication`. As chaves `mode` e `development.resume-on-start` ainda são aceitas num arquivo ou variável, com aviso (não quebra uma configuração que já existia). Os casos do modo desenvolvimento foram reescritos (DEV-002, 011 e 012 e FIF-046) ou retirados. |
| 4d. Compose, Helm, `cluster.sh` e documentos | ✅ **feita** | O chart do Helm tem o perfil de um nó em `config.replication.factor: 1` (o `config.mode: development` antigo ainda vale e é o mesmo perfil; o chart não o repassa ao nó); `uc-02-desenvolvimento`, `uc-06-s3-externo` e `00-static.sh` passaram a usar o fator 1 (`run.sh static`: 104 ok). `docker-compose.dev.yml` e `docker/cluster.sh` só constroem as imagens locais e não falavam do modo. Docs 01, 02, 03, 06, 07, 11, 14, 20, 20b, 21, 29, 34, 35, os READMEs, `llms.txt` e os diagramas (mapa mental, portas do nó, pirâmide de testes) deixaram de citar o modo. Os screenshots do TUI (`capture-tui.sh`) ainda mostram "· production". |
| 5. Completar a cobertura dos casos sem teste | ✅ **feita** | Os 15 casos, em três grupos de cinco (5a a 5c; seção 9.8). |
| 5a. Casos sem teste | ✅ **feita** | **NOVO-RTN-05** (UC-RTN-048), **NOVO-RTN-14** (UC-RTN-045; o código saiu: o maior `deliveryCount` entre as cópias, PRB-062 resolvido), **NOVO-RTN-26** (UC-RTN-047), **NOVO-RTN-27** (UC-RTN-046) e **NOVO-BKP-03** (UC-BKP-045; o `MemoryStore` ganhou `set_slow`). |
| 5b. Fim da restauração, nível cluster | ✅ **feita** | **NOVO-RTN-01** (UC-RTN-049), **NOVO-RTN-13** (UC-RTN-050), **NOVO-RTN-18** (UC-RTN-051), **NOVO-RTN-23** (UC-RTN-052) e **NOVO-RTN-24** (UC-RTN-053). Só testes: o código já fazia isso. |
| 5c. Propriedade, partida e configuração | ✅ **feita** | **NOVO-RTN-25** (UC-RTN-054), **NOVO-RTN-30** (UC-RTN-055), **NOVO-RTN-31** (UC-RTN-056), **NOVO-RTN-34** (UC-RTN-057) e **NOVO-CFG-05** (UC-CFG-013). Um código: `Config::validate` passou a recusar `storage.excluded.max-size` e `storage.excluded.max-age` zerados; o resto são testes. |
| 6. Revisar a documentação | ⬜ pendente | (1) Tirar dos documentos os comentários e as notas que nasceram da execução do plano (marcas de etapa, "feito na 3d", desvios do desenho) e incorporá-los como descrição de como o sistema funciona hoje, em documentação de estado. (2) Levar o que está neste documento e que serve à documentação oficial para os documentos que já tratam do assunto. (3) Apagar este documento: o repositório guarda como as coisas estão e quais são os planos e passos futuros ([31-pendencias.md](31-pendencias.md), [32-roadmap.md](32-roadmap.md)), não o plano de uma execução. |

**Desvios do desenho na etapa 0** (a seção 4.12 abaixo já está corrigida):

- A versão de cada membro **não** vai na tabela de membros nem no join: `Member` e `Change` têm `deny_unknown_fields`, e um campo novo faria as versões já publicadas rejeitarem a tabela. O nó lê a versão no `version` do `NodeStatus` (`Request::Status`), que toda versão já publica; quem não responde a rota ou não traz versão conta como anterior.
- Quem age é **um só nó por vez**: o de menor `nodeId` entre os da versão mais nova (em vez de "o nó que entrou"), para que dois nós não peçam drains diferentes.
- Os casos **NOVO-ING-05** (nenhuma vida restaurável com um nó anterior presente) e **NOVO-ING-06** (disco de versão anterior não migrado) **não** foram feitos: dependem do log de excluídos e da quarentena (etapas 2 e 3).
- Mapeamento dos casos: NOVO-ING-01 → UC-ING-029; 02 → 030; 03 → 031; 04 → 032; 07 → 033; novos: UC-ING-034 (um nó age) e UC-ING-035 (comparação de versões).
- Mapeamento da etapa 1: NOVO-DEF-01 → UC-DEF-039; NOVO-DEF-02 → UC-DEF-040; NOVO-DEF-03 já estava coberto por UC-DEF-026, UC-DEF-027 e UC-DEF-035 (versão maior vence, lápide de versão maior vence a criação), sem caso novo; NOVO-DEF-04 e NOVO-DEF-05 ficam para a etapa 3 (partida a frio).
- O reinício **de processo** com as linhas lidas só acontece hoje no modo desenvolvimento (`resume-on-start`); em produção o nó sobe limpo e a leitura ao subir vira regra geral na etapa 3. Por isso a prova de "sobrevive ao reinício" é dividida: o banco reabre com as linhas (U) e a regra de leitura mantém só o que descreve a definição carregada (U); a prova de cluster (C) cobre a gravação em todos os nós.

**Valores e nomes fixados** (viram `D` quando a etapa que os usa for feita; `G`, o modo estrito, a folga e a sondagem já são parâmetros `cluster.restore.*`, D169): `G` = 5 min; modo estrito em `cluster.restore.strict=false`; forçar = `POST /cluster/restore/force`, só com o token root; pasta por ID (o nó não reutiliza o ID); a varredura do S3 soma ao `restore()` do sobrevivente único; log de excluídos de 100 MB e 24 h; folga de relógio de 5 s; sondagem de 2 h a cada 10 s.

## 1. O problema

Em produção, **parar o cluster inteiro perde mensagens e definições de tópico**: `docker compose down` + `up` com os volumes mantidos sobe um cluster sem nenhum tópico ([11-durabilidade.md](11-durabilidade.md), cenário "todos os nós param ao mesmo tempo, sem backup"; achado D em [31-pendencias.md](31-pendencias.md)). Com o backup em S3 ([33-backup-s3.md](33-backup-s3.md)) o cluster se restaura, mas sem ele não há saída. Uma manutenção que pare tudo, ou uma queda geral, zera o cluster.

Quando **alguns** nós caem, o cluster se recupera sem problema: as réplicas têm a mensagem e o nó novo sincroniza. O problema é só a **queda total seguida de partida a frio**, quando não resta nenhum nó vivo para dizer o que existe.

**Alcance.** A proposta trata o caso mais severo: todos os nós morrem **sem dar sinal** e voltam juntos. O que estava **só em memória** em todos os nós se perde; a escrita em disco no commit fica fora desta proposta. Só se restaura o que já estava em disco.

## 2. Como é hoje (verificado no código)

| Ponto | Comportamento | Onde |
|---|---|---|
| Identidade | Todo processo nasce com **ID novo** | `node.rs` (`NodeShared::new(id::new_at(...))`) |
| Tópicos e tokens | Em produção **não são lidos do disco** na partida (`load = dev && resume_on_start`); vêm do cluster pela sincronização | `node.rs`, `engine/topics`, `engine/tokens` |
| Versões das definições | O mapa de versões e as lápides das definições ficam **só em memória** (as lápides expiram em 1 h) | `cluster/replication/definitions.rs` |
| Mensagens | O disco da vida anterior é posto de lado em `_previous-life`; o nó entra **vazio** e sincroniza | `cluster/service/mod.rs` (`recover_on_boot`), `storage/messages` |
| Recuperação | Depois de ativo, o nó devolve **só as mensagens que os pares listam como perdas** e então apaga a pasta | `cluster/recovery/mod.rs` (`run_streaming`, `held_previous_life`) |
| Primeiro nó | `start_alone()` põe o disco de uma vida com manifesto na pasta da vida e o assume (a partida a frio, D172); o disco sem manifesto segue sendo apagado (D111). Os casos **UC-RTN-017** e **UC-RTN-036** fixam isso | `cluster/service/mod.rs`, `tests/recovery.rs` |
| Parada planejada | Cada nó descarrega a memória, grava a marca de save e **sai da tabela** de membros; o disco fica íntegro e ninguém o relê | `cluster/drain/mod.rs` |
| Backup em S3 | `restore()` **pula tópicos que já existem** e só restaura mensagens dos tópicos que acabou de recriar | `cluster/backup/restore.rs` |

## 3. Por que o ID é novo hoje

O ID novo é uma defesa: um nó que ficou fora do ar por um tempo e ficou **dessincronizado** não pode trazer de volta dado já excluído ou desatualizado. Como o nó nasce vazio e o cluster reconstrói tudo, essa classe de erro não existe. O preço é que **todo reinício ressincroniza todos os blocos do nó** (o ID novo muda a posição no anel) e que a queda total perde tudo. Qualquer mudança aqui precisa **preservar a defesa**.

## 4. A proposta

**Ideia central: disponibilidade primeiro, verdade depois.** O cluster sobe e atende mensagens **novas** de imediato. As mensagens **antigas** ficam em quarentena no disco e só voltam a ser entregues quando o novo primário do bloco confere que elas não estão mortas.

### 4.1 Dados do cluster e dados da vida

| Dado | Persiste entre vidas? | Onde |
|---|---|---|
| Tópicos e tokens (definições), versões e lápides | **Sim**, e é carregado na partida | banco do nó (`_node.db`) |
| Mensagens | Ficam **na pasta da vida** em que foram gravadas | pasta por ID antigo, em quarentena |
| Tabela de membros da vida e versão dela (manifesto) | Sim, junto da pasta | manifesto da vida |

Como o nó já sabe os tópicos e tokens, ele **recebe e entrega mensagens novas desde o primeiro segundo** (as filas comuns; a FIFO espera, seção 4.10).

### 4.2 Definições (tópicos e tokens)

- Cada definição já tem **versão** e **dona** no anel, com desempate por impressão digital. ✅ **Implementado (etapa 1, D166):** o mapa de versões e as lápides são persistidos (`definition_versions`), com a lápide **junto do `topicId`** (um tópico recriado tem outro `topicId`); a leitura na partida vale hoje só quando o nó retoma as definições do disco.
- Na junção de dois estados, **vence a versão maior** por definição. Não é preciso uma versão global do cluster: a versão por definição é suficiente, e a versão da tabela de membros já é global e serializada.
- Na partida a frio, **o nó de menor ID coordena** a reunião das definições de todos os nós conectados (seção 4.9). Se o cluster já está de pé, o nó entra limpo e recebe as definições dele pela sincronização, como hoje.

### 4.3 Estado da mensagem: o que realmente precisa de versão

- **Exclusão (ack, ou envio à dead-letter):** é um estado **absorvente**. Não existe versão posterior a "morta". Na junção, **se qualquer cópia diz morta, vale a morta**.
- **Invisibilidade e recibo:** na retomada são **zerados** e o `deliveryCount` fica com o maior valor entre as cópias. É o que `apply_local` faz, na recuperação de perdas e na restauração das vidas (`Messages::raise_delivery_count`, etapa 5a). O pior efeito é uma reentrega antecipada, aceita em *at-least-once*. Não é preciso uma revisão por mensagem.

### 4.4 Log de excluídos

- Cada nó que guarda uma cópia de um bloco, **primário ou não**, registra no seu **SQLite local** o `id` de toda mensagem excluída dali (ack, ou envio à dead-letter), com o tópico (e o `topicId`) e o momento. O ack grava o registro **antes de confirmar** (hoje o ack apaga o arquivo e a linha, D12). Assim a fonte da verdade sobre o que morreu fica **replicada em vários nós**.
- O log tem **limite de tamanho e de tempo**, configuráveis (por exemplo 100 MB e 24 horas; os nomes dos parâmetros ainda serão definidos). Os registros mais antigos saem primeiro.
- Cada log tem a **data de início da gravação contínua**: o instante a partir do qual ele garante ter as exclusões. Ela começa na criação do log e só avança quando um registro é descartado pelo limite (por isso não é o registro mais antigo: um cluster sem exclusões por três dias cobre os três dias). Um nó novo **herda** essa data das fontes ao sincronizar os blocos dele, e por isso não a puxa para a data da entrada.
- A **data de corte do cluster** é a **mais recente** dessas datas entre os membros ativos (o elo mais fraco). Não é a data do log mais antigo: cada nó só registra os blocos que guarda, e um nó de pouco tráfego, que guarda por muito tempo, não viu as exclusões dos blocos dos outros. Com a data do elo mais fraco, toda exclusão posterior a ela está registrada em todos os donos que a registraram. Um nó que apaga cedo por causa do tamanho encurta a janela do cluster inteiro; o limite deve ser dimensionado para isso, e a data de corte deve ser visível ao operador.
- Os **logs de vidas passadas** que ainda estão em disco são lidos e **incorporados ao log atual**, respeitando o tempo e o tamanho totais. Eles só acrescentam evidência (mais `id`s mortos). A data de início da gravação só recua se o intervalo antigo e o das fontes se encontram, sem buraco; se as fontes já apagaram o período em que o nó esteve parado, a data fica depois do buraco.
- Uma vida só pode ser restaurada se a **última atividade** dela (`last_seen`, no manifesto) for posterior à data de corte do cluster, com uma folga para a diferença de relógios. Fora da janela, a vida volta ao caminho de hoje (identidade nova, disco descartado, só as perdas listadas).
- **Durabilidade.** O registro é gravado **na mesma transação** que apaga a mensagem, e o `storage.db.synchronous` **continua `NORMAL`** (sem fsync extra no ack). Uma queda do sistema pode perder a última janela de transações do log; isso cabe no *at-least-once* (seção 4.11). O custo no ack é uma inserção a mais, e deve ser medido na etapa 2.
- **Relógios.** A comparação do `last_seen` (relógio do nó que escreveu a vida) com a data de corte (relógios de outros nós) usa uma **folga configurável** (nome e valor ainda a definir, da ordem de segundos). Se o desvio real for maior que a folga, o **risco é aceito**: uma vida pode ser restaurada fora da janela, ou barrada dentro dela, e a consequência é a reentrega residual da seção 4.11.
- **O S3 não é prova de vida.** Com o backup ativo, o diário do S3 serve só de **prova de morte** (seção 4.6). Ele não confirma que uma mensagem está viva, porque o diário tem defasagem de até `backup.meta.interval` ([33-backup-s3.md](33-backup-s3.md)): um ack recente pode ainda não estar nele. Por isso o S3 **não alarga a janela** acima, e não dispensa o relógio. Ele é também uma **fonte de candidatas** (seção 4.13), que passam pela mesma conferência de mortos.
- A sincronização já carrega linhas curtas com `delete_at_ms` (`apply_tombstone`, hoje para a deduplicação da FIFO). A ideia é **generalizar** para todas as filas, e os registros acompanham o bloco quando os donos mudam.

### 4.5 Quarentena por vida

- Cada vida tem **sua pasta, com o ID antigo no nome**. Hoje, se o nó reinicia mais de uma vez antes de terminar a recuperação, o código mantém só a primeira pasta e apaga a nova; com uma pasta por ID várias vidas convivem.
- Cada vida tem um **manifesto**: o ID, a **tabela de membros** (com os IPs) e a **versão** dela no momento da parada, o instante da **última atividade** (`last_seen`, gravado periodicamente e na parada limpa) e a **marca d'água** do maior `id` (seção 4.10). Ao subir, o nó usa a tabela de IPs para reencontrar os nós antigos (seção 4.9) e **anuncia** os manifestos que guarda, para o cluster saber quantos nós eram esperados (seção 4.8).
- A quarentena **não é apagada** pela recuperação de hoje; só no fim da restauração (seção 4.7).
- **Sem teto próprio de espaço.** As pastas das vidas ocupam disco até o voto final. Não há limite nem limpeza separados; se o disco encher, vale o comportamento de sempre (o nó entra em `failed`, como com o `outbox` do backup).

### 4.6 Restauração: cada nó envia, o novo primário decide

- Cada nó lê a pasta de cada vida em quarentena, bloco a bloco, e **envia as mensagens ao novo primário do bloco** (o do anel atual), **com o ID original**, pelo caminho `Recover` com replicação que já existe. O nó **não decide nada** sobre a mensagem: só envia.
- O **novo primário recebe e trata** cada mensagem. Antes de carregá-la, confere se o `id` está morto, da fonte mais barata para a mais cara:
  1. no seu **log de excluídos local** (seção 4.4);
  2. nos **outros nós**: pergunta a todos os nós presentes, em lote por bloco (uma lista de `id`s; a resposta é o subconjunto que consta no log deles). Pergunta a **todos**, e não só aos donos do bloco, porque os donos novos não são os antigos e os logs ficam nos nós, não no anel;
  3. no **S3**, se o backup está configurado: o diário do bucket registra cada ack como uma remoção (D144). Uma mensagem removida no diário está morta. S3 inacessível é evidência ausente: o primário decide com as duas primeiras fontes e registra um aviso.
- **Tópico.** Antes de tudo, o primário confere o tópico: se ele foi **excluído**, ou **recriado com outro `topicId`** (o da vida fica registrado junto do tópico na pasta), a mensagem é **descartada**, sem consultar mortos. Isso vale para a quarentena inteira e é testado; hoje a varredura já apaga mensagens de tópicos desconhecidos.
- Se **qualquer fonte** diz que morreu, o primário descarta. Senão, **ativa**: replica aos donos atuais, com o quórum, e deixa a mensagem disponível. Se já a tem (`Existing`), pula.
- Uma mensagem que está **só na vida**, dentro da janela e sem registro de exclusão em nenhuma fonte, **é ativada**: ela é uma mensagem confirmada ou uma publicação que não alcançou o quórum, e entregá-la é aceitável em *at-least-once*.
- As fontes só **acrescentam evidência de morte**. Nenhuma delas alarga a janela da seção 4.4.

### 4.7 Fim da restauração

- **Recebimento durável.** O novo primário grava a mensagem restaurada **em disco** e a replica com o quórum antes de confirmar. Só então o nó de origem a marca como **enviada**, e ela não é reenviada numa partida seguinte. Assim uma segunda queda não perde uma mensagem que estava salva. A **pasta da vida só é apagada no fim da restauração**, depois da votação abaixo. Quando o nó termina de enviar todas as vidas **e a varredura do S3** (seção 4.13), ele avisa que terminou.
- **Votação.** O coordenador (o nó de menor ID, seção 4.9) abre a votação quando **todos os nós presentes** avisaram que terminaram. A decisão exige **todos os presentes**, porque cada um tem disco a enviar; os nós esperados que ainda não voltaram são tratados pelo prazo da seção 4.8. A votação muda o status do cluster para **restaurado do disco**, um nome que não se confunde com a restauração do S3. Só então as filas **FIFO são liberadas** (seção 4.10) e as pastas das vidas são apagadas.
- **Se a votação não fecha** (por exemplo, no modo estrito, enquanto falta um nó esperado), a FIFO continua sem entrega e o cluster mostra uma condição visível (por exemplo `previous-life-pending`). A operação de forçar (seção 4.8) muda o status.
- **Nó que volta depois da liberação.** Ele entra como nó novo e limpo, e as vidas dele seguem o caminho da seção 4.6. As mensagens FIFO dele são ativadas em ordem de `id` e podem ser entregues depois de mensagens mais novas do grupo que já foram entregues: a ordem dessas mensagens não é garantida. É a exceção, não a regra.
- As **filas comuns** não esperam a votação: são liberadas assim que as definições estão distribuídas (seção 4.9), e as mensagens antigas vão entrando enquanto isso.

### 4.8 Nós que não voltam

- A restauração das mensagens antigas só **começa** quando todos os nós esperados (os membros da maior tabela antiga, pelos manifestos) se conectaram, ou passado um prazo configurável `G`. Isso não atrasa a distribuição das definições nem a liberação das filas comuns. A regra clássica de maioria da tabela não vale aqui, porque o voto de exclusão usa como base os nós **saudáveis** (`ceil(S × 51%)` de `S` saudáveis, [vote/mod.rs](../../crates/taba-node/src/cluster/vote/mod.rs)), e um par saudável pode excluir a maioria da tabela; por isso se espera por **todos**, e não por uma maioria.
- **Depois de `G`**, no modo padrão a restauração **segue com um aviso** e usa os nós presentes. No **modo estrito**, uma configuração, ela **segura até a operação de forçar**. O valor inicial de `G` é configurável (da ordem de alguns minutos) e ainda será definido.
- A operação de **forçar** muda o status do cluster e libera o que está retido, com o risco declarado (podem ressurgir mensagens já consumidas, seção 4.11). Do contrário a FIFO nunca é liberada se um nó nunca voltar.
- A mesma operação serve ao achado C (cluster travado por perda de dois nós).

### 4.9 Partida: como o nó decide

1. Todos os nós sobem **juntos**, cada um com **ID novo**, e a primeira coisa é tentar os nós da **tabela de IPs da vida anterior** (guardada no manifesto, seção 4.5), além do `cluster.entry-address`.
2. Se **nenhum responde**, o nó **assume que é o primeiro** e assume o que está em disco: cria uma tabela só com ele e carrega tópicos, tokens, versões e lápides. As **mensagens antigas continuam em quarentena**.
3. Quando **vários se conectam**, o de **menor ID é o coordenador**. Ele reúne as definições de **todos os nós conectados** e aplica a maior versão de cada uma (as lápides de definições valem), e as distribui aos demais. Reunir só as do disco dele não basta: ele pode ser o nó que ficou fora antes e tem o disco velho. A partir daí, as **filas comuns recebem e entregam**; a FIFO ainda não (seção 4.10).
4. Um nó que chega **depois**, com o cluster já de pé, entra pelo join de hoje, como **nó novo e limpo**, e recebe as definições do cluster. Os dados em disco dele seguem para a restauração (seção 4.6).
5. Se **dois ou mais grupos** de nós assumem o disco porque não se alcançaram no primeiro momento, vale o **critério de desempate** que já existe no reencontro de partições (D101): perde o lado com menos membros e, no empate, o de menor `nodeId`. O lado perdedor refaz o join com ID novo e **reaproveita os dados que tem em disco**. Como as definições se resolvem por versão e as mensagens pela consulta de mortos, o desempate só decide quem se hidrata; não decide qual dado é a verdade.
6. **Janela de risco aceita:** um nó atrasado que sobe sozinho serve com as definições que tem, e um token revogado enquanto ele estava fora seria aceito. A janela termina assim que algum nó responde, quando a versão maior vence, e é limitada pela expiração dos tokens.
7. A flag `cluster.first-node` continua necessária só para o cluster que **nunca existiu**, sem tabela anterior.

### 4.10 Filas FIFO durante a restauração

- A FIFO **só é liberada** quando o status do cluster muda para restaurado do disco (seção 4.7), porque só então tudo o que estava em disco já está no seu primário. Até lá ela **recebe mensagens e não as entrega**.
- As mensagens novas ficam numa **lista provisória** no novo primário do grupo, marcadas como **sem posição efetiva** na fila. Elas recebem um `id` normal na hora, e o gerador de IDs começa acima da **marca d'água**, o maior `id` da vida antiga, guardada no manifesto: os IDs novos são sempre maiores que os antigos, e nenhum `id` já devolvido ao produtor muda depois.
- As mensagens **restauradas** chegam ao novo primário do grupo pelo caminho da seção 4.6 e também ficam sem entrega até a liberação.
- **Nenhuma deduplicação acontece durante a restauração:** todas as publicações são aceitas.
- Na liberação, o primário **efetiva** a lista provisória. Deduplica contra as chaves restauradas e dentro da própria lista, em ordem de `id`, descarta as duplicatas, e só então libera a entrega.
- Consequência aceita: o produtor de uma mensagem que depois é descartada como duplicata recebeu "publicada" (`duplicate=false`) na resposta. Isso será a **exceção**, não a regra.

### 4.11 Garantias e limites

- A restauração mantém o *at-least-once*: as mensagens que estavam em disco são restauradas, e as já consumidas **não** voltam, **salvo nos dois casos abaixo**.
- **Reentrega residual (aceita):** uma mensagem já consumida pode ser entregue de novo quando (a) um nó que tem registros de exclusão não está presente (depois do prazo `G`, ou quando só voltaram nós velhos que se confirmam entre si), ou (b) uma queda faz o log de excluídos perder as últimas entradas (durabilidade do log, seção 4.4). O consumidor precisa ser **idempotente**, como já se exige na restauração do S3.
- Com **todos os nós esperados presentes**, a reentrega por (a) **não acontece**; é isso que o teste de propriedade da seção 4.8 deve provar.
- O que estava **só em memória** em todos os nós se perde (seção 1).

### 4.12 Atualização de versão

Os formatos novos (manifesto, pasta por ID, esquema de `_node.db`, log de excluídos) e as mensagens novas entre nós (anúncio de manifesto, consulta de mortos, coordenação, votação) são contratos ([24-convencoes.md](24-convencoes.md)). A convivência de versões **não é tratada com compatibilidade entre elas**: a versão nova, ao entrar no cluster, **retira as anteriores**.

- ✅ **Implementado (etapa 0, D165).** Cada nó publica a **versão** do programa no `NodeStatus` (a tabela de membros e o join não mudam). Só o nó de menor `nodeId` entre os da versão mais nova age: ele **drena os de versão anterior, um a um**, pelo drain que já existe ([cluster/drain/mod.rs](../../crates/taba-node/src/cluster/drain/mod.rs)); o próximo só começa quando o anterior saiu da tabela. Código em [cluster/upgrade/mod.rs](../../crates/taba-node/src/cluster/upgrade/mod.rs).
- O drain seguinte **só começa** se o cluster continuar com pelo menos 3 nós ativos e o nó que entrou já tiver terminado de sincronizar (item "Reduzir" de [23-operacao.md](23-operacao.md)). Se algum nó não drena no prazo, o processo para nele e mostra uma **condição visível**; o operador decide (cancelar o drain ou parar o nó).
- Na convivência, **só circula o que as duas versões entendem**: as mensagens novas entre nós **não são enviadas** a quem não anunciou suportá-las.
- Enquanto houver um nó da versão anterior, ele **não grava o log de excluídos**. Pela regra da seção 4.4 (a data de corte é a do elo mais fraco), isso faz com que **nenhuma vida seja restaurável** até esse nó sair; ou seja, a convivência nunca cria uma restauração falsa. Quando o último nó anterior sai, o log passa a valer a partir da data em que **todos** gravam.
- O **disco** de uma versão anterior **não é migrado**. O nó novo lê o que as duas versões entendem e segue o caminho de hoje (vida anterior, só as perdas que os pares listam). Os formatos novos só existem em nós novos. Os nós retirados por drain voltam, se voltarem, como nós novos e limpos (seção 4.5).
- **Fora do alcance:** voltar a uma versão anterior depois que um nó gravou os formatos novos. Uma versão anterior que entra **não** drena as mais novas (a regra vale só no sentido novo → antigo).

### 4.13 Varredura do S3 na partida

Com o backup em S3 configurado, a partida do nó tem um **último passo**:

- O nó **lista o que existe no S3** (segmentos e diário) **dos blocos de que ele é primário** e **indexa as mensagens vivas**: as que estão em segmento e não têm remoção no diário. É o que permite recuperar também o que se perdeu do disco.
- As mensagens achadas passam pelo **mesmo caminho** da seção 4.6: tópico (seção 4.6), conferência de mortos no log local e nos outros nós, e ativação com quórum. As que o nó já tem (`Existing`) são puladas, como acontece quando o disco já as trouxe.
- O passo **não impede o cluster de ficar disponível**: as definições e as filas comuns não esperam por ele. Ele só **retém a entrega das filas FIFO**: o aviso de "terminei" da seção 4.7 inclui a varredura, então a FIFO só é liberada depois dela.
- S3 inacessível não trava nada: o passo é pulado com um **aviso**, e o nó segue só com o disco.
- ✅ **Implementado (etapa 3f, D173).** `Recovery::scan_backup` roda no fim da passada de `restore_lives` (com ou sem vidas): por tópico lê o diário uma vez (o cache de `journal_removed`), e `Backup::scan_topic` lista os segmentos, lê só o rodapé de cada um e traz as mensagens sem remoção no diário que o nó não tem e cujo bloco ele é primário; elas entram por `accept_restore`. Tópico cujo diário não responde é pulado com aviso.

### 4.14 Reencontro de grupos que assumiram o disco

O reencontro de hoje só dispara para nós que se excluíram por voto (`excluded_members`). Dois grupos que subiram sem se falar (seção 4.9, item 5) nunca entram nessa lista, então precisam se procurar de outra forma.

- **Sondagem dos IPs antigos.** Enquanto o cluster tem **menos membros que a tabela da vida anterior** (do manifesto, seção 4.5), cada nó tenta de tempos em tempos os IPs que faltam. Dois valores são configuráveis: o **tempo total** da sondagem (por exemplo 2 horas) e o **intervalo** entre tentativas (por exemplo 10 s). Os parâmetros são `cluster.restore.probe.timeout` e `cluster.restore.probe.interval`.
- A sondagem **termina** quando a tabela se completa ou quando o tempo acaba. Se acabar com nós faltando, o cluster mostra uma **condição visível**; daí em diante o nó que falta entra pelo join normal, quando voltar.
- Quando um grupo alcança outro, vale o desempate do D101 (menos membros perde; no empate, o menor `nodeId` ganha).
- **O perdedor não apaga nada.** Hoje o reencontro, para quem perde, usa `handle_exclusion`, que apaga o estado local. Aqui o perdedor **move o disco para a quarentena** (pasta com o ID dele, seção 4.5), **entrega as definições dele** (a maior versão de cada uma vence, as lápides valem) e refaz o join com ID novo. As mensagens dele seguem o caminho da seção 4.6, como as de qualquer vida em quarentena.
- O que o grupo perdedor aceitou enquanto esteve separado vive na pasta da vida dele, e por isso também é restaurado e conferido contra os mortos.
- ✅ **Implementado (etapa 3g, D174).** `ClusterService::run_probe` (em `cluster/service/probe.rs`) lê o manifesto ao começar o bootstrap e sonda a cada `cluster.restore.probe.interval`, até `cluster.restore.probe.timeout`; o grupo menor chama `regroup`: grava o manifesto, põe o disco na pasta da vida (`set_aside_old_disk`), guarda as definições, troca o ID e entra na tabela do vencedor. Se o tempo acaba com nós faltando, a condição `members-missing` fica visível.
- ✅ **Implementado (etapa 4a, D175).** O perfil de um nó no caminho de produção: `ClusterRules.single_node` e `start_single_node` (em `cluster/service/mod.rs`), `Config::is_single_node` e `relaxations`, `Tokens::ensure_dev`, a condição `reduced-profile` e o `required` da FIFO limitado ao fator (`cluster/replication/coordinator.rs`). Testes: `tests/single_node.rs`. Falta migrar o resto (4b a 4d). A lista do que está relaxado vai no log da partida, e a condição `reduced-profile` não a traz.
- ✅ **Implementado (etapa 4b, D175).** Os testes de integração foram para o perfil de um nó: `TestNode::builder()` é o perfil (fator 1, sem TLS, sem endereço de entrada, `Transport` real em `127.0.0.1`) e `TestNode::single()` espera o nó ficar ativo e a restauração terminar. Correções achadas no caminho: a varredura pula o disco da vida anterior enquanto ele espera ser posto de lado (ela carregava as mensagens antigas no estado de um tópico e a restauração as achava "já mantidas", ou segurava a pasta aberta e a mudança falhava); `set_aside_old_disk` descarta o estado lido antes da mudança; `move_dir` tenta o rename de novo antes de copiar; o nó solta os bancos dos tópicos ao parar; cancelar o drain de um nó sozinho mantém o disco dele (não apaga mais as definições e as mensagens antes da nova partida); a latência de uma publicação coordenada pelo cluster entra nas estatísticas do tópico. Comportamentos que mudaram para os testes: um reinício traz de volta o que estava vivo como mensagens novas (prontas, com o mesmo `deliveryCount` e um token novo); só o modo desenvolvimento deixa a porta de cluster fechada. Os casos que descreviam o modo desenvolvimento foram reescritos ou retirados na etapa 4c. Achados: PRB-067, PRB-068 e o problema conhecido do teste de reinício da FIFO.
- ✅ **Implementado (etapa 4c, D175).** O modo desenvolvimento saiu do código. `Config` não tem mais `mode` nem `resume_on_start` (a partida de um nó só é `replication.factor=1`); a porta do cluster sempre abre e `RunningNode::cluster_addr()` devolve o endereço, não um `Option`. `NodeShared::new` não recebe o modo e o nó nasce `joining`. O `DrainController` perdeu a marca de save (`save_completed` saiu de `NodeStateStore`): cancelar um drain `drained` sempre devolve como nó novo. `Topics::open`, `Tokens::open` e o `resume` de `Messages::new` saíram (a partida só apaga o disco que não é de vida anterior); o backup sem cluster (`Solo`) saiu e o backup sem transporte fica desligado, com aviso. `mode` e `development.resume-on-start` ficam numa lista de chaves removidas (`params::REMOVED`): aceitas, ignoradas, com aviso no log. O `NodeStatus` não tem mais `mode` (o TUI do `taba-cli` mostra só a versão). Saíram do catálogo de erros o `topic.fifo-needs-replication` e das docs 16 e 19 a linha dele. Os testes unitários que montavam um nó de desenvolvimento passaram a montar um nó de fator 1. Os testes de reinício a partir do disco (`Messages::new` com `resume`) saíram, e a prova de reinício fica com os testes de restauração (RTN) e os de integração. Casos retirados: DRN-021 a 027 (025 estava só no 26b), DEV-001, 003, 004, 005, 010 e 013, RTN-011 e SEG-005. A documentação que guia o usuário (início rápido, CLI, Compose, Helm) é a etapa 4d.
- ✅ **Implementado (etapa 4d, D175).** O chart do Helm: `taba.isDev` vale com `config.replication.factor: 1` ou com o antigo `config.mode: development`; nesse perfil o chart tira o `mode` da configuração e escreve `replication.factor: 1` e `cluster.tls.enabled: false` (a porta do cluster sempre abre no nó, então vai também a `cluster.port`; o `entry-address` só no cluster). Os guias (início rápido, CLI, README, chart) passaram a `TABA_REPLICATION_FACTOR=1` (um nó para desenvolvimento, sem token raiz, sem autenticação) e `--set config.replication.factor=1`.
- ✅ **Implementado (etapa 5a).** O **código**: `Messages::raise_delivery_count` e a chamada em `Recovery::apply_local` (quando a carga acha a mensagem já mantida, o nó toma o maior `deliveryCount`); o `MemoryStore` de teste ganhou `set_slow` (as leituras e listagens de mensagens e diário esperam). Os **testes**: `tests/cold_start.rs` (memória perdida com o cluster inteiro fora, log que perde as últimas entradas, balde lento), `tests/restore_lives.rs` (maior contador), `tests/messages.rs` (pastas de vidas sem teto) e `engine/messages/tests/blocks.rs`. Casos **UC-RTN-045 a UC-RTN-048 e UC-BKP-045**. PRB-062 saiu da lista de pendências e o item (c) do PRB-065 também.
- ✅ **Implementado (etapa 5b).** Só **testes**, sem código novo: `tests/restore_lives.rs` (vida fora da janela; `Existing` pulado), `tests/restore_vote.rs` (ordem de `id` da FIFO do nó que volta; cluster travado por perda de dois nós, liberado por forçar) e `tests/cold_start.rs` (a propriedade da seção 4.8 no cluster simulado, em três rodadas de queda e retorno). Casos **UC-RTN-049 a UC-RTN-053**.
- ✅ **Implementado (etapa 5c).** Um **código** e testes. O código: `Config::validate` recusa `storage.excluded.max-size` e `storage.excluded.max-age` zerados (um log que não guarda nada deixaria toda vida anterior fora da janela, e a restauração seria calada); `config/tests.rs`. Os testes: `tests/restore_vote.rs` (a mensagem já consumida cujo log estava num nó que não voltou só volta depois de `G`; com todos os nós presentes ela não espera `G` e não volta) e `tests/cold_start.rs` (a maior versão vence sobre o disco do coordenador; o nó atrasado que sobe sozinho serve o que tem e a versão maior vence quando outro responde; o nó que faltava entra depois pelo join normal e a condição `members-missing` some). Casos **UC-RTN-054 a UC-RTN-057** e **UC-CFG-013** ampliado.

### 4.15 O processo de partida, passo a passo

A tabela junta o que o código faz hoje (produção, em cluster) e o que a proposta **mantém**, **muda** ou **acrescenta**, na ordem em que acontece.

| # | Passo | Hoje | Na proposta |
|---|---|---|---|
| 1 | Identidade | ID novo a cada processo (`id::new_at`) | **Mantém.** O ID antigo passa a ser o **nome da pasta** da vida (4.5) |
| 2 | Banco do nó (`_node.db`) | Abre. Tópicos e tokens **não** são carregados em produção (`load = dev && resume_on_start`) | **Muda.** Abre com o esquema novo (versões e lápides, 4.2) e **lê as definições** para a memória, mas só as usa se o nó assumir o disco (passo 6) ou como contribuição à reunião do coordenador |
| 3 | Mensagens em disco | Fica à espera (`new_keeping_disk`). A pasta só é posta de lado em `_previous-life` **depois** que a listagem achou o cluster; se o nó parte sozinho, o disco é descartado | **Muda.** O disco **sempre** vira uma **pasta de vida em quarentena**, com manifesto (4.5), tenha o nó achado o cluster ou não. Várias vidas convivem. Nada é apagado antes do voto final |
| 4 | Backup local do S3 (`_backup.db`) | Zerado em toda partida de produção | **Mantém** |
| 5 | Descoberta | Listagem (`ListNodes`) no `cluster.entry-address`. Sem resposta, só parte sozinho quem tem `cluster.first-node`, ou, com S3, quem o bucket autoriza (`startup_decision`) | **Acrescenta** a **tabela de IPs da vida anterior** (do manifesto) às tentativas. Sem resposta de ninguém, o nó **assume que é o primeiro** (4.9); `first-node` fica só para o cluster que nunca existiu. Com S3, a decisão do bucket continua valendo para quem parte sozinho |
| 6 | Partir sozinho (`start_alone`) | Tabela só com ele, versão 1; **libera o disco antigo para a varredura apagar** | **Muda.** Cria a tabela e **assume o disco**: carrega tópicos, tokens, versões e lápides (4.9). As mensagens seguem em quarentena e entram pela restauração (4.6) |
| 7 | Vários nós se conectam juntos | Não existe: cada um entra pelo join, um por vez | **Acrescenta.** O de **menor ID coordena**: reúne as definições de **todos** os conectados (maior versão vence) e as distribui (4.9). Os grupos que não se alcançaram se resolvem por 4.14 |
| 8 | Join e sincronização | `Join` (com ID, endereços, porta e **versão do programa**) a um membro sorteado; a sincronização traz definições, blocos e eventos; o nó fica `Active` | **Mantém.** O nó que chega depois entra **limpo** e recebe as definições do cluster. A **versão** que o join já carrega é usada pelo drain das anteriores (4.12) |
| 9 | Recuperação da vida anterior | Em segundo plano, devolve **só as perdas que os pares listam** e apaga a pasta (D111) | **Muda.** Envia **todas** as mensagens da quarentena ao novo primário de cada bloco, que confere os mortos (4.6); espera os nós esperados ou `G` (4.8) |
| 10 | Varredura do S3 | Não existe na partida; só o `restore()` do sobrevivente único | **Acrescenta.** Último passo da partida, por bloco de que o nó é primário (4.13) |
| 11 | Filas | Tudo disponível assim que o nó está `Active` | **Muda.** As filas **comuns** recebem e entregam já com as definições distribuídas; a **FIFO** só recebe até o status **restaurado do disco** (4.7, 4.10) |
| 12 | Fim | A pasta é apagada ao terminar a recuperação | **Muda.** Votação dos presentes, status **restaurado do disco** e só então as pastas são apagadas (4.7) |
| 13 | Em segundo plano | Nada | **Acrescenta.** Sondagem dos IPs antigos (4.14) e gravação periódica do `last_seen` no manifesto (4.5) |

**O que não muda:** a entrada de um nó num cluster que já está de pé (join, sincronização, quórum), o drain, o freezing e a votação de exclusão.

### 4.16 Fim do modo desenvolvimento: o perfil de um nó

Hoje o `mode=development` é um nó sem cluster, com persistência própria (`dev && resume_on_start` em `node.rs`), sem TLS, sem autenticação e com a FIFO aceita sem réplica. Com a proposta, a partida de produção já assume o disco (seção 4.15), então esse modo **deixa de existir**. Não há mais `mode`: **todo nó é um nó de cluster**, e o desenvolvimento usa o mesmo caminho com uma configuração mínima.

**Padrão de produção.** Todos os valores padrão continuam sendo os de produção, para **no mínimo 3 nós**: `replication.factor` de produção, TLS do cluster ligado, autenticação pela configuração de produção, `cluster.entry-address` obrigatório. Nada do que está abaixo muda o padrão.

**O perfil de um nó** é um conjunto documentado de parâmetros, que o desenvolvedor liga de propósito:

| Parâmetro | Valor no perfil | O que faz |
|---|---|---|
| `replication.factor` | `1` | Um nó só. O quórum de escrita sai do fator (maioria de 1 é 1; `write.quorum=1` também vale). O nó parte sozinho, **sem** `cluster.entry-address` e **sem** `cluster.first-node` |
| `cluster.tls.enabled` | `false` | Desliga o TLS do cluster, sem pasta de segredos. **Só é aceito com `replication.factor=1`**: com mais de um nó o processo recusa partir |
| `auth.dev-token` | um segredo de pelo menos 32 caracteres | **Cria**, na partida, um token de trabalho **`admin`** (não o root) com esse segredo, **se ele ainda não existir**, para exemplos, Compose e testes. Liga a autenticação. **Só é aceito com `replication.factor=1`**; a documentação o marca como **só para desenvolvimento** |
| `health.single-node-condition` | `true` (padrão) | Mostra no `/health` a condição **perfil reduzido** quando o nó roda com algo relaxado (fator 1, TLS desligado ou `auth.dev-token`), listando o que é. É informativa: não derruba o `/health/ready`. `false` a esconde |

Nomes e valores finais ainda a definir.

**O token `dev-token`.** É um token de trabalho comum, guardado como qualquer outro (só o hash do segredo, nunca em claro), com as permissões `admin`, `publish` e `consume`. `admin` inclui `manage` e `create`, e não tem prefixos. A partida o cria se não existir, e se o segredo do parâmetro mudou, o atualiza. Como não é root, **não faz o que só o root faz**: parar o cluster inteiro e criar, alterar ou revogar outros tokens `admin`. Sem `auth.root-token`, essas operações ficam indisponíveis no perfil de um nó.

**Regras que mudam no código**

- **FIFO com fator 1.** O quórum da FIFO passa a ser `min(2, fator)`, e a recusa de FIFO com fator menor que 2 (`fifo_needs_replication`) sai.
- **Persistência.** Vale o caminho da seção 4.15: sem outro nó, o nó assume o disco e restaura a quarentena para si. O primário é ele para todos os blocos, e a votação do fim tem um só votante.
- **Começar limpo:** `development.resume-on-start=false` deixa de existir. Apaga-se o volume ou o diretório.

**Sai do código:** o `Mode`, o carregamento direto de tópicos, tokens e mensagens, a marca de save como condição de retomada, e o ramo de "sem cluster" (`Solo`) do backup, porque o nó de um nó **tem** cluster, só com ele.

**Custo.** O modo aparece no `docker-compose.dev.yml`, nos testes do Helm (`uc-02-desenvolvimento.sh`), em `docker/cluster.sh`, em testes (por exemplo `Mode::Development` no drain) e em vários documentos. A migração dessas peças é parte da etapa 4.

**Exemplo** de um nó para desenvolvimento:

```
TABA_REPLICATION_FACTOR=1
TABA_CLUSTER_TLS_ENABLED=false
TABA_AUTH_DEV_TOKEN=um-segredo-qualquer-com-32-ou-mais-caracteres
```

## 5. Riscos e pontos em aberto

Não há ponto de desenho em aberto. Um ponto decidido sai desta lista e passa a ser descrito na proposta; os códigos não são reaproveitados.

Resta definir **valores e nomes**: o prazo `G` e a configuração do modo estrito (seção 4.8), os limites do log de excluídos e a folga de relógio (seção 4.4), o tempo e o intervalo da sondagem (seção 4.14). Antes do código da etapa 3 vem o **teste de propriedade** da regra da seção 4.8.


## 6. Alternativas consideradas

| Alternativa | Por que não |
|---|---|
| O `first-node` retoma sozinho do disco (como o modo desenvolvimento) | Os outros nós descartam o disco deles e perdem mensagens que só eles tinham; um erro de rede no `first-node` com pares vivos o faria subir sozinho com dado velho e dividir o cluster; exige uma partida a frio coordenada |
| Uma versão única de configuração do cluster | Não detecta o que o `ack` mudou: um nó fora por uma semana tem a mesma versão e ainda guarda mensagens já consumidas |
| "O nó de maior versão vence" para as mensagens | As mensagens são particionadas pelo anel; o nó de maior versão não tem tudo. O certo para mensagens é a **união** do que cada nó tem, conferida contra os logs de mortos |
| Liberar cada bloco por testemunhas (donos antigos que anunciam o manifesto) | Obriga a calcular quem eram os donos antigos com o fator, o quórum e o anel **da vida anterior**, e a provar qual tabela é a final. O fluxo em que cada nó envia tudo ao novo primário, que consulta os mortos, dispensa esse cálculo |
| ID estável entre vidas | Permitiria ressincronizar só o delta, mas traz de volta o nó desatualizado que reaparece. É a mudança maior e fica para depois |

## 7. Etapas sugeridas

0. ✅ **Versão no ingresso e drain das anteriores (feita)** (seção 4.12): anúncio de versão e drain, um a um, dos nós de versão anterior. Precede a etapa 1, porque a etapa 1 já muda o formato do disco.
1. ✅ **Persistir versões e lápides das definições (feita, D166)**, com o `topicId`, e carregá-las na partida. Não muda o comportamento visível da API.
2. **Log de excluídos durável**, com limite de tamanho e de tempo, em cada dono do bloco e propagado com ele.
3. **Quarentena por vida e restauração:** pasta por ID, manifesto, coordenação pelo nó de menor ID, envio ao novo primário, consulta de mortos (log local, outros nós e S3), votação do fim, varredura do S3 na partida (seção 4.13), liberação da FIFO e a operação de forçar. Precedida do teste de propriedade da seção 4.8.
4. **Remover o `mode=development`** e criar o perfil de um nó (seção 4.16), depois que a etapa 3 funcionar.
5. **Completar a cobertura** dos 15 casos que ainda não têm teste no nível previsto (seção 9.8), em três grupos de cinco (5a a 5c).
6. **Revisar a documentação** (etapa 6): tirar os comentários nascidos da execução, levar o que serve aos documentos oficiais e apagar este documento.
7. **Fora do alcance:** escrita em disco no commit, que preservaria também o que estava só em memória.
8. Decidir depois o **ID estável** e o ressincronismo só do delta.

## 8. Perguntas para decidir

1. Que prazo e que forma tem a operação de **forçar a liberação**, e quem pode chamá-la? (seção 4.8)
2. "Pasta por ID" é a leitura certa, ou a intenção é o nó **reutilizar o mesmo ID**? (seções 4.5 e 6)
3. Qual o valor inicial de `G` e como se chama a configuração do modo estrito? (seção 4.8)
4. Com S3, a restauração do sobrevivente único de hoje (`spawn_restore`) continua como está, quando o disco está vazio, junto da varredura da seção 4.13, ou a varredura a substitui? (seções 4.13 e 4.15, passo 5)

## 9. Casos de uso

Os casos abaixo provam que cada regra da proposta tem um comportamento observável e um teste previsto ([26-casos-de-uso.md](26-casos-de-uso.md)). **Ficam neste documento até a implementação**: só depois de executados (com teste) eles são movidos para o catálogo oficial ([26a](26a-casos-de-uso-implementados.md)), e cada um recebe então o **próximo número livre** da área (`UC-ÁREA-NNN`). Até lá têm um código **provisório** `NOVO-ÁREA-NN`, que não existe nos catálogos e não pode ser citado em teste. Nível: **U** unitário, **I** um nó, **C** cluster simulado.

### 9.1 Definições, log de excluídos e quarentena

| Código | Caso → resultado | Nível | Seção |
|---|---|---|---|
| NOVO-DEF-01 | Tópicos, tokens, versões e lápides (com `topicId`) são gravados no `_node.db` → sobrevivem ao reinício do processo | C | 4.2 |
| NOVO-DEF-02 | A lápide guarda o `topicId` → ela não apaga um tópico recriado com o mesmo nome e outro `topicId` | U | 4.2 |
| NOVO-DEF-03 | Junção de dois estados de definições → vence a maior versão por definição, e a lápide de versão maior vence a definição | U | 4.2 |
| NOVO-DEF-04 | Nó que parte sozinho e assume o disco → carrega tópicos, tokens, versões e lápides e atende sem esperar o cluster | C | 4.9 |
| NOVO-DEF-05 | Nó que chega com o cluster já de pé → entra limpo e recebe as definições do cluster; as do disco dele não as sobrepõem | C | 4.9 |
| NOVO-ESC-01 | Ack de uma mensagem → todo nó que guarda cópia (primário ou não) grava o registro de exclusão no log local **antes** de confirmar | C | 4.4 |
| NOVO-ESC-02 | Envio à dead-letter → também grava o registro de exclusão | C | 4.4 |
| NOVO-ARM-01 | O log tem limite de tamanho e de tempo → os registros mais antigos saem primeiro | U | 4.4 |
| NOVO-ARM-02 | Data de início da gravação contínua → começa na criação do log e só avança quando um registro é descartado pelo limite (um cluster sem exclusões por três dias cobre os três dias) | U | 4.4 |
| NOVO-ARM-03 | Logs de vidas passadas → são incorporados ao atual, e a data de início só recua se não há buraco entre os intervalos | U | 4.4 |
| NOVO-ARM-04 | O registro é gravado na mesma transação que apaga a mensagem, com `storage.db.synchronous=NORMAL` → não há fsync extra no ack | U | 4.4 |
| NOVO-SIN-01 | Os donos do bloco mudam → o log acompanha o bloco, e o nó novo herda a data de início das fontes (não a puxa para a data da entrada) | C | 4.4 |
| NOVO-SIN-02 | Data de corte do cluster → é a **mais recente** entre os membros ativos (o elo mais fraco) | C | 4.4 |
| NOVO-OPS-01 | A data de corte do cluster → fica visível ao operador | I | 4.4 |
| NOVO-RTN-01 | Uma vida cujo `last_seen` é anterior à data de corte (com a folga) → não é restaurada e segue o caminho de hoje (disco descartado, só as perdas listadas) | C | 4.4 |
| NOVO-RTN-02 | O disco da vida anterior → vira **sempre** uma pasta com o ID antigo, tenha o nó achado o cluster ou não, e várias vidas convivem (um segundo reinício não apaga a primeira) | C | 4.5 |
| NOVO-RTN-03 | Manifesto da vida → traz o ID, a tabela de membros com IPs e versão, o `last_seen` e a marca d'água; o `last_seen` é gravado periodicamente e na parada limpa | U | 4.5 |
| NOVO-RTN-04 | A quarentena → não é apagada pela recuperação; só no fim da restauração, depois do voto | C | 4.5, 4.7 |
| NOVO-RTN-05 | O disco enche por causa das pastas de vidas → não há teto próprio; vale o comportamento de sempre (o nó entra em `failed`) | I | 4.5 |

### 9.2 Restauração das mensagens

| Código | Caso → resultado | Nível | Seção |
|---|---|---|---|
| NOVO-RTN-06 | Cada nó lê a quarentena bloco a bloco → envia as mensagens ao **novo primário** do bloco, com o ID original, pelo caminho `Recover`; o nó não decide nada | C | 4.6 |
| NOVO-RTN-07 | Mensagem de tópico excluído ou recriado com outro `topicId` → é descartada, sem consultar mortos | C | 4.6 |
| NOVO-RTN-08 | O `id` consta no log local do novo primário → a mensagem é descartada | C | 4.6 |
| NOVO-RTN-09 | O `id` consta no log de outro nó presente → é descartada; a consulta vai em lote por bloco a **todos** os presentes, não só aos donos | C | 4.6 |
| NOVO-RTN-10 | O diário do S3 registra a remoção do `id` → a mensagem é descartada | C | 4.6 |
| NOVO-RTN-11 | S3 inacessível → o primário decide com as duas primeiras fontes e registra um aviso | C | 4.6 |
| NOVO-RTN-12 | Mensagem só na vida, dentro da janela e sem registro de exclusão em nenhuma fonte → é ativada (replica com quórum e fica disponível) | C | 4.6 |
| NOVO-RTN-13 | O primário já tem a mensagem (`Existing`) → ela é pulada | C | 4.6 |
| NOVO-RTN-14 | Estado de uma mensagem em duas cópias → se qualquer uma diz morta, vale a morta; invisibilidade e recibo são zerados e o `deliveryCount` fica com o maior | U | 4.3 |
| NOVO-RTN-15 | Recebimento durável → o primário grava em disco e replica com quórum **antes** de confirmar, e só então a origem marca "enviada"; uma segunda queda não perde a mensagem nem a reenvia | C | 4.7 |
| NOVO-RTN-16 | Todos os presentes avisam que terminaram → o coordenador (menor ID) abre a votação, que exige todos os presentes; o status vira **restaurado do disco**, a FIFO é liberada e só então as pastas são apagadas | C | 4.7 |
| NOVO-RTN-17 | A votação não fecha → a FIFO continua sem entrega e a condição `previous-life-pending` fica visível até a operação de forçar | C | 4.7 |
| NOVO-RTN-18 | Nó que volta depois da liberação → entra limpo; suas mensagens FIFO são ativadas em ordem de `id`, e a ordem delas em relação às mais novas não é garantida | C | 4.7 |
| NOVO-RTN-19 | Filas comuns durante a restauração → recebem e entregam assim que as definições estão distribuídas; as antigas vão entrando depois | C | 4.7 |
| NOVO-OPS-02 | A condição `previous-life-pending` e o aviso de restauração com nós ausentes → aparecem no `/health` e no log | I | 4.7, 4.8 |

### 9.3 Nós que não voltam e partida a frio

| Código | Caso → resultado | Nível | Seção |
|---|---|---|---|
| NOVO-RTN-20 | Nem todos os nós esperados (membros da maior tabela antiga, pelos manifestos) conectaram → a restauração das mensagens antigas **não começa**; as definições e as filas comuns não esperam | C | 4.8 |
| NOVO-RTN-21 | Passou o prazo `G`, modo padrão → a restauração segue com um aviso, usando os nós presentes | C | 4.8 |
| NOVO-RTN-22 | Passou `G`, modo estrito → a restauração segue retida até a operação de forçar | C | 4.8 |
| NOVO-RTN-23 | Operação de forçar → muda o status e libera o que está retido; serve também ao cluster travado por perda de dois nós | C | 4.8 |
| NOVO-RTN-24 | **Teste de propriedade:** com todos os nós esperados presentes → nenhuma mensagem já consumida é entregue de novo | C | 4.8, 4.11 |
| NOVO-RTN-25 | Fora disso (nó com registros ausente após `G`, ou só nós velhos que se confirmam) → a reentrega só pode acontecer depois de `G` | C | 4.8, 4.11 |
| NOVO-RTN-26 | Uma queda faz o log perder as últimas entradas → a reentrega é aceita (o consumidor é idempotente) e o teste a documenta | C | 4.11 |
| NOVO-RTN-27 | O que estava só em memória em todos os nós → é perdido | C | 4.11 |
| NOVO-RTN-28 | Todos os nós sobem juntos e nenhum responde → o nó assume que é o primeiro, carrega as definições e deixa as mensagens em quarentena (**substitui UC-RTN-017**) | C | 4.9 |
| NOVO-RTN-29 | A descoberta → também tenta a tabela de IPs da vida anterior, que está no manifesto | C | 4.9 |
| NOVO-RTN-30 | Vários nós se conectam → o de menor ID coordena, reúne as definições de **todos** os conectados (maior versão vence) e as distribui; um coordenador de disco velho não as impõe | C | 4.9 |
| NOVO-RTN-31 | Um nó atrasado sobe sozinho → serve com as definições que tem, e a janela termina quando algum nó responde e a versão maior vence | C | 4.9 |
| NOVO-RTN-32 | `cluster.first-node` → só é necessário para o cluster que nunca existiu, sem tabela anterior | C | 4.9 |
| NOVO-RTN-33 | Dois grupos assumiram o disco sem se falar → cada nó sonda os IPs antigos, no intervalo configurado, enquanto o cluster tem menos membros que a tabela antiga, até o tempo total configurado | C | 4.14 |
| NOVO-RTN-34 | A sondagem termina → quando a tabela completa, ou quando o tempo acaba; se faltam nós, uma condição fica visível, e o nó que falta entra depois pelo join normal | C | 4.14 |
| NOVO-RTN-35 | Quando os grupos se alcançam → vale o desempate do D101; o perdedor move o disco para a quarentena, entrega as definições dele, refaz o join com ID novo e **não** usa `handle_exclusion` | C | 4.14 |
| NOVO-RTN-36 | Mensagens aceitas pelo grupo perdedor enquanto esteve separado → são restauradas e conferidas contra os mortos | C | 4.14 |

### 9.4 FIFO

| Código | Caso → resultado | Nível | Seção |
|---|---|---|---|
| NOVO-FIF-01 | A FIFO durante a restauração → recebe mensagens e não entrega, até o status **restaurado do disco** | C | 4.10 |
| NOVO-FIF-02 | O gerador de IDs → começa acima da marca d'água; os `id` novos são maiores que os antigos e nenhum `id` já devolvido muda | U, C | 4.10 |
| NOVO-FIF-03 | Mensagens novas na restauração → vão para a lista provisória do primário, sem posição efetiva, e nenhuma deduplicação acontece (todas são aceitas) | C | 4.10 |
| NOVO-FIF-04 | A liberação → o primário deduplica contra as chaves restauradas e dentro da lista, em ordem de `id`, descarta as duplicatas e só então libera a entrega | C | 4.10 |
| NOVO-FIF-05 | O produtor de uma mensagem depois descartada como duplicata → recebeu `duplicate=false` (exceção aceita) | C | 4.10 |
| NOVO-FIF-06 | Mensagens FIFO restauradas → chegam ao primário do grupo pelo caminho da seção 4.6 e ficam sem entrega até a liberação | C | 4.10 |

### 9.5 S3, atualização de versão e perfil de um nó

| Código | Caso → resultado | Nível | Seção |
|---|---|---|---|
| NOVO-BKP-01 | Última etapa da partida com S3 → o nó lista segmentos e diário dos blocos de que é primário e indexa as mensagens vivas (sem remoção no diário) | C | 4.13 |
| NOVO-BKP-02 | As mensagens achadas no S3 → passam pela mesma conferência (tópico, mortos no log local e nos outros nós, quórum); as que o nó já tem são puladas | C | 4.13 |
| NOVO-BKP-03 | A varredura → não impede o cluster de ficar disponível; segura só a FIFO, porque o aviso de "terminei" inclui a varredura | C | 4.13 |
| NOVO-BKP-04 | S3 inacessível na partida → o passo é pulado com aviso e o nó segue só com o disco | C | 4.13 |
| NOVO-BKP-05 | A consulta ao S3 → distingue "removida" de "não encontrada" e lê o diário uma vez por tópico, com cache | U | 4.13 |
| NOVO-BKP-06 | Mensagem sem remoção no diário do S3, mas morta no log local → é descartada: o S3 não é prova de vida e não alarga a janela | C | 4.4, 4.13 |
| NOVO-ING-01 | Um nó de versão nova entra → drena os de versão anterior um a um; o próximo só começa quando o anterior chegou a `drained` | C | 4.12 |
| NOVO-ING-02 | O próximo drain deixaria menos de 3 nós ativos, ou o nó novo não terminou de sincronizar → não começa | C | 4.12 |
| NOVO-ING-03 | Um nó não drena no prazo → o processo para ali e uma condição visível aparece | C | 4.12 |
| NOVO-ING-04 | Convivência de versões → as mensagens novas entre nós não são enviadas a quem não anunciou suportá-las | C | 4.12 |
| NOVO-ING-05 | Enquanto há um nó de versão anterior → nenhuma vida é restaurável; depois que o último sai, o log vale a partir da data em que **todos** gravam | C | 4.12 |
| NOVO-ING-06 | Disco de uma versão anterior → não é migrado; o nó novo segue o caminho de hoje | C | 4.12 |
| NOVO-ING-07 | Uma versão anterior que entra → não drena as mais novas | C | 4.12 |
| NOVO-CFG-01 | Sem parâmetros → valem os padrões de produção: `cluster.entry-address` exigido, TLS ligado e `replication.factor` de produção | U | 4.16 |
| NOVO-CFG-02 | `replication.factor=1` → o nó parte sozinho, sem `cluster.entry-address` e sem `cluster.first-node`, e o quórum de escrita é 1 | I | 4.16 |
| NOVO-CFG-03 | `cluster.tls.enabled=false` → só é aceito com `replication.factor=1` (com mais de um nó o processo recusa partir) e dispensa a pasta de segredos | U | 4.16 |
| NOVO-CFG-04 | `auth.dev-token` → só é aceito com `replication.factor=1` e exige pelo menos 32 caracteres | U | 4.16 |
| NOVO-CFG-05 | Parâmetros novos (limites do log, folga de relógio, `G`, modo estrito, sondagem) → valores inválidos impedem a partida | U | 4.4, 4.8, 4.14 |
| NOVO-CFG-06 | `mode` e `development.resume-on-start` → deixam de existir | U | 4.16 |
| NOVO-AUT-01 | `auth.dev-token` → a partida cria um token de trabalho com `admin`, `publish` e `consume`, guardado só pelo hash, e liga a autenticação | I | 4.16 |
| NOVO-AUT-02 | O segredo do `auth.dev-token` mudou, ou o nó reiniciou → o token é atualizado e não duplicado | I | 4.16 |
| NOVO-AUT-03 | O `dev-token` não é root → não para o cluster e não cria, altera nem revoga outros tokens `admin` | I | 4.16 |
| NOVO-OPS-03 | `health.single-node-condition` → o `/health` lista a condição **perfil reduzido** com o que está relaxado, sem derrubar o `/health/ready`; `false` a esconde | I | 4.16 |
| NOVO-ESC-03 | FIFO com `replication.factor=1` → é aceita, e o quórum dela é `min(2, fator)` | U, I | 4.16 |
| NOVO-RTN-37 | Perfil de um nó → o reinício restaura tópicos, tokens e mensagens pelo caminho comum (o nó é primário de todos os blocos e a votação tem um votante) | I | 4.16 |

### 9.6 Casos que já existem e mudam

| Caso de hoje | O que acontece | Substituído por |
|---|---|---|
| UC-RTN-017 (primeiro nó apaga o disco antigo) | **Retirado**: o primeiro nó passa a assumir o disco | NOVO-RTN-28 |
| UC-RTN-014, UC-RTN-015, UC-RTN-016 (recuperação do reinício devolve só as perdas) | **Reescritos**: a recuperação passa a enviar todas as mensagens da quarentena ao novo primário, que confere os mortos | NOVO-RTN-02, NOVO-RTN-06 a NOVO-RTN-13 |
| RTN-011 (recuperação em modo desenvolvimento não se aplica) | **Retirado**, junto com o modo | NOVO-RTN-37 |
| DEV-001 a DEV-013 (modo desenvolvimento) | **Retirados**, um a um: 001 e 004 por NOVO-CFG-06; 002 e 003 por NOVO-CFG-02 e NOVO-ESC-03; 005 e 011 por NOVO-OPS-03; 010 por NOVO-AUT-01 e NOVO-AUT-03; 013 por NOVO-RTN-37. Os casos 006 a 009 e 012 (comportamento do nó único) seguem valendo, agora rodando no perfil de um nó (nível I). Na 4c ficaram 002, 011 e 012 reescritos para o perfil, e 001, 003, 004, 005, 010 e 013 retirados | NOVO-CFG-02, NOVO-AUT-01, NOVO-OPS-03, NOVO-RTN-37 |
| DRN-021 a DRN-027 (drain como *save*) | **Retirados** na 4c: o que era "save" virou a restauração comum | NOVO-RTN-37 |
| UC-TOP-004, UC-TOP-019, UC-CNS-037, UC-FIF-042 (FIFO exige fator pelo menos 2) | **Reescritos**: a FIFO com fator 1 é aceita | NOVO-ESC-03 |

### 9.7 Cobertura: de cada regra ao seu caso

| Seção da proposta | Casos |
|---|---|
| 4.2 Definições | NOVO-DEF-01 a NOVO-DEF-05 |
| 4.3 Estado da mensagem | NOVO-RTN-14 |
| 4.4 Log de excluídos | NOVO-ESC-01, 02; NOVO-ARM-01 a 04; NOVO-SIN-01, 02; NOVO-OPS-01; NOVO-RTN-01; NOVO-BKP-06; NOVO-CFG-05 |
| 4.5 Quarentena por vida | NOVO-RTN-02 a 05 |
| 4.6 Restauração | NOVO-RTN-06 a 13 |
| 4.7 Fim da restauração | NOVO-RTN-15 a 19; NOVO-OPS-02 |
| 4.8 Nós que não voltam | NOVO-RTN-20 a 25 |
| 4.9 Partida | NOVO-DEF-04, 05; NOVO-RTN-28 a 32 |
| 4.10 FIFO | NOVO-FIF-01 a 06 |
| 4.11 Garantias e limites | NOVO-RTN-24 a 27 |
| 4.12 Atualização de versão | NOVO-ING-01 a 07 |
| 4.13 Varredura do S3 | NOVO-BKP-01 a 06 |
| 4.14 Reencontro | NOVO-RTN-33 a 36 |
| 4.15 Processo de partida | NOVO-RTN-02, 28 a 30 e 37 (a tabela só reúne regras das outras seções) |
| 4.16 Perfil de um nó | NOVO-CFG-01 a 06; NOVO-AUT-01 a 03; NOVO-OPS-03; NOVO-ESC-03; NOVO-RTN-37 |

### 9.8 Estado da cobertura (2026-10-22)

Estado dos 82 casos provisórios **depois das etapas 0 a 3 e de 5a a 5c**: **71** cobertos, **0** em parte, **0** sem teste e **11** na etapa 4 (as linhas da etapa 4 ainda não foram reconferidas contra os testes que a 4b a 4d escreveram). Com a 5c, todos os casos que não são da etapa 4 têm teste.

Legenda: ✅ coberto no nível previsto · ◐ coberto em parte (diz o que falta) · ❌ sem teste · ⬜ etapa 4 (ainda não implementado). Os casos oficiais são os do catálogo ([26a](26a-casos-de-uso-implementados.md)); "Código efetivo" é onde a regra está no código.

| Código | Estado | Casos oficiais | Código efetivo | Testes / o que falta |
|---|---|---|---|---|
| NOVO-DEF-01 | ✅ | UC-DEF-039 | `storage/versions`, `cluster/replication/definitions.rs` | `tests/definition_versions.rs`, `storage/versions/tests.rs` |
| NOVO-DEF-02 | ✅ | UC-DEF-040 | `storage/versions`, `cluster/replication/definitions.rs` | `tests/definition_versions.rs`, `storage/versions/tests.rs` |
| NOVO-DEF-03 | ✅ | UC-DEF-026, 027, 035 | `cluster/replication/definitions.rs` | `cluster/replication/definitions/tests.rs` |
| NOVO-DEF-04 | ✅ | UC-DEF-041 | `ClusterService::start_alone_with_disk`, `Definitions::resume_from_disk` | `tests/cold_start.rs` |
| NOVO-DEF-05 | ✅ | UC-DEF-042, UC-RTN-039 | `ClusterService::release_old_disk`, `recover_previous_life`, `Definitions::offer` | `tests/cold_start.rs` |
| NOVO-ESC-01 | ✅ | UC-ESC-037 | `storage/messages/excluded`, `engine/messages` | `tests/excluded_log.rs`, `storage/messages/excluded/tests.rs` |
| NOVO-ESC-02 | ✅ | UC-ESC-038 | `storage/messages/excluded`, `engine/messages` | `tests/excluded_log.rs`, `storage/messages/excluded/tests.rs` |
| NOVO-ESC-03 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-ARM-01 | ✅ | UC-ARM-026 | `storage/messages/excluded` | `storage/messages/excluded/tests.rs` |
| NOVO-ARM-02 | ✅ | UC-ARM-027 | `storage/messages/excluded` | `storage/messages/excluded/tests.rs` |
| NOVO-ARM-03 | ✅ | UC-ARM-028 | `storage/messages/excluded` (`import_excluded`) | `storage/messages/excluded/tests.rs` |
| NOVO-ARM-04 | ✅ | UC-ARM-029 | `storage/messages/excluded` | `storage/messages/excluded/tests.rs` |
| NOVO-SIN-01 | ✅ | UC-SIN-031 | `cluster/sync/mod.rs`, `storage/messages/excluded` | `tests/excluded_log.rs` |
| NOVO-SIN-02 | ✅ | UC-SIN-032 | `Recovery::cluster_cutoff` | `tests/excluded_log.rs` |
| NOVO-OPS-01 | ✅ | UC-OPS-052 | `cluster/health`, `NodeStatus.excludedSince` | `tests/excluded_log.rs` |
| NOVO-OPS-02 | ✅ | UC-OPS-053 | `cluster/health` (`previous-life-pending`) | `tests/restore_vote.rs`, `cluster/health/tests.rs` |
| NOVO-OPS-03 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-RTN-01 | ✅ | UC-RTN-018, UC-RTN-049 | `restore::life_restorable`, `Recovery::restore_lives` | `cluster/restore/tests.rs`, `tests/restore_lives.rs` |
| NOVO-RTN-02 | ✅ | UC-RTN-023 | `MessageStore::set_aside_old_disk` (`storage/messages`) | `tests/recovery.rs` |
| NOVO-RTN-03 | ✅ | UC-RTN-019, 024 | `restore::Manifest`, `ClusterService::write_manifest` | `cluster/restore/tests.rs`, `tests/recovery.rs` |
| NOVO-RTN-04 | ✅ | UC-RTN-025 | `MessageStore::drop_old_disk`, `Recovery::drop_lives` | `tests/recovery.rs` |
| NOVO-RTN-05 | ✅ | UC-RTN-048, UC-FAL-028 | `MessageStore::set_aside_old_disk`, `storage::is_fatal` | `tests/messages.rs`, `storage/db/tests.rs` |
| NOVO-RTN-06 | ✅ | UC-RTN-026 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-07 | ✅ | UC-RTN-027 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-08 | ✅ | UC-RTN-028 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-09 | ✅ | UC-RTN-028 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-10 | ✅ | UC-RTN-029 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-11 | ✅ | UC-RTN-029 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-12 | ✅ | UC-RTN-026 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs` |
| NOVO-RTN-13 | ✅ | UC-RTN-050, UC-BKP-041 | `Recovery::restore_lives`, `send_life`, `accept_restore` (`cluster/recovery/restoration.rs`) | `tests/restore_lives.rs`, `tests/scan_s3.rs` |
| NOVO-RTN-14 | ✅ | UC-RTN-045 | `Messages::raise_delivery_count`, `Recovery::apply_local` (`cluster/recovery/mod.rs`) | `engine/messages/tests/blocks.rs`, `tests/restore_lives.rs` |
| NOVO-RTN-15 | ✅ | UC-RTN-030 | `accept_restore` com `durable=true` | `tests/restore_lives.rs` |
| NOVO-RTN-16 | ✅ | UC-RTN-031 | `Recovery::restoration`, `guided`, `decide` (`cluster/recovery/finish.rs`), `restore::Restoration` | `tests/restore_vote.rs` |
| NOVO-RTN-17 | ✅ | UC-RTN-032, 035, UC-OPS-053 | `Recovery::restoration`, `guided`, `decide` (`cluster/recovery/finish.rs`), `restore::Restoration` | `tests/restore_vote.rs` |
| NOVO-RTN-18 | ✅ | UC-RTN-033, UC-RTN-051 | `Recovery::restoration`, `guided`, `decide` (`cluster/recovery/finish.rs`), `restore::Restoration` | `tests/restore_vote.rs` |
| NOVO-RTN-19 | ✅ | UC-RTN-034 | `Recovery::restoration`, `guided`, `decide` (`cluster/recovery/finish.rs`), `restore::Restoration` | `tests/restore_vote.rs` |
| NOVO-RTN-20 | ✅ | UC-RTN-020, 032 | `restore::start`, `expected_nodes`, `life_restorable` (`cluster/restore/mod.rs`) | `cluster/restore/tests.rs`, `tests/restore_vote.rs` |
| NOVO-RTN-21 | ✅ | UC-RTN-032 | `restore::start`, `expected_nodes`, `life_restorable` (`cluster/restore/mod.rs`) | `tests/restore_vote.rs` |
| NOVO-RTN-22 | ✅ | UC-RTN-032 | `restore::start`, `expected_nodes`, `life_restorable` (`cluster/restore/mod.rs`) | `tests/restore_vote.rs` |
| NOVO-RTN-23 | ✅ | UC-RTN-035, UC-RTN-052 | `POST /cluster/restore/force`, `restore::Restoration::force` | `tests/restore_vote.rs` |
| NOVO-RTN-24 | ✅ | UC-RTN-021, UC-RTN-053 | `restore::start`, `expected_nodes`, `life_restorable` (`cluster/restore/mod.rs`) | `cluster/restore/tests.rs` (modelo), `tests/cold_start.rs` (cluster) |
| NOVO-RTN-25 | ✅ | UC-RTN-022, UC-RTN-054 | `restore::start`, `expected_nodes`, `life_restorable` (`cluster/restore/mod.rs`) | `cluster/restore/tests.rs` (modelo), `tests/restore_vote.rs` (cluster simulado) |
| NOVO-RTN-26 | ✅ | UC-RTN-047 | `MessageStore::remove_excluded` (o log na mesma transação) | `tests/cold_start.rs` |
| NOVO-RTN-27 | ✅ | UC-RTN-046, UC-DRN-003 | `Messages` (mensagens em memória) | `tests/cold_start.rs` |
| NOVO-RTN-28 | ✅ | UC-RTN-036 | `ClusterService::discover_previous_table`, `start_alone_with_disk`, `attempt_join` (`cluster/service/mod.rs`) | `tests/cold_start.rs` |
| NOVO-RTN-29 | ✅ | UC-RTN-037 | `ClusterService::discover_previous_table`, `start_alone_with_disk`, `attempt_join` (`cluster/service/mod.rs`) | `tests/cold_start.rs` |
| NOVO-RTN-30 | ✅ | UC-RTN-036, 039, 055 | `ClusterService::discover_previous_table`, `start_alone_with_disk`, `attempt_join` (`cluster/service/mod.rs`) | `tests/cold_start.rs` |
| NOVO-RTN-31 | ✅ | UC-RTN-037, 056 | `ClusterService::discover_previous_table`, `start_alone_with_disk`, `attempt_join` (`cluster/service/mod.rs`) | `tests/cold_start.rs` |
| NOVO-RTN-32 | ✅ | UC-RTN-038 | `ClusterService::discover_previous_table`, `start_alone_with_disk`, `attempt_join` (`cluster/service/mod.rs`) | `tests/cold_start.rs` |
| NOVO-RTN-33 | ✅ | UC-RTN-040, 041 | `ClusterService::run_probe`, `probe_round` (`cluster/service/probe.rs`) | `tests/regroup.rs` |
| NOVO-RTN-34 | ✅ | UC-RTN-041, 042, 057, UC-OPS-054 | `run_probe`, `members_missing`, condição `members-missing` | `tests/regroup.rs`, `tests/cold_start.rs`, `cluster/health/tests.rs` |
| NOVO-RTN-35 | ✅ | UC-RTN-040 | `ClusterService::regroup`, `loses_reunion` | `tests/regroup.rs`, `cluster/service/tests.rs` |
| NOVO-RTN-36 | ✅ | UC-RTN-040 | `ClusterService::regroup` e a restauração das vidas | `tests/regroup.rs` |
| NOVO-RTN-37 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-FIF-01 | ✅ | UC-FIF-048 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `tests/restore_vote.rs`, `engine/messages/tests/hold.rs` |
| NOVO-FIF-02 | ✅ | UC-FIF-049 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `core/id/tests.rs`, `tests/restore_vote.rs` |
| NOVO-FIF-03 | ✅ | UC-FIF-050 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `engine/messages/tests/hold.rs`, `tests/restore_vote.rs` |
| NOVO-FIF-04 | ✅ | UC-FIF-051 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `engine/messages/tests/hold.rs`, `tests/restore_vote.rs` |
| NOVO-FIF-05 | ✅ | UC-FIF-052 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `tests/restore_vote.rs` |
| NOVO-FIF-06 | ✅ | UC-FIF-053 | `Messages::hold_fifo` e a liberação (`engine/messages`), `IdGenerator` com piso (`core/id`) | `engine/messages/tests/hold.rs`, `tests/restore_vote.rs` |
| NOVO-BKP-01 | ✅ | UC-BKP-041 | `Backup::scan_topic`, `Recovery::scan_backup` | `tests/scan_s3.rs` |
| NOVO-BKP-02 | ✅ | UC-BKP-041, 042 | `Backup::scan_topic`, `Recovery::scan_backup` | `tests/scan_s3.rs` |
| NOVO-BKP-03 | ✅ | UC-BKP-045 | `Recovery::restore_lives`, `scan_backup`, `Messages::hold_fifo` | `tests/cold_start.rs` |
| NOVO-BKP-04 | ✅ | UC-BKP-043 | `Recovery::scan_backup` | `tests/scan_s3.rs` |
| NOVO-BKP-05 | ✅ | UC-BKP-044 | `Recovery::journal_removed` (cache) | `tests/scan_s3.rs` |
| NOVO-BKP-06 | ✅ | UC-BKP-042 | `Backup::scan_topic`, `accept_restore` | `tests/scan_s3.rs` |
| NOVO-ING-01 | ✅ | UC-ING-029 | `cluster/upgrade/mod.rs` | `tests/upgrade.rs`, `cluster/upgrade/tests.rs` |
| NOVO-ING-02 | ✅ | UC-ING-030 | `cluster/upgrade/mod.rs` | `tests/upgrade.rs`, `cluster/upgrade/tests.rs` |
| NOVO-ING-03 | ✅ | UC-ING-031 | `cluster/upgrade/mod.rs` | `tests/upgrade.rs`, `cluster/health/tests.rs` |
| NOVO-ING-04 | ✅ | UC-ING-032 | `cluster/upgrade/mod.rs` | `tests/upgrade.rs`, `cluster/upgrade/tests.rs` |
| NOVO-ING-05 | ✅ | UC-ING-037 | `Recovery::cluster_cutoff` | `tests/restore_lives.rs` |
| NOVO-ING-06 | ✅ | UC-ING-036 | `MessageStore::set_aside_old_disk` (disco sem manifesto) | `tests/recovery.rs` |
| NOVO-ING-07 | ✅ | UC-ING-033 | `cluster/upgrade/mod.rs` | `tests/upgrade.rs`, `cluster/upgrade/tests.rs` |
| NOVO-CFG-01 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-CFG-02 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-CFG-03 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-CFG-04 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-CFG-05 | ✅ | UC-CFG-013 | `config/params`, `Config::validate` | `config/tests.rs` |
| NOVO-CFG-06 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-AUT-01 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-AUT-02 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |
| NOVO-AUT-03 | ⬜ | — | — | Etapa 4 (perfil de um nó, seção 4.16): ainda não implementado |

## Fontes e provas

- Estado atual: `node.rs` (abertura de tópicos, tokens e mensagens), `cluster/service/mod.rs` (`start_alone`, `recover_on_boot`, `recover_previous_life`, `attempt_join`), `cluster/recovery/mod.rs`, `storage/messages/mod.rs` (`set_aside_old_disk`), `cluster/replication/definitions.rs` (mapa de versões em memória), `cluster/drain/mod.rs` (saída da tabela e marca de save), `cluster/vote/mod.rs` (a base do voto são os nós saudáveis), `cluster/backup/restore.rs` (`restore`, `recover_missing`, `find_message`, o diário como lista de remoções), `cluster/sync/mod.rs` (`apply_tombstone` na carga), `cluster/transport/mod.rs` (`found_no_cluster`), `cluster/replication/messages_api.rs` (papéis obrigatórios do quórum) e `core/id` (`IdGenerator`, sem piso hoje).
- O comportamento do primeiro nó está fixado pelo teste `tests/recovery.rs` › `the_first_node_of_a_new_cluster_wipes_the_old_disk_and_keeps_no_previous_life`.
- O diário do S3 como lista de remoções é o D144 ([30-decisoes.md](30-decisoes.md)).
- Nenhuma das regras acima foi testada ainda. A regra da seção 4.8 pede um teste de propriedade antes do código: com todos os nós esperados presentes a reentrega nunca acontece, e fora disso só acontece depois de `G`.
