#!/usr/bin/env bash
# Funções do bench/run.sh. Para `source`; sozinho não faz nada.

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_REPO_ROOT="$(cd "$BENCH_DIR/.." && pwd)"

# O Git Bash no Windows converte caminhos como /out.
export MSYS_NO_PATHCONV=1

bench_log() { printf '\n== %s\n' "$*"; }
bench_info() { printf '  %s\n' "$*"; }
bench_die() { echo "ERRO: $*" >&2; exit 1; }

# Caminho que o Docker aceita como origem de um bind mount (no Windows, C:/...).
bench_hostpath() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }
# A pasta bench/ no formato que o Docker e o Node.js aceitam como caminho de arquivo.
BENCH_DIR_NATIVE="$(bench_hostpath "$BENCH_DIR")"

# Núcleos e memória (GB, inteiro) que o Docker enxerga: "<núcleos> <gb>". Sem Docker, os valores de bench_default_resources.
bench_docker_resources() {
  local line
  if line="$(timeout 20 docker info --format '{{.NCPU}} {{.MemTotal}}' 2>/dev/null)" && [ -n "$line" ] && [ "${line%% *}" != "0" ]; then
    local cpus="${line%% *}" bytes="${line##* }"
    echo "$cpus $((bytes / 1024 / 1024 / 1024))"
  else
    return 1
  fi
}

# Divide a máquina: o servidor recebe metade dos núcleos (no máximo 4) e metade da memória (no máximo 8 GB); o executor,
# o resto. Imprime "<cpus do servidor> <gb do servidor> <cpus do executor> <gb do executor>".
bench_split() {
  local cpus="$1" gb="$2" s_cpus s_gb
  s_cpus=$((cpus / 2)); [ "$s_cpus" -gt 4 ] && s_cpus=4; [ "$s_cpus" -lt 1 ] && s_cpus=1
  s_gb=$((gb / 2)); [ "$s_gb" -gt 8 ] && s_gb=8; [ "$s_gb" -lt 1 ] && s_gb=1
  echo "$s_cpus $s_gb $((cpus - s_cpus)) $((gb - s_gb))"
}

# Os sistemas que existem: as pastas de bench/systems com compose.yml.
bench_all_systems() {
  local d
  for d in "$BENCH_DIR"/systems/*/; do
    [ -f "$d/compose.yml" ] && basename "$d"
  done
}

# Gira a lista: bench_rotate <deslocamento> <item>... imprime os itens começando pelo de índice (deslocamento % n).
bench_rotate() {
  local shift_by="$1"; shift
  local n=$# i
  local items=("$@")
  for ((i = 0; i < n; i++)); do
    echo "${items[$(((i + shift_by) % n))]}"
  done
}

# O arquivo de driver de um sistema para um workload e um nível de durabilidade: o específico do workload, se houver.
bench_driver_file() {
  local system="$1" durability="$2" workload="$3" dir="$BENCH_DIR/systems/$1"
  if [ -f "$dir/driver-$durability-$workload.yaml" ]; then
    echo "$dir/driver-$durability-$workload.yaml"
  elif [ -f "$dir/driver-$durability.yaml" ]; then
    echo "$dir/driver-$durability.yaml"
  else
    return 1
  fi
}

# Cópia de um workload com a duração e o aquecimento do perfil.
bench_render_workload() {
  local workload="$1" duration="$2" warmup="$3" out="$4"
  sed -e "s/@DURATION@/$duration/" -e "s/@WARMUP@/$warmup/" "$BENCH_DIR/workloads/$workload.yaml" > "$out"
}

# Estimativa do tempo total em minutos: cada célula leva duração + aquecimento + 1 min de montagem; cada subida, 1 min.
bench_estimate_minutes() {
  local systems="$1" durabilities="$2" workloads="$3" rounds="$4" duration="$5" warmup="$6"
  echo $((rounds * systems * durabilities * (1 + workloads * (duration + warmup + 1))))
}

bench_format_minutes() {
  local m="$1"
  if [ "$m" -ge 60 ]; then printf '%dh%02dmin' $((m / 60)) $((m % 60)); else printf '%dmin' "$m"; fi
}

# Sobe o servidor de um sistema e espera ficar saudável.
#   bench_system_up <sistema> <durabilidade>   (variáveis BENCH_* já exportadas)
bench_system_up() {
  local system="$1" durability="$2"
  export BENCH_DURABILITY="$durability"
  docker compose -p "bench-$system" -f "$BENCH_DIR_NATIVE/systems/$system/compose.yml" up -d --wait --wait-timeout 240 >&2
}

# Derruba o servidor e apaga o volume.
bench_system_down() {
  local system="$1"
  BENCH_DURABILITY="${BENCH_DURABILITY:-default}" docker compose -p "bench-$system" \
    -f "$BENCH_DIR_NATIVE/systems/$system/compose.yml" down -v --remove-orphans >/dev/null 2>&1 || true
}

# Uma célula: um workload contra o servidor que está no ar.
#   bench_run_cell <sistema> <driver.yaml> <workload.yaml> <pasta raw> <nome do json> <cpus> <gb> <heap gb> <timeout min>
bench_run_cell() {
  local system="$1" driver="$2" workload="$3" raw="$4" name="$5" cpus="$6" gb="$7" heap="$8" limit="$9"
  local container="bench-runner-$system"
  docker rm -f "$container" >/dev/null 2>&1 || true
  timeout "$((limit * 60))" docker run --rm --name "$container" --network "bench-${system}_default" \
    --cpus "$cpus" --memory "${gb}g" -e HEAP_OPTS="-Xms${heap}g -Xmx${heap}g" \
    -v "$(bench_hostpath "$(dirname "$driver")"):/drivers:ro" \
    -v "$(bench_hostpath "$(dirname "$workload")"):/workloads:ro" \
    -v "$(bench_hostpath "$raw"):/out" \
    "$BENCH_RUNNER_IMAGE" \
    "bin/benchmark --drivers /drivers/$(basename "$driver") --output /out/$name.json /workloads/$(basename "$workload") & p=\$!
     while kill -0 \$p 2>/dev/null; do
       if [ -s /out/$name.json ]; then sleep 3; kill \$p 2>/dev/null; break; fi
       sleep 1
     done
     wait \$p 2>/dev/null; [ -s /out/$name.json ]" \
    2>&1 | head -c 100000000 > "$raw/$name.log"
  local status=${PIPESTATUS[0]}
  docker rm -f "$container" >/dev/null 2>&1 || true
  return $status
}
