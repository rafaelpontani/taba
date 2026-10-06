#!/usr/bin/env bash
# Gera os diagramas a partir de doc/diagramas/fonte/*.json:
#   1. valida e renderiza com o Archify (gates: validate, deliver, check, browser-check);
#   2. exporta o SVG para doc/diagramas/img/ (é o que os .md embutem, versionado);
#   3. o HTML interativo só é mantido com --html, em doc/diagramas/html/ (não versionado).
#
# Uso: doc/diagramas/scripts/gerar.sh [--html] [nome ...]
#   sem nomes: gera todos. Nome = arquivo em fonte/ sem .json (ex.: recepcao).
# Variáveis: ARCHIFY_BIN (caminho do archify.mjs), ARCHIFY_CHROME (executável do Chrome/Edge).
set -euo pipefail

raiz="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$raiz"   # meta.output dos JSON é relativo à raiz do repositório

manter_html=0
nomes=()
for arg in "$@"; do
  case "$arg" in
    --html) manter_html=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) nomes+=("$arg") ;;
  esac
done

archify="${ARCHIFY_BIN:-}"
if [ -z "$archify" ]; then
  for c in "$HOME/.claude/skills/archify/bin/archify.mjs" "./node_modules/archify/bin/archify.mjs"; do
    [ -f "$c" ] && archify="$c" && break
  done
fi
[ -n "$archify" ] && [ -f "$archify" ] || { echo "Archify não encontrado. Defina ARCHIFY_BIN." >&2; exit 2; }

if [ ${#nomes[@]} -eq 0 ]; then
  for f in doc/diagramas/fonte/*.json; do nomes+=("$(basename "$f" .json)"); done
fi

mkdir -p doc/diagramas/img doc/diagramas/html
falhas=0
for nome in "${nomes[@]}"; do
  json="doc/diagramas/fonte/$nome.json"
  html="doc/diagramas/html/$nome.html"
  svg="doc/diagramas/img/$nome.svg"
  [ -f "$json" ] || { echo "[$nome] fonte não encontrada: $json" >&2; falhas=$((falhas+1)); continue; }

  tipo="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).diagram_type)' "$json")"
  echo "[$nome] ($tipo)"
  if ! saida="$(node "$archify" finalize "$tipo" "$json" "$html" --quality showcase --json 2>&1)"; then
    echo "  FALHOU no Archify. Diagnósticos:" >&2
    echo "$saida" | node -e '
      let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
        try{const r=JSON.parse(s);(r.diagnostics||[]).forEach(d=>console.error("  - ["+d.code+"] "+d.message));}
        catch{console.error(s.slice(0,800));}
      })' >&2
    falhas=$((falhas+1)); continue
  fi
  node doc/diagramas/scripts/exportar-svg.mjs "$html" "$svg" || falhas=$((falhas+1))
done

if [ "$manter_html" -eq 0 ]; then
  rm -rf doc/diagramas/html
else
  echo "HTML interativo em doc/diagramas/html/ (não versionado)."
fi

[ "$falhas" -eq 0 ] || { echo "$falhas diagrama(s) com falha." >&2; exit 1; }
echo "Concluído: ${#nomes[@]} diagrama(s)."
