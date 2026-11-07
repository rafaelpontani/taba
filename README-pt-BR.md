<p align="center">
  <img src="doc/diagrams/img/pt-BR/hero.gif" width="100%" alt="Taba: mensageria open source eficiente (filas, FIFO e pub/sub) que escala e se gerencia sozinha, numa só imagem Docker. Animação: 3 nós, cada um desenhado como um prédio de 3 andares. Vários produtores chegam e saem e publicam mensagens; cada mensagem tem um nó primário, que a replica para os outros dois, e todo nó é primário de algumas delas. Os consumidores chegam depois, leem de forma assíncrona e confirmam com um ack.">
</p>

🇺🇸 [English](README.md) | 🇧🇷 **Português (Brasil)**

[![Release](https://github.com/rafaelpontani/taba/actions/workflows/release.yml/badge.svg?branch=main)](https://github.com/rafaelpontani/taba/actions/workflows/release.yml) [![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/rafaelpontani/taba/badge)](https://scorecard.dev/viewer/?uri=github.com/rafaelpontani/taba) [![SLSA provenance](https://img.shields.io/badge/SLSA-provenance-blue)](#verificando-um-release)

O **Taba** é uma **suíte de mensageria feita para ser eficiente**: filas (como o SQS), filas FIFO e pub/sub (como o SNS) na mesma API HTTP, a partir de **uma única imagem Docker** e **sem dependências externas** (sem coordenador, sem banco de dados, sem broker à parte). Ele **se gerencia sozinho**: adicione um nó e o cluster se rebalanceia; retire um e ele entrega o seu trabalho antes de sair, sem downtime; um nó que falha é excluído por votação e volta sozinho. Toda mensagem é replicada entre os nós e mantida em memória, com **backup contínuo no S3**. Vêm junto um **Helm chart**, uma **interface de terminal** e pacotes publicados.

O objetivo é fazer o trabalho de um broker de mensagens com **poucos recursos**: pouca CPU e memória por mensagem, poucas máquinas e pouco tempo gasto para operar. Se você quer fugir da cobrança por mensagem, de um cluster de brokers para operar ou da dependência de uma nuvem, o Taba é para você.

## Mensageria, não streaming

O Taba é da classe dos **brokers de mensagens** (RabbitMQ, NATS JetStream, SQS), não da classe das **plataformas de streaming** (Kafka, Redpanda, Pulsar). Elas resolvem problemas diferentes, por isso este README compara o Taba só com sistemas da classe dele.

**Por que não dá para comparar as duas.** Uma plataforma de streaming é um log de acréscimo dividido em partições. O consumidor não pega uma mensagem: lê o log a partir de uma posição (um offset) e avança essa posição. É por isso que ela é tão rápida para gravar bytes e relê-los: o broker só acrescenta e lê em sequência, e quase todo o trabalho em volta de cada mensagem fica para a aplicação:

- **Confirmar uma mensagem e não a seguinte.** O offset é um número por partição: confirmar a mensagem 10 confirma da 1 à 9. Uma mensagem que falhou no meio precisa ser tratada pela aplicação, ou trava a partição.
- **Reentregar só a que falhou e mandá-la para uma fila de mensagens mortas** depois de algumas tentativas: a aplicação escreve essa lógica (tópicos de nova tentativa, um tópico de mensagens mortas).
- **Dividir a carga:** o número de consumidores trabalhando em paralelo é limitado pelo número de partições; consumidores além disso ficam parados.
- **Expirar uma mensagem só, ou escondê-la por um tempo** (visibilidade): o log só expira segmentos inteiros, por idade ou tamanho.

Um broker faz esse trabalho ele mesmo, para cada mensagem: registra a entrega, espera o ack, reentrega no prazo, conta as tentativas e move a mensagem para a fila de mensagens mortas. Medir os dois na mesma carga compara um log sem nada a fazer com um broker fazendo o trabalho que o log deixa para a aplicação. Os números de vazão saem de trabalhos diferentes, então nenhum deles diz qual sistema é mais rápido para uma fila.

Por isso o Taba é medido [contra brokers da mesma classe](doc/pt-BR/38-benchmarks.md#o-que-a-comparação-mede). Se o que você precisa é reler o histórico ou processar streams, uma plataforma de streaming é a ferramenta certa; se precisa de filas, FIFO e pub/sub com cada mensagem tratada por si, o Taba é.

## Em resumo

| | |
|---|---|
| **Tópicos** | Filas (uma mensagem para um consumidor, reentrega quando não confirmada, dead-letter), filas **FIFO** (ordem por grupo e deduplicação) e distribuidores (pub/sub por prefixo de nome) |
| **Cluster** | 3 ou mais nós idênticos, **sem coordenador e sem ponto único de falha**. Um nó que falha é excluído por votação; quando se recupera, volta sozinho |
| **Escala** | **Horizontal**, nos dois sentidos: um nó novo assume a sua parte dos tópicos; um nó que sai faz o drain antes. Autoscaling com KEDA no Kubernetes |
| **Durabilidade** | Cada mensagem é confirmada por um **quorum** de nós (3 cópias por padrão) e copiada **continuamente para o S3** (AWS, SeaweedFS, MinIO, Ceph...) |
| **Implantação** | Uma imagem Docker (58 MB), Docker Compose ou o **Helm chart** em `oci://ghcr.io/rafaelpontani/charts/taba` |
| **Operação** | `taba-cli`, uma linha de comando com **interface de terminal em tela cheia** (saúde do cluster, tópicos, gráficos das filas), além de métricas Prometheus e OpenTelemetry |
| **Clientes** | HTTP simples, de qualquer linguagem; conectores prontos para **Java, C#/.NET, Python, PHP, Ruby, Node.js, Go e Rust** (Rust no crates.io, C# no nuget.org), todos sem dependências, além de um driver para o [OpenMessaging Benchmark](connectors/omb/README.md). Aplicações que usam o **AWS SDK (SQS e SNS)** se conectam sem mudar o código ([gateway](#use-o-aws-sdk-sem-mudar-o-seu-código)) |
| **Licença** | Servidor AGPLv3, conectores Apache-2.0 |

## Por que o Taba

**Rápido por padrão.** As mensagens são gravadas e servidas da **memória**, replicadas entre os nós; o disco só entra quando a memória enche. O consumidor lê o que já está na RAM, sem tuning e sem configuração extra. A memória não é dividida igualmente: os tópicos mais lidos mantêm mais mensagens quentes.

**Custo de operação baixo.**
- **Nada cobrado por mensagem:** você paga as máquinas, e só.
- **Pouco para rodar:** sem cluster de brokers, sem coordenador (ZooKeeper, etcd), sem banco de dados externo. Cada nó usa cerca de **35 MiB de RAM** atendendo a carga de demonstração mostrada abaixo.
- **Ninguém rebalanceia à mão:** aumente ou diminua o cluster e ele move os blocos dos tópicos entre os nós sozinho, sem parar. Na redução, o nó que sai faz o drain antes, um nó de cada vez.
- **Backup quase sem custo de S3:** uma mensagem consumida antes de a janela do backup fechar (60 s por padrão) **nunca vai para o S3**, então o tráfego rápido quase não gera requisições ao bucket. O que fica é enviado em segmentos.

**Confiável.**
- Uma escrita só é confirmada depois que um **quorum** de nós a guarda em memória.
- Os nós **se monitoram** e excluem por votação o que falha; ele **volta sozinho** quando se recupera.
- O **backup no S3 nunca atrasa uma escrita** e permite que um cluster inteiro que caiu **se restaure sozinho** ao subir (só a última janela do backup fica em risco).
- Todo candidato é validado de ponta a ponta antes de virar release final: um cluster de três nós com nós caindo, fazendo drain e voltando, **sem perda de mensagens confirmadas**, mais backup e restauração no S3 ([docker/validate.sh](docker/validate.sh)).

## Desempenho

Medido com o [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark) contra o RabbitMQ (fila de quórum), um broker da mesma classe, numa máquina só, com os mesmos limites de CPU e memória para os dois e toda escrita confirmada depois do `fsync`: um nó com 4 núcleos e 3 GB, e um cluster de três nós com 2 núcleos e 1,5 GB cada. Mensagens de 1 KB, cada uma consumida e confirmada.

![Vazão máxima, mensagens de 1 KB: Taba 29 mil mensagens por segundo em um nó e 10 mil em três nós; RabbitMQ 15 mil e 5,6 mil](doc/diagrams/img/pt-BR/bench-throughput.svg)

![Latência ponta a ponta a 10.000 mensagens por segundo: Taba p50 2 ms e p99 17 ms em um nó, p50 232 ms e p99 1,5 s em três nós; RabbitMQ p50 517 ms e p99 1 s em um nó, p50 1,2 s e p99 2 s em três nós](doc/diagrams/img/pt-BR/bench-latency.svg)

- **Vazão:** o Taba consome **2 vezes** mais mensagens por segundo que o RabbitMQ em um nó (29.200 contra 14.800) e **1,8 vez** em três nós (10.000 contra 5.600).
- **Latência:** a 10.000 mensagens por segundo fixas, uma mensagem leva **2 ms** (p50) da publicação à entrega em um nó, contra 517 ms; em três nós, 232 ms contra 1,2 s, e ali o RabbitMQ não acompanha a taxa (5.900 por segundo).
- **Memória parada:** 21 MB em um nó e 87 MB nos três nós, contra 227 MB e 534 MB.

O NATS JetStream, o outro broker da mesma classe, também está na rodada, com a mesma garantia (`sync_interval: always`, um fsync por mensagem): nesta máquina ele falha acima de cerca de 380 mensagens por segundo, porque cada fsync trava o servidor e o cliente desiste depois de 2 s, então os gráficos o deixam fora das barras e dizem isso. São números de uma rodada local (2026-11-03, um notebook; três rodadas do Taba em três nós na taxa máxima, com a mediana), úteis para comparar os sistemas entre si, não como números absolutos. Todos os gráficos, o método e como rodar você mesmo estão no [guia do benchmark](doc/pt-BR/38-benchmarks.md#resultados).

## Veja funcionando

A interface de terminal do `taba-cli` (`taba-cli tui`) no cluster do Compose, sob uma carga de demonstração ([doc/diagrams/scripts/capture-tui.sh](doc/diagrams/scripts/capture-tui.sh)):

| Saúde do cluster | Tópicos, com os gráficos da fila selecionada |
|---|---|
| ![Aba Cluster: condições, os nós e os gráficos do nó selecionado](doc/diagrams/img/pt-BR/tui-cluster.png) | ![Aba Tópicos: filas, um distribuidor e uma fila FIFO, com profundidade e taxas de publicação e consumo](doc/diagrams/img/pt-BR/tui-topics.png) |
| **Fluxo de publicação e consumo de uma fila** | **Um nó em detalhe, com o seu backup no S3** |
| ![Gráfico de fluxo da fila orders: mensagens que entram, saem e são confirmadas por segundo](doc/diagrams/img/pt-BR/tui-queue-chart.png) | ![Detalhe do nó: armazenamento, sinais de quorum e canais e o estado do backup no S3](doc/diagrams/img/pt-BR/tui-node.png) |

Tudo o que a interface faz também pode ser feito com comandos de uma linha ([doc/pt-BR/20b-cli-ui.md](doc/pt-BR/20b-cli-ui.md)).

## Experimente em um minuto

Com Docker, um cluster de três nós (atrás de um HAProxy de exemplo) sobe com um comando. Um contêiner de inicialização cria o certificado do cluster e o token raiz, o primeiro nó forma o cluster e os outros dois entram:

```bash
docker compose up -d              # três nós "healthy" em cerca de um minuto (imagens públicas do Docker Hub)
```

Para construir as imagens a partir deste repositório (desenvolvimento): `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d`.

Depois, com o [`taba-cli`](doc/pt-BR/20-cli.md), que já vem no Compose apontado para o cluster (e com o token raiz):

```bash
docker compose run --rm taba-cli cluster status -o table             # os três nós, sem condições
docker compose run --rm taba-cli topic create orders --type queue
echo '{"order": 9001}' | docker compose run --rm -T taba-cli publish orders --stdin
docker compose run --rm taba-cli consume orders --wait 5 --ack      # recebe e confirma
```


A API de cliente também está em `http://localhost:8080` para qualquer cliente HTTP (com `Authorization: Bearer <token raiz>`; o token está no volume `secrets`: `docker compose run --rm --entrypoint cat taba-cli /secrets/root-token`; no Git Bash do Windows, prefixe o comando com `MSYS_NO_PATHCONV=1`). O Compose já sobe o backup no S3, num SeaweedFS ao lado do cluster. `docker compose down -v` apaga tudo. O HAProxy é só um exemplo: qualquer balanceador serve. Para um único nó, sem cluster, veja [Desenvolva em um nó](#desenvolva-em-um-nó).

**Quer um cluster já populado para testar?** O `docker/seed-demo.sh` apaga tudo, sobe o cluster, cria um **token `admin`** local (`local-admin`, o jeito certo de usar o Taba: o token raiz só serve para criá-lo e não é usado fora dos contêineres) e o grava em `docker/.local/admin-token` (ignorado pelo git), aponta o perfil `local_admin` do `taba-cli` instalado na sua máquina para `http://localhost:8080` e esse token (atualiza o perfil se existir, senão o cria como padrão; `--no-profile` pula essa parte) e enche o cluster de filas (equilibradas, barulhenta e FIFO) e distribuidores, com 100.000 mensagens cada. Depois, `taba-cli topic list` já funciona. Detalhes em [Dados de demonstração](doc/pt-BR/25-testes.md#dados-de-demonstração-dockerseed-demosh).

> **Cuidado em produção:** pare e atualize os nós **um de cada vez** (`docker compose stop` envia `SIGTERM`, que dispara um *drain* ordenado). Perder **dois nós seguidos** trava um cluster de 3 nós. Parar **todos** os nós de uma vez perde as mensagens e os tópicos, a não ser que o backup no S3 esteja ligado: aí o cluster se restaura sozinho ao subir, menos a última janela do backup. Veja o [guia de operação](doc/pt-BR/23-operacao.md).

## Taba × as alternativas

Sistemas da mesma classe, os brokers de mensagens ([por que as plataformas de streaming não estão aqui](#mensageria-não-streaming)):

| | **Taba** | SQS + SNS | RabbitMQ | NATS JetStream |
|---|---|---|---|---|
| **Onde roda** | Em qualquer lugar: máquina, VM, Docker, Kubernetes | Só na AWS | Onde você hospedar, ou como serviço gerenciado | Onde você hospedar, ou como serviço gerenciado |
| **O que operar** | A mesma imagem em 3 nós (1 para desenvolvimento) | Nada (gerenciado) | Um broker (mais nós para alta disponibilidade) | Um servidor (3 ou mais para streams replicados) |
| **Fila e pub/sub** | Na mesma API HTTP | Dois serviços separados | Filas e exchanges | Subjects, streams e consumers |
| **Trabalho por mensagem** | Ack, reentrega, dead-letter e expiração por mensagem, no broker | Por mensagem, no serviço | Ack, reentrega e dead-letter por mensagem, no broker | Ack e reentrega por mensagem; sem fila de mensagens mortas (um aviso quando as tentativas acabam) |
| **Alta disponibilidade** | Embutida: replicação por quorum, votação e volta automática | Gerenciada pela AWS | Quorum queues num cluster | Streams replicados por Raft |
| **Escala** | Adicione ou retire um nó: ele se rebalanceia sozinho | Automática (gerenciada) | Nós do cluster; líderes das filas rebalanceados por comando | Servidores no cluster; streams posicionados na criação e movidos por comando |
| **Backup** | Contínuo no S3, restaurado ao subir | Gerenciado pela AWS | Exportação de definições; mensagens com ferramentas externas | Snapshots dos streams por comando |
| **Desenvolvimento local** | A mesma imagem da produção | Um emulador (LocalStack) ou uma conta AWS | O mesmo software | O mesmo software |
| **Cobrança** | Sem cobrança por mensagem: só infraestrutura | Por requisição | Infraestrutura, ou um plano gerenciado | Infraestrutura, ou um plano gerenciado |

## Perguntas frequentes

**O Taba é um servidor único?** Não. É um cluster de nós idênticos, sem coordenador. Três nós são o mínimo recomendado para produção; um nó é o perfil de um nó, para desenvolvimento.

**Ele escala horizontalmente?** Sim. Os tópicos são divididos em blocos num anel de hash consistente; um nó novo assume a sua parte e um nó que sai entrega a sua parte antes. No Kubernetes, o chart pode escalar com KEDA.

**O que acontece se um nó cair?** Os outros detectam, o excluem por votação e continuam atendendo pelas réplicas. Quando ele volta, é readmitido e sincronizado. Com 3 nós, o cluster sobrevive a um nó de cada vez.

**Ele suporta S3?** Sim, como backup contínuo de mensagens, tópicos e tokens em qualquer S3 que respeite escritas condicionais (AWS S3, SeaweedFS, MinIO, Ceph). O Compose já sobe com ele ligado, num SeaweedFS. Detalhes em [doc/pt-BR/33-backup-s3.md](doc/pt-BR/33-backup-s3.md).

**Tem Helm chart?** Sim: `oci://ghcr.io/rafaelpontani/charts/taba`, com backup no S3, Ingress, Prometheus, OpenTelemetry e autoscaling com KEDA como opções ([Rode no Kubernetes](#rode-no-kubernetes-helm)).

**Tem interface de usuário?** Sim, no terminal: `taba-cli tui` ([Veja funcionando](#veja-funcionando)). Não há painel web; para gráficos de longo prazo, use as métricas do Prometheus.

**Minha aplicação que usa o Amazon SQS ou SNS pode usar o Taba?** Pode, com o gateway do nó: você troca o endpoint do SDK e mantém o código ([como, com exemplos em seis linguagens](#use-o-aws-sdk-sem-mudar-o-seu-código)). Cobre o SQS (padrão e FIFO) e tópicos do SNS que publicam nessas filas; o que não é coberto está listado ali.

**Está em produção?** Sim. Toda versão é publicada no Docker Hub, GHCR, crates.io, nuget.org e GitHub Releases, assinada e com proveniência ([Verificando um release](#verificando-um-release)).

## Limites

O que limita um cluster Taba é o hardware que você dá a ele, não um número fixo:

- **Quantos nós:** o código não tem máximo. Cada nó assume uma parte do anel de hash (128 posições virtuais por nó). Os testes automatizados rodam clusters de até 5 nós, e todo candidato a release é validado de ponta a ponta no Compose.
- **Quanto dado:** cada mensagem fica em 3 nós (`replication.factor`), então o cluster guarda cerca da soma da memória e do disco dos nós dividida por 3. A memória pode ter um teto por nó (`memory.limit`); acima dele, as mensagens transbordam para o disco local.
- **Tamanho da mensagem:** 5 MB por padrão, configurável por tópico.
- **Mínimo para produção:** 3 nós. Perder **dois nós seguidos** num cluster de 3 nós trava o cluster (não há quorum), e os nós devem ser parados e atualizados **um de cada vez**.

O que o Taba não faz:

- **Não substitui um protocolo, exceto o da AWS:** o Taba fala HTTP, e um gateway opcional fala os protocolos do **SQS e do SNS** ([acima](#use-o-aws-sdk-sem-mudar-o-seu-código), com os seus limites). Ainda não há compatibilidade com AMQP ou MQTT, então vir do RabbitMQ significa trocar o cliente.
- **Não é uma plataforma de streaming:** processamento de streams e reprodução do histórico de mensagens estão fora do escopo ([Mensageria, não streaming](#mensageria-não-streaming)). Para filas e pub/sub, sim; para reler o passado, não.
- **Fora do escopo por enquanto:** interface web, autenticação avançada (OAuth/OIDC) e multirregião.
- **Ainda por vir:** os pacotes dos conectores Java, Node.js, Python, Go, PHP e Ruby (hoje os arquivos do Node.js, Python, Ruby, Java e PHP vão anexados a cada [Release](https://github.com/rafaelpontani/taba/releases) e o Go é lido do repositório: [como instalar](doc/pt-BR/21-conectores.md#usar-os-conectores-antes-de-publicados)). Veja o [roadmap](doc/pt-BR/32-roadmap.md) e os [problemas conhecidos](doc/pt-BR/36-problemas-conhecidos.md).

## Desenvolva em um nó

Para desenvolver, um contêiner basta (um cluster de um nó, dados dentro do contêiner, **sem autenticação e sem token raiz**, então nunca deve ficar exposto numa rede):

```bash
docker run -d --name taba-dev -p 8080:8080 -e TABA_REPLICATION_FACTOR=1 rafaelpontani/taba

# o taba-cli (Linux, macOS, Git Bash; no PowerShell: scripts/install-cli.ps1). Por padrão usa http://127.0.0.1:8080 sem token
curl -fsSL https://raw.githubusercontent.com/rafaelpontani/taba/main/scripts/install-cli.sh | sh

taba-cli topic create orders --type queue
echo '{"order": 9001}' | taba-cli publish orders --stdin
taba-cli consume orders --wait 5 --ack
docker rm -f taba-dev                  # apaga o nó e os dados
```

O [`scripts/install-cli.sh`](scripts/install-cli.sh) baixa o binário do último Release final, confere o SHA256 contra o `SHA256SUMS` e o coloca em `~/.local/bin` (no macOS, no Git Bash/MINGW64 e no Windows os Releases não têm binário, então ele roda `cargo install taba-cli` ou, sem cargo, instala um `taba-cli` que executa a imagem Docker; o [`scripts/install-cli.ps1`](scripts/install-cli.ps1) faz o mesmo no PowerShell; `TABA_CLI_VERSION=0.1.1` escolhe a versão, `TABA_CLI_DIR=/usr/local/bin` a pasta). Sem instalar nada, a imagem também serve: `docker run --rm -e TABA_CLI_URL=http://host.docker.internal:8080 rafaelpontani/taba-cli topic list`. Tokens (raiz e `admin`) só existem com autenticação, ou seja, no cluster: o Compose acima ou o [Helm](#rode-no-kubernetes-helm) abaixo.

## Rode no Kubernetes (Helm)

O chart Helm está em [deploy/helm/taba/](deploy/helm/taba/). Precisa de Kubernetes 1.27+ e de uma `StorageClass` padrão.

```bash
# um cluster de 3 nós (o certificado do cluster e o token raiz são gerados uma vez, num Secret)
helm install taba oci://ghcr.io/rafaelpontani/charts/taba -n taba --create-namespace
# ou, de um clone deste repositório: helm install taba deploy/helm/taba -n taba --create-namespace
helm test taba -n taba
kubectl -n taba get secret taba-secrets -o jsonpath='{.data.root-token}' | base64 -d     # o token raiz
kubectl -n taba port-forward svc/taba 8080:8080                                          # a API em http://localhost:8080
```

Configurações comuns (todas opcionais, combine como precisar):

```bash
# um único nó para desenvolvimento (replication.factor=1: sem autenticação, sem TLS de cluster)
helm install taba deploy/helm/taba -n taba --create-namespace --set config.replication.factor=1

# expor a API por um Ingress (use qualquer controlador de Ingress mantido)
helm upgrade --install taba deploy/helm/taba -n taba \
  --set ingress.enabled=true --set ingress.className=nginx --set 'ingress.hosts[0].host=taba.example.com' \
  --set 'ingress.hosts[0].paths[0].path=/' --set 'ingress.hosts[0].paths[0].pathType=Prefix'

# backup em S3: um SeaweedFS embutido (para testes) ou o seu bucket (AWS, Ceph, ...)
helm upgrade --install taba deploy/helm/taba -n taba --set seaweedfs.enabled=true
helm upgrade --install taba deploy/helm/taba -n taba --set backup.s3.bucket=meu-bucket --set backup.s3.region=us-east-1

# métricas: um ServiceMonitor, um Prometheus ou um Telegraf próprios, ou push OTLP
helm upgrade --install taba deploy/helm/taba -n taba --set collector.type=prometheus
helm upgrade --install taba deploy/helm/taba -n taba --set otel.endpoint=http://otel-collector:4317

# autoscaling com KEDA (precisa do KEDA e de um Prometheus; nunca abaixo de 3 nós, reduz um nó de cada vez)
helm upgrade --install taba deploy/helm/taba -n taba \
  --set autoscaling.enabled=true --set autoscaling.prometheus.serverAddress=http://prometheus:9090
```

Para obter o token raiz e criar um token `admin` com o `taba-cli` (sem instalar nada: ele roda como um pod descartável):

```bash
ROOT=$(kubectl -n taba get secret taba-secrets -o jsonpath='{.data.root-token}' | base64 -d | tr -d '\n')

kubectl -n taba run taba-cli --rm -i --restart=Never --image=rafaelpontani/taba-cli:latest \
  --env TABA_CLI_URL=http://taba:8080 --env TABA_CLI_TOKEN="$ROOT" \
  -- token create --name ci-admin --permission admin --token-only
# imprime tb_<id>.<segredo>: o segredo aparece uma única vez, guarde-o (o cluster guarda só o hash)
```

Só o token raiz cria, altera ou revoga tokens `admin` e para o cluster; guarde-o num cofre e use o `admin` (ou um mais restrito, como `--permission publish,consume --prefix pedidos-`) no dia a dia. Mais em [doc/pt-BR/20-cli.md](doc/pt-BR/20-cli.md) e [doc/pt-BR/14-seguranca.md](doc/pt-BR/14-seguranca.md).

Qualquer parâmetro do nó vai em `config`, num arquivo de values (por exemplo `config.message.max-bytes`). Todos os values, o que o chart cria, as validações que ele faz e as regras de segurança do autoscaling estão em [doc/pt-BR/34-chart-helm.md](doc/pt-BR/34-chart-helm.md) (em inglês: [doc/en-US/34-helm-chart.md](doc/en-US/34-helm-chart.md)); os modos de operação, cada um com seus testes em bash, estão em [doc/pt-BR/35-chart-helm-casos-de-uso.md](doc/pt-BR/35-chart-helm-casos-de-uso.md).

## A API em quatro chamadas

Tudo é HTTP. O contrato formal (OpenAPI) ainda não está definido; as chamadas abaixo estão descritas na [referência da API de cliente](doc/pt-BR/16-api-cliente.md).

| O que você quer | Chamada |
|---|---|
| Criar um tópico (Fila ou Distribuidor) | `POST /topics` |
| Publicar uma mensagem | `POST /topics/{topic}/messages` |
| Receber mensagens (long polling de até 20 s) | `GET /topics/{topic}/messages` |
| Confirmar o consumo (ack) | `DELETE /topics/{topic}/messages/{id}` |

Se o consumidor não confirmar a tempo, a mensagem volta a ficar disponível e é entregue de novo, como no SQS (a invisibilidade padrão é 120 s e pode ser ajustada).

## Conectores

A API é HTTP, então qualquer linguagem já fala com o Taba pelo cliente HTTP que tem. Além disso, o projeto oferece **conectores fáceis de usar** para as principais linguagens, para publicar e consumir mensagens em poucas linhas. Eles não têm dependências externas e rodam em versões antigas, de até 15 anos, desde que a plataforma fale **TLS 1.2 ou superior** (veja a [versão base](doc/pt-BR/21-conectores.md) de cada um):

| Linguagem | Pasta | Versão base | Estado | Exemplo de uso |
|---|---|---|---|---|
| Java | `connectors/java/` | Java 11 | [pronto](connectors/java/README.md) (nove operações bloqueantes e com `CompletableFuture`, `subscribe`) | [README](connectors/java/README.md) |
| C# | `connectors/csharp/` | .NET Framework 4.6.1 e .NET 5, 6, 8 e 10 | [pronto](connectors/csharp/README.md) (nove operações assíncronas e síncronas, `subscribe`) | [README](connectors/csharp/README.md) |
| Python | `connectors/python/` | 3.9 | [pronto](connectors/python/README.md) (clientes síncrono e `asyncio`, `subscribe`) | [README](connectors/python/README.md) |
| PHP | `connectors/php/` | 5.6 | [pronto](connectors/php/README.md) (nove operações, `subscribe`, consumo com handler; uma thread só, então sem renovação automática da invisibilidade) | [README](connectors/php/README.md) |
| Ruby | `connectors/ruby/` | 2.1 | [pronto](connectors/ruby/README.md) (nove operações, blocos, `subscribe`, `CancelToken`) | [README](connectors/ruby/README.md) |
| Node.js | `connectors/node/` | 18 | [pronto](connectors/node/README.md) (nove operações com `Promise`, `subscribe`, tipos incluídos) | [README](connectors/node/README.md) |
| Go | `connectors/go/` | 1.7 | [pronto](connectors/go/README.md) (nove operações com `context.Context`, `Subscribe`) | [README](connectors/go/README.md) |
| Rust | `connectors/rust/client/` | 1.70 | síncrono pronto ([README](connectors/rust/client/README.md)) | [exemplo](connectors/rust/client/README.md) |

Os oito conectores prontos (Java, C#, Python, PHP, Ruby, Node.js, Go e Rust) têm as mesmas operações, os mesmos padrões e os mesmos erros: os [testes de contrato](connectors/contract/README.md) passam um mesmo cenário por cada um deles contra um nó de verdade, cada um no runtime mais antigo que suporta, e depois fazem um ler as mensagens dos outros (a única diferença é o PHP: roda numa thread só, então não renova a invisibilidade enquanto o handler roda). Publicar e consumir com ack automático (a mensagem só é apagada se o handler não falhar):

```java
// Java
try (TabaClient client = new TabaClient(TabaConfig.of("http://localhost:8080").token(token))) {
    client.publishText("orders", "hello");
    client.consume("orders", new ConsumeOptions().waitTime(20), ctx -> System.out.println(ctx.getMessage().getBodyText()));
}
```

```csharp
// C#
using (var client = new TabaClient(new TabaConfig("http://localhost:8080") { Token = token }))
{
    await client.PublishJsonAsync("orders", new { order = 9001 });
    await client.ConsumeAsync<Order>("orders", async ctx => await ProcessAsync(ctx.Body));
}
```

```python
# Python
with Client("http://localhost:8080", token=token) as client:
    client.publish_json("orders", {"order": 9001})
    client.consume_with("orders", lambda ctx: print(ctx.message.body_text), wait_time=20)
```

```js
// Node.js
const client = new TabaClient({ baseUrl: 'http://localhost:8080', token });
await client.publishJson('orders', { order: 9001 });
await client.consumeWith('orders', { waitTime: 20 }, async ({ message }) => console.log(message.bodyText));
```

```go
// Go
client, _ := taba.NewClient(taba.Config{BaseURL: "http://localhost:8080", Token: token})
client.PublishJSON(ctx, "orders", map[string]int{"order": 9001})
client.ConsumeWith(ctx, "orders", &taba.ConsumeOptions{WaitTime: taba.Int(20)}, func(c *taba.MessageContext) error {
    fmt.Println(c.Message.BodyText())
    return nil // a mensagem é apagada; devolver um erro a mantém
})
```

```php
// PHP
$client = new TabaClient(new TabaConfig('http://localhost:8080', $token));
$client->publishJson('orders', array('order' => 9001));
$client->consumeWith('orders', function ($context) { echo $context->message->bodyText(); }, array('waitTime' => 20));
```

```ruby
# Ruby
client = Taba::Client.new('http://localhost:8080', token: token)
client.publish_json('orders', order: 9001)
client.consume_with('orders', wait_time: 20) { |context| puts context.message.body_text }
```

```rust
// Rust (feature "serde")
client.publish_object("orders", &Order { number: 9001 })?;
client.consume_typed::<Order, _>("orders", &ConsumeOptions::default(), |d| {
    process_order(&d.body)?;
    Ok(())
})?;
```

Cada um também tem `subscribe` (o laço de consumo inteiro, até ser cancelado) e chamadas em lote (`publishBatch`, `ackBatch`, `changeVisibilityBatch`); veja o README de cada um.

O `taba-cli`, a linha de comando de cliente ([connectors/rust/cli](connectors/rust/cli/README.md)), está pronto e é o caminho mais curto para experimentar um cluster.

**Driver do OpenMessaging Benchmark.** O [`connectors/omb`](connectors/omb/README.md) é um driver para o [OpenMessaging Benchmark](https://github.com/openmessaging/benchmark), escrito sobre o conector Java, para medir o Taba com a mesma ferramenta e as mesmas cargas do RabbitMQ e do NATS. É ele que o [benchmark](#desempenho) usa.

**Provider Terraform e OpenTofu.** O [`connectors/terraform`](connectors/terraform/README.md) gerencia filas, distribuidores e tokens de acesso como código (`rafaelpontani/taba`), com `plan`, detecção de desvio, `import`, uma proteção contra remover uma fila que ainda tem mensagens (`force_destroy`) e um `create_or_update` opcional que adota o que já existe. Os zips dele vêm em cada [Release](https://github.com/rafaelpontani/taba/releases); os registries vêm a seguir ([o guia](doc/pt-BR/40-terraform.md)).

As outras linguagens (C e C++ ficam adiados, sem data) usam o próprio cliente HTTP. O desenho está no [guia dos conectores](doc/pt-BR/21-conectores.md).

> As versões base são propostas e podem mudar até serem confirmadas (pendência D68, veja as [pendências](doc/pt-BR/31-pendencias.md)).

## Use o AWS SDK sem mudar o seu código

Uma aplicação que já envia para o **Amazon SQS** ou publica em tópicos do **Amazon SNS** que alimentam filas SQS pode rodar no Taba **mudando só o endpoint**. O nó tem um gateway que fala os protocolos do SQS e do SNS (JSON e Query, SigV4) e transforma cada chamada numa chamada da API de cliente; então as filas são filas comuns do Taba: replicadas por quórum, com backup no S3 e acessíveis também pela API nativa e pelos conectores. Ele faz parte do nó e da imagem, vem **desligado por padrão**, e o SQS e o SNS são atendidos em **uma porta só**, a `9324`.

**Vantagens**
- **Sem mudar código, sem cliente novo:** o SDK que você já usa (C#, Java, Rust, Go, Python, Node.js, nas versões atuais e nas antigas) continua funcionando. Os mesmos cenários rodam com o SDK nativo de cada linguagem nos [testes de contrato do gateway](connectors/gateway-contract/README.md), e também no ElasticMQ e no moto, para mostrar que os testes estão certos.
- **Saia da AWS sem reescrever:** sem cobrança por requisição, e as mesmas filas rodam na sua máquina, numa VM ou no Kubernetes. O desenvolvimento local usa a mesma imagem da produção, sem emulador.
- **Nada a mais para operar:** o gateway está dentro de cada nó. Não há serviço extra nem estado próprio, e qualquer nó atende qualquer chamada.
- **Um sistema, duas portas:** uma mensagem enviada pelo SDK é lida por um conector nativo, e o contrário também. Há filas padrão e **FIFO**, atraso, dead-letter, atributos de mensagem, long polling e os códigos de erro da AWS ([a lista completa](doc/pt-BR/39-gateway.md)).

**Limitações** (cada uma é um erro explícito, nunca ignorada em silêncio; todas no [doc 39](doc/pt-BR/39-gateway.md))
- Só SQS e SNS. O SNS entrega **só para filas SQS**: sem assinantes HTTP, e-mail, SMS ou Lambda.
- `RawMessageDelivery=true` e `FilterPolicy` numa assinatura são **recusados**, **tópicos** FIFO são recusados, e um tópico tem no máximo 20 assinaturas. A notificação não é assinada, então uma aplicação que verifica a assinatura do SNS falha.
- A assinatura é um **prefixo** do nome: assinar `orders` também copia para `orders-eu`, e um tópico e uma fila não podem ter o mesmo nome.
- As chaves de acesso são estáticas e ficam num arquivo (`gateway.aws.credentials-file`); credenciais temporárias de um papel IAM não são aceitas.
- Sair da AWS é ficar sem IAM, KMS e CloudWatch: a política da fila e os atributos de criptografia são aceitos e ignorados.

### Rode

Com o [cluster do Docker Compose](#experimente-em-um-minuto) o gateway já vem **ligado** (SQS e SNS em `localhost:9324`), com uma chave de acesso de demonstração que você usa na hora:

```bash
docker compose up -d
export AWS_ENDPOINT_URL_SQS=http://localhost:9324 AWS_ENDPOINT_URL_SNS=http://localhost:9324
export AWS_ACCESS_KEY_ID=taba AWS_SECRET_ACCESS_KEY=taba-secret AWS_REGION=us-east-1
```

Essa chave age com o token raiz e serve só para experimentar o Taba (veja [as chaves de acesso](doc/pt-BR/39-gateway.md#as-chaves-de-acesso)). Ou um nó para desenvolvimento (sem autenticação, então qualquer chave de acesso é aceita; nunca deve ficar numa rede):

```bash
docker run -d --name taba-dev -p 8080:8080 -p 9324:9324 \
  -e TABA_REPLICATION_FACTOR=1 -e TABA_GATEWAY_SQS_ENABLED=true -e TABA_GATEWAY_SNS_ENABLED=true \
  rafaelpontani/taba
```

Aponte o SDK para ele. Os SDKs atuais leem o endpoint do ambiente, então **o código não muda em nada**:

```bash
export AWS_ENDPOINT_URL_SQS=http://localhost:9324
export AWS_ENDPOINT_URL_SNS=http://localhost:9324
export AWS_ACCESS_KEY_ID=test AWS_SECRET_ACCESS_KEY=test AWS_REGION=us-east-1   # qualquer valor enquanto a autenticação está desligada
```

As versões antigas dos SDKs (anteriores à variável de endpoint) recebem o endereço no código, como em todos os exemplos abaixo. Num cluster a autenticação está ligada: cada aplicação recebe uma chave de acesso ligada a um token do Taba (suas permissões e prefixos de tópico) num arquivo de credenciais montado nos nós. Com o [chart Helm](#rode-no-kubernetes-helm): `kubectl create secret generic aws-keys --from-file=credentials.json` e `helm install taba oci://ghcr.io/rafaelpontani/charts/taba --set gateway.sqs.enabled=true --set gateway.sns.enabled=true --set gateway.credentialsSecret.name=aws-keys --set gateway.sqs.publicUrl=https://queues.example.com`. O formato do arquivo, as permissões e as configurações estão no [guia do gateway](doc/pt-BR/39-gateway.md) e na [configuração](doc/pt-BR/22-configuracao.md).

### Conecte

Em todos os exemplos a fila se chama `orders`: é o nome da fila SQS e também o nome do tópico (fila) do Taba, então `taba-cli consume orders` lê as mesmas mensagens. O SQS não usa o nome nas chamadas de mensagem, e sim a **URL da fila** (`QueueUrl`). Você a obtém pelo nome: `CreateQueue` (cria a fila ou devolve a que já existe) ou `GetQueueUrl` devolvem `http://localhost:9324/000000000000/orders`, e é essa URL que vai em `SendMessage`, `ReceiveMessage` e `DeleteMessage`. A `QueueArn` (`arn:aws:sqs:us-east-1:000000000000:orders`) é a identificação da fila que o SNS usa na assinatura.

```python
# Python (boto3): criar uma fila, enviar, receber e apagar
import boto3

sqs = boto3.client("sqs", region_name="us-east-1", endpoint_url="http://localhost:9324")
url = sqs.create_queue(QueueName="orders")["QueueUrl"]   # CreateQueue recebe o NOME e devolve a URL; as outras chamadas usam a URL
sqs.send_message(QueueUrl=url, MessageBody='{"order": 9001}')
for m in sqs.receive_message(QueueUrl=url, WaitTimeSeconds=5).get("Messages", []):
    print(m["Body"])
    sqs.delete_message(QueueUrl=url, ReceiptHandle=m["ReceiptHandle"])
```

O SNS publicando na mesma fila:

```python
sns = boto3.client("sns", region_name="us-east-1", endpoint_url="http://localhost:9324")
topic = sns.create_topic(Name="events")["TopicArn"]
queue_arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]   # arn:aws:sqs:us-east-1:000000000000:orders
sns.subscribe(TopicArn=topic, Protocol="sqs", Endpoint=queue_arn)   # a fila "orders" passa a receber toda publicação
sns.publish(TopicArn=topic, Message="hello", Subject="greeting")
```

```js
// Node.js (AWS SDK v3)
import { SQSClient, CreateQueueCommand, SendMessageCommand, ReceiveMessageCommand } from "@aws-sdk/client-sqs";

const sqs = new SQSClient({ region: "us-east-1", endpoint: "http://localhost:9324" });
const { QueueUrl } = await sqs.send(new CreateQueueCommand({ QueueName: "orders" }));   // o nome entra, a URL sai
await sqs.send(new SendMessageCommand({ QueueUrl, MessageBody: "hello" }));
const { Messages } = await sqs.send(new ReceiveMessageCommand({ QueueUrl, WaitTimeSeconds: 5 }));
```

```java
// Java (AWS SDK v2)
SqsClient sqs = SqsClient.builder()
    .region(Region.US_EAST_1)
    .endpointOverride(URI.create("http://localhost:9324"))
    .build();
String url = sqs.createQueue(r -> r.queueName("orders")).queueUrl();   // o nome entra, a URL sai
sqs.sendMessage(r -> r.queueUrl(url).messageBody("hello"));
```

```csharp
// C# (AWSSDK.SQS)
var sqs = new AmazonSQSClient(new AmazonSQSConfig { ServiceURL = "http://localhost:9324", AuthenticationRegion = "us-east-1" });
var url = (await sqs.CreateQueueAsync("orders")).QueueUrl;   // o nome entra, a URL sai
await sqs.SendMessageAsync(new SendMessageRequest { QueueUrl = url, MessageBody = "hello" });
```

```go
// Go (aws-sdk-go-v2)
cfg, _ := config.LoadDefaultConfig(ctx, config.WithRegion("us-east-1"))
client := sqs.NewFromConfig(cfg, func(o *sqs.Options) { o.BaseEndpoint = aws.String("http://localhost:9324") })
created, _ := client.CreateQueue(ctx, &sqs.CreateQueueInput{QueueName: aws.String("orders")})   // o nome entra, a URL sai
client.SendMessage(ctx, &sqs.SendMessageInput{QueueUrl: created.QueueUrl, MessageBody: aws.String("hello")})
```

```rust
// Rust (aws-sdk-sqs)
let shared = aws_config::defaults(BehaviorVersion::latest()).region("us-east-1").load().await;
let conf = aws_sdk_sqs::config::Builder::from(&shared).endpoint_url("http://localhost:9324").build();
let client = aws_sdk_sqs::Client::from_conf(conf);
let url = client.create_queue().queue_name("orders").send().await?.queue_url().unwrap().to_string();   // o nome entra, a URL sai
client.send_message().queue_url(&url).message_body("hello").send().await?;
```

E a mesma mensagem sem SDK nenhum, pela API nativa (o corpo de uma mensagem enviada sem atributos é guardado como veio):

```bash
taba-cli consume orders --wait 5 --ack
```

## Como funciona por dentro

**Taba** vem de **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos. Em tupi, *taba* é a **aldeia**: cada nó é uma oca, o cluster é a aldeia, e as mensagens circulam entre elas sem que você precise cuidar disso.

![C4 nível 0: contexto do Taba](doc/diagrams/img/pt-BR/c4-level0-context.svg)

| Letra | Promessa | Como o Taba cumpre |
|---|---|---|
| **T**ópicos | Uma API enxuta, com fila (entrega única) e distribuição por prefixo num só lugar. | Você envia a um tópico com verbos HTTP e escolhe o tipo: Tópico Fila ou Tópico Distribuidor. |
| **A**ssíncrono | O produtor envia e segue em frente. | Uma escrita é confirmada em memória por um número configurável de nós; o disco só entra no overflow. |
| **B**alanceado | Nenhum nó carrega tudo sozinho. | Um anel de hash consistente divide os tópicos em blocos e os replica em vários nós. |
| **A**utogerido | Sem coordenador e sem operação manual. | Os nós se descobrem, se monitoram e votam para tirar quem falha. Quando o nó volta, é readmitido. |

![Anel de hash: blocos e papéis](doc/diagrams/img/pt-BR/ring-hash.svg)

*O anel de hash divide o trabalho entre os nós.*

![Tópico espalhado e redundante](doc/diagrams/img/pt-BR/spread-topic.svg)

*Um tópico espalhado pelo cluster e replicado: se um nó cai, a aldeia continua de pé.*

## Verificando um release

Toda versão é compilada e publicada só pelo [workflow de release](.github/workflows/release.yml), a partir do commit da sua tag, depois de todos os testes passarem. Cada artefato pode ser conferido de forma independente, sem confiar na palavra deste repositório:

| Artefato | Onde | Prova |
|---|---|---|
| Imagens `taba` e `taba-cli` | Docker Hub (`rafaelpontani/…`) e GHCR (`ghcr.io/rafaelpontani/…`) | Proveniência SLSA, SBOM, assinatura cosign; label `org.opencontainers.image.revision` = commit |
| Binários, pacotes NuGet, chart | [GitHub Releases](https://github.com/rafaelpontani/taba/releases) (imutáveis) | `SHA256SUMS` e uma atestação por arquivo |
| Crates `taba-client`, `taba-cli` | crates.io | Trusted Publishing a partir do workflow; `.cargo_vcs_info.json` com o commit |
| Pacotes `Taba.Client*` | nuget.org | Trusted Publishing; `<repository commit>` e o link da Release no `.nuspec`; SourceLink |
| Chart Helm | `oci://ghcr.io/rafaelpontani/charts/taba` | Assinatura cosign e atestação; `appVersion` = a versão |

```bash
# imagens (qualquer tag: 0.1.0, 0.1, 0, latest... todas apontam para o mesmo digest)
gh attestation verify oci://ghcr.io/rafaelpontani/taba:latest -R rafaelpontani/taba
cosign verify ghcr.io/rafaelpontani/taba:latest \
  --certificate-identity-regexp '^https://github.com/rafaelpontani/taba/.github/workflows/release.yml@refs/heads/(main|release)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com

# arquivos de uma Release
VERSION=0.1.1
gh release download "v$VERSION" -R rafaelpontani/taba && sha256sum -c SHA256SUMS
gh attestation verify "taba-$VERSION-x86_64-unknown-linux-musl" -R rafaelpontani/taba

# o commit dentro do binário
docker run --rm rafaelpontani/taba:latest --version     # taba 0.1.1 (commit <sha>)
```

Versões: a `main` publica as finais (`v0.1.0`) e a `release` publica as candidatas (`v0.1.0-rc.N`). As tags flutuantes (`v0.1`, `v0`, `latest`, `0.1-rc`, `rc`...) só andam para a maior versão da sua linha. O fluxo completo está em [CI/CD e releases](doc/pt-BR/27-ci-cd.md); para relatar uma vulnerabilidade, veja o [SECURITY.md](SECURITY.md).

## Documentação

A documentação está em [doc/](doc/README.md), uma pasta por idioma: comece em [doc/pt-BR/README.md](doc/pt-BR/README.md) para português do Brasil (resumos para IA estão em [llms.txt](llms.txt), em inglês, e em [doc/pt-BR/llms.txt](doc/pt-BR/llms.txt)) ou em [doc/en-US/README.md](doc/en-US/README.md) para inglês. As duas pastas têm o mesmo conteúdo e toda mudança vai para todas. Outros idiomas entrarão como pastas irmãs. O código, os identificadores, as rotas, as chaves de configuração e os códigos de erro estão em inglês.

## Licença

- **Servidor** (`crates/`, a imagem `taba`, o chart Helm e o resto do repositório): [GNU AGPLv3](LICENSE).
- **Conectores** (`connectors/`: `taba-client`, `taba-cli` e os pacotes `Taba.Client*`): [Apache-2.0](connectors/LICENSE).

Contribuições são bem-vindas: veja o [CONTRIBUTING.md](CONTRIBUTING.md) (em inglês). Um pull request entra depois que o autor assina o [CLA](CLA.md), uma única vez.
