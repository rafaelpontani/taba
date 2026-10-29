#!/usr/bin/env bash
# Throughput trend of the publish path: one node (replication.factor 1) and three nodes (factor 3), the same cores per
# node, the load made by wrk spread across the nodes, all inside one Docker network. Both profiles use the production
# configuration (mutual TLS in the cluster, the root token of `taba init`), like the Compose.
#   bench/cluster-trend.sh <cores per node> <connections per node> <image>...
#   bench/cluster-trend.sh 3 64 taba:before taba:after
# Environment: BODY_BYTES (message size, default 1032 = 1 KB of payload), ROUNDS (default 3), SECONDS_PER_ROUND (15),
# NODES ("1 3": which sizes to run).
# A result from one machine is never an absolute number: compare images measured by the same run.
set -uo pipefail

CPUS=${1:?cores per node}; CONNS=${2:?connections per node}; shift 2
[ $# -gt 0 ] || { echo "usage: $0 <cores per node> <connections per node> <image>..." >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
ENVF=$HERE/systems/taba/env-default
BODY_BYTES=${BODY_BYTES:-1032}
ROUNDS=${ROUNDS:-3}
SECONDS_PER_ROUND=${SECONDS_PER_ROUND:-15}
NODES=${NODES:-"1 3"}
NET=tb-net

docker network create $NET >/dev/null 2>&1
# Plays the role of the load balancer with no node behind it: the name resolves but nobody listens, so the first node
# starts the cluster alone.
docker rm -f tb-entry >/dev/null 2>&1
docker run -d --name tb-entry --network $NET alpine:3.20 sleep 7200 >/dev/null

node() { # name factor
  local extra=(-e TABA_CLUSTER_ENTRY_ADDRESS=tb-n0:8081)
  [ "$1" = tb-n0 ] && extra=(-e TABA_CLUSTER_ENTRY_ADDRESS=tb-entry:8081 -e TABA_CLUSTER_FIRST_NODE=true)
  docker run -d --name "$1" --hostname "$1" --network $NET --cpus "$CPUS" -m 3g --env-file "$ENVF" \
    -v tb-secrets:/etc/taba/secrets:ro -e TABA_REPLICATION_FACTOR="$2" -e TABA_OPERATIONS_PORT=9090 \
    "${extra[@]}" "$image" >/dev/null
}
ready() {
  for _ in $(seq 1 120); do
    docker exec "$1" wget -q -O - http://127.0.0.1:9090/health/ready 2>/dev/null | grep -q ready && return 0
    sleep 1
  done
  echo "$1 did not become ready"; docker logs --tail 15 "$1"; return 1
}
cleanup() { docker rm -f -v tb-n0 tb-n1 tb-n2 >/dev/null 2>&1; }
finish() { cleanup; docker rm -f tb-entry >/dev/null 2>&1; docker volume rm tb-secrets >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1; }
trap finish EXIT

LUA='wrk.method = "POST"
wrk.body = string.rep("a", tonumber(os.getenv("BODY_BYTES")))
wrk.headers["Content-Type"] = "application/octet-stream"
wrk.headers["Authorization"] = os.getenv("AUTH")'

for image in "$@"; do
  docker volume rm tb-secrets >/dev/null 2>&1
  MSYS_NO_PATHCONV=1 docker run --rm --user 0 --entrypoint sh -v tb-secrets:/secrets "$image" \
    -c 'taba init --out /secrets >/dev/null && chown -R 10001:10001 /secrets'
  TOKEN=$(MSYS_NO_PATHCONV=1 docker run --rm --entrypoint cat -v tb-secrets:/secrets "$image" /secrets/root-token | tr -d '\r\n')
  for nodes in $NODES; do
    cleanup
    node tb-n0 "$nodes"; ready tb-n0 || continue
    if [ "$nodes" -ge 3 ]; then
      node tb-n1 "$nodes"; ready tb-n1 || continue
      node tb-n2 "$nodes"; ready tb-n2 || continue
    fi
    targets=$(for i in $(seq 0 $((nodes-1))); do printf "tb-n%s " "$i"; done)
    MSYS_NO_PATHCONV=1 docker run --rm --network $NET --cpus 3 \
      -e TOKEN_FULL="$TOKEN" -e LABEL="$image nodes=$nodes rf=$nodes" -e TARGETS="$targets" -e CONNS="$CONNS" \
      -e BODY_BYTES="$BODY_BYTES" -e ROUNDS="$ROUNDS" -e SECONDS_PER_ROUND="$SECONDS_PER_ROUND" -e LUA="$LUA" \
      alpine:3.20 sh -c '
      apk add -q curl wrk >/dev/null 2>&1
      printf "%s\n" "$LUA" > /tmp/post.lua
      export AUTH="Bearer $TOKEN_FULL"; first=${TARGETS%% *}
      code=$(curl -s -o /tmp/create -w "%{http_code}" -H "Authorization: $AUTH" -H "Content-Type: application/json" -X POST http://$first:8080/topics -d "{\"name\":\"wd-q\",\"type\":\"queue\"}")
      [ "$code" = 201 ] || { echo "create topic: $code $(cat /tmp/create)"; exit 1; }
      sleep 2
      for round in $(seq 1 $ROUNDS); do
        for t in $TARGETS; do wrk -t1 -c$CONNS -d${SECONDS_PER_ROUND}s --latency -s /tmp/post.lua http://$t:8080/topics/wd-q/messages > /tmp/w-$t 2>&1 & done
        wait
        rps=$(cat /tmp/w-* | awk "/Requests\/sec/ {s+=\$2} END {printf \"%.0f\", s}")
        p50=$(cat /tmp/w-* | awk "/ 50%/ {print \$2}" | tr "\n" " ")
        p99=$(cat /tmp/w-* | awk "/ 99%/ {print \$2}" | tr "\n" " ")
        non=$(cat /tmp/w-* | awk "/Non-2xx/ {s+=\$NF} END {print s+0}")
        echo "$LABEL round=$round total=$rps msg/s p50=[$p50] p99=[$p99] non2xx=$non"
      done' &
    sleep $((SECONDS_PER_ROUND * 2 - 2))
    docker stats --no-stream --format '   {{.Name}} cpu={{.CPUPerc}} mem={{.MemUsage}}' $(docker ps --format '{{.Names}}' | grep '^tb-n')
    wait
  done
done
