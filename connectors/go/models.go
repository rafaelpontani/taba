package taba

import (
	"encoding/base64"
	"encoding/json"
	"strings"
)

// Topic is a topic of the API.
type Topic struct {
	ID   string
	Name string
	// Type is "queue" or "distributor".
	Type string
	// CreatedAt is the creation date, as RFC 3339 text.
	CreatedAt string
	Fifo      *bool
	Volatile  *bool
	// Tags are the telemetry tags (never nil; empty when the topic has none).
	Tags map[string]string
	// Prefixes (distributor only) are the destination prefixes; nil on a queue.
	Prefixes []string
	// IdleTimeout (volatile queue only) is the seconds without a consumer until the queue is deleted.
	IdleTimeout *int
	// MaxMessageBytes, TTL and MaxDeliveries (queue only) are nil when the topic follows the node default.
	MaxMessageBytes *int
	TTL             *int
	MaxDeliveries   *int
	// DeadLetter is true only on a dead-letter queue, whose Source is the queue it belongs to.
	DeadLetter *bool
	Source     string
	// Raw is the topic JSON, as the server returned it (with the effective values, in "effective").
	Raw string
}

// Page is one page of ListTopics.
type Page struct {
	Items []Topic
	// NextCursor is the cursor of the next page, or "" on the last one.
	NextCursor string
	Raw        string
}

// TopicRequest is POST /topics. Only what was filled in is sent.
type TopicRequest struct {
	Name string
	// Type is "queue" or "distributor".
	Type string
	Tags map[string]string
	// Prefixes (distributor only) are the name prefixes of the queues each publication is copied to.
	Prefixes        []string
	Volatile        *bool
	IdleTimeout     *int
	Fifo            *bool
	MaxMessageBytes *int
	TTL             *int
	MaxDeliveries   *int
}

// Recreate is what CreateOrUpdateTopic does when the topic exists with another type, fifo or volatile (fields that
// do not change in place).
type Recreate string

const (
	// RecreateNever refuses with 409 topic.immutable-mismatch; nothing changes.
	RecreateNever Recreate = ""
	// RecreateIfEmpty removes and creates it again only if the queue and its dead-letter hold no message on any
	// node (409 topic.not-empty otherwise).
	RecreateIfEmpty Recreate = "if-empty"
	// RecreateAlways removes it with its messages and dead-letter and creates it again (new ID).
	RecreateAlways Recreate = "always"
)

// TopicOutcome is what PUT /topics/{topic} did.
type TopicOutcome string

const (
	// TopicCreated: it did not exist (201).
	TopicCreated TopicOutcome = "created"
	// TopicUpdated: it existed; the mutable fields are now the declared ones, or already were (200).
	TopicUpdated TopicOutcome = "updated"
	// TopicRecreated: it was removed and created again with a new ID (201 with Taba-Topic-Recreated: true).
	TopicRecreated TopicOutcome = "recreated"
)

// TopicPut is the answer of CreateOrUpdateTopic: the topic as it is now and what was done.
type TopicPut struct {
	Topic   Topic
	Outcome TopicOutcome
}

// TopicUpdate is PATCH /topics/{topic}: only what was filled in is sent.
type TopicUpdate struct {
	// Tags replaces the whole map (an empty, non-nil map clears the tags).
	Tags        map[string]string
	Prefixes    []string
	IdleTimeout *int
	// MaxMessageBytes, TTL and MaxDeliveries: nil leaves the field out, ClearInt() sends null (back to the
	// server default) and SetInt(n) sends n.
	MaxMessageBytes *NullableInt
	TTL             *NullableInt
	MaxDeliveries   *NullableInt
}

// ConsumeOptions are the consume options. The connector sends only what was set: whatever is nil takes the
// server default.
type ConsumeOptions struct {
	MaxMessages *int
	// VisibilityTimeout is the seconds of invisibility for the delivered messages.
	VisibilityTimeout *int
	// WaitTime is the seconds of long polling.
	WaitTime *int
	// AutoCreate creates the queue if it does not exist; Volatile, IdleTimeout, Fifo and Tags only apply with it.
	AutoCreate  bool
	Volatile    *bool
	IdleTimeout *int
	Fifo        *bool
	Tags        map[string]string
	RequestID   string
	Traceparent string
}

func (o *ConsumeOptions) copy() ConsumeOptions {
	if o == nil {
		return ConsumeOptions{}
	}
	return *o
}

// PublishOptions are the options of Publish and its variants.
type PublishOptions struct {
	// ContentType defaults to application/json (PublishText and PublishJSON set their own).
	ContentType     string
	GroupID         string
	DeduplicationID string
	RequestID       string
	Traceparent     string
	// Fifo must be true when the topic is a FIFO queue: only there can a 502/504 be repeated.
	Fifo bool
}

// Message is a delivered message. Body always comes out as bytes (doc 21, "The delivered message").
type Message struct {
	ID string
	// Body is, for JSON, the JSON text as it came; for text/*, the UTF-8 text; for base64, the decoded bytes.
	Body            []byte
	ContentType     string
	ReceiptToken    string
	DeliveryCount   int
	PublishedAt     string
	InvisibleUntil  string
	GroupID         string
	DeduplicationID string
	DeadLetteredAt  string
	// DeadLetterReason is set when the message came from a dead-letter queue.
	DeadLetterReason string
	// Raw is the message JSON, as the server returned it.
	Raw string
}

// BodyText is the Body as UTF-8 text.
func (m *Message) BodyText() string { return string(m.Body) }

// JSON reads the Body as JSON into v (a pointer), like json.Unmarshal.
func (m *Message) JSON(v interface{}) error { return json.Unmarshal(m.Body, v) }

// DistributorDelivery is the copy of a publication made by a distributor to a queue.
type DistributorDelivery struct {
	Topic        string
	ID           string
	ErrorCode    string
	ErrorMessage string
}

// Succeeded is true when the copy did not fail.
func (d DistributorDelivery) Succeeded() bool { return d.ErrorCode == "" }

// PublishResponse is the answer of a publication.
type PublishResponse struct {
	// ID is the message id (queue); empty in a distributor's response.
	ID string
	// Duplicate (FIFO queue only) is true when it repeated a publication within the deduplication window.
	Duplicate *bool
	// Matched (distributor only) is how many queues matched.
	Matched    *int
	Deliveries []DistributorDelivery
	Raw        string
}

// VisibilityResponse is the answer of ChangeVisibility.
type VisibilityResponse struct {
	ID             string
	InvisibleUntil string
	Raw            string
}

// BatchMessage is one message of PublishBatch: the same fields as Publish. The bytes travel in base64.
type BatchMessage struct {
	Body []byte
	// ContentType defaults to application/json.
	ContentType     string
	GroupID         string
	DeduplicationID string
}

// BatchResult is the result of one item of a batch, in the order of the request: the Status the single call
// would have answered and what it would have returned. An item that failed does not return an error: OK is false.
type BatchResult struct {
	// Status: 201 published, 200 a FIFO duplicate, a distributor or a new invisibility, 204 acked, or the error's.
	Status         int
	ID             string
	Duplicate      *bool
	Matched        *int
	InvisibleUntil string
	ErrorCode      string
	ErrorMessage   string
	Raw            string
}

// OK is true when the item succeeded.
func (r BatchResult) OK() bool { return r.ErrorCode == "" && r.Status < 300 }

// -- reading the responses ------------------------------------------------------------------------------------

func invalid(what string, err error) *Error {
	return clientError("client.invalid-response", "%s: %v", what, err)
}

func firstError(errs ...error) error {
	for _, e := range errs {
		if e != nil {
			return e
		}
	}
	return nil
}

func parseTopic(raw []byte) (Topic, error) {
	f, err := parseFields(raw)
	if err != nil {
		return Topic{}, invalid("topic", err)
	}
	t := Topic{Raw: string(raw)}
	var errs [14]error
	t.ID, _, errs[0] = f.str("id")
	t.Name, _, errs[1] = f.str("name")
	t.Type, _, errs[2] = f.str("type")
	t.CreatedAt, _, errs[3] = f.str("createdAt")
	t.Fifo, errs[4] = f.boolean("fifo")
	t.Volatile, errs[5] = f.boolean("volatile")
	t.Tags, errs[6] = f.stringMap("tags")
	if f.raw("prefixes") != nil {
		t.Prefixes, errs[7] = f.strings("prefixes")
	}
	t.IdleTimeout, errs[8] = f.integer("idleTimeout")
	t.MaxMessageBytes, errs[9] = f.integer("maxMessageBytes")
	t.TTL, errs[10] = f.integer("ttl")
	t.MaxDeliveries, errs[11] = f.integer("maxDeliveries")
	t.DeadLetter, errs[12] = f.boolean("deadLetter")
	t.Source, _, errs[13] = f.str("source")
	if err := firstError(errs[:]...); err != nil {
		return Topic{}, invalid("topic", err)
	}
	return t, nil
}

func isJSONType(contentType string) bool {
	media := strings.ToLower(strings.TrimSpace(strings.SplitN(contentType, ";", 2)[0]))
	return media == "application/json" || strings.HasSuffix(media, "+json")
}

func parseMessage(raw []byte) (*Message, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("message", err)
	}
	m := &Message{Raw: string(raw)}
	var errs [10]error
	m.ID, _, errs[0] = f.str("id")
	contentType, ok, e := f.str("contentType")
	errs[1] = e
	if !ok {
		contentType = "application/json"
	}
	m.ContentType = contentType
	m.ReceiptToken, _, errs[2] = f.str("receiptToken")
	var count *int
	count, errs[3] = f.integer("deliveryCount")
	if count != nil {
		m.DeliveryCount = *count
	}
	m.PublishedAt, _, errs[4] = f.str("publishedAt")
	m.InvisibleUntil, _, errs[5] = f.str("invisibleUntil")
	m.GroupID, _, errs[6] = f.str("groupId")
	m.DeduplicationID, _, errs[7] = f.str("deduplicationId")
	m.DeadLetteredAt, _, errs[8] = f.str("deadLetteredAt")
	m.DeadLetterReason, _, errs[9] = f.str("deadLetterReason")
	if err := firstError(errs[:]...); err != nil {
		return nil, invalid("message", err)
	}
	body := f.raw("body")
	encoding, _, _ := f.str("bodyEncoding")
	switch {
	case body == nil:
		m.Body = []byte{}
	case encoding == "base64":
		var text string
		if err := json.Unmarshal(body, &text); err != nil {
			return nil, invalid("message", err)
		}
		data, err := base64.StdEncoding.DecodeString(text)
		if err != nil {
			return nil, clientError("client.invalid-response", "invalid base64 body")
		}
		m.Body = data
	case isJSONType(contentType):
		m.Body = append([]byte(nil), body...)
	default:
		var text string
		if err := json.Unmarshal(body, &text); err != nil {
			return nil, invalid("message", err)
		}
		m.Body = []byte(text)
	}
	return m, nil
}

func parseMessages(raw []byte) ([]*Message, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("consume", err)
	}
	out := []*Message{}
	listing := f.raw("messages")
	if listing == nil {
		return out, nil
	}
	items, err := arrayItems(listing)
	if err != nil {
		return nil, invalid("messages", err)
	}
	for _, item := range items {
		m, err := parseMessage(item)
		if err != nil {
			return nil, err
		}
		out = append(out, m)
	}
	return out, nil
}

func parsePage(raw []byte) (*Page, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("page", err)
	}
	page := &Page{Raw: string(raw), Items: []Topic{}}
	if listing := f.raw("items"); listing != nil {
		items, err := arrayItems(listing)
		if err != nil {
			return nil, invalid("page items", err)
		}
		for _, item := range items {
			t, err := parseTopic(item)
			if err != nil {
				return nil, err
			}
			page.Items = append(page.Items, t)
		}
	}
	page.NextCursor, _, err = f.str("nextCursor")
	if err != nil {
		return nil, invalid("page", err)
	}
	return page, nil
}

// errorOf reads the {"code","message"} object of a delivery or a batch item that failed. found is true when
// the item carries an error object, even one without a code.
func errorOf(f jsonFields) (code, message string, found bool) {
	raw := f.raw("error")
	if raw == nil || raw[0] != '{' {
		return "", "", false
	}
	e, err := parseFields(raw)
	if err != nil {
		return "", "", false
	}
	code, _, _ = e.str("code")
	message, _, _ = e.str("message")
	return code, message, true
}

func parsePublishResponse(raw []byte) (*PublishResponse, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("publication", err)
	}
	r := &PublishResponse{Raw: string(raw)}
	var errs [3]error
	r.ID, _, errs[0] = f.str("id")
	r.Duplicate, errs[1] = f.boolean("duplicate")
	r.Matched, errs[2] = f.integer("matched")
	if err := firstError(errs[:]...); err != nil {
		return nil, invalid("publication", err)
	}
	if listing := f.raw("deliveries"); listing != nil {
		items, err := arrayItems(listing)
		if err != nil {
			return nil, invalid("publication", err)
		}
		for _, item := range items {
			d, err := parseFields(item)
			if err != nil {
				return nil, invalid("publication", err)
			}
			delivery := DistributorDelivery{}
			delivery.Topic, _, _ = d.str("topic")
			delivery.ID, _, _ = d.str("id")
			if code, message, found := errorOf(d); found {
				delivery.ErrorCode, delivery.ErrorMessage = code, message
			}
			r.Deliveries = append(r.Deliveries, delivery)
		}
	}
	return r, nil
}

func parseBatchResults(raw []byte) ([]BatchResult, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("batch", err)
	}
	results := []BatchResult{}
	listing := f.raw("results")
	if listing == nil {
		return results, nil
	}
	items, err := arrayItems(listing)
	if err != nil {
		return nil, invalid("batch results", err)
	}
	for _, item := range items {
		r, err := parseFields(item)
		if err != nil {
			return nil, invalid("batch results", err)
		}
		result := BatchResult{Raw: string(item)}
		var errs [5]error
		var status *int
		status, errs[0] = r.integer("status")
		if status != nil {
			result.Status = *status
		}
		result.ID, _, errs[1] = r.str("id")
		result.Duplicate, errs[2] = r.boolean("duplicate")
		result.Matched, errs[3] = r.integer("matched")
		result.InvisibleUntil, _, errs[4] = r.str("invisibleUntil")
		if err := firstError(errs[:]...); err != nil {
			return nil, invalid("batch results", err)
		}
		result.ErrorCode, result.ErrorMessage, _ = errorOf(r)
		results = append(results, result)
	}
	return results, nil
}

func parseVisibilityResponse(raw []byte) (*VisibilityResponse, error) {
	f, err := parseFields(raw)
	if err != nil {
		return nil, invalid("visibility", err)
	}
	r := &VisibilityResponse{Raw: string(raw)}
	r.ID, _, _ = f.str("id")
	r.InvisibleUntil, _, _ = f.str("invisibleUntil")
	return r, nil
}
