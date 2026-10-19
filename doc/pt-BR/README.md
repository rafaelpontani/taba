---
título: Documentação do Taba (pt-BR)
resumo: Índice da documentação em português, com a ordem de leitura para cada perfil.
tipo: referência
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [README.md, AGENTS.md, crates/taba-node/src/lib.rs]
---

# Taba — documentação (pt-BR)

O Taba é um sistema de mensageria open source, de uma só imagem Docker, que reúne **fila** (cada mensagem para um consumidor) e **distribuição** (cada publicação copiada para várias filas) na mesma API HTTP. Esta documentação descreve **como ele é hoje**, para quem nunca viu o projeto.

> **Como ler os selos de status.** Cada recurso descrito aqui leva uma marca: ✅ implementado (existe código **e** teste), 🟡 parcial (o texto diz o que falta), ⬜ planejado (ainda não existe), ⚠️ limite conhecido (aponta para uma pendência `PRB-…` em [31-pendencias.md](31-pendencias.md)).

> **De onde vem esta documentação.** Os documentos do desenho original foram escritos antes do código, para guiá-lo, e saíram da árvore (estão no histórico do git). Esta pasta foi escrita **depois**, lendo o código. Quando os dois divergem, vale o código.

## Por onde começar

| Você é… | Leia nesta ordem |
|---|---|
| **Quem vai usar** o Taba numa aplicação | [01](01-visao-geral.md) → [02](02-inicio-rapido.md) → [03](03-conceitos.md) → [09](09-topicos.md) → [12](12-entrega.md) → [16](16-api-cliente.md) → [21](21-conectores.md) |
| **Quem vai operar** um cluster | [01](01-visao-geral.md) → [02](02-inicio-rapido.md) → [03](03-conceitos.md) → [07](07-cluster.md) → [22](22-configuracao.md) → [23](23-operacao.md) → [33](33-backup-s3.md) → [34](34-chart-helm.md) → [13](13-observabilidade.md) → [14](14-seguranca.md) |
| **Quem vai contribuir** com o código | [03](03-conceitos.md) → [04](04-arquitetura.md) → [06](06-anel-e-replicacao.md) → [10](10-publicacao.md) → [24](24-convencoes.md) → [25](25-testes.md) → [26](26-casos-de-uso.md) |
| **Uma IA** (agente) que vai ler ou alterar o projeto | [llms.txt](llms.txt) → [03](03-conceitos.md) → [04](04-arquitetura.md) → o documento do assunto → [24](24-convencoes.md) |

## Índice

### Parte A — Entender

| Arquivo | Conteúdo |
|---|---|
| [01-visao-geral.md](01-visao-geral.md) | O que é o Taba, para quem, objetivos e requisitos com o estado de cada um, o que ele não é |
| [02-inicio-rapido.md](02-inicio-rapido.md) | Subir o cluster de três nós, publicar e consumir, derrubar um nó |
| [03-conceitos.md](03-conceitos.md) | O modelo mental em uma página: nó, cluster, tópico, mensagem, bloco, anel, quórum |

### Parte B — Como funciona por dentro

| Arquivo | Conteúdo |
|---|---|
| [04-arquitetura.md](04-arquitetura.md) | Camadas do nó, as três portas, composição, mapa de módulos |
| [05-dependencias.md](05-dependencias.md) | Bibliotecas, ferramentas e dependências de sistema, e por quê |
| [06-anel-e-replicacao.md](06-anel-e-replicacao.md) | Hash circular, blocos, papéis primário/secundário/terciário |
| [07-cluster.md](07-cluster.md) | Entrada, falha, votação, freezing, retorno, drain, sincronização, modo desenvolvimento |
| [08-armazenamento.md](08-armazenamento.md) | Memória, overflow em disco, SQLite, reinício |

### Parte C — Especificação funcional

| Arquivo | Conteúdo |
|---|---|
| [09-topicos.md](09-topicos.md) | Fila, distribuidor, fila volátil, FIFO, dead-letter |
| [10-publicacao.md](10-publicacao.md) | O caminho de uma mensagem publicada |
| [11-durabilidade.md](11-durabilidade.md) | O que é garantido, o que não é, e em que cenário |
| [12-entrega.md](12-entrega.md) | Consumo, long polling, invisibilidade, ack, limite de entregas, grupos |
| [13-observabilidade.md](13-observabilidade.md) | Métricas, saúde, condições, traces, logs |
| [14-seguranca.md](14-seguranca.md) | Tokens, TLS mútuo do cluster, rate limit, IP do cliente |

### Parte D — Interfaces

| Arquivo | Conteúdo |
|---|---|
| [15-api-visao-geral.md](15-api-visao-geral.md) | As três APIs e as convenções comuns |
| [16-api-cliente.md](16-api-cliente.md) | Tópicos, mensagens e tokens, rota a rota |
| [17-api-operacao.md](17-api-operacao.md) | Saúde, métricas e drain |
| [18-api-cluster.md](18-api-cluster.md) | Rotas entre nós e o canal permanente |
| [19-erros-e-limites.md](19-erros-e-limites.md) | Catálogo de erros e limites |
| [20-cli.md](20-cli.md) | `taba` (servidor) e `taba-cli` (cliente) |
| [20b-cli-ui.md](20b-cli-ui.md) | A interface em tela cheia (`taba-cli tui`): abrir, configurar, telas e teclas |
| [21-conectores.md](21-conectores.md) | Bibliotecas de cliente e o contrato comum |

### Parte E — Operar

| Arquivo | Conteúdo |
|---|---|
| [22-configuracao.md](22-configuracao.md) | Todos os parâmetros e padrões |
| [23-operacao.md](23-operacao.md) | Runbook: subir, escalar, retirar, diagnosticar, backup, parar e iniciar |
| [33-backup-s3.md](33-backup-s3.md) | Backup opcional em S3: o que vai ao bucket, restauração, evicção, flush, parada |
| [34-chart-helm.md](34-chart-helm.md) | Chart Helm: o que cria, todos os values (Ingress, MinIO/S3, coletor Prometheus/Telegraf, OTLP, KEDA) |
| [35-chart-helm-casos-de-uso.md](35-chart-helm-casos-de-uso.md) | Chart Helm: modos de operação e os testes em bash de cada um |

### Parte F — Contribuir

| Arquivo | Conteúdo |
|---|---|
| [24-convencoes.md](24-convencoes.md) | Código, API, pastas, idioma |
| [25-testes.md](25-testes.md) | Níveis de teste e como escrever um |
| [36-problemas-conhecidos.md](36-problemas-conhecidos.md) | Testes desligados por falharem de forma intermitente no CI |
| [26-casos-de-uso.md](26-casos-de-uso.md) | O formato `UC-…` e a regra de cobertura |
| [26a-casos-de-uso-implementados.md](26a-casos-de-uso-implementados.md) | Casos com teste |
| [26b-casos-de-uso-planejados.md](26b-casos-de-uso-planejados.md) | Casos sem teste ainda |
| [27-ci-cd.md](27-ci-cd.md) | Workflows, imagens e releases |

### Parte G — Referência e projeto

| Arquivo | Conteúdo |
|---|---|
| [28-glossario.md](28-glossario.md) | Termos do projeto |
| [29-mapa-mental.md](29-mapa-mental.md) | O projeto em um mapa |
| [30-decisoes.md](30-decisoes.md) | Por que cada coisa é como é |
| [31-pendencias.md](31-pendencias.md) | O que ainda está em aberto |
| [32-roadmap.md](32-roadmap.md) | O que vem a seguir |

## Convenções desta documentação

- Esta pasta tem uma **versão espelho em inglês**, [`../en-US/`](../en-US/README.md), com os mesmos arquivos (o prefixo `NN-` é a identidade; o resto do nome está em inglês). **Toda mudança aqui deve ser aplicada também lá** ([24-convencoes.md](24-convencoes.md#documentação-em-vários-idiomas)).
- Texto em português do Brasil. Identificadores (rotas, chaves de configuração, campos, códigos de erro) ficam em inglês, em `código`.
- Um nome como `05-particionamento-replicacao.md`, sem pasta, é um documento do desenho original (escrito antes do código), **removido** da árvore: está no histórico do git, no commit `71183b9` e anteriores. Onde ele e o código divergem, vale o código.
- Todo arquivo começa com metadados (`resumo`, `status`, `verificado-em`, `fontes`) e termina com **Fontes e provas**: os caminhos no código e os casos de uso `UC-…` que comprovam o que foi dito.
- Os diagramas são imagens SVG geradas de JSON em `doc/diagrams/source/` (o índice está em [24-convencoes.md](24-convencoes.md#índice-dos-diagramas)); o SVG é a saída, o JSON é a fonte.
- Links entre arquivos usam o nome do arquivo; não há numeração de seções contínua entre arquivos como na documentação histórica.
