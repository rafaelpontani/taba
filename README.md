<p align="center">
  <img src="doc/diagramas/img/hero.gif" width="100%" alt="Taba: mensageria open source que substitui SQS, SNS, Kafka e RabbitMQ com uma só imagem Docker. Animação: 3 nós, cada um desenhado como um prédio de 3 andares. Vários produtores entram e saem e publicam mensagens; cada mensagem tem um nó primário, que a replica nos outros dois, e todo nó é o primário de parte delas. Consumidores chegam depois, leem de forma assíncrona e confirmam com ack.">
</p>

**Taba** é um sistema de mensageria **leve, rápido e open source** que roda com **uma única imagem Docker**, em 1 nó para testar e em 3 para produção. Ele reúne **fila** (entrega única, como o SQS) e **pub/sub** (broadcast, como o SNS) na mesma API HTTP, e escala adicionando nós, sem coordenador e sem configuração complexa.

Se você quer sair da cobrança por mensagem, de um cluster de brokers para operar ou da dependência de uma nuvem, o Taba é para você.

## O que você ganha

- **Uma imagem, sem dependências:** todos os nós rodam a mesma imagem Docker, simples e leve. Sobe 1 nó para testar e 3 para produção, sem broker, coordenador ou banco à parte. Roda em uma máquina, em uma VM ou no Kubernetes.
- **Rápido:** as mensagens ficam em memória, replicadas entre os nós. O disco só entra quando a memória enche.
- **Sempre disponível:** cada tópico é espalhado e replicado. Se um nó cai, o cluster segue de pé, e o nó volta sozinho quando se recupera.
- **Escala sem esforço:** um nó novo entra no cluster, se descobre sozinho e assume a sua parte. Não há coordenador para configurar.
- **Custo quase zero:** sem cobrança por mensagem e sem obrigação de nuvem. Você paga só pela máquina em que roda.
- **Filas FIFO:** ordem de entrega por grupo e deduplicação de publicações, para quando a sequência importa.
- **API simples:** poucas rotas HTTP, sem SDK obrigatório e sem protocolo novo para aprender.
- **Conectores simples nas principais linguagens:** C#, Java, Rust, C, C++, Python, PHP, Ruby, Go e Node.js, inclusive em versões antigas (veja [Conectores](#conectores)).
- **Open source (AGPLv3):** o código é aberto e, se alguém oferecer o Taba como serviço com modificações, precisa compartilhá-las sob a mesma licença.

> Velocidade e custo são **metas do projeto**. Ainda não há benchmarks publicados, e o Taba está em fase de documentação (veja [Status e limites](#status-e-limites)).

## Taba × as alternativas

| | **Taba** | SQS + SNS | Kafka | RabbitMQ |
|---|---|---|---|---|
| **Onde roda** | Qualquer lugar: máquina, VM, Docker, Kubernetes | Só na AWS | Onde você hospedar, ou serviço gerenciado | Onde você hospedar, ou serviço gerenciado |
| **O que subir** | A mesma imagem em 3 nós (1 para dev) | Nada (gerenciado) | Brokers e coordenação de metadados | Broker (mais nós para alta disponibilidade) |
| **Fila e pub/sub** | Na mesma API HTTP | Dois serviços separados | Tópicos e grupos de consumo | Filas e exchanges |
| **Cobrança** | Sem cobrança por mensagem: só a infraestrutura | Por requisição | Infraestrutura, ou plano gerenciado | Infraestrutura, ou plano gerenciado |
| **Escalar** | Adicionar um nó | Automático (gerenciado) | Brokers e partições | Nós no cluster |

## A API em quatro chamadas

Tudo é HTTP. O contrato completo (OpenAPI) ainda está em definição; os verbos abaixo seguem o [rascunho da API](doc/06-api.md).

| O que você quer | Chamada |
|---|---|
| Criar um tópico (Fila ou Distribuidor) | `POST /topics` |
| Publicar uma mensagem | `POST /topics/{topic}/messages` |
| Receber mensagens (com long polling de até 20 s) | `GET /topics/{topic}/messages` |
| Confirmar o consumo (ack) | `DELETE /topics/{topic}/messages/{id}` |

Se o consumidor não confirmar a tempo, a mensagem volta a ficar disponível e é entregue de novo, como no SQS (a invisibilidade padrão é de 120 s e pode ser ajustada).

## Conectores

A API é HTTP, então qualquer linguagem já fala com o Taba pelo cliente HTTP que ela tem. Além disso, o projeto prevê **conectores simples de usar** nas principais linguagens, para você publicar e consumir mensagens em poucas linhas. Eles não têm dependências externas e rodam em versões antigas, de até 15 anos (veja a [versão-base](doc/19a-conectores-linguagens.md) de cada uma):

| Linguagem | Pasta | Versão-base | Estado | Exemplo de uso |
|---|---|---|---|---|
| Java | `connectors/java/` | Java 6 | planejado | TODO |
| C# | `connectors/csharp/` | .NET Framework 3.5 | planejado | TODO |
| Python | `connectors/python/` | 2.6 | planejado | TODO |
| PHP | `connectors/php/` | 5.3 | planejado | TODO |
| Ruby | `connectors/ruby/` | 1.8.7 | planejado | TODO |
| Node.js | `connectors/node/` | 0.10 | planejado | TODO |
| Go | `connectors/go/` | 1.4 | planejado | TODO |
| Rust | `connectors/rust/` | 1.31 | planejado | TODO |
| C | `connectors/c/` | C89 | planejado | TODO |
| C++ | `connectors/cpp/` | C++03 | planejado | TODO |

Outras linguagens usam o cliente HTTP delas ou a biblioteca em C. O desenho está em [doc/19-conectores.md](doc/19-conectores.md).

<!-- TODO: adicionar exemplos de uso (criar tópico, publicar, consumir com long polling e ack) em cada linguagem, quando os conectores existirem. -->

> As versões-base são propostas e podem mudar até a confirmação ([08b-pendencias.md](doc/08b-pendencias.md), D68).

## Status e limites

- **Em fase de documentação:** o projeto ainda não tem código de aplicação. O que existe é o desenho completo, em [doc/](doc/README.md).
- **Não é um substituto de protocolo:** o Taba fala HTTP. Não há compatibilidade com AMQP, MQTT ou o protocolo do Kafka, então trocar de broker significa trocar o cliente.
- **Não é uma plataforma de streaming:** processamento de streams e reprocessamento do histórico de mensagens estão fora do escopo. Para filas e pub/sub, sim; para reler o passado, não.
- **Fora do escopo por enquanto:** interface gráfica, autenticação avançada e multi-região.

## Como funciona por dentro

**Taba** vem de **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos *(inglês: **T**opics, **A**synchronous, **B**alanced and **A**utonomous)*. Em tupi, *taba* é a **aldeia**: cada nó é uma oca, o cluster é a aldeia, e as mensagens circulam entre elas sem que você precise cuidar disso.

![C4 nível 0: contexto do Taba](doc/diagramas/img/c4-nivel0-contexto.svg)

| Letra | Promessa | Como o Taba cumpre |
|---|---|---|
| **T**ópicos | Uma API enxuta, com fila (entrega única) e distribuição por prefixo no mesmo lugar. | Você envia para um tópico com verbos HTTP e escolhe o tipo: Tópico Fila ou Tópico Distribuidor. |
| **A**ssíncronos | O produtor envia e segue em frente. | A escrita é confirmada em memória por um número configurável de nós; o disco só entra por overflow. |
| **B**alanceados | Nenhum nó carrega tudo sozinho. | Um hash circular reparte os tópicos em blocos e os replica em vários nós. |
| **A**utogeridos | Sem coordenador e sem operação manual. | Os nós se descobrem, se monitoram e votam a saída de quem falha. Quando o nó volta, é reintegrado. |

![Anel de hash: blocos e papéis](doc/diagramas/img/anel-hash.svg)

*O anel de hash reparte o trabalho entre os nós.*

![Tópico espalhado e redundante](doc/diagramas/img/topico-espalhado.svg)

*Um tópico espalhado pelo cluster e replicado: se um nó cai, a aldeia continua de pé.*

## Documentação

Quer entender cada detalhe? Comece pela [visão geral](doc/01-visao-geral.md) e siga o [índice da documentação](doc/README.md).

## Licença

[GNU AGPLv3](LICENSE).
