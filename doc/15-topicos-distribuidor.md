# Tópico Distribuidor e fila volátil

[← Índice](README.md)

## 14. Tópico Distribuidor, fila volátil e nó gestor

O Taba tem dois **tipos de tópico** e um **atributo** de ciclo de vida.

> **Análogos ao SQS e ao SNS.** O **Queue Topic (QT)** é o análogo do Amazon SQS, e o **Distributor Topic (DT)** é o análogo do Amazon SNS. São análogos, **não equivalentes**: o Taba usa nomes próprios para não misturar o seu comportamento com o desses serviços. Em particular, o DT não tem assinantes nem protocolos de entrega (HTTP, e-mail etc.): ele só copia a publicação para os QTs cujo nome começa com um de seus prefixos.

### 14.1 Nomenclatura

| Nome | Sigla | Tipo na API | Descrição |
|---|---|---|---|
| **Tópico Fila** (*Queue Topic*) | **QT** | `queue` | Guarda mensagens e as entrega a consumidores, uma mensagem para um consumidor (seções 12 e 13). Substitui o antigo tipo `single`. |
| **Tópico Distribuidor** (*Distributor Topic*) | **DT** | `distributor` | Não guarda mensagens. Recebe uma publicação e a **copia para os QTs** cujo nome começa com algum dos seus prefixos (seção 14.2). Substitui o antigo tipo `broadcast`. |
| **Fila volátil** (*Queue Topic Volatile*) | **QTV** | `queue` com `volatile: true` | Um QT que é **excluído sozinho**, com suas mensagens, se ficar sem consumidor por mais de `idleTimeout` (seção 14.3). |
| **Nó gestor** | — | — | Nó administrativo que cuida da vida de uma QTV: decide quando ela expirou (seção 14.5). Não participa do caminho de dados. |

Nos textos, "fila" sem qualificação significa QT. Um DT **nunca** é consumido: `GET` e ack de mensagem num DT são rejeitados com `409`.

### 14.2 Tópico Distribuidor (DT)

Um DT é só uma **definição** (seção 3.3): nome, tipo, tags e a lista de **prefixos** (`prefixes`) que ele distribui. Não tem mensagens em disco nem em memória, não tem lista de prontos e não tem dono de entrega.

![Publicação em um Tópico Distribuidor](diagramas/img/distribuidor-publicacao.svg)

*Fonte: [distribuidor-publicacao.json](diagramas/fonte/distribuidor-publicacao.json).*

**Publicação** (`POST /topics/{td}/messages`):

1. O nó que recebe a chamada **casa os prefixos** do DT com a tabela de tópicos, que todo nó tem (seção 3.3). Casam **somente QTs** cujo nome começa com algum prefixo; um DT nunca é destino, o que evita laços, e a **fila de dead-letter** de uma fila (seção 12.10 de [13-entrega-de-mensagens.md](13-entrega-de-mensagens.md)) também não, mesmo que o nome comece com o prefixo.
2. Para cada fila que casou, o nó gera uma **cópia independente**, com **ID próprio** (GUID v7) e o mesmo conteúdo, e a publica com o procedimento normal de mensagem daquela fila: registro + commit por quórum, primário sempre incluso (seção 7.2). O ID decide o bloco, então cada cópia cai em blocos e nós diferentes.
3. As cópias são **independentes entre si**: a falha de uma (por exemplo, quórum não atingido) não desfaz as outras e não há rollback global.
4. O DT **responde** depois que todas as cópias terminaram, com o resultado por fila (ID da cópia ou erro).

**Regras:**

- Uma fila que casa com **dois ou mais prefixos do mesmo DT** recebe **uma cópia só**. Uma fila que casa com prefixos de **DTs diferentes** recebe uma cópia de cada publicação em cada DT.
- **Sem fila casando** não é erro: o DT fez o trabalho dele, que é receber. A resposta é sucesso, com lista vazia, e a mensagem é descartada. O nó registra o evento `distributor-no-target` ([11-observabilidade.md](11-observabilidade.md)).
- O casamento é feito **na hora da publicação**, sobre as filas que existem naquele instante. Uma fila criada depois só recebe o que for publicado depois; não há retroativo.
- **Falha parcial** não é erro do DT: a resposta é `200` e cada item da lista traz o `id` da cópia ou o `error` daquela fila. Quem quiser reenviar para uma fila específica publica direto nela.
- Não há ordem entre as filas destino.
- **Prefixos:** de 1 a 100 caracteres, mesmo conjunto do nome de tópico (`A-Za-z0-9_-`); **prefixo vazio é rejeitado** (casaria com tudo). Máximo de 20 prefixos por DT (limite fixo, [12-limites-api.md](12-limites-api.md)). São definidos na criação e podem ser alterados por `PATCH`; a alteração vale a partir do commit e só afeta publicações posteriores.

Exemplo de resposta de `POST /topics/pedidos-dist/messages` com dois destinos:

```json
{
  "matched": 2,
  "deliveries": [
    { "topic": "pedidos-eu", "id": "01927f3a-..." },
    { "topic": "pedidos-us", "error": "quorum-not-reached" }
  ]
}
```

> O custo do casamento é local: a tabela de tópicos é mantida ordenada por nome (ou em árvore de prefixos) em cada nó, e o casamento é uma busca por faixa. Um DT com muitas filas destino gera muitas cópias por publicação; o número de destinos por publicação tem um teto configurável (`topics.distributor.max-targets`), **sem limite por padrão**. Se o teto está definido e a publicação casa com mais filas, ela é **recusada inteira** (`422`, nenhuma cópia é feita), em vez de entregar só a parte que cabe, o que deixaria destinos escolhidos pela ordem do casamento, sem critério para quem recebe.

### 14.3 Fila volátil (QTV)

Uma QTV é um QT com dois metadados extras na definição do tópico:

| Campo | Descrição |
|---|---|
| `volatile` | `true` para QTV. Só QT pode ser volátil. |
| `idleTimeout` | Tempo **sem consumidor** depois do qual a fila é excluída. Em segundos. Padrão **600** (10 minutos), configurável na criação. |

Além disso, todo tópico passa a ter um **`id` (GUID v7)**, gerado pelo nó coordenador antes da fase de registro da criação e gravado na definição (`_topics/${topico}.json`). O nome continua sendo a chave da API; o `id` é interno (aparece nas leituras de tópico e na observabilidade). Recriar um tópico com o mesmo nome produz um `id` novo, e é por isso que a exclusão da seção 14.5 carrega o `id`.

**O que conta como consumidor:** **qualquer** `GET /topics/{topic}/messages`, com ou sem mensagem para entregar, com ou sem long polling. O `GET` na fila de dead-letter da QTV **não conta**. A dead-letter é excluída junto com a QTV. O nó que recebe o `GET` registra o instante **no começo da chamada**; num long polling de 20 s, a data já vale desde o primeiro instante, não só ao final. **Publicar não conta:** uma QTV só com produtores e sem consumidores é excluída e as mensagens se perdem. Isso é intencional.

**Quando o relógio começa:** na **criação** da fila. O prazo de exclusão é `max(createdAt, último GET em qualquer nó) + idleTimeout`.

**Alteração:** `PATCH` pode alterar `idleTimeout` (vale para a próxima verificação do gestor). `volatile` e o tipo são imutáveis.

### 14.4 Criação automática pelo `GET`

O consumidor pode criar a fila na própria chamada de consumo:

```
GET /topics/{topic}/messages?autoCreate=true&idleTimeout=300&tag.time=financeiro
```

![Criação automática de fila volátil](diagramas/img/fila-volatil-criacao.svg)

*Fonte: [fila-volatil-criacao.json](diagramas/fonte/fila-volatil-criacao.json).*

| Parâmetro | Descrição |
|---|---|
| `autoCreate` | `true` cria a fila se ela não existir. |
| `volatile` | Padrão **`true`** quando `autoCreate` cria a fila. `false` cria um QT persistente. |
| `idleTimeout` | Segundos. Padrão **600** (`topics.volatile.idle-timeout.default`). |
| `tag.<chave>` | Tags da fila (mesmas regras da seção 8.1). |

**Regras:**

- **Só cria QT.** Nunca cria DT.
- **Idempotente:** se o tópico já existe e é QT, os parâmetros de criação são **ignorados** e o `GET` segue como um consumo normal. Se o nome pertence a um DT, retorna `409`.
- **Criação concorrente:** vários consumidores subindo juntos disputam a criação. A criação usa o registro + commit de todos os nós (seção 3.3), que rejeita o nome duplicado; quem perde a disputa trata como "já existe" e prossegue.
- **Latência:** o primeiro `GET` paga o registro + commit em todos os nós. Isso ocorre uma vez por fila.
- **Teto de filas voláteis:** `topics.volatile.max-count` limita quantas filas voláteis existem no cluster, **sem limite por padrão**. Com o teto definido e atingido, a criação pelo `autoCreate` é recusada com `422`. A checagem é local, sobre a tabela de tópicos que todo nó tem; criações simultâneas podem ultrapassar o teto por pouco, o que é aceito. O `autoCreate` não tem rate limit próprio: vale a categoria de consumo ([12-limites-api.md](12-limites-api.md)).
- **Nome inválido** retorna `400`, como na criação normal.
- O `GET` que criou a fila **já conta como consumidor** (seção 14.3).
- Sem `autoCreate`, o `GET` de um tópico inexistente retorna `404`.

### 14.5 Nó gestor

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): o próprio nó é o gestor de todas as filas voláteis, e a verificação final consulta só ele.

O **nó gestor** de uma QTV cuida **só da vida do tópico**: nunca de suas mensagens, e nunca do caminho de entrega. É uma função **administrativa** do nó, não de trabalho. Isso não contraria a decisão de não haver nó controlador por tópico (seção 12.7): aquele descarte tratava de quem controla a **entrega**, que aqui continua sem dono de tópico.

**Como o gestor é escolhido, sem eleição:**

1. Calcula-se o **hash do `id` (GUID v7) do tópico** e localiza-se essa posição no anel (seção 7.1). O GUID v7 tem o timestamp nos bits altos, então o hash é indispensável para o resultado ser uniforme, como no ID das mensagens.
2. O **nó físico primário** dessa posição é o gestor do tópico.

Qualquer nó chega ao mesmo gestor a partir da tabela de tópicos e da tabela de membros, que todos têm. Como o `id` é hasheado, a distribuição das QTVs entre os nós é homogênea, independente do padrão dos nomes.

**A lista de tópicos gerenciados é derivada, não guardada.** Cada nó calcula localmente quais QTVs têm ele como gestor. Não existe uma lista para transferir entre nós: ela muda sozinha quando muda a tabela de membros ou a de tópicos (seção 14.6).

**Ciclo do gestor para cada QTV que gerencia:**

![Exclusão de uma fila volátil pelo gestor](diagramas/img/fila-volatil-exclusao.svg)

*Fonte: [fila-volatil-exclusao.json](diagramas/fonte/fila-volatil-exclusao.json).*

1. **Agenda** a verificação para `createdAt + idleTimeout` (uma estrutura de prazos ordenada por vencimento, uma verificação por vez, sem uma thread por tópico).
2. No vencimento, faz a **verificação final**: pergunta a **todos os nós** há quanto tempo foi o último `GET` do tópico (leitura local do monitoramento de cada nó, `GET /cluster/topics/{id}/idle`). Cada nó responde uma **duração decorrida**, não um timestamp, para não depender de relógios sincronizados.
3. **Se algum nó teve `GET` mais recente** que o `idleTimeout`, a fila não expirou: o gestor **reagenda** para o novo prazo (`idleTimeout` menos a menor duração recebida) e volta ao passo 2 nesse instante.
4. **Se todos os nós responderam e nenhum teve `GET` no intervalo**, o gestor **exclui** a fila com o mesmo protocolo de duas fases do `DELETE /topics/{topic}` (seção 3.3), com confirmação de **todos** os nós. A chamada de registro carrega o **`id`** do tópico, e cada nó só aceita se o tópico atual com aquele nome ainda tiver esse `id`. Assim, uma fila que foi excluída e recriada no meio do caminho nunca é apagada por engano. No commit, cada nó apaga as mensagens do tópico (em memória, em disco e metadados), sem checar se estão pendentes.

**Regras de segurança da verificação:**

- **Nó que não responde** (ainda não desregistrado): o gestor **não exclui**; tenta de novo depois de `topics.volatile.idle-check.retry-interval`. Na dúvida, a fila continua viva.
- **Nó sem registro de `GET`** para o tópico (acabou de subir, ou reiniciou sem memória): responde **o tempo desde que subiu**. Assim, uma reinicialização só pode **adiar** a exclusão, nunca antecipá-la.
- **Corrida residual:** um `GET` que chega depois da verificação final e antes do commit da exclusão recebe `404` (ou recria a fila, se usar `autoCreate`, com `id` novo e sem as mensagens antigas). É a mesma perda aceita para o `DELETE` comum.
- **Mensagens em voo:** um consumidor que ainda tem mensagem invisível recebe `404` ao tentar o ack depois da exclusão.
- **Exclusões e mudanças de membros** podem correr em paralelo; não há espera entre elas (diferente da seção 6.8).
- **Gestor fora do ar** e ainda não desregistrado: nenhuma QTV dele é excluída nesse intervalo. **A fila continua recebendo mensagens e consumidores normalmente.** Quando o nó é desregistrado, o novo gestor assume (seção 14.6).

### 14.6 Gestor, comissionamento, descomissionamento e rebalanceamento

> **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): não há entrada, saída nem rebalanceamento de nós, então o gestor nunca muda.

Como o gestor é o primário do hash do `id` no anel, a lista de tópicos gerenciados faz parte de cada mudança do anel.

![Troca de gestor em mudanças do anel](diagramas/img/fila-volatil-troca-gestor.svg)

*Fonte: [fila-volatil-troca-gestor.json](diagramas/fonte/fila-volatil-troca-gestor.json).*

| Evento | O que acontece com as QTVs gerenciadas |
|---|---|
| **Entrada de nó** (seção 6.2) | O nó novo recebe a **tabela de tópicos** por stream logo depois do bootstrap ([02-arquitetura.md](02-arquitetura.md), seção 3.3), já com `id`, `volatile`, `idleTimeout` e `createdAt`. Ao assumir a posição no anel, ele calcula quais QTVs passam a ser suas, mas só começa a gerenciá-las quando fica pronto (seção 7.3 de [05-particionamento-replicacao.md](05-particionamento-replicacao.md)); até lá, nenhuma QTV desse trecho do anel é excluída, o mesmo efeito de um gestor fora do ar. Cada nó existente recalcula o seu conjunto no commit do registro e **abandona** as QTVs cuja posição passou ao novo nó. |
| **Saída de nó** (seção 6.3) | No commit do desregistro, cada nó recalcula o conjunto: quem é o novo primário das posições do nó removido **assume** as QTVs dele. |
| **Falha no registro de nó** (seção 6.5) | O anel volta a ser calculado sem o nó, e o conjunto de cada nó volta ao anterior. |
| **Retorno de nó excluído** (seção 6.6) | O nó refaz o bootstrap do zero; vale a linha da entrada de nó. |
| **Tópico criado ou excluído** | Cada nó atualiza o seu conjunto ao aplicar o commit da definição (seção 3.3). |

**Ao assumir uma QTV, o novo gestor não parte de zero:** ele não tem o relógio do gestor anterior, e nem precisa. O estado de verdade está nos nós (o último `GET` de cada um). Por isso, ao assumir, ele faz **imediatamente a consulta de ociosidade** (passo 2 da seção 14.5) e calcula o prazo real. Assim, um anel instável, que troca de gestor com frequência, não adia a exclusão indefinidamente. Quando muitas QTVs mudam de gestor de uma vez, as consultas são espalhadas no tempo (jitter) para não criar um pico.

**Exclusão em andamento durante a troca de gestor:** se o gestor antigo já tinha disparado uma exclusão e o novo assume, as duas tentativas são inofensivas. O registro carrega o `id`, então a segunda chamada encontra o tópico já excluído (ou recriado com outro `id`) e é rejeitada sem efeito.

### 14.7 Observabilidade

Eventos e métricas novos (detalhes em [11-observabilidade.md](11-observabilidade.md)):

| Evento | Quando |
|---|---|
| `distributor-no-target` | Uma publicação em um DT não casou nenhuma fila. Não é erro, e não é falha de saúde. |
| `distributor-partial` | Ao menos uma cópia de uma publicação em DT falhou. Atributos: DT, fila, causa. |
| `volatile-deleted` | Uma QTV foi excluída por ociosidade. Atributos: nome, `id`, tempo de vida, mensagens descartadas (por nó, no commit). |
| `volatile-check-blocked` | O gestor não pôde concluir a verificação final porque algum nó não respondeu. |

### 14.8 Parâmetros

| Parâmetro | Chave sugerida | Padrão |
|---|---|---|
| Ociosidade padrão de fila volátil | `topics.volatile.idle-timeout.default` | **600 s** |
| Intervalo de nova tentativa da verificação | `topics.volatile.idle-check.retry-interval` | A definir (sugestão: 30 s) |
| Espalhamento (jitter) das consultas ao assumir tópicos | `topics.volatile.takeover.jitter` | A definir |

Limites fixos: `idleTimeout` de 60 s a 7 dias; até 20 prefixos por DT, ver [12-limites-api.md](12-limites-api.md).
