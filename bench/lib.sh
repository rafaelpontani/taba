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

# O arquivo de driver de um sistema para um workload, um nível de durabilidade e um número de nós: o específico do
# workload, se houver, e o do cluster antes do de um nó. Sem driver de cluster, o de um nó serve (o Taba fala com o
# HAProxy, que se chama server como o nó único).
#   bench_driver_file <sistema> <durabilidade> <workload> [nós]
bench_driver_file() {
  local system="$1" durability="$2" workload="$3" nodes="${4:-1}" dir="$BENCH_DIR/systems/$1" f
  local names=("driver-$durability-$workload" "driver-$durability")
  [ "$nodes" -gt 1 ] && names=("driver-$durability-$workload-cluster" "driver-$durability-cluster" "${names[@]}")
  for f in "${names[@]}"; do
    if [ -f "$dir/$f.yaml" ]; then echo "$dir/$f.yaml"; return 0; fi
  done
  return 1
}

# O compose de um sistema: compose.yml (um nó) ou compose-cluster.yml (três nós, serviços node-0, node-1 e node-2).
# Falha quando o sistema não tem cluster.
bench_compose_file() {
  local system="$1" nodes="${2:-1}"
  if [ "$nodes" -gt 1 ]; then
    [ -f "$BENCH_DIR/systems/$system/compose-cluster.yml" ] || return 1
    echo "$BENCH_DIR_NATIVE/systems/$system/compose-cluster.yml"
  else
    echo "$BENCH_DIR_NATIVE/systems/$system/compose.yml"
  fi
}

# O nome de uma célula: <sistema>[-x<nós>]-<durabilidade>-<workload>-<rodada> (o relatório lê o nome do arquivo).
bench_cell_name() {
  local system="$1" nodes="$2" durability="$3" workload="$4" round="$5"
  if [ "$nodes" -gt 1 ]; then echo "$system-x$nodes-$durability-$workload-$round"; else echo "$system-$durability-$workload-$round"; fi
}

# Divide a máquina para um cluster: cada nó recebe metade dos núcleos e da memória que um nó único receberia (no mínimo
# 1 núcleo e 512 MB); o executor fica com o resto, menos 1 núcleo para o balanceador do Taba. Imprime
# "<cpus por nó> <mb por nó> <cpus do executor> <gb do executor>".
bench_split_cluster() {
  local cpus="$1" gb="$2" nodes="$3" single_cpus single_gb n_cpus n_mb r_cpus r_gb
  read -r single_cpus single_gb _ _ < <(bench_split "$cpus" "$gb")
  n_cpus=$((single_cpus / 2)); [ "$n_cpus" -lt 1 ] && n_cpus=1
  n_mb=$((single_gb * 1024 / 2)); [ "$n_mb" -lt 512 ] && n_mb=512
  r_cpus=$((cpus - n_cpus * nodes - 1)); [ "$r_cpus" -lt 1 ] && r_cpus=1
  r_gb=$(((gb * 1024 - n_mb * nodes) / 1024)); [ "$r_gb" -lt 1 ] && r_gb=1
  echo "$n_cpus $n_mb $r_cpus $r_gb"
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
#   bench_system_up <sistema> <durabilidade> [nós]   (variáveis BENCH_* já exportadas)
bench_system_up() {
  local system="$1" durability="$2" nodes="${3:-1}"
  export BENCH_DURABILITY="$durability"
  docker compose -p "bench-$system" -f "$(bench_compose_file "$system" "$nodes")" up -d --wait --wait-timeout 300 >&2
}

# Derruba o servidor e apaga os volumes (dos dois compose: não sobra nada de uma rodada de outro tamanho).
bench_system_down() {
  local system="$1" f
  for f in compose.yml compose-cluster.yml; do
    [ -f "$BENCH_DIR/systems/$system/$f" ] || continue
    BENCH_DURABILITY="${BENCH_DURABILITY:-default}" docker compose -p "bench-$system" \
      -f "$BENCH_DIR_NATIVE/systems/$system/$f" down -v --remove-orphans >/dev/null 2>&1 || true
  done
}

# Os contêineres do servidor de um sistema: todos os serviços do projeto, o balanceador incluído.
bench_server_containers() {
  docker ps --filter "label=com.docker.compose.project=bench-$1" --format '{{.Names}}' | sort
}

# Uma amostra de CPU (em núcleos) e memória (em MB) de cada contêiner do servidor, em linhas
# "<epoch>;<contêiner>;<núcleos>;<mb>".
bench_stats_sample() {
  local containers now
  containers="$(bench_server_containers "$1")"
  [ -n "$containers" ] || return 0
  now=$(date +%s)
  # shellcheck disable=SC2086
  docker stats --no-stream --format '{{.Name}};{{.CPUPerc}};{{.MemUsage}}' $containers 2>/dev/null |
    awk -F';' -v now="$now" '{
      cpu = $2; sub(/%/, "", cpu)
      split($3, m, " "); v = m[1]; u = v; sub(/^[0-9.]+/, "", u); sub(/[A-Za-z]+$/, "", v)
      f = (u ~ /^G/) ? 1024 : (u ~ /^M/) ? 1 : (u ~ /^k/) ? 1 / 1024 : 1 / 1048576
      printf "%s;%s;%.2f;%.1f\n", now, $1, cpu / 100, v * f }'
}

# Amostra o servidor de tempos em tempos até ser morto: bench_stats_loop <sistema> <arquivo>.
bench_stats_loop() {
  while :; do bench_stats_sample "$1" >> "$2"; sleep 2; done
}

# Espera o OMB começar a fase medida (a linha "Starting benchmark traffic" do log do executor) e imprime a hora (epoch):
# as amostras do JSON do OMB contam dali. Lê o log do contêiner, não o arquivo: a saída do `docker run` chega ao arquivo
# em blocos, minutos depois. Desiste depois de <limite> segundos.
#   bench_wait_measure_start <sistema> <limite em s>
bench_wait_measure_start() {
  local system="$1" limit="$2" i
  for ((i = 0; i < limit; i++)); do
    if docker logs --tail 2000 "bench-runner-$system" 2>&1 | grep -q 'Starting benchmark traffic'; then date +%s; return 0; fi
    sleep 1
  done
  return 1
}

# Provoca as falhas de um workload. O arquivo <workload>.faults tem linhas "<segundos> <kill|start> <serviço>", contados
# do início da fase medida; cada falha feita vai para <eventos> como "<segundos desde o início>;<ação>;<serviço>".
#   bench_faults <sistema> <nós> <faults> <eventos> <limite em s>
bench_faults() {
  local system="$1" nodes="$2" faults="$3" events="$4" limit="$5" t0 at action service now
  t0=$(bench_wait_measure_start "$system" "$limit") || return 0
  echo "$t0" > "${events%.events}.t0"
  while read -r at action service; do
    case "$at" in ''|'#'*) continue ;; esac
    now=$(date +%s)
    [ $((t0 + at)) -gt "$now" ] && sleep $((t0 + at - now))
    docker compose -p "bench-$system" -f "$(bench_compose_file "$system" "$nodes")" "$action" "$service" >/dev/null 2>&1 || true
    echo "$(($(date +%s) - t0));$action;$service" >> "$events"
  done < "$faults"
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
    2>&1 | { head -c 100000000; cat > /dev/null; } > "$raw/$name.log"
  local status=${PIPESTATUS[0]}
  docker rm -f "$container" >/dev/null 2>&1 || true
  return $status
}
