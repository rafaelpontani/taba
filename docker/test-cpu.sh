#!/usr/bin/env bash
# Roda o `cargo test` num contêiner Linux com CPU limitada, para reproduzir os testes que só falham
# nos runners lentos do CI (ubuntu-latest tem 4 vCPU; ubuntu-24.04-arm, 4, mais lentos por núcleo).
#
#   docker/test-cpu.sh -- --test failures the_messages_of_the_excluded     # 2 CPUs, uma vez
#   CPUS=1 REPEAT=10 docker/test-cpu.sh -- --test failures                 # 1 CPU, dez vezes
#   CPUS=0.5 MEMORY=4g docker/test-cpu.sh -- --workspace                   # meia CPU
#
# Variáveis (todas opcionais):
#   CPUS      Cota de CPU do contêiner (`docker run --cpus`; aceita fração). Padrão 2.
#   CPUSET    Núcleos fixos (`--cpuset-cpus`, ex.: 0-1). Vazio: o escalonador escolhe. Com CPUS=2 e
#             CPUSET=0-1 a limitação é de núcleos de verdade, sem rajada em mais núcleos.
#   MEMORY    Teto de memória. Padrão 7g (o do runner do GitHub).
#   REPEAT    Quantas vezes repetir; para na primeira falha. Padrão 1.
#   IMAGE     Imagem do Rust. Padrão rust:1-bookworm (glibc, como o runner).
#   THREADS   `RUST_TEST_THREADS` (testes em paralelo). Vazio: um por CPU visível, como no CI.
#
# O que vem depois de `--` vai para `cargo test` (ex.: `--test failures nome_do_teste`). Os
# volumes `taba-cargo` e `taba-target` guardam o cache; `docker volume rm` os apaga.
set -euo pipefail

CPUS="${CPUS:-2}"
MEMORY="${MEMORY:-7g}"
REPEAT="${REPEAT:-1}"
IMAGE="${IMAGE:-rust:1-bookworm}"
[ "${1:-}" = "--" ] && shift

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# O Git Bash do Windows converteria `/work` em caminho do Windows.
export MSYS_NO_PATHCONV=1

args=(--rm --cpus "$CPUS" --memory "$MEMORY"
  -v "$root:/work" -v taba-cargo:/usr/local/cargo/registry -v taba-target:/target
  -e CARGO_TARGET_DIR=/target -e CARGO_TERM_COLOR=never -w /work)
[ -n "${CPUSET:-}" ] && args+=(--cpuset-cpus "$CPUSET")
[ -n "${THREADS:-}" ] && args+=(-e "RUST_TEST_THREADS=$THREADS")

# O `nproc` do contêiner mostra os núcleos da máquina, não a cota: o runtime de testes dimensiona
# as threads pelo cgroup, então o log abaixo confere o que ele enxerga.
echo "taba: cpus=$CPUS cpuset=${CPUSET:-—} memory=$MEMORY repeat=$REPEAT image=$IMAGE" >&2
for i in $(seq 1 "$REPEAT"); do
  echo "taba: execução $i de $REPEAT" >&2
  if ! docker run "${args[@]}" "$IMAGE" bash -c \
    'echo "taba: nproc=$(nproc)" >&2; cargo test --locked "$@"' _ "$@"; then
    echo "taba: falhou na execução $i" >&2
    exit 1
  fi
done
echo "taba: $REPEAT execução(ões) sem falha" >&2
