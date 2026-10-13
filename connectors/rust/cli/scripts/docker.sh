#!/bin/sh
# Constrói a imagem Docker do taba-cli (doc/pt-BR/20-cli.md) com o buildx.
#
#   docker.sh                       imagem taba-cli para a plataforma desta máquina (fica no Docker local)
#   docker.sh --multi               linux/amd64 e linux/arm64 (precisa de --push ou de --output)
#   docker.sh --tag meu/taba-cli:1  outro nome para a imagem
#
# O contexto é a raiz do repositório; o Dockerfile copia só a pasta connectors/rust/ (o workspace do Cargo
# dos conectores).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)   # a raiz do repositório: o contexto do build

tag="taba-cli"
platform=""
extra=""
while [ $# -gt 0 ]; do
  case "$1" in
    --multi) platform="linux/amd64,linux/arm64" ;;
    --tag) shift; tag="${1:?--tag precisa de um nome}" ;;
    --push) extra="$extra --push" ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "docker.sh: opção desconhecida: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -n "$platform" ]; then
  case "$extra" in
    *--push*) ;;
    *) echo "docker.sh: --multi não cabe no Docker local; use --push (ou rode o buildx com --output)" >&2; exit 2 ;;
  esac
  # shellcheck disable=SC2086
  exec docker buildx build --platform "$platform" -f "$here/../Dockerfile" -t "$tag" $extra "$root"
fi
# shellcheck disable=SC2086
exec docker buildx build --load -f "$here/../Dockerfile" -t "$tag" $extra "$root"
