# Conectores por linguagem

[← Índice](README.md) · [Conectores](19-conectores.md)

## 18.10 Versão-base e particularidades de cada linguagem

O desenho geral (pastas, superfície, contrato, regras de código e plano) está em [19-conectores.md](19-conectores.md). Este arquivo traz, **por linguagem**, a versão mais antiga suportada (a **versão-base**), o que muda por causa dela e o que o conector usa para HTTP, JSON e base64.

> As versões-base são **propostas** e a confirmação está em D68 ([08b-pendencias.md](08b-pendencias.md)).

### 18.10.1 Como a versão-base foi escolhida

A janela é de **15 anos** (de 2011 em diante). Para linguagens que existiam, a base é a versão **entregue pelas distribuições de suporte longo daquela época** (RHEL 6, de novembro de 2010, com suporte estendido até 2024; Ubuntu 10.04 e 12.04 LTS; Debian 6): é o que, de fato, ainda roda em servidores antigos. Para as que ainda não existiam em 2011 (Go e Rust) ou não vinham nas distribuições (Node.js), a base é a **mais antiga estável e utilizável**, e isso é dito na linha.

| Linguagem | Pasta | Versão-base | Lançada | Por quê | Também testada em |
|---|---|---|---|---|---|
| **Java** | `connectors/java/` | **Java 6** (`-source 1.6`) | 2006 | OpenJDK 6 do RHEL 6; ainda comum em servidores de aplicação antigos. | 8 (LTS), a LTS mais recente |
| **C#** | `connectors/csharp/` | **.NET Framework 3.5 SP1** (C# 3) | 2008 | Padrão do Windows Server 2008 R2 e do Windows 7; roda também no 4.x. | .NET Framework 4.8, .NET 8 (`netstandard2.0`) |
| **Python** | `connectors/python/` | **2.6** | 2008 | Python do RHEL 6 (2.6.6). | 2.7, 3.2 a 3.13 |
| **PHP** | `connectors/php/` | **5.3** | 2009 | PHP do RHEL 6 (5.3.3) e do Ubuntu 10.04. | 5.6, 7.4, 8.x |
| **Ruby** | `connectors/ruby/` | **1.8.7** | 2008 | Ruby do RHEL 6 (1.8.7). | 1.9.3, 2.7, 3.x |
| **C** | `connectors/c/` | **C89** (ANSI) | 1989 | Compila com GCC 4.1 (RHEL 5) e MSVC 2008. A única dependência é a `libcurl` 7.19 (RHEL 6). | C99, C11, a mais recente |
| **C++** | `connectors/cpp/` | **C++03** | 2003 | GCC 4.4 (RHEL 6) e MSVC 2008, sem C++11. | C++11 a C++20 |
| **Node.js** | `connectors/node/` | **0.10** | 2013 | A mais antiga com o `http` completo e o ecossistema npm estável (o Node 0.6 de 2011 não vinha em distribuição nenhuma). | 4, 12, a LTS mais recente |
| **Go** | `connectors/go/` | **1.4** | 2014 | **Não há Go de 2011**: o Go 1.0 é de março de 2012, e a 1.3 trouxe o `Client.Timeout`. A 1.4 é a base prática. | 1.13, a mais recente |
| **Rust** | `connectors/rust/` | **1.31** (edição 2018) | 2018 | **Não há Rust de 2011**: a 1.0 é de 2015. A 1.31 é a primeira da edição 2018, e o conector precisa só da biblioteca-padrão. | a estável mais recente |

Nenhuma dessas versões recebe correção de segurança da própria linguagem. O conector as **suporta**, mas o README de cada pasta avisa que rodar nelas é responsabilidade de quem opera o ambiente.

### 18.10.2 Quadro de tecnologia

O que cada conector usa, na versão-base:

| Linguagem | HTTP | JSON | Base64 | Teste (unitário) |
|---|---|---|---|---|
| Java 6 | `HttpURLConnection` | Leitor mínimo (seção 18.5) | Próprio (~30 linhas) | JUnit 4 |
| C# (.NET 3.5) | `HttpWebRequest` | Leitor mínimo | `Convert.ToBase64String` | NUnit 2.6 |
| Python 2.6 | `httplib` / `http.client` | `json` da biblioteca-padrão | `base64` | `unittest` |
| PHP 5.3 | `curl` (com *fallback* para *streams*) | `json_decode` | `base64_decode` | PHPUnit 4.8 |
| Ruby 1.8.7 | `Net::HTTP` | Leitor mínimo | `pack('m')` | `test/unit` |
| C89 | `libcurl` 7.19 | Leitor mínimo | Próprio (~30 linhas) | Teste em C simples, sem estrutura externa |
| C++03 | via `libtaba` | via `libtaba` | via `libtaba` | O mesmo teste em C, mais um de exceções |
| Node 0.10 | `http` / `https` | `JSON` | `Buffer` | `assert` + um *runner* de 30 linhas |
| Go 1.4 | `net/http` | `encoding/json` | `encoding/base64` | `testing` |
| Rust 1.31 | HTTP/1.1 próprio sobre `std::net::TcpStream` | Leitor mínimo | Próprio (~30 linhas) | `#[test]` |

Nas linhas com "Leitor mínimo", o JSON todo é lido por ele. Nas com a biblioteca da linguagem, o `body` continua sendo extraído como **trecho de texto** pelo leitor mínimo, para entregar o JSON qualquer sem reserializar (seção 18.5).

### 18.10.3 Particularidades

#### Java (`connectors/java/`)

- **Sintaxe de Java 6:** sem *diamond* (`<>`), sem `try-with-resources`, sem `switch` sobre `String`, sem lambdas e sem `java.util.Base64` (é do 8). Os fluxos fecham em `finally`.
- **Saída:** um `.jar` único, compilado num **JDK 8** com `-source 1.6 -target 1.6` (o JDK 12 em diante já não gera para 6). Roda do Java 6 ao mais recente. Se um dia o JDK 8 sair do CI, a versão-base sobe.
- **`PATCH`:** o `HttpURLConnection` **recusa** `PATCH` (`ProtocolException`). É o único verbo da superfície que ele não faz; o `updateTopic` depende de D72.
- **Sem `javax.xml.bind`:** o JAXB saiu do JDK 11. O conector não o usa.
- **Pacote:** artefato `taba-client`. O *groupId* e o nome do pacote Java dependem da organização que publica, e ficam para D70.

#### C# (`connectors/csharp/`)

- **Alvos:** `net35` e `netstandard2.0`, num mesmo projeto. O `net35` é a base. Compila-se com o SDK atual e os *reference assemblies* do .NET Framework 3.5, e a versão de linguagem fica no C# 3 (sem `async`/`await`, sem `dynamic`, sem argumentos nomeados).
- **API síncrona.** Em `netstandard2.0` entram os métodos `...Async` (com `Task`), em código sob `#if`. A base antiga não tem `Task`.
- **Pacote:** `Taba.Client` (NuGet), com o *namespace* `Taba`.
- **TLS 1.2:** no .NET 3.5 e no 4.0, o protocolo precisa de ajuste do sistema e do `ServicePointManager.SecurityProtocol`, que o conector **não** muda sozinho (D69).

#### Python (`connectors/python/`)

- **Um arquivo de código** (`taba/client.py`) mais o leitor do `body`, sem dependências, no mesmo código para Python 2 e 3: `try: import httplib except ImportError: import http.client as httplib`.
- **Proibido na base 2.6:** *dict* e *set comprehension*, literal de *set*, `{}` sem índice em `str.format`, `OrderedDict`, `with` com vários contextos e `unittest` novo (`assertIn`, `skipTest`). Os bytes usam `bytes`, que no 2.6 é o `str`.
- **HTTPS no 2.6 e 2.7 antigo não confere o certificado.** O README avisa e recomenda a conexão simples atrás do balanceador ou um Python 2.7.9+ (D69).
- **Pacote:** `taba` (PyPI), *wheel* universal `py2.py3-none-any`.

#### PHP (`connectors/php/`)

- **PHP 5.3:** *namespaces* e *closures* existem; **não** há `[]` para listas (5.4), *traits* (5.4), `JSON_UNESCAPED_UNICODE` (5.4) nem `finally` (5.5). O conector usa `array()` e a classe `Taba\Client`.
- **HTTP:** a extensão `curl` (padrão na maioria das instalações); se não existir, *streams* (`file_get_contents` com `stream_context_create`, com `ignore_errors`). As duas ficam atrás de uma interface `Transport`, que a aplicação também pode trocar.
- **Instalação:** Composer (`taba/taba-client`) ou um arquivo único para copiar (`taba.php`), útil nas hospedagens sem Composer. **Autoload PSR-4** na instalação por Composer; o arquivo único dispensa o autoload.
- **Packagist** pede o `composer.json` na raiz do repositório: a pasta vira um espelho somente leitura (D70).

#### Ruby (`connectors/ruby/`)

- **Ruby 1.8.7:** sem `->`, sem a sintaxe `{a: 1}` (usa `:a => 1`), sem `String#force_encoding` (as cadeias são bytes), sem `each_with_object`. Para o UTF-8 o conector testa `respond_to?(:force_encoding)`.
- **`PATCH`:** `Net::HTTP::Patch` só existe a partir do 1.9.3. Na 1.8.7, o conector define a classe com `Class.new(Net::HTTPRequest)`.
- **JSON:** a biblioteca `json` só vem no 1.9, então o leitor mínimo faz tudo.
- **Pacote:** a *gem* `taba-client`, com o módulo `Taba`.

#### C (`connectors/c/`)

- **C89:** declarações no começo do bloco, comentários `/* */`, sem `//`, sem `stdint.h` (usa `long` e `unsigned long`), sem `bool` e sem `snprintf` (no MSVC 2008, `_snprintf`; o conector embrulha numa função).
- **`libcurl`** é a única dependência, por *link* dinâmico, versão 7.19 ou posterior. O *handle* do curl não é seguro entre *threads*: **um cliente por *thread***.
- **Interface:** `taba_client_new(config)`, as nove operações da seção 18.3 e `taba_free_*` para cada resultado. Todo resultado de texto ou de corpo é do chamador, liberado com a função própria da biblioteca (que também resolve o problema de misturar `malloc` de DLLs diferentes no Windows).
- **Build:** `Makefile` simples e `CMakeLists.txt` (CMake 2.8, de 2009). Gera `libtaba.a` e `libtaba.so`/`taba.dll`.
- **Um transporte pluggável:** um ponteiro de função (`taba_http_fn`) substitui a `libcurl` para quem tem o próprio cliente HTTP (sistemas embarcados).

#### C++ (`connectors/cpp/`)

- **C++03:** sem `auto`, `nullptr`, `std::unique_ptr`, *lambdas* nem `std::string_view`. Usa `std::string`, `std::vector`, `std::map` e exceções (`taba::Error`, que herda de `std::runtime_error`).
- **Um cabeçalho** (`taba.hpp`) sobre a `libtaba`. A aplicação faz o *link* com a `libtaba`, e o cabeçalho só dá a interface tipada e o fechamento automático (RAII).
- Seguro entre *threads* **só** com um cliente por *thread* (a `libcurl` por baixo).

#### Node.js (`connectors/node/`)

- **ES5:** sem `let`, `const`, *arrow functions*, classes nem `Promise` (só existem em versões novas). O conector usa `var`, `function` e protótipos.
- **API de *callback*** `(err, resultado)` como base. Se existir `Promise` no ambiente, as mesmas funções devolvem uma *promise* quando o *callback* é omitido.
- **`Buffer`:** `Buffer.from` não existe no 0.10 e `new Buffer` está obsoleto nas versões novas. O conector escolhe pelo teste `typeof Buffer.from === 'function'`.
- **Pacote:** `taba-client` (npm), com `index.d.ts` opcional para quem usa TypeScript.

#### Go (`connectors/go/`)

- **Sem `context`** na base (ele entrou no Go 1.7). Os métodos têm a assinatura simples, e o arquivo `client_go17.go`, com `// +build go1.7`, acrescenta as variantes `...Context(ctx, ...)`. Toolchains antigas pulam o arquivo.
- **Sem módulos** na base (entraram no 1.11): a pasta vale em `GOPATH` e também leva um `go.mod`, que as versões antigas ignoram. Como o módulo fica numa subpasta, a tag segue o formato `connectors/go/v1.2.0`.
- Um `Client` é seguro entre *goroutines* (o `http.Client` do Go é).

#### Rust (`connectors/rust/`)

- **Sem dependências** e sem `async`: um cliente **síncrono** e bloqueante, com um HTTP/1.1 mínimo sobre `std::net::TcpStream` (linha de status, cabeçalhos, `Content-Length` e `chunked`) e os tempos limite de `set_read_timeout`/`set_write_timeout`. É a base mais simples e a que atravessa as edições do Rust.
- **TLS:** não está na biblioteca-padrão. Há um *trait* `Transport`, e uma *feature* opcional `tls` (com uma biblioteca de TLS) que **sobe a versão mínima do Rust**: quem precisa de HTTPS direto usa a *feature* e uma toolchain mais nova; quem fala por trás de um balanceador que termina o TLS fica na base (D69).
- **Cliente assíncrono:** não faz parte da base. Quem usa `tokio` chama o cliente síncrono dentro de `spawn_blocking`. Uma *feature* `async` pode vir depois, sem afetar a base (D71).
- **`taba-protocol`:** não é usado (seção 18.2).

### 18.10.4 E as outras linguagens?

Quem usa Perl, Kotlin, Scala, Dart, Swift, Elixir, Lua e outras tem duas saídas, sem conector próprio:

- **Linguagens da JVM e .NET** usam direto os conectores Java e C# (Kotlin e Scala chamam a classe Java; F# e VB.NET, a de C#).
- **Qualquer linguagem com FFI para C** (Perl, Lua, Swift, Dart, Elixir via NIF) usa a `libtaba`.
- **O restante** usa o próprio cliente HTTP, com o contrato OpenAPI e os exemplos de `curl` da documentação.

Um conector novo entra quando houver demanda e alguém para mantê-lo; o critério é cumprir a seção 18.1 e passar na suíte de conformidade (seção 18.7).
