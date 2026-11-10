---
título: Provider Terraform e OpenTofu
resumo: "O provider do Taba para Terraform e OpenTofu (`rafaelpontani/taba`), em `connectors/terraform`: o que ele gerencia (filas, distribuidores, tokens de acesso e quatro data sources), a configuração do provider, como funcionam o `plan`, o desvio e o `import`, o `force_destroy`, o `create_or_update` opcional (quando usar e os riscos), o segredo do token no estado e a rotação, as novas tentativas com um nó fora, como ele é testado e publicado, e os limites."
tipo: guia
status: parcial
verificado-em: 2026-11-10 @ 114dbb9
fontes: [connectors/terraform/internal/provider/provider.go, connectors/terraform/internal/provider/queue_resource.go, connectors/terraform/internal/provider/distributor_resource.go, connectors/terraform/internal/provider/token_resource.go, connectors/terraform/internal/provider/topic_data_sources.go, connectors/terraform/internal/provider/token_data_sources.go, connectors/terraform/internal/provider/common.go, connectors/terraform/internal/provider/planmodifiers.go, connectors/terraform/internal/provider/acceptance_test.go, connectors/terraform/scripts/test.sh, connectors/terraform/scripts/cluster.sh, connectors/terraform/scripts/package.go, connectors/terraform/scripts/verify-release.sh, connectors/terraform/scripts/mirror.sh, connectors/go/tokens.go, connectors/go/transport.go, .github/workflows/release.yml]
---

# Provider Terraform e OpenTofu

> Leia antes: [16-api-cliente.md](16-api-cliente.md) (tópicos e tokens) e [14-seguranca.md](14-seguranca.md) (tokens e permissões).

O provider gerencia como código o que de outro jeito se cria pela API de clientes ou pelo `taba-cli`: **filas, distribuidores e tokens de acesso**, com `plan`, detecção de desvio, `import` e remoção controlada. É um programa Go em `connectors/terraform` (terraform-plugin-framework) que chama a API de clientes pelo conector Go (`connectors/go`), então não precisa de nada no nó além da API de clientes. O mesmo binário serve o **Terraform** e o **OpenTofu**, e o [provider Pulumi](41-pulumi.md) é gerado a partir dele.

**Estado.** O provider está pronto e testado (unitários, aceitação contra um nó real com Terraform e OpenTofu, uma rodada de três nós com um nó fora). Está publicado no [Terraform Registry](https://registry.terraform.io/providers/rafaelpontani/taba) como `rafaelpontani/taba` desde a `v0.3.8-rc.3`, e no [OpenTofu Registry](https://search.opentofu.org/provider/rafaelpontani/taba), com o mesmo nome e a mesma chave de assinatura, com todas as versões desde a `v0.3.8-rc.3` ([Publicação](#publicação)). Os zips de cada versão também ficam na Release do GitHub dela.

```hcl
terraform {
  required_providers {
    taba = { source = "rafaelpontani/taba" }
  }
}

provider "taba" {
  endpoint = "https://taba.example.com:8080" # ou TABA_URL
  token    = var.taba_token                  # ou TABA_TOKEN
}

resource "taba_queue" "orders" {
  name           = "orders"
  ttl            = 86400
  max_deliveries = 5
}

resource "taba_distributor" "events" {
  name     = "events"
  prefixes = ["orders", "billing"]
}

resource "taba_token" "orders_app" {
  name           = "orders-app"
  permissions    = ["publish", "consume"]
  topic_prefixes = ["orders"]
  lifecycle { create_before_destroy = true }
}
```

A versão do provider é a versão do repositório (`vX.Y.Z`), e ele mira **o nó da mesma versão**: não há detecção do que o nó suporta nem caminho para nós mais antigos (D221).

## Configuração

| Atributo | Variável de ambiente | Padrão | |
|---|---|---|---|
| `endpoint` | `TABA_URL` | obrigatório | URL da API de clientes (o balanceador do cluster) |
| `token` | `TABA_TOKEN` | nenhum (chamadas sem autenticação) | Sensível. Filas e distribuidores precisam de `create` e `manage` dentro dos prefixos; tokens precisam do token root (ou de um `admin`, para tokens que não são `admin`) |
| `ca_cert_pem` / `ca_cert_file` | `TABA_CA_CERT` (um PEM ou um caminho) | as raízes do sistema | Uma autoridade certificadora privada do certificado da API |
| `request_timeout` | | `30s` | Tempo limite de cada chamada (uma duração do Go) |
| `retry_max_wait` | | `3m` | Espera total entre as tentativas de uma chamada que recebe `429` ou `503` ([abaixo](#novas-tentativas-e-um-nó-fora)); `0s` = sem nova tentativa |
| `create_or_update` | `TABA_CREATE_OR_UPDATE` (`true`/`false`) | `false` | Padrão de todos os recursos ([abaixo](#adotar-o-que-já-existe)); outro texto na variável é erro de configuração |

## Recursos e data sources

| | Muda no lugar | Substitui (`-/+`) | Calculados |
|---|---|---|---|
| `taba_queue` | `tags`, `ttl`, `max_deliveries`, `max_message_bytes`, `idle_timeout` (só volátil) | `name`, `fifo`, `volatile` | `id`, `created_at`, `dead_letter_name`, `effective_ttl`, `effective_max_deliveries`, `effective_max_message_bytes` |
| `taba_distributor` | `prefixes` (1 a 20, na ordem declarada), `tags` | `name` | `id`, `created_at` |
| `taba_token` | `name`, `expires_at` | `permissions`, `topic_prefixes` (conjuntos) | `id`, `created_at`, `token` (o segredo, sensível) |

Além desses, `force_destroy` (fila) e `create_or_update` (os três) são só do provider e nunca chegam ao nó.

- **O que não é declarado é o padrão do nó.** Um atributo omitido é `null` no estado, e os `effective_*` mostram o valor em vigor, então uma mudança do padrão do nó não é diferença falsa. Tirar um atributo da configuração o volta ao padrão: a atualização envia o estado declarado inteiro com `PUT /topics/{topic}` (D215), porque o `PATCH` não consegue voltar o `idleTimeout`. `idle_timeout` não declarado é 600 numa fila volátil e `null` nas outras.
- **Conferido no `plan`**, antes de qualquer coisa chegar ao nó: as regras do nome (1 a 100 caracteres `A-Za-z0-9_-`, sem terminar em `-dead-letter`), as faixas e as tags de [16-api-cliente.md](16-api-cliente.md#regras-de-nome-tags-e-faixas), `idle_timeout` só com `volatile = true`, um `expires_at` no passado e `admin` com `topic_prefixes`.
- **A fila de dead-letter** (`<nome>-dead-letter`) nasce e morre com a fila e é só leitura: não é um recurso, só `dead_letter_name`.
- **Data sources:** `taba_topic` (pelo nome, inclusive uma fila de dead-letter), `taba_topics` (por fragmento do nome, `type`, `tags`, `include_dead_letters`), `taba_token` (pelo `id`) e `taba_tokens` (por fragmento do nome, do mais antigo ao mais novo). Nunca devolvem um segredo, e servem a quem só **usa** filas criadas por outro time.
- **Erros** do nó mantêm o código, e os `details` viram linhas `- campo: mensagem`.

## Plan, desvio e import

- **Leitura:** um `GET` por recurso. Um tópico ou token que sumiu (`404`) sai do estado, e o `plan` propõe criá-lo de novo. Uma mudança feita fora do Terraform aparece como diferença a desfazer.
- **Import:** um tópico pelo nome (`terraform import taba_queue.orders orders`, ou um bloco `import {}`), um token pelo `id`. Importar um distribuidor como `taba_queue` (ou o contrário), ou uma fila de dead-letter, é erro, não substituição. Depois de um import, `force_destroy` é `false` e `create_or_update` é `null`.
- **A data do token:** o nó só aceita e devolve `AAAA-MM-DDTHH:MM:SS.mmmZ`. O provider aceita qualquer RFC 3339 em `expires_at`, envia o formato do nó e guarda o texto declarado enquanto for o mesmo instante, então `2027-01-01T00:00:00Z` não é diferença com `...00.000Z`, nem depois de um import.

## Proteger uma fila com mensagens

O `force_destroy` existe porque `DELETE /topics/{topic}` sempre funciona e apaga todas as mensagens, até as pendentes. Por isso o provider remove uma fila com `DELETE ?ifEmpty=true` (D216), a não ser com `force_destroy = true`, como o `aws_s3_bucket` do provider da AWS (D219):

| Situação | `force_destroy = false` (padrão) | `force_destroy = true` |
|---|---|---|
| Campo mutável mudou | `PUT` | `PUT` |
| Campo imutável mudou (`-/+`) | `DELETE ?ifEmpty=true` + `POST` | `DELETE` + `POST` |
| `terraform destroy` | `DELETE ?ifEmpty=true` | `DELETE` |
| Adotado por `create_or_update` com outro campo imutável | `PUT ?recreate=if-empty` | `PUT ?recreate=if-empty` |

Uma fila ou dead-letter com mensagens é então o erro `409 topic.not-empty`, que diz quantas há e as duas saídas: esvaziá-la, ou **aplicar** `force_destroy = true` antes. O valor é lido do estado, então defini-lo no mesmo `apply` que substitui a fila não vale (como no `aws_s3_bucket`). A conferência é de melhor esforço (PRB-103). Um distribuidor não guarda mensagens e não tem `force_destroy`.

## Adotar o que já existe

O `create_or_update` vem desligado por padrão: criar algo que já existe é erro, como o Terraform faz normalmente, porque adotar em silêncio o que outra configuração gerencia é perigoso (D218). O erro do `409 topic.already-exists` nomeia as duas saídas: `import`, ou `create_or_update = true`.

O bloco `import {}` cobre "sei que existe". O `create_or_update` cobre "pode ou não existir": a mesma configuração aplicada em ambientes onde parte foi criada à mão, pelo `taba-cli` ou por uma aplicação no arranque, sem um passo manual por ambiente. Liga-se no provider (todos os recursos) ou por recurso (`create_or_update = true|false` vence o do provider). Só vale na criação; depois o recurso pertence ao estado como qualquer outro, e o `destroy` **remove o que foi adotado**.

**Fila e distribuidor:** um `PUT /topics/{name}?recreate=if-empty` com o estado declarado, decidido de forma atômica pelo dono da definição (D210):

- ausente: criado, sem aviso;
- existente: levado ao estado declarado (o não declarado volta ao padrão do nó), com o aviso "Took over";
- outro tipo, `fifo` ou `volatile`: recriado só se a fila e a dead-letter estiverem vazias, com o aviso "Recreated"; com mensagens, `409 topic.not-empty` vira erro com as contagens e a dica de `import` + `force_destroy`, e o tópico não é tocado. A adoção **nunca** usa `recreate=always`: o `plan` só mostrou `+ create`, e apagar mensagens ali seria uma destruição que o `plan` não mostrou;
- `503`: "nada foi alterado, tente de novo"; `409 cluster.conflict` se repete até três vezes.

**Token** (o nome não é único e o segredo nunca volta): antes do `POST`, o provider lista os tokens com o nome (a mesma lista que a criação já faz) e fica com os de nome exato. Nenhum: cria. Um com as mesmas permissões e prefixos: adota, corrige `expires_at` por `PATCH` só se for outro instante (`null` declarado tira o prazo), deixa `token` **nulo** e avisa que o segredo não pode ser recuperado (`terraform apply -replace` cria um novo). Vários, ou um com outras permissões ou prefixos: erro com os ids, nada alterado. Essa adoção é do lado do cliente e não é atômica (PRB-106).

## Tokens

- **O segredo fica no estado.** `token` só é conhecido na resposta da criação, então o estado o guarda: trate o estado como segredo (um backend remoto cifrado, acesso restrito). Um token adotado ou importado tem `token = null`.
- **Rotação:** `terraform apply -replace=taba_token.x` cria um token novo e revoga o antigo; com `lifecycle { create_before_destroy = true }` não há momento sem token.
- **Vencimento:** o nó esconde um token vencido (`404`, como um revogado), então ele sai do estado e o `plan` propõe criá-lo; como o `expires_at` está no passado, o `plan` recusa com "o token venceu: mude `expires_at` (um token novo) ou remova o recurso". Nada é recriado em silêncio.
- **Autenticação desligada** (`404 route.not-found` em `/tokens`), um token do provider que não administra tokens (`403 auth.forbidden`) e `401` são erros com uma dica. Uma leitura com a autenticação desligada não tira o token do estado.
- **Uma resposta de criação perdida.** Um `POST /tokens` respondido com `503 cluster.quorum-unavailable` pode ter criado o token no dono da definição ([16-api-cliente.md](16-api-cliente.md#tokens)). O provider lista os tokens com o nome **antes** do `POST`; depois do `503`, o órfão é o token igual (nome, permissões, prefixos) que não estava nessa lista. Um: revogado, e o `POST` tentado de novo. Nenhum: tentado de novo. Dois ou mais: erro com os ids, nada revogado. As tentativas ficam dentro do `retry_max_wait`; quando ele acaba, erro depois de revogar. O limite residual é a PRB-107.

## Novas tentativas e um nó fora

As chamadas repetem `429` e `503`, como diz o contrato comum dos conectores ([21-conectores.md](21-conectores.md#o-contrato-comum)), esperando o `Retry-After` ou 1, 2, 4... até 64 s, e não fazem a tentativa cuja espera passaria do `retry_max_wait` (o `Config.RetryMaxWait` do conector Go, [21-conectores.md](21-conectores.md#go-githubcomrafaelpontanitabaconnectorsgo-package-taba)). O padrão `3m` vem de uma medida: um nó que caiu continua membro, e dono de parte das definições, até os outros o excluírem por votação (uns 65 s no cluster do Compose), e nesse intervalo uma criação que é dele recebe `503 cluster.quorum-unavailable` com `Retry-After: 5`. Um orçamento contado em tentativas (8 tentativas, 35 s) fez o `apply` falhar 4 vezes em 5 (D220). `0s` desliga as novas tentativas.

Um `POST /topics` respondido com `503 cluster.commit-timeout` "vai se completar": se a nova tentativa recebe `409 topic.already-exists`, o provider lê o tópico e o assume como criação sua só se ele for o declarado.

## Junto com o chart Helm

O nó e a configuração dele são o chart Helm ([34-chart-helm.md](34-chart-helm.md)), que o Terraform instala pelo provider `helm`; as filas, distribuidores e tokens são este provider. O token root está no Secret `<release>-secrets`, chave `root-token`:

```hcl
resource "helm_release" "taba" {
  name             = "taba"
  repository       = "oci://ghcr.io/rafaelpontani/charts"
  chart            = "taba"
  version          = "0.2.0"
  namespace        = "taba"
  create_namespace = true
  wait             = true
}

data "kubernetes_secret" "taba" {
  metadata {
    name      = "taba-secrets"
    namespace = "taba"
  }
  depends_on = [helm_release.taba]
}

provider "taba" {
  endpoint = "https://taba.example.com:8080"
  token    = data.kubernetes_secret.taba.data["root-token"]
}
```

Um provider configurado a partir de um recurso da mesma configuração só funciona quando esse recurso já existe na hora do `plan`; uma configuração separada para o cluster (ou `-target` na primeira vez) é a saída de costume.

## Fora do escopo

- Operação do cluster (drain, backup, `cluster/stop`): ações, não estado desejado; ficam no `taba-cli` e na API de operação.
- Configuração e implantação do nó: o chart Helm.
- Grupos de consumo e as chaves do gateway SQS/SNS: não têm definição na API de clientes.
- Mensagens.

## Testes

| Nível | Onde | O quê |
|---|---|---|
| Unitários | `internal/provider/*_test.go` (menos `acceptance_test.go`) | Um servidor HTTP falso que grava cada requisição: corpos, rotas, conferências do `plan`, `create_or_update`, a resposta perdida do token, o orçamento de novas tentativas |
| Aceitação | `internal/provider/acceptance_test.go` (`TF_ACC=1`) | 10 testes com terraform-plugin-testing contra um nó com autenticação ligada, com **Terraform e OpenTofu**: criar, import com verificação, desvio, `force_destroy`, substituição, tokens, `create_or_update`, data sources; depois de cada passo o framework planeja de novo e falha se o `plan` não for vazio |
| Três nós | `scripts/cluster.sh` | O cluster do Compose: aplica 10 filas e 10 tokens, mata o `taba-2`, aplica 40 de cada logo em seguida (cada um criado uma vez só), `plan` vazio, o nó de volta, `plan` vazio de novo, `destroy`. Uns 5 minutos; à mão antes de um release que mude o provider, não no CI |

`scripts/test.sh` roda tudo em contêineres (`unit`, `lint` com golangci-lint, `docs` confere que o `docs/` regerado pelo `tfplugindocs` é igual ao do repositório, `acc` com as duas CLIs); as CLIs são baixadas uma vez, conferidas pelo `SHA256SUMS` delas. Os jobs do CI estão em [27-ci-cd.md](27-ci-cd.md). Casos de uso `UC-IAC-001` a `UC-IAC-023` ([26a](26a-casos-de-uso-implementados.md)).

## Publicação

O Terraform Registry e o OpenTofu Registry só leem um repositório público chamado `terraform-provider-<nome>` com releases assinados, então o provider é publicado por um **repositório espelho**, `rafaelpontani/terraform-provider-taba`, alimentado pelo `release.yml`; o código continua aqui (D211).

1. `scripts/package.go` compila as 13 plataformas (`-trimpath`, sem cgo, zips com data fixa: o mesmo commit gera os mesmos arquivos), o `_manifest.json` (protocolo 6.0) e o `_SHA256SUMS`. Os zips vão para a Release do Taba no GitHub da versão.
2. Só quando a variável `TERRAFORM_PROVIDER_REPO` existe, o job `terraform-registry` assina o `SHA256SUMS` (um `.sig` binário destacado, com uma chave **RSA**: o Terraform Registry recusa chaves ECC), roda `scripts/verify-release.sh` contra a imagem `taba:<versão>` (somas, assinatura, instalação da versão exata a partir dos arquivos com Terraform e OpenTofu, `apply`, `plan` vazio, `destroy`) e só então `scripts/mirror.sh`: a fonte de `connectors/terraform` com o conector Go em `go/` e a licença dos conectores (`connectors/LICENSE`, Apache-2.0, D164; o script para se o `LICENSE` do espelho não for Apache), um commit, a tag `v<versão>` e uma Release com os arquivos (pré-release no `-rc.N`).
3. Os registries leem o espelho sozinhos: o Terraform Registry pelo namespace `rafaelpontani`, reivindicado no HCP Terraform (organização `taba`, Registry → Public namespaces), que guarda a chave pública e um webhook no espelho; o OpenTofu Registry pela entrada do provider e da chave em `opentofu/registry`. Uma Release nova no espelho é uma versão nova nos dois, sem passo à mão. O `terraform init` confere a assinatura com essa chave (`self-signed, key ID 9CF9BC950E956E3F`), e o `tofu init` com a mesma (`signed, key ID 9CF9BC950E956E3F`).

O que o mantenedor cria está em [27-ci-cd.md](27-ci-cd.md#provider-terraform).

**A documentação no OpenTofu Registry.** O OpenTofu Registry instala qualquer provider, mas só mostra a documentação (as páginas de `docs/`) de uma versão cujo repositório tem uma licença da lista aprovada por ele (Apache-2.0, MPL-2.0, MIT, BSD e outras; não GPL nem AGPL), e responde `451 Unavailable for Legal Reasons` para as outras. Até a `v0.3.11` o espelho levava por engano o `LICENSE` do servidor (AGPL-3.0), então essas versões não têm documentação lá (as tags do espelho não são reescritas); da próxima versão em diante ele leva o `connectors/LICENSE`. As páginas também estão em `connectors/terraform/docs/` e no `docs/` de cada tag do espelho.

## Limites

- **PRB-106:** a adoção de um token não é atômica; dois `apply` ao mesmo tempo podem criar dois tokens com o mesmo nome (o seguinte acusa a ambiguidade). Os tópicos não têm isso, por causa do `PUT`.
- **PRB-107:** depois de uma resposta de criação perdida, um token igual criado por outra pessoa entre a lista e o `503` seria revogado.
- **PRB-109:** só o conector Go administra tokens, então o cenário de contrato dos conectores não os cobre.
- A conferência de vazio do `force_destroy` é de melhor esforço (PRB-103), e durante uma atualização gradual um `PUT` pode falhar (PRB-104).

## Fontes e provas

- `connectors/terraform/internal/provider/` (provider, recursos, data sources), `connectors/terraform/scripts/` (testes, empacotamento, publicação), `connectors/go/tokens.go` e `connectors/go/transport.go` (tokens e `RetryMaxWait`), os jobs `connector-terraform`, `terraform-provider` e `terraform-registry` de `.github/workflows/`.
- Decisões D210, D211, D215, D216, D218 a D221 em [30-decisoes.md](30-decisoes.md); problemas PRB-103, PRB-104, PRB-106, PRB-107 e PRB-109 em [31-pendencias.md](31-pendencias.md).
- Casos de uso `UC-IAC-001` a `UC-IAC-023` em [26a](26a-casos-de-uso-implementados.md), e `UC-CNT-044` (tokens no conector Go).
