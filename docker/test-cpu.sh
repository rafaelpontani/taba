#!/usr/bin/env bash
# Runs `cargo test` in a CPU-limited Linux container, to reproduce the tests that only fail on slow CI
# runners (ubuntu-latest has 4 vCPUs; ubuntu-24.04-arm has 4, slower per core).
#
#   docker/test-cpu.sh -- --test failures the_messages_of_the_excluded     # 2 CPUs, once
#   CPUS=1 REPEAT=10 docker/test-cpu.sh -- --test failures                 # 1 CPU, ten times
#   CPUS=0.5 MEMORY=4g docker/test-cpu.sh -- --workspace                   # half a CPU
#
# Variables (all optional):
#   CPUS      The container's CPU quota (`docker run --cpus`; accepts fractions). Default 2.
#   CPUSET    Pinned cores (`--cpuset-cpus`, e.g. 0-1). Empty: the scheduler chooses. With CPUS=2 and
#             CPUSET=0-1 the limit is real cores, with no bursting onto more cores.
#   MEMORY    Memory ceiling. Default 7g (the GitHub runner's).
#   REPEAT    How many times to repeat; stops at the first failure. Default 1.
#   IMAGE     Rust image. Default rust:1-bookworm (glibc, like the runner).
#   THREADS   `RUST_TEST_THREADS` (parallel tests). Empty: one per visible CPU, as in CI.
#
# What comes after `--` goes to `cargo test` (e.g. `--test failures test_name`). The volumes
# `taba-cargo` and `taba-target` keep the cache; `docker volume rm` deletes them.
set -euo pipefail

CPUS="${CPUS:-2}"
MEMORY="${MEMORY:-7g}"
REPEAT="${REPEAT:-1}"
IMAGE="${IMAGE:-rust:1-bookworm}"
[ "${1:-}" = "--" ] && shift

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Git Bash on Windows would convert `/work` into a Windows path.
export MSYS_NO_PATHCONV=1

args=(--rm --cpus "$CPUS" --memory "$MEMORY"
  -v "$root:/work" -v taba-cargo:/usr/local/cargo/registry -v taba-target:/target
  -e CARGO_TARGET_DIR=/target -e CARGO_TERM_COLOR=never -w /work)
[ -n "${CPUSET:-}" ] && args+=(--cpuset-cpus "$CPUSET")
[ -n "${THREADS:-}" ] && args+=(-e "RUST_TEST_THREADS=$THREADS")

# The container's `nproc` shows the machine's cores, not the quota: the test runtime sizes its
# threads from the cgroup, so the log below checks what it sees.
echo "taba: cpus=$CPUS cpuset=${CPUSET:-—} memory=$MEMORY repeat=$REPEAT image=$IMAGE" >&2
for i in $(seq 1 "$REPEAT"); do
  echo "taba: run $i of $REPEAT" >&2
  if ! docker run "${args[@]}" "$IMAGE" bash -c \
    'echo "taba: nproc=$(nproc)" >&2; cargo test --locked "$@"' _ "$@"; then
    echo "taba: failed on run $i" >&2
    exit 1
  fi
done
echo "taba: $REPEAT run(s) without failure" >&2
