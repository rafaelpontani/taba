---
título: Segurança
resumo: Quem confia em quem no Taba — tokens da API de cliente, TLS mútuo entre nós e rotação de certificados, rate limit, IP do cliente, rotas de operação sem autenticação e os riscos conhecidos.
tipo: explicação
status: estável
verificado-em: 2026-10-13 @ 71183b9
fontes: [crates/taba-node/src/secrets, crates/taba-node/src/cluster/tls, crates/taba-node/src/http/auth, crates/taba-node/src/http/ratelimit, crates/taba-node/src/http/client_ip, crates/taba-node/src/engine/tokens, crates/taba-node/src/core/token]
---

# Segurança

> Leia antes: [04-arquitetura.md](04-arquitetura.md) (as três portas).

## Fronteiras de confiança

O Taba separa os públicos **por porta** (não só por caminho), para a rede (firewall, grupo de segurança, `NetworkPolicy`) liberar ou bloquear cada um sem olhar a requisição:

| Porta | Público | Como se protege |
|---|---|---|
| **Cliente** (`api.port`, 8080) | Aplicações | **Token** opcional (liga sozinho com um token raiz) + rate limit. Use HTTPS no balanceador |
| **Cluster** (`cluster.port`, 8081) | **Só os outros nós** | **TLS mútuo** com o certificado do cluster. Nunca publicar fora da rede dos nós |
| **Operação** (`operations.port`) | Operador, Prometheus, balanceador | **Sem autenticação**: proteção só de rede (porta própria + `operations.allowed-ips`) |

![Zonas de confiança das três portas](../diagrams/img/pt-BR/security-zones.svg)

As três portas precisam ser **diferentes entre si**; valor repetido ou porta ocupada impede o nó de subir. A porta de cliente responde `404` a `/cluster/*`; a do cluster responde `404` a tudo que não seja `/cluster/*`.

## A pasta de segredos e `taba init`

`taba init --out <pasta>` cria, uma vez, a **pasta de segredos** que se monta em todos os nós (`secrets.dir`, padrão `/etc/taba/secrets`):

```
secrets/
  root-token                      43 caracteres (256 bits aleatórios em base64): o token raiz
  tls/
    cluster-<AAAAMMDD>.crt  certificado autoassinado do cluster
    cluster-<AAAAMMDD>.key
```

- `--days` é a validade do certificado (padrão **1825**, cinco anos).
- `taba cert generate --days <n> [--starts-in <duração>] --out <pasta>` cria **só** um par novo (para rotação). `--starts-in` (ex.: `14d`) adia o início da validade.
- `taba token generate` imprime um segredo aleatório de 256 bits (um token raiz avulso).
- **Montar a pasta liga tudo sozinho:** o TLS do cluster (se `tls/` existe) e a autenticação (se há `root-token`).
- Em produção, **gere uma vez** e entregue como segredo da plataforma (Secret do Kubernetes, Docker secret, volume): **não a cada início de contêiner**.

> **No Windows com Git Bash**, comandos Docker que recebem um caminho (`/secrets/root-token`) têm o caminho "traduzido" para o do Windows. Use `MSYS_NO_PATHCONV=1` (ou `docker compose exec taba-0 cat /etc/taba/secrets/root-token`).

## Autenticação por token (API de cliente)

### Quando liga

| Situação | Autenticação |
|---|---|
| Há um **token raiz** (arquivo `root-token` na pasta de segredos, `auth.root-token-file` ou `auth.root-token`) | **Ligada**: toda rota de cliente exige `Authorization: Bearer <token>` |
| Não há token raiz | **Desligada**: nenhuma rota exige nada |
| `auth.enabled=false` | Forçada desligada, mesmo com a pasta montada (para manter o TLS sem exigir token) |
| `auth.enabled=true` **sem** token raiz | O nó **não sobe** |
| **Modo desenvolvimento** | **Nunca** exigida; as rotas `/tokens` respondem `404` |

O token raiz precisa de **pelo menos 32 caracteres**, senão o nó não sobe. As rotas de operação e as `/cluster/*` **não** usam token.

### Os dois tipos de token

- **Token raiz:** vem da configuração, igual em todos os nós. **Acesso total**, sem limite de prefixos, e é o **único** que gerencia os tokens (`/tokens`). Existe desde o primeiro instante (não vem da tabela), então **não há "primeiro admin" para criar**. Nos logs e traces aparece como `token.id=root`. O `auth.root-token-file` é relido a cada `auth.reload-interval` (padrão 60 s) e aceita **duas linhas** (a atual e a anterior): dá para trocar o segredo sem reiniciar nenhum nó.
- **Token de acesso** (`tb_<id>.<segredo>`): criado pelo raiz via `POST /tokens`. O `id` é um GUID v7; o segredo tem 256 bits. **O cluster guarda só o hash salgado** (`SHA-256(sal ‖ segredo)`, sal aleatório de 128 bits por token), **nunca** o segredo, que aparece **só na resposta da criação**. O `id` do prefixo permite achar o token e identificá-lo em logs e traces **sem expor o segredo** (o token inteiro nunca é registrado).

Todo nó conhece todos os tokens: **validar é uma consulta local em memória**, sem rede. Criação, alteração e revogação passam pelo **dono da definição** e se espalham a todos ([08-armazenamento.md](08-armazenamento.md)); só o sal e o hash trafegam entre nós.

### Permissões e prefixos

| Permissão | Permite |
|---|---|
| `publish` | `POST /topics/{topic}/messages` |
| `consume` | `GET`, `PUT .../visibility` e `DELETE` (ack) de mensagens |
| `create` | `POST /topics` e o `autoCreate` do `GET` (que exige também `consume`). **Só cria** |
| `admin` | `PATCH` e `DELETE` em `/topics/{topic}`. **Inclui `create`** |

Todo token válido consulta `GET /topics` e `GET /topics/{topic}` dentro dos seus prefixos.

**`topicPrefixes`** (até 20, de 1 a 100 caracteres): o token só alcança tópicos cujo nome **começa com** algum prefixo; vazio = todos. `GET /topics` devolve só os tópicos dentro dos prefixos; `POST /topics` só aceita nome (e, num distribuidor, **cada `prefix`** de distribuição) dentro deles, para um token restrito não injetar mensagens em filas fora do alcance. A dead-letter é alcançada pelo prefixo da fila de origem. Na publicação num distribuidor, a permissão é conferida **só no distribuidor**.

### Exemplo verificado (cluster de 3 nós)

```bash
TOK=$(docker compose exec -T taba-0 cat /etc/taba/secrets/root-token)

curl -s -X POST localhost:8080/tokens -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' \
  -d '{"name":"servico-pedidos","permissions":["publish","create"],"topicPrefixes":["pedidos"]}'
# 201 {"id":"01a13649-…","token":"tb_01a13649-….<segredo>","permissions":["publish","create"],"topicPrefixes":["pedidos"],…}
```

| Chamada | Resultado |
|---|---|
| `GET /topics` sem token | `401 auth.unauthorized`, com `WWW-Authenticate: Bearer` (a resposta não diz *qual* o problema) |
| Publicar em `pedidos` com o token novo | `201` |
| Consumir (`GET`) com o mesmo token | `403 auth.forbidden` (falta `consume`) |
| Publicar em `t2` (fora do prefixo) | `403 auth.out-of-scope` |
| `GET /tokens` com o token novo | `403 auth.forbidden` (só o raiz) |

### Outras regras

- **Alterar:** só `name` e `expiresAt` (`null` remove o prazo). **Permissões e prefixos são imutáveis** (`400 validation.immutable-field`): para mudar o alcance, crie outro e revogue este.
- **Revogar:** vale em **todos** os nós no commit; as chamadas seguintes respondem `401`.
- **Validade:** `expiresAt` é absoluto e opcional; cada nó ignora o vencido e o apaga na varredura local (`retention.sweep.interval`), sem propagar. **Depende do relógio** dos nós (NTP).
- **Nó em sincronização ou em drain:** não tem a tabela completa e **não valida o token**: repassa o `Authorization` e **o destino confere**.
- Logs: token criado, alterado ou revogado (`info`, com o `id`); `401`/`403` (`warn`, `debug` em volume).
- **PRB-017:** a validação de um `id` desconhecido responde um pouco mais rápido que a de um `id` existente com segredo errado (não calcula o hash): em tese permite descobrir `id`s por tempo de resposta. O segredo continua protegido.

## TLS mútuo entre os nós

A porta do cluster usa **TLS mútuo com um certificado único do cluster**, lido de **uma pasta** (`cluster.tls.dir`, padrão `<secrets.dir>/tls`) e **recarregada sozinha** a cada `cluster.tls.reload-interval` (padrão 60 s). Não há CA nem certificado por nó: a confiança é a dos **próprios certificados** (*pinning*).

| Regra | Efeito |
|---|---|
| **Quem o nó aceita** | Quem apresenta um certificado **igual** a um da pasta, ou **emitido** por um deles (uma CA posta na pasta), desde que ele **não tenha expirado** (`notAfter`). A data de início **não** conta para a confiança: um nó aceita o certificado novo antes de qualquer um passar a apresentá-lo |
| **Qual o nó apresenta** | O par de **início mais recente que já começou a valer** (`notBefore` ≤ agora) e ainda não expirado, ao **receber** e ao **abrir** conexões (HTTP e o WebSocket do canal) |
| **Nome do host** | **Não é verificado**: os nós se conectam por **IP** (o da tabela de membros) |
| **Sem a pasta** | A porta do cluster atende em **HTTP puro** e o nó registra `warn` |
| **Pasta definida e inexistente, ou sem par válido** | O nó **não sobe** |
| **Modo desenvolvimento** | A porta não é aberta |

**Identidade de quem chama:** como todos usam o mesmo certificado, o cluster reconhece o chamador pelo **IP de origem** da conexão e confere na tabela de membros. Por isso o nó informa os seus IPs (`cluster.advertise-addresses`, ou a interface que sai para o `cluster.entry-address`).

### Rotação sem reiniciar nada

1. Gere o par novo com início no futuro: `taba cert generate --days 1825 --starts-in 14d --out <pasta>`.
2. Acrescente-o à pasta (ex.: nova entrada no Secret). Em até `cluster.tls.reload-interval`, cada nó passa a **confiar** nele, mas segue apresentando o antigo.
3. Quando a data de início chega, **todos passam a apresentar o novo sozinhos**.
4. O antigo deixa de ser aceito ao expirar; pode ser retirado antes.

O `--starts-in` precisa ser **maior** que o tempo para o par novo chegar a **todos** os nós (propagação do Secret + `reload-interval`): um nó que ainda não o tem, quando a data chega, rejeita quem o apresenta. Conexões já abertas (o canal) seguem com o antigo até cair, o que é inofensivo.

**Recarga com erro:** arquivo ilegível, par incompleto ou certificado inválido → o nó **mantém o conjunto anterior** e registra `error`; só muda quando a pasta inteira é válida.

**Expiração:** um expirado deixa de ser aceito e apresentado. O nó registra `warn` a partir de `cluster.tls.expiry-warning` (padrão **30 dias**) e expõe `taba.cluster.tls.expires_in` para alertar no Prometheus.

### Balanceador na frente da porta do cluster

Só o primeiro passo do bootstrap (`GET /cluster/nodes`) passa por um balanceador (`cluster.entry-address`). Esse listener deve **repassar o TLS sem abri-lo** (TCP/L4, como um NLB) ou ser um nome DNS que resolve direto para os nós (um `Service` *headless*). Um balanceador que **termina** o TLS (como um ALB) perde o certificado do cliente e derruba a autenticação. (No exemplo do Compose, o HAProxy usa modo TCP para a porta 8081.) O HTTPS da API de **cliente** é do balanceador e independe disto.

### Limite aceito

Quem obtiver a chave de **um** nó pode se passar por **qualquer** nó: o mesmo risco de um segredo compartilhado, mas com tráfego cifrado. Quem quer certificado por nó pode usar uma CA (cert-manager, malha de serviço): ponha o `.crt` da CA na pasta para confiar e o par do próprio nó para apresentar.

## Rate limit

Limite de requisições **por nó**, **token bucket** por categoria e por chave. **Desligado por padrão**: sem `ratelimit.*.rps` a categoria **não tem limite**. O `ratelimit.burst` (estouro permitido) vale `ceil(rps)` por categoria se ausente.

| Categoria | Rotas | Chave |
|---|---|---|
| **Publicação** (`ratelimit.publish.rps`) | `POST /topics/{topic}/messages` | `id` do token válido (o raiz tem o contador `root`); sem token válido, o **IP do cliente** |
| **Consumo** (`ratelimit.consume.rps`) | `GET` de mensagens, `PUT .../visibility`, `DELETE` (ack) | idem |
| **Administrativo** (`ratelimit.admin.rps`) | `/topics*` e `/tokens*` | idem |
| **Operação** (`ratelimit.ops.rps`) | `/health/cluster?scope=cluster` e `/metrics/topics` | sempre o **IP** |

- Estourado: `429 rate-limit.exceeded` com `Retry-After`.
- **Isentas:** `/health/live`, `/health/ready`, `/metrics` e `/health/cluster?scope=node`: um `429` no `/health/ready` faria o balanceador tirar o nó por engano.
- **Contadores locais a cada nó**: um cliente espalhado por N nós pode chegar a N vezes o limite (aceito).
- **Ordem:** autenticação (local) → rate limit → recusa por drain/sincronização → autorização. O `401` também conta **por IP**, para quem tenta adivinhar tokens não ficar sem limite; um `403` conta no token.
- Todos os tokens têm o **mesmo** limite; limite por token não existe.
- Nó em sincronização/drain conta **por IP** e repassa; o destino não conta de novo. A **porta do cluster não tem rate limit**.
- Métrica: `taba.ratelimit.rejected` (atributo `key`: `token` ou `ip`).

## IP do cliente e proxies confiáveis

Para as chamadas contadas **por IP** (e para `operations.allowed-ips`), o IP pode vir de **cabeçalho**, o que importa atrás de balanceador:

| Chave | Padrão | Efeito |
|---|---|---|
| `client-ip.headers.enabled` | `true` | Liga o uso dos cabeçalhos; desligado, vale sempre o endereço da conexão |
| `client-ip.headers` | `X-Forwarded-For` | Cabeçalhos consultados, em ordem; vale o primeiro presente. Em lista separada por vírgula vale o **primeiro** endereço (o cliente original). Ausente ou inválido: cai no endereço da conexão |
| `client-ip.trusted-proxies` | vazia | Lista de IPs/CIDR: os cabeçalhos só valem se a conexão vier de um deles |

> ⚠️ **PRB-015:** com `client-ip.trusted-proxies` **vazia** (o padrão), **qualquer cliente forja** `X-Forwarded-For` e escolhe o próprio IP: escapa do rate limit por IP e da lista `operations.allowed-ips`. **Em produção, preencha `trusted-proxies` com o IP do balanceador, ou desligue `client-ip.headers.enabled` se não há balanceador.**

## Rotas de operação: proteção de rede

`/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain` **não têm autenticação** (decisão D63). Use **a porta própria** (`operations.port`), a **rede** (grupo de segurança, `NetworkPolicy`) e, em complemento, `operations.allowed-ips` (403 `ops.ip-not-allowed`). Atenção ao `POST /node/drain`: **qualquer um que alcance a porta de operação pode tirar um nó do cluster**. Detalhes em [13-observabilidade.md](13-observabilidade.md).

## Resumo dos riscos conhecidos

| Código | Risco | Mitigação |
|---|---|---|
| PRB-015 | `X-Forwarded-For` forjável com `trusted-proxies` vazia | Preencher a lista ou desligar os cabeçalhos |
| PRB-017 | Tempo de resposta diferente para `id` de token desconhecido | Sem efeito sobre o segredo |
| PRB-051 | O cluster reconhece o chamador pelo IP de origem; nó com várias interfaces pode falhar | Fixar `cluster.advertise-addresses` |
| D63 | Rotas de operação sem autenticação | Porta própria + rede + `allowed-ips` |
| — | Quem tem a chave de um nó se passa por qualquer nó | Isolar a porta do cluster; CA se necessário |
| — | O token trafega em claro se o balanceador não termina o TLS | HTTPS na frente da API de cliente |

## Fontes e provas

- `secrets/mod.rs` (`taba init`, `cert generate`, `token generate`), `cluster/tls/mod.rs` (aceitação, apresentação, recarga), `http/auth` (`authenticate`, `authorize`), `core/token` (formato e hash), `engine/tokens`, `http/ratelimit`, `http/client_ip`, `cluster/service` (identidade pelo IP).
- Tabela "Exemplo verificado" executada contra um cluster de 3 nós subido pelo `docker-compose.yml`.
- Casos de uso: UC-AUT (autenticação e tokens), UC-LIM (rate limit e IP), UC-SEG (portas, TLS mútuo e certificados); veja [26a](26a-casos-de-uso-implementados.md). Testes: `tests/auth.rs`, `ratelimit.rs`, `mtls.rs`, `secrets.rs`.
- Histórico: `06-api.md` (8.0), `06a-api-cliente.md` (8.1.3), `12-limites-api.md`.
