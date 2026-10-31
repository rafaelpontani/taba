#!/usr/bin/env bash
# Perfil de CPU do servidor durante uma rodada do bench/run.sh. Roda ao lado dele (em outro terminal ou em segundo plano):
# espera o executor do OMB subir, acompanha ao vivo as taxas que o OMB imprime e o uso do servidor, e tira o perfil
# (`perf record -g` num contêiner privilegiado no espaço de processos do servidor) quando o backlog passa de um limite ou
# depois de um tempo. Tudo vai com horário para <saída>/monitor.log; cada perfil, para <saída>/perf-N/.
#
#   bench/profile.sh [--system taba] [--backlog 2000] [--after <s>] [--duration 30] [--repeat 1] [--interval 120]
#                    [--offcpu 10] [--server-grep <regex>] [--frequency 499] [--output <pasta>] [--wait-start 3600]
#
#   --backlog <K>   dispara quando o backlog do OMB chega a K mil mensagens (padrão 2000; 0 desliga)
#   --after <s>     dispara depois de s segundos de executor no ar, se o backlog ainda não disparou
#   --repeat <n>    quantos perfis tirar, com --interval segundos entre eles
#   --offcpu <s>    depois do perfil de CPU, um de espera (troca de contexto) de s segundos (padrão 10; 0 desliga)
#   --server-grep   as linhas do log do servidor que vão para o monitor.log (padrão "HUNT|panicked")
#   --output        padrão bench/results/perfil-<data>-<hora>
#
# Exemplo, perfil do consumo com o backlog em disco:
#   echo TABA_MEMORY_LIMIT=64MB >> bench/systems/taba/env-default    (desfazer depois)
#   bench/profile.sh --backlog 2000 --repeat 2 &
#   bench/run.sh --profile quick --systems taba --workloads backlog-1kb --taba-image taba:fp --allow-small
#
# A cadeia de chamadas só sai legível com a imagem compilada com frame pointers (`RUSTFLAGS` com
# `-C force-frame-pointers=yes`); sem eles, use os arquivos self.txt (tempo próprio de cada função).
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=bench/lib.sh
source "$HERE/lib.sh"

SYSTEM=taba
BACKLOG_K=2000
AFTER=""
DURATION=30
OFFCPU=10
SERVER_GREP="HUNT|panicked"
REPEAT=1
INTERVAL=120
FREQUENCY=499
OUTPUT=""
WAIT_START=3600
# O token das métricas do servidor do bench (o de desenvolvimento do compose.yml do Taba).
METRICS_TOKEN="${METRICS_TOKEN:-tb_dev.bench-dev-secret-0123456789abcdef0123456}"

while [ $# -gt 0 ]; do
  case "$1" in
    --system) SYSTEM="$2"; shift 2 ;;
    --backlog) BACKLOG_K="$2"; shift 2 ;;
    --after) AFTER="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --offcpu) OFFCPU="$2"; shift 2 ;;
    --server-grep) SERVER_GREP="$2"; shift 2 ;;
    --repeat) REPEAT="$2"; shift 2 ;;
    --interval) INTERVAL="$2"; shift 2 ;;
    --frequency) FREQUENCY="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --wait-start) WAIT_START="$2"; shift 2 ;;
    -h|--help) awk 'NR > 1 && !/^#/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) bench_die "argumento desconhecido: $1" ;;
  esac
done

SERVER="bench-$SYSTEM-server-1"
RUNNER="bench-runner-$SYSTEM"
OUTPUT="${OUTPUT:-$HERE/results/perfil-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUTPUT"
OUTPUT="$(cd "$OUTPUT" && pwd)"
LOG="$OUTPUT/monitor.log"
STATE="$OUTPUT/.backlog"
: > "$STATE"

log() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*" | tee -a "$LOG"; }
running() { [ -n "$(docker ps -q --filter "name=^$1$" 2>/dev/null)" ]; }

log "perfil de $SERVER; saída em $OUTPUT"
log "gatilho: backlog >= ${BACKLOG_K} K${AFTER:+ ou ${AFTER} s de executor}; $REPEAT perfil(s) de ${DURATION} s a ${FREQUENCY} Hz"

# 1. O executor do OMB no ar (o servidor sobe antes dele).
waited=0
until running "$RUNNER"; do
  [ "$waited" -ge "$WAIT_START" ] && { log "o executor $RUNNER não subiu em ${WAIT_START} s"; exit 1; }
  sleep 2; waited=$((waited + 2))
done
started=$(date +%s)
log "executor no ar"

# 2. As taxas do OMB, ao vivo: uma linha a cada 10 s, e o último backlog (em K) para o gatilho.
(
  docker logs -f "$RUNNER" 2>&1 | while IFS= read -r line; do
    case "$line" in
      *"Pub rate"*)
        # O horário do OMB vai junto: ao conectar, o `docker logs -f` entrega de uma vez o que já foi impresso.
        rates=$(printf '%s\n' "$line" | sed -nE 's/^([0-9:]+)[.0-9]* .*Pub rate +([0-9.]+) msg\/s.*Cons rate +([0-9.]+) msg\/s.*Backlog: +([0-9.]+) K.*/\1 \2 \3 \4/p')
        if [ -n "$rates" ]; then
          set -- $rates
          log "omb $1 publicação $2/s consumo $3/s backlog $4 K"
          echo "${4%.*}" > "$STATE"
        fi ;;
      *Aggregated*|*Exception*|*ERROR*) log "omb: ${line:0:220}" ;;
    esac
  done
) &
rates_pid=$!

# As linhas do log do servidor que casam com --server-grep (uma medição temporária, um pânico), ao vivo.
(
  docker logs -f "$SERVER" 2>&1 | grep --line-buffered -E "$SERVER_GREP" | while IFS= read -r line; do
    log "servidor: ${line:0:3000}"
  done
) &
server_pid=$!

# 3. CPU e memória do servidor a cada 15 s.
(
  while running "$RUNNER"; do
    stats=$(docker stats --no-stream --format '{{.CPUPerc}} {{.MemUsage}}' "$SERVER" 2>/dev/null)
    [ -n "$stats" ] && log "servidor cpu/memória $stats"
    sleep 15
  done
) &
stats_pid=$!

cleanup() { kill "$rates_pid" "$stats_pid" "$server_pid" 2>/dev/null; rm -f "$STATE"; }
trap cleanup EXIT

# As métricas de duração das rotas do Taba (porta de operações, lidas de dentro do contêiner do servidor), para a
# latência média de cada rota durante o perfil. Outro sistema não tem essa rota: fica vazio.
metrics() {
  [ "$SYSTEM" = taba ] || return 0
  docker exec "$SERVER" wget -qO- --header "Authorization: Bearer $METRICS_TOKEN" "http://127.0.0.1:9090/metrics" 2>/dev/null \
    | grep -E '^http_server_request_duration_seconds_(sum|count)\{' \
    | sed -E 's/^http_server_request_duration_seconds_(sum|count)\{.*http_request_method="([A-Z]+)".*http_route="([^"]+)".*\} (.*)$/\1 \2 \3 \4/'
}

# A latência média e as chamadas por segundo de cada rota entre duas leituras de `metrics`.
route_latency() {
  local before="$1" after="$2" seconds="$3"
  awk -v secs="$seconds" '
    FNR == NR { base[$1 " " $2 " " $3] += $4; next }
    { now[$1 " " $2 " " $3] += $4; routes[$2 " " $3] = 1 }
    END {
      for (r in routes) {
        n = now["count " r] - base["count " r]; t = now["sum " r] - base["sum " r]
        if (n > 0) printf "    %-45s %8.1f chamadas/s  média %8.1f ms\n", r, n / secs, 1000 * t / n
      }
    }' "$before" "$after" | sort -k3 -nr
}

# Roda o perf num contêiner privilegiado no espaço de processos do servidor (lá o processo 1 é o nó), com a pasta do
# perfil em /out e esta pasta em /bench.
perf_box() {
  local dir="$1"; shift
  docker run --rm --privileged --pid="container:$SERVER" -v "$(bench_hostpath "$dir"):/out" \
    -v "$(bench_hostpath "$HERE"):/bench:ro" alpine:3.20 sh -c "apk add --no-cache perf >/dev/null 2>&1 || exit 3; $*"
}

# 4. Um perfil: primeiro o de CPU (onde o tempo de processador vai), depois o de espera (onde as threads ficam paradas
# e por quanto tempo), com a latência por rota medida no intervalo do primeiro.
profile() {
  local n="$1" dir="$OUTPUT/perf-$1" status
  mkdir -p "$dir"
  docker image inspect alpine:3.20 >/dev/null 2>&1 || docker pull -q alpine:3.20 >/dev/null 2>&1
  log "perfil $n: CPU por ${DURATION} s"
  metrics > "$dir/metrics-before.txt"
  local t0; t0=$(date +%s)
  perf_box "$dir" "perf record -F $FREQUENCY -g -p 1 -o /tmp/cpu.data -- sleep $DURATION >/dev/null 2>&1; cp /tmp/cpu.data /out/cpu.data" >>"$LOG" 2>&1
  status=$?
  metrics > "$dir/metrics-after.txt"
  local secs=$(( $(date +%s) - t0 ))
  if [ "$OFFCPU" -gt 0 ] && [ "$status" -eq 0 ]; then
    log "perfil $n: espera por ${OFFCPU} s"
    # Duas gravações ao mesmo tempo (ver profile.awk): os tempos, de todas as CPUs e sem pilha, e as pilhas do
    # servidor, por DWARF.
    perf_box "$dir" "
      perf record -e sched:sched_switch -a -o /tmp/t.data -- sleep $((OFFCPU + 1)) >/dev/null 2>&1 &
      perf record -e sched:sched_switch -p 1 --call-graph dwarf,16384 -o /tmp/s.data -- sleep $OFFCPU >/dev/null 2>&1
      wait
      perf script -i /tmp/t.data -F tid,time,event,trace > /out/offcpu-times.txt 2>/dev/null
      perf script -i /tmp/s.data -F tid,time,event,ip,sym > /out/offcpu-stacks.txt 2>/dev/null
    " >>"$LOG" 2>&1 || log "perfil $n: o perfil de espera falhou"
  fi
  if [ "$status" -ne 0 ]; then
    log "perfil $n: falhou (código $status)"
    return
  fi
  log "perfil $n: gerando os relatórios"
  perf_box "$dir" "
    perf report -i /out/cpu.data --children --sort symbol --stdio -g none 2>/dev/null | grep -v '^\$' | grep -v '^#' | head -250 > /out/children.txt
    perf report -i /out/cpu.data --no-children --sort symbol --stdio -g none 2>/dev/null | grep -v '^\$' | grep -v '^#' | head -150 > /out/self.txt
    perf report -i /out/cpu.data --no-children --sort comm --stdio -g none 2>/dev/null | grep -v '^\$' | grep -v '^#' | head -30 > /out/threads.txt
    perf report -i /out/cpu.data --children --sort symbol --stdio -g caller,0.5,callee --percent-limit 2 2>/dev/null | grep -v '^\$' | head -4000 > /out/tree.txt
    if [ -s /out/offcpu-stacks.txt ]; then
      awk -v mode=offcpu -f /bench/profile.awk /out/offcpu-times.txt /out/offcpu-stacks.txt | sort -rn | head -200 > /out/offcpu.txt
    fi
  " >>"$LOG" 2>&1
  log "perfil $n: pronto em $dir"
  if [ -s "$dir/metrics-after.txt" ]; then
    log "perfil $n: latência média por rota durante o perfil (routes.txt):"
    route_latency "$dir/metrics-before.txt" "$dir/metrics-after.txt" "$secs" | tee "$dir/routes.txt" >> "$LOG"
  fi
  log "perfil $n: o que mais custa, com o que chama (children.txt):"
  head -45 "$dir/children.txt" | awk -v mode=cpu -f "$HERE/profile.awk" >> "$LOG"
  log "perfil $n: tempo próprio (self.txt):"
  head -20 "$dir/self.txt" | awk -v mode=self -f "$HERE/profile.awk" >> "$LOG"
  if [ -s "$dir/offcpu.txt" ]; then
    # A musl estática não tem frame pointer nem CFI: uma espera em futex (lock, condvar) para no `syscall`.
    log "perfil $n: onde as threads do servidor esperam, tempo somado em ${OFFCPU} s (offcpu.txt; [S] dormindo, [R] sem CPU):"
    head -25 "$dir/offcpu.txt" | cut -c1-300 | sed 's/^/    /' >> "$LOG"
  fi
  rm -f "$dir/cpu.data" "$dir/offcpu-times.txt" "$dir/offcpu-stacks.txt"
}

# 5. O gatilho, depois os perfis seguintes a cada --interval; para quando o executor sai.
taken=0
while running "$RUNNER" && [ "$taken" -lt "$REPEAT" ]; do
  backlog=$(cat "$STATE" 2>/dev/null); backlog=${backlog:-0}
  elapsed=$(( $(date +%s) - started ))
  fire=0
  if [ "$taken" -gt 0 ]; then
    [ "$elapsed" -ge "$next_at" ] && fire=1
  elif [ "$BACKLOG_K" -gt 0 ] && [ "$backlog" -ge "$BACKLOG_K" ]; then
    log "gatilho: backlog ${backlog} K"; fire=1
  elif [ -n "$AFTER" ] && [ "$elapsed" -ge "$AFTER" ]; then
    log "gatilho: ${elapsed} s de executor"; fire=1
  fi
  if [ "$fire" -eq 1 ]; then
    taken=$((taken + 1))
    profile "$taken"
    next_at=$(( $(date +%s) - started + INTERVAL ))
  fi
  sleep 2
done
[ "$taken" -lt "$REPEAT" ] && log "o executor saiu com $taken de $REPEAT perfil(s)"

# Continua anotando as taxas até o executor sair, para o log cobrir a rodada inteira.
while running "$RUNNER"; do sleep 5; done
log "executor saiu; fim"
