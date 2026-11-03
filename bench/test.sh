#!/usr/bin/env bash
# Teste do próprio bench/run.sh, de ponta a ponta com o Taba: o perfil smoke (1 min) contra um nó, verifica que o JSON do
# OMB e o relatório saíram. Precisa de Docker e do Node.js 22; os números não valem (roda abaixo do mínimo, se preciso).
#   bench/test.sh                      # TABA_IMAGE=<imagem> usa uma imagem pronta em vez de construir a do commit atual
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

bench_log "Plano de todos os perfis"
"$BENCH_DIR/run.sh" --dry-run --profile quick >/dev/null
"$BENCH_DIR/run.sh" --dry-run --profile standard --durability both >/dev/null

bench_log "Argumentos inválidos são recusados"
if "$BENCH_DIR/run.sh" --dry-run --systems inexistente >/dev/null 2>&1; then bench_die "aceitou um sistema inexistente"; fi
if "$BENCH_DIR/run.sh" --dry-run --durability nunca >/dev/null 2>&1; then bench_die "aceitou uma durabilidade inexistente"; fi

bench_log "Taba, perfil smoke"
"$BENCH_DIR/run.sh" --profile smoke --systems taba --allow-small --output "$out" ${TABA_IMAGE:+--taba-image "$TABA_IMAGE"}
[ -s "$out/raw/taba-default-simple-1kb-1.json" ] || bench_die "o JSON do OMB não saiu"
[ -s "$out/machine.json" ] || bench_die "machine.json não saiu"
[ -s "$out/results.md" ] || bench_die "results.md não saiu"
grep -q "não representativo" "$out/results.md" || bench_die "o relatório não traz a faixa de máquina local"
[ -s "$out/charts/simple-1kb-default-latency.svg" ] || bench_die "o gráfico não saiu"
[ ! -e "$out/failed.txt" ] || bench_die "uma célula falhou: $(cat "$out/failed.txt")"
if [ -n "$(docker ps -aq --filter name=bench-)" ]; then bench_die "sobrou contêiner do benchmark"; fi
echo "bench/test.sh: ok"
