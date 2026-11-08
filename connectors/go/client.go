package taba

import (
	"context"
	"encoding/json"
)

// Client is the Taba client API client (doc 21): the nine operations, Subscribe and the consume with a handler.
// It is safe for concurrent use.
type Client struct {
	config Config
	t      *transport
}

// NewClient validates the configuration and returns a client. A bad BaseURL is a client.invalid-config *Error.
func NewClient(config Config) (*Client, error) {
	t, err := newTransport(config)
	if err != nil {
		return nil, err
	}
	return &Client{config: t.config, t: t}, nil
}

// Config returns the configuration in use, with the defaults filled in.
func (c *Client) Config() Config { return c.config }

// Close closes the idle connections. The client can be used again afterwards.
func (c *Client) Close() { c.t.close() }

// -- topics --------------------------------------------------------------------------------------------------

// CreateTopic is POST /topics. If a 503 hid the answer and the topic exists on the retry, it returns the topic.
func (c *Client) CreateTopic(ctx context.Context, request *TopicRequest) (*Topic, error) {
	call, err := createTopicCall(request)
	if err != nil {
		return nil, err
	}
	reply, err := c.t.send(ctx, call)
	if err != nil {
		return nil, err
	}
	if reply.recovered {
		return c.GetTopic(ctx, request.Name)
	}
	t, err := parseTopic(reply.body)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// GetTopic is GET /topics/{topic}.
func (c *Client) GetTopic(ctx context.Context, name string) (*Topic, error) {
	reply, err := c.t.send(ctx, getTopicCall(name))
	if err != nil {
		return nil, err
	}
	t, err := parseTopic(reply.body)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// ListTopics is GET /topics. A nil limit and empty cursor and name do not go in the query.
func (c *Client) ListTopics(ctx context.Context, limit *int, cursor, name string) (*Page, error) {
	reply, err := c.t.send(ctx, listTopicsCall(limit, cursor, name))
	if err != nil {
		return nil, err
	}
	return parsePage(reply.body)
}

// ListAllTopics returns all topics, reading the pages one after the other.
func (c *Client) ListAllTopics(ctx context.Context, name string) ([]Topic, error) {
	all := []Topic{}
	cursor := ""
	for {
		page, err := c.ListTopics(ctx, nil, cursor, name)
		if err != nil {
			return nil, err
		}
		all = append(all, page.Items...)
		cursor = page.NextCursor
		if cursor == "" {
			return all, nil
		}
	}
}

// UpdateTopic is a real PATCH /topics/{topic}, with no verb-override header.
func (c *Client) UpdateTopic(ctx context.Context, name string, update *TopicUpdate) (*Topic, error) {
	call, err := updateTopicCall(name, update)
	if err != nil {
		return nil, err
	}
	reply, err := c.t.send(ctx, call)
	if err != nil {
		return nil, err
	}
	t, err := parseTopic(reply.body)
	if err != nil {
		return nil, err
	}
	return &t, nil
}

// DeleteTopic is DELETE /topics/{topic}. If a 503 hid the answer and the topic is gone on the retry, it succeeds.
func (c *Client) DeleteTopic(ctx context.Context, name string) error {
	_, err := c.t.send(ctx, deleteTopicCall(name))
	return err
}

// CreateOrUpdateTopic is PUT /topics/{topic}: brings the topic to the declared state, creating it if it does not
// exist. What is left out goes back to the server default (a full replacement, unlike UpdateTopic); request.Name is
// the topic of the path. Another type, fifo or volatile is 409 topic.immutable-mismatch unless recreate says
// otherwise. Repeated after a 503 like the other calls: the route is idempotent.
func (c *Client) CreateOrUpdateTopic(ctx context.Context, request *TopicRequest, recreate Recreate) (*TopicPut, error) {
	call, err := putTopicCall(request, recreate)
	if err != nil {
		return nil, err
	}
	reply, err := c.t.send(ctx, call)
	if err != nil {
		return nil, err
	}
	t, err := parseTopic(reply.body)
	if err != nil {
		return nil, err
	}
	outcome := TopicUpdated
	if reply.status == 201 {
		outcome = TopicCreated
		if reply.recreated {
			outcome = TopicRecreated
		}
	}
	return &TopicPut{Topic: t, Outcome: outcome}, nil
}

// DeleteTopicIfEmpty is DELETE /topics/{topic}?ifEmpty=true: removes the queue only if it and its dead-letter hold
// no message on any node (409 topic.not-empty otherwise; a distributor holds none).
func (c *Client) DeleteTopicIfEmpty(ctx context.Context, name string) error {
	call := deleteTopicCall(name)
	call.withQuery("ifEmpty", "true")
	_, err := c.t.send(ctx, call)
	return err
}

// -- publishing ----------------------------------------------------------------------------------------------

// Publish is POST /topics/{topic}/messages. The body goes as bytes, without validation. options may be nil;
// set options.Fifo when the topic is a FIFO queue (only there can a 502/504 be repeated).
func (c *Client) Publish(ctx context.Context, topic string, body []byte, options *PublishOptions) (*PublishResponse, error) {
	var o PublishOptions
	if options != nil {
		o = *options
	}
	reply, err := c.t.send(ctx, publishCall(topic, body, o))
	if err != nil {
		return nil, err
	}
	return parsePublishResponse(reply.body)
}

// PublishJSON serializes the value with encoding/json and publishes it as application/json.
func (c *Client) PublishJSON(ctx context.Context, topic string, value interface{}) (*PublishResponse, error) {
	return c.PublishJSONWith(ctx, topic, value, nil)
}

// PublishJSONWith is PublishJSON with options (ContentType is ignored).
func (c *Client) PublishJSONWith(ctx context.Context, topic string, value interface{}, options *PublishOptions) (*PublishResponse, error) {
	if value == nil {
		return nil, clientError("client.invalid-request", "value is required")
	}
	data, err := json.Marshal(value)
	if err != nil {
		return nil, clientError("client.invalid-request", "cannot serialize the value: %v", err)
	}
	return c.publishAs(ctx, topic, data, "application/json", options)
}

// PublishRawJSON publishes a JSON text the application already built, without serializing.
func (c *Client) PublishRawJSON(ctx context.Context, topic, text string, options *PublishOptions) (*PublishResponse, error) {
	return c.publishAs(ctx, topic, []byte(text), "application/json", options)
}

// PublishText publishes a text (Content-Type: text/plain; charset=utf-8).
func (c *Client) PublishText(ctx context.Context, topic, text string, options *PublishOptions) (*PublishResponse, error) {
	return c.publishAs(ctx, topic, []byte(text), "text/plain; charset=utf-8", options)
}

func (c *Client) publishAs(ctx context.Context, topic string, data []byte, contentType string, options *PublishOptions) (*PublishResponse, error) {
	var o PublishOptions
	if options != nil {
		o = *options
	}
	o.ContentType = contentType
	return c.Publish(ctx, topic, data, &o)
}

// PublishBatch is POST /topics/{topic}/messages/batch: many messages in one request (1 to the server's
// api.batch.max-messages, 1000 by default). Each item is answered as its single publication and one that
// fails does not undo the others (look at OK). On a FIFO queue the items of a group are published in the order
// of the slice. A failure of the network is not repeated (it could duplicate).
func (c *Client) PublishBatch(ctx context.Context, topic string, messages []BatchMessage) ([]BatchResult, error) {
	reply, err := c.t.send(ctx, publishBatchCall(topic, messages))
	if err != nil {
		return nil, err
	}
	return parseBatchResults(reply.body)
}

// -- consuming -----------------------------------------------------------------------------------------------

// Consume is GET /topics/{topic}/messages. The read time is WaitTime + 10 seconds, not the RequestTimeout. A
// timeout is not repeated (the node may have captured messages, which come back by themselves at the end of
// the VisibilityTimeout). It returns an empty slice if there are no messages. options may be nil.
func (c *Client) Consume(ctx context.Context, topic string, options *ConsumeOptions) ([]*Message, error) {
	return c.consume(ctx, topic, options.copy())
}

func (c *Client) consume(ctx context.Context, topic string, o ConsumeOptions) ([]*Message, error) {
	reply, err := c.t.send(ctx, consumeCall(topic, o))
	if err != nil {
		return nil, err
	}
	return parseMessages(reply.body)
}

// Ack is DELETE /topics/{topic}/messages/{id}?receiptToken= for a message Consume returned.
func (c *Client) Ack(ctx context.Context, topic string, message *Message) error {
	return c.AckByID(ctx, topic, message.ID, message.ReceiptToken)
}

// AckByID acknowledges a message by its id and receipt token.
func (c *Client) AckByID(ctx context.Context, topic, messageID, receiptToken string) error {
	_, err := c.t.send(ctx, ackCall(topic, messageID, receiptToken))
	return err
}

// ChangeVisibility is PUT /topics/{topic}/messages/{id}/visibility?receiptToken= for a message Consume returned.
func (c *Client) ChangeVisibility(ctx context.Context, topic string, message *Message, seconds int) (*VisibilityResponse, error) {
	return c.ChangeVisibilityByID(ctx, topic, message.ID, message.ReceiptToken, seconds)
}

// ChangeVisibilityByID changes the invisibility of a message by its id and receipt token.
func (c *Client) ChangeVisibilityByID(ctx context.Context, topic, messageID, receiptToken string, seconds int) (*VisibilityResponse, error) {
	reply, err := c.t.send(ctx, changeVisibilityCall(topic, messageID, receiptToken, seconds))
	if err != nil {
		return nil, err
	}
	return parseVisibilityResponse(reply.body)
}

// AckBatch is POST /topics/{topic}/messages/ack: it acknowledges many messages Consume returned, in one request.
func (c *Client) AckBatch(ctx context.Context, topic string, messages []*Message) ([]BatchResult, error) {
	reply, err := c.t.send(ctx, receiptsCall(topic, "ack", messages, nil))
	if err != nil {
		return nil, err
	}
	return parseBatchResults(reply.body)
}

// ChangeVisibilityBatch is POST /topics/{topic}/messages/visibility: the invisibility of each message,
// seconds from now.
func (c *Client) ChangeVisibilityBatch(ctx context.Context, topic string, messages []*Message, seconds int) ([]BatchResult, error) {
	reply, err := c.t.send(ctx, receiptsCall(topic, "visibility", messages, &seconds))
	if err != nil {
		return nil, err
	}
	return parseBatchResults(reply.body)
}
