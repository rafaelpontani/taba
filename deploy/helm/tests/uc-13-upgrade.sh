#!/usr/bin/env bash
# UC-13 — Upgrade rolling: `helm upgrade` troca a configuração de todos os nós, um por vez, sem perder
# mensagens e sem nunca ficar com menos de 2 nós prontos.
UC=uc-13-upgrade
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-13-upgrade.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-13.1 três nós prontos"                     wait_nodes 3
check "UC-13.2 cluster saudável"                     wait_healthy 300
tcli_up
topic_create uc13
check "UC-13.3 publica 14 mensagens"                 publish_n uc13 14

SECRET_RV=$(K -n "$NS" get secret "$(secrets_name)" -o jsonpath='{.metadata.resourceVersion}')
OLD_REV=$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.currentRevision}')
MIN_READY=3
( while :; do
    r=$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.readyReplicas}' 2>/dev/null)
    echo "${r:-0}" >>"${TMPDIR:-/tmp}/uc13-ready.$$"; sleep 2
  done ) &
SAMPLER=$!
log "helm upgrade: muda message.max-bytes (checksum da config muda → rollout)"
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-13-upgrade.yaml" --set "image.tag=$TAG" --set config.message.max-bytes=2MB --wait --timeout 15m >&2
UP=$?
kill "$SAMPLER" 2>/dev/null; wait "$SAMPLER" 2>/dev/null
MIN_READY=$(sort -n "${TMPDIR:-/tmp}/uc13-ready.$$" | head -n1); rm -f "${TMPDIR:-/tmp}/uc13-ready.$$"

check "UC-13.4 helm upgrade concluiu"                test "$UP" = 0
check "UC-13.5 o StatefulSet trocou de revisão"      test "$(K -n "$NS" get sts "$(fullname)" -o jsonpath='{.status.currentRevision}')" != "$OLD_REV"
check "UC-13.6 nunca menos de 2 nós prontos (maxUnavailable=1)" test "${MIN_READY:-0}" -ge 2
check "UC-13.7 três nós prontos depois"              wait_nodes 3
check "UC-13.8 cluster saudável depois"              wait_healthy 600
check "UC-13.9 nova config aplicada em todos os nós" bash -c "for i in 0 1 2; do kubectl ${KCTX:+--context $KCTX} -n $NS exec $(fullname)-\$i -- grep -q 'max-bytes: 2MB' /etc/taba/taba.yml || exit 1; done"
tcli_up
check "UC-13.10 as 14 mensagens estão lá (14/14)"    test "$(consume_count uc13 14)" = 14
check "UC-13.11 o Secret do cluster não foi regerado" test "$(K -n "$NS" get secret "$(secrets_name)" -o jsonpath='{.metadata.resourceVersion}')" = "$SECRET_RV"

summary
