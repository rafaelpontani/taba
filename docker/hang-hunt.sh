#!/usr/bin/env bash
# Hunts a test that hangs (or fails) only sometimes, which a normal run never shows (doc 25, "Investigating a test that
# hangs or fails only sometimes"; the case that taught it is in doc 36, "A channel accept that locked the peers twice").
#
# In a Linux container (the Windows binary has no gdb) it builds one integration test binary, starts it in several
# processes at once, round after round, with the detailed node logs on (`TABA_TEST_LOG`), and watches each round:
#   - a process that passes loses its log (only the problems are kept);
#   - a process that fails keeps its log;
#   - a process still running after `HANG_AFTER` seconds is a hang: the stack of every thread is taken with gdb
#     (`thread apply all bt`) before it is killed, and the log is kept.
# It stops at the first round with a problem, or, with `KEEP_GOING=1`, goes on through all the rounds keeping every bad log
# (to count and classify them: a validation of a fix is `KEEP_GOING=1` and a look at the messages of the logs kept).
#
# Usage:
#   docker/hang-hunt.sh <test-file> [test name filter...]
#   docker/hang-hunt.sh nodes_api a_prune_that_fails a_node_that_does_not_answer_makes
#   ROUNDS=200 PAR=6 docker/hang-hunt.sh channel
#
# Environment (all optional):
#   ROUNDS=60          rounds before giving up
#   PAR=8              processes of the binary at once (the load that exposes the race)
#   HANG_AFTER=70      seconds before a process counts as hung
#   KEEP_GOING=0       1: do not stop at the first problem
#   TEST_THREADS=15    `--test-threads` of each process
#   TABA_TEST_LOG=...  log filter (RUST_LOG syntax); default `taba_node=trace` without the HTTP libraries
#   OUT=target/hang-hunt   where the logs and stacks go (relative to the repository root)
#   PACKAGE=taba-node  the crate that owns the test file
#   IMAGE=rust:1-bookworm  the container image
#
# Output: $OUT/run_<round>_<n>.log (the log of a bad run), $OUT/stack_<round>_<n>.txt (the threads of a hung one) and
# $OUT/progress.txt. Exit code 0: no problem in all the rounds; 1: a problem was captured.
#
# Needs Docker. On Windows the checkout has CRLF line endings: the container strips them from its own copy of this script. The repository is mounted read-only; the build goes to the volumes `taba-hunt-target` and
# `taba-hunt-cargo` (kept between runs so the next one does not rebuild everything; `docker volume rm` them to
# clean up).

set -euo pipefail

ROUNDS=${ROUNDS:-60}
PAR=${PAR:-8}
HANG_AFTER=${HANG_AFTER:-70}
KEEP_GOING=${KEEP_GOING:-0}
TEST_THREADS=${TEST_THREADS:-15}
PACKAGE=${PACKAGE:-taba-node}
IMAGE=${IMAGE:-rust:1-bookworm}
TABA_TEST_LOG=${TABA_TEST_LOG:-taba_node=trace,hyper=warn,hyper_util=warn,reqwest=warn}

# ---- Inside the container ----
if [[ "${1:-}" == "--inner" ]]; then
  shift
  FILE=$1
  shift
  say() { echo "$*" | tee -a /out/progress.txt; }
  if ! command -v gdb >/dev/null; then
    apt-get update -qq && apt-get install -y -qq gdb >/dev/null 2>&1
  fi
  cd /src
  export CARGO_TARGET_DIR=/target TABA_TEST_LOG
  say "build $PACKAGE --test $FILE: $(date +%T)"
  build=$(cargo test -p "$PACKAGE" --test "$FILE" --no-run --locked 2>&1) || { echo "$build" | tail -20; exit 2; }
  BIN=$(echo "$build" | sed -n "s|.*Executable tests/$FILE.rs (\(.*\))|\1|p" | tail -1)
  [[ -x "$BIN" ]] || { echo "binary of $FILE not found"; exit 2; }
  say "binary: $BIN | rounds=$ROUNDS par=$PAR hang-after=${HANG_AFTER}s log=$TABA_TEST_LOG filters: $*"
  problems=0
  for round in $(seq 1 "$ROUNDS"); do
    pids=()
    for n in $(seq 1 "$PAR"); do
      "$BIN" --nocapture --test-threads="$TEST_THREADS" "$@" >"/out/run_${round}_$n.log" 2>&1 &
      pids+=($!)
    done
    end=$((SECONDS + HANG_AFTER))
    while ((SECONDS < end)); do
      alive=0
      for p in "${pids[@]}"; do kill -0 "$p" 2>/dev/null && alive=1; done
      ((alive == 0)) && break
      sleep 1
    done
    problem=0
    n=0
    for p in "${pids[@]}"; do
      n=$((n + 1))
      if kill -0 "$p" 2>/dev/null; then
        problem=1
        say "round $round run $n pid $p HUNG after ${HANG_AFTER}s: $(grep -c . "/out/run_${round}_$n.log") log lines, last test line: $(grep -E '^test ' "/out/run_${round}_$n.log" | tail -1 | cut -c1-120)"
        gdb -p "$p" -batch -ex "set pagination off" -ex "info threads" -ex "thread apply all bt 40" \
          >"/out/stack_${round}_$n.txt" 2>&1 || true
        kill -9 "$p" 2>/dev/null || true
      else
        rc=0
        wait "$p" || rc=$?
        if ((rc != 0)); then
          problem=1
          say "round $round run $n FAILED (exit $rc): /out/run_${round}_$n.log"
        else
          rm -f "/out/run_${round}_$n.log"
        fi
      fi
    done
    say "round $round done (problem=$problem)"
    if ((problem == 1)); then
      problems=$((problems + 1))
      if [[ "$KEEP_GOING" != 1 ]]; then
        say "stopped at the first problem: logs and stacks are in the output folder"
        exit 1
      fi
    fi
  done
  if ((problems > 0)); then
    say "$problems rounds of $ROUNDS had a problem: the logs and stacks are in the output folder"
    exit 1
  fi
  say "no problem in $ROUNDS rounds of $PAR processes"
  exit 0
fi

# ---- On the host ----
[[ $# -ge 1 ]] || { sed -n '2,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 64; }
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT=${OUT:-target/hang-hunt}
mkdir -p "$ROOT/$OUT"
rm -f "$ROOT/$OUT"/run_* "$ROOT/$OUT"/stack_* "$ROOT/$OUT/progress.txt"
# Git Bash on Windows needs the Windows form of the paths for the mounts, and no path conversion of the rest.
win() { (cd "$1" && (pwd -W 2>/dev/null || pwd)); }
export MSYS_NO_PATHCONV=1
status=0
docker run --rm --cap-add=SYS_PTRACE \
  -e ROUNDS -e PAR -e HANG_AFTER -e KEEP_GOING -e TEST_THREADS -e PACKAGE -e TABA_TEST_LOG \
  -v "$(win "$ROOT"):/src:ro" \
  -v taba-hunt-target:/target \
  -v taba-hunt-cargo:/usr/local/cargo/registry \
  -v "$(win "$ROOT/$OUT"):/out" \
  -w /src "$IMAGE" bash -c 'tr -d "\r" </src/docker/hang-hunt.sh >/tmp/hang-hunt.sh && exec bash /tmp/hang-hunt.sh --inner "$@"' _ "$@" || status=$?
echo "output: $ROOT/$OUT"
exit $status
