"""
The Python driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and
runs each line against a real Taba through the connector, in the style of the language.

    python contract/driver.py --url http://host:8080 --token <token> --prefix ct-python-0101 --lang python \
        --scenario ../contract/scenarios/core.scn
"""

import base64
import os
import re
import sys
import threading
import time
import urllib.parse

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from taba import UNSET, BatchMessage, Cancelled, CancelToken, Client, TabaError, TopicRequest, TopicUpdate  # noqa: E402


def parse_args(argv):
    out = {"scenario": []}
    for i in range(0, len(argv), 2):
        key = argv[i][2:]
        if key == "scenario":
            out["scenario"].append(argv[i + 1])
        else:
            out[key] = argv[i + 1]
    return out


ARGS = parse_args(sys.argv[1:])

# ---- the scenario language ------------------------------------------------------------------------------

_VARIABLE = re.compile(r"\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)")


def expand(value, variables):
    """Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched."""
    out = []
    last = 0
    for m in _VARIABLE.finditer(value):
        out.append(urllib.parse.unquote(value[last : m.start()]))
        if m.group(0) == "{p}":
            out.append(ARGS["prefix"])
        elif m.group(0) == "{lang}":
            out.append(ARGS["lang"])
        else:
            out.append(variables.get(m.group(1), {}).get(m.group(2), ""))
        last = m.end()
    out.append(urllib.parse.unquote(value[last:]))
    return "".join(out)


def parse_line(text, variables):
    head, _, tail = text.partition(" => ")
    parts = head.split()
    params = {}
    for pair in parts[1:]:
        key, _, value = pair.partition("=")
        params[key] = expand(value, variables)
    expects = []
    for token in tail.split():
        m = re.search(r"[=~]", token)
        expects.append((token[: m.start()], token[m.start()], expand(token[m.start() + 1 :], variables)))
    return parts[0], params, expects


# ---- the operations -------------------------------------------------------------------------------------


def num(v):
    return None if v is None else int(v)


def flag(v):
    return None if v is None else v == "true"


def items(v):
    return None if v is None else [x for x in v.split(",") if x]


def tags(v):
    return None if v is None else dict(t.split(":", 1) for t in v.split(","))


def nullable(v):
    """"null" goes back to the server default; absent leaves the field out."""
    return UNSET if v is None else None if v == "null" else int(v)


def render(message):
    if message.content_type.split(";")[0].strip().lower() == "application/octet-stream":
        return "b64:" + base64.b64encode(message.body).decode("ascii")
    return message.body_text


def text_of(value):
    return "" if value is None else str(value).lower() if isinstance(value, bool) else str(value)


def topic_fields(t):
    return {"id": t.id, "name": t.name, "type": t.type, "fifo": text_of(t.fifo), "volatile": text_of(t.volatile), "raw": t.raw}


def publish_fields(r):
    return {"id": r.id or "", "duplicate": text_of(r.duplicate), "matched": text_of(r.matched)}


def consume_options(p):
    return {"max_messages": num(p.get("max")), "wait_time": num(p.get("wait")), "visibility_timeout": num(p.get("visibility"))}


def message_fields(messages):
    fields = {"count": str(len(messages)), "bodies": ",".join(render(m) for m in messages)}
    if messages:
        first = messages[0]
        fields.update(
            id=first.id,
            receiptToken=first.receipt_token,
            deliveryCount=str(first.delivery_count),
            contentType=first.content_type,
            body=render(first),
        )
    return fields


def publish_options(p):
    return {"group_id": p.get("groupId"), "deduplication_id": p.get("deduplicationId"), "fifo": flag(p.get("fifo")) or False}


def op_create_topic(c, p):
    return topic_fields(
        c.create_topic(
            TopicRequest(
                name=p["name"], type=p["type"], fifo=flag(p.get("fifo")), prefixes=items(p.get("prefixes")),
                tags=tags(p.get("tags")), volatile=flag(p.get("volatile")), ttl=num(p.get("ttl")),
                max_deliveries=num(p.get("maxDeliveries")), max_message_bytes=num(p.get("maxMessageBytes")),
                idle_timeout=num(p.get("idleTimeout")),
            )
        )
    )


def op_list_topics(c, p):
    topics = c.list_all_topics(p.get("name"))
    return {"count": str(len(topics)), "names": ",".join(sorted(t.name for t in topics))}


def op_update_topic(c, p):
    return topic_fields(
        c.update_topic(
            p["name"],
            TopicUpdate(
                prefixes=items(p.get("prefixes")), tags=tags(p.get("tags")), idle_timeout=num(p.get("idleTimeout")),
                ttl=nullable(p.get("ttl")), max_deliveries=nullable(p.get("maxDeliveries")),
                max_message_bytes=nullable(p.get("maxMessageBytes")),
            ),
        )
    )


def op_delete_topic(c, p):
    c.delete_topic(p["name"])
    return {}


def op_publish_bytes(c, p):
    options = publish_options(p)
    return publish_fields(c.publish(p["topic"], base64.b64decode(p["base64"]), content_type=p.get("contentType", "application/octet-stream"), **options))


def op_publish_batch(c, p):
    """A batch of texts (one item per text); bad=true adds an item with an invalid content type."""
    batch = [BatchMessage(t, content_type="text/plain; charset=utf-8", group_id=p.get("groupId")) for t in items(p["texts"])]
    if flag(p.get("bad")):
        batch.append(BatchMessage("x", content_type="not a type"))
    results = c.publish_batch(p["topic"], batch)
    return {"count": str(len(results)), "statuses": ",".join(str(r.status) for r in results)}


def op_settle_batch(c, p):
    """Consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes the receipt
    of the first one old before the batch: it is given back and taken again."""
    messages = c.consume(p["topic"], **consume_options(p))
    if flag(p.get("stale")) and messages:
        c.change_visibility(p["topic"], messages[0], 0)
        c.consume(p["topic"], max_messages=1, wait_time=2, visibility_timeout=num(p.get("visibility")))
    if p["action"] == "visibility":
        results = c.change_visibility_batch(p["topic"], messages, int(p["seconds"]))
    else:
        results = c.ack_batch(p["topic"], messages)
    return {"count": str(len(results)), "statuses": ",".join(str(r.status) for r in results)}


def op_change_visibility(c, p):
    r = c.change_visibility(p["topic"], p["id"], int(p["seconds"]), p["receipt"])
    return {"id": r.id, "invisibleUntil": r.invisible_until}


def op_handle(c, p):
    """Consuming with a handler: mode ok (deleted), fail (raises), keep (delete = False) or slow (holds the message
    for `hold` seconds, which only works if the invisibility is renewed)."""
    seen = []
    lost = []

    def handler(context):
        seen.append(render(context.message))
        if p["mode"] == "fail":
            raise RuntimeError("the handler failed")
        if p["mode"] == "keep":
            context.delete = False
        if p["mode"] == "slow":
            time.sleep(float(p["hold"]))
        lost.append(context.lease_lost)

    failed = False
    handled = 0
    try:
        handled = c.consume_with(p["topic"], handler, **consume_options(p))
    except TabaError:
        raise
    except Exception:  # noqa: BLE001 - the handler's own failure
        failed = True
    return {
        "handled": str(handled), "failed": text_of(failed), "bodies": ",".join(seen),
        "leaseLost": text_of(bool(lost and lost[0])),
    }


def op_subscribe(c, p):
    """The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first one fail."""
    cancel = CancelToken()
    seen = []
    timer = threading.Timer(float(p["timeout"]), cancel.cancel)
    timer.start()

    def handler(message):
        seen.append(render(message))
        if len(seen) >= int(p["count"]):
            cancel.cancel()
        if p.get("failFirst") == "true" and len(seen) == 1:
            raise RuntimeError("the handler failed")

    try:
        c.subscribe(p["topic"], handler, cancel=cancel, visibility_timeout=num(p.get("visibility")))
    finally:
        timer.cancel()
    return {"count": str(len(seen)), "bodies": ",".join(seen)}


def op_cancel_consume(c, p):
    """A long polling cancelled from outside must end at once, not at the end of the wait."""
    cancel = CancelToken()
    started = time.time()
    threading.Timer(float(p["after"]), cancel.cancel).start()
    cancelled = False
    try:
        c.consume(p["topic"], wait_time=20, cancel=cancel)
    except Cancelled:
        cancelled = True
    return {"cancelled": text_of(cancelled), "fast": text_of(time.time() - started < 10)}


def op_drain(c, p):
    """Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted."""
    seen = []
    deadline = time.time() + float(p["timeout"])
    while len(seen) < int(p["count"]) and time.time() < deadline:
        for message in c.consume(p["topic"], max_messages=10, wait_time=1, visibility_timeout=60):
            seen.append(render(message))
            c.ack(p["topic"], message)
    return {"count": str(len(seen)), "bodies": ",".join(sorted(seen))}


def op_sleep(c, p):
    time.sleep(float(p["seconds"]))
    return {}


OPERATIONS = {
    "createTopic": op_create_topic,
    "getTopic": lambda c, p: topic_fields(c.get_topic(p["name"])),
    "listTopics": op_list_topics,
    "updateTopic": op_update_topic,
    "deleteTopic": op_delete_topic,
    "publishText": lambda c, p: publish_fields(c.publish_text(p["topic"], p["body"], **publish_options(p))),
    "publishJson": lambda c, p: publish_fields(c.publish_raw_json(p["topic"], p["body"], **publish_options(p))),
    "publishBytes": op_publish_bytes,
    "publishBatch": op_publish_batch,
    "settleBatch": op_settle_batch,
    "consume": lambda c, p: message_fields(c.consume(p["topic"], **consume_options(p))),
    "ack": lambda c, p: c.ack(p["topic"], p["id"], p["receipt"]) or {},
    "changeVisibility": op_change_visibility,
    "handle": op_handle,
    "subscribe": op_subscribe,
    "cancelConsume": op_cancel_consume,
    "drain": op_drain,
    "sleep": op_sleep,
}

# ---- the run --------------------------------------------------------------------------------------------


def check(result, expects):
    problems = []
    failure = "error" in result
    if failure and not any(key == "error" for key, _, _ in expects):
        problems.append("unexpected error %s (status %s): %s" % (result["error"], result["status"], result.get("message")))
    if not failure and any(key == "error" for key, _, _ in expects):
        problems.append("expected an error, but the call succeeded")
    for key, op, value in expects:
        actual = result.get(key)
        if op == "~":
            mismatch = value not in (actual or "")
        elif value == "*":
            mismatch = not actual
        else:
            mismatch = actual != value
        if mismatch:
            problems.append("%s %s %r, got %r" % (key, op, value, actual))
    return problems


def main():
    clients = {}

    def client_for(p):
        key = (p.get("url"), p.get("token"), p.get("attempts"))
        if key not in clients:
            options = {"token": p.get("token", ARGS["token"])}
            if p.get("attempts"):
                options["max_attempts"] = int(p["attempts"])
            clients[key] = Client(p.get("url", ARGS["url"]), **options)
        return clients[key]

    variables = {}
    passed = failed = skipped = 0
    for path in ARGS["scenario"]:
        with open(path, encoding="utf-8") as handle:
            lines = handle.read().splitlines()
        for n, raw in enumerate(lines, 1):
            text = raw.strip()
            if not text or text.startswith("#"):
                continue
            where = "%s:%d" % (os.path.basename(path), n)
            op, params, expects = parse_line(text, variables)
            if op not in OPERATIONS:
                skipped += 1
                print("SKIP %s %s (not supported by this connector)" % (where, op))
                continue
            try:
                result = OPERATIONS[op](client_for(params), params)
            except TabaError as e:
                result = {"error": e.code, "status": str(e.status), "message": str(e)}
            if "as" in params:
                variables[params["as"]] = result
            problems = check(result, expects)
            if not problems:
                passed += 1
                print("PASS %s %s" % (where, op))
            else:
                failed += 1
                print("FAIL %s %s" % (where, text))
                for problem in problems:
                    print("       " + problem)
    for client in clients.values():
        client.close()
    print("%s: %d passed, %d failed, %d skipped" % (ARGS["lang"], passed, failed, skipped))
    sys.exit(0 if failed == 0 else 1)


main()
