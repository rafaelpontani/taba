#!/usr/bin/env bash
# Starts the root Compose from scratch and fills it with a test scenario: queues, distributors and 100
# thousand messages in each, with one "noisy" queue (a producer that dominates the queue).
#
#   1. deletes everything (`docker compose down -v`: volumes, secrets, data and the backup bucket) and starts
#      the three-node cluster (taba-0, taba-1, taba-2) building this repository's images;
#   2. creates an `admin` token named "local-admin" (the root token only bootstraps it) and writes it to docker/.local/admin-token;
#   3. points the taba-cli profile "local_admin" of this machine to http://localhost:8080 and the new token
#      (it updates the profile if it exists; otherwise it creates it and makes it the default);
#   4. creates 3 queues behind 2 distributors, plus a FIFO queue;
#   5. publishes MESSAGES messages to each distributor (in parallel, with taba-cli) and shows the counts.
#
# Scenario (doc/en-US/25-testing.md, "Demo data"):
#
#   distributor demo-orders  (prefix demo-orders-)  ->  queue demo-orders-billing   100 thousand messages
#                                                         queue demo-orders-stock       100 thousand messages
#       100 groups (customer-001..100) with the same amount: a balanced queue.
#   distributor demo-events  (prefix demo-events-)  ->  queue demo-events-audit     100 thousand messages
#       95% of the messages from the group noisy-integration and 5% spread over 50 groups (service-01..50):
#       the noisy queue (condition `noisy-group`; see `?scope=node` and `/groups` in the documentation).
#
# Usage:
#   docker/seed-demo.sh                   # everything, from scratch; leaves the cluster running at the end
#   docker/seed-demo.sh --no-build        # reuses the images already built
#   docker/seed-demo.sh --messages 10000  # another amount per distributor (default: 100000)
#   docker/seed-demo.sh --no-profile      # does not touch the taba-cli profile
#
# Variables (optional):
#   MESSAGES   messages published to each distributor (default 100000; --messages wins)
#   PARALLEL   how many taba-cli containers publish at the same time (default 8)
#   TOKEN_FILE where to write the admin token (default docker/.local/admin-token, ignored by git)
#   ADMIN_NAME name of the admin token (default local-admin)
#   PROFILE    the taba-cli profile to configure (default local_admin)
#   TABA_CLI_BIN  the taba-cli of this machine (default: the one on the PATH, or this repository's build)
#
# The token has the `admin` permission (it manages tokens and operates the cluster; the root token is not used
# outside the containers). The file stays on your machine only (0600 where the
# system allows it); use it like this:
#   curl -H "Authorization: Bearer $(cat docker/.local/admin-token)" localhost:8080/topics
#
# Needs Docker (with the compose plugin). The host's port 8080 (HAProxy) must be free. It takes a few
# minutes: building the images and publishing the 200 thousand messages.

set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/cluster.sh"
cd "$TABA_ROOT"

MESSAGES="${MESSAGES:-100000}"
PARALLEL="${PARALLEL:-8}"
TOKEN_FILE="${TOKEN_FILE:-$TABA_ROOT/docker/.local/admin-token}"
ADMIN_NAME="${ADMIN_NAME:-local-admin}"
PROFILE="${PROFILE:-local_admin}"
API_URL="http://localhost:8080"
CONFIGURE_PROFILE=1

while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) TABA_BUILD=0 ;;
    --no-profile) CONFIGURE_PROFILE=0 ;;
    --messages) shift; MESSAGES="${1:?usage: --messages <n>}" ;;
    -h|--help) sed -n '2,/^# minutes/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) taba_die "unknown argument: $1 (see --help)" ;;
  esac
  shift
done
case "$MESSAGES$PARALLEL" in *[!0-9]*|"") taba_die "MESSAGES and PARALLEL must be integers" ;; esac
[ "$MESSAGES" -ge 1000 ] || taba_die "MESSAGES must be at least 1000 (the scenario's groups divide the total)"

taba_require_docker

# --- 1. cluster from scratch ------------------------------------------------------------------

taba_up

# --- 2. admin token ---------------------------------------------------------------------------

# The root token is only for bootstrapping: it lives in the `secrets` volume (the Compose taba-cli already uses it)
# and only it can create an `admin` token. Everything outside the containers uses the admin token, not the root.
# The step above deleted the volumes, so no admin token exists yet and it is always created here; its secret only
# appears in the creation response, which is why it goes straight to the file.
taba_step "Creating the local admin token ($ADMIN_NAME) and writing it to $TOKEN_FILE"
mkdir -p "$(dirname "$TOKEN_FILE")"
read -r admin_id token < <(taba_create_token "$ADMIN_NAME" --permission admin)
# No newline at the end: `$(cat file)` and `--data-binary @file` get the exact token.
( umask 077; printf '%s' "$token" > "$TOKEN_FILE" )
chmod 600 "$TOKEN_FILE" 2>/dev/null || true
taba_ok "admin token $admin_id written (${#token} characters)"
me="$(curl -s -H "Authorization: Bearer $token" "$API_URL/auth/me" | tr -d '\n ')"
case "$me" in
  *'"root":false'*'"admin"'*|*'"admin"'*'"root":false'*) taba_ok "the API accepts it: $me" ;;
  *) taba_die "the new token was not accepted as an admin (GET /auth/me: $me)" ;;
esac


# --- 2b. taba-cli profile ---------------------------------------------------------------------

# Points the profile $PROFILE of the taba-cli installed on this machine to the new cluster, so that a plain
# `taba-cli topic list` works. If the profile exists, only its url and token change (the token becomes a
# `token-file`, which is this script's file); otherwise it is created and made the default. The binary comes
# from TABA_CLI_BIN, the PATH or this repository's build; without one, the step is skipped.
configure_profile() {
  local cli="${TABA_CLI_BIN:-}" candidate file
  if [ -z "$cli" ]; then
    for candidate in taba-cli "$TABA_ROOT/connectors/rust/target/release/taba-cli"; do
      if command -v "$candidate" >/dev/null 2>&1; then cli="$candidate"; break; fi
    done
  fi
  if [ -z "$cli" ]; then
    echo "  taba-cli not found on this machine: profile $PROFILE not configured (scripts/install-cli.sh installs it;"
    echo "  then: taba-cli profile add $PROFILE --url $API_URL --token-file $TOKEN_FILE --default)"
    return 0
  fi
  file="$TOKEN_FILE"
  command -v cygpath >/dev/null 2>&1 && file="$(cygpath -w "$TOKEN_FILE")"   # Windows binary, Windows path
  if "$cli" profile show "$PROFILE" >/dev/null 2>&1; then
    "$cli" profile set "$PROFILE" --url "$API_URL" --token-file "$file" --unset token -q
    taba_ok "profile $PROFILE updated: $API_URL and the new token ($("$cli" profile path))"
  else
    "$cli" profile add "$PROFILE" --url "$API_URL" --token-file "$file" --default -q
    taba_ok "profile $PROFILE created and set as the default ($("$cli" profile path))"
  fi
}

if [ "$CONFIGURE_PROFILE" -eq 1 ]; then
  taba_step "Configuring the taba-cli profile $PROFILE"
  configure_profile || echo "  could not configure the profile (the rest of the script goes on)"
fi

# --- 3. queues and distributors ---------------------------------------------------------------

taba_step "Creating the queues and the distributors"
# A distributor only copies to the queues that exist when it publishes: the queues come first.
for queue in demo-orders-billing demo-orders-stock demo-events-audit; do
  taba_ensure_topic "$queue" --type queue --tag demo=taba
done
taba_ensure_topic demo-orders --type distributor --prefix demo-orders- --tag demo=taba
taba_ensure_topic demo-events --type distributor --prefix demo-events- --tag demo=taba
# A FIFO queue is not behind a distributor: it is published to directly, in order within each group.
taba_ensure_topic demo-payments-fifo --type queue --fifo --tag demo=taba

# --- 4. messages ------------------------------------------------------------------------------

# Each line of the plan is "topic|group|count|mode". A container reads its lines and runs one
# `taba-cli publish --count` per line (a container per line would cost ~1 s of startup each).
# Mode "bulk" repeats the same body; mode "seq" (FIFO) publishes numbered bodies with a deduplication ID, since
# a FIFO queue drops a repetition of the same body in the same group.
plan="$(mktemp -d)"
trap 'rm -rf "$plan"' EXIT

orders_groups=100
orders_per_group=$((MESSAGES / orders_groups))
noisy=$((MESSAGES * 95 / 100))
quiet_groups=50
quiet_per_group=$(((MESSAGES - noisy) / quiet_groups))
fifo_groups=20
fifo_per_group=$((MESSAGES / 10 / fifo_groups))
chunk=5000   # the noisy group is split in blocks so that every worker gets a share of it

{
  g=1
  while [ "$g" -le "$orders_groups" ]; do
    printf 'demo-orders|customer-%03d|%d|bulk\n' "$g" "$orders_per_group"
    g=$((g + 1))
  done
  left=$noisy
  while [ "$left" -gt 0 ]; do
    n=$((left < chunk ? left : chunk))
    printf 'demo-events|noisy-integration|%d|bulk\n' "$n"
    left=$((left - n))
  done
  g=1
  while [ "$g" -le "$quiet_groups" ]; do
    printf 'demo-events|service-%02d|%d|bulk\n' "$g" "$quiet_per_group"
    g=$((g + 1))
  done
  g=1
  while [ "$g" -le "$fifo_groups" ]; do
    printf 'demo-payments-fifo|account-%02d|%d|seq\n' "$g" "$fifo_per_group"
    g=$((g + 1))
  done
} > "$plan/all"

total_orders=$((orders_per_group * orders_groups))
total_events=$((noisy + quiet_per_group * quiet_groups))
total_fifo=$((fifo_per_group * fifo_groups))
taba_step "Publishing $total_orders messages to demo-orders, $total_events to demo-events and $total_fifo to demo-payments-fifo ($PARALLEL in parallel)"
# Deals the lines out to the workers round-robin.
awk -v n="$PARALLEL" -v dir="$plan" '{ print > (dir "/worker-" (NR % n)) }' "$plan/all"

worker() {
  # The body is fixed per line (--count repeats the same publication). If one fails, the container exits with an error.
  taba_compose run --rm -T --entrypoint sh taba-cli -c '
    while IFS="|" read -r topic group count mode; do
      if [ "$mode" = seq ]; then
        i=1
        while [ "$i" -le "$count" ]; do
          taba-cli publish "$topic" "{\"group\":\"$group\",\"seq\":$i}" --json --group-id "$group" \
            --deduplication-id "$group-$i" --id-only -q >/dev/null || exit 1
          i=$((i + 1))
        done
      else
        taba-cli publish "$topic" "{\"demo\":true,\"topic\":\"$topic\",\"group\":\"$group\"}" \
          --json --group-id "$group" --count "$count" --id-only -q >/dev/null || exit 1
      fi
    done' < "$1" 2> "$1.log"
}

started=$SECONDS
pids=()
for f in "$plan"/worker-*; do
  case "$f" in *.log) continue ;; esac
  worker "$f" &
  pids+=("$!")
done
failed=0
for pid in "${pids[@]}"; do
  wait "$pid" || failed=1
done
if [ "$failed" -ne 0 ]; then
  for log in "$plan"/*.log; do grep -vE '^ *(Container|Network|Volume)' "$log" | tail -n 5 >&2 || true; done
  taba_die "one of the publishers failed (last lines of each above)"
fi
taba_ok "published in $((SECONDS - started)) s"

# --- 5. result --------------------------------------------------------------------------------

taba_step "State of the queues (the per-node counts may take a few seconds to settle)"
sleep 5
for queue in demo-orders-billing demo-orders-stock demo-events-audit demo-payments-fifo; do
  taba_cli stats topic "$queue" -o json 2>/dev/null \
    | grep -E '"(name|depth|groups|noisyShare|worstCondition)"' | tr -d '\n ' ; echo
done

cat <<EOF

Done. The cluster is still running (docker/cluster.sh down, or docker compose down -v, deletes everything).

  Client API      http://localhost:8080
  Admin token     $TOKEN_FILE
  Example         curl -H "Authorization: Bearer \$(cat "$TOKEN_FILE")" http://localhost:8080/topics
  Noisy groups    curl -H "Authorization: Bearer \$(cat "$TOKEN_FILE")" "http://localhost:8080/stats/topics/demo-events-audit/groups?limit=3"
  Noisy condition curl -H "Authorization: Bearer \$(cat "$TOKEN_FILE")" "http://localhost:8080/stats/topics/demo-events-audit?scope=node"
  CLI             docker compose run --rm taba-cli tui
  Local taba-cli  taba-cli topic list   (profile $PROFILE, if the step above found a taba-cli)
EOF
