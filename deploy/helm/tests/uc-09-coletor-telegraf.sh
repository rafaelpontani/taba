#!/usr/bin/env bash
# UC-09 — Coletor Telegraf do chart: lê /metrics dos pods (annotations prometheus.io/*) e reexpõe em :9273.
UC=uc-09-coletor-telegraf
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-09-coletor-telegraf.yaml" || { fail "helm install"; summary; exit 1; }

TEL="http://$(fullname)-telegraf:9273/metrics"
scrape() { K -n "$NS" exec "$(pod0)" -- wget -qO- "$TEL"; }

check "UC-09.1 nós prontos"                          wait_nodes 3
check "UC-09.2 Telegraf pronto"                      K -n "$NS" rollout status "deploy/$(fullname)-telegraf" --timeout=180s
has_taba() { scrape | grep -q 'taba'; }
check "UC-09.3 métricas do Taba saem em :9273 (até 3 min)" wait_for 180 has_taba
three_pods() { [ "$(scrape | grep -o 'pod_name="[^"]*"\|pod="[^"]*"' | sort -u | wc -l)" -ge 3 ]; }
check "UC-09.4 uma série por nó (3 pods distintos)"  wait_for 120 three_pods
check "UC-09.5 sem erro de permissão nos logs"       bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS logs deploy/$(fullname)-telegraf | grep -Ei 'forbidden|unauthorized'"

# Substituir os outputs: o Telegraf precisa subir com o TOML novo (aqui, só um 'file' para stdout).
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-09-coletor-telegraf.yaml" --set "image.tag=$TAG" --reuse-values \
  --set-string 'collector.telegraf.outputs=[[outputs.file]]
  files = ["stdout"]' --wait --timeout 5m >/dev/null 2>&1
has_stdout() { K -n "$NS" logs "deploy/$(fullname)-telegraf" | grep -q 'taba'; }
check "UC-09.6 outputs configuráveis (file/stdout)"  wait_for 120 has_stdout

summary
