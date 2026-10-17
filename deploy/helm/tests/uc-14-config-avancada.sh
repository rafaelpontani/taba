#!/usr/bin/env bash
# UC-14 — Configuração avançada: várias chaves de config.*, extraEnv e limites. O nó RECUSA chave desconhecida,
# então subir pronto já prova que as chaves do values existem no nó; os demais testes provam o efeito.
UC=uc-14-config-avancada
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cluster_up; trap cleanup EXIT; ns_fresh
install "$CI/uc-14-config-avancada.yaml" || { fail "helm install"; summary; exit 1; }

check "UC-14.1 nós aceitaram todas as chaves e ficaram prontos" wait_nodes 3
check "UC-14.2 cluster saudável"                     wait_healthy 300
cfg() { K -n "$NS" exec "$(pod0)" -- cat /etc/taba/taba.yml; }
check_out "UC-14.3 ConfigMap montado com as chaves"  'max-bytes: 1MB' cfg
check_out "UC-14.4 allowed-ips na config"            '192\.168\.0\.0/16' cfg
check_out "UC-14.5 extraEnv chegou ao processo"      'TABA_API_PAGINATION_MAX_LIMIT=500' in_pod "$(pod0)" env
check_out "UC-14.6 logs em JSON no nível debug"      '^\{' bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS logs $(pod0) --tail=3"

tcli_up
topic_create uc14
big() { head -c "$1" /dev/zero | tr '\0' a | sed 's/.*/{"x":"&"}/'; }
small_ok() { big 500000 | tcli_in publish uc14 --stdin >/dev/null; }
check "UC-14.7 mensagem dentro do limite passa" small_ok
big_over() { big 1500000 | tcli_in publish uc14 --stdin >/dev/null 2>&1; }
check_fails "UC-14.8 mensagem acima de message.max-bytes (1MB) é recusada" big_over
check "UC-14.9 ops acessível do pod 'tcli' (CIDR liberado)" bash -c "test \"\$(kubectl ${KCTX:+--context $KCTX} -n $NS exec tcli -- wget -qO- --timeout=5 http://$(pod0).$(fullname)-headless:9090/health/live | grep -c live)\" -ge 1"

# Sobrescrever pela linha de comando precisa funcionar e reiniciar os nós (checksum da config).
H upgrade "$REL" "$CHART" -n "$NS" -f "$CI/uc-14-config-avancada.yaml" --set "image.tag=$TAG" --set config.message.max-bytes=3MB --wait --timeout 15m >/dev/null 2>&1
check "UC-14.10 helm upgrade --set muda a config nos nós" wait_for 300 bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec $(pod0) -- grep -q 'max-bytes: 3MB' /etc/taba/taba.yml"
check "UC-14.11 cluster volta saudável"              wait_healthy 600

summary
