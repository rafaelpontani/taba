{{/* Nome base, nome completo e rótulos. */}}
{{- define "taba.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "taba.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "taba.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "taba.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "taba.selectorLabels" -}}
app.kubernetes.io/name: {{ include "taba.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: node
{{- end -}}

{{/* Rótulos de um componente auxiliar (coletor, seaweedfs). Uso: include "taba.componentLabels" (dict "ctx" . "component" "seaweedfs") */}}
{{- define "taba.componentLabels" -}}
helm.sh/chart: {{ printf "%s-%s" .ctx.Chart.Name .ctx.Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
app.kubernetes.io/name: {{ include "taba.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/component: {{ .component }}
app.kubernetes.io/managed-by: {{ .ctx.Release.Service }}
{{- end -}}

{{- define "taba.componentSelector" -}}
app.kubernetes.io/name: {{ include "taba.name" .ctx }}
app.kubernetes.io/instance: {{ .ctx.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end -}}

{{- define "taba.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "taba.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{- define "taba.image" -}}
{{- printf "%s:%s" .Values.image.repository (default .Chart.AppVersion .Values.image.tag) -}}
{{- end -}}

{{/* Nome do Secret com root-token + tls.crt + tls.key. */}}
{{- define "taba.secretsName" -}}
{{- default (printf "%s-secrets" (include "taba.fullname" .)) .Values.secrets.existingSecret -}}
{{- end -}}

{{- define "taba.isDev" -}}
{{- if eq (toString (dig "mode" "production" .Values.config)) "development" -}}true{{- end -}}
{{- end -}}

{{/* Backup ligado: S3 embutido (SeaweedFS) ou bucket externo. */}}
{{- define "taba.backupEnabled" -}}
{{- if or .Values.seaweedfs.enabled (and .Values.backup.s3 .Values.backup.s3.bucket) -}}true{{- end -}}
{{- end -}}

{{- define "taba.s3SecretName" -}}
{{- if .Values.seaweedfs.enabled -}}
{{- include "taba.seaweedfsSecretName" . -}}
{{- else if .Values.backup.s3.existingSecret -}}
{{- .Values.backup.s3.existingSecret -}}
{{- else -}}
{{- printf "%s-s3" (include "taba.fullname" .) -}}
{{- end -}}
{{- end -}}

{{- define "taba.seaweedfsSecretName" -}}
{{- default (printf "%s-seaweedfs" (include "taba.fullname" .)) .Values.seaweedfs.auth.existingSecret -}}
{{- end -}}

{{/* Converte "25s", "2m", "1h" ou "500ms" em segundos (inteiro, arredondado para cima). */}}
{{- define "taba.seconds" -}}
{{- $v := toString . -}}
{{- if hasSuffix "ms" $v -}}{{- div (add (trimSuffix "ms" $v | atoi) 999) 1000 -}}
{{- else if hasSuffix "s" $v -}}{{- trimSuffix "s" $v | atoi -}}
{{- else if hasSuffix "m" $v -}}{{- mul (trimSuffix "m" $v | atoi) 60 -}}
{{- else if hasSuffix "h" $v -}}{{- mul (trimSuffix "h" $v | atoi) 3600 -}}
{{- else -}}{{- fail (printf "duração inválida %q (use ms, s, m ou h)" $v) -}}
{{- end -}}
{{- end -}}

{{/* Réplicas mínimas seguras (doc 23: nunca menos de 3 nós em produção). */}}
{{- define "taba.minReplicas" -}}
{{- $factor := int (dig "replication" "factor" 3 .Values.config) -}}
{{- max 3 $factor -}}
{{- end -}}

{{/* terminationGracePeriodSeconds mínimo = drain.timeout + backup.drain.timeout (se houver backup). */}}
{{- define "taba.minGrace" -}}
{{- $drain := include "taba.seconds" (dig "drain" "timeout" "25s" .Values.config) | int -}}
{{- $bk := 0 -}}
{{- if include "taba.backupEnabled" . -}}
{{- $bk = include "taba.seconds" (dig "backup" "drain" "timeout" "30s" .Values.config) | int -}}
{{- end -}}
{{- add $drain $bk 5 -}}
{{- end -}}

{{/* Endereço de entrada do cluster: Service -entry (só pods PRONTOS), porta de cluster, TCP puro (sem terminar TLS). */}}
{{- define "taba.entryAddress" -}}
{{- printf "%s-entry.%s.svc.%s:%d" (include "taba.fullname" .) .Release.Namespace .Values.clusterDomain (int .Values.ports.cluster) -}}
{{- end -}}

{{/*
Configuração final do nó (taba.yml). O que o chart controla (portas, diretórios, entry-address, backup S3
embutido) sobrescreve o que vier em `config`; as credenciais NUNCA entram aqui (vão por variável de ambiente).
*/}}
{{- define "taba.config" -}}
{{- $cfg := deepCopy .Values.config -}}
{{- $_ := set $cfg "api" (mergeOverwrite (default (dict) (get $cfg "api")) (dict "port" (int .Values.ports.api))) -}}
{{- $_ = set $cfg "operations" (mergeOverwrite (default (dict) (get $cfg "operations")) (dict "port" (int .Values.ports.operations))) -}}
{{- $_ = set $cfg "storage" (mergeOverwrite (default (dict) (get $cfg "storage")) (dict "dir" "/var/lib/taba")) -}}
{{- $_ = set $cfg "secrets" (mergeOverwrite (default (dict) (get $cfg "secrets")) (dict "dir" "/etc/taba/secrets")) -}}
{{- if not (include "taba.isDev" .) -}}
{{- $_ = set $cfg "cluster" (mergeOverwrite (default (dict) (get $cfg "cluster")) (dict "port" (int .Values.ports.cluster) "entry-address" (include "taba.entryAddress" .))) -}}
{{- end -}}
{{- if .Values.seaweedfs.enabled -}}
{{- $bk := default (dict) (get $cfg "backup") -}}
{{- $s3 := mergeOverwrite (default (dict) (get $bk "s3")) (dict "bucket" .Values.seaweedfs.bucket "endpoint" (printf "http://%s-seaweedfs.%s.svc.%s:8333" (include "taba.fullname" .) .Release.Namespace .Values.clusterDomain)) -}}
{{- $_ = set $bk "s3" $s3 -}}
{{- $_ = set $cfg "backup" $bk -}}
{{- else if .Values.backup.s3.bucket -}}
{{- $bk := default (dict) (get $cfg "backup") -}}
{{- $s3 := default (dict) (get $bk "s3") -}}
{{- $_ = set $s3 "bucket" .Values.backup.s3.bucket -}}
{{- if .Values.backup.s3.endpoint -}}{{- $_ = set $s3 "endpoint" .Values.backup.s3.endpoint -}}{{- end -}}
{{- if .Values.backup.s3.region -}}{{- $_ = set $s3 "region" .Values.backup.s3.region -}}{{- end -}}
{{- if .Values.backup.s3.prefix -}}{{- $_ = set $s3 "prefix" .Values.backup.s3.prefix -}}{{- end -}}
{{- if not (kindIs "invalid" .Values.backup.s3.pathStyle) -}}{{- $_ = set $s3 "path-style" .Values.backup.s3.pathStyle -}}{{- end -}}
{{- $_ = set $bk "s3" $s3 -}}
{{- $_ = set $cfg "backup" $bk -}}
{{- end -}}
{{- toYaml $cfg -}}
{{- end -}}

{{/* Pré-condições que não cabem no JSON Schema. */}}
{{- define "taba.validate" -}}
{{- if and .Values.seaweedfs.enabled .Values.backup.s3.bucket -}}
{{- fail "seaweedfs.enabled e backup.s3.bucket são exclusivos: use o S3 embutido (SeaweedFS) OU um S3 externo" -}}
{{- end -}}
{{- if and (not (include "taba.isDev" .)) (not .Values.autoscaling.enabled) (lt (int .Values.replicaCount) (int (include "taba.minReplicas" .))) (not .Values.allowFewReplicas) -}}
{{- fail (printf "replicaCount=%d é menor que o mínimo seguro (%s = max(3, replication.factor)); use allowFewReplicas=true só em testes" (int .Values.replicaCount) (include "taba.minReplicas" .)) -}}
{{- end -}}
{{- if lt (int .Values.terminationGracePeriodSeconds) (int (include "taba.minGrace" .)) -}}
{{- fail (printf "terminationGracePeriodSeconds=%d é menor que drain.timeout + backup.drain.timeout + 5 = %s: o pod seria morto antes de entregar as mensagens a outro nó" (int .Values.terminationGracePeriodSeconds) (include "taba.minGrace" .)) -}}
{{- end -}}
{{- if and .Values.autoscaling.enabled (lt (int .Values.autoscaling.minReplicas) (int (include "taba.minReplicas" .))) -}}
{{- fail (printf "autoscaling.minReplicas=%d é menor que o mínimo seguro (%s)" (int .Values.autoscaling.minReplicas) (include "taba.minReplicas" .)) -}}
{{- end -}}
{{- if or (eq (int .Values.ports.api) (int .Values.ports.cluster)) (eq (int .Values.ports.api) (int .Values.ports.operations)) (eq (int .Values.ports.cluster) (int .Values.ports.operations)) -}}
{{- fail "ports.api, ports.cluster e ports.operations devem ser diferentes" -}}
{{- end -}}
{{- $factor := int (dig "replication" "factor" 3 .Values.config) -}}
{{- $quorum := int (dig "write" "quorum" (add (div $factor 2) 1) .Values.config) -}}
{{- if or (lt $quorum 1) (gt $quorum $factor) -}}
{{- fail (printf "config.write.quorum=%d deve estar entre 1 e replication.factor=%d" $quorum $factor) -}}
{{- end -}}
{{- $evictAbove := int (dig "backup" "disk" "evict-above" 85 .Values.config) -}}
{{- $evictTo := int (dig "backup" "disk" "evict-to" 70 .Values.config) -}}
{{- $pauseAt := int (dig "backup" "disk" "pause-at" 90 .Values.config) -}}
{{- if not (and (lt $evictTo $evictAbove) (lt $evictAbove $pauseAt) (le $pauseAt 100)) -}}
{{- fail "config.backup.disk: é preciso evict-to < evict-above < pause-at <= 100" -}}
{{- end -}}
{{- if and .Values.backup.s3.bucket (ne (empty .Values.backup.s3.accessKey) (empty .Values.backup.s3.secretKey)) -}}
{{- fail "backup.s3.accessKey e backup.s3.secretKey andam sempre em par" -}}
{{- end -}}
{{- if and .Values.autoscaling.enabled (include "taba.isDev" .) -}}
{{- fail "autoscaling não funciona em mode=development (cluster de um nó)" -}}
{{- end -}}
{{- if and .Values.autoscaling.enabled (not .Values.autoscaling.prometheus.serverAddress) (ne .Values.collector.type "prometheus") -}}
{{- fail "autoscaling.prometheus.serverAddress é obrigatório (ou use collector.type=prometheus)" -}}
{{- end -}}
{{- end -}}
