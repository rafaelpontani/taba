---
título: Armazenamento
resumo: Onde cada dado do nó mora — memória primeiro, overflow para o disco, um banco SQLite por tópico, o banco do nó, o banco de eventos — e o que acontece num reinício.
tipo: explicação
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [crates/taba-node/src/storage, crates/taba-node/src/engine/messages/mod.rs, crates/taba-node/src/engine/messages/rates, crates/taba-node/src/engine/topics/mod.rs, crates/taba-node/src/engine/tokens/mod.rs]
---

# Armazenamento

> Leia antes: [03-conceitos.md](03-conceitos.md). Leia depois: [11-durabilidade.md](11-durabilidade.md).

## Regra geral

**As mensagens vivem em memória.** O disco entra por **overflow** (a memória encheu), pelo **drain** (o nó vai sair) e para guardar o que **precisa sobreviver**: as definições de tópico, os tokens e o estado do nó. A durabilidade de uma mensagem confirmada vem da **replicação em vários nós** ([10-publicacao.md](10-publicacao.md)), não do disco de um nó.

Cada mensagem está em **um só lugar** no nó (memória **ou** disco), e todas as operações (entrega, ack, invisibilidade, `ttl`, dead-letter) funcionam igual nos dois.

## Mapa do disco (`storage.dir`)

A última pasta do caminho **precisa se chamar `taba`** (validação de partida). Padrão: `/var/lib/taba`.

```
${storage.dir}/
  _node.db                 banco do nó (SQLite, WAL)
  _backup.db               banco do backup em S3 (só com `backup.s3.bucket`): fila de envio, referências, segmentos
  _sync/sync.db            banco de eventos; só existe durante a sincronização
  _previous-life/          disco da vida anterior, posto de lado num reinício (até a recuperação)
  <topico>/                uma pasta por tópico (maiúsculas viram "~" + minúscula: Pedidos → ~pedidos/)
    _topic.db              banco do tópico (SQLite, WAL), só com o que está em disco
    <id>.body              o corpo de cada mensagem em disco, só os bytes como vieram
```

Tudo é **estritamente local**: nenhum desses arquivos é copiado entre nós. O que trafega são as mensagens e os metadados pelo protocolo de registro + commit.

![Mapa do disco de um nó](../diagrams/img/pt-BR/disk-storage.svg)

## Memória e overflow

- **Limite:** `memory.limit` (sem padrão). **Sem ele, nada vai para o disco por overflow.**
- Ao atingir o limite, as mensagens excedentes são gravadas em disco: primeiro o **corpo** em `<topico>/<id>.body`, depois a **linha** no banco do tópico. Ao ler uma mensagem que não está em memória, ela é buscada no disco.
- **Divisão entre tópicos.** A memória não é dividida igualmente. Cada tópico tem uma **taxa de leitura** (média móvel das mensagens consumidas por segundo na janela `memory.rate.window`, padrão **5 min**; a mesma janela calcula `publishRate` e `consumeRate`). Cada tópico tem direito a uma **reserva** (`memory.topic.reserve`, padrão **1000** mensagens) independente da taxa, para um tópico novo não começar jogando mensagens em disco. Acima das reservas, quem tem menor taxa de leitura perde a memória primeiro.
- **O que fica na memória:** a lista de prontos (as próximas a entregar) e as recém-chegadas. Não se usa LRU/LFU: cada mensagem é lida **uma vez** e apagada pelo ack, então histórico de acesso não prediz nada.
- **Resumo do overflow:** um log `info` (`taba.storage.overflow`) a cada 60 s, só se houve overflow.
- **Drain:** o limite efetivo cai a **zero** e tudo vai para o disco ([07-cluster.md](07-cluster.md)).

## O banco do nó (`_node.db`)

SQLite local, **uma conexão atrás de um `Mutex`** (escritor único). Tabelas:

| Tabela | Chave | Guarda |
|---|---|---|
| `topics` | `name` | A **definição** de cada tópico (JSON). Uma linha por tópico, gravada no commit, numa transação |
| `tokens` | `id` | Os tokens de acesso: nome, permissões, prefixos, datas, **sal** (128 bits) e **hash salgado** do segredo. **Nunca o segredo** |
| `state` | `key` | Estado do nó; hoje a marca `save_completed` (o fim do drain) |
| `losses` | `(topic, id)` | As mensagens declaradas perdidas numa sincronização, consultadas por quem volta de uma exclusão. Expiram em `cluster.message-loss.ttl` (padrão 14 d) |
| `definition_versions` | `(kind, key)` | A **versão** de cada tópico e de cada token que o nó aplicou e a **lápide** de cada remoção, com o `topicId` do tópico e o instante em que foi aplicada. Uma linha por definição, escrita junto com a definição; a lápide sai depois de 1 hora ([37-retomada-do-cluster.md](37-retomada-do-cluster.md)) |

A versão do esquema é gravada no banco (`SCHEMA_VERSION`); um banco de versão **mais nova** que a do nó não é tocado.

## O banco do tópico (`<topico>/_topic.db`)

Tabela `messages`, **uma linha por mensagem em disco**: `id`, `content_type`, `group_id`, `published_at`, `delivery_count`, `invisible_until`, `receipt_token`, `age_base` (de onde o `ttl` conta: a publicação, ou a entrada na dead-letter), `dead_lettered_at`, `dead_letter_reason`, `position` (a posição no anel) e, na FIFO, `deduplication_id` e `delete_at`. Índices por `invisible_until`, `age_base`, `position` e `(group_id, deduplication_id)` e `delete_at`.

Ele responde: a mensagem mais antiga, as próximas a vencer, as mensagens de um bloco (faixa pela posição, para os streams de sincronização e a limpeza) e as contagens.

**Escritor único.** Toda escrita no banco de um tópico passa por **uma fila de comandos**, executada uma de cada vez. Leituras longas (os streams de sincronização) usam conexões de leitura próprias (modo WAL) e não bloqueiam o escritor.

**Ordem de gravação** (para a queda não corromper): na escrita, **primeiro o arquivo, depois a linha**; no ack, **primeiro a linha, depois o arquivo**. Uma queda deixa no máximo **arquivo sem linha** (lixo inofensivo) ou **linha sem arquivo** (a mensagem se perdeu e é descartada na leitura). O nó que reinicia volta como nó novo e refaz do cluster.

**Durabilidade do SQLite:** `storage.db.synchronous` (padrão `NORMAL`, com WAL): como a durabilidade vem da replicação, não se paga o `fsync` de cada gravação.

**Banco corrompido ou disco cheio:** o nó entra em `failed` ([07-cluster.md](07-cluster.md)). **Não há tentativa de reparo.**

## Definições de tópico e tokens: quem decide

Uma definição de tópico ou de token é um dado **pequeno, raro de mudar e do interesse do cluster inteiro**. **Todo nó conhece todas**; não são particionadas pelo anel.

- **Dono da definição.** Cada definição tem um **dono** no anel: o primeiro nó ativo na posição do hash de `topic:<nome>` (ou `token:<id>`). Criação, alteração e remoção são **propostas ao dono** (`POST /cluster/definitions/propose`). Ele confere com o próprio estado (nome já existe, `expectedId`), dá a **próxima versão**, grava num **herdeiro** (o próximo dono da posição que responde; se há outros donos e nenhum responde, a mudança é **recusada com `503`**) e aplica em si. Em seguida manda a mudança decidida a todos os outros (`POST /cluster/definitions/apply`) e responde ao cliente depois dos que respondem dentro de `write.commit.timeout`; os demais a recebem pelo quadro `definition` do canal.
- **Versão:** um nó só aplica uma versão **maior ou igual** à que já tem.
- **Falha:** nunca há alteração parcial visível; um nó só serve a nova definição **no commit**.
- **Remoção:** `DELETE /topics/{topic}` **sempre funciona**, mesmo com mensagens pendentes: cada nó apaga todas as mensagens do tópico que tem (memória, disco e metadados).
- **Dead-letter:** não tem definição própria; é derivada da fila de origem, criada e excluída com ela, e não gera linha em `topics`.
- **Tokens:** o segredo **nunca trafega entre nós**: só o sal e o hash. A validação é uma consulta **local em memória**. Fora do modo desenvolvimento o nó **não confia** nos tokens que acha no disco (podem estar desatualizados); eles vêm do cluster.
- **Dados de consumo** (profundidade, taxas, histogramas) **nunca vão ao disco**: ficam em memória, por nó.

## O que se refaz na partida

A **lista de prontos**, a **lista de invisíveis**, as **contagens por grupo** e as **taxas** são **derivadas** dos metadados e reconstruídas quando o tópico é aberto (reinício ou novo dono). Não se guardam. A lista de invisíveis é **completa** e derivada do `invisible_until` das mensagens; por isso **a tabela `expiry`** que a documentação histórica cita **não existe** no código (pendência D82).

## Banco de eventos (`_sync/sync.db`)

Existe só durante a sincronização de um nó que entra: tabela `events` (número de sequência crescente, método, caminho, cabeçalhos, corpo — a chamada como chegou) e `loaded` (os IDs que a carga trouxe, para a verificação final). É descartado quando a tabela esvazia ([07-cluster.md](07-cluster.md), seção 5). Limite: `cluster.sync.log.max-bytes`.

## Reinício

| Situação | O que acontece |
|---|---|
| **Produção** | Todo reinício é de **nó novo** (ID novo). O disco antigo é posto de lado em `_previous-life` e usado, em segundo plano, para devolver ao cluster mensagens que só ele tinha |
| **Desenvolvimento**, `resume-on-start` ligado | Retoma o que está em `storage.dir`: definições (mantendo `id` e data), mensagens e metadados, listas reconstruídas |
| **Desenvolvimento**, desligado | Apaga `storage.dir` e começa limpo, com aviso |

## O banco do backup (`_backup.db`)

Só existe com o **backup opcional em S3** ([33-backup-s3.md](33-backup-s3.md)). É local como os outros, e **nunca** entra no caminho da publicação: o commit de uma mensagem só **insere uma linha** nele.

| Tabela | Guarda |
|---|---|
| `outbox` | Os `id` das mensagens que o nó (dono) ainda não mandou ao S3. Só o `id`: o corpo e os metadados vêm de onde a mensagem está. Sem teto: se o disco enche, o nó entra em `failed`, como com qualquer falta de espaço |
| `saved` | Onde cada mensagem salva está no bucket (segmento, deslocamento, tamanho): é o que deixa o nó **apagar o corpo do disco** e ler de volta por `Range`. A réplica recebe a referência por aviso do primário |
| `segments` | Para cada segmento que **este nó** gravou: quantas mensagens ainda vivem. Quando chega a zero o primário apaga o segmento do bucket, sem consultar o S3 |
| `journal` | As mensagens com estado alterado (ou removidas) desde o último diário |

Numa partida em produção o banco é **zerado**: o nó é novo, e o que o anterior guardava (fila, referências) não vale para a identidade nova (o que o disco da vida anterior devolve entra de novo pela publicação normal, e o novo dono o enfileira).

**Evicção.** Com o disco acima de `backup.disk.evict-above` (85%), o nó apaga só o **arquivo do corpo** (`<id>.body`) das mensagens que já têm referência no bucket, até `backup.disk.evict-to` (70%). A linha em `_topic.db` fica, então a ordem "arquivo primeiro, linha depois" segue valendo. Na leitura, o corpo ausente vira um `Range` no segmento. Detalhes e a pausa durante o drain: [33-backup-s3.md](33-backup-s3.md).

## Limites conhecidos

- **PRB-025:** `diskBytes` e `bytes` somam o tamanho dos arquivos `.body` do tópico, **uma chamada por mensagem em disco**, a cada leitura de `/metrics/topics*`.
- **PRB-019:** achar a mensagem do meio para rebaixar e conferir as prontas quando `maxDeliveries` muda varrem a memória do tópico.
- **D80:** o nome do tópico vira nome de pasta; nomes reservados do Windows (`con`, `nul`, `aux`, `com1`…) ainda não são tratados.

## Fontes e provas

- `storage/backup` (o depósito de objetos, o segmento, o diário, os nomes e o `_backup.db`); `storage/db` (`_node.db`, esquema), `storage/messages` (corpo, banco do tópico, ordem de gravação, nome da pasta), `storage/synclog`, `storage/state`, `storage/losses`, `storage/tokens`, `storage/versions`; `engine/messages` (overflow, `rates`, `reserve_memory`); `engine/topics`, `engine/tokens`.
- Casos de uso: UC-ARM (armazenamento), UC-DEF (definições replicadas), UC-DRN (drain como save); veja [26a](26a-casos-de-uso-implementados.md).
- Histórico: `02-arquitetura.md` (3.2 a 3.4).
