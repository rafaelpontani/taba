#!/usr/bin/env bash
# UC-06 — S3 externo (qualquer S3): o cluster aponta para um SeaweedFS que vive FORA do release (release "storage"),
# com chaves em Secret e prefixo próprio. (Credenciais por IRSA só renderizam: ver 00-static.sh.)
UC=uc-06-s3-externo
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh

log "instalando o 'S3 externo' (release storage: 1 nó + SeaweedFS)"
H upgrade --install storage "$CHART" -n "$NS" --set "image.tag=$TAG" \
  --set config.replication.factor=1 --set seaweedfs.enabled=true \
  --set seaweedfs.auth.accessKey=taba --set seaweedfs.auth.secretKey=taba-s3-secret \
  --set persistence.size=1Gi --wait --timeout 10m >&2 || { fail "helm install storage"; summary; exit 1; }

install "$CI/uc-06-s3-externo.yaml" || { fail "helm install"; summary; exit 1; }
bucket_ls() { s3_ls http://storage-taba-seaweedfs:8333 taba taba-s3-secret "$1"; }

check "UC-06.1 nós prontos"                          wait_nodes 3
check "UC-06.2 cluster saudável"                     wait_healthy 300
check "UC-06.3 o release não subiu S3 próprio"       bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get sts $(fullname)-seaweedfs 2>/dev/null"
check "UC-06.4 chaves vieram do Secret do chart"     bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get secret $(fullname)-s3 -o jsonpath='{.data.accessKey}' | base64 -d | grep -qx taba"
check "UC-06.5 chaves não estão no ConfigMap"        bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get cm $(fullname) -o yaml | grep -q taba-s3-secret"

tcli_up
topic_create uc06
check "UC-06.6 publica 20 mensagens"                 publish_n uc06 20
check "UC-06.7 flush do nó"                          bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- wget -qO- --post-data= --header \"Authorization: Bearer $(root_token)\" http://127.0.0.1:9090/node/backup/flush >/dev/null"
has_objects() { [ "$(bucket_ls taba/cluster-a/ 2>/dev/null | wc -l)" -gt 0 ]; }
check "UC-06.8 objetos no prefixo cluster-a (até 2 min)" wait_for 120 has_objects

summary
