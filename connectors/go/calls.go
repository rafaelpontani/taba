package taba

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"sort"
	"strconv"
	"time"
)

const (
	// longPollSlack: consume waits WaitTime plus this, not the RequestTimeout.
	longPollSlack = 10 * time.Second

	handlerMaxMessages       = 1
	handlerVisibilityTimeout = 120
	handlerWaitTime          = 20
)

// jsonObject builds a JSON object keeping the order of the keys it is given (no HTML escaping).
type jsonObject struct {
	buf   bytes.Buffer
	count int
}

func marshal(v interface{}) []byte {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return []byte("null")
	}
	return bytes.TrimRight(buf.Bytes(), "\n")
}

func (o *jsonObject) put(key string, value interface{}) {
	if o.count == 0 {
		o.buf.WriteByte('{')
	} else {
		o.buf.WriteByte(',')
	}
	o.count++
	o.buf.Write(marshal(key))
	o.buf.WriteByte(':')
	o.buf.Write(marshal(value))
}

// putRaw adds a value already written as JSON (a json.RawMessage by value is encoded wrongly before Go 1.8).
func (o *jsonObject) putRaw(key string, value []byte) {
	if o.count == 0 {
		o.buf.WriteByte('{')
	} else {
		o.buf.WriteByte(',')
	}
	o.count++
	o.buf.Write(marshal(key))
	o.buf.WriteByte(':')
	o.buf.Write(value)
}

func (o *jsonObject) bytes() []byte {
	if o.count == 0 {
		return []byte("{}")
	}
	return append(o.buf.Bytes(), '}')
}

func sortedKeys(m map[string]string) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func createTopicCall(r *TopicRequest) (*apiCall, error) {
	if r == nil {
		return nil, clientError("client.invalid-request", "request is required")
	}
	var o jsonObject
	o.put("name", r.Name)
	topicFields(&o, r)
	call := (&apiCall{method: "POST", path: "/topics"}).withJSON(o.bytes())
	call.recover = append(call.recover, recoverRule{409, "topic.already-exists"})
	return call, nil
}

// putTopicCall is PUT /topics/{topic}: the fields of POST /topics without the name (it goes in the path).
func putTopicCall(r *TopicRequest, recreate Recreate) (*apiCall, error) {
	if r == nil {
		return nil, clientError("client.invalid-request", "request is required")
	}
	if recreate != RecreateNever && recreate != RecreateIfEmpty && recreate != RecreateAlways {
		return nil, clientError("client.invalid-request", "recreate must be RecreateIfEmpty or RecreateAlways")
	}
	var o jsonObject
	topicFields(&o, r)
	call := (&apiCall{method: "PUT", path: "/topics/" + segment(r.Name)}).withJSON(o.bytes())
	if recreate != RecreateNever {
		call.withQuery("recreate", string(recreate))
	}
	return call, nil
}

func topicFields(o *jsonObject, r *TopicRequest) {
	o.put("type", r.Type)
	if len(r.Tags) > 0 {
		o.put("tags", r.Tags)
	}
	if len(r.Prefixes) > 0 {
		o.put("prefixes", r.Prefixes)
	}
	if r.Volatile != nil {
		o.put("volatile", *r.Volatile)
	}
	if r.IdleTimeout != nil {
		o.put("idleTimeout", *r.IdleTimeout)
	}
	if r.Fifo != nil {
		o.put("fifo", *r.Fifo)
	}
	if r.MaxMessageBytes != nil {
		o.put("maxMessageBytes", *r.MaxMessageBytes)
	}
	if r.TTL != nil {
		o.put("ttl", *r.TTL)
	}
	if r.MaxDeliveries != nil {
		o.put("maxDeliveries", *r.MaxDeliveries)
	}
}

func getTopicCall(name string) *apiCall {
	return &apiCall{method: "GET", path: "/topics/" + segment(name)}
}

func listTopicsCall(limit *int, cursor, name string) *apiCall {
	call := &apiCall{method: "GET", path: "/topics"}
	if limit != nil {
		call.withQuery("limit", strconv.Itoa(*limit))
	}
	if cursor != "" {
		call.withQuery("cursor", cursor)
	}
	if name != "" {
		call.withQuery("name", name)
	}
	return call
}

func nullable(o *jsonObject, key string, n *NullableInt) {
	if n == nil {
		return
	}
	if n.null {
		o.put(key, nil)
	} else {
		o.put(key, n.value)
	}
}

func updateTopicCall(name string, u *TopicUpdate) (*apiCall, error) {
	if u == nil {
		return nil, clientError("client.invalid-request", "update is required")
	}
	var o jsonObject
	if u.Tags != nil {
		o.put("tags", u.Tags)
	}
	if u.Prefixes != nil {
		o.put("prefixes", u.Prefixes)
	}
	if u.IdleTimeout != nil {
		o.put("idleTimeout", *u.IdleTimeout)
	}
	nullable(&o, "maxMessageBytes", u.MaxMessageBytes)
	nullable(&o, "ttl", u.TTL)
	nullable(&o, "maxDeliveries", u.MaxDeliveries)
	return (&apiCall{method: "PATCH", path: "/topics/" + segment(name)}).withJSON(o.bytes()), nil
}

func deleteTopicCall(name string) *apiCall {
	call := &apiCall{method: "DELETE", path: "/topics/" + segment(name)}
	call.recover = append(call.recover, recoverRule{404, "topic.not-found"})
	return call
}

func publishCall(topic string, body []byte, o PublishOptions) *apiCall {
	call := &apiCall{method: "POST", path: "/topics/" + segment(topic) + "/messages"}
	call.body, call.hasBody = body, true
	if call.body == nil {
		call.body = []byte{}
	}
	call.contentType = o.ContentType
	if call.contentType == "" {
		call.contentType = "application/json"
	}
	call.requestID = o.RequestID
	call.traceparent = o.Traceparent
	call.retryGateway = o.Fifo
	if o.GroupID != "" {
		call.withQuery("groupId", o.GroupID)
	}
	if o.DeduplicationID != "" {
		call.withQuery("deduplicationId", o.DeduplicationID)
	}
	return call
}

func publishBatchCall(topic string, messages []BatchMessage) *apiCall {
	var items bytes.Buffer
	items.WriteByte('[')
	for i, m := range messages {
		if i > 0 {
			items.WriteByte(',')
		}
		var item jsonObject
		item.put("body", base64.StdEncoding.EncodeToString(m.Body))
		item.put("bodyEncoding", "base64")
		if m.ContentType != "" {
			item.put("contentType", m.ContentType)
		}
		if m.GroupID != "" {
			item.put("groupId", m.GroupID)
		}
		if m.DeduplicationID != "" {
			item.put("deduplicationId", m.DeduplicationID)
		}
		items.Write(item.bytes())
	}
	items.WriteByte(']')
	var o jsonObject
	o.putRaw("messages", items.Bytes())
	return (&apiCall{method: "POST", path: "/topics/" + segment(topic) + "/messages/batch"}).withJSON(o.bytes())
}

// receiptsCall is the ack (route "ack") or the change of invisibility ("visibility") of many messages in one request.
func receiptsCall(topic, route string, messages []*Message, seconds *int) *apiCall {
	var items bytes.Buffer
	items.WriteByte('[')
	for i, m := range messages {
		if i > 0 {
			items.WriteByte(',')
		}
		var item jsonObject
		item.put("id", m.ID)
		item.put("receiptToken", m.ReceiptToken)
		if seconds != nil {
			item.put("visibilityTimeout", *seconds)
		}
		items.Write(item.bytes())
	}
	items.WriteByte(']')
	var o jsonObject
	o.putRaw("messages", items.Bytes())
	return (&apiCall{method: "POST", path: "/topics/" + segment(topic) + "/messages/" + route}).withJSON(o.bytes())
}

func consumeCall(topic string, o ConsumeOptions) *apiCall {
	call := &apiCall{method: "GET", path: "/topics/" + segment(topic) + "/messages"}
	if o.MaxMessages != nil {
		call.withQuery("maxMessages", strconv.Itoa(*o.MaxMessages))
	}
	if o.VisibilityTimeout != nil {
		call.withQuery("visibilityTimeout", strconv.Itoa(*o.VisibilityTimeout))
	}
	if o.WaitTime != nil {
		call.withQuery("waitTime", strconv.Itoa(*o.WaitTime))
	}
	if o.AutoCreate {
		call.withQuery("autoCreate", "true")
		if o.Volatile != nil {
			call.withQuery("volatile", strconv.FormatBool(*o.Volatile))
		}
		if o.IdleTimeout != nil {
			call.withQuery("idleTimeout", strconv.Itoa(*o.IdleTimeout))
		}
		if o.Fifo != nil {
			call.withQuery("fifo", strconv.FormatBool(*o.Fifo))
		}
		for _, k := range sortedKeys(o.Tags) {
			call.withQuery("tag."+k, o.Tags[k])
		}
	}
	call.requestID = o.RequestID
	call.traceparent = o.Traceparent
	wait := 0
	if o.WaitTime != nil {
		wait = *o.WaitTime
	}
	call.timeout = time.Duration(wait)*time.Second + longPollSlack
	return call
}

func ackCall(topic, id, receipt string) *apiCall {
	call := &apiCall{method: "DELETE", path: "/topics/" + segment(topic) + "/messages/" + segment(id)}
	return call.withQuery("receiptToken", receipt)
}

func changeVisibilityCall(topic, id, receipt string, seconds int) *apiCall {
	call := &apiCall{method: "PUT", path: "/topics/" + segment(topic) + "/messages/" + segment(id) + "/visibility"}
	call.withQuery("receiptToken", receipt)
	var o jsonObject
	o.put("visibilityTimeout", seconds)
	return call.withJSON(o.bytes())
}

// handlerDefaults: one message at a time, 20 s of long polling and 120 s of invisibility, which the renewal keeps.
func handlerDefaults(options *ConsumeOptions) ConsumeOptions {
	o := options.copy()
	if o.MaxMessages == nil {
		o.MaxMessages = Int(handlerMaxMessages)
	}
	if o.VisibilityTimeout == nil {
		o.VisibilityTimeout = Int(handlerVisibilityTimeout)
	}
	if o.WaitTime == nil {
		o.WaitTime = Int(handlerWaitTime)
	}
	return o
}

func subscribeDefaults(options *ConsumeOptions) ConsumeOptions {
	o := options.copy()
	if o.WaitTime == nil {
		o.WaitTime = Int(handlerWaitTime)
	}
	if o.MaxMessages == nil {
		o.MaxMessages = Int(handlerMaxMessages)
	}
	return o
}

// renewalInterval is a third of the invisibility (and never all of it), so a failed renewal can be tried again in time.
func renewalInterval(leaseSeconds int) time.Duration {
	if leaseSeconds < 1 {
		leaseSeconds = 1
	}
	return time.Duration(leaseSeconds) * time.Second / 3
}
