#!/usr/bin/env bash
# Shared pieces of the scripts of connectors/omb. For `source`; it does nothing by itself.

OMB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OMB_REPO_ROOT="$(cd "$OMB_DIR/../.." && pwd)"
OMB_WORK="$OMB_DIR/.work"
OMB_MAVEN_IMAGE="${OMB_MAVEN_IMAGE:-maven:3.9-eclipse-temurin-11}"

# Git Bash on Windows converts paths such as /repo.
export MSYS_NO_PATHCONV=1

omb_hostpath() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }

# The OpenMessaging Benchmark at the commit of OMB_VERSION, in .work/omb.
omb_checkout() {
  local commit
  commit="$(tr -d '[:space:]' < "$OMB_DIR/OMB_VERSION")"
  mkdir -p "$OMB_WORK"
  if [ ! -d "$OMB_WORK/omb/.git" ]; then
    git clone -q https://github.com/openmessaging/benchmark.git "$OMB_WORK/omb"
  fi
  [ -d "$OMB_WORK/omb/.git" ] || { echo "the OMB checkout failed" >&2; return 1; }
  (cd "$OMB_WORK/omb" && { git fetch -q origin "$commit" 2>/dev/null || true; git checkout -q "$commit"; })
  [ "$(cd "$OMB_WORK/omb" && git rev-parse HEAD)" = "$commit" ] || { echo "the OMB checkout is not at $commit" >&2; return 1; }
}

# Runs a shell command in the Maven/Java 11 container with the repository at /repo and the shared Maven cache.
#   omb_maven <command> [docker options...]
omb_maven() {
  local command="$1"; shift
  docker run --rm -v "$(omb_hostpath "$OMB_REPO_ROOT"):/repo" -v taba-omb-m2:/root/.m2 -w /repo "$@" \
    "$OMB_MAVEN_IMAGE" sh -c "$command"
}
