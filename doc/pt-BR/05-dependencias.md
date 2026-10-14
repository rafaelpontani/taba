---
título: Dependências
resumo: O que o Taba usa para compilar, rodar e testar — bibliotecas do servidor e por quê, dependências de sistema, ferramentas e a regra de "zero dependências" dos conectores.
tipo: referência
status: estável
verificado-em: 2026-10-14 @ efeb47a
fontes: [Cargo.toml, crates/taba-node/Cargo.toml, connectors/rust/Cargo.toml, connectors/rust/client/Cargo.toml, connectors/csharp, Dockerfile, docker-compose.yml]
---

# Dependências

Há quatro grupos: o **servidor** (o crate `taba-node`), os **conectores**, o **ambiente de execução** (o que a imagem e o cluster precisam) e as **ferramentas de desenvolvimento**.

## 1. O servidor (`taba-node`)

Linguagem **Rust**, edição 2024, runtime assíncrono **tokio**. O workspace tem um único crate. Versões exatas estão em `crates/taba-node/Cargo.toml` e `Cargo.lock`; a tabela abaixo diz **para que** cada uma serve.

| Necessidade | Biblioteca | Onde entra |
|---|---|---|
| Runtime e tarefas assíncronas, canais, relógio | `tokio`, `tokio-util` | Em todo lugar: listas de prontos, filas de saída do canal, tarefas de fundo, `CancellationToken` |
| HTTP (os três listeners) | `axum` (com `ws`), `hyper`, `hyper-util`, `tower`, `tower-http`, `tower-service`, `http-body-util` | Roteadores de `http/`; `tower-http` com `catch-panic` transforma um *panic* num `500` |
| TLS mútuo | `rustls` (provedor `ring`), `tokio-rustls`, `x509-parser`, `rcgen` | Porta do cluster, recarga dos certificados, `taba init` e `taba cert generate`. O nome do host **não** é verificado: a confiança é a do próprio certificado |
| Cliente HTTP entre nós | `reqwest` (só `rustls`) | O `Transport` real chama a porta do cluster dos outros nós |
| WebSocket do canal | `axum` (ws), `tokio-tungstenite`, `futures-util` | Canal permanente entre cada par de nós |
| Backup em S3 | `aws-sdk-s3` e `aws-config` (o SDK oficial) | `storage/backup/store`: o `S3Store` fala com AWS e com MinIO (`backup.s3.endpoint`). Compila no Windows (MSVC) sem ferramentas extras; traz o `aws-lc-rs` (o provedor TLS do próprio SDK, separado do `ring` do resto do nó) |
| Banco local | `rusqlite` com SQLite **embutida** (`bundled`) | `_node.db`, um banco por tópico, banco de eventos de sincronização |
| JSON | `serde`, `serde_json` (`raw_value`) | DTOs das APIs e do canal |
| YAML | `serde_norway` | Arquivo de configuração `.yml` |
| Identificadores | `uuid` (feature `v7`), `getrandom` | GUID v7 de mensagem, tópico, nó e token; segredos aleatórios |
| Hash e codificação | `sha2`, `base64` | Posição no anel (SHA-256), hash salgado do token, corpo em base64 |
| Datas | `time` | RFC 3339 em UTC com milissegundos |
| Erros | `thiserror` | `TabaError` e os erros de cada módulo |
| Redes e IPs | `ipnet` | `operations.allowed-ips`, `client-ip.trusted-proxies` (faixas CIDR) |
| Disco | `fs4` | Espaço livre em disco (`diskFreeBytes`) |
| Logs e traces | `tracing`, `tracing-subscriber` (`json`, `env-filter`), `tracing-opentelemetry` | Formato `text`, `color` ou `json` no `stdout` |
| OpenTelemetry | `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` (`http-proto` e `grpc-tonic`), `opentelemetry-appender-tracing`, `opentelemetry-proto`, `prost`, `tonic` | Traces e logs pelo SDK; **as métricas OTLP são montadas à mão** com `opentelemetry-proto`, porque o SDK não aceita dados agregados de fora |

**Só em testes** (`[dev-dependencies]`): `reqwest` com `json` e `tempfile` (pasta temporária por teste).

### Escolhas que valem explicar

- **SQLite embutida.** O nó não depende de um banco instalado, mas o **build precisa de um compilador C** (MSVC, `gcc` ou `clang`). No alvo `-gnu` do Windows, o `cc` não pode achar o `gcc` do Cygwin.
- **`ring` como provedor TLS** do `rustls`: é instalado como padrão do processo ao abrir o TLS ou o OTLP, porque outra dependência traz o `aws-lc` e os dois não podem coexistir como padrão.
- **Sem biblioteca de linha de comando:** o `taba` e o `taba-cli` leem os argumentos por código próprio.
- **Sem framework de injeção de dependência:** as fronteiras são *traits* (`Transport`, `Clock`), e o `main` monta as peças ([04-arquitetura.md](04-arquitetura.md)).

## 2. Ambiente de execução

| O quê | Para quê | Obrigatório |
|---|---|---|
| **Docker** | Rodar a imagem `taba` (Alpine, binário estático ligado à musl, usuário `taba` de uid 10001) | Não: o binário roda sozinho |
| **Docker Compose** | O cluster de três nós de exemplo | Só para o exemplo |
| **HAProxy** | Balanceador de **exemplo** do Compose (porta 8080 de cliente; descoberta pela porta 8081) | Não: qualquer balanceador serve |
| **Pasta de segredos** | Certificado do cluster e token raiz, criados por `taba init` | Em produção |
| **Pasta de dados** (`storage.dir`) | Os arquivos do nó; **a última pasta do caminho precisa se chamar `taba`** | Sim |
| **Coletor OTLP / Prometheus** | Receber a telemetria | Opcional |

O Taba **não precisa** de banco de dados externo, broker, coordenador (ZooKeeper, etcd) nem serviço de descoberta. A descoberta usa o endereço de um balanceador (`cluster.entry-address`).

## 3. Conectores

**Regra:** conector **sem dependências externas** e capaz de rodar em versões antigas (até 15 anos), desde que a plataforma fale **TLS 1.2 ou superior**.

| Conector | Dependências reais | Observação |
|---|---|---|
| Rust (`taba-client`) | `rustls` (`ring`, `tls12`) e `rustls-native-certs`; `serde` e `serde_json` só com a feature opcional `serde` | HTTP/1.1 e leitor de JSON próprios. Edição 2021 |
| `taba-cli` | `taba-client` | Sem outras |
| C# (`Taba.Client`) | Nenhuma além da plataforma; alvos `net461`, `net5.0`, `net6.0`, `net8.0` e `net10.0` | C# 7.3. A extensão de injeção de dependência (`Taba.Client.Extensions.DependencyInjection`) é um pacote **separado** que usa `Microsoft.Extensions.*` |
| Testes do C# | NUnit 3.14, `Microsoft.NET.Test.Sdk`, `Microsoft.Extensions.*` | Só nos testes |

## 4. Ferramentas de desenvolvimento

| Ferramenta | Para quê |
|---|---|
| **Rust** (estável; `rustup`) | Compilar o servidor e o conector Rust. No Windows com Git Bash, o `link.exe` do coreutils esconde o do MSVC: use `cargo +stable-x86_64-pc-windows-gnu` se o link falhar |
| **.NET SDK 8 ou 10** | Compilar e testar o conector C# |
| **Node.js 22+** | Os scripts de diagramas (`doc/diagramas/scripts`) |
| **Chrome ou Edge** (variável `ARCHIFY_CHROME`) | Exportar os SVG e o GIF do herói do README |
| **Archify** (`~/.claude/skills/archify`, variável `ARCHIFY_BIN`) | Gerar os diagramas a partir do JSON |
| **ffmpeg** (variável `FFMPEG`) | O GIF do herói |
| **GitHub Actions** | CI/CD ([27-ci-cd.md](27-ci-cd.md)) |

Comandos de qualidade do dia a dia estão em [24-convencoes.md](24-convencoes.md).

## Fontes e provas

- `crates/taba-node/Cargo.toml`, `Cargo.toml` (workspace), `connectors/rust/client/Cargo.toml`, `connectors/csharp/*/*.csproj`, `Dockerfile`, `docker-compose.yml`, `AGENTS.md` (seções sobre SQLite e Windows).
- Casos de uso: UC-CNT (conectores), UC-CFG (configuração e validações de partida); veja [26a](26a-casos-de-uso-implementados.md).
- Histórico: `18-estrutura-codigo.md` (17.2), `19-conectores.md`.
