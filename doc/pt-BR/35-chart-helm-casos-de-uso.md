---
título: Chart Helm — casos de uso
resumo: Os modos de operação do chart Helm do Taba (mínimo, desenvolvimento, segredos próprios, Ingress, backup em S3 embutido (SeaweedFS) ou externo, restauração, coletores, OTLP, autoscaling com KEDA, upgrade, configuração avançada e falhas), cada um com seus values, o resultado esperado e os testes em bash que o provam.
tipo: guia
status: estável
verificado-em: 2026-10-16 @ 4faa639
fontes: [deploy/helm/tests, deploy/helm/taba/ci, deploy/helm/taba/values.yaml]
---

# Chart Helm — casos de uso

> Leia antes: [34-chart-helm.md](34-chart-helm.md) (os values). Aqui cada **modo de operação** do chart tem os values que o ligam, o que se espera ver e os **testes** que provam. Os conceitos de operação (escalar, drenar, atualizar) estão em [23-operacao.md](23-operacao.md).

## Como rodar os testes

Os testes são scripts bash em `deploy/helm/tests/`. Cada caso de uso **aplica o chart** (`helm upgrade --install` com o arquivo `deploy/helm/taba/ci/uc-NN-*.yaml`) num namespace próprio e roda um ou mais testes; cada teste imprime `PASS`, `FAIL` ou `SKIP`.

```bash
cd deploy/helm/tests
./run.sh static          # sem cluster: lint, renderização e casos negativos (helm ou Docker)
./run.sh 01 05           # só estes casos de uso
./run.sh                 # tudo (demora; o UC-12 sozinho leva ~15 min)
```

| Ferramenta | Para quê |
|---|---|
| `helm` 3.x e `kubectl` | tudo (sem `helm` local, o `00-static.sh` usa a imagem `alpine/helm` pelo Docker) |
| `kind` + Docker | os casos de uso 01–15 criam o cluster `taba-helm` sozinhos. Para usar outro cluster: `TABA_TEST_CONTEXT=<contexto>` |
| acesso à internet | baixa imagens, e o KEDA (UC-12) e o CRD do Prometheus Operator (UC-10) |

Variáveis: `TABA_TEST_TAG` (tag das imagens `taba` e `taba-cli`, padrão `rc`), `TABA_TEST_LOAD` (imagens locais para `kind load`), `TABA_TEST_KEEP=1` (não apaga o namespace no fim), `TABA_TEST_TIMEOUT`, `TABA_TEST_NO_NETPOL=1` (CNI sem NetworkPolicy), `TABA_TEST_INGRESS_URL` (UC-04 com tráfego real). **Falta de ferramenta ou de rede é `SKIP` (código 77), nunca `PASS`.** O código de saída é 0 sem falhas.

O `00-static.sh` cobre cada caso também sem cluster: renderiza os `ci/*.yaml` e confere os manifestos (prefixo `UC-NN` nos nomes dos testes), além de recusas do chart (UC-15).

## Mapa dos casos

| Caso | Modo de operação | Values (`ci/`) | Script |
|---|---|---|---|
| [UC-01](#uc-01) | Produção mínima: 3 nós, segredos gerados | `uc-01-minimo.yaml` | `uc-01-minimo.sh` |
| [UC-02](#uc-02) | Um nó: perfil de um nó, sem auth | `uc-02-desenvolvimento.yaml` | `uc-02-desenvolvimento.sh` |
| [UC-03](#uc-03) | Segredos do operador e rotação do token | `uc-03-segredos-existentes.yaml` | `uc-03-segredos-existentes.sh` |
| [UC-04](#uc-04) | Ingress (host, classe, TLS) | `uc-04-ingress.yaml` | `uc-04-ingress.sh` |
| [UC-05](#uc-05) | Backup em S3 embutido (SeaweedFS) | `uc-05-backup-seaweedfs.yaml` | `uc-05-backup-seaweedfs.sh` |
| [UC-06](#uc-06) | Backup em S3 externo | `uc-06-s3-externo.yaml` | `uc-06-s3-externo.sh` |
| [UC-07](#uc-07) | Restauração depois de perder todos os nós | `uc-05-backup-seaweedfs.yaml` | `uc-07-restore.sh` |
| [UC-08](#uc-08) | Coletor Prometheus do chart | `uc-08-coletor-prometheus.yaml` | `uc-08-coletor-prometheus.sh` |
| [UC-09](#uc-09) | Coletor Telegraf do chart | `uc-09-coletor-telegraf.yaml` | `uc-09-coletor-telegraf.sh` |
| [UC-10](#uc-10) | Prometheus Operator (ServiceMonitor) | `uc-10-prometheus-operator.yaml` | `uc-10-prometheus-operator.sh` |
| [UC-11](#uc-11) | Push OTLP | `uc-11-otlp.yaml` | `uc-11-otlp.sh` |
| [UC-12](#uc-12) | Autoscaling com KEDA | `uc-12-autoscaling-keda.yaml` | `uc-12-autoscaling-keda.sh` |
| [UC-13](#uc-13) | Upgrade rolling | `uc-13-upgrade.yaml` | `uc-13-upgrade.sh` |
| [UC-14](#uc-14) | Configuração avançada do nó | `uc-14-config-avancada.yaml` | `uc-14-config-avancada.sh` |
| [UC-15](#uc-15) | Falhas e proteções | `uc-15-falhas.yaml` | `uc-15-falhas.sh` |

---

## UC-01

**Produção mínima.** Três nós; o Job de pré-instalação roda `taba init` e cria o Secret; tudo o mais no padrão.

```bash
helm install taba deploy/helm/taba -n taba --create-namespace
```

**Esperado:** três pods `ready`, `/health/cluster` sem condições, token de 32+ caracteres, API recusando chamada sem token (`401`), publicar e consumir funcionam, o Service da API só publica a 8080, o `helm uninstall` preserva o Secret.

| Teste | O que prova |
|---|---|
| UC-01.1 – 01.3 | Três pods prontos, cluster saudável, `/health/ready` em cada nó |
| UC-01.4 – 01.5 | Secret com `root-token`, `tls.crt`, `tls.key`; token longo |
| UC-01.6 | PDB permite 1 indisponível |
| UC-01.7 | `helm test` (readiness e `/metrics` pelo Service `-ops`) |
| UC-01.8 | API sem token → `401` |
| UC-01.9 – 01.10 | Publicar 10 e consumir 10 |
| UC-01.11 | O Service da API só expõe a 8080 |
| UC-01.12 | Secret sobrevive ao `helm uninstall` |

## UC-02

**Um nó.** `config.replication.factor: 1` (o antigo `config.mode: development` vale também): um nó, sem autenticação, sem TLS de cluster, sem Secret e sem Job. Para laboratório e testes de integração de aplicações.

**Esperado:** um pod; nenhum Secret nem Job; mesmo com `replicaCount: 3` o StatefulSet fica em 1; publicar sem token funciona.

| Teste | O que prova |
|---|---|
| UC-02.1 – 02.3 | Um pod pronto, sem Secret e sem Job de init |
| UC-02.4 | `/health/ready` |
| UC-02.5 | `replicas: 1` apesar de `replicaCount: 3` |
| UC-02.6 – 02.7 | Publicar e consumir sem token |

## UC-03

**Segredos do operador.** `secrets.existingSecret` aponta para um Secret que você controla (certificado de uma CA sua, token de um cofre). O chart não gera nada. O token é trocado por **duas linhas** (novo, anterior) sem reiniciar os nós.

**Esperado:** o token do operador é aceito, um errado não, após a rotação o novo e o anterior valem, nenhum pod reinicia.

| Teste | O que prova |
|---|---|
| UC-03.0 | O teste gera o par com `taba init` e cria o Secret |
| UC-03.1 – 03.3 | Três nós saudáveis; o chart não criou Secret nem Job próprio |
| UC-03.4 – 03.5 | Token certo entra, token errado não |
| UC-03.6 | O novo token passa a valer sem reiniciar (até 4 min: kubelet + `auth.reload-interval`) |
| UC-03.7 | O anterior continua valendo na rotação |
| UC-03.8 | Nenhum reinício |

## UC-04

**Ingress.** `ingress.enabled`, `className`, `annotations`, `hosts`, `tls`. Só a porta `api` é backend.

**Esperado:** o objeto `Ingress` com a classe, o host e o TLS; o backend é a porta `api` do Service, com endpoints; o Service da API não publica 8081 nem 9090. Com `TABA_TEST_INGRESS_URL` (um controlador no cluster), a API responde pelo Ingress e `/metrics` não.

| Teste | O que prova |
|---|---|
| UC-04.1 – 04.4 | Nós prontos; classe, host, TLS e annotation aplicados |
| UC-04.5 – 04.6 | Backend `api`, com endpoints |
| UC-04.7 | 8081 e 9090 fora do Service |
| UC-04.8 | API via Ingress (**SKIP** sem controlador) |
| UC-04.9 | `/metrics` não passa pelo Ingress |

## UC-05

**Backup em S3 embutido (SeaweedFS).** `seaweedfs.enabled: true`. O Job cria o bucket; as credenciais vão por Secret; o backup liga sozinho.

**Esperado:** SeaweedFS pronto, bucket `taba` criado, `GET /node/backup` mostra o backup configurado, depois do `flush` aparecem objetos no bucket.

| Teste | O que prova |
|---|---|
| UC-05.1 – 05.3 | SeaweedFS, três nós e cluster prontos |
| UC-05.4 | Bucket criado pelo Job |
| UC-05.5 | O nó alcança o S3 (`s3Reachable`) |
| UC-05.6 | Credenciais fora do ConfigMap |
| UC-05.7 – 05.9 | Publicar 20, `flush`, objetos no bucket |
| UC-05.10 | O S3 exige credenciais (acesso anônimo recusado) |

## UC-06

**Backup em S3 externo.** `backup.s3.*` com endpoint, prefixo e chaves em Secret. O teste instala um SeaweedFS num **outro release** (`storage`) e aponta para ele; na AWS o equivalente é IRSA (sem chaves), verificado só na renderização (`00-static.sh`).

**Esperado:** nenhum S3 embutido no release do cluster; as chaves vêm do Secret do chart e não estão no ConfigMap; os objetos aparecem no prefixo configurado.

| Teste | O que prova |
|---|---|
| UC-06.1 – 06.2 | Nós prontos e saudáveis |
| UC-06.3 | Sem S3 próprio |
| UC-06.4 – 06.5 | Chaves em Secret, fora do ConfigMap |
| UC-06.6 – 06.8 | Publicar 20, `flush`, objetos em `cluster-a/` |

## UC-07

**Restauração.** Com backup em SeaweedFS: parada planejada (`POST /cluster/stop`), StatefulSet a 0 (os volumes dos nós são apagados por `whenScaled: Delete`), de volta a 3. O primeiro nó a pegar o lock restaura definições, tokens e mensagens ([33-backup-s3.md](33-backup-s3.md)).

**Esperado:** o cluster volta saudável **com as mensagens**.

| Teste | O que prova |
|---|---|
| UC-07.1 – 07.2 | Cluster saudável, 15 mensagens publicadas |
| UC-07.3 – 07.4 | `flush` do cluster e `cluster/stop` (root token) |
| UC-07.5 | Volumes dos nós apagados |
| UC-07.6 – 07.7 | Três nós voltam e o cluster fica saudável |
| UC-07.8 | As 15 mensagens voltaram do bucket |

## UC-08

**Coletor Prometheus do chart.** `collector.type: prometheus`: um Prometheus que descobre os pods do release (só no namespace) e coleta `/metrics` da porta de operações. `remoteWrite` envia para um backend de longo prazo.

| Teste | O que prova |
|---|---|
| UC-08.1 – 08.2 | Nós e Prometheus prontos |
| UC-08.3 | Três alvos `up` |
| UC-08.4 – 08.5 | `taba_cluster_members` chega, com rótulos `namespace` e `pod` |
| UC-08.6 | Pods anotados com `prometheus.io/*` |
| UC-08.7 | RBAC só de pods |

## UC-09

**Coletor Telegraf do chart.** `collector.type: telegraf`: lê `/metrics` dos pods anotados e reexpõe em `:9273` (padrão) ou envia para os `outputs` TOML que você escrever (InfluxDB, Kafka, arquivo…).

| Teste | O que prova |
|---|---|
| UC-09.1 – 09.2 | Nós e Telegraf prontos |
| UC-09.3 – 09.4 | Métricas em `:9273`, de três pods |
| UC-09.5 | Sem erro de permissão |
| UC-09.6 | `outputs` substituíveis (aqui, `file` para stdout) |

## UC-10

**Prometheus Operator.** `collector.type: prometheus-operator`: o chart cria um `ServiceMonitor` para o **seu** Prometheus, só se o CRD existir.

| Teste | O que prova |
|---|---|
| UC-10.1 | Sem o CRD o chart instala e não cria o `ServiceMonitor` |
| UC-10.2 – 10.4 | Com o CRD: criado, com as `labels` do seu seletor, porta `ops` e intervalo |
| UC-10.5 – 10.6 | O seletor casa com o Service `-ops`, que tem 3 endpoints |
| UC-10.7 | Sem annotations `prometheus.io` (o operador as ignora) |

## UC-11

**Push OTLP.** `otel.*` viram `OTEL_*` no pod. O teste sobe um OpenTelemetry Collector e confere as métricas chegando; `/metrics` continua funcionando e o nó não depende do collector.

| Teste | O que prova |
|---|---|
| UC-11.1 – 11.3 | Nós prontos; `OTEL_*` no pod |
| UC-11.4 – 11.5 | O collector recebe métricas com `service.name` configurado |
| UC-11.6 | `/metrics` (pull) intacto |
| UC-11.7 | Collector fora do ar não derruba os nós |

## UC-12

**Autoscaling com KEDA.** Veja [34-chart-helm.md](34-chart-helm.md#autoscaling-com-keda). O teste instala o KEDA (`TABA_TEST_KEDA_VERSION`, padrão 2.17.2) e usa o Prometheus do chart.

**Esperado:** piso de 3; o tráfego com `perNodeRps` baixo leva a 4; ao voltar o alvo, **um nó por vez** de volta a 3, o cluster saudável e **todas as mensagens ainda lá**.

| Teste | O que prova |
|---|---|
| UC-12.1 – 12.5 | Piso de 3; `ScaledObject` pronto; HPA do KEDA com scale-down de 1 pod; cluster saudável |
| UC-12.6 – 12.7 | Scale-up por volume de requisições até 4 nós; cluster saudável com 4 |
| UC-12.8 – 12.9 | Reduz até o piso e **nunca abaixo de 3** |
| UC-12.10 | Cluster saudável depois |
| UC-12.11 | As 20 mensagens publicadas antes sobrevivem (o nó drenou) |
| UC-12.12 | O volume do nó removido foi apagado |
| UC-12.13 | `autoscaling.paused` aplica a anotação de pausa |

O gatilho de **disco** só é provado por renderização (`00-static.sh`: consulta `kubelet_volume_stats_*` ou `taba_storage_disk_free_bytes`), porque encher um volume real é lento e depende da `StorageClass`.

## UC-13

**Upgrade rolling.** `helm upgrade` com uma mudança de `config` (checksum no pod) reinicia os nós **um por vez**.

**Esperado:** nunca menos de 2 nós prontos durante o upgrade, nova configuração em todos, **14 de 14** mensagens depois ([23-operacao.md](23-operacao.md)), Secret do cluster intacto.

| Teste | O que prova |
|---|---|
| UC-13.1 – 13.3 | Cluster saudável com 14 mensagens |
| UC-13.4 – 13.5 | `helm upgrade` concluiu e o StatefulSet trocou de revisão |
| UC-13.6 | Nunca menos de 2 nós prontos |
| UC-13.7 – 13.8 | Três prontos e saudável depois |
| UC-13.9 | Nova configuração em todos os nós |
| UC-13.10 | 14/14 mensagens |
| UC-13.11 | Secret não regerado |

## UC-14

**Configuração avançada.** Muitas chaves de `config.*` ao mesmo tempo, `extraEnv`, limite de taxa, `trusted-proxies`, `allowed-ips`. Como o nó recusa chave desconhecida, subir pronto já prova que as chaves existem.

| Teste | O que prova |
|---|---|
| UC-14.1 – 14.2 | Os nós aceitaram todas as chaves |
| UC-14.3 – 14.5 | ConfigMap, `allowed-ips` e `extraEnv` chegaram |
| UC-14.6 | Logs em JSON no nível `debug` |
| UC-14.7 – 14.8 | Mensagem dentro de `message.max-bytes` passa; acima é recusada |
| UC-14.9 | Operações acessíveis de dentro do CIDR liberado |
| UC-14.10 – 14.11 | `helm upgrade --set` muda a configuração e o cluster volta saudável |

## UC-15

**Falhas e proteções.** `NetworkPolicy`, PDB e recuperação.

| Teste | O que prova |
|---|---|
| UC-15.1 – 15.2 | Três nós saudáveis; PDB permite 1 indisponível |
| UC-15.3 – 15.4 | Um pod de fora **não** alcança 9090 nem 8081 (**SKIP** com `TABA_TEST_NO_NETPOL=1`) |
| UC-15.5 | O pod de fora alcança a API (8080) |
| UC-15.6 | Um nó morre e reentra (3 prontos, sem condições) |
| UC-15.7 | O `taba-0` (first-node) morre e volta como nó comum, sem formar cluster novo |
| UC-15.8 | Dois nós morrem juntos: sem quórum por instantes, depois se recupera |
| UC-15.9 | As 14 mensagens continuam lá |

As **recusas do chart** (`replicaCount` abaixo do mínimo, grace period curto, portas repetidas, `quorum` inválido, `evict-*` fora de ordem, S3 embutido + S3 externo, chave sem par, valores fora do enum, autoscaling inseguro) são testadas em `00-static.sh`, sob o prefixo `UC-15`.

---

## Escolhendo o modo

| Quero… | Use |
|---|---|
| Experimentar uma aplicação | UC-02 |
| Subir um cluster de verdade, mínimo | UC-01 |
| Expor a API fora do cluster | UC-04 (e `trusted-proxies` do [UC-14](#uc-14)) |
| Backup sem nuvem | UC-05 (e restaure como no UC-07) |
| Backup na nuvem | UC-06 |
| Métricas num stack que eu já tenho | UC-10 (Operator) ou `collector.type: none` + anotações |
| Métricas sem stack | UC-08 (Prometheus) ou UC-09 (Telegraf) |
| Métricas para um OpenTelemetry Collector | UC-11 |
| Capacidade que acompanha a carga | UC-12 |

## Fontes e provas

- Scripts e valores de teste: `deploy/helm/tests/` (`lib.sh`, `run.sh`, `00-static.sh`, `uc-*.sh`) e `deploy/helm/taba/ci/`.
- Um caso de uso é "provado" quando o seu script termina com `0 falhas`; os `SKIP` ficam explícitos na saída e no resumo do `run.sh`.
- Execução registrada (2026-10-16, `kind` 0.30 com Kubernetes 1.34, imagens `rafaelpontani/taba:rc` e `taba-cli:rc`, KEDA 2.17.2): `00-static.sh` 103 testes e UC-01 a UC-15 sem falhas; o único `SKIP` é o UC-04.8 (sem controlador de Ingress no kind). Nada foi exercitado ainda num cluster de produção, com outro CNI ou com discos reais cheios (o gatilho de disco do KEDA é provado só por renderização).
