#!/usr/bin/env bash
# UC-12 — Autoscaling com KEDA: sobe KEDA, instala o chart com o Prometheus do chart e prova
#   (a) scale-up por volume de requisições,
#   (b) scale-down de UM nó por vez, com o nó drenando antes de sair (nenhuma mensagem perdida),
#   (c) piso de 3 nós e cluster saudável depois.
# Demora: ~15 min (janelas de estabilização reais, só encurtadas pelo ci/uc-12).
UC=uc-12-autoscaling-keda
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh

KEDA_VERSION=${TABA_TEST_KEDA_VERSION:-2.17.2}
if ! H status keda -n keda >/dev/null 2>&1; then
  log "instalando KEDA $KEDA_VERSION"
  H repo add kedacore https://kedacore.github.io/charts >/dev/null 2>&1
  H repo update kedacore >/dev/null 2>&1
  H upgrade --install keda kedacore/keda -n keda --create-namespace --version "$KEDA_VERSION" --wait --timeout 5m >&2 \
    || { skip "UC-12" "não consegui instalar o KEDA (sem rede?)"; exit 77; }
fi

install "$CI/uc-12-autoscaling-keda.yaml" || { fail "helm install"; summary; exit 1; }
SO=$(fullname)

check "UC-12.1 nós prontos (piso 3)"                 wait_nodes 3
check "UC-12.2 ScaledObject Ready"                   wait_for 120 bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get scaledobject $SO -o jsonpath='{.status.conditions[?(@.type==\"Ready\")].status}' | grep -q True"
check "UC-12.3 KEDA criou o HPA do StatefulSet"      K -n "$NS" get hpa "keda-hpa-$SO"
check "UC-12.4 HPA com scale-down de 1 pod"          bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get hpa keda-hpa-$SO -o jsonpath='{.spec.behavior.scaleDown.policies[0].value}' | grep -qx 1"
check "UC-12.5 cluster saudável"                     wait_healthy 300
tcli_up
topic_create uc12

# --- (a) scale-up: alvo de req/s por nó quase zero + tráfego → o HPA pede o máximo (4) -----------------
log "forçando scale-up (perNodeRps=0.05) e gerando tráfego"
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-12-autoscaling-keda.yaml" --set "image.tag=$TAG" --set autoscaling.requests.perNodeRps=0.05 --wait --timeout 5m >/dev/null 2>&1
( for i in $(seq 1 600); do printf '{"n":%d}\n' "$i" | tcli publish uc12 --stdin >/dev/null 2>&1; sleep 0.5; done ) &
LOAD=$!
check "UC-12.6 scale-up para 4 nós (até 8 min)"     wait_for 480 ready_is 4
kill "$LOAD" 2>/dev/null; wait "$LOAD" 2>/dev/null
check "UC-12.7 cluster saudável com 4 nós"           wait_healthy 300

# --- (b) scale-down seguro ------------------------------------------------------------------------------
consume_count uc12 100000 >/dev/null 2>&1 || true   # esvazia o que o gerador de carga publicou
topic_create uc12b
publish_n uc12b 20
log "voltando o alvo para 1000 req/s por nó: o HPA deve reduzir 1 nó por vez"
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-12-autoscaling-keda.yaml" --set "image.tag=$TAG" --wait --timeout 5m >/dev/null 2>&1
MIN_SEEN=4
for ((i = 0; i < 90; i++)); do
  n=$(K -n "$NS" get sts "$SO" -o jsonpath='{.status.replicas}')
  [ "${n:-4}" -lt "$MIN_SEEN" ] && MIN_SEEN=$n
  [ "$n" = 3 ] && break
  sleep 5
done
check "UC-12.8 reduziu para o piso de 3 nós"         test "$MIN_SEEN" = 3
check "UC-12.9 nunca abaixo de 3"                    test "$(K -n "$NS" get sts "$SO" -o jsonpath='{.status.replicas}')" -ge 3
check "UC-12.10 cluster saudável depois do scale-down" wait_healthy 300
check "UC-12.11 as 20 mensagens sobreviveram (drenadas para outro nó)" test "$(consume_count uc12b 20)" = 20
check "UC-12.12 PVC do nó removido foi apagado (whenScaled=Delete)" bash -c "test -z \"\$(kubectl ${KCTX:+--context $KCTX} -n $NS get pvc -o name | grep 'data-$SO-3')\""

# --- (c) pausa -------------------------------------------------------------------------------------------
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-12-autoscaling-keda.yaml" --set "image.tag=$TAG" --set autoscaling.paused=true --set autoscaling.pausedReplicas=3 --wait --timeout 5m >/dev/null 2>&1
check "UC-12.13 annotation de pausa aplicada"        bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS get scaledobject $SO -o jsonpath='{.metadata.annotations}' | grep -q paused-replicas"

summary
