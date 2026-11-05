package taba

import (
	"context"
	"sync"
)

// MessageContext is what the ConsumeWith handler receives: the Message, the Delete flag (the message is deleted
// at the end if the handler returns nil; the default is true) and ChangeVisibility. While the handler runs, the
// connector renews the invisibility by itself, so no other consumer takes the message.
type MessageContext struct {
	Message *Message
	// Delete: set it to false to keep the message even though the handler returned nil.
	Delete bool

	client *Client
	topic  string
	ctx    context.Context

	mu      sync.Mutex
	lease   int
	expired bool
}

// LeaseSeconds is the seconds of invisibility the automatic renewal requests on each round.
func (m *MessageContext) LeaseSeconds() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.lease
}

// LeaseLost is true when the server refused the renewal (the message already went back to the queue or was deleted).
func (m *MessageContext) LeaseLost() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.expired
}

// ChangeVisibility changes the invisibility to seconds from now on. The value becomes the one of the automatic renewal.
func (m *MessageContext) ChangeVisibility(seconds int) error {
	if _, err := m.client.ChangeVisibility(m.ctx, m.topic, m.Message, seconds); err != nil {
		return err
	}
	m.mu.Lock()
	m.lease = seconds
	m.mu.Unlock()
	return nil
}

// Decode reads the body as JSON into v (a pointer), like json.Unmarshal.
func (m *MessageContext) Decode(v interface{}) error { return m.Message.JSON(v) }

// ConsumeWith reads from the queue and hands each message to handler. The message is deleted if the handler
// returns nil (set ctx.Delete to false to keep it). While the handler runs, the invisibility is renewed every
// third of it, by another goroutine. Defaults: one message at a time, 20 s of long polling, 120 s of
// invisibility. If the handler returns an error the message is not deleted and the error is returned. It
// returns how many messages were handled. options may be nil.
func (c *Client) ConsumeWith(ctx context.Context, topic string, options *ConsumeOptions, handler func(*MessageContext) error) (int, error) {
	opts := handlerDefaults(options)
	messages, err := c.consume(ctx, topic, opts)
	if err != nil {
		return 0, err
	}
	handled := 0
	for _, message := range messages {
		mc := &MessageContext{Message: message, Delete: true, client: c, topic: topic, ctx: ctx, lease: *opts.VisibilityTimeout}
		if err := c.handle(ctx, topic, mc, func() error { return handler(mc) }); err != nil {
			return handled, err
		}
		handled++
	}
	return handled, nil
}

func (c *Client) handle(ctx context.Context, topic string, mc *MessageContext, run func() error) error {
	stopCtx, stop := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		c.keepAlive(stopCtx, topic, mc)
	}()
	err := func() error {
		defer func() {
			stop()
			<-done
		}()
		return run()
	}()
	if err != nil {
		return err
	}
	if mc.Delete {
		// Detached from ctx: the message was already handled, and acknowledging it must not be cut short.
		if err := c.Ack(context.Background(), topic, mc.Message); err != nil && !lostOrGone(err) {
			return err
		}
	}
	return nil
}

// keepAlive renews the invisibility until the handler finishes. It never fails the handler.
func (c *Client) keepAlive(ctx context.Context, topic string, mc *MessageContext) {
	for ctx.Err() == nil {
		if err := c.t.sleep(ctx, renewalInterval(mc.LeaseSeconds())); err != nil {
			return
		}
		_, err := c.ChangeVisibility(ctx, topic, mc.Message, mc.LeaseSeconds())
		if err != nil && lostOrGone(err) {
			// The message already went back to the queue or was deleted: renewing is no longer of any use.
			mc.mu.Lock()
			mc.expired = true
			mc.mu.Unlock()
			return
		}
		// A transient failure (network, 5xx) is tried again on the next round, before the deadline expires.
	}
}

// Subscribe is the consume loop: long polling (WaitTime 20 and one message at a time, if not set), calls
// handler and acknowledges by itself only if it returns nil (a message that fails goes back to the queue at the
// end of the VisibilityTimeout, and the error goes to onError, which may be nil). It blocks until ctx is
// cancelled and then returns nil. A server error while consuming is returned.
func (c *Client) Subscribe(ctx context.Context, topic string, options *ConsumeOptions, handler func(*Message) error, onError func(*Message, error)) error {
	poll := subscribeDefaults(options)
	for ctx.Err() == nil {
		messages, err := c.consume(ctx, topic, poll)
		if err != nil {
			if ctx.Err() != nil {
				return nil
			}
			return err
		}
		for _, message := range messages {
			if err := handler(message); err != nil {
				if onError != nil {
					onError(message, err)
				}
			} else if err := c.Ack(context.Background(), topic, message); err != nil && !lostOrGone(err) {
				return err
			}
			if ctx.Err() != nil {
				return nil
			}
		}
	}
	return nil
}
