#!/usr/bin/env bash
# Generates the diagrams from doc/diagrams/source/*.json, once per language:
#   1. English (en-US) is the source language: source/*.json is rendered as it is. Every other language is
#      the same JSON with the display strings replaced by doc/diagrams/i18n/<lang>.json
#      (a flat table "English string": "translation"); a string missing from the table stays in English;
#   2. validates and renders with Archify (gates: validate, deliver, check, browser-check);
#   3. exports the SVG to doc/diagrams/img/<lang>/ (it is what the .md files embed, versioned);
#   4. the interactive HTML is only kept with --html, in doc/diagrams/html/<lang>/ (not versioned).
#
# Usage: doc/diagrams/scripts/generate.sh [--html] [--lang <code>] [name ...]
#   no names: generates all. Name = file in source/ without .json (e.g. write-reception).
#   no --lang: generates every language (en-US and one per file in i18n/).
# Variables: ARCHIFY_BIN (path of archify.mjs), ARCHIFY_CHROME (Chrome/Edge executable).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$root"   # the JSON's meta.output is relative to the repository root

keep_html=0
names=()
languages=()
while [ $# -gt 0 ]; do
  case "$1" in
    --html) keep_html=1 ;;
    --lang) shift; languages+=("${1:?--lang needs a code}") ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) names+=("$1") ;;
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

if [ ${#languages[@]} -eq 0 ]; then
  languages=(en-US)
  for f in doc/diagrams/i18n/*.json; do [ -f "$f" ] && languages+=("$(basename "$f" .json)"); done
fi
if [ ${#names[@]} -eq 0 ]; then
  for f in doc/diagrams/source/*.json; do names+=("$(basename "$f" .json)"); done
fi

failures=0
total=0
for lang in "${languages[@]}"; do
  table="doc/diagrams/i18n/$lang.json"
  if [ "$lang" != en-US ] && [ ! -f "$table" ]; then echo "[$lang] translation table not found: $table" >&2; failures=$((failures+1)); continue; fi
  mkdir -p "doc/diagrams/img/$lang" "doc/diagrams/html/$lang"
  for name in "${names[@]}"; do
    source_file="doc/diagrams/source/$name.json"
    json="doc/diagrams/html/$lang/$name.json"
    html="doc/diagrams/html/$lang/$name.html"
    svg="doc/diagrams/img/$lang/$name.svg"
    [ -f "$source_file" ] || { echo "[$lang/$name] source not found: $source_file" >&2; failures=$((failures+1)); continue; }
    total=$((total+1))

    node doc/diagrams/scripts/translate.mjs "$source_file" "$json" "$lang" "$table" || { failures=$((failures+1)); continue; }
    type="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).diagram_type)' "$json")"
    echo "[$lang/$name] ($type)"
    if ! output="$(node "$archify" finalize "$type" "$json" "$html" --quality showcase --json 2>&1)"; then
      echo "  FAILED in Archify. Diagnostics:" >&2
      echo "$output" | node -e '
        let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{
          try{const r=JSON.parse(s);(r.diagnostics||[]).forEach(d=>console.error("  - ["+d.code+"] "+d.message));}
          catch{console.error(s.slice(0,800));}
        })' >&2
      failures=$((failures+1)); continue
    fi
    node doc/diagrams/scripts/export-svg.mjs "$html" "$svg" || failures=$((failures+1))
  done
done

if [ "$keep_html" -eq 0 ]; then
  rm -rf doc/diagrams/html
else
  echo "Interactive HTML in doc/diagrams/html/<lang>/ (not versioned)."
fi

[ "$failures" -eq 0 ] || { echo "$failures diagram(s) failed." >&2; exit 1; }
echo "Done: $total diagram(s)."
