#!/usr/bin/env bash
# UC-05 — Backup em S3 embutido (SeaweedFS): bucket criado pelo Job, nó com backup ligado, flush grava objetos no bucket.
UC=uc-05-backup-seaweedfs
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-05-backup-seaweedfs.yaml" || { fail "helm install"; summary; exit 1; }

SW="http://$(fullname)-seaweedfs:8333"
sw_key()    { K -n "$NS" get secret "$(fullname)-seaweedfs" -o jsonpath='{.data.accessKey}' | base64 -d; }
sw_secret() { K -n "$NS" get secret "$(fullname)-seaweedfs" -o jsonpath='{.data.secretKey}' | base64 -d; }
bucket_ls() { s3_ls "$SW" "$(sw_key)" "$(sw_secret)" "$1"; }

check "UC-05.1 SeaweedFS pronto"                     K -n "$NS" wait --for=condition=Ready "pod/$(fullname)-seaweedfs-0" --timeout=300s
check "UC-05.2 três nós prontos"                     wait_nodes 3
check "UC-05.3 cluster saudável"                     wait_healthy 300
check "UC-05.4 bucket 'taba' criado pelo Job"        bucket_ls taba/
check "UC-05.5 o nó alcança o S3 (s3Reachable)"      bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- wget -qO- http://127.0.0.1:9090/node/backup | grep -q 's3Reachable.:true'"
check "UC-05.6 credenciais do S3 só em Secret"       bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get cm $(fullname) -o yaml | grep -Eqi 'secret-key|access-key'"

tcli_up
topic_create uc05
check "UC-05.7 publica 20 mensagens"                 publish_n uc05 20
flush() { K -n "$NS" exec "$(pod0)" -- wget -qO- --post-data= --header "Authorization: Bearer $(root_token)" http://127.0.0.1:9090/node/backup/flush; }
check "UC-05.8 flush do nó responde"                 flush
has_objects() { [ "$(bucket_ls taba/taba/ 2>/dev/null | wc -l)" -gt 0 ]; }
check "UC-05.9 objetos de backup aparecem no bucket (até 2 min)" wait_for 120 has_objects
check "UC-05.10 o S3 exige credenciais (anônimo é recusado)" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- wget -qO- $SW/taba/ 2>/dev/null | grep -q 'ListBucketResult'"

summary
