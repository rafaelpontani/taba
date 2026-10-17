#!/usr/bin/env bash
# Roda os testes do chart.
#   ./run.sh              estáticos (00) + todos os casos de uso (precisam de kind/kubectl/helm)
#   ./run.sh static       só os estáticos (sem cluster)
#   ./run.sh 01 05 12     só esses casos de uso (o número do uc-NN)
# Código de saída: 0 tudo ok (ignorados não contam como falha) · 1 alguma falha.
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

if [ $# -eq 0 ]; then scripts=(00-static.sh uc-*.sh)
elif [ "$1" = static ]; then scripts=(00-static.sh)
else scripts=(); for n in "$@"; do scripts+=($(ls uc-"$n"-*.sh 2>/dev/null)); done
fi

declare -A result
failed=0
for s in "${scripts[@]}"; do
  printf '\n\033[1m== %s ==\033[0m\n' "$s"
  bash "$s"; code=$?
  case $code in
    0)  result[$s]=ok ;;
    77) result[$s]=ignorado ;;
    *)  result[$s]=FALHOU; failed=1 ;;
  esac
done

printf '\n\033[1mResumo\033[0m\n'
for s in "${scripts[@]}"; do printf '  %-40s %s\n' "$s" "${result[$s]}"; done
exit $failed
