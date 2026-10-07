# API

[← Índice](README.md)

## 8. API (rascunho inicial)

> Contrato formal (OpenAPI) ainda não definido — esta seção lista os verbos e recursos previstos até o momento.

O Taba expõe **três APIs HTTP**, cada uma em sua porta e para um público. Cada uma tem o seu arquivo. A **numeração das seções é contínua** entre eles (a seção 8.1.1, por exemplo, está no arquivo da API de operação), e este arquivo reúne o que vale para as três: as portas e a segurança da porta do cluster.

| API | Arquivo | Seções | Porta | Quem usa |
|---|---|---|---|---|
| **API de cliente** | [06a-api-cliente.md](06a-api-cliente.md) | 8.1 (tópicos, mensagens, tokens), 8.1.2 (paginação), 8.1.3 (autenticação por token) | `api.port` | Aplicações |
| **API de operação** | [06b-api-operacao.md](06b-api-operacao.md) | 8.1.1 (métricas, saúde, drain) | `operations.port`, ou `api.port` | Operadores, Prometheus, orquestrador |
| **API do cluster** | [06c-api-cluster.md](06c-api-cluster.md) | 8.2 (rotas entre nós), 8.2.1 (streams NDJSON) | `cluster.port` | Os outros nós |
| **Convenções e DTOs padronizados** | [06d-api-convencoes.md](06d-api-convencoes.md) | 8.3 (formato, cabeçalhos, `ErrorResponse`, `Page<T>`, códigos de status e de erro) | — | Vale para as três APIs |

Cada arquivo é um **manual**: para cada rota traz a permissão, os parâmetros, o DTO de entrada, o DTO de saída, os códigos de status com o código de erro de cada um, e um exemplo. Os códigos de status são os do **HTTP**: nenhum DTO os repete no corpo.

### 8.0 Portas: cliente, cluster e operação

O nó atende em **portas separadas, uma por público**. Assim cada porta é liberada ou bloqueada na rede (firewall, grupo de segurança, `NetworkPolicy`) sem olhar o caminho da requisição, e restringir a API interna não depende de filtro na aplicação.

| Porta | Parâmetro | Padrão | Quem acessa | O que serve |
|---|---|---|---|---|
| **API de cliente** | `api.port` | A definir (sugestão: 8080) | Aplicações clientes, em geral pelo balanceador | As rotas da seção 8.1 ([06a](06a-api-cliente.md)). Sem `operations.port`, também as rotas de operação (seção 8.1.1, [06b](06b-api-operacao.md)). |
| **Cluster** | `cluster.port` | A definir (sugestão: 8081) | **Só os outros nós** (mais o balanceador interno do bootstrap, abaixo) | As rotas `/cluster/*` da seção 8.2 ([06c](06c-api-cluster.md)) e o canal WebSocket `/cluster/channel`. |
| **Operação** (opcional) | `operations.port` | Vazia (usa `api.port`) | Prometheus, orquestrador, operadores | `/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)). |

- **Separação por porta, não só por caminho.** São listeners diferentes do mesmo processo. A porta de cliente responde `404` a `/cluster/*`; a porta do cluster responde `404` a tudo que não seja `/cluster/*`. As três portas precisam ser diferentes entre si; valores repetidos, ou uma porta ocupada, impedem o nó de subir (a mesma falha explícita de `operations.port`).
- **A tabela de membros guarda o IP e a `cluster.port` de cada nó** (seção 6.2 de [04-cluster.md](04-cluster.md)). Toda chamada entre nós vai **direto** ao IP e à porta do cluster do par, sem balanceador no caminho (seção 8.2.1, [06c](06c-api-cluster.md)). A única chamada que passa por um balanceador é a primeira do bootstrap, `GET /cluster/nodes`, e por isso `cluster.entry-address` é o endereço da **porta do cluster** do balanceador.
- **Dois listeners no balanceador.** Um para `api.port`, o do tráfego de clientes. Outro, **interno**, para `cluster.port`, usado só no bootstrap; em Kubernetes pode ser um `Service` interno (ou *headless*) em vez de um balanceador. A porta do cluster **nunca** é publicada no balanceador de clientes, no `-p` do Docker nem fora da rede dos nós.
- **A porta do cluster não tem rate limit nem leitura de IP do cliente.** O tráfego é entre nós, e os cabeçalhos de `client-ip.headers` são ignorados nela ([12-limites-api.md](12-limites-api.md)). Quem repassa uma chamada de cliente a outro nó leva o IP já resolvido em cabeçalho interno (seção 6.9 de [04-cluster.md](04-cluster.md)).
- **Autenticação:** a porta do cluster usa TLS mútuo com os certificados do cluster (seção 8.0.1). A API de cliente tem **autenticação opcional por token** (seção 8.1.3, [06a](06a-api-cliente.md)). As rotas de operação **não têm autenticação**; a proteção delas é de rede.
- **Modo desenvolvimento** (seção 6.4 de [04-cluster.md](04-cluster.md)): a porta do cluster **não é aberta** e `cluster.port` é ignorado.

#### 8.0.1 Segurança da porta do cluster

A comunicação entre nós usa **TLS mútuo com certificados do cluster**, gerados pelo operador e lidos de **uma pasta**. Não há CA nem certificado por nó, e a confiança é a dos próprios certificados (*pinning*). A pasta pode ter **vários certificados ao mesmo tempo**, cada um com a sua data de início e de expiração, e o nó a **recarrega sozinho**: trocar o certificado não exige reiniciar nenhum nó.

- **Geração.** `taba init --out <pasta>`, da própria imagem, cria de uma vez a **pasta de segredos**: o primeiro certificado (em `tls/`) e o token raiz (`root-token`, seção 8.1.3 de [06a](06a-api-cliente.md)). Para gerar só um certificado, como numa rotação, use `taba cert generate --days <n> [--starts-in <duração>] --out <pasta>`: cria um par `cluster-<data de início>.crt` e `.key` (o nome traz a data, para gerações sucessivas não se sobrescreverem), autoassinado e válido para servidor e cliente. `--starts-in` define o `notBefore` no futuro (padrão: agora, com uma folga de alguns minutos para diferença de relógio). O operador o roda quando precisa de um certificado novo, **não a cada início de container**, e entrega a pasta a todos os nós como segredo da plataforma (Secret do Kubernetes, Docker secret ou volume).
- **Configuração.** A pasta é `cluster.tls.dir`, cujo padrão é `<secrets.dir>/tls` (`secrets.dir` vale `/etc/taba/secrets`); `cluster.tls.reload-interval` define de quanto em quanto tempo ela é conferida ([10-configuracao.md](10-configuracao.md)). Basta **montar a pasta de segredos** no nó: o TLS liga **sozinho** quando a pasta `tls` existe, sem variável de ligar, e a porta do cluster passa a exigir TLS. Cada par é um `<nome>.crt` mais um `<nome>.key` de mesmo nome; um `.crt` sem `.key` serve só para confiar.
- **Quem o nó aceita.** Quem apresentar um certificado igual a um da pasta, ou emitido por um deles, desde que ele **ainda não tenha expirado** (`notAfter`). A data de início não conta para a confiança, para um nó aceitar o certificado novo antes de qualquer um passar a apresentá-lo.
- **Qual o nó apresenta.** O par de **início mais recente que já começou a valer** (`notBefore` ≤ agora) e ainda não expirou. Isso vale ao **receber** e ao **abrir** uma conexão (chamadas HTTP e o WebSocket `/cluster/channel`). Cada lado exige que o outro apresente um dos certificados aceitos; sem isso o handshake falha e nenhuma rota é alcançada. Os nós se conectam por **IP** (o da tabela de membros), então o nome do host **não é verificado**.
- **Sem a pasta** (o padrão não existe e `cluster.tls.dir` não foi definido), a porta do cluster atende em HTTP puro e o nó registra um aviso (`warn`) no log. Com a pasta existente, o nó **não sobe** se não houver nela ao menos um par válido; e um `cluster.tls.dir` definido que não existe também impede a subida. Em `mode=development` a porta não é aberta e o TLS é ignorado.
- **Balanceador.** O listener da porta do cluster no balanceador precisa **repassar o TLS sem abri-lo** (TCP/L4, como um NLB), ou o `cluster.entry-address` aponta para um nome DNS que resolve direto para os nós (por exemplo, um `Service` *headless*). Um balanceador que **termina** o TLS (como um ALB) perderia o certificado do cliente e derrubaria a autenticação. Só o bootstrap passa por ele (seção 8.0). O TLS da API de cliente (ex.: certificado do ACM no ALB) é do balanceador e independe disto.
- **Rotação, sem reiniciar nenhum nó:**
  1. Gerar o certificado novo com início no futuro: `taba cert generate --days 1825 --starts-in 14d --out <pasta>`.
  2. Acrescentar o par novo à pasta (por exemplo, nova entrada no Secret). Cada nó o percebe em até `cluster.tls.reload-interval` e passa a **confiar** nele, mas segue apresentando o antigo.
  3. Quando a data de início chega, **todos os nós passam a apresentar o novo sozinhos**, sem nenhuma ação.
  4. O antigo deixa de ser aceito quando expira; o operador pode retirá-lo da pasta antes.

  O prazo de `--starts-in` precisa ser maior que o tempo de o par novo chegar a **todos** os nós (a propagação do Secret e o `reload-interval`): um nó que ainda não tem o novo, quando a data chega, passa a rejeitar quem o apresenta. Os relógios dos nós devem estar em NTP (seção 13.3 de [14-invisibilidade.md](14-invisibilidade.md)). Conexões já abertas, como o canal WebSocket, seguem com o certificado antigo até caírem, o que é inofensivo porque ele continua aceito até expirar.
- **Recarga.** Se uma recarga encontra arquivo ilegível, par incompleto ou certificado inválido, o nó **mantém o conjunto anterior** e registra `error` no log. O conjunto só muda quando a pasta inteira é válida.
- **Expiração.** Um certificado expirado deixa de ser aceito e de ser apresentado. Para não chegar lá, o nó registra `warn` no log a partir de `cluster.tls.expiry-warning` (padrão 30 dias) antes do `notAfter` do certificado em uso, e expõe a métrica `taba.cluster.tls.expires_in` ([11-observabilidade.md](11-observabilidade.md)), para alertar no Prometheus.
- **Limite aceito.** Quem obtiver a chave de **um** nó pode se passar por qualquer nó, o mesmo nível de risco de um segredo compartilhado, mas com tráfego cifrado. Quem quiser um certificado por nó, emitido por uma CA (cert-manager, malha de serviço), pode colocar na pasta o `.crt` da CA para confiar nela e o par do próprio nó para apresentar.

**Rotas de operação:** `/metrics`, `/metrics/topics/*`, `/health/*` e `/node/drain` ficam **sem autenticação**, por decisão. Quem instala protege com a porta própria (`operations.port`) e com a rede (grupo de segurança, `NetworkPolicy`) e, em complemento, com `operations.allowed-ips` ([11-observabilidade.md](11-observabilidade.md#acesso-às-rotas-de-operação)).

**Exemplo com Docker.** Uma vez, o operador cria a pasta de segredos e a entrega a todos os nós:

```bash
docker run --rm -v "$PWD/secrets:/out" taba init --out /out
```

Cada nó monta a pasta e informa o endereço do cluster (`--stop-timeout` deve ser maior que `drain.timeout`; a porta do cluster e a de operação ficam presas à interface privada do host):

```bash
docker run -d --name taba-1 --stop-timeout 30 \
  -p 8080:8080 -p 10.0.0.5:8081:8081 -p 10.0.0.5:9090:9090 \
  -v taba-data:/var/lib/taba \
  -v "$PWD/secrets:/etc/taba/secrets:ro" \
  -e TABA_CLUSTER_ENTRY_ADDRESS=taba-interno.exemplo:8081 \
  taba
```

No primeiro nó de um cluster novo, acrescente `-e TABA_CLUSTER_FIRST_NODE=true`. No modo desenvolvimento basta `-e TABA_MODE=development` e a porta `8080`, sem pasta de segredos.
