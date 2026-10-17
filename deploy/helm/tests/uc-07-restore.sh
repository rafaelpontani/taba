#!/usr/bin/env bash
# UC-07 — Restore: com backup em SeaweedFS, para o cluster inteiro (parada planejada), apaga os volumes dos nós,
# sobe de novo e confere que as mensagens voltaram do bucket.
UC=uc-07-restore
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-05-backup-seaweedfs.yaml" || { fail "helm install"; summary; exit 1; }

wait_nodes 3 && wait_healthy 300 && pass "UC-07.1 três nós prontos e cluster saudável" || { fail "UC-07.1 cluster não ficou saudável"; summary; exit 1; }
tcli_up
topic_create uc07
check "UC-07.2 publica 15 mensagens"                 publish_n uc07 15
api_post() { K -n "$NS" exec "$(pod0)" -- wget -qO- --post-data= --header "Authorization: Bearer $(root_token)" "http://127.0.0.1:9090$1"; }
check "UC-07.3 flush do cluster (API, root token)" api_post /cluster/backup/flush
check "UC-07.4 parada planejada (cluster/stop)" api_post /cluster/stop

no_node_pvcs() { [ -z "$(K -n "$NS" get pvc -o name | grep "data-$(fullname)-[0-9]")" ]; }
log "escalando o StatefulSet a 0 (whenScaled=Delete apaga os volumes dos nós)"
K -n "$NS" scale sts "$(fullname)" --replicas=0 >/dev/null
check "UC-07.5 volumes dos nós foram apagados ao escalar a 0" wait_for 240 no_node_pvcs

K -n "$NS" scale sts "$(fullname)" --replicas=3 >/dev/null
check "UC-07.6 cluster volta com 3 nós"              wait_nodes 3 900
check "UC-07.7 cluster saudável após o restore"      wait_healthy 600
tcli_up
check "UC-07.8 as 15 mensagens voltaram do bucket"   test "$(consume_count uc07 15)" = 15

summary
