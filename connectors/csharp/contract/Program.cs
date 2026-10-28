// The C# driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and runs
// each line against a real Taba through the connector, in the style of the language.
//
//   dotnet run -- --url http://host:8080 --token TOKEN --prefix ct-csharp-0101 --lang csharp \
//       --scenario ../../contract/scenarios/core.scn

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Taba;

namespace Taba.Contract
{
    internal static class Program
    {
        private static readonly Dictionary<string, string> Args = new Dictionary<string, string>();
        private static readonly List<string> Scenarios = new List<string>();
        private static readonly Regex Variable = new Regex(@"\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)");

        private delegate Task<Dictionary<string, string>> Operation(TabaClient client, Dictionary<string, string> p);

        private sealed class Expect
        {
            public string Key;
            public char Op;
            public string Value;
        }

        // ---- the scenario language ----------------------------------------------------------------------

        /// <summary>Decodes the %XX runs as UTF-8 (a "+" stays as it is).</summary>
        private static string PercentDecode(string text)
        {
            byte[] bytes = Encoding.UTF8.GetBytes(text);
            var output = new List<byte>();
            for (int i = 0; i < bytes.Length; i++)
            {
                if (bytes[i] == (byte)'%' && i + 2 < bytes.Length)
                {
                    output.Add(Convert.ToByte(Encoding.ASCII.GetString(bytes, i + 1, 2), 16));
                    i += 2;
                }
                else
                {
                    output.Add(bytes[i]);
                }
            }
            return Encoding.UTF8.GetString(output.ToArray());
        }

        /// <summary>Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched.</summary>
        private static string Expand(string value, Dictionary<string, Dictionary<string, string>> variables)
        {
            var output = new StringBuilder();
            int last = 0;
            foreach (Match m in Variable.Matches(value))
            {
                output.Append(PercentDecode(value.Substring(last, m.Index - last)));
                if (m.Value == "{p}")
                {
                    output.Append(Args["prefix"]);
                }
                else if (m.Value == "{lang}")
                {
                    output.Append(Args["lang"]);
                }
                else if (variables.TryGetValue(m.Groups[1].Value, out var v) && v.TryGetValue(m.Groups[2].Value, out var field))
                {
                    output.Append(field);
                }
                last = m.Index + m.Length;
            }
            return output.Append(PercentDecode(value.Substring(last))).ToString();
        }

        // ---- helpers of the operations ------------------------------------------------------------------

        private static int? Int(Dictionary<string, string> p, string key) => p.TryGetValue(key, out var v) ? int.Parse(v) : (int?)null;

        private static long? Long(Dictionary<string, string> p, string key) => p.TryGetValue(key, out var v) ? long.Parse(v) : (long?)null;

        private static bool? Flag(Dictionary<string, string> p, string key) => p.TryGetValue(key, out var v) ? v == "true" : (bool?)null;

        private static List<string> Items(string value) => value.Split(',').Where(s => s.Length > 0).ToList();

        private static Dictionary<string, string> Tags(string value) =>
            value.Split(',').Select(t => t.Split(new[] { ':' }, 2)).ToDictionary(t => t[0], t => t[1]);

        // "null" goes back to the server default; absent leaves the field out.
        private static Field<long> NullableField(Dictionary<string, string> p, string key) =>
            !p.TryGetValue(key, out var v) ? Field<long>.Keep : v == "null" ? Field<long>.UseDefault : Field<long>.Set(long.Parse(v));

        private static string Text(object value) => value == null ? "" : value is bool b ? (b ? "true" : "false") : value.ToString();

        private static string Render(Message message)
        {
            string type = message.ContentType.Split(';')[0].Trim().ToLowerInvariant();
            return type == "application/octet-stream" ? "b64:" + Convert.ToBase64String(message.Body) : message.BodyText;
        }

        private static Dictionary<string, string> Fields(params string[] pairs)
        {
            var output = new Dictionary<string, string>();
            for (int i = 0; i < pairs.Length; i += 2)
            {
                output[pairs[i]] = pairs[i + 1];
            }
            return output;
        }

        private static Dictionary<string, string> TopicFields(Topic t) =>
            Fields("id", t.Id, "name", t.Name, "type", t.Type, "fifo", Text(t.Fifo), "volatile", Text(t.Volatile), "raw", t.Raw);

        private static Dictionary<string, string> PublishFields(PublishResponse r) =>
            Fields("id", Text(r.Id), "duplicate", Text(r.Duplicate), "matched", Text(r.Matched));

        private static ConsumeOptions ConsumeOpts(Dictionary<string, string> p) =>
            new ConsumeOptions { MaxMessages = Int(p, "max"), WaitTime = Int(p, "wait"), VisibilityTimeout = Int(p, "visibility") };

        private static PublishOptions PublishOpts(Dictionary<string, string> p, string contentType) =>
            new PublishOptions
            {
                ContentType = contentType,
                GroupId = p.TryGetValue("groupId", out var g) ? g : null,
                DeduplicationId = p.TryGetValue("deduplicationId", out var d) ? d : null,
                Fifo = p.TryGetValue("fifo", out var f) && f == "true",
            };

        private static Dictionary<string, string> MessageFields(IList<Message> messages)
        {
            var output = Fields("count", messages.Count.ToString(), "bodies", string.Join(",", messages.Select(Render)));
            if (messages.Count > 0)
            {
                Message first = messages[0];
                output["id"] = first.Id;
                output["receiptToken"] = first.ReceiptToken;
                output["deliveryCount"] = first.DeliveryCount.ToString();
                output["contentType"] = first.ContentType;
                output["body"] = Render(first);
            }
            return output;
        }

        // ---- the operations -----------------------------------------------------------------------------

        private static Dictionary<string, Operation> Operations()
        {
            var ops = new Dictionary<string, Operation>();
            ops["createTopic"] = async (c, p) =>
            {
                var request = new TopicRequest
                {
                    Name = p["name"], Type = p["type"], Fifo = Flag(p, "fifo"), Volatile = Flag(p, "volatile"),
                    Ttl = Long(p, "ttl"), MaxDeliveries = Long(p, "maxDeliveries"), MaxMessageBytes = Long(p, "maxMessageBytes"),
                    IdleTimeout = Long(p, "idleTimeout"),
                };
                if (p.ContainsKey("prefixes")) request.Prefixes = Items(p["prefixes"]);
                if (p.ContainsKey("tags")) request.Tags = Tags(p["tags"]);
                return TopicFields(await c.CreateTopicAsync(request));
            };
            ops["getTopic"] = async (c, p) => TopicFields(await c.GetTopicAsync(p["name"]));
            ops["listTopics"] = async (c, p) =>
            {
                IList<Topic> topics = await c.ListAllTopicsAsync(p.TryGetValue("name", out var name) ? name : null);
                return Fields("count", topics.Count.ToString(), "names", string.Join(",", topics.Select(t => t.Name).OrderBy(n => n, StringComparer.Ordinal)));
            };
            ops["updateTopic"] = async (c, p) =>
            {
                var update = new TopicUpdate
                {
                    IdleTimeout = Long(p, "idleTimeout"),
                    Ttl = NullableField(p, "ttl"), MaxDeliveries = NullableField(p, "maxDeliveries"), MaxMessageBytes = NullableField(p, "maxMessageBytes"),
                };
                if (p.ContainsKey("prefixes")) update.Prefixes = Items(p["prefixes"]);
                if (p.ContainsKey("tags")) update.Tags = Tags(p["tags"]);
                return TopicFields(await c.UpdateTopicAsync(p["name"], update));
            };
            ops["deleteTopic"] = async (c, p) =>
            {
                await c.DeleteTopicAsync(p["name"]);
                return new Dictionary<string, string>();
            };

            ops["publishText"] = async (c, p) => PublishFields(await c.PublishAsync(p["topic"], Encoding.UTF8.GetBytes(p["body"]), PublishOpts(p, "text/plain; charset=utf-8")));
            ops["publishJson"] = async (c, p) => PublishFields(await c.PublishAsync(p["topic"], Encoding.UTF8.GetBytes(p["body"]), PublishOpts(p, "application/json")));
            ops["publishBytes"] = async (c, p) => PublishFields(await c.PublishAsync(p["topic"], Convert.FromBase64String(p["base64"]),
                PublishOpts(p, p.TryGetValue("contentType", out var type) ? type : "application/octet-stream")));

            ops["consume"] = async (c, p) => MessageFields(await c.ConsumeAsync(p["topic"], ConsumeOpts(p)));
            ops["ack"] = async (c, p) =>
            {
                await c.AckAsync(p["topic"], p["id"], p["receipt"]);
                return new Dictionary<string, string>();
            };
            ops["changeVisibility"] = async (c, p) =>
            {
                VisibilityResponse r = await c.ChangeVisibilityAsync(p["topic"], p["id"], p["receipt"], int.Parse(p["seconds"]));
                return Fields("id", r.Id, "invisibleUntil", r.InvisibleUntil);
            };

            // Consuming with a handler: mode ok (deleted), fail (throws), keep (Delete = false) or slow (holds the
            // message for `hold` seconds, which only works if the invisibility is renewed).
            ops["handle"] = async (c, p) =>
            {
                var seen = new List<string>();
                bool leaseLost = false;
                bool failed = false;
                int handled = 0;
                try
                {
                    handled = await c.ConsumeAsync(p["topic"], ConsumeOpts(p), async context =>
                    {
                        seen.Add(Render(context.Message));
                        if (p["mode"] == "fail") throw new InvalidOperationException("the handler failed");
                        if (p["mode"] == "keep") context.Delete = false;
                        if (p["mode"] == "slow") await Task.Delay(TimeSpan.FromSeconds(double.Parse(p["hold"])));
                        leaseLost = context.LeaseLost;
                    });
                }
                catch (TabaException)
                {
                    throw;
                }
                catch (Exception)
                {
                    failed = true;
                }
                return Fields("handled", handled.ToString(), "failed", Text(failed), "bodies", string.Join(",", seen), "leaseLost", Text(leaseLost));
            };

            // The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first fail.
            ops["subscribe"] = async (c, p) =>
            {
                var seen = new List<string>();
                using (var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(double.Parse(p["timeout"]))))
                {
                    await c.SubscribeAsync(p["topic"], new ConsumeOptions { VisibilityTimeout = Int(p, "visibility") }, message =>
                    {
                        seen.Add(Render(message));
                        if (seen.Count >= int.Parse(p["count"])) cancel.Cancel();
                        if (p.TryGetValue("failFirst", out var ff) && ff == "true" && seen.Count == 1) throw new InvalidOperationException("the handler failed");
                        return Task.CompletedTask;
                    }, cancel.Token);
                }
                return Fields("count", seen.Count.ToString(), "bodies", string.Join(",", seen));
            };

            // A long polling cancelled from outside must end at once, not at the end of the wait.
            ops["cancelConsume"] = async (c, p) =>
            {
                var started = DateTime.UtcNow;
                bool cancelled = false;
                using (var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(double.Parse(p["after"]))))
                {
                    try
                    {
                        await c.ConsumeAsync(p["topic"], new ConsumeOptions { WaitTime = 20 }, cancel.Token);
                    }
                    catch (OperationCanceledException)
                    {
                        cancelled = true;
                    }
                }
                return Fields("cancelled", Text(cancelled), "fast", Text(DateTime.UtcNow - started < TimeSpan.FromSeconds(10)));
            };

            // Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
            ops["drain"] = async (c, p) =>
            {
                var seen = new List<string>();
                DateTime deadline = DateTime.UtcNow + TimeSpan.FromSeconds(double.Parse(p["timeout"]));
                while (seen.Count < int.Parse(p["count"]) && DateTime.UtcNow < deadline)
                {
                    foreach (Message message in await c.ConsumeAsync(p["topic"], new ConsumeOptions { MaxMessages = 10, WaitTime = 1, VisibilityTimeout = 60 }))
                    {
                        seen.Add(Render(message));
                        await c.AckAsync(p["topic"], message);
                    }
                }
                seen.Sort(StringComparer.Ordinal);
                return Fields("count", seen.Count.ToString(), "bodies", string.Join(",", seen));
            };
            ops["sleep"] = async (c, p) =>
            {
                await Task.Delay(TimeSpan.FromSeconds(double.Parse(p["seconds"])));
                return new Dictionary<string, string>();
            };
            return ops;
        }

        // ---- the run ------------------------------------------------------------------------------------

        private static List<string> Check(Dictionary<string, string> result, List<Expect> expects)
        {
            var problems = new List<string>();
            bool failure = result.ContainsKey("error");
            bool expectsError = expects.Any(e => e.Key == "error");
            if (failure && !expectsError)
            {
                problems.Add("unexpected error " + result["error"] + " (status " + result["status"] + "): " + result["message"]);
            }
            if (!failure && expectsError)
            {
                problems.Add("expected an error, but the call succeeded");
            }
            foreach (Expect e in expects)
            {
                result.TryGetValue(e.Key, out var actual);
                bool mismatch = e.Op == '~' ? actual == null || !actual.Contains(e.Value)
                    : e.Value == "*" ? string.IsNullOrEmpty(actual)
                    : actual != e.Value;
                if (mismatch)
                {
                    problems.Add(e.Key + " " + e.Op + " \"" + e.Value + "\", got \"" + actual + "\"");
                }
            }
            return problems;
        }

        private static async Task<int> Main(string[] argv)
        {
            for (int i = 0; i < argv.Length; i += 2)
            {
                string key = argv[i].Substring(2);
                if (key == "scenario") Scenarios.Add(argv[i + 1]);
                else Args[key] = argv[i + 1];
            }
            Console.OutputEncoding = new UTF8Encoding(false);
            Dictionary<string, Operation> operations = Operations();
            var clients = new Dictionary<string, TabaClient>();
            var variables = new Dictionary<string, Dictionary<string, string>>();
            int passed = 0, failed = 0, skipped = 0;
            foreach (string path in Scenarios)
            {
                string[] lines = File.ReadAllLines(path, Encoding.UTF8);
                for (int n = 0; n < lines.Length; n++)
                {
                    string text = lines[n].Trim();
                    if (text.Length == 0 || text.StartsWith("#")) continue;
                    string where = Path.GetFileName(path) + ":" + (n + 1);
                    int arrow = text.IndexOf(" => ", StringComparison.Ordinal);
                    string[] head = (arrow < 0 ? text : text.Substring(0, arrow)).Trim().Split((char[])null, StringSplitOptions.RemoveEmptyEntries);
                    var parameters = new Dictionary<string, string>();
                    foreach (string pair in head.Skip(1))
                    {
                        int eq = pair.IndexOf('=');
                        parameters[pair.Substring(0, eq)] = Expand(pair.Substring(eq + 1), variables);
                    }
                    var expects = new List<Expect>();
                    if (arrow >= 0)
                    {
                        foreach (string token in text.Substring(arrow + 4).Split((char[])null, StringSplitOptions.RemoveEmptyEntries))
                        {
                            int at = token.IndexOfAny(new[] { '=', '~' });
                            expects.Add(new Expect { Key = token.Substring(0, at), Op = token[at], Value = Expand(token.Substring(at + 1), variables) });
                        }
                    }
                    if (!operations.TryGetValue(head[0], out Operation operation))
                    {
                        skipped++;
                        Console.WriteLine("SKIP " + where + " " + head[0] + " (not supported by this connector)");
                        continue;
                    }
                    string clientKey = parameters.GetValueOrDefault("url") + "|" + parameters.GetValueOrDefault("token") + "|" + parameters.GetValueOrDefault("attempts");
                    if (!clients.TryGetValue(clientKey, out TabaClient client))
                    {
                        var config = new TabaConfig(parameters.TryGetValue("url", out var url) ? url : Args["url"])
                        {
                            Token = parameters.TryGetValue("token", out var token) ? token : Args["token"],
                        };
                        if (parameters.ContainsKey("attempts")) config.MaxAttempts = int.Parse(parameters["attempts"]);
                        client = new TabaClient(config);
                        clients[clientKey] = client;
                    }
                    Dictionary<string, string> result;
                    try
                    {
                        result = await operation(client, parameters);
                    }
                    catch (TabaException e)
                    {
                        result = Fields("error", e.Code, "status", e.Status.ToString(), "message", e.Message);
                    }
                    if (parameters.TryGetValue("as", out var name)) variables[name] = result;
                    List<string> problems = Check(result, expects);
                    if (problems.Count == 0)
                    {
                        passed++;
                        Console.WriteLine("PASS " + where + " " + head[0]);
                    }
                    else
                    {
                        failed++;
                        Console.WriteLine("FAIL " + where + " " + text);
                        foreach (string problem in problems) Console.WriteLine("       " + problem);
                    }
                }
            }
            foreach (TabaClient client in clients.Values) client.Dispose();
            Console.WriteLine(Args["lang"] + ": " + passed + " passed, " + failed + " failed, " + skipped + " skipped");
            return failed == 0 ? 0 : 1;
        }
    }
}
