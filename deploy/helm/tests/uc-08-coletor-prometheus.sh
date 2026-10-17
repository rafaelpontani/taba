#!/usr/bin/env bash
# UC-08 — Coletor Prometheus do chart: sobe um Prometheus que descobre os pods do Taba e coleta /metrics.
UC=uc-08-coletor-prometheus
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-08-coletor-prometheus.yaml" || { fail "helm install"; summary; exit 1; }

PROM="http://$(fullname)-prometheus:9090"
query() { K -n "$NS" exec "$(pod0)" -- wget -qO- "$PROM/api/v1/query?query=$1"; }

check "UC-08.1 nós prontos"                          wait_nodes 3
check "UC-08.2 Prometheus pronto"                    K -n "$NS" rollout status "deploy/$(fullname)-prometheus" --timeout=180s
targets_up() { [ "$(query 'up%7Bjob%3D%22taba%22%7D%3D%3D1' | grep -o '"pod"' | wc -l)" -ge 3 ]; }
check "UC-08.3 três alvos 'up' (até 3 min)"          wait_for 180 targets_up
has_metric() { query 'count%28taba_cluster_members%29' | grep -q '"status":"success"' && query 'count%28taba_cluster_members%29' | grep -q '"value"'; }
check "UC-08.4 métricas do Taba chegam (taba_cluster_members)" wait_for 120 has_metric
has_pod_label() { query 'up%7Bjob%3D%22taba%22%7D' | grep -q "\"pod\":\"$(fullname)-0\""; }
check "UC-08.5 rótulos namespace/pod presentes"      wait_for 60 has_pod_label
check "UC-08.6 pods anotados para scrape"            bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get pod $(pod0) -o jsonpath='{.metadata.annotations}' | grep -q 'prometheus.io/scrape'"
check "UC-08.7 Role do coletor é só de pods"         bash -c "test \"\$(kubectl ${KCTX:+--context $KCTX} -n $NS get role $(fullname)-prometheus -o jsonpath='{.rules[0].resources[0]}')\" = pods"

summary
