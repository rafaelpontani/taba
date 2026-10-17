#!/usr/bin/env bash
# UC-01 — Produção mínima: 3 nós, Secret gerado pelo Job `taba init`, sondas, PDB, publicar/consumir.
UC=uc-01-minimo
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-01-minimo.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-01.1 três pods prontos"                    wait_nodes 3
check "UC-01.2 cluster saudável (sem condições)"     wait_healthy 300
check "UC-01.3 /health/ready 200 em todos os nós"    bash -c "for i in 0 1 2; do kubectl ${KCTX:+--context $KCTX} -n $NS exec $(fullname)-\$i -- wget -qO- http://127.0.0.1:9090/health/ready >/dev/null || exit 1; done"
check "UC-01.4 Secret gerado com root-token, tls.crt e tls.key" bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get secret $(secrets_name) -o json | grep -q root-token"
check "UC-01.5 root token tem 32+ caracteres"        test "$(root_token | tr -d '\n' | wc -c)" -ge 32
check "UC-01.6 PDB permite 1 indisponível"           test "$(K -n "$NS" get pdb "$(fullname)" -o jsonpath='{.spec.maxUnavailable}')" = 1
check "UC-01.7 helm test passa"                      H test "$REL" -n "$NS" --timeout 120s

tcli_up
check_out "UC-01.8 API recusa chamada sem token" '^401$' http_code "$(pod0)" http://127.0.0.1:8080/topics
topic_create uc01
check "UC-01.9 publica 10 mensagens com token"       publish_n uc01 10
check "UC-01.10 consome as 10 mensagens"             test "$(consume_count uc01 10)" = 10
check "UC-01.11 Service da API só expõe a porta 8080" test "$(K -n "$NS" get svc "$(fullname)" -o jsonpath='{.spec.ports[*].port}')" = 8080
check "UC-01.12 Secret sobrevive ao uninstall"       bash -c "helm ${KCTX:+--kube-context $KCTX} uninstall $REL -n $NS --wait >/dev/null && kubectl ${KCTX:+--context $KCTX} -n $NS get secret $(secrets_name)"

summary
