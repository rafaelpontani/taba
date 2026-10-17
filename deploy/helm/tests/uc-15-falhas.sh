#!/usr/bin/env bash
# UC-15 — Falhas e proteções: NetworkPolicy fecha cluster/operações, PDB, e o cluster se recupera quando
# pods morrem (um, o primeiro nó, e dois ao mesmo tempo) sem perder mensagens.
# NetworkPolicy só vale se o CNI do cluster aplicar políticas (TABA_TEST_NO_NETPOL=1 pula esses testes).
UC=uc-15-falhas
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-15-falhas.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-15.1 três nós prontos e saudáveis"         wait_nodes 3
wait_healthy 300 >/dev/null 2>&1
check "UC-15.2 PDB: 1 indisponível permitido"        test "$(K -n "$NS" get pdb "$(fullname)" -o jsonpath='{.status.disruptionsAllowed}')" = 1
tcli_up
topic_create uc15
publish_n uc15 14

# --- NetworkPolicy ----------------------------------------------------------------------------------------
if [ "${TABA_TEST_NO_NETPOL:-0}" = 1 ]; then
  skip "UC-15.3/4 NetworkPolicy" "TABA_TEST_NO_NETPOL=1 (CNI sem suporte)"
else
  K -n "$NS" run outsider --restart=Never --image=busybox:1.37 --labels=app=outsider -- sleep 3600 >/dev/null
  K -n "$NS" wait --for=condition=Ready pod/outsider --timeout=120s >/dev/null
  PODIP=$(K -n "$NS" get pod "$(pod0)" -o jsonpath='{.status.podIP}')
  check "UC-15.3 pod de fora NÃO alcança a porta de operações (9090)" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS exec outsider -- wget -qO- -T 5 http://$PODIP:9090/health/live"
  check "UC-15.4 pod de fora NÃO alcança a porta de cluster (8081)"   bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS exec outsider -- nc -z -w 5 $PODIP 8081"
  check "UC-15.5 pod de fora alcança a API (8080)"                    bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec outsider -- wget -S -q -O /dev/null -T 5 http://$(fullname):8080/topics 2>&1 | grep -q 'HTTP/'"
fi

# --- um nó qualquer cai -------------------------------------------------------------------------------------
K -n "$NS" delete pod "$(fullname)-1" --wait=false >/dev/null
sleep 5
wait_nodes 3 600 && wait_healthy 600 && pass "UC-15.6b nó 1 reentrou (3 prontos, sem condições)" || fail "UC-15.6b nó 1 não reentrou"

# --- o primeiro nó (ordinal 0) cai: volta como nó comum (os outros estão de pé) ---------------------------------
K -n "$NS" delete pod "$(pod0)" --wait=false >/dev/null
wait_nodes 3 600 && wait_healthy 600 && pass "UC-15.7 nó 0 (first-node) reentrou sem formar cluster novo" || fail "UC-15.7 nó 0 não reentrou"

# --- dois nós caem ao mesmo tempo: sem quórum por instantes, depois recupera ----------------------------------------
K -n "$NS" delete pod "$(fullname)-1" "$(fullname)-2" --wait=false >/dev/null
wait_nodes 3 900 && wait_healthy 900 && pass "UC-15.8 dois nós caíram juntos e o cluster se recuperou" || fail "UC-15.8 cluster não se recuperou"

tcli_up
check "UC-15.9 as 14 mensagens continuam lá"         test "$(consume_count uc15 14)" = 14

summary
