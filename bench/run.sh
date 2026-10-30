#!/usr/bin/env bash
# Benchmark comparativo local: roda o OpenMessaging Benchmark contra o Taba e contra outros sistemas de mensageria, tudo
# em Docker, um sistema por vez, com os mesmos limites de CPU e memória. Detalhes em bench/README.md.
#   bench/run.sh                                  # perfil quick, todos os sistemas, durabilidade padrão
#   bench/run.sh --profile standard
#   bench/run.sh --systems taba,kafka --workloads fanout-1kb
#   bench/run.sh --durability fsync               # default, fsync ou both
#   bench/run.sh --taba-image rafaelpontani/taba:0.1.9
#   bench/run.sh --dry-run                        # imprime o plano e termina
#   bench/run.sh --allow-small                    # roda abaixo do mínimo (só para testar o script; números sem valor)
#   bench/run.sh --report-only bench/results/<pasta>
# O resultado de uma máquina local serve para comparar os sistemas entre si, na mesma máquina e na mesma rodada; não é
# número absoluto.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

PROFILE=quick
SYSTEMS=""
WORKLOADS=""
DURABILITY=default
TABA_IMAGE=""
CPUS=""
MEMORY=""
DRY_RUN=0
REPORT_ONLY=""
OUTPUT=""
MIN_CPUS=4
MIN_GB=8
ALLOW_SMALL=0

usage() { sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --profile) PROFILE="$2"; shift 2 ;;
    --systems) SYSTEMS="${2//,/ }"; shift 2 ;;
    --workloads) WORKLOADS="${2//,/ }"; shift 2 ;;
    --durability) DURABILITY="$2"; shift 2 ;;
    --taba-image) TABA_IMAGE="$2"; shift 2 ;;
    --cpus) CPUS="$2"; shift 2 ;;
    --memory) MEMORY="$2"; shift 2 ;;
    --output) OUTPUT="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --allow-small) ALLOW_SMALL=1; shift ;;
    --report-only) REPORT_ONLY="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; bench_die "argumento desconhecido: $1" ;;
  esac
done

if [ -n "$REPORT_ONLY" ]; then
  command -v node >/dev/null || bench_die "o relatório precisa do Node.js 22"
  exec node "$BENCH_DIR_NATIVE/report.mjs" "$(bench_hostpath "$REPORT_ONLY")"
fi

[ -f "$BENCH_DIR/profile-$PROFILE" ] || bench_die "perfil desconhecido: $PROFILE (quick, standard)"
# shellcheck disable=SC1090
source "$BENCH_DIR/profile-$PROFILE"
[ -n "$WORKLOADS" ] || WORKLOADS="$PROFILE_WORKLOADS"

# Sistemas
if [ -z "$SYSTEMS" ]; then SYSTEMS="$(bench_all_systems | tr '\n' ' ')"; fi
for s in $SYSTEMS; do
  [ -f "$BENCH_DIR/systems/$s/compose.yml" ] || bench_die "sistema desconhecido: $s (existentes: $(bench_all_systems | tr '\n' ' '))"
done
# Workloads
for w in $WORKLOADS; do
  [ -f "$BENCH_DIR/workloads/$w.yaml" ] || bench_die "workload desconhecido: $w"
done
# Durabilidade
case "$DURABILITY" in
  default|fsync) DURABILITIES="$DURABILITY" ;;
  both) DURABILITIES="default fsync" ;;
  *) bench_die "durabilidade desconhecida: $DURABILITY (default, fsync, both)" ;;
esac
for s in $SYSTEMS; do
  for d in $DURABILITIES; do
    for w in $WORKLOADS; do
      bench_driver_file "$s" "$d" "$w" >/dev/null || bench_die "o sistema $s não tem driver para a durabilidade $d"
    done
  done
done

# Recursos
if resources="$(bench_docker_resources)"; then
  total_cpus="${resources% *}"; total_gb="${resources#* }"
elif [ "$DRY_RUN" -eq 1 ]; then
  total_cpus=8; total_gb=16
  echo "aviso: o Docker não responde; o plano usa uma máquina de ${total_cpus} núcleos e ${total_gb} GB" >&2
else
  bench_die "o Docker não está disponível"
fi
if [ "$total_cpus" -lt "$MIN_CPUS" ] || [ "$total_gb" -lt "$MIN_GB" ]; then
  msg="o Docker enxerga ${total_cpus} núcleos e ${total_gb} GB; o mínimo é ${MIN_CPUS} núcleos e ${MIN_GB} GB (falta: $([ "$total_cpus" -lt "$MIN_CPUS" ] && echo "$((MIN_CPUS - total_cpus)) núcleos ")$([ "$total_gb" -lt "$MIN_GB" ] && echo "$((MIN_GB - total_gb)) GB"))"
  if [ "$DRY_RUN" -eq 1 ] || [ "$ALLOW_SMALL" -eq 1 ]; then echo "aviso: $msg (os números desta rodada não valem)" >&2; else bench_die "$msg"; fi
fi
read -r SERVER_CPUS SERVER_GB RUNNER_CPUS RUNNER_GB < <(bench_split "$total_cpus" "$total_gb")
[ -z "$CPUS" ] || SERVER_CPUS="$CPUS"
[ -z "$MEMORY" ] || SERVER_GB="${MEMORY%[gG]}"
RUNNER_HEAP_GB=$((RUNNER_GB * 6 / 10)); [ "$RUNNER_HEAP_GB" -lt 1 ] && RUNNER_HEAP_GB=1

n_systems=$(wc -w <<<"$SYSTEMS"); n_workloads=$(wc -w <<<"$WORKLOADS"); n_dur=$(wc -w <<<"$DURABILITIES")
estimate="$(bench_estimate_minutes "$n_systems" "$n_dur" "$n_workloads" "$ROUNDS" "$DURATION" "$WARMUP")"

commit="$((cd "$BENCH_REPO_ROOT" && git rev-parse --short HEAD) 2>/dev/null || echo unknown)"
omb_commit="$(tr -d '[:space:]' < "$BENCH_REPO_ROOT/connectors/omb/OMB_VERSION")"
[ -n "$OUTPUT" ] || OUTPUT="$BENCH_DIR/results/$(date +%Y%m%d-%H%M%S)-$commit"

bench_log "Plano (resultado de máquina local: compara os sistemas entre si, não é número absoluto)"
bench_info "perfil:        $PROFILE ($ROUNDS rodada(s), $DURATION min + $WARMUP min de aquecimento)"
bench_info "sistemas:      $SYSTEMS"
bench_info "workloads:     $WORKLOADS"
bench_info "durabilidade:  $DURABILITIES"
bench_info "máquina:       $total_cpus núcleos, $total_gb GB"
bench_info "servidor:      $SERVER_CPUS núcleos, ${SERVER_GB} GB (igual para todos)"
bench_info "executor OMB:  $RUNNER_CPUS núcleos, ${RUNNER_GB} GB (heap ${RUNNER_HEAP_GB} GB)"
bench_info "tempo estimado: $(bench_format_minutes "$estimate")"
bench_info "saída:         $OUTPUT"
if [ "$DRY_RUN" -eq 1 ]; then
  for ((r = 0; r < ROUNDS; r++)); do
    # shellcheck disable=SC2086
    bench_info "rodada $((r + 1)): ordem $(bench_rotate "$r" $SYSTEMS | tr '\n' ' ')"
  done
  exit 0
fi

command -v docker >/dev/null || bench_die "docker não encontrado"
docker compose version >/dev/null 2>&1 || bench_die "o Docker Compose v2 é necessário"

RAW="$OUTPUT/raw"; WORK="$OUTPUT/work"
mkdir -p "$RAW" "$WORK"
CURRENT_SYSTEM=""
cleanup() {
  local status=$?
  [ -z "$CURRENT_SYSTEM" ] || bench_system_down "$CURRENT_SYSTEM"
  exit "$status"
}
trap cleanup EXIT
trap 'echo; echo "interrompido" >&2; exit 130' INT TERM

# Imagens
export BENCH_RUNNER_IMAGE="taba-bench-runner:local"
bench_log "Imagem do executor ($BENCH_RUNNER_IMAGE, OMB $omb_commit)"
docker build -q -f "$BENCH_DIR_NATIVE/Dockerfile.runner" --build-arg "OMB_COMMIT=$omb_commit" -t "$BENCH_RUNNER_IMAGE" "$(bench_hostpath "$BENCH_REPO_ROOT")" >/dev/null
case " $SYSTEMS " in
  *" taba "*)
    if [ -z "$TABA_IMAGE" ]; then
      TABA_IMAGE="taba:bench-$commit"
      bench_log "Imagem do Taba ($TABA_IMAGE, construída do commit atual)"
      docker build -q -t "$TABA_IMAGE" "$(bench_hostpath "$BENCH_REPO_ROOT")" >/dev/null
    fi ;;
esac

export BENCH_TABA_IMAGE="${TABA_IMAGE:-unused}"
export BENCH_CPUS="$SERVER_CPUS" BENCH_MEMORY="${SERVER_GB}g"
export BENCH_SMP="$SERVER_CPUS" BENCH_REDPANDA_MEMORY="$((SERVER_GB * 1024 * 3 / 4))M"

# machine.json
cpu_model="$(docker run --rm "$BENCH_RUNNER_IMAGE" "grep -m1 'model name' /proc/cpuinfo | cut -d: -f2" 2>/dev/null | sed 's/^ *//' | tr -d '"' || true)"
{
  printf '{\n'
  printf '  "date": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '  "taba_commit": "%s",\n' "$commit"
  printf '  "omb_commit": "%s",\n' "$omb_commit"
  printf '  "taba_image": "%s",\n' "${TABA_IMAGE:-}"
  printf '  "profile": "%s",\n' "$PROFILE"
  printf '  "rounds": %s,\n' "$ROUNDS"
  printf '  "duration_minutes": %s,\n' "$DURATION"
  printf '  "warmup_minutes": %s,\n' "$WARMUP"
  printf '  "systems": "%s",\n' "$SYSTEMS"
  printf '  "workloads": "%s",\n' "$WORKLOADS"
  printf '  "durabilities": "%s",\n' "$DURABILITIES"
  printf '  "cpu_model": "%s",\n' "${cpu_model:-desconhecida}"
  printf '  "docker_cpus": %s,\n' "$total_cpus"
  printf '  "docker_memory_gb": %s,\n' "$total_gb"
  printf '  "docker_version": "%s",\n' "$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo ?)"
  printf '  "docker_os": "%s",\n' "$(docker info --format '{{.OperatingSystem}}' 2>/dev/null | tr -d '"' || echo ?)"
  printf '  "server_cpus": %s,\n' "$SERVER_CPUS"
  printf '  "server_memory_gb": %s,\n' "$SERVER_GB"
  printf '  "runner_cpus": %s,\n' "$RUNNER_CPUS"
  printf '  "runner_memory_gb": %s,\n' "$RUNNER_GB"
  printf '  "images": {"taba": "%s", "kafka": "apache/kafka:3.8.0", "redpanda": "redpandadata/redpanda:v24.2.7", "rabbitmq": "rabbitmq:3.13.7", "nats": "nats:2.10.22-alpine"}\n' "${TABA_IMAGE:-}"
  printf '}\n'
} > "$OUTPUT/machine.json"

# Os arquivos de workload do perfil.
for w in $WORKLOADS; do bench_render_workload "$w" "$DURATION" "$WARMUP" "$WORK/$w.yaml"; done

timeout_min=$(((DURATION + WARMUP) * 3 + 15))
failed=0
for ((r = 0; r < ROUNDS; r++)); do
  # shellcheck disable=SC2046,SC2086
  for system in $(bench_rotate "$r" $SYSTEMS); do
    for dur in $DURABILITIES; do
      bench_log "Rodada $((r + 1))/$ROUNDS: $system ($dur)"
      CURRENT_SYSTEM="$system"
      bench_system_down "$system"
      if ! bench_system_up "$system" "$dur"; then
        echo "FALHOU: $system não ficou saudável" >&2
        echo "$system $dur subida $((r + 1))" >> "$OUTPUT/failed.txt"
        docker compose -p "bench-$system" -f "$BENCH_DIR_NATIVE/systems/$system/compose.yml" logs --tail 30 >&2 || true
        bench_system_down "$system"; CURRENT_SYSTEM=""; failed=1
        continue
      fi
      for w in $WORKLOADS; do
        name="$system-$dur-$w-$((r + 1))"
        bench_info "$name"
        driver="$(bench_driver_file "$system" "$dur" "$w")"
        if ! bench_run_cell "$system" "$driver" "$WORK/$w.yaml" "$RAW" "$name" "$RUNNER_CPUS" "$RUNNER_GB" "$RUNNER_HEAP_GB" "$timeout_min"; then
          echo "FALHOU: $name (veja $RAW/$name.log)" >&2
          echo "$system $dur $w $((r + 1))" >> "$OUTPUT/failed.txt"
          failed=1
        fi
      done
      bench_system_down "$system"; CURRENT_SYSTEM=""
    done
  done
done

if command -v node >/dev/null; then
  bench_log "Relatório"
  node "$BENCH_DIR_NATIVE/report.mjs" "$(bench_hostpath "$OUTPUT")"
else
  echo "Node.js não encontrado: gere o relatório depois com bench/run.sh --report-only $OUTPUT" >&2
fi
[ "$failed" -eq 0 ] || { echo "Algumas células falharam (failed.txt)." >&2; exit 1; }
