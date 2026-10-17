---
título: Chart Helm — configuração
resumo: Como instalar o Taba no Kubernetes com o chart Helm — o que ele cria, todos os values (nó, segredos, Ingress, backup em S3 embutido (SeaweedFS) ou externo, coletor Prometheus ou Telegraf, OTLP, autoscaling com KEDA), como cada um vira configuração do nó e o que o chart recusa.
tipo: referência
status: estável
verificado-em: 2026-10-16 @ 4faa639
fontes: [deploy/helm/taba/Chart.yaml, deploy/helm/taba/values.yaml, deploy/helm/taba/values.schema.json, deploy/helm/taba/templates, deploy/helm/tests, crates/taba-node/src/config/params/mod.rs, docker-compose.yml]
---

# Chart Helm — configuração

> Leia antes: [22-configuracao.md](22-configuracao.md) (as chaves do nó), [23-operacao.md](23-operacao.md) (escalar, atualizar, drenar) e [33-backup-s3.md](33-backup-s3.md). Os modos de operação do chart, cada um com seus testes, estão em [35-chart-helm-casos-de-uso.md](35-chart-helm-casos-de-uso.md).

O chart fica em `deploy/helm/taba/`. Ele traduz o que o `docker-compose.yml` faz (`init`, nós, SeaweedFS, balanceador) para Kubernetes e acrescenta o que só faz sentido lá: Ingress, coletor de métricas e autoscaling.

```bash
helm install taba deploy/helm/taba -n taba --create-namespace
kubectl -n taba get pods -w                      # os nós ficam "ready" depois de entrar no cluster
kubectl -n taba get secret taba-secrets -o jsonpath='{.data.root-token}' | base64 -d
```

Pré-requisitos: Kubernetes 1.27+, uma `StorageClass` padrão (um volume por nó) e acesso à imagem `rafaelpontani/taba`. O KEDA, o Prometheus Operator e um controlador de Ingress são **opcionais** e só entram quando o recurso correspondente é ligado.

## O que o chart cria

| Recurso | Para quê |
|---|---|
| `StatefulSet` `<release>` | Os nós. Um volume por nó em `/var/lib/taba`; `enableServiceLinks: false` (os Services injetariam `TABA_PORT`, `TABA_SERVICE_HOST`… e o nó lê o prefixo `TABA_*` como configuração) |
| `Service` `<release>` | A **API do cliente** (8080). É o único que o Ingress ou um balanceador deve alcançar |
| `Service` `<release>-entry` | **Entrada do cluster** (`cluster.entry-address`), porta 8081, TCP puro. Só tem pods **prontos**: sem nó pronto ninguém responde, e é assim que o primeiro nó sabe que deve formar o cluster |
| `Service` `<release>-headless` | Identidade dos pods (`taba-0.taba-headless`), inclui pods ainda em bootstrap |
| `Service` `<release>-ops` | Porta de operações (9090) para o coletor. **Sem autenticação**: nunca sai do cluster |
| `ConfigMap` `<release>` | O `taba.yml` do nó (a árvore `config` dos values) |
| `Secret` `<release>-secrets` | `root-token`, `tls.crt`, `tls.key`. Gerado **uma vez** pelo Job de pré-instalação (`taba init`); não é apagado no `helm uninstall` nem recriado no upgrade |
| `PodDisruptionBudget` | No máximo 1 nó indisponível |
| `Ingress` | Opcional (`ingress.enabled`) |
| SeaweedFS (`StatefulSet`, `Service`, `Secret`, Job do bucket) | Opcional (`seaweedfs.enabled`) |
| Prometheus ou Telegraf (`Deployment`, `ConfigMap`, RBAC) ou `ServiceMonitor` | Opcional (`collector.type`) |
| `ScaledObject` (KEDA) | Opcional (`autoscaling.enabled`) |
| `NetworkPolicy` | Opcional (`networkPolicy.enabled`) |

Decisões que o chart toma por você (vêm de [23-operacao.md](23-operacao.md)):

- **Primeiro nó.** O pod de ordinal 0 sobe com `cluster.first-node=true`. É seguro deixar ligado para sempre: se os outros estão de pé, ele entra neles em vez de formar um cluster novo.
- **IP do pod.** `cluster.advertise-addresses` recebe o IP do pod (downward API). Sem isso o nó descobriria o próprio IP resolvendo o `entry-address`, que só responde quando já existe um nó pronto.
- **Sondas** na porta de operações: `startup` e `liveness` em `/health/live`, `readiness` em `/health/ready`.
- **Atualização** de um nó por vez (`maxUnavailable: 1`) e `podManagementPolicy: Parallel` (os nós novos tentam entrar sozinhos até o primeiro ficar pronto).
- **Parada.** `terminationGracePeriodSeconds` precisa cobrir `drain.timeout` + `backup.drain.timeout` + 5 s; o chart **recusa** valores menores (o pod seria morto antes de entregar as mensagens a outro nó).

## Values

### Nós

| Value | Padrão | Efeito |
|---|---|---|
| `image.repository` / `image.tag` | `rafaelpontani/taba` / `appVersion` do chart (`0.1`) | Todos os nós devem rodar a mesma versão. `0.1` é o ponteiro seguro ([27-ci-cd.md](27-ci-cd.md)) |
| `replicaCount` | `3` | Produção: nunca menos de `max(3, replication.factor)`. O chart recusa menos, salvo `allowFewReplicas=true` (só testes). Ignorado com `autoscaling.enabled` |
| `terminationGracePeriodSeconds` | `120` | Veja acima; mínimo `drain.timeout + backup.drain.timeout + 5` |
| `ports.api` / `ports.cluster` / `ports.operations` | `8080` / `8081` / `9090` | Devem ser diferentes (o nó não sobe com portas repetidas). Viram `api.port`, `cluster.port`, `operations.port` |
| `persistence.size` / `storageClass` / `accessModes` | `10Gi` / padrão do cluster / `ReadWriteOnce` | Volume de `storage.dir` (`/var/lib/taba`, cujo último diretório já se chama `taba`, como o nó exige) |
| `persistence.retentionPolicy.whenScaled` | `Delete` | Ao reduzir réplicas o nó sai drenado e volta como **nó novo**: o volume antigo não serve |
| `resources`, `nodeSelector`, `tolerations`, `affinity`, `topologySpreadConstraints`, `priorityClassName` | vazio | Padrão do Kubernetes. Sem `affinity`, o chart pede (preferência) nós em hosts diferentes |
| `podSecurityContext` / `containerSecurityContext` | uid/gid/fsGroup `10001`, sem privilégios | O usuário da imagem é o uid 10001 |
| `probes.*` | startup até 10 min | Ajuste se a sincronização inicial for mais longa |
| `extraEnv`, `extraEnvFrom`, `extraVolumes`, `extraVolumeMounts` | vazio | Saída de emergência: qualquer `TABA_*`, ou `AWS_*` para a cadeia de credenciais da AWS |
| `serviceAccount.annotations` | vazio | Ex.: `eks.amazonaws.com/role-arn` para backup em S3 sem chaves (IRSA) |
| `podDisruptionBudget.enabled` / `maxUnavailable` | `true` / `1` | |
| `networkPolicy.enabled` / `apiFrom` / `opsFrom` | `false` | Liga: API aberta (ou só `apiFrom`); 8081 e 9090 só para os pods do release e `opsFrom` |

### Configuração do nó (`config`)

A árvore `config` é o `taba.yml` do nó: **cada ponto da chave vira um nível** ([22-configuracao.md](22-configuracao.md)). Tudo que não for definido fica no padrão do nó. O `values.yaml` traz todas as chaves relevantes comentadas, com o padrão ao lado. Exemplo:

```yaml
config:
  replication: { factor: 3 }
  message: { max-bytes: 1MB }
  retention: { ttl: { default: 7d } }
  ratelimit: { publish: { rps: 500 }, burst: 1000 }
  client-ip: { trusted-proxies: [10.0.0.0/8] }   # o CIDR do seu controlador de Ingress
  operations: { allowed-ips: [10.0.0.0/8] }
  telemetry: { topics: { mode: by-tags } }
```

O nó **recusa chave desconhecida** (erro de digitação impede o start), então um `config` errado aparece no primeiro `helm install --wait`. O chart controla e **sobrescreve**: `api.port`, `operations.port`, `cluster.port`, `cluster.entry-address`, `storage.dir`, `secrets.dir` e, com backup, `backup.s3.bucket`/`endpoint`/`region`/`prefix`/`path-style`. Os parâmetros aceitos e ignorados pelo nó ([22-configuracao.md](22-configuracao.md)) não estão no `values.yaml`.

`config.mode: development` sobe **um nó**, sem autenticação, sem TLS de cluster, sem Secret e sem Job (para laboratório).

Mudar `config` muda um checksum no pod e os nós são reiniciados **um por vez** no `helm upgrade`.

### Validações do chart

O `helm install`/`upgrade`/`template` falha com mensagem em português quando:

- `replicaCount` é menor que `max(3, replication.factor)` (sem `allowFewReplicas`);
- `terminationGracePeriodSeconds` é menor que `drain.timeout` + `backup.drain.timeout` + 5 s;
- as três portas se repetem;
- `config.write.quorum` não está entre 1 e `replication.factor`;
- `config.backup.disk` não cumpre `evict-to < evict-above < pause-at ≤ 100`;
- `seaweedfs.enabled` e `backup.s3.bucket` estão ligados juntos;
- `backup.s3.accessKey` e `secretKey` não vêm em par;
- `collector.type`, `config.mode`, `config.telemetry.logs.format` ou `config.telemetry.topics.mode` têm valor inválido (`values.schema.json`);
- o autoscaling tem `minReplicas` abaixo do mínimo seguro, está em `mode: development`, não tem Prometheus ou usa `disk.source=app` sem `capacityBytes`.

### Segredos do cluster

| Value | Padrão | Efeito |
|---|---|---|
| `secrets.existingSecret` | vazio | Secret seu, com as chaves `root-token`, `tls.crt` e `tls.key`. Vazio = o Job de pré-instalação roda `taba init` **uma vez** |
| `secrets.init.certDays` | `1825` | Validade do certificado gerado |
| `secrets.init.kubectlImage` | `alpine/k8s` | Só o Job de geração usa (`kubectl create secret`) |

O Job só **cria** o Secret se ele não existir, e o Secret não tem dono Helm: o `helm uninstall` o preserva (apague à mão para recomeçar). O `root-token` aceita **duas linhas** (novo e anterior) para rotação sem reiniciar ([14-seguranca.md](14-seguranca.md)); o kubelet atualiza o arquivo montado em até ~1 minuto e o nó o relê a cada `auth.reload-interval`.

### Service e Ingress

```yaml
ingress:
  enabled: true
  className: nginx            # vazio = classe padrão; alguns controladores querem a annotation legada
  annotations:
    nginx.ingress.kubernetes.io/proxy-body-size: 6m   # message.max-bytes (5MB) + envelope
  hosts:
    - host: taba.example.com
      paths: [{ path: /, pathType: Prefix }]
  tls:
    - secretName: taba-tls
      hosts: [taba.example.com]
```

- O Ingress aponta **só** para a porta `api` do Service. As portas 8081 (mTLS entre nós) e 9090 (operações, sem autenticação) **não existem** no Service da API e não devem ser expostas por nenhum caminho.
- Termine o TLS no Ingress: a API do nó é HTTP ([14-seguranca.md](14-seguranca.md)).
- Ajuste o limite de corpo do controlador acima de `message.max-bytes` (5 MB), ou ele devolverá `413` antes do nó.
- Preencha `config.client-ip.trusted-proxies` com o CIDR do controlador para que o limite de taxa por IP veja o IP real do cliente (`X-Forwarded-For`).
- O projeto `ingress-nginx` foi aposentado em março de 2026; o chart usa só `networking.k8s.io/v1`, que qualquer controlador mantido atende. O chart **não** cria `HTTPRoute` (Gateway API).
- `service.type: LoadBalancer` é a alternativa sem Ingress.

### Backup em S3 (doc 33)

Escolha **um** destino. Sem nenhum, não há backup (`422 backup.not-configured` nas rotas).

**S3 embutido (SeaweedFS)** (laboratório e ambientes sem nuvem):

```yaml
seaweedfs:
  enabled: true
  bucket: taba
  persistence: { size: 50Gi }
  auth: { existingSecret: "" }   # vazio = chave secreta aleatória, mantida nos upgrades
```

Cria `StatefulSet` + `Service` + `Secret` e um Job (`post-install`/`post-upgrade`) que cria o bucket (só se ele ainda não existir), como o `seaweedfs-init` do Compose. As credenciais vão para os nós por `TABA_BACKUP_S3_ACCESS_KEY`/`SECRET_KEY` (Secret), **nunca** no ConfigMap. O SeaweedFS sobe master, volume, filer e gateway S3 em **um** processo, em um nó: bom para testar o backup, não é um armazenamento de produção redundante. A escolha do SeaweedFS (e não do Garage) é deliberada: o backup precisa de **escrita condicional atômica** ([33-backup-s3.md](33-backup-s3.md)).

**S3 externo** (AWS, Ceph, SeaweedFS ou MinIO próprios…; **não** o Garage):

```yaml
backup:
  s3:
    bucket: meu-bucket
    endpoint: ""              # vazio = AWS; fora da AWS use a URL (liga path-style sozinho)
    region: us-east-1
    prefix: producao          # vários clusters podem dividir um bucket com prefixos diferentes
    existingSecret: meu-s3    # chaves accessKey e secretKey; ou accessKey/secretKey inline
```

Na AWS prefira **IRSA** (sem chaves): deixe `accessKey`/`secretKey`/`existingSecret` vazios e anote o `serviceAccount` com o papel. O papel precisa de `s3:PutObject`, `GetObject`, `DeleteObject` e `ListBucket`. O S3 precisa aceitar escrita condicional (`If-None-Match`/`If-Match`), que AWS, SeaweedFS e MinIO aceitam (o Garage **ignora** os cabeçalhos e não serve). Não crie *lifecycle* que apague o que o Taba ainda usa.

Os ajustes finos (`delay`, `segment.*`, `heartbeat.interval`, `dead-after`, `disk.*`…) ficam em `config.backup`. Com backup ligado o chart usa `terminationGracePeriodSeconds` maior que `drain.timeout` + `backup.drain.timeout`.

### Métricas: coletor (`collector.type`)

O nó expõe `GET /metrics` na porta de operações (um alvo por nó, sem agregação) e, opcionalmente, faz push OTLP. O chart oferece quatro modos:

| `collector.type` | O que faz |
|---|---|
| `none` (padrão) | Só expõe: os pods levam `prometheus.io/scrape`, `/port` e `/path` para quem já tem um coletor |
| `prometheus-operator` | Cria um `ServiceMonitor` para o **seu** Prometheus (só se o CRD `monitoring.coreos.com/v1` existir; `collector.serviceMonitor.requireCrd=false` força). Ajuste `labels` para casar com o `serviceMonitorSelector` do seu Prometheus |
| `prometheus` | Sobe um Prometheus (`Deployment`, descoberta de pods **só no namespace do release**, RBAC só de leitura de pods). `collector.prometheus.remoteWrite`, `extraScrapeConfigs`, `retention` e `persistence` configuram o resto |
| `telegraf` | Sobe um Telegraf com `inputs.prometheus` sobre os pods anotados. Os `outputs` são **TOML livre** em `collector.telegraf.outputs` (padrão: `outputs.prometheus_client` em `:9273`); segredos dos outputs por `collector.telegraf.envFromSecret` |

Não use `sum` nas métricas `taba.cluster.*` (cada nó reporta a visão do cluster; use `max`/`min`). Veja [13-observabilidade.md](13-observabilidade.md).

**Push OTLP** é independente do coletor: `otel.endpoint` liga (`OTEL_EXPORTER_OTLP_ENDPOINT`), com `protocol`, `headers` (ou `headersSecret`, preferível, porque podem ser segredo), `certificateConfigMap` (CA em PEM), `metricExportInterval` (ms), `tracesSampler`, `tracesSamplerArg`, `serviceName` e `resourceAttributes`. Collector fora do ar não derruba o nó.

### Autoscaling com KEDA

Desligado por padrão. Precisa do **KEDA** instalado e de um **Prometheus** que enxergue os nós (`autoscaling.prometheus.serverAddress`, ou `collector.type=prometheus`, que já aponta para o Prometheus do chart).

```yaml
autoscaling:
  enabled: true
  minReplicas: 3        # nunca abaixo de max(3, replication.factor)
  maxReplicas: 6
  prometheus: { serverAddress: http://prometheus.monitoring:9090 }
  disk:     { enabled: true, threshold: 70 }         # % de uso do volume, o pior nó
  requests: { enabled: true, perNodeRps: 500 }       # requisições/s por nó
```

| Gatilho | Consulta (padrão) | Escala quando |
|---|---|---|
| `disk` | `max(clamp_min(100 × (1 − kubelet_volume_stats_available_bytes / kubelet_volume_stats_capacity_bytes), 0))` nos PVCs `data-<release>-N` (ou `taba_storage_disk_free_bytes` com `disk.source=app` + `capacityBytes`) | o pior volume passa de `disk.threshold` % (padrão 70, abaixo do `backup.disk.evict-above` de 85) |
| `requests` | `sum(rate(http_server_request_duration_seconds_count{server_port="8080"}[2m])) or vector(0)` | o total dividido por nós passa de `requests.perNodeRps` |

Qualquer gatilho pede mais nós; **só reduz** quando todos estão abaixo do limite. As consultas podem ser trocadas inteiras (`disk.query`, `requests.query`).

**Por que é seguro** (cada item é verificado em [35-chart-helm-casos-de-uso.md](35-chart-helm-casos-de-uso.md#uc-12)):

- **Piso.** `minReplicas` nunca abaixo de `max(3, replication.factor)` (o chart recusa).
- **Um nó por vez.** `behavior.scaleDown`: 1 pod a cada 600 s, com janela de estabilização de 900 s; `scaleUp`: 1 pod a cada 300 s (as entradas no cluster são serializadas, [23-operacao.md](23-operacao.md)).
- **Sem perder mensagem.** O StatefulSet remove sempre o **maior ordinal** (nunca o `taba-0`, que é o primeiro nó). O pod recebe SIGTERM, o nó faz o **drain** (reenvia o que tem a outros nós, e envia ao S3 se há backup) e só então sai; o `terminationGracePeriodSeconds` garante esse tempo. O `drain.on-stop` fica ligado (padrão).
- **Nunca reduz um cluster doente.** As consultas levam `and on() (min(taba_cluster_quorum_available) == 1)` (`autoscaling.healthGuard`): sem quórum, ou sem a métrica, a consulta não devolve valor, `ignoreNullValues: "false"` faz o KEDA tratar como erro e o HPA **não escala nem reduz**.
- **Volume.** `persistence.retentionPolicy.whenScaled: Delete` apaga o volume do nó removido (ele voltaria como nó novo de qualquer jeito).
- **Pausa.** `autoscaling.paused: true` + `pausedReplicas: N` congela o número de réplicas (anotação `autoscaling.keda.sh/paused-replicas`), útil durante um upgrade ou uma investigação. `autoscaling.fallback` define réplicas fixas se o Prometheus ficar fora.
- Com o autoscaling ligado o chart **omite** `spec.replicas` do StatefulSet, para o `helm upgrade` não brigar com o HPA.

## Instalar, atualizar, desinstalar

```bash
helm upgrade --install taba deploy/helm/taba -n taba --create-namespace -f meus-values.yaml --wait --timeout 10m
helm test taba -n taba                        # /health/ready de um nó e /metrics pelo Service -ops
helm uninstall taba -n taba                   # mantém o Secret taba-secrets e os volumes dos nós
```

- `helm upgrade` troca os nós **um por vez** esperando o `ready` ([23-operacao.md](23-operacao.md)); o Secret do cluster **não** é regerado.
- Para trocar de versão do Taba basta `--set image.tag=...`; todos os nós precisam da mesma versão.
- Para apagar tudo, inclusive segredos e dados: `helm uninstall` e depois `kubectl delete secret taba-secrets` e `kubectl delete pvc -l app.kubernetes.io/instance=taba`.

## Armadilhas conhecidas

- **`TABA_*` no ambiente do pod.** Por isso `enableServiceLinks: false`; um Service chamado `taba` injetaria `TABA_PORT` etc.
- **Dois nós nunca param ao mesmo tempo de propósito.** Um `kubectl drain` de um nó do Kubernetes respeita o PDB (1 por vez); aguarde o `ready` entre um e outro.
- **Perda total.** Se todos os nós e volumes se perdem, o restauro vem do S3 sozinho no start ([33-backup-s3.md](33-backup-s3.md)); sem backup, não há o que restaurar.
- **O Secret `taba-secrets` não faz parte do release.** Quem o apaga com o cluster no ar derruba a recarga de certificado e de token; restaure-o antes de reiniciar os nós.
- **Garage não serve como S3 do backup.** Ele ignora `If-None-Match`/`If-Match` (nas versões 2.1.0 e 2.4.1 a criação "só se não existir" sobrescreve), e sem isso dois nós podem restaurar ao mesmo tempo.
- **MinIO.** O projeto parou de publicar imagens no Docker Hub e no Quay; por isso o S3 embutido do chart é o SeaweedFS. Se você usa MinIO como S3 externo, a imagem é por sua conta (espelhada no seu registry).
- **Volumes do SeaweedFS.** Cada volume do SeaweedFS pré-aloca espaço (`seaweedfs.volumeSizeLimitMB`, padrão 1024 MB, até `maxVolumes` volumes): ajuste ao tamanho do disco.

## Fontes e provas

- `deploy/helm/taba/` (chart), `values.schema.json` e as validações de `templates/_helpers.tpl`.
- Testes em `deploy/helm/tests/`: `00-static.sh` (lint, renderização de cada `ci/*.yaml`, asserções e casos negativos, sem cluster) e um script por caso de uso (`uc-NN-*.sh`, num cluster [kind](https://kind.sigs.k8s.io)). A tabela caso → teste está em [35-chart-helm-casos-de-uso.md](35-chart-helm-casos-de-uso.md).
- Chaves do nó: `crates/taba-node/src/config/params/mod.rs` e [22-configuracao.md](22-configuracao.md). Comportamento de entrada no cluster, drain e backup: [23-operacao.md](23-operacao.md), [33-backup-s3.md](33-backup-s3.md).
