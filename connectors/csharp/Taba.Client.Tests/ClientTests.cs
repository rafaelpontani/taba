using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary>Os testes unitários do cliente: sem rede, com o transporte de mentira.</summary>
    [TestFixture]
    public class ClientTests
    {
        private const string TopicJson = "{\"id\":\"t1\",\"name\":\"pedidos\",\"type\":\"queue\",\"createdAt\":\"2026-01-01T00:00:00Z\",\"fifo\":false,\"futuro\":[1,2]}";

        private static readonly byte[] NoBytes = new byte[0];

        private sealed class Rig
        {
            public ScriptedHandler Handler = new ScriptedHandler();
            public List<TimeSpan> Waits = new List<TimeSpan>();
            public TabaConfig Config = new TabaConfig("http://taba.test:8080");
            private TabaClient _client;

            public TabaClient Client
            {
                get
                {
                    if (_client == null)
                    {
                        _client = new TabaClient(
                            Config,
                            Handler,
                            (span, token) =>
                            {
                                lock (Waits)
                                {
                                    Waits.Add(span);
                                }
                                return Task.FromResult(0);
                            });
                    }
                    return _client;
                }
            }
        }

        private static string Error(string code, string message = "falhou")
        {
            return "{\"code\":\"" + code + "\",\"message\":\"" + message + "\",\"requestId\":\"req-1\"}";
        }

        private static string MessagesJson(params string[] messages)
        {
            return "{\"messages\":[" + string.Join(",", messages) + "]}";
        }

        private static string MessageJson(string id, string body, string contentType = "application/json", string extra = "")
        {
            return "{\"id\":\"" + id + "\",\"body\":" + body + ",\"contentType\":\"" + contentType + "\"," +
                "\"receiptToken\":\"rt_" + id + "\",\"deliveryCount\":2,\"publishedAt\":\"2026-01-01T00:00:00Z\"," +
                "\"invisibleUntil\":\"2026-01-01T00:02:00Z\"" + extra + "}";
        }

        // ------------------------------------------------------------------------------------------
        // A superfície e a configuração
        // ------------------------------------------------------------------------------------------

        /// UC: UC-CNT-001, UC-CNT-029
        [Test]
        public void the_nine_operations_build_the_expected_routes()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(201, TopicJson)
                .Reply(200, TopicJson)
                .Reply(200, "{\"items\":[" + TopicJson + "],\"nextCursor\":\"c2\"}")
                .Reply(200, TopicJson)
                .Reply(204)
                .Reply(201, "{\"id\":\"m1\"}")
                .Reply(200, MessagesJson())
                .Reply(204)
                .Reply(200, "{\"id\":\"m1\",\"invisibleUntil\":\"2026-01-01T00:05:00Z\"}");
            TabaClient client = rig.Client;

            client.CreateTopic(new TopicRequest { Name = "pedidos", Type = "queue" });
            client.GetTopic("pedidos");
            client.ListTopics(10, "c1", "ped");
            client.UpdateTopic("pedidos", new TopicUpdate { IdleTimeout = 60 });
            client.DeleteTopic("pedidos");
            client.PublishRawJson("pedidos", "{\"a\":1}");
            client.Consume("pedidos");
            client.Ack("pedidos", "m1", "rt_m1");
            client.ChangeVisibility("pedidos", "m1", "rt_m1", 30);

            var routes = rig.Handler.Requests.Select(r => r.Method + " " + r.PathAndQuery).ToArray();
            Assert.That(routes, Is.EqualTo(new[]
            {
                "POST /topics",
                "GET /topics/pedidos",
                "GET /topics?limit=10&cursor=c1&name=ped",
                "PATCH /topics/pedidos",
                "DELETE /topics/pedidos",
                "POST /topics/pedidos/messages",
                "GET /topics/pedidos/messages",
                "DELETE /topics/pedidos/messages/m1?receiptToken=rt_m1",
                "PUT /topics/pedidos/messages/m1/visibility?receiptToken=rt_m1",
            }));
        }

        /// UC: UC-CNT-029
        [Test]
        public void update_topic_is_a_real_patch_without_a_method_override_header()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, TopicJson);
            rig.Client.UpdateTopic("pedidos", new TopicUpdate
            {
                Tags = new Dictionary<string, string> { { "time", "a\"b" } },
                Prefixes = new List<string> { "pedidos-" },
                IdleTimeout = 90,
                Ttl = Field<long>.UseDefault,
                MaxDeliveries = 5,
            });
            Seen seen = rig.Handler.Last;
            Assert.That(seen.Method, Is.EqualTo("PATCH"));
            Assert.That(seen.Header("X-HTTP-Method-Override"), Is.Null);
            Assert.That(
                seen.BodyText,
                Is.EqualTo("{\"tags\":{\"time\":\"a\\\"b\"},\"prefixes\":[\"pedidos-\"],\"idleTimeout\":90,\"ttl\":null,\"maxDeliveries\":5}"));
        }

        /// UC: UC-CNT-001
        [Test]
        public void create_topic_sends_only_what_was_filled()
        {
            var rig = new Rig();
            rig.Handler.Reply(201, TopicJson).Reply(201, TopicJson);
            rig.Client.CreateTopic(new TopicRequest { Name = "pedidos", Type = "queue" });
            Assert.That(rig.Handler.Last.BodyText, Is.EqualTo("{\"name\":\"pedidos\",\"type\":\"queue\"}"));

            rig.Client.CreateTopic(new TopicRequest
            {
                Name = "fifo-1",
                Type = "queue",
                Fifo = true,
                Volatile = false,
                MaxMessageBytes = 1024,
                Ttl = 3600,
                MaxDeliveries = 3,
                IdleTimeout = 120,
            });
            Assert.That(
                rig.Handler.Last.BodyText,
                Is.EqualTo("{\"name\":\"fifo-1\",\"type\":\"queue\",\"volatile\":false,\"idleTimeout\":120,\"fifo\":true,\"maxMessageBytes\":1024,\"ttl\":3600,\"maxDeliveries\":3}"));
        }

        /// UC: UC-CNT-002
        [Test]
        public void config_defaults_and_required_base_url()
        {
            var config = new TabaConfig();
            Assert.That(config.ConnectTimeout, Is.EqualTo(TimeSpan.FromSeconds(5)));
            Assert.That(config.RequestTimeout, Is.EqualTo(TimeSpan.FromSeconds(30)));
            Assert.That(config.MaxAttempts, Is.EqualTo(3));
            Assert.That(config.UserAgent, Is.EqualTo("taba-csharp/" + TabaClient.Version));
            Assert.That(config.Token, Is.Null);

            foreach (string bad in new[] { null, "", "   ", "taba.test:8080", "ftp://taba.test" })
            {
                var e = Assert.Throws<TabaException>(() => new TabaClient(new TabaConfig(bad)));
                Assert.That(e.Code, Is.EqualTo("client.invalid-config"), "baseUrl " + bad);
                Assert.That(e.Status, Is.EqualTo(0));
            }
        }

        /// UC: UC-CNT-003
        [Test]
        public void a_token_goes_in_every_call_and_without_it_no_call_carries_authorization()
        {
            var rig = new Rig();
            rig.Config.Token = "tk_segredo";
            rig.Handler.Reply(200, TopicJson).Reply(204);
            rig.Client.GetTopic("a");
            rig.Client.DeleteTopic("a");
            Assert.That(rig.Handler.Requests.Select(r => r.Header("Authorization")), Is.All.EqualTo("Bearer tk_segredo"));

            var open = new Rig();
            open.Handler.Reply(200, TopicJson);
            open.Client.GetTopic("a");
            Assert.That(open.Handler.Last.Header("Authorization"), Is.Null);
        }

        /// UC: UC-CNT-018
        [Test]
        public void headers_carry_the_agent_and_the_optional_ids()
        {
            var rig = new Rig();
            rig.Config.UserAgent = "taba-csharp/0.1.0 meu-app/2";
            rig.Handler.Reply(201, "{\"id\":\"m1\"}").Reply(200, MessagesJson());
            rig.Client.Publish(
                "pedidos",
                Encoding.UTF8.GetBytes("{}"),
                new PublishOptions { RequestId = "req-9", Traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01" });
            Seen publish = rig.Handler.Last;
            Assert.That(publish.Header("Accept"), Is.EqualTo("application/json"));
            Assert.That(publish.Header("User-Agent"), Does.Contain("taba-csharp/0.1.0").And.Contain("meu-app/2"));
            Assert.That(publish.Header("X-Request-Id"), Is.EqualTo("req-9"));
            Assert.That(publish.Header("traceparent"), Does.StartWith("00-0af7"));

            rig.Client.Consume("pedidos", new ConsumeOptions { RequestId = "req-10" });
            Assert.That(rig.Handler.Last.Header("X-Request-Id"), Is.EqualTo("req-10"));
            Assert.That(rig.Handler.Last.Header("Content-Type"), Is.Null, "uma consulta sem corpo não leva Content-Type");
        }

        // ------------------------------------------------------------------------------------------
        // publish, consume, ack
        // ------------------------------------------------------------------------------------------

        /// UC: UC-CNT-004, UC-CNT-025
        [Test]
        public void publish_sends_raw_bytes_with_content_type_and_query()
        {
            var rig = new Rig();
            rig.Handler.Reply(201, "{\"id\":\"m1\",\"duplicate\":false}").Reply(201, "{\"id\":\"m2\"}").Reply(201, "{\"id\":\"m3\"}");

            PublishResponse first = rig.Client.Publish(
                "pedidos",
                new byte[] { 0x89, 0x50, 0x4e, 0x47, 0x00, 0xff },
                new PublishOptions { ContentType = "image/png", GroupId = "cliente-42", DeduplicationId = "pedido:9001" });
            Seen seen = rig.Handler.Last;
            Assert.That(seen.Path, Is.EqualTo("/topics/pedidos/messages"));
            Assert.That(seen.PathAndQuery, Does.EndWith("?groupId=cliente-42&deduplicationId=pedido%3A9001"));
            Assert.That(seen.Header("Content-Type"), Is.EqualTo("image/png"));
            Assert.That(seen.Body, Is.EqualTo(new byte[] { 0x89, 0x50, 0x4e, 0x47, 0x00, 0xff }), "o corpo vai sem validação nem conversão");
            Assert.That(first.Id, Is.EqualTo("m1"));
            Assert.That(first.Duplicate, Is.False);

            // O texto vai e volta em UTF-8, qualquer que seja o charset local.
            rig.Client.PublishText("pedidos", "olá, 世界 😀");
            Assert.That(rig.Handler.Last.Header("Content-Type"), Is.EqualTo("text/plain; charset=utf-8"));
            Assert.That(rig.Handler.Last.Body, Is.EqualTo(Encoding.UTF8.GetBytes("olá, 世界 😀")));

            rig.Client.PublishRawJson("pedidos", "{\"a\":1}");
            Assert.That(rig.Handler.Last.Header("Content-Type"), Is.EqualTo("application/json"));
        }

        /// UC: UC-CNT-004
        [Test]
        public void a_distributor_answer_lists_each_copy_and_the_partial_failures()
        {
            var rig = new Rig();
            rig.Handler.Reply(
                200,
                "{\"matched\":2,\"deliveries\":[{\"topic\":\"pedidos-a\",\"id\":\"m1\"},{\"topic\":\"pedidos-b\",\"error\":{\"code\":\"validation.group-id-required\",\"message\":\"sem groupId\"}}]}");
            PublishResponse response = rig.Client.PublishRawJson("eventos", "{}");
            Assert.That(response.Id, Is.Null);
            Assert.That(response.Matched, Is.EqualTo(2));
            Assert.That(response.Deliveries.Count, Is.EqualTo(2));
            Assert.That(response.Deliveries[0].Succeeded, Is.True);
            Assert.That(response.Deliveries[0].Topic, Is.EqualTo("pedidos-a"));
            Assert.That(response.Deliveries[0].Id, Is.EqualTo("m1"));
            Assert.That(response.Deliveries[1].Succeeded, Is.False);
            Assert.That(response.Deliveries[1].ErrorCode, Is.EqualTo("validation.group-id-required"));
            Assert.That(response.Deliveries[1].ErrorMessage, Is.EqualTo("sem groupId"));
        }

        /// UC: UC-CNT-005
        [Test]
        public void consume_sends_only_what_was_given()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, MessagesJson()).Reply(200, MessagesJson()).Reply(200, MessagesJson());

            Assert.That(rig.Client.Consume("pedidos"), Is.Empty);
            Assert.That(rig.Handler.Last.PathAndQuery, Is.EqualTo("/topics/pedidos/messages"), "nenhum padrão próprio");

            rig.Client.Consume("pedidos", new ConsumeOptions { MaxMessages = 5, VisibilityTimeout = 60, WaitTime = 20 });
            Assert.That(
                rig.Handler.Last.PathAndQuery,
                Is.EqualTo("/topics/pedidos/messages?maxMessages=5&visibilityTimeout=60&waitTime=20"));

            rig.Client.Consume(
                "pedidos",
                new ConsumeOptions
                {
                    AutoCreate = true,
                    Volatile = true,
                    IdleTimeout = 300,
                    Fifo = false,
                    Tags = new Dictionary<string, string> { { "time", "pagamentos" } },
                });
            Assert.That(
                rig.Handler.Last.PathAndQuery,
                Is.EqualTo("/topics/pedidos/messages?autoCreate=true&volatile=true&idleTimeout=300&fifo=false&tag.time=pagamentos"));
        }

        /// UC: UC-CNT-005
        [Test]
        public void auto_create_parameters_are_left_out_without_auto_create()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, MessagesJson());
            rig.Client.Consume("pedidos", new ConsumeOptions { Volatile = true, IdleTimeout = 5, Fifo = true });
            Assert.That(rig.Handler.Last.PathAndQuery, Is.EqualTo("/topics/pedidos/messages"));
        }

        /// UC: UC-CNT-008
        [Test]
        public void the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout()
        {
            Assert.That(
                TabaClient.ConsumeCall("t", new ConsumeOptions { WaitTime = 20 }).Timeout,
                Is.EqualTo(TimeSpan.FromSeconds(30)));
            Assert.That(
                TabaClient.ConsumeCall("t", new ConsumeOptions { WaitTime = 25 }).Timeout,
                Is.EqualTo(TimeSpan.FromSeconds(35)), "acima do requestTimeout de 30 s");
            Assert.That(TabaClient.ConsumeCall("t", null).Timeout, Is.EqualTo(TimeSpan.FromSeconds(10)));
        }

        /// UC: UC-CNT-006
        [Test]
        public void ack_and_visibility_put_the_receipt_token_in_the_query()
        {
            var rig = new Rig();
            rig.Handler.Reply(204).Reply(200, "{\"id\":\"m 1\",\"invisibleUntil\":\"2026-01-01T00:05:00Z\"}");
            rig.Client.Ack("pedidos", "m 1", "rt/+=x");
            Assert.That(rig.Handler.Last.Method, Is.EqualTo("DELETE"));
            Assert.That(rig.Handler.Last.PathAndQuery, Is.EqualTo("/topics/pedidos/messages/m%201?receiptToken=rt%2F%2B%3Dx"));

            VisibilityResponse response = rig.Client.ChangeVisibility("pedidos", "m 1", "rt/+=x", 45);
            Assert.That(rig.Handler.Last.Method, Is.EqualTo("PUT"));
            Assert.That(rig.Handler.Last.PathAndQuery, Does.EndWith("/visibility?receiptToken=rt%2F%2B%3Dx"));
            Assert.That(rig.Handler.Last.BodyText, Is.EqualTo("{\"visibilityTimeout\":45}"));
            Assert.That(response.InvisibleUntil, Is.EqualTo("2026-01-01T00:05:00Z"));
        }

        /// UC: UC-CNT-007
        [Test]
        public void list_all_topics_follows_next_cursor_until_the_last_page()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(200, "{\"items\":[" + TopicJson + "],\"nextCursor\":\"c2\"}")
                .Reply(200, "{\"items\":[" + TopicJson + "," + TopicJson + "],\"nextCursor\":\"c3\"}")
                .Reply(200, "{\"items\":[" + TopicJson + "]}");
            IList<Topic> all = rig.Client.ListAllTopics("ped");
            Assert.That(all.Count, Is.EqualTo(4));
            Assert.That(
                rig.Handler.Requests.Select(r => r.PathAndQuery),
                Is.EqualTo(new[] { "/topics?name=ped", "/topics?cursor=c2&name=ped", "/topics?cursor=c3&name=ped" }));

            rig.Handler.Reply(200, "{\"items\":[],\"nextCursor\":null}");
            Page<Topic> empty = rig.Client.ListTopics();
            Assert.That(empty.Items, Is.Empty);
            Assert.That(empty.NextCursor, Is.Null);
        }

        // ------------------------------------------------------------------------------------------
        // Repetição de chamadas e erros
        // ------------------------------------------------------------------------------------------

        /// UC: UC-CNT-009, UC-CNT-017
        [Test]
        public void a_timeout_is_not_repeated_and_is_a_client_side_error()
        {
            var rig = new Rig();
            rig.Config.RequestTimeout = TimeSpan.FromMilliseconds(150);
            rig.Handler.Hang();
            var e = Assert.Throws<TabaException>(() => rig.Client.PublishRawJson("pedidos", "{}"));
            Assert.That(e.Code, Is.EqualTo("client.timeout"));
            Assert.That(e.Status, Is.EqualTo(0));
            Assert.That(e.IsClientSide, Is.True);
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(1));
            Assert.That(rig.Waits, Is.Empty);
        }

        /// UC: UC-CNT-010
        [Test]
        public void retries_429_and_503_waiting_for_retry_after_then_backoff()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(429, Error("rate-limit.exceeded"), "application/json", "Retry-After", "7")
                .Reply(503, Error("cluster.quorum-unavailable"), "application/json", "Retry-After", "2")
                .Reply(200, TopicJson);
            Topic topic = rig.Client.GetTopic("pedidos");
            Assert.That(topic.Name, Is.EqualTo("pedidos"));
            Assert.That(rig.Waits, Is.EqualTo(new[] { TimeSpan.FromSeconds(7), TimeSpan.FromSeconds(2) }));
        }

        /// UC: UC-CNT-010
        [Test]
        public void backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts()
        {
            var rig = new Rig();
            rig.Config.MaxAttempts = 4;
            rig.Handler
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(503, Error("cluster.quorum-unavailable", "sem quórum"));
            var e = Assert.Throws<TabaException>(() => rig.Client.GetTopic("pedidos"));
            Assert.That(e.Status, Is.EqualTo(503));
            Assert.That(e.Code, Is.EqualTo("cluster.quorum-unavailable"));
            Assert.That(e.Message, Is.EqualTo("sem quórum"));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(4));
            Assert.That(rig.Waits, Is.EqualTo(new[] { TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2), TimeSpan.FromSeconds(4) }));
        }

        /// UC: UC-CNT-011
        [Test]
        public void after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(409, Error("topic.already-exists"))
                .Reply(200, TopicJson)
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(404, Error("topic.not-found"));
            Topic created = rig.Client.CreateTopic(new TopicRequest { Name = "pedidos", Type = "queue" });
            Assert.That(created.Name, Is.EqualTo("pedidos"));
            Assert.That(rig.Handler.Requests[2].Method, Is.EqualTo("GET"), "o tópico existente é buscado");

            rig.Client.DeleteTopic("pedidos");
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(5));
        }

        /// UC: UC-CNT-011, UC-CNT-014
        [Test]
        public void without_a_previous_503_a_409_or_404_is_a_plain_error()
        {
            var rig = new Rig();
            rig.Handler.Reply(409, Error("topic.already-exists")).Reply(404, Error("topic.not-found"));
            var conflict = Assert.Throws<TabaException>(() => rig.Client.CreateTopic(new TopicRequest { Name = "a", Type = "queue" }));
            Assert.That(conflict.Code, Is.EqualTo("topic.already-exists"));
            var missing = Assert.Throws<TabaException>(() => rig.Client.DeleteTopic("a"));
            Assert.That(missing.Status, Is.EqualTo(404));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(2));
        }

        /// UC: UC-CNT-012
        [Test]
        public void gateway_errors_are_only_repeated_when_publishing_to_a_fifo()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(502, Error("forward.bad-gateway"))
                .Reply(504, Error("forward.timeout"))
                .Reply(201, "{\"id\":\"m1\"}")
                .Reply(502, Error("forward.bad-gateway"));
            PublishResponse ok = rig.Client.Publish("fifo", NoBytes, new PublishOptions { Fifo = true, GroupId = "g" });
            Assert.That(ok.Id, Is.EqualTo("m1"));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(3));

            var e = Assert.Throws<TabaException>(() => rig.Client.PublishRawJson("fila", "{}"));
            Assert.That(e.Status, Is.EqualTo(502));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(4), "fora da FIFO não repete");
        }

        /// UC: UC-CNT-013
        [Test]
        public void network_failure_before_sending_is_repeated_and_after_is_not()
        {
            var rig = new Rig();
            var refused = new HttpRequestException("recusada", new SocketException((int)SocketError.ConnectionRefused));
            rig.Handler.Fail(refused).Fail(refused).Reply(200, TopicJson);
            Assert.That(rig.Client.GetTopic("pedidos").Id, Is.EqualTo("t1"));
            Assert.That(rig.Waits, Is.EqualTo(new[] { TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2) }));

            var dropped = new HttpRequestException("caiu", new IOException("conexão encerrada no meio da resposta"));
            rig.Handler.Fail(dropped);
            var e = Assert.Throws<TabaException>(() => rig.Client.PublishRawJson("pedidos", "{}"));
            Assert.That(e.Code, Is.EqualTo("client.network"));
            Assert.That(e.Status, Is.EqualTo(0));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(4), "depois de enviar não repete");

            var other = new Rig();
            other.Config.MaxAttempts = 2;
            other.Handler.Fail(refused).Fail(refused);
            var last = Assert.Throws<TabaException>(() => other.Client.GetTopic("x"));
            Assert.That(last.Code, Is.EqualTo("client.network"));
            Assert.That(other.Handler.Requests.Count, Is.EqualTo(2));
        }

        /// UC: UC-CNT-013
        [Test]
        public void a_real_refused_connection_is_a_network_error_after_the_attempts()
        {
            // A porta 1 de localhost não tem ninguém: recusa de verdade, pela pilha do sistema.
            using (var client = new TabaClient(new TabaConfig("http://127.0.0.1:1") { MaxAttempts = 2 }, null, (s, t) => Task.FromResult(0)))
            {
                var e = Assert.Throws<TabaException>(() => client.GetTopic("x"));
                Assert.That(e.Code, Is.EqualTo("client.network"));
                Assert.That(e.IsClientSide, Is.True);
            }
        }

        /// UC: UC-CNT-014
        [Test]
        public void a_4xx_other_than_429_is_never_repeated()
        {
            foreach (int status in new[] { 400, 401, 403, 404, 409, 413, 422 })
            {
                var rig = new Rig();
                rig.Handler.Reply(status, Error("validation.failed"));
                var e = Assert.Throws<TabaException>(() => rig.Client.GetTopic("x"));
                Assert.That(e.Status, Is.EqualTo(status));
                Assert.That(rig.Handler.Requests.Count, Is.EqualTo(1), "status " + status);
            }
        }

        /// UC: UC-CNT-017, UC-CNT-018
        [Test]
        public void the_error_carries_the_error_response_fields_and_the_headers()
        {
            var rig = new Rig();
            rig.Handler.Reply(
                429,
                "{\"code\":\"rate-limit.exceeded\",\"message\":\"devagar\",\"details\":[{\"field\":\"x\",\"reason\":\"y\"}],\"campoNovo\":true}",
                "application/json",
                "Retry-After", "9",
                "X-Request-Id", "req-do-cabecalho");
            rig.Config.MaxAttempts = 1;
            var e = Assert.Throws<TabaException>(() => rig.Client.GetTopic("x"));
            Assert.That(e.Status, Is.EqualTo(429));
            Assert.That(e.Code, Is.EqualTo("rate-limit.exceeded"));
            Assert.That(e.Message, Is.EqualTo("devagar"));
            Assert.That(e.RetryAfter, Is.EqualTo(9));
            Assert.That(e.RequestId, Is.EqualTo("req-do-cabecalho"), "sem requestId no corpo, vale o cabeçalho");
            Assert.That(e.Details, Is.EqualTo("[{\"field\":\"x\",\"reason\":\"y\"}]"));
            Assert.That(e.IsClientSide, Is.False);
            Assert.That(Encoding.UTF8.GetString(e.Body), Does.Contain("campoNovo"));
        }

        /// UC: UC-CNT-017
        [Test]
        public void a_non_json_error_still_reports_a_status_code()
        {
            var rig = new Rig();
            rig.Handler.Reply(500, "<html>Bad gateway do proxy</html>", "text/html").Reply(500, string.Empty);
            var html = Assert.Throws<TabaException>(() => rig.Client.GetTopic("x"));
            Assert.That(html.Code, Is.EqualTo("http.500"));
            Assert.That(html.Message, Does.Contain("Bad gateway"));
            var empty = Assert.Throws<TabaException>(() => rig.Client.GetTopic("x"));
            Assert.That(empty.Code, Is.EqualTo("http.500"));
            Assert.That(empty.Message, Does.Contain("sem corpo"));
        }

        // ------------------------------------------------------------------------------------------
        // A mensagem entregue
        // ------------------------------------------------------------------------------------------

        /// UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
        [Test]
        public void messages_come_back_as_bytes_for_json_text_and_base64()
        {
            var rig = new Rig();
            byte[] png = { 0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a };
            rig.Handler.Reply(
                200,
                MessagesJson(
                    MessageJson("m1", "{ \"pedido\": 9001,\n \"itens\": [\"a\", \"b\"] }", extra: ",\"groupId\":\"g1\",\"deduplicationId\":\"d1\",\"deadLetteredAt\":\"2026-02-01T00:00:00Z\",\"deadLetterReason\":\"maxDeliveries\",\"novo\":{\"x\":1}"),
                    MessageJson("m2", "\"olá \\u4e16\\u754c \\ud83d\\ude00\\n\"", "text/plain; charset=utf-8"),
                    MessageJson("m3", "\"" + Convert.ToBase64String(png) + "\"", "image/png", ",\"bodyEncoding\":\"base64\""),
                    MessageJson("m4", "[1,2]", "application/vnd.pedido+json"),
                    MessageJson("m5", "\"x\"").Replace("\"body\":\"x\",", string.Empty)));
            IList<Message> messages = rig.Client.Consume("pedidos");

            Assert.That(messages.Count, Is.EqualTo(5));
            Message json = messages[0];
            Assert.That(json.BodyText, Is.EqualTo("{ \"pedido\": 9001,\n \"itens\": [\"a\", \"b\"] }"), "o trecho de texto, sem reserializar");
            Assert.That(json.Id, Is.EqualTo("m1"));
            Assert.That(json.ContentType, Is.EqualTo("application/json"));
            Assert.That(json.ReceiptToken, Is.EqualTo("rt_m1"));
            Assert.That(json.DeliveryCount, Is.EqualTo(2));
            Assert.That(json.PublishedAt, Is.EqualTo("2026-01-01T00:00:00Z"));
            Assert.That(json.InvisibleUntil, Is.EqualTo("2026-01-01T00:02:00Z"));
            Assert.That(json.GroupId, Is.EqualTo("g1"));
            Assert.That(json.DeduplicationId, Is.EqualTo("d1"));
            Assert.That(json.DeadLetteredAt, Is.EqualTo("2026-02-01T00:00:00Z"));
            Assert.That(json.DeadLetterReason, Is.EqualTo("maxDeliveries"));
            Assert.That(messages[1].BodyText, Is.EqualTo("olá 世界 😀\n"));
            Assert.That(messages[1].Body, Is.EqualTo(Encoding.UTF8.GetBytes("olá 世界 😀\n")));
            Assert.That(messages[2].Body, Is.EqualTo(png));
            Assert.That(messages[3].BodyText, Is.EqualTo("[1,2]"), "*/*+json");
            Assert.That(messages[4].Body, Is.Empty);
            Assert.That(messages[4].GroupId, Is.Null);
        }

        /// UC: UC-CNT-026
        [Test]
        public void an_invalid_answer_is_an_invalid_response_error_and_not_a_crash()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, "isto não é JSON").Reply(200, MessagesJson(MessageJson("m1", "\"@@@\"", "image/png", ",\"bodyEncoding\":\"base64\"")));
            var e = Assert.Throws<TabaException>(() => rig.Client.GetTopic("x"));
            Assert.That(e.Code, Is.EqualTo("client.invalid-response"));
            var base64 = Assert.Throws<TabaException>(() => rig.Client.Consume("x"));
            Assert.That(base64.Code, Is.EqualTo("client.invalid-response"));
        }

        /// UC: UC-CNT-028
        [Test]
        public void unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, "{\"zzz\":{\"a\":[1,2,{\"b\":null}]},\"id\":\"t9\",\"name\":\"n\",\"type\":\"distributor\",\"createdAt\":\"x\",\"outro\":\"y\"}");
            Topic topic = rig.Client.GetTopic("n");
            Assert.That(topic.Id, Is.EqualTo("t9"));
            Assert.That(topic.Type, Is.EqualTo("distributor"));
            Assert.That(topic.Fifo, Is.Null);
            Assert.That(TabaClient.ContractVersion, Is.EqualTo("1"));
        }

        // ------------------------------------------------------------------------------------------
        // Concorrência, assíncrono e subscribe
        // ------------------------------------------------------------------------------------------

        /// UC: UC-CNT-020
        [Test]
        public void many_threads_can_use_one_client_at_the_same_time()
        {
            var rig = new Rig();
            const int calls = 64;
            for (int i = 0; i < calls; i++)
            {
                rig.Handler.Reply(200, TopicJson);
            }
            TabaClient client = rig.Client;
            Task<Topic>[] tasks = Enumerable.Range(0, calls).Select(i => Task.Run(() => client.GetTopicAsync("pedidos"))).ToArray();
            Task.WaitAll(tasks);
            Assert.That(tasks.All(t => t.Result.Name == "pedidos"), Is.True);
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(calls));
        }

        /// UC: UC-CNT-032
        [Test]
        public async Task the_async_forms_do_the_same_as_the_sync_ones()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(503, Error("cluster.quorum-unavailable"))
                .Reply(201, TopicJson)
                .Reply(201, "{\"id\":\"m1\"}")
                .Reply(200, MessagesJson(MessageJson("m1", "{}")))
                .Reply(204)
                .Reply(404, Error("topic.not-found"));
            Topic topic = await rig.Client.CreateTopicAsync(new TopicRequest { Name = "pedidos", Type = "queue" });
            Assert.That(topic.Name, Is.EqualTo("pedidos"));
            Assert.That(rig.Waits, Is.EqualTo(new[] { TimeSpan.FromSeconds(1) }), "a mesma repetição");

            await rig.Client.PublishTextAsync("pedidos", "oi");
            IList<Message> messages = await rig.Client.ConsumeAsync("pedidos");
            await rig.Client.AckAsync("pedidos", messages[0]);
            Assert.That(rig.Handler.Last.PathAndQuery, Is.EqualTo("/topics/pedidos/messages/m1?receiptToken=rt_m1"));

            var e = Assert.ThrowsAsync<TabaException>(() => rig.Client.GetTopicAsync("pedidos"));
            Assert.That(e.Code, Is.EqualTo("topic.not-found"), "os mesmos erros");
        }

        /// UC: UC-CNT-032
        [Test]
        public void every_operation_has_a_sync_and_an_async_form()
        {
            var methods = typeof(TabaClient).GetMethods().Where(m => m.DeclaringType == typeof(TabaClient)).Select(m => m.Name).ToArray();
            foreach (string operation in new[] { "CreateTopic", "GetTopic", "ListTopics", "UpdateTopic", "DeleteTopic", "Publish", "Consume", "Ack", "ChangeVisibility", "Subscribe" })
            {
                Assert.That(methods, Does.Contain(operation), operation);
                Assert.That(methods, Does.Contain(operation + "Async"), operation + "Async");
            }
            foreach (var method in typeof(TabaClient).GetMethods().Where(m => m.Name.EndsWith("Async", StringComparison.Ordinal) && m.DeclaringType == typeof(TabaClient)))
            {
                Assert.That(typeof(Task).IsAssignableFrom(method.ReturnType), Is.True, method.Name);
                Assert.That(method.GetParameters().Last().ParameterType.Name, Is.EqualTo(method.Name == "SubscribeAsync" ? "Action`2" : "CancellationToken"), method.Name);
            }
        }

        /// UC: UC-CNT-020
        [Test]
        public async Task cancelling_the_token_cancels_the_call_without_a_client_error()
        {
            var rig = new Rig();
            rig.Handler.Hang();
            using (var cts = new CancellationTokenSource())
            {
                Task<IList<Message>> pending = rig.Client.ConsumeAsync("pedidos", new ConsumeOptions { WaitTime = 20 }, cts.Token);
                cts.Cancel();
                try
                {
                    await pending;
                    Assert.Fail("deveria ter sido cancelada");
                }
                catch (OperationCanceledException)
                {
                }
            }
        }

        /// UC: UC-CNT-031
        [Test]
        public async Task subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(200, MessagesJson(MessageJson("m1", "{\"n\":1}")))
                .Reply(204)
                .Reply(200, MessagesJson(MessageJson("m2", "{\"n\":2}")))
                .Reply(200, MessagesJson(MessageJson("m3", "{\"n\":3}")))
                .Reply(409, Error("message.receipt-expired"));
            var handled = new List<string>();
            var failures = new List<string>();
            using (var cts = new CancellationTokenSource())
            {
                await rig.Client.SubscribeAsync(
                    "pedidos",
                    null,
                    message =>
                    {
                        handled.Add(message.Id);
                        if (message.Id == "m2")
                        {
                            throw new InvalidOperationException("não deu");
                        }
                        if (message.Id == "m3")
                        {
                            cts.Cancel();
                        }
                        return Task.FromResult(0);
                    },
                    cts.Token,
                    (message, error) => failures.Add(message.Id + ":" + error.Message));
            }

            Assert.That(handled, Is.EqualTo(new[] { "m1", "m2", "m3" }));
            Assert.That(failures, Is.EqualTo(new[] { "m2:não deu" }));
            Assert.That(
                rig.Handler.Requests.Select(r => r.Method).ToArray(),
                Is.EqualTo(new[] { "GET", "DELETE", "GET", "GET", "DELETE" }), "m2 não foi confirmada; o 409 do ack de m3 é ignorado");
            Assert.That(rig.Handler.Requests[0].PathAndQuery, Is.EqualTo("/topics/pedidos/messages?maxMessages=1&waitTime=20"));
        }

        /// UC: UC-CNT-031
        [Test]
        public void the_sync_subscribe_blocks_until_cancelled()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, MessagesJson(MessageJson("m1", "{}"))).Reply(204);
            using (var cts = new CancellationTokenSource())
            {
                int seen = 0;
                rig.Client.Subscribe("pedidos", new ConsumeOptions { WaitTime = 5, MaxMessages = 3 }, m =>
                {
                    seen++;
                    cts.Cancel();
                }, cts.Token);
                Assert.That(seen, Is.EqualTo(1));
            }
            Assert.That(rig.Handler.Requests[0].PathAndQuery, Is.EqualTo("/topics/pedidos/messages?maxMessages=3&waitTime=5"));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(2), "a mensagem tratada é confirmada mesmo com o cancelamento");
        }

        private sealed class UpperSerializer : ITabaJsonSerializer
        {
            public string Serialize(object value)
            {
                return "{\"custom\":\"" + value + "\"}";
            }

            public T Deserialize<T>(string json)
            {
                throw new NotSupportedException();
            }
        }

        /// UC: UC-CNT-033
        [Test]
        public async Task publish_json_serializes_the_object_and_sends_it_as_json()
        {
            var rig = new Rig();
            rig.Handler.Reply(201, "{\"id\":\"m1\"}").Reply(201, "{\"id\":\"m2\"}");

            await rig.Client.PublishJsonAsync("pedidos", new { pedido = 9001, itens = new[] { "a", "b" } });
            Assert.That(rig.Handler.Last.BodyText, Is.EqualTo("{\"pedido\":9001,\"itens\":[\"a\",\"b\"]}"));
            Assert.That(rig.Handler.Last.Header("content-type"), Does.StartWith("application/json"));

            rig.Config.JsonSerializer = new UpperSerializer();
            await new TabaClient(rig.Config, rig.Handler).PublishJsonAsync("pedidos", "x");
            Assert.That(rig.Handler.Last.BodyText, Is.EqualTo("{\"custom\":\"x\"}"), "o serializador da configuração vale no lugar do padrão");

            Assert.ThrowsAsync<ArgumentNullException>(() => rig.Client.PublishJsonAsync<object>("pedidos", null));
        }

        /// UC: UC-CNT-034
        [Test]
        public async Task consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(200, MessagesJson(MessageJson("m1", "{\"n\":1}")))
                .Reply(204)
                .Reply(200, MessagesJson(MessageJson("m2", "{\"n\":2}")))
                .Reply(200, MessagesJson(MessageJson("m3", "{\"n\":3}")));
            var client = new TabaClient(rig.Config, rig.Handler, (span, token) => Task.Delay(Timeout.Infinite, token));
            string seen = null;

            int handled = await client.ConsumeAsync("pedidos", null, context =>
            {
                seen = context.Message.BodyText + "/" + context.Message.DeliveryCount;
                return Task.FromResult(0);
            });
            Assert.That(handled, Is.EqualTo(1));
            Assert.That(seen, Is.EqualTo("{\"n\":1}/2"), "a mensagem inteira, com todas as propriedades");
            Assert.That(rig.Handler.Requests[0].PathAndQuery, Is.EqualTo("/topics/pedidos/messages?maxMessages=1&visibilityTimeout=120&waitTime=20"), "padrões do consume com handler");
            Assert.That(rig.Handler.Requests.Select(r => r.Method).ToArray(), Is.EqualTo(new[] { "GET", "DELETE" }), "apagada por padrão");

            await client.ConsumeAsync("pedidos", null, context =>
            {
                context.Delete = false;
                return Task.FromResult(0);
            });
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(3), "Delete = false: não apaga");

            var e = Assert.ThrowsAsync<InvalidOperationException>(() => client.ConsumeAsync("pedidos", null, context =>
            {
                throw new InvalidOperationException("não deu");
            }));
            Assert.That(e.Message, Is.EqualTo("não deu"));
            Assert.That(rig.Handler.Requests.Count, Is.EqualTo(4), "com exceção, não apaga");
        }

        /// UC: UC-CNT-034
        [Test]
        public async Task consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(200, MessagesJson(MessageJson("m1", "{}")))
                .Reply(200, "{\"id\":\"m1\",\"invisibleUntil\":\"2026-01-01T00:01:00Z\"}")
                .Reply(200, "{\"id\":\"m1\",\"invisibleUntil\":\"2026-01-01T00:05:00Z\"}")
                .Reply(204);
            var waits = new List<TimeSpan>();
            int calls = 0;
            Func<TimeSpan, CancellationToken, Task> delay = async (span, token) =>
            {
                lock (waits)
                {
                    waits.Add(span);
                }
                if (Interlocked.Increment(ref calls) > 1)
                {
                    await Task.Delay(Timeout.Infinite, token);
                }
            };
            var client = new TabaClient(rig.Config, rig.Handler, delay);

            await client.ConsumeAsync("pedidos", new ConsumeOptions { VisibilityTimeout = 30 }, async context =>
            {
                while (rig.Handler.Requests.Count < 2)
                {
                    await Task.Delay(5);
                }
                Assert.That(context.LeaseSeconds, Is.EqualTo(30));
                await context.ChangeVisibilityAsync(300);
                Assert.That(context.LeaseSeconds, Is.EqualTo(300), "a renovação passa a pedir o novo valor");
            });

            Seen renewal = rig.Handler.Requests[1];
            Assert.That(renewal.Method, Is.EqualTo("PUT"));
            Assert.That(renewal.PathAndQuery, Is.EqualTo("/topics/pedidos/messages/m1/visibility?receiptToken=rt_m1"));
            Assert.That(renewal.BodyText, Is.EqualTo("{\"visibilityTimeout\":30}"));
            Assert.That(rig.Handler.Requests[2].BodyText, Is.EqualTo("{\"visibilityTimeout\":300}"));
            Assert.That(waits[0], Is.EqualTo(TimeSpan.FromSeconds(10)), "renova a cada um terço do prazo, sempre antes de vencer");
            Assert.That(rig.Handler.Requests.Select(r => r.Method).ToArray(), Is.EqualTo(new[] { "GET", "PUT", "PUT", "DELETE" }));
        }

        public sealed class Pedido
        {
            public int Numero { get; set; }

            public List<string> Itens { get; set; }
        }

        /// UC: UC-CNT-034
        [Test]
        public async Task consume_of_t_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t()
        {
            var rig = new Rig();
            rig.Handler
                .Reply(200, MessagesJson(MessageJson("m1", "{\"numero\":9001,\"itens\":[\"a\",\"b\"]}")))
                .Reply(204)
                .Reply(200, MessagesJson(MessageJson("m2", "[1,2]")));
            var client = new TabaClient(rig.Config, rig.Handler, (span, token) => Task.Delay(Timeout.Infinite, token));
            Pedido seen = null;

            await client.ConsumeAsync<Pedido>("pedidos", context =>
            {
                seen = context.Body;
                Assert.That(context.Message.Id, Is.EqualTo("m1"), "a mensagem inteira continua disponível");
                return Task.FromResult(0);
            });
            Assert.That(seen.Numero, Is.EqualTo(9001));
            Assert.That(seen.Itens, Is.EqualTo(new[] { "a", "b" }));

            bool called = false;
            var e = Assert.ThrowsAsync<TabaException>(() => client.ConsumeAsync<Pedido>("pedidos", null, context =>
            {
                called = true;
                return Task.FromResult(0);
            }));
            Assert.That(e.Code, Is.EqualTo("client.invalid-response"));
            Assert.That(called, Is.False);
            Assert.That(rig.Handler.Requests.Select(r => r.Method).ToArray(), Is.EqualTo(new[] { "GET", "DELETE", "GET" }), "a que não é um Pedido não é apagada");
        }

        /// UC: UC-CNT-034
        [Test]
        public void the_sync_consume_with_a_handler_works_like_the_async_one()
        {
            var rig = new Rig();
            rig.Handler.Reply(200, MessagesJson(MessageJson("m1", "{}"))).Reply(204);
            var delay = new Func<TimeSpan, CancellationToken, Task>((span, token) => Task.Delay(Timeout.Infinite, token));
            var client = new TabaClient(rig.Config, rig.Handler, delay);
            string id = null;
            int handled = client.Consume("pedidos", context => id = context.Message.Id);
            Assert.That(handled, Is.EqualTo(1));
            Assert.That(id, Is.EqualTo("m1"));
            Assert.That(rig.Handler.Last.Method, Is.EqualTo("DELETE"));
        }
    }
}
