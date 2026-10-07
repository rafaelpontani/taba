# Conectores

[← Índice](README.md) · [Conectores por linguagem](19a-conectores-linguagens.md)

## 18. Conectores (bibliotecas de cliente)

Os **conectores** são as bibliotecas que deixam uma aplicação publicar e consumir mensagens do Taba em poucas linhas, nas principais linguagens. Eles falam a API de cliente ([06a-api-cliente.md](06a-api-cliente.md)) e não acrescentam protocolo nenhum.

> Os nomes de pacotes, as versões-base e as faixas abaixo são **propostas**, como no restante da documentação. A decisão firme é a lista de linguagens, a regra de simplicidade e a janela de 15 anos (seção 18.1). As versões-base de cada linguagem estão em [19a-conectores-linguagens.md](19a-conectores-linguagens.md), e o que ainda depende de confirmação está em [08b-pendencias.md](08b-pendencias.md) (D68 a D72).

### 18.1 Objetivos e regras

**Linguagens (D67):** C#, Java, Rust, C, C++, Python, PHP, Ruby, Go e Node.js.

Três regras valem para todos os conectores e desempatam qualquer dúvida de desenho:

1. **Simples de usar.** Publicar, consumir e confirmar cabem em poucas linhas, sem configuração obrigatória além do endereço (e do token, se a autenticação estiver ligada).
2. **Simples de implementar.** Cada conector é pequeno o bastante para uma pessoa entender o código inteiro numa tarde e para um conector novo nascer em poucos dias. Por isso a superfície é mínima (seção 18.3), **não há dependência externa** (seção 18.5) e o comportamento é o mesmo em todas as linguagens (seção 18.4).
3. **Funciona em versões antigas.** O conector roda em linguagens e plataformas com até **15 anos** (a janela é de 2011 em diante), no estilo de suporte de longo prazo das distribuições Linux daquela época (RHEL 6, Ubuntu 10.04 e 12.04 LTS). Código novo não pode quebrar a versão-base de cada linguagem, e o conector não exige o que só as versões novas têm (seção 18.6).

Fora das três regras, o conector **não tenta ser esperto**: não guarda estado, não faz buffer de publicações, não reordena. O servidor é quem decide (quórum, FIFO, entrega justa).

### 18.2 Onde ficam: pastas do repositório

Os conectores ficam **no mesmo repositório** do servidor, numa pasta de primeiro nível, `connectors/`, com **uma subpasta por linguagem**. Um repositório só mantém o contrato e os conectores juntos (uma mudança na API e nos conectores entra num mesmo commit) e dispensa N repositórios para a equipe pequena.

```
taba/
  doc/                      documentação
  crates/                   o servidor (Rust, doc/18-estrutura-codigo.md)
  connectors/
    README.md               tabela de linguagens, versões-base e estado de cada conector
    contrato/               o que vale para todos (seção 18.4)
      fixtures/             pares requisição/resposta HTTP, em arquivos de texto
      cenarios/             roteiros da suíte de conformidade (seção 18.7)
      docker-compose.yml    sobe um Taba em modo desenvolvimento para os testes de integração
    csharp/                 Taba.Client       (NuGet)
    java/                   taba-client       (Maven Central)
    rust/                   taba-client       (crates.io)
    c/                      libtaba           (fonte C89 + Makefile + CMake)
    cpp/                    taba.hpp          (cabeçalho C++03 sobre a libtaba)
    python/                 taba              (PyPI)
    php/                    taba/taba-client  (Packagist)
    ruby/                   taba-client       (RubyGems)
    go/                     taba-go           (módulo Go)
    node/                   taba-client       (npm)
```

Cada pasta de linguagem é **autônoma**: tem o seu `README.md` (instalação e um exemplo completo), o arquivo de empacotamento da linguagem, os testes e a pasta `exemplos/`. Nenhuma pasta importa código de outra, **exceto** `cpp/`, que reaproveita a `c/` (seção 18.5).

Estrutura interna padrão de cada pasta (os nomes das pastas de código seguem a convenção da linguagem; o que importa é a mesma organização):

```
connectors/<linguagem>/
  README.md                 instalação, exemplo de 10 linhas, versão-base suportada
  <empacotamento>           pom.xml, *.csproj, Cargo.toml, setup.py, composer.json, *.gemspec, go.mod, package.json, Makefile
  src/ (ou o padrão da linguagem)
  test/                     testes unitários com as fixtures de ../contrato/fixtures
  exemplos/                 publicar, consumir com long polling e ack, FIFO
```

**Relação com o `taba-client` de [18-estrutura-codigo.md](18-estrutura-codigo.md).** O `taba-client` em Rust passa a ser o conector `connectors/rust/`. Como o conector não tem dependências e precisa compilar em Rust antigo (seção 18.5), ele **não reaproveita** o `taba-protocol` do servidor, que usa `serde` e o `tokio`. O contrato entre os dois é o da API, garantido pela suíte de conformidade, não por código compartilhado.

**Publicação.** O repositório é a fonte. Cada conector sai para o registro da sua linguagem (nome entre parênteses acima) a partir da pasta, com uma tag própria (`connectors/python/v1.2.0`). Duas linguagens exigem cuidado: o **Packagist** (PHP) espera o `composer.json` na raiz do repositório, e o **Go** aceita módulo em subpasta, mas só com a tag prefixada. A solução proposta para o PHP é um espelho somente leitura (`git subtree split`), e está em aberto em D70.

### 18.3 Superfície: o que cada conector oferece

A superfície é a **mesma** em todas as linguagens, com os nomes adaptados à convenção local (`publish`, `Publish`, `publish_message`...). São **nove operações** e uma configuração.

| Operação | Rota ([06a](06a-api-cliente.md)) | Devolve |
|---|---|---|
| `createTopic(request)` | `POST /topics` | `Topic` |
| `getTopic(name)` | `GET /topics/{topic}` | `Topic` |
| `listTopics(limit, cursor, name)` | `GET /topics` | `Page<Topic>` (e um iterador sobre todas as páginas, onde a linguagem permite) |
| `updateTopic(name, request)` | `PATCH /topics/{topic}` | `Topic` |
| `deleteTopic(name)` | `DELETE /topics/{topic}` | nada |
| `publish(topic, body, opções)` | `POST /topics/{topic}/messages` | `PublishResponse` (ou `DistributorPublishResponse`) |
| `consume(topic, opções)` | `GET /topics/{topic}/messages` | lista de `Message` (vazia se não há) |
| `ack(topic, message)` | `DELETE /topics/{topic}/messages/{id}?receiptToken=` | nada |
| `changeVisibility(topic, message, segundos)` | `PUT /topics/{topic}/messages/{id}/visibility?receiptToken=` | `VisibilityResponse` |

**Fora da superfície (de propósito):**

- **Tokens (`/tokens`).** Quem cria e revoga tokens é o operador, com o `curl` ou o `taba token`, não a aplicação. Entra depois, se houver demanda.
- **API de operação** (`/metrics`, `/health`, `/node`) e **API do cluster**: não são de aplicação.
- **Loop de consumo (`subscribe(handler)`)**, que faz long polling, chama o `handler` e confirma sozinho. É um **adicional** opcional, de poucas linhas sobre o `consume` + `ack`, e entra por linguagem quando ela tem uma forma natural de expressá-lo (D71).

**`publish`** recebe os **bytes** da mensagem e o `Content-Type`; os atalhos `publishJson(topic, textoJson)` e `publishText(topic, texto)` só escolhem o `Content-Type`. As opções são `groupId`, `deduplicationId`, `contentType` e `requestId`. O conector não valida o corpo: quem valida é o servidor (`400` `validation.invalid-body`).

**`consume`** tem as opções `maxMessages`, `visibilityTimeout`, `waitTime`, `autoCreate` (com `volatile`, `idleTimeout`, `fifo` e `tags`). O conector manda os valores que a pessoa informou e **não aplica padrões próprios**: o que faltar vale o padrão do servidor, que pode mudar sem mexer no conector.

**Configuração** (um objeto, sem variáveis de ambiente: `TABA_*` é o prefixo da configuração do servidor, [10-configuracao.md](10-configuracao.md)):

| Campo | Padrão | Descrição |
|---|---|---|
| `baseUrl` | — (obrigatório) | Endereço do balanceador da API de cliente (`http://host:porta` ou `https://...`). |
| `token` | nenhum | Vai em `Authorization: Bearer`. Sem ele, a chamada vai sem o cabeçalho. |
| `connectTimeout` | 5 s | Tempo para abrir a conexão. |
| `requestTimeout` | 30 s | Tempo total de uma chamada comum. |
| `maxAttempts` | 3 | Tentativas, contando a primeira (seção 18.4). |
| `userAgent` | `taba-<linguagem>/<versão>` | Acrescenta o nome do aplicativo. |

### 18.4 Contrato comum

Estas regras são idênticas em **todos** os conectores. Elas são o que faz a suíte de conformidade (seção 18.7) poder ser a mesma para todos.

**Tempo limite do long polling.** No `consume`, o tempo limite de leitura da conexão é `waitTime + 10 s`, nunca o `requestTimeout`: um `waitTime` de 20 s com o limite de 30 s derrubaria a chamada à toa. Estourar o tempo no `consume` **não é seguro de repetir às cegas** (o nó pode ter capturado mensagens), então o conector devolve o erro de tempo e as mensagens voltam sozinhas ao fim do `visibilityTimeout`.

**Repetição de chamadas.** O conector repete **só** o que o catálogo de [06d-api-convencoes.md](06d-api-convencoes.md) (seção 8.3.5) manda repetir, e espera o `Retry-After`:

| Resposta | Repete? |
|---|---|
| `429`, `503` | Sim, até `maxAttempts`, esperando o `Retry-After` (sem ele, 1 s, 2 s, 4 s). Em `503` de operação que cria ou apaga tópico, o conector trata `409 topic.already-exists` e `404` da repetição como sucesso. |
| `502`, `504` | **Só** na publicação em fila FIFO (o primário deduplica). Fora dela, **não** repete (duplicaria a mensagem), e o erro sobe. |
| Falha de rede antes de enviar o corpo (recusa de conexão, DNS) | Sim, como um `503`. |
| Falha de rede **depois** de enviar o corpo | Não (não se sabe se foi aplicado). O erro sobe. |
| Qualquer `4xx` (menos `429`) | Nunca. |

**Mensagem entregue.** O conector sempre devolve o corpo como **bytes**, de modo que a aplicação não dependa do formato:

| `contentType` e `bodyEncoding` na resposta | O que o conector entrega em `body` |
|---|---|
| `application/json` ou `*/*+json` | O **texto JSON** do campo `body`, tal qual veio (a aplicação o decodifica com a biblioteca de JSON que preferir). |
| `text/*` em UTF-8 | A string, em UTF-8. |
| `bodyEncoding: base64` | Os bytes já **decodificados**. |

Além do `body`, a `Message` tem `id`, `contentType`, `receiptToken`, `deliveryCount`, `publishedAt` e `invisibleUntil` (datas como texto RFC 3339 e, se a linguagem tiver um tipo de data simples, convertidas), e `groupId`, `deduplicationId`, `deadLetteredAt` e `deadLetterReason` quando vierem. O conector **ignora campos desconhecidos** na resposta ([06d](06d-api-convencoes.md), seção 8.3.1).

**Erro.** Uma só forma de erro por conector (exceção, `Result`, código de retorno), com os campos de `ErrorResponse`: `status` (HTTP), `code` (`grupo.motivo`), `message`, `requestId`, `retryAfter` (segundos, se houve) e `details`. O programa da aplicação testa o `code`, nunca o `message`. Erro de rede e de tempo são do mesmo tipo, com `status` zero e `code` `client.network` ou `client.timeout`, códigos que **só existem no cliente**.

**Cabeçalhos.** O conector envia `Authorization`, `Content-Type`, `Accept: application/json`, `User-Agent` e, se a aplicação informar, `X-Request-Id` e `traceparent` ([06d](06d-api-convencoes.md), seção 8.3.2). Lê `Retry-After` e `X-Request-Id` da resposta.

**Conexão.** HTTP/1.1, com conexão persistente (*keep-alive*) **quando a biblioteca HTTP da linguagem a oferece sem custo**. Se o HTTP da versão-base não reaproveita conexão, abrir uma por chamada é aceitável: o desempenho é menos importante que rodar. O conector **não** exige HTTP/2.

**Segurança de thread.** O conector não guarda estado além da configuração e do que a biblioteca HTTP já guarda. O objeto cliente é **seguro para uso concorrente** em Java, C#, Go, Rust e Node. Em **C e C++** (o *handle* da `libcurl` não é compartilhável) e em **Python, PHP e Ruby**, vale **um cliente por thread**, e o README de cada pasta diz como.

### 18.5 Sem dependências: o leitor de JSON mínimo

Uma dependência externa (uma biblioteca de JSON ou de HTTP) quase sempre é o que quebra numa versão antiga, e é o que mais encarece a manutenção. A regra é: **só a biblioteca-padrão da linguagem**, salvo as exceções abaixo.

**HTTP.** Cada conector usa o cliente HTTP que a linguagem traz (`HttpURLConnection`, `HttpWebRequest`, `http.client`, `Net::HTTP`, `net/http`, `http` do Node, `libcurl` no C). Três linguagens não trazem: o **Rust** (implementação mínima de HTTP/1.1 sobre `std::net::TcpStream`, com a TLS plugável, [19a](19a-conectores-linguagens.md)), o **C++** (usa a `libtaba`) e o **C** (`libcurl`, que é a exceção única: está em todas as distribuições, desde muito antes de 2011).

**JSON.** O que o conector lê é **pequeno e fixo**: os DTOs de [06a](06a-api-cliente.md) e o `ErrorResponse`. O que escreve é mais simples ainda (um objeto plano, com mapas de texto em `tags`). Em vez de depender de uma biblioteca de JSON (que nas versões antigas ou não existe, ou muda de API entre versões), cada conector leva um **leitor de JSON mínimo**, de umas 150 a 250 linhas, que:

- percorre o texto uma vez e sabe **pular** qualquer valor (objeto, lista, texto, número, `true`, `false`, `null`);
- entrega os campos de um objeto como pares **nome → trecho do texto**, sem montar uma árvore;
- decodifica uma *string* JSON (escapes, `\uXXXX` com pares substitutos) quando o campo é texto.

Isso resolve de uma vez o campo `body`, que é um **JSON qualquer**: o conector devolve o **trecho de texto** do `body` sem interpretá-lo (seção 18.4). Nas linguagens em que a versão-base **já tem JSON na biblioteca-padrão** (Python, PHP, Node e Go), vale o mais simples: usar o da linguagem para os DTOs e o leitor mínimo apenas para extrair o trecho do `body`. Nas demais (Java, C#, Rust, C, C++ e Ruby 1.8), o leitor mínimo faz tudo. A decisão por linguagem está em [19a](19a-conectores-linguagens.md).

**Base64.** O mesmo vale: a biblioteca-padrão, ou umas 30 linhas onde ela não tem (Java 6, C, C++, Rust).

**Teste do leitor.** O leitor mínimo é a única parte "perigosa" copiada em dez linguagens. Por isso a pasta `contrato/fixtures/` tem um conjunto de documentos JSON de teste (escapes, Unicode, números grandes, aninhamento profundo, espaços e quebras de linha, valores vazios) e **todo conector roda o mesmo conjunto**.

**C++ sobre C.** O `connectors/cpp/` é um **único cabeçalho** em C++03 que embrulha a `libtaba` (`connectors/c/`): classes, `std::string`, `std::vector` e exceções sobre a API em C. Evita implementar a segunda vez o HTTP, o JSON e a repetição de chamadas, e o mesmo núcleo C serve de base para quem precisar ligar o Taba a uma linguagem sem conector (FFI).

### 18.6 Versões antigas: regras de código

A janela de 15 anos não é só "compilar": é **não usar o que a versão-base não tem**. Cada pasta de linguagem tem, no README, a versão-base e a lista do que é proibido. As regras gerais:

- **Nada de sintaxe ou biblioteca posterior à versão-base.** Sem *lambdas* no Java 6, sem `async/await` no C# 3, sem `{a: 1}` no Ruby 1.8, sem `[]` para listas no PHP 5.3, sem *f-strings* no Python 2.6.
- **A compilação roda nas duas pontas.** O CI compila e testa **na versão-base** e **na mais recente** de cada linguagem (seção 18.7). Compilar só na recente deixa passar o uso acidental de recurso novo.
- **Compilador novo, alvo antigo.** Onde a ferramenta atual ainda gera código para a versão antiga (Java com `-source 1.6` num JDK 8, .NET com vários alvos), gera-se o artefato antigo. Onde não gera mais (JDK 9 em diante não compila para 6), usa-se a ferramenta antiga no CI.
- **TLS antigo.** A pilha de TLS das plataformas antigas não fala TLS 1.2 ou 1.3. O conector usa a TLS da plataforma e **não** traz uma própria. Para esses ambientes, o caminho é o balanceador do Taba aceitar uma conexão simples dentro da rede privada (D69).
- **Texto em UTF-8, sempre.** Várias plataformas antigas assumem o *charset* local. O conector converte sempre de e para UTF-8 de forma explícita.
- **Relógio e datas.** O conector não depende do relógio local, e só converte datas se a linguagem tiver um tipo simples para isso.

### 18.7 Qualidade: testes e conformidade

Com dez conectores, o risco é o comportamento **divergir** entre eles. A defesa é um contrato único, testado da mesma forma em todos:

| Camada | O que cobre | Onde roda |
|---|---|---|
| **Fixtures** (unitário, sem rede) | Cada rota: a requisição que o conector monta (verbo, caminho, query, cabeçalhos, corpo) e a leitura de uma resposta pronta, incluindo `ErrorResponse`, `Page<T>`, corpo JSON, texto e base64, e o `Retry-After`. Os arquivos ficam em `contrato/fixtures/` e **todo** conector os lê. | Na máquina da pessoa e no CI, na versão-base e na mais recente. |
| **Leitor de JSON** | O conjunto de documentos da seção 18.5. | Idem. |
| **Integração** (contra o Taba de verdade) | Os `cenarios/` de `contrato/`: criar tópico, publicar, consumir com long polling, ack, invisibilidade, fila FIFO com duplicata, distribuidor com falha parcial, token sem permissão. Rodam contra um Taba em **modo desenvolvimento** ([04-cluster.md](04-cluster.md), seção 6.4), de um nó só, subido pelo `docker-compose.yml` de `contrato/`. | No CI, depois que existir um Taba em modo desenvolvimento; antes dele, só as fixtures. |

O contrato fica **congelado por versão** da API: uma mudança em [06a](06a-api-cliente.md) ou [06d](06d-api-convencoes.md) atualiza as fixtures e os cenários no mesmo commit, e o CI mostra quais conectores precisam acompanhar.

**Versão.** Cada conector tem a sua versão (SemVer) e declara **a versão do contrato da API** que implementa. Um conector antigo continua funcionando com um servidor novo enquanto o servidor só **acrescentar** campos (por isso o conector ignora os que não conhece). Uma quebra de contrato é uma versão maior do servidor e dos conectores.

### 18.8 Plano de entrega

A ordem leva em conta **o valor** (as linguagens mais usadas primeiro) e **o aprendizado** (os primeiros conectores definem o contrato; os demais o repetem).

| Fase | Entrega | Pré-requisito |
|---|---|---|
| 0 — Contrato | `contrato/`: fixtures, o conjunto de teste do leitor de JSON, os cenários e o `docker-compose.yml` (sem imagem ainda, só o esqueleto). O `connectors/README.md`. | Contrato OpenAPI da API de cliente (passo 1 de [08c-proximos-passos.md](08c-proximos-passos.md)) estável. |
| 1 — Referência | **Python** (a mais curta e a mais legível), como implementação de referência, e **Java**, por ter a base mais antiga. Juntas, estressam o desenho nas duas pontas (mais enxuta e mais restrita). | Fase 0. |
| 2 — Núcleo C | **C** (`libtaba`) e **C++** (cabeçalho). | Fase 1 (o contrato já foi exercitado). |
| 3 — Plataformas | **C#**, **Node.js** e **Go**. | Fase 1. |
| 4 — Restantes | **PHP**, **Ruby** e **Rust**. | Fase 1. |
| 5 — Integração | Os cenários de integração passam em todos contra o Taba real; exemplos de uso preenchem a tabela de [Conectores](../README.md#conectores) do README da raiz; publicação nos registros. | Um Taba que rode em modo desenvolvimento: o protótipo de nó único (passo 3 de [08c](08c-proximos-passos.md)), em imagem Docker para o CI. |

As fases 2, 3 e 4 **não dependem entre si** e podem andar em paralelo, por pessoas diferentes. O que nunca anda em paralelo com o resto é a fase 0: sem o contrato congelado, os dez conectores divergem.

### 18.9 Impacto em outros pontos da documentação

- **README da raiz:** a seção "Clientes" passa a "Conectores", com as dez linguagens e o estado de cada uma ([README](../README.md#conectores)).
- **[18-estrutura-codigo.md](18-estrutura-codigo.md):** o `taba-client` vira o conector `connectors/rust/`, sem depender do `taba-protocol`.
- **[03-escopo.md](03-escopo.md):** os conectores entram no escopo inicial (seção 4.4).
- **[09-glossario.md](09-glossario.md):** o termo **Conector**.
