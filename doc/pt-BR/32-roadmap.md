---
título: Roadmap
resumo: O que já está pronto (a versão 0.1) e o que vem a seguir, em marcos — versionamento do contrato de cliente, demais conectores, publicação, Kubernetes, operação — com as pendências que cada marco fecha.
tipo: referência
status: parcial
verificado-em: 2026-10-14 @ efeb47a
fontes: [connectors/README.md, doc/pt-BR/30-decisoes.md, doc/pt-BR/31-pendencias.md]
---

# Roadmap

> Este é o **plano de próximos passos**, só com o que está **aberto** (o feito está resumido em "Onde estamos"). As dúvidas e problemas citados estão em [31-pendencias.md](31-pendencias.md); o que já foi decidido, em [30-decisoes.md](30-decisoes.md). Regra do projeto: ao fechar uma pendência, apague a entrada e a referência aqui.

## Onde estamos: a versão 0.1

A **0.1 fecha** quando uma pessoa sobe um cluster de três nós com um comando, usa a API pelo `taba-cli` num terminal e por um aplicativo de console em C#. Os cinco primeiros passos a fecharam (um sexto, o contrato OpenAPI, veio depois); todos estão **feitos**:

| Passo | Estado |
|---|---|
| 1 — `taba-cli`, o primeiro conector (comandos, perfis, TUI, imagem Docker, binários para 6 plataformas) | ✅ |
| 2 — Docker e Compose (`Dockerfile`, `docker-compose.yml` de três nós, HAProxy de exemplo, `taba init`) | ✅ |
| 3 — Conector C# (nove operações, `subscribe`, injeção de dependência, testes unitários e de integração) | ✅ |
| 4 — Exemplo de console em C# (`examples/csharp-console/`) | ✅ |
| 5 — Validação de ponta a ponta (`docker/validate.sh`: cluster, `taba-cli`, queda e volta de nós, sem perda de mensagem) | ✅ |
| 6 — Contrato OpenAPI (APIs de cliente e de monitoramento), gerado das rotas, servido por cada nó com uma interface em `/docs` e publicado em `openapi/` | ✅ |

Também está pronto o **servidor** inteiro (nó, cluster, FIFO, volátil, distribuidor, tokens, mTLS, observabilidade) e o **CI/CD** com versionamento automático ([27-ci-cd.md](27-ci-cd.md)).

> ⚠️ **Ressalvas da 0.1** (achadas ao documentar, ver [31-pendencias.md](31-pendencias.md) "Descobertas"): parar o cluster **inteiro** perde mensagens e tópicos; perder **dois nós seguidos** trava o cluster; `scope=cluster` das métricas de tópico não agrega entre nós; sete parâmetros de configuração não têm efeito.

## Marcos

A ordem é uma **proposta**; os marcos 1 e 2 não dependem um do outro.

### Marco 1 — Contrato da API de cliente

| Item | Detalhe | Fecha |
|---|---|---|
| **Política de versionamento nos caminhos** ⬜ | Os documentos OpenAPI existem ([15](15-api-visao-geral.md)); falta decidir a política do prefixo (hoje não há `/v1`) **antes** de os outros conectores fixarem os caminhos | RF-22 |
| **Suíte de conformidade** 🟡 | `connectors/contract/` ✅: os cenários (`core.scn` e os entre linguagens), os drivers dos cinco conectores e um Taba de um nó no Docker ([21-conectores.md](21-conectores.md#os-testes-de-contrato), D183). Faltam ⬜: as fixtures (pares requisição/resposta) e os documentos de teste do leitor de JSON | UC-CNT (nível `K`) |

### Marco 2 — Os demais conectores

Em fases ([21-conectores.md](21-conectores.md)); as fases 2, 3 e 4 **não dependem entre si**, só da 1:

| Fase | Linguagens | Observação |
|---|---|---|
| 1 — Referência | **Python** (a mais legível) e **Java** (a base mais antiga) ✅ | Estressam o desenho nas duas pontas. Prontos (D68.2) com os testes de contrato (D183); falta a publicação (PRB-079) |
| 2 — Núcleo C | **C** (`libtaba`) e **C++** (cabeçalho C++03 sobre a `libtaba`) | |
| 3 — Plataformas | **Node.js** ✅ e **Go** | Node.js pronto (D68.2) |
| 4 — Restantes | **PHP** e **Ruby**; no **Rust**, a forma `async` | |
| 5 — Integração | Os cenários passam em **todos** contra o Taba real ✅ (`connectors/contract/run.sh`, D183); exemplos de uso preenchem a tabela do README ⬜ | Depende do marco 1 |

Decisões que precisam estar fechadas: **D68** (versões-base das linguagens ainda por fazer), **D70.1** e **D70.2** (nomes de organização e pacote; espelho do PHP; versões independentes).

### Marco 3 — Publicação

| Item | Detalhe |
|---|---|
| Pacotes 🟡 | nuget.org (`Taba.*`) e crates.io (`taba-client`, `taba-cli`) automatizados no `release.yml` por Trusted Publishing, ligados pelas variáveis `PUBLISH_CRATES` e `NUGET_USER` ([27-ci-cd.md](27-ci-cd.md#publicação-e-metadados)); faltam os registros das outras linguagens. **Não podem ser apagados** |
| Imagens ✅ | Docker Hub e GHCR, com o mesmo digest em todas as tags ([27-ci-cd.md](27-ci-cd.md#tags-docker-e-tags-flutuantes)) |
| Repositório público ⬜ | Hoje privado; a publicação definitiva é uma decisão do dono |

### Marco 4 — Operação em produção

| Item | Detalhe | Fecha |
|---|---|---|
| **Kubernetes** 🟡 | Chart Helm pronto e testado em kind ([34-chart-helm.md](34-chart-helm.md)); falta a validação em um cluster de verdade (sondas, parada com drain, `Service` interno da porta do cluster) e publicar o chart | RNF-10 |
| **Exclusão forçada** ⬜ | Rota manual de exclusão para destravar um cluster em `quorum-lost` (hoje a saída é reiniciar tudo; a retomada do cluster restaura o que estava em disco); ver "Evolução futura" do protocolo de cluster | Descoberta C |
| **ID estável entre vidas** ⬜ | Reaproveitar o ID de um nó que volta para ressincronizar só o delta, sem trazer de volta o nó desatualizado (D176) | Retomada do cluster |
| **Escrita em disco no commit** ⬜ | Gravar em disco ao confirmar, para preservar também o que estava só em memória numa queda total (D176) | Retomada do cluster |
| **Backup e restauração** 🟡 | ✅ Feito: backup opcional em S3 (segmentos, diário, definições, batimento, restauração, evicção, flush, parada do cluster; [33-backup-s3.md](33-backup-s3.md)). ⬜ Falta: compactar o diário (PRB-053), recuperar mais cedo os segmentos de um primário que deixou de sê-lo (PRB-054), restaurar sem ler os segmentos inteiros (PRB-055), apagar do bucket definições removidas com o gestor fora do ar (PRB-056), validar contra um MinIO/AWS reais (o teste de hoje usa um S3 de mentira em HTTP) e medir a latência de publicação com e sem S3 | Descoberta J |
| **Agregação entre nós das métricas de tópico** ⬜ | A rota interna e o `fan-out` (com `stats.topics.cache-ttl`) | Descoberta A |
| **Benchmarks** 🟡 | ✅ Feito: o script comparativo local `bench/run.sh` (OMB em Docker contra Kafka, Redpanda, RabbitMQ e NATS JetStream; [38-benchmarks.md](38-benchmarks.md)). ⬜ Falta: uma rodada `standard` publicada, a medição do cluster de três nós e o benchmark de regressão no CI | RNF-02 |

### Marco 5 — Fechar as pendências de calibragem

As pendências "sem bloqueio" de [31-pendencias.md](31-pendencias.md) são, em sua maioria, **calibragem** ou decisões pequenas:

| Grupo | Itens |
|---|---|
| Padrões de configuração | **D84** (rate limit), **D90** (limiares das condições de tópico), **D91** (métricas de tópico em `/metrics`), **D89** (teto da deduplicação FIFO), **D87** (teto de filas voláteis), **D85** |
| Memória do índice de entrega | **D88**, **PRB-020**: falta o padrão de `delivery.fair.max-groups` e o teto na fila FIFO e na tabela de réplicas |
| Drain | **D75** (drain pela API que estoura o prazo), **D77**, **PRB-005** (recusa o ack em drain) |
| Segurança | **D76** (filtro de IPs vs. health checks), **PRB-015** (`X-Forwarded-For` forjável com a lista vazia), **PRB-017** |
| Cluster | **D93**, **D99**, **D102**, **D104**, **D109**, **D120**, **PRB-051** (IP de saída) |
| Observabilidade | **D117**, **D118**, **PRB-026** (métricas de processo só no Linux), **PRB-050** (limites do OTLP), **PRB-025**, **PRB-039** |
| Portabilidade | **D80** (nomes reservados do Windows), **PRB-008** |

### Marco 6 — Documentação e repositório

| Item | Detalhe |
|---|---|
| **Idioma da raiz** ✅ | `README.md` e `AGENTS.md` em **en-US**, apontando para `doc/README.md` (que lista a documentação de cada idioma) |
| **`uc_coverage` lendo os catálogos novos** ✅ | Lê os cartões de `26a`/`26b`; falta um gerador que os mantenha ([31-pendencias.md](31-pendencias.md), item E) |
| **Diagramas desta documentação** ✅ | Os 31 pontos estão desenhados (índice em [24-convencoes.md](24-convencoes.md#índice-dos-diagramas)); a fonte é o JSON do Archify |
| **Verificação automática** ⬜ | `node doc/pt-BR/scripts/verificar.mjs` e `verificar-idiomas.mjs` num job de PR ([27-ci-cd.md](27-ci-cd.md)) |
| **Job de integração no CI** 🟡 | Os testes de contrato dos conectores rodam no `ci.yml` em todo pull request para `release` ✅; `connectors/csharp/scripts/integration.sh` e `validate.sh` continuam só à mão ⬜ |
| **Traduções** 🟡 | ✅ `doc/en-US/` espelha `doc/pt-BR/`. ⬜ Outras pastas de idioma ao lado delas; toda mudança vale para todas ([24-convencoes.md](24-convencoes.md#documentação-em-vários-idiomas)) |

## Fora do escopo (por enquanto)

Interface gráfica de administração; autenticação avançada (OAuth/OIDC, política por operação); protocolos AMQP, MQTT e Kafka; processamento de streams e reprocessamento do histórico; **multi-região**. Itens fora do escopo podem ser reavaliados.

## Fontes e provas

- `doc/08c-proximos-passos.md` (os passos da 0.1 e o "depois"), `doc/08b-pendencias.md` (as pendências por marco), `connectors/README.md` (o estado dos conectores), `21-pipeline-ci-cd.md` e `.github/` (o que falta no CI).
- Os estados ✅/⬜ foram conferidos contra o repositório (existência de pastas, de workflows, de manifestos) e contra execuções reais descritas em [23-operacao.md](23-operacao.md).
