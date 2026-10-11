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
    /// Os cenários de integração (UC-CNT-027), contra um Taba de verdade. Só rodam com <c>TABA_URL</c>;
    /// o <c>scripts/integration.sh</c> sobe o Compose, cria os tokens e as filas e define estas variáveis:
    /// <list type="bullet">
    /// <item><c>TABA_URL</c>: a API de cliente (o HAProxy do Compose).</item>
    /// <item><c>TABA_TOKEN</c>: um token com <c>publish</c>, <c>consume</c>, <c>create</c> e <c>admin</c> nos tópicos <c>csharp-*</c>.</item>
    /// <item><c>TABA_TOKEN_READONLY</c>: um token só com <c>consume</c> nos tópicos <c>csharp-*</c>.</item>
    /// </list>
    /// As filas <c>csharp-shared</c>, <c>csharp-shared-fifo</c>, <c>csharp-fan</c> (distribuidor de prefixo
    /// <c>csharp-fan-</c>), <c>csharp-fan-a</c> (fila) e <c>csharp-fan-b</c> (FIFO) também vêm do script.
    /// Sem <c>TABA_URL</c>, os testes são ignorados.
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
                Assert.Ignore("TABA_URL não definida: rode connectors/csharp/scripts/integration.sh");
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

        /// <summary>Cria uma fila só deste teste e a apaga no fim.</summary>
        private async Task<string> NewQueueAsync(string kind, bool fifo = false)
        {
            string name = Unique(kind);
            await _client.CreateTopicAsync(new TopicRequest { Name = name, Type = "queue", Fifo = fifo ? (bool?)true : null });
            return name;
        }

        /// <summary>Tira da fila tudo o que há, para o teste começar de um ponto conhecido.</summary>
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
                Tags = new Dictionary<string, string> { { "time", "pedidos" } },
            });
            try
            {
                Assert.That(created.Name, Is.EqualTo(name));
                Assert.That(created.Type, Is.EqualTo("queue"));
                Assert.That(created.Id, Is.Not.Empty);

                Assert.That((await _client.GetTopicAsync(name)).Id, Is.EqualTo(created.Id));
                Assert.That((await _client.ListAllTopicsAsync(name)).Select(t => t.Name), Does.Contain(name), "a busca é por \"contém\": a dead-letter da fila também aparece");

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

                // Criar de novo é um conflito de verdade (sem o 503 antes).
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
            Assert.That(missing.Status, Is.EqualTo(404), "apagar o que não existe, sem 503 antes, é um 404");
        }

        /// UC: UC-CNT-027
        [Test]
        public async Task a_message_is_published_consumed_and_acked_once()
        {
            string topic = await NewQueueAsync("basico");
            try
            {
                PublishResponse published = await _client.PublishRawJsonAsync(topic, "{ \"pedido\": 9001, \"itens\": [\"a\", \"b\"] }");
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
                Assert.That(message.BodyText.Replace(" ", string.Empty), Is.EqualTo("{\"pedido\":9001,\"itens\":[\"a\",\"b\"]}"));

                await _client.AckAsync(topic, message);
                Assert.That(await _client.ConsumeAsync(topic), Is.Empty, "confirmada, não volta");

                var again = Assert.ThrowsAsync<TabaException>(() => _client.AckAsync(topic, message));
                Assert.That(again.Status, Is.AnyOf(404, 409), "o recibo já foi usado");
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
                await _client.PublishTextAsync(topic, "olá, 世界 😀");
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

                Assert.That(Encoding.UTF8.GetString(bodies["text/plain"]), Is.EqualTo("olá, 世界 😀"));
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
                var e = Assert.ThrowsAsync<TabaException>(() => _client.PublishRawJsonAsync(topic, "{isto não é json"));
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
            string topic = await NewQueueAsync("espera");
            try
            {
                var watch = Stopwatch.StartNew();
                IList<Message> empty = await _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 2 });
                Assert.That(empty, Is.Empty);
                Assert.That(watch.Elapsed, Is.GreaterThanOrEqualTo(TimeSpan.FromSeconds(1.5)), "esperou o waitTime");

                Task<IList<Message>> waiting = _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 15, VisibilityTimeout = 30 });
                await Task.Delay(500);
                watch.Restart();
                await _client.PublishRawJsonAsync(topic, "{\"chegou\":true}");
                IList<Message> got = await waiting;
                Assert.That(got.Count, Is.EqualTo(1));
                Assert.That(watch.Elapsed, Is.LessThan(TimeSpan.FromSeconds(8)), "voltou antes de esgotar o waitTime");
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
                Assert.That(await _client.ConsumeAsync(topic, new ConsumeOptions { VisibilityTimeout = 60 }), Is.Empty, "invisível");

                VisibilityResponse extended = await _client.ChangeVisibilityAsync(topic, first, 120);
                Assert.That(extended.Id, Is.EqualTo(first.Id));
                Assert.That(extended.InvisibleUntil, Is.Not.EqualTo(first.InvisibleUntil));

                await _client.ChangeVisibilityAsync(topic, first, 0);
                Message second = null;
                for (int i = 0; i < 20 && second == null; i++)
                {
                    second = (await _client.ConsumeAsync(topic, new ConsumeOptions { VisibilityTimeout = 30, WaitTime = 1 })).FirstOrDefault();
                }
                Assert.That(second, Is.Not.Null, "a mensagem liberada volta");
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
                var options = new PublishOptions { Fifo = true, GroupId = "cliente-42", DeduplicationId = "pedido-9001" };
                PublishResponse first = await _client.PublishAsync(topic, Utf8("{\"n\":1}"), options);
                PublishResponse repeated = await _client.PublishAsync(topic, Utf8("{\"n\":1}"), options);
                Assert.That(first.Duplicate, Is.False);
                Assert.That(repeated.Duplicate, Is.True);
                Assert.That(repeated.Id, Is.EqualTo(first.Id), "numa duplicata vem o id da original");

                await _client.PublishAsync(topic, Utf8("{\"n\":2}"), new PublishOptions { Fifo = true, GroupId = "cliente-42", DeduplicationId = "pedido-9002" });
                await _client.PublishAsync(topic, Utf8("{\"n\":3}"), new PublishOptions { Fifo = true, GroupId = "cliente-42", DeduplicationId = "pedido-9003" });

                var order = new List<string>();
                for (int i = 0; i < 10 && order.Count < 3; i++)
                {
                    foreach (Message message in await _client.ConsumeAsync(topic, new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 30, WaitTime = 1 }))
                    {
                        Assert.That(message.GroupId, Is.EqualTo("cliente-42"));
                        order.Add(message.BodyText);
                        await _client.AckAsync(topic, message);
                    }
                }
                Assert.That(order, Is.EqualTo(new[] { "{\"n\":1}", "{\"n\":2}", "{\"n\":3}" }), "uma só mensagem do grupo por vez, em ordem");

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
            // As filas vêm do script: csharp-fan-a (comum) e csharp-fan-b (FIFO), sob o distribuidor csharp-fan.
            await DrainAsync("csharp-fan-a");
            PublishResponse response = await _client.PublishRawJsonAsync("csharp-fan", "{\"evento\":\"criado\"}");

            Assert.That(response.Matched, Is.EqualTo(2));
            Assert.That(response.Deliveries.Count, Is.EqualTo(2));
            DistributorDelivery copy = response.Deliveries.Single(d => d.Topic == "csharp-fan-a");
            DistributorDelivery failed = response.Deliveries.Single(d => d.Topic == "csharp-fan-b");
            Assert.That(copy.Succeeded, Is.True);
            Assert.That(copy.Id, Is.Not.Empty);
            Assert.That(failed.Succeeded, Is.False, "sem groupId a cópia para a FIFO falha, só ela");
            Assert.That(failed.ErrorCode, Is.EqualTo("validation.group-id-required"));

            Message delivered = (await _client.ConsumeAsync("csharp-fan-a", new ConsumeOptions { VisibilityTimeout = 30, WaitTime = 3 })).Single();
            Assert.That(delivered.Id, Is.EqualTo(copy.Id));
            await _client.AckAsync("csharp-fan-a", delivered);

            // Com o groupId, as duas copiam.
            await DrainAsync("csharp-fan-b");
            PublishResponse both = await _client.PublishAsync(
                "csharp-fan",
                Utf8("{\"evento\":\"alterado\"}"),
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
            await _client.PublishRawJsonAsync(topic, "{\"marca\":\"" + marker + "\"}");
            Message message = (await _client.ConsumeAsync(topic, new ConsumeOptions { WaitTime = 5, VisibilityTimeout = 30 })).Single();
            Assert.That(message.BodyText, Does.Contain(marker));

            // O token só de leitura consome a mesma fila.
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
            Assume.That(_readOnlyToken, Is.Not.Null.And.Not.Empty, "TABA_TOKEN_READONLY não definida");
            using (TabaClient reader = As(_readOnlyToken))
            {
                var publish = Assert.ThrowsAsync<TabaException>(() => reader.PublishRawJsonAsync("csharp-shared", "{}"));
                Assert.That(publish.Status, Is.EqualTo(403));
                Assert.That(publish.Code, Is.EqualTo("auth.forbidden"));

                var create = Assert.ThrowsAsync<TabaException>(() => reader.CreateTopicAsync(new TopicRequest { Name = Unique("negado"), Type = "queue" }));
                Assert.That(create.Code, Is.EqualTo("auth.forbidden"));

                // Consumir é permitido.
                IList<Message> none = await reader.ConsumeAsync("csharp-shared", new ConsumeOptions { MaxMessages = 1, VisibilityTimeout = 1 });
                Assert.That(none.Count, Is.LessThanOrEqualTo(1));
                foreach (Message message in none)
                {
                    await reader.ChangeVisibilityAsync("csharp-shared", message, 0);
                }
            }

            // Fora dos prefixos do token da aplicação.
            var outside = Assert.ThrowsAsync<TabaException>(() => _client.CreateTopicAsync(new TopicRequest { Name = "outro-" + Guid.NewGuid().ToString("N").Substring(0, 6), Type = "queue" }));
            Assert.That(outside.Status, Is.EqualTo(403));
            Assert.That(outside.Code, Is.EqualTo("auth.out-of-scope"));

            // Sem token válido.
            using (TabaClient anonymous = As(null))
            {
                var unauthorized = Assert.ThrowsAsync<TabaException>(() => anonymous.GetTopicAsync("csharp-shared"));
                Assert.That(unauthorized.Status, Is.EqualTo(401));
                Assert.That(unauthorized.Code, Is.EqualTo("auth.unauthorized"));
            }
            using (TabaClient forged = As("tk_isto-nao-e-um-token"))
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
                Assert.That(await _client.ConsumeAsync(topic), Is.Empty, "tudo foi confirmado");
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
