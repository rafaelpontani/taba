#!/usr/bin/env bash
# Controle do cluster Taba do Compose da raiz, para reaproveitar nos testes integrados.
#
# Dois jeitos de usar:
#
#   1. Como biblioteca, num script de teste (de um conector ou o geral):
#
#        source "$(dirname "${BASH_SOURCE[0]}")/../../docker/cluster.sh"   # ajuste o caminho
#        taba_require_docker
#        trap taba_teardown EXIT        # derruba o cluster (ou mantém, com TABA_KEEP=1)
#        taba_up                        # sobe e espera os três nós saudáveis
#        read -r ID TOKEN < <(taba_create_token meu-teste --permission publish,consume --prefix meu-)
#        taba_ensure_topic meu-fila --type queue
#        taba_kill taba-3               # queda brusca
#        taba_wait_members 2 120        # o nó foi excluído por votação
#        taba_start taba-3
#        taba_wait_stable 240           # volta a 3 membros, sem condições além de node-frozen
#
#   2. Como comando, para montar o cenário à mão ou de outra linguagem:
#
#        docker/cluster.sh up [--no-build]
#        docker/cluster.sh down
#        docker/cluster.sh kill|stop|start|restart <nó>     # taba-1, taba-2 ou taba-3
#        docker/cluster.sh wait-healthy|wait-stable [segundos]
#        docker/cluster.sh wait-members <n> [segundos]
#        docker/cluster.sh status
#        docker/cluster.sh token <nome> [opções do 'taba-cli token create']   # imprime "id segredo"
#        docker/cluster.sh topic <nome> [opções do 'taba-cli topic create']   # cria se não existir
#        docker/cluster.sh cli <argumentos do taba-cli>
#
# Variáveis: TABA_NODES (padrão "taba-1 taba-2 taba-3"), TABA_KEEP=1 (o taba_teardown mantém o
# cluster), TABA_BUILD=0 (o taba_up reaproveita as imagens), TABA_CLI_VERBOSE=1 (mostra o stderr do
# docker compose run).
#
# Precisa de Docker (com o plugin compose). A porta 8080 do host (HAProxy) precisa estar livre.

TABA_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TABA_NODES="${TABA_NODES:-taba-1 taba-2 taba-3}"
TABA_KEEP="${TABA_KEEP:-0}"
TABA_BUILD="${TABA_BUILD:-1}"

# O Git Bash do Windows converte caminhos como /secrets/root-token.
export MSYS_NO_PATHCONV=1

taba_step() { printf '\n== %s\n' "$*"; }
taba_ok() { printf '  ok: %s\n' "$*"; }
taba_die() { echo "FALHOU: $*" >&2; exit 1; }

# Roda o docker compose da raiz, de qualquer diretório.
taba_compose() { (cd "$TABA_ROOT" && docker compose "$@"); }

taba_require_docker() {
  command -v docker >/dev/null || taba_die "docker não encontrado"
  docker compose version >/dev/null 2>&1 || taba_die "o plugin 'docker compose' não está instalado"
  docker info >/dev/null 2>&1 || taba_die "o Docker não está rodando"
}

# O taba-cli do Compose, já apontado para o cluster e com o token raiz do volume.
taba_cli() {
  if [ "${TABA_CLI_VERBOSE:-0}" = "1" ]; then
    taba_compose run --rm -T taba-cli "$@" | tr -d '\r'
  else
    taba_compose run --rm -T taba-cli "$@" 2>/dev/null | tr -d '\r'
  fi
}

# --- ciclo de vida ---------------------------------------------------------------------------

# Sobe o cluster do zero (apaga o que houver) e espera os nós ficarem saudáveis.
taba_up() {
  taba_step "Subindo o cluster do zero"
  taba_compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
  if [ "$TABA_BUILD" -eq 1 ]; then
    taba_compose --profile tools build
  fi
  taba_compose up -d
  taba_wait_healthy "${1:-240}"
}

taba_down() {
  taba_compose --profile tools down -v --remove-orphans >/dev/null 2>&1 || true
}

# Para usar em `trap taba_teardown EXIT`: mostra os logs se falhou e derruba (ou mantém) o cluster.
taba_teardown() {
  local status="${1:-$?}"   # o código de saída, se o chamador já o tiver perdido de vista
  if [ "$status" -ne 0 ]; then
    echo >&2
    echo "Falhou (código $status). Últimas linhas dos nós:" >&2
    # shellcheck disable=SC2086
    taba_compose logs --tail 25 $TABA_NODES 2>/dev/null >&2 || true
  fi
  if [ "$TABA_KEEP" -eq 1 ]; then
    taba_step "Cluster mantido (docker/cluster.sh down, ou docker compose down -v, para apagar)"
  else
    taba_step "Derrubando o cluster e apagando os dados"
    taba_down
  fi
  return "$status"
}

# --- falhas ----------------------------------------------------------------------------------

taba_kill() { taba_compose kill "$1" >/dev/null 2>&1; }       # queda brusca
taba_stop() { taba_compose stop "$1" >/dev/null 2>&1; }       # saída ordenada (drain)
taba_start() { taba_compose start "$1" >/dev/null 2>&1; }
taba_restart() { taba_stop "$1"; taba_start "$1"; }

# --- estado ----------------------------------------------------------------------------------

taba_healthy_nodes() {
  local n=0 id state node
  for node in $TABA_NODES; do
    id="$(taba_compose ps -q "$node" 2>/dev/null || true)"
    [ -n "$id" ] || continue
    state="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$id" 2>/dev/null || echo none)"
    [ "$state" = "healthy" ] && n=$((n + 1))
  done
  echo "$n"
}

taba_node_count() { set -- $TABA_NODES; echo "$#"; }

# Quantos membros ativos o cluster enxerga (pelo taba-1; se ele estiver fora, vazio).
taba_active_members() {
  taba_cli health cluster -o json | grep -o '"active": *[0-9]*' | head -n 1 | grep -o '[0-9]*$' || true
}

# As condições do cluster, menos o aviso `node-frozen`: o ID antigo de um nó que caiu fica congelado
# por cluster.freezing.duration (5 min); o nó reiniciado tem ID novo e não é afetado.
taba_other_conditions() {
  taba_cli health cluster -o json | grep -o '"condition": *"[^"]*"' | grep -v node-frozen || true
}

taba_status() { taba_cli health cluster -o table; }

# --- esperas ---------------------------------------------------------------------------------

# Espera uma condição (um comando que devolve 0), com limite de tempo em segundos.
taba_wait_for() {
  local what="$1" limit="$2"; shift 2
  local deadline=$((SECONDS + limit))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || taba_die "$what: não aconteceu em ${limit} s"
    sleep 3
  done
  taba_ok "$what (${SECONDS}s no total)"
}

taba_all_healthy() { [ "$(taba_healthy_nodes)" -eq "$(taba_node_count)" ]; }
taba_wait_healthy() { taba_wait_for "todos os nós ficam saudáveis" "${1:-240}" taba_all_healthy; }

_taba_has_members() { [ "$(taba_active_members)" = "$1" ]; }
taba_wait_members() { taba_wait_for "o cluster tem $1 membros ativos" "${2:-120}" _taba_has_members "$1"; }

_taba_stable() {
  [ "$(taba_active_members)" = "$(taba_node_count)" ] && [ -z "$(taba_other_conditions)" ] && taba_all_healthy
}
taba_wait_stable() {
  taba_wait_for "todos os membros de volta, sem condições (só node-frozen é aceito)" "${1:-240}" _taba_stable
}

# --- dados -----------------------------------------------------------------------------------

# Cria um token e imprime "id segredo" (o segredo aparece só na resposta da criação).
#   read -r ID TOKEN < <(taba_create_token meu-teste --permission publish,consume --prefix meu-)
taba_create_token() {
  local name="$1"; shift
  local out id secret
  out="$(taba_cli token create --name "$name" "$@" -o json)"
  id="$(printf '%s' "$out" | sed -n 's/.*"id": *"\([^"]*\)".*/\1/p' | head -n 1)"
  secret="$(printf '%s' "$out" | sed -n 's/.*"token": *"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$id" ] && [ -n "$secret" ] || taba_die "não consegui ler o token criado: $out"
  echo "$id $secret"
}

taba_revoke_token() { taba_cli token revoke "$1" --yes >/dev/null 2>&1 || true; }

# Cria o tópico se ainda não existir.
taba_ensure_topic() {
  local name="$1"; shift
  if taba_cli topic get "$name" >/dev/null 2>&1; then
    echo "  $name já existe"
  else
    taba_cli topic create "$name" "$@" >/dev/null
    echo "  $name criado"
  fi
}

# --- como comando ----------------------------------------------------------------------------

_taba_main() {
  local cmd="${1:-}"; shift || true
  taba_require_docker
  cd "$TABA_ROOT"
  case "$cmd" in
    up) [ "${1:-}" = "--no-build" ] && TABA_BUILD=0; taba_up ;;
    down) taba_down ;;
    kill|stop|start|restart) [ -n "${1:-}" ] || taba_die "uso: cluster.sh $cmd <nó>"; "taba_$cmd" "$1" ;;
    wait-healthy) taba_wait_healthy "${1:-240}" ;;
    wait-stable) taba_wait_stable "${1:-240}" ;;
    wait-members) [ -n "${1:-}" ] || taba_die "uso: cluster.sh wait-members <n> [segundos]"; taba_wait_members "$1" "${2:-120}" ;;
    status) taba_status ;;
    token) taba_create_token "$@" ;;
    topic) taba_ensure_topic "$@" ;;
    cli) TABA_CLI_VERBOSE=1 taba_cli "$@" ;;
    ""|-h|--help) sed -n '2,35p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' ;;
    *) echo "comando desconhecido: $cmd (veja --help)" >&2; return 2 ;;
  esac
}

# Executado direto (não com `source`): vira o comando.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  _taba_main "$@"
fi
