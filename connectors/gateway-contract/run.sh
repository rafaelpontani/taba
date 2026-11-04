#!/usr/bin/env bash
# The gateway contract tests (connectors/gateway-contract/README.md): the native AWS SDK of each language, in every version
# of the suite, against one Taba node with the SQS and SNS gateway on, or against the emulators.
#
#   connectors/gateway-contract/run.sh                      # the normal run: every line must pass on Taba
#   connectors/gateway-contract/run.sh --expect-fail        # the red run: every line must fail because nothing answers
#   connectors/gateway-contract/run.sh --target emulator    # scenarios/aws on ElasticMQ and moto: every line must pass
#   connectors/gateway-contract/run.sh python node          # only these languages (csharp java rust go python node)
#   connectors/gateway-contract/run.sh --variant old        # only the variants with this name
#   connectors/gateway-contract/run.sh --jobs 4 --keep      # parallel drivers; leave the containers running at the end
#   TABA_IMAGE=rafaelpontani/taba:latest connectors/gateway-contract/run.sh
#
# Needs Docker. The rules that make the red run and the emulator run mean something are in the README.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

TARGET=taba
EXPECT_FAIL=0
JOBS=2
LANGS=()
ONLY_VARIANT=""
ALL_LANGS=(csharp java rust go python node)
while [ $# -gt 0 ]; do
  case "$1" in
    --expect-fail) EXPECT_FAIL=1 ;;
    --target) shift; TARGET="${1:-}" ;;
    --variant) shift; ONLY_VARIANT="${1:-}" ;;
    --jobs) shift; JOBS="${1:-2}" ;;
    --keep) export GW_KEEP=1 ;;
    -h|--help) sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    csharp|java|rust|go|python|node) LANGS+=("$1") ;;
    *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done
[ "$TARGET" = taba ] || [ "$TARGET" = emulator ] || gw_die "--target is taba or emulator"
[ "$TARGET" = taba ] || [ "$EXPECT_FAIL" -eq 0 ] || gw_die "--expect-fail only makes sense against Taba"
FULL_SET=0
[ ${#LANGS[@]} -gt 0 ] || { LANGS=("${ALL_LANGS[@]}"); FULL_SET=1; }
[ -z "$ONLY_VARIANT" ] || FULL_SET=0

WANT=pass
[ "$EXPECT_FAIL" -eq 0 ] || WANT=connect

gw_require_docker
gw_network_up
trap 'gw_down $?' EXIT

if [ "$TARGET" = taba ]; then gw_target_taba_up; else gw_target_emulator_up; fi
OUT="$GW_DIR/.out/$GW_RUN_ID"
mkdir -p "$OUT"
echo "logs: $OUT"

short() { case "$1" in csharp) echo cs ;; java) echo jv ;; rust) echo rs ;; go) echo go ;; python) echo py ;; node) echo nd ;; esac; }

# The scenario files of a driver run, in order. On Taba: everything in one run (one prefix, one process). On the emulators
# SNS needs moto for SQS as well, so it is a run of its own.
TABA_FILES=(aws/sqs-standard.scn aws/sqs-fifo.scn aws/sns.scn taba/auth.scn taba/limits-sqs.scn taba/limits-fifo.scn taba/limits-sns.scn taba/native.scn)
# GW_FILES="aws/sqs-standard.scn taba/auth.scn ..." runs only these files on Taba (a phase of the plan that is not finished yet).
[ -z "${GW_FILES:-}" ] || read -ra TABA_FILES <<<"$GW_FILES"
SQS_FILES=(aws/sqs-standard.scn aws/sqs-fifo.scn)
SNS_FILES=(aws/sns.scn)

# job <name> <lang> <variant> <prefix> <sqs url> <sns url> <extra driver args...> -- <files...>   (the log goes to $OUT/<name>.log)
job() {
  local name="$1"; shift
  gw_run_driver "$@" >"$OUT/$name.log" 2>&1
}

wait_for_slot() { while [ "$(jobs -rp | wc -l)" -ge "$JOBS" ]; do sleep 1; done; }

JUDGE=()   # name|expected lines
start_job() { # start_job <name> <lang> <variant> <prefix> <sqs> <sns> <extra...> -- <files...>
  local name="$1" lang="$2" variant="$3" prefix="$4" sqs="$5" sns="$6"; shift 6
  local files=() seen=0 a extra=()
  for a in "$@"; do
    if [ "$a" = -- ]; then seen=1; elif [ $seen -eq 1 ]; then files+=("$a"); else extra+=("$a"); fi
  done
  JUDGE+=("$name|$(gw_scenario_lines "${files[@]}")|$prefix|$lang")
  wait_for_slot
  job "$name" "$lang" "$variant" "$prefix" "$sqs" "$sns" ${extra[@]+"${extra[@]}"} -- "${files[@]}" &
}

gw_step "Drivers: ${LANGS[*]} ($TARGET, $([ "$EXPECT_FAIL" -eq 1 ] && echo "red run: every line must fail" || echo "every line must pass"))"
idx=0
for lang in "${LANGS[@]}"; do
  for variant in $(gw_variants "$lang"); do
    [ -z "$ONLY_VARIANT" ] || [ "$variant" = "$ONLY_VARIANT" ] || continue
    prefix="gw${GW_RUN_ID}-$(short "$lang")${idx}"
    idx=$((idx + 1))
    if [ "$TARGET" = taba ]; then
      start_job "$lang-$variant" "$lang" "$variant" "$prefix" "$GW_SQS_URL" "$GW_SNS_URL" -- "${TABA_FILES[@]}"
    else
      start_job "$lang-$variant-sqs" "$lang" "$variant" "$prefix" "$GW_ELASTICMQ_URL" "$GW_MOTO_URL" -- "${SQS_FILES[@]}"
      start_job "$lang-$variant-sns" "$lang" "$variant" "${prefix}s" "$GW_MOTO_URL" "$GW_MOTO_URL" -- "${SNS_FILES[@]}"
    fi
  done
done
wait

# ---- the languages read each other's messages (every language, first variant) -------------------------------------------
if [ "$FULL_SET" -eq 1 ]; then
  gw_step "Cross-language: each language sends to its own queue, and the next one reads it"
  if [ "$TARGET" = taba ]; then CSQS="$GW_SQS_URL"; CSNS="$GW_SNS_URL"; else CSQS="$GW_ELASTICMQ_URL"; CSNS="$GW_MOTO_URL"; fi
  xprefix="gw${GW_RUN_ID}-x"
  first() { gw_variants "$1" | head -n 1; }
  JOBS_SAVE="$JOBS"
  JOBS=1
  start_job "cross-setup" python "$(first python)" "$xprefix" "$CSQS" "$CSNS" -- aws/cross-setup.scn
  wait
  JOBS="$JOBS_SAVE"
  for lang in "${ALL_LANGS[@]}"; do
    start_job "cross-publish-$lang" "$lang" "$(first "$lang")" "$xprefix" "$CSQS" "$CSNS" -- aws/cross-publish.scn
  done
  wait
  prev="${ALL_LANGS[${#ALL_LANGS[@]} - 1]}"
  for lang in "${ALL_LANGS[@]}"; do
    start_job "cross-drain-$lang" "$lang" "$(first "$lang")" "$xprefix" "$CSQS" "$CSNS" --prev "$prev" -- aws/cross-drain.scn
    prev="$lang"
  done
  wait
fi

# ---- the verdict ----------------------------------------------------------------------------------------------------------------
gw_step "Result"
BAD=0
for entry in "${JUDGE[@]}"; do
  IFS='|' read -r name expected prefix lang <<<"$entry"
  gw_judge "$OUT/$name.log" "$expected" "$WANT" "$name" || BAD=$((BAD + 1))
  # What a native Taba consumer reads of what the SDK sent (scenarios/taba/native.scn); on the red run the queue must not exist.
  if [ "$TARGET" = taba ]; then
    case "$name" in cross-*) continue ;; esac
    native="$(gw_native_check "$prefix-native" "$lang")"
    if [ "$WANT" = pass ] && [ "$native" = ok ]; then echo "  ok   $name: a native consumer reads both messages"
    elif [ "$WANT" = connect ] && [ "$native" = missing ]; then echo "  red  $name: the queue of the native check does not exist"
    else echo "  BAD  $name: native check: $native"; BAD=$((BAD + 1)); fi
  fi
done

echo
if [ "$BAD" -gt 0 ]; then
  echo "FAILED: $BAD driver run(s) are not as wanted (logs in $OUT)" >&2
  exit 1
fi
if [ "$EXPECT_FAIL" -eq 1 ]; then
  echo "red run confirmed: every line of every driver failed because nothing answers"
else
  echo "all the lines passed"
fi
