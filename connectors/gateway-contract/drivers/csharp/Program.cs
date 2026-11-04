// The C# driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
// runs each line through AWSSDK.SQS and AWSSDK.SimpleNotificationService, the way an application would. Prints one
// RESULT line per scenario line.
using System.Diagnostics;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Amazon;
using Amazon.Runtime;
using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;
using Amazon.SQS;
using Amazon.SQS.Model;
using SnsAttr = Amazon.SimpleNotificationService.Model.MessageAttributeValue;
using SqsAttr = Amazon.SQS.Model.MessageAttributeValue;

var args2 = new Dictionary<string, string>();
var scenarios = new List<string>();
var creds = new Dictionary<string, (string, string)>();
for (int i = 0; i + 1 < args.Length; i += 2)
{
    var key = args[i][2..];
    if (key == "scenario") scenarios.Add(args[i + 1]);
    else if (key == "cred")
    {
        var (name, pair) = Split2(args[i + 1], '=');
        var (ak, sk) = Split2(pair, ':');
        creds[name] = (ak, sk);
    }
    else args2[key] = args[i + 1];
}

static (string, string) Split2(string s, char c)
{
    int k = s.IndexOf(c);
    return k < 0 ? (s, "") : (s[..k], s[(k + 1)..]);
}

string A(string k, string d = "") => args2.TryGetValue(k, out var v) ? v : d;

bool Reachable(string url)
{
    var u = new Uri(url);
    try
    {
        using var c = new TcpClient();
        return c.ConnectAsync(u.Host, u.Port).Wait(3000) && c.Connected;
    }
    catch { return false; }
}

AmazonSQSClient sqs = null!;
AmazonSimpleNotificationServiceClient sns = null!;

void MakeClients(AWSCredentials? credentials)
{
    var region = RegionEndpoint.GetBySystemName(A("region", "us-east-1"));
    var code = A("endpoint-mode") == "code";
    var sqsConfig = new AmazonSQSConfig { RegionEndpoint = region, MaxErrorRetry = 0, Timeout = TimeSpan.FromSeconds(60) };
    var snsConfig = new AmazonSimpleNotificationServiceConfig { RegionEndpoint = region, MaxErrorRetry = 0, Timeout = TimeSpan.FromSeconds(60) };
    if (code) { sqsConfig.ServiceURL = A("sqs-endpoint"); snsConfig.ServiceURL = A("sns-endpoint"); }
    sqs = credentials == null ? new AmazonSQSClient(sqsConfig) : new AmazonSQSClient(credentials, sqsConfig);
    sns = credentials == null ? new AmazonSimpleNotificationServiceClient(snsConfig) : new AmazonSimpleNotificationServiceClient(credentials, snsConfig);
}
MakeClients(null);

// ---- the scenario language -----------------------------------------------------------------------------------------

var variable = new Regex(@"\{p\}|\{lang\}|\{prev\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)");
var expectRe = new Regex(@"^([A-Za-z0-9_.]+)(>=|<=|!=|=|~)(.*)$", RegexOptions.Singleline);

string Unquote(string s)
{
    try { return Uri.UnescapeDataString(s); } catch { return s; }
}

string Expand(string value, Dictionary<string, Result> vars)
{
    var sb = new StringBuilder();
    int last = 0;
    foreach (Match m in variable.Matches(value))
    {
        sb.Append(Unquote(value[last..m.Index]));
        sb.Append(m.Value switch
        {
            "{p}" => A("prefix"),
            "{lang}" => A("lang"),
            "{prev}" => A("prev"),
            _ => vars.TryGetValue(m.Groups[1].Value, out var r) && r.M.TryGetValue(m.Groups[2].Value, out var v) ? v : "",
        });
        last = m.Index + m.Length;
    }
    sb.Append(Unquote(value[last..]));
    return sb.ToString();
}

(string op, Dictionary<string, string> p, List<(string key, string op, string want)> expects) ParseLine(string text, Dictionary<string, Result> vars)
{
    int at = text.IndexOf(" => ", StringComparison.Ordinal);
    var head = at < 0 ? text : text[..at];
    var tail = at < 0 ? "" : text[(at + 4)..];
    var parts = head.Split(' ', StringSplitOptions.RemoveEmptyEntries);
    var p = new Dictionary<string, string>();
    foreach (var part in parts.Skip(1))
    {
        var (k, v) = Split2(part, '=');
        p[k] = Expand(v, vars);
    }
    var expects = new List<(string, string, string)>();
    foreach (var token in tail.Split(' ', StringSplitOptions.RemoveEmptyEntries))
    {
        var m = expectRe.Match(token);
        expects.Add((m.Groups[1].Value, m.Groups[2].Value, Expand(m.Groups[3].Value, vars)));
    }
    return (parts[0], p, expects);
}

string? Check(Result r, List<(string key, string op, string want)> expects)
{
    foreach (var (key, op, want) in expects)
    {
        var got = r.M.TryGetValue(key, out var g) ? g : "";
        bool ok = op switch
        {
            "=" => want == "*" ? got != "" : got == want,
            "!=" => got != want,
            "~" => got.Contains(want, StringComparison.Ordinal),
            ">=" => double.TryParse(got, out var a) && double.TryParse(want, out var b) && a >= b,
            _ => double.TryParse(got, out var a2) && double.TryParse(want, out var b2) && a2 <= b2,
        };
        if (!ok) return $"{key}{op}{want} but was '{(got.Length > 200 ? got[..200] : got)}'";
    }
    return null;
}

// ---- the operations --------------------------------------------------------------------------------------------------

string Md5(string s) => Convert.ToHexString(MD5.HashData(Encoding.UTF8.GetBytes(s))).ToLowerInvariant();

Dictionary<string, string> Pairs(string t)
{
    var d = new Dictionary<string, string>();
    if (t == "") return d;
    foreach (var item in t.Split(',')) { var (k, v) = Split2(item, ':'); d[k] = v; }
    return d;
}

static string[] Three(string item)
{
    int a = item.IndexOf(':'), b = item.IndexOf(':', a + 1);
    return new[] { item[..a], item[(a + 1)..b], item[(b + 1)..] };
}

Dictionary<string, SqsAttr> SqsAttrsIn(string t)
{
    var d = new Dictionary<string, SqsAttr>();
    if (t == "") return d;
    foreach (var item in t.Split(','))
    {
        var x = Three(item);
        d[x[0]] = x[1] == "Binary"
            ? new SqsAttr { DataType = x[1], BinaryValue = new MemoryStream(Convert.FromBase64String(x[2][4..])) }
            : new SqsAttr { DataType = x[1], StringValue = x[2] };
    }
    return d;
}

Dictionary<string, SnsAttr> SnsAttrsIn(string t)
{
    var d = new Dictionary<string, SnsAttr>();
    if (t == "") return d;
    foreach (var item in t.Split(','))
    {
        var x = Three(item);
        d[x[0]] = x[1] == "Binary"
            ? new SnsAttr { DataType = x[1], BinaryValue = new MemoryStream(Convert.FromBase64String(x[2][4..])) }
            : new SnsAttr { DataType = x[1], StringValue = x[2] };
    }
    return d;
}

void MsgAttrsOut(Message m, Dictionary<string, string> r)
{
    foreach (var (name, a) in m.MessageAttributes ?? new())
    {
        r["msgattr_" + name] = a.DataType.StartsWith("Binary") ? "b64:" + Convert.ToBase64String(a.BinaryValue.ToArray()) : a.StringValue;
        r["msgattrtype_" + name] = a.DataType;
    }
}

static string[] NamesOf(IEnumerable<string> urls) => urls.Select(u => u.TrimEnd('/').Split('/').Last()).OrderBy(x => x, StringComparer.Ordinal).ToArray();

Result Received(List<Message> msgs, long elapsed)
{
    var r = new Result();
    r.M["count"] = msgs.Count.ToString();
    r.M["elapsedMs"] = elapsed.ToString();
    r.M["bodies"] = string.Join(",", msgs.Select(m => m.Body));
    r.M["sorted"] = string.Join(",", msgs.Select(m => m.Body).OrderBy(x => x, StringComparer.Ordinal));
    r.Handles = msgs.Select(m => m.ReceiptHandle).ToList();
    if (msgs.Count > 0)
    {
        var m = msgs[0];
        r.M["body"] = m.Body; r.M["id"] = m.MessageId; r.M["handle"] = m.ReceiptHandle; r.M["md5"] = m.MD5OfBody;
        r.M["md5Ok"] = (Md5(m.Body) == m.MD5OfBody).ToString().ToLowerInvariant();
        r.M["size"] = new System.Globalization.StringInfo(m.Body).LengthInTextElements.ToString();
        foreach (var (k, v) in m.Attributes ?? new()) r.M["attr_" + k] = v;
        MsgAttrsOut(m, r.M);
        try
        {
            using var doc = JsonDocument.Parse(m.Body);
            if (doc.RootElement.ValueKind == JsonValueKind.Object)
                foreach (var prop in doc.RootElement.EnumerateObject())
                    if (prop.Value.ValueKind == JsonValueKind.String) r.M["json_" + prop.Name] = prop.Value.GetString()!;
        }
        catch (JsonException) { }
    }
    return r;
}

Result R(params string[] kv)
{
    var r = new Result();
    for (int i = 0; i + 1 < kv.Length; i += 2) r.M[kv[i]] = kv[i + 1];
    return r;
}

Result Sent(SendMessageResponse o, string body) =>
    R("id", o.MessageId, "md5", o.MD5OfMessageBody, "md5Ok", (Md5(body) == o.MD5OfMessageBody).ToString().ToLowerInvariant(), "sequence", o.SequenceNumber ?? "");

Result Attrs(Dictionary<string, string> d) { var r = new Result(); foreach (var (k, v) in d) r.M["attr_" + k] = v; return r; }

List<string> HandlesOf(Dictionary<string, string> p, Dictionary<string, Result> vars) => vars.TryGetValue(p["from"].TrimStart('$'), out var r) ? r.Handles : new();

async Task<Result> Op(string name, Dictionary<string, string> p, Dictionary<string, Result> vars)
{
    string G(string k) => p.TryGetValue(k, out var v) ? v : "";
    switch (name)
    {
        case "createQueue":
        {
            var attrs = Pairs(G("attrs"));
            if (G("fifo") == "true") attrs["FifoQueue"] = "true";
            if (G("redriveArn") != "") attrs["RedrivePolicy"] = $"{{\"deadLetterTargetArn\":\"{G("redriveArn")}\",\"maxReceiveCount\":{G("redriveMax")}}}";
            var req = new CreateQueueRequest { QueueName = G("name") };
            if (attrs.Count > 0) req.Attributes = attrs;
            return R("url", (await sqs.CreateQueueAsync(req)).QueueUrl, "name", G("name"));
        }
        case "getQueueUrl": return R("url", (await sqs.GetQueueUrlAsync(G("name"))).QueueUrl);
        case "deleteQueue": await sqs.DeleteQueueAsync(G("queue")); return R();
        case "purgeQueue": await sqs.PurgeQueueAsync(G("queue")); return R();
        case "listQueues":
        {
            var urls = (await sqs.ListQueuesAsync(new ListQueuesRequest { QueueNamePrefix = G("prefix") })).QueueUrls ?? new();
            return R("count", urls.Count.ToString(), "names", string.Join(",", NamesOf(urls)));
        }
        case "getQueueAttributes":
            return Attrs((await sqs.GetQueueAttributesAsync(new GetQueueAttributesRequest { QueueUrl = G("queue"), AttributeNames = G("names").Split(',').ToList() })).Attributes);
        case "setQueueAttributes": await sqs.SetQueueAttributesAsync(G("queue"), Pairs(G("attrs"))); return R();
        case "tagQueue": await sqs.TagQueueAsync(new TagQueueRequest { QueueUrl = G("queue"), Tags = Pairs(G("tags")) }); return R();
        case "untagQueue": await sqs.UntagQueueAsync(new UntagQueueRequest { QueueUrl = G("queue"), TagKeys = G("keys").Split(',').ToList() }); return R();
        case "listQueueTags":
        {
            var r = new Result();
            foreach (var (k, v) in (await sqs.ListQueueTagsAsync(new ListQueueTagsRequest { QueueUrl = G("queue") })).Tags ?? new()) r.M["tag_" + k] = v;
            return r;
        }
        case "send":
        case "sendBig":
        {
            var body = name == "sendBig" ? new string('x', int.Parse(G("size"))) : G("body");
            var req = new SendMessageRequest { QueueUrl = G("queue"), MessageBody = body };
            if (name == "send")
            {
                if (G("delay") != "") req.DelaySeconds = int.Parse(G("delay"));
                if (G("msgattrs") != "") req.MessageAttributes = SqsAttrsIn(G("msgattrs"));
                if (G("group") != "") req.MessageGroupId = G("group");
                if (G("dedup") != "") req.MessageDeduplicationId = G("dedup");
            }
            return Sent(await sqs.SendMessageAsync(req), body);
        }
        case "sendBatch":
        case "sendBatchTooMany":
        {
            var bodies = name == "sendBatch" ? G("bodies").Split(',').ToList() : Enumerable.Range(0, int.Parse(G("count"))).Select(i => "m" + i).ToList();
            var delays = G("delays") == "" ? new string[0] : G("delays").Split(',');
            var entries = new List<SendMessageBatchRequestEntry>();
            for (int i = 0; i < bodies.Count; i++)
            {
                var e = new SendMessageBatchRequestEntry { Id = i.ToString(), MessageBody = bodies[i] };
                if (delays.Length > 0) e.DelaySeconds = int.Parse(delays[i]);
                if (G("group") != "") e.MessageGroupId = G("group");
                if (G("dedupPrefix") != "") e.MessageDeduplicationId = G("dedupPrefix") + i;
                entries.Add(e);
            }
            var o = await sqs.SendMessageBatchAsync(new SendMessageBatchRequest { QueueUrl = G("queue"), Entries = entries });
            return R("ok", (o.Successful?.Count ?? 0).ToString(), "failed", (o.Failed?.Count ?? 0).ToString());
        }
        case "receive":
        {
            var req = new ReceiveMessageRequest { QueueUrl = G("queue"), MaxNumberOfMessages = G("max") == "" ? 1 : int.Parse(G("max")), WaitTimeSeconds = G("wait") == "" ? 0 : int.Parse(G("wait")) };
            if (G("visibility") != "") req.VisibilityTimeout = int.Parse(G("visibility"));
            if (G("attrs") != "") req.MessageSystemAttributeNames = G("attrs").Split(',').ToList();
            if (G("msgattrs") != "") req.MessageAttributeNames = G("msgattrs").Split(',').ToList();
            var sw = Stopwatch.StartNew();
            var o = await sqs.ReceiveMessageAsync(req);
            return Received(o.Messages ?? new(), sw.ElapsedMilliseconds);
        }
        case "drainAll":
        {
            int want = int.Parse(G("count"));
            var deadline = DateTime.UtcNow.AddSeconds(int.Parse(G("timeout")));
            var r = new Result();
            var bodies = new List<string>();
            while (bodies.Count < want && DateTime.UtcNow < deadline)
            {
                var req = new ReceiveMessageRequest { QueueUrl = G("queue"), MaxNumberOfMessages = 10, WaitTimeSeconds = 1 };
                if (G("msgattrs") != "") req.MessageAttributeNames = G("msgattrs").Split(',').ToList();
                foreach (var m in (await sqs.ReceiveMessageAsync(req)).Messages ?? new())
                {
                    bodies.Add(m.Body);
                    MsgAttrsOut(m, r.M);
                    await sqs.DeleteMessageAsync(G("queue"), m.ReceiptHandle);
                }
            }
            r.M["count"] = bodies.Count.ToString();
            r.M["bodies"] = string.Join(",", bodies);
            return r;
        }
        case "deleteMessage": await sqs.DeleteMessageAsync(G("queue"), G("handle")); return R();
        case "changeVisibility": await sqs.ChangeMessageVisibilityAsync(G("queue"), G("handle"), int.Parse(G("seconds"))); return R();
        case "deleteAll":
        {
            var entries = HandlesOf(p, vars).Select((h, i) => new DeleteMessageBatchRequestEntry { Id = i.ToString(), ReceiptHandle = h }).ToList();
            var o = await sqs.DeleteMessageBatchAsync(new DeleteMessageBatchRequest { QueueUrl = G("queue"), Entries = entries });
            return R("ok", (o.Successful?.Count ?? 0).ToString(), "failed", (o.Failed?.Count ?? 0).ToString());
        }
        case "changeVisibilityAll":
        {
            var entries = HandlesOf(p, vars).Select((h, i) => new ChangeMessageVisibilityBatchRequestEntry { Id = i.ToString(), ReceiptHandle = h, VisibilityTimeout = int.Parse(G("seconds")) }).ToList();
            var o = await sqs.ChangeMessageVisibilityBatchAsync(new ChangeMessageVisibilityBatchRequest { QueueUrl = G("queue"), Entries = entries });
            return R("ok", (o.Successful?.Count ?? 0).ToString(), "failed", (o.Failed?.Count ?? 0).ToString());
        }
        case "allowSnsToSend":
        {
            var policy = $"{{\"Version\":\"2012-10-17\",\"Statement\":[{{\"Effect\":\"Allow\",\"Principal\":{{\"Service\":\"sns.amazonaws.com\"}},\"Action\":\"sqs:SendMessage\",\"Resource\":\"{G("queueArn")}\",\"Condition\":{{\"ArnEquals\":{{\"aws:SourceArn\":\"{G("topic")}\"}}}}}}]}}";
            await sqs.SetQueueAttributesAsync(G("queue"), new Dictionary<string, string> { ["Policy"] = policy });
            return R();
        }
        case "sleep": await Task.Delay((int)(double.Parse(G("seconds")) * 1000)); return R();
        case "useCredentials": { var (ak, sk) = creds[G("name")]; MakeClients(new BasicAWSCredentials(ak, sk)); return R(); }
        case "createTopic":
        {
            var req = new CreateTopicRequest { Name = G("name") };
            if (G("fifo") == "true") req.Attributes = new Dictionary<string, string> { ["FifoTopic"] = "true" };
            return R("arn", (await sns.CreateTopicAsync(req)).TopicArn);
        }
        case "deleteTopic": await sns.DeleteTopicAsync(G("topic")); return R();
        case "listTopics":
        {
            var names = new List<string>();
            string? token = null;
            do
            {
                var o = await sns.ListTopicsAsync(new ListTopicsRequest { NextToken = token });
                names.AddRange(o.Topics.Select(t => t.TopicArn.Split(':').Last()).Where(n => n.StartsWith(G("prefix"), StringComparison.Ordinal)));
                token = string.IsNullOrEmpty(o.NextToken) ? null : o.NextToken;
            } while (token != null);
            names.Sort(StringComparer.Ordinal);
            return R("count", names.Count.ToString(), "names", string.Join(",", names));
        }
        case "getTopicAttributes": return Attrs((await sns.GetTopicAttributesAsync(G("topic"))).Attributes);
        case "subscribe":
        {
            var req = new SubscribeRequest { TopicArn = G("topic"), Protocol = G("protocol"), Endpoint = G("endpoint"), ReturnSubscriptionArn = true };
            if (G("attrs") != "") req.Attributes = Pairs(G("attrs"));
            return R("arn", (await sns.SubscribeAsync(req)).SubscriptionArn);
        }
        case "unsubscribe": await sns.UnsubscribeAsync(G("sub")); return R();
        case "listSubscriptions":
        {
            var subs = (await sns.ListSubscriptionsByTopicAsync(G("topic"))).Subscriptions ?? new();
            return R("count", subs.Count.ToString(), "endpoints", string.Join(",", subs.Select(s => s.Endpoint).OrderBy(x => x, StringComparer.Ordinal)),
                "protocols", string.Join(",", subs.Select(s => s.Protocol).Distinct().OrderBy(x => x, StringComparer.Ordinal)));
        }
        case "getSubscriptionAttributes": return Attrs((await sns.GetSubscriptionAttributesAsync(G("sub"))).Attributes);
        case "setSubscriptionAttributes":
            foreach (var (k, v) in Pairs(G("attrs"))) await sns.SetSubscriptionAttributesAsync(G("sub"), k, v);
            return R();
        case "publish":
        {
            var req = new PublishRequest { TopicArn = G("topic"), Message = G("message") };
            if (G("subject") != "") req.Subject = G("subject");
            if (G("msgattrs") != "") req.MessageAttributes = SnsAttrsIn(G("msgattrs"));
            if (G("group") != "") req.MessageGroupId = G("group");
            if (G("dedup") != "") req.MessageDeduplicationId = G("dedup");
            return R("messageId", (await sns.PublishAsync(req)).MessageId);
        }
        case "publishBatch":
        {
            var entries = G("messages").Split(',').Select((m, i) => new PublishBatchRequestEntry { Id = i.ToString(), Message = m }).ToList();
            var o = await sns.PublishBatchAsync(new PublishBatchRequest { TopicArn = G("topic"), PublishBatchRequestEntries = entries });
            return R("ok", (o.Successful?.Count ?? 0).ToString(), "failed", (o.Failed?.Count ?? 0).ToString());
        }
    }
    throw new InvalidOperationException("operation " + name + " is not implemented");
}

bool unreachable = !(Reachable(A("sqs-endpoint")) && Reachable(A("sns-endpoint")));
var counts = new Dictionary<string, int> { ["PASS"] = 0, ["FAIL"] = 0, ["CONNECT"] = 0 };
var variables = new Dictionary<string, Result>();

foreach (var path in scenarios)
{
    var file = Path.GetFileName(path);
    var lines = File.ReadAllLines(path, Encoding.UTF8);
    for (int n = 0; n < lines.Length; n++)
    {
        var text = lines[n].Trim();
        if (text == "" || text.StartsWith('#')) continue;
        string status = "PASS", detail = "", opName = text.Split(' ')[0];
        try
        {
            var (op, p, expects) = ParseLine(text, variables);
            opName = op;
            var expected = expects.Where(e => e.key == "error").Select(e => e.want).FirstOrDefault();
            try
            {
                var result = await Op(op, p, variables);
                if (expected != null) { status = "FAIL"; detail = $"expected error {expected} but the call worked"; }
                else { var problem = Check(result, expects); if (problem != null) { status = "FAIL"; detail = problem; } }
                if (p.TryGetValue("as", out var asName)) variables[asName] = result;
            }
            catch (AmazonServiceException e) when (e.StatusCode != 0 && !string.IsNullOrEmpty(e.ErrorCode))
            {
                var code = e.ErrorCode[(e.ErrorCode.LastIndexOf('#') + 1)..];
                if (expected == null || !expected.Split('|').Contains(code))
                { status = "FAIL"; detail = $"error {code}: {(e.Message.Length > 160 ? e.Message[..160] : e.Message)}"; }
            }
        }
        catch (Exception e)
        {
            if (unreachable) { status = "CONNECT"; detail = e.GetType().Name; }
            else { status = "FAIL"; detail = $"{e.GetType().Name}: {(e.Message.Length > 200 ? e.Message[..200] : e.Message)}"; }
        }
        counts[status]++;
        Console.WriteLine($"RESULT {status} {file}:{n + 1} {opName}{(detail == "" ? "" : " :: " + detail)}");
    }
}
var sdk = typeof(AmazonSQSClient).Assembly.GetName().Version;
Console.WriteLine($"SUMMARY lang={A("lang")} variant={A("variant")} sdk=AWSSDK.SQS-{sdk} pass={counts["PASS"]} fail={counts["FAIL"]} connect={counts["CONNECT"]}");

class Result
{
    public Dictionary<string, string> M = new();
    public List<string> Handles = new();
}
