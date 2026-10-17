#!/usr/bin/env bash
# UC-03 — Segredos existentes: o chart usa um Secret criado pelo operador (root-token, tls.crt, tls.key),
# não gera nada e aceita a rotação do root token sem reiniciar.
UC=uc-03-segredos-existentes
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
need docker
cluster_up; trap cleanup EXIT; ns_fresh
export TABA_SECRETS_NAME=taba-meus-segredos

TMP=$(mktemp -d)
WT=$(winpath "$TMP")   # o kubectl/docker nativos do Windows não leem /tmp do Git Bash
docker run --rm --user 0 -v "$WT:/out" "rafaelpontani/taba:$TAG" init --out /out/s >/dev/null 2>&1 \
  && chmod -R a+r "$TMP/s"
CRT=$(ls "$TMP"/s/tls/*.crt 2>/dev/null | head -n1)
[ -n "$CRT" ] || { fail "UC-03.0 taba init gerou o certificado"; summary; exit 1; }
K -n "$NS" create secret generic "$TABA_SECRETS_NAME" \
  --from-file=root-token="$WT/s/root-token" --from-file=tls.crt="$(winpath "$CRT")" --from-file=tls.key="$(winpath "${CRT%.crt}.key")" >/dev/null
TOKEN_A=$(tr -d '\n' <"$TMP/s/root-token")

install "$CI/uc-03-segredos-existentes.yaml" || { fail "helm install"; summary; exit 1; }
check "UC-03.1 três pods prontos"                    wait_nodes 3
check "UC-03.2 cluster saudável"                     wait_healthy 300
check "UC-03.3 o chart não criou Job nem Secret próprio" bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS get secret $(fullname)-secrets 2>/dev/null"

restarts0() { K -n "$NS" get pods -l app.kubernetes.io/component=node -o jsonpath='{.items[*].status.containerStatuses[0].restartCount}' | tr ' ' '
' | awk '{s+=$1} END{print s+0}'; }
RESTARTS0=$(restarts0)
tcli_up
topic_create uc03
check "UC-03.4 token do operador é aceito"           publish_n uc03 3
check "UC-03.5 token errado é recusado"              bash -c "! kubectl ${KCTX:+--context $KCTX} -n $NS exec tcli -- env TABA_CLI_TOKEN=token-errado-token-errado-token-errado-1234 taba-cli topic list >/dev/null 2>&1"

# Rotação: duas linhas (novo, anterior) — o nó recarrega o arquivo a cada auth.reload-interval (60 s) e o
# kubelet atualiza o Secret montado em até ~1 min.
TOKEN_B=$(docker run --rm "rafaelpontani/taba:$TAG" token generate | tr -d '\n')
printf '%s\n%s\n' "$TOKEN_B" "$TOKEN_A" >"$TMP/rot"
K -n "$NS" create secret generic "$TABA_SECRETS_NAME" --dry-run=client -o yaml \
  --from-file=root-token="$WT/rot" --from-file=tls.crt="$(winpath "$CRT")" --from-file=tls.key="$(winpath "${CRT%.crt}.key")" | K -n "$NS" apply -f - >/dev/null
tok_b_works() { K -n "$NS" exec tcli -- env "TABA_CLI_TOKEN=$TOKEN_B" taba-cli topic list; }
check "UC-03.6 token novo passa a valer sem reiniciar (até 4 min)" wait_for 240 tok_b_works
check "UC-03.7 token anterior continua valendo na rotação" bash -c "kubectl ${KCTX:+--context $KCTX} -n $NS exec tcli -- env TABA_CLI_TOKEN=$TOKEN_A taba-cli topic list >/dev/null"
restarts() { restarts0; }
check "UC-03.8 nenhum pod reiniciou depois do primeiro start" test "$(restarts)" -le "$RESTARTS0"

rm -rf "$TMP"
summary
