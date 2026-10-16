using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary>
    /// The integration scenarios (UC-CNT-027), against a real Taba. They only run with <c>TABA_URL</c>;
    /// <c>scripts/integration.sh</c> starts Compose, creates the tokens and the queues and sets these variables:
    /// <list type="bullet">
    /// <item><c>TABA_URL</c>: the client API (the Compose HAProxy).</item>
    /// <item><c>TABA_TOKEN</c>: a token with <c>publish</c>, <c>consume</c>, <c>create</c> and <c>manage</c> on the <c>csharp-*</c> topics.</item>
    /// <item><c>TABA_TOKEN_READONLY</c>: a token with only <c>consume</c> on the <c>csharp-*</c> topics.</item>
    /// </list>
    /// The queues <c>csharp-shared</c>, <c>csharp-shared-fifo</c>, <c>csharp-fan</c> (distributor with prefix
    /// <c>csharp-fan-</c>), <c>csharp-fan-a</c> (queue) and <c>csharp-fan-b</c> (FIFO) also come from the script.
    /// Without <c>TABA_URL</c>, the tests are ignored.
    /// </summary>
    [TestFixture]
    [Category("Integration")]
    public class IntegrationTests
    {
        private string _url;
        private string _token;
        private string _readOnlyToken;
        private TabaClient _client;

        [OneTimeSetUp]
        public void Connect()
        {
            _url = Environment.GetEnvironmentVariable("TABA_URL");
            if (string.IsNullOrEmpty(_url))
            {
                Assert.Ignore("TABA_URL is not set: run connectors/csharp/scripts/integration.sh");
            }
            _token = Environment.GetEnvironmentVariable("TABA_TOKEN");
            _readOnlyToken = Environment.GetEnvironmentVariable("TABA_TOKEN_READONLY");
            _client = new TabaClient(new TabaConfig(_url) { Token = _token, UserAgent = "taba-csharp-tests/" + TabaClient.Version });
        }

        [OneTimeTearDown]
        public void Disconnect()
        {
            if (_client != null)
            {
                _client.Dispose();
            }
        }

        private static string Unique(string kind)
        {
            return "csharp-" + kind + "-" + Guid.NewGuid().ToString("N").Substring(0, 8);
        }

        private TabaClient As(string token)
        {
            return new TabaClient(new TabaConfig(_url) { Token = token });
        }

        /// <summary>Creates a queue just for this test and deletes it at the end.</summary>
        private async Task<string> NewQueueAsync(string kind, bool fifo = false)
        {
            string name = Unique(kind);
            await _client.CreateTopicAsync(new TopicRequest { Name = name, Type = "queue", Fifo = fifo ? (bool?)true : null });
            return name;
        }

        /// <summary>Drains everything from the queue, so the test starts from a known point.</summary>
        private async Task DrainAsync(string topic)
        {
            while (true)
            {
                IList<Message> messages = await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 10, VisibilityTimeout = 30 });
                if (messages.Count == 0)
                {
                    return;
                }
                foreach (Message message in messages)
                {
                    await _client.AckAsync(topic, message);
                }
            }
        }

        private static byte[] Utf8(string text)
        {
            return Encoding.UTF8.GetBytes(text);
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_topic_can_be_created_read_listed_updated_and_deleted()
        {
            string name = Unique("crud");
            Topic created = await _client.CreateTopicAsync(new TopicRequest
            {
                Name = name,
                Type = "queue",
                Tags = new Dictionary<string, string> { { "time", "orders" } },
            });
            try
            {
                Assert.That(created.Name, Is.EqualTo(name));
                Assert.That(created.Type, Is.EqualTo("queue"));
                Assert.That(created.Id, Is.Not.Empty);

                Assert.That((await _client.GetTopicAsync(name)).Id, Is.EqualTo(created.Id));
                Assert.That((await _client.ListAllTopicsAsync(name)).Select(t => t.Name), Does.Contain(name), "the search is \"contains\": the queue's dead-letter also shows up");

                Page<Topic> page = await _client.ListTopicsAsync(1, null, "csharp-");
                Assert.That(page.Items.Count, Is.EqualTo(1));

                Topic updated = await _client.UpdateTopicAsync(name, new TopicUpdate
                {
                    Tags = new Dictionary<string, string> { { "time", "cobranca" } },
                    Ttl = 3600,
                });
                Assert.That(updated.Raw, Does.Contain("cobranca"));
                Assert.That(updated.Raw, Does.Contain("3600"));

                Topic reset = await _client.UpdateTopicAsync(name, new TopicUpdate { Ttl = Field<long>.UseDefault });
                Assert.That(reset.Raw, Does.Not.Contain("3600"));

                // Creating again is a real conflict (without the 503 before).
                var conflict = Assert.ThrowsAsync<TabaException>(() => _client.CreateTopicAsync(new TopicRequest { Name = name, Type = "queue" }));
                Assert.That(conflict.Code, Is.EqualTo("topic.already-exists"));
                Assert.That(conflict.Status, Is.EqualTo(409));
            }
            finally
            {
                await _client.DeleteTopicAsync(name);
            }
            var gone = Assert.ThrowsAsync<TabaException>(() => _client.GetTopicAsync(name));
            Assert.That(gone.Code, Is.EqualTo("topic.not-found"));
            var missing = Assert.ThrowsAsync<TabaException>(() => _client.DeleteTopicAsync(name));
            Assert.That(missing.Status, Is.EqualTo(404), "deleting what does not exist, without a 503 before, is a 404");
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_message_is_published_consumed_and_acked_once()
        {
            string topic = await NewQueueAsync("basico");
            try
            {
                PublishResponse published = await _client.PublishRawJsonAsync(topic, "{ \"order\": 9001, \"items\": [\"a\", \"b\"] }");
                Assert.That(published.Id, Is.Not.Empty);

                IList<Message> messages = await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 30, WaitTime = 5 });
                Assert.That(messages.Count, Is.EqualTo(1));
                Message message = messages[0];
                Assert.That(message.Id, Is.EqualTo(published.Id));
                Assert.That(message.ContentType, Does.StartWith("application/json"));
                Assert.That(message.DeliveryCount, Is.EqualTo(1));
                Assert.That(message.ReceiptToken, Is.Not.Empty);
                Assert.That(message.PublishedAt, Is.Not.Empty);
                Assert.That(message.InvisibleUntil, Is.Not.Empty);
                Assert.That(message.BodyText.Replace(" ", string.Empty), Is.EqualTo("{\"order\":9001,\"items\":[\"a\",\"b\"]}"));

                await _client.AckAsync(topic, message);
                Assert.That(await _client.ConsumeAsync(topic), Is.Empty, "acknowledged, it does not come back");

                var again = Assert.ThrowsAsync<TabaException>(() => _client.AckAsync(topic, message));
                Assert.That(again.Status, Is.AnyOf(404, 409), "the receipt was already used");
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027, UC-CNT-015
        [Test]
        public async Task text_json_and_binary_bodies_come_back_as_the_same_bytes()
        {
            string topic = await NewQueueAsync("corpos");
            try
            {
                byte[] png = { 0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff, 0xfe };
                await _client.PublishTextAsync(topic, "café, 世界 😀");
                await _client.PublishAsync(topic, png, new PublishOptions { ContentType = "image/png" });
                await _client.PublishAsync(topic, Utf8("[1,2,3]"), new PublishOptions { ContentType = "application/vnd.lote+json" });

                var bodies = new Dictionary<string, byte[]>();
                for (int round = 0; round < 10 && bodies.Count < 3; round++)
                {
                    foreach (Message message in await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 10, VisibilityTimeout = 30 }))
                    {
                        bodies[message.ContentType.Split(';')[0]] = message.Body;
                        await _client.AckAsync(topic, message);
                    }
                }

                Assert.That(Encoding.UTF8.GetString(bodies["text/plain"]), Is.EqualTo("café, 世界 😀"));
                Assert.That(bodies["image/png"], Is.EqualTo(png));
                Assert.That(Encoding.UTF8.GetString(bodies["application/vnd.lote+json"]), Is.EqualTo("[1,2,3]"));
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task the_server_refuses_a_body_that_is_not_the_declared_json()
        {
            string topic = await NewQueueAsync("invalido");
            try
            {
                var e = Assert.ThrowsAsync<TabaException>(() => _client.PublishRawJsonAsync(topic, "{this is not json"));
                Assert.That(e.Status, Is.EqualTo(400));
                Assert.That(e.Code, Is.EqualTo("validation.invalid-body"));
                Assert.That(e.RequestId, Is.Not.Empty);
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027, UC-CNT-008
        [Test]
        public async Task long_polling_waits_on_an_empty_queue_and_returns_as_soon_as_a_message_arrives()
        {
            string topic = await NewQueueAsync("wait");
            try
            {
                var watch = Stopwatch.StartNew();
                IList<Message> empty = await _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 2 });
                Assert.That(empty, Is.Empty);
                Assert.That(watch.Elapsed, Is.GreaterThanOrEqualTo(TimeSpan.FromSeconds(1.5)), "it waited the waitTime");

                Task<IList<Message>> waiting = _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 15, VisibilityTimeout = 30 });
                await Task.Delay(500);
                watch.Restart();
                await _client.PublishRawJsonAsync(topic, "{\"arrived\":true}");
                IList<Message> got = await waiting;
                Assert.That(got.Count, Is.EqualTo(1));
                Assert.That(watch.Elapsed, Is.LessThan(TimeSpan.FromSeconds(8)), "it returned before waitTime ran out");
                await _client.AckAsync(topic, got[0]);
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task an_unacked_message_is_invisible_until_released_and_then_comes_back()
        {
            string topic = await NewQueueAsync("invisivel");
            try
            {
                await _client.PublishRawJsonAsync(topic, "{\"n\":1}");
                Message first = (await _client.ConsumeAsync(topic, new ConsumeOptions { VisibilityTimeout = 60, WaitTime = 5 })).Single();
                Assert.That(await _client.ConsumeAsync(topic, new ConsumeOptions { VisibilityTimeout = 60 }), Is.Empty, "invisible");

                VisibilityResponse extended = await _client.ChangeVisibilityAsync(topic, first, 120);
                Assert.That(extended.Id, Is.EqualTo(first.Id));
                Assert.That(extended.InvisibleUntil, Is.Not.EqualTo(first.InvisibleUntil));

                await _client.ChangeVisibilityAsync(topic, first, 0);
                Message second = null;
                for (int i = 0; i < 20 && second == null; i++)
                {
                    second = (await _client.ConsumeAsync(topic, new ConsumeOptions { VisibilityTimeout = 30, WaitTime = 1 })).FirstOrDefault();
                }
                Assert.That(second, Is.Not.Null, "the released message comes back");
                Assert.That(second.Id, Is.EqualTo(first.Id));
                Assert.That(second.DeliveryCount, Is.EqualTo(2));
                await _client.AckAsync(topic, second);
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_fifo_queue_reports_a_duplicate_and_keeps_the_order_inside_a_group()
        {
            string topic = await NewQueueAsync("fifo", fifo: true);
            try
            {
                var options = new PublishOptions { Fifo = true, GroupId = "customer-42", DeduplicationId = "order-9001" };
                PublishResponse first = await _client.PublishAsync(topic, Utf8("{\"n\":1}"), options);
                PublishResponse repeated = await _client.PublishAsync(topic, Utf8("{\"n\":1}"), options);
                Assert.That(first.Duplicate, Is.False);
                Assert.That(repeated.Duplicate, Is.True);
                Assert.That(repeated.Id, Is.EqualTo(first.Id), "on a duplicate the original's id comes back");

                await _client.PublishAsync(topic, Utf8("{\"n\":2}"), new PublishOptions { Fifo = true, GroupId = "customer-42", DeduplicationId = "order-9002" });
                await _client.PublishAsync(topic, Utf8("{\"n\":3}"), new PublishOptions { Fifo = true, GroupId = "customer-42", DeduplicationId = "order-9003" });

                var order = new List<string>();
                for (int i = 0; i < 10 && order.Count < 3; i++)
                {
                    foreach (Message message in await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 30, WaitTime = 1 }))
                    {
                        Assert.That(message.GroupId, Is.EqualTo("customer-42"));
                        order.Add(message.BodyText);
                        await _client.AckAsync(topic, message);
                    }
                }
                Assert.That(order, Is.EqualTo(new[] { "{\"n\":1}", "{\"n\":2}", "{\"n\":3}" }), "a single message of the group at a time, in order");

                var noGroup = Assert.ThrowsAsync<TabaException>(() => _client.PublishAsync(topic, Utf8("{}"), new PublishOptions { Fifo = true }));
                Assert.That(noGroup.Code, Is.EqualTo("validation.group-id-required"));
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_distributor_copies_to_each_matching_queue_and_a_partial_failure_is_not_an_error()
        {
            // The queues come from the script: csharp-fan-a (plain) and csharp-fan-b (FIFO), under the csharp-fan distributor.
            await DrainAsync("csharp-fan-a");
            PublishResponse response = await _client.PublishRawJsonAsync("csharp-fan", "{\"event\":\"created\"}");

            Assert.That(response.Matched, Is.EqualTo(2));
            Assert.That(response.Deliveries.Count, Is.EqualTo(2));
            DistributorDelivery copy = response.Deliveries.Single(d => d.Topic == "csharp-fan-a");
            DistributorDelivery failed = response.Deliveries.Single(d => d.Topic == "csharp-fan-b");
            Assert.That(copy.Succeeded, Is.True);
            Assert.That(copy.Id, Is.Not.Empty);
            Assert.That(failed.Succeeded, Is.False, "without a groupId the copy to the FIFO fails, only that one");
            Assert.That(failed.ErrorCode, Is.EqualTo("validation.group-id-required"));

            Message delivered = (await _client.ConsumeAsync("csharp-fan-a", new ConsumeOptions { VisibilityTimeout = 30, WaitTime = 3 })).Single();
            Assert.That(delivered.Id, Is.EqualTo(copy.Id));
            await _client.AckAsync("csharp-fan-a", delivered);

            // With the groupId, both copy.
            await DrainAsync("csharp-fan-b");
            PublishResponse both = await _client.PublishAsync(
                "csharp-fan",
                Utf8("{\"event\":\"changed\"}"),
                new PublishOptions { GroupId = "g1" });
            Assert.That(both.Deliveries.All(d => d.Succeeded), Is.True);
            await DrainAsync("csharp-fan-a");
            await DrainAsync("csharp-fan-b");
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task the_shared_queue_created_by_the_script_works_with_the_application_token()
        {
            string topic = "csharp-shared";
            await DrainAsync(topic);
            string marker = Guid.NewGuid().ToString("N");
            await _client.PublishRawJsonAsync(topic, "{\"mark\":\"" + marker + "\"}");
            Message message = (await _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 5, VisibilityTimeout = 30 })).Single();
            Assert.That(message.BodyText, Does.Contain(marker));

            // The read-only token consumes the same queue.
            using (TabaClient reader = As(_readOnlyToken))
            {
                Assert.That((await reader.GetTopicAsync(topic)).Name, Is.EqualTo(topic));
            }
            await _client.AckAsync(topic, message);
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_token_without_the_permission_or_outside_its_prefixes_is_refused()
        {
            Assume.That(_readOnlyToken, Is.Not.Null.And.Not.Empty, "TABA_TOKEN_READONLY is not set");
            using (TabaClient reader = As(_readOnlyToken))
            {
                var publish = Assert.ThrowsAsync<TabaException>(() => reader.PublishRawJsonAsync("csharp-shared", "{}"));
                Assert.That(publish.Status, Is.EqualTo(403));
                Assert.That(publish.Code, Is.EqualTo("auth.forbidden"));

                var create = Assert.ThrowsAsync<TabaException>(() => reader.CreateTopicAsync(new TopicRequest { Name = Unique("denied"), Type = "queue" }));
                Assert.That(create.Code, Is.EqualTo("auth.forbidden"));

                // Consuming is allowed.
                IList<Message> none = await reader.ConsumeAsync("csharp-shared", new ConsumeOptions { MaxMessages = 1, VisibilityTimeout = 1 });
                Assert.That(none.Count, Is.LessThanOrEqualTo(1));
                foreach (Message message in none)
                {
                    await reader.ChangeVisibilityAsync("csharp-shared", message, 0);
                }
            }

            // Outside the application token's prefixes.
            var outside = Assert.ThrowsAsync<TabaException>(() => _client.CreateTopicAsync(new TopicRequest { Name = "other-" + Guid.NewGuid().ToString("N").Substring(0, 6), Type = "queue" }));
            Assert.That(outside.Status, Is.EqualTo(403));
            Assert.That(outside.Code, Is.EqualTo("auth.out-of-scope"));

            // Without a valid token.
            using (TabaClient anonymous = As(null))
            {
                var unauthorized = Assert.ThrowsAsync<TabaException>(() => anonymous.GetTopicAsync("csharp-shared"));
                Assert.That(unauthorized.Status, Is.EqualTo(401));
                Assert.That(unauthorized.Code, Is.EqualTo("auth.unauthorized"));
            }
            using (TabaClient forged = As("tk_this-is-not-a-token"))
            {
                var unauthorized = Assert.ThrowsAsync<TabaException>(() => forged.GetTopicAsync("csharp-shared"));
                Assert.That(unauthorized.Status, Is.EqualTo(401));
            }
        }

        /// UC: UC-CNT-027, UC-CNT-031
        [Test]
        public async Task subscribe_handles_and_acks_every_message_until_cancelled()
        {
            string topic = await NewQueueAsync("assina");
            try
            {
                for (int n = 1; n <= 3; n++)
                {
                    await _client.PublishRawJsonAsync(topic, "{\"n\":" + n + "}");
                }
                var received = new List<string>();
                using (var cts = new CancellationTokenSource(TimeSpan.FromSeconds(60)))
                {
                    await _client.SubscribeAsync(
                        topic,
                        new ConsumeOptions { WaitTime = 2, MaxMessages = 2 },
                        message =>
                        {
                            lock (received)
                            {
                                received.Add(message.BodyText);
                                if (received.Count == 3)
                                {
                                    cts.Cancel();
                                }
                            }
                            return Task.FromResult(0);
                        },
                        cts.Token);
                }
                Assert.That(received.OrderBy(x => x), Is.EqualTo(new[] { "{\"n\":1}", "{\"n\":2}", "{\"n\":3}" }));
                Assert.That(await _client.ConsumeAsync(topic), Is.Empty, "everything was acknowledged");
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027, UC-CNT-020
        [Test]
        public async Task many_parallel_publishers_share_one_client()
        {
            string topic = await NewQueueAsync("paralelo");
            try
            {
                const int total = 40;
                await Task.WhenAll(Enumerable.Range(0, total).Select(n => _client.PublishRawJsonAsync(topic, "{\"n\":" + n + "}")));

                var seen = new HashSet<string>();
                for (int round = 0; round < 40 && seen.Count < total; round++)
                {
                    foreach (Message message in await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 10, VisibilityTimeout = 60, WaitTime = 1 }))
                    {
                        seen.Add(message.BodyText);
                        await _client.AckAsync(topic, message);
                    }
                }
                Assert.That(seen.Count, Is.EqualTo(total));
            }
            finally
            {
                await _client.DeleteTopicAsync(topic);
            }
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task auto_create_makes_the_queue_on_consume()
        {
            string name = Unique("auto");
            try
            {
                IList<Message> none = await _client.ConsumeAsync(name, new ConsumeOptions
                {
                    AutoCreate = true,
                    Volatile = true,
                    IdleTimeout = 300,
                });
                Assert.That(none, Is.Empty);
                Topic topic = await _client.GetTopicAsync(name);
                Assert.That(topic.Volatile, Is.True);
            }
            finally
            {
                await _client.DeleteTopicAsync(name);
            }
        }
    }
}
