package taba

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

var ctxb = context.Background()

func decodeBody(t *testing.T, r recorded) map[string]interface{} {
	var out map[string]interface{}
	if err := json.Unmarshal(r.Body, &out); err != nil {
		t.Fatalf("body is not JSON: %q", r.text())
	}
	return out
}

// UC: UC-CNT-001, UC-CNT-029
func TestTheNineOperationsBuildTheExpectedRoutes(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, topicJSON("a b")))

	f.client.CreateTopic(ctxb, &TopicRequest{Name: "a b", Type: "queue"})
	f.client.GetTopic(ctxb, "a b")
	f.client.ListTopics(ctxb, Int(5), "cur", "ord")
	f.client.UpdateTopic(ctxb, "a b", &TopicUpdate{IdleTimeout: Int(9)})
	f.client.DeleteTopic(ctxb, "a b")
	f.client.Publish(ctxb, "a b", []byte("{}"), nil)
	f.client.Consume(ctxb, "a b", nil)
	f.client.AckByID(ctxb, "a b", "m 1", "r/1")
	f.client.ChangeVisibilityByID(ctxb, "a b", "m 1", "r/1", 30)

	got := f.server.all()
	want := []struct{ method, path string }{
		{"POST", "/topics"},
		{"GET", "/topics/a%20b"},
		{"GET", "/topics"},
		{"PATCH", "/topics/a%20b"},
		{"DELETE", "/topics/a%20b"},
		{"POST", "/topics/a%20b/messages"},
		{"GET", "/topics/a%20b/messages"},
		{"DELETE", "/topics/a%20b/messages/m%201"},
		{"PUT", "/topics/a%20b/messages/m%201/visibility"},
	}
	if len(got) != len(want) {
		t.Fatalf("want %d requests, got %d", len(want), len(got))
	}
	for i, w := range want {
		assertEqual(t, w.method, got[i].Method, "method")
		assertEqual(t, w.path, got[i].Path, "path")
	}
	assertEqual(t, "5", got[2].Query["limit"], "limit")
	assertEqual(t, "cur", got[2].Query["cursor"], "cursor")
	assertEqual(t, "ord", got[2].Query["name"], "name")
	assertEqual(t, "r/1", got[7].Query["receiptToken"], "ack receipt")
	assertEqual(t, `{"idleTimeout":9}`, got[3].text(), "patch body")
	assertEqual(t, `{"visibilityTimeout":30}`, got[8].text(), "visibility body")
	// Taba does not accept a verb override: every verb is real.
	for _, r := range got {
		if r.Header.Get("X-HTTP-Method-Override") != "" {
			t.Errorf("%s %s carries a method override header", r.Method, r.Path)
		}
	}
}

// UC: UC-CNT-001
func TestCreateTopicSendsOnlyWhatWasFilled(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(201, topicJSON("q")))
	f.client.CreateTopic(ctxb, &TopicRequest{Name: "q", Type: "queue"})
	assertEqual(t, `{"name":"q","type":"queue"}`, f.server.all()[0].text(), "minimal body")

	f.client.CreateTopic(ctxb, &TopicRequest{
		Name: "d", Type: "distributor", Prefixes: []string{"a-", "b-"}, Tags: map[string]string{"k": "v"},
		Fifo: Bool(true), TTL: Int(60), MaxDeliveries: Int(3), Volatile: Bool(false), IdleTimeout: Int(5), MaxMessageBytes: Int(1024),
	})
	body := decodeBody(t, f.server.all()[1])
	for _, key := range []string{"prefixes", "tags", "fifo", "ttl", "maxDeliveries", "volatile", "idleTimeout", "maxMessageBytes"} {
		if _, ok := body[key]; !ok {
			t.Errorf("%s is missing from %v", key, body)
		}
	}
	// PATCH: nil leaves out, ClearInt sends null, SetInt sends the value.
	f.client.UpdateTopic(ctxb, "q", &TopicUpdate{TTL: ClearInt(), MaxDeliveries: SetInt(4)})
	assertEqual(t, `{"maxDeliveries":4,"ttl":null}`, normalized(f.server.all()[2].Body), "patch body")
}

func normalized(body []byte) string {
	var m map[string]interface{}
	json.Unmarshal(body, &m)
	out, _ := json.Marshal(m)
	return string(out)
}

// UC: UC-CNT-018
func TestHeadersCarryTheAgentAndTheOptionalIDs(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.client.Publish(ctxb, "q", []byte("x"), &PublishOptions{RequestID: "req-1", Traceparent: "00-aa-bb-01"})
	f.client.Consume(ctxb, "q", &ConsumeOptions{RequestID: "req-2"})
	got := f.server.all()
	assertEqual(t, "req-1", got[0].Header.Get("X-Request-Id"), "request id")
	assertEqual(t, "00-aa-bb-01", got[0].Header.Get("traceparent"), "traceparent")
	assertEqual(t, "application/json", got[0].Header.Get("Accept"), "accept")
	if !strings.HasPrefix(got[0].Header.Get("User-Agent"), "taba-go/") {
		t.Errorf("user agent is %q", got[0].Header.Get("User-Agent"))
	}
	assertEqual(t, "req-2", got[1].Header.Get("X-Request-Id"), "consume request id")
	assertEqual(t, "", got[1].Header.Get("traceparent"), "no traceparent")
}

// UC: UC-CNT-004, UC-CNT-025
func TestPublishSendsRawBytesWithContentTypeAndQuery(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(201, map[string]interface{}{"id": "m1", "duplicate": true}))
	payload := []byte{0, 1, 2, 255}
	r, err := f.client.Publish(ctxb, "q", payload, &PublishOptions{ContentType: "application/octet-stream", GroupID: "g 1", DeduplicationID: "d1"})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "m1", r.ID, "id")
	assertEqual(t, true, *r.Duplicate, "duplicate")
	got := f.server.all()[0]
	if !bytes.Equal(got.Body, payload) {
		t.Errorf("body %v", got.Body)
	}
	assertEqual(t, "application/octet-stream", got.Header.Get("Content-Type"), "content type")
	assertEqual(t, "g 1", got.Query["groupId"], "group")
	assertEqual(t, "d1", got.Query["deduplicationId"], "dedup")
	// No content type: application/json. The connector does not validate the body.
	f.client.Publish(ctxb, "q", []byte("not json"), nil)
	assertEqual(t, "application/json", f.server.all()[1].Header.Get("Content-Type"), "default content type")
	f.client.PublishText(ctxb, "q", "olá", nil)
	assertEqual(t, "text/plain; charset=utf-8", f.server.all()[2].Header.Get("Content-Type"), "text content type")
	assertEqual(t, "olá", f.server.all()[2].text(), "text body")
}

// UC: UC-CNT-041
func TestTheBatchesSendOneJSONAndReadOneResultPerItem(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(200, map[string]interface{}{"results": []interface{}{
			map[string]interface{}{"status": 201, "id": "a"},
			map[string]interface{}{"status": 200, "id": "b", "duplicate": true},
			map[string]interface{}{"status": 400, "error": map[string]string{"code": "message.invalid-content-type", "message": "bad"}},
		}}),
		reply(200, map[string]interface{}{"results": []interface{}{
			map[string]interface{}{"status": 204},
			map[string]interface{}{"status": 409, "error": map[string]string{"code": "message.receipt-invalid", "message": "old"}},
		}}),
		reply(200, map[string]interface{}{"results": []interface{}{map[string]interface{}{"status": 200, "id": "m1", "invisibleUntil": "2026-10-27T10:00:30Z"}}}),
	)
	results, err := f.client.PublishBatch(ctxb, "q", []BatchMessage{
		{Body: []byte("one"), ContentType: "text/plain; charset=utf-8", GroupID: "g"},
		{Body: []byte{0, 255}},
		{Body: []byte("x"), ContentType: "not a type"},
	})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 3, len(results), "results")
	assertEqual(t, true, results[0].OK(), "first ok")
	assertEqual(t, true, *results[1].Duplicate, "duplicate")
	assertEqual(t, false, results[2].OK(), "third failed alone")
	assertEqual(t, "message.invalid-content-type", results[2].ErrorCode, "third code")

	sent := f.server.all()[0]
	assertEqual(t, "POST", sent.Method, "method")
	assertEqual(t, "/topics/q/messages/batch", sent.Path, "path")
	items := decodeBody(t, sent)["messages"].([]interface{})
	first := items[0].(map[string]interface{})
	assertEqual(t, "b25l", first["body"], "base64 of one")
	assertEqual(t, "base64", first["bodyEncoding"], "encoding")
	assertEqual(t, "g", first["groupId"], "group")
	if _, ok := items[1].(map[string]interface{})["contentType"]; ok {
		t.Error("a content type nobody set must not be sent")
	}

	messages := []*Message{{ID: "m1", ReceiptToken: "r1"}, {ID: "m2", ReceiptToken: "r2"}}
	acked, err := f.client.AckBatch(ctxb, "q", messages)
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 204, acked[0].Status, "ack status")
	assertEqual(t, "message.receipt-invalid", acked[1].ErrorCode, "old receipt fails alone")
	assertEqual(t, "/topics/q/messages/ack", f.server.all()[1].Path, "ack path")
	assertEqual(t, `{"messages":[{"id":"m1","receiptToken":"r1"},{"id":"m2","receiptToken":"r2"}]}`, f.server.all()[1].text(), "ack body")

	visible, _ := f.client.ChangeVisibilityBatch(ctxb, "q", messages[:1], 30)
	assertEqual(t, "2026-10-27T10:00:30Z", visible[0].InvisibleUntil, "invisible until")
	assertEqual(t, "/topics/q/messages/visibility", f.server.all()[2].Path, "visibility path")
	assertEqual(t, `{"messages":[{"id":"m1","receiptToken":"r1","visibilityTimeout":30}]}`, f.server.all()[2].text(), "visibility body")
}

// UC: UC-CNT-004
func TestADistributorAnswerListsEachCopyAndThePartialFailures(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, `{"matched":2,"deliveries":[{"topic":"a-1","id":"x"},{"topic":"a-2","error":{"code":"queue.full","message":"full"}}]}`))
	r, err := f.client.Publish(ctxb, "d", []byte("{}"), nil)
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 2, *r.Matched, "matched")
	assertEqual(t, "", r.ID, "no id")
	assertEqual(t, true, r.Deliveries[0].Succeeded(), "first copy")
	assertEqual(t, false, r.Deliveries[1].Succeeded(), "second copy")
	assertEqual(t, "queue.full", r.Deliveries[1].ErrorCode, "second code")
}

// UC: UC-CNT-002
func TestConfigDefaultsAndRequiredBaseURL(t *testing.T) {
	for _, bad := range []string{"", "  ", "localhost:8080", "ftp://x", "http://"} {
		_, err := NewClient(Config{BaseURL: bad})
		assertError(t, "client.invalid-config", err)
	}
	c, err := NewClient(Config{BaseURL: "http://localhost:8080"})
	if err != nil {
		t.Fatal(err)
	}
	config := c.Config()
	assertEqual(t, 5*time.Second, config.ConnectTimeout, "connect timeout")
	assertEqual(t, 30*time.Second, config.RequestTimeout, "request timeout")
	assertEqual(t, 3, config.MaxAttempts, "attempts")
	assertEqual(t, "taba-go/"+Version, config.UserAgent, "user agent")
}

// UC: UC-CNT-003
func TestATokenGoesInEveryCallAndWithoutItNoCallCarriesAuthorization(t *testing.T) {
	with := newFixture(t, func(c *Config) { c.Token = "tb_x.secret" })
	defer with.close()
	with.client.GetTopic(ctxb, "a")
	with.client.Consume(ctxb, "a", nil)
	for _, r := range with.server.all() {
		assertEqual(t, "Bearer tb_x.secret", r.Header.Get("Authorization"), "authorization")
	}
	without := newFixture(t)
	defer without.close()
	without.client.GetTopic(ctxb, "a")
	assertEqual(t, "", without.server.all()[0].Header.Get("Authorization"), "no authorization")
}

// UC: UC-CNT-028
func TestUnknownFieldsInResponsesAreIgnoredAndTheContractVersionIsDeclared(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, `{"id":"t1","name":"q","type":"queue","createdAt":"x","somethingNew":{"a":[1,2]},"fifo":false}`))
	topic, err := f.client.GetTopic(ctxb, "q")
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "q", topic.Name, "name")
	assertEqual(t, false, *topic.Fifo, "fifo")
	if topic.Volatile != nil {
		t.Error("volatile was not in the response")
	}
	assertEqual(t, "1", ContractVersion, "contract version")
}

// UC: UC-CNT-005
func TestConsumeSendsOnlyWhatWasGiven(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.client.Consume(ctxb, "q", nil)
	f.client.Consume(ctxb, "q", &ConsumeOptions{MaxMessages: Int(5), VisibilityTimeout: Int(60), WaitTime: Int(20)})
	got := f.server.all()
	assertEqual(t, "", got[0].Raw, "no defaults of its own")
	assertEqual(t, "5", got[1].Query["maxMessages"], "max")
	assertEqual(t, "60", got[1].Query["visibilityTimeout"], "visibility")
	assertEqual(t, "20", got[1].Query["waitTime"], "wait")
}

// UC: UC-CNT-005
func TestAutoCreateParametersAreLeftOutWithoutAutoCreate(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.client.Consume(ctxb, "q", &ConsumeOptions{Volatile: Bool(true), Fifo: Bool(true), Tags: map[string]string{"a": "b"}})
	f.client.Consume(ctxb, "q", &ConsumeOptions{AutoCreate: true, Volatile: Bool(true), IdleTimeout: Int(30), Fifo: Bool(false), Tags: map[string]string{"a": "b"}})
	got := f.server.all()
	assertEqual(t, "", got[0].Raw, "ignored without autoCreate")
	assertEqual(t, "true", got[1].Query["autoCreate"], "autoCreate")
	assertEqual(t, "true", got[1].Query["volatile"], "volatile")
	assertEqual(t, "30", got[1].Query["idleTimeout"], "idleTimeout")
	assertEqual(t, "false", got[1].Query["fifo"], "fifo")
	assertEqual(t, "b", got[1].Query["tag.a"], "tag")
}

// UC: UC-CNT-008
func TestTheConsumeReadTimeoutIsWaitTimePlusTenSecondsAndNotTheRequestTimeout(t *testing.T) {
	assertEqual(t, 30*time.Second, consumeCall("q", ConsumeOptions{WaitTime: Int(20)}).timeout, "wait 20")
	assertEqual(t, 10*time.Second, consumeCall("q", ConsumeOptions{}).timeout, "no wait")
	f := newFixture(t, func(c *Config) { c.RequestTimeout = 100 * time.Millisecond })
	defer f.close()
	f.server.replyWith(func(recorded) answer {
		time.Sleep(400 * time.Millisecond)
		return reply(200, map[string]interface{}{"messages": []interface{}{}})
	})
	// The ordinary timeout would cut this at 100 ms; the long polling has WaitTime + 10 s.
	if _, err := f.client.Consume(ctxb, "q", &ConsumeOptions{WaitTime: Int(1)}); err != nil {
		t.Fatal(err)
	}
}

// UC: UC-CNT-006
func TestAckAndVisibilityPutTheReceiptTokenInTheQuery(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(answer{status: 204}, reply(200, map[string]string{"id": "m1", "invisibleUntil": "later"}))
	m := &Message{ID: "m1", ReceiptToken: "a/b c"}
	if err := f.client.Ack(ctxb, "q", m); err != nil {
		t.Fatal(err)
	}
	v, err := f.client.ChangeVisibility(ctxb, "q", m, 45)
	if err != nil {
		t.Fatal(err)
	}
	got := f.server.all()
	assertEqual(t, "a/b c", got[0].Query["receiptToken"], "ack token")
	assertEqual(t, "a/b c", got[1].Query["receiptToken"], "visibility token")
	assertEqual(t, "later", v.InvisibleUntil, "invisible until")
}

// UC: UC-CNT-007
func TestListAllTopicsFollowsNextCursorUntilTheLastPage(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(200, map[string]interface{}{"items": []interface{}{topicJSON("a"), topicJSON("b")}, "nextCursor": "c1"}),
		reply(200, map[string]interface{}{"items": []interface{}{topicJSON("c")}, "nextCursor": nil}),
	)
	all, err := f.client.ListAllTopics(ctxb, "")
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 3, len(all), "topics")
	got := f.server.all()
	assertEqual(t, "", got[0].Raw, "first page has no cursor")
	assertEqual(t, "c1", got[1].Query["cursor"], "second page cursor")
}

// UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
func TestMessagesComeBackAsBytesForJSONTextAndBase64(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, `{"messages":[
	  {"id":"m1","body":{"a": 1.0,"b":"é"},"contentType":"application/json","receiptToken":"r1","deliveryCount":2,"groupId":"g","deduplicationId":"d","extra":1},
	  {"id":"m2","body":"café","contentType":"text/plain; charset=utf-8","receiptToken":"r2"},
	  {"id":"m3","body":"AAEC/w==","bodyEncoding":"base64","contentType":"application/octet-stream","receiptToken":"r3"},
	  {"id":"m4","body":[1,2],"contentType":"application/vnd.x+json","receiptToken":"r4","deadLetteredAt":"t","deadLetterReason":"max"}
	]}`))
	messages, err := f.client.Consume(ctxb, "q", nil)
	if err != nil {
		t.Fatal(err)
	}
	// JSON is the text of the field as it came, not re-formatted.
	assertEqual(t, `{"a": 1.0,"b":"é"}`, messages[0].BodyText(), "json body")
	assertEqual(t, 2, messages[0].DeliveryCount, "delivery count")
	assertEqual(t, "g", messages[0].GroupID, "group")
	assertEqual(t, "café", messages[1].BodyText(), "text body")
	if !bytes.Equal(messages[2].Body, []byte{0, 1, 2, 255}) {
		t.Errorf("base64 body %v", messages[2].Body)
	}
	assertEqual(t, "[1,2]", messages[3].BodyText(), "+json body")
	assertEqual(t, "max", messages[3].DeadLetterReason, "dead letter")
	var number struct{ A float64 }
	if err := messages[0].JSON(&number); err != nil || number.A != 1 {
		t.Errorf("JSON(): %v %v", err, number)
	}
}

// UC: UC-CNT-026
func TestAnInvalidAnswerIsAnInvalidResponseErrorAndNotACrash(t *testing.T) {
	for _, body := range []string{"not json", "[]", `{"messages":[{"id":5}]}`, `{"messages":[{"id":"m","body":"!!","bodyEncoding":"base64"}]}`, `{"messages":{}}`} {
		f := newFixture(t)
		f.server.setFallback(reply(200, body))
		_, err := f.client.Consume(ctxb, "q", nil)
		assertError(t, "client.invalid-response", err)
		f.close()
	}
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, "oops"))
	_, err := f.client.GetTopic(ctxb, "q")
	e := assertError(t, "client.invalid-response", err)
	assertEqual(t, true, e.IsClientSide(), "client side")
}

// UC: UC-CNT-009, UC-CNT-017
func TestATimeoutIsNotRepeatedAndIsAClientSideError(t *testing.T) {
	f := newFixture(t, func(c *Config) { c.RequestTimeout = 150 * time.Millisecond })
	defer f.close()
	f.server.setFallback(hung)
	_, err := f.client.GetTopic(ctxb, "q")
	e := assertError(t, "client.timeout", err)
	assertEqual(t, 0, e.Status, "status")
	assertEqual(t, 1, f.server.count(), "attempts")
}

// UC: UC-CNT-010
func TestRetries429And503WaitingForRetryAfterThenBackoff(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(429, `{"code":"rate.limited","message":"slow"}`).with("Retry-After", "7"),
		reply(503, `{"code":"cluster.unavailable","message":"x"}`).with("Retry-After", "2"),
		reply(200, topicJSON("q")),
	)
	if _, err := f.client.GetTopic(ctxb, "q"); err != nil {
		t.Fatal(err)
	}
	assertEqual(t, 3, f.server.count(), "requests")
	waits := f.sleep.recorded()
	assertEqual(t, 7*time.Second, waits[0], "first wait")
	assertEqual(t, 2*time.Second, waits[1], "second wait")
}

// UC: UC-CNT-010
func TestBackoffWithoutRetryAfterIsOneTwoFourSecondsUpToMaxAttempts(t *testing.T) {
	f := newFixture(t, func(c *Config) { c.MaxAttempts = 4 })
	defer f.close()
	f.server.setFallback(reply(503, `{"code":"cluster.unavailable","message":"down","requestId":"r9"}`))
	_, err := f.client.GetTopic(ctxb, "q")
	e := assertError(t, "cluster.unavailable", err)
	assertEqual(t, 503, e.Status, "status")
	assertEqual(t, 4, f.server.count(), "attempts")
	assertEqual(t, []time.Duration{time.Second, 2 * time.Second, 4 * time.Second}[2], f.sleep.recorded()[2], "third wait")
	assertEqual(t, 3, len(f.sleep.recorded()), "waits")
	assertEqual(t, time.Second, f.sleep.recorded()[0], "first wait")
}

// UC: UC-CNT-011
func TestAfterA503A409OnCreateAndA404OnDeleteCountAsSuccess(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(503, `{"code":"cluster.unavailable","message":"x"}`),
		reply(409, `{"code":"topic.already-exists","message":"x"}`),
		reply(200, topicJSON("q")),
		reply(503, `{"code":"cluster.unavailable","message":"x"}`),
		reply(404, `{"code":"topic.not-found","message":"x"}`),
	)
	topic, err := f.client.CreateTopic(ctxb, &TopicRequest{Name: "q", Type: "queue"})
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "q", topic.Name, "topic")
	got := f.server.all()
	assertEqual(t, "GET", got[2].Method, "the topic is read after the recovered 409")
	if err := f.client.DeleteTopic(ctxb, "q"); err != nil {
		t.Fatal(err)
	}
}

// UC: UC-CNT-011, UC-CNT-014
func TestWithoutAPrevious503A409OrA404IsAPlainError(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(
		reply(409, `{"code":"topic.already-exists","message":"x"}`),
		reply(404, `{"code":"topic.not-found","message":"x"}`),
	)
	_, err := f.client.CreateTopic(ctxb, &TopicRequest{Name: "q", Type: "queue"})
	assertError(t, "topic.already-exists", err)
	assertError(t, "topic.not-found", f.client.DeleteTopic(ctxb, "q"))
	assertEqual(t, 2, f.server.count(), "no retry")
}

// UC: UC-CNT-012
func TestGatewayErrorsAreOnlyRepeatedWhenPublishingToAFIFO(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(502, `{"code":"cluster.bad-gateway","message":"x"}`))
	_, err := f.client.Publish(ctxb, "q", []byte("{}"), nil)
	assertError(t, "cluster.bad-gateway", err)
	assertEqual(t, 1, f.server.count(), "a plain queue is not repeated")
	_, err = f.client.Publish(ctxb, "fifo", []byte("{}"), &PublishOptions{Fifo: true})
	assertError(t, "cluster.bad-gateway", err)
	assertEqual(t, 4, f.server.count(), "a FIFO is repeated up to the attempts")
	f.server.setFallback(reply(504, `{"code":"cluster.timeout","message":"x"}`))
	_, err = f.client.Consume(ctxb, "q", nil)
	assertError(t, "cluster.timeout", err)
	assertEqual(t, 5, f.server.count(), "consume is not repeated")
}

// UC: UC-CNT-013
func TestANetworkFailureAfterSendingIsNotRepeated(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(destroyed)
	_, err := f.client.Publish(ctxb, "q", []byte("{}"), nil)
	assertError(t, "client.network", err)
	assertEqual(t, 1, f.server.count(), "attempts")
}

func closedAddress(t *testing.T) string {
	l, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	addr := l.Addr().String()
	l.Close()
	return addr
}

// UC: UC-CNT-013
func TestARefusedConnectionIsRepeatedAndThenANetworkError(t *testing.T) {
	client, err := NewClient(Config{BaseURL: "http://" + closedAddress(t)})
	if err != nil {
		t.Fatal(err)
	}
	sleep := &fakeSleep{}
	client.t.sleep = sleep.sleep
	_, err = client.GetTopic(ctxb, "q")
	e := assertError(t, "client.network", err)
	assertEqual(t, 0, e.Status, "status")
	assertEqual(t, 2, len(sleep.recorded()), "waits between the 3 attempts")
}

// UC: UC-CNT-013
func TestARefusedConnectionSucceedsWhenTheNodeComesBack(t *testing.T) {
	addr := closedAddress(t)
	client, err := NewClient(Config{BaseURL: "http://" + addr})
	if err != nil {
		t.Fatal(err)
	}
	var server *httptest.Server
	client.t.sleep = func(ctx context.Context, d time.Duration) error {
		if server == nil {
			l, err := net.Listen("tcp", addr)
			if err != nil {
				t.Fatal(err)
			}
			server = httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Write([]byte(`{"id":"t1","name":"q","type":"queue","createdAt":"x"}`))
			}))
			server.Listener = l
			server.Start()
		}
		return nil
	}
	topic, err := client.GetTopic(ctxb, "q")
	if server != nil {
		defer server.Close()
	}
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "q", topic.Name, "topic")
}

// UC: UC-CNT-014
func TestA4xxOtherThan429IsNeverRepeated(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	for _, status := range []int{400, 401, 403, 404, 409, 413, 422} {
		before := f.server.count()
		f.server.reply(reply(status, `{"code":"x.y","message":"m"}`))
		_, err := f.client.GetTopic(ctxb, "q")
		e := assertError(t, "x.y", err)
		assertEqual(t, status, e.Status, "status")
		assertEqual(t, before+1, f.server.count(), "one attempt")
	}
}

// UC: UC-CNT-017, UC-CNT-018
func TestTheErrorCarriesTheErrorResponseFieldsAndTheHeaders(t *testing.T) {
	f := newFixture(t, func(c *Config) { c.MaxAttempts = 1 })
	defer f.close()
	f.server.reply(
		reply(429, `{"code":"rate.limited","message":"slow down","requestId":"body-id","details":{"limit":5}}`).with("Retry-After", "9"),
		reply(500, `{"code":"internal.error","message":"boom"}`).with("X-Request-Id", "header-id"),
	)
	_, err := f.client.GetTopic(ctxb, "q")
	e := assertError(t, "rate.limited", err)
	assertEqual(t, 429, e.Status, "status")
	assertEqual(t, "slow down", e.Message, "message")
	assertEqual(t, "body-id", e.RequestID, "request id")
	assertEqual(t, 9, e.RetryAfter, "retry after")
	assertEqual(t, true, e.HasRetryAfter(), "has retry after")
	assertEqual(t, `{"limit":5}`, e.Details, "details")
	assertEqual(t, false, e.IsClientSide(), "server side")
	if !contains(e.Error(), "body-id") {
		t.Errorf("Error() = %q", e.Error())
	}
	_, err = f.client.GetTopic(ctxb, "q")
	e = assertError(t, "internal.error", err)
	assertEqual(t, "header-id", e.RequestID, "request id from the header")
	assertEqual(t, false, e.HasRetryAfter(), "no retry after")
}

// UC: UC-CNT-017
func TestANonJSONErrorStillReportsAStatusCode(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.reply(reply(400, "<html>bad</html>"), reply(401, ""))
	_, err := f.client.GetTopic(ctxb, "q")
	e := assertError(t, "http.400", err)
	assertEqual(t, "<html>bad</html>", e.Message, "message")
	_, err = f.client.GetTopic(ctxb, "q")
	e = assertError(t, "http.401", err)
	if !contains(e.Message, "401") {
		t.Errorf("message %q", e.Message)
	}
}

// UC: UC-CNT-020
func TestManyGoroutinesCanUseOneClientAtTheSameTime(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(reply(200, topicJSON("q")))
	var wg sync.WaitGroup
	errs := make(chan error, 40)
	for i := 0; i < 40; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := f.client.GetTopic(ctxb, "q")
			errs <- err
		}()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	assertEqual(t, 40, f.server.count(), "requests")
}

// UC: UC-CNT-020
func TestCancellingTheContextCancelsTheCallWithoutAClientError(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(hung)
	ctx, cancel := context.WithCancel(ctxb)
	time.AfterFunc(100*time.Millisecond, cancel)
	started := time.Now()
	_, err := f.client.Consume(ctx, "q", &ConsumeOptions{WaitTime: Int(20)})
	if err != context.Canceled {
		t.Fatalf("want context.Canceled, got %v", err)
	}
	if time.Since(started) > 5*time.Second {
		t.Error("the long polling waited for its timeout")
	}
	if _, isTaba := err.(*Error); isTaba {
		t.Error("a cancellation is not a *Error")
	}
	// Already cancelled: no request goes out.
	before := f.server.count()
	_, err = f.client.GetTopic(ctx, "q")
	if err != context.Canceled || f.server.count() != before {
		t.Errorf("err %v, requests %d -> %d", err, before, f.server.count())
	}
}

// UC: UC-CNT-031
func TestSubscribeAcksOnlyWhatTheHandlerAcceptedAndStopsOnCancel(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.batches([]interface{}{messageJSON("m1", `"a"`, nil), messageJSON("m2", `"b"`, nil), messageJSON("m3", `"c"`, nil)})
	ctx, cancel := context.WithCancel(ctxb)
	defer cancel()
	var seen []string
	var failed []string
	err := f.client.Subscribe(ctx, "q", &ConsumeOptions{VisibilityTimeout: Int(60)}, func(m *Message) error {
		seen = append(seen, m.ID)
		if m.ID == "m3" {
			cancel()
		}
		if m.ID == "m2" {
			return errors.New("the handler failed")
		}
		return nil
	}, func(m *Message, err error) { failed = append(failed, m.ID+":"+err.Error()) })
	if err != nil {
		t.Fatal(err)
	}
	assertEqual(t, "m1,m2,m3", strings.Join(seen, ","), "seen")
	assertEqual(t, "m2:the handler failed", strings.Join(failed, ","), "onError")
	var deleted []string
	for _, r := range f.server.all() {
		if r.Method == "DELETE" {
			deleted = append(deleted, r.Path)
		}
	}
	assertEqual(t, "/topics/q/messages/m1,/topics/q/messages/m3", strings.Join(deleted, ","), "only the accepted ones are acked")
	first := f.server.all()[0]
	assertEqual(t, "20", first.Query["waitTime"], "default long polling")
	assertEqual(t, "1", first.Query["maxMessages"], "one at a time")
	assertEqual(t, "60", first.Query["visibilityTimeout"], "given visibility")
}

// UC: UC-CNT-031
func TestSubscribeReturnsWithoutAnErrorWhenCancelledDuringTheLongPolling(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.server.setFallback(hung)
	ctx, cancel := context.WithCancel(ctxb)
	time.AfterFunc(100*time.Millisecond, cancel)
	started := time.Now()
	if err := f.client.Subscribe(ctx, "q", nil, func(*Message) error { return nil }, nil); err != nil {
		t.Fatal(err)
	}
	if time.Since(started) > 5*time.Second {
		t.Error("Subscribe waited for the long polling")
	}
	// A server error while consuming ends the loop with the error.
	f.server.setFallback(reply(403, `{"code":"auth.forbidden","message":"no"}`))
	err := f.client.Subscribe(ctxb, "q", nil, func(*Message) error { return nil }, nil)
	assertError(t, "auth.forbidden", err)
}

// UC: UC-CNT-033
func TestPublishJSONSerializesTheObjectAndSendsItAsJSON(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	type order struct {
		Order int    `json:"order"`
		Note  string `json:"note"`
	}
	f.client.PublishJSON(ctxb, "q", order{9001, "<ok> & é"})
	got := f.server.all()[0]
	assertEqual(t, "application/json", got.Header.Get("Content-Type"), "content type")
	var back order
	if err := json.Unmarshal(got.Body, &back); err != nil || back.Order != 9001 || back.Note != "<ok> & é" {
		t.Errorf("body %q (%v)", got.text(), err)
	}
	f.client.PublishRawJSON(ctxb, "q", `{"a": 1}`, &PublishOptions{GroupID: "g"})
	assertEqual(t, `{"a": 1}`, f.server.all()[1].text(), "raw json untouched")
	assertEqual(t, "g", f.server.all()[1].Query["groupId"], "options")
	_, err := f.client.PublishJSON(ctxb, "q", nil)
	assertError(t, "client.invalid-request", err)
	_, err = f.client.PublishJSON(ctxb, "q", make(chan int))
	assertError(t, "client.invalid-request", err)
	assertEqual(t, 2, f.server.count(), "nothing was sent for the invalid ones")
}

// UC: UC-CNT-034
func TestConsumeWithAHandlerDeletesByDefaultAndKeepsTheMessageWhenAskedOrWhenItFails(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	f.client.t.sleep = blockingSleep
	deletes := func() int {
		n := 0
		for _, r := range f.server.all() {
			if r.Method == "DELETE" {
				n++
			}
		}
		return n
	}
	f.server.batches([]interface{}{messageJSON("m1", `{"n":1}`, nil)}, []interface{}{messageJSON("m2", `"x"`, nil)}, []interface{}{messageJSON("m3", `"y"`, nil)})
	handled, err := f.client.ConsumeWith(ctxb, "q", nil, func(mc *MessageContext) error {
		var v struct{ N int }
		if err := mc.Decode(&v); err != nil || v.N != 1 {
			t.Errorf("decode: %v %v", err, v)
		}
		return nil
	})
	if err != nil || handled != 1 || deletes() != 1 {
		t.Fatalf("handled %d, err %v, deletes %d", handled, err, deletes())
	}
	first := f.server.all()[0]
	assertEqual(t, "1", first.Query["maxMessages"], "one at a time")
	assertEqual(t, "120", first.Query["visibilityTimeout"], "120 s of invisibility")
	assertEqual(t, "20", first.Query["waitTime"], "20 s of long polling")

	f.client.ConsumeWith(ctxb, "q", nil, func(mc *MessageContext) error { mc.Delete = false; return nil })
	assertEqual(t, 1, deletes(), "Delete=false keeps the message")

	boom := errors.New("boom")
	_, err = f.client.ConsumeWith(ctxb, "q", nil, func(mc *MessageContext) error { return boom })
	assertEqual(t, boom, err, "the handler's error is returned")
	assertEqual(t, 1, deletes(), "a failing handler keeps the message")
}

// UC: UC-CNT-034
func TestConsumeWithAHandlerRenewsTheVisibilityWhileItRunsAndLetsItChangeIt(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	var mu sync.Mutex
	calls := 0
	var interval time.Duration
	f.client.t.sleep = func(ctx context.Context, d time.Duration) error {
		mu.Lock()
		calls++
		n := calls
		if n == 1 {
			interval = d
		}
		mu.Unlock()
		if n == 1 {
			return nil
		}
		return blockingSleep(ctx, d)
	}
	f.server.batches([]interface{}{messageJSON("m1", `"a"`, nil)})
	puts := func() []recorded {
		var out []recorded
		for _, r := range f.server.all() {
			if r.Method == "PUT" {
				out = append(out, r)
			}
		}
		return out
	}
	_, err := f.client.ConsumeWith(ctxb, "q", &ConsumeOptions{VisibilityTimeout: Int(30)}, func(mc *MessageContext) error {
		waitFor(t, func() bool { return len(puts()) >= 1 })
		assertEqual(t, 30, mc.LeaseSeconds(), "lease")
		if err := mc.ChangeVisibility(60); err != nil {
			return err
		}
		assertEqual(t, 60, mc.LeaseSeconds(), "lease after the change")
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	assertEqual(t, 10*time.Second, interval, "a third of the invisibility")
	mu.Unlock()
	p := puts()
	assertEqual(t, `{"visibilityTimeout":30}`, p[0].text(), "renewal")
	assertEqual(t, `{"visibilityTimeout":60}`, p[1].text(), "explicit change")
	assertEqual(t, "rt-m1", p[0].Query["receiptToken"], "receipt")
}

// UC: UC-CNT-034
func TestARefusedRenewalMarksTheLeaseAsLostAndTheDeleteIsTolerated(t *testing.T) {
	f := newFixture(t)
	defer f.close()
	var mu sync.Mutex
	calls := 0
	f.client.t.sleep = func(ctx context.Context, d time.Duration) error {
		mu.Lock()
		calls++
		n := calls
		mu.Unlock()
		if n == 1 {
			return nil
		}
		return blockingSleep(ctx, d)
	}
	f.server.setFallbackFunc(func(r recorded) answer {
		switch r.Method {
		case "GET":
			return reply(200, map[string]interface{}{"messages": []interface{}{messageJSON("m1", `"a"`, nil)}})
		case "PUT":
			return reply(409, `{"code":"message.receipt-invalid","message":"gone"}`)
		}
		return reply(404, `{"code":"message.not-found","message":"gone"}`)
	})
	var lost bool
	handled, err := f.client.ConsumeWith(ctxb, "q", nil, func(mc *MessageContext) error {
		waitFor(t, mc.LeaseLost)
		lost = mc.LeaseLost()
		return nil
	})
	if err != nil {
		t.Fatalf("the 404 of the delete must be tolerated: %v", err)
	}
	assertEqual(t, 1, handled, "handled")
	assertEqual(t, true, lost, "lease lost")
}
