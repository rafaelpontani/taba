# Conectores por linguagem

[← Índice](README.md) · [Conectores](19-conectores.md)

## 18.10 Versão-base e particularidades de cada linguagem

O desenho geral (pastas, superfície, contrato, regras de código e plano) está em [19-conectores.md](19-conectores.md). Este arquivo traz, **por linguagem**, a versão mais antiga suportada (a **versão-base**), o que muda por causa dela e o que o conector usa para HTTP, JSON e base64.

> As versões-base são **propostas** e a confirmação está em D68 ([08b-pendencias.md](08b-pendencias.md)). As regras que as determinam já estão decididas: TLS 1.2 (D69), `PATCH` (D72) e assíncrono (D71).

### 18.10.1 Como a versão-base foi escolhida

A janela continua sendo de **15 anos** (de 2011 em diante), mas ela é só o **teto**: o piso real vem de três regras, e a versão-base é a **mais antiga** de cada linguagem que cumpre as três.

1. **TLS 1.2 ou superior (D69).** A linguagem, com a pilha de TLS da plataforma, fala TLS 1.2 ou 1.3 e **confere o certificado** por padrão. Plataformas que só falam TLS 1.0 ou 1.1 **não entram**: o Taba não abre porta HTTP simples nem aceita TLS antigo para elas, e o conector não traz uma pilha de TLS própria (a exceção é o Rust, na tabela abaixo).
2. **Todos os verbos da superfície (D72).** O cliente HTTP da biblioteca-padrão faz `GET`, `POST`, `PUT`, `PATCH` e `DELETE`. Linguagem cujo cliente HTTP não faz `PATCH` **não é suportada** nessa versão, e o Taba não aceita `X-HTTP-Method-Override`: a base sobe até uma versão que faz.
3. **Assíncrono onde existe (D71).** Onde a linguagem tem um modelo assíncrono, o conector o oferece (`Task`, `CompletableFuture`, `Promise`, `async`/`await`, `context`). Não é critério de piso: a base não sobe só por causa dele, e o assíncrono fica no código das versões que o têm. Onde a linguagem não tem (C, C++, PHP, Ruby), o conector é só síncrono.

| Linguagem | Pasta | Versão-base | Lançada | Por quê | Também testada em |
|---|---|---|---|---|---|
| **Java** | `connectors/java/` | **Java 11** (LTS) | 2018 | O `HttpURLConnection` **nunca** fez `PATCH` (ele recusa o verbo em qualquer versão); o `java.net.http.HttpClient` do 11 faz, tem TLS 1.2 e 1.3 por padrão e é assíncrono. O Java 8 ficou de fora por causa do `PATCH`. | 17 (LTS), a LTS mais recente |
| **C#** | `connectors/csharp/` | Cinco alvos: **.NET Framework 4.6.1** (clássico, `net461`) e o moderno: **.NET 5** (`net5.0`), **6**, **8** e **10** | 2015 / 2020 | `HttpClient` com `Task`, `async`/`await`, `PATCH` por `HttpMethod("PATCH")` e TLS 1.2 nos dois. O .NET 5 é o piso da linha moderna (o .NET Core 1.x a 3.x saíram). | .NET Framework 4.8; .NET 8 e a mais recente |
| **Python** | `connectors/python/` | **2.7.9** e **3.4.3** | 2014 / 2015 | São as primeiras versões de cada linha que conferem o certificado HTTPS por padrão (PEP 476) e falam TLS 1.2. O 2.6 e o 2.7 antigo saíram. | 2.7.18, 3.5 a 3.13 |
| **PHP** | `connectors/php/` | **5.6** | 2014 | TLS 1.2 e verificação de certificado ligada por padrão nos *streams*; `curl` faz `PATCH`. O 5.3 a 5.5 saíram. | 7.4, 8.x |
| **Ruby** | `connectors/ruby/` | **2.0** | 2013 | `Net::HTTP::Patch` e `json` na biblioteca-padrão, `Net::HTTP` com certificado conferido por padrão e TLS 1.2 (com o OpenSSL 1.0.1 ou posterior). O 1.8.7 e o 1.9 saíram. | 2.7, 3.x |
| **C** | `connectors/c/` | **C89** (ANSI) | 1989 | A linguagem não limita o TLS: quem limita é a `libcurl`, e a **7.34** (dezembro de 2013) é a primeira que exige TLS 1.2 como mínimo (`CURL_SSLVERSION_TLSv1_2`) e faz `PATCH` (`CURLOPT_CUSTOMREQUEST`). | C99, C11, a mais recente |
| **C++** | `connectors/cpp/` | **C++03** | 2003 | Embrulha a `libtaba`, com a mesma exigência de `libcurl` 7.34. | C++11 a C++20 |
| **Node.js** | `connectors/node/` | **0.12** | 2015 | `Promise` nativa e TLS 1.2 por padrão (o 0.10 negociava só TLS 1.0 por padrão); `https.request` faz qualquer verbo. | 4, 12, a LTS mais recente |
| **Go** | `connectors/go/` | **1.7** | 2016 | `context.Context` na biblioteca-padrão (o jeito de Go de cancelar chamadas e de ser assíncrono), `net/http` com `PATCH` e TLS 1.2. Não há Go de 2011 (o 1.0 é de 2012). | 1.13, a mais recente |
| **Rust** | `connectors/rust/client/` | **1.70** (edição 2021) | 2023 | Não há Rust de 2011 (a 1.0 é de 2015), e o TLS 1.2 **não está na biblioteca-padrão**: o conector usa o `rustls`, e a base é a versão mínima que ele (e o `tokio`, no assíncrono) pedem. **Estimativa:** fecha com o protótipo, olhando o `rust-version` das dependências fixadas. | a estável mais recente |

Nenhuma dessas versões recebe correção de segurança de todos os fornecedores. O conector as **suporta**, mas o README de cada pasta avisa que rodar nelas é responsabilidade de quem opera o ambiente.

**O que isso muda em relação à proposta anterior:**

| Linguagem | Antes | Agora | Motivo |
|---|---|---|---|
| Java | 6 | **11** | O `HttpURLConnection` nunca fez `PATCH`; o `java.net.http` do 11 faz e é assíncrono |
| C# | .NET 3.5 | **.NET Framework 4.6.1** (clássico) e **.NET 5, 6, 8 e 10** (moderno) | `HttpClient`, `Task` e TLS 1.2; o SDK 10 compila todos (o SDK 8 compila todos menos o `net10.0`) |
| Python | 2.6 | **2.7.9 e 3.4.3** | Primeiras que conferem o certificado HTTPS por padrão e falam TLS 1.2 |
| PHP | 5.3 | **5.6** | TLS 1.2 e verificação do certificado nos *streams* |
| Ruby | 1.8.7 | **2.0** | `json` e `Net::HTTP::Patch` na biblioteca-padrão |
| Node | 0.10 | **0.12** | `Promise` nativa e TLS 1.2 por padrão |
| Go | 1.4 | **1.7** | `context` na biblioteca-padrão, sem arquivos duplicados por versão |
| Rust | 1.31 | **1.70** (estimativa) | O TLS exige o `rustls`, e a base é o mínimo dele e do `tokio` |
| C / C++ | C89 / C++03 | **iguais**, com `libcurl` 7.34 (era 7.19) | A linguagem não limita o TLS; quem limita é a `libcurl` |

### 18.10.2 Quadro de tecnologia

O que cada conector usa, na versão-base:

| Linguagem | HTTP | JSON | Base64 | Assíncrono | Teste (unitário) |
|---|---|---|---|---|---|
| Java 11 | `java.net.http.HttpClient` (HTTP/1.1) | Leitor mínimo (seção 18.5) | `java.util.Base64` | `CompletableFuture` (`...Async`) | JUnit 5 |
| C# (.NET 4.6.1 e .NET 5) | `HttpClient` | Leitor mínimo | `Convert.ToBase64String` | `Task` (`...Async`), com `async`/`await` | NUnit 3 |
| Python 2.7.9 / 3.4.3 | `httplib` / `http.client` | `json` da biblioteca-padrão | `base64` | `taba.aio` (`async`/`await`, no 3.5+) | `unittest` |
| PHP 5.6 | `curl` (com *fallback* para *streams*) | `json_decode` | `base64_decode` | — | PHPUnit 5.7 |
| Ruby 2.0 | `Net::HTTP` | `json` da biblioteca-padrão | `Base64` (`pack('m')`) | — | `minitest` |
| C89 | `libcurl` 7.34 | Leitor mínimo | Próprio (~30 linhas) | — | Teste em C simples, sem estrutura externa |
| C++03 | via `libtaba` | via `libtaba` | via `libtaba` | — | O mesmo teste em C, mais um de exceções |
| Node 0.12 | `http` / `https` | `JSON` | `Buffer` | `Promise` (a API) | `assert` + um *runner* de 30 linhas |
| Go 1.7 | `net/http` | `encoding/json` | `encoding/base64` | `context.Context` + *goroutines* | `testing` |
| Rust 1.70 | HTTP/1.1 próprio sobre `std::net::TcpStream`, com `rustls` | Leitor mínimo | Próprio (~30 linhas) | *feature* `async` (`tokio` + `tokio-rustls`) | `#[test]` |

Nas linhas com "Leitor mínimo", o JSON todo é lido por ele. Nas com a biblioteca da linguagem, o `body` continua sendo extraído como **trecho de texto** pelo leitor mínimo, para entregar o JSON qualquer sem reserializar (seção 18.5).

**O assíncrono é uma segunda forma de chamar as mesmas nove operações**, com a mesma montagem de requisição e a mesma leitura de resposta (o núcleo não depende de entrada e saída). Ele não muda o contrato: a repetição de chamadas, o tempo limite do `consume` e os erros são os mesmos da seção 18.4.

### 18.10.3 Particularidades

#### Java (`connectors/java/`)

- **Java 11:** `var`, `java.util.Base64` e `java.net.http` estão disponíveis; lambdas e `try-with-resources` também. O código compila com `--release 11`.
- **`HttpClient`:** configurado em HTTP/1.1 (sem HTTP/2, seção 18.4). O `connectTimeout` vai no cliente e o tempo da chamada no `HttpRequest.timeout` (`waitTime + 10 s` no `consume`). O `PATCH` sai por `method("PATCH", ...)`.
- **Assíncrono:** cada operação tem a versão `...Async`, que devolve `CompletableFuture<T>` por cima do `sendAsync`; a síncrona espera a assíncrona.
- **Sem `javax.xml.bind`:** o JAXB saiu do JDK 11. O conector não o usa.
- **Pacote:** artefato `taba-client`. O *groupId* e o nome do pacote Java dependem da organização que publica (D70.1).

#### C# (`connectors/csharp/`)

- **Cinco alvos num mesmo projeto:** `net461` (**.NET clássico**: o Framework 4.6.1 a 4.8) e, no **.NET moderno**, `net5.0`, `net6.0`, `net8.0` e `net10.0` (D137; o `net10.0` só entra quando o SDK é o 10 ou mais novo, por uma condição no `.csproj`). O código é o mesmo, com `#if` só onde as APIs diferem. A linguagem fica no C# 7.3 (`LangVersion`) nos dois, para não divergir.
- **Um só SDK compila tudo:** o **SDK do .NET 8 ou do .NET 10** (decidido, D68.1) gera os alvos (o `net461` com os *reference assemblies* do pacote `Microsoft.NETFramework.ReferenceAssemblies`, os modernos com o *targeting pack* baixado do NuGet; o SDK 8 não gera o `net10.0`), sem instalar SDKs antigos no CI. O SDK avisa que o .NET 5 está fora de suporte; o aviso é suprimido (`NETSDK1138`). Os **testes** rodam no .NET 6, 8 e 10 (cada um contra o alvo de mesma versão) e, quando houver um Windows no CI, no Framework 4.8.
- **Assíncrono primeiro:** os métodos `...Async` (com `Task`) são a implementação, e os síncronos esperam por eles.
- **`PATCH`:** `new HttpMethod("PATCH")` no `HttpRequestMessage`.
- **TLS 1.2:** no .NET Framework 4.6.x, o protocolo usado é o do `ServicePointManager.SecurityProtocol`, e a partir do 4.7 é o do sistema. O conector **não** o altera sozinho; o README diz como conferir.
- **Pacote:** `Taba.Client` (NuGet), com o *namespace* `Taba`.

#### Python (`connectors/python/`)

- **Um código para Python 2 e 3**, sem dependências: `try: import httplib except ImportError: import http.client as httplib`. Arquivos: `taba/client.py` e o leitor do `body`.
- **Proibido na base (2.7.9 e 3.4.3):** *f-strings*, anotações de tipo, `yield from`, `nonlocal`, `async`/`await` (fora do `aio.py`) e `unittest.mock` no Python 2.
- **Assíncrono (`taba/aio.py`):** só no Python 3.5 ou superior, importado à parte. A biblioteca-padrão **não tem** cliente HTTP assíncrono, então o `aio` roda o cliente síncrono em `loop.run_in_executor` e expõe `async def` para as nove operações. O pip no Python 2 pode avisar de erro de sintaxe ao compilar esse arquivo; a instalação segue.
- **HTTPS** confere o certificado por padrão nas duas bases. O conector não desliga a verificação.
- **Pacote:** `taba` (PyPI), *wheel* universal `py2.py3-none-any`.

#### PHP (`connectors/php/`)

- **PHP 5.6:** *namespaces*, *closures*, `[]` para listas, *traits* e `finally` existem; **não** há tipos escalares, `??` nem `declare(strict_types=1)` (7.0). A classe é `Taba\Client`.
- **HTTP:** a extensão `curl` (padrão na maioria das instalações); se não existir, *streams* (`file_get_contents` com `stream_context_create`, com `ignore_errors`). As duas ficam atrás de uma interface `Transport`, que a aplicação também pode trocar.
- **Sem assíncrono:** o PHP não tem modelo assíncrono na biblioteca-padrão.
- **Instalação:** Composer (`taba/taba-client`) ou um arquivo único para copiar (`taba.php`), útil nas hospedagens sem Composer. **Autoload PSR-4** na instalação por Composer; o arquivo único dispensa o autoload.
- **Packagist** pede o `composer.json` na raiz do repositório: a pasta vira um espelho somente leitura (D70.2).

#### Ruby (`connectors/ruby/`)

- **Ruby 2.0:** `{a: 1}`, `->` e argumentos nomeados existem; **não** há argumentos nomeados obrigatórios (2.1), `&.` e `Array#dig` (2.3).
- **`PATCH`:** `Net::HTTP::Patch`. **JSON:** a biblioteca `json` da biblioteca-padrão (o leitor mínimo extrai o `body`).
- **Texto:** as cadeias já têm *encoding*; o conector força UTF-8 de forma explícita.
- **Sem assíncrono:** o Ruby não tem modelo assíncrono na biblioteca-padrão; quem precisa usa *threads*.
- **Pacote:** a *gem* `taba-client`, com o módulo `Taba`.

#### C (`connectors/c/`)

- **C89:** declarações no começo do bloco, comentários `/* */`, sem `//`, sem `stdint.h` (usa `long` e `unsigned long`), sem `bool` e sem `snprintf` (no MSVC antigo, `_snprintf`; o conector embrulha numa função).
- **`libcurl`** é a única dependência, por *link* dinâmico, versão **7.34 ou posterior**, com um *backend* de TLS que fale TLS 1.2 (OpenSSL 1.0.1 ou posterior, NSS, Schannel). O conector pede `CURL_SSLVERSION_TLSv1_2` como mínimo. O *handle* do curl não é seguro entre *threads*: **um cliente por *thread***.
- **Interface:** `taba_client_new(config)`, as nove operações da seção 18.3 e `taba_free_*` para cada resultado. Todo resultado de texto ou de corpo é do chamador, liberado com a função própria da biblioteca (que também resolve o problema de misturar `malloc` de DLLs diferentes no Windows).
- **Sem assíncrono:** a `libtaba` bloqueia; quem quer concorrência usa *threads* (um cliente por *thread*).
- **Build:** `Makefile` simples e `CMakeLists.txt`. Gera `libtaba.a` e `libtaba.so`/`taba.dll`.
- **Um transporte pluggável:** um ponteiro de função (`taba_http_fn`) substitui a `libcurl` para quem tem o próprio cliente HTTP (sistemas embarcados), que então responde pelo TLS 1.2.

#### C++ (`connectors/cpp/`)

- **C++03:** sem `auto`, `nullptr`, `std::unique_ptr`, *lambdas* nem `std::string_view`. Usa `std::string`, `std::vector`, `std::map` e exceções (`taba::Error`, que herda de `std::runtime_error`).
- **Um cabeçalho** (`taba.hpp`) sobre a `libtaba`. A aplicação faz o *link* com a `libtaba`, e o cabeçalho só dá a interface tipada e o fechamento automático (RAII).
- **Sem assíncrono:** o C++03 não tem. Quem usa C++11 ou posterior chama o cliente dentro de `std::async` ou de uma *thread*, com um cliente por *thread*.
- Seguro entre *threads* **só** com um cliente por *thread* (a `libcurl` por baixo).

#### Node.js (`connectors/node/`)

- **ES5:** sem `let`, `const`, *arrow functions* nem classes (a base é o 0.12). O conector usa `var`, `function` e protótipos.
- **API de `Promise`:** as operações devolvem uma `Promise`, que a aplicação usa com `.then` ou com `async`/`await` (Node 7.6 ou posterior). Não há API de *callback*.
- **`Buffer`:** `Buffer.from` não existe no 0.12 e `new Buffer` está obsoleto nas versões novas. O conector escolhe pelo teste `typeof Buffer.from === 'function'`.
- **Pacote:** `taba-client` (npm), com `index.d.ts` opcional para quem usa TypeScript.

#### Go (`connectors/go/`)

- **`context` desde a base** (entrou no Go 1.7): todo método recebe o `ctx context.Context` como primeiro argumento, e o cancelamento e o prazo são os do contexto. Não há `// +build` nem variantes duplicadas.
- **`PATCH`:** `http.MethodPatch`. A concorrência é a do Go: o `Client` é seguro entre *goroutines* (o `http.Client` do Go é).
- **Módulos:** a pasta vale em `GOPATH` (Go 1.7) e também leva um `go.mod`, que as versões antigas ignoram. Como o módulo fica numa subpasta, a tag segue o formato `connectors/go/v1.2.0`.

#### Rust (`connectors/rust/client/`)

- **Núcleo sem entrada e saída:** montar a requisição HTTP/1.1 (linha, cabeçalhos, `Content-Length`) e interpretar a resposta (linha de status, cabeçalhos, `Content-Length` e `chunked`) são funções puras, testadas com as fixtures. As duas formas de chamar só acrescentam o transporte.
- **Síncrono (base):** bloqueante, sobre `std::net::TcpStream` (com `set_read_timeout`/`set_write_timeout`) e o `rustls` para o HTTPS.
- **Assíncrono (*feature* `async`):** o mesmo núcleo sobre `tokio::net::TcpStream` e `tokio-rustls`, com `async fn` para as nove operações.
- **Dependências:** `rustls` (TLS 1.2 e 1.3, sem OpenSSL), uma fonte de certificados raiz e, só no `async`, o `tokio`. É a **única exceção** à regra "sem dependências" fora o `libcurl` do C (seção 18.5). Um *trait* `Transport` deixa a aplicação trocar a pilha.
- **`taba-protocol`:** não é usado (seção 18.2).

### 18.10.4 E as outras linguagens?

Quem usa Perl, Kotlin, Scala, Dart, Swift, Elixir, Lua e outras tem duas saídas, sem conector próprio:

- **Linguagens da JVM e .NET** usam direto os conectores Java e C# (Kotlin e Scala chamam a classe Java; F# e VB.NET, a de C#).
- **Qualquer linguagem com FFI para C** (Perl, Lua, Swift, Dart, Elixir via NIF) usa a `libtaba`.
- **O restante** usa o próprio cliente HTTP, com o contrato OpenAPI e os exemplos de `curl` da documentação.

Um conector novo entra quando houver demanda e alguém para mantê-lo; o critério é cumprir a seção 18.1 e passar na suíte de conformidade (seção 18.7).

### 18.10.5 Imagens Docker de compilação e teste

Cada conector compila e testa **dentro de um contêiner**, com a mesma imagem na máquina da pessoa e no CI, para que ninguém precise instalar a linguagem nem versões antigas dela. Cada pasta traz um `Dockerfile` com `ARG BASE_IMAGE`, e os testes rodam uma vez por imagem da coluna "Testes", **na versão-base e na mais recente** (seção 18.7). Em todos os casos a imagem é a **oficial** da linguagem, e no CI a tag é fixada por *digest*. As tags abaixo são **propostas**; as de versões antigas podem ter saído do Docker Hub, e então a imagem é construída a partir do fonte da própria versão.

| Linguagem | Imagem de compilação | Imagens de teste |
|---|---|---|
| Java | `eclipse-temurin:21-jdk` (`--release 11` gera para a base) | `eclipse-temurin:11-jdk`, `:17-jdk`, `:21-jdk` |
| C# | `mcr.microsoft.com/dotnet/sdk:10.0` ou `:8.0` (compilam `net461`, `net5.0`, `net6.0` e `net8.0`; o `net10.0` só no `:10.0`) | `dotnet/sdk:8.0` e `:10.0` para o .NET moderno; para o Framework 4.8, `mcr.microsoft.com/dotnet/framework/runtime:4.8` (contêiner Windows) |
| Python | `python:3.13` (gera a *wheel* universal) | `python:2.7`, `python:3.4`, `python:3.13` |
| PHP | `composer:2` | `php:5.6-cli`, `php:7.4-cli`, `php:8-cli` |
| Ruby | `ruby:3` | `ruby:2.0`, `ruby:2.7`, `ruby:3` |
| C | `gcc:latest` (com `-std=c89 -pedantic`) | `ubuntu:14.04` (`libcurl` 7.35, a base), `gcc:latest` |
| C++ | `gcc:latest` (com `-std=c++03 -pedantic`) | `ubuntu:14.04`, `gcc:latest` com `-std=c++03` e `-std=c++20` |
| Node.js | `node:22` | `node:0.12`, `node:4`, `node:12`, `node:22` |
| Go | `golang:1` | `golang:1.7`, `golang:1.13`, `golang:1` |
| Rust | `rust:1` | `rust:1.70`, `rust:1` |

- **Versão exata da base:** onde a base é uma versão de correção (Python 2.7.9 e 3.4.3), a imagem oficial é a **última** daquela linha (2.7.18, 3.4.10). Confirmar a 2.7.9 e a 3.4.3 exatamente exigiria imagens construídas do fonte; fica como possível refinamento.
- **.NET Framework:** só roda em Windows. O alvo `net461` **compila** na imagem Linux do SDK 10, mas seus **testes** precisam de um executor Windows no CI.
- **Integração:** os cenários de [19-conectores.md](19-conectores.md) (seção 18.7) sobem o Taba pelo `docker-compose.yml` de `contrato/`, e a imagem do conector entra na mesma rede.
