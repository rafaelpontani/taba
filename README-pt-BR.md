<p align="center">
  <img src="doc/diagramas/img/pt-BR/hero.gif" width="100%" alt="Taba: mensageria open source que substitui SQS, SNS, Kafka e RabbitMQ com uma única imagem Docker. Animação: 3 nós, cada um desenhado como um prédio de 3 andares. Vários produtores chegam e saem e publicam mensagens; cada mensagem tem um nó primário, que a replica para os outros dois, e todo nó é primário de algumas delas. Os consumidores chegam depois, leem de forma assíncrona e confirmam com um ack.">
</p>

🇺🇸 [English](README.md) | 🇧🇷 **Português (Brasil)**

O **Taba** é um sistema de mensageria **leve, rápido e open source** que roda a partir de **uma única imagem Docker**: 1 nó para experimentar, 3 para produção. Ele reúne **filas** (cada mensagem vai para um consumidor, como o SQS) e **pub/sub** (cada publicação é copiada para várias filas, como o SNS) na mesma API HTTP, e cresce com a adição de nós, sem coordenador e sem configuração complexa.

Se você quer fugir da cobrança por mensagem, de um cluster de brokers para operar ou da dependência de uma nuvem, o Taba é para você.

## O que você ganha

- **Uma imagem, sem dependências:** todo nó roda a mesma imagem Docker, simples e pequena. Suba 1 nó para testar e 3 para produção, sem broker, coordenador ou banco de dados à parte. Roda numa máquina, numa VM ou no Kubernetes.
- **Rápido:** as mensagens vivem em memória, replicadas entre os nós. O disco só entra quando a memória enche.
- **Sempre disponível:** todo tópico é espalhado e replicado. Se um nó cai, o cluster continua, e o nó volta sozinho quando se recupera.
- **Cresce sem esforço:** um nó novo entra no cluster, o descobre sozinho e assume a sua parte. Não há coordenador para configurar.
- **Custo quase zero:** sem cobrança por mensagem e sem amarra a uma nuvem. Você só paga a máquina em que ele roda.
- **Filas FIFO:** ordem de entrega por grupo e deduplicação na publicação, para quando a sequência importa.
- **API simples:** poucas rotas HTTP, sem SDK obrigatório e sem protocolo novo para aprender.
- **Conectores simples nas principais linguagens:** C#, Java, Rust, C, C++, Python, PHP, Ruby, Go e Node.js, inclusive versões antigas (veja [Conectores](#conectores)).
- **Open source (AGPLv3):** o código é aberto e, se alguém oferecer o Taba como serviço com modificações, deve compartilhá-las sob a mesma licença.

> Velocidade e custo são **objetivos do projeto**. Ainda não há benchmarks publicados (veja [Estado e limites](#estado-e-limites)).

## Taba × as alternativas

| | **Taba** | SQS + SNS | Kafka | RabbitMQ |
|---|---|---|---|---|
| **Onde roda** | Em qualquer lugar: máquina, VM, Docker, Kubernetes | Só na AWS | Onde você hospedar, ou como serviço gerenciado | Onde você hospedar, ou como serviço gerenciado |
| **O que operar** | A mesma imagem em 3 nós (1 para desenvolvimento) | Nada (gerenciado) | Brokers e coordenação de metadados | Um broker (mais nós para alta disponibilidade) |
| **Fila e pub/sub** | Na mesma API HTTP | Dois serviços separados | Tópicos e grupos de consumidores | Filas e exchanges |
| **Cobrança** | Sem cobrança por mensagem: só infraestrutura | Por requisição | Infraestrutura, ou um plano gerenciado | Infraestrutura, ou um plano gerenciado |
| **Escala** | Adicione um nó | Automática (gerenciada) | Brokers e partições | Nós do cluster |

## Experimente em um minuto

Com Docker, um cluster de três nós (atrás de um HAProxy de exemplo) sobe com um comando. Um contêiner de inicialização cria o certificado do cluster e o token raiz, o primeiro nó forma o cluster e os outros dois entram:

```bash
docker compose up -d              # três nós "healthy" em cerca de um minuto (imagens públicas do Docker Hub)
```

Para construir as imagens a partir deste repositório (desenvolvimento): `docker compose -f docker-compose.yml -f docker-compose.dev.yml up --build -d`.

Depois, com o [`taba-cli`](doc/pt-BR/20-cli.md), que já vem no Compose apontado para o cluster (e com o token raiz):

```bash
docker compose run --rm taba-cli health cluster -o table             # os três nós, sem condições
docker compose run --rm taba-cli topic create orders --type queue
echo '{"order": 9001}' | docker compose run --rm -T taba-cli publish orders --stdin
docker compose run --rm taba-cli consume orders --wait 5 --ack      # recebe e confirma
```

A API de cliente também está em `http://localhost:8080` para qualquer cliente HTTP (com `Authorization: Bearer <token raiz>`; o token está no volume `secrets`: `docker compose run --rm --entrypoint cat taba-cli /secrets/root-token`; no Git Bash do Windows, prefixe o comando com `MSYS_NO_PATHCONV=1`). `docker compose down -v` apaga tudo. O HAProxy é só um exemplo: qualquer balanceador serve. Para um único nó, sem cluster, use `docker run -p 8080:8080 -e TABA_MODE=development taba`.

> **Cuidado em produção:** pare e atualize os nós **um de cada vez** (`docker compose stop` envia `SIGTERM`, que dispara um *drain* ordenado). Parar **todos** os nós de uma vez perde as mensagens e os tópicos, e perder **dois nós seguidos** trava um cluster de 3 nós. Veja o [guia de operação](doc/pt-BR/23-operacao.md).

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
| Java | `connectors/java/` | Java 11 | planejado | TODO |
| C# | `connectors/csharp/` | .NET Framework 4.6.1 e .NET 5, 6, 8 e 10 | [pronto](connectors/csharp/README.md) (nove operações assíncronas e síncronas, `subscribe`) | [exemplo de console](exemplos/csharp-console/) |
| Python | `connectors/python/` | 2.7.9 e 3.4.3 | planejado | TODO |
| PHP | `connectors/php/` | 5.6 | planejado | TODO |
| Ruby | `connectors/ruby/` | 2.0 | planejado | TODO |
| Node.js | `connectors/node/` | 0.12 | planejado | TODO |
| Go | `connectors/go/` | 1.7 | planejado | TODO |
| Rust | `connectors/rust/client/` | 1.70 | síncrono pronto ([README](connectors/rust/client/README.md)) | [exemplo](connectors/rust/client/README.md) |
| C | `connectors/c/` | C89 | planejado | TODO |
| C++ | `connectors/cpp/` | C++03 | planejado | TODO |

O `taba-cli`, a linha de comando de cliente ([connectors/rust/cli](connectors/rust/cli/README.md)), está pronto e é o caminho mais curto para experimentar um cluster.

As outras linguagens usam o próprio cliente HTTP ou a biblioteca C. O desenho está no [guia dos conectores](doc/pt-BR/21-conectores.md).

<!-- TODO: adicionar exemplos de uso (criar tópico, publicar, consumir com long polling e ack) para cada linguagem, quando os conectores existirem. -->

> As versões base são propostas e podem mudar até serem confirmadas (pendência D68, veja as [pendências](doc/pt-BR/31-pendencias.md)).

## Estado e limites

- **Versão 0.1 validada de ponta a ponta:** o cluster de três nós sobe com `docker compose up`, se forma sozinho, atende pelo `taba-cli`, detecta a queda de um nó, o exclui por votação e o readmite sem perder mensagens confirmadas (script em [docker/validate.sh](docker/validate.sh)). A documentação completa está em [doc/](doc/README.md). Ainda faltam: os demais conectores, o contrato OpenAPI e a publicação nos repositórios de pacotes. Veja o [roadmap](doc/pt-BR/32-roadmap.md).
- **Não substitui um protocolo:** o Taba fala HTTP. Não há compatibilidade com os protocolos AMQP, MQTT ou Kafka, então trocar de broker significa trocar o cliente.
- **Não é uma plataforma de streaming:** processamento de streams e reprodução do histórico de mensagens estão fora do escopo. Para filas e pub/sub, sim; para reler o passado, não.
- **Fora do escopo por enquanto:** interface gráfica, autenticação avançada e multirregião.

## Como funciona por dentro

**Taba** vem de **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos. Em tupi, *taba* é a **aldeia**: cada nó é uma oca, o cluster é a aldeia, e as mensagens circulam entre elas sem que você precise cuidar disso.

![C4 nível 0: contexto do Taba](doc/diagramas/img/pt-BR/c4-nivel0-contexto.svg)

| Letra | Promessa | Como o Taba cumpre |
|---|---|---|
| **T**ópicos | Uma API enxuta, com fila (entrega única) e distribuição por prefixo num só lugar. | Você envia a um tópico com verbos HTTP e escolhe o tipo: Tópico Fila ou Tópico Distribuidor. |
| **A**ssíncrono | O produtor envia e segue em frente. | Uma escrita é confirmada em memória por um número configurável de nós; o disco só entra no overflow. |
| **B**alanceado | Nenhum nó carrega tudo sozinho. | Um anel de hash consistente divide os tópicos em blocos e os replica em vários nós. |
| **A**utogerido | Sem coordenador e sem operação manual. | Os nós se descobrem, se monitoram e votam para tirar quem falha. Quando o nó volta, é readmitido. |

![Anel de hash: blocos e papéis](doc/diagramas/img/pt-BR/anel-hash.svg)

*O anel de hash divide o trabalho entre os nós.*

![Tópico espalhado e redundante](doc/diagramas/img/pt-BR/topico-espalhado.svg)

*Um tópico espalhado pelo cluster e replicado: se um nó cai, a aldeia continua de pé.*

## Documentação

A documentação está em [doc/](doc/README.md), uma pasta por idioma. **Hoje ela está escrita em português do Brasil**: comece em [doc/pt-BR/README.md](doc/pt-BR/README.md) (um índice para IA está em [doc/pt-BR/llms.txt](doc/pt-BR/llms.txt)). Outros idiomas entrarão como pastas irmãs. O código, os identificadores, as rotas, as chaves de configuração e os códigos de erro estão em inglês.

## Licença

[GNU AGPLv3](LICENSE).
