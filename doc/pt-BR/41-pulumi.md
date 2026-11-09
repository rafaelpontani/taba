---
título: Provider Pulumi
resumo: "O provider do Taba para o Pulumi (`taba`), em `connectors/pulumi`: o provider Terraform ligado ao TypeScript/JavaScript, Python, .NET e Go; os pacotes e como o plugin é instalado, a configuração do provider, os recursos e os nomes deles nos SDKs, por que nada recebe nome automático, `preview`, desvio e `import`, o `forceDestroy`, o `createOrUpdate` opcional, como funcionam a substituição e a rotação de um token no Pulumi, `protect` e `retainOnDelete`, o segredo do token no estado, como os SDKs são gerados e testados e como são publicados."
tipo: guia
status: parcial
verificado-em: 2026-11-08 @ f8a9a0a
fontes: [connectors/pulumi/provider/resources.go, connectors/pulumi/provider/docs_edits.go, connectors/pulumi/provider/resources_test.go, connectors/pulumi/provider/cmd/pulumi-resource-taba/main.go, connectors/pulumi/provider/cmd/pulumi-tfgen-taba/main.go, connectors/pulumi/provider/cmd/pulumi-resource-taba/schema.json, connectors/pulumi/scripts/generate.sh, connectors/pulumi/scripts/test.sh, connectors/pulumi/scripts/package.sh, connectors/pulumi/scripts/verify-release.sh, connectors/pulumi/scripts/mirror.sh, connectors/pulumi/examples/scenarios.sh, connectors/terraform/shim/shim.go, .github/workflows/release.yml, .github/workflows/build-connector-pulumi.yml]
---

# Provider Pulumi

> Leia antes: [40-terraform.md](40-terraform.md) (o comportamento de cada recurso está definido lá), [16-api-cliente.md](16-api-cliente.md) (tópicos e tokens) e [14-seguranca.md](14-seguranca.md) (tokens e permissões).

O provider gerencia como código, num programa Pulumi, o que o [provider Terraform e OpenTofu](40-terraform.md) gerencia: **filas, distribuidores e tokens de acesso**, com `preview`, detecção de desvio, `import` e remoção controlada. É **o mesmo provider**, ligado ao Pulumi pelo [pulumi-terraform-bridge](https://github.com/pulumi/pulumi-terraform-bridge): o `connectors/pulumi` embute o provider Terraform (`connectors/terraform/shim`), mapeia os recursos dele para tokens do Pulumi e gera os SDKs de **TypeScript/JavaScript, Python, .NET e Go**. Existe um só código para o comportamento e um só conjunto de testes dele; um comportamento errado no Pulumi está errado no Terraform, e é corrigido e testado lá (D212).

**Estado.** O provider está pronto e testado localmente: unitários, SDKs gerados em dia, cenários de aceitação em TypeScript e Python contra um nó real e os arquivos do release instalados como um usuário os recebe. A pipeline o empacota a cada versão; **nada foi publicado ainda**: o repositório espelho e os pacotes do npm, do PyPI e do NuGet esperam a configuração do mantenedor ([Publicação](#publicação)). O Java ainda não é gerado ([31-pendencias.md](31-pendencias.md)).

```typescript
import * as taba from "@rafaelpontani/pulumi-taba";

const orders = new taba.Queue("orders", { name: "orders", ttl: 86400, maxDeliveries: 5 });

const events = new taba.Distributor("events", { name: "events", prefixes: ["orders", "billing"] });

const token = new taba.Token("orders-app", {
    name: "orders-app",
    permissions: ["publish", "consume"],
    topicPrefixes: [orders.name],
});

export const secret = token.secret; // um segredo do Pulumi
```

A versão do provider é a versão do repositório (`vX.Y.Z`) e ele fala com **o nó da mesma versão**, como o do Terraform (D221).

## Instalação

Um programa precisa de duas coisas: o **pacote** da linguagem dele e o **plugin** (o binário do provider) que a CLI do Pulumi executa.

| Linguagem | Pacote | Instalar |
|---|---|---|
| TypeScript / JavaScript | `@rafaelpontani/pulumi-taba` (npm) | `npm install @rafaelpontani/pulumi-taba` |
| Python | `pulumi-taba` (PyPI; import `pulumi_taba`) | `pip install pulumi-taba` |
| .NET | `Taba.Pulumi` (nuget.org) | `dotnet add package Taba.Pulumi` |
| Go | `github.com/rafaelpontani/pulumi-taba/sdk/go/taba` | `go get github.com/rafaelpontani/pulumi-taba/sdk` |

O plugin não vem dentro desses pacotes. Os pacotes levam o `pluginDownloadURL`, então a CLI baixa o plugin sozinha no primeiro `pulumi up`; para instalá-lo antes (um agente de build sem internet na hora de rodar, uma versão fixa):

```bash
pulumi plugin install resource taba 0.3.9 --server github://api.github.com/rafaelpontani/pulumi-taba
```

Ele lê a Release `v0.3.9` do repositório espelho, que tem `pulumi-resource-taba-v0.3.9-<os>-<arch>.tar.gz` para linux, darwin e windows em amd64 e arm64 (D213). Os mesmos arquivos estão na Release do Taba, e `pulumi plugin install resource taba <versão> --file <arquivo>` instala um sem rede. Use a versão do pacote: o provider e o SDK de uma versão andam juntos.

O Python segue a PEP 440, então um candidato `0.3.9-rc.1` é `0.3.9rc1` no PyPI; o npm mantém `0.3.9-rc.1` e põe os candidatos na tag `next`, então `npm install` e `pip install` nunca pegam um candidato sozinhos.

## Configuração

As mesmas opções do provider Terraform, no `pulumi config` (uma configuração da stack) ou em `new taba.Provider(...)`, e as mesmas variáveis de ambiente:

| Opção | Variável de ambiente | Padrão | |
|---|---|---|---|
| `taba:endpoint` | `TABA_URL` | obrigatória | URL da API de clientes (o balanceador do cluster) |
| `taba:token` | `TABA_TOKEN` | nenhum (chamadas sem autenticação) | Um segredo: `pulumi config set --secret taba:token ...`. Filas e distribuidores precisam de `create` e `manage` dentro dos prefixos; tokens precisam do token raiz (ou de um `admin` para tokens que não são `admin`) |
| `taba:caCertPem` / `taba:caCertFile` | `TABA_CA_CERT` (um PEM ou um caminho) | as raízes do sistema | Uma autoridade certificadora privada do certificado da API. O certificado é público, então `caCertPem` não é segredo |
| `taba:requestTimeout` | | `30s` | Timeout de cada chamada (uma duração do Go) |
| `taba:retryMaxWait` | | `3m` | Espera total entre as tentativas de uma chamada que recebe `429` ou `503` ([40-terraform.md](40-terraform.md#novas-tentativas-e-um-nó-fora), D220); `0s` = sem nova tentativa |
| `taba:createOrUpdate` | `TABA_CREATE_OR_UPDATE` (`true`/`false`) | `false` | Padrão de todos os recursos ([abaixo](#adotar-o-que-já-existe)) |

```bash
pulumi config set taba:endpoint https://taba.example.com:8080
pulumi config set --secret taba:token "$ROOT_TOKEN"
```

## Recursos e funções

| Terraform | Pulumi (token) | |
|---|---|---|
| `taba_queue` | `taba:index/queue:Queue` | `Queue` |
| `taba_distributor` | `taba:index/distributor:Distributor` | `Distributor` |
| `taba_token` | `taba:index/token:Token` | `Token` |
| data sources `taba_topic`, `taba_topics`, `taba_token`, `taba_tokens` | `taba:index/getTopic:getTopic` e as três irmãs | `getTopic`, `getTopics`, `getToken`, `getTokens` (e as formas `...Output`) |

Os atributos mantêm o significado, a validação e o que é calculado (as tabelas de [40-terraform.md](40-terraform.md#recursos-e-data-sources)); só a grafia segue cada linguagem: `maxDeliveries` em TypeScript, `max_deliveries` em Python, `MaxDeliveries` em .NET e Go. O `id` é o do Terraform: o nome do tópico, e o `id` que o servidor dá a um token. Dois nomes diferem do Terraform:

- **O segredo de um token é `secret`, não `token`** (D212). A classe C# `Token` gerada não pode ter uma propriedade chamada `Token` (CS0542), então o campo tem o mesmo nome em todas as linguagens. É um segredo do Pulumi: aparece como `[secret]` na CLI e fica criptografado no estado pelo backend.
- **`createOrUpdate`** também é uma propriedade de cada recurso (`new taba.Queue("orders", { name: "orders", createOrUpdate: true })`), e `forceDestroy` da fila.

Uma função (`taba.getTopic({ name: "orders" })`) lê uma fila ou um token criado em outro lugar e nunca devolve um segredo. Uma função cuja entrada não depende de um recurso do programa **roda durante o `preview`, antes de o recurso existir**: passe a saída do recurso (`taba.getTopicOutput({ name: orders.name })`) quando ela precisar esperar por ele.

## Sem nomes automáticos

O Pulumi dá ao recurso um nome físico com um sufixo aleatório (`orders-1a2b3c4`) quando o provider permite ("autonaming"). O Taba não permite, e **`name` é obrigatório** em `Queue`, `Distributor` e `Token` (D222): o nome de uma fila é um contrato com as aplicações que publicam e consomem, e é como o `createOrUpdate` acha uma fila que já existe, o que um sufixo tornaria impossível. O nome Pulumi do recurso (`"orders"` em `new taba.Queue("orders", ...)`) é só a identidade dentro do programa e pode ser diferente de `name`.

## Preview, desvio e import

- **Preview:** o plano confere as mesmas regras do Terraform antes de qualquer coisa chegar ao nó (nomes, faixas, tags, `idleTimeout` só com `volatile`, um `expiresAt` no passado, `admin` com `topicPrefixes`) e mostra `~` para uma mudança no lugar e `+-` para uma substituição.
- **Desvio:** `pulumi refresh` lê cada recurso de novo; um tópico ou token apagado fora do Pulumi sai do estado, e um campo mudado fora aparece como uma diferença que o próximo `pulumi up` desfaz.
- **Import:** `pulumi import taba:index/queue:Queue orders orders` (o nome do tópico), `pulumi import taba:index/token:Token app <id do token>`, ou a opção `import` do recurso. Importar um distribuidor como `Queue` (ou o contrário), ou uma fila de mensagens mortas, é um erro. O programa que o `pulumi import` escreve não traz `forceDestroy` nem `createOrUpdate` (são do provider, `false`/`null` depois de um import), e o recurso importado fica com `protect`: tire a proteção para destruí-lo.

## Proteger uma fila com mensagens

O `forceDestroy` funciona como no Terraform ([40-terraform.md](40-terraform.md#proteger-uma-fila-com-mensagens), D219): sem ele, uma remoção ou uma substituição usa `DELETE ?ifEmpty=true` e uma fila com mensagens é o erro `409 topic.not-empty`, que diz quantas são e as duas saídas. O valor é lido do **estado**, então ele precisa ser aplicado por um `pulumi up` **antes** daquele que substitui ou remove a fila:

```bash
# 1. forceDestroy: true no programa, pulumi up   (nada mais muda)
# 2. a mudança que substitui a fila, ou pulumi destroy
```

## Adotar o que já existe

O `createOrUpdate` vem desligado (D218) e tem o comportamento, os avisos e os limites de [40-terraform.md](40-terraform.md#adotar-o-que-já-existe): uma fila ou um distribuidor é criado, ou levado ao estado declarado, por um `PUT ?recreate=if-empty` decidido pelo nó; um token só é adotado quando exatamente um tem o nome, as permissões e os prefixos (o segredo dele não volta: `pulumi up --replace <urn>` faz um novo). Os avisos "Took over" e "Recreated" aparecem como diagnósticos do `pulumi up`.

```typescript
const provider = new taba.Provider("taba", { createOrUpdate: true });
const orders = new taba.Queue("orders", { name: "orders" }, { provider });
// ou, para um recurso: new taba.Queue("orders", { name: "orders", createOrUpdate: true })
// ou, para a stack inteira: pulumi config set taba:createOrUpdate true
```

O `pulumi import` cobre "sei que existe"; o `createOrUpdate` cobre "pode ou não existir". Depois do `up`, um recurso adotado pertence ao estado como qualquer outro, e o `pulumi destroy` **o remove**.

## Substituição e rotação

Por padrão o Pulumi cria o recurso novo **antes** de apagar o antigo. Para uma fila ou um distribuidor isso falharia com `409 topic.already-exists` (o nome é o id), então os dois são declarados `deleteBeforeReplace`: uma mudança de `fifo` ou `volatile` apaga primeiro (com o `forceDestroy` e a conferência de fila vazia acima) e depois cria. Um `Token` mantém criar antes de apagar, porque o id dele é do servidor: a **rotação** é

```bash
pulumi up --replace 'urn:pulumi:dev::project::taba:index/token:Token::orders-app'
```

que cria o token novo, atualiza quem usa o `secret` dele e revoga o antigo, sem nenhum momento sem um token válido. `permissions` e `topicPrefixes` também substituem um token; `name` e `expiresAt` o mudam no lugar.

## `protect` e `retainOnDelete`

Opções do Pulumi que completam o `forceDestroy`:

| Opção | Efeito |
|---|---|
| `protect: true` | O `pulumi destroy` e uma substituição recusam o recurso até a proteção sair. Use numa fila que é dado de produção |
| `retainOnDelete: true` | O recurso sai do estado e **continua no nó**. Use para passar uma fila a outro programa ou ferramenta |
| `forceDestroy: true` (fila) | Permite apagar uma fila que ainda tem mensagens, na próxima remoção |

## Tokens

- **O segredo está no estado**, como um segredo do Pulumi. Guarde o estado num backend que o criptografe (Pulumi Cloud, ou um próprio com `--secrets-provider`); `pulumi stack output --show-secrets` o imprime.
- Um token **adotado** (`createOrUpdate`) ou **importado** tem o `secret` vazio, e o aviso do provider diz isso.
- A expiração, uma resposta de criação perdida e as permissões necessárias são as mesmas de [40-terraform.md](40-terraform.md#tokens).

## Como é construído

- O `provider/resources.go` mapeia o provider Terraform (`shim.New(version)`): os tokens do Pulumi, o campo `secret`, `deleteBeforeReplace` e nenhum nome automático. O bridge usa o mesmo `terraform-plugin-framework` (v1.19) e `terraform-plugin-go` (v0.31) do provider Terraform, sem fork; ele exige Go 1.26, então `connectors/pulumi/provider` é um módulo Go à parte, com dois `replace` locais (`connectors/terraform` e `connectors/go`).
- O `provider/docs_edits.go` adapta as descrições que vêm de `connectors/terraform/docs/`: o bloco `lifecycle` do exemplo do token (só existe no Terraform e quebra o conversor) e a frase da rotação.
- O `scripts/generate.sh` roda o `pulumi-tfgen-taba` num contêiner (a CLI do Pulumi é baixada uma vez num volume do Docker e conferida pelo `SHA256SUMS` dela) e escreve `provider/cmd/pulumi-resource-taba/schema.json` e os quatro SDKs em `sdk/`, que são **versionados** com `linguist-generated`. Uma mudança no schema ou num exemplo do provider Terraform pede o `generate.sh` de novo; o CI falha se os SDKs não estiverem em dia.
- Os SDKs são gerados com uma versão provisória (`0.0.0`); o `scripts/package.sh` escreve a real numa cópia, então a árvore commitada não muda a cada release.

## Testes

| Nível | Comando | O quê |
|---|---|---|
| Unitários | `scripts/test.sh unit` | `gofmt`, `go vet` e `provider/resources_test.go`: todo recurso e data source do provider Terraform tem um token Pulumi e está no schema (um recurso novo no Terraform sem mapeamento falha aqui), `name` é obrigatório e nada tem nome automático, tudo que é sensível no Terraform é `secret`, a ordem da substituição |
| SDKs | `scripts/test.sh sdks` | Roda o `generate.sh` de novo e compara: o schema e os SDKs commitados são os gerados |
| Aceitação | `scripts/test.sh acc` | Compila o plugin e os SDKs (.NET e Go são compilados) e roda o `examples/scenarios.sh` com TypeScript e Python contra um nó, com o backend de arquivo (sem conta no Pulumi). Depois de cada passo, `pulumi preview --expect-no-changes` |
| Arquivos do release | `scripts/verify-release.sh <pasta> <versão>` | Instala o plugin, o pacote npm e a wheel **dos arquivos** que o `package.sh` fez e roda `up` (três recursos), um `preview` vazio e `destroy` contra a imagem da versão |

Os cenários do `acc`: o primeiro `up` (fila, distribuidor, token e funções); uma atualização no lugar; a mudança de um campo imutável numa fila com mensagens, recusada e depois feita após o `forceDestroy`; um desvio feito pela API, visto pelo `refresh` e corrigido; o `createOrUpdate` sobre uma fila que existe; a rotação do token; o `import` de uma fila e de um token; o `destroy` sem sobras no nó. Tudo roda em contêineres (`TABA_IMAGE` escolhe o nó, `taba:local` por padrão); o job de CI `connector-pulumi` roda os unitários e a conferência dos SDKs, e a aceitação roda no `connector-contract` contra a imagem construída do commit. Um programa em .NET ou Go com `preview` ainda não roda ([31-pendencias.md](31-pendencias.md)).

Casos de uso `UC-IAC-024` a `UC-IAC-034` ([26a](26a-casos-de-uso-implementados.md), [26b](26b-casos-de-uso-planejados.md)).

## Publicação

O `release.yml` empacota o provider a cada versão (`scripts/package.sh`: seis arquivos do plugin, o pacote npm, a wheel e o sdist, o pacote NuGet, com o README do provider como o que o nuget.org mostra, o módulo do SDK Go e um `SHA256SUMS`, reproduzíveis) e os arquivos vão para a Release do Taba. Depois o job `pulumi-verify` roda o `verify-release.sh` contra a imagem recém-publicada (ele lê também o `.nupkg`: id, versão e README, que não mudam depois do push) e, só se passar, cada destino roda, cada um com a sua variável (vazia = não publica):

| Destino | Variável | O quê |
|---|---|---|
| Espelho `rafaelpontani/pulumi-taba` (D213) | `PULUMI_PROVIDER_REPO` e o segredo `PULUMI_MIRROR_TOKEN` | `scripts/mirror.sh`: o SDK Go (`sdk/`), o schema, os exemplos e o README na `main`, as tags `v<versão>` e `sdk/v<versão>` (o módulo Go está em `sdk/`, então o Go lê as versões das tags `sdk/`) e uma Release com os arquivos, que é o que `pulumi plugin install --server github://...` baixa; o que a API do GitHub responde com 5xx é tentado de novo (até 5 vezes) e uma Release criada só em parte é completada |
| npm | `PULUMI_NPM=true` | Trusted Publishing; `--tag next` num candidato; depois instala o pacote do npm e confere |
| PyPI | `PULUMI_PYPI=true` | Trusted Publishing; `skip-existing` |
| nuget.org | `PULUMI_NUGET_USER` | Trusted Publishing (`NuGet/login`); `--skip-duplicate` |

O espelho existe porque o caminho do módulo Go e o download do plugin do Pulumi pedem um repositório com tags e Releases próprias, e o ruleset de tags deste repositório não permite uma tag como `sdk/v0.3.9` (D213). Todo job é idempotente, então repetir é inofensivo, e um candidato (`-rc.N`) sai como pré-release. O que o mantenedor cria uma vez está em [27-ci-cd.md](27-ci-cd.md#provider-pulumi).

## Limites

- **Java** (Maven Central) não é gerado: pede uma conta no Central Portal, um namespace verificado e uma chave GPG própria ([31-pendencias.md](31-pendencias.md)).
- **O Pulumi Registry** ainda não lista o provider: a listagem é um pull request para o `pulumi/registry`, revisado pela Pulumi, e espera a primeira versão final ([31-pendencias.md](31-pendencias.md)).
- O `forceDestroy` é lido do estado (aplique antes), e um recurso importado fica com `protect`.
- Os limites do provider Terraform valem: PRB-103, PRB-104, PRB-106, PRB-107 ([40-terraform.md](40-terraform.md#limites)).

## Fontes e provas

- `connectors/pulumi/provider/` (mapeamento, testes, plugin e `tfgen`), `connectors/pulumi/scripts/` (geração, testes, empacotamento, publicação), `connectors/pulumi/examples/` (os cenários), `connectors/terraform/shim/` (o pacote que o bridge embute), os jobs `connector-pulumi`, `pulumi-provider`, `pulumi-verify`, `pulumi-mirror`, `pulumi-npm`, `pulumi-pypi` e `pulumi-nuget` de `.github/workflows/`.
- Decisões D212, D213 e D222 em [30-decisoes.md](30-decisoes.md), com D211 e D218 a D221 do provider Terraform.
- Casos de uso `UC-IAC-024` a `UC-IAC-034` em [26a](26a-casos-de-uso-implementados.md) e [26b](26b-casos-de-uso-planejados.md).
