#!/usr/bin/env bash
# The end-to-end validation of version 0.1 (doc/pt-BR/32-roadmap.md).
#
# Starts the root Compose from scratch and checks, in this order:
#   1. the cluster forms: three healthy nodes and `taba-cli health cluster` with no conditions;
#   2. taba-cli publishes and consumes (topic, publish, consume --ack, list);
#   3. an abrupt node crash (`docker compose kill`): it is detected, the node is excluded by vote,
#      publishing and consuming go on with two nodes, and the node returns as a new node (`docker compose start`);
#   4. the orderly exit of another node (`docker compose stop`, which triggers the drain) and its return;
#   5. at no point is a published message (acknowledged by the quorum) lost: at the end, the set of
#      confirmed messages is exactly the published one;
#   6. the S3 backup (the Compose SeaweedFS): `POST /cluster/stop`, the nodes' disks wiped, the
#      cluster starts again and the published messages come back from the bucket.
#
# It only uses taba-cli: it is the general integrated cluster test, not a connector's. Each connector has
# its own test (e.g. connectors/csharp/scripts/integration.sh). Starting, tearing down and causing failures
# in the cluster lives in docker/cluster.sh, which the connector tests also use (source or command line).
#
# Usage:
#   docker/validate.sh                  # everything, from scratch; deletes the cluster at the end
#   docker/validate.sh --keep           # leaves the cluster up at the end
#   docker/validate.sh --no-build       # reuses the images already built
#
# Needs Docker (with the compose plugin). The host's port 8080 must be free.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/cluster.sh"
cd "$TABA_ROOT"

while [ $# -gt 0 ]; do
  case "$1" in
    --keep) TABA_KEEP=1 ;;
    --no-build) TABA_BUILD=0 ;;
    -h|--help) sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

# Shortcuts for the functions in docker/cluster.sh, the library the connector tests also use.
step() { taba_step "$@"; }
ok() { taba_ok "$@"; }
die() { taba_die "$@"; }
cli() { taba_cli "$@"; }
active_members() { taba_active_members; }

taba_require_docker

PUBLISHED="$(mktemp)"; ACKED="$(mktemp)"
cleanup() {
  local status=$?
  rm -f "$PUBLISHED" "$ACKED"
  taba_teardown "$status" || true
  exit "$status"
}
trap cleanup EXIT

# Publishes "$2" text messages "$1-N" to topic "$3" and records each one in $PUBLISHED.
publish_many() {
  local prefix="$1" count="$2" topic="$3" n
  for n in $(seq 1 "$count"); do
    cli publish "$topic" --text "$prefix-$n" --id-only >/dev/null || die "publish $prefix-$n to $topic"
    echo "$prefix-$n" >> "$PUBLISHED"
  done
}

# Consumes and acknowledges whatever there is (until empty), recording it in $ACKED.
drain_topic() {
  local topic="$1" max_rounds="${2:-20}" round out
  for round in $(seq 1 "$max_rounds"); do
    out="$(cli consume "$topic" --max 10 --wait 3 --ack -o raw)"
    [ -n "$out" ] || return 0
    printf '%s\n' "$out" >> "$ACKED"
  done
}

# --- 1. the cluster forms ----------------------------------------------------------------------

taba_up

step "The cluster formed: taba-cli health cluster"
[ "$(active_members)" = "3" ] || die "expected 3 active members, found '$(active_members)'"
cli health cluster -o json | grep -q '"conditions": \[\]' || die "the cluster has conditions: $(cli health cluster -o table)"
ok "3 active members and no conditions"

# --- 2. taba-cli --------------------------------------------------------------------------------

step "taba-cli: create a topic, publish and consume"
cli topic create validation-cli --type queue >/dev/null
cli publish validation-cli --text "hello from taba-cli" --id-only >/dev/null
got="$(cli consume validation-cli --max 1 --wait 5 --ack -o raw)"
[ "$got" = "hello from taba-cli" ] || die "consume returned '$got'"
cli topic list --name validation-cli | grep -q validation-cli || die "topic list does not show the topic"
ok "publish, consume --ack and topic list"

# --- 3. abrupt crash ----------------------------------------------------------------------------

step "Abrupt crash of taba-2 (docker compose kill)"
cli topic create validation-crash --type queue >/dev/null
publish_many before 30 validation-crash
ok "30 messages published and confirmed by the quorum"

taba_kill taba-2
taba_wait_members 2 120

publish_many during 10 validation-crash
ok "10 messages published with two nodes"
drain_topic validation-crash 2
[ "$(wc -l < "$ACKED")" -ge 10 ] || die "consuming with two nodes returned fewer than 10 messages"
ok "consuming goes on with two nodes ($(wc -l < "$ACKED") acknowledged so far)"

taba_start taba-2
taba_wait_stable 240

# --- 4. orderly exit ----------------------------------------------------------------------------

step "Orderly exit of taba-1 (docker compose stop, with drain)"
publish_many drain 10 validation-crash
taba_stop taba-1
taba_wait_members 2 120
taba_start taba-1
taba_wait_stable 240

# --- 5. nothing was lost ------------------------------------------------------------------------

step "No acknowledged message was lost"
drain_topic validation-crash 30
sort -u "$PUBLISHED" > /tmp/taba-published.txt
sort -u "$ACKED" > /tmp/taba-acked.txt
if ! diff -q /tmp/taba-published.txt /tmp/taba-acked.txt >/dev/null; then
  echo "published and not received:" >&2
  comm -23 /tmp/taba-published.txt /tmp/taba-acked.txt >&2
  die "the received set differs from the published one"
fi
ok "$(wc -l < /tmp/taba-published.txt) messages published, all received and acknowledged"

# --- 6. S3 backup (SeaweedFS) ------------------------------------------------------------------------

step "S3 backup: stop the cluster, wipe the nodes' disks and restore from SeaweedFS"
cli topic create validation-backup --type queue >/dev/null
BACKUP_PUBLISHED="$(mktemp)"
for n in $(seq 1 20); do
  cli publish validation-backup --text "backup-$n" --id-only >/dev/null || die "publish backup-$n"
  echo "backup-$n" >> "$BACKUP_PUBLISHED"
done
ok "20 messages published to validation-backup"

stop="$(taba_compose exec -T taba-0 wget -q -O - --post-data='' http://127.0.0.1:9090/cluster/stop)" || die "POST /cluster/stop"
case "$stop" in *'"stopList":true'*) ok "planned stop: the node list was written to S3" ;; *) die "/cluster/stop did not write the list: $stop" ;; esac

taba_compose stop taba-0 taba >/dev/null 2>&1
# -v also removes the replicas' anonymous volumes (the first node's `data-0` is a named volume).
taba_compose rm -f -v taba-0 taba >/dev/null 2>&1
for volume in $(docker volume ls -q | grep -E '(^|_)data-0$'); do docker volume rm "$volume" >/dev/null; done
ok "the nodes' disks were wiped: only the bucket keeps the messages"

taba_compose up -d
taba_wait_healthy 240
BACKUP_ACKED="$(mktemp)"
for round in $(seq 1 6); do
  cli consume validation-backup --max 10 --wait 5 --ack -o raw >> "$BACKUP_ACKED" || true
done
sort -u "$BACKUP_PUBLISHED" > /tmp/taba-backup-published.txt
sort -u "$BACKUP_ACKED" > /tmp/taba-backup-acked.txt
if ! diff -q /tmp/taba-backup-published.txt /tmp/taba-backup-acked.txt >/dev/null; then
  echo "published and not restored:" >&2
  comm -23 /tmp/taba-backup-published.txt /tmp/taba-backup-acked.txt >&2
  die "the restore from S3 did not return everything"
fi
rm -f "$BACKUP_PUBLISHED" "$BACKUP_ACKED"
ok "the 20 messages came back from S3 after the infrastructure was wiped"

step "Final state"
taba_status | sed -n 1,6p

step "The validation passed"
