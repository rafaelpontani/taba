#!/usr/bin/env bash
# Generates the diagrams from doc/diagramas/fonte/*.json, once per language:
#   1. English is the source language: fonte/*.json is rendered as it is. Every other language is
#      the same JSON with the display strings replaced by doc/diagramas/i18n/<lang>.json
#      (a flat table "English string": "translation"); a string missing from the table stays in English;
#   2. validates and renders with Archify (gates: validate, deliver, check, browser-check);
#   3. exports the SVG to doc/diagramas/img/<lang>/ (it is what the .md files embed, versioned);
#   4. the interactive HTML is only kept with --html, in doc/diagramas/html/<lang>/ (not versioned).
#
# Usage: doc/diagramas/scripts/gerar.sh [--html] [--lang <code>] [name ...]
#   no names: generates all. Name = file in fonte/ without .json (e.g. recepcao).
#   no --lang: generates every language (en and one per file in i18n/).
# Variables: ARCHIFY_BIN (path of archify.mjs), ARCHIFY_CHROME (Chrome/Edge executable).
set -euo pipefail

raiz="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$raiz"   # the JSON's meta.output is relative to the repository root

manter_html=0
nomes=()
idiomas=()
while [ $# -gt 0 ]; do
  case "$1" in
    --html) manter_html=1 ;;
    --lang) shift; idiomas+=("${1:?--lang needs a code}") ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) nomes+=("$1") ;;
  esac
  shift
done

archify="${ARCHIFY_BIN:-}"
if [ -z "$archify" ]; then
  for c in "$HOME/.claude/skills/archify/bin/archify.mjs" "./node_modules/archify/bin/archify.mjs"; do
    [ -f "$c" ] && archify="$c" && break
  done
fi
[ -n "$archify" ] && [ -f "$archify" ] || { echo "Archify not found. Set ARCHIFY_BIN." >&2; exit 2; }

if [ ${#idiomas[@]} -eq 0 ]; then
  idiomas=(en)
  for f in doc/diagramas/i18n/*.json; do [ -f "$f" ] && idiomas+=("$(basename "$f" .json)"); done
fi
if [ ${#nomes[@]} -eq 0 ]; then
  for f in doc/diagramas/fonte/*.json; do nomes+=("$(basename "$f" .json)"); done
fi

falhas=0
total=0
for lang in "${idiomas[@]}"; do
  tabela="doc/diagramas/i18n/$lang.json"
  if [ "$lang" != en ] && [ ! -f "$tabela" ]; then echo "[$lang] translation table not found: $tabela" >&2; falhas=$((falhas+1)); continue; fi
  mkdir -p "doc/diagramas/img/$lang" "doc/diagramas/html/$lang"
  for nome in "${nomes[@]}"; do
    fonte="doc/diagramas/fonte/$nome.json"
    json="doc/diagramas/html/$lang/$nome.json"
    html="doc/diagramas/html/$lang/$nome.html"
    svg="doc/diagramas/img/$lang/$nome.svg"
    [ -f "$fonte" ] || { echo "[$lang/$nome] source not found: $fonte" >&2; falhas=$((falhas+1)); continue; }
    total=$((total+1))

    node doc/diagramas/scripts/traduzir.mjs "$fonte" "$json" "$lang" "$tabela" || { falhas=$((falhas+1)); continue; }
    tipo="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).diagram_type)' "$json")"
    echo "[$lang/$nome] ($tipo)"
    if ! saida="$(node "$archify" finalize "$tipo" "$json" "$html" --quality showcase --json 2>&1)"; then
      echo "  FAILED in Archify. Diagnostics:" >&2
      echo "$saida" | node -e '
        let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
          try{const r=JSON.parse(s);(r.diagnostics||[]).forEach(d=>console.error("  - ["+d.code+"] "+d.message));}
          catch{console.error(s.slice(0,800));}
        })' >&2
      falhas=$((falhas+1)); continue
    fi
    node doc/diagramas/scripts/exportar-svg.mjs "$html" "$svg" || falhas=$((falhas+1))
  done
done

if [ "$manter_html" -eq 0 ]; then
  rm -rf doc/diagramas/html
else
  echo "Interactive HTML in doc/diagramas/html/<lang>/ (not versioned)."
fi

[ "$falhas" -eq 0 ] || { echo "$falhas diagram(s) failed." >&2; exit 1; }
echo "Done: $total diagram(s)."
