#!/bin/sh
# The README of one package: connectors/pulumi/README.md with only the blocks of that language. The README has one block
# per language, between "<!-- lang:<language> -->" and "<!-- /lang -->" (nodejs, python, dotnet, go); GitHub and the
# mirror show all of them, and npm, PyPI and nuget.org each show the one of their language. POSIX sh and awk only: it
# runs in the containers of package.sh.
#   connectors/pulumi/scripts/readme.sh dotnet [README.md] > README.md
set -eu
lang="${1:?usage: readme.sh <nodejs|python|dotnet|go> [README.md]}"
case "$lang" in
  nodejs | python | dotnet | go) ;;
  *) echo "readme.sh: unknown language $lang (nodejs, python, dotnet, go)" >&2; exit 1 ;;
esac
src="${2:-$(dirname "$0")/../README.md}"
grep -q "<!-- lang:$lang -->" "$src" || { echo "readme.sh: $src has no block for $lang" >&2; exit 1; }
# A blank line right after a removed block is dropped too, so the blocks of the other languages leave no gap.
awk -v lang="$lang" '
  { line = $0; sub(/\r$/, "", line) }
  line ~ /^<!-- lang:[a-z]+ -->$/ { inblock = 1; keep = (line == "<!-- lang:" lang " -->"); next }
  line == "<!-- /lang -->" { inblock = 0; skipblank = !keep; next }
  skipblank && line == "" { skipblank = 0; next }
  { skipblank = 0 }
  inblock && !keep { next }
  { print }
' "$src"
