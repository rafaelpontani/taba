import asyncio
import json
import queue
import threading
import unittest
from typing import Any, List

from taba import (
    BatchMessage,
    CONTRACT_VERSION,
    VERSION,
    Cancelled,
    CancelToken,
    Client,
    Config,
    TabaError,
    TopicRequest,
    TopicUpdate,
)
from taba._calls import consume_call
from taba.models import parse_message
from taba.aio import AsyncClient

from .support import (
    DESTROY,
    HANG,
    ClientTestCase,
    FakeSleep,
    ScriptedServer,
    blocking_sleep,
    message_json,
    reply,
    topic_json,
    wait_for,
)


class OperationsTests(ClientTestCase):
    # UC: UC-CNT-001, UC-CNT-029
    def test_the_nine_operations_build_the_expected_routes(self) -> None:
        c, s = self.client, self.server
        s.fallback(reply(200, topic_json("orders")))
        c.create_topic(TopicRequest("orders", "queue"))
        c.get_topic("orders")
        s.reply(reply(200, {"items": [topic_json("orders")]}))
        c.list_topics(limit=10, cursor="c1", name="ord")
        c.update_topic("orders", TopicUpdate(idle_timeout=60))
        c.delete_topic("orders")
        s.reply(reply(200, {"id": "m1"}))
        c.publish("orders", b'{"a":1}', group_id="g", deduplication_id="d")
        s.reply(reply(200, {"messages": []}))
        c.consume("orders", max_messages=2)
        c.ack("orders", "m 1", "rt")
        s.reply(reply(200, {"id": "m1", "invisibleUntil": "x"}))
        c.change_visibility("orders", "m1", 30, "rt")

        self.assertEqual(
            ["%s %s" % (r.method, r.url) for r in s.requests],
            [
                "POST /topics",
                "GET /topics/orders",
                "GET /topics?limit=10&cursor=c1&name=ord",
                "PATCH /topics/orders",
                "DELETE /topics/orders",
                "POST /topics/orders/messages?groupId=g&deduplicationId=d",
                "GET /topics/orders/messages?maxMessages=2",
                "DELETE /topics/orders/messages/m%201?receiptToken=rt",
                "PUT /topics/orders/messages/m1/visibility?receiptToken=rt",
            ],
        )
        self.assertEqual(s.requests[8].text, '{"visibilityTimeout":30}')

    # UC: UC-CNT-029
    def test_update_topic_is_a_real_patch_without_a_method_override_header(self) -> None:
        self.server.fallback(reply(200, topic_json("orders")))
        self.client.update_topic(
            "orders",
            TopicUpdate(ttl=None, max_deliveries=5, tags={"a": "b"}, prefixes=["x-"]),
        )
        request = self.server.requests[0]
        self.assertEqual(request.method, "PATCH")
        self.assertNotIn("x-http-method-override", request.headers)
        # `None` sends null (back to the server default); an unset field is left out.
        self.assertEqual(
            json.loads(request.text), {"ttl": None, "maxDeliveries": 5, "tags": {"a": "b"}, "prefixes": ["x-"]}
        )

    # UC: UC-CNT-001
    def test_create_topic_sends_only_what_was_filled(self) -> None:
        self.server.fallback(reply(201, topic_json("orders")))
        self.client.create_topic(TopicRequest("orders", "queue"))
        self.client.create_topic(TopicRequest("d", "distributor", prefixes=["p-"], fifo=False, ttl=30, tags={}))
        self.assertEqual(self.server.requests[0].text, '{"name":"orders","type":"queue"}')
        self.assertEqual(
            json.loads(self.server.requests[1].text),
            {"name": "d", "type": "distributor", "prefixes": ["p-"], "fifo": False, "ttl": 30},
        )

    # UC: UC-CNT-018
    def test_headers_carry_the_agent_and_the_optional_ids(self) -> None:
        client = Client(base_url=self.server.url, user_agent="my-app taba-python/9")
        self.addCleanup(client.close)
        self.server.fallback(reply(200, {"id": "m1"}))
        client.publish("q", b"x", request_id="req-1", traceparent="00-abc-def-01")
        client.publish("q", b"x")
        first, second = self.server.requests
        self.assertEqual(first.headers["accept"], "application/json")
        self.assertEqual(first.headers["user-agent"], "my-app taba-python/9")
        self.assertEqual(first.headers["x-request-id"], "req-1")
        self.assertEqual(first.headers["traceparent"], "00-abc-def-01")
        self.assertNotIn("x-request-id", second.headers)
        self.assertNotIn("traceparent", second.headers)

    # UC: UC-CNT-004, UC-CNT-025
    def test_publish_sends_raw_bytes_with_content_type_and_query(self) -> None:
        self.server.fallback(reply(200, {"id": "m1", "duplicate": True}))
        data = bytes([0, 1, 2, 255, 254])
        response = self.client.publish(
            "q", data, content_type="application/octet-stream", group_id="g1", deduplication_id="d1"
        )
        self.assertEqual(response.id, "m1")
        self.assertIs(response.duplicate, True)
        request = self.server.requests[0]
        self.assertEqual(request.body, data)
        self.assertEqual(request.headers["content-type"], "application/octet-stream")
        self.assertEqual(request.query["groupId"], "g1")
        self.assertEqual(request.query["deduplicationId"], "d1")

        self.client.publish_text("q", "olá ✓")
        self.assertEqual(self.server.requests[1].headers["content-type"], "text/plain; charset=utf-8")
        self.assertEqual(self.server.requests[1].text, "olá ✓")
        self.client.publish_raw_json("q", '{"n": 1.0}')
        self.assertEqual(self.server.requests[2].headers["content-type"], "application/json")
        self.assertEqual(self.server.requests[2].text, '{"n": 1.0}')
        self.client.publish("q", b"{}")
        self.assertEqual(self.server.requests[3].headers["content-type"], "application/json")

    # UC: UC-CNT-041
    def test_the_batches_send_one_json_and_read_one_result_per_item(self) -> None:
        self.server.fallback(
            reply(
                200,
                {
                    "results": [
                        {"status": 201, "id": "m1"},
                        {"status": 200, "id": "m0", "duplicate": True},
                        {"status": 400, "error": {"code": "validation.invalid-content-type", "message": "bad"}},
                    ]
                },
            )
        )
        results = self.client.publish_batch(
            "q",
            [
                BatchMessage(
                    bytes([0, 1, 255]), content_type="application/octet-stream", group_id="g1", deduplication_id="d1"
                ),
                BatchMessage("olá", content_type="text/plain; charset=utf-8"),
                BatchMessage(b"{}", content_type="nope"),
            ],
        )
        request = self.server.requests[0]
        self.assertEqual(request.method, "POST")
        self.assertTrue(request.url.startswith("/topics/q/messages/batch"), request.url)
        self.assertEqual(
            request.text,
            '{"messages":[{"body":"AAH/","bodyEncoding":"base64","contentType":"application/octet-stream",'
            '"groupId":"g1","deduplicationId":"d1"},'
            '{"body":"b2zDoQ==","bodyEncoding":"base64","contentType":"text/plain; charset=utf-8"},'
            '{"body":"e30=","bodyEncoding":"base64","contentType":"nope"}]}',
        )
        self.assertTrue(results[0].ok)
        self.assertEqual((results[0].status, results[0].id), (201, "m1"))
        self.assertIs(results[1].duplicate, True)
        self.assertFalse(results[2].ok)
        self.assertEqual(results[2].error_code, "validation.invalid-content-type")

        messages = [parse_message('{"id":"%s","body":1,"receiptToken":"rt-%s"}' % (i, i)) for i in ("m1", "m2")]
        self.server.fallback(
            reply(
                200,
                {
                    "results": [
                        {"status": 204, "id": "m1"},
                        {"status": 409, "error": {"code": "message.stale-receipt", "message": "old"}},
                    ]
                },
            )
        )
        acked = self.client.ack_batch("q", messages)
        request = self.server.requests[1]
        self.assertTrue(request.url.startswith("/topics/q/messages/ack"), request.url)
        self.assertEqual(
            request.text,
            '{"messages":[{"id":"m1","receiptToken":"rt-m1"},{"id":"m2","receiptToken":"rt-m2"}]}',
        )
        self.assertTrue(acked[0].ok)
        self.assertEqual(acked[1].error_code, "message.stale-receipt")

        self.server.fallback(
            reply(200, {"results": [{"status": 200, "id": "m1", "invisibleUntil": "2026-10-29T10:00:00Z"}]})
        )
        changed = self.client.change_visibility_batch("q", messages[:1], 30)
        request = self.server.requests[2]
        self.assertTrue(request.url.startswith("/topics/q/messages/visibility"), request.url)
        self.assertEqual(
            request.text, '{"messages":[{"id":"m1","receiptToken":"rt-m1","visibilityTimeout":30}]}'
        )
        self.assertEqual(changed[0].invisible_until, "2026-10-29T10:00:00Z")

    # UC: UC-CNT-004
    def test_a_distributor_answer_lists_each_copy_and_the_partial_failures(self) -> None:
        self.server.fallback(
            reply(
                200,
                {
                    "matched": 2,
                    "deliveries": [
                        {"topic": "a-1", "id": "m1"},
                        {"topic": "a-2", "error": {"code": "queue.full", "message": "full"}},
                    ],
                },
            )
        )
        response = self.client.publish("dist", b"{}")
        self.assertIsNone(response.id)
        self.assertEqual(response.matched, 2)
        self.assertEqual(len(response.deliveries), 2)
        self.assertTrue(response.deliveries[0].succeeded)
        self.assertEqual(response.deliveries[0].id, "m1")
        self.assertFalse(response.deliveries[1].succeeded)
        self.assertEqual(response.deliveries[1].error_code, "queue.full")
        self.assertEqual(response.deliveries[1].error_message, "full")


class ConfigTests(unittest.TestCase):
    # UC: UC-CNT-002
    def test_config_defaults_and_required_base_url(self) -> None:
        for bad in (None, "", "  ", "ftp://x", "not a url"):
            with self.assertRaises(TabaError) as context:
                Client(Config(base_url=bad))  # type: ignore[arg-type]
            self.assertEqual(context.exception.code, "client.invalid-config")
            self.assertEqual(context.exception.status, 0)
        with self.assertRaises(TabaError):
            Client()
        client = Client("http://localhost:8080/")
        self.assertEqual(client.config.connect_timeout, 5.0)
        self.assertEqual(client.config.request_timeout, 30.0)
        self.assertEqual(client.config.max_attempts, 3)
        self.assertEqual(client.config.user_agent, "taba-python/" + VERSION)
        with self.assertRaises(TypeError):
            Client(Config("http://x"), token="t")

    # UC: UC-CNT-003
    def test_a_token_goes_in_every_call_and_without_it_no_call_carries_authorization(self) -> None:
        server = ScriptedServer()
        self.addCleanup(server.close)
        server.fallback(reply(200, topic_json("a")))
        with Client(base_url=server.url, token="secret") as with_token, Client(server.url) as without:
            with_token.get_topic("a")
            without.get_topic("a")
        self.assertEqual(server.requests[0].headers["authorization"], "Bearer secret")
        self.assertNotIn("authorization", server.requests[1].headers)

    # UC: UC-CNT-028
    def test_unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared(self) -> None:
        server = ScriptedServer()
        self.addCleanup(server.close)
        server.fallback(reply(200, dict(topic_json("a"), novo={"campo": [1, {"x": "y"}]}, outro=None)))
        with Client(server.url) as client:
            self.assertEqual(client.get_topic("a").name, "a")
        self.assertEqual(CONTRACT_VERSION, "1")


class ConsumeTests(ClientTestCase):
    # UC: UC-CNT-005
    def test_consume_sends_only_what_was_given(self) -> None:
        self.server.fallback(reply(200, {"messages": []}))
        self.assertEqual(self.client.consume("q"), [])
        self.client.consume("q", max_messages=5, visibility_timeout=60, wait_time=15)
        self.assertEqual(self.server.requests[0].url, "/topics/q/messages")
        self.assertEqual(
            self.server.requests[1].url, "/topics/q/messages?maxMessages=5&visibilityTimeout=60&waitTime=15"
        )
        self.client.consume("q", auto_create=True, volatile=True, idle_timeout=30, fifo=False, tags={"env": "dev"})
        query = self.server.requests[2].query
        self.assertEqual(query["autoCreate"], "true")
        self.assertEqual(query["volatile"], "true")
        self.assertEqual(query["idleTimeout"], "30")
        self.assertEqual(query["fifo"], "false")
        self.assertEqual(query["tag.env"], "dev")

    # UC: UC-CNT-005
    def test_auto_create_parameters_are_left_out_without_auto_create(self) -> None:
        from taba import ConsumeOptions

        call = consume_call("q", ConsumeOptions(volatile=True, idle_timeout=30, fifo=True, tags={"a": "b"}))
        self.assertEqual(call.query, [])

    # UC: UC-CNT-008
    def test_the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout(self) -> None:
        from taba import ConsumeOptions

        self.assertEqual(consume_call("q", ConsumeOptions(wait_time=20)).timeout, 30.0)
        self.assertEqual(consume_call("q", ConsumeOptions(wait_time=25)).timeout, 35.0)
        self.assertEqual(consume_call("q", ConsumeOptions()).timeout, 10.0)

    # UC: UC-CNT-006
    def test_ack_and_visibility_put_the_receipt_token_in_the_query(self) -> None:
        self.server.fallback(reply(200, {"id": "m1", "invisibleUntil": "2026-10-27T10:05:00Z"}))
        self.client.ack("q", "m1", "a b&c=d")
        visibility = self.client.change_visibility("q", "m1", 45, "rt")
        self.assertEqual(self.server.requests[0].query["receiptToken"], "a b&c=d")
        self.assertEqual(self.server.requests[1].query["receiptToken"], "rt")
        self.assertEqual(visibility.invisible_until, "2026-10-27T10:05:00Z")
        self.assert_error("client.invalid-request", lambda: self.client.ack("q", "m1"))

    # UC: UC-CNT-007
    def test_list_all_topics_follows_next_cursor_until_the_last_page(self) -> None:
        self.server.reply(
            reply(200, {"items": [topic_json("a"), topic_json("b")], "nextCursor": "c2"}),
            reply(200, {"items": [topic_json("c")], "nextCursor": None}),
        )
        topics = self.client.list_all_topics("x")
        self.assertEqual([t.name for t in topics], ["a", "b", "c"])
        self.assertNotIn("cursor", self.server.requests[0].query)
        self.assertEqual(self.server.requests[1].query["cursor"], "c2")
        self.assertEqual(self.server.requests[1].query["name"], "x")

    # UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
    def test_messages_come_back_as_bytes_for_json_text_and_base64(self) -> None:
        messages = [
            '{"id":"m1","body":{"pedido": 9001, "valor": 1.0, "grande": 12345678901234567890},"contentType":"application/json",'
            '"receiptToken":"rt-m1","deliveryCount":3,"publishedAt":"p","invisibleUntil":"i","groupId":"g","deduplicationId":"d",'
            '"novo":{"campo":[1,2]}}',
            '{"id":"m2","body":"olá ✓ \\u00e9","contentType":"text/plain; charset=utf-8","receiptToken":"rt-m2"}',
            '{"id":"m3","body":"AAEC/w==","contentType":"application/octet-stream","bodyEncoding":"base64","receiptToken":"rt-m3"}',
            '{"id":"m4","body":{"a":1},"contentType":"application/vnd.taba+json","receiptToken":"rt-m4",'
            '"deadLetteredAt":"d","deadLetterReason":"max-deliveries"}',
        ]
        self.server.fallback(reply(200, '{"messages":[' + ",".join(messages) + "]}"))
        m1, m2, m3, m4 = self.client.consume("q")
        # The JSON text comes exactly as the server sent it, not re-serialized.
        self.assertEqual(m1.body_text, '{"pedido": 9001, "valor": 1.0, "grande": 12345678901234567890}')
        self.assertIsInstance(m1.body, bytes)
        self.assertEqual(m1.delivery_count, 3)
        self.assertEqual(m1.group_id, "g")
        self.assertEqual(m1.deduplication_id, "d")
        self.assertEqual(m1.receipt_token, "rt-m1")
        self.assertEqual(m1.published_at, "p")
        self.assertEqual(m1.invisible_until, "i")
        self.assertIsNone(m1.dead_lettered_at)
        self.assertEqual(m2.body_text, "olá ✓ é")
        self.assertEqual(m2.content_type, "text/plain; charset=utf-8")
        self.assertEqual(m3.body, bytes([0, 1, 2, 255]))
        self.assertEqual(m4.body_text, '{"a":1}')
        self.assertEqual(m4.dead_lettered_at, "d")
        self.assertEqual(m4.dead_letter_reason, "max-deliveries")
        self.assertEqual(m4.json(), {"a": 1})

    # UC: UC-CNT-026
    def test_an_invalid_answer_is_an_invalid_response_error_and_not_a_crash(self) -> None:
        self.server.reply(
            reply(200, "not json"),
            reply(200, '{"messages":[{"id":"m1","body":"AAA","bodyEncoding":"base64"}]}'),
            reply(200, '{"messages":[{"id":"m1","body":"x"'),
            reply(200, "[1,2]"),
        )
        self.assert_error("client.invalid-response", lambda: self.client.get_topic("a"))
        self.assert_error("client.invalid-response", lambda: self.client.consume("q"))
        self.assert_error("client.invalid-response", lambda: self.client.consume("q"))
        self.assert_error("client.invalid-response", lambda: self.client.get_topic("a"))


class RetryTests(ClientTestCase):
    # UC: UC-CNT-009, UC-CNT-017
    def test_a_timeout_is_not_repeated_and_is_a_client_side_error(self) -> None:
        client = Client(base_url=self.server.url, request_timeout=0.2, _sleep=self.sleep)
        self.addCleanup(client.close)
        self.server.fallback(HANG)
        error = self.assert_error("client.timeout", lambda: client.get_topic("a"))
        self.assertEqual(error.status, 0)
        self.assertTrue(error.is_client_side)
        self.assertEqual(len(self.server.requests), 1)

    # UC: UC-CNT-010
    def test_retries_429_and_503_waiting_for_retry_after_then_backoff(self) -> None:
        self.server.reply(
            reply(429, {"code": "rate.limited", "message": "slow"}, {"Retry-After": "7"}),
            reply(503, {"code": "node.unavailable", "message": "x"}, {"Retry-After": "2"}),
            reply(200, topic_json("a")),
        )
        self.assertEqual(self.client.get_topic("a").name, "a")
        self.assertEqual(len(self.server.requests), 3)
        self.assertEqual(self.sleep.waits, [7.0, 2.0])

    # UC: UC-CNT-010
    def test_backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts(self) -> None:
        client = Client(base_url=self.server.url, max_attempts=4, _sleep=self.sleep)
        self.addCleanup(client.close)
        self.server.fallback(reply(503, {"code": "node.unavailable", "message": "x"}))
        error = self.assert_error("node.unavailable", lambda: client.get_topic("a"))
        self.assertEqual(error.status, 503)
        self.assertEqual(len(self.server.requests), 4)
        self.assertEqual(self.sleep.waits, [1.0, 2.0, 4.0])

        once = Client(base_url=self.server.url, max_attempts=1, _sleep=self.sleep)
        self.addCleanup(once.close)
        self.server.fallback(reply(429, {"code": "rate.limited", "message": "x"}))
        self.assert_error("rate.limited", lambda: once.get_topic("a"))
        self.assertEqual(len(self.server.requests), 5)

    # UC: UC-CNT-011
    def test_after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success(self) -> None:
        self.server.reply(
            reply(503, {"code": "node.unavailable", "message": "x"}, {"Retry-After": "0"}),
            reply(409, {"code": "topic.already-exists", "message": "exists"}),
            reply(200, topic_json("orders")),
            reply(503, {"code": "node.unavailable", "message": "x"}, {"Retry-After": "0"}),
            reply(404, {"code": "topic.not-found", "message": "gone"}),
        )
        topic = self.client.create_topic(TopicRequest("orders", "queue"))
        self.assertEqual(topic.name, "orders")
        self.assertEqual([r.method for r in self.server.requests], ["POST", "POST", "GET"])
        self.client.delete_topic("orders")
        self.assertEqual(len(self.server.requests), 5)

    # UC: UC-CNT-011, UC-CNT-014
    def test_without_a_previous_503_a_409_or_a_404_is_a_plain_error(self) -> None:
        self.server.reply(
            reply(409, {"code": "topic.already-exists", "message": "exists"}),
            reply(404, {"code": "topic.not-found", "message": "gone"}),
        )
        self.assert_error("topic.already-exists", lambda: self.client.create_topic(TopicRequest("a", "queue")))
        self.assert_error("topic.not-found", lambda: self.client.delete_topic("a"))
        self.assertEqual(len(self.server.requests), 2)

    # UC: UC-CNT-012
    def test_gateway_errors_are_only_repeated_when_publishing_to_a_fifo(self) -> None:
        self.server.reply(reply(502, "bad gateway"), reply(504, "timeout"), reply(200, {"id": "m1"}))
        response = self.client.publish("q", b"{}", fifo=True, deduplication_id="d")
        self.assertEqual(response.id, "m1")
        self.assertEqual(len(self.server.requests), 3)

        self.server.fallback(reply(502, "bad gateway"))
        self.assertEqual(self.assert_error("http.502", lambda: self.client.publish("q", b"{}")).status, 502)
        self.assertEqual(len(self.server.requests), 4)
        self.server.fallback(reply(504, "timeout"))
        self.assertEqual(self.assert_error("http.504", lambda: self.client.get_topic("a")).status, 504)
        self.assertEqual(len(self.server.requests), 5)

    # UC: UC-CNT-013
    def test_network_failure_after_sending_is_not_repeated(self) -> None:
        self.server.reply(DESTROY)
        error = self.assert_error("client.network", lambda: self.client.publish("q", b"{}"))
        self.assertEqual(error.status, 0)
        self.assertEqual(len(self.server.requests), 1)
        self.assertEqual(self.sleep.waits, [])

    # UC: UC-CNT-013
    def test_a_refused_connection_is_repeated_and_then_a_network_error(self) -> None:
        server = ScriptedServer()
        url = server.url
        server.close()
        sleep = FakeSleep()
        with Client(base_url=url, max_attempts=2, _sleep=sleep) as client:
            error = self.assert_error("client.network", lambda: client.publish("q", b"{}"))
        self.assertEqual(error.status, 0)
        self.assertEqual(sleep.waits, [1.0])

    # UC: UC-CNT-013
    def test_a_refused_connection_succeeds_when_the_node_comes_back(self) -> None:
        server = ScriptedServer()
        port = server.port
        server.close()
        revived: List[ScriptedServer] = []

        def sleep(seconds: float, token: Any) -> None:
            if not revived:
                node = ScriptedServer(port)
                node.fallback(reply(200, topic_json("a")))
                revived.append(node)
                self.addCleanup(node.close)

        with Client(base_url="http://127.0.0.1:%d" % port, _sleep=sleep) as client:
            self.assertEqual(client.get_topic("a").name, "a")

    # UC: UC-CNT-014
    def test_a_4xx_other_than_429_is_never_repeated(self) -> None:
        for status in (400, 401, 403, 404, 409, 422):
            self.server.reply(reply(status, {"code": "x.%d" % status, "message": "no"}))
            self.assertEqual(self.assert_error("x.%d" % status, lambda: self.client.get_topic("a")).status, status)
        self.assertEqual(len(self.server.requests), 6)
        self.assertEqual(self.sleep.waits, [])

    # UC: UC-CNT-017, UC-CNT-018
    def test_the_error_carries_the_error_response_fields_and_the_headers(self) -> None:
        self.server.reply(
            reply(
                422,
                {
                    "code": "topic.invalid-name",
                    "message": "bad name",
                    "requestId": "body-id",
                    "details": [{"field": "name", "reason": "x"}],
                },
                {"Retry-After": "9", "X-Request-Id": "header-id"},
            ),
            reply(400, {"code": "x.y", "message": "m"}, {"X-Request-Id": "header-id"}),
        )
        error = self.assert_error("topic.invalid-name", lambda: self.client.create_topic(TopicRequest("!", "queue")))
        self.assertEqual(error.status, 422)
        self.assertEqual(error.message, "bad name")
        self.assertEqual(error.request_id, "body-id")
        self.assertEqual(error.retry_after, 9)
        self.assertEqual(json.loads(error.details or ""), [{"field": "name", "reason": "x"}])
        self.assertTrue(error.body)
        self.assertIn("topic.invalid-name: bad name", str(error))
        self.assertEqual(self.assert_error("x.y", lambda: self.client.get_topic("a")).request_id, "header-id")

    # UC: UC-CNT-017
    def test_a_non_json_error_still_reports_a_status_code(self) -> None:
        self.server.reply(reply(500, "<html>boom</html>", {"Content-Type": "text/html"}), reply(500, ""))
        html = self.assert_error("http.500", lambda: self.client.get_topic("a"))
        self.assertEqual(html.message, "<html>boom</html>")
        empty = self.assert_error("http.500", lambda: self.client.get_topic("a"))
        self.assertIn("no body", empty.message)

    # UC: UC-CNT-020
    def test_many_threads_can_use_one_client_at_the_same_time(self) -> None:
        self.server.fallback(reply(200, topic_json("a")))
        results: List[str] = []
        errors: List[BaseException] = []

        def work() -> None:
            try:
                for _ in range(5):
                    results.append(self.client.get_topic("a").name)
            except BaseException as e:  # noqa: BLE001
                errors.append(e)

        threads = [threading.Thread(target=work) for _ in range(10)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(errors, [])
        self.assertEqual(len(results), 50)
        self.assertEqual(len(self.server.requests), 50)

    # UC: UC-CNT-020
    def test_cancelling_the_token_cancels_the_call_without_a_client_error(self) -> None:
        self.server.fallback(HANG)
        token = CancelToken()
        threading.Timer(0.1, token.cancel).start()
        with self.assertRaises(Cancelled):
            self.client.consume("q", wait_time=20, cancel=token)
        self.assertEqual(len(self.server.requests), 1)
        with self.assertRaises(Cancelled):
            self.client.get_topic("a", cancel=token)
        self.assertEqual(len(self.server.requests), 1)


class HandlerTests(ClientTestCase):
    def setUp(self) -> None:
        super().setUp()
        self.client = Client(base_url=self.server.url, _sleep=blocking_sleep)
        self.addCleanup(self.client.close)

    # UC: UC-CNT-031
    def test_subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel(self) -> None:
        token = CancelToken()
        self.server.batches(
            [message_json("m1", {"n": 1})], [message_json("m2", {"n": 2})], [message_json("m3", {"n": 3})]
        )
        handled: List[str] = []
        failures: List[Any] = []

        def handler(message: Any) -> None:
            handled.append(message.id)
            if message.id == "m2":
                raise RuntimeError("boom")
            if message.id == "m3":
                token.cancel()

        self.client.subscribe("q", handler, cancel=token, on_error=lambda m, e: failures.append((m.id, str(e))))
        self.assertEqual(handled, ["m1", "m2", "m3"])
        self.assertEqual(failures, [("m2", "boom")])
        acks = [r.url for r in self.server.requests if r.method == "DELETE"]
        self.assertEqual(acks, ["/topics/q/messages/m1?receiptToken=rt-m1", "/topics/q/messages/m3?receiptToken=rt-m3"])
        first = self.server.requests[0].query
        self.assertEqual(first["waitTime"], "20")
        self.assertEqual(first["maxMessages"], "1")

    # UC: UC-CNT-031
    def test_subscribe_returns_without_an_error_when_cancelled_during_the_long_polling(self) -> None:
        token = CancelToken()
        self.server.fallback(HANG)
        threading.Timer(0.1, token.cancel).start()
        self.client.subscribe("q", lambda message: None, cancel=token)
        self.assertEqual(len(self.server.requests), 1)

    # UC: UC-CNT-033
    def test_publish_json_serializes_the_object_and_sends_it_as_json(self) -> None:
        self.server.fallback(reply(200, {"id": "m1"}))
        self.client.publish_json("q", {"pedido": 9001, "itens": ["a"]})
        self.assertEqual(self.server.requests[0].headers["content-type"], "application/json")
        self.assertEqual(json.loads(self.server.requests[0].text), {"pedido": 9001, "itens": ["a"]})

        class Own:
            @staticmethod
            def dumps(value: Any) -> str:
                return '{"own":true}'

            loads = staticmethod(json.loads)

        with Client(base_url=self.server.url, serializer=Own) as own:
            own.publish_json("q", {"x": 1})
        self.assertEqual(self.server.requests[1].text, '{"own":true}')
        self.assert_error("client.invalid-request", lambda: self.client.publish_json("q", None))
        self.assert_error("client.invalid-request", lambda: self.client.publish_json("q", {"x": object()}))

    # UC: UC-CNT-034
    def test_consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails(self) -> None:
        self.server.batches(
            [message_json("m1", {"n": 1})], [message_json("m2", {"n": 2})], [message_json("m3", {"n": 3})]
        )

        def first(context: Any) -> None:
            self.assertEqual(context.message.id, "m1")
            self.assertTrue(context.delete)

        def keep(context: Any) -> None:
            context.delete = False

        def fail(context: Any) -> None:
            raise RuntimeError("handler failed")

        self.assertEqual(self.client.consume_with("q", first), 1)
        self.assertEqual(self.client.consume_with("q", keep), 1)
        with self.assertRaisesRegex(RuntimeError, "handler failed"):
            self.client.consume_with("q", fail)
        self.assertEqual([r.method for r in self.server.requests], ["GET", "DELETE", "GET", "GET"])
        poll = self.server.requests[0].query
        self.assertEqual(poll["maxMessages"], "1")
        self.assertEqual(poll["visibilityTimeout"], "120")
        self.assertEqual(poll["waitTime"], "20")

    # UC: UC-CNT-034
    def test_consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it(self) -> None:
        server = ScriptedServer()
        self.addCleanup(server.close)
        server.batches([message_json("m1", {"n": 1})])
        ticks: "queue.Queue[int]" = queue.Queue()

        def sleep(seconds: float, token: Any) -> None:
            # Each wait of the renewal ends only when the test says so (a "tick").
            while True:
                token.raise_if_cancelled()
                try:
                    ticks.get(timeout=0.02)
                    return
                except queue.Empty:
                    pass

        def renewals() -> List[Any]:
            return [r for r in server.requests if r.method == "PUT"]

        def handler(context: Any) -> None:
            ticks.put(1)
            ticks.put(1)
            wait_for(lambda: len(renewals()) == 2)
            self.assertEqual(context.lease_seconds, 90)
            context.change_visibility(300)
            self.assertEqual(context.lease_seconds, 300)
            ticks.put(1)
            ticks.put(1)
            wait_for(lambda: len(renewals()) == 5)

        with Client(base_url=server.url, _sleep=sleep) as client:
            client.consume_with("q", handler, visibility_timeout=90)
        bodies = [json.loads(r.text)["visibilityTimeout"] for r in renewals()]
        self.assertEqual(bodies, [90, 90, 300, 300, 300])
        self.assertEqual(server.requests[-1].method, "DELETE")

    # UC: UC-CNT-034
    def test_a_refused_renewal_marks_the_lease_as_lost_and_the_delete_is_tolerated(self) -> None:
        server = ScriptedServer()
        self.addCleanup(server.close)
        gone = reply(409, {"code": "message.receipt-expired", "message": "x"})
        server.reply(reply(200, {"messages": [message_json("m1", {"n": 1})]}), gone)
        server.fallback(gone)
        rounds = [0]

        def sleep(seconds: float, token: Any) -> None:
            rounds[0] += 1
            if rounds[0] > 1:
                token.wait(30)
                token.raise_if_cancelled()

        seen: List[bool] = []

        def handler(context: Any) -> None:
            wait_for(lambda: context.lease_lost)
            seen.append(context.lease_lost)

        with Client(base_url=server.url, _sleep=sleep) as client:
            client.consume_with("q", handler)
        self.assertEqual(seen, [True])

    # UC: UC-CNT-034
    def test_consume_json_hands_over_the_parsed_body_and_refuses_a_body_that_is_not_json(self) -> None:
        self.server.batches(
            [message_json("m1", {"pedido": 9001})],
            [message_json("m2", "not json", contentType="text/plain")],
        )
        bodies: List[Any] = []
        self.client.consume_json("q", lambda context: bodies.append(context.body))
        self.assertEqual(bodies, [{"pedido": 9001}])
        self.assert_error(
            "client.invalid-response",
            lambda: self.client.consume_json("q", lambda context: self.fail("the handler must not run")),
        )
        self.assertEqual([r.method for r in self.server.requests], ["GET", "DELETE", "GET"])


class AsyncTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.server = ScriptedServer()
        self.addCleanup(self.server.close)

    # UC: UC-CNT-032
    async def test_the_async_forms_do_the_same_as_the_sync_ones(self) -> None:
        self.server.fallback(reply(200, {"items": [topic_json("a")], "messages": [], "id": "m1", "name": "a"}))
        async with AsyncClient(self.server.url, token="tk") as client:
            await client.create_topic(TopicRequest("a", "queue"))
            await client.get_topic("a")
            page = await client.list_topics(limit=1)
            await client.list_all_topics()
            await client.update_topic("a", TopicUpdate())
            await client.delete_topic("a")
            response = await client.publish("a", b"x", group_id="g")
            await client.publish_json("a", {"x": 1})
            await client.publish_text("a", "t")
            await client.publish_raw_json("a", "{}")
            await client.consume("a", wait_time=1)
            await client.ack("a", "m1", "rt")
            await client.change_visibility("a", "m1", 5, "rt")
        self.assertEqual(response.id, "m1")
        self.assertEqual(len(page.items), 1)
        self.assertEqual(len(self.server.requests), 13)
        self.assertEqual(self.server.requests[0].headers["authorization"], "Bearer tk")
        self.assertEqual(self.server.requests[6].url, "/topics/a/messages?groupId=g")

    # UC: UC-CNT-032
    async def test_errors_and_retries_are_the_same_in_the_async_client(self) -> None:
        self.server.reply(reply(503, {"code": "node.unavailable", "message": "x"}, {"Retry-After": "0"}))
        self.server.reply(reply(404, {"code": "topic.not-found", "message": "gone"}))
        async with AsyncClient(self.server.url) as client:
            with self.assertRaises(TabaError) as context:
                await client.get_topic("a")
        self.assertEqual(context.exception.code, "topic.not-found")
        self.assertEqual(len(self.server.requests), 2)

    # UC: UC-CNT-020
    async def test_cancelling_the_task_cancels_the_call_in_flight(self) -> None:
        self.server.fallback(HANG)
        async with AsyncClient(self.server.url) as client:
            task = asyncio.ensure_future(client.consume("q", wait_time=20))
            await asyncio.sleep(0.2)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
        self.assertEqual(len(self.server.requests), 1)

    # UC: UC-CNT-031
    async def test_the_async_subscribe_acks_only_what_the_handler_accepted_and_stops_when_the_task_is_cancelled(self) -> None:
        self.server.batches([message_json("m1", {"n": 1})], [message_json("m2", {"n": 2})])
        handled: List[str] = []
        failures: List[str] = []

        async def handler(message: Any) -> None:
            handled.append(message.id)
            if message.id == "m2":
                raise RuntimeError("boom")

        async with AsyncClient(self.server.url) as client:
            task = asyncio.ensure_future(
                client.subscribe("q", handler, on_error=lambda m, e: failures.append(m.id))
            )
            while len(handled) < 2:
                await asyncio.sleep(0.01)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
        self.assertEqual(handled[:2], ["m1", "m2"])
        self.assertEqual(failures, ["m2"])
        acks = [r.url for r in self.server.requests if r.method == "DELETE"]
        self.assertEqual(acks, ["/topics/q/messages/m1?receiptToken=rt-m1"])

    # UC: UC-CNT-034
    async def test_the_async_consume_with_renews_the_visibility_and_deletes_when_the_handler_returns(self) -> None:
        self.server.batches([message_json("m1", {"pedido": 9001})])
        rounds = 0

        async def sleep(seconds: float) -> None:
            nonlocal rounds
            rounds += 1
            if rounds > 2:
                await asyncio.sleep(30)

        def renewals() -> List[Any]:
            return [r for r in self.server.requests if r.method == "PUT"]

        async def handler(context: Any) -> None:
            self.assertEqual(context.body, {"pedido": 9001})
            while len(renewals()) < 2:
                await asyncio.sleep(0.01)
            await context.change_visibility(200)
            self.assertEqual(context.lease_seconds, 200)

        async with AsyncClient(self.server.url, _sleep=sleep) as client:
            self.assertEqual(await client.consume_json("q", handler, visibility_timeout=90), 1)
        self.assertEqual([json.loads(r.text)["visibilityTimeout"] for r in renewals()][:2], [90, 90])
        self.assertEqual(self.server.requests[-1].method, "DELETE")
