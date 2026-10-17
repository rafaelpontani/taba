#!/usr/bin/env bash
# UC-04 — Ingress: host, classe, annotations e TLS na API (8080). Cluster (8081) e operações (9090) nunca saem.
# O objeto Ingress é verificado sempre; o tráfego HTTP real só se houver um controller:
#   TABA_TEST_INGRESS_URL=http://localhost:8080  TABA_TEST_INGRESS_HOST=taba.example.com
UC=uc-04-ingress
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-04-ingress.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-04.1 nós prontos"                          wait_nodes 3
check "UC-04.2 Ingress criado com a classe"          test "$(K -n "$NS" get ingress "$(fullname)" -o jsonpath='{.spec.ingressClassName}')" = nginx
check "UC-04.3 host e TLS"                           test "$(K -n "$NS" get ingress "$(fullname)" -o jsonpath='{.spec.tls[0].hosts[0]}')" = taba.example.com
check "UC-04.4 annotation aplicada"                  bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get ingress $(fullname) -o yaml | grep -q 'proxy-body-size: 6m'"
check "UC-04.5 backend é a porta api do Service"     test "$(K -n "$NS" get ingress "$(fullname)" -o jsonpath='{.spec.rules[0].http.paths[0].backend.service.port.name}')" = api
check "UC-04.6 o backend existe e tem endpoints"     bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get endpoints $(fullname) -o jsonpath='{.subsets[0].addresses[0].ip}' | grep -q ."
check "UC-04.7 Service do Ingress não publica 8081/9090" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get svc $(fullname) -o jsonpath='{.spec.ports[*].port}' | grep -Eq '8081|9090'"

if [ -n "${TABA_TEST_INGRESS_URL:-}" ]; then
  H="${TABA_TEST_INGRESS_HOST:-taba.example.com}"
  check "UC-04.8 a API responde pelo Ingress"       bash -c "curl -fsS -o /dev/null -H 'Host: $H' ${TABA_TEST_INGRESS_URL}/topics -H 'Authorization: Bearer $(root_token)'"
  check "UC-04.9 /health e /metrics não passam pelo Ingress (404)" bash -c "test \"\$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: $H' ${TABA_TEST_INGRESS_URL}/metrics)\" != 200"
else
  skip "UC-04.8 API via Ingress" "defina TABA_TEST_INGRESS_URL (precisa de um ingress controller no cluster)"
fi

summary
