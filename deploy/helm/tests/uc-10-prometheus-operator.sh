#!/usr/bin/env bash
# UC-10 — Prometheus Operator: o chart cria um ServiceMonitor quando o CRD existe (e não cria sem ele).
# Basta o CRD: o teste confere o objeto, o seletor e os endpoints, sem precisar subir o operador.
UC=uc-10-prometheus-operator
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
need curl
cluster_up; trap cleanup EXIT; ns_fresh

CRD_URL="https://raw.githubusercontent.com/prometheus-operator/prometheus-operator/v0.84.0/example/prometheus-operator-crd/monitoring.coreos.com_servicemonitors.yaml"
K delete crd servicemonitors.monitoring.coreos.com --ignore-not-found >/dev/null 2>&1

install "$CI/uc-10-prometheus-operator.yaml" || { fail "helm install sem CRD"; summary; exit 1; }
check "UC-10.1 sem o CRD o chart instala e não cria ServiceMonitor" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get servicemonitor 2>/dev/null | grep -q taba"

if ! K apply --server-side -f "$CRD_URL" >/dev/null 2>&1; then
  skip "UC-10.2..5 com CRD" "não consegui baixar o CRD (sem rede?)"; summary; exit $?
fi
K wait --for condition=established crd/servicemonitors.monitoring.coreos.com --timeout=60s >/dev/null
install "$CI/uc-10-prometheus-operator.yaml" || { fail "helm upgrade com CRD"; summary; exit 1; }

check "UC-10.2 ServiceMonitor criado"                K -n "$NS" get servicemonitor "$(fullname)"
check "UC-10.3 label exigida pelo seu Prometheus"    test "$(K -n "$NS" get servicemonitor "$(fullname)" -o jsonpath='{.metadata.labels.release}')" = kube-prometheus-stack
check "UC-10.4 intervalo e porta 'ops'"              bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get servicemonitor $(fullname) -o jsonpath='{.spec.endpoints[0].port}/{.spec.endpoints[0].interval}' | grep -qx 'ops/15s'"
check "UC-10.5 o seletor casa com o Service -ops"    test "$(K -n "$NS" get svc -l app.kubernetes.io/instance=$REL,app.kubernetes.io/service=ops -o name | wc -l)" = 1
check "UC-10.6 nós prontos"                          wait_nodes 3
check "UC-10.6b o Service -ops tem 3 endpoints"      test "$(K -n "$NS" get endpoints "$(fullname)-ops" -o jsonpath='{.subsets[0].addresses[*].ip}' | wc -w)" = 3
check "UC-10.7 sem annotations prometheus.io nos pods" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get pod $(pod0) -o jsonpath='{.metadata.annotations}' | grep -q prometheus.io"

summary
