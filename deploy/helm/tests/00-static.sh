#!/usr/bin/env bash
# Testes estáticos do chart: lint, renderização de cada ci/*.yaml, asserções sobre os manifestos e casos
# negativos (valores inválidos que o chart deve recusar). Não precisa de cluster; sem `helm` local usa a
# imagem alpine/helm pelo Docker.
UC=00-static
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

command -v helm >/dev/null 2>&1 || command -v docker >/dev/null 2>&1 || { skip "$UC" "precisa de helm ou docker"; exit 77; }

CH=$(chart_path)
render() { helm_static template t "$CH" "$@"; }
render_ci() { local f=$1; shift; render -f "$(ci_path "$f")" "$@"; }
has()  { grep -Eq -- "$1"; }
hasnt() { ! grep -Eq -- "$1"; }

# --- lint e renderização -------------------------------------------------------------------------
check "lint do chart" helm_static lint "$CH"
for f in "$CI"/*.yaml; do
  b=$(basename "$f")
  extra=()
  [ "$b" = uc-12-autoscaling-keda.yaml ] && extra=(--api-versions keda.sh/v1alpha1)
  check "renderiza ci/$b" render_ci "$b" "${extra[@]}"
done

# --- UC-01: padrão ------------------------------------------------------------------------------
DEF=$(render_ci uc-01-minimo.yaml)
check "UC-01 StatefulSet com 3 réplicas"        has 'replicas: 3' <<<"$DEF"
check "UC-01 grace period cobre o drain"        has 'terminationGracePeriodSeconds: 120' <<<"$DEF"
check "UC-01 sondas na porta de operações"      has 'path: /health/ready' <<<"$DEF"
check "UC-01 Service headless publica não-prontos" has 'publishNotReadyAddresses: true' <<<"$DEF"
check "UC-01 IP do pod informado como advertise-address" has "TABA_CLUSTER_ADVERTISE_ADDRESSES" <<<"$DEF"
check "UC-01 Service -entry só com pods prontos" test "$(awk '/name: t-taba-entry/{f=1} f&&/^---/{exit} f' <<<"$DEF" | grep -c publishNotReady)" = 0
check "UC-01 IP do pod informado como advertise-address" has 'TABA_CLUSTER_ADVERTISE_ADDRESSES' <<<"$DEF"
check "UC-01 entry-address aponta para o Service -entry (só prontos)" has 'entry-address: t-taba-entry\.default\.svc\.cluster\.local:8081' <<<"$DEF"
check "UC-01 PDB maxUnavailable 1"              has 'kind: PodDisruptionBudget' <<<"$DEF"
check "UC-01 Job de segredos pré-instalação"    has 'helm.sh/hook": pre-install,pre-upgrade|helm.sh/hook: pre-install,pre-upgrade' <<<"$DEF"
check "UC-01 sem variáveis de link de Service (TABA_*)" has 'enableServiceLinks: false' <<<"$DEF"
check "UC-01 sem Ingress por padrão"            hasnt 'kind: Ingress' <<<"$DEF"
check "UC-01 sem backup por padrão"             hasnt 'TABA_BACKUP_S3|bucket:' <<<"$DEF"
check "UC-01 primeiro nó só no ordinal 0"       has 'HOSTNAME##\*-.*= "0"' <<<"$DEF"

# --- UC-02: desenvolvimento ---------------------------------------------------------------------
DEV=$(render_ci uc-02-desenvolvimento.yaml)
check "UC-02 1 réplica"                         has 'replicas: 1' <<<"$DEV"
check "UC-02 sem porta de cluster"              hasnt 'name: cluster' <<<"$DEV"
check "UC-02 sem Secret montado nem Job init"   hasnt 'secretName:|-init' <<<"$DEV"

# --- UC-03: segredos existentes -----------------------------------------------------------------
EX=$(render_ci uc-03-segredos-existentes.yaml)
check "UC-03 usa o Secret informado"            has 'secretName: taba-meus-segredos' <<<"$EX"
check "UC-03 não gera segredos"                 hasnt 'kind: Job' <<<"$EX"

# --- UC-04: ingress -----------------------------------------------------------------------------
ING=$(render_ci uc-04-ingress.yaml)
check "UC-04 Ingress renderizado"               has 'kind: Ingress' <<<"$ING"
check "UC-04 className"                         has 'ingressClassName: nginx' <<<"$ING"
check "UC-04 annotations"                       has 'proxy-body-size: 6m' <<<"$ING"
check "UC-04 TLS"                               has 'secretName: taba-tls' <<<"$ING"
ING_ONLY=$(awk '/^kind: Ingress/{f=1} f&&/^---/{exit} f' <<<"$ING")
check "UC-04 só a porta api como backend"       has 'name: api' <<<"$ING_ONLY"
check "UC-04 nunca 8081/9090 no Ingress"        hasnt '8081|9090|name: (cluster|ops)' <<<"$ING_ONLY"
check "UC-04 sem className quando vazio"        hasnt 'ingressClassName' < <(render --set ingress.enabled=true)

# --- UC-05: SeaweedFS embutido -------------------------------------------------------------------
MIN=$(render_ci uc-05-backup-seaweedfs.yaml)
check "UC-05 StatefulSet do SeaweedFS"          has 'name: t-taba-seaweedfs' <<<"$MIN"
check "UC-05 Job cria o bucket"                 has 's3.bucket.create -name taba' <<<"$MIN"
check "UC-05 bucket e endpoint na config"       has 'endpoint: http://t-taba-seaweedfs\.default\.svc\.cluster\.local:8333' <<<"$MIN"
check "UC-05 credenciais por Secret, não na config" has 'TABA_BACKUP_S3_SECRET_KEY' <<<"$MIN"
check "UC-05 tuning do backup vai para a config" has 'dead-after: 15s' <<<"$MIN"
check "UC-05 bucket só é criado se não existir" has 's3.bucket.list' <<<"$MIN"

# --- UC-06: S3 externo --------------------------------------------------------------------------
S3=$(render_ci uc-06-s3-externo.yaml)
check "UC-06 bucket/endpoint/prefix na config"  has 'prefix: cluster-a' <<<"$S3"
check "UC-06 chaves viram Secret"               has 'secretKey: "taba-s3-secret"' <<<"$S3"
CM=$(awk '/^kind: ConfigMap/{f=1} f&&/^---/{exit} f' <<<"$S3")
check "UC-06 chaves NÃO aparecem no ConfigMap"  hasnt 'taba-s3-secret|access-key|secret-key' <<<"$CM"
check "UC-06 não sobe SeaweedFS"               hasnt 'name: t-taba-seaweedfs' <<<"$S3"
IRSA=$(render --set backup.s3.bucket=b --set-string 'serviceAccount.annotations.eks\.amazonaws\.com/role-arn=arn:aws:iam::1:role/taba')
check "UC-06 IRSA: annotation no ServiceAccount" has 'role-arn: arn:aws:iam::1:role/taba' <<<"$IRSA"
check "UC-06 IRSA: sem chaves nem Secret"       hasnt 'TABA_BACKUP_S3_ACCESS_KEY|name: t-taba-s3' <<<"$IRSA"

# --- UC-08/09/10: coletores ---------------------------------------------------------------------
PRO=$(render_ci uc-08-coletor-prometheus.yaml)
check "UC-08 Deployment do Prometheus"          has 'image: prom/prometheus' <<<"$PRO"
check "UC-08 descoberta de pods por porta ops"  has 'regex: ops' <<<"$PRO"
check "UC-08 Role só de pods"                   has 'resources: \[pods\]' <<<"$PRO"
check "UC-08 pods anotados para scrape"         has 'prometheus.io/scrape: "true"' <<<"$PRO"
check "UC-08 remote_write opcional"             has 'url: https://m/api/v1/push' < <(render_ci uc-08-coletor-prometheus.yaml --set 'collector.prometheus.remoteWrite[0].url=https://m/api/v1/push')
TEL=$(render_ci uc-09-coletor-telegraf.yaml)
check "UC-09 Deployment do Telegraf"            has 'image: telegraf:' <<<"$TEL"
check "UC-09 input prometheus por pods"         has 'monitor_kubernetes_pods = true' <<<"$TEL"
check "UC-09 output padrão prometheus_client"   has 'outputs.prometheus_client' <<<"$TEL"
check "UC-09 outputs substituíveis"             has 'outputs.influxdb_v2' < <(render_ci uc-09-coletor-telegraf.yaml --set-string 'collector.telegraf.outputs=[[outputs.influxdb_v2]]')
OPR=$(render_ci uc-10-prometheus-operator.yaml --api-versions monitoring.coreos.com/v1)
check "UC-10 ServiceMonitor com CRD presente"   has 'kind: ServiceMonitor' <<<"$OPR"
check "UC-10 labels do selector do Prometheus"  has 'release: kube-prometheus-stack' <<<"$OPR"
check "UC-10 sem CRD não renderiza"             hasnt 'kind: ServiceMonitor' < <(render_ci uc-10-prometheus-operator.yaml)
check "UC-10 sem annotations prometheus.io"     hasnt 'prometheus.io/scrape' <<<"$OPR"

# --- UC-11: OTLP --------------------------------------------------------------------------------
OT=$(render_ci uc-11-otlp.yaml)
check "UC-11 OTEL_EXPORTER_OTLP_ENDPOINT"       has 'OTEL_EXPORTER_OTLP_ENDPOINT, value: "http://otel-collector:4317"' <<<"$OT"
check "UC-11 intervalo e nome do serviço"       has 'OTEL_METRIC_EXPORT_INTERVAL' <<<"$OT"
check "UC-11 headers via Secret quando informado" has 'secretKeyRef' < <(render --set otel.endpoint=http://x:4317 --set otel.headersSecret.name=hdrs)
check "UC-11 sem OTEL por padrão"               hasnt 'OTEL_' <<<"$DEF"

# --- UC-12: autoscaling KEDA --------------------------------------------------------------------
KED=$(render_ci uc-12-autoscaling-keda.yaml)
check "UC-12 ScaledObject apontando o StatefulSet" has 'kind: StatefulSet' <<<"$(awk '/^kind: ScaledObject/{f=1} f' <<<"$KED")"
check "UC-12 StatefulSet sem spec.replicas"     hasnt 'replicas: 3' <<<"$(awk '/^kind: StatefulSet/{f=1} f&&/podManagementPolicy/{exit} f' <<<"$KED")"
check "UC-12 piso 3 e teto"                     has 'minReplicaCount: 3' <<<"$KED"
check "UC-12 scale-down de 1 pod por 60s/janela 60" has 'stabilizationWindowSeconds: 60' <<<"$KED"
check "UC-12 gatilho de disco"                  has 'name: disk' <<<"$KED"
check "UC-12 gatilho de requisições"            has 'name: requests' <<<"$KED"
check "UC-12 guarda de quórum nas consultas"    has 'taba_cluster_quorum_available' <<<"$KED"
check "UC-12 ocioso é 0, não erro (or vector(0))" has 'or vector.0.' <<<"$KED"
check "UC-12 disco nunca negativo (clamp_min)" has 'clamp_min' <<<"$KED"
check "UC-12 consulta nula vira erro (não reduz)" has 'ignoreNullValues: "false"' <<<"$KED"
check "UC-12 usa o Prometheus do chart"         has 'http://t-taba-prometheus\.default\.svc\.cluster\.local:9090' <<<"$KED"
check "UC-12 pausa congela réplicas"            has 'paused-replicas: "4"' < <(render_ci uc-12-autoscaling-keda.yaml --api-versions keda.sh/v1alpha1 --set autoscaling.paused=true --set autoscaling.pausedReplicas=4)
check "UC-12 disk.source=kubelet usa kubelet_volume_stats" has 'kubelet_volume_stats_capacity_bytes' < <(render_ci uc-12-autoscaling-keda.yaml --set autoscaling.disk.source=kubelet)

# --- UC-14: configuração avançada ---------------------------------------------------------------
ADV=$(render_ci uc-14-config-avancada.yaml)
check "UC-14 chaves aninhadas na config"        has 'max-bytes: 1MB' <<<"$ADV"
check "UC-14 trusted-proxies"                   has '10\.0\.0\.0/8' <<<"$ADV"
check "UC-14 extraEnv"                          has 'TABA_API_PAGINATION_MAX_LIMIT' <<<"$ADV"
check "UC-14 mudança de config muda o checksum" test "$(grep -m1 checksum/config <<<"$DEF")" != "$(grep -m1 checksum/config <<<"$ADV")"

# --- UC-15: rede e validações negativas ---------------------------------------------------------
check "UC-15 NetworkPolicy"                     has 'kind: NetworkPolicy' < <(render_ci uc-15-falhas.yaml)
check_fails "UC-15 recusa replicaCount=2"       render --set replicaCount=2
check "UC-15 libera replicaCount=1 com allowFewReplicas" render --set replicaCount=1 --set allowFewReplicas=true
check_fails "UC-15 recusa grace period curto"   render --set terminationGracePeriodSeconds=20
check_fails "UC-15 com backup exige grace maior (25+30+5)" render -f "$(ci_path uc-05-backup-seaweedfs.yaml)" --set terminationGracePeriodSeconds=50
check_fails "UC-15 recusa portas repetidas"     render --set ports.cluster=8080
check_fails "UC-15 recusa quorum > factor"      render --set config.write.quorum=5
check_fails "UC-15 recusa evict-to >= evict-above" render --set config.backup.disk.evict-to=90
check_fails "UC-15 recusa SeaweedFS + S3 externo" render --set seaweedfs.enabled=true --set backup.s3.bucket=b
check_fails "UC-15 recusa accessKey sem secretKey" render --set backup.s3.bucket=b --set backup.s3.accessKey=a
check_fails "UC-15 recusa collector.type inválido" render --set collector.type=grafana
check_fails "UC-15 recusa mode inválido"        render --set config.mode=staging
check_fails "UC-15 recusa autoscaling abaixo de 3" render --set autoscaling.enabled=true --set autoscaling.minReplicas=2 --set autoscaling.prometheus.serverAddress=http://p:9090
check_fails "UC-15 recusa autoscaling sem Prometheus" render --set autoscaling.enabled=true
check_fails "UC-15 recusa autoscaling em development" render --set autoscaling.enabled=true --set config.mode=development --set autoscaling.prometheus.serverAddress=http://p:9090
check_fails "UC-15 recusa disk.source=app sem capacidade" render --set autoscaling.enabled=true --set autoscaling.disk.source=app --set autoscaling.prometheus.serverAddress=http://p:9090

summary
