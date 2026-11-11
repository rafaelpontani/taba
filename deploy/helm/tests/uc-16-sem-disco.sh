#!/usr/bin/env bash
# UC-16 — Sem disco permanente: os nós usam emptyDir, nenhum PVC é criado para eles; um pod que cai volta
# com o disco vazio e o cluster se recupera pelas réplicas; quando TODOS os pods caem, as mensagens voltam
# do backup em S3.
UC=uc-16-sem-disco
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-16-sem-disco.yaml" || { fail "helm install"; summary; exit 1; }

wait_nodes 3 && wait_healthy 300 && pass "UC-16.1 três nós prontos e cluster saudável" || { fail "UC-16.1 cluster não ficou saudável"; summary; exit 1; }
no_node_pvcs() { [ -z "$(K -n "$NS" get pvc -o name | grep "data-$(fullname)-[0-9]")" ]; }
check "UC-16.2 nenhum PVC para os nós"               no_node_pvcs
check "UC-16.3 o volume data do pod é emptyDir"      test "$(K -n "$NS" get pod "$(pod0)" -o jsonpath='{.spec.volumes[?(@.name=="data")].emptyDir.sizeLimit}')" = 1Gi

tcli_up
topic_create uc16
check "UC-16.4 publica 15 mensagens"                 publish_n uc16 15

# --- um pod cai: volta com o disco vazio, as réplicas mantêm as mensagens -----------------------------------
K -n "$NS" delete pod "$(fullname)-1" --wait=false >/dev/null
sleep 5
wait_nodes 3 600 && wait_healthy 600 && pass "UC-16.5 o nó 1 voltou com o disco vazio e o cluster está saudável" || fail "UC-16.5 o nó 1 não reentrou"

# --- todos os pods caem: o disco de todos se perde e o backup restaura --------------------------------------------
api_post() { K -n "$NS" exec "$(pod0)" -- wget -qO- --post-data= --header "Authorization: Bearer $(root_token)" "http://127.0.0.1:9090$1"; }
check "UC-16.6 flush do cluster (API, root token)"   api_post /cluster/backup/flush
check "UC-16.7 parada planejada (cluster/stop)"      api_post /cluster/stop
log "escalando o StatefulSet a 0: os emptyDir dos nós somem com os pods"
K -n "$NS" scale sts "$(fullname)" --replicas=0 >/dev/null
no_node_pods() { [ -z "$(K -n "$NS" get pod -o name | grep "pod/$(fullname)-[0-9]")" ]; }
check "UC-16.8 todos os pods dos nós saíram"         wait_for 240 no_node_pods

K -n "$NS" scale sts "$(fullname)" --replicas=3 >/dev/null
check "UC-16.9 cluster volta com 3 nós"              wait_nodes 3 900
check "UC-16.10 cluster saudável após o restore"     wait_healthy 600
tcli_up
check "UC-16.11 as 15 mensagens voltaram do bucket"  test "$(consume_count uc16 15)" = 15

summary
