#!/usr/bin/env bash
# UC-02 — Perfil de um nó: replication.factor=1, 1 nó, sem auth e sem TLS de cluster, sem Secret.
UC=uc-02-desenvolvimento
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-02-desenvolvimento.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-02.1 um único pod pronto"                  wait_nodes 1
check "UC-02.2 nenhum Secret de cluster foi criado"  bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get secret $(secrets_name) 2>/dev/null"
check "UC-02.3 nenhum Job de init"                   bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get job 2>/dev/null | grep -q init"
check "UC-02.4 /health/ready 200"                    ops "$(pod0)" /health/ready
check "UC-02.5 réplicas ficam em 1 mesmo com replicaCount=3" test "$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.spec.replicas}')" = 1

tcli_up
topic_create uc02
check "UC-02.6 publica sem token"                    publish_n uc02 5
check "UC-02.7 consome as 5 mensagens"               test "$(consume_count uc02 5)" = 5

summary
