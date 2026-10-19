#!/usr/bin/env bash
# Captura telas reais da TUI do taba-cli (doc/<lang>/20b, README) num cluster do Compose sob carga.
#
#   doc/diagrams/scripts/capture-tui.sh            # todos os idiomas (en-US e pt-BR)
#   doc/diagrams/scripts/capture-tui.sh --lang pt-BR
#
# Precisa de Docker e Node 22. Sobe o Compose da raiz se não estiver de pé, baixa o taba-cli
# Linux do último Release, gera carga com tui-load.mjs e grava com o VHS (em contêiner, na rede
# do Compose) os PNG em doc/diagrams/img/<lang>/tui-*.png. Os gráficos só mostram o que foi
# medido desde que a TUI abriu e as taxas são médias móveis de 5 min, por isso cada gravação
# espera o histórico encher (WARMUP, 330 s).
# Rode da raiz do repositório.
set -euo pipefail

LANGS=(en-US pt-BR)
if [[ "${1:-}" == "--lang" ]]; then LANGS=("$2"); fi

VHS_IMAGE="${VHS_IMAGE:-ghcr.io/charmbracelet/vhs:v0.10.0}"
WARMUP="${WARMUP:-330}"
NETWORK="${NETWORK:-taba_default}"
ROOT="$(pwd)"
WORK="$(mktemp -d)"
# No Git Bash, gh e docker são programas Windows: precisam do caminho C:/... (MSYS_NO_PATHCONV abaixo).
if command -v cygpath > /dev/null; then WORK="$(cygpath -m "$WORK")"; fi
trap 'rm -rf "$WORK"; [[ -n "${LOAD_PID:-}" ]] && kill "$LOAD_PID" 2>/dev/null || true' EXIT

export MSYS_NO_PATHCONV=1

if ! docker compose ps --status running --format '{{.Name}}' | grep -q taba; then
  docker compose up -d --wait
fi

docker compose run --rm --entrypoint cat taba-cli /secrets/root-token > "$WORK/root-token" 2>/dev/null

VERSION="$(gh release view -R rafaelpontani/taba --json tagName -q .tagName)"
gh release download "$VERSION" -R rafaelpontani/taba -p "taba-cli-${VERSION#v}-x86_64-unknown-linux-musl" -O "$WORK/taba-cli"
chmod +x "$WORK/taba-cli"

cat > "$WORK/config.yml" <<'EOF'
default: production
profiles:
  production:
    url: http://haproxy:8080
    token-file: /vhs/root-token
EOF

node doc/diagrams/scripts/tui-load.mjs --token-file "$WORK/root-token" --seconds $((WARMUP + 120)) > "$WORK/load.log" 2>&1 &
LOAD_PID=$!

# Uma sessão da TUI por tela e por idioma: o histórico dos gráficos de um tópico só é medido
# enquanto a lista de tópicos está aberta, e o do cluster enquanto a aba Cluster está.
tape() { # tape <lang> <nome> <teclas antes da espera> <passos depois da espera>
  cat > "$WORK/$2-$1.tape" <<EOF
Output $2-$1.gif
Set Shell bash
Set FontSize 16
Set Width 1440
Set Height 800
Set Padding 16
Set Theme "Catppuccin Mocha"
Env TABA_CLI_CONFIG "/vhs/config.yml"
Hide
Type "clear && /vhs/taba-cli tui --lang $1 --history-interval 1s --no-mouse"
Enter
Sleep 3s
$3
Sleep ${WARMUP}s
Show
Sleep 1s
$4
EOF
}

for lang in "${LANGS[@]}"; do
  tape "$lang" cluster "" "Screenshot tui-cluster-$lang.png
Type \"i\"
Sleep 2s
Screenshot tui-node-$lang.png"
  # orders é o 6º tópico em ordem de nome (as dead-letter ficam escondidas).
  tape "$lang" topics "Type \"2\"
Sleep 2s
Down 5" "Screenshot tui-topics-$lang.png
Type \"g\"
Sleep 2s
Down
Enter
Sleep 2s
Screenshot tui-queue-chart-$lang.png"
done

# Todas as sessões gravam em paralelo, sob a mesma carga.
pids=()
for lang in "${LANGS[@]}"; do
  for name in cluster topics; do
    docker run --rm --network "$NETWORK" -v "$WORK:/vhs" -w /vhs "$VHS_IMAGE" "$name-$lang.tape" > "$WORK/vhs-$name-$lang.log" 2>&1 &
    pids+=($!)
  done
done
for pid in "${pids[@]}"; do wait "$pid"; done
cat "$WORK/load.log"

for lang in "${LANGS[@]}"; do
  for shot in cluster node topics queue-chart; do
    cp "$WORK/tui-$shot-$lang.png" "$ROOT/doc/diagrams/img/$lang/tui-$shot.png"
  done
  echo "doc/diagrams/img/$lang/tui-{cluster,node,topics,queue-chart}.png"
done
