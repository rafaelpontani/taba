---
título: Visão geral
resumo: O que é o Taba, para quem serve, o que ele não é, e a lista de objetivos e requisitos com o estado de cada um.
tipo: explicação
status: estável
verificado-em: 2026-10-19 @ 55b9495
fontes: [README.md, Cargo.toml, crates/taba-node/src/lib.rs, docker-compose.yml, connectors/]
---

# Visão geral

> Leia depois: [02-inicio-rapido.md](02-inicio-rapido.md) (ver funcionando) e [03-conceitos.md](03-conceitos.md) (o vocabulário).

## O que é

O **Taba** é um sistema de mensageria: aplicações **publicam** mensagens em **tópicos** e outras aplicações as **consomem**, de forma assíncrona. Ele reúne em uma só API HTTP dois padrões que, numa nuvem, costumam ser dois serviços:

- **Fila** (como o Amazon SQS): cada mensagem é entregue a **um** consumidor, e volta a ficar disponível se ele não a confirmar a tempo.
- **Distribuição** (como o Amazon SNS): cada publicação é **copiada** para todas as filas cujo nome começa com um dos prefixos do tópico distribuidor.

O nome vem de **T**ópicos **A**ssíncronos **B**alanceados e **A**utogeridos. Em tupi, *taba* é a aldeia: cada nó é uma oca, o cluster é a aldeia.

Cada nó é **o mesmo programa** (o binário `taba`, escrito em Rust), empacotado numa única imagem Docker. Um cluster são três ou mais nós iguais, sem coordenador, sem banco externo e sem broker à parte. Para testar, um nó basta (modo desenvolvimento).

![Contexto: quem usa o Taba](../diagrams/img/pt-BR/c4-level0-context.svg)

## Para quem serve

| Situação | Por que o Taba |
|---|---|
| Quer fila e pub/sub sem pagar por mensagem nem depender de uma nuvem | Roda onde houver Docker; o custo é o da máquina |
| Quer um cluster de mensageria que se organiza sozinho | Nó novo entra e assume sua parte; nó que cai é excluído por votação e, ao voltar, é reintegrado |
| Quer escalar nos dois sentidos sem parar | Adicionar ou retirar um nó rebalanceia os blocos sozinho; o nó que sai faz o drain antes. No Kubernetes, o KEDA escala o chart ([34-chart-helm.md](34-chart-helm.md)) |
| Quer backup sem pagar por ele em latência | O backup no S3 é contínuo, nunca atrasa uma escrita, e um cluster que caiu inteiro se restaura sozinho ao subir ([33-backup-s3.md](33-backup-s3.md)) |
| Quer integrar por HTTP, sem protocolo novo | A API é HTTP e JSON; os conectores são opcionais |

## O que o Taba não é

| Não é | Consequência |
|---|---|
| Um substituto de protocolo | Não fala AMQP, MQTT nem o protocolo do Kafka; trocar de broker é trocar o cliente |
| Uma plataforma de streaming | Não relê o histórico: depois do ack a mensagem some (na FIFO, só o corpo; ver [09-topicos.md](09-topicos.md)) |
| Multi-região | Um cluster vive numa só rede |
| Uma interface web | Há API, linha de comando com uma interface de terminal em tela cheia ([20b-cli-ui.md](20b-cli-ui.md)) e telemetria; não há painel web |
| Autenticação avançada | Há tokens simples com permissões e prefixos; não há OAuth/OIDC |

## Objetivos e requisitos

Os requisitos têm código estável (`RF` funcional, `RNF` não funcional) para serem citados em outros documentos. O estado é o do código hoje.

### Requisitos funcionais

| Código | Requisito | Estado | Detalhe |
|---|---|---|---|
| RF-01 | Criar, listar, ler, alterar e remover tópicos | ✅ | [16-api-cliente.md](16-api-cliente.md) |
| RF-02 | Tópico **fila** (entrega única) e **distribuidor** (cópia por prefixo) | ✅ | [09-topicos.md](09-topicos.md) |
| RF-03 | Fila **volátil** que some sozinha sem consumidor, criável pelo próprio consumo | ✅ | [09-topicos.md](09-topicos.md) |
| RF-04 | Fila **FIFO**: ordem por grupo e deduplicação | ✅ | [09-topicos.md](09-topicos.md) |
| RF-05 | Publicar, consumir (com long polling) e confirmar (ack) | ✅ | [12-entrega.md](12-entrega.md) |
| RF-06 | Reentrega por **invisibilidade** temporária, alterável depois da entrega | ✅ | [12-entrega.md](12-entrega.md) |
| RF-07 | Validade (`ttl`), limite de entregas e **dead-letter** | ✅ | [12-entrega.md](12-entrega.md) |
| RF-08 | Grupos de mensagens e **entrega justa** entre produtores | ✅ | [12-entrega.md](12-entrega.md) |
| RF-09 | Mensagens em memória, com **overflow para o disco** | ✅ | [08-armazenamento.md](08-armazenamento.md) |
| RF-10 | **Replicação** em vários nós e confirmação por **quórum** | ✅ | [10-publicacao.md](10-publicacao.md) |
| RF-11 | Formação automática do cluster (entrada de nós) | ✅ | [07-cluster.md](07-cluster.md) |
| RF-12 | Detecção de falha, **votação** de exclusão e **freezing** | ✅ | [07-cluster.md](07-cluster.md) |
| RF-13 | Retorno de um nó excluído, com recuperação de mensagens perdidas | ✅ | [07-cluster.md](07-cluster.md) |
| RF-14 | **Drain**: sair do cluster sem perder o que está em memória | ✅ | [07-cluster.md](07-cluster.md) |
| RF-15 | **Modo desenvolvimento**: um nó só, sem cluster | ✅ | [07-cluster.md](07-cluster.md) |
| RF-16 | Autenticação por **token** com permissões e prefixos | ✅ | [14-seguranca.md](14-seguranca.md) |
| RF-17 | **TLS mútuo** entre nós, com certificado do cluster e rotação | ✅ | [14-seguranca.md](14-seguranca.md) |
| RF-18 | Limite de taxa (rate limit) e filtro de IPs de operação | ✅ | [14-seguranca.md](14-seguranca.md) |
| RF-19 | Métricas, saúde e condições; exportação OTLP e Prometheus | ✅ | [13-observabilidade.md](13-observabilidade.md) |
| RF-20 | Linha de comando do servidor (`taba`) e do cliente (`taba-cli`) | ✅ | [20-cli.md](20-cli.md) |
| RF-21 | Conectores de cliente em dez linguagens | 🟡 | Rust e C# prontos; oito planejados ([21-conectores.md](21-conectores.md)) |
| RF-22 | Contrato OpenAPI da API de cliente | ⬜ | [32-roadmap.md](32-roadmap.md) |

### Requisitos não funcionais

| Código | Requisito | Estado | Como é atendido |
|---|---|---|---|
| RNF-01 | **API simples**: poucas rotas HTTP, sem protocolo novo | ✅ | [15-api-visao-geral.md](15-api-visao-geral.md) |
| RNF-02 | **Rapidez**: o caminho crítico passa pela memória; disco só por overflow | ✅ (sem benchmark publicado) | [10-publicacao.md](10-publicacao.md) |
| RNF-03 | **Escalabilidade simples**: nó novo entra sem configuração manual complexa | ✅ | [07-cluster.md](07-cluster.md) |
| RNF-04 | **Configuração simples**: padrões sensatos, tudo por arquivo `.yml` ou `TABA_*` | ✅ | [22-configuracao.md](22-configuracao.md) |
| RNF-05 | **Uma só imagem Docker**, leve | ✅ | [27-ci-cd.md](27-ci-cd.md) |
| RNF-06 | **Sem dependências externas** no servidor (SQLite embutido) e nos conectores | ✅ | [05-dependencias.md](05-dependencias.md) |
| RNF-07 | **Sem perda de mensagem confirmada** quando um nó cai | ✅ (com `replication.factor` ≥ 2) | [11-durabilidade.md](11-durabilidade.md) |
| RNF-08 | **Durabilidade configurável**: o usuário escolhe fator de replicação e quórum | ✅ | [11-durabilidade.md](11-durabilidade.md) |
| RNF-09 | **Conectores em versões antigas** (até 15 anos), desde que falem TLS 1.2 | 🟡 | Só os dois conectores existentes ([21-conectores.md](21-conectores.md)) |
| RNF-10 | **Kubernetes como cidadão de primeira classe** | 🟡 | Sondas e parada com drain existem e há um chart Helm ([34-chart-helm.md](34-chart-helm.md)) testado num cluster kind; ainda não validado num cluster de produção ([32-roadmap.md](32-roadmap.md)) |
| RNF-11 | **Observável** por padrão: OpenTelemetry | ✅ | [13-observabilidade.md](13-observabilidade.md) |

> **Velocidade e custo são metas.** Não há benchmark publicado; o selo ✅ de RNF-02 diz que o desenho coloca a memória no caminho crítico, não que haja uma medição.

## Princípios de design

Estes oito princípios explicam muitas escolhas do código. Cada um aponta para onde se vê o efeito.

1. **Simplicidade primeiro.** Menos conceitos, menos configuração, padrões sensatos.
2. **Velocidade por padrão.** O caminho crítico (publicar, entregar) passa pela memória.
3. **Durabilidade configurável.** O quórum de escrita (`write.quorum`) decide o commit sem esperar todas as réplicas ([11-durabilidade.md](11-durabilidade.md)).
4. **Operação automática.** O cluster se organiza sozinho ([07-cluster.md](07-cluster.md)).
5. **Um protocolo, vários usos.** O mesmo registro + commit em duas fases serve a mensagens (decide o **quórum**) e a mudanças de membros (decidem **todos os nós**).
6. **Falha isolada não vira falha de saúde.** Perder uma mensagem numa sincronização gera um evento; só a falha do nó inteiro leva à exclusão.
7. **Resiliência à instabilidade.** Tentativas antes de votar e o período de freezing evitam reagir a falhas passageiras.
8. **Memória para quem mais usa.** A memória não é dividida igualmente entre tópicos: quem tem mais leitura mantém mais mensagens "quentes".

## Comparação com alternativas

| | **Taba** | SQS + SNS | Kafka | RabbitMQ |
|---|---|---|---|---|
| Onde roda | Qualquer lugar com Docker | Só na AWS | Onde hospedar, ou gerenciado | Onde hospedar, ou gerenciado |
| O que subir | A mesma imagem, 3 nós (1 para dev) | Nada | Brokers e coordenação | Broker (mais nós para alta disponibilidade) |
| Fila e pub/sub | Na mesma API HTTP | Dois serviços | Tópicos e grupos de consumo | Filas e *exchanges* |
| Alta disponibilidade | Embutida: replicação por quorum, votação, volta automática | Gerenciada | Partições replicadas | Quorum queues |
| Escala | Adicione ou retire um nó; ele se rebalanceia sozinho | Gerenciada | Partições redistribuídas à mão ou por ferramentas | Nós; líderes das filas rebalanceados por comando |
| Backup | Contínuo no S3, restaurado ao subir | Gerenciado | Ferramentas externas | Exportação de definições; ferramentas externas |
| Cobrança | Só a infraestrutura | Por requisição | Infraestrutura ou plano | Infraestrutura ou plano |
| Protocolo | HTTP/JSON | HTTP/JSON | Binário próprio | AMQP e outros |

## Estado do projeto

- **Servidor:** nó e cluster implementados no crate `taba-node`; testes de unidade, de um nó e de cluster (rede em memória), mais testes por TCP e TLS reais.
- **Validação de ponta a ponta:** `docker/validate.sh` sobe o Compose do zero e confere a formação do cluster, o `taba-cli`, a queda e a volta de nós e a ausência de perda de mensagens confirmadas.
- **Versão:** publicada como `0.1.x` no Docker Hub, GHCR, crates.io, nuget.org e GitHub Releases; o pipeline calcula o número a partir dos commits ([27-ci-cd.md](27-ci-cd.md)), e o `Cargo.toml` mantém `0.1.0`. Licenças: o servidor é **AGPL-3.0-only** e os conectores (`connectors/`) são **Apache-2.0**, com CLA para contribuições ([30-decisoes.md](30-decisoes.md#licença-e-contribuições-d164)).
- **Prontos além do servidor:** o Helm chart, testado no kind ([34-chart-helm.md](34-chart-helm.md)), o backup opcional no S3 ([33-backup-s3.md](33-backup-s3.md)) e a interface de terminal do `taba-cli` ([20b-cli-ui.md](20b-cli-ui.md)).
- **Faltam:** contrato OpenAPI, os outros oito conectores, validação num cluster Kubernetes real e benchmarks publicados ([32-roadmap.md](32-roadmap.md)).

## Fontes e provas

- Código: `crates/taba-node/src/lib.rs` (mapa de módulos), `Cargo.toml`, `docker-compose.yml`, `docker/validate.sh`.
- Casos de uso: o catálogo está em [26a-casos-de-uso-implementados.md](26a-casos-de-uso-implementados.md); as áreas citadas por requisito aparecem nos documentos de detalhe.
- Histórico: `01-visao-geral.md`, `03-escopo.md`, `07-principios-design.md`.
