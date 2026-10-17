#!/usr/bin/env bash
# UC-11 — Push OTLP: variáveis OTEL_* chegam ao nó e as métricas aparecem num OpenTelemetry Collector.
UC=uc-11-otlp
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh

K -n "$NS" apply -f - >/dev/null <<'YAML'
apiVersion: v1
kind: ConfigMap
metadata: { name: otel-collector }
data:
  config.yaml: |
    receivers:
      otlp:
        protocols:
          grpc: { endpoint: 0.0.0.0:4317 }
    exporters:
      debug: { verbosity: detailed }
    service:
      pipelines:
        metrics: { receivers: [otlp], exporters: [debug] }
---
apiVersion: apps/v1
kind: Deployment
metadata: { name: otel-collector }
spec:
  replicas: 1
  selector: { matchLabels: { app: otel-collector } }
  template:
    metadata: { labels: { app: otel-collector } }
    spec:
      containers:
        - name: otel
          image: otel/opentelemetry-collector:0.130.0
          args: ["--config=/etc/otel/config.yaml"]
          volumeMounts: [{ name: cfg, mountPath: /etc/otel }]
      volumes: [{ name: cfg, configMap: { name: otel-collector } }]
---
apiVersion: v1
kind: Service
metadata: { name: otel-collector }
spec:
  selector: { app: otel-collector }
  ports: [{ name: grpc, port: 4317 }]
YAML
K -n "$NS" rollout status deploy/otel-collector --timeout=180s >/dev/null

install "$CI/uc-11-otlp.yaml" || { fail "helm install"; summary; exit 1; }
check "UC-11.1 nós prontos"                          wait_nodes 3
env_of() { K -n "$NS" exec "$(pod0)" -- env; }
check "UC-11.2 OTEL_EXPORTER_OTLP_ENDPOINT no pod"   bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- env | grep -qx 'OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317'"
check "UC-11.3 intervalo e service name no pod"      bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- env | grep -q 'OTEL_METRIC_EXPORT_INTERVAL=10000' && kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- env | grep -q 'OTEL_SERVICE_NAME=taba-teste'"
got_metrics() { K -n "$NS" logs deploy/otel-collector | grep -q 'taba'; }
check "UC-11.4 o collector recebe métricas do Taba (até 2 min)" wait_for 120 got_metrics
has_service() { K -n "$NS" logs deploy/otel-collector | grep -q 'service.name: Str(taba-teste)'; }
check "UC-11.5 resource service.name=taba-teste"     wait_for 60 has_service
check "UC-11.6 /metrics (pull) continua funcionando" bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- wget -qO- http://127.0.0.1:9090/metrics | grep -q '^taba_'"

# Collector fora do ar não derruba o nó (OTLP é best-effort).
K -n "$NS" scale deploy/otel-collector --replicas=0 >/dev/null
check "UC-11.7 nós seguem prontos com o collector fora do ar" bash -c "sleep 30; test \"\$(kubectl ${KCTX:+--context $KCTX} -n $NS get sts $(fullname) -o jsonpath='{.status.readyReplicas}')\" = 3"

summary
