"""
The Python driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
runs each line through boto3, the way an application would. Prints one `RESULT` line per scenario line.

    python driver.py --sqs-endpoint http://host:9324 --sns-endpoint http://host:9325 --endpoint-mode env|code \
        --prefix gw-python-0101 --lang python --variant current --scenario file.scn ...
"""

import base64
import hashlib
import json
import re
import socket
import sys
import time
import urllib.parse

import boto3
import botocore
from botocore.config import Config
from botocore.exceptions import ClientError


def parse_args(argv):
    out = {"scenario": [], "cred": []}
    for i in range(0, len(argv), 2):
        key = argv[i][2:]
        if key in out:
            out[key].append(argv[i + 1])
        else:
            out[key] = argv[i + 1]
    return out


ARGS = parse_args(sys.argv[1:])
REGION = ARGS.get("region", "us-east-1")
CREDS = {}
for item in ARGS["cred"]:
    name, _, pair = item.partition("=")
    CREDS[name] = tuple(pair.split(":", 1))


def reachable(url):
    parsed = urllib.parse.urlparse(url)
    try:
        socket.create_connection((parsed.hostname, parsed.port or 80), timeout=3).close()
        return True
    except OSError:
        return False


UNREACHABLE = not all(reachable(ARGS[k]) for k in ("sqs-endpoint", "sns-endpoint"))

CONFIG = Config(retries={"max_attempts": 1, "mode": "standard"}, connect_timeout=5, read_timeout=40)
CLIENTS = {}


def make_clients(creds=None):
    kw = {"region_name": REGION, "config": CONFIG}
    if creds:
        kw["aws_access_key_id"], kw["aws_secret_access_key"] = creds
    sqs_kw, sns_kw = dict(kw), dict(kw)
    if ARGS.get("endpoint-mode") == "code":
        sqs_kw["endpoint_url"] = ARGS["sqs-endpoint"]
        sns_kw["endpoint_url"] = ARGS["sns-endpoint"]
    CLIENTS["sqs"] = boto3.client("sqs", **sqs_kw)
    CLIENTS["sns"] = boto3.client("sns", **sns_kw)


make_clients()

# ---- the scenario language ------------------------------------------------------------------------------

_VARIABLE = re.compile(r"\{p\}|\{lang\}|\{prev\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)")


def expand(value, variables):
    out, last = [], 0
    for m in _VARIABLE.finditer(value):
        out.append(urllib.parse.unquote(value[last : m.start()]))
        if m.group(0) == "{p}":
            out.append(ARGS["prefix"])
        elif m.group(0) == "{lang}":
            out.append(ARGS["lang"])
        elif m.group(0) == "{prev}":
            out.append(ARGS.get("prev", ""))
        else:
            out.append(str(variables.get(m.group(1), {}).get(m.group(2), "")))
        last = m.end()
    out.append(urllib.parse.unquote(value[last:]))
    return "".join(out)


_EXPECT = re.compile(r"^([A-Za-z0-9_.]+)(>=|<=|!=|=|~)(.*)$")


def parse_line(text, variables):
    head, _, tail = text.partition(" => ")
    parts = head.split()
    params, raw = {}, {}
    for part in parts[1:]:
        k, _, v = part.partition("=")
        raw[k] = v
        params[k] = expand(v, variables)
    expects = []
    for token in tail.split():
        m = _EXPECT.match(token)
        expects.append((m.group(1), m.group(2), expand(m.group(3), variables)))
    return parts[0], params, raw, expects


def check(result, expects):
    for key, op, want in expects:
        got = str(result.get(key, ""))
        if op == "=" and want == "*":
            ok = got != ""
        elif op == "=":
            ok = got == want
        elif op == "!=":
            ok = got != want
        elif op == "~":
            ok = want in got
        elif op == ">=":
            ok = float(got or "nan") >= float(want)
        else:
            ok = float(got or "nan") <= float(want)
        if not ok:
            return "%s%s%s but was %r" % (key, op, want, got[:200])
    return None


# ---- the operations -------------------------------------------------------------------------------------

def md5(text):
    return hashlib.md5(text.encode("utf-8")).hexdigest()


def pairs(text):
    if not text:
        return {}
    return dict(item.split(":", 1) for item in text.split(","))


def msg_attrs_in(text):
    out = {}
    for item in text.split(",") if text else []:
        name, typ, value = item.split(":", 2)
        if typ == "Binary":
            out[name] = {"DataType": typ, "BinaryValue": base64.b64decode(value[4:])}
        else:
            out[name] = {"DataType": typ, "StringValue": value}
    return out


def msg_attrs_out(message, result):
    for name, a in (message.get("MessageAttributes") or {}).items():
        typ = a["DataType"]
        if typ.startswith("Binary"):
            result["msgattr_" + name] = "b64:" + base64.b64encode(a["BinaryValue"]).decode()
        else:
            result["msgattr_" + name] = a["StringValue"]
        result["msgattrtype_" + name] = typ


def names_of(urls):
    return sorted(u.rstrip("/").rsplit("/", 1)[-1] for u in urls)


def received(messages, elapsed, result=None):
    result = result if result is not None else {}
    result["count"] = len(messages)
    result["elapsedMs"] = elapsed
    result["bodies"] = ",".join(m["Body"] for m in messages)
    result["sorted"] = ",".join(sorted(m["Body"] for m in messages))
    result["_msgs"] = [{"handle": m["ReceiptHandle"], "id": m["MessageId"]} for m in messages]
    if messages:
        m = messages[0]
        result.update(body=m["Body"], id=m["MessageId"], handle=m["ReceiptHandle"], md5=m["MD5OfBody"],
                      md5Ok=str(md5(m["Body"]) == m["MD5OfBody"]).lower(), size=len(m["Body"]))
        for k, v in (m.get("Attributes") or {}).items():
            result["attr_" + k] = v
        msg_attrs_out(m, result)
        try:
            doc = json.loads(m["Body"])
            for k, v in (doc.items() if isinstance(doc, dict) else []):
                if isinstance(v, str):
                    result["json_" + k] = v
        except ValueError:
            pass
    return result


def op_createQueue(p):
    attrs = pairs(p.get("attrs"))
    if p.get("fifo") == "true":
        attrs["FifoQueue"] = "true"
    if p.get("redriveArn"):
        attrs["RedrivePolicy"] = json.dumps({"deadLetterTargetArn": p["redriveArn"], "maxReceiveCount": int(p["redriveMax"])})
    kw = {"QueueName": p["name"]}
    if attrs:
        kw["Attributes"] = attrs
    url = CLIENTS["sqs"].create_queue(**kw)["QueueUrl"]
    return {"url": url, "name": p["name"]}


def op_getQueueUrl(p):
    return {"url": CLIENTS["sqs"].get_queue_url(QueueName=p["name"])["QueueUrl"]}


def op_deleteQueue(p):
    CLIENTS["sqs"].delete_queue(QueueUrl=p["queue"])
    return {}


def op_purgeQueue(p):
    CLIENTS["sqs"].purge_queue(QueueUrl=p["queue"])
    return {}


def op_listQueues(p):
    urls = CLIENTS["sqs"].list_queues(QueueNamePrefix=p["prefix"]).get("QueueUrls", [])
    return {"count": len(urls), "names": ",".join(names_of(urls))}


def op_getQueueAttributes(p):
    names = p["names"].split(",")
    r = CLIENTS["sqs"].get_queue_attributes(QueueUrl=p["queue"], AttributeNames=names).get("Attributes", {})
    return {"attr_" + k: v for k, v in r.items()}


def op_setQueueAttributes(p):
    CLIENTS["sqs"].set_queue_attributes(QueueUrl=p["queue"], Attributes=pairs(p["attrs"]))
    return {}


def op_tagQueue(p):
    CLIENTS["sqs"].tag_queue(QueueUrl=p["queue"], Tags=pairs(p["tags"]))
    return {}


def op_untagQueue(p):
    CLIENTS["sqs"].untag_queue(QueueUrl=p["queue"], TagKeys=p["keys"].split(","))
    return {}


def op_listQueueTags(p):
    return {"tag_" + k: v for k, v in CLIENTS["sqs"].list_queue_tags(QueueUrl=p["queue"]).get("Tags", {}).items()}


def send_args(p, body):
    kw = {"MessageBody": body}
    if p.get("delay") not in (None, ""):
        kw["DelaySeconds"] = int(p["delay"])
    if p.get("msgattrs"):
        kw["MessageAttributes"] = msg_attrs_in(p["msgattrs"])
    if p.get("group"):
        kw["MessageGroupId"] = p["group"]
    if p.get("dedup"):
        kw["MessageDeduplicationId"] = p["dedup"]
    return kw


def sent(r, body):
    return {"id": r["MessageId"], "md5": r["MD5OfMessageBody"], "md5Ok": str(md5(body) == r["MD5OfMessageBody"]).lower(),
            "sequence": r.get("SequenceNumber", "")}


def op_send(p):
    return sent(CLIENTS["sqs"].send_message(QueueUrl=p["queue"], **send_args(p, p["body"])), p["body"])


def op_sendBig(p):
    body = "x" * int(p["size"])
    return sent(CLIENTS["sqs"].send_message(QueueUrl=p["queue"], MessageBody=body), body)


def op_sendBatch(p):
    delays = p["delays"].split(",") if p.get("delays") else []
    entries = []
    for i, body in enumerate(p["bodies"].split(",")):
        e = {"Id": str(i), "MessageBody": body}
        if delays:
            e["DelaySeconds"] = int(delays[i])
        if p.get("group"):
            e["MessageGroupId"] = p["group"]
        if p.get("dedupPrefix"):
            e["MessageDeduplicationId"] = p["dedupPrefix"] + str(i)
        entries.append(e)
    r = CLIENTS["sqs"].send_message_batch(QueueUrl=p["queue"], Entries=entries)
    return {"ok": len(r.get("Successful", [])), "failed": len(r.get("Failed", []))}


def op_sendBatchTooMany(p):
    entries = [{"Id": str(i), "MessageBody": "m%d" % i} for i in range(int(p["count"]))]
    CLIENTS["sqs"].send_message_batch(QueueUrl=p["queue"], Entries=entries)
    return {}


def op_receive(p):
    kw = {"QueueUrl": p["queue"], "MaxNumberOfMessages": int(p.get("max", "1")), "WaitTimeSeconds": int(p.get("wait", "0"))}
    if p.get("visibility"):
        kw["VisibilityTimeout"] = int(p["visibility"])
    if p.get("attrs"):
        kw["AttributeNames"] = p["attrs"].split(",")
    if p.get("msgattrs"):
        kw["MessageAttributeNames"] = p["msgattrs"].split(",")
    t0 = time.time()
    r = CLIENTS["sqs"].receive_message(**kw)
    return received(r.get("Messages", []), int((time.time() - t0) * 1000))


def op_drainAll(p):
    want, deadline = int(p["count"]), time.time() + int(p["timeout"])
    got, result = [], {}
    while len(got) < want and time.time() < deadline:
        kw = {"QueueUrl": p["queue"], "MaxNumberOfMessages": 10, "WaitTimeSeconds": 1}
        if p.get("msgattrs"):
            kw["MessageAttributeNames"] = p["msgattrs"].split(",")
        for m in CLIENTS["sqs"].receive_message(**kw).get("Messages", []):
            got.append(m)
            msg_attrs_out(m, result)
            CLIENTS["sqs"].delete_message(QueueUrl=p["queue"], ReceiptHandle=m["ReceiptHandle"])
    result["count"] = len(got)
    result["bodies"] = ",".join(m["Body"] for m in got)
    return result


def op_deleteMessage(p):
    CLIENTS["sqs"].delete_message(QueueUrl=p["queue"], ReceiptHandle=p["handle"])
    return {}


def op_changeVisibility(p):
    CLIENTS["sqs"].change_message_visibility(QueueUrl=p["queue"], ReceiptHandle=p["handle"], VisibilityTimeout=int(p["seconds"]))
    return {}


def handles_of(p, variables):
    return [m["handle"] for m in variables[p["from"].lstrip("$")]["_msgs"]]


def op_deleteAll(p, variables):
    entries = [{"Id": str(i), "ReceiptHandle": h} for i, h in enumerate(handles_of(p, variables))]
    r = CLIENTS["sqs"].delete_message_batch(QueueUrl=p["queue"], Entries=entries)
    return {"ok": len(r.get("Successful", [])), "failed": len(r.get("Failed", []))}


def op_changeVisibilityAll(p, variables):
    entries = [{"Id": str(i), "ReceiptHandle": h, "VisibilityTimeout": int(p["seconds"])} for i, h in enumerate(handles_of(p, variables))]
    r = CLIENTS["sqs"].change_message_visibility_batch(QueueUrl=p["queue"], Entries=entries)
    return {"ok": len(r.get("Successful", [])), "failed": len(r.get("Failed", []))}


def op_allowSnsToSend(p):
    policy = {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
              "Action": "sqs:SendMessage", "Resource": p["queueArn"], "Condition": {"ArnEquals": {"aws:SourceArn": p["topic"]}}}]}
    CLIENTS["sqs"].set_queue_attributes(QueueUrl=p["queue"], Attributes={"Policy": json.dumps(policy)})
    return {}


def op_sleep(p):
    time.sleep(float(p["seconds"]))
    return {}


def op_useCredentials(p):
    make_clients(CREDS[p["name"]])
    return {}


def op_createTopic(p):
    kw = {"Name": p["name"]}
    if p.get("fifo") == "true":
        kw["Attributes"] = {"FifoTopic": "true"}
    return {"arn": CLIENTS["sns"].create_topic(**kw)["TopicArn"]}


def op_deleteTopic(p):
    CLIENTS["sns"].delete_topic(TopicArn=p["topic"])
    return {}


def op_listTopics(p):
    arns, token = [], None
    while True:
        r = CLIENTS["sns"].list_topics(**({"NextToken": token} if token else {}))
        arns += [t["TopicArn"] for t in r.get("Topics", [])]
        token = r.get("NextToken")
        if not token:
            break
    names = sorted(a.rsplit(":", 1)[-1] for a in arns if a.rsplit(":", 1)[-1].startswith(p["prefix"]))
    return {"count": len(names), "names": ",".join(names)}


def op_getTopicAttributes(p):
    return {"attr_" + k: v for k, v in CLIENTS["sns"].get_topic_attributes(TopicArn=p["topic"])["Attributes"].items()}


def op_subscribe(p):
    kw = {"TopicArn": p["topic"], "Protocol": p["protocol"], "Endpoint": p["endpoint"], "ReturnSubscriptionArn": True}
    if p.get("attrs"):
        kw["Attributes"] = pairs(p["attrs"])
    return {"arn": CLIENTS["sns"].subscribe(**kw)["SubscriptionArn"]}


def op_unsubscribe(p):
    CLIENTS["sns"].unsubscribe(SubscriptionArn=p["sub"])
    return {}


def op_listSubscriptions(p):
    subs = CLIENTS["sns"].list_subscriptions_by_topic(TopicArn=p["topic"]).get("Subscriptions", [])
    return {"count": len(subs), "endpoints": ",".join(sorted(s["Endpoint"] for s in subs)),
            "protocols": ",".join(sorted(set(s["Protocol"] for s in subs)))}


def op_getSubscriptionAttributes(p):
    return {"attr_" + k: v for k, v in CLIENTS["sns"].get_subscription_attributes(SubscriptionArn=p["sub"])["Attributes"].items()}


def op_setSubscriptionAttributes(p):
    for k, v in pairs(p["attrs"]).items():
        CLIENTS["sns"].set_subscription_attributes(SubscriptionArn=p["sub"], AttributeName=k, AttributeValue=v)
    return {}


def op_publish(p):
    kw = {"TopicArn": p["topic"], "Message": p["message"]}
    if p.get("subject"):
        kw["Subject"] = p["subject"]
    if p.get("msgattrs"):
        kw["MessageAttributes"] = msg_attrs_in(p["msgattrs"])
    if p.get("group"):
        kw["MessageGroupId"] = p["group"]
    if p.get("dedup"):
        kw["MessageDeduplicationId"] = p["dedup"]
    return {"messageId": CLIENTS["sns"].publish(**kw)["MessageId"]}


def op_publishBatch(p):
    entries = [{"Id": str(i), "Message": m} for i, m in enumerate(p["messages"].split(","))]
    r = CLIENTS["sns"].publish_batch(TopicArn=p["topic"], PublishBatchRequestEntries=entries)
    return {"ok": len(r.get("Successful", [])), "failed": len(r.get("Failed", []))}


NEEDS_VARIABLES = ("deleteAll", "changeVisibilityAll")


def run_file(path, variables):
    counts = {"PASS": 0, "FAIL": 0, "CONNECT": 0}
    name = path.replace("\\", "/").rsplit("/", 1)[-1]
    for number, line in enumerate(open(path, encoding="utf-8"), 1):
        text = line.strip()
        if not text or text.startswith("#"):
            continue
        status, detail = "PASS", ""
        op = text.split()[0]
        try:
            op, params, raw, expects = parse_line(text, variables)
            expected_error = next((w for k, o, w in expects if k == "error"), None)
            try:
                fn = globals()["op_" + op]
                result = fn(params, variables) if op in NEEDS_VARIABLES else fn(params)
                if expected_error is not None:
                    status, detail = "FAIL", "expected error %s but the call worked" % expected_error
                else:
                    problem = check(result, expects)
                    if problem:
                        status, detail = "FAIL", problem
                if "as" in params:
                    variables[params["as"]] = result
            except ClientError as e:
                code = e.response.get("Error", {}).get("Code", "")
                if expected_error is not None and code in expected_error.split("|"):
                    pass
                else:
                    status, detail = "FAIL", "error %s: %s" % (code, str(e)[:160])
        except Exception as e:  # noqa: BLE001 - anything else is a failure of the line
            if UNREACHABLE:
                status, detail = "CONNECT", type(e).__name__
            else:
                status, detail = "FAIL", "%s: %s" % (type(e).__name__, str(e)[:200])
        counts[status] += 1
        print("RESULT %s %s:%d %s%s" % (status, name, number, op, (" :: " + detail) if detail else ""), flush=True)
    return counts


def main():
    variables, total = {}, {"PASS": 0, "FAIL": 0, "CONNECT": 0}
    for path in ARGS["scenario"]:
        for k, v in run_file(path, variables).items():
            total[k] += v
    print("SUMMARY lang=%s variant=%s sdk=boto3-%s pass=%d fail=%d connect=%d" % (
        ARGS["lang"], ARGS.get("variant", ""), boto3.__version__, total["PASS"], total["FAIL"], total["CONNECT"]), flush=True)


main()
