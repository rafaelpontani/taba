---
título: Início rápido
resumo: Subir o Taba (um nó ou o cluster de três nós), criar um tópico, publicar, consumir, confirmar e ver o cluster reagir à queda de um nó.
tipo: tutorial
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [docker-compose.yml, docker/haproxy.cfg, Dockerfile, crates/taba-node/src/cli/mod.rs, connectors/rust/cli]
---

# Início rápido

Em cerca de dez minutos você sobe o Taba, publica e consome uma mensagem e vê o que acontece quando um nó cai. Há dois caminhos:

| Caminho | Quando usar | O que sobe |
|---|---|---|
| **A. Um nó** (modo desenvolvimento) | Conhecer a API, desenvolver um cliente | 1 processo, sem réplicas |
| **B. Cluster de três nós** (Docker Compose) | Ver replicação, falha e recuperação | 3 nós + HAProxy de exemplo |

> Pré-requisitos: **A** pede o binário `taba` (veja [05-dependencias.md](05-dependencias.md) para compilar) ou Docker. **B** pede Docker com Compose.

## A. Um nó, em modo desenvolvimento

```bash
# A pasta de dados precisa terminar em "taba" (o nó recusa outro nome).
mkdir -p /tmp/taba-dev/taba
TABA_MODE=development TABA_STORAGE_DIR=/tmp/taba-dev/taba \
TABA_API_PORT=8080 TABA_OPERATIONS_PORT=9090 taba
```

O log avisa que o nó é "um cluster de um só" e não deve ser usado em produção. Em modo desenvolvimento **não há autenticação** (não há token raiz).

```bash
# 1. Criar um tópico do tipo fila
curl -s -X POST localhost:8080/topics -H 'Content-Type: application/json' \
  -d '{"name":"pedidos","type":"queue"}'

# 2. Publicar: a resposta traz o id da mensagem (HTTP 201)
curl -s -X POST localhost:8080/topics/pedidos/messages -H 'Content-Type: application/json' \
  -d '{"pedido":9001}'
# {"id":"01a13635-a913-71b2-9b55-85517752ddbc"}

# 3. Consumir, esperando até 1 s por uma mensagem (long polling)
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[{"id":"01a13635-a913-…","body":{"pedido":9001},"contentType":"application/json",
#   "receiptToken":"01a13635-aa19-…","deliveryCount":1,"publishedAt":"…","invisibleUntil":"…"}]}

# 4. Confirmar (ack): HTTP 204. Use o id e o receiptToken da resposta anterior.
curl -s -i -X DELETE "localhost:8080/topics/pedidos/messages/<id>?receiptToken=<receiptToken>"

# 5. Consumir de novo: a fila está vazia
curl -s "localhost:8080/topics/pedidos/messages?waitTime=1"
# {"messages":[]}
```

Se você **não** fizer o passo 4, a mensagem volta a ficar disponível quando passa o `invisibleUntil` (padrão: 120 s depois da entrega), com `deliveryCount` 2. Esse é o mecanismo central da entrega ([12-entrega.md](12-entrega.md)).

Perguntas ao nó (porta de **operação**, 9090):

```bash
curl -s localhost:9090/health/ready          # {"health":"ready"}
curl -s localhost:9090/health/cluster        # estado e condições; "worstCondition":"ok"
curl -s localhost:9090/metrics/topics/pedidos # métricas do tópico (publicadas, consumidas, fila)
```

Um erro tem sempre o mesmo formato ([19-erros-e-limites.md](19-erros-e-limites.md)):

```bash
curl -s localhost:8080/topics/nao-existe
# {"code":"topic.not-found","message":"O tópico não existe.","requestId":"…"}   (HTTP 404)
```

### O mesmo com o `taba-cli`

O `taba-cli` é a linha de comando do cliente ([20-cli.md](20-cli.md)). Aponte-o para o nó por variáveis:

```bash
export TABA_CLI_URL=http://localhost:8080 TABA_CLI_OPS_URL=http://localhost:9090
taba-cli topic create fila2 --type queue
echo '{"x":1}' | taba-cli publish fila2 --stdin
taba-cli consume fila2 --wait 1 --ack        # recebe e confirma
taba-cli health cluster -o table
```

## B. Cluster de três nós com Docker Compose

Na raiz do repositório:

```bash
docker compose up -d                # baixa as imagens públicas (rafaelpontani/taba) e sobe tudo
docker compose ps                   # os três nós ficam "healthy" ao terminar de sincronizar
```

Para construir a imagem a partir do repositório (quem desenvolve), some o override `docker-compose.dev.yml`:

```bash
docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d
```

A tag das imagens públicas vem de `TABA_VERSION` (padrão `latest`): `TABA_VERSION=1.2 docker compose up -d`.

O que o Compose faz, em ordem (`docker-compose.yml`):

1. O serviço **`init`** roda `taba init --out /secrets` uma vez e cria, num volume, o **certificado do cluster** (TLS mútuo entre nós) e o **token raiz**. Nas vezes seguintes não faz nada.
2. O **HAProxy** (de exemplo) escuta na porta `8080` e balanceia a API de cliente para os nós que estiverem prontos. Qualquer balanceador serve.
3. O **`taba-0`** sobe com `cluster.first-node=true`: o primeiro nó forma o cluster sozinho.
4. As **réplicas do serviço `taba`** (duas por padrão, `taba-1` e `taba-2`) esperam o `taba-0` ficar saudável, descobrem o cluster pelo HAProxy (`cluster.entry-address`), entram e sincronizam. Para mais nós: `docker compose up -d --scale taba=4`; o HAProxy descobre as réplicas pelo DNS. Cada réplica tem um volume de dados anônimo, apagado por `docker compose down -v`.

Use o `taba-cli` do Compose, já apontado para o cluster e com o token raiz:

```bash
docker compose run --rm taba-cli health cluster -o table            # três nós, sem condições
docker compose run --rm taba-cli topic create pedidos --type queue
echo '{"pedido": 9001}' | docker compose run --rm -T taba-cli publish pedidos --stdin
docker compose run --rm taba-cli consume pedidos --wait 5 --ack
```

A API de cliente também fica em `http://localhost:8080` para qualquer cliente HTTP, agora **com autenticação**: envie `Authorization: Bearer <token raiz>`. O token está no volume `secrets`:

```bash
docker compose run --rm --entrypoint cat taba-cli /secrets/root-token
```

### Derrubar um nó e ver o cluster reagir

Há **duas** formas de tirar um nó, e elas são diferentes. As réplicas são do mesmo serviço, então o `docker compose stop` não as distingue; o `docker/cluster.sh` aceita os nomes `taba-0` (o primeiro nó), `taba-1` e `taba-2` (o `taba-N` é a réplica N do serviço `taba`):

```bash
# 1. Parada normal: SIGTERM -> o nó faz DRAIN e sai do cluster por conta própria (sem votação)
docker/cluster.sh stop taba-2
docker compose run --rm taba-cli health cluster -o table   # já mostra 2 nós ativos, membersVersion subiu
docker/cluster.sh start taba-2                               # volta como nó novo e sincroniza

# 2. Falha: SIGKILL -> o cluster precisa DETECTAR a queda e votar
docker/cluster.sh kill taba-2
docker compose run --rm taba-cli health cluster              # em ~4 s: write-quorum-at-risk, member-unreachable, under-replicated
docker compose run --rm taba-cli health cluster              # em ~8 s: taba-2 excluído; sobra a condição node-frozen
docker/cluster.sh start taba-2                               # volta como nó novo (ID novo: sem freezing) e sincroniza
```

O que acontece por baixo ([07-cluster.md](07-cluster.md)): na **falha**, os outros nós notam que o canal permanente com o `taba-3` caiu, tentam reconectar, abrem uma **votação** de exclusão e o retiram da tabela de membros; as mensagens dele já existiam em outros nós (fator de replicação 3), então nada que foi confirmado se perde. Na **parada normal**, o nó grava tudo em disco e se desregistra sozinho. Nos dois casos, ao voltar, o nó entra **como nó novo**, sincroniza os blocos e passa a atender.

> ⚠️ **Não derrube um segundo nó antes de o primeiro voltar.** Com 3 nós, a perda de dois em sequência rápida deixa um único sobrevivente (menos que `cluster.vote.min-voters`, 2): o cluster fica em `quorum-lost` e os nós reiniciados não conseguem reentrar. Veja [23-operacao.md](23-operacao.md).

![O que o docker compose up cria](../diagrams/img/pt-BR/compose-deployment.svg)

### Limpeza

```bash
docker compose down -v      # apaga contêineres e volumes (inclusive o token raiz)
```

## Próximos passos

- Entender o vocabulário: [03-conceitos.md](03-conceitos.md).
- Escolher o tipo de tópico: [09-topicos.md](09-topicos.md).
- Todas as rotas: [16-api-cliente.md](16-api-cliente.md).
- Rodar em produção: [23-operacao.md](23-operacao.md).

## Fontes e provas

- `docker-compose.yml`, `docker/haproxy.cfg`, `Dockerfile`, `crates/taba-node/src/cli/mod.rs` (`taba init`).
- Os exemplos do caminho A foram executados contra um nó real em modo desenvolvimento (respostas copiadas da execução).
- `docker/validate.sh` automatiza o roteiro do caminho B de ponta a ponta.
- Casos de uso: UC-DEV (modo desenvolvimento), UC-ING (entrada de nós), UC-CLI (`taba-cli`); veja [26a](26a-casos-de-uso-implementados.md).
