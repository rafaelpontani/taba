#!/usr/bin/env bash
# Builds the installable file of each connector that is not in a registry yet (PRB-079), to attach to the Release:
#
#   .github/scripts/package-connectors.sh <output-folder>
#
#   Node.js  taba-client-<v>.tgz                  npm install ./taba-client-<v>.tgz
#   Python   taba_client-<v>-py3-none-any.whl     pip install taba_client-<v>-py3-none-any.whl
#   Ruby     taba-client-<v>.gem                  gem install ./taba-client-<v>.gem
#   Java     taba-client-<v>.jar                  on the classpath (no dependencies)
#   PHP      taba-client-php-<v>.zip              Composer "package" repository, or unzip and require src/autoload.php
#
# The version of each file is the one in the connector's own manifest (the connectors version on their own, D70.2);
# nothing is rewritten. Go has no file: its module path is the repository (connectors/go/README.md). Each package is
# built in the container of a current runtime, from a copy of the folder, so the working tree is left untouched.
# Used by release.yml (job connector-packages) and by ci.yml (so a broken package is found on the pull request).

set -euo pipefail

out="${1:?usage: package-connectors.sh <output-folder>}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

# Git Bash on Windows rewrites container paths and wants Windows paths for -v.
export MSYS_NO_PATHCONV=1
host() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }
src_root="$(host "$root")"
out_host="$(host "$out")"

# build <image> <connector folder> <command run in a copy of the folder at /build; files to ship go to /out>
build() {
  local image="$1" folder="$2" script="$3"
  echo "==> $folder ($image)"
  docker run --rm \
    -v "$src_root/connectors/$folder:/src:ro" -v "$out_host:/out" \
    -e HOME=/tmp -e npm_config_cache=/tmp/npm -e MAVEN_CONFIG=/tmp/m2 \
    "$image" sh -ec "mkdir /build && cp -R /src/. /build && cd /build && $script"
}

build node:20 node 'npm pack --silent --pack-destination /out'

build python:3.12-slim python 'pip wheel --quiet --no-deps --wheel-dir /out .'

build ruby:3.3 ruby 'gem build --quiet taba-client.gemspec && mv taba-client-*.gem /out/'

build maven:3.9-eclipse-temurin-11 java \
  'mvn -q -B -Dmaven.test.skip=true -Dmaven.repo.local=/tmp/m2 package && cp target/taba-client-*.jar /out/ && rm -f /out/*-sources.jar /out/*-javadoc.jar'

# PHP: composer.json, README and src/ at the root of the zip, which is what a Composer "package" repository expects.
version="$(sed -n "s/.*const VERSION = '\([^']*\)'.*/\1/p" "$root/connectors/php/src/Client.php" | head -n 1)"
[ -n "$version" ] || { echo "could not read the PHP connector version" >&2; exit 1; }
build python:3.12-slim php \
  "rm -rf tests contract && python -m zipfile -c /out/taba-client-php-$version.zip composer.json README.md src"

echo "==> files in $out"
ls -l "$out"
